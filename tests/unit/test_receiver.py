from __future__ import annotations

from dataclasses import replace

import pytest

from deckellink.clock import FakeClock
from deckellink.domain.models import EndDetection, LineEnding, Profile
from deckellink.domain.results import ReceiveProgress, TrafficEvent
from deckellink.domain.states import EndReason, ReceiveState
from deckellink.simulation.scenarios import SAMPLE_PROGRAM
from deckellink.transfer.cancellation import CancellationToken
from deckellink.transfer.receiver import (
    EndSequenceDetector,
    ProgramReceiver,
    ReceiveCallbacks,
    normalize_received,
    process_received,
)
from deckellink.transport.simulated_transport import SimulatedTransport, SimulationConfig


def receive(
    profile: Profile,
    config: SimulationConfig,
    clock: FakeClock,
    token: CancellationToken | None = None,
    callbacks: ReceiveCallbacks | None = None,
) -> tuple[ProgramReceiver, SimulatedTransport]:
    transport = SimulatedTransport(profile.serial, config, clock)
    return ProgramReceiver(transport, profile, clock=clock, token=token, callbacks=callbacks), transport


def test_receive_until_m30(profile_a: Profile, clock: FakeClock) -> None:
    states: list[ReceiveState] = []
    traffic: list[TrafficEvent] = []
    receiver, transport = receive(
        profile_a,
        SimulationConfig(machine_program=SAMPLE_PROGRAM + b"GARBAGE AFTER\r\n" * 5),
        clock,
        callbacks=ReceiveCallbacks(on_state=states.append, on_traffic=traffic.append),
    )
    result = receiver.receive()
    assert result.state is ReceiveState.COMPLETED
    assert result.end_reason is EndReason.END_SEQUENCE
    assert result.raw.startswith(SAMPLE_PROGRAM)
    assert result.raw.endswith(b"M30\r\n")
    assert b"".join(e.data for e in traffic) == result.raw
    assert states[:3] == [ReceiveState.OPENING_PORT, ReceiveState.LISTENING, ReceiveState.RECEIVING]
    assert states[-1] is ReceiveState.COMPLETED
    assert result.first_byte_at is not None and result.last_byte_at is not None
    assert not transport.is_open


def test_receive_until_inactivity(profile_a: Profile, clock: FakeClock) -> None:
    profile = replace(profile_a, receive=replace(profile_a.receive, end_detection=EndDetection.INACTIVITY))
    receiver, _ = receive(profile, SimulationConfig(machine_program=SAMPLE_PROGRAM), clock)
    result = receiver.receive()
    assert result.state is ReceiveState.COMPLETED
    assert result.end_reason is EndReason.INACTIVITY
    assert result.raw == SAMPLE_PROGRAM
    expected = (
        0.5 + len(SAMPLE_PROGRAM) / profile.serial.bytes_per_second + profile.receive.inactivity_timeout_ms / 1000
    )
    assert result.duration_s == pytest.approx(expected, abs=1.1)


def test_end_sequence_only_mode_times_out_without_m30(profile_a: Profile, clock: FakeClock) -> None:
    profile = replace(profile_a, receive=replace(profile_a.receive, end_detection=EndDetection.END_SEQUENCE))
    data = SAMPLE_PROGRAM.replace(b"N110 M30\r\n", b"")
    receiver, _ = receive(profile, SimulationConfig(machine_program=data), clock)
    result = receiver.receive()
    assert result.state is ReceiveState.TIMED_OUT
    assert result.raw == data


def test_partial_reads(profile_a: Profile, clock: FakeClock) -> None:
    traffic: list[TrafficEvent] = []
    receiver, _ = receive(
        profile_a,
        SimulationConfig(machine_program=SAMPLE_PROGRAM, read_chunk_sizes=(1, 2, 5)),
        clock,
        callbacks=ReceiveCallbacks(on_traffic=traffic.append),
    )
    result = receiver.receive()
    assert result.raw == SAMPLE_PROGRAM
    assert max(len(e.data) for e in traffic) <= 5


def test_null_bytes_received_and_stripped(profile_a: Profile, clock: FakeClock) -> None:
    receiver, _ = receive(
        profile_a, SimulationConfig(machine_program=b"\x00" * 10 + SAMPLE_PROGRAM, inject_null_every=7), clock
    )
    result = receiver.receive()
    assert result.end_reason is EndReason.END_SEQUENCE
    assert b"\x00" in result.raw
    program = process_received(result, profile_a)
    assert program.normalized == SAMPLE_PROGRAM
    assert program.complete
    assert any("Nullbyte" in n for n in program.notes)


