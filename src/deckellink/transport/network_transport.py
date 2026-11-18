"""RS-232/Ethernet adapters (serial device servers) via pyserial URL handlers.

Supported connection types:

* ``tcp``     - raw TCP socket (adapter in "TCP server" / "real COM off" mode, e.g. Moxa
                NPort "TCP Server", USR-TCP232 "TCP Server"). Serial parameters (baud
                rate, parity, handshake) must be configured **in the adapter**; modem
                lines are not visible to DeckelLink.
* ``rfc2217`` - Telnet COM Port Control (RFC 2217). Baud rate, data format and
                control lines are negotiated by DeckelLink; modem lines are reported.

Alternatively, vendor drivers that create a virtual COM port can be used with the
normal "COM-Port" connection type.
"""

from __future__ import annotations

import re
from enum import StrEnum
from typing import Any

import serial

from deckellink.domain.models import SerialSettings
from deckellink.transport.base import ModemStatus, TransportConfigurationError
from deckellink.transport.serial_transport import SerialTransport

_HOST_RE = re.compile(r"^[A-Za-z0-9.\-:\[\]]{1,253}$")


class ConnectionKind(StrEnum):
    SERIAL = "serial"
    TCP = "tcp"
    RFC2217 = "rfc2217"

    @property
    def label(self) -> str:
        return {
            "serial": "COM-Port (lokal, auch virtueller COM-Port eines Netzwerkadapters)",
            "tcp": "Netzwerkadapter - TCP-Rohdaten (TCP-Server-Modus)",
            "rfc2217": "Netzwerkadapter - RFC 2217 (Telnet COM Port Control)",
        }[self.value]

    @property
    def is_network(self) -> bool:
        return self is not ConnectionKind.SERIAL


def validate_endpoint(host: str, port: int) -> str | None:
    """Return a German error text, or ``None`` if host/port are usable."""
    if not host.strip():
        return "Keine IP-Adresse bzw. kein Hostname des Netzwerkadapters angegeben"
    if not _HOST_RE.match(host.strip()):
        return f"Ungültige Adresse: {host!r}"
    if not 1 <= port <= 65535:
        return f"Ungültiger TCP-Port: {port}"
    return None


def build_url(kind: ConnectionKind, host: str, port: int) -> str:
    host = host.strip()
    if ":" in host and not host.startswith("["):
        host = f"[{host}]"  # IPv6 literal
    scheme = "socket" if kind is ConnectionKind.TCP else "rfc2217"
    return f"{scheme}://{host}:{port}"


def endpoint_label(kind: ConnectionKind, host: str, port: int) -> str:
    prefix = "TCP" if kind is ConnectionKind.TCP else "RFC2217"
    return f"{prefix} {host.strip()}:{port}"


class NetworkSerialTransport(SerialTransport):
    """Serial transport over an Ethernet device server."""

    def __init__(self, kind: ConnectionKind, host: str, port: int, settings: SerialSettings) -> None:
        if not kind.is_network:
            raise ValueError("NetworkSerialTransport requires a network connection kind")
        self.kind = kind
        self.host = host.strip()
        self.tcp_port = port
        self.url = build_url(kind, host, port)

        def factory() -> Any:
            return serial.serial_for_url(self.url, do_not_open=True)

        super().__init__(self.url, settings, serial_factory=factory)
        self._label = endpoint_label(kind, host, port)

    @property
    def name(self) -> str:
        return self._label

    def open(self) -> None:
        problem = validate_endpoint(self.host, self.tcp_port)
        if problem:
            raise TransportConfigurationError(problem, self._label)
        s = self.settings
        if self.kind is ConnectionKind.TCP and (s.require_cts_before_send or s.require_dsr_before_send):
            raise TransportConfigurationError(
                "Bei einer TCP-Rohverbindung sind CTS/DSR für DeckelLink nicht sichtbar. Im Profil "
                "'CTS/DSR vor dem Senden erforderlich' abschalten und den Handshake im Adapter "
                "konfigurieren - oder RFC 2217 verwenden.",
                self._label,
            )
        super().open()

    def get_modem_status(self) -> ModemStatus:
        if self.kind is ConnectionKind.TCP:
            # pyserial's socket handler reports fixed dummy values - do not show them as real lines.
            return ModemStatus()
        return super().get_modem_status()


def create_hardware_transport(
    kind: ConnectionKind, com_port: str, host: str, tcp_port: int, settings: SerialSettings
) -> SerialTransport:
    """Transport for real hardware: local/virtual COM port or Ethernet adapter."""
    if kind.is_network:
        return NetworkSerialTransport(kind, host, tcp_port, settings)
    return SerialTransport(com_port, settings)


def connection_target(kind: ConnectionKind, com_port: str, host: str, tcp_port: int) -> str:
    """Non-empty string when the real connection is configured (used by the gates)."""
    if kind.is_network:
        return "" if validate_endpoint(host, tcp_port) else endpoint_label(kind, host, tcp_port)
    return com_port
