"""DIALOG 11 simulator window (offline only)."""

from __future__ import annotations

from pathlib import Path

import pytest
from PySide6.QtCore import QEvent, QPoint, QPointF, Qt
from PySide6.QtGui import QMouseEvent, QWheelEvent
from PySide6.QtWidgets import QTableWidget, QTableWidgetItem
from pytestqt.qtbot import QtBot

from deckellink.gui.simulator_window import SimulatorWindow
from deckellink.gui.widgets.toolpath_view import ToolpathView, ViewMode
from tests.gui.conftest import WindowFactory

PROGRAM = "N10 T1\nN20 G0 X0 Y0 Z10\nN30 G1 Z-2 F600\nN40 X30\nN50 Y30\nN60 G81 OA0 TI5 SI2 EA5\nN70 M30\n"


def wait_sim(qtbot: QtBot, window: SimulatorWindow) -> None:
    qtbot.waitUntil(lambda: window.result is not None and window._thread is None, timeout=10000)


@pytest.fixture
def sim(qtbot: QtBot) -> SimulatorWindow:
    window = SimulatorWindow(PROGRAM, "TEST.nc")
    qtbot.addWidget(window)
    window.show()
    wait_sim(qtbot, window)
    return window


def test_demo_window_runs(qtbot: QtBot) -> None:
    window = SimulatorWindow()
    qtbot.addWidget(window)
    window.show()
    wait_sim(qtbot, window)
    assert window.result is not None and not window.result.errors
    assert "Demoprogramm" in window.summary_label.text()
    assert "SIMULATION" in window.mode_display.text()
    window.to_end()
    assert window.view.done_moves == len(window.result.moves)
    assert window.view.grab().width() > 0


def test_playback_controls_and_displays(qtbot: QtBot, sim: SimulatorWindow) -> None:
    assert sim.playback is not None
    sim.softkey(2)  # single block
    assert sim.running
    while sim.running:
        sim.advance(0.5)
    first_stop = sim.time
    assert 0 < first_stop < sim.playback.total
    sim.softkey(1)
    sim.advance(1.0)
    sim.softkey(3)
    assert not sim.running
    sim.softkey(5)
    assert sim.time == pytest.approx(sim.playback.total)
    assert sim.dro_labels[0][0].text().strip() == "30.000"
    assert sim.dro_labels[2][0].text().strip() == "5.000"
    assert any(
        sim.modal_tree.topLevelItem(i).text(0) == "Wegbedingung" for i in range(sim.modal_tree.topLevelItemCount())
    )
    assert "Zeile" in sim.block_label.text()
    sim.softkey(4)
    assert sim.time == 0
    sim._slider_moved(500)
    assert sim.time == pytest.approx(sim.playback.total / 2)
    sim.start()
    sim._tick()
    sim.pause()
    sim.editor.setTextCursor(sim.editor.textCursor())
    cursor = sim.editor.textCursor()
    cursor.movePosition(cursor.MoveOperation.Start)
    cursor.movePosition(cursor.MoveOperation.Down, n=1)
    sim.editor.setTextCursor(cursor)
    assert sim.time == pytest.approx(sim.playback.time_of_line(2) or 0)


def test_view_keys_and_modes(qtbot: QtBot, sim: SimulatorWindow) -> None:
    start = sim.view.mode
    sim.softkey(6)
    assert sim.view.mode is not start
    sim.softkey(7)
    assert sim.view.zoom == 1.0
    sim.softkey(8)
    assert sim.tabs.currentIndex() == 3
    sim.set_mode(1)
    assert sim.editor.isReadOnly() and "HANDFAHREN" in sim.mode_display.text()
    before = sim.manual_position
    sim.softkey(2)  # X+
    sim.softkey(5)  # Z-
    assert sim.manual_position[0] == pytest.approx(before[0] + 10)
    assert sim.manual_position[2] == pytest.approx(before[2] - 10)
    sim.softkey(7)
    assert sim.jog_step.currentIndex() == 4
    sim.softkey(8)
    sim.set_mode(3)
    assert not sim.editor.isReadOnly()
    sim.editor.setPlainText(PROGRAM.replace("X30", "X50"))

    def widened() -> bool:
        bounds = sim.result.bounds() if sim.result is not None else None
        return bounds is not None and bounds[1][0] == pytest.approx(50)

    qtbot.waitUntil(widened, timeout=10000)
    sim.softkey(5)
    assert sim.tabs.currentIndex() == 1
    sim.softkey(2)  # demo
    wait_sim(qtbot, sim)
    assert sim.source_name == "Demoprogramm"
    sim.softkey(6)
    sim.set_mode(2)
    sim.softkey(6)
    sim.softkey(7)


