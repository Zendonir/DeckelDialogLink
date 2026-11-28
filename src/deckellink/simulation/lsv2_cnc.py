"""Simulated CNC speaking LSV2 (DIN 66019 basic mode).

The simulator reacts to the bytes written by DeckelLink and produces timed
answers. It models

* the CNC as receiver (memory transfer and drip-feed with a limited block
  buffer that is consumed at a fixed execution rate), and
* the CNC as sender (program output to the PC).

Faults (NAK, lost acknowledgement, WACK, EOT abort, disconnect, BCC errors,
CNC never ready) are reproducible through :class:`Lsv2SimConfig`.
"""

from __future__ import annotations

import heapq
import itertools
import threading
from dataclasses import dataclass

from deckellink.clock import Clock, SystemClock
from deckellink.domain.models import ProtocolSettings, SerialSettings
from deckellink.nc.checks import split_eot_trailer
from deckellink.transfer.lsv2 import (
    ACK0,
    ACK1,
    ENQ,
    EOT,
    NAK,
    WACK,
    EventKind,
    FrameDecoder,
    encode_telegram,
)
from deckellink.transport.base import (
    ConnectionLostError,
    ModemStatus,
    PortBusyError,
    PortNotFoundError,
    PortOpenError,
)
from deckellink.transport.simulated_transport import SIMULATED_PORT_NAME


@dataclass(frozen=True)
class Lsv2SimConfig:
    speed_factor: float = 1.0
    open_error: str | None = None
    ready_after_s: float = 0.5
    never_ready: bool = False
    response_delay_s: float = 0.02
    buffer_telegrams: int = 0  # 0 = memory transfer: acknowledge immediately
    execution_s_per_telegram: float = 0.0  # drip-feed: time the CNC needs per block
    nak_on_telegrams: tuple[int, ...] = ()
    drop_ack_on_telegrams: tuple[int, ...] = ()
    wack_on_telegrams: tuple[int, ...] = ()
    eot_after_telegrams: int | None = None
    disconnect_after_telegrams: int | None = None
    program: bytes = b""  # data the CNC sends when the PC receives
    send_lines_per_telegram: int = 1
    corrupt_bcc_on_sent_telegrams: tuple[int, ...] = ()
    cts: bool = True
    dsr: bool = True


