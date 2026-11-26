"""Simulator: DIALOG 11 menu structure, machine panel keys, auxiliary modes, DIA, transfer follow-up."""

from __future__ import annotations

from pathlib import Path

import pytest
from PySide6.QtWidgets import QFileDialog, QInputDialog
from pytestqt.qtbot import QtBot

from deckellink.gui.simulator_dialogs import CuttingDataDialog, DiaDialog, SelectProgramDialog
from deckellink.gui.simulator_window import SimulatorWindow
from tests.gui.test_simulator_gui import PROGRAM, wait_sim


@pytest.fixture
def sim(qtbot: QtBot) -> SimulatorWindow:
    window = SimulatorWindow(PROGRAM, "TEST.nc", follow_path=Path("TEST.nc"))
    qtbot.addWidget(window)
    window.show()
    wait_sim(qtbot, window)
    return window


def test_panel_keys_and_displays(sim: SimulatorWindow) -> None:
    assert sim.playback is not None
    sim.single_block()
    assert sim.running
    while sim.running:
        sim.advance(0.5)
    assert 0 < sim.time < sim.playback.total
    sim.start()
    sim.advance(1.0)
    sim.pause()
    assert not sim.running
    sim.to_end()
    assert sim.time == pytest.approx(sim.playback.total)
    assert sim.dro_labels[0][0].text().strip() == "30.000"
    assert sim.dro_labels[2][0].text().strip() == "5.000"
    labels = [sim.modal_tree.topLevelItem(i).text(0) for i in range(sim.modal_tree.topLevelItemCount())]
    assert "Wegbedingung" in labels
    sim.rewind()
    assert sim.time == 0
    sim._slider_moved(500)
    assert sim.time == pytest.approx(sim.playback.total / 2)
    cursor = sim.editor.textCursor()
    cursor.movePosition(cursor.MoveOperation.Start)
    cursor.movePosition(cursor.MoveOperation.Down, n=1)
    sim.editor.setTextCursor(cursor)
    assert sim.time == pytest.approx(sim.playback.time_of_line(2) or 0)
    for button in sim.panel_buttons:
        button.click()
    sim.pause()


def test_automatic_menu_tree(sim: SimulatorWindow) -> None:
    assert sim.playback is not None
    sim.set_mode(2)
    assert sim.softkeys[0].text().startswith("F1  21")
    assert not sim.softkeys[2].isEnabled()
    sim.softkey(1)  # 21 TESTLAUF
    sim.softkey(1)  # 211 TESTLAUF
    assert "211 TESTLAUF" in sim.mode_display.text()
    sim.softkey(1)  # 2111 FOLGESATZ EIN/AUS
    assert sim.single_block_mode and "EINZELSATZ" in sim.mode_display.text()
    sim.softkey(7)  # 2117 START/STOP
    assert sim.running
    while sim.running:
        sim.advance(0.5)
    assert 0 < sim.time < sim.playback.total
    sim.softkey(1)
    sim.softkey(3)  # 2113 ECHTZEIT
    assert sim.realtime and sim.speed_combo.currentIndex() == 0
    sim.softkey(6)  # 2116 GRAFIK
    assert not sim.graphics_on and "GRAFIK AUS" in sim.mode_display.text()
    sim.softkey(6)
    assert sim.graphics_on
    sim.softkey(7)
    sim.softkey(7)  # start/stop toggles
    sim.softkey(8)  # 2118 ZURÜCK
    assert sim.menu.code == "21"
    sim.softkey(2)  # 212 TESTLAUF MASCH KPL
    assert sim.running and sim.speed_combo.currentIndex() == sim.speed_combo.count() - 1
    sim.pause()
    sim.menu_back()
    sim.menu_back()
    assert sim.menu.code == "2"
    sim.softkey(4)  # 24 WERKZEUG MAGAZIN (machine function)
    assert "nicht nachgebildet" in sim.screen_text.toPlainText()
    sim.softkey(8)
    sim.softkey(6)
    sim.softkey(3)  # empty slot
    sim.softkey(9)
    sim.run_action("unknown_action")
    assert "nicht belegt" in sim.statusBar().currentMessage()


