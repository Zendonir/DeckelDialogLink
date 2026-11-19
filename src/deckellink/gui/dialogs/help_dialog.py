"""Built-in manual (bundled Markdown documents) and "About" dialog."""

from __future__ import annotations

import platform
import sys

from PySide6.QtCore import QUrl
from PySide6.QtGui import QDesktopServices
from PySide6.QtWidgets import (
    QDialog,
    QDialogButtonBox,
    QHBoxLayout,
    QListWidget,
    QMessageBox,
    QTextBrowser,
    QVBoxLayout,
    QWidget,
)

from deckellink.gui.app_support import HELP_DOCUMENTS, find_document
from deckellink.safety import SCOPE_NOTICE
from deckellink.version import APP_NAME, __version__


class HelpDialog(QDialog):
    def __init__(self, parent: QWidget | None = None, start: str = "README.md") -> None:
        super().__init__(parent)
        self.setWindowTitle(f"{APP_NAME} - Handbuch")
        self.resize(1000, 720)
        layout = QVBoxLayout(self)
        body = QHBoxLayout()
        self.index = QListWidget()
        self.index.setMaximumWidth(280)
        self.index.setAccessibleName("Inhaltsverzeichnis")
        self.documents = [doc for doc in HELP_DOCUMENTS if find_document(doc.file_name) is not None]
        for doc in self.documents:
            self.index.addItem(doc.title)
        body.addWidget(self.index)
        self.view = QTextBrowser()
        self.view.setOpenLinks(False)
        self.view.anchorClicked.connect(self._link)
        self.view.setAccessibleName("Handbuchtext")
        body.addWidget(self.view, 1)
        layout.addLayout(body, 1)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Close)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)
        self.index.currentRowChanged.connect(self._show)
        if not self.documents:
            self.view.setPlainText("Die Dokumentation wurde nicht gefunden.")
            return
        names = [doc.file_name for doc in self.documents]
        self.index.setCurrentRow(names.index(start) if start in names else 0)

    def _show(self, row: int) -> None:
        if not 0 <= row < len(self.documents):
            return
        path = find_document(self.documents[row].file_name)
        if path is None:
            return
        self.view.setMarkdown(path.read_text(encoding="utf-8"))
        self.view.verticalScrollBar().setValue(0)

    def current_title(self) -> str:
        row = self.index.currentRow()
        return self.documents[row].title if 0 <= row < len(self.documents) else ""

    def _link(self, url: QUrl) -> None:
        target = url.toString().split("#", 1)[0].rsplit("/", 1)[-1]
        names = [doc.file_name for doc in self.documents]
        if target in names:
            self.index.setCurrentRow(names.index(target))
        elif url.scheme() in ("http", "https"):
            QDesktopServices.openUrl(url)


def about_text() -> str:
    return (
        f"<h3>{APP_NAME} {__version__}</h3>"
        "<p>NC-Datenübertragung für DECKEL FP2A mit DIALOG 11 (V.24/RS-232, Netzwerkadapter, LSV2).</p>"
        f"<p>{SCOPE_NOTICE}</p>"
        "<p><b>Vorabversion:</b> Die Kommunikationsparameter der DIALOG 11 sind nicht an einer realen "
        "Maschine verifiziert.</p>"
        f"<p>Python {sys.version.split()[0]} · {platform.system()} {platform.release()}<br>"
        "MIT-Lizenz · ohne Gewährleistung</p>"
    )


def show_about(parent: QWidget | None) -> None:
    QMessageBox.about(parent, f"Über {APP_NAME}", about_text())
