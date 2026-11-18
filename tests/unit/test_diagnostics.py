from __future__ import annotations

import json
import logging
import zipfile
from datetime import UTC, datetime
from pathlib import Path

import pytest

from deckellink.clock import FakeClock
from deckellink.config import AppConfig
from deckellink.diagnostics.error_messages import describe_error
from deckellink.diagnostics.export import ExportOptions, export_diagnostics, mask_paths
from deckellink.diagnostics.loopback_test import (
    LoopbackPattern,
    LoopbackVerdict,
    make_pattern,
    run_loopback_test,
)
from deckellink.diagnostics.modem_status import format_modem_status, signal_text
from deckellink.diagnostics.terminal import (
    MAX_ENTRY_BYTES,
    TerminalBuffer,
    TerminalEntry,
    TerminalMode,
    TrafficFileLogger,
)
from deckellink.domain.errors import FieldError, ProfileValidationError, ValidationBlockedError
from deckellink.domain.models import Profile, SerialSettings
from deckellink.domain.results import Direction, TrafficEvent
from deckellink.nc.transformer import TransformError
from deckellink.storage.files import FileExistsConflict
from deckellink.storage.paths import AppPaths
from deckellink.transfer.cancellation import CancellationToken
from deckellink.transport.base import (
    HandshakeTimeoutError,
    ModemStatus,
    PortOpenError,
    TransportReadError,
    TransportTimeoutError,
    TransportWriteError,
)
from deckellink.transport.loopback import LoopbackTransport
from deckellink.transport.simulated_transport import SimulatedTransport, SimulationConfig

T0 = datetime(2026, 11, 18, 9, 41, 32, 125000, tzinfo=UTC)


def test_terminal_entry_formats() -> None:
    entry = TerminalEntry(Direction.TX, T0, b"N0010 G00 X0 Y0\r\n")
    combined = entry.format(TerminalMode.COMBINED)
    assert combined.startswith("TX 09:41:32.125\n")
    assert "ASCII: N0010 G00 X0 Y0<CR><LF>" in combined
    assert "HEX:   4E 30 30 31 30 20 47 30 30 20 58 30 20 59 30 0D 0A" in combined
    assert entry.format(TerminalMode.HEX).endswith("0D 0A")
    assert "<CR><LF>" in entry.format(TerminalMode.ASCII)
    assert "<1B>" in TerminalEntry(Direction.RX, T0, b"\x1b").format(TerminalMode.ASCII)


def test_terminal_buffer_line_split_and_limit() -> None:
    buffer = TerminalBuffer(max_entries=10)
    done = buffer.add(TrafficEvent(Direction.TX, T0, b"N10\r\nN2"))
    assert [e.data for e in done] == [b"N10\r\n"]
    done = buffer.add(TrafficEvent(Direction.TX, T0, b"0\r\n"))
    assert [e.data for e in done] == [b"N20\r\n"]
    buffer.add(TrafficEvent(Direction.TX, T0, b"PART"))
    done = buffer.add(TrafficEvent(Direction.RX, T0, b"OK"))
    assert [e.data for e in done] == [b"PART"]  # direction change flushes
    assert [e.data for e in buffer.flush()] == [b"OK"]
    assert buffer.tx_bytes == 14 and buffer.rx_bytes == 2
    for _ in range(50):
        buffer.add(TrafficEvent(Direction.RX, T0, b"X\n"))
    assert len(buffer.entries) == 10  # bounded history
    long = buffer.add(TrafficEvent(Direction.TX, T0, b"A" * (MAX_ENTRY_BYTES + 5)))
    assert len(long[-1].data) == MAX_ENTRY_BYTES
    assert "TX" in buffer.render(TerminalMode.ASCII)
    buffer.resize(20)
    assert buffer.max_entries == 20
    buffer.clear()
    buffer.reset_counters()
    assert buffer.entries == [] and buffer.tx_bytes == 0


def test_traffic_file_logger(tmp_path: Path) -> None:
    logger = TrafficFileLogger(tmp_path / "logs" / "traffic.log")
    logger.write(TerminalEntry(Direction.TX, T0, b"x"))
    logger.open()
    assert logger.is_open
    logger.write(TerminalEntry(Direction.TX, T0, b"N10\r\n"))
    logger.close()
    assert "4E 31 30 0D 0A" in (tmp_path / "logs" / "traffic.log").read_text(encoding="utf-8")


def test_modem_status_text() -> None:
    assert signal_text(None) == "unbekannt"
    text = format_modem_status(ModemStatus(cts=True, dsr=False))
    assert "CTS=EIN" in text and "DSR=AUS" in text and "DCD=unbekannt" in text


@pytest.mark.parametrize("pattern", list(LoopbackPattern))
def test_loopback_patterns_respect_data_bits(pattern: LoopbackPattern) -> None:
    packets = make_pattern(pattern, 300, 7)
    data = b"".join(packets)
    assert len(data) == 300
    assert max(data) <= 0x7F
    if pattern is LoopbackPattern.VARIABLE_PACKETS:
        assert len({len(p) for p in packets}) > 2
    assert max(b"".join(make_pattern(pattern, 100, 5))) <= 0x1F


def test_loopback_pass(clock: FakeClock) -> None:
    settings = SerialSettings()
    packets = make_pattern(LoopbackPattern.BYTE_RANGE, 256, 7)
    events: list[TrafficEvent] = []
    result = run_loopback_test(LoopbackTransport(settings, clock), packets, clock=clock, on_traffic=events.append)
    assert result.verdict is LoopbackVerdict.PASSED
    assert result.received == result.sent
    assert result.hints == ()
    assert result.duration_s > 0
    assert sum(len(e.data) for e in events if e.direction is Direction.RX) == 256
    assert "Bestanden" in result.summary()


