"""Differential test: Python and Rust must analyse/prepare NC data, run LSV2 and handle parameter files identically.

Usage: python tools/rust_parity.py [--cases N] [--seed S] [--cli PATH]

Generates random NC files (line endings, tabs, NUL, umlauts, control characters,
EOT trailers, comments, program ends ...) and random profile variants, runs the
Python analyzer/transformer and ``deckellink-cli analyze-json`` and compares all
results field by field. Exit code 1 on any difference.
"""

from __future__ import annotations

import argparse
import json
import random
import subprocess
import sys
import tempfile
from dataclasses import replace
from pathlib import Path
from typing import Any

from deckellink.clock import FakeClock
from deckellink.domain.errors import DeckelLinkError
from deckellink.domain.models import (
    CommentHandling,
    LineEnding,
    Profile,
    TabHandling,
    WhitespaceHandling,
)
from deckellink.domain.results import TrafficEvent
from deckellink.nc.analyzer import analyze
from deckellink.nc.transformer import transform
from deckellink.parameters.fileformat import compatibility_problems, parse_parameter_file
from deckellink.parameters.service import analyze_parameters, diff_parameters, prepare_parameter_payload
from deckellink.profiles.loader import builtin_profiles_dir
from deckellink.profiles.schema import parse_profile
from deckellink.simulation.lsv2_cnc import Lsv2SimConfig, Lsv2SimulatedCnc
from deckellink.transfer.lsv2_transfer import Lsv2ProgramReceiver, Lsv2ProgramSender
from deckellink.transfer.receiver import ReceiveCallbacks
from deckellink.transfer.sender import SendCallbacks

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CLI = ROOT / "rust" / "target" / "release" / "deckellink-cli"

PIECES = [
    b"N10 G00 X0 Y0",
    b"N20 G01 X10.5 F200",
    b"M30",
    b"M2",
    b"N100M30",
    b"M02",
    b"M300",
    b"(KOMMENTAR)",
    b"N30 (M30 IM KOMMENTAR) G0",
    b";SEMIKOLON",
    b"  ;EINGERUECKT",
    b"",
    b"   ",
    b"\t",
    b"N40 G0\tX1",
    b"N50 X\x00Y",
    b"\x00",
    "N60 ÄÖÜ".encode("latin-1"),
    b"N70 \x1b",
    b"N80 \x7f",
    b"%",
    b"N90 X1.M2",
    b"m30",
    b"N95 G0 X" + b"9" * 130,
    b"  N96 G0  ",
    b"\x04",
]
ENDINGS = [b"\r\n", b"\n", b"\r", b"\r\n", b"\r\n"]


def random_program(rng: random.Random) -> bytes:
    out = bytearray()
    for _ in range(rng.randint(0, 14)):
        out += rng.choice(PIECES)
        if rng.random() < 0.9:
            out += rng.choice(ENDINGS)
    if rng.random() < 0.25:
        out += b"\x04" + rng.choice([b"", b"\r\n", b"\n", b"\r\n\x00"])
    return bytes(out)


def random_profile(base: Profile, rng: random.Random) -> Profile:
    t = base.transfer
    t = replace(
        t,
        line_ending=rng.choice(list(LineEnding)),
        program_start=rng.choice(["", "", "%", "M82"]),
        program_end=rng.choice(["M30", "M30,M2", "M2", "", "M30;M02"]),
        append_program_end=rng.random() < 0.3,
        remove_blank_lines=rng.random() < 0.7,
        strip_null_bytes=rng.random() < 0.8,
        skip_semicolon_lines=rng.random() < 0.3,
        normalize_line_endings=rng.random() < 0.8,
        tab_handling=rng.choice(list(TabHandling)),
        whitespace=rng.choice(list(WhitespaceHandling)),
        comment_handling=rng.choice(list(CommentHandling)),
        comment_open=rng.choice(["(", "(", ";", ""]),
        comment_close=rng.choice([")", ")", ""]),
        ensure_final_newline=rng.random() < 0.8,
        send_eot_trailer=rng.random() < 0.7,
        max_line_length=rng.choice([0, 20, 120]),
        max_program_bytes=rng.choice([0, 50, 500_000]),
    )
    serial = replace(base.serial, data_bits=rng.choice([6, 7, 7, 8]))
    return replace(base, transfer=t, serial=serial)


