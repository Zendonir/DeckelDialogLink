"""Simulated CNC endpoint.

Replaces only the transport layer; the same sender/receiver logic as for the
real serial port is used. Timing is derived from the profile's baud rate and
the injectable :class:`~deckellink.clock.Clock`, so tests are deterministic.
"""

from __future__ import annotations

import math
import threading
from dataclasses import dataclass

from deckellink.clock import Clock, SystemClock
from deckellink.domain.models import SerialSettings
from deckellink.transport.base import (
    ConnectionLostError,
    ModemStatus,
    PortBusyError,
    PortNotFoundError,
    PortOpenError,
    TransportTimeoutError,
    TransportWriteError,
)

SIMULATED_PORT_NAME = "SIMULATOR"


@dataclass(frozen=True)
class SimulationConfig:
    """Behaviour of the simulated machine/connection."""

    speed_factor: float = 1.0
    open_error: str | None = None  # "busy" | "not_found" | "generic"
    cts: bool = True
    dsr: bool = True
    dcd: bool = False
    ri: bool = False
    cts_on_after_s: float | None = None
    max_write_chunk: int = 0
    write_stall_after_bytes: int | None = None
    write_error_after_bytes: int | None = None
    disconnect_after_bytes: int | None = None
    machine_program: bytes = b""
    machine_send_delay_s: float = 0.5
    read_chunk_sizes: tuple[int, ...] = ()
    corrupt_offsets: tuple[int, ...] = ()
    inject_null_every: int = 0


def build_machine_stream(config: SimulationConfig) -> bytes:
    """Apply null injection and corruption to the machine program."""
    data = bytearray(config.machine_program)
    if config.inject_null_every > 0:
        out = bytearray()
        for index, byte in enumerate(data):
            out.append(byte)
            if (index + 1) % config.inject_null_every == 0:
                out.append(0)
        data = out
    for offset in config.corrupt_offsets:
        if 0 <= offset < len(data):
            data[offset] = data[offset] ^ 0x40 if data[offset] not in (0x0D, 0x0A) else 0x7E
    return bytes(data)


