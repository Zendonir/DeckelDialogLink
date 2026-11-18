"""Real RS-232 transport based on pyserial."""

from __future__ import annotations

import errno
import logging
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

import serial
from serial.tools import list_ports

from deckellink.domain.models import Parity, SerialSettings
from deckellink.transport.base import (
    ConnectionLostError,
    ModemStatus,
    PortBusyError,
    PortNotFoundError,
    PortOpenError,
    TransportError,
    TransportReadError,
    TransportTimeoutError,
    TransportWriteError,
)

log = logging.getLogger(__name__)

PARITY_MAP = {
    Parity.NONE: serial.PARITY_NONE,
    Parity.EVEN: serial.PARITY_EVEN,
    Parity.ODD: serial.PARITY_ODD,
    Parity.MARK: serial.PARITY_MARK,
    Parity.SPACE: serial.PARITY_SPACE,
}
BYTESIZE_MAP = {5: serial.FIVEBITS, 6: serial.SIXBITS, 7: serial.SEVENBITS, 8: serial.EIGHTBITS}
STOPBITS_MAP = {
    1.0: serial.STOPBITS_ONE,
    1.5: serial.STOPBITS_ONE_POINT_FIVE,
    2.0: serial.STOPBITS_TWO,
}

_BUSY_MARKERS = ("permissionerror", "access is denied", "zugriff verweigert", "busy", "in use")
_MISSING_MARKERS = (
    "filenotfounderror",
    "no such file",
    "cannot find",
    "could not find",
    "das system kann die angegebene datei nicht finden",
    "the system cannot find",
)


@dataclass(frozen=True)
class PortInfo:
    device: str
    description: str
    hwid: str

    @property
    def label(self) -> str:
        if self.description and self.description not in ("n/a", self.device):
            return f"{self.device} - {self.description}"
        return self.device


def available_ports() -> list[PortInfo]:
    """List serial ports known to the OS. Never raises."""
    try:
        ports = list_ports.comports()
    except Exception as exc:  # pragma: no cover - platform specific
        log.warning("Listing serial ports failed: %s", exc)
        return []
    return sorted(
        (PortInfo(p.device, p.description or "", p.hwid or "") for p in ports),
        key=lambda p: p.device,
    )


def classify_open_error(exc: BaseException, port: str) -> PortOpenError:
    text = f"{type(exc).__name__} {exc} {exc!r}".lower()
    err_no = getattr(exc, "errno", None)
    if err_no in (errno.EACCES, errno.EBUSY) or any(m in text for m in _BUSY_MARKERS):
        return PortBusyError(f"{port} ist belegt oder der Zugriff wurde verweigert: {exc}", port)
    if err_no == errno.ENOENT or any(m in text for m in _MISSING_MARKERS):
        return PortNotFoundError(f"{port} wurde nicht gefunden: {exc}", port)
    return PortOpenError(f"{port} konnte nicht geöffnet werden: {exc}", port)


