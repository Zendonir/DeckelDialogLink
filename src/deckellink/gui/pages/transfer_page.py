"""Page "Übertragen": load, check and send an NC program."""

from __future__ import annotations

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QCheckBox,
    QFormLayout,
    QGridLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QPlainTextEdit,
    QProgressBar,
    QPushButton,
    QSplitter,
    QVBoxLayout,
    QWidget,
)

from deckellink.domain.models import Profile
from deckellink.domain.results import SendResult, TransferProgress
from deckellink.domain.states import SendState
from deckellink.gui.style import StatusKind
from deckellink.gui.viewmodels import (
    LoadedProgram,
    SendGate,
    format_bytes,
    format_duration,
    format_rate,
    preview_text,
)
from deckellink.gui.widgets.error_panel import ErrorPanel
from deckellink.gui.widgets.findings_table import FindingsTable
from deckellink.gui.widgets.status import SignalPanel, StatusIndicator


def _state_kind(state: SendState) -> StatusKind:
    if state in (SendState.SENDING, SendState.FINISHING, SendState.OPENING_PORT):
        return StatusKind.ACTIVE
    if state in (SendState.WAITING_FOR_HANDSHAKE, SendState.VALIDATING, SendState.CANCELLED):
        return StatusKind.WARNING
    if state is SendState.FAILED:
        return StatusKind.ERROR
    if state in (SendState.READY, SendState.COMPLETED):
        return StatusKind.READY
    return StatusKind.IDLE


