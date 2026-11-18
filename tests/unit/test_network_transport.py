"""RS-232/Ethernet adapter support, tested against a local TCP server (no COM port)."""

from __future__ import annotations

import socket
import threading
import time
from collections.abc import Iterator
from dataclasses import replace

import pytest
import serial

from deckellink.clock import SystemClock
from deckellink.config import AppConfig, load_config, save_config
from deckellink.diagnostics.error_messages import describe_error
from deckellink.domain.models import Profile
from deckellink.domain.states import ReceiveState, SendState
from deckellink.nc.transformer import transform
from deckellink.simulation.scenarios import SAMPLE_PROGRAM
from deckellink.transfer.receiver import ProgramReceiver
from deckellink.transfer.sender import ProgramSender
from deckellink.transport.base import ModemStatus, NetworkUnreachableError, TransportConfigurationError
from deckellink.transport.network_transport import (
    ConnectionKind,
    NetworkSerialTransport,
    build_url,
    connection_target,
    create_hardware_transport,
    endpoint_label,
    validate_endpoint,
)
from deckellink.transport.serial_transport import SerialTransport, classify_open_error


class TcpDeviceServer:
    """Minimal stand-in for an Ethernet device server in TCP server mode."""

    def __init__(self, to_send: bytes = b"", send_delay: float = 0.1) -> None:
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(1)
        self.port = self.sock.getsockname()[1]
        self.received = bytearray()
        self.to_send = to_send
        self.send_delay = send_delay
        self.done = threading.Event()
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def _run(self) -> None:
        conn, _ = self.sock.accept()
        with conn:
            if self.to_send:
                time.sleep(self.send_delay)
                conn.sendall(self.to_send)
            # Like a real device server: keep the connection until the client closes it.
            conn.settimeout(0.3)
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                try:
                    data = conn.recv(4096)
                except TimeoutError:
                    continue
                except OSError:
                    break
                if not data:
                    break
                self.received += data
        self.done.set()

    def close(self) -> None:
        self.sock.close()


@pytest.fixture
def no_handshake(profile_b: Profile) -> Profile:
    return replace(profile_b, transfer=replace(profile_b.transfer, start_delay_ms=0, end_delay_ms=0))


@pytest.fixture
def server() -> Iterator[TcpDeviceServer]:
    srv = TcpDeviceServer()
    yield srv
    srv.close()


def test_urls_labels_and_validation() -> None:
    assert build_url(ConnectionKind.TCP, " 192.168.0.50 ", 4001) == "socket://192.168.0.50:4001"
    assert build_url(ConnectionKind.RFC2217, "moxa.local", 4001) == "rfc2217://moxa.local:4001"
    assert build_url(ConnectionKind.TCP, "fe80::1", 23) == "socket://[fe80::1]:23"
    assert endpoint_label(ConnectionKind.TCP, "10.0.0.2", 4001) == "TCP 10.0.0.2:4001"
    assert validate_endpoint("", 1) is not None
    assert validate_endpoint("bad host", 1) is not None
    assert validate_endpoint("10.0.0.2", 0) is not None
    assert validate_endpoint("10.0.0.2", 4001) is None
    assert connection_target(ConnectionKind.SERIAL, "COM3", "", 1) == "COM3"
    assert connection_target(ConnectionKind.TCP, "COM3", "", 4001) == ""
    assert connection_target(ConnectionKind.RFC2217, "", "10.0.0.2", 4001) == "RFC2217 10.0.0.2:4001"
    assert ConnectionKind.TCP.is_network and not ConnectionKind.SERIAL.is_network
    assert all(kind.label for kind in ConnectionKind)


def test_factory(no_handshake: Profile) -> None:
    assert type(create_hardware_transport(ConnectionKind.SERIAL, "COM3", "", 1, no_handshake.serial)) is SerialTransport
    net = create_hardware_transport(ConnectionKind.RFC2217, "", "10.0.0.2", 4001, no_handshake.serial)
    assert isinstance(net, NetworkSerialTransport) and net.name == "RFC2217 10.0.0.2:4001"
    assert type(serial.serial_for_url(net.url, do_not_open=True)).__module__ == "serial.rfc2217"
    with pytest.raises(ValueError, match="network"):
        NetworkSerialTransport(ConnectionKind.SERIAL, "x", 1, no_handshake.serial)


