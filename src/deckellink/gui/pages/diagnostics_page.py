"""Page "Diagnose": port state, modem lines, counters, loopback and export."""

from __future__ import annotations

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from deckellink.domain.models import Profile
from deckellink.gui.style import StatusKind
from deckellink.gui.widgets.error_panel import ErrorPanel
from deckellink.gui.widgets.status import SignalPanel, StatusIndicator
from deckellink.transport.base import ModemStatus


class DiagnosticsPage(QWidget):
    monitor_toggled = Signal(bool)
    control_line_requested = Signal(str, bool)
    loopback_requested = Signal()
    export_requested = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        root = QVBoxLayout(self)
        actions = QHBoxLayout()
        self.monitor_button = QPushButton("Port zur &Diagnose öffnen")
        self.monitor_button.setCheckable(True)
        self.monitor_button.setToolTip(
            "Öffnet den Port nur zum Beobachten der Modemleitungen. Es werden keine Daten gesendet."
        )
        self.monitor_button.toggled.connect(self.monitor_toggled.emit)
        self.loopback_button = QPushButton("&Loopback-Test …")
        self.loopback_button.clicked.connect(self.loopback_requested.emit)
        self.export_button = QPushButton("Diagnosepaket &exportieren …")
        self.export_button.clicked.connect(self.export_requested.emit)
        for button in (self.monitor_button, self.loopback_button, self.export_button):
            actions.addWidget(button)
        actions.addStretch(1)
        root.addLayout(actions)
        self.error_panel = ErrorPanel()
        root.addWidget(self.error_panel)

        port_box = QGroupBox("Schnittstelle")
        form = QFormLayout(port_box)
        self.port_state = StatusIndicator()
        self.port_state.set_status(StatusKind.IDLE, "Port geschlossen")
        form.addRow("Portstatus:", self.port_state)
        self.port = QLabel("–")
        self.profile = QLabel("–")
        self.baud = QLabel("–")
        self.frame = QLabel("–")
        self.flow = QLabel("–")
        self.tx = QLabel("0")
        self.rx = QLabel("0")
        self.last_error = QLabel("–")
        self.last_error.setWordWrap(True)
        self.last_state = QLabel("–")
        form.addRow("Port:", self.port)
        form.addRow("Profil:", self.profile)
        form.addRow("Baudrate:", self.baud)
        form.addRow("Datenrahmen:", self.frame)
        form.addRow("Flow Control:", self.flow)
        form.addRow("Gesendete Bytes:", self.tx)
        form.addRow("Empfangene Bytes:", self.rx)
        form.addRow("Letzter Fehler:", self.last_error)
        form.addRow("Letzter Zustandswechsel:", self.last_state)
        root.addWidget(port_box)

        signal_box = QGroupBox("Modemleitungen")
        signal_layout = QVBoxLayout(signal_box)
        self.signals = SignalPanel(controllable=True)
        self.signals.control_requested.connect(self.control_line_requested.emit)
        signal_layout.addWidget(self.signals)
        self.signal_hint = QLabel(
            "Eingänge (CTS, DSR, DCD, RI) zeigen den Zustand der Gegenstelle. RTS/DTR können nur bei "
            "geöffnetem Diagnoseport geschaltet werden. Bei aktivem RTS/CTS-Handshake steuert der Treiber RTS."
        )
        self.signal_hint.setWordWrap(True)
        signal_layout.addWidget(self.signal_hint)
        root.addWidget(signal_box)
        root.addStretch(1)

    def set_profile(self, profile: Profile, port: str) -> None:
        self.port.setText(port)
        self.profile.setText(f"{profile.display_name} [{profile.id}]")
        self.baud.setText(str(profile.serial.baud_rate))
        self.frame.setText(profile.serial.frame)
        self.flow.setText(profile.serial.flow_control_label)

    def set_counters(self, tx: int, rx: int) -> None:
        self.tx.setText(str(tx))
        self.rx.setText(str(rx))

    def set_port_open(self, is_open: bool, text: str = "") -> None:
        if is_open:
            self.port_state.set_status(StatusKind.READY, text or "Port geöffnet")
        else:
            self.port_state.set_status(StatusKind.IDLE, text or "Port geschlossen")
        self.signals.set_controls_enabled(is_open)
        if not is_open:
            self.signals.set_status(ModemStatus())
        self.monitor_button.blockSignals(True)
        self.monitor_button.setChecked(is_open)
        self.monitor_button.setText("Diagnoseport &schließen" if is_open else "Port zur &Diagnose öffnen")
        self.monitor_button.blockSignals(False)

    def set_busy(self, busy: bool, monitoring: bool) -> None:
        self.monitor_button.setEnabled(not busy or monitoring)
        self.loopback_button.setEnabled(not busy)
