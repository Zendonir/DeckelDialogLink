"""Differential test: Python and Rust implementation must analyse and prepare NC data identically.

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

from deckellink.domain.errors import DeckelLinkError
from deckellink.domain.models import (
    CommentHandling,
    LineEnding,
    Profile,
    TabHandling,
    WhitespaceHandling,
)
from deckellink.nc.analyzer import analyze
from deckellink.nc.transformer import transform
from deckellink.profiles.loader import builtin_profiles_dir
from deckellink.profiles.schema import parse_profile

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
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