class Lsv2SimulatedCnc:
    """Transport implementation acting as an LSV2 speaking CNC."""

    def __init__(
        self,
        settings: SerialSettings,
        protocol: ProtocolSettings,
        config: Lsv2SimConfig | None = None,
        clock: Clock | None = None,
    ) -> None:
        self._settings = settings
        self._protocol = protocol
        self.config = config or Lsv2SimConfig()
        self._clock = clock or SystemClock()
        self._rate = max(1.0, settings.bytes_per_second * max(self.config.speed_factor, 0.001))
        self._cancel = threading.Event()
        self._open = False
        self._outgoing: list[tuple[float, int, bytes]] = []
        self._seq = itertools.count()
        self._buffer = bytearray()
        self._decoder = FrameDecoder(protocol)
        self._opened_at = 0.0
        self._linked = False
        self._last_ack = ACK0
        self._next_ack = ACK1
        self._nak_done: set[int] = set()
        self._drop_done: set[int] = set()
        self._wack_done: set[int] = set()
        self._exec_start: float | None = None
        self._last_ack_time = 0.0
        self._disconnected = False
        self.telegrams_received = 0
        self.received_texts: list[bytes] = []
        self.eot_received = False
        self.sent_eot = False
        self.naks_sent = 0
        # sender role
        self._send_telegrams = self._split_program()
        self._send_index = 0
        self._send_linked = False
        self._next_enq_at = 0.0
        self._send_expected = ACK1
        self._corrupted: set[int] = set()

    # -- Transport properties ------------------------------------------------
    @property
    def name(self) -> str:
        return SIMULATED_PORT_NAME

    @property
    def is_simulated(self) -> bool:
        return True

    @property
    def is_open(self) -> bool:
        return self._open

    @property
    def bytes_waiting(self) -> int:
        now = self._clock.monotonic()
        return sum(len(d) for t, _, d in self._outgoing if t <= now) + len(self._buffer)

    @property
    def out_waiting(self) -> int:
        return 0

    @property
    def received_program(self) -> bytes:
        return b"".join(self.received_texts)

    @property
    def machine_received(self) -> bytes:
        return self.received_program

    @property
    def is_sender(self) -> bool:
        return bool(self._send_telegrams)

    # -- lifecycle -----------------------------------------------------------
    def open(self) -> None:
        error = self.config.open_error
        if error == "busy":
            raise PortBusyError("Simulierter Fehler: Port belegt", self.name)
        if error == "not_found":
            raise PortNotFoundError("Simulierter Fehler: Port existiert nicht", self.name)
        if error:
            raise PortOpenError("Simulierter Fehler beim Öffnen", self.name)
        self._open = True
        self._cancel.clear()
        self._opened_at = self._clock.monotonic()
        self._next_enq_at = self._opened_at + self.config.ready_after_s

    def close(self) -> None:
        self._open = False

    def cancel(self) -> None:
        self._cancel.set()

    def get_modem_status(self) -> ModemStatus:
        if not self._open:
            return ModemStatus()
        return ModemStatus(cts=self.config.cts, dsr=self.config.dsr, dcd=False, ri=False, rts=True, dtr=False)

    def set_control_lines(self, rts: bool | None = None, dtr: bool | None = None) -> None:
        self._require_open()

    def reset_input_buffer(self) -> None:
        self._buffer.clear()

    def _require_open(self) -> None:
        if not self._open or self._disconnected:
            raise ConnectionLostError("Simulierte LSV2-Verbindung ist getrennt.", self.name)

    # -- output scheduling ---------------------------------------------------
    def _queue(self, data: bytes, delay: float | None = None) -> None:
        when = self._clock.monotonic() + (self.config.response_delay_s if delay is None else delay)
        heapq.heappush(self._outgoing, (when, next(self._seq), data))

    def _queue_at(self, when: float, data: bytes) -> None:
        heapq.heappush(self._outgoing, (when, next(self._seq), data))

    def _tick(self) -> None:
        """Time driven behaviour (CNC sending ENQ when it wants to transmit)."""
        if not self.is_sender or self._send_linked or self.sent_eot:
            return
        now = self._clock.monotonic()
        if now >= self._next_enq_at and not self.config.never_ready:
            self._queue(bytes((ENQ,)), 0.0)
            self._next_enq_at = now + self._protocol.enq_interval_ms / 1000.0

    # -- writes from the PC -----------------------------------------------------
    def write(self, data: bytes) -> int:
        self._require_open()
        if not data:
            return 0
        if self._clock.sleep(len(data) / self._rate, self._cancel):
            return 0
        mask = (1 << self._settings.data_bits) - 1
        for event in self._decoder.feed(bytes(b & mask for b in data)):
            if self.is_sender:
                self._handle_as_sender(event.kind)
            else:
                self._handle_as_receiver(event.kind, event.text, event.bcc_ok)
        return len(data)

    def _handle_as_receiver(self, kind: EventKind, text: bytes, bcc_ok: bool) -> None:
        cfg = self.config
        now = self._clock.monotonic()
        if kind is EventKind.ENQ:
            if not self._linked:
                if cfg.never_ready or now - self._opened_at < cfg.ready_after_s:
                    return
                self._linked = True
                self._last_ack, self._next_ack = ACK0, ACK1
                self._queue(ACK0)
            else:
                self._queue_at(max(now + cfg.response_delay_s, self._last_ack_time), self._last_ack)
            return
        if kind is EventKind.EOT:
            self.eot_received = True
            self._linked = False
            return
        if kind is not EventKind.TELEGRAM or not self._linked:
            return
        number = self.telegrams_received + 1
        if not bcc_ok or (number in cfg.nak_on_telegrams and number not in self._nak_done):
            self._nak_done.add(number)
            self.naks_sent += 1
            self._queue(bytes((NAK,)))
            return
        self.telegrams_received = number
        self.received_texts.append(text)
        ack = self._next_ack
        self._last_ack = ack
        self._next_ack = ACK0 if ack == ACK1 else ACK1
        ack_time = now + cfg.response_delay_s
        if cfg.buffer_telegrams > 0 and cfg.execution_s_per_telegram > 0:
            if self._exec_start is None:
                self._exec_start = now
            free_at = self._exec_start + (number - cfg.buffer_telegrams) * cfg.execution_s_per_telegram
            ack_time = max(ack_time, free_at)
        self._last_ack_time = ack_time
        if cfg.disconnect_after_telegrams is not None and number >= cfg.disconnect_after_telegrams:
            self._disconnected = True
            return
        if number in cfg.drop_ack_on_telegrams and number not in self._drop_done:
            self._drop_done.add(number)
            return
        if number in cfg.wack_on_telegrams and number not in self._wack_done:
            self._wack_done.add(number)
            self._queue_at(ack_time, WACK)
            return
        self._queue_at(ack_time, ack)
        if cfg.eot_after_telegrams is not None and number >= cfg.eot_after_telegrams:
            self._queue_at(ack_time + 0.01, bytes((EOT,)))
            self.sent_eot = True
            self._linked = False

    # -- CNC as sender -----------------------------------------------------------
    def _split_program(self) -> list[bytes]:
        data, _trailer = split_eot_trailer(self.config.program)
        if not data:
            return []
        lines = data.splitlines(keepends=True)
        step = max(1, self.config.send_lines_per_telegram)
        texts = [b"".join(lines[i : i + step]) for i in range(0, len(lines), step)]
        if not self._protocol.include_line_ending:
            texts = [t.rstrip(b"\r\n") for t in texts]
        return texts

    def _send_current(self) -> None:
        index = self._send_index
        frame = encode_telegram(self._send_telegrams[index], self._protocol)
        number = index + 1
        if number in self.config.corrupt_bcc_on_sent_telegrams and number not in self._corrupted:
            self._corrupted.add(number)
            frame = frame[:-1] + bytes((frame[-1] ^ 0x01,))
        self._queue(frame)

    def _handle_as_sender(self, kind: EventKind) -> None:
        if self.sent_eot:
            return
        if not self._send_linked:
            if kind is EventKind.ACK0:
                self._send_linked = True
                self._send_expected = ACK1
                self._send_current()
            return
        expected_kind = EventKind.ACK1 if self._send_expected == ACK1 else EventKind.ACK0
        if kind is expected_kind:
            self._send_index += 1
            self._send_expected = ACK0 if self._send_expected == ACK1 else ACK1
            if self._send_index >= len(self._send_telegrams):
                self._queue(bytes((EOT,)))
                self.sent_eot = True
            else:
                self._send_current()
        elif kind is EventKind.NAK:
            self._send_current()

    # -- reads ---------------------------------------------------------------------
    def read(self, size: int) -> bytes:
        self._require_open()
        deadline = self._clock.monotonic() + self._settings.read_timeout_ms / 1000.0
        while True:
            if self._cancel.is_set():
                return b""
            self._tick()
            now = self._clock.monotonic()
            while self._outgoing and self._outgoing[0][0] <= now:
                self._buffer += heapq.heappop(self._outgoing)[2]
            if self._buffer:
                chunk = bytes(self._buffer[:size])
                del self._buffer[:size]
                return chunk
            if now >= deadline:
                return b""
            wake = deadline
            if self._outgoing:
                wake = min(wake, self._outgoing[0][0])
            if self.is_sender and not self._send_linked and not self.config.never_ready:
                wake = min(wake, max(self._next_enq_at, now))
            if self._clock.sleep(max(wake - now, 1e-4), self._cancel):
                return b""
