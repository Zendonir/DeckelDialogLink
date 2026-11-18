from __future__ import annotations

import errno
from typing import Any

import pytest
import serial

from deckellink.clock import FakeClock
from deckellink.domain.models import Parity, SerialSettings
from deckellink.transport.base import (
    ConnectionLostError,
    ModemStatus,
    PortBusyError,
    PortNotFoundError,
    PortOpenError,
    Transport,
    TransportError,
    TransportReadError,
    TransportTimeoutError,
    TransportWriteError,
)
from deckellink.transport.loopback import LoopbackTransport
from deckellink.transport.serial_transport import (
    PortInfo,
    SerialTransport,
    available_ports,
    classify_open_error,
)
from deckellink.transport.simulated_transport import SimulatedTransport, SimulationConfig, build_machine_stream

S = SerialSettings(rts_cts=True, require_cts_before_send=True)


class FakeSerial:
    """Stands in for serial.Serial - no real COM port is ever touched."""

    def __init__(self) -> None:
        self.is_open = False
        self.written = bytearray()
        self.to_read = bytearray(b"HELLO")
        self.fail_open: BaseException | None = None
        self.fail_write: BaseException | None = None
        self.fail_read: BaseException | None = None
        self.cancelled: list[str] = []
        self.rts = False
        self.dtr = False
        self.cts = True
        self.dsr = False
        self.cd = False
        self.ri = False
        self.in_waiting = 5
        self.out_waiting = 0

    def open(self) -> None:
        if self.fail_open:
            raise self.fail_open
        self.is_open = True

    def close(self) -> None:
        self.is_open = False

    def write(self, data: bytes) -> int:
        if self.fail_write:
            raise self.fail_write
        self.written += data
        return len(data)

    def read(self, size: int) -> bytes:
        if self.fail_read:
            raise self.fail_read
        chunk = bytes(self.to_read[:size])
        del self.to_read[:size]
        return chunk

    def cancel_read(self) -> None:
        self.cancelled.append("read")

    def cancel_write(self) -> None:
        self.cancelled.append("write")

    def reset_input_buffer(self) -> None:
        self.to_read.clear()


def make_serial(fake: FakeSerial, settings: SerialSettings = S, port: str = "COM7") -> SerialTransport:
    return SerialTransport(port, settings, serial_factory=lambda: fake)


def test_serial_transport_configuration() -> None:
    fake = FakeSerial()
    settings = SerialSettings(
        baud_rate=9600,
        data_bits=7,
        parity=Parity.EVEN,
        stop_bits=2,
        rts_cts=False,
        xon_xoff=True,
        initial_rts=True,
        initial_dtr=True,
        read_timeout_ms=250,
        write_timeout_ms=5000,
        inter_byte_timeout_ms=20,
    )
    transport = make_serial(fake, settings)
    assert isinstance(transport, Transport)
    transport.open()
    f: Any = fake
    assert (f.port, f.baudrate, f.bytesize, f.parity, f.stopbits) == (
        "COM7",
        9600,
        serial.SEVENBITS,
        serial.PARITY_EVEN,
        serial.STOPBITS_TWO,
    )
    assert (f.timeout, f.write_timeout, f.inter_byte_timeout) == (0.25, 5.0, 0.02)
    assert (f.rtscts, f.xonxoff, f.dsrdtr, f.rts, f.dtr) == (False, True, False, True, True)
    assert transport.is_open and not transport.is_simulated
    assert transport.write(b"N10\r\n") == 5
    assert transport.read(3) == b"HEL"
    assert transport.bytes_waiting == 5
    assert transport.out_waiting == 0
    status = transport.get_modem_status()
    assert status == ModemStatus(cts=True, dsr=False, dcd=False, ri=False, rts=True, dtr=True)
    transport.set_control_lines(rts=False, dtr=False)
    assert (fake.rts, fake.dtr) == (False, False)
    transport.cancel()
    assert fake.cancelled == ["read", "write"]
    transport.reset_input_buffer()
    transport.close()
    assert not transport.is_open
    assert transport.get_modem_status() == ModemStatus()
    transport.close()


def test_serial_transport_no_port() -> None:
    with pytest.raises(PortOpenError):
        SerialTransport("", S, serial_factory=FakeSerial).open()


@pytest.mark.parametrize(
    ("exc", "expected"),
    [
        (
            serial.SerialException("could not open port 'COM3': PermissionError(13, 'Zugriff verweigert')"),
            PortBusyError,
        ),
        (serial.SerialException("could not open port 'COM9': FileNotFoundError(2, 'not found')"), PortNotFoundError),
        (OSError(errno.EBUSY, "busy"), PortBusyError),
        (OSError(errno.ENOENT, "missing"), PortNotFoundError),
        (serial.SerialException("strange"), PortOpenError),
    ],
)
def test_open_error_classification(exc: BaseException, expected: type) -> None:
    fake = FakeSerial()
    fake.fail_open = exc
    with pytest.raises(expected) as info:
        make_serial(fake).open()
    assert type(info.value) is expected
    assert type(classify_open_error(exc, "COM3")) is expected


