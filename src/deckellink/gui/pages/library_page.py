"""Page "Programmverwaltung": NC programs, macros, tool files and the tool table."""

from __future__ import annotations

import logging
from collections.abc import Callable
from datetime import datetime
from pathlib import Path

from PySide6.QtCore import Qt, Signal, SignalInstance
from PySide6.QtWidgets import (
    QAbstractItemView,
    QComboBox,
    QFileDialog,
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMessageBox,
    QPlainTextEdit,
    QPushButton,
    QSplitter,
    QTableWidget,
    QTableWidgetItem,
    QTabWidget,
    QTreeWidget,
    QTreeWidgetItem,
    QVBoxLayout,
    QWidget,
)

from deckellink.domain.errors import DeckelLinkError
from deckellink.gui.viewmodels import format_bytes
from deckellink.storage.library import EntryKind, LibraryEntry, LibraryError, ProgramLibrary
from deckellink.storage.tooldb import (
    PLACEHOLDERS,
    Tool,
    ToolDatabase,
    ToolDataError,
    ToolExportTemplate,
    check_program_tools,
    export_tool_file,
)

log = logging.getLogger(__name__)

PREVIEW_LIMIT = 100_000
TOOL_COLUMNS = ("T-Nr.", "Bezeichnung", "Typ", "Ø mm", "Länge mm", "Schneiden", "F mm/min", "S 1/min", "Bemerkung")
NOW: Callable[[], datetime] = lambda: datetime.now().astimezone()  # noqa: E731


