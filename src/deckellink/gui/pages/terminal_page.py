"""Page "Terminal": TX/RX traffic in ASCII, HEX or combined view (read-only)."""

from __future__ import annotations

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QHBoxLayout,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from deckellink.diagnostics.terminal import TerminalBuffer, TerminalEntry, TerminalMode


class TerminalPage(QWidget):
    file_logging_toggled = Signal(bool)

    def __init__(self, buffer: TerminalBuffer, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.buffer = buffer
        root = QVBoxLayout(self)
        bar = QHBoxLayout()
        bar.addWidget(QLabel("Ansicht:"))
        self.mode_combo = QComboBox()
        for mode in TerminalMode:
            self.mode_combo.addItem(mode.label, mode.value)
        self.mode_combo.setCurrentIndex(2)
        self.mode_combo.setAccessibleName("Terminalansicht")
        self.mode_combo.currentIndexChanged.connect(self.rerender)
        bar.addWidget(self.mode_combo)
        self.clear_button = QPushButton("&Leeren")
        self.clear_button.clicked.connect(self.clear)
        bar.addWidget(self.clear_button)
        self.file_log = QCheckBox("Vollständiges Rohdatenprotokoll in Datei schreiben")
        self.file_log.setToolTip(
            "Schreibt alle gesendeten und empfangenen Bytes in eine Datei im Log-Ordner. "
            "Enthält NC-Inhalte - nur bei Bedarf aktivieren."
        )
        self.file_log.toggled.connect(self.file_logging_toggled.emit)
        bar.addWidget(self.file_log)
        bar.addStretch(1)
        self.counters = QLabel()
        bar.addWidget(self.counters)
        root.addLayout(bar)
        self.view = QPlainTextEdit()
        self.view.setReadOnly(True)
        self.view.setAccessibleName("Terminalausgabe")
        self.view.setLineWrapMode(QPlainTextEdit.LineWrapMode.NoWrap)
        root.addWidget(self.view, 1)
        root.addWidget(
            QLabel(
                f"Sichtbare Historie begrenzt auf {buffer.max_entries} Einträge. "
                "Das Terminal sendet keine Befehle an die Maschine."
            )
        )
        self.apply_history_limit()
        self.update_counters()

    @property
    def mode(self) -> TerminalMode:
        return TerminalMode(self.mode_combo.currentData())

    def apply_history_limit(self) -> None:
        lines_per_entry = 4 if self.mode is TerminalMode.COMBINED else 1
        self.view.setMaximumBlockCount(self.buffer.max_entries * lines_per_entry)

    def append_entries(self, entries: list[TerminalEntry]) -> None:
        for entry in entries:
            self.view.appendPlainText(entry.format(self.mode))
        self.update_counters()

    def rerender(self) -> None:
        self.apply_history_limit()
        self.view.setPlainText(self.buffer.render(self.mode))
        self.update_counters()

    def clear(self) -> None:
        self.buffer.clear()
        self.view.clear()

    def update_counters(self) -> None:
        self.counters.setText(f"TX: {self.buffer.tx_bytes} Bytes   RX: {self.buffer.rx_bytes} Bytes")
