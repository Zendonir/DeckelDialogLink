"""Page "Maschinenparameter": read (backup), compare and - heavily guarded - write."""

from __future__ import annotations

from pathlib import Path

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QAbstractItemView,
    QCheckBox,
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QPlainTextEdit,
    QProgressBar,
    QPushButton,
    QSplitter,
    QTreeWidget,
    QTreeWidgetItem,
    QVBoxLayout,
    QWidget,
)

from deckellink.domain.results import TransferProgress
from deckellink.gui.style import StatusKind
from deckellink.gui.viewmodels import format_bytes
from deckellink.gui.widgets.error_panel import ErrorPanel
from deckellink.gui.widgets.findings_table import FindingsTable
from deckellink.gui.widgets.status import StatusIndicator
from deckellink.parameters.archive import StoredParameterSet
from deckellink.parameters.service import ParameterCandidate, ParameterDiff, ParameterWriteGate

SCOPE_TEXT = (
    "⚠ Maschinenparameter bestimmen u. a. Verfahrwege, Endschalter und Antriebsverhalten. DeckelLink "
    "liest Parameter nur, wenn Sie die Parameterausgabe an der CNC starten, und schreibt sie nur in eine "
    "an der CNC durch Berechtigte freigegebene Parametereingabe. Es werden keine Fernbefehle gesendet. "
    "Das Parameterformat der DIALOG 11 ist nicht verifiziert."
)


