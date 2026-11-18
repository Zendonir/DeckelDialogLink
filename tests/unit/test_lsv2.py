"""LSV2 / DIN 66019 link layer, program transfer and drip-feed (simulated CNC only)."""

from __future__ import annotations

import json
from dataclasses import replace
from typing import Any

import pytest

from deckellink.clock import FakeClock
from deckellink.domain.errors import ProfileValidationError
from deckellink.domain.models import BccMode, Profile, ProtocolMode, ProtocolSettings
from deckellink.domain.results import TrafficEvent, TransferProgress
from deckellink.domain.states import EndReason, ReceiveState, SendState
from deckellink.nc.transformer import transform
from deckellink.profiles.loader import builtin_profiles_dir
from deckellink.profiles.repository import ProfileRepository
from deckellink.profiles.schema import parse_profile, safety_warnings
from deckellink.simulation.lsv2_cnc import Lsv2SimConfig, Lsv2SimulatedCnc
from deckellink.simulation.runner import run_lsv2_receive_scenario, run_lsv2_send_scenario
from deckellink.simulation.scenarios import SAMPLE_PROGRAM, SCENARIOS, get_scenario
from deckellink.transfer.cancellation import CancellationToken
from deckellink.transfer.lsv2 import (
    ACK0,
    ACK1,
    DLE,
    ETX,
    STX,
    WACK,
    EventKind,
    FrameDecoder,
    Lsv2Error,
    encode_telegram,
    xor_bytes,
)
from deckellink.transfer.lsv2_transfer import Lsv2ProgramReceiver, Lsv2ProgramSender, build_telegrams
from deckellink.transfer.receiver import ReceiveCallbacks, process_received
from deckellink.transfer.sender import SendCallbacks

P = ProtocolSettings(mode=ProtocolMode.LSV2)
LSV2_SCENARIOS = [s for s in SCENARIOS if s.protocol is ProtocolMode.LSV2]


@pytest.fixture
def lsv2(repo: ProfileRepository) -> Profile:
    profile = repo.get("dialog11_lsv2_7e1")
    assert profile is not None
    return profile


# ---- framing ------------------------------------------------------------------
def test_encode_transparent_with_bcc() -> None:
    frame = encode_telegram(b"N10 X1\r\n", P)
    assert frame[:2] == bytes((DLE, STX))
    assert frame[-3:-1] == bytes((DLE, ETX))
    assert frame[-1] == xor_bytes(b"N10 X1\r\n") ^ ETX


def test_dle_stuffing_and_transmitted_bcc() -> None:
    settings = replace(P, bcc_mode=BccMode.TRANSMITTED)
    frame = encode_telegram(b"A\x10B", settings)
    assert frame == bytes((DLE, STX)) + b"A\x10\x10B" + bytes((DLE, ETX)) + bytes(
        (xor_bytes(b"A\x10\x10B" + bytes((DLE, ETX))),)
    )
    events = FrameDecoder(settings).feed(frame)
    assert events[0].kind is EventKind.TELEGRAM
    assert events[0].text == b"A\x10B" and events[0].bcc_ok


def test_non_transparent_and_no_bcc() -> None:
    settings = replace(P, transparent=False, bcc_mode=BccMode.NONE)
    frame = encode_telegram(b"N10", settings)
    assert frame == b"\x02N10\x03"
    assert FrameDecoder(settings).feed(frame)[0].text == b"N10"
    with pytest.raises(Lsv2Error):
        encode_telegram(b"A\x03B", settings)


@pytest.mark.parametrize("mode", list(BccMode))
@pytest.mark.parametrize("transparent", [True, False])
def test_roundtrip_all_modes(mode: BccMode, transparent: bool) -> None:
    settings = replace(P, bcc_mode=mode, transparent=transparent)
    decoder = FrameDecoder(settings)
    frame = encode_telegram(b"N20 G01 X5 Y-3.5\r\n", settings)
    events = []
    for byte in b"\x00\x00" + frame:  # byte-by-byte with leading noise
        events.extend(decoder.feed(bytes((byte,))))
    assert [e.kind for e in events] == [EventKind.TELEGRAM]
    assert events[0].bcc_ok and events[0].text == b"N20 G01 X5 Y-3.5\r\n"
    assert decoder.ignored_bytes == 2


def test_bcc_error_detected() -> None:
    frame = bytearray(encode_telegram(b"N10", P))
    frame[-1] ^= 0x01
    assert not FrameDecoder(P).feed(bytes(frame))[0].bcc_ok


def test_control_events() -> None:
    events = FrameDecoder(P).feed(b"\x05\x04\x15" + ACK0 + ACK1 + WACK + b"\x10<" + b"\x10Z")
    assert [e.kind for e in events] == [
        EventKind.ENQ,
        EventKind.EOT,
        EventKind.NAK,
        EventKind.ACK0,
        EventKind.ACK1,
        EventKind.WACK,
        EventKind.RVI,
    ]