def test_loopback_not_bridged_fails_with_hints(clock: FakeClock) -> None:
    result = run_loopback_test(
        LoopbackTransport(SerialSettings(), clock, bridged=False),
        make_pattern(LoopbackPattern.ASCII_TEXT, 50, 7),
        timeout_s=2,
        clock=clock,
    )
    assert result.verdict is LoopbackVerdict.FAILED
    assert result.timed_out
    assert any("TX und RX" in h for h in result.hints)


def test_loopback_partial(clock: FakeClock) -> None:
    result = run_loopback_test(
        LoopbackTransport(SerialSettings(), clock, corrupt_every=10),
        make_pattern(LoopbackPattern.REPEATED, 100, 7),
        clock=clock,
    )
    assert result.verdict is LoopbackVerdict.PARTIAL
    assert len(result.mismatches) == 10
    assert result.matched_bytes == 90
    dropped = run_loopback_test(
        LoopbackTransport(SerialSettings(), clock, drop_every=10),
        make_pattern(LoopbackPattern.REPEATED, 100, 7),
        timeout_s=1,
        clock=clock,
    )
    assert dropped.verdict is LoopbackVerdict.PARTIAL and dropped.timed_out


def test_loopback_open_error_and_cancel(clock: FakeClock) -> None:
    transport = SimulatedTransport(SerialSettings(), SimulationConfig(open_error="busy"), clock)
    result = run_loopback_test(transport, [b"abc"], clock=clock)
    assert result.verdict is LoopbackVerdict.FAILED
    assert result.error is not None and "belegt" in result.error.title
    token = CancellationToken()
    token.cancel()
    cancelled = run_loopback_test(LoopbackTransport(SerialSettings(), clock), [b"abc"], clock=clock, token=token)
    assert cancelled.verdict is LoopbackVerdict.FAILED
    assert cancelled.error is not None and "abgebrochen" in cancelled.error.title


def test_mask_paths() -> None:
    text = r"Opened C:\Users\Max\Desktop\PROG.nc and /home/max/nc/TEIL.nc."
    masked = mask_paths(text)
    assert "Max" not in masked and "max" not in masked
    assert "<PFAD>/PROG.nc" in masked and "<PFAD>/TEIL.nc." in masked


def test_export_package(tmp_path: Path, profile_a: Profile) -> None:
    paths = AppPaths.from_root(tmp_path / "data").ensure()
    (paths.logs / "deckellink.log").write_text(r"file C:\Users\Max\secret\A.nc loaded", encoding="utf-8")
    nc = tmp_path / "PROG.nc"
    nc.write_bytes(b"N10 M30\r\n")
    config = AppConfig(last_open_dir=r"C:\Users\Max")
    target = export_diagnostics(
        paths, config, profile_a, ExportOptions(last_result={"state": "completed"}), clock=FakeClock(T0)
    )
    with zipfile.ZipFile(target) as archive:
        names = set(archive.namelist())
        assert {"system.json", "config.json", "profile.json", "last_result.json", "logs/deckellink.log"} <= names
        assert not any(n.startswith("nc/") or n.startswith("raw/") for n in names)
        assert "Max" not in archive.read("logs/deckellink.log").decode()
        assert json.loads(archive.read("config.json"))["last_open_dir"] == "<entfernt>"
    with_nc = export_diagnostics(
        paths,
        config,
        profile_a,
        ExportOptions(mask_paths=False, include_nc_files=[nc], include_raw_files=[nc]),
        clock=FakeClock(T0),
    )
    assert with_nc != target
    with zipfile.ZipFile(with_nc) as archive:
        assert "nc/PROG.nc" in archive.namelist() and "raw/PROG.nc" in archive.namelist()
        assert "Max" in archive.read("logs/deckellink.log").decode()


@pytest.mark.parametrize(
    ("exc", "fragment"),
    [
        (PortOpenError("x", "COM3"), "COM3 konnte nicht geöffnet werden"),
        (HandshakeTimeoutError("x", "COM3", ("CTS",)), "Sendefreigabe (CTS)"),
        (TransportTimeoutError("x"), "Zeitüberschreitung"),
        (TransportWriteError("x"), "Schreibfehler"),
        (TransportReadError("x"), "Lesefehler"),
        (ProfileValidationError([FieldError("serial.parity", "falsch")]), "Profil ist ungültig"),
        (TransformError("tab"), "aufbereitet"),
        (ValidationBlockedError(["Fehler"]), "gesperrt"),
        (FileExistsConflict("x"), "existiert bereits"),
        (PermissionError("x"), "verweigert"),
        (OSError("x"), "Datei- oder Systemfehler"),
        (RuntimeError("boom"), "Unerwarteter Fehler"),
    ],
)
def test_describe_error_is_user_friendly(exc: BaseException, fragment: str, caplog: pytest.LogCaptureFixture) -> None:
    with caplog.at_level(logging.DEBUG):
        error = describe_error(exc, operation="Test", port="COM3", profile_id="p1")
    assert fragment in error.title
    assert error.causes
    assert type(exc).__name__ in error.technical_details
    assert "Profil: p1" in error.technical_details
    assert "Mögliche Ursachen" in error.as_text()
    assert any("p1" in r.getMessage() for r in caplog.records)


def test_port_open_message_matches_spec() -> None:
    error = describe_error(PortOpenError("could not open port", "COM3"), port="COM3")
    assert error.title == "COM3 konnte nicht geöffnet werden."
    assert "Der Anschluss wird bereits von einem anderen Programm verwendet." in error.causes
    assert "Der USB-RS-232-Adapter wurde getrennt." in error.causes
