"""DIALOG 11 graphic simulator window (offline, never connected to a machine)."""

from __future__ import annotations

import logging
from dataclasses import dataclass
from pathlib import Path

from PySide6.QtCore import Qt, QThread, QTimer, Signal
from PySide6.QtGui import QColor, QFont, QKeySequence, QShortcut, QTextCursor, QTextFormat
from PySide6.QtWidgets import (
    QButtonGroup,
    QComboBox,
    QDoubleSpinBox,
    QFileDialog,
    QFormLayout,
    QGridLayout,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QListWidget,
    QListWidgetItem,
    QMainWindow,
    QMessageBox,
    QPlainTextEdit,
    QPushButton,
    QSlider,
    QSplitter,
    QTableWidget,
    QTableWidgetItem,
    QTabWidget,
    QTextEdit,
    QTreeWidget,
    QTreeWidgetItem,
    QVBoxLayout,
    QWidget,
)

from deckellink.dialog11sim.geometry import Vec3
from deckellink.dialog11sim.interpreter import simulate
from deckellink.dialog11sim.model import Severity, SimConfig, SimResult, ToolData
from deckellink.dialog11sim.playback import Frame, Playback, format_duration
from deckellink.dialog11sim.reference import DEMO_PARAMETERS, DEMO_PROGRAM, FUNCTION_REFERENCE
from deckellink.gui.style import SIMULATION_BANNER_STYLE
from deckellink.gui.widgets.toolpath_view import ToolpathView
from deckellink.storage.files import FileExistsConflict, write_new_file

log = logging.getLogger(__name__)

SPEEDS: tuple[tuple[str, float], ...] = (
    ("Echtzeit 1x", 1.0),
    ("2x", 2.0),
    ("5x", 5.0),
    ("10x", 10.0),
    ("50x", 50.0),
    ("200x", 200.0),
    ("1000x", 1000.0),
)
TICK_MS = 40
JOG_STEPS = ("0.01", "0.1", "1", "10", "100")
MONO = QFont("Consolas")
MONO.setStyleHint(QFont.StyleHint.Monospace)

SOFTKEYS: dict[int, tuple[str, ...]] = {
    1: ("X −", "X +", "Y −", "Y +", "Z −", "Z +", "SCHRITT", "INFO"),
    2: ("START", "EINZELSATZ", "HALT", "ANFANG", "ENDE", "ANSICHT", "EINPASSEN", "INFO"),
    3: ("GRAFIK\nNEU", "DEMO", "DATEI\nÖFFNEN", "SPEICHERN\nUNTER", "PRÜFEN", "ANSICHT", "EINPASSEN", "INFO"),
}
MODE_NAMES = {1: "1 HANDFAHREN", 2: "2 AUTOMATIK", 3: "3 PROGRAMM"}


@dataclass
class _Job:
    text: str
    config: SimConfig


class _SimulationThread(QThread):
    done = Signal(object)

    def __init__(self, job: _Job) -> None:
        super().__init__()
        self.job = job

    def run(self) -> None:
        try:
            result = simulate(self.job.text, self.job.config)
        except Exception as exc:  # the simulator must never take the application down
            log.exception("Simulation failed")
            result = SimResult(end_reason=f"Interner Fehler der Simulation: {exc}")
        self.done.emit(result)