class SerialTransport:
    """pyserial implementation of :class:`deckellink.transport.base.Transport`."""

    def __init__(
        self,
        port: str,
        settings: SerialSettings,
        serial_factory: Callable[[], Any] = serial.Serial,
    ) -> None:
        self._port = port
        self._settings = settings
        self._factory = serial_factory
        self._serial: Any = None

    @property
    def name(self) -> str:
        return self._port

    @property
    def is_simulated(self) -> bool:
        return False

    @property
    def is_open(self) -> bool:
        return bool(self._serial is not None and self._serial.is_open)

    @property
    def settings(self) -> SerialSettings:
        return self._settings

    def open(self) -> None:
        if self.is_open:
            return
        if not self._port:
            raise PortOpenError("Kein COM-Port ausgewählt.", "")
        s = self._settings
        ser = self._factory()
        try:
            ser.port = self._port
            ser.baudrate = s.baud_rate
            ser.bytesize = BYTESIZE_MAP[s.data_bits]
            ser.parity = PARITY_MAP[s.parity]
            ser.stopbits = STOPBITS_MAP[float(s.stop_bits)]
            ser.timeout = s.read_timeout_ms / 1000.0
            ser.write_timeout = s.write_timeout_ms / 1000.0
            ser.inter_byte_timeout = s.inter_byte_timeout_ms / 1000.0 if s.inter_byte_timeout_ms > 0 else None
            ser.rtscts = s.rts_cts
            ser.xonxoff = s.xon_xoff
            ser.dsrdtr = s.dsr_dtr
            # Initial line states are applied by pyserial when the port opens.
            ser.rts = s.initial_rts
            ser.dtr = s.initial_dtr
        except (ValueError, KeyError) as exc:
            raise PortOpenError(f"Ungültige Schnittstellenparameter: {exc}", self._port) from exc
        try:
            ser.open()
        except (serial.SerialException, OSError) as exc:
            raise classify_open_error(exc, self._port) from exc
        self._serial = ser
        log.info("Port opened: %s (%s)", self._port, s.summary)

    def close(self) -> None:
        ser = self._serial
        self._serial = None
        if ser is None:
            return
        try:
            ser.close()
            log.info("Port closed: %s", self._port)
        except Exception as exc:  # closing must never raise into callers
            log.warning("Error while closing %s: %s", self._port, exc)

    def _require_open(self) -> Any:
        if self._serial is None or not self._serial.is_open:
            raise ConnectionLostError(f"{self._port} ist nicht geöffnet.", self._port)
        return self._serial

    def write(self, data: bytes) -> int:
        ser = self._require_open()
        try:
            written = ser.write(data)
        except serial.SerialTimeoutException as exc:
            raise TransportTimeoutError(f"Schreib-Timeout auf {self._port} (Handshake blockiert?)", self._port) from exc
        except serial.SerialException as exc:
            raise ConnectionLostError(f"Schreibfehler auf {self._port}: {exc}", self._port) from exc
        except OSError as exc:
            raise TransportWriteError(f"Schreibfehler auf {self._port}: {exc}", self._port) from exc
        return int(written or 0)

    def read(self, size: int) -> bytes:
        ser = self._require_open()
        try:
            return bytes(ser.read(size))
        except serial.SerialException as exc:
            raise ConnectionLostError(f"Lesefehler auf {self._port}: {exc}", self._port) from exc
        except OSError as exc:
            raise TransportReadError(f"Lesefehler auf {self._port}: {exc}", self._port) from exc

    def cancel(self) -> None:
        ser = self._serial
        if ser is None:
            return
        for method in ("cancel_read", "cancel_write"):
            func = getattr(ser, method, None)
            if callable(func):
                try:
                    func()
                except Exception as exc:  # best effort
                    log.debug("%s failed: %s", method, exc)

    @property
    def bytes_waiting(self) -> int:
        ser = self._serial
        if ser is None:
            return 0
        try:
            return int(ser.in_waiting)
        except (serial.SerialException, OSError):
            return 0

    @property
    def out_waiting(self) -> int:
        ser = self._serial
        if ser is None:
            return 0
        try:
            return int(ser.out_waiting)
        except (serial.SerialException, OSError, AttributeError, NotImplementedError):
            return 0

    def get_modem_status(self) -> ModemStatus:
        ser = self._serial
        if ser is None or not ser.is_open:
            return ModemStatus()

        def safe(attr: str) -> bool | None:
            try:
                return bool(getattr(ser, attr))
            except (serial.SerialException, OSError, AttributeError, NotImplementedError):
                return None

        return ModemStatus(
            cts=safe("cts"),
            dsr=safe("dsr"),
            dcd=safe("cd"),
            ri=safe("ri"),
            rts=safe("rts"),
            dtr=safe("dtr"),
        )

    def set_control_lines(self, rts: bool | None = None, dtr: bool | None = None) -> None:
        ser = self._require_open()
        try:
            if rts is not None:
                ser.rts = rts
            if dtr is not None:
                ser.dtr = dtr
        except (serial.SerialException, OSError) as exc:
            raise TransportError(f"Steuerleitungen konnten nicht gesetzt werden: {exc}", self._port) from exc

    def reset_input_buffer(self) -> None:
        ser = self._serial
        if ser is None:
            return
        try:
            ser.reset_input_buffer()
        except (serial.SerialException, OSError) as exc:
            log.debug("reset_input_buffer failed: %s", exc)
