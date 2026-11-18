"""Table of analysis findings."""

from __future__ import annotations

from collections.abc import Iterable

from PySide6.QtGui import QColor
from PySide6.QtWidgets import QAbstractItemView, QHeaderView, QTableWidget, QTableWidgetItem, QWidget

from deckellink.domain.results import Finding, Severity

_COLORS = {Severity.INFO: "#e3f2fd", Severity.WARNING: "#fff8e1", Severity.ERROR: "#fdecea"}
_SYMBOLS = {Severity.INFO: "ℹ", Severity.WARNING: "⚠", Severity.ERROR: "✖"}


class FindingsTable(QTableWidget):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(0, 3, parent)
        self.setHorizontalHeaderLabels(["Stufe", "Befund", "Zeilen"])
        self.verticalHeader().setVisible(False)
        self.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)
        self.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self.setWordWrap(True)
        header = self.horizontalHeader()
        header.setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        header.setSectionResizeMode(1, QHeaderView.ResizeMode.Stretch)
        header.setSectionResizeMode(2, QHeaderView.ResizeMode.Interactive)
        self.setAccessibleName("Prüfbefunde")

    def set_findings(self, findings: Iterable[Finding]) -> None:
        items = list(findings)
        self.setRowCount(len(items))
        for row, finding in enumerate(items):
            color = QColor(_COLORS[finding.severity])
            cells = (
                f"{_SYMBOLS[finding.severity]} {finding.severity.label}",
                finding.message,
                finding.lines_text,
            )
            for column, text in enumerate(cells):
                item = QTableWidgetItem(text)
                item.setBackground(color)
                item.setToolTip(text)
                self.setItem(row, column, item)
        self.resizeRowsToContents()