def test_manual_menu(sim: SimulatorWindow, monkeypatch: pytest.MonkeyPatch) -> None:
    sim.set_mode(1)
    assert sim.editor.isReadOnly() and "HANDFAHREN" in sim.mode_display.text()
    sim.softkey(2)  # 12 INKREMENT
    before = sim.manual_position
    sim.softkey(2)  # X+
    sim.softkey(5)  # Z-
    assert sim.manual_position[0] == pytest.approx(before[0] + 10)
    assert sim.manual_position[2] == pytest.approx(before[2] - 10)
    sim.softkey(7)
    assert sim.jog_step.currentIndex() == 4
    sim.softkey(8)
    sim.softkey(1)  # 11 PRESET
    assert sim.dro_labels[0][0].text().strip() == "0.000"
    assert sim.goto("X12,5 Y-3")
    assert sim.dro_labels[0][0].text().strip() == "12.500"
    assert not sim.goto("nichts")
    monkeypatch.setattr(QInputDialog, "getText", lambda *a, **k: ("Z7", True))
    sim.softkey(3)  # 13 FAHREN
    assert sim.dro_labels[2][0].text().strip() == "7.000"
    monkeypatch.setattr(CuttingDataDialog, "exec", lambda self: 0)
    sim.softkey(6)  # 16 SCHNITTDATEN
    sim.softkey(5)  # 15 not simulated
    sim.softkey(8)  # 18 WEITER
    sim.softkey(1)
    assert "nicht nachgebildet" in sim.screen_text.toPlainText()


def test_program_menu_and_aux_modes(sim: SimulatorWindow, monkeypatch: pytest.MonkeyPatch) -> None:
    sim.set_mode(3)
    assert not sim.editor.isReadOnly()
    sim.softkey(4)  # 34 WERKZEUGE
    assert sim.tabs.currentIndex() == 2
    sim.softkey(2)  # 32 UNTERPROGRAMM - none
    assert "Kein Abschnitt" in sim.screen_text.toPlainText()
    sim.softkey(7)  # 37 KONTUR
    assert sim.contour_on and "KONTUR EIN" in sim.mode_display.text()
    sim.softkey(1)  # 31 HAUPTPROGRAMM
    assert sim.editor.textCursor().blockNumber() == 0
    sim.softkey(8)  # 38 WEITER
    for key in (4, 5, 6):
        sim.softkey(key)
    sim.softkey(7)  # 387 DATEN-TRANSFER
    sim.softkey(6)
    assert "KONFIG I/O" in sim.screen_text.toPlainText()
    sim.softkey(8)  # START without NACHLADEN
    sim.softkey(3)  # NACHLADEN
    assert sim.dripfeed_on and "Nachladefunktion" in sim.screen_text.toPlainText()
    sim.softkey(8)  # START
    assert sim.mode == 2 and sim.running and "NACHLADEN" in sim.mode_display.text()
    sim.pause()
    sim.aux_mode("HELP")
    sim.softkey(1)
    assert "387 DATEN-TRANSFER" in sim.screen_text.toPlainText()
    sim.softkey(3)
    assert "2117 START STOP" in sim.screen_text.toPlainText()
    sim.softkey(5)
    assert sim.tabs.currentIndex() == 3
    sim.aux_mode("MODE")
    sim.softkey(3)  # GRAFIK KONFIG
    sim.softkey(1)
    assert sim.view.mode.value == "xy"
    sim.softkey(2)
    sim.softkey(3)
    yaw = sim.view.yaw
    sim.softkey(5)
    assert sim.view.yaw == yaw + 15
    sim.softkey(8)
    sim.softkey(4)
    assert sim.view.zoom > 1
    sim.softkey(5)
    sim.softkey(6)
    assert not sim.show_tool
    sim.softkey(7)
    assert not sim.view.show_rapid
    sim.softkey(8)
    sim.softkey(1)
    assert not sim.graphics_on
    sim.softkey(2)
    assert sim.graphics_on
    sim.aux_mode("GRAF")
    assert not sim.graphics_on
    sim.aux_mode("GRAF")
    sim.aux_mode("INFO")
    sim.softkey(4)
    assert sim.tabs.currentIndex() == 4
    sim.softkey(5)
    assert sim.tabs.currentIndex() == 1
    sim.softkey(3)
    sim.softkey(1)


def test_edit_functions(sim: SimulatorWindow, monkeypatch: pytest.MonkeyPatch) -> None:
    sim.set_mode(3)
    sim.aux_mode("EDIT")
    assert sim.edit_find("X30")
    assert not sim.edit_find("GIBTESNICHT")
    assert sim.edit_replace("X30", "X35") == 1
    assert "X35" in sim.editor.toPlainText()
    cursor = sim.editor.textCursor()
    cursor.movePosition(cursor.MoveOperation.Start)
    sim.editor.setTextCursor(cursor)
    sim.softkey(2)  # copy current line
    cursor = sim.editor.textCursor()
    cursor.movePosition(cursor.MoveOperation.End)
    sim.editor.setTextCursor(cursor)
    sim.softkey(6)  # paste
    assert sim.editor.toPlainText().count("N10 T1") == 2
    sim.softkey(4)  # delete
    sim.softkey(5)  # cut
    monkeypatch.setattr(QInputDialog, "getText", lambda *a, **k: ("N20", True))
    sim.softkey(1)
    sim.softkey(3)  # replace with dialog: "N20" -> "N20"
    sim.set_mode(2)
    sim.aux_mode("EDIT")
    sim.softkey(4)
    assert "nur in Hauptbetriebsart 3" in sim.statusBar().currentMessage()
    assert sim.edit_replace("X", "Y") == 0


