"""Machine parameter backup, diff, write gate and transfer (simulator only)."""

from __future__ import annotations

import json
from dataclasses import replace
from datetime import UTC, datetime, timedelta
from pathlib import Path

import pytest

from deckellink.clock import FakeClock
from deckellink.domain.errors import ProfileValidationError
from deckellink.domain.models import Profile
from deckellink.domain.states import EndReason, ReceiveState, SendState
from deckellink.parameters.archive import ParameterArchive
from deckellink.parameters.service import (
    WRITE_CONFIRMATION_PHRASE,
    ParameterBackupInfo,
    ParameterCandidate,
    ParameterWriteGate,
    analyze_parameters,
    diff_parameters,
    evaluate_parameter_write_gate,
    parameter_profile,
    parameter_transfer_settings,
    prepare_parameter_payload,
)
from deckellink.parameters.transfer import read_parameters, write_parameters
from deckellink.profiles.repository import ProfileRepository
from deckellink.profiles.schema import parse_profile, safety_warnings
from deckellink.simulation.lsv2_cnc import Lsv2SimConfig, Lsv2SimulatedCnc
from deckellink.simulation.scenarios import SAMPLE_PARAMETERS
from deckellink.storage.checksums import sha256_bytes
from deckellink.transfer.cancellation import CancellationToken
from deckellink.transfer.receiver import process_received
from deckellink.transport.simulated_transport import SimulatedTransport, SimulationConfig

NOW = datetime(2026, 11, 18, 10, 0, tzinfo=UTC)


def writable(profile: Profile) -> Profile:
    return replace(profile, parameters=replace(profile.parameters, allow_write=True))


def backup(
    profile: Profile, data: bytes = SAMPLE_PARAMETERS, age_min: float = 5, port: str = "SIMULATOR"
) -> ParameterBackupInfo:
    return ParameterBackupInfo("PARAM_1.par", data, NOW - timedelta(minutes=age_min), profile.id, port, True)


def candidate(profile: Profile, data: bytes) -> ParameterCandidate:
    analysis = analyze_parameters(data, profile)
    payload = None if analysis.has_errors else prepare_parameter_payload(data, profile)
    return ParameterCandidate("NEU.par", data, analysis, payload)


def gate(base: Profile, **overrides: object) -> ParameterWriteGate:
    args: dict[str, object] = {
        "profile": writable(base),
        "session_unlocked": True,
        "candidate": candidate(base, SAMPLE_PARAMETERS.replace(b"P0001=37", b"P0001=38")),
        "backup": backup(base),
        "now": NOW,
        "port_label": "SIMULATOR",
        "simulation": True,
        "port": "",
        "busy": False,
    }
    args.update(overrides)
    return evaluate_parameter_write_gate(**args)  # type: ignore[arg-type]


# ---- defaults and profile ---------------------------------------------------------------
def test_write_disabled_by_default_in_all_builtin_profiles(repo: ProfileRepository) -> None:
    for profile in repo.profiles:
        assert profile.parameters.allow_write is False


def test_parameter_section_validation(profile_a: Profile) -> None:
    data = profile_a.to_dict()
    data["parameters"]["allow_write"] = True
    profile, warnings = parse_profile(data)
    assert profile.parameters.allow_write
    assert any("Maschinenparametern" in w for w in warnings)
    data["parameters"]["identification_pattern"] = "(broken"
    with pytest.raises(ProfileValidationError) as info:
        parse_profile(data)
    assert "parameters.identification_pattern" in [e.field for e in info.value.errors]
    del data["parameters"]
    assert parse_profile(data)[0].parameters.allow_write is False
    assert not any("Maschinenparametern" in w for w in safety_warnings(profile_a))


# ---- analysis / transformation ----------------------------------------------------------------
def test_parameter_payload_is_not_altered(profile_a: Profile) -> None:
    data = b"P1 = 5   \r\n\r\n;KOMMENTAR\r\nP2\t=\t7\nP3=1"
    payload = prepare_parameter_payload(data, profile_a)
    # only line endings normalised - whitespace, blank lines, tabs, semicolons, missing final newline kept
    assert payload.data == b"P1 = 5   \r\n\r\n;KOMMENTAR\r\nP2\t=\t7\r\nP3=1"
    settings = parameter_transfer_settings(profile_a)
    assert settings.program_end == "" and not settings.append_program_end and settings.program_start == ""


def test_parameter_analysis(profile_a: Profile) -> None:
    ok = analyze_parameters(SAMPLE_PARAMETERS, profile_a)
    assert not ok.has_errors and not ok.has("MISSING_PROGRAM_END")
    bad = analyze_parameters("P1=Ä\r\n".encode("latin-1"), profile_a)
    assert bad.has_errors
    tabs = analyze_parameters(b"P1\t=1\r\n", profile_a)
    assert tabs.get("TABS") is not None and not tabs.has_errors
    patterned = replace(profile_a, parameters=replace(profile_a.parameters, identification_pattern=r"^\(PARAM"))
    assert analyze_parameters(b"N10 G00 X0\r\nM30\r\n", patterned).has("NOT_A_PARAMETER_FILE")
    assert not analyze_parameters(b"(PARAMETER)\r\nP1=1\r\n", patterned).has_errors


