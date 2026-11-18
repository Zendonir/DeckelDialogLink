from __future__ import annotations

from dataclasses import replace
from pathlib import Path

import pytest
from PySide6.QtWidgets import QMessageBox
from pytestqt.qtbot import QtBot

from deckellink.config import AppConfig
from deckellink.gui.dialogs.parameter_write_dialog import ParameterWriteDialog, ParameterWriteInfo
from deckellink.gui.main_window import NAV_ITEMS
from deckellink.gui.style import StatusKind
from deckellink.parameters.service import WRITE_CONFIRMATION_PHRASE, diff_parameters
from deckellink.simulation.scenarios import SAMPLE_PARAMETERS
from deckellink.storage.paths import AppPaths
from tests.gui.conftest import TestMainWindow, WindowFactory, wait_idle

pytestmark = pytest.mark.gui


def make_writable_profile(window: TestMainWindow) -> None:
    data = window.settings_page.editor.to_dict()
    data["parameters"]["allow_write"] = True
    window.save_profile(data)
    assert window.profile.parameters.allow_write


def read_parameters(qtbot: QtBot, window: TestMainWindow) -> None:
    window.parameters_page.read_button.click()
    wait_idle(qtbot, window, timeout=30000)


def test_parameter_page_present_and_write_locked(make_window: WindowFactory) -> None:
    window = make_window()
    assert "Maschinenparameter" in NAV_ITEMS
    page = window.parameters_page
    assert not page.write_button.isEnabled()
    assert "Profil erlaubt kein Schreiben" in page.gate_label.text()
    assert "nicht freigeschaltet" in page.gate_label.text()
    assert not window.param_unlocked


def test_unlock_requires_confirmation(make_window: WindowFactory, monkeypatch: pytest.MonkeyPatch) -> None:
    window = make_window()
    monkeypatch.setattr(QMessageBox, "warning", lambda *a, **k: QMessageBox.StandardButton.No)
    window.parameters_page.unlock_box.setChecked(True)
    assert not window.param_unlocked
    assert not window.parameters_page.unlock_box.isChecked()
    monkeypatch.setattr(QMessageBox, "warning", lambda *a, **k: QMessageBox.StandardButton.Yes)
    window.parameters_page.unlock_box.setChecked(True)
    assert window.param_unlocked
    window.shutdown()
    assert not window.param_unlocked


def test_read_write_and_verify_cycle(qtbot: QtBot, make_window: WindowFactory, paths: AppPaths, tmp_path: Path) -> None:
    window = make_window()
    page = window.parameters_page
    read_parameters(qtbot, window)
    sets = list((paths.root / "parameters").glob("*.par"))
    assert len(sets) == 1 and sets[0].read_bytes() == SAMPLE_PARAMETERS
    assert window.param_backup is not None
    assert page.state.kind is StatusKind.READY
    assert page.tree.topLevelItemCount() == 1

    edited = tmp_path / "EDIT.par"
    edited.write_bytes(SAMPLE_PARAMETERS.replace(b"P0003=111", b"P0003=112"))
    window.select_parameter_file(edited)
    assert "1 geänderte" in page.diff_summary.text()
    assert "+P0003=112" in page.diff_view.toPlainText()
    assert not page.write_button.isEnabled()  # profile does not allow writing yet

    make_writable_profile(window)
    window.set_parameter_write_unlocked(True, confirm=False)
    # profile change invalidates the backup (different profile id)
    assert "anderem Profil" in page.gate_label.text()
    read_parameters(qtbot, window)
    window.select_parameter_file(edited)
    assert page.write_button.isEnabled(), page.gate_label.text()

    confirmations: list[ParameterWriteInfo] = []

    def decline(info: ParameterWriteInfo, _parent: object) -> bool:
        confirmations.append(info)
        return False

    def accept(info: ParameterWriteInfo, _parent: object) -> bool:
        confirmations.append(info)
        return True

    window.confirm_parameter_write = decline
    window.start_parameter_write()
    assert not window.busy and window.last_written_parameters is None

    window.confirm_parameter_write = accept
    page.write_button.click()
    wait_idle(qtbot, window)
    assert confirmations[-1].diff.changed_lines == 1
    assert window.last_written_parameters == edited.read_bytes()
    assert window.param_backup is None  # a new read is required
    assert not page.write_button.isEnabled()
    assert edited.read_bytes().endswith(b"(ENDE SIMULATION)\r\n")  # original untouched

    # Read back: simulator returns the original sample -> deviation detected
    read_parameters(qtbot, window)
    assert page.verify_label.kind is StatusKind.ERROR
    assert "ABWEICHUNG" in page.verify_label.status_text


def test_compare_two_sets(qtbot: QtBot, make_window: WindowFactory, paths: AppPaths) -> None:
    window = make_window()
    window.compare_parameter_set(paths.root / "missing.par")
    assert "noch keine aktuelle Sicherung" in window.parameters_page.diff_summary.text()
    read_parameters(qtbot, window)
    stored = sorted((paths.root / "parameters").glob("*.par"))[0]
    window.compare_parameter_set(stored)
    assert "Keine Unterschiede" in window.parameters_page.diff_summary.text()
    window.compare_parameter_set(paths.root / "missing.par")
    assert window.parameters_page.error_panel.error is not None


def test_parameter_read_with_lsv2_profile(qtbot: QtBot, make_window: WindowFactory) -> None:
    window = make_window(AppConfig(active_profile_id="dialog11_lsv2_7e1"))
    read_parameters(qtbot, window)
    assert window.param_backup is not None
    assert window.param_backup.data == SAMPLE_PARAMETERS


def test_parameter_read_error_shown(qtbot: QtBot, make_window: WindowFactory) -> None:
    window = make_window(AppConfig(simulation_scenario="port_open_error"))
    read_parameters(qtbot, window)
    assert window.parameters_page.state.kind is StatusKind.ERROR
    assert "belegt" in window.parameters_page.error_panel.title.text()


def test_write_dialog_requires_typed_phrase(qtbot: QtBot) -> None:
    info = ParameterWriteInfo(
        simulation=False,
        port="COM3",
        profile_name="P",
        protocol="LSV2",
        file_name="X.par",
        sha256="ab" * 32,
        backup_name="PARAM_1.par",
        backup_age_min=3,
        diff=diff_parameters(b"A=1\n", b"A=2\n"),
    )
    dialog = ParameterWriteDialog(info)
    qtbot.addWidget(dialog)
    assert not dialog.write_button.isEnabled()
    dialog.ack.setChecked(True)
    dialog.phrase.setText("parameter schreiben")
    assert not dialog.write_button.isEnabled()
    dialog.phrase.setText(WRITE_CONFIRMATION_PHRASE)
    assert dialog.write_button.isEnabled()
    dialog.accept()
    assert dialog.confirmed()
    dialog2 = ParameterWriteDialog(replace(info, simulation=True))
    qtbot.addWidget(dialog2)
    dialog2.reject()
    assert not dialog2.confirmed()
