"""LSV2 link layer (DIN 66019 / ISO 1745 basic mode) over a serial transport.

Implemented procedure (public standard; its exact use by the DIALOG 11 is NOT verified):

* Link establishment: the sending station transmits ``ENQ``; the receiving station
  answers ``DLE 0`` (ACK0) when ready. No answer or ``NAK`` = not ready.
* Telegram: ``[DLE] STX <text> [DLE] ETX <BCC>``. In transparent mode every DLE in
  the text is doubled. The BCC is an XOR checksum (see :class:`BccMode`).
* Acknowledgements alternate ``DLE 1`` / ``DLE 0``. ``NAK`` requests a repetition.
  ``DLE ;`` (WACK) means "received, wait"; the sender polls again with ``ENQ``.
* On a missing acknowledgement the sender transmits ``ENQ``; the receiver repeats
  its last acknowledgement. This avoids duplicated telegrams.
* ``EOT`` ends the link (either side). ``DLE <`` (RVI) from the receiver is treated
  as a request to stop.

The CNC paces the data stream through its acknowledgements. This is what makes
the NC drip-feed mode ("Nachladebetrieb") possible: DeckelLink only delivers the
next telegram after the control acknowledged the previous one. It never starts
program execution.
"""

from __future__ import annotations

import logging
from collections import deque
from collections.abc import Callable
from dataclasses import dataclass
from enum import StrEnum

from deckellink.clock import Clock
from deckellink.domain.errors import DeckelLinkError
from deckellink.domain.models import BccMode, ProtocolSettings
from deckellink.domain.results import Direction, TrafficEvent
from deckellink.transfer.cancellation import CancellationToken, OperationCancelled
from deckellink.transport.base import Transport, TransportTimeoutError

log = logging.getLogger(__name__)

STX = 0x02
ETX = 0x03
EOT = 0x04
ENQ = 0x05
DLE = 0x10
NAK = 0x15
ACK0 = bytes((DLE, 0x30))
ACK1 = bytes((DLE, 0x31))
WACK = bytes((DLE, 0x3B))
RVI = bytes((DLE, 0x3C))


class Lsv2Error(DeckelLinkError):
    """Protocol level failure."""


class Lsv2LinkTimeout(TransportTimeoutError):
    """The CNC did not become ready (no ACK0 on ENQ)."""


class Lsv2AckTimeout(TransportTimeoutError):
    """The CNC did not acknowledge a telegram after all retries."""


class Lsv2RetryExceeded(Lsv2Error):
    """Too many NAKs / repetitions."""


class CncAbortError(Lsv2Error):
    """The CNC ended the transfer (EOT or RVI) before all data was delivered."""


class EventKind(StrEnum):
    ENQ = "ENQ"
    EOT = "EOT"
    NAK = "NAK"
    ACK0 = "ACK0"
    ACK1 = "ACK1"
    WACK = "WACK"
    RVI = "RVI"
    TELEGRAM = "TELEGRAM"


@dataclass(frozen=True)
class LinkEvent:
    kind: EventKind
    text: bytes = b""
    bcc_ok: bool = True


def xor_bytes(data: bytes) -> int:
    value = 0
    for byte in data:
        value ^= byte
    return value


def encode_telegram(text: bytes, settings: ProtocolSettings) -> bytes:
    """Frame ``text`` as a telegram including BCC."""
    if settings.transparent:
        body = text.replace(bytes((DLE,)), bytes((DLE, DLE)))
        frame = bytes((DLE, STX)) + body + bytes((DLE, ETX))
        transmitted_after_stx = body + bytes((DLE, ETX))
    else:
        if any(b in (STX, ETX, DLE) for b in text):
            raise Lsv2Error("Telegrammtext enthält Steuerzeichen (nur im Transparentmodus zulässig).")
        frame = bytes((STX,)) + text + bytes((ETX,))
        transmitted_after_stx = text + bytes((ETX,))
    if settings.bcc_mode is BccMode.NONE:
        return frame
    return frame + bytes((compute_bcc(text, transmitted_after_stx, settings.bcc_mode),))


def compute_bcc(text: bytes, transmitted_after_stx: bytes, mode: BccMode) -> int:
    """XOR block check: over text + ETX, or over all transmitted bytes after STX."""
    if mode is BccMode.TEXT_ETX:
        return xor_bytes(text) ^ ETX
    return xor_bytes(transmitted_after_stx)