def test_decoder_resync_and_overflow() -> None:
    decoder = FrameDecoder(P, max_text=4)
    events = decoder.feed(b"\x10\x02AB\x10\x02" + encode_telegram(b"CD", P)[2:])
    assert events[-1].text == b"CD" and events[-1].bcc_ok
    overflow = decoder.feed(b"\x10\x02ABCDEFG")
    assert overflow and not overflow[0].bcc_ok
    bad = FrameDecoder(P).feed(b"\x10\x02AB\x10Q")
    assert bad[0].kind is EventKind.TELEGRAM and not bad[0].bcc_ok


# ---- telegram building ----------------------------------------------------------------
def test_build_telegrams(lsv2: Profile) -> None:
    payload = transform(SAMPLE_PROGRAM, lsv2.transfer)
    telegrams = build_telegrams(payload, lsv2)
    assert len(telegrams) == payload.line_count
    assert b"".join(t for t, _ in telegrams) == payload.data
    assert telegrams[-1][1] == len(payload.data)
    two = replace(lsv2, protocol=replace(lsv2.protocol, lines_per_telegram=5, include_line_ending=False))
    grouped = build_telegrams(payload, two)
    assert len(grouped) == 3 and b"\r" not in grouped[0][0]
    tiny = replace(lsv2, protocol=replace(lsv2.protocol, max_telegram_length=8))
    with pytest.raises(Lsv2Error, match="Zeile"):
        build_telegrams(payload, tiny)


# ---- sending / drip-feed -------------------------------------------------------------
def test_memory_transfer(lsv2: Profile) -> None:
    states: list[SendState] = []
    traffic: list[TrafficEvent] = []
    clock = FakeClock()
    cnc = Lsv2SimulatedCnc(lsv2.serial, lsv2.protocol, Lsv2SimConfig(), clock)
    payload = transform(SAMPLE_PROGRAM, lsv2.transfer)
    result = Lsv2ProgramSender(
        cnc, lsv2, clock=clock, callbacks=SendCallbacks(on_state=states.append, on_traffic=traffic.append)
    ).send(payload)
    assert result.state is SendState.COMPLETED
    assert cnc.received_program == payload.data
    assert cnc.telegrams_received == payload.line_count
    assert cnc.eot_received
    assert SendState.SENDING in states and SendState.DRIP_FEEDING not in states
    assert not cnc.is_open
    sent = b"".join(e.data for e in traffic if e.direction.value == "TX")
    assert sent.startswith(b"\x05") and sent.endswith(b"\x04")


def test_drip_feed_is_paced_by_cnc(lsv2: Profile) -> None:
    clock = FakeClock()
    progress: list[TransferProgress] = []
    cnc = Lsv2SimulatedCnc(
        lsv2.serial, lsv2.protocol, Lsv2SimConfig(buffer_telegrams=3, execution_s_per_telegram=2.0), clock
    )
    payload = transform(SAMPLE_PROGRAM, lsv2.transfer)
    states: list[SendState] = []
    result = Lsv2ProgramSender(
        cnc,
        lsv2,
        drip_feed=True,
        clock=clock,
        callbacks=SendCallbacks(on_state=states.append, on_progress=progress.append),
    ).send(payload)
    assert result.state is SendState.COMPLETED
    assert SendState.DRIP_FEEDING in states
    assert cnc.received_program == payload.data
    # 12 telegrams, buffer 3 -> the CNC acknowledges telegram 4..12 only after executing blocks
    assert result.duration_s >= (payload.line_count - 3) * 2.0
    assert progress[-1].current_line == payload.line_count


def test_drip_feed_waits_longer_than_normal_ack_timeout(lsv2: Profile) -> None:
    # Execution takes 30 s per block, far more than ack_timeout_ms (5 s): drip-feed must wait.
    clock = FakeClock()
    cnc = Lsv2SimulatedCnc(
        lsv2.serial, lsv2.protocol, Lsv2SimConfig(buffer_telegrams=1, execution_s_per_telegram=30.0), clock
    )
    payload = transform(b"N10\r\nN20\r\nN30 M30\r\n", lsv2.transfer)
    assert Lsv2ProgramSender(cnc, lsv2, drip_feed=True, clock=clock).send(payload).state is SendState.COMPLETED
    clock2 = FakeClock()
    cnc2 = Lsv2SimulatedCnc(
        lsv2.serial, lsv2.protocol, Lsv2SimConfig(buffer_telegrams=1, execution_s_per_telegram=30.0), clock2
    )
    normal = Lsv2ProgramSender(cnc2, lsv2, drip_feed=False, clock=clock2).send(payload)
    assert normal.state is SendState.FAILED
    assert normal.error is not None and "quittiert" in normal.error.title


