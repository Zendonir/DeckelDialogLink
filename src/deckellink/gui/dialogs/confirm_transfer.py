"""Confirmation immediately before a transfer (with optional safety notice)."""

from __future__ import annotations

from dataclasses import dataclass, field

from PySide6.QtWidgets import (
    QCheckBox,
    QDialog,
    QDialogButtonBox,
    QFormLayout,
    QLabel,
    QVBoxLayout,
    QWidget,
)

from deckellink.gui.style import REAL_BANNER_STYLE, SIMULATION_BANNER_STYLE
from deckellink.safety import DRIP_FEED_NOTICE, SAFETY_NOTICE, SCOPE_NOTICE


@dataclass(frozen=True)
class ConfirmInfo:
    simulation: bool
    file_name: str
    file_path: str
    transfer_bytes: int
    lines: int
    profile_name: str
    profile_verified: bool
    port: str
    serial_summary: str
    warnings: tuple[str, ...] = ()
    applied: tuple[str, ...] = ()
    show_safety_notice: bool = False
    appended_program_end: bool = False
    protocol: str = ""
    drip_feed: bool = False


@dataclass(frozen=True)
class ConfirmOutcome:
    accepted: bool
    suppress_safety_notice: bool = False
    notes: tuple[str, ...] = field(default=())


class ConfirmTransferDialog(QDialog):
    def __init__(self, info: ConfirmInfo, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle("Übertragung bestätigen")
        self.setModal(True)
        layout = QVBoxLayout(self)
        banner = QLabel(
            "⚠ SIMULATION - es werden keine Daten an eine Maschine gesendet"
            if info.simulation
            else f"⚡ REALE ÜBERTRAGUNG an die Maschine über {info.port}"
        )
        banner.setStyleSheet(SIMULATION_BANNER_STYLE if info.simulation else REAL_BANNER_STYLE)
        banner.setWordWrap(True)
        layout.addWidget(banner)

        form = QFormLayout()
        form.addRow("Datei:", QLabel(info.file_name))
        path = QLabel(info.file_path)
        path.setWordWrap(True)
        form.addRow("Pfad:", path)
        form.addRow("Transferdaten:", QLabel(f"{info.transfer_bytes} Bytes, {info.lines} Zeilen"))
        form.addRow(
            "Profil:",
            QLabel(f"{info.profile_name} ({'verifiziert' if info.profile_verified else 'NICHT verifiziert'})"),
        )
        form.addRow("Schnittstelle:", QLabel(f"{info.port} | {info.serial_summary}"))
        if info.applied:
            applied = QLabel("\n".join(info.applied))
            applied.setWordWrap(True)
            form.addRow("Aufbereitung:", applied)
        if info.appended_program_end:
            form.addRow("Achtung:", QLabel("Programmende wird auf Ihre Bestätigung hin ergänzt."))
        layout.addLayout(form)

        if info.warnings:
            warn = QLabel("Bestätigte Warnungen:\n" + "\n".join(f"⚠ {w}" for w in info.warnings))
            warn.setWordWrap(True)
            layout.addWidget(warn)

        scope = QLabel(SCOPE_NOTICE)
        scope.setWordWrap(True)
        layout.addWidget(scope)

        self.drip_ack = QCheckBox("Ich habe verstanden: Die Maschine muss bei Störungen an der CNC angehalten werden.")
        self.drip_ack.setVisible(False)
        if info.drip_feed:
            drip = QLabel(DRIP_FEED_NOTICE)
            drip.setWordWrap(True)
            drip.setStyleSheet("background:#fdecea; border:2px solid #b3261e; padding:8px; font-weight:bold;")
            layout.addWidget(drip)
            self.drip_ack.setVisible(True)
            layout.addWidget(self.drip_ack)
        self.suppress = QCheckBox("Sicherheitshinweis künftig nicht mehr anzeigen")
        self.suppress.setVisible(False)
        if info.show_safety_notice and not info.simulation:
            notice = QLabel("SICHERHEITSHINWEIS\n\n" + SAFETY_NOTICE)
            notice.setWordWrap(True)
            notice.setStyleSheet("background:#fff8e1; border:2px solid #b58a00; padding:8px;")
            layout.addWidget(notice)
            self.suppress.setVisible(True)
            self.suppress.setToolTip("Kann in den Einstellungen wieder aktiviert werden.")
            layout.addWidget(self.suppress)

        if info.protocol:
            form.addRow("Protokoll:", QLabel(info.protocol))
        buttons = QDialogButtonBox()
        self.start_button = buttons.addButton(
            "Übertragung starten" if not info.simulation else "Simulation starten",
            QDialogButtonBox.ButtonRole.AcceptRole,
        )
        cancel = buttons.addButton("Abbrechen", QDialogButtonBox.ButtonRole.RejectRole)
        cancel.setDefault(True)
        cancel.setFocus()
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)
        if info.drip_feed:
            self.start_button.setText("Nachladebetrieb starten")
            self.start_button.setEnabled(False)
            self.drip_ack.toggled.connect(self.start_button.setEnabled)

    def outcome(self) -> ConfirmOutcome:
        return ConfirmOutcome(
            accepted=self.result() == QDialog.DialogCode.Accepted,
            suppress_safety_notice=not self.suppress.isHidden() and self.suppress.isChecked(),
        )


def ask_confirmation(info: ConfirmInfo, parent: QWidget | None) -> ConfirmOutcome:
    dialog = ConfirmTransferDialog(info, parent)
    dialog.exec()
    return dialog.outcome()
