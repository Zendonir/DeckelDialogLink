"""Transport abstraction. The GUI never talks to pyserial directly."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol, runtime_checkable

from deckellink.domain.errors import DeckelLinkError


@dataclass(frozen=True)
class ModemStatus:
    """RS-232 line states. ``None`` means the adapter/driver cannot report it."""

    cts: bool | None = None
    dsr: bool | None = None
    dcd: bool | None = None
    ri: bool | None = None
    rts: bool | None = None
    dtr: bool | None = None

    def as_dict(self) -> dict[str, bool | None]:
        return {
            "CTS": self.cts,
            "DSR": self.dsr,
            "DCD": self.dcd,
            "RI": self.ri,
            "RTS": self.rts,
            "DTR": self.dtr,
        }


class TransportError(DeckelLinkError):
    """Base class for transport failures."""

    def __init__(self, message: str, port: str = "") -> None:
        super().__init__(message)
        self.port = port


class PortOpenError(TransportError):
    """The port could not be opened."""


class PortBusyError(PortOpenError):
    """The port is in use or access was denied."""


class PortNotFoundError(PortOpenError):
    """The port does not exist (adapter unplugged, renamed COM number)."""


class NetworkUnreachableError(PortOpenError):
    """The RS-232/Ethernet adapter could not be reached over the network."""


class TransportConfigurationError(TransportError):
    """Profile settings cannot be fulfilled by the chosen connection type."""


class TransportTimeoutError(TransportError):
    """No progress within the configured timeout."""


class HandshakeTimeoutError(TransportTimeoutError):
    def __init__(self, message: str, port: str = "", missing: tuple[str, ...] = ()) -> None:
        super().__init__(message, port)
        self.missing = missing


class ConnectionLostError(TransportError):
    """The connection was interrupted during an operation."""


class TransportWriteError(TransportError):
    """The driver reported a write error."""


class TransportReadError(TransportError):
    """The driver reported a read error."""


@runtime_checkable
class Transport(Protocol):
    """Byte transport to the CNC (real serial port, simulator or loopback)."""

    @property
    def name(self) -> str: ...

    @property
    def is_open(self) -> bool: ...

    @property
    def is_simulated(self) -> bool: ...

    @property
    def bytes_waiting(self) -> int: ...

    @property
    def out_waiting(self) -> int: ...

    def open(self) -> None: ...

    def close(self) -> None: ...

    def write(self, data: bytes) -> int:
        """Write bytes. May write fewer bytes than given; returns count written."""
        ...

    def read(self, size: int) -> bytes:
        """Read up to ``size`` bytes; returns ``b''`` after the read timeout."""
        ...

    def cancel(self) -> None:
        """Abort blocking reads/writes (thread-safe)."""
        ...

    def get_modem_status(self) -> ModemStatus: ...

    def set_control_lines(self, rts: bool | None = None, dtr: bool | None = None) -> None: ...

    def reset_input_buffer(self) -> None: ...