def python_result(data: bytes, profile: Profile) -> dict[str, Any]:
    analysis = analyze(data, profile.transfer, profile.serial)
    try:
        prepared = transform(data, profile.transfer, append_program_end_confirmed=True)
        transform_json: dict[str, Any] = {
            "ok": True,
            "data": prepared.data.hex(),
            "line_count": prepared.line_count,
            "offsets": list(prepared.line_end_offsets),
            "applied": list(prepared.applied),
            "appended": prepared.appended_program_end,
        }
    except DeckelLinkError as exc:
        transform_json = {"ok": False, "error": str(exc)}
    return {
        "findings": [
            {
                "code": f.code,
                "severity": f.severity.value,
                "message": f.message,
                "lines": list(f.lines),
                "count": f.count,
            }
            for f in analysis.findings
        ],
        "byte_count": analysis.byte_count,
        "line_count": analysis.line_count,
        "longest_line": analysis.longest_line,
        "has_program_end": analysis.has_program_end,
        "sha256": analysis.sha256,
        "endings": dict(analysis.line_ending_counts),
        "transform": transform_json,
    }


LSV2_PROGRAM = (
    b"(PARITY)\r\nN10 G90 G17\r\nN20 G00 X0 Y0 Z50\r\nN30 \x10DLE\r\nN40 G01 Z-1 F100\r\nN50 X40\r\nN60 M30\r\n"
)
LSV2_CASES: list[tuple[str, dict[str, Any], dict[str, Any]]] = [
    ("send", {}, {}),
    ("drip", {"buffer_telegrams": 2, "execution_s_per_telegram": 1.5}, {}),
    ("send", {"nak_on_telegrams": [2, 4]}, {}),
    ("send", {"drop_ack_on_telegrams": [3]}, {}),
    ("drip", {"wack_on_telegrams": [2, 3]}, {}),
    ("drip", {"eot_after_telegrams": 3}, {}),
    ("send", {"never_ready": True}, {"link_timeout_ms": 5000}),
    ("send", {"disconnect_after_telegrams": 3}, {}),
    ("send", {}, {"transparent": False, "bcc_mode": "transmitted"}),
    ("send", {}, {"bcc_mode": "none", "lines_per_telegram": 3, "include_line_ending": False}),
    ("send", {}, {"start_telegrams": ["%{name}"], "end_telegrams": ["END"]}),
    ("receive", {"program": LSV2_PROGRAM.hex()}, {}),
    (
        "receive",
        {"program": LSV2_PROGRAM.hex(), "corrupt_bcc_on_sent_telegrams": [2], "send_lines_per_telegram": 2},
        {},
    ),
    ("receive", {"program": LSV2_PROGRAM.hex()}, {"include_line_ending": False}),
]


def python_lsv2(profile: Profile, mode: str, config: dict[str, Any]) -> dict[str, Any]:
    sim = Lsv2SimConfig(
        never_ready=bool(config.get("never_ready", False)),
        buffer_telegrams=int(config.get("buffer_telegrams", 0)),
        execution_s_per_telegram=float(config.get("execution_s_per_telegram", 0.0)),
        nak_on_telegrams=tuple(config.get("nak_on_telegrams", ())),
        drop_ack_on_telegrams=tuple(config.get("drop_ack_on_telegrams", ())),
        wack_on_telegrams=tuple(config.get("wack_on_telegrams", ())),
        eot_after_telegrams=config.get("eot_after_telegrams"),
        disconnect_after_telegrams=config.get("disconnect_after_telegrams"),
        corrupt_bcc_on_sent_telegrams=tuple(config.get("corrupt_bcc_on_sent_telegrams", ())),
        send_lines_per_telegram=int(config.get("send_lines_per_telegram", 1)),
        program=bytes.fromhex(config.get("program", "")),
    )
    clock = FakeClock()
    cnc = Lsv2SimulatedCnc(profile.serial, profile.protocol, sim, clock)
    traffic: list[TrafficEvent] = []
    if mode == "receive":
        received = Lsv2ProgramReceiver(
            cnc, profile, clock=clock, callbacks=ReceiveCallbacks(on_traffic=traffic.append)
        ).receive()
        state, data, duration = received.state.value, received.raw.hex(), received.duration_s
    else:
        payload = transform(LSV2_PROGRAM, profile.transfer)
        sender = Lsv2ProgramSender(
            cnc,
            profile,
            drip_feed=mode == "drip",
            program_name="PROGRAMM",
            clock=clock,
            callbacks=SendCallbacks(on_traffic=traffic.append),
        )
        result = sender.send(payload)
        state, data, duration = result.state.value, cnc.received_program.hex(), result.duration_s
    return {
        "state": state,
        "tx": b"".join(e.data for e in traffic if e.direction.value == "TX").hex(),
        "rx": b"".join(e.data for e in traffic if e.direction.value == "RX").hex(),
        "data": data,
        "duration": round(duration * 1000) / 1000,
    }


