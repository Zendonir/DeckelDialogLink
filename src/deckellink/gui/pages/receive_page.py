"""Page "Empfangen": receive a program from the CNC."""

from __future__ import annotations

from pathlib import Path

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QTabWidget,
    QVBoxLayout,
    QWidget,
)

from deckellink.domain.results import ReceivedProgram, ReceiveProgress, ReceiveResult
from deckellink.domain.states import ReceiveState
from deckellink.gui.style import StatusKind
from deckellink.gui.viewmodels import SendGate, format_bytes, format_duration
from deckellink.gui.widgets.error_panel import ErrorPanel
from deckellink.gui.widgets.findings_table import FindingsTable
from deckellink.gui.widgets.status import SignalPanel, StatusIndicator
from deckellink.nc.checks import format_ascii, format_hex
from deckellink.storage.archive import StoredReceive
from deckellink.storage.checksums import sha256_bytes

PREVIEW_MAX_BLOCKS = 5000


def _state_kind(state: ReceiveState) -> StatusKind:
    if state in (ReceiveState.RECEIVING, ReceiveState.FINALIZING, ReceiveState.OPENING_PORT):
        return StatusKind.ACTIVE
    if state in (ReceiveState.LISTENING, ReceiveState.CANCELLED, ReceiveState.TIMED_OUT):
        return StatusKind.WARNING
    if state is ReceiveState.FAILED:
        return StatusKind.ERROR
    if state is ReceiveState.COMPLETED:
        return StatusKind.READY
    return StatusKind.IDLE