class FrameDecoder:
    """Incremental decoder turning received bytes into :class:`LinkEvent` objects.

    Bytes outside of telegrams that are not protocol characters (e.g. NUL leader)
    are ignored and counted in ``ignored_bytes``.
    """

    _IDLE = 0
    _IDLE_DLE = 1
    _TEXT = 2
    _TEXT_DLE = 3
    _BCC = 4

    def __init__(self, settings: ProtocolSettings, max_text: int = 65536) -> None:
        self.settings = settings
        self.max_text = max_text
        self.ignored_bytes = 0
        self._state = self._IDLE
        self._text = bytearray()
        self._raw = bytearray()

    def feed(self, data: bytes) -> list[LinkEvent]:
        events: list[LinkEvent] = []
        for byte in data:
            event = self._step(byte)
            if event is not None:
                events.append(event)
        return events

    def _begin_text(self) -> None:
        self._state = self._TEXT
        self._text = bytearray()
        self._raw = bytearray()

    def _end_text(self) -> LinkEvent | None:
        if self.settings.bcc_mode is BccMode.NONE:
            self._state = self._IDLE
            return LinkEvent(EventKind.TELEGRAM, bytes(self._text), True)
        self._state = self._BCC
        return None

    def _step(self, byte: int) -> LinkEvent | None:
        transparent = self.settings.transparent
        state = self._state
        if state == self._IDLE:
            simple = {ENQ: EventKind.ENQ, EOT: EventKind.EOT, NAK: EventKind.NAK}
            if byte in simple:
                return LinkEvent(simple[byte])
            if byte == DLE:
                self._state = self._IDLE_DLE
            elif byte == STX and not transparent:
                self._begin_text()
            else:
                self.ignored_bytes += 1
            return None
        if state == self._IDLE_DLE:
            self._state = self._IDLE
            mapping = {0x30: EventKind.ACK0, 0x31: EventKind.ACK1, 0x3B: EventKind.WACK, 0x3C: EventKind.RVI}
            if byte in mapping:
                return LinkEvent(mapping[byte])
            if byte == STX and transparent:
                self._begin_text()
            else:
                self.ignored_bytes += 2
            return None
        if state == self._TEXT:
            self._raw.append(byte)
            if transparent and byte == DLE:
                self._state = self._TEXT_DLE
                return None
            if not transparent and byte == ETX:
                return self._end_text()
            self._text.append(byte)
            if len(self._text) > self.max_text:
                self._state = self._IDLE
                return LinkEvent(EventKind.TELEGRAM, bytes(self._text), False)
            return None
        if state == self._TEXT_DLE:
            self._raw.append(byte)
            if byte == DLE:
                self._text.append(DLE)
                self._state = self._TEXT
                return None
            if byte == ETX:
                return self._end_text()
            if byte == STX:  # resynchronisation: a new telegram starts
                self._begin_text()
                return None
            self._state = self._IDLE
            return LinkEvent(EventKind.TELEGRAM, bytes(self._text), False)
        # _BCC
        self._state = self._IDLE
        text = bytes(self._text)
        expected = compute_bcc(text, bytes(self._raw), self.settings.bcc_mode)
        return LinkEvent(EventKind.TELEGRAM, text, byte == expected)


