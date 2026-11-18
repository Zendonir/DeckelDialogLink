"""In-memory loopback transport (TX bridged to RX) for tests and the simulator."""

from __future__ import annotations

import threading
from collections import deque

from deckellink.clock import Clock, SystemClock
from deckellink.domain.models import SerialSettings
from deckellink.transport.base import ConnectionLostError, ModemStatus

LOOPBACK_PORT_NAME = "LOOPBACK (simuliert)"


class LoopbackTransport:
    """Everything written can be read back.

    ``bridged=False`` simulates a missing TX/RX bridge, ``corrupt_every`` flips a
    bit in every n-th byte and ``drop_every`` loses every n-th byte.
    """

    def __init__(
        self,
        settings: SerialSettings,
        clock: Clock | None = None,
        *,
        bridged: bool = True,
        corrupt_every: int = 0,
        drop_every: int = 0,
    ) -> None:
        self._settings = settings
        self._clock = clock or SystemClock()
        self._bridged = bridged
        self._corrupt_every = corrupt_every
        self._drop_every = drop_every
        self._buffer: deque[int] = deque()
        self._open = False
        self._cancel = threading.Event()
        self._counter = 0
        self._rts = settings.initial_rts
        self._dtr = settings.initial_dtr

    @property
    def name(self) -> str:
        return LOOPBACK_PORT_NAME

    @property
    def is_simulated(self) -> bool:
        return True

    @property
    def is_open(self) -> bool:
        return self._open

    @property
    def bytes_waiting(self) -> int:
        return len(self._buffer)

    @property
    def out_waiting(self) -> int:
        return 0

    def open(self) -> None:
        self._open = True
        self._cancel.clear()

    def close(self) -> None:
        self._open = False
        self._buffer.clear()

    def cancel(self) -> None:
        self._cancel.set()

    def _require_open(self) -> None:
        if not self._open:
            raise ConnectionLostError("Loopback ist nicht geöffnet.", self.name)

    def write(self, data: bytes) -> int:
        self._require_open()
        mask = (1 << self._settings.data_bits) - 1
        rate = max(1.0, self._settings.bytes_per_second)
        if self._clock.sleep(len(data) / rate, self._cancel):
            return 0
        if self._bridged:
            for byte in data:
                self._counter += 1
                if self._drop_every and self._counter % self._drop_every == 0:
                    continue
                value = byte & mask
                if self._corrupt_every and self._counter % self._corrupt_every == 0:
                    value ^= 0x01
                self._buffer.append(value)
        return len(data)

    def read(self, size: int) -> bytes:
        self._require_open()
        if not self._buffer:
            self._clock.sleep(self._settings.read_timeout_ms / 1000.0, self._cancel)
        count = min(size, len(self._buffer))
        return bytes(self._buffer.popleft() for _ in range(count))

    def get_modem_status(self) -> ModemStatus:
        if not self._open:
            return ModemStatus()
        # A typical loopback plug also bridges RTS->CTS and DTR->DSR.
        return ModemStatus(cts=self._rts, dsr=self._dtr, dcd=self._dtr, ri=False, rts=self._rts, dtr=self._dtr)

    def set_control_lines(self, rts: bool | None = None, dtr: bool | None = None) -> None:
        self._require_open()
        if rts is not None:
            self._rts = rts
        if dtr is not None:
            self._dtr = dtr

    def reset_input_buffer(self) -> None:
        self._buffer.clear()