def test_missing_m30_marked_incomplete(profile_a: Profile, clock: FakeClock) -> None:
    data = SAMPLE_PROGRAM.replace(b"N110 M30\r\n", b"")
    receiver, _ = receive(profile_a, SimulationConfig(machine_program=data), clock)
    result = receiver.receive()
    assert result.state is ReceiveState.COMPLETED
    assert result.end_reason is EndReason.INACTIVITY
    program = process_received(result, profile_a)
    assert not program.complete
    assert program.analysis.has("MISSING_PROGRAM_END")
    assert any("UNVOLLSTÄNDIG" in n for n in program.notes)


def test_max_receive_size(profile_a: Profile, clock: FakeClock) -> None:
    profile = replace(profile_a, receive=replace(profile_a.receive, max_receive_bytes=100))
    receiver, transport = receive(profile, SimulationConfig(machine_program=SAMPLE_PROGRAM * 3), clock)
    result = receiver.receive()
    assert result.state is ReceiveState.FAILED
    assert result.end_reason is EndReason.MAX_SIZE
    assert len(result.raw) == 100
    assert result.error is not None
    assert not transport.is_open
    assert not process_received(result, profile).complete


def test_user_cancel(profile_a: Profile, clock: FakeClock) -> None:
    token = CancellationToken()

    def on_progress(progress: ReceiveProgress) -> None:
        if progress.bytes_received >= 50:
            token.cancel()

    receiver, transport = receive(
        profile_a,
        SimulationConfig(machine_program=SAMPLE_PROGRAM, read_chunk_sizes=(10,)),
        clock,
        token,
        ReceiveCallbacks(on_progress=on_progress),
    )
    result = receiver.receive()
    assert result.state is ReceiveState.CANCELLED
    assert result.end_reason is EndReason.CANCELLED
    assert len(result.raw) == 50
    assert not transport.is_open


def test_no_data_times_out(profile_a: Profile, clock: FakeClock) -> None:
    profile = replace(profile_a, receive=replace(profile_a.receive, first_byte_timeout_ms=3000))
    receiver, _ = receive(profile, SimulationConfig(machine_program=b""), clock)
    result = receiver.receive()
    assert result.state is ReceiveState.TIMED_OUT
    assert result.end_reason is EndReason.NO_DATA
    assert result.raw == b""
    assert result.duration_s == pytest.approx(3.0, abs=1.1)


def test_connection_lost_keeps_partial_data(profile_a: Profile, clock: FakeClock) -> None:
    receiver, transport = receive(
        profile_a, SimulationConfig(machine_program=SAMPLE_PROGRAM, disconnect_after_bytes=60), clock
    )
    result = receiver.receive()
    assert result.state is ReceiveState.FAILED
    assert result.end_reason is EndReason.ERROR
    assert result.raw == SAMPLE_PROGRAM[:60]
    assert result.error is not None and "unterbrochen" in result.error.title
    assert not transport.is_open


def test_open_error(profile_a: Profile, clock: FakeClock) -> None:
    receiver, _ = receive(profile_a, SimulationConfig(open_error="busy"), clock)
    result = receiver.receive()
    assert result.state is ReceiveState.FAILED
    assert result.error is not None and "belegt" in result.error.title


def test_corrupted_bytes_reported(profile_a: Profile, clock: FakeClock) -> None:
    receiver, _ = receive(profile_a, SimulationConfig(machine_program=b"N10 X\xc1\r\nM30\r\n"), clock)
    result = receiver.receive()
    program = process_received(result, profile_a)
    assert program.decode_errors == 1
    assert program.analysis.has("NON_ASCII")


def test_mixed_line_endings_normalized(profile_a: Profile) -> None:
    assert normalize_received(b"N10\nN20\rN30\r\nM30", profile_a) == b"N10\r\nN20\r\nN30\r\nM30"
    keep = replace(profile_a, receive=replace(profile_a.receive, normalize_line_endings=False, strip_null_bytes=False))
    assert normalize_received(b"N10\n\x00", keep) == b"N10\n\x00"


def test_end_sequence_detector_across_chunks() -> None:
    detector = EndSequenceDetector("M30")
    buffer = b"N10 X1\r\nN20 M3"
    assert not detector.feed(buffer)
    buffer += b"0"
    # token at the very end could still become M300 -> not yet confirmed
    assert not detector.feed(buffer)
    buffer += b"\r"
    assert detector.feed(buffer)
    assert not detector.line_terminated(buffer)
    assert detector.line_terminated(buffer, LineEnding.CR)
    buffer += b"\n"
    assert detector.line_terminated(buffer)


def test_end_sequence_detector_rejects_m300() -> None:
    detector = EndSequenceDetector("M30")
    assert not detector.feed(b"N10 M300\r\n")
    assert detector.feed(b"N10 M300\r\nN20 M30\r\n")
    assert not EndSequenceDetector("").enabled