def test_dia_and_program_selection(
    qtbot: QtBot, sim: SimulatorWindow, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sim.set_mode(2)
    sim.aux_mode("DIA")

    def fill(self: DiaDialog) -> int:
        self.fields["X"].setText("15,5")
        return 1

    monkeypatch.setattr(DiaDialog, "exec", fill)
    sim.softkey(2)  # G1 GERADE
    assert sim.mode == 3
    assert "N80 G1 X15.5" in sim.editor.toPlainText()
    monkeypatch.setattr(DiaDialog, "exec", lambda self: 0)
    assert sim.dia("DRILL") is None
    other = tmp_path / "OTHER.nc"
    other.write_text("N10 G0 X0 Y0 Z5\nN20 G1 X40 F100\nN30 G1 Y40\nN40 M30\n", encoding="ascii")

    def choose(self: SelectProgramDialog) -> int:
        self.chosen_path = other
        self.chosen_name = "OTHER.nc"
        self.start_block.setText("N30")
        return 1

    monkeypatch.setattr(SelectProgramDialog, "exec", choose)
    sim.set_mode(2)
    sim.softkey(5)  # 25 ANWAHL VORLAUF
    wait_sim(qtbot, sim)
    qtbot.waitUntil(lambda: sim.source_name == "OTHER.nc" and sim.pending_start_block is None, timeout=10000)
    assert sim.playback is not None and sim.time == pytest.approx(sim.playback.time_of_line(3) or -1)
    assert not sim.apply_start_block(999)
    monkeypatch.setattr(SelectProgramDialog, "exec", lambda self: 0)
    assert not sim.select_program()
    monkeypatch.setattr(QFileDialog, "getSaveFileName", lambda *a, **k: ("", ""))
    sim.set_mode(3)
    sim.softkey(8)
    sim.softkey(7)
    sim.softkey(2)  # AUSLESEN (cancelled save)
    sim.softkey(1)  # EINLESEN (cancelled selection)
    assert "EINLESEN" in sim.screen_text.toPlainText()


def test_follow_transfer_display(sim: SimulatorWindow) -> None:
    assert sim.follow_box.isChecked()
    sim.follow_transfer(4)
    assert "ÜBERTRAGEN BIS ZEILE 4" in sim.mode_display.text()
    assert sim.playback is not None and sim.time == pytest.approx(sim.playback.time_until_line(4))
    sim.follow_box.setChecked(False)
    sim.follow_transfer(6)
    assert sim.follow_line == 4


def test_dialogs_directly(qtbot: QtBot, tmp_path: Path) -> None:
    from deckellink.gui.simulator_dialogs import cutting_data
    from deckellink.storage.library import EntryKind, ProgramLibrary

    speed, feed = cutting_data(10, 100, 0.05, 3)
    assert speed == pytest.approx(3183.1, abs=0.1) and feed == pytest.approx(477.5, abs=0.1)
    assert cutting_data(0, 100, 0.1, 2) == (0.0, 0.0)
    dialog = CuttingDataDialog()
    qtbot.addWidget(dialog)
    assert "Drehzahl" in dialog.result_label.text()
    dia = DiaDialog("PATTERN", None)
    qtbot.addWidget(dia)
    dia.fields["G"].setText("89.2")
    dia.fields["R"].setText("20")
    assert dia.block() == "G89.2 R20"
    library = ProgramLibrary(tmp_path / "lib")
    from datetime import UTC, datetime

    library.import_bytes("A.nc", b"N10 G0 X0\r\n", now=datetime(2026, 1, 1, tzinfo=UTC))
    select = SelectProgramDialog(library, (EntryKind.PROGRAM,))
    qtbot.addWidget(select)
    select.list.setCurrentRow(0)
    select.accept()
    assert select.chosen_path is not None and select.chosen_path.name == "A.nc"
    assert select.start_number() is None
    empty = SelectProgramDialog(None, (EntryKind.PROGRAM,), with_start_block=False)
    qtbot.addWidget(empty)
    empty.accept()
    assert empty.chosen_path is None