def test_serial_write_read_errors() -> None:
    fake = FakeSerial()
    transport = make_serial(fake)
    with pytest.raises(ConnectionLostError):
        transport.write(b"x")
    transport.open()
    fake.fail_write = serial.SerialTimeoutException("timeout")
    with pytest.raises(TransportTimeoutError):
        transport.write(b"x")
    fake.fail_write = serial.SerialException("gone")
    with pytest.raises(ConnectionLostError):
        transport.write(b"x")
    fake.fail_write = OSError("io")
    with pytest.raises(TransportWriteError):
        transport.write(b"x")
    fake.fail_read = serial.SerialException("gone")
    with pytest.raises(ConnectionLostError):
        transport.read(1)
    fake.fail_read = OSError("io")
    with pytest.raises(TransportReadError):
        transport.read(1)


def test_available_ports_never_raises() -> None:
    ports = available_ports()
    assert isinstance(ports, list)
    assert PortInfo("COM3", "USB Serial", "x").label == "COM3 - USB Serial"
    assert PortInfo("COM3", "n/a", "").label == "COM3"


def test_simulated_transport_basic(clock: FakeClock) -> None:
    settings = SerialSettings()
    transport = SimulatedTransport(settings, SimulationConfig(machine_program=b"ABC", machine_send_delay_s=0.1), clock)
    assert isinstance(transport, Transport)
    assert transport.get_modem_status() == ModemStatus()
    transport.open()
    transport.open()
    assert transport.open_count == 1
    assert transport.bytes_waiting == 0
    received = b""
    while len(received) < 3:
        received += transport.read(10)
    assert received == b"ABC"
    assert transport.read(10) == b""
    assert transport.write(b"\xffX") == 2
    assert transport.machine_received == b"\x7fX"  # masked to 7 data bits
    transport.set_control_lines(rts=False, dtr=True)
    status = transport.get_modem_status()
    assert (status.rts, status.dtr, status.cts, status.dsr) == (False, True, True, True)
    transport.reset_input_buffer()
    transport.close()
    assert transport.close_count == 1
    with pytest.raises(ConnectionLostError):
        transport.write(b"x")


def test_simulated_timing_controllable(clock: FakeClock) -> None:
    settings = SerialSettings(baud_rate=9600, parity=Parity.NONE, data_bits=8)  # 960 B/s
    transport = SimulatedTransport(settings, SimulationConfig(speed_factor=0.5), clock)
    transport.open()
    start = clock.monotonic()
    transport.write(b"x" * 480)
    assert clock.monotonic() - start == pytest.approx(1.0)


def test_simulated_cts_blocks_hardware_handshake(clock: FakeClock) -> None:
    transport = SimulatedTransport(
        SerialSettings(rts_cts=True, write_timeout_ms=2000), SimulationConfig(cts=False), clock
    )
    transport.open()
    with pytest.raises(TransportTimeoutError):
        transport.write(b"abc")
    assert clock.monotonic() == pytest.approx(2.0)


def test_simulated_reproducible_errors(clock: FakeClock) -> None:
    for config, exc in (
        (SimulationConfig(write_error_after_bytes=0), TransportWriteError),
        (SimulationConfig(disconnect_after_bytes=0), ConnectionLostError),
        (SimulationConfig(write_stall_after_bytes=0), TransportTimeoutError),
    ):
        transport = SimulatedTransport(SerialSettings(), config, clock)
        transport.open()
        with pytest.raises(exc):
            transport.write(b"a")
    assert issubclass(PortBusyError, TransportError)


def test_simulated_cancel_interrupts(clock: FakeClock) -> None:
    transport = SimulatedTransport(SerialSettings(), SimulationConfig(), clock)
    transport.open()
    transport.cancel()
    assert transport.write(b"abc") == 0
    assert transport.read(5) == b""


def test_machine_stream_modifications() -> None:
    config = SimulationConfig(machine_program=b"ABCDEF\r\n", inject_null_every=2, corrupt_offsets=(0, 7, 99))
    stream = build_machine_stream(config)
    assert stream.count(0) == 4
    assert stream[0] == ord("A") ^ 0x40


def test_loopback_transport(clock: FakeClock) -> None:
    settings = SerialSettings(data_bits=7)
    transport = LoopbackTransport(settings, clock)
    assert isinstance(transport, Transport)
    transport.open()
    transport.write(b"hello\xff")
    assert transport.bytes_waiting == 6
    assert transport.read(100) == b"hello\x7f"
    assert transport.read(10) == b""
    transport.set_control_lines(rts=True, dtr=False)
    assert transport.get_modem_status().cts is True
    transport.reset_input_buffer()
    transport.cancel()
    assert transport.write(b"x") == 0
    transport.close()
    assert transport.get_modem_status() == ModemStatus()
    with pytest.raises(ConnectionLostError):
        transport.read(1)
    broken = LoopbackTransport(settings, clock, bridged=False)
    broken.open()
    broken.write(b"abc")
    assert broken.read(3) == b""
