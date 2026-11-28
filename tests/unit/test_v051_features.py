"""0.5.1: EOT at file end, M2 as program end, parameter file structure and write blocks.

All parameter data here is synthetic; it only mimics the observed structure.
"""

from __future__ import annotations

from dataclasses import replace
from datetime import UTC, datetime, timedelta

import pytest

from deckellink.domain.models import Profile
from deckellink.nc.analyzer import analyze
from deckellink.nc.checks import (
    format_ascii,
    primary_program_end,
    program_end_alternatives,
    program_end_pattern,
    split_eot_trailer,
)
from deckellink.nc.transformer import transform
from deckellink.parameters.fileformat import changed_groups, compatibility_problems, parse_parameter_file
from deckellink.parameters.service import (
    ParameterBackupInfo,
    ParameterCandidate,
    analyze_parameters,
    evaluate_parameter_write_gate,
    prepare_parameter_payload,
)
from deckellink.simulation.scenarios import SAMPLE_PARAMETERS

NOW = datetime(2026, 11, 20, 10, 0, tzinfo=UTC)


def group(kind: str, number: int, name: str, lines: list[str], checksum: str | None = "ABCD") -> str:
    text = f"%M{kind}G{number:02d} ({name})\r\n" + "".join(f"{line}\r\n" for line in lines) + "?\r\n"
    return text + (f"{checksum}\r\n" if checksum else "")


def header(kind: str = "P", version: str = "XYZ 1.000", machine_no: str = "0001") -> str:
    base = 65000 if kind == "P" else 0
    return group(
        kind,
        0,
        "KOPF",
        [
            f"M{base:05d} TEST",
            f"M{base + 2:05d} {version}",
            f"M{base + 26:05d} ",
            f"M{base + 33:05d} TESTMASCHINE",
            f"M{base + 34:05d} {machine_no}",
            f"M{base + 35:05d} 01.02.03",
        ],
        "1111",
    )


def param_file(*parts: str, eot: bool = True) -> bytes:
    return ("".join(parts) + ("\x04\r\n" if eot else "")).encode("ascii")


GROUP_A = group("P", 3, "SPINDEL", ["M39000 0", "M39001 5", "M39001 6"], "2222")
GROUP_B = group("P", 4, "ACHSEN", ["M40000 1", "M40001 "], "3333")


# ---- EOT -------------------------------------------------------------------------------
def test_split_eot_trailer() -> None:
    assert split_eot_trailer(b"A\r\n\x04\r\n") == (b"A\r\n", b"\x04\r\n")
    assert split_eot_trailer(b"A\r\n\x04") == (b"A\r\n", b"\x04")
    assert split_eot_trailer(b"A\x04B\r\n") == (b"A\x04B\r\n", b"")
    assert "<EOT>" in format_ascii(b"\x04")


def test_analyzer_accepts_trailing_eot_but_not_embedded(profile_a: Profile) -> None:
    ok = analyze(b"N10 G00\r\nN20 M30\r\n\x04\r\n", profile_a.transfer, profile_a.serial)
    assert not ok.has_errors and ok.has("TRANSMISSION_END") and ok.has_program_end
    assert ok.byte_count == 21
    bad = analyze(b"N10 G00\r\n\x04N20 M30\r\n", profile_a.transfer, profile_a.serial)
    assert bad.has("CONTROL_CHARS")
    assert analyze(b"\x04\r\n", profile_a.transfer).has("EMPTY_FILE")


def test_transformer_keeps_or_drops_eot(profile_a: Profile) -> None:
    data = b"N10 G00\r\nN20 M30\r\n\x04\r\n"
    kept = transform(data, profile_a.transfer)
    assert kept.data == data and kept.line_count == 2 and kept.line_end_offsets[-1] == 18
    dropped = transform(data, replace(profile_a.transfer, send_eot_trailer=False))
    assert dropped.data == b"N10 G00\r\nN20 M30\r\n"
    assert any("EOT" in a for a in dropped.applied)


# ---- M2 ----------------------------------------------------------------------------------
def test_program_end_alternatives() -> None:
    assert program_end_alternatives("M30, M2") == ["M30", "M2"]
    assert program_end_alternatives("M30;M2") == ["M30", "M2"]
    assert primary_program_end("M30,M2") == "M30"
    assert program_end_pattern("") is None
    pattern = program_end_pattern("M30,M2")
    assert pattern is not None
    for text in (b"N10 M30", b"N10 M2", b"N10 M02", b"M2", b"N10 m2"):
        assert pattern.search(text), text
    for text in (b"N10 M20", b"N10 M230", b"N10 M3", b"N10 X1.M2", b"N10 AM2"):
        assert not pattern.search(text), text


