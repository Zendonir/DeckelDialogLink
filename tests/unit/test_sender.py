from __future__ import annotations

from dataclasses import replace

import pytest

from deckellink.clock import FakeClock
from deckellink.domain.models import Profile
from deckellink.domain.results import TrafficEvent, TransferProgress, TransformResult
from deckellink.domain.states import SendState
from deckellink.nc.transformer import transform
from deckellink.simulation.scenarios import SAMPLE_PROGRAM
from deckellink.transfer.cancellation import CancellationToken
from deckellink.transfer.sender import ProgramSender, SendCallbacks
from deckellink.transport.base import ModemStatus
from deckellink.transport.simulated_transport import SimulatedTransport, SimulationConfig


def payload(profile: Profile, data: bytes = SAMPLE_PROGRAM) -> TransformResult:
    return transform(data, profile.transfer)


def make_sender(
    profile: Profile,
    config: SimulationConfig,
    clock: FakeClock,
    token: CancellationToken | None = None,
    callbacks: SendCallbacks | None = None,
) -> tuple[ProgramSender, SimulatedTransport]:
    transport = SimulatedTransport(profile.serial, config, clock)
    return ProgramSender(transport, profile, clock=clock, token=token, callbacks=callbacks), transport


def test_complete_transfer(profile_a: Profile, clock: FakeClock) -> None:
    states: list[SendState] = []
    progress: list[TransferProgress] = []
    traffic: list[TrafficEvent] = []
    modem: list[ModemStatus] = []
    callbacks = SendCallbacks(states.append, progress.append, traffic.append, modem.append)
    sender, transport = make_sender(profile_a, SimulationConfig(), clock, callbacks=callbacks)
    data = payload(profile_a)
    result = sender.send(data)
    assert result.state is SendState.COMPLETED
    assert transport.machine_received == data.data
    assert result.bytes_sent == result.bytes_total == len(data.data)
    assert states == [
        SendState.OPENING_PORT,
        SendState.WAITING_FOR_HANDSHAKE,
        SendState.SENDING,
        SendState.FINISHING,
        SendState.COMPLETED,
    ]
    assert b"".join(e.data for e in traffic) == data.data
    assert progress[-1].percent == 100.0
    assert progress[-1].current_line == data.line_count
    assert not transport.is_open
    assert modem and modem[0].cts is True
    # bytes at 4800/9 B/s (7N1) + 1 s start + 1 s end delay
    assert result.duration_s == pytest.approx(2 + len(data.data) / profile_a.serial.bytes_per_second, rel=0.01)
    assert result.error is None
    assert result.to_dict()["state"] == "completed"


def test_partial_writes(fast_profile: Profile, clock: FakeClock) -> None:
    sender, transport = make_sender(fast_profile, SimulationConfig(max_write_chunk=3), clock)
    data = payload(fast_profile)
    result = sender.send(data)
    assert result.state is SendState.COMPLETED
    assert transport.machine_received == data.data


def test_write_timeout(fast_profile: Profile, clock: FakeClock) -> None:
    sender, transport = make_sender(fast_profile, SimulationConfig(write_stall_after_bytes=50), clock)
    result = sender.send(payload(fast_profile))
    assert result.state is SendState.FAILED
    assert result.bytes_sent == 50
    assert result.error is not None and "Zeitüberschreitung" in result.error.title
    assert not transport.is_open


def test_cts_wait_then_success(fast_profile: Profile, clock: FakeClock) -> None:
    sender, _ = make_sender(fast_profile, SimulationConfig(cts=False, cts_on_after_s=2.0), clock)
    result = sender.send(payload(fast_profile))
    assert result.state is SendState.COMPLETED
    assert result.duration_s >= 2.0


def test_cts_never_arrives_handshake_timeout(fast_profile: Profile, clock: FakeClock) -> None:
    profile = replace(fast_profile, serial=replace(fast_profile.serial, handshake_timeout_ms=3000))
    sender, transport = make_sender(profile, SimulationConfig(cts=False), clock)
    result = sender.send(payload(profile))
    assert result.state is SendState.FAILED
    assert result.bytes_sent == 0
    assert transport.machine_received == b""
    assert result.error is not None and "CTS" in result.error.title
    assert result.duration_s == pytest.approx(3.0, abs=0.1)
    assert not transport.is_open


def test_dsr_required(fast_profile: Profile, clock: FakeClock) -> None:
    profile = replace(
        fast_profile,
        serial=replace(fast_profile.serial, require_dsr_before_send=True, handshake_timeout_ms=500),
    )
    sender, _ = make_sender(profile, SimulationConfig(dsr=False), clock)
    result = sender.send(payload(profile))
    assert result.state is SendState.FAILED
    assert result.error is not None and "DSR" in result.error.title


def test_user_cancel_stops_sending(fast_profile: Profile, clock: FakeClock) -> None:
    token = CancellationToken()

    def on_progress(progress: TransferProgress) -> None:
        if progress.bytes_done >= 64:
            token.cancel()

    sender, transport = make_sender(
        fast_profile, SimulationConfig(), clock, token, SendCallbacks(on_progress=on_progress)
    )
    result = sender.send(payload(fast_profile))
    assert result.state is SendState.CANCELLED
    assert result.bytes_sent == 64
    assert len(transport.machine_received) == 64
    assert result.error is None
    assert not transport.is_open


