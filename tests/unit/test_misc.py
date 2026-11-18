from __future__ import annotations

import json
import logging
import threading
from pathlib import Path

import pytest

from deckellink.clock import FakeClock, ScaledClock, SystemClock
from deckellink.config import AppConfig, load_config, save_config
from deckellink.domain.models import TransferSettings
from deckellink.domain.results import AnalysisResult, TransformResult
from deckellink.domain.states import EndReason, ReceiveState, SendState
from deckellink.gui.viewmodels import (
    LoadedProgram,
    evaluate_receive_gate,
    evaluate_send_gate,
    format_bytes,
    format_duration,
    format_rate,
    preview_text,
)
from deckellink.logging.setup import log_shutdown, log_startup, set_level, setup_logging
from deckellink.nc.analyzer import analyze
from deckellink.transfer.cancellation import CancellationToken, OperationCancelled


def test_config_roundtrip_and_validation(tmp_path: Path) -> None:
    config, warnings = load_config(tmp_path)
    assert config == AppConfig() and warnings == []
    assert config.simulation_mode is True  # safe default
    config.port = "COM3"
    config.backup_retention_days = 30
    save_config(config, tmp_path)
    save_config(config, tmp_path)
    assert (tmp_path / "settings.json.bak").is_file()
    loaded, warnings = load_config(tmp_path)
    assert loaded == config and warnings == []
    (tmp_path / "settings.json").write_text(
        json.dumps(
            {
                "port": 3,
                "simulation_mode": "yes",
                "log_level": "LOUD",
                "backup_retention_days": -1,
                "terminal_history_lines": 5,
            }
        ),
        encoding="utf-8",
    )
    loaded, warnings = load_config(tmp_path)
    assert loaded.port == "" and loaded.simulation_mode is True and loaded.log_level == "INFO"
    assert loaded.terminal_history_lines == 100
    assert len(warnings) == 4
    (tmp_path / "settings.json").write_text("{broken", encoding="utf-8")
    loaded, warnings = load_config(tmp_path)
    assert loaded == AppConfig() and warnings
    (tmp_path / "settings.json").write_text("[]", encoding="utf-8")
    assert load_config(tmp_path)[1]


def test_clocks() -> None:
    fake = FakeClock()
    event = threading.Event()
    assert fake.sleep(2.5, event) is False
    assert fake.monotonic() == 2.5
    event.set()
    assert fake.sleep(1, event) is True
    assert fake.monotonic() == 2.5
    system = SystemClock()
    assert system.sleep(0) is False
    assert system.sleep(0.001) is False
    assert system.sleep(1, event) is True
    assert system.now().tzinfo is not None
    scaled = ScaledClock(1000)
    t0 = scaled.monotonic()
    scaled.sleep(1.0)
    assert scaled.monotonic() - t0 >= 0.9
    assert scaled.scale == 1000
    assert scaled.now().tzinfo is not None
    with pytest.raises(ValueError, match="positive"):
        ScaledClock(0)


def test_cancellation_token() -> None:
    token = CancellationToken()
    calls: list[str] = []
    token.register(lambda: calls.append("a"))

    def failing() -> None:
        raise RuntimeError("ignored")

    token.register(failing)
    token.raise_if_cancelled()
    token.cancel()
    token.cancel()
    assert calls == ["a"]
    token.register(lambda: calls.append("late"))
    assert calls == ["a", "late"]
    with pytest.raises(OperationCancelled):
        token.raise_if_cancelled()


def test_state_labels() -> None:
    assert all(state.label for state in SendState)
    assert all(state.label for state in ReceiveState)
    assert all(reason.label for reason in EndReason)
    assert SendState.SENDING.is_active and SendState.FAILED.is_terminal
    assert ReceiveState.LISTENING.is_active and ReceiveState.TIMED_OUT.is_terminal


def _program(data: bytes) -> LoadedProgram:
    analysis: AnalysisResult = analyze(data, TransferSettings())
    payload = TransformResult(data, 1, (len(data),), ()) if not analysis.has_errors else None
    return LoadedProgram(Path("x.nc"), data, analysis, payload)


def test_send_gate() -> None:
    gate = evaluate_send_gate(None, simulation=True, port="", busy=False, warnings_acknowledged=False)
    assert not gate.allowed and "Keine Datei" in gate.reason_text
    ok = _program(b"N10 M30\r\n")
    assert evaluate_send_gate(ok, simulation=True, port="", busy=False, warnings_acknowledged=False).allowed
    no_port = evaluate_send_gate(ok, simulation=False, port="", busy=False, warnings_acknowledged=False)
    assert not no_port.allowed and "COM-Port" in no_port.reason_text
    assert evaluate_send_gate(ok, simulation=False, port="COM3", busy=False, warnings_acknowledged=False).allowed
    assert not evaluate_send_gate(ok, simulation=True, port="", busy=True, warnings_acknowledged=True).allowed
    warn = _program(b"N10 X1\r\n")  # missing M30 -> warning
    assert not evaluate_send_gate(warn, simulation=True, port="", busy=False, warnings_acknowledged=False).allowed
    assert evaluate_send_gate(warn, simulation=True, port="", busy=False, warnings_acknowledged=True).allowed
    error = _program(b"")
    gate = evaluate_send_gate(error, simulation=True, port="", busy=False, warnings_acknowledged=True)
    assert not gate.allowed and "Fehler" in gate.reason_text
    assert not evaluate_receive_gate(simulation=False, port="", busy=False).allowed
    assert evaluate_receive_gate(simulation=True, port="", busy=False).allowed


def test_formatting() -> None:
    assert format_bytes(10) == "10 B"
    assert format_bytes(12698) == "12,4 KB"
    assert format_bytes(3 * 1024 * 1024) == "3,00 MB"
    assert format_duration(98) == "00:01:38"
    assert format_rate(533.3) == "533 B/s"
    assert preview_text(b"A\r\nB\r\nC\r\n", max_lines=2).endswith("(1 weitere Zeilen)")


def test_logging_setup(tmp_path: Path) -> None:
    path = setup_logging(tmp_path, "DEBUG")
    setup_logging(tmp_path, "INFO", console=False)
    log_startup()
    logging.getLogger("deckellink.test").info("hello")
    set_level("WARNING")
    logging.getLogger("deckellink.test").info("hidden")
    log_shutdown()
    set_level("INFO")
    text = path.read_text(encoding="utf-8")
    assert "hello" in text and "hidden" not in text and "Python:" in text
    root = logging.getLogger()
    for handler in list(root.handlers):
        if getattr(handler, "_deckellink_handler", False):
            root.removeHandler(handler)
            handler.close()