def test_analyzer_recognises_m2(profile_a: Profile) -> None:
    result = analyze(b"N10 G00\r\nN20 M02\r\n", profile_a.transfer)
    assert result.has_program_end and not result.has("MISSING_PROGRAM_END")


def test_append_uses_first_alternative(profile_a: Profile) -> None:
    settings = replace(profile_a.transfer, append_program_end=True)
    result = transform(b"N10 G00\r\n", settings, append_program_end_confirmed=True)
    assert result.data.endswith(b"M30\r\n") and result.appended_program_end


# ---- parameter file structure ---------------------------------------------------------------
def test_parse_valid_file() -> None:
    info = parse_parameter_file(param_file(header(), GROUP_A, GROUP_B))
    assert info is not None
    assert info.findings == ()
    assert [g.key for g in info.groups] == ["%MPG00", "%MPG03", "%MPG04"]
    assert info.group("%MPG03") is not None and info.group("%MPG03").checksum == "2222"  # type: ignore[union-attr]
    assert info.group("%MPG99") is None
    assert info.software_version == "XYZ 1.000" and info.software_versions == ("XYZ 1.000",)
    assert (info.machine_type, info.machine_number, info.date) == ("TESTMASCHINE", "0001", "01.02.03")
    assert info.eot and info.has_header and info.kind_label == "MPG"
    assert "MPG-Format, 3 Gruppe(n)" in info.summary and "XYZ 1.000" in info.summary


def test_parse_mdg_header() -> None:
    info = parse_parameter_file(param_file(header("D", "ABC V1"), group("D", 3, "SPINDEL", ["M03000 1"])))
    assert info is not None and info.kind_label == "MDG"
    assert info.software_version == "ABC V1" and info.machine_number == "0001"


def test_parse_non_parameter_data() -> None:
    assert parse_parameter_file(b"N10 G00\r\nM30\r\n") is None
    info = parse_parameter_file(b"%MPG03 (X)\r\n")
    assert info is not None and info.kind_label == "MPG" and info.software_version == ""


@pytest.mark.parametrize(
    ("data", "code"),
    [
        (param_file("FREMD\r\n", header()), "PARAM_DATA_OUTSIDE_GROUP"),
        (param_file(header(), group("P", 3, "X", ["M39000 0", "UNSINN"])), "PARAM_LINE_FORMAT"),
        (param_file(header(), "%MPG03 (X)\r\nM39000 0\r\n%MPG04 (Y)\r\nM1 2\r\n"), "PARAM_GROUP_UNTERMINATED"),
        (param_file(header(), "%MPG03 (X)\r\nM39000 0\r\n"), "PARAM_GROUP_UNTERMINATED"),
        (param_file(header(), group("P", 3, "X", ["M39000 0"], None)), "PARAM_CHECKSUM_MISSING"),
        (param_file(header(), group("P", 3, "X", ["M39000 0"], None), GROUP_B), "PARAM_CHECKSUM_MISSING"),
        (param_file(header(), GROUP_A, GROUP_A), "PARAM_DUPLICATE_GROUP"),
        (param_file(header(), group("D", 3, "X", ["M03000 1"])), "PARAM_MIXED_KINDS"),
        (param_file(GROUP_A), "PARAM_NO_HEADER"),
        (param_file(header(), GROUP_A, eot=False), "PARAM_NO_EOT"),
    ],
)
def test_structure_findings(data: bytes, code: str) -> None:
    info = parse_parameter_file(data)
    assert info is not None
    assert code in [f.code for f in info.findings]


def test_structure_findings_in_parameter_analysis(profile_a: Profile) -> None:
    good = analyze_parameters(param_file(header(), GROUP_A), profile_a)
    assert not good.has_errors and good.has("TRANSMISSION_END")
    broken = analyze_parameters(param_file(header(), group("P", 3, "X", ["M39000 0"], None)), profile_a)
    assert broken.has_errors and broken.has("PARAM_CHECKSUM_MISSING")
    assert analyze_parameters(b"N10 G00\r\n", profile_a).has("NOT_A_PARAMETER_FILE")
    payload = prepare_parameter_payload(param_file(header(), GROUP_A), profile_a)
    assert payload.data == param_file(header(), GROUP_A)  # sent exactly as read, including EOT


