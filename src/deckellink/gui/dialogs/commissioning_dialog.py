"""Commissioning assistant dialog."""

from __future__ import annotations

from datetime import datetime

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QAbstractItemView,
    QComboBox,
    QDialog,
    QFormLayout,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
)

from deckellink.commissioning import STEPS, CommissioningRecord, StepStatus
from deckellink.domain.models import Profile, VerificationStatus

ACTION_LABELS = {
    "loopback": "Loopback-Test …",
    "format": "Datenformat …",
    "receive": "Zum Empfang",
    "send": "Zum Senden",
    "parameters": "Zu Parametern",
    "help": "Anleitung …",
}


class CommissioningDialog(QDialog):
    action_requested = Signal(str)
    changed = Signal()
    export_requested = Signal()
    mark_verified_requested = Signal()

    def __init__(self, record: CommissioningRecord, profile: Profile, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.record = record
        self.setWindowTitle("Inbetriebnahme-Assistent")
        self.setModal(False)
        self.resize(1000, 640)
        layout = QVBoxLayout(self)
        verified = profile.verification_status is VerificationStatus.VERIFIED
        intro = QLabel(
            f"Profil: <b>{profile.display_name}</b> ({'verifiziert' if verified else 'nicht verifiziert'}).<br>"
            "Arbeiten Sie die Schritte nach der Inbetriebnahme-Anleitung ab und dokumentieren Sie das Ergebnis. "
            "Das Profil kann erst als verifiziert markiert werden, wenn alle Pflichtschritte erledigt sind."
        )
        intro.setWordWrap(True)
        layout.addWidget(intro)
        form = QFormLayout()
        self.tester = QLineEdit(record.tester)
        self.tester.setPlaceholderText("Name der prüfenden Person")
        self.tester.textEdited.connect(self._meta_changed)
        form.addRow("Prüfer:", self.tester)
        self.machine = QLineEdit(record.machine_info)
        self.machine.setPlaceholderText("z. B. Maschinennummer, Steuerungs-/Softwarestand")
        self.machine.textEdited.connect(self._meta_changed)
        form.addRow("Maschine:", self.machine)
        layout.addLayout(form)

        self.table = QTableWidget(len(STEPS), 5)
        self.table.setHorizontalHeaderLabels(["Schritt", "Pflicht", "Status", "Notiz", "Aktion"])
        self.table.verticalHeader().setVisible(False)
        self.table.setSelectionMode(QAbstractItemView.SelectionMode.NoSelection)
        header = self.table.horizontalHeader()
        for column in (0, 1, 2, 4):
            header.setSectionResizeMode(column, QHeaderView.ResizeMode.ResizeToContents)
        header.setSectionResizeMode(3, QHeaderView.ResizeMode.Stretch)
        self.status_boxes: dict[str, QComboBox] = {}
        self.note_edits: dict[str, QLineEdit] = {}
        for row, step in enumerate(STEPS):
            item = QTableWidgetItem(f"{row + 1}. {step.title}")
            item.setToolTip(step.description)
            self.table.setItem(row, 0, item)
            self.table.setItem(row, 1, QTableWidgetItem("ja" if step.required else "nein"))
            combo = QComboBox()
            for status in StepStatus:
                combo.addItem(status.label, status.value)
            combo.setCurrentIndex(combo.findData(record.step(step.id).status.value))
            combo.currentIndexChanged.connect(lambda _i, s=step.id: self._status_changed(s))
            self.table.setCellWidget(row, 2, combo)
            self.status_boxes[step.id] = combo
            note = QLineEdit(record.step(step.id).note)
            note.setToolTip(step.description)
            note.textEdited.connect(lambda text, s=step.id: self._note_changed(s, text))
            self.table.setCellWidget(row, 3, note)
            self.note_edits[step.id] = note
            if step.action:
                button = QPushButton(ACTION_LABELS[step.action])
                button.clicked.connect(lambda _c=False, a=step.action: self.action_requested.emit(a))
                self.table.setCellWidget(row, 4, button)
        layout.addWidget(self.table, 1)

        self.summary = QLabel()
        self.summary.setWordWrap(True)
        layout.addWidget(self.summary)
        buttons = QHBoxLayout()
        self.export_button = QPushButton("Protokoll &exportieren …")
        self.export_button.clicked.connect(self.export_requested.emit)
        self.verify_button = QPushButton("Profil als &verifiziert markieren …")
        self.verify_button.setObjectName("PrimaryButton")
        self.verify_button.clicked.connect(self.mark_verified_requested.emit)
        close = QPushButton("Schließen")
        close.clicked.connect(self.close)
        buttons.addWidget(self.export_button)
        buttons.addWidget(self.verify_button)
        buttons.addStretch(1)
        buttons.addWidget(close)
        layout.addLayout(buttons)
        self._update_summary()

    def _meta_changed(self, *_args: object) -> None:
        self.record.tester = self.tester.text().strip()
        self.record.machine_info = self.machine.text().strip()
        self._update_summary()
        self.changed.emit()

    def _status_changed(self, step_id: str) -> None:
        combo = self.status_boxes[step_id]
        self.record.set_status(step_id, StepStatus(combo.currentData()), datetime.now().astimezone())
        self._update_summary()
        self.changed.emit()

    def _note_changed(self, step_id: str, text: str) -> None:
        self.record.step(step_id).note = text
        self.changed.emit()

    def set_status(self, step_id: str, status: StepStatus) -> None:
        combo = self.status_boxes[step_id]
        combo.setCurrentIndex(combo.findData(status.value))

    def _update_summary(self) -> None:
        missing = self.record.missing_required()
        done = self.record.done_count()
        if missing:
            text = f"{done} von {len(STEPS)} Schritten bearbeitet. Offene Pflichtschritte: " + ", ".join(
                s.title for s in missing
            )
        else:
            text = f"{done} von {len(STEPS)} Schritten bearbeitet. Alle Pflichtschritte erledigt."
        if not self.record.tester.strip():
            text = text.rstrip(".") + ". Bitte Prüfer eintragen."
        self.summary.setText(text)
        self.verify_button.setEnabled(self.record.can_mark_verified())