def test_diff() -> None:
    old = b"A=1\r\nB=2\r\nC=3\r\n"
    assert diff_parameters(old, b"A=1\nB=2\nC=3").identical  # line endings ignored
    diff = diff_parameters(old, b"A=1\r\nB=9\r\nC=3\r\nD=4\r\n")
    assert (diff.changed_lines, diff.added_lines, diff.removed_lines) == (1, 1, 0)
    assert "-B=2" in diff.diff_text and "+B=9" in diff.diff_text
    assert "geänderte" in diff.summary
    removed = diff_parameters(old, b"A=1\r\n")
    assert removed.removed_lines == 2
    big = diff_parameters(b"", b"".join(b"X%d\n" % i for i in range(1000)))
    assert big.truncated and "weitere Diff-Zeilen" in big.diff_text


# ---- write gate ----------------------------------------------------------------------
def test_gate_allows_only_when_all_conditions_hold(profile_a: Profile) -> None:
    ok = gate(profile_a)
    assert ok.allowed, ok.reason_text
    assert ok.diff is not None and ok.diff.changed_lines == 1


@pytest.mark.parametrize(
    ("overrides", "reason"),
    [
        ({"profile": None}, "Profil erlaubt kein Schreiben"),
        ({"session_unlocked": False}, "nicht freigeschaltet"),
        ({"backup": None}, "Keine aktuelle Sicherung"),
        ({"candidate": None}, "Keine Parameterdatei"),
        ({"busy": True}, "anderer Vorgang"),
        ({"simulation": False, "port": ""}, "COM-Port"),
    ],
)
def test_gate_reasons(profile_a: Profile, overrides: dict[str, object], reason: str) -> None:
    if "profile" in overrides:
        overrides = {"profile": profile_a}
    result = gate(profile_a, **overrides)
    assert not result.allowed
    assert reason in result.reason_text


def test_gate_backup_freshness_and_identity(profile_a: Profile) -> None:
    assert "min alt" in gate(profile_a, backup=backup(profile_a, age_min=61)).reason_text
    assert "anderem Profil" in gate(profile_a, backup=backup(profile_a, port="COM9")).reason_text
    incomplete = replace(backup(profile_a), complete=False)
    assert "unvollständig" in gate(profile_a, backup=incomplete).reason_text
    same = gate(profile_a, candidate=candidate(profile_a, SAMPLE_PARAMETERS))
    assert "identisch" in same.reason_text
    invalid = gate(profile_a, candidate=candidate(profile_a, b"P1=\xc4\r\n"))
    assert "Prüffehler" in invalid.reason_text
    assert WRITE_CONFIRMATION_PHRASE == "PARAMETER SCHREIBEN"


# ---- transfer and archive -----------------------------------------------------------------------
def test_read_store_and_list_parameters(profile_a: Profile, tmp_path: Path) -> None:
    clock = FakeClock()
    transport = SimulatedTransport(profile_a.serial, SimulationConfig(machine_program=SAMPLE_PARAMETERS), clock)
    result = read_parameters(transport, profile_a, clock=clock, token=CancellationToken())
    assert result.state is ReceiveState.COMPLETED and result.end_reason is EndReason.INACTIVITY
    assert result.raw == SAMPLE_PARAMETERS
    program = process_received(result, parameter_profile(profile_a))
    assert program.complete
    archive = ParameterArchive(tmp_path)
    stored = archive.store(result, program, profile_a)
    assert stored is not None and stored.complete
    assert stored.path.suffix == ".par" and stored.path.read_bytes() == SAMPLE_PARAMETERS
    assert stored.sha256 == sha256_bytes(SAMPLE_PARAMETERS)
    assert stored.raw_path is not None and stored.raw_path.is_file()
    second = archive.store(result, program, profile_a)
    assert second is not None and second.path != stored.path  # never overwritten
    listed = archive.list_sets()
    assert len(listed) == 2 and listed[0].read_at is not None
    meta = json.loads(stored.meta_path.read_text(encoding="utf-8"))
    assert meta["type"] == "machine_parameters"
    assert archive.store(replace(result, raw=b""), program, profile_a) is None
    (archive.directory / "broken.par").write_bytes(b"x")
    (archive.directory / "broken.json").write_text("{", encoding="utf-8")
    assert any(not s.metadata for s in archive.list_sets())


def test_write_parameters_plain_and_lsv2(profile_a: Profile, repo: ProfileRepository) -> None:
    data = SAMPLE_PARAMETERS.replace(b"P0002=74", b"P0002=75")
    clock = FakeClock()
    transport = SimulatedTransport(profile_a.serial, SimulationConfig(), clock)
    payload = prepare_parameter_payload(data, profile_a)
    result = write_parameters(transport, profile_a, payload, clock=clock, token=CancellationToken())
    assert result.state is SendState.COMPLETED
    assert transport.machine_received == data

    lsv2 = repo.get("dialog11_lsv2_7e1")
    assert lsv2 is not None
    clock2 = FakeClock()
    cnc = Lsv2SimulatedCnc(lsv2.serial, lsv2.protocol, Lsv2SimConfig(), clock2)
    result2 = write_parameters(
        cnc, lsv2, prepare_parameter_payload(data, lsv2), clock=clock2, token=CancellationToken()
    )
    assert result2.state is SendState.COMPLETED
    assert cnc.received_program == data and cnc.eot_received
    clock3 = FakeClock()
    sender_cnc = Lsv2SimulatedCnc(lsv2.serial, lsv2.protocol, Lsv2SimConfig(program=SAMPLE_PARAMETERS), clock3)
    read = read_parameters(sender_cnc, lsv2, clock=clock3, token=CancellationToken())
    assert read.state is ReceiveState.COMPLETED and read.raw == SAMPLE_PARAMETERS


def test_sample_parameters_are_marked_as_simulation() -> None:
    assert b"KEIN ECHTES DIALOG-11-FORMAT" in SAMPLE_PARAMETERS
