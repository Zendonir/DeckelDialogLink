"""Page "Programmverwaltung" and its integration (simulator, send, drip feed)."""

from __future__ import annotations

from pathlib import Path

import pytest
from PySide6.QtWidgets import QMessageBox
from pytestqt.qtbot import QtBot

from deckellink.config import AppConfig
from deckellink.domain.states import SendState
from deckellink.gui.main_window import NAV_ITEMS
from deckellink.storage.library import EntryKind
from deckellink.storage.tooldb import Tool, ToolExportTemplate
from tests.gui.conftest import WindowFactory, wait_idle
from tests.gui.test_simulator_gui import wait_sim

PROGRAM = b"N10 G17 T1 M6; M3\r\nN20 G0 X0 Y0 Z10\r\nN30 G1 Z-2 F600\r\nN40 X30\r\nN50 M30\r\n"


@pytest.fixture
def no_dialogs(monkeypatch: pytest.MonkeyPatch) -> list[str]:
    warnings: list[str] = []
    monkeypatch.setattr(QMessageBox, "warning", lambda *a: warnings.append(str(a[2])))
    monkeypatch.setattr(QMessageBox, "question", lambda *a: QMessageBox.StandardButton.Yes)
    return warnings


def test_library_page_programs(make_window: WindowFactory, tmp_path: Path, no_dialogs: list[str]) -> None:
    window = make_window()
    page = window.library_page
    window.nav.setCurrentRow(len(NAV_ITEMS) - 1)
    assert window.stack.currentIndex() == 8
    source = tmp_path / "FLANSCH.nc"
    source.write_bytes(PROGRAM)
    entry = page.import_path(source)
    assert entry is not None and page.tree.topLevelItemCount() == 1
    assert page.selected() is not None and "T1" in page.tool_check.text() and "Nicht in der" in page.tool_check.text()
    assert page.import_path(tmp_path / "missing.nc") is None and no_dialogs
    page.name_edit.setText("Flansch")
    page.number_edit.setText("17")
    page.description_edit.setText("Erstteil")
    page.kind_combo.setCurrentIndex(page.kind_combo.findData("macro"))
    page.meta_button.click()
    stored = window.library.get(entry.id)
    assert stored is not None and stored.number == "17" and stored.kind is EntryKind.MACRO
    assert not page.drip_button.isEnabled()
    newer = tmp_path / "NEU.nc"
    newer.write_bytes(PROGRAM.replace(b"X30", b"X35"))
    assert page.new_version_from(stored, newer)
    assert page.versions.count() == 1
    page.versions.setCurrentRow(0)
    page.restore_button.click()
    assert window.library.get(entry.id).read() == PROGRAM  # type: ignore[union-attr]
    assert not page.new_version_from(stored, tmp_path / "missing.nc")
    page.select(entry.id)
    page.delete_button.click()
    assert page.tree.topLevelItemCount() == 0 and page.selected() is None