# ---- compatibility and checksum blocks --------------------------------------------------------
def _info(*parts: str) -> object:
    info = parse_parameter_file(param_file(*parts))
    assert info is not None
    return info


def test_changed_group_detection() -> None:
    old = parse_parameter_file(param_file(header(), GROUP_A, GROUP_B))
    stale = parse_parameter_file(param_file(header(), GROUP_A.replace("M39001 5", "M39001 7"), GROUP_B))
    fresh = parse_parameter_file(
        param_file(header(), GROUP_A.replace("M39001 5", "M39001 7").replace("2222", "2A2A"), GROUP_B)
    )
    assert old is not None and stale is not None and fresh is not None
    assert [(c.key, c.checksum_changed) for c in changed_groups(old, stale)] == [("%MPG03", False)]
    assert [(c.key, c.checksum_changed) for c in changed_groups(old, fresh)] == [("%MPG03", True)]
    assert any("Prüfsumme aber nicht" in r for r in compatibility_problems(old, stale))
    assert compatibility_problems(old, fresh) == []


def test_version_machine_and_kind_mismatch_block() -> None:
    old = parse_parameter_file(param_file(header(), GROUP_A))
    assert old is not None
    other_version = parse_parameter_file(param_file(header(version="XYZ 2.000"), GROUP_A))
    other_machine = parse_parameter_file(param_file(header(machine_no="0002"), GROUP_A))
    no_header = parse_parameter_file(param_file(GROUP_A))
    mdg = parse_parameter_file(param_file(header("D"), group("D", 3, "SPINDEL", ["M03000 1"])))
    assert other_version and other_machine and no_header and mdg
    assert any("Softwarestand der Datei (XYZ 2.000)" in r for r in compatibility_problems(old, other_version))
    assert any("Maschinennummer" in r for r in compatibility_problems(old, other_machine))
    assert any("keine Kopfgruppe" in r for r in compatibility_problems(old, no_header))
    assert any("Sicherung hat keine Kopfgruppe" in r for r in compatibility_problems(no_header, old))
    assert any("Dateiformat MDG" in r for r in compatibility_problems(old, mdg))


def _gate(profile: Profile, backup_data: bytes, candidate_data: bytes) -> str:
    writable = replace(profile, parameters=replace(profile.parameters, allow_write=True))
    analysis = analyze_parameters(candidate_data, writable)
    candidate = ParameterCandidate(
        "NEU.par",
        candidate_data,
        analysis,
        None if analysis.has_errors else prepare_parameter_payload(candidate_data, writable),
    )
    backup = ParameterBackupInfo("ALT.par", backup_data, NOW - timedelta(minutes=1), writable.id, "SIMULATOR", True)
    gate = evaluate_parameter_write_gate(
        profile=writable,
        session_unlocked=True,
        candidate=candidate,
        backup=backup,
        now=NOW,
        port_label="SIMULATOR",
        simulation=True,
        port="",
        busy=False,
    )
    return "" if gate.allowed else gate.reason_text


def test_gate_blocks_stale_checksum_and_version(profile_a: Profile) -> None:
    base = param_file(header(), GROUP_A)
    assert (
        _gate(profile_a, base, param_file(header(), GROUP_A.replace("2222", "2A2A").replace("M39000 0", "M39000 1")))
        == ""
    )
    assert "Prüfsumme aber nicht" in _gate(
        profile_a, base, param_file(header(), GROUP_A.replace("M39000 0", "M39000 1"))
    )
    assert "Softwarestand" in _gate(profile_a, base, param_file(header(version="NEU 9"), GROUP_A))
    unpatterned = replace(profile_a, parameters=replace(profile_a.parameters, identification_pattern=""))
    assert "keine erkennbare Gruppenstruktur" in _gate(unpatterned, base, b"P1=1\r\n")
    assert "Sicherung hat keine erkennbare" in _gate(unpatterned, b"P1=1\r\n", base)


def test_sample_parameters_have_valid_structure() -> None:
    info = parse_parameter_file(SAMPLE_PARAMETERS)
    assert info is not None and info.findings == ()
    assert info.machine_type == "SIMULATOR" and len(info.groups) == 3
    assert _info(header()) is not None