@pytest.mark.parametrize("scenario", LSV2_SCENARIOS, ids=lambda s: s.id)
def test_lsv2_scenarios(scenario: Any, lsv2: Profile) -> None:
    if scenario.expected_send_state:
        result, cnc = run_lsv2_send_scenario(scenario, lsv2)
        assert result.state.value == scenario.expected_send_state
        assert not cnc.is_open
        if result.state is SendState.COMPLETED:
            assert cnc.received_program == transform(scenario.sample_program, lsv2.transfer).data
        else:
            assert result.error is not None and result.error.causes
    if scenario.expected_drip_state:
        drip, _ = run_lsv2_send_scenario(scenario, lsv2, drip_feed=True)
        assert drip.state.value == scenario.expected_drip_state
    if scenario.expected_receive_state:
        received, _ = run_lsv2_receive_scenario(scenario, lsv2)
        assert received.state.value == scenario.expected_receive_state


def test_nak_and_lost_ack_do_not_duplicate(lsv2: Profile) -> None:
    for scenario_id in ("lsv2_nak", "lsv2_lost_ack", "lsv2_wack"):
        result, cnc = run_lsv2_send_scenario(get_scenario(scenario_id), lsv2)
        payload = transform(SAMPLE_PROGRAM, lsv2.transfer)
        assert result.state is SendState.COMPLETED, scenario_id
        assert cnc.telegrams_received == payload.line_count, scenario_id
        assert cnc.received_program == payload.data, scenario_id
    _, nak_cnc = run_lsv2_send_scenario(get_scenario("lsv2_nak"), lsv2)
    assert nak_cnc.naks_sent == 2


def test_retry_limit(lsv2: Profile) -> None:
    profile = replace(lsv2, protocol=replace(lsv2.protocol, max_retries=1))
    clock = FakeClock()
    cnc = Lsv2SimulatedCnc(profile.serial, profile.protocol, Lsv2SimConfig(), clock)
    # CNC expects a different BCC -> every telegram is answered with NAK
    cnc._decoder = FrameDecoder(replace(profile.protocol, bcc_mode=BccMode.TRANSMITTED))
    payload = transform(b"N10 X1\r\nM30\r\n", profile.transfer)
    result = Lsv2ProgramSender(cnc, profile, clock=clock).send(payload)
    assert result.state is SendState.FAILED
    assert result.error is not None and "NAK" in result.error.title


def test_cnc_abort_and_not_ready_messages(lsv2: Profile) -> None:
    abort, cnc = run_lsv2_send_scenario(get_scenario("lsv2_cnc_abort"), lsv2, drip_feed=True)
    assert abort.error is not None and "EOT" in abort.error.title
    assert cnc.telegrams_received == 5
    short = replace(lsv2, protocol=replace(lsv2.protocol, link_timeout_ms=5000))
    clock = FakeClock()
    not_ready, _ = run_lsv2_send_scenario(get_scenario("lsv2_not_ready"), short, clock=clock)
    assert not_ready.error is not None and "DLE 0" in not_ready.error.title
    assert 5.0 <= not_ready.duration_s <= 8.0


def test_user_cancel_sends_eot(lsv2: Profile) -> None:
    result, cnc = run_lsv2_send_scenario(get_scenario("lsv2_dripfeed"), lsv2, drip_feed=True, cancel_after_bytes=60)
    assert result.state is SendState.CANCELLED
    assert cnc.eot_received
    assert cnc.telegrams_received < transform(SAMPLE_PROGRAM, lsv2.transfer).line_count
    no_eot = replace(lsv2, protocol=replace(lsv2.protocol, send_eot_on_cancel=False))
    _, cnc2 = run_lsv2_send_scenario(get_scenario("lsv2_dripfeed"), no_eot, drip_feed=True, cancel_after_bytes=60)
    assert not cnc2.eot_received


def test_start_and_end_telegrams(lsv2: Profile) -> None:
    profile = replace(lsv2, protocol=replace(lsv2.protocol, start_telegrams=("%{name}",), end_telegrams=("END",)))
    clock = FakeClock()
    cnc = Lsv2SimulatedCnc(profile.serial, profile.protocol, Lsv2SimConfig(), clock)
    payload = transform(b"N10\r\nM30\r\n", profile.transfer)
    Lsv2ProgramSender(cnc, profile, program_name="TEIL1", clock=clock).send(payload)
    assert cnc.received_texts[0] == b"%TEIL1" and cnc.received_texts[-1] == b"END"