class TransferPage(QWidget):
    open_requested = Signal()
    check_requested = Signal()
    send_requested = Signal()
    cancel_requested = Signal()
    receive_requested = Signal()
    sample_requested = Signal()
    options_changed = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        root = QVBoxLayout(self)

        # --- actions --------------------------------------------------------
        actions = QHBoxLayout()
        self.open_button = QPushButton("&Datei öffnen …")
        self.open_button.setToolTip("NC-Programm auswählen (Strg+O). Die Datei wird nicht verändert.")
        self.open_button.clicked.connect(self.open_requested.emit)
        self.check_button = QPushButton("&Prüfen")
        self.check_button.setToolTip("Datei erneut von der Festplatte laden und prüfen")
        self.check_button.clicked.connect(self.check_requested.emit)
        self.sample_button = QPushButton("Beispielprogramm (Simulator)")
        self.sample_button.setToolTip("Beispielprogramm des aktuellen Simulator-Szenarios erzeugen und laden")
        self.sample_button.clicked.connect(self.sample_requested.emit)
        self.send_button = QPushButton("An Maschine &senden …")
        self.send_button.setObjectName("PrimaryButton")
        self.send_button.setToolTip("Übertragung vorbereiten - vor dem Start folgt eine Bestätigung")
        self.send_button.clicked.connect(self.send_requested.emit)
        self.receive_button = QPushButton("Von Maschine &empfangen")
        self.receive_button.setToolTip("Wechselt zum Empfang")
        self.receive_button.clicked.connect(self.receive_requested.emit)
        self.cancel_button = QPushButton("&Abbrechen")
        self.cancel_button.setObjectName("CancelButton")
        self.cancel_button.setToolTip("Übertragung sofort kontrolliert abbrechen")
        self.cancel_button.setEnabled(False)
        self.cancel_button.clicked.connect(self.cancel_requested.emit)
        for button in (
            self.open_button,
            self.check_button,
            self.sample_button,
            self.send_button,
            self.receive_button,
            self.cancel_button,
        ):
            actions.addWidget(button)
        actions.addStretch(1)
        root.addLayout(actions)
        self.gate_label = QLabel()
        self.gate_label.setWordWrap(True)
        self.gate_label.setAccessibleName("Sendevoraussetzungen")
        root.addWidget(self.gate_label)

        self.error_panel = ErrorPanel()
        root.addWidget(self.error_panel)

        splitter = QSplitter()
        root.addWidget(splitter, 1)

        # --- left: file + findings ------------------------------------------
        left = QWidget()
        left_layout = QVBoxLayout(left)
        left_layout.setContentsMargins(0, 0, 0, 0)
        file_box = QGroupBox("NC-Programm")
        file_form = QFormLayout(file_box)
        self.file_name = QLabel("–")
        self.file_path = QLabel("–")
        self.file_path.setWordWrap(True)
        self.file_size = QLabel("–")
        self.line_count = QLabel("–")
        self.original_bytes = QLabel("–")
        self.transfer_bytes = QLabel("–")
        self.checksum = QLabel("–")
        file_form.addRow("Dateiname:", self.file_name)
        file_form.addRow("Pfad:", self.file_path)
        file_form.addRow("Dateigröße:", self.file_size)
        file_form.addRow("Zeilen:", self.line_count)
        file_form.addRow("Originalbytes:", self.original_bytes)
        file_form.addRow("Transferbytes:", self.transfer_bytes)
        file_form.addRow("SHA-256 Original:", self.checksum)
        left_layout.addWidget(file_box)

        findings_box = QGroupBox("Prüfergebnis")
        findings_layout = QVBoxLayout(findings_box)
        self.verdict = StatusIndicator()
        self.verdict.set_status(StatusKind.IDLE, "Keine Datei geprüft")
        findings_layout.addWidget(self.verdict)
        self.findings = FindingsTable()
        findings_layout.addWidget(self.findings, 1)
        self.ack_warnings = QCheckBox("Ich habe die Warnungen gelesen und bestätige die Übertragung trotzdem.")
        self.ack_warnings.setVisible(False)
        self.ack_warnings.toggled.connect(lambda _c: self.options_changed.emit())
        findings_layout.addWidget(self.ack_warnings)
        self.append_end = QCheckBox("Fehlendes Programmende ergänzen (ausdrücklich bestätigen)")
        self.append_end.setToolTip("Nur verfügbar, wenn das Profil das Ergänzen erlaubt.")
        self.append_end.setVisible(False)
        self.append_end.toggled.connect(lambda _c: self.options_changed.emit())
        findings_layout.addWidget(self.append_end)
        left_layout.addWidget(findings_box, 1)
        splitter.addWidget(left)

        # --- right: preview + transfer -------------------------------------------
        right = QWidget()
        right_layout = QVBoxLayout(right)
        right_layout.setContentsMargins(0, 0, 0, 0)
        preview_box = QGroupBox("Transfer-Vorschau (aufbereitete Daten)")
        preview_layout = QVBoxLayout(preview_box)
        self.applied = QLabel()
        self.applied.setWordWrap(True)
        preview_layout.addWidget(self.applied)
        self.preview = QPlainTextEdit()
        self.preview.setReadOnly(True)
        self.preview.setAccessibleName("Transfer-Vorschau")
        preview_layout.addWidget(self.preview)
        right_layout.addWidget(preview_box, 1)

        transfer_box = QGroupBox("Übertragung")
        grid = QGridLayout(transfer_box)
        self.state = StatusIndicator()
        self.state.set_status(StatusKind.IDLE, SendState.IDLE.label)
        grid.addWidget(self.state, 0, 0, 1, 4)
        self.connection = QLabel("–")
        self.connection.setWordWrap(True)
        grid.addWidget(QLabel("Verbindung:"), 1, 0)
        grid.addWidget(self.connection, 1, 1, 1, 3)
        self.progress = QProgressBar()
        self.progress.setRange(0, 1000)
        self.progress.setFormat("%p %")
        self.progress.setAccessibleName("Übertragungsfortschritt")
        grid.addWidget(self.progress, 2, 0, 1, 4)
        self.line_label = QLabel("Zeile – von –")
        self.bytes_label = QLabel("0 B übertragen")
        self.start_label = QLabel("Start: –")
        self.duration_label = QLabel("Dauer: 00:00:00")
        self.rate_label = QLabel("Rate: –")
        grid.addWidget(self.line_label, 3, 0)
        grid.addWidget(self.bytes_label, 3, 1)
        grid.addWidget(self.start_label, 3, 2)
        grid.addWidget(self.duration_label, 3, 3)
        grid.addWidget(self.rate_label, 4, 0)
        self.signals = SignalPanel()
        grid.addWidget(self.signals, 5, 0, 1, 4)
        right_layout.addWidget(transfer_box)
        splitter.addWidget(right)
        splitter.setSizes([450, 550])

        self.setTabOrder(self.open_button, self.check_button)
        self.setTabOrder(self.check_button, self.sample_button)
        self.setTabOrder(self.sample_button, self.send_button)
        self.setTabOrder(self.send_button, self.receive_button)
        self.setTabOrder(self.receive_button, self.cancel_button)

    # ------------------------------------------------------------------
    def show_program(self, program: LoadedProgram | None, profile: Profile, *, reset_ack: bool = True) -> None:
        """Display ``program``. ``reset_ack`` clears the warning confirmation (new file/profile)."""
        if reset_ack:
            self.ack_warnings.blockSignals(True)
            self.ack_warnings.setChecked(False)
            self.ack_warnings.blockSignals(False)
        if program is None:
            for label in (
                self.file_name,
                self.file_path,
                self.file_size,
                self.line_count,
                self.original_bytes,
                self.transfer_bytes,
                self.checksum,
            ):
                label.setText("–")
            self.findings.set_findings([])
            self.preview.clear()
            self.applied.clear()
            self.ack_warnings.setVisible(False)
            self.append_end.setVisible(False)
            self.verdict.set_status(StatusKind.IDLE, "Keine Datei geprüft")
            return
        analysis = program.analysis
        self.file_name.setText(program.path.name)
        self.file_path.setText(str(program.path))
        self.file_size.setText(format_bytes(len(program.original)))
        self.line_count.setText(str(analysis.line_count))
        self.original_bytes.setText(f"{len(program.original)} Bytes")
        self.checksum.setText(analysis.sha256[:32] + "…")
        self.checksum.setToolTip(analysis.sha256)
        self.findings.set_findings(analysis.findings)
        if program.payload is not None:
            self.transfer_bytes.setText(f"{len(program.payload.data)} Bytes, {program.payload.line_count} Zeilen")
            self.preview.setPlainText(preview_text(program.payload.data))
            applied = ", ".join(program.payload.applied) or "keine Änderungen"
            self.applied.setText(f"Aufbereitung: {applied}")
        else:
            self.transfer_bytes.setText("–")
            self.preview.setPlainText("")
            self.applied.setText(f"Keine Transferdaten: {program.transform_error or 'Prüffehler'}")
        if analysis.has_errors:
            self.verdict.set_status(StatusKind.ERROR, f"{len(analysis.errors)} Fehler - Übertragung gesperrt")
        elif analysis.has_warnings:
            self.verdict.set_status(
                StatusKind.WARNING, f"{len(analysis.warnings)} Warnung(en) - Bestätigung erforderlich"
            )
        else:
            self.verdict.set_status(StatusKind.READY, "Keine Fehler oder Warnungen")
        self.ack_warnings.setVisible(analysis.has_warnings and not analysis.has_errors)
        can_append = profile.transfer.append_program_end and analysis.has("MISSING_PROGRAM_END")
        self.append_end.setVisible(can_append)
        if not can_append:
            self.append_end.blockSignals(True)
            self.append_end.setChecked(False)
            self.append_end.blockSignals(False)

    def set_connection_text(self, text: str) -> None:
        self.connection.setText(text)

    def apply_gate(self, gate: SendGate, *, busy: bool, simulation: bool) -> None:
        self.send_button.setEnabled(gate.allowed)
        self.send_button.setText("Simuliert &senden …" if simulation else "An Maschine &senden …")
        self.gate_label.setText("✔ Bereit zum Senden" if gate.allowed else f"Senden nicht möglich: {gate.reason_text}")
        for button in (self.open_button, self.check_button, self.sample_button, self.receive_button):
            button.setEnabled(not busy)
        self.sample_button.setVisible(simulation)

    def set_state(self, state: SendState) -> None:
        self.state.set_status(_state_kind(state), state.label)
        self.cancel_button.setEnabled(state.is_active)

    def reset_progress(self) -> None:
        self.progress.setValue(0)
        self.line_label.setText("Zeile – von –")
        self.bytes_label.setText("0 B übertragen")
        self.start_label.setText("Start: –")
        self.duration_label.setText("Dauer: 00:00:00")
        self.rate_label.setText("Rate: –")

    def set_progress(self, progress: TransferProgress) -> None:
        self.progress.setValue(int(progress.percent * 10))
        self.progress.setFormat(f"{progress.percent:.0f} %")
        self.line_label.setText(f"Zeile {progress.current_line} von {progress.total_lines}")
        self.bytes_label.setText(
            f"{format_bytes(progress.bytes_done)} von {format_bytes(progress.bytes_total)} übertragen"
        )
        if progress.started_at is not None:
            self.start_label.setText(f"Start: {progress.started_at.strftime('%H:%M:%S')}")
        self.duration_label.setText(f"Dauer: {format_duration(progress.elapsed_s)}")
        self.rate_label.setText(f"Rate: {format_rate(progress.rate_bps)}")

    def show_result(self, result: SendResult) -> None:
        self.set_state(result.state)
        # Total duration includes start/end delays; the rate label keeps the rate of the
        # actual sending phase from the last progress update.
        self.duration_label.setText(f"Dauer: {format_duration(result.duration_s)}")
        if result.error is not None:
            self.error_panel.show_error(result.error)
