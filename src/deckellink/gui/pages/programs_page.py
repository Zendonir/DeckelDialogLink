"""Page "NC-Programme": archive of received programs."""

from __future__ import annotations

import json
from pathlib import Path

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QAbstractItemView,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QSplitter,
    QTreeWidget,
    QTreeWidgetItem,
    QVBoxLayout,
    QWidget,
)

from deckellink.gui.viewmodels import format_bytes
from deckellink.storage.archive import ArchivedProgram

PREVIEW_LIMIT = 200_000


class ProgramsPage(QWidget):
    refresh_requested = Signal()
    open_folder_requested = Signal()
    export_requested = Signal(Path)
    load_for_send_requested = Signal(Path)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        root = QVBoxLayout(self)
        actions = QHBoxLayout()
        self.refresh_button = QPushButton("&Aktualisieren")
        self.refresh_button.clicked.connect(self.refresh_requested.emit)
        self.folder_button = QPushButton("Ordner öffnen")
        self.folder_button.clicked.connect(self.open_folder_requested.emit)
        self.export_button = QPushButton("Speichern &unter …")
        self.export_button.setToolTip("Kopie speichern. Vorhandene Zieldateien werden vorher gesichert.")
        self.export_button.clicked.connect(self._export_selected)
        self.load_button = QPushButton("Zum Senden &laden")
        self.load_button.clicked.connect(self._load_selected)
        for button in (self.refresh_button, self.folder_button, self.export_button, self.load_button):
            actions.addWidget(button)
        actions.addStretch(1)
        root.addLayout(actions)
        self.info = QLabel(
            "Empfangene Programme (vollständig: 'received', unvollständig: 'failed'). Dateien werden nie überschrieben."
        )
        self.info.setWordWrap(True)
        root.addWidget(self.info)

        splitter = QSplitter(Qt.Orientation.Vertical)
        self.tree = QTreeWidget()
        self.tree.setHeaderLabels(["Datei", "Status", "Größe", "Datum", "Profil", "SHA-256"])
        self.tree.setRootIsDecorated(False)
        self.tree.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        self.tree.header().setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        self.tree.itemSelectionChanged.connect(self._selection_changed)
        self.tree.setAccessibleName("Liste empfangener Programme")
        splitter.addWidget(self.tree)
        details = QSplitter()
        self.meta_view = QPlainTextEdit()
        self.meta_view.setReadOnly(True)
        self.content_view = QPlainTextEdit()
        self.content_view.setReadOnly(True)
        details.addWidget(self.meta_view)
        details.addWidget(self.content_view)
        splitter.addWidget(details)
        root.addWidget(splitter, 1)
        self._update_buttons()

    def _selected_path(self) -> Path | None:
        items = self.tree.selectedItems()
        if not items:
            return None
        value = items[0].data(0, Qt.ItemDataRole.UserRole)
        return Path(value) if value else None

    def _export_selected(self) -> None:
        path = self._selected_path()
        if path is not None:
            self.export_requested.emit(path)

    def _load_selected(self) -> None:
        path = self._selected_path()
        if path is not None:
            self.load_for_send_requested.emit(path)

    def _update_buttons(self) -> None:
        selected = self._selected_path() is not None
        self.export_button.setEnabled(selected)
        self.load_button.setEnabled(selected)

    def set_programs(self, programs: list[ArchivedProgram]) -> None:
        self.tree.clear()
        for program in programs:
            meta = program.metadata or {}
            if program.complete is None:
                status = "? ohne Metadaten"
            elif program.complete:
                status = "✔ vollständig"
            else:
                status = "⚠ unvollständig"
            item = QTreeWidgetItem(
                [
                    program.path.name,
                    status,
                    format_bytes(program.size),
                    program.modified.strftime("%d.%m.%Y %H:%M:%S"),
                    str(meta.get("profile_id", "–")),
                    str(meta.get("sha256_normalized", ""))[:16],
                ]
            )
            item.setData(0, Qt.ItemDataRole.UserRole, str(program.path))
            item.setToolTip(0, str(program.path))
            self.tree.addTopLevelItem(item)
        self.meta_view.clear()
        self.content_view.clear()
        self._update_buttons()

    def _selection_changed(self) -> None:
        self._update_buttons()
        path = self._selected_path()
        if path is None:
            return
        meta_path = path.with_suffix(".json")
        try:
            meta = json.loads(meta_path.read_text(encoding="utf-8")) if meta_path.is_file() else None
            self.meta_view.setPlainText(
                json.dumps(meta, indent=2, ensure_ascii=False) if meta else "Keine Metadaten vorhanden."
            )
            data = path.read_bytes()[:PREVIEW_LIMIT]
            self.content_view.setPlainText(data.decode("ascii", errors="replace"))
        except (OSError, json.JSONDecodeError) as exc:
            self.meta_view.setPlainText(f"Datei konnte nicht gelesen werden: {exc}")
