"""Small dialogs of the DIALOG 11 simulator: program selection, DIA input, cutting data."""

from __future__ import annotations

import math
from pathlib import Path

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog,
    QDialogButtonBox,
    QDoubleSpinBox,
    QFileDialog,
    QFormLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
    QWidget,
)

from deckellink.dialog11sim.menus import DIA_FORMS, build_block
from deckellink.storage.library import EntryKind, ProgramLibrary


class SelectProgramDialog(QDialog):
    """ANWAHL / PROGRAMM VERWALTEN: choose a program from the program management or a file."""

    def __init__(
        self,
        library: ProgramLibrary | None,
        kinds: tuple[EntryKind, ...],
        parent: QWidget | None = None,
        *,
        with_start_block: bool = True,
    ) -> None:
        super().__init__(parent)
        self.setWindowTitle("Programm anwählen (Simulator)")
        self.resize(560, 420)
        self.chosen_path: Path | None = None
        self.chosen_name = ""
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel("Programme aus der Programmverwaltung von DeckelLink:"))
        self.list = QListWidget()
        entries = [e for e in library.entries() if e.kind in kinds] if library is not None else []
        for entry in entries:
            label = f"{entry.name}   [{entry.kind.label}{', Nr. ' + entry.number if entry.number else ''}]"
            item = QListWidgetItem(label)
            item.setData(Qt.ItemDataRole.UserRole, (str(entry.path), entry.name))
            self.list.addItem(item)
        if not entries:
            self.list.addItem(QListWidgetItem("(keine Einträge - Programme auf der Seite Programmverwaltung anlegen)"))
        self.list.itemDoubleClicked.connect(lambda _i: self.accept())
        layout.addWidget(self.list, 1)
        file_button = QPushButton("Andere Datei …")
        file_button.clicked.connect(self._choose_file)
        layout.addWidget(file_button)
        form = QFormLayout()
        self.start_block = QLineEdit()
        self.start_block.setPlaceholderText("leer = Programmanfang, sonst Satznummer, z. B. 120")
        if with_start_block:
            form.addRow("Vorlauf bis Satz N:", self.start_block)
        layout.addLayout(form)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def _choose_file(self) -> None:
        name, _ = QFileDialog.getOpenFileName(self, "Programm wählen", str(Path.home()))
        if name:
            self.chosen_path = Path(name)
            self.chosen_name = Path(name).name
            super().accept()

    def accept(self) -> None:
        item = self.list.currentItem()
        data = item.data(Qt.ItemDataRole.UserRole) if item is not None else None
        if data:
            self.chosen_path = Path(data[0])
            self.chosen_name = str(data[1])
        super().accept()

    def start_number(self) -> int | None:
        text = self.start_block.text().strip().upper().lstrip("N")
        return int(text) if text.isdigit() else None


class DiaDialog(QDialog):
    """DIA: dialog-supported input of one block."""

    def __init__(self, kind: str, number: int | None, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        default_g, fields = DIA_FORMS[kind]
        self.kind = kind
        self.number = number
        self.setWindowTitle(f"DIA - {default_g}")
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel("Nur gewünschte Adressen ausfüllen. Der Satz wird hinter der Cursorzeile eingefügt."))
        form = QFormLayout()
        self.fields: dict[str, QLineEdit] = {}
        for address, label in fields:
            edit = QLineEdit()
            if address == "G":
                edit.setPlaceholderText(default_g)
            form.addRow(f"{label}:", edit)
            self.fields[address] = edit
            edit.textChanged.connect(self._update_preview)
        layout.addLayout(form)
        self.preview = QLabel()
        self.preview.setStyleSheet("font-family: monospace; font-weight: bold;")
        layout.addWidget(self.preview)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel)
        buttons.button(QDialogButtonBox.StandardButton.Ok).setText("ABSCHLUSS (einfügen)")
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)
        self._update_preview()

    def values(self) -> dict[str, str]:
        return {address: edit.text() for address, edit in self.fields.items()}

    def block(self) -> str:
        return build_block(self.kind, self.values(), self.number)

    def _update_preview(self) -> None:
        self.preview.setText(self.block())


class CuttingDataDialog(QDialog):
    """SCHNITTDATEN: spindle speed and feed from cutting speed, tooth feed and flutes."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle("Schnittdaten berechnen")
        layout = QVBoxLayout(self)
        form = QFormLayout()
        self.diameter = _spin(0.1, 500, 10, " mm")
        self.vc = _spin(1, 2000, 100, " m/min")
        self.fz = _spin(0.001, 5, 0.05, " mm", decimals=3)
        self.flutes = QSpinBox()
        self.flutes.setRange(1, 50)
        self.flutes.setValue(3)
        form.addRow("Werkzeugdurchmesser D:", self.diameter)
        form.addRow("Schnittgeschwindigkeit vc:", self.vc)
        form.addRow("Vorschub je Zahn fz:", self.fz)
        form.addRow("Schneidenzahl z:", self.flutes)
        layout.addLayout(form)
        self.result_label = QLabel()
        self.result_label.setStyleSheet("font-weight: bold;")
        layout.addWidget(self.result_label)
        for widget in (self.diameter, self.vc, self.fz):
            widget.valueChanged.connect(self._update)
        self.flutes.valueChanged.connect(self._update)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Close)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)
        self._update()

    def values(self) -> tuple[float, float]:
        return cutting_data(self.diameter.value(), self.vc.value(), self.fz.value(), self.flutes.value())

    def _update(self) -> None:
        speed, feed = self.values()
        self.result_label.setText(f"Drehzahl S = {speed:.0f} 1/min    Vorschub F = {feed:.0f} mm/min")


def cutting_data(diameter: float, vc: float, fz: float, flutes: int) -> tuple[float, float]:
    """S = vc·1000 / (π·D), F = S·fz·z."""
    if diameter <= 0:
        return 0.0, 0.0
    speed = vc * 1000.0 / (math.pi * diameter)
    return speed, speed * fz * flutes


def _spin(lo: float, hi: float, value: float, suffix: str, decimals: int = 2) -> QDoubleSpinBox:
    spin = QDoubleSpinBox()
    spin.setDecimals(decimals)
    spin.setRange(lo, hi)
    spin.setValue(value)
    spin.setSuffix(suffix)
    return spin