class LibraryPage(QWidget):
    """Program and tool management. Sending always goes through the "Übertragen" page."""

    send_requested = Signal(str, bool)  # entry id, drip feed
    simulate_requested = Signal(str, bool)  # entry id, edit mode
    template_changed = Signal(object)  # ToolExportTemplate
    tools_changed = Signal()
    open_folder_requested = Signal(Path)

    def __init__(
        self, library: ProgramLibrary, tools: ToolDatabase, template: ToolExportTemplate, parent: QWidget | None = None
    ) -> None:
        super().__init__(parent)
        self.library = library
        self.tool_db = tools
        self.template = template
        self.entries: list[LibraryEntry] = []
        root = QVBoxLayout(self)
        intro = QLabel(
            "Programme, Makros und Werkzeugdateien auf dem PC verwalten. Jede Änderung legt eine neue Version an, "
            "Löschen verschiebt in den Papierkorb. An die Maschine geht eine Datei nur über die Seite "
            "<i>Übertragen</i> mit der üblichen Bestätigung."
        )
        intro.setWordWrap(True)
        root.addWidget(intro)
        self.tabs = QTabWidget()
        self.tabs.addTab(self._build_programs(), "Programme")
        self.tabs.addTab(self._build_tools(), "Werkzeuge")
        root.addWidget(self.tabs, 1)
        self.refresh()
        self.load_tools()

    # ------------------------------------------------------------------ programs tab
    def _build_programs(self) -> QWidget:
        page = QWidget()
        layout = QVBoxLayout(page)
        buttons = QHBoxLayout()
        self.import_button = QPushButton("&Importieren …")
        self.import_button.clicked.connect(self._import_dialog)
        self.version_button = QPushButton("Neue Version aus Datei …")
        self.version_button.clicked.connect(self._version_dialog)
        self.edit_button = QPushButton("Im Simulator &bearbeiten")
        self.edit_button.clicked.connect(lambda: self._emit_selected(self.simulate_requested, True))
        self.view_button = QPushButton("Im Simulator ansehen")
        self.view_button.clicked.connect(lambda: self._emit_selected(self.simulate_requested, False))
        self.send_button = QPushButton("Zum &Senden laden")
        self.send_button.clicked.connect(lambda: self._emit_selected(self.send_requested, False))
        self.drip_button = QPushButton("&Nachladebetrieb (LSV2) …")
        self.drip_button.setToolTip(
            "Lädt die Datei auf der Seite Übertragen und startet den Nachladebetrieb "
            "(Bestätigung folgt; Start der Bearbeitung nur an der CNC)."
        )
        self.drip_button.clicked.connect(lambda: self._emit_selected(self.send_requested, True))
        self.delete_button = QPushButton("In Papierkorb")
        self.delete_button.clicked.connect(self._delete_selected)
        self.folder_button = QPushButton("Ordner öffnen")
        self.folder_button.clicked.connect(lambda: self.open_folder_requested.emit(self.library.root))
        for button in (self.import_button, self.version_button, self.delete_button, self.folder_button):
            buttons.addWidget(button)
        buttons.addStretch(1)
        layout.addLayout(buttons)
        machine = QHBoxLayout()
        for button in (self.edit_button, self.view_button, self.send_button, self.drip_button):
            machine.addWidget(button)
        machine.addStretch(1)
        layout.addLayout(machine)

        splitter = QSplitter(Qt.Orientation.Horizontal)
        self.tree = QTreeWidget()
        self.tree.setHeaderLabels(["Name", "Art", "Nr.", "Werkzeuge", "Größe", "Geändert", "Zuletzt gesendet"])
        self.tree.setRootIsDecorated(False)
        self.tree.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        self.tree.header().setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        self.tree.currentItemChanged.connect(lambda *_: self._show_selected())
        splitter.addWidget(self.tree)

        detail = QWidget()
        detail_layout = QVBoxLayout(detail)
        form = QFormLayout()
        self.name_edit = QLineEdit()
        self.kind_combo = QComboBox()
        for kind in EntryKind:
            self.kind_combo.addItem(kind.label, kind.value)
        self.number_edit = QLineEdit()
        self.number_edit.setPlaceholderText("Programmnummer an der Steuerung, z. B. 17")
        self.description_edit = QLineEdit()
        form.addRow("Name:", self.name_edit)
        form.addRow("Art:", self.kind_combo)
        form.addRow("Nummer:", self.number_edit)
        form.addRow("Beschreibung:", self.description_edit)
        detail_layout.addLayout(form)
        self.meta_button = QPushButton("Angaben speichern")
        self.meta_button.clicked.connect(self._save_meta)
        detail_layout.addWidget(self.meta_button)
        self.tool_check = QLabel()
        self.tool_check.setWordWrap(True)
        detail_layout.addWidget(self.tool_check)
        self.preview = QPlainTextEdit()
        self.preview.setReadOnly(True)
        self.preview.setLineWrapMode(QPlainTextEdit.LineWrapMode.NoWrap)
        detail_layout.addWidget(self.preview, 1)
        detail_layout.addWidget(QLabel("Frühere Versionen (neueste zuerst):"))
        self.versions = QListWidget()
        self.versions.setMaximumHeight(110)
        detail_layout.addWidget(self.versions)
        self.restore_button = QPushButton("Gewählte Version wiederherstellen (als neue Version)")
        self.restore_button.clicked.connect(self._restore_version)
        detail_layout.addWidget(self.restore_button)
        splitter.addWidget(detail)
        splitter.setSizes([560, 520])
        layout.addWidget(splitter, 1)
        return page

    def refresh(self, select: str | None = None) -> None:
        selected = self.selected()
        current = select or (selected.id if selected is not None else None)
        self.entries = self.library.entries()
        self.tree.clear()
        for entry in self.entries:
            item = QTreeWidgetItem(
                [
                    entry.name,
                    entry.kind.label,
                    entry.number,
                    " ".join(entry.tools),
                    format_bytes(entry.size),
                    entry.updated_at.replace("T", " ")[:19],
                    (entry.last_sent_at.replace("T", " ")[:19] + f" ({entry.last_sent_mode})")
                    if entry.last_sent_at
                    else "",
                ]
            )
            item.setData(0, Qt.ItemDataRole.UserRole, entry.id)
            self.tree.addTopLevelItem(item)
            if entry.id == current:
                self.tree.setCurrentItem(item)
        self._show_selected()

    def selected(self) -> LibraryEntry | None:
        item = self.tree.currentItem() if hasattr(self, "tree") else None
        if item is None:
            return None
        entry_id = item.data(0, Qt.ItemDataRole.UserRole)
        return next((e for e in self.entries if e.id == entry_id), None)

    def select(self, entry_id: str) -> None:
        for index in range(self.tree.topLevelItemCount()):
            item = self.tree.topLevelItem(index)
            if item is not None and item.data(0, Qt.ItemDataRole.UserRole) == entry_id:
                self.tree.setCurrentItem(item)
                return

    def _show_selected(self) -> None:
        entry = self.selected()
        has = entry is not None
        for widget in (
            self.version_button,
            self.edit_button,
            self.view_button,
            self.send_button,
            self.drip_button,
            self.delete_button,
            self.meta_button,
            self.restore_button,
        ):
            widget.setEnabled(has)
        self.versions.clear()
        if entry is None:
            self.preview.setPlainText("")
            self.tool_check.setText("")
            for edit in (self.name_edit, self.number_edit, self.description_edit):
                edit.setText("")
            return
        self.name_edit.setText(entry.name)
        self.kind_combo.setCurrentIndex(self.kind_combo.findData(entry.kind.value))
        self.number_edit.setText(entry.number)
        self.description_edit.setText(entry.description)
        try:
            data = entry.read()
        except OSError as exc:
            self.preview.setPlainText(f"Datei nicht lesbar: {exc}")
            return
        text = data[:PREVIEW_LIMIT].decode("ascii", errors="replace")
        self.preview.setPlainText(text + ("\n… (gekürzt)" if len(data) > PREVIEW_LIMIT else ""))
        self.tool_check.setText(self.tool_check_text(entry))
        for path in self.library.versions(entry):
            item = QListWidgetItem(path.stem)
            item.setData(Qt.ItemDataRole.UserRole, str(path))
            self.versions.addItem(item)
        self.edit_button.setEnabled(entry.kind in (EntryKind.PROGRAM, EntryKind.MACRO))
        self.drip_button.setEnabled(entry.kind is EntryKind.PROGRAM)

    def tool_check_text(self, entry: LibraryEntry) -> str:
        if entry.kind is EntryKind.TOOLS:
            return (
                "⚠ Werkzeugdatei: Beim Einlesen an der Steuerung werden Werkzeugdaten überschrieben. "
                "Nur Dateien senden, deren Format von der Steuerung stammt bzw. geprüft ist."
            )
        if not entry.tools:
            return "Keine Werkzeugaufrufe gefunden."
        missing = check_program_tools(entry.tools, self.tool_db.tools)
        if missing:
            return f"⚠ Nicht in der Werkzeugtabelle: {', '.join(missing)} (verwendet: {', '.join(entry.tools)})"
        return f"✔ Alle Werkzeuge in der Werkzeugtabelle ({', '.join(entry.tools)})"

    def _emit_selected(self, signal: SignalInstance, flag: bool) -> None:
        entry = self.selected()
        if entry is not None:
            signal.emit(entry.id, flag)

    # ------------------------------------------------------------------ program actions
    def _import_dialog(self) -> None:
        names, _ = QFileDialog.getOpenFileNames(
            self,
            "Dateien in die Programmverwaltung übernehmen",
            str(Path.home()),
            "NC-Programme (*.nc *.txt *.tap *.d11 *.mpf);;Alle Dateien (*)",
        )
        for name in names:
            self.import_path(Path(name))

    def import_path(self, path: Path, kind: EntryKind | None = None) -> LibraryEntry | None:
        try:
            entry = self.library.import_file(path, kind=kind, now=NOW())
        except (LibraryError, DeckelLinkError, OSError) as exc:
            self._warn("Importieren", str(exc))
            return None
        self.refresh(entry.id)
        return entry

    def _version_dialog(self) -> None:
        entry = self.selected()
        if entry is None:
            return
        name, _ = QFileDialog.getOpenFileName(self, f"Neue Version für {entry.name}", str(Path.home()))
        if name:
            self.new_version_from(entry, Path(name))

    def new_version_from(self, entry: LibraryEntry, path: Path) -> bool:
        try:
            self.library.save_version(entry, path.read_bytes(), now=NOW())
        except (LibraryError, DeckelLinkError, OSError) as exc:
            self._warn("Neue Version", str(exc))
            return False
        self.refresh(entry.id)
        return True

    def save_content(self, entry_id: str, data: bytes) -> str:
        """Called from the simulator: store edited text as a new version."""
        entry = self.library.get(entry_id)
        if entry is None:
            raise LibraryError("Eintrag existiert nicht mehr.")
        self.library.save_version(entry, data, now=NOW())
        self.refresh(entry_id)
        return f"Als neue Version von '{entry.name}' gespeichert"

    def _save_meta(self) -> None:
        entry = self.selected()
        if entry is None:
            return
        self.library.update_meta(
            entry,
            name=self.name_edit.text() or entry.id,
            kind=str(self.kind_combo.currentData()),
            number=self.number_edit.text(),
            description=self.description_edit.text(),
        )
        self.refresh(entry.id)

    def _restore_version(self) -> None:
        entry = self.selected()
        item = self.versions.currentItem()
        if entry is None or item is None:
            return
        try:
            self.library.restore_version(entry, Path(str(item.data(Qt.ItemDataRole.UserRole))), now=NOW())
        except (LibraryError, OSError) as exc:
            self._warn("Version wiederherstellen", str(exc))
            return
        self.refresh(entry.id)

    def _delete_selected(self) -> None:
        entry = self.selected()
        if entry is None:
            return
        answer = QMessageBox.question(
            self,
            "In Papierkorb",
            f"'{entry.name}' in den Papierkorb verschieben?\n"
            "(Ordner 'deleted' in der Programmverwaltung, nichts wird endgültig gelöscht.)",
        )
        if answer == QMessageBox.StandardButton.Yes:
            self.delete_entry(entry)

    def delete_entry(self, entry: LibraryEntry) -> None:
        self.library.delete(entry, now=NOW())
        self.refresh()

    # ------------------------------------------------------------------ tools tab
    def _build_tools(self) -> QWidget:
        page = QWidget()
        layout = QVBoxLayout(page)
        info = QLabel(
            "Werkzeugtabelle von DeckelLink. Sie wird vom Simulator (Radius = Ø/2) und für die Werkzeugprüfung "
            "der Programme verwendet. Speichern legt vorher eine Sicherung an."
        )
        info.setWordWrap(True)
        layout.addWidget(info)
        self.tool_table = QTableWidget(0, len(TOOL_COLUMNS))
        self.tool_table.setHorizontalHeaderLabels(list(TOOL_COLUMNS))
        self.tool_table.horizontalHeader().setSectionResizeMode(QHeaderView.ResizeMode.Stretch)
        layout.addWidget(self.tool_table, 1)
        row = QHBoxLayout()
        self.add_tool_button = QPushButton("Werkzeug hinzufügen")
        self.add_tool_button.clicked.connect(self.add_tool_row)
        self.remove_tool_button = QPushButton("Zeile entfernen")
        self.remove_tool_button.clicked.connect(self._remove_tool_row)
        self.save_tools_button = QPushButton("Werkzeugtabelle &speichern")
        self.save_tools_button.clicked.connect(self.save_tools)
        self.reload_tools_button = QPushButton("Verwerfen / neu laden")
        self.reload_tools_button.clicked.connect(self.load_tools)
        for button in (self.add_tool_button, self.remove_tool_button, self.save_tools_button, self.reload_tools_button):
            row.addWidget(button)
        row.addStretch(1)
        layout.addLayout(row)
        self.tool_status = QLabel()
        self.tool_status.setWordWrap(True)
        layout.addWidget(self.tool_status)

        group = QGroupBox("Werkzeugdatei für die Steuerung (Vorlage, NICHT verifiziert)")
        group_layout = QVBoxLayout(group)
        warn = QLabel(
            "Das Format, in dem die DIALOG 11 Werkzeugdaten einliest, ist nicht bekannt. Empfohlen: Werkzeugdatei "
            "an der Steuerung ausgeben, auf der Seite Empfangen aufnehmen und hier als „Werkzeugdatei“ "
            "importieren. Diese Datei kann unverändert zurückgesendet werden. Eine Vorlage nur nach diesem "
            f"Muster einrichten. Platzhalter: {' '.join(PLACEHOLDERS)}"
        )
        warn.setWordWrap(True)
        group_layout.addWidget(warn)
        template_form = QFormLayout()
        self.header_edit = QPlainTextEdit(self.template.header)
        self.line_edit = QLineEdit(self.template.line)
        self.footer_edit = QPlainTextEdit(self.template.footer)
        for edit in (self.header_edit, self.footer_edit):
            edit.setMaximumHeight(54)
        template_form.addRow("Kopfzeilen:", self.header_edit)
        template_form.addRow("Zeile je Werkzeug:", self.line_edit)
        template_form.addRow("Schlusszeilen:", self.footer_edit)
        group_layout.addLayout(template_form)
        template_buttons = QHBoxLayout()
        self.template_button = QPushButton("Vorlage speichern")
        self.template_button.clicked.connect(self._save_template)
        self.export_button = QPushButton("Werkzeugdatei erzeugen → Programmverwaltung")
        self.export_button.clicked.connect(self.export_tools)
        template_buttons.addWidget(self.template_button)
        template_buttons.addWidget(self.export_button)
        template_buttons.addStretch(1)
        group_layout.addLayout(template_buttons)
        layout.addWidget(group)
        return page

    def load_tools(self) -> None:
        tools = self.tool_db.load()
        self.tool_table.setRowCount(0)
        for tool in tools:
            self._append_tool(tool)
        problems = self.tool_db.problems
        self.tool_status.setText(
            (f"{len(tools)} Werkzeug(e) geladen." if tools else "Noch keine Werkzeuge angelegt.")
            + (" ⚠ " + "; ".join(problems) if problems else "")
        )

    def _append_tool(self, tool: Tool) -> None:
        row = self.tool_table.rowCount()
        self.tool_table.insertRow(row)
        values = (
            str(tool.number),
            tool.name,
            tool.kind,
            _num(tool.diameter),
            _num(tool.length),
            str(tool.flutes),
            _num(tool.feed),
            _num(tool.speed),
            tool.note,
        )
        for col, value in enumerate(values):
            self.tool_table.setItem(row, col, QTableWidgetItem(value))

    def add_tool_row(self) -> None:
        numbers = {t.number for t in self.table_tools(strict=False)}
        number = next(n for n in range(1, 10000) if n not in numbers)
        self._append_tool(Tool(number=number))

    def _remove_tool_row(self) -> None:
        row = self.tool_table.currentRow()
        if row >= 0:
            self.tool_table.removeRow(row)

    def table_tools(self, *, strict: bool = True) -> list[Tool]:
        tools: list[Tool] = []
        for row in range(self.tool_table.rowCount()):
            cells = [_cell(self.tool_table, row, col) for col in range(len(TOOL_COLUMNS))]
            try:
                tools.append(
                    Tool(
                        number=int(cells[0]),
                        name=cells[1],
                        kind=cells[2] or "Fräser",
                        diameter=_float(cells[3]),
                        length=_float(cells[4]),
                        flutes=int(cells[5] or 0),
                        feed=_float(cells[6]),
                        speed=_float(cells[7]),
                        note=cells[8],
                    )
                )
            except ValueError as exc:
                if strict:
                    raise ToolDataError(f"Zeile {row + 1}: ungültiger Zahlenwert ({exc})") from exc
        return tools

    def save_tools(self) -> bool:
        try:
            self.tool_db.save(self.table_tools())
        except (ToolDataError, OSError) as exc:
            self.tool_status.setText(f"✖ Nicht gespeichert: {exc}")
            return False
        self.tool_status.setText(f"✔ {len(self.tool_db.tools)} Werkzeug(e) gespeichert.")
        self.tools_changed.emit()
        self._show_selected()
        return True

    def current_template(self) -> ToolExportTemplate:
        return ToolExportTemplate(self.header_edit.toPlainText(), self.line_edit.text(), self.footer_edit.toPlainText())

    def _save_template(self) -> None:
        self.template = self.current_template()
        self.template_changed.emit(self.template)
        self.tool_status.setText("Vorlage gespeichert (nicht verifiziert).")

    def export_tools(self) -> LibraryEntry | None:
        try:
            data = export_tool_file(self.table_tools(), self.current_template())
            entry = self.library.import_bytes(
                "WERKZEUGE_VORLAGE.nc", data, kind=EntryKind.TOOLS, source="Vorlage (nicht verifiziert)", now=NOW()
            )
        except (ToolDataError, LibraryError, OSError) as exc:
            self.tool_status.setText(f"✖ {exc}")
            return None
        self.library.update_meta(entry, description="Aus Vorlage erzeugt - Format NICHT verifiziert")
        self.refresh(entry.id)
        self.tabs.setCurrentIndex(0)
        self.tool_status.setText(f"Werkzeugdatei '{entry.name}' in der Programmverwaltung angelegt.")
        return entry

    def _warn(self, title: str, text: str) -> None:
        log.warning("%s: %s", title, text)
        QMessageBox.warning(self, title, text)


def _cell(table: QTableWidget, row: int, col: int) -> str:
    item = table.item(row, col)
    return item.text().strip() if item is not None else ""


def _float(text: str) -> float:
    return float(text.replace(",", ".")) if text else 0.0


def _num(value: float) -> str:
    return f"{value:g}"
