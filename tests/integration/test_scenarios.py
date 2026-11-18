"""All predefined simulator scenarios run through the real transfer logic (no COM port)."""

from __future__ import annotations

import time
from pathlib import Path

import pytest

from deckellink.clock import FakeClock
from deckellink.domain.models import Profile, ProtocolMode
from deckellink.domain.states import EndReason, ReceiveState, SendState
from deckellink.nc.analyzer import analyze
from deckellink.nc.transformer import transform
from deckellink.simulation.runner import run_receive_scenario, run_send_scenario
from deckellink.simulation.scenarios import SCENARIOS, ScenarioKind, get_scenario
from deckellink.storage.archive import ReceiveArchive
from deckellink.storage.paths import AppPaths
from deckellink.transfer.receiver import process_received

PLAIN = [s for s in SCENARIOS if s.protocol is ProtocolMode.PLAIN]
SEND_SCENARIOS = [s for s in PLAIN if s.expected_send_state]
RECEIVE_SCENARIOS = [s for s in PLAIN if s.expected_receive_state]


def test_required_scenarios_exist() -> None:
    ids = {s.id for s in SCENARIOS}
    assert {
        "send_ok",
        "receive_ok",
        "cts_off",
        "send_timeout",
        "user_cancel",
        "receive_no_m30",
        "receive_nulls",
        "disconnect",
        "invalid_ascii_file",
        "slow",
    } <= ids
    assert get_scenario("unknown").id == "send_ok"


@pytest.mark.parametrize("scenario", SEND_SCENARIOS, ids=lambda s: s.id)
def test_send_scenarios(scenario: object, profile_a: Profile) -> None:
    from deckellink.simulation.scenarios import Scenario

    assert isinstance(scenario, Scenario)
    result, transport = run_send_scenario(scenario, profile_a)
    assert result.state.value == scenario.expected_send_state
    assert not transport.is_open
    if result.state is SendState.COMPLETED:
        assert transport.machine_received == transform(scenario.sample_program, profile_a.transfer).data
    else:
        assert len(transport.machine_received) < result.bytes_total or result.bytes_total == 0


@pytest.mark.parametrize("scenario", RECEIVE_SCENARIOS, ids=lambda s: s.id)
def test_receive_scenarios(scenario: object, profile_a: Profile, paths: AppPaths) -> None:
    from deckellink.simulation.scenarios import Scenario

    assert isinstance(scenario, Scenario)
    result = run_receive_scenario(scenario, profile_a)
    assert result.state.value == scenario.expected_receive_state
    if result.raw:
        program = process_received(result, profile_a)
        stored = ReceiveArchive(paths).store(result, program, profile_a)
        assert stored is not None and stored.nc_path.is_file()
        expected_dir = paths.received if program.complete else paths.failed
        assert stored.nc_path.parent == expected_dir


def test_cts_off_without_handshake_profile_still_sends(profile_b: Profile) -> None:
    result, transport = run_send_scenario(get_scenario("cts_off"), profile_b)
    assert result.state is SendState.COMPLETED
    assert transport.machine_received


def test_receive_no_m30_is_incomplete(profile_a: Profile) -> None:
    result = run_receive_scenario(get_scenario("receive_no_m30"), profile_a)
    assert result.end_reason is EndReason.INACTIVITY
    assert not process_received(result, profile_a).complete


def test_receive_nulls_removed(profile_a: Profile) -> None:
    result = run_receive_scenario(get_scenario("receive_nulls"), profile_a)
    program = process_received(result, profile_a)
    assert b"\x00" in result.raw and b"\x00" not in program.normalized
    assert program.complete


def test_mixed_endings_and_extra_crlf(profile_a: Profile) -> None:
    result = run_receive_scenario(get_scenario("receive_mixed_endings"), profile_a)
    program = process_received(result, profile_a)
    assert b"\n" in result.raw.replace(b"\r\n", b"")
    assert program.normalized.count(b"\r\n") == program.normalized.count(b"\n")
    assert program.complete


def test_corrupted_bytes_detected(profile_a: Profile) -> None:
    result = run_receive_scenario(get_scenario("receive_corrupted"), profile_a)
    program = process_received(result, profile_a)
    assert program.normalized != get_scenario("receive_ok").config.machine_program


def test_invalid_ascii_file_blocked(profile_a: Profile) -> None:
    scenario = get_scenario("invalid_ascii_file")
    analysis = analyze(scenario.sample_program, profile_a.transfer, profile_a.serial)
    assert analysis.has_errors
    assert {"NON_ASCII", "CONTROL_CHARS"} <= {f.code for f in analysis.errors}


def test_user_cancel_receive(profile_a: Profile) -> None:
    result = run_receive_scenario(get_scenario("user_cancel"), profile_a)
    assert result.state is ReceiveState.CANCELLED
    assert 100 <= len(result.raw) < 400


def test_slow_scenario_takes_longer(profile_a: Profile) -> None:
    fast_clock, slow_clock = FakeClock(), FakeClock()
    fast, _ = run_send_scenario(get_scenario("send_ok"), profile_a, clock=fast_clock)
    slow, _ = run_send_scenario(get_scenario("slow"), profile_a, clock=slow_clock)
    delays = (profile_a.transfer.start_delay_ms + profile_a.transfer.end_delay_ms) / 1000
    assert slow.duration_s - delays > (fast.duration_s - delays) * 4


def test_reproducible_results(profile_a: Profile) -> None:
    first = run_receive_scenario(get_scenario("receive_corrupted"), profile_a)
    second = run_receive_scenario(get_scenario("receive_corrupted"), profile_a)
    assert first.raw == second.raw and first.duration_s == second.duration_s


def test_simulation_runs_fast_with_fake_clock(profile_a: Profile) -> None:
    start = time.perf_counter()
    for scenario in PLAIN:
        if scenario.expected_send_state:
            run_send_scenario(scenario, profile_a)
        if scenario.kind in (ScenarioKind.RECEIVE, ScenarioKind.BOTH):
            run_receive_scenario(scenario, profile_a)
    assert time.perf_counter() - start < 10


def test_no_real_serial_port_used(monkeypatch: pytest.MonkeyPatch, profile_a: Profile) -> None:
    import serial

    def forbidden(*_args: object, **_kwargs: object) -> None:
        raise AssertionError("real serial port must not be used")

    monkeypatch.setattr(serial, "Serial", forbidden)
    run_send_scenario(get_scenario("send_ok"), profile_a)
    run_receive_scenario(get_scenario("receive_ok"), profile_a)


def test_sample_files_are_not_modified(tmp_path: Path, profile_a: Profile) -> None:
    path = tmp_path / "PROG.nc"
    original = get_scenario("send_ok").sample_program
    path.write_bytes(original)
    run_send_scenario(get_scenario("send_ok"), profile_a, data=path.read_bytes())
    assert path.read_bytes() == original
