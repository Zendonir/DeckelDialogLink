from __future__ import annotations

import shutil
import sys
import threading
from pathlib import Path

import pytest
from PySide6.QtCore import QMimeData, QPointF, Qt, QUrl
from PySide6.QtGui import QDropEvent
from PySide6.QtWidgets import QApplication, QFileDialog, QMessageBox
from pytestqt.qtbot import QtBot

from deckellink.commissioning import STEPS, StepStatus
from deckellink.config import AppConfig
from deckellink.domain.models import VerificationStatus
from deckellink.gui.app_support import acquire_instance_lock, install_exception_hooks
from deckellink.gui.style import apply_theme
from deckellink.simulation.scenarios import SAMPLE_PROGRAM
from deckellink.storage.paths import AppPaths
from tests.gui.conftest import WindowFactory, wait_idle

pytestmark = pytest.mark.gui


def copy_valid(fixtures_dir: Path, tmp_path: Path) -> Path:
    target = tmp_path / "valid.nc"
    shutil.copy(fixtures_dir / "valid.nc", target)
    return target


def test_window_fits_small_screens(make_window: WindowFactory) -> None:
    window = make_window()
    hint = window.minimumSizeHint()
    assert hint.width() <= 1024 and hint.height() <= 700
    window.resize(1024, 640)
    assert window.width() <= 1024


def test_menus_present(make_window: WindowFactory) -> None:
    window = make_window()
    titles = [action.text() for action in window.menuBar().actions()]
    assert titles == ["&Datei", "&Ansicht", "&Werkzeuge", "&Hilfe"]
    tool_texts = [a.text() for a in window.tools_menu.actions() if a.text()]
    assert any("Inbetriebnahme" in t for t in tool_texts) and any("Datenformat" in t for t in tool_texts)


def test_help_dialog(make_window: WindowFactory) -> None:
    window = make_window()
    dialog = window.open_help("SAFETY.md")
    assert dialog.current_title() == "Sicherheitskonzept"
    assert "Sicherheitskonzept" in dialog.view.toPlainText()
    dialog.index.setCurrentRow(0)
    assert "DeckelLink" in dialog.view.toPlainText()
    dialog._link(QUrl("docs/TROUBLESHOOTING.md"))
    assert dialog.current_title() == "Fehlersuche"
    dialog.close()


def test_recent_files_and_initial_file(make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path) -> None:
    window = make_window()
    path = copy_valid(fixtures_dir, tmp_path)
    window.open_initial_file(path)
    assert window.program is not None and window.program.path == path
    assert not window.busy  # never sends automatically
    assert window.config.recent_files[0] == str(path)
    entries = [a.text() for a in window.recent_menu.actions()]
    assert any(str(path) in e for e in entries)
    window.clear_recent_files()
    assert window.config.recent_files == []
    window.open_initial_file(tmp_path / "missing.nc")
    assert "nicht gefunden" in window.statusBar().currentMessage()


def test_drop_file_loads_it(make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path) -> None:
    window = make_window()
    path = copy_valid(fixtures_dir, tmp_path)
    mime = QMimeData()
    mime.setUrls([QUrl.fromLocalFile(str(path))])
    event = QDropEvent(
        QPointF(10, 10), Qt.DropAction.CopyAction, mime, Qt.MouseButton.LeftButton, Qt.KeyboardModifier.NoModifier
    )
    window.dropEvent(event)
    assert window.program is not None and window.program.path == path
    assert window.nav.currentRow() == 0


def test_font_zoom(make_window: WindowFactory) -> None:
    window = make_window()
    window.change_font_scale(20)
    assert window.config.font_scale_percent == 120
    app = QApplication.instance()
    assert isinstance(app, QApplication)
    assert "font-size: 12.0pt" in app.styleSheet()
    window.change_font_scale(0)
    assert window.config.font_scale_percent == 100
    apply_theme(app)


def test_format_detection_and_adopt(
    qtbot: QtBot, make_window: WindowFactory, paths: AppPaths, monkeypatch: pytest.MonkeyPatch
) -> None:
    window = make_window(scale=50)
    window.open_format_detection()
    dialog = window.format_dialog
    assert dialog is not None
    dialog.duration.setValue(10)
    dialog.start_button.click()
    wait_idle(qtbot, window, timeout=30000)
    assert dialog.guess is not None and dialog.guess.frame == "7E1"
    assert list(paths.raw.glob("FORMAT_*.raw"))
    monkeypatch.setattr(QMessageBox, "question", lambda *a, **k: QMessageBox.StandardButton.Yes)
    dialog.adopt_button.click()
    assert window.profile.serial.frame == "7E1"
    assert "7E1 ermittelt" in window.profile.display_name
    assert window.profile.verification_status is VerificationStatus.UNVERIFIED
    dialog.close()
    assert window.format_dialog is None