PARAM_HEADERS = [
    b"%MPG00 (KOPF)",
    b"%MPG01 (ACHSEN)",
    b"%MPG02 (SPINDEL)",
    b"%MPG01 (ACHSEN)  ",
    b"%MDG00 (KOPF)",
    b"%MDG03 (X)",
    b"%MPG1 (KAPUTT)",
    b"%MPG04 ()\t",
]
PARAM_LINES = [
    b"M65001 V1.2",
    b"M65002 SIM     0.000",
    b"M65033 FP2A",
    b"M65034 4711",
    b"M00001 D1",
    b"M00034 99",
    b"M01001 5",
    b"M01002",
    b"M01003 -12.5",
    b"M01001 5",
    b"M1234 X",
    b"M012345 1",
    b"X01001 1",
    b"M01004 \xe4",
    b"M01005\t7",
    b"M01006 1\x00",
    b"",
]
CHECKSUMS = [b"AB12", b"00ff", b"C001", b"12345", b"XYZ1", b""]


def random_parameter_file(rng: random.Random) -> bytes:
    out = bytearray()
    if rng.random() < 0.1:
        out += b"VORSPANN\r\n"
    for _ in range(rng.randint(0, 5)):
        out += rng.choice(PARAM_HEADERS) + b"\r\n"
        for _ in range(rng.randint(0, 80 if rng.random() < 0.2 else 8)):
            out += rng.choice(PARAM_LINES) + rng.choice([b"\r\n", b"\r\n", b"\n", b"\r"])
        if rng.random() < 0.85:
            out += b"?\r\n"
            if rng.random() < 0.9:
                out += rng.choice(CHECKSUMS) + b"\r\n"
    if rng.random() < 0.6:
        out += b"\x04" + rng.choice([b"", b"\r\n"])
    return bytes(out)


def mutate_parameter_file(data: bytes, rng: random.Random) -> bytes:
    lines = data.split(b"\r\n")
    for _ in range(rng.randint(0, 6)):
        if not lines:
            break
        index = rng.randrange(len(lines))
        action = rng.random()
        if action < 0.4:
            lines[index] = rng.choice(PARAM_LINES + CHECKSUMS)
        elif action < 0.6:
            del lines[index]
        elif action < 0.8:
            lines.insert(index, rng.choice(PARAM_LINES))
        else:
            lines[index] = lines[index].replace(b"5", b"6")
    return b"\r\n".join(lines)


def python_param(profile: Profile, old: bytes, new: bytes) -> dict[str, Any]:
    analysis = analyze_parameters(new, profile)
    diff = diff_parameters(old, new, "alt", "neu")
    before, after = parse_parameter_file(old), parse_parameter_file(new)
    try:
        payload = prepare_parameter_payload(new, profile)
        payload_json: dict[str, Any] = {"ok": True, "data": payload.data.hex(), "applied": list(payload.applied)}
    except DeckelLinkError as exc:
        payload_json = {"ok": False, "error": str(exc)}
    structure: dict[str, Any] | None = None
    if after is not None:
        structure = {
            "kind": after.kind,
            "groups": [
                {"key": g.key, "label": g.label, "line": g.line, "lines": list(g.lines), "checksum": g.checksum}
                for g in after.groups
            ],
            "header": [[k, v] for k, v in sorted(after.header.items())],
            "eot": after.eot,
            "summary": after.summary,
            "software_versions": list(after.software_versions),
            "machine_type": after.machine_type,
            "machine_number": after.machine_number,
            "date": after.date,
            "findings": [finding_json(f) for f in after.findings],
        }
    return {
        "findings": [finding_json(f) for f in analysis.findings],
        "structure": structure,
        "diff": {
            "changed": diff.changed_lines,
            "added": diff.added_lines,
            "removed": diff.removed_lines,
            "text": diff.diff_text,
            "truncated": diff.truncated,
        },
        "problems": compatibility_problems(before, after) if before is not None and after is not None else None,
        "payload": payload_json,
    }