def test_tool_table_and_template(
    qtbot: QtBot, make_window: WindowFactory, tmp_path: Path, no_dialogs: list[str]
) -> None:
    window = make_window()
    page = window.library_page
    page.add_tool_row()
    page.add_tool_row()
    assert page.tool_table.rowCount() == 2
    page.tool_table.item(0, 3).setText("8,5")  # type: ignore[union-attr]
    page.tool_table.item(0, 2).setText("Schaft")  # type: ignore[union-attr]
    page.tool_table.item(1, 2).setText("Bohrer")  # type: ignore[union-attr]
    assert page.save_tools() and len(window.tool_db.tools) == 2
    assert window.tool_db.get(1).diameter == 8.5  # type: ignore[union-attr]
    page.tool_table.item(1, 0).setText("1")  # type: ignore[union-attr]
    assert not page.save_tools() and "Doppelte" in page.tool_status.text()
    page.tool_table.item(1, 3).setText("abc")  # type: ignore[union-attr]
    assert not page.save_tools()
    page.load_tools()
    assert page.tool_table.rowCount() == 2
    page.tool_table.setCurrentCell(1, 0)
    page._remove_tool_row()
    assert page.tool_table.rowCount() == 1
    page.load_tools()
    assert page.export_tools() is None and "Keine Vorlage" in page.tool_status.text()
    page.line_edit.setText("T{T} R{R} L{L}")
    page.template_button.click()
    assert window.config.tool_export_line == "T{T} R{R} L{L}"
    exported = page.export_tools()
    assert exported is not None and exported.kind is EntryKind.TOOLS
    assert exported.read() == b"T1 R4.25 L0\r\nT2 R5 L0\r\n"
    assert "Werkzeugdaten überschrieben" in page.tool_check_text(window.library.get(exported.id))  # type: ignore[arg-type]
    # sending a tool file shows the extra warnings in the confirmation
    window.send_library_entry(exported.id, False)
    window.transfer_page.ack_warnings.setChecked(True)  # tool file has no M30 -> warning must be acknowledged
    window.start_send()
    notes = window.confirmations[-1].warnings
    assert any("WERKZEUGDATEI" in n for n in notes) and any("NICHT verifiziert" in n for n in notes)
    wait_idle(qtbot, window)


def test_send_drip_feed_and_simulator_from_library(
    qtbot: QtBot, make_window: WindowFactory, tmp_path: Path, no_dialogs: list[str]
) -> None:
    window = make_window(AppConfig(active_profile_id="dialog11_lsv2_7e1"))
    window.tool_db.save([Tool(1, diameter=6)])
    source = tmp_path / "TEIL.nc"
    source.write_bytes(PROGRAM)
    entry = window.library_page.import_path(source)
    assert entry is not None
    sim = window.simulate_library_entry(entry.id, True)
    assert sim is not None
    wait_sim(qtbot, sim)
    assert sim.mode == 3 and sim.save_library_button.isVisible()
    assert sim.tool_table.item(0, 1).text() == "3"  # type: ignore[union-attr]
    sim.editor.setPlainText(PROGRAM.decode().replace("X30", "X40"))
    assert sim.save_to_library()
    assert b"X40" in window.library.get(entry.id).read()  # type: ignore[union-attr]
    viewer = window.simulate_library_entry(entry.id, False)
    assert viewer is not None and viewer.save_callback is None
    wait_sim(qtbot, viewer)
    assert window.simulate_library_entry("gibt-es-nicht", False) is None
    # drip feed straight from the program management (simulated LSV2 CNC)
    window.send_library_entry(entry.id, True)
    assert window.confirmations and window.confirmations[-1].drip_feed
    wait_idle(qtbot, window, timeout=30000)
    assert window.transfer_page.state.kind is not None
    stored = window.library.get(entry.id)
    assert stored is not None and stored.last_sent_mode == "Simulator"
    assert viewer.follow_line is not None  # the simulator followed the transmitted lines
    window.send_library_entry("gibt-es-nicht", False)
    for w in (sim, viewer):
        w.close()


def test_drip_feed_needs_valid_file(make_window: WindowFactory, tmp_path: Path, no_dialogs: list[str]) -> None:
    window = make_window(AppConfig(active_profile_id="dialog11_lsv2_7e1"))
    bad = tmp_path / "BAD.nc"
    bad.write_bytes("N10 G0 XÄ\r\n".encode("latin-1"))
    entry = window.library_page.import_path(bad)
    assert entry is not None
    window.send_library_entry(entry.id, True)
    assert not window.confirmations
    assert "Prüffehler" in window.statusBar().currentMessage()


def test_template_from_config(make_window: WindowFactory) -> None:
    window = make_window(AppConfig(tool_export_line="T{T}"))
    assert window.library_page.current_template() == ToolExportTemplate("", "T{T}", "")
    assert SendState.COMPLETED.label