def test_send_over_tcp(server: TcpDeviceServer, no_handshake: Profile) -> None:
    transport = NetworkSerialTransport(ConnectionKind.TCP, "127.0.0.1", server.port, no_handshake.serial)
    payload = transform(SAMPLE_PROGRAM, no_handshake.transfer)
    result = ProgramSender(transport, no_handshake, clock=SystemClock()).send(payload)
    assert result.state is SendState.COMPLETED, result.error
    assert result.port == f"TCP 127.0.0.1:{server.port}"
    assert server.done.wait(5)
    assert bytes(server.received) == payload.data
    assert not transport.is_open


def test_receive_over_tcp(no_handshake: Profile) -> None:
    srv = TcpDeviceServer(to_send=SAMPLE_PROGRAM)
    try:
        transport = NetworkSerialTransport(ConnectionKind.TCP, "127.0.0.1", srv.port, no_handshake.serial)
        result = ProgramReceiver(transport, no_handshake, clock=SystemClock()).receive()
        assert result.state is ReceiveState.COMPLETED, result.error
        assert result.raw == SAMPLE_PROGRAM
    finally:
        srv.close()


def test_tcp_modem_lines_not_faked(server: TcpDeviceServer, no_handshake: Profile) -> None:
    transport = NetworkSerialTransport(ConnectionKind.TCP, "127.0.0.1", server.port, no_handshake.serial)
    transport.open()
    try:
        assert transport.get_modem_status() == ModemStatus()
    finally:
        transport.close()


def test_tcp_rejects_cts_requirement(profile_a: Profile) -> None:
    transport = NetworkSerialTransport(ConnectionKind.TCP, "127.0.0.1", 9, profile_a.serial)
    with pytest.raises(TransportConfigurationError, match="RFC 2217"):
        transport.open()
    result = ProgramSender(transport, profile_a).send(transform(b"M30\r\n", profile_a.transfer))
    assert result.state is SendState.FAILED
    assert result.error is not None and "Verbindungsart" in result.error.title
    bad = NetworkSerialTransport(ConnectionKind.TCP, "", 1, profile_a.serial)
    with pytest.raises(TransportConfigurationError):
        bad.open()


def test_unreachable_adapter_is_reported(no_handshake: Profile) -> None:
    probe = socket.socket()
    probe.bind(("127.0.0.1", 0))
    free_port = probe.getsockname()[1]
    probe.close()
    transport = NetworkSerialTransport(ConnectionKind.TCP, "127.0.0.1", free_port, no_handshake.serial)
    with pytest.raises(NetworkUnreachableError) as info:
        transport.open()
    error = describe_error(info.value, port=transport.name)
    assert "nicht erreichbar" in error.title
    assert any("TCP-Server" in c for c in error.causes)


def test_classify_network_errors() -> None:
    exc = serial.SerialException("Could not open port socket://10.0.0.9:4001: timed out")
    assert isinstance(classify_open_error(exc, "socket://10.0.0.9:4001"), NetworkUnreachableError)
    local = serial.SerialException("could not open port COM3: timed out")
    assert not isinstance(classify_open_error(local, "COM3"), NetworkUnreachableError)


def test_config_network_fields(tmp_path: object) -> None:
    from pathlib import Path

    folder = Path(str(tmp_path))
    config = AppConfig(connection_kind="rfc2217", network_host="10.0.0.2", network_port=4002)
    save_config(config, folder)
    loaded, warnings = load_config(folder)
    assert loaded == config and warnings == []
    (folder / "settings.json").write_text('{"connection_kind": "usb", "network_port": 70000}', encoding="utf-8")
    loaded, warnings = load_config(folder)
    assert loaded.connection_kind == "serial" and loaded.network_port == 4001 and len(warnings) == 2