def finding_json(f: Any) -> dict[str, Any]:
    return {
        "code": f.code,
        "severity": f.severity.value,
        "message": f.message,
        "lines": list(f.lines),
        "count": f.count,
    }


def parameter_parity(cli: Path, tmp_dir: Path, cases: int, rng: random.Random) -> int:
    base_json = json.loads((builtin_profiles_dir() / "dialog11_7n1_rtscts.json").read_text(encoding="utf-8"))
    base, _ = parse_profile(base_json)
    failures = 0
    for case in range(cases):
        profile = replace(
            base,
            parameters=replace(
                base.parameters,
                identification_pattern=rng.choice(["", "", "%MPG", "^%M[PD]G", "XYZ", "M650[0-9]{2}"]),
                max_bytes=rng.choice([1_000_000, 1_000_000, 300]),
            ),
            transfer=replace(base.transfer, line_ending=rng.choice(list(LineEnding))),
            serial=replace(base.serial, data_bits=rng.choice([7, 8])),
        )
        if case % 40 == 0:
            # long files with many changes: autojunk heuristic and diff truncation (> 400 diff lines)
            count = rng.randint(200, 700)
            old = b"%MPG01 (LANG)\r\n" + b"".join(b"M01%03d %d\r\n" % (n % 1000, n % 7) for n in range(count))
            new = b"%MPG01 (LANG)\r\n" + b"".join(
                b"M01%03d %d\r\n" % (n % 1000, (n % 7) + (rng.random() < 0.4)) for n in range(count)
            )
        else:
            old = random_parameter_file(rng)
            new = mutate_parameter_file(old, rng) if rng.random() < 0.8 else random_parameter_file(rng)
        paths = [tmp_dir / "param_profile.json", tmp_dir / "alt.par", tmp_dir / "neu.par"]
        paths[0].write_text(json.dumps(profile.to_dict()), encoding="utf-8")
        paths[1].write_bytes(old)
        paths[2].write_bytes(new)
        completed = subprocess.run([str(cli), "param-json", *map(str, paths)], capture_output=True, check=False)
        if completed.returncode != 0:
            failures += 1
            print(f"param case {case}: CLI failed: {completed.stderr.decode(errors='replace')}")
            continue
        rust = json.loads(completed.stdout)
        python = python_param(profile, old, new)
        if rust != python:
            failures += 1
            if failures <= 5:
                print(f"--- param case {case} differs\nold={old!r}\nnew={new!r}")
                for key in python:
                    if python[key] != rust.get(key):
                        print(f"  {key}:\n    python={python[key]}\n    rust  ={rust.get(key)}")
    print(f"{cases} parameter cases, {failures} differences")
    return failures


def diagnostics_parity(cli: Path, tmp_dir: Path, cases: int, rng: random.Random) -> int:
    """Format detection (analyze_capture) and loopback test patterns."""
    from deckellink.diagnostics.format_detect import analyze_capture, encode_frames_as_8n1
    from deckellink.diagnostics.loopback_test import LoopbackPattern, make_pattern
    from deckellink.domain.models import Parity

    failures = 0
    for case in range(cases):
        text = b"".join(rng.choice(PIECES) + b"\r\n" for _ in range(rng.randint(0, 40)))
        data = encode_frames_as_8n1(text, rng.choice(list(Parity))) if rng.random() < 0.8 else text
        noisy = bytearray(data)
        for _ in range(rng.choice([0, 0, 1, 3, 20])):
            if noisy:
                noisy[rng.randrange(len(noisy))] = rng.randrange(256)
        capture = bytes(noisy)
        length, bits = rng.randint(1, 300), rng.choice([5, 6, 7, 8])
        path = tmp_dir / "capture.bin"
        path.write_bytes(capture)
        completed = subprocess.run(
            [str(cli), "diag-json", str(path), str(length), str(bits)], capture_output=True, check=False
        )
        rust = json.loads(completed.stdout) if completed.returncode == 0 else {"error": completed.stderr.decode()}
        guess = analyze_capture(capture)
        python = {
            "report": guess.report(),
            "frame": guess.frame,
            "confidence": guess.confidence,
            "patterns": [[p.hex() for p in make_pattern(kind, length, bits)] for kind in LoopbackPattern],
        }
        if rust != python:
            failures += 1
            if failures <= 5:
                print(f"--- diagnostics case {case} differs (length={length}, bits={bits}, capture={capture!r})")
                for key, value in python.items():
                    if value != rust.get(key):
                        print(f"  {key}:\n    python={value}\n    rust  ={rust.get(key)}")
    print(f"{cases} diagnostics cases, {failures} differences")
    return failures


