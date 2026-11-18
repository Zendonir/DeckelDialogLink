"""Loopback test assistant (non-modal)."""

from __future__ import annotations

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QComboBox,
    QDialog,
    QDoubleSpinBox,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
    QWidget,
)

from deckellink.diagnostics.loopback_test import LoopbackPattern, LoopbackResult, LoopbackVerdict
from deckellink.diagnostics.modem_status import format_modem_status
from deckellink.gui.style import StatusKind
from deckellink.gui.widgets.status import StatusIndicator

INSTRUCTIONS = (
    "1. Kabel von der Maschine trennen.\n"
    "2. Am PC-seitigen Stecker TX und RX brücken (DB9: Pin 2-3, DB25: Pin 2-3).\n"
    "3. Für Handshake-Tests zusätzlich RTS-CTS (DB9: 7-8) und DTR-DSR (DB9: 4-6) brücken.\n"
    "4. Test starten. Im Simulationsmodus wird eine simulierte Brücke verwendet."
)


class LoopbackDialog(QDialog):
    run_requested = Signal(str, int, float)
    cancel_requested = Signal()

    def __init__(self, port_label: str, data_bits: int, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle("Loopback-Test")
        self.setModal(False)
        self.resize(640, 520)
        layout = QVBoxLayout(self)
        intro = QLabel(INSTRUCTIONS)
        intro.setWordWrap(True)
        layout.addWidget(intro)
        form = QFormLayout()
        form.addRow("Schnittstelle:", QLabel(f"{port_label} ({data_bits} Datenbits)"))
        self.pattern = QComboBox()
        for pattern in LoopbackPattern:
            self.pattern.addItem(pattern.label, pattern.value)
        form.addRow("Testmuster:", self.pattern)
        self.length = QSpinBox()
        self.length.setRange(1, 65536)
        self.length.setValue(256)
        self.length.setSuffix(" Bytes")
        form.addRow("Datenmenge:", self.length)
        self.timeout = QDoubleSpinBox()
        self.timeout.setRange(0.5, 120.0)
        self.timeout.setValue(3.0)
        self.timeout.setSuffix(" s")
        form.addRow("Timeout:", self.timeout)
        if data_bits < 8:
            form.addRow("", QLabel(f"Bei {data_bits} Datenbits werden keine Werte über 0x7F gesendet."))
        layout.addLayout(form)
        buttons = QHBoxLayout()
        self.run_button = QPushButton("Test &starten")
        self.run_button.setObjectName("PrimaryButton")
        self.run_button.clicked.connect(self._run)
        self.cancel_button = QPushButton("Abbrechen")
        self.cancel_button.setEnabled(False)
        self.cancel_button.clicked.connect(self.cancel_requested.emit)
        close = QPushButton("Schließen")
        close.clicked.connect(self.close)
        buttons.addWidget(self.run_button)
        buttons.addWidget(self.cancel_button)
        buttons.addStretch(1)
        buttons.addWidget(close)
        layout.addLayout(buttons)
        self.verdict = StatusIndicator()
        self.verdict.set_status(StatusKind.IDLE, "Noch nicht ausgeführt")
        layout.addWidget(self.verdict)
        self.details = QPlainTextEdit()
        self.details.setReadOnly(True)
        layout.addWidget(self.details, 1)

    def _run(self) -> None:
        self.run_button.setEnabled(False)
        self.cancel_button.setEnabled(True)
        self.verdict.set_status(StatusKind.ACTIVE, "Test läuft …")
        self.details.clear()
        self.run_requested.emit(str(self.pattern.currentData()), self.length.value(), self.timeout.value())

    def show_result(self, result: LoopbackResult) -> None:
        self.run_button.setEnabled(True)
        self.cancel_button.setEnabled(False)
        kind = {
            LoopbackVerdict.PASSED: StatusKind.READY,
            LoopbackVerdict.PARTIAL: StatusKind.WARNING,
            LoopbackVerdict.FAILED: StatusKind.ERROR,
        }[result.verdict]
        self.verdict.set_status(kind, result.verdict.label)
        lines = [result.summary(), f"Modemleitungen: {format_modem_status(result.modem)}"]
        if result.mismatches:
            shown = ", ".join(f"#{i}: {result.sent[i]:02X}→{result.received[i]:02X}" for i in result.mismatches[:20])
            lines.append(f"Abweichungen: {shown}")
        if result.error is not None:
            lines.append("")
            lines.append(result.error.as_text())
        if result.hints:
            lines.append("")
            lines.append("Mögliche Ursachen:")
            lines.extend(f"- {hint}" for hint in result.hints)
        self.details.setPlainText("\n".join(lines))

    def set_running(self, running: bool) -> None:
        self.run_button.setEnabled(not running)
        self.cancel_button.setEnabled(running)