def test_commissioning_assistant(
    qtbot: QtBot, make_window: WindowFactory, paths: AppPaths, monkeypatch: pytest.MonkeyPatch
) -> None:
    window = make_window()
    window.open_commissioning()
    dialog = window.commissioning_dialog
    assert dialog is not None
    assert not dialog.verify_button.isEnabled()
    # simulator results never tick steps automatically
    window._commissioning_auto("loopback", "x")
    assert dialog.record.step("loopback").status is StepStatus.OPEN
    for step in STEPS:
        if step.required:
            dialog.set_status(step.id, StepStatus.DONE)
    assert not dialog.verify_button.isEnabled()
    dialog.tester.setText("M. Muster")
    dialog.tester.textEdited.emit("M. Muster")
    assert dialog.verify_button.isEnabled()
    monkeypatch.setattr(window, "_open_folder", lambda _p: None)
    dialog.export_button.click()
    assert list(paths.exports.glob("Inbetriebnahme_*.md"))
    monkeypatch.setattr(QMessageBox, "question", lambda *a, **k: QMessageBox.StandardButton.Yes)
    dialog.verify_button.click()
    assert window.profile.verification_status is VerificationStatus.VERIFIED
    assert window.profile.id != "dialog11_7n1_rtscts"  # builtin stays unverified
    assert "M. Muster" in window.profile.description
    builtin = window.repo.get("dialog11_7n1_rtscts")
    assert builtin is not None and builtin.verification_status is VerificationStatus.UNVERIFIED


def test_replay_scenario(
    qtbot: QtBot, make_window: WindowFactory, paths: AppPaths, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    recording = tmp_path / "AUFNAHME.raw"
    recording.write_bytes(b"\x00\x00N10 G00 X1\r\nN20 M30\r\n")
    window = make_window(AppConfig(simulation_scenario="replay"))
    assert not window.settings_page.replay_button.isHidden()
    monkeypatch.setattr(QFileDialog, "getOpenFileName", lambda *a, **k: (str(recording), ""))
    window.choose_replay_file()
    assert window.config.simulation_replay_file == str(recording)
    window.start_receive()
    wait_idle(qtbot, window)
    stored = list(paths.received.glob("*.nc"))
    assert len(stored) == 1 and stored[0].read_bytes() == b"N10 G00 X1\r\nN20 M30\r\n"


def test_profile_export_import_gui(make_window: WindowFactory, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    window = make_window()
    target = tmp_path / "geteilt.json"
    monkeypatch.setattr(QFileDialog, "getSaveFileName", lambda *a, **k: (str(target), ""))
    window.settings_page.export_button.click()
    assert target.is_file()
    monkeypatch.setattr(QFileDialog, "getOpenFileName", lambda *a, **k: (str(target), ""))
    window.settings_page.import_button.click()
    assert window.profile.id.startswith("dialog11_7n1_rtscts_import")
    target.write_text("{kaputt", encoding="utf-8")
    window.settings_page.import_button.click()
    assert window.settings_page.profile_error.error is not None


def test_single_instance_lock(paths: AppPaths) -> None:
    first = acquire_instance_lock(paths)
    assert first is not None
    assert acquire_instance_lock(paths) is None
    first.unlock()
    again = acquire_instance_lock(paths)
    assert again is not None
    again.unlock()


def test_unexpected_error_dialog(qtbot: QtBot) -> None:
    shown: list[str] = []
    previous = (sys.excepthook, threading.excepthook)
    try:
        hook = install_exception_hooks(show=shown.append)
        try:
            raise RuntimeError("kaputt")
        except RuntimeError as exc:
            hook(RuntimeError, exc, exc.__traceback__)
        qtbot.waitUntil(lambda: bool(shown), timeout=2000)
        assert "RuntimeError: kaputt" in shown[0]
    finally:
        sys.excepthook, threading.excepthook = previous


def test_sample_program_unchanged() -> None:
    assert SAMPLE_PROGRAM.endswith(b"M30\r\n")