def lsv2_parity(cli: Path, tmp_dir: Path) -> int:
    lsv2_json = json.loads((builtin_profiles_dir() / "dialog11_lsv2_7e1.json").read_text(encoding="utf-8"))
    failures = 0
    for index, (mode, config, protocol_changes) in enumerate(LSV2_CASES):
        data = json.loads(json.dumps(lsv2_json))
        data["protocol"].update(protocol_changes)
        profile, _ = parse_profile(data)
        profile_path = tmp_dir / "lsv2.json"
        nc_path = tmp_dir / "lsv2.nc"
        profile_path.write_text(json.dumps(profile.to_dict()), encoding="utf-8")
        nc_path.write_bytes(LSV2_PROGRAM)
        completed = subprocess.run(
            [str(cli), "lsv2-trace", str(profile_path), str(nc_path), mode, json.dumps(config)],
            capture_output=True,
            check=False,
        )
        rust = json.loads(completed.stdout) if completed.returncode == 0 else {"error": completed.stderr.decode()}
        python = python_lsv2(profile, mode, config)
        if rust != python:
            failures += 1
            print(f"--- LSV2 case {index} ({mode}, {config}, {protocol_changes}) differs")
            for key in python:
                if python[key] != rust.get(key):
                    print(f"  {key}:\n    python={python[key]}\n    rust  ={rust.get(key)}")
    print(f"{len(LSV2_CASES)} LSV2 wire cases, {failures} differences")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", type=int, default=2000)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--cli", type=Path, default=DEFAULT_CLI)
    args = parser.parse_args()
    if not args.cli.is_file():
        print(f"Rust CLI not found: {args.cli} (cargo build --release first)")
        return 2
    rng = random.Random(args.seed)
    data_json = json.loads((builtin_profiles_dir() / "dialog11_7n1_rtscts.json").read_text(encoding="utf-8"))
    base, _ = parse_profile(data_json)
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        tmp_dir = Path(tmp)
        for case in range(args.cases):
            profile = random_profile(base, rng)
            program = random_program(rng)
            profile_path = tmp_dir / "profile.json"
            nc_path = tmp_dir / "program.nc"
            profile_path.write_text(json.dumps(profile.to_dict()), encoding="utf-8")
            nc_path.write_bytes(program)
            completed = subprocess.run(
                [str(args.cli), "analyze-json", str(profile_path), str(nc_path)],
                capture_output=True,
                check=False,
            )
            if completed.returncode != 0:
                print(f"case {case}: CLI failed: {completed.stderr.decode(errors='replace')}")
                failures += 1
                continue
            rust = json.loads(completed.stdout)
            python = python_result(program, profile)
            if rust != python:
                failures += 1
                if failures <= 5:
                    print(f"--- case {case} differs\nprogram={program!r}\ntransfer={profile.transfer}")
                    for key in python:
                        if python[key] != rust.get(key):
                            print(f"  {key}:\n    python={python[key]}\n    rust  ={rust.get(key)}")
        print(f"{args.cases} cases, {failures} differences")
        failures += lsv2_parity(args.cli, tmp_dir)
        failures += parameter_parity(args.cli, tmp_dir, max(args.cases // 2, 50), rng)
        failures += diagnostics_parity(args.cli, tmp_dir, max(args.cases // 4, 50), rng)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
