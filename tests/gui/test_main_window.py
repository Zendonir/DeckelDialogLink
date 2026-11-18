from __future__ import annotations

import json
import shutil
from pathlib import Path
from typing import Any

import pytest
from PySide6.QtWidgets import QDialog, QFileDialog, QMessageBox
from pytestqt.qtbot import QtBot

from deckellink.config import AppConfig
from deckellink.diagnostics.loopback_test import LoopbackVerdict
from deckellink.domain.states import ReceiveState, SendState
from deckellink.gui import main_window as main_window_module
from deckellink.gui.main_window import NAV_ITEMS, MainWindow
from deckellink.gui.style import StatusKind
from deckellink.storage.paths import AppPaths
from deckellink.transport.simulated_transport import SimulatedTransport, SimulationConfig
from tests.gui.conftest import WindowFactory, wait_idle

pytestmark = pytest.mark.gui


def load_fixture(window: MainWindow, fixtures_dir: Path, tmp_path: Path, name: str = "valid.nc") -> Path:
    target = tmp_path / name
    shutil.copy(fixtures_dir / name, target)
    window.load_file(target)
    return target


def test_window_starts_in_visible_simulation_mode(make_window: WindowFactory) -> None:
    window = make_window()
    assert window.nav.count() == len(NAV_ITEMS) == 8
    assert window.header.machine_label.text() == "DECKEL FP2A"
    assert window.header.control_label.text() == "DIALOG 11"
    assert "SIMULATIONSMODUS" in window.header.mode_banner.text()
    assert window.header.mode_banner.isVisible()
    assert "SIMULATOR" in window.header.connection_label.text()
    assert "7N1" in window.header.connection_label.text()
    assert "NICHT verifiziert" in window.header.profile_label.text()
    assert window.header.status.kind is StatusKind.READY
    assert window.transfer_page.send_button.text().startswith("Simuliert")


def test_send_disabled_without_file(make_window: WindowFactory) -> None:
    window = make_window()
    assert not window.transfer_page.send_button.isEnabled()
    assert "Keine Datei geladen" in window.transfer_page.gate_label.text()
    window.start_send()
    assert not window.busy


def test_send_disabled_without_port_in_real_mode(
    make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path
) -> None:
    window = make_window(AppConfig(simulation_mode=False, port=""))
    load_fixture(window, fixtures_dir, tmp_path)
    assert "REALBETRIEB" in window.header.mode_banner.text()
    assert window.header.status.kind is StatusKind.IDLE
    assert not window.transfer_page.send_button.isEnabled()
    assert "COM-Port" in window.transfer_page.gate_label.text()
    assert not window.receive_page.start_button.isEnabled()
    window.set_port("COM42")
    assert window.transfer_page.send_button.isEnabled()
    assert window.transfer_page.send_button.text().startswith("An Maschine")


def test_simulated_send_completes_and_original_unchanged(
    qtbot: QtBot, make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path
) -> None:
    window = make_window()
    path = load_fixture(window, fixtures_dir, tmp_path)
    original = path.read_bytes()
    assert window.transfer_page.send_button.isEnabled()
    window.transfer_page.send_button.click()
    assert window.busy
    wait_idle(qtbot, window)
    assert window.transfer_page.state.status_text == SendState.COMPLETED.label
    assert window.last_result is not None and window.last_result["state"] == "completed"
    assert window.transfer_page.progress.value() == 1000
    assert path.read_bytes() == original
    info = window.confirmations[0]
    assert info.simulation and info.file_name == "valid.nc"
    assert not info.show_safety_notice or info.simulation
    assert window.terminal.tx_bytes == len(original)
    assert "TX" in window.terminal_page.view.toPlainText()
    window.terminal_page.mode_combo.setCurrentIndex(1)
    assert "4E 31 30" in window.terminal_page.view.toPlainText()