class ParametersPage(QWidget):
    read_requested = Signal()
    cancel_requested = Signal()
    open_folder_requested = Signal()
    unlock_toggled = Signal(bool)
    compare_requested = Signal(Path)
    select_requested = Signal(Path)
    load_external_requested = Signal()
    write_requested = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        root = QVBoxLayout(self)
        scope = QLabel(SCOPE_TEXT)
        scope.setWordWrap(True)
        scope.setStyleSheet("background:#fff8e1; border:2px solid #b58a00; padding:6px;")
        root.addWidget(scope)

        actions = QHBoxLayout()
        self.read_button = QPushButton("Parameter &auslesen (Sicherung)")
        self.read_button.setObjectName("PrimaryButton")
        self.read_button.setToolTip(
            "Port öffnen und auf die Parameterausgabe der CNC warten. Starten Sie danach die Ausgabe an der CNC."
        )
        self.read_button.clicked.connect(self.read_requested.emit)
        self.cancel_button = QPushButton("Abbrechen")
        self.cancel_button.setObjectName("CancelButton")
        self.cancel_button.setEnabled(False)
        self.cancel_button.clicked.connect(self.cancel_requested.emit)
        self.folder_button = QPushButton("Parameterordner öffnen")
        self.folder_button.clicked.connect(self.open_folder_requested.emit)
        for button in (self.read_button, self.cancel_button, self.folder_button):
            actions.addWidget(button)
        actions.addStretch(1)
        root.addLayout(actions)

        self.state = StatusIndicator()
        self.state.set_status(StatusKind.IDLE, "Keine Parameterübertragung aktiv")
        root.addWidget(self.state)
        self.progress = QProgressBar()
        self.progress.setRange(0, 1000)
        root.addWidget(self.progress)
        self.backup_label = QLabel("Aktuelle Sicherung: keine (in dieser Sitzung noch nicht ausgelesen)")
        self.backup_label.setWordWrap(True)
        root.addWidget(self.backup_label)
        self.verify_label = StatusIndicator()
        self.verify_label.setVisible(False)
        root.addWidget(self.verify_label)
        self.error_panel = ErrorPanel()
        root.addWidget(self.error_panel)

        splitter = QSplitter()
        left = QWidget()
        left_layout = QVBoxLayout(left)
        left_layout.setContentsMargins(0, 0, 0, 0)
        left_layout.addWidget(QLabel("Gespeicherte Parametersätze (werden nie überschrieben):"))
        self.tree = QTreeWidget()
        self.tree.setHeaderLabels(["Datei", "Ausgelesen", "Status", "Größe", "Profil", "SHA-256"])
        self.tree.setRootIsDecorated(False)
        self.tree.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        self.tree.header().setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        self.tree.setAccessibleName("Gespeicherte Parametersätze")
        self.tree.itemSelectionChanged.connect(self._update_buttons)
        left_layout.addWidget(self.tree, 1)
        row = QHBoxLayout()
        self.compare_button = QPushButton("Mit aktueller Sicherung &vergleichen")
        self.compare_button.clicked.connect(self._compare)
        self.select_button = QPushButton("Zum Schreiben auswählen")
        self.select_button.clicked.connect(self._select)
        self.external_button = QPushButton("Externe Datei zum Schreiben laden …")
        self.external_button.clicked.connect(self.load_external_requested.emit)
        for button in (self.compare_button, self.select_button, self.external_button):
            row.addWidget(button)
        left_layout.addLayout(row)
        splitter.addWidget(left)

        right = QWidget()
        right_layout = QVBoxLayout(right)
        right_layout.setContentsMargins(0, 0, 0, 0)
        self.diff_summary = QLabel("Vergleich: –")
        right_layout.addWidget(self.diff_summary)
        self.diff_view = QPlainTextEdit()
        self.diff_view.setReadOnly(True)
        self.diff_view.setLineWrapMode(QPlainTextEdit.LineWrapMode.NoWrap)
        self.diff_view.setAccessibleName("Unterschiede der Parameter")
        right_layout.addWidget(self.diff_view, 1)

        write_box = QGroupBox("Parameter schreiben")
        write_layout = QVBoxLayout(write_box)
        self.unlock_box = QCheckBox("Schreibfunktion für diese Sitzung freischalten")
        self.unlock_box.setToolTip("Wird beim Beenden von DeckelLink automatisch zurückgesetzt.")
        self.unlock_box.toggled.connect(self.unlock_toggled.emit)
        write_layout.addWidget(self.unlock_box)
        form = QFormLayout()
        self.candidate_name = QLabel("–")
        self.candidate_sha = QLabel("–")
        self.candidate_format = QLabel("–")
        self.candidate_format.setWordWrap(True)
        form.addRow("Datei:", self.candidate_name)
        form.addRow("SHA-256:", self.candidate_sha)
        form.addRow("Aufbau:", self.candidate_format)
        write_layout.addLayout(form)
        self.findings = FindingsTable()
        self.findings.setMaximumHeight(120)
        write_layout.addWidget(self.findings)
        self.gate_label = QLabel()
        self.gate_label.setWordWrap(True)
        self.gate_label.setAccessibleName("Schreibvoraussetzungen")
        write_layout.addWidget(self.gate_label)
        self.write_button = QPushButton("Parameter an Maschine &schreiben …")
        self.write_button.setObjectName("CancelButton")
        self.write_button.setEnabled(False)
        self.write_button.clicked.connect(self.write_requested.emit)
        write_layout.addWidget(self.write_button)
        right_layout.addWidget(write_box)
        splitter.addWidget(right)
        splitter.setSizes([480, 520])
        root.addWidget(splitter, 1)
        self._busy = False
        self._update_buttons()

    # ------------------------------------------------------------------
    def _selected(self) -> Path | None:
        items = self.tree.selectedItems()
        if not items:
            return None
        value = items[0].data(0, Qt.ItemDataRole.UserRole)
        return Path(value) if value else None

    def _compare(self) -> None:
        path = self._selected()
        if path is not None:
            self.compare_requested.emit(path)

    def _select(self) -> None:
        path = self._selected()
        if path is not None:
            self.select_requested.emit(path)

    def _update_buttons(self) -> None:
        selected = self._selected() is not None
        self.compare_button.setEnabled(selected and not self._busy)
        self.select_button.setEnabled(selected and not self._busy)
        self.external_button.setEnabled(not self._busy)

    def set_sets(self, sets: list[StoredParameterSet]) -> None:
        self.tree.clear()
        for item_set in sets:
            meta = item_set.metadata
            status = "✔ vollständig" if item_set.complete else "⚠ unvollständig/ungeprüft"
            read_at = item_set.read_at
            item = QTreeWidgetItem(
                [
                    item_set.path.name,
                    read_at.strftime("%d.%m.%Y %H:%M:%S") if read_at else "–",
                    status,
                    format_bytes(int(meta.get("normalized_bytes", 0))),
                    str(meta.get("profile_id", "–")),
                    item_set.sha256[:16],
                ]
            )
            item.setData(0, Qt.ItemDataRole.UserRole, str(item_set.path))
            self.tree.addTopLevelItem(item)
        self._update_buttons()

    def set_busy(self, busy: bool, reading_or_writing: bool) -> None:
        self._busy = busy
        self.read_button.setEnabled(not busy)
        self.cancel_button.setEnabled(reading_or_writing)
        self.unlock_box.setEnabled(not busy)
        self._update_buttons()

    def set_unlocked(self, unlocked: bool) -> None:
        self.unlock_box.blockSignals(True)
        self.unlock_box.setChecked(unlocked)
        self.unlock_box.blockSignals(False)

    def show_diff(self, diff: ParameterDiff | None, title: str) -> None:
        if diff is None:
            self.diff_summary.setText(f"Vergleich: {title}")
            self.diff_view.clear()
            return
        self.diff_summary.setText(f"Vergleich {title}: {diff.summary}")
        self.diff_view.setPlainText(diff.diff_text or "Keine Unterschiede.")

    def show_candidate(self, candidate: ParameterCandidate | None) -> None:
        if candidate is None:
            self.candidate_name.setText("–")
            self.candidate_sha.setText("–")
            self.candidate_format.setText("–")
            self.findings.set_findings([])
            return
        self.candidate_name.setText(candidate.name)
        self.candidate_sha.setText(candidate.analysis.sha256[:32] + "…")
        structure = candidate.structure
        self.candidate_format.setText(structure.summary if structure else "Keine Parametergruppen erkannt")
        self.findings.set_findings(candidate.analysis.findings)

    def apply_gate(self, gate: ParameterWriteGate) -> None:
        self.write_button.setEnabled(gate.allowed)
        self.gate_label.setText(
            "⚠ Schreiben möglich - Bestätigung folgt" if gate.allowed else f"Schreiben gesperrt: {gate.reason_text}"
        )

    def set_progress(self, progress: TransferProgress) -> None:
        self.progress.setValue(int(progress.percent * 10))

    def set_received_bytes(self, count: int) -> None:
        self.progress.setRange(0, 0 if count else 1000)
        self.state.set_status(StatusKind.ACTIVE, f"Empfange Parameter: {count} Bytes")