class SimulatedTransport:
    """In-process CNC simulator implementing the Transport protocol."""

    def __init__(
        self,
        settings: SerialSettings,
        config: SimulationConfig | None = None,
        clock: Clock | None = None,
    ) -> None:
        self._settings = settings
        self.config = config or SimulationConfig()
        self._clock = clock or SystemClock()
        self._open = False
        self._cancel = threading.Event()
        self._rate = max(1.0, settings.bytes_per_second * max(self.config.speed_factor, 0.001))
        self._rx_stream = build_machine_stream(self.config)
        self._rx_pos = 0
        self._rx_start = 0.0
        self._opened_at = 0.0
        self._chunk_index = 0
        self._tx = bytearray()
        self._rts = settings.initial_rts
        self._dtr = settings.initial_dtr
        self._lock = threading.Lock()
        self.open_count = 0
        self.close_count = 0

    # -- properties --------------------------------------------------------
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
    def machine_received(self) -> bytes:
        """Bytes the simulated CNC has received."""
        with self._lock:
            return bytes(self._tx)

    @property
    def bytes_waiting(self) -> int:
        if not self._open:
            return 0
        return self._available()

    @property
    def out_waiting(self) -> int:
        return 0

    # -- lifecycle ---------------------------------------------------------
    def open(self) -> None:
        if self._open:
            return
        error = self.config.open_error
        if error == "busy":
            raise PortBusyError("Simulierter Fehler: Port belegt (Zugriff verweigert)", self.name)
        if error == "not_found":
            raise PortNotFoundError("Simulierter Fehler: Port existiert nicht", self.name)
        if error:
            raise PortOpenError("Simulierter Fehler beim Öffnen des Ports", self.name)
        self._cancel.clear()
        self._open = True
        self.open_count += 1
        self._opened_at = self._clock.monotonic()
        self._rx_start = self._opened_at + self.config.machine_send_delay_s
        self._rx_pos = 0
        self._chunk_index = 0

    def close(self) -> None:
        if self._open:
            self.close_count += 1
        self._open = False

    def cancel(self) -> None:
        self._cancel.set()

    # -- signals -----------------------------------------------------------
    def _cts_now(self) -> bool:
        if self.config.cts_on_after_s is not None:
            return self._clock.monotonic() - self._opened_at >= self.config.cts_on_after_s
        return self.config.cts

    def get_modem_status(self) -> ModemStatus:
        if not self._open:
            return ModemStatus()
        return ModemStatus(
            cts=self._cts_now(),
            dsr=self.config.dsr,
            dcd=self.config.dcd,
            ri=self.config.ri,
            rts=self._rts,
            dtr=self._dtr,
        )

    def set_control_lines(self, rts: bool | None = None, dtr: bool | None = None) -> None:
        self._require_open()
        if rts is not None:
            self._rts = rts
        if dtr is not None:
            self._dtr = dtr

    def reset_input_buffer(self) -> None:
        self._rx_pos = max(self._rx_pos, self._rx_pos + self._available())

    def _require_open(self) -> None:
        if not self._open:
            raise ConnectionLostError("Simulator ist nicht geöffnet.", self.name)

    # -- data --------------------------------------------------------------
    def write(self, data: bytes) -> int:
        self._require_open()
        if not data:
            return 0
        cfg = self.config
        write_timeout = self._settings.write_timeout_ms / 1000.0
        sent = len(self._tx)
        if self._settings.rts_cts and not self._cts_now():
            self._clock.sleep(write_timeout, self._cancel)
            if self._cancel.is_set():
                return 0
            if not self._cts_now():
                raise TransportTimeoutError("Simulierter Schreib-Timeout: CTS nicht gesetzt", self.name)
        count = len(data)
        if cfg.max_write_chunk > 0:
            count = min(count, cfg.max_write_chunk)
        for limit, kind in (
            (cfg.disconnect_after_bytes, "disconnect"),
            (cfg.write_error_after_bytes, "error"),
            (cfg.write_stall_after_bytes, "stall"),
        ):
            if limit is None:
                continue
            if sent >= limit:
                if kind == "disconnect":
                    self._open = False
                    raise ConnectionLostError("Simulierter Verbindungsabbruch", self.name)
                if kind == "error":
                    raise TransportWriteError("Simulierter Schreibfehler", self.name)
                self._clock.sleep(write_timeout, self._cancel)
                if self._cancel.is_set():
                    return 0
                raise TransportTimeoutError("Simulierter Schreib-Timeout", self.name)
            count = min(count, limit - sent)
        if self._clock.sleep(count / self._rate, self._cancel):
            return 0
        mask = (1 << self._settings.data_bits) - 1
        with self._lock:
            self._tx.extend(b & mask for b in data[:count])
        return count

    def _available(self) -> int:
        now = self._clock.monotonic()
        if now < self._rx_start:
            return 0
        produced = min(len(self._rx_stream), math.floor((now - self._rx_start) * self._rate + 1e-9))
        return max(0, produced - self._rx_pos)

    def read(self, size: int) -> bytes:
        self._require_open()
        cfg = self.config
        deadline = self._clock.monotonic() + self._settings.read_timeout_ms / 1000.0
        while True:
            if self._cancel.is_set():
                return b""
            if cfg.disconnect_after_bytes is not None and self._rx_pos >= cfg.disconnect_after_bytes:
                self._open = False
                raise ConnectionLostError("Simulierter Verbindungsabbruch", self.name)
            available = self._available()
            if available > 0:
                count = min(size, available)
                if cfg.read_chunk_sizes:
                    count = min(count, cfg.read_chunk_sizes[self._chunk_index % len(cfg.read_chunk_sizes)])
                    self._chunk_index += 1
                if cfg.disconnect_after_bytes is not None:
                    count = min(count, max(1, cfg.disconnect_after_bytes - self._rx_pos))
                chunk = self._rx_stream[self._rx_pos : self._rx_pos + count]
                self._rx_pos += len(chunk)
                return chunk
            now = self._clock.monotonic()
            remaining = deadline - now
            if remaining <= 0:
                return b""
            if self._rx_pos >= len(self._rx_stream):
                wait = remaining
            else:
                next_at = self._rx_start + (self._rx_pos + 1) / self._rate
                wait = min(remaining, max(next_at - now, 1e-4))
            if self._clock.sleep(wait, self._cancel):
                return b""
