"""Header bar: machine, connection status, port, profile and operating mode."""

from __future__ import annotations

from PySide6.QtWidgets import QFrame, QHBoxLayout, QLabel, QSizePolicy, QVBoxLayout, QWidget

from deckellink.gui.style import REAL_BANNER_STYLE, SIMULATION_BANNER_STYLE, StatusKind
from deckellink.gui.widgets.status import StatusIndicator


class HeaderBar(QFrame):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName("HeaderBar")
        layout = QHBoxLayout(self)
        titles = QVBoxLayout()
        self.machine_label = QLabel("DECKEL FP2A")
        self.machine_label.setObjectName("MachineTitle")
        self.control_label = QLabel("DIALOG 11")
        self.control_label.setObjectName("ControlTitle")
        titles.addWidget(self.machine_label)
        titles.addWidget(self.control_label)
        layout.addLayout(titles)
        layout.addSpacing(20)

        info = QVBoxLayout()
        self.status = StatusIndicator()
        self.status.setSizePolicy(QSizePolicy.Policy.Ignored, QSizePolicy.Policy.Preferred)
        info.addWidget(self.status)
        self.connection_label = QLabel()
        self.connection_label.setSizePolicy(QSizePolicy.Policy.Ignored, QSizePolicy.Policy.Preferred)
        self.connection_label.setAccessibleName("Verbindungsparameter")
        info.addWidget(self.connection_label)
        self.profile_label = QLabel()
        self.profile_label.setSizePolicy(QSizePolicy.Policy.Ignored, QSizePolicy.Policy.Preferred)
        self.profile_label.setAccessibleName("Aktives Profil")
        info.addWidget(self.profile_label)
        layout.addLayout(info, 1)

        self.mode_banner = QLabel()
        self.mode_banner.setAccessibleName("Betriebsart")
        layout.addWidget(self.mode_banner)

    def set_mode(self, simulation: bool, port: str) -> None:
        if simulation:
            self.mode_banner.setText("⚠ SIMULATIONSMODUS\nkeine Verbindung zur Maschine")
            self.mode_banner.setStyleSheet(SIMULATION_BANNER_STYLE)
            self.mode_banner.setToolTip(
                "Alle Übertragungen laufen gegen den eingebauten Simulator. Es werden keine Daten "
                "an eine reale Maschine gesendet."
            )
        else:
            self.mode_banner.setText(f"⚡ REALBETRIEB\nSchnittstelle {port or '(kein Port)'}")
            self.mode_banner.setStyleSheet(REAL_BANNER_STYLE)
            self.mode_banner.setToolTip("Übertragungen gehen an die reale serielle Schnittstelle.")

    def set_connection(self, port: str, summary: str) -> None:
        self.connection_label.setText(f"{port}  |  {summary}")

    def set_profile(self, name: str, verified: bool) -> None:
        status = "verifiziert" if verified else "NICHT verifiziert"
        self.profile_label.setText(f"Profil: {name}  ({status})")

    def set_status(self, kind: StatusKind, text: str) -> None:
        self.status.set_status(kind, text)