def test_cancel_available_during_transfer(
    qtbot: QtBot, make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path
) -> None:
    window = make_window(AppConfig(simulation_scenario="user_cancel"), scale=1.0)
    load_fixture(window, fixtures_dir, tmp_path)
    window.start_send()
    qtbot.waitUntil(lambda: window.transfer_page.cancel_button.isEnabled(), timeout=5000)
    assert window.header.status.kind in (StatusKind.ACTIVE, StatusKind.WARNING)
    assert not window.transfer_page.open_button.isEnabled()
    assert not window.settings_page.isEnabled()
    window.transfer_page.cancel_button.click()
    wait_idle(qtbot, window)
    assert window.transfer_page.state.status_text == SendState.CANCELLED.label
    assert not window.transfer_page.cancel_button.isEnabled()
    assert window.transfer_page.open_button.isEnabled()


def test_declined_confirmation_does_not_send(make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path) -> None:
    window = make_window(accept=False)
    load_fixture(window, fixtures_dir, tmp_path)
    window.start_send()
    assert not window.busy
    assert window.last_result is None


def test_error_is_user_friendly(qtbot: QtBot, make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path) -> None:
    window = make_window(AppConfig(simulation_scenario="port_open_error"))
    load_fixture(window, fixtures_dir, tmp_path)
    window.start_send()
    wait_idle(qtbot, window)
    panel = window.transfer_page.error_panel
    assert not panel.isHidden()
    assert "belegt" in panel.title.text()
    assert "Mögliche Ursachen" in panel.causes.text()
    assert not panel.details.isVisible()
    panel.toggle.click()
    assert panel.details.isVisible()
    assert "PortBusyError" in panel.details.toPlainText()
    assert window.header.status.kind is StatusKind.ERROR
    assert window.diagnostics_page.last_error.text() != "–"