class SimulatorWindow(QMainWindow):
    """Offline training and checking tool. It has no access to any port or transport."""

    simulation_finished = Signal()

    def __init__(self, text: str | None = None, name: str = "", parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle("DeckelLink - DIALOG-11-Simulator (offline)")
        self.resize(1360, 860)
        self.mode = 2
        self.result: SimResult | None = None
        self.playback: Playback | None = None
        self.time = 0.0
        self.running = False
        self.stop_at: float | None = None
        self.manual_position: Vec3 = (0.0, 0.0, 100.0)
        self._thread: _SimulationThread | None = None
        self._threads: list[_SimulationThread] = []  # keep Python references until Qt reports "finished"
        self._pending: _Job | None = None
        self.source_name = name or ("Demoprogramm" if text is None else "Programm")
        self.timer = QTimer(self)
        self.timer.setInterval(TICK_MS)
        self.timer.timeout.connect(self._tick)
        self.edit_timer = QTimer(self)
        self.edit_timer.setSingleShot(True)
        self.edit_timer.setInterval(700)
        self.edit_timer.timeout.connect(self.resimulate)
        self._build()
        self.parameter_edit.setPlainText(
            "\n".join(f"{k}={v}" for k, v in DEMO_PARAMETERS.items()) if text is None else ""
        )
        self.editor.setPlainText(DEMO_PROGRAM if text is None else text)
        self.editor.textChanged.connect(self._text_changed)
        self.set_mode(2)
        self.resimulate()

    # ------------------------------------------------------------------ layout
    def _build(self) -> None:
        central = QWidget()
        root = QVBoxLayout(central)
        banner = QLabel("SIMULATION DIALOG 11 - offline, keine Verbindung zur Maschine. Nur Darstellung, keine Gewähr.")
        banner.setStyleSheet(SIMULATION_BANNER_STYLE)
        root.addWidget(banner)

        top = QHBoxLayout()
        self.mode_group = QButtonGroup(self)
        self.mode_buttons: dict[int, QPushButton] = {}
        for number, label in MODE_NAMES.items():
            button = QPushButton(label)
            button.setCheckable(True)
            button.setMinimumHeight(34)
            self.mode_group.addButton(button, number)
            self.mode_buttons[number] = button
            top.addWidget(button)
        self.mode_group.idClicked.connect(self.set_mode)
        self.mode_display = QLabel()
        self.mode_display.setStyleSheet(
            "background:#0f1b2d;color:#ffd43b;font-weight:bold;padding:6px 10px;border:1px solid #495057;"
        )
        top.addWidget(self.mode_display, 1)
        root.addLayout(top)

        splitter = QSplitter()
        # left: program listing / editor
        left = QWidget()
        left_layout = QVBoxLayout(left)
        left_layout.setContentsMargins(0, 0, 0, 0)
        self.listing_label = QLabel()
        left_layout.addWidget(self.listing_label)
        self.editor = QPlainTextEdit()
        self.editor.setFont(MONO)
        self.editor.setLineWrapMode(QPlainTextEdit.LineWrapMode.NoWrap)
        self.editor.cursorPositionChanged.connect(self._cursor_moved)
        left_layout.addWidget(self.editor)
        splitter.addWidget(left)
        # centre: graphics
        middle = QWidget()
        middle_layout = QVBoxLayout(middle)
        middle_layout.setContentsMargins(0, 0, 0, 0)
        self.view = ToolpathView()
        middle_layout.addWidget(self.view, 1)
        view_row = QHBoxLayout()
        self.view_combo = QComboBox()
        for mode in self.view.mode.__class__:
            self.view_combo.addItem(mode.label, mode)
        self.view_combo.setCurrentIndex(3)
        self.view_combo.currentIndexChanged.connect(lambda i: self.view.set_mode(self.view_combo.itemData(i)))
        view_row.addWidget(QLabel("Ansicht:"))
        view_row.addWidget(self.view_combo)
        self.rapid_box = QPushButton("Eilgang anzeigen")
        self.rapid_box.setCheckable(True)
        self.rapid_box.setChecked(True)
        self.rapid_box.toggled.connect(self._toggle_rapid)
        view_row.addWidget(self.rapid_box)
        view_row.addStretch(1)
        middle_layout.addLayout(view_row)
        splitter.addWidget(middle)
        # right: displays
        self.tabs = QTabWidget()
        self.tabs.addTab(self._build_display_tab(), "Anzeige")
        self.messages = QListWidget()
        self.messages.itemDoubleClicked.connect(self._message_clicked)
        self.tabs.addTab(self.messages, "Meldungen")
        self.tabs.addTab(self._build_setup_tab(), "Werkzeuge / Nullpunkte")
        self.tabs.addTab(self._build_info_tab(), "INFO G/M")
        splitter.addWidget(self.tabs)
        splitter.setSizes([360, 620, 380])
        root.addWidget(splitter, 1)

        # playback row
        play = QHBoxLayout()
        self.time_slider = QSlider(Qt.Orientation.Horizontal)
        self.time_slider.setRange(0, 1000)
        self.time_slider.sliderMoved.connect(self._slider_moved)
        play.addWidget(self.time_slider, 1)
        self.time_label = QLabel("0:00:00 / 0:00:00")
        self.time_label.setFont(MONO)
        play.addWidget(self.time_label)
        play.addWidget(QLabel("Tempo:"))
        self.speed_combo = QComboBox()
        for label, factor in SPEEDS:
            self.speed_combo.addItem(label, factor)
        self.speed_combo.setCurrentIndex(3)
        play.addWidget(self.speed_combo)
        self.jog_step = QComboBox()
        self.jog_step.addItems(JOG_STEPS)
        self.jog_step.setCurrentIndex(3)
        play.addWidget(QLabel("Schritt (mm):"))
        play.addWidget(self.jog_step)
        root.addLayout(play)

        # softkeys F1..F8
        keys = QHBoxLayout()
        self.softkeys: list[QPushButton] = []
        for index in range(8):
            button = QPushButton()
            button.setMinimumHeight(50)
            button.setStyleSheet("font-weight:bold;")
            button.clicked.connect(lambda _c=False, i=index: self.softkey(i + 1))
            keys.addWidget(button)
            self.softkeys.append(button)
            QShortcut(QKeySequence(f"F{index + 1}"), self, lambda i=index: self.softkey(i + 1))
        root.addLayout(keys)
        self.setCentralWidget(central)
        self.statusBar().showMessage("Bereit")

    def _build_display_tab(self) -> QWidget:
        page = QWidget()
        layout = QVBoxLayout(page)
        dro = QGridLayout()
        dro.addWidget(QLabel("<b>Achse</b>"), 0, 0)
        dro.addWidget(QLabel("<b>Istwert (Werkstück)</b>"), 0, 1)
        dro.addWidget(QLabel("<b>Restweg</b>"), 0, 2)
        self.dro_labels: list[tuple[QLabel, QLabel]] = []
        big = QFont(MONO)
        big.setPointSize(max(big.pointSize(), 10) + 4)
        big.setBold(True)
        for row, axis in enumerate(("X", "Y", "Z", "C"), start=1):
            name = QLabel(axis)
            name.setFont(big)
            value = QLabel("0.000")
            value.setFont(big)
            value.setStyleSheet("background:#0f1b2d;color:#69db7c;padding:2px 8px;")
            rest = QLabel("")
            rest.setFont(MONO)
            dro.addWidget(name, row, 0)
            dro.addWidget(value, row, 1)
            dro.addWidget(rest, row, 2)
            self.dro_labels.append((value, rest))
        layout.addLayout(dro)
        self.tech_label = QLabel()
        self.tech_label.setFont(MONO)
        layout.addWidget(self.tech_label)
        self.block_label = QLabel()
        self.block_label.setFont(MONO)
        self.block_label.setWordWrap(True)
        self.block_label.setStyleSheet("background:#0f1b2d;color:#e9ecef;padding:4px;")
        layout.addWidget(self.block_label)
        layout.addWidget(QLabel("<b>Modalliste</b> (wirksame Befehle)"))
        self.modal_tree = QTreeWidget()
        self.modal_tree.setHeaderLabels(["Gruppe", "Wirksam"])
        self.modal_tree.setRootIsDecorated(False)
        self.modal_tree.header().setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        layout.addWidget(self.modal_tree, 1)
        self.summary_label = QLabel()
        self.summary_label.setWordWrap(True)
        layout.addWidget(self.summary_label)
        return page

    def _build_setup_tab(self) -> QWidget:
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.addWidget(QLabel("Werkzeugtabelle des Simulators (Radius für Bahnkorrektur und Zyklen):"))
        self.tool_table = QTableWidget(8, 3)
        self.tool_table.setHorizontalHeaderLabels(["T-Nr.", "Radius mm", "Länge mm"])
        self.tool_table.horizontalHeader().setSectionResizeMode(QHeaderView.ResizeMode.Stretch)
        for row in range(8):
            for col, value in enumerate((str(row + 1), "5", "0")):
                self.tool_table.setItem(row, col, QTableWidgetItem(value))
        layout.addWidget(self.tool_table)
        layout.addWidget(QLabel("Nullpunktspeicher G51.n (Werte relativ zu G51.0):"))
        self.zero_table = QTableWidget(5, 4)
        self.zero_table.setHorizontalHeaderLabels(["n", "X", "Y", "Z"])
        self.zero_table.horizontalHeader().setSectionResizeMode(QHeaderView.ResizeMode.Stretch)
        for row in range(5):
            for col, value in enumerate((str(row + 1), "0", "0", "0")):
                self.zero_table.setItem(row, col, QTableWidgetItem(value))
        layout.addWidget(self.zero_table)
        layout.addWidget(QLabel("Parametertabelle (je Zeile z. B. P1=10, PP2=(P1+5), P3=%*1):"))
        self.parameter_edit = QPlainTextEdit()
        self.parameter_edit.setFont(MONO)
        self.parameter_edit.setMaximumHeight(110)
        layout.addWidget(self.parameter_edit)
        form = QFormLayout()
        self.rapid_spin = QDoubleSpinBox()
        self.rapid_spin.setRange(100, 60000)
        self.rapid_spin.setValue(5000)
        self.rapid_spin.setSuffix(" mm/min")
        self.rapid_spin.setToolTip("Annahme für die Laufzeitschätzung - Eilgang der FP2A nicht hinterlegt.")
        form.addRow("Eilgang (Annahme):", self.rapid_spin)
        self.start_spins: list[QDoubleSpinBox] = []
        start_row = QHBoxLayout()
        for start_value in (0.0, 0.0, 100.0):
            spin = QDoubleSpinBox()
            spin.setRange(-5000, 5000)
            spin.setValue(start_value)
            self.start_spins.append(spin)
            start_row.addWidget(spin)
        form.addRow("Startposition X/Y/Z:", start_row)
        layout.addLayout(form)
        apply = QPushButton("Übernehmen und neu simulieren")
        apply.clicked.connect(self.resimulate)
        layout.addWidget(apply)
        layout.addStretch(1)
        return page

    def _build_info_tab(self) -> QWidget:
        text = QTextEdit()
        text.setReadOnly(True)
        rows = "".join(
            f"<tr><td><b>{code}</b></td><td>{meaning}</td><td>{support}</td></tr>"
            for code, meaning, support in FUNCTION_REFERENCE
        )
        text.setHtml(
            "<p>Übersicht der vom Simulator ausgewerteten Befehle (eigene Kurzbeschreibungen nach den "
            "DECKEL-Trainingsunterlagen). Die Simulation ersetzt keine Prüfung an der Maschine.</p>"
            f"<table border='1' cellspacing='0' cellpadding='3'><tr><th>Befehl</th><th>Bedeutung</th>"
            f"<th>Simulator</th></tr>{rows}</table>"
        )
        return text

    # ------------------------------------------------------------------ configuration
    def config(self) -> SimConfig:
        tools: dict[int, ToolData] = {}
        for row in range(self.tool_table.rowCount()):
            try:
                number = int(_cell(self.tool_table, row, 0))
                tools[number] = ToolData(
                    radius=float(_cell(self.tool_table, row, 1).replace(",", ".")),
                    length=float(_cell(self.tool_table, row, 2).replace(",", ".") or 0),
                )
            except ValueError:
                continue
        zeros: dict[int, Vec3] = {}
        for row in range(self.zero_table.rowCount()):
            try:
                n = int(_cell(self.zero_table, row, 0))
                x, y, z = (float(_cell(self.zero_table, row, c).replace(",", ".") or 0) for c in (1, 2, 3))
                zeros[n] = (x, y, z)
            except ValueError:
                continue
        table: dict[str, str] = {}
        for line in self.parameter_edit.toPlainText().splitlines():
            if "=" in line:
                key, value = line.split("=", 1)
                key = key.strip().upper()
                if key.startswith("P"):
                    table[key] = value.strip()
        start = (self.start_spins[0].value(), self.start_spins[1].value(), self.start_spins[2].value())
        return SimConfig(
            rapid_feed=self.rapid_spin.value(),
            start_position=start,
            tools=tools,
            zero_points=zeros,
            parameter_table=table,
        )

    # ------------------------------------------------------------------ simulation
    def resimulate(self) -> None:
        job = _Job(self.editor.toPlainText(), self.config())
        if self._thread is not None and self._thread.isRunning():
            self._pending = job
            return
        self.statusBar().showMessage("Simulation läuft …")
        self._start_thread(job)

    def _start_thread(self, job: _Job) -> None:
        thread = _SimulationThread(job)
        thread.done.connect(self._simulation_done)
        thread.finished.connect(lambda t=thread: self._thread_finished(t))
        self._threads.append(thread)
        self._thread = thread
        thread.start()

    def _thread_finished(self, thread: _SimulationThread) -> None:
        if thread in self._threads:
            self._threads.remove(thread)
        thread.deleteLater()

    def _simulation_done(self, result: SimResult) -> None:
        self._thread = None
        if self._pending is not None:
            job, self._pending = self._pending, None
            self._start_thread(job)
            return
        self.result = result
        self.playback = Playback(result, self.config().start_position)
        self.view.set_result(result)
        self.time = min(self.time, self.playback.total) if self.mode == 2 else 0.0
        self._fill_messages(result)
        errors, warnings = len(result.errors), len(result.warnings)
        self.summary_label.setText(
            f"<b>{self.source_name}</b>: {result.executed_blocks} Sätze ausgeführt, {len(result.moves)} Bewegungen, "
            f"Vorschubweg {result.feed_length:.0f} mm, Eilgang {result.rapid_length:.0f} mm, "
            f"Laufzeit geschätzt {format_duration(result.total_time_s)}.<br>Ende: {result.end_reason or '-'}"
            f"<br>{errors} Fehler, {warnings} Warnungen."
        )
        self.tabs.setTabText(1, f"Meldungen ({errors + warnings})" if errors + warnings else "Meldungen")
        self.statusBar().showMessage(
            f"Simulation fertig: {len(result.moves)} Bewegungen, {errors} Fehler, {warnings} Warnungen"
        )
        self._show_frame()
        self.simulation_finished.emit()

    def _fill_messages(self, result: SimResult) -> None:
        self.messages.clear()
        for message in result.messages:
            item = QListWidgetItem(str(message))
            item.setData(256, message.line)
            if message.severity is Severity.ERROR:
                item.setForeground(QColor("#b3261e"))
            elif message.severity is Severity.WARNING:
                item.setForeground(QColor("#8a6100"))
            self.messages.addItem(item)

    def _text_changed(self) -> None:
        if self.mode == 3:
            self.edit_timer.start()

    # ------------------------------------------------------------------ modes and softkeys
    def set_mode(self, mode: int) -> None:
        self.mode = mode
        self.mode_buttons[mode].setChecked(True)
        self.pause()
        self.editor.setReadOnly(mode != 3)
        self.listing_label.setText(
            {
                1: "Handfahren: Achsen des Simulators mit F1-F6 verfahren (keine Maschine!)",
                2: "Programm (Automatik/Testlauf) - aktueller Satz ist markiert",
                3: "Programm bearbeiten - die Grafik wird nach Änderungen neu berechnet. "
                "Die Originaldatei bleibt unverändert.",
            }[mode]
        )
        for index, label in enumerate(SOFTKEYS[mode]):
            self.softkeys[index].setText(f"F{index + 1}\n{label}")
        self.jog_step.setEnabled(mode == 1)
        if mode == 1:
            frame = self._current_frame()
            self.manual_position = frame.position if frame is not None else self.manual_position
            self.view.manual_trace = [self.manual_position]
            self.view.set_tool(self.manual_position, self.view.tool_radius, self.view.done_moves)
        else:
            self.view.manual_trace = []
        self._update_mode_display()
        self._show_frame()

    def _update_mode_display(self) -> None:
        state = "LÄUFT" if self.running else "HALT"
        extra = {1: f"Schritt {self.jog_step.currentText()} mm", 2: f"TESTLAUF {state}", 3: "EDITIEREN"}[self.mode]
        self.mode_display.setText(f"{MODE_NAMES[self.mode]}  ·  {extra}  ·  GRAFIK EIN  ·  SIMULATION")

    def softkey(self, number: int) -> None:
        if self.mode == 1:
            self._softkey_manual(number)
        elif self.mode == 2:
            self._softkey_auto(number)
        else:
            self._softkey_program(number)

    def _softkey_auto(self, number: int) -> None:
        if number == 1:
            self.start()
        elif number == 2:
            self.single_block()
        elif number == 3:
            self.pause()
        elif number == 4:
            self.rewind()
        elif number == 5:
            self.to_end()
        else:
            self._common_key(number)

    def _softkey_program(self, number: int) -> None:
        if number == 1:
            self.resimulate()
        elif number == 2:
            self.load_text(DEMO_PROGRAM, "Demoprogramm")
            self.parameter_edit.setPlainText("\n".join(f"{k}={v}" for k, v in DEMO_PARAMETERS.items()))
        elif number == 3:
            self.open_file()
        elif number == 4:
            self.save_as()
        elif number == 5:
            self.tabs.setCurrentIndex(1)
            self.resimulate()
        else:
            self._common_key(number)

    def _softkey_manual(self, number: int) -> None:
        if number <= 6:
            axis = (number - 1) // 2
            sign = 1.0 if number % 2 == 0 else -1.0
            self.jog(axis, sign * float(self.jog_step.currentText()))
        elif number == 7:
            self.jog_step.setCurrentIndex((self.jog_step.currentIndex() + 1) % self.jog_step.count())
            self._update_mode_display()
        else:
            self.tabs.setCurrentIndex(3)

    def _common_key(self, number: int) -> None:
        if number == 6:
            mode = self.view.cycle_mode()
            self.view_combo.blockSignals(True)
            self.view_combo.setCurrentIndex(self.view_combo.findData(mode))
            self.view_combo.blockSignals(False)
        elif number == 7:
            self.view.fit()
        elif number == 8:
            self.tabs.setCurrentIndex(3)

    # ------------------------------------------------------------------ playback
    def start(self) -> None:
        if self.playback is None:
            return
        if self.time >= self.playback.total:
            self.time = 0.0
        self.stop_at = None
        self.running = True
        self.timer.start()
        self._update_mode_display()

    def single_block(self) -> None:
        if self.playback is None:
            return
        if self.time >= self.playback.total:
            self.time = 0.0
        self.stop_at = self.playback.next_block_end(self.time)
        self.running = True
        self.timer.start()
        self._update_mode_display()

    def pause(self) -> None:
        self.running = False
        self.timer.stop()
        if hasattr(self, "mode_display"):
            self._update_mode_display()

    def rewind(self) -> None:
        self.pause()
        self.time = 0.0
        self._show_frame()

    def to_end(self) -> None:
        self.pause()
        if self.playback is not None:
            self.time = self.playback.total
        self._show_frame()

    def advance(self, seconds: float) -> None:
        """Advance simulation time (used by the timer and by tests)."""
        if self.playback is None:
            return
        self.time = min(self.time + seconds, self.playback.total)
        if self.stop_at is not None and self.time >= self.stop_at:
            self.time = self.stop_at
            self.stop_at = None
            self.pause()
        if self.time >= self.playback.total:
            self.pause()
        self._show_frame()

    def _tick(self) -> None:
        factor = float(self.speed_combo.currentData() or 1.0)
        self.advance(TICK_MS / 1000.0 * factor)

    def _slider_moved(self, value: int) -> None:
        if self.playback is not None:
            self.pause()
            self.time = self.playback.total * value / 1000.0
            self._show_frame()

    def _current_frame(self) -> Frame | None:
        if self.playback is None:
            return None
        return self.playback.frame(self.time)

    def _show_frame(self) -> None:
        frame = self._current_frame()
        if frame is None or self.playback is None:
            return
        if self.mode == 1:
            self._show_position(self.manual_position, (0.0, 0.0, 0.0), frame)
            return
        radius = frame.snapshot.tool_radius
        done = frame.move_index if frame.move_index >= 0 else 0
        if frame.finished:
            done = len(self.playback.result.moves)
        self.view.set_tool(frame.position, radius, done)
        self._show_position(frame.position, frame.remaining, frame)
        total = self.playback.total
        self.time_slider.blockSignals(True)
        self.time_slider.setValue(int(1000 * self.time / total) if total > 0 else 0)
        self.time_slider.blockSignals(False)
        self.time_label.setText(f"{format_duration(self.time)} / {format_duration(total)}")
        self._highlight_line(frame.line)

    def _show_position(self, position: Vec3, remaining: Vec3, frame: Frame) -> None:
        for axis in range(3):
            value, rest = self.dro_labels[axis]
            value.setText(f"{position[axis]:10.3f}")
            rest.setText(f"{remaining[axis]:9.3f}")
        self.dro_labels[3][0].setText(f"{frame.snapshot.c_axis:10.3f}")
        snap = frame.snapshot
        self.tech_label.setText(
            f"F {snap.feed:8.1f}   S {snap.speed:7.0f}   {snap.tool} (R {snap.tool_radius:g})   {snap.spindle}"
        )
        self.block_label.setText(f"Satz: {self._line_text(frame.line)}")
        self.modal_tree.clear()
        for group, modal_value in snap.modal:
            QTreeWidgetItem(self.modal_tree, [group, modal_value])

    def _line_text(self, line: int) -> str:
        if line <= 0:
            return "-"
        block = self.editor.document().findBlockByNumber(line - 1)
        return f"Zeile {line}: {block.text().strip()}" if block.isValid() else f"Zeile {line}"

    def _highlight_line(self, line: int) -> None:
        if line <= 0:
            self.editor.setExtraSelections([])
            return
        block = self.editor.document().findBlockByNumber(line - 1)
        if not block.isValid():
            return
        selection = QTextEdit.ExtraSelection()
        selection.format.setBackground(QColor("#ffe066"))
        selection.format.setProperty(QTextFormat.Property.FullWidthSelection, True)
        cursor = QTextCursor(block)
        selection.cursor = cursor
        self.editor.setExtraSelections([selection])
        if self.mode == 2:
            self.editor.blockSignals(True)
            self.editor.setTextCursor(cursor)
            self.editor.ensureCursorVisible()
            self.editor.blockSignals(False)

    def _cursor_moved(self) -> None:
        if self.mode != 2 or self.running or self.playback is None:
            return
        line = self.editor.textCursor().blockNumber() + 1
        time = self.playback.time_of_line(line)
        if time is not None:
            self.time = time
            self._show_frame()

    def _message_clicked(self, item: QListWidgetItem) -> None:
        line = int(item.data(256) or 0)
        if line > 0:
            block = self.editor.document().findBlockByNumber(line - 1)
            if block.isValid():
                self.editor.setTextCursor(QTextCursor(block))
                self.editor.setFocus()

    def _toggle_rapid(self, checked: bool) -> None:
        self.view.show_rapid = checked
        self.view.update()

    # ------------------------------------------------------------------ manual jog (simulator only)
    def jog(self, axis: int, delta: float) -> None:
        position = list(self.manual_position)
        position[axis] += delta
        self.manual_position = (position[0], position[1], position[2])
        self.view.manual_trace.append(self.manual_position)
        self.view.set_tool(self.manual_position, self.view.tool_radius, self.view.done_moves)
        frame = self._current_frame()
        if frame is not None:
            self._show_position(self.manual_position, (0.0, 0.0, 0.0), frame)
        self.statusBar().showMessage(f"Simulator-Achse {'XYZ'[axis]} um {delta:+g} mm verfahren (keine Maschine)")

    # ------------------------------------------------------------------ files
    def load_text(self, text: str, name: str) -> None:
        self.source_name = name
        self.editor.blockSignals(True)
        self.editor.setPlainText(text)
        self.editor.blockSignals(False)
        self.time = 0.0
        self.resimulate()

    def open_file(self) -> None:
        name, _ = QFileDialog.getOpenFileName(
            self, "Programm für Simulation öffnen", str(Path.home()), "NC-Programme (*.nc *.txt *.tap *.d11 *.*)"
        )
        if name:
            self.load_path(Path(name))

    def load_path(self, path: Path) -> None:
        try:
            data = path.read_bytes()
        except OSError as exc:
            QMessageBox.warning(self, "Datei lesen", f"Datei konnte nicht gelesen werden: {exc}")
            return
        self.load_text(data.decode("ascii", errors="replace"), path.name)

    def save_as(self) -> None:
        name, _ = QFileDialog.getSaveFileName(
            self,
            "Bearbeitetes Programm als NEUE Datei speichern",
            str(Path.home() / "SIMULATOR_PROGRAMM.nc"),
            "NC-Programme (*.nc *.txt)",
            options=QFileDialog.Option.DontConfirmOverwrite,
        )
        if name:
            self.save_to(Path(name))

    def save_to(self, path: Path) -> bool:
        text = self.editor.toPlainText().replace("\r\n", "\n").replace("\n", "\r\n")
        try:
            data = text.encode("ascii")
        except UnicodeEncodeError:
            QMessageBox.warning(self, "Speichern", "Das Programm enthält Zeichen außerhalb von 7-Bit-ASCII.")
            return False
        try:
            write_new_file(path, data)
        except FileExistsConflict:
            QMessageBox.warning(self, "Speichern", f"{path.name} existiert bereits - bitte einen neuen Namen wählen.")
            return False
        except OSError as exc:
            QMessageBox.warning(self, "Speichern", f"Speichern fehlgeschlagen: {exc}")
            return False
        self.statusBar().showMessage(f"Gespeichert als {path.name} (neue Datei)")
        log.info("Simulator program saved as new file %s (%d bytes)", path.name, len(data))
        return True

    def closeEvent(self, event: object) -> None:
        self.pause()
        for thread in list(self._threads):
            thread.wait(10000)
        super().closeEvent(event)  # type: ignore[arg-type]


def _cell(table: QTableWidget, row: int, col: int) -> str:
    item = table.item(row, col)
    return item.text().strip() if item is not None else ""