class ReceivePage(QWidget):
    start_requested = Signal()
    cancel_requested = Signal()
    open_folder_requested = Signal()
    load_for_send_requested = Signal(Path)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        root = QVBoxLayout(self)
        actions = QHBoxLayout()
        self.start_button = QPushButton("&Empfang starten")
        self.start_button.setObjectName("PrimaryButton")
        self.start_button.setToolTip(
            "Port öffnen und auf Daten der CNC warten. Starten Sie danach die Ausgabe an der Steuerung."
        )
        self.start_button.clicked.connect(self.start_requested.emit)
        self.cancel_button = QPushButton("&Abbrechen")
        self.cancel_button.setObjectName("CancelButton")
        self.cancel_button.setEnabled(False)
        self.cancel_button.clicked.connect(self.cancel_requested.emit)
        self.folder_button = QPushButton("Empfangsordner öffnen")
        self.folder_button.clicked.connect(self.open_folder_requested.emit)
        self.load_button = QPushButton("Empfangene Datei zum Senden laden")
        self.load_button.setEnabled(False)
        self.load_button.clicked.connect(self._emit_load)
        for button in (self.start_button, self.cancel_button, self.folder_button, self.load_button):
            actions.addWidget(button)
        actions.addStretch(1)
        root.addLayout(actions)
        self.gate_label = QLabel()
        root.addWidget(self.gate_label)
        self.error_panel = ErrorPanel()
        root.addWidget(self.error_panel)

        status_box = QGroupBox("Empfangsstatus")
        status_form = QFormLayout(status_box)
        self.state = StatusIndicator()
        self.state.set_status(StatusKind.IDLE, ReceiveState.IDLE.label)
        status_form.addRow(self.state)
        self.connection = QLabel("–")
        self.bytes_label = QLabel("0 B")
        self.elapsed_label = QLabel("00:00:00")
        self.idle_label = QLabel("–")
        status_form.addRow("Verbindung:", self.connection)
        status_form.addRow("Empfangen:", self.bytes_label)
        status_form.addRow("Dauer:", self.elapsed_label)
        status_form.addRow("Ohne Daten seit:", self.idle_label)
        self.signals = SignalPanel()
        status_form.addRow(self.signals)
        root.addWidget(status_box)

        self.tabs = QTabWidget()
        self.ascii_view = QPlainTextEdit()
        self.hex_view = QPlainTextEdit()
        for view in (self.ascii_view, self.hex_view):
            view.setReadOnly(True)
            view.setMaximumBlockCount(PREVIEW_MAX_BLOCKS)
        self.ascii_view.setAccessibleName("Empfangsvorschau ASCII")
        self.hex_view.setAccessibleName("Empfangsvorschau HEX")
        self.tabs.addTab(self.ascii_view, "ASCII-Vorschau")
        self.tabs.addTab(self.hex_view, "HEX-Vorschau")

        result_widget = QWidget()
        result_layout = QVBoxLayout(result_widget)
        form = QFormLayout()
        self.result_state = StatusIndicator()
        self.result_state.set_status(StatusKind.IDLE, "Noch kein Empfang")
        form.addRow(self.result_state)
        self.end_reason = QLabel("–")
        self.raw_bytes = QLabel("–")
        self.normalized_bytes = QLabel("–")
        self.sha_raw = QLabel("–")
        self.sha_norm = QLabel("–")
        self.saved_to = QLabel("–")
        self.saved_to.setWordWrap(True)
        self.notes = QLabel("")
        self.notes.setWordWrap(True)
        form.addRow("Abschlussgrund:", self.end_reason)
        form.addRow("Rohbytes:", self.raw_bytes)
        form.addRow("Normalisierte Bytes:", self.normalized_bytes)
        form.addRow("SHA-256 Rohdaten:", self.sha_raw)
        form.addRow("SHA-256 NC-Datei:", self.sha_norm)
        form.addRow("Gespeichert:", self.saved_to)
        form.addRow("Hinweise:", self.notes)
        result_layout.addLayout(form)
        self.findings = FindingsTable()
        result_layout.addWidget(self.findings, 1)
        self.tabs.addTab(result_widget, "Ergebnis")
        root.addWidget(self.tabs, 1)
        self._stored: StoredReceive | None = None

    def _emit_load(self) -> None:
        if self._stored is not None:
            self.load_for_send_requested.emit(self._stored.nc_path)

    def apply_gate(self, gate: SendGate, *, busy: bool) -> None:
        self.start_button.setEnabled(gate.allowed)
        self.gate_label.setText("" if gate.allowed else f"Empfang nicht möglich: {gate.reason_text}")
        self.load_button.setEnabled(self._stored is not None and not busy)

    def set_connection_text(self, text: str) -> None:
        self.connection.setText(text)

    def begin(self) -> None:
        self.ascii_view.clear()
        self.hex_view.clear()
        self.error_panel.clear()
        self.bytes_label.setText("0 B")
        self.idle_label.setText("–")
        self.tabs.setCurrentIndex(0)

    def set_state(self, state: ReceiveState) -> None:
        self.state.set_status(_state_kind(state), state.label)
        self.cancel_button.setEnabled(state.is_active)

    def set_progress(self, progress: ReceiveProgress) -> None:
        self.bytes_label.setText(f"{format_bytes(progress.bytes_received)} ({progress.bytes_received} Bytes)")
        self.elapsed_label.setText(format_duration(progress.elapsed_s))
        self.idle_label.setText(f"{progress.idle_s:.1f} s")

    def append_data(self, data: bytes) -> None:
        self.ascii_view.moveCursor(self.ascii_view.textCursor().MoveOperation.End)
        text = format_ascii(data).replace("<LF>", "<LF>\n")
        self.ascii_view.insertPlainText(text)
        self.hex_view.appendPlainText(format_hex(data))

    def show_result(
        self,
        result: ReceiveResult,
        program: ReceivedProgram | None,
        stored: StoredReceive | None,
    ) -> None:
        self.set_state(result.state)
        self._stored = stored
        self.end_reason.setText(result.end_reason.label)
        self.raw_bytes.setText(str(len(result.raw)))
        self.sha_raw.setText(sha256_bytes(result.raw))
        if program is not None:
            self.normalized_bytes.setText(str(len(program.normalized)))
            self.sha_norm.setText(sha256_bytes(program.normalized))
            self.notes.setText("\n".join(program.notes) or "–")
            self.findings.set_findings(program.analysis.findings)
            if program.complete:
                self.result_state.set_status(StatusKind.READY, "Programm vollständig empfangen")
            else:
                self.result_state.set_status(StatusKind.WARNING, "UNVOLLSTÄNDIG - Programm nicht ungeprüft verwenden")
        else:
            self.normalized_bytes.setText("–")
            self.sha_norm.setText("–")
            self.notes.setText("Keine Daten empfangen.")
            self.findings.set_findings([])
            self.result_state.set_status(StatusKind.ERROR, "Keine Daten empfangen")
        if stored is not None:
            parts = [str(stored.nc_path), str(stored.meta_path)]
            if stored.raw_path is not None:
                parts.append(str(stored.raw_path))
            self.saved_to.setText("\n".join(parts))
        else:
            self.saved_to.setText("nicht gespeichert")
        if result.error is not None:
            self.error_panel.show_error(result.error)
        self.tabs.setCurrentIndex(2)
