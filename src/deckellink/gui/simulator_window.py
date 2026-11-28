"""DIALOG 11 graphic simulator window (offline, never connected to a machine)."""

from __future__ import annotations

import logging
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from PySide6.QtCore import Qt, QThread, QTimer, Signal
from PySide6.QtGui import QColor, QFont, QKeySequence, QShortcut, QTextCursor, QTextFormat
from PySide6.QtWidgets import (
    QButtonGroup,
    QCheckBox,
    QComboBox,
    QDoubleSpinBox,
    QFileDialog,
    QFormLayout,
    QGridLayout,
    QHBoxLayout,
    QHeaderView,
    QInputDialog,
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
from deckellink.dialog11sim.menus import AUX_MODES, BACK, MAIN_MODES, NOT_SIMULATED, MenuItem, structure_text
from deckellink.dialog11sim.model import Severity, SimConfig, SimResult, ToolData
from deckellink.dialog11sim.playback import Frame, Playback, format_duration
from deckellink.dialog11sim.reference import DEMO_PARAMETERS, DEMO_PROGRAM, FUNCTION_REFERENCE
from deckellink.gui.simulator_dialogs import CuttingDataDialog, DiaDialog, SelectProgramDialog
from deckellink.gui.style import SIMULATION_BANNER_STYLE
from deckellink.gui.widgets.toolpath_view import ToolpathView
from deckellink.storage.files import FileExistsConflict, write_new_file
from deckellink.storage.library import EntryKind, ProgramLibrary

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

SCREEN_TAB = 5
NOTES_TAB = 4
SOFTKEY_STYLE = "font-weight:bold;"
SOFTKEY_OFF_STYLE = "font-weight:bold;color:#868e96;font-style:italic;"
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

    def __init__(
        self,
        text: str | None = None,
        name: str = "",
        parent: QWidget | None = None,
        *,
        tools: dict[int, ToolData] | None = None,
        library: ProgramLibrary | None = None,
        save_callback: Callable[[bytes], str] | None = None,
        follow_path: Path | None = None,
    ) -> None:
        super().__init__(parent)
        self.tools = dict(tools or {})
        self.library = library
        self.save_callback = save_callback
        self.follow_path = follow_path
        self.menu_stack: list[MenuItem] = [MAIN_MODES[2]]
        self.single_block_mode = False
        self.realtime = False
        self.graphics_on = True
        self.contour_on = False
        self.dripfeed_on = False
        self.show_tool = True
        self.preset_offset: Vec3 = (0.0, 0.0, 0.0)
        self.follow_line: int | None = None
        self.pending_start_block: int | None = None
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
        self.aux_buttons: dict[str, QPushButton] = {}
        for code in AUX_MODES:
            button = QPushButton(code)
            button.setMinimumHeight(34)
            button.setToolTip(AUX_MODES[code].description)
            button.clicked.connect(lambda _c=False, c=code: self.aux_mode(c))
            self.aux_buttons[code] = button
            top.addWidget(button)
        self.back_button = QPushButton("◄ ZURÜCK")
        self.back_button.setMinimumHeight(34)
        self.back_button.clicked.connect(self.menu_back)
        top.addWidget(self.back_button)
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
        self.listing_label.setWordWrap(True)
        left_layout.addWidget(self.listing_label)
        file_row = QHBoxLayout()
        self.open_button = QPushButton("Datei öffnen …")
        self.open_button.clicked.connect(self.open_file)
        self.save_as_button = QPushButton("Als neue Datei …")
        self.save_as_button.clicked.connect(self.save_as)
        self.save_library_button = QPushButton("In Programmverwaltung speichern")
        self.save_library_button.setVisible(self.save_callback is not None)
        self.save_library_button.clicked.connect(self.save_to_library)
        self.demo_button = QPushButton("Demo")
        self.demo_button.clicked.connect(self.load_demo)
        for button in (self.open_button, self.save_as_button, self.save_library_button, self.demo_button):
            file_row.addWidget(button)
        left_layout.addLayout(file_row)
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
        self.notes_edit = QPlainTextEdit()
        self.notes_edit.setPlaceholderText("Notizen (nur in diesem Simulatorfenster, werden nicht gespeichert)")
        self.tabs.addTab(self.notes_edit, "Notizen")
        self.screen_text = QPlainTextEdit()
        self.screen_text.setReadOnly(True)
        self.screen_text.setFont(MONO)
        self.tabs.addTab(self.screen_text, "Bildschirm")
        splitter.addWidget(self.tabs)
        splitter.setSizes([360, 620, 380])
        self.splitter = splitter
        root.addWidget(splitter, 1)

        # machine panel keys and playback row
        play = QHBoxLayout()
        panel = (
            ("▶ ZYKLUS-START", self.start, "Testlauf starten (Folgesatz bzw. Einzelsatz)"),
            ("■ ZYKLUS-STOP", self.pause, "Anhalten"),
            ("EINZELSATZ", self.single_block, "Nächsten Satz ausführen"),
            ("⏮ ANFANG", self.rewind, "Zurück an den Programmanfang"),
            ("⏭ ENDE", self.to_end, "Ans Programmende"),
        )
        self.panel_buttons: list[QPushButton] = []
        for text, slot, tip in panel:
            button = QPushButton(text)
            button.setToolTip(tip)
            button.clicked.connect(slot)
            play.addWidget(button)
            self.panel_buttons.append(button)
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
        self.follow_box = QCheckBox("Übertragung mitverfolgen")
        self.follow_box.setToolTip(
            "Zeigt bei einer laufenden Übertragung dieses Programms (Senden/Nachladebetrieb) den bisher "
            "übertragenen Stand. Das ist der Übertragungsstand, NICHT die Position der Maschine."
        )
        self.follow_box.setChecked(self.follow_path is not None)
        self.follow_box.setVisible(self.follow_path is not None)
        play.addWidget(self.follow_box)
        root.addLayout(play)

        # softkeys F1..F8
        keys = QHBoxLayout()
        self.softkeys: list[QPushButton] = []
        for index in range(8):
            button = QPushButton()
            button.setMinimumHeight(64)
            button.setStyleSheet(SOFTKEY_STYLE)
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
        layout.addWidget(
            QLabel(
                "Werkzeugtabelle des Simulators (Radius für Bahnkorrektur und Zyklen)"
                + (" - übernommen aus der Werkzeugverwaltung:" if self.tools else ":")
            )
        )
        rows = [(n, t.radius, t.length) for n, t in sorted(self.tools.items())] or [(n, 5.0, 0.0) for n in range(1, 9)]
        self.tool_table = QTableWidget(len(rows) + 4, 3)
        self.tool_table.setHorizontalHeaderLabels(["T-Nr.", "Radius mm", "Länge mm"])
        self.tool_table.horizontalHeader().setSectionResizeMode(QHeaderView.ResizeMode.Stretch)
        for row, (number, radius, length) in enumerate(rows):
            for col, value in enumerate((str(number), f"{radius:g}", f"{length:g}")):
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
        if self.pending_start_block is not None:
            number, self.pending_start_block = self.pending_start_block, None
            self.apply_start_block(number)
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

    # ------------------------------------------------------------------ modes and menus
    @property
    def menu(self) -> MenuItem:
        return self.menu_stack[-1]

    def set_mode(self, mode: int) -> None:
        """Main mode key 1/2/3: opens the main menu of that mode."""
        self.mode = mode
        self.mode_buttons[mode].setChecked(True)
        self.menu_stack = [MAIN_MODES[mode]]
        self.pause()
        self.editor.setReadOnly(mode != 3)
        self.listing_label.setText(
            {
                1: "Handfahren: Achsen nur im Simulator verfahren (12 INKREMENT, 13 FAHREN) - keine Maschine!",
                2: "Automatik/Testlauf - aktueller Satz ist markiert. Start/Stopp/Einzelsatz über die Tasten unten.",
                3: "Programm bearbeiten - die Grafik wird nach Änderungen neu berechnet. "
                "Die Originaldatei bleibt unverändert.",
            }[mode]
        )
        self.jog_step.setEnabled(mode == 1)
        if mode == 1:
            frame = self._current_frame()
            self.manual_position = frame.position if frame is not None else self.manual_position
            self.view.manual_trace = [self.manual_position]
            self.view.set_tool(self.manual_position, self.view.tool_radius, self.view.done_moves)
        else:
            self.view.manual_trace = []
        self._refresh_menu()
        self._show_frame()

    def aux_mode(self, code: str) -> None:
        """Auxiliary mode key (MODE, INFO, HELP, EDIT, GRAF, DIA)."""
        item = AUX_MODES[code]
        if not item.is_menu:
            self.run_action(item.action, item)
            return
        self.menu_stack = [self.menu_stack[0], item]
        self._refresh_menu()

    def menu_back(self) -> None:
        if len(self.menu_stack) > 1:
            self.menu_stack.pop()
        self._refresh_menu()

    def _refresh_menu(self) -> None:
        for index, item in enumerate(self.menu.slots()):
            button = self.softkeys[index]
            if item is None:
                button.setText(f"F{index + 1}")
                button.setEnabled(False)
                button.setToolTip("")
                button.setStyleSheet(SOFTKEY_STYLE)
                continue
            code = f"{item.code} " if item.code and item.code not in AUX_MODES else ""
            button.setText(f"F{index + 1}  {code}\n{item.label}")
            button.setEnabled(True)
            tip = item.description + ("" if item.available else f" - {NOT_SIMULATED}")
            button.setToolTip(tip + (" (Menünummer abgeleitet)" if item.assumed else ""))
            button.setStyleSheet(SOFTKEY_STYLE if item.available else SOFTKEY_OFF_STYLE)
        self.back_button.setEnabled(len(self.menu_stack) > 1)
        self._update_mode_display()

    def _update_mode_display(self) -> None:
        if not hasattr(self, "mode_display"):
            return
        path = " › ".join(item.title for item in self.menu_stack)
        flags = []
        if self.mode == 2:
            flags.append("TESTLAUF LÄUFT" if self.running else "TESTLAUF HALT")
            flags.append("EINZELSATZ" if self.single_block_mode else "FOLGESATZ")
            if self.realtime:
                flags.append("ECHTZEIT")
        if self.mode == 1:
            flags.append(f"Schritt {self.jog_step.currentText()} mm")
        flags.append("GRAFIK EIN" if self.graphics_on else "GRAFIK AUS")
        if self.contour_on:
            flags.append("KONTUR EIN")
        if self.dripfeed_on:
            flags.append("NACHLADEN")
        if self.follow_line is not None:
            flags.append(f"ÜBERTRAGEN BIS ZEILE {self.follow_line}")
        self.mode_display.setText(f"{path}  ·  {'  ·  '.join(flags)}  ·  SIMULATION")

    def softkey(self, number: int) -> None:
        item = self.menu.slots()[number - 1] if 1 <= number <= 8 else None
        if item is None:
            return
        if not item.available:
            self.show_screen(f"{item.title}\n\n{item.description}\n\n{NOT_SIMULATED}")
            self.statusBar().showMessage(f"{item.title.replace(chr(10), ' ')}: {NOT_SIMULATED}", 8000)
            return
        if item.is_menu:
            self.menu_stack.append(item)
            self._refresh_menu()
            return
        if item.action == BACK:
            self.menu_back()
            return
        self.run_action(item.action, item)

    def show_screen(self, text: str) -> None:
        self.screen_text.setPlainText(text)
        self.tabs.setCurrentIndex(SCREEN_TAB)

    def run_action(self, action: str, item: MenuItem | None = None) -> None:
        name, _, arg = action.partition(":")
        handlers: dict[str, Callable[[], object]] = {
            "preset": self.preset,
            "goto": self.goto_dialog,
            "cutting_data": lambda: CuttingDataDialog(self).exec(),
            "jog_step": self._next_jog_step,
            "toggle_single": self._toggle_single,
            "toggle_realtime": self._toggle_realtime,
            "select": self.select_program,
            "toggle_graphics": self.toggle_graphics,
            "start_stop": lambda: self.pause() if self.running else self.start(),
            "test_fast": self._test_fast,
            "edit_main": lambda: self._goto_section(None),
            "edit_sub": lambda: self._goto_section("%*"),
            "edit_macro": lambda: self._goto_section("%%"),
            "tools": lambda: self.tabs.setCurrentIndex(2),
            "parameters": lambda: self._focus_setup(self.parameter_edit),
            "zero_points": lambda: self._focus_setup(self.zero_table),
            "contour": self._toggle_contour,
            "library": lambda: self.select_program(EntryKind.PROGRAM, EntryKind.MACRO),
            "library_macros": lambda: self.select_program(EntryKind.MACRO),
            "notes": lambda: self.tabs.setCurrentIndex(NOTES_TAB),
            "messages": lambda: self.tabs.setCurrentIndex(1),
            "transfer_in": self._transfer_in,
            "transfer_out": self._transfer_out,
            "toggle_dripfeed": self._toggle_dripfeed,
            "config_io": self._config_io,
            "transfer_start": self._transfer_start,
            "screen_text": lambda: self._set_screen("text"),
            "screen_graphics": lambda: self._set_screen("graphics"),
            "toggle_screen": lambda: self._set_screen("text" if self.graphics_on else "graphics"),
            "toggle_tool": self._toggle_tool,
            "toggle_rapid": lambda: self.rapid_box.setChecked(not self.rapid_box.isChecked()),
            "clear": self.rewind,
            "reference": lambda: self.tabs.setCurrentIndex(3),
            "find": self.edit_find,
            "replace": self.edit_replace,
            "copy": lambda: self._edit("copy"),
            "cut": lambda: self._edit("cut"),
            "delete": lambda: self._edit("delete"),
            "paste": lambda: self._edit("paste"),
        }
        if name == "jog":
            axis, sign = arg.split(":")
            self.jog(int(axis), (1.0 if sign == "+" else -1.0) * float(self.jog_step.currentText()))
        elif name == "view":
            self._set_view(arg)
        elif name == "rotate":
            self.view.yaw += float(arg)
            self.view.set_mode(self.view.mode)
        elif name == "zoom":
            if arg == "in":
                self.view.zoom = min(self.view.zoom * 1.5, 200.0)
                self.view.update()
            else:
                self.view.fit()
        elif name == "structure":
            root = MAIN_MODES[int(arg)] if arg else None
            self.show_screen("Menüstruktur des Simulators nach der DIALOG 11\n\n" + structure_text(root))
        elif name == "dia":
            self.dia(arg)
        elif name in handlers:
            handlers[name]()
        else:
            self.statusBar().showMessage(f"Funktion '{action}' ist im Simulator nicht belegt", 5000)
        self._update_mode_display()
        del item

    # ------------------------------------------------------------------ menu actions
    def _focus_setup(self, widget: QWidget) -> None:
        self.tabs.setCurrentIndex(2)
        widget.setFocus()

    def _next_jog_step(self) -> None:
        self.jog_step.setCurrentIndex((self.jog_step.currentIndex() + 1) % self.jog_step.count())
        self._update_mode_display()

    def _toggle_single(self) -> None:
        self.single_block_mode = not self.single_block_mode

    def _toggle_realtime(self) -> None:
        self.realtime = not self.realtime
        self.speed_combo.setCurrentIndex(0 if self.realtime else 3)

    def _test_fast(self) -> None:
        self.speed_combo.setCurrentIndex(self.speed_combo.count() - 1)
        self.realtime = False
        self.start()

    def _toggle_contour(self) -> None:
        self.contour_on = not self.contour_on
        self.statusBar().showMessage(
            "Konturrechner: freie Konturelemente (UW, UR, FA) werden im Simulator nicht berechnet", 8000
        )

    def _toggle_tool(self) -> None:
        self.show_tool = not self.show_tool
        self.view.tool_radius = self.view.tool_radius if self.show_tool else 0.0
        self._show_frame()

    def toggle_graphics(self) -> None:
        self._set_screen("text" if self.graphics_on else "graphics")

    def _set_screen(self, screen: str) -> None:
        self.graphics_on = screen == "graphics"
        sizes = self.splitter.sizes()
        total = sum(sizes) or 1360
        if self.graphics_on:
            self.splitter.setSizes([int(total * 0.26), int(total * 0.46), int(total * 0.28)])
        else:
            self.splitter.setSizes([int(total * 0.62), 0, int(total * 0.38)])
        self._update_mode_display()

    def _set_view(self, arg: str) -> None:
        from deckellink.gui.widgets.toolpath_view import ViewMode

        mode = self.view.cycle_mode() if arg == "cycle" else ViewMode(arg)
        if arg != "cycle":
            self.view.set_mode(mode)
        self.view_combo.blockSignals(True)
        self.view_combo.setCurrentIndex(self.view_combo.findData(mode))
        self.view_combo.blockSignals(False)

    def _goto_section(self, prefix: str | None) -> None:
        if self.mode != 3:
            self.set_mode(3)
        document = self.editor.document()
        target = 0
        if prefix is not None:
            for number in range(document.blockCount()):
                text = document.findBlockByNumber(number).text().strip()
                if text.startswith(prefix) and (prefix != "%*" or not text.startswith("%%")):
                    target = number
                    break
            else:
                self.show_screen(
                    f"Kein Abschnitt '{prefix}n' im Programm.\n\nNeuer Abschnitt: am Ende eine Zeile nur mit "
                    f"'{prefix}1' einfügen und darunter die Sätze schreiben (Simulator-Konvention, siehe Handbuch)."
                )
                return
        cursor = QTextCursor(document.findBlockByNumber(target))
        self.editor.setTextCursor(cursor)
        self.editor.setFocus()

    def preset(self) -> None:
        """PRESET (simulator only): the current manual position becomes the displayed zero."""
        self.preset_offset = self.manual_position
        self.statusBar().showMessage("PRESET: Anzeige an dieser Position auf 0 gesetzt (nur Simulator)", 8000)
        self._show_frame()

    def goto_dialog(self) -> None:
        current = " ".join(f"{a}{v:g}" for a, v in zip("XYZ", self._display(self.manual_position), strict=True))
        text, ok = QInputDialog.getText(self, "FAHREN (Simulator)", "Zielposition, z. B. X10 Y20 Z5:", text=current)
        if ok:
            self.goto(text)

    def goto(self, text: str) -> bool:
        target = list(self._display(self.manual_position))
        import re as _re

        found = False
        for axis, value in _re.findall(r"([XYZ])\s*([-+]?\d+(?:[.,]\d*)?)", text.upper()):
            target["XYZ".index(axis)] = float(value.replace(",", "."))
            found = True
        if not found:
            self.statusBar().showMessage("Keine gültige Position (z. B. X10 Y20 Z5)", 5000)
            return False
        position = (
            target[0] + self.preset_offset[0],
            target[1] + self.preset_offset[1],
            target[2] + self.preset_offset[2],
        )
        self.manual_position = position
        self.view.manual_trace.append(position)
        self.view.set_tool(position, self.view.tool_radius, self.view.done_moves)
        self._show_frame()
        return True

    def _display(self, position: Vec3) -> Vec3:
        if self.mode != 1:
            return position
        return (
            position[0] - self.preset_offset[0],
            position[1] - self.preset_offset[1],
            position[2] - self.preset_offset[2],
        )

    def select_program(self, *kinds: EntryKind) -> bool:
        dialog = SelectProgramDialog(self.library, kinds or (EntryKind.PROGRAM, EntryKind.MACRO), self)
        if dialog.exec() != SelectProgramDialog.DialogCode.Accepted or dialog.chosen_path is None:
            return False
        self.load_path(dialog.chosen_path)
        self.pending_start_block = dialog.start_number()
        return True

    def apply_start_block(self, number: int) -> bool:
        """VORLAUF: jump to block N."""
        document = self.editor.document()
        for index in range(document.blockCount()):
            text = document.findBlockByNumber(index).text().strip().upper()
            matches = text.startswith(f"N{number} ") or text == f"N{number}"
            time = self.playback.time_of_line(index + 1) if matches and self.playback is not None else None
            if time is not None:
                self.time = time
                self._show_frame()
                return True
        self.statusBar().showMessage(f"Satz N{number} nicht gefunden", 5000)
        return False

    def _transfer_in(self) -> None:
        self.show_screen(
            "DATEN-TRANSFER › EINLESEN\n\nAn der Steuerung: Programm von einem Partnergerät einlesen.\n"
            "In DeckelLink entspricht das der Seite 'Übertragen' (Senden) bzw. 'Programmverwaltung' → "
            "'Zum Senden laden'.\n\nIm Simulator: Programm aus der Programmverwaltung oder einer Datei anwählen."
        )
        self.select_program()

    def _transfer_out(self) -> None:
        self.show_screen(
            "DATEN-TRANSFER › AUSLESEN\n\nAn der Steuerung: Programm an ein Partnergerät ausgeben.\n"
            "In DeckelLink entspricht das der Seite 'Empfangen'.\n\nIm Simulator: Programm als neue Datei speichern."
        )
        self.save_as()

    def _toggle_dripfeed(self) -> None:
        self.dripfeed_on = not self.dripfeed_on
        self.show_screen(
            "DATEN-TRANSFER › NACHLADEN " + ("EIN" if self.dripfeed_on else "AUS") + "\n\n"
            "An der Steuerung: Nachladefunktion aktivieren (F3), EINLESEN wählen (F1) und starten (F8), dann in "
            "Hauptbetriebsart 2 das Programm anwählen und mit Zyklus-Start abarbeiten.\n\n"
            "In DeckelLink: Profil mit LSV2-Protokoll wählen und auf der Seite 'Übertragen' bzw. in der "
            "Programmverwaltung 'Nachladebetrieb (LSV2)' starten. Die CNC fordert die Sätze über Quittungen an; "
            "DeckelLink startet die Bearbeitung nie.\n\nIm Simulator: START (F8) zeigt den Ablauf als Testlauf."
        )

    def _config_io(self) -> None:
        self.show_screen(
            "DATEN-TRANSFER › KONFIG I/O\n\nAn der Steuerung: Schnittstellenparameter für bis zu 7 Partnergeräte "
            "(Gerätenummer, Kanal, Baudrate bis 19200, Datenbits, Stoppbits, Parität, Protokoll).\n\n"
            "Die Werte müssen mit dem DeckelLink-Profil übereinstimmen (Einstellungen → Profil). "
            "Bitte an der Steuerung ablesen und in der Verifikations-Checkliste eintragen."
        )

    def _transfer_start(self) -> None:
        if self.dripfeed_on:
            self.set_mode(2)
            self.dripfeed_on = True
            self.start()
            self.statusBar().showMessage("Nachladebetrieb (Simulation): Programm läuft als Testlauf", 8000)
        else:
            self.statusBar().showMessage("Im Simulator gibt es keine Übertragung - EINLESEN/AUSLESEN wählen", 8000)

    def dia(self, kind: str) -> str | None:
        if self.mode != 3:
            self.set_mode(3)
        dialog = DiaDialog(kind, self._next_block_number(), self)
        if dialog.exec() != DiaDialog.DialogCode.Accepted:
            return None
        block = dialog.block()
        self.insert_block(block)
        return block

    def insert_block(self, block: str) -> None:
        cursor = self.editor.textCursor()
        cursor.movePosition(QTextCursor.MoveOperation.EndOfBlock)
        cursor.insertText("\n" + block)
        self.editor.setTextCursor(cursor)

    def _next_block_number(self) -> int:
        import re as _re

        numbers = [int(n) for n in _re.findall(r"(?m)^\s*N(\d+)", self.editor.toPlainText().upper())]
        return (max(numbers) + 10) if numbers else 10

    def edit_find(self, text: str | None = None) -> bool:
        if text is None:
            text, ok = QInputDialog.getText(self, "EDIT › SUCHEN", "Suchtext:")
            if not ok:
                return False
        found = self.editor.find(text)
        if not found:
            self.editor.moveCursor(QTextCursor.MoveOperation.Start)
            found = self.editor.find(text)
        self.statusBar().showMessage(f"'{text}' {'gefunden' if found else 'nicht gefunden'}", 5000)
        return bool(found)

    def edit_replace(self, old: str | None = None, new: str | None = None) -> int:
        if self.mode != 3:
            self.statusBar().showMessage("EDIT ist nur in Hauptbetriebsart 3 PROGRAMM möglich", 5000)
            return 0
        if old is None or new is None:
            old, ok = QInputDialog.getText(self, "EDIT › ERSETZEN", "Suchen:")
            if not ok or not old:
                return 0
            new, ok = QInputDialog.getText(self, "EDIT › ERSETZEN", f"'{old}' ersetzen durch:")
            if not ok:
                return 0
        text = self.editor.toPlainText()
        count = text.count(old)
        if count:
            cursor = self.editor.textCursor()
            cursor.select(QTextCursor.SelectionType.Document)
            cursor.insertText(text.replace(old, new))
        self.statusBar().showMessage(f"{count} Stelle(n) ersetzt", 5000)
        return count

    def _edit(self, operation: str) -> None:
        if self.mode != 3 and operation != "copy":
            self.statusBar().showMessage("EDIT ist nur in Hauptbetriebsart 3 PROGRAMM möglich", 5000)
            return
        cursor = self.editor.textCursor()
        if not cursor.hasSelection() and operation in ("copy", "cut", "delete"):
            cursor.select(QTextCursor.SelectionType.BlockUnderCursor)
            self.editor.setTextCursor(cursor)
        if operation == "copy":
            self.editor.copy()
        elif operation == "cut":
            self.editor.cut()
        elif operation == "paste":
            self.editor.paste()
        else:
            cursor.removeSelectedText()

    # ------------------------------------------------------------------ transfer follow-up (display only)
    def follow_transfer(self, line: int) -> None:
        """Show the transfer progress of DeckelLink (line already transmitted), not the machine position."""
        if not self.follow_box.isChecked() or self.playback is None:
            return
        self.pause()
        self.follow_line = line
        self.time = self.playback.time_until_line(line)
        self._show_frame()
        self._update_mode_display()

    # ------------------------------------------------------------------ playback
    def start(self) -> None:
        if self.playback is None:
            return
        if self.single_block_mode:
            self.single_block()
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
            self._show_position(self._display(self.manual_position), (0.0, 0.0, 0.0), frame)
            return
        radius = frame.snapshot.tool_radius if self.show_tool else 0.0
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
            self._show_position(self._display(self.manual_position), (0.0, 0.0, 0.0), frame)
        self.statusBar().showMessage(f"Simulator-Achse {'XYZ'[axis]} um {delta:+g} mm verfahren (keine Maschine)")

    # ------------------------------------------------------------------ files
    def load_demo(self) -> None:
        self.load_text(DEMO_PROGRAM, "Demoprogramm")
        self.parameter_edit.setPlainText("\n".join(f"{k}={v}" for k, v in DEMO_PARAMETERS.items()))

    def save_to_library(self) -> bool:
        if self.save_callback is None:
            return False
        text = self.editor.toPlainText().replace("\r\n", "\n").replace("\n", "\r\n")
        try:
            message = self.save_callback(text.encode("ascii"))
        except UnicodeEncodeError:
            QMessageBox.warning(self, "Speichern", "Das Programm enthält Zeichen außerhalb von 7-Bit-ASCII.")
            return False
        except Exception as exc:  # storage errors are shown, never fatal
            QMessageBox.warning(self, "Speichern", f"Speichern fehlgeschlagen: {exc}")
            return False
        self.statusBar().showMessage(message, 8000)
        return True

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
