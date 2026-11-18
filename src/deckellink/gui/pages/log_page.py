"""Page "Protokoll": application log view."""

from __future__ import annotations

import contextlib
import logging

from PySide6.QtCore import QObject, Signal
from PySide6.QtWidgets import (
    QComboBox,
    QHBoxLayout,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from deckellink.logging.setup import LOG_FORMAT

MAX_LOG_BLOCKS = 5000


class LogEmitter(QObject):
    record = Signal(int, str)


class QtLogHandler(logging.Handler):
    """Forwards log records to the GUI via a queued Qt signal (thread-safe)."""

    def __init__(self) -> None:
        super().__init__(logging.DEBUG)
        self.emitter = LogEmitter()
        self.setFormatter(logging.Formatter(LOG_FORMAT))

    def emit(self, record: logging.LogRecord) -> None:
        # The emitter may already be destroyed during shutdown.
        with contextlib.suppress(RuntimeError):
            self.emitter.record.emit(record.levelno, self.format(record))


class LogPage(QWidget):
    open_folder_requested = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        root = QVBoxLayout(self)
        bar = QHBoxLayout()
        bar.addWidget(QLabel("Anzeigen ab:"))
        self.level = QComboBox()
        for name in ("DEBUG", "INFO", "WARNING", "ERROR"):
            self.level.addItem(name, getattr(logging, name))
        self.level.setCurrentIndex(1)
        bar.addWidget(self.level)
        self.clear_button = QPushButton("Anzeige leeren")
        bar.addWidget(self.clear_button)
        self.folder_button = QPushButton("Log-Ordner öffnen")
        self.folder_button.clicked.connect(self.open_folder_requested.emit)
        bar.addWidget(self.folder_button)
        bar.addStretch(1)
        root.addLayout(bar)
        self.view = QPlainTextEdit()
        self.view.setReadOnly(True)
        self.view.setMaximumBlockCount(MAX_LOG_BLOCKS)
        self.view.setAccessibleName("Anwendungsprotokoll")
        self.clear_button.clicked.connect(self.view.clear)
        root.addWidget(self.view, 1)
        root.addWidget(QLabel("NC-Inhalte werden nicht in das Standardprotokoll geschrieben."))

    def add_record(self, level: int, text: str) -> None:
        if level >= int(self.level.currentData()):
            self.view.appendPlainText(text)
