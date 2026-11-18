"""Regression: the UI must stay readable when the OS runs in dark mode (Windows 10/11)."""

from __future__ import annotations

import pytest
from PySide6.QtGui import QColor, QPalette
from PySide6.QtWidgets import QApplication, QLabel

from deckellink.gui.style import apply_theme, light_palette
from tests.gui.conftest import WindowFactory

pytestmark = pytest.mark.gui


def _luminance(color: QColor) -> float:
    return 0.2126 * color.redF() + 0.7152 * color.greenF() + 0.0722 * color.blueF()


def test_light_theme_overrides_dark_system_palette(make_window: WindowFactory) -> None:
    app = QApplication.instance()
    assert isinstance(app, QApplication)
    dark = QPalette()
    for role, value in (
        (QPalette.ColorRole.Window, "#202020"),
        (QPalette.ColorRole.WindowText, "#f0f0f0"),
        (QPalette.ColorRole.Base, "#1c1c1c"),
        (QPalette.ColorRole.Text, "#f0f0f0"),
    ):
        dark.setColor(role, QColor(value))
    app.setPalette(dark)
    apply_theme(app)
    palette = app.palette()
    # dark text on light backgrounds
    assert _luminance(palette.color(QPalette.ColorRole.Text)) < 0.3
    assert _luminance(palette.color(QPalette.ColorRole.Base)) > 0.9
    assert _luminance(palette.color(QPalette.ColorRole.WindowText)) < 0.3
    assert _luminance(palette.color(QPalette.ColorRole.Window)) > 0.8

    window = make_window()
    label = window.transfer_page.file_name
    assert isinstance(label, QLabel)
    assert _luminance(label.palette().color(QPalette.ColorRole.WindowText)) < 0.3
    view = window.terminal_page.view
    assert _luminance(view.palette().color(QPalette.ColorRole.Base)) > 0.9


def test_disabled_text_is_distinguishable() -> None:
    palette = light_palette()
    enabled = palette.color(QPalette.ColorGroup.Active, QPalette.ColorRole.ButtonText)
    disabled = palette.color(QPalette.ColorGroup.Disabled, QPalette.ColorRole.ButtonText)
    assert enabled != disabled
