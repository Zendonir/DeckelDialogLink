"""Inline error display with collapsible technical details (no modal dialog chains)."""

from __future__ import annotations

from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from deckellink.domain.errors import UserFacingError


class ErrorPanel(QFrame):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName("ErrorPanel")
        layout = QVBoxLayout(self)
        top = QHBoxLayout()
        self.title = QLabel()
        self.title.setObjectName("ErrorTitle")
        self.title.setWordWrap(True)
        top.addWidget(QLabel("✖"))
        top.addWidget(self.title, 1)
        self.close_button = QPushButton("Ausblenden")
        self.close_button.setToolTip("Fehlermeldung ausblenden")
        self.close_button.clicked.connect(self.clear)
        top.addWidget(self.close_button)
        layout.addLayout(top)
        self.causes = QLabel()
        self.causes.setWordWrap(True)
        layout.addWidget(self.causes)
        self.toggle = QToolButton()
        self.toggle.setText("▸ Technische Details")
        self.toggle.setCheckable(True)
        self.toggle.toggled.connect(self._toggle_details)
        layout.addWidget(self.toggle)
        self.details = QPlainTextEdit()
        self.details.setReadOnly(True)
        self.details.setMaximumHeight(120)
        self.details.setVisible(False)
        layout.addWidget(self.details)
        self.error: UserFacingError | None = None
        self.setVisible(False)

    def _toggle_details(self, checked: bool) -> None:
        self.details.setVisible(checked)
        self.toggle.setText(("▾" if checked else "▸") + " Technische Details")

    def show_error(self, error: UserFacingError) -> None:
        self.error = error
        self.title.setText(error.title)
        if error.causes:
            self.causes.setText("Mögliche Ursachen:\n" + "\n".join(f"• {cause}" for cause in error.causes))
        else:
            self.causes.setText("")
        self.details.setPlainText(error.technical_details or "Keine weiteren Details.")
        self.toggle.setChecked(False)
        self.setVisible(True)

    def clear(self) -> None:
        self.error = None
        self.setVisible(False)