class Lsv2Link:
    """One end of an LSV2 link on top of a :class:`Transport`."""

    def __init__(
        self,
        transport: Transport,
        settings: ProtocolSettings,
        clock: Clock,
        token: CancellationToken,
        on_traffic: Callable[[TrafficEvent], None] | None = None,
        read_size: int = 64,
    ) -> None:
        self.transport = transport
        self.settings = settings
        self.clock = clock
        self.token = token
        self.on_traffic = on_traffic
        self.read_size = read_size
        self.decoder = FrameDecoder(settings, max_text=max(settings.max_telegram_length * 2, 1024))
        self._events: deque[LinkEvent] = deque()
        self._next_ack = ACK1
        self._last_ack = ACK0
        self.established = False
        self.telegrams_sent = 0
        self.repetitions = 0

    # -- I/O -------------------------------------------------------------
    def write(self, data: bytes) -> None:
        pos = 0
        stall_since: float | None = None
        timeout = self.settings.ack_timeout_ms / 1000.0
        while pos < len(data):
            self.token.raise_if_cancelled()
            written = self.transport.write(data[pos:])
            if written > 0:
                if self.on_traffic:
                    self.on_traffic(TrafficEvent(Direction.TX, self.clock.now(), data[pos : pos + written]))
                pos += written
                stall_since = None
                continue
            now = self.clock.monotonic()
            if stall_since is None:
                stall_since = now
            elif now - stall_since >= timeout:
                raise TransportTimeoutError("Schreibvorgang ohne Fortschritt", self.transport.name)
            if self.clock.sleep(0.01, self.token.event):
                raise OperationCancelled("Abbruch")

    def wait_event(self, timeout_s: float | None) -> LinkEvent | None:
        """Next decoded event, or ``None`` after ``timeout_s`` (``None`` = wait forever)."""
        deadline = None if timeout_s is None else self.clock.monotonic() + timeout_s
        while True:
            if self._events:
                return self._events.popleft()
            self.token.raise_if_cancelled()
            if deadline is not None and self.clock.monotonic() >= deadline:
                return None
            data = self.transport.read(self.read_size)
            self.token.raise_if_cancelled()
            if data:
                if self.on_traffic:
                    self.on_traffic(TrafficEvent(Direction.RX, self.clock.now(), data))
                self._events.extend(self.decoder.feed(data))

    # -- sender role -----------------------------------------------------
    def establish(self) -> None:
        """Send ENQ until the CNC answers ACK0 (or ``link_timeout_ms`` expires)."""
        s = self.settings
        deadline = self.clock.monotonic() + s.link_timeout_ms / 1000.0
        while True:
            self.write(bytes((ENQ,)))
            interval_end = self.clock.monotonic() + s.enq_interval_ms / 1000.0
            while True:
                remaining = interval_end - self.clock.monotonic()
                event = self.wait_event(max(remaining, 0.0))
                if event is None:
                    break
                if event.kind is EventKind.ACK0:
                    self.established = True
                    self._next_ack = ACK1
                    log.info("LSV2 link established on %s", self.transport.name)
                    return
                if event.kind is EventKind.EOT:
                    raise CncAbortError("Die CNC hat den Verbindungsaufbau mit EOT abgelehnt.")
                if event.kind is EventKind.NAK:
                    break  # not ready yet - wait for next ENQ interval
            if self.clock.monotonic() >= deadline:
                raise Lsv2LinkTimeout(
                    f"Keine Empfangsbereitschaft (DLE 0) der CNC innerhalb {s.link_timeout_ms} ms",
                    self.transport.name,
                )
            remaining_interval = interval_end - self.clock.monotonic()
            if remaining_interval > 0 and self.clock.sleep(remaining_interval, self.token.event):
                raise OperationCancelled("Abbruch beim Verbindungsaufbau")

    def send_telegram(self, text: bytes, ack_timeout_s: float | None) -> None:
        """Send one telegram and wait for the matching acknowledgement."""
        s = self.settings
        frame = encode_telegram(text, s)
        expected = self._next_ack
        previous = ACK0 if expected == ACK1 else ACK1
        expected_kind = EventKind.ACK1 if expected == ACK1 else EventKind.ACK0
        previous_kind = EventKind.ACK0 if previous == ACK0 else EventKind.ACK1
        errors = 0
        self.write(frame)
        while True:
            event = self.wait_event(ack_timeout_s)
            if event is None:
                errors += 1
                if errors > s.max_retries:
                    raise Lsv2AckTimeout(
                        f"Keine Quittung der CNC für Telegramm {self.telegrams_sent + 1}", self.transport.name
                    )
                log.warning("LSV2: no acknowledgement, polling with ENQ (%d)", errors)
                self.write(bytes((ENQ,)))
                continue
            kind = event.kind
            if kind is expected_kind:
                self._next_ack = previous
                self.telegrams_sent += 1
                return
            if kind is EventKind.WACK:
                if self.clock.sleep(s.wack_poll_ms / 1000.0, self.token.event):
                    raise OperationCancelled("Abbruch")
                self.write(bytes((ENQ,)))
                continue
            if kind in (EventKind.NAK, previous_kind):
                errors += 1
                self.repetitions += 1
                if errors > s.max_retries:
                    raise Lsv2RetryExceeded(
                        f"Telegramm {self.telegrams_sent + 1} nach {s.max_retries} Wiederholungen nicht quittiert"
                    )
                log.warning("LSV2: telegram %d repeated (%s)", self.telegrams_sent + 1, kind.value)
                self.write(frame)
                continue
            if kind is EventKind.EOT:
                raise CncAbortError("Die CNC hat die Übertragung mit EOT beendet.")
            if kind is EventKind.RVI:
                raise CncAbortError("Die CNC hat eine Unterbrechung (RVI) angefordert.")
            log.debug("LSV2: ignoring unexpected %s while waiting for acknowledgement", kind.value)

    def finish(self) -> None:
        self.write(bytes((EOT,)))
        self.established = False

    # -- receiver role ---------------------------------------------------
    def accept(self, timeout_s: float | None) -> bool:
        """Wait for ENQ from the CNC and confirm readiness with ACK0."""
        while True:
            event = self.wait_event(timeout_s)
            if event is None:
                return False
            if event.kind is EventKind.ENQ:
                self.write(ACK0)
                self._last_ack = ACK0
                self._next_ack = ACK1
                self.established = True
                return True

    def receive_event(self, timeout_s: float | None) -> LinkEvent | None:
        """Receiver loop step: acknowledges telegrams and answers ENQ polls.

        Returns accepted telegrams, ``EOT`` or ``None`` on timeout.
        """
        while True:
            event = self.wait_event(timeout_s)
            if event is None:
                return None
            if event.kind is EventKind.TELEGRAM:
                if not event.bcc_ok:
                    self.repetitions += 1
                    self.write(bytes((NAK,)))
                    continue
                self.write(self._next_ack)
                self._last_ack = self._next_ack
                self._next_ack = ACK0 if self._next_ack == ACK1 else ACK1
                return event
            if event.kind is EventKind.ENQ:
                self.write(self._last_ack)
                continue
            if event.kind is EventKind.EOT:
                self.established = False
                return event
