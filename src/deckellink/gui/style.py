"""Colours, symbols and stylesheet. Colour is never the only information carrier."""

from __future__ import annotations

from enum import StrEnum

from PySide6.QtCore import Qt
from PySide6.QtGui import QColor, QPalette
from PySide6.QtWidgets import QApplication


class StatusKind(StrEnum):
    READY = "ready"  # green
    WARNING = "warning"  # yellow
    ERROR = "error"  # red
    ACTIVE = "active"  # blue
    IDLE = "idle"  # grey

    @property
    def symbol(self) -> str:
        return {"ready": "✔", "warning": "⚠", "error": "✖", "active": "⇄", "idle": "○"}[self.value]

    @property
    def background(self) -> str:
        return {
            "ready": "#1e7b34",
            "warning": "#b58a00",
            "error": "#b3261e",
            "active": "#1f5fa8",
            "idle": "#5f6368",
        }[self.value]

    @property
    def foreground(self) -> str:
        return "#000000" if self is StatusKind.WARNING else "#ffffff"


SIMULATION_BANNER_STYLE = (
    "background-color: #ffcc00; color: #000000; font-weight: bold; padding: 6px 12px;border: 3px dashed #000000;"
)
REAL_BANNER_STYLE = (
    "background-color: #8b0000; color: #ffffff; font-weight: bold; padding: 6px 12px;border: 3px solid #ffffff;"
)

APP_STYLESHEET = """
QWidget { font-size: 10pt; }
QMainWindow { background: #eceff1; }
QFrame#HeaderBar { background: #263238; }
QFrame#HeaderBar QLabel { color: #eceff1; }
QLabel#MachineTitle { font-size: 16pt; font-weight: bold; color: #ffffff; }
QLabel#ControlTitle { font-size: 11pt; color: #b0bec5; }
QListWidget#Navigation { background: #37474f; color: #eceff1; border: none; font-size: 11pt; }
QListWidget#Navigation::item { padding: 10px 14px; }
QListWidget#Navigation::item:selected { background: #546e7a; color: #ffffff; }
QGroupBox { font-weight: bold; border: 1px solid #b0bec5; border-radius: 3px; margin-top: 12px;
            background: #ffffff; }
QGroupBox::title { subcontrol-origin: margin; left: 8px; padding: 0 4px; }
QPushButton { padding: 6px 14px; }
QPushButton#PrimaryButton { background: #1f5fa8; color: white; font-weight: bold; }
QPushButton#PrimaryButton:disabled { background: #90a4ae; color: #eceff1; }
QPushButton#CancelButton { background: #b3261e; color: white; font-weight: bold; }
QPushButton#CancelButton:disabled { background: #cfd8dc; color: #78909c; }
QFrame#ErrorPanel { background: #fdecea; border: 2px solid #b3261e; border-radius: 3px; }
QLabel#ErrorTitle { color: #8b0000; font-weight: bold; font-size: 11pt; }
QPlainTextEdit, QTextEdit { font-family: Consolas, "DejaVu Sans Mono", monospace; }
QProgressBar { min-height: 22px; text-align: center; }
"""


# --------------------------------------------------------------------------
# Fixed light theme
#
# The stylesheet above uses light backgrounds. If the OS runs in dark mode
# (Windows 10/11 "dark"), Qt would otherwise take light text colours and dark
# input fields from the system, producing invisible labels and black lists.
# DeckelLink therefore always uses its own light palette with the Fusion style.

_LIGHT_COLORS: dict[str, str] = {
    "Window": "#eceff1",
    "WindowText": "#1b1f23",
    "Base": "#ffffff",
    "AlternateBase": "#f5f7f8",
    "Text": "#1b1f23",
    "Button": "#e3e7ea",
    "ButtonText": "#1b1f23",
    "BrightText": "#ffffff",
    "ToolTipBase": "#fffde7",
    "ToolTipText": "#1b1f23",
    "Highlight": "#1f5fa8",
    "HighlightedText": "#ffffff",
    "Link": "#1f5fa8",
    "PlaceholderText": "#6b7378",
    "Light": "#ffffff",
    "Midlight": "#f0f2f4",
    "Mid": "#b0bec5",
    "Dark": "#78909c",
    "Shadow": "#455a64",
}
_DISABLED_TEXT = "#9aa3a8"


def light_palette() -> QPalette:
    palette = QPalette()
    for role_name, color in _LIGHT_COLORS.items():
        palette.setColor(getattr(QPalette.ColorRole, role_name), QColor(color))
    for role in (
        QPalette.ColorRole.WindowText,
        QPalette.ColorRole.Text,
        QPalette.ColorRole.ButtonText,
        QPalette.ColorRole.PlaceholderText,
    ):
        palette.setColor(QPalette.ColorGroup.Disabled, role, QColor(_DISABLED_TEXT))
    palette.setColor(QPalette.ColorGroup.Disabled, QPalette.ColorRole.Button, QColor("#eceff1"))
    palette.setColor(QPalette.ColorGroup.Disabled, QPalette.ColorRole.Base, QColor("#f5f7f8"))
    return palette


def apply_theme(app: QApplication) -> None:
    """Apply the fixed light theme, independent of the OS colour scheme."""
    hints = app.styleHints()
    set_scheme = getattr(hints, "setColorScheme", None)  # Qt >= 6.8
    if callable(set_scheme):
        set_scheme(Qt.ColorScheme.Light)
    app.setStyle("Fusion")
    app.setPalette(light_palette())
    app.setStyleSheet(APP_STYLESHEET)