def test_cancel_before_start(fast_profile: Profile, clock: FakeClock) -> None:
    token = CancellationToken()
    token.cancel()
    sender, transport = make_sender(fast_profile, SimulationConfig(), clock, token)
    result = sender.send(payload(fast_profile))
    assert result.state is SendState.CANCELLED
    assert transport.open_count == 0


def test_cancel_during_start_delay(profile_a: Profile, clock: FakeClock) -> None:
    token = CancellationToken()
    states: list[SendState] = []

    def on_state(state: SendState) -> None:
        states.append(state)
        if state is SendState.WAITING_FOR_HANDSHAKE:
            token.cancel()

    sender, transport = make_sender(profile_a, SimulationConfig(), clock, token, SendCallbacks(on_state=on_state))
    result = sender.send(payload(profile_a))
    assert result.state is SendState.CANCELLED
    assert transport.machine_received == b""


@pytest.mark.parametrize(
    ("config", "title"),
    [
        (SimulationConfig(write_error_after_bytes=10), "Schreibfehler"),
        (SimulationConfig(disconnect_after_bytes=10), "unterbrochen"),
        (SimulationConfig(open_error="busy"), "belegt"),
        (SimulationConfig(open_error="not_found"), "nicht gefunden"),
        (SimulationConfig(open_error="generic"), "nicht geöffnet"),
    ],
)
def test_transport_errors_close_port(
    fast_profile: Profile, clock: FakeClock, config: SimulationConfig, title: str
) -> None:
    sender, transport = make_sender(fast_profile, config, clock)
    result = sender.send(payload(fast_profile))
    assert result.state is SendState.FAILED
    assert result.error is not None
    assert title in result.error.title
    assert result.error.causes
    assert not transport.is_open


def test_progress_calculation(fast_profile: Profile, clock: FakeClock) -> None:
    progress: list[TransferProgress] = []
    sender, _ = make_sender(
        fast_profile, SimulationConfig(), clock, callbacks=SendCallbacks(on_progress=progress.append)
    )
    data = payload(fast_profile)
    sender.send(data)
    done = [p.bytes_done for p in progress]
    assert done == sorted(done)
    assert progress[0].bytes_done == 0
    middle = next(p for p in progress if p.bytes_done >= len(data.data) // 2)
    assert middle.percent == pytest.approx(100 * middle.bytes_done / len(data.data))
    assert middle.current_line == data.line_for_offset(middle.bytes_done)
    assert 1 <= middle.current_line <= data.line_count
    assert progress[-1].rate_bps == pytest.approx(fast_profile.serial.bytes_per_second, rel=0.05)


def test_character_and_packet_delays(fast_profile: Profile, clock: FakeClock) -> None:
    data = payload(fast_profile, b"N10\r\nM30\r\n")
    n = len(data.data)
    bps = fast_profile.serial.bytes_per_second
    profile = replace(fast_profile, transfer=replace(fast_profile.transfer, character_delay_ms=10))
    sender, transport = make_sender(profile, SimulationConfig(), clock)
    result = sender.send(data)
    assert transport.machine_received == data.data
    assert result.duration_s == pytest.approx(n / bps + (n - 1) * 0.01, rel=0.01)

    clock2 = FakeClock()
    profile = replace(fast_profile, transfer=replace(fast_profile.transfer, packet_size=4, packet_delay_ms=100))
    sender, transport = make_sender(profile, SimulationConfig(), clock2)
    result = sender.send(data)
    assert transport.machine_received == data.data
    assert result.duration_s == pytest.approx(n / bps + (n // 4) * 0.1, rel=0.01)


def test_port_left_open_when_configured(fast_profile: Profile, clock: FakeClock) -> None:
    profile = replace(fast_profile, transfer=replace(fast_profile.transfer, close_port_after_send=False))
    sender, transport = make_sender(profile, SimulationConfig(), clock)
    result = sender.send(payload(profile))
    assert result.port_left_open
    assert transport.is_open
    transport.close()


def test_drain_timeout(fast_profile: Profile, clock: FakeClock) -> None:
    class StuckTransport(SimulatedTransport):
        @property
        def out_waiting(self) -> int:
            return 10

    transport = StuckTransport(fast_profile.serial, SimulationConfig(), clock)
    result = ProgramSender(transport, fast_profile, clock=clock).send(payload(fast_profile))
    assert result.state is SendState.FAILED
    assert not transport.is_open


def test_zero_write_stall_detected(fast_profile: Profile, clock: FakeClock) -> None:
    class ZeroWriter(SimulatedTransport):
        def write(self, data: bytes) -> int:
            self._clock.sleep(0.01)
            return 0

    transport = ZeroWriter(fast_profile.serial, SimulationConfig(), clock)
    result = ProgramSender(transport, fast_profile, clock=clock).send(payload(fast_profile))
    assert result.state is SendState.FAILED
    assert result.error is not None and "Zeitüberschreitung" in result.error.title