def test_real_mode_missing_port_error_and_safety_notice(
    qtbot: QtBot, make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    def fake_serial(port: str, settings: Any) -> SimulatedTransport:
        return SimulatedTransport(settings, SimulationConfig(open_error="not_found"))

    monkeypatch.setattr(main_window_module, "SerialTransport", fake_serial)
    window = make_window(AppConfig(simulation_mode=False, port="COM42"))
    load_fixture(window, fixtures_dir, tmp_path)
    window.start_send()
    wait_idle(qtbot, window)
    info = window.confirmations[0]
    assert not info.simulation and info.show_safety_notice and info.port == "COM42"
    assert "nicht gefunden" in window.transfer_page.error_panel.title.text()


def test_warnings_must_be_acknowledged(make_window: WindowFactory, tmp_path: Path) -> None:
    window = make_window()
    path = tmp_path / "nom30.nc"
    path.write_bytes(b"N10 G90\r\nN20 X1\r\n")
    window.load_file(path)
    page = window.transfer_page
    assert page.ack_warnings.isVisible()
    assert not page.send_button.isEnabled()
    assert "Warnungen" in page.gate_label.text()
    page.ack_warnings.setChecked(True)
    assert page.send_button.isEnabled()


def test_errors_block_transfer(make_window: WindowFactory, fixtures_dir: Path, tmp_path: Path) -> None:
    window = make_window()
    load_fixture(window, fixtures_dir, tmp_path, "invalid_chars.nc")
    page = window.transfer_page
    assert not page.send_button.isEnabled()
    assert page.verdict.kind is StatusKind.ERROR
    assert page.findings.rowCount() >= 2
    assert not page.ack_warnings.isVisible()


def test_simulated_receive_stores_program(qtbot: QtBot, make_window: WindowFactory, paths: AppPaths) -> None:
    window = make_window(AppConfig(simulation_scenario="receive_ok"))
    window.receive_page.start_button.click()
    wait_idle(qtbot, window)
    assert window.receive_page.state.status_text == ReceiveState.COMPLETED.label
    files = list(paths.received.glob("*.nc"))
    assert len(files) == 1
    meta = json.loads(files[0].with_suffix(".json").read_text(encoding="utf-8"))
    assert meta["complete"] is True and len(meta["sha256_normalized"]) == 64
    assert window.programs_page.tree.topLevelItemCount() == 1
    assert "M30" in window.receive_page.ascii_view.toPlainText()
    assert window.receive_page.hex_view.toPlainText()
    item = window.programs_page.tree.topLevelItem(0)
    assert item is not None
    window.programs_page.tree.setCurrentItem(item)
    assert "sha256_raw" in window.programs_page.meta_view.toPlainText()
    window.receive_page.load_button.click()
    assert window.program is not None and window.program.path == files[0]
    assert window.nav.currentRow() == 0


def test_receive_with_send_only_scenario_uses_sample(qtbot: QtBot, make_window: WindowFactory, paths: AppPaths) -> None:
    window = make_window(AppConfig(simulation_scenario="send_ok"))
    window.start_receive()
    wait_idle(qtbot, window)
    assert list(paths.received.glob("*.nc"))


def test_incomplete_receive_goes_to_failed(qtbot: QtBot, make_window: WindowFactory, paths: AppPaths) -> None:
    window = make_window(AppConfig(simulation_scenario="receive_no_m30"))
    window.start_receive()
    wait_idle(qtbot, window, timeout=30000)
    assert list(paths.failed.glob("*.nc"))
    assert window.receive_page.result_state.kind is StatusKind.WARNING


def test_sample_and_append_program_end(make_window: WindowFactory) -> None:
    window = make_window(AppConfig(simulation_scenario="invalid_ascii_file"))
    window.load_sample()
    assert window.program is not None and window.program.analysis.has_errors
    assert not window.transfer_page.send_button.isEnabled()


def test_profile_edit_creates_copy(make_window: WindowFactory, paths: AppPaths) -> None:
    window = make_window()
    editor = window.settings_page.editor
    editor.name_edit.setText("Mein Profil")
    data = editor.to_dict()
    data["serial"]["baud_rate"] = 9600
    window.save_profile(data)
    assert window.profile.id != "dialog11_7n1_rtscts"
    assert window.profile.serial.baud_rate == 9600
    assert (paths.profiles / f"{window.profile.id}.json").is_file()
    assert window.config.active_profile_id == window.profile.id
    assert "9600" in window.header.connection_label.text()
    builtin = window.repo.get("dialog11_7n1_rtscts")
    assert builtin is not None and builtin.serial.baud_rate == 4800


def test_invalid_profile_shows_field_errors(make_window: WindowFactory) -> None:
    window = make_window()
    data = window.settings_page.editor.to_dict()
    data["display_name"] = ""
    data["serial"]["parity"] = "bogus"
    window.save_profile(data)
    panel = window.settings_page.profile_error
    assert not panel.isHidden()
    assert "serial.parity" in panel.causes.text()
    assert window.profile.id == "dialog11_7n1_rtscts"


def test_profile_switch_duplicate_delete(make_window: WindowFactory, monkeypatch: pytest.MonkeyPatch) -> None:
    window = make_window()
    window.select_profile("dialog11_7e1_none")
    assert window.profile.serial.frame == "7E1"
    assert "Kein Handshake" in window.header.connection_label.text()
    window.duplicate_profile("dialog11_7e1_none")
    copy_id = window.profile.id
    assert copy_id.startswith("dialog11_7e1_none_kopie")
    monkeypatch.setattr(QMessageBox, "question", lambda *a, **k: QMessageBox.StandardButton.Yes)
    window.delete_profile(copy_id)
    assert window.repo.get(copy_id) is None
    window.delete_profile("dialog11_7e1_none")
    assert window.repo.get("dialog11_7e1_none") is not None
    window.reload_profiles()


def test_broken_profile_reported_on_start(make_window: WindowFactory, paths: AppPaths) -> None:
    (paths.profiles / "broken.json").write_text("{ nope", encoding="utf-8")
    window = make_window(AppConfig(active_profile_id="missing_profile"))
    assert window.profile.id == "dialog11_7n1_rtscts"
    assert window.settings_page.profile_error.error is not None
    assert "broken.json" in window.settings_page.profile_error.causes.text()


def test_mode_switch_and_settings(make_window: WindowFactory, paths: AppPaths) -> None:
    window = make_window()
    window.settings_page.real_radio.setChecked(True)
    assert not window.config.simulation_mode
    assert "REALBETRIEB" in window.header.mode_banner.text()
    window.settings_page.sim_radio.setChecked(True)
    assert window.config.simulation_mode
    window.settings_page.safety_notice.setChecked(False)
    window.settings_page.retention.setValue(10)
    window.settings_page.raw_logging.setChecked(True)
    assert window.traffic_logger is not None
    assert not window.config.show_safety_notice
    assert window.backups.retention_days == 10
    window.settings_page.raw_logging.setChecked(False)
    assert window.traffic_logger is None
    saved = json.loads((paths.config / "settings.json").read_text(encoding="utf-8"))
    assert saved["backup_retention_days"] == 10
    window.set_scenario("slow")
    assert window.config.simulation_scenario == "slow"


def test_diagnostics_monitor_and_control_lines(qtbot: QtBot, make_window: WindowFactory) -> None:
    window = make_window(scale=1.0)
    page = window.diagnostics_page
    page.monitor_button.click()
    qtbot.waitUntil(lambda: "CTS\nEIN" in page.signals.text_of("CTS"), timeout=5000)
    assert window.header.status.text().endswith("Verbunden (Diagnose)")
    window.request_control_line("DTR", True)
    qtbot.waitUntil(lambda: "DTR\nEIN" in page.signals.text_of("DTR"), timeout=5000)
    page.monitor_button.click()
    wait_idle(qtbot, window)
    assert page.port_state.kind is StatusKind.IDLE


def test_loopback_dialog_in_simulation(qtbot: QtBot, make_window: WindowFactory) -> None:
    window = make_window()
    window.open_loopback()
    dialog = window.loopback_dialog
    assert dialog is not None
    dialog.length.setValue(64)
    dialog.run_button.click()
    wait_idle(qtbot, window)
    assert dialog.verdict.status_text == LoopbackVerdict.PASSED.label
    assert "64 von 64" in dialog.details.toPlainText()
    dialog.close()
    assert window.loopback_dialog is None


def test_export_diagnostics_from_gui(
    make_window: WindowFactory, paths: AppPaths, monkeypatch: pytest.MonkeyPatch
) -> None:
    window = make_window()
    monkeypatch.setattr("deckellink.gui.main_window.ExportDialog.exec", lambda self: QDialog.DialogCode.Accepted)
    monkeypatch.setattr(window, "_open_folder", lambda _p: None)
    window.export_diagnostics_package()
    assert list(paths.exports.glob("DeckelLink_Diagnose_*.zip"))


def test_export_program_copy(
    make_window: WindowFactory, paths: AppPaths, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    window = make_window()
    source = paths.received / "A.nc"
    source.write_bytes(b"N10 M30\r\n")
    target = tmp_path / "out.nc"
    target.write_bytes(b"OLD")
    monkeypatch.setattr(QFileDialog, "getSaveFileName", lambda *a, **k: (str(target), ""))
    window.export_program(source)
    assert target.read_bytes() == b"N10 M30\r\n"
    assert any(p.read_bytes() == b"OLD" for p in paths.backups.rglob("*.nc"))


def test_log_page_receives_records(qtbot: QtBot, make_window: WindowFactory) -> None:
    window = make_window()
    import logging

    logging.getLogger("deckellink.test").warning("Sichtbare Testmeldung")
    qtbot.waitUntil(lambda: "Sichtbare Testmeldung" in window.log_page.view.toPlainText(), timeout=3000)


def test_close_event_shuts_down_cleanly(qtbot: QtBot, paths: AppPaths) -> None:
    window = MainWindow(paths, AppConfig(), sim_time_scale=200)
    qtbot.addWidget(window)
    window.show()
    assert window.close()
    assert window.task is None
