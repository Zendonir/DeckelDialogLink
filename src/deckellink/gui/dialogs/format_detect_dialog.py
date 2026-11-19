"""Dialog: determine parity/data bits by listening only."""

from __future__ import annotations

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QDialog,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
    QWidget,
)

from deckellink.diagnostics.format_detect import FormatGuess
from deckellink.gui.style import StatusKind
from deckellink.gui.widgets.status import StatusIndicator

INSTRUCTIONS = (
    "So funktioniert es: DeckelLink öffnet die Schnittstelle mit der Baudrate des Profils als 8N1 und "
    "liest nur mit - es wird nichts an die Maschine gesendet. Starten Sie danach an der CNC die Ausgabe "
    "eines (möglichst langen) Programms. Aus dem achten Bit jedes Zeichens wird die Parität geschätzt."
)


class FormatDetectDialog(QDialog):
    start_requested = Signal(int)
    cancel_requested = Signal()
    adopt_requested = Signal(object)

    def __init__(self, port_label: str, baud_rate: int, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle("Datenformat ermitteln (nur Mitlesen)")
        self.setModal(False)
        self.resize(640, 520)
        self.guess: FormatGuess | None = None
        layout = QVBoxLayout(self)
        intro = QLabel(INSTRUCTIONS)
        intro.setWordWrap(True)
        layout.addWidget(intro)
        form = QFormLayout()
        form.addRow("Schnittstelle:", QLabel(f"{port_label}, {baud_rate} Baud"))
        self.duration = QSpinBox()
        self.duration.setRange(5, 600)
        self.duration.setValue(60)
        self.duration.setSuffix(" s")
        form.addRow("Max. Mitlesedauer:", self.duration)
        layout.addLayout(form)
        row = QHBoxLayout()
        self.start_button = QPushButton("Mitlesen &starten")
        self.start_button.setObjectName("PrimaryButton")
        self.start_button.clicked.connect(self._start)
        self.cancel_button = QPushButton("Beenden und auswerten")
        self.cancel_button.setEnabled(False)
        self.cancel_button.clicked.connect(self.cancel_requested.emit)
        self.adopt_button = QPushButton("Als Profilkopie übernehmen …")
        self.adopt_button.setEnabled(False)
        self.adopt_button.setToolTip("Legt eine Kopie des aktiven Profils mit dem ermittelten Datenformat an.")
        self.adopt_button.clicked.connect(lambda: self.adopt_requested.emit(self.guess))
        row.addWidget(self.start_button)
        row.addWidget(self.cancel_button)
        row.addStretch(1)
        row.addWidget(self.adopt_button)
        layout.addLayout(row)
        self.state = StatusIndicator()
        self.state.set_status(StatusKind.IDLE, "Noch nicht gestartet")
        layout.addWidget(self.state)
        self.report = QPlainTextEdit()
        self.report.setReadOnly(True)
        layout.addWidget(self.report, 1)

    def _start(self) -> None:
        self.guess = None
        self.adopt_button.setEnabled(False)
        self.set_running(True)
        self.state.set_status(StatusKind.WARNING, "Lese mit - jetzt Ausgabe an der CNC starten")
        self.report.clear()
        self.start_requested.emit(self.duration.value())

    def set_running(self, running: bool) -> None:
        self.start_button.setEnabled(not running)
        self.cancel_button.setEnabled(running)

    def set_received(self, count: int) -> None:
        self.state.set_status(StatusKind.ACTIVE, f"Empfangen: {count} Bytes")

    def show_guess(self, guess: FormatGuess) -> None:
        self.guess = guess
        self.set_running(False)
        found = guess.parity is not None
        self.state.set_status(
            StatusKind.READY if found else StatusKind.WARNING,
            f"Ergebnis: {guess.frame}" if found else "Kein eindeutiges Ergebnis",
        )
        self.report.setPlainText(guess.report())
        self.adopt_button.setEnabled(found)
