"""Confirmation before writing machine parameters (typed confirmation required)."""

from __future__ import annotations

from dataclasses import dataclass

from PySide6.QtWidgets import (
    QCheckBox,
    QDialog,
    QDialogButtonBox,
    QFormLayout,
    QLabel,
    QLineEdit,
    QPlainTextEdit,
    QVBoxLayout,
    QWidget,
)

from deckellink.gui.style import REAL_BANNER_STYLE, SIMULATION_BANNER_STYLE
from deckellink.parameters.service import WRITE_CONFIRMATION_PHRASE, ParameterDiff
from deckellink.safety import PARAMETER_WRITE_NOTICE


@dataclass(frozen=True)
class ParameterWriteInfo:
    simulation: bool
    port: str
    profile_name: str
    protocol: str
    file_name: str
    sha256: str
    backup_name: str
    backup_age_min: float
    diff: ParameterDiff


class ParameterWriteDialog(QDialog):
    def __init__(self, info: ParameterWriteInfo, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle("Maschinenparameter schreiben - Bestätigung")
        self.setModal(True)
        self.resize(760, 640)
        layout = QVBoxLayout(self)
        banner = QLabel(
            "⚠ SIMULATION - Parameter werden an den Simulator gesendet"
            if info.simulation
            else f"⚡ MASCHINENPARAMETER werden an die reale Maschine über {info.port} gesendet"
        )
        banner.setStyleSheet(SIMULATION_BANNER_STYLE if info.simulation else REAL_BANNER_STYLE)
        banner.setWordWrap(True)
        layout.addWidget(banner)
        notice = QLabel(PARAMETER_WRITE_NOTICE)
        notice.setWordWrap(True)
        notice.setStyleSheet("background:#fdecea; border:2px solid #b3261e; padding:8px; font-weight:bold;")
        layout.addWidget(notice)

        form = QFormLayout()
        form.addRow("Datei:", QLabel(info.file_name))
        form.addRow("SHA-256:", QLabel(info.sha256))
        form.addRow("Profil / Protokoll:", QLabel(f"{info.profile_name} / {info.protocol}"))
        form.addRow("Anschluss:", QLabel(info.port))
        form.addRow("Verglichen mit Sicherung:", QLabel(f"{info.backup_name} ({info.backup_age_min:.0f} min alt)"))
        form.addRow("Änderungen:", QLabel(info.diff.summary))
        layout.addLayout(form)
        diff = QPlainTextEdit(info.diff.diff_text)
        diff.setReadOnly(True)
        diff.setLineWrapMode(QPlainTextEdit.LineWrapMode.NoWrap)
        layout.addWidget(diff, 1)

        self.ack = QCheckBox(
            "Die Parametereingabe ist an der CNC durch eine berechtigte Person freigegeben, und ich habe "
            "alle Änderungen geprüft."
        )
        layout.addWidget(self.ack)
        layout.addWidget(QLabel(f"Zur Bestätigung eingeben: {WRITE_CONFIRMATION_PHRASE}"))
        self.phrase = QLineEdit()
        self.phrase.setAccessibleName("Bestätigungstext")
        layout.addWidget(self.phrase)

        buttons = QDialogButtonBox()
        self.write_button = buttons.addButton("Parameter schreiben", QDialogButtonBox.ButtonRole.AcceptRole)
        cancel = buttons.addButton("Abbrechen", QDialogButtonBox.ButtonRole.RejectRole)
        cancel.setDefault(True)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)
        self.write_button.setEnabled(False)
        self.ack.toggled.connect(self._update)
        self.phrase.textChanged.connect(self._update)

    def _update(self, *_args: object) -> None:
        self.write_button.setEnabled(self.ack.isChecked() and self.phrase.text().strip() == WRITE_CONFIRMATION_PHRASE)

    def confirmed(self) -> bool:
        return (
            self.result() == QDialog.DialogCode.Accepted
            and self.ack.isChecked()
            and self.phrase.text().strip() == WRITE_CONFIRMATION_PHRASE
        )


def ask_parameter_write(info: ParameterWriteInfo, parent: QWidget | None) -> bool:
    dialog = ParameterWriteDialog(info, parent)
    dialog.exec()
    return dialog.confirmed()
