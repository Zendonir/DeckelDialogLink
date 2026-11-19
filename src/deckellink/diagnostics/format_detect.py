"""Passive detection of parity / data bits from data sent by the CNC.

The port is opened as 8N1 at the profile's baud rate and DeckelLink **only
listens** - nothing is written to the machine. A 7-bit character with parity
(7E1/7O1) occupies exactly one 8N1 frame: the parity bit appears as bit 7 of the
received byte. Checking bit 7 against the 7 data bits therefore reveals even or
odd parity. With 7N1/7N2 bit 7 samples the stop bit (always 1).

Limitations (stated in the result): the baud rate must already be right (judged
by the share of printable characters); a continuous 7N1 stream can cause framing
errors when read as 8N1; mark parity cannot be told apart from 7N2.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field, replace

from deckellink.clock import Clock
from deckellink.domain.models import Parity, SerialSettings
from deckellink.domain.results import Direction, TrafficEvent
from deckellink.transfer.cancellation import CancellationToken
from deckellink.transport.base import Transport

MIN_SAMPLE = 32
CONFIDENT_SHARE = 0.97
PRINTABLE_OK = 0.85


def capture_settings(profile_serial: SerialSettings) -> SerialSettings:
    """8N1 at the profile's baud rate; flow control kept so the CNC is allowed to send."""
    return replace(profile_serial, data_bits=8, parity=Parity.NONE, stop_bits=1)


def _even_parity_bit(value: int) -> int:
    return bin(value & 0x7F).count("1") & 1


def _printable(value: int) -> bool:
    low = value & 0x7F
    return 0x20 <= low < 0x7F or low in (0x0A, 0x0D, 0x09, 0x00, 0x25)


@dataclass(frozen=True)
class FormatGuess:
    sample_size: int
    printable_share: float
    even_share: float
    odd_share: float
    msb_one_share: float
    msb_zero_share: float
    data_bits: int | None
    parity: Parity | None
    stop_bits: float | None
    confidence: str  # "hoch" | "mittel" | "keine"
    explanation: tuple[str, ...] = field(default=())

    @property
    def frame(self) -> str:
        if self.data_bits is None or self.parity is None or self.stop_bits is None:
            return "unbestimmt"
        return f"{self.data_bits}{self.parity.letter}{int(self.stop_bits)}"

    def report(self) -> str:
        lines = [
            f"Ausgewertete Zeichen: {self.sample_size}",
            f"Lesbare Zeichen (Baudrate plausibel): {self.printable_share:.0%}",
            f"Bit 7 passt zu gerader Parität: {self.even_share:.0%}",
            f"Bit 7 passt zu ungerader Parität: {self.odd_share:.0%}",
            f"Bit 7 immer 1 (Stoppbit/Mark): {self.msb_one_share:.0%}",
            "",
            f"Ergebnis: {self.frame} (Sicherheit: {self.confidence})",
        ]
        lines.extend(self.explanation)
        return "\n".join(lines)


def analyze_capture(data: bytes) -> FormatGuess:
    """Evaluate bytes received as 8N1. Pure function (unit tested)."""
    sample = bytes(b for b in data if b not in (0x00, 0x80, 0xFF))
    n = len(sample)
    if n < MIN_SAMPLE:
        return FormatGuess(
            n,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            None,
            None,
            None,
            "keine",
            (
                f"Zu wenige Daten (mindestens {MIN_SAMPLE} Zeichen nötig). Bitte ein längeres Programm "
                "von der CNC ausgeben.",
            ),
        )
    printable = sum(1 for b in sample if _printable(b)) / n
    even = sum(1 for b in sample if (b >> 7) == _even_parity_bit(b)) / n
    odd = sum(1 for b in sample if (b >> 7) != _even_parity_bit(b)) / n
    msb_one = sum(1 for b in sample if b & 0x80) / n
    msb_zero = 1.0 - msb_one
    notes: list[str] = []
    if printable < PRINTABLE_OK:
        notes.append(
            "Viele unlesbare Zeichen: Die Baudrate im Profil passt vermutlich nicht. Andere Baudrate "
            "einstellen und erneut mitlesen."
        )
        return FormatGuess(n, printable, even, odd, msb_one, msb_zero, None, None, None, "keine", tuple(notes))

    data_bits: int | None = 7
    parity: Parity | None
    stop: float | None = 1
    if msb_one >= CONFIDENT_SHARE:
        parity = Parity.NONE
        notes.append(
            "Bit 7 ist praktisch immer 1: kein Paritätsbit (7N1 oder 7N2). Mark-Parität (7M1) ist davon "
            "nicht unterscheidbar."
        )
    elif msb_zero >= CONFIDENT_SHARE:
        parity = Parity.NONE
        data_bits = 8
        notes.append("Bit 7 ist praktisch immer 0: 8 Datenbits ohne Parität (8N1) oder Space-Parität (7S1).")
    elif even >= CONFIDENT_SHARE:
        parity = Parity.EVEN
        notes.append("Bit 7 entspricht der geraden Parität: 7E1.")
    elif odd >= CONFIDENT_SHARE:
        parity = Parity.ODD
        notes.append("Bit 7 entspricht der ungeraden Parität: 7O1.")
    else:
        notes.append(
            "Kein eindeutiges Muster. Mögliche Ursachen: Übertragungsstörungen, Rahmenfehler bei 7N1 "
            "oder 8 Datenbits mit Sonderzeichen."
        )
        return FormatGuess(n, printable, even, odd, msb_one, msb_zero, None, None, None, "keine", tuple(notes))
    confidence = "hoch" if n >= 200 and printable >= 0.95 else "mittel"
    notes.append(
        "Das Ergebnis ist ein Hinweis. Bestätigen Sie es durch einen normalen Empfang mit dem passenden "
        "Profil und tragen Sie es in die Verifikations-Checkliste ein."
    )
    return FormatGuess(n, printable, even, odd, msb_one, msb_zero, data_bits, parity, stop, confidence, tuple(notes))


def capture_passively(
    transport: Transport,
    *,
    duration_s: float,
    max_bytes: int,
    clock: Clock,
    token: CancellationToken,
    on_traffic: Callable[[TrafficEvent], None] | None = None,
    on_progress: Callable[[int], None] | None = None,
) -> bytes:
    """Read only. Never writes to the transport. Ends after ``duration_s`` or ``max_bytes``."""
    buffer = bytearray()
    transport.open()
    try:
        deadline = clock.monotonic() + duration_s
        while clock.monotonic() < deadline and len(buffer) < max_bytes and not token.is_cancelled:
            chunk = transport.read(256)
            if chunk:
                buffer += chunk
                if on_traffic:
                    on_traffic(TrafficEvent(Direction.RX, clock.now(), chunk))
                if on_progress:
                    on_progress(len(buffer))
    finally:
        transport.close()
    return bytes(buffer[:max_bytes])


def encode_frames_as_8n1(text: bytes, parity: Parity) -> bytes:
    """How a 7-bit stream with ``parity`` looks when read as 8N1 (simulator / tests)."""
    out = bytearray()
    for value in text:
        low = value & 0x7F
        if parity is Parity.EVEN:
            bit = _even_parity_bit(low)
        elif parity is Parity.ODD:
            bit = 1 - _even_parity_bit(low)
        elif parity is Parity.SPACE:
            bit = 0
        else:  # NONE (stop bit sampled) or MARK
            bit = 1
        out.append(low | (bit << 7))
    return bytes(out)