def test_handshake_lines_respected(lsv2: Profile) -> None:
    profile = replace(lsv2, serial=replace(lsv2.serial, require_cts_before_send=True, handshake_timeout_ms=1000))
    clock = FakeClock()
    cnc = Lsv2SimulatedCnc(profile.serial, profile.protocol, Lsv2SimConfig(cts=False), clock)
    result = Lsv2ProgramSender(cnc, profile, clock=clock).send(transform(b"M30\r\n", profile.transfer))
    assert result.state is SendState.FAILED
    assert result.error is not None and "CTS" in result.error.title


# ---- receiving --------------------------------------------------------------------------------
def test_receive_program(lsv2: Profile) -> None:
    traffic: list[TrafficEvent] = []
    clock = FakeClock()
    cnc = Lsv2SimulatedCnc(lsv2.serial, lsv2.protocol, Lsv2SimConfig(program=SAMPLE_PROGRAM), clock)
    result = Lsv2ProgramReceiver(
        cnc, lsv2, clock=clock, callbacks=ReceiveCallbacks(on_traffic=traffic.append)
    ).receive()
    assert result.state is ReceiveState.COMPLETED
    assert result.end_reason is EndReason.EOT
    assert result.raw == SAMPLE_PROGRAM
    assert process_received(result, lsv2).complete
    assert cnc.sent_eot


def test_receive_with_corrupted_telegram_and_without_line_endings(lsv2: Profile) -> None:
    profile = replace(lsv2, protocol=replace(lsv2.protocol, include_line_ending=False))
    result, _ = run_lsv2_receive_scenario(get_scenario("lsv2_nak"), profile)
    assert result.state is ReceiveState.COMPLETED
    assert result.raw == SAMPLE_PROGRAM


def test_receive_timeouts_and_limits(lsv2: Profile) -> None:
    quiet = replace(lsv2, receive=replace(lsv2.receive, first_byte_timeout_ms=3000))
    clock = FakeClock()
    cnc = Lsv2SimulatedCnc(quiet.serial, quiet.protocol, Lsv2SimConfig(program=SAMPLE_PROGRAM, never_ready=True), clock)
    result = Lsv2ProgramReceiver(cnc, quiet, clock=clock).receive()
    assert result.state is ReceiveState.TIMED_OUT and result.end_reason is EndReason.NO_DATA
    small = replace(lsv2, receive=replace(lsv2.receive, max_receive_bytes=50))
    limited, _ = run_lsv2_receive_scenario(get_scenario("lsv2_ok"), small)
    assert limited.state is ReceiveState.FAILED and limited.end_reason is EndReason.MAX_SIZE
    assert len(limited.raw) <= 50
    token = CancellationToken()
    token.cancel()
    clock3 = FakeClock()
    cnc3 = Lsv2SimulatedCnc(lsv2.serial, lsv2.protocol, Lsv2SimConfig(program=SAMPLE_PROGRAM), clock3)
    assert Lsv2ProgramReceiver(cnc3, lsv2, clock=clock3, token=token).receive().state is ReceiveState.CANCELLED


# ---- profile --------------------------------------------------------------------------------
def test_lsv2_profile_and_validation(lsv2: Profile) -> None:
    assert lsv2.protocol.is_lsv2
    assert lsv2.verification_status.value == "unverified"
    assert any("LSV2" in w for w in safety_warnings(lsv2))
    data = json.loads((builtin_profiles_dir() / "dialog11_lsv2_7e1.json").read_text(encoding="utf-8"))
    profile, _ = parse_profile(data)
    assert parse_profile(profile.to_dict())[0] == profile
    for key, value in (
        ("mode", "zmodem"),
        ("bcc_mode", "crc16"),
        ("lines_per_telegram", 0),
        ("start_telegrams", "M82"),
        ("start_telegrams", ["ä"]),
        ("end_telegrams", ["x"] * 11),
    ):
        broken = json.loads(json.dumps(data))
        broken["protocol"][key] = value
        with pytest.raises(ProfileValidationError) as info:
            parse_profile(broken)
        assert f"protocol.{key}" in [e.field for e in info.value.errors]
    risky = replace(
        lsv2,
        protocol=replace(
            lsv2.protocol,
            drip_feed_ack_timeout_ms=0,
            start_telegrams=("M82",),
            max_retries=0,
            include_line_ending=False,
        ),
    )
    text = "\n".join(safety_warnings(risky))
    assert "ohne Zeitlimit" in text and "Starttelegramme" not in text and "Start-/Endtelegramme" in text
    assert "ohne Wiederholungen" in text and "ohne Zeilenende" in text


def test_plain_profiles_default_to_plain_protocol(profile_a: Profile) -> None:
    assert profile_a.protocol.mode is ProtocolMode.PLAIN
