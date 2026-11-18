"""Serial loopback test (TX bridged to RX on the adapter/cable)."""

from __future__ import annotations

import logging
from collections.abc import Callable
from dataclasses import dataclass, field
from enum import StrEnum

from deckellink.clock import Clock, SystemClock
from deckellink.diagnostics.error_messages import describe_error
from deckellink.domain.errors import UserFacingError
from deckellink.domain.results import Direction, TrafficEvent
from deckellink.nc.checks import max_value_for_data_bits
from deckellink.transfer.cancellation import CancellationToken, OperationCancelled
from deckellink.transport.base import ModemStatus, Transport

log = logging.getLogger(__name__)


class LoopbackPattern(StrEnum):
    ASCII_TEXT = "ascii_text"
    BYTE_RANGE = "byte_range"
    REPEATED = "repeated"
    VARIABLE_PACKETS = "variable_packets"

    @property
    def label(self) -> str:
        return {
            "ascii_text": "ASCII-Text",
            "byte_range": "Bytefolge (zulässiger Datenbitbereich)",
            "repeated": "Wiederholtes Muster 0x55/0xAA-ähnlich",
            "variable_packets": "Variable Paketgrößen",
        }[self.value]


class LoopbackVerdict(StrEnum):
    PASSED = "passed"
    PARTIAL = "partial"
    FAILED = "failed"

    @property
    def label(self) -> str:
        return {"passed": "Bestanden", "partial": "Teilweise bestanden", "failed": "Fehlgeschlagen"}[self.value]


FAILURE_HINTS = (
    "TX und RX sind am Stecker nicht gebrückt (DB9: Pin 2-3, DB25: Pin 2-3).",
    "Falscher COM-Port ausgewählt.",
    "Der Port ist durch ein anderes Programm belegt.",
    "Falsche Baudrate oder falsches Datenformat (betrifft nur reale Gegenstellen).",
    "Ungeeigneter USB-RS-232-Adapter oder fehlender Treiber.",
    "Handshake blockiert (bei RTS/CTS auch RTS-CTS brücken).",
    "Kein echter RS-232-Pegel (TTL-Adapter mit 3,3/5 V statt ±12 V).",
)


def make_pattern(pattern: LoopbackPattern, length: int, data_bits: int) -> list[bytes]:
    """Return the test data as a list of packets."""
    length = max(1, length)
    max_value = max_value_for_data_bits(data_bits)
    if pattern is LoopbackPattern.ASCII_TEXT:
        text = b"DECKELLINK LOOPBACK 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ\r\n"
        data = bytes(b & max_value for b in (text * (length // len(text) + 1))[:length])
        return [data]
    if pattern is LoopbackPattern.BYTE_RANGE:
        data = bytes((i % (max_value + 1)) for i in range(length))
        return [data]
    if pattern is LoopbackPattern.REPEATED:
        unit = bytes(b & max_value for b in (0x55, 0x2A, 0x55, 0x2A, 0x0F, 0x70))
        return [(unit * (length // len(unit) + 1))[:length]]
    packets = []
    sizes = (1, 2, 7, 16, 31, 64, 3)
    produced = 0
    index = 0
    while produced < length:
        size = min(sizes[index % len(sizes)], length - produced)
        packets.append(bytes(min(((produced + i) % 0x5F) + 0x20, max_value) for i in range(size)))
        produced += size
        index += 1
    return packets


@dataclass
class LoopbackResult:
    verdict: LoopbackVerdict
    sent: bytes
    received: bytes
    duration_s: float
    mismatches: list[int] = field(default_factory=list)
    timed_out: bool = False
    modem: ModemStatus = field(default_factory=ModemStatus)
    error: UserFacingError | None = None
    hints: tuple[str, ...] = ()

    @property
    def matched_bytes(self) -> int:
        return sum(1 for a, b in zip(self.sent, self.received, strict=False) if a == b)

    def summary(self) -> str:
        return (
            f"{self.verdict.label}: {len(self.received)} von {len(self.sent)} Bytes empfangen, "
            f"{len(self.mismatches)} Abweichung(en), Dauer {self.duration_s:.2f} s"
            + (", Zeitüberschreitung" if self.timed_out else "")
        )


def run_loopback_test(
    transport: Transport,
    packets: list[bytes],
    *,
    timeout_s: float = 3.0,
    clock: Clock | None = None,
    token: CancellationToken | None = None,
    on_traffic: Callable[[TrafficEvent], None] | None = None,
) -> LoopbackResult:
    clock = clock or SystemClock()
    token = token or CancellationToken()
    sent = b"".join(packets)
    received = bytearray()
    t0 = clock.monotonic()
    timed_out = False
    modem = ModemStatus()
    try:
        transport.open()
        token.register(transport.cancel)
        transport.reset_input_buffer()
        modem = transport.get_modem_status()
        for packet in packets:
            token.raise_if_cancelled()
            pos = 0
            while pos < len(packet):
                written = transport.write(packet[pos:])
                token.raise_if_cancelled()
                if written <= 0:
                    raise OperationCancelled("Schreiben abgebrochen")
                if on_traffic:
                    on_traffic(TrafficEvent(Direction.TX, clock.now(), packet[pos : pos + written]))
                pos += written
            # Read back what is available so the receive buffer cannot overflow.
            while transport.bytes_waiting > 0:
                chunk = transport.read(transport.bytes_waiting)
                if not chunk:
                    break
                received += chunk
                if on_traffic:
                    on_traffic(TrafficEvent(Direction.RX, clock.now(), chunk))
        deadline = clock.monotonic() + timeout_s
        while len(received) < len(sent):
            token.raise_if_cancelled()
            if clock.monotonic() >= deadline:
                timed_out = True
                break
            chunk = transport.read(len(sent) - len(received))
            if chunk:
                received += chunk
                if on_traffic:
                    on_traffic(TrafficEvent(Direction.RX, clock.now(), chunk))
        modem = transport.get_modem_status()
    except OperationCancelled:
        transport.close()
        return LoopbackResult(
            LoopbackVerdict.FAILED,
            sent,
            bytes(received),
            clock.monotonic() - t0,
            modem=modem,
            error=UserFacingError("Loopback-Test abgebrochen.", operation="Loopback-Test"),
        )
    except Exception as exc:
        transport.close()
        return LoopbackResult(
            LoopbackVerdict.FAILED,
            sent,
            bytes(received),
            clock.monotonic() - t0,
            modem=modem,
            error=describe_error(exc, operation="Loopback-Test", port=transport.name),
            hints=FAILURE_HINTS,
        )
    transport.close()
    duration = clock.monotonic() - t0
    got = bytes(received)
    mismatches = [i for i, (a, b) in enumerate(zip(sent, got, strict=False)) if a != b]
    if got == sent:
        verdict = LoopbackVerdict.PASSED
    elif not got:
        verdict = LoopbackVerdict.FAILED
    else:
        verdict = LoopbackVerdict.PARTIAL
    result = LoopbackResult(
        verdict=verdict,
        sent=sent,
        received=got,
        duration_s=duration,
        mismatches=mismatches,
        timed_out=timed_out,
        modem=modem,
        hints=() if verdict is LoopbackVerdict.PASSED else FAILURE_HINTS,
    )
    log.info("Loopback test on %s: %s", transport.name, result.summary())
    return result