def test_messages_and_setup(qtbot: QtBot) -> None:
    window = SimulatorWindow("N10 G2 X5 Y5\nN20 T3\nN30 G41 X0 Y0\nN40 G1 X10 F100\nN50 G40\nN60 M30", "x")
    qtbot.addWidget(window)
    wait_sim(qtbot, window)
    assert window.messages.count() >= 1
    window._message_clicked(window.messages.item(0))
    assert "Meldungen (" in window.tabs.tabText(1)
    _item(window.tool_table, 2, 1).setText("7,5")
    _item(window.tool_table, 0, 0).setText("x")
    _item(window.zero_table, 0, 1).setText("ungültig")
    window.parameter_edit.setPlainText("P1=10\nkein parameter\nPP2 = (P1 x 2)")
    config = window.config()
    assert config.tools[3].radius == 7.5 and 1 not in config.tools
    assert config.parameter_table == {"P1": "10", "PP2": "(P1 x 2)"}
    assert 1 not in config.zero_points
    window.resimulate()
    window.resimulate()  # second request is queued while the first runs
    wait_sim(qtbot, window)
    assert window.result is not None
    window._toggle_rapid(False)
    assert not window.view.show_rapid


def test_save_as_never_overwrites(
    qtbot: QtBot, sim: SimulatorWindow, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from PySide6.QtWidgets import QMessageBox

    warnings: list[str] = []
    monkeypatch.setattr(QMessageBox, "warning", lambda *args: warnings.append(str(args[2])))
    target = tmp_path / "NEU.nc"
    assert sim.save_to(target)
    assert target.read_bytes().startswith(b"N10 T1\r\n")
    assert not sim.save_to(target)
    assert "existiert bereits" in warnings[-1]
    sim.editor.setPlainText("N10 Ä")
    assert not sim.save_to(tmp_path / "UMLAUT.nc")
    sim.load_path(tmp_path / "missing.nc")
    assert len(warnings) == 3
    sim.load_path(target)
    wait_sim(qtbot, sim)
    assert sim.source_name == "NEU.nc"


def test_toolpath_view_interaction(qtbot: QtBot, sim: SimulatorWindow) -> None:

    view: ToolpathView = sim.view
    for mode in ViewMode:
        view.set_mode(mode)
        view.repaint()
        assert view.to_screen((0.0, 0.0, 0.0)) is not None
    view.set_mode(ViewMode.ISO)
    yaw = view.yaw
    view.mousePressEvent(_mouse(view, QPointF(50, 50), Qt.MouseButton.RightButton))
    view.mouseMoveEvent(_mouse(view, QPointF(80, 60), Qt.MouseButton.RightButton))
    assert view.yaw != yaw
    view.mousePressEvent(_mouse(view, QPointF(50, 50), Qt.MouseButton.LeftButton))
    view.mouseMoveEvent(_mouse(view, QPointF(60, 60), Qt.MouseButton.LeftButton))
    view.mouseReleaseEvent(_mouse(view, QPointF(60, 60), Qt.MouseButton.LeftButton))
    assert view.pan != QPointF(0, 0)
    zoom = view.zoom
    view.wheelEvent(
        QWheelEvent(
            QPointF(10, 10),
            QPointF(10, 10),
            QPoint(0, 0),
            QPoint(0, 120),
            Qt.MouseButton.NoButton,
            Qt.KeyboardModifier.NoModifier,
            Qt.ScrollPhase.NoScrollPhase,
            False,
        )
    )
    assert view.zoom > zoom
    view.mouseDoubleClickEvent(_mouse(view, QPointF(5, 5), Qt.MouseButton.LeftButton))
    assert view.zoom == 1.0
    view.set_bounds((0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    view.manual_trace = [(0.0, 0.0, 0.0), (5.0, 5.0, 0.0)]
    view.set_tool((1.0, 1.0, 1.0), 2.0, 1)
    for mode in (ViewMode.XY, ViewMode.XZ):
        view.set_mode(mode)
        view.repaint()
    view.set_tool((1.0, 1.0, 1.0), 2.0, 0)
    view.set_result(None)
    view.repaint()


def _mouse(view: ToolpathView, pos: QPointF, button: Qt.MouseButton) -> QMouseEvent:
    del view
    return QMouseEvent(QEvent.Type.MouseButtonPress, pos, pos, button, button, Qt.KeyboardModifier.NoModifier)


def _item(table: QTableWidget, row: int, col: int) -> QTableWidgetItem:
    item = table.item(row, col)
    assert item is not None
    return item


def test_main_window_opens_simulator(make_window: WindowFactory, tmp_path: Path, qtbot: QtBot) -> None:
    window = make_window()
    demo = window.open_simulator()
    wait_sim(qtbot, demo)
    assert demo.source_name == "Demoprogramm"
    nc = tmp_path / "TEIL.nc"
    nc.write_bytes(PROGRAM.encode("ascii"))
    window.load_file(nc)
    opened = window.open_simulator()
    wait_sim(qtbot, opened)
    assert opened.source_name == "TEIL.nc" and opened.editor.toPlainText().startswith("N10 T1")
    assert nc.read_bytes() == PROGRAM.encode("ascii")  # original never touched
    assert len(window.simulator_windows) == 2
    for w in (demo, opened):
        w.close()
