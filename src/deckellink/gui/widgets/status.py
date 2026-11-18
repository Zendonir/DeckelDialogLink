"""Status indicator and modem signal panel."""

from __future__ import annotations

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import QCheckBox, QGridLayout, QLabel, QWidget

from deckellink.diagnostics.modem_status import (
    INPUT_SIGNALS,
    OUTPUT_SIGNALS,
    SIGNAL_DESCRIPTIONS,
    signal_text,
)
from deckellink.gui.style import StatusKind
from deckellink.transport.base import ModemStatus


class StatusIndicator(QLabel):
    """Coloured badge with symbol and text (colour is not the only cue)."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.kind = StatusKind.IDLE
        self.status_text = ""
        self.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.set_status(StatusKind.IDLE, "Inaktiv")

    def set_status(self, kind: StatusKind, text: str) -> None:
        self.kind = kind
        self.status_text = text
        self.setText(f"{kind.symbol}  {text}")
        self.setAccessibleName(f"Status: {text}")
        self.setStyleSheet(
            f"background:{kind.background}; color:{kind.foreground}; font-weight:bold;"
            "padding:4px 10px; border-radius:3px;"
        )


class SignalPanel(QWidget):
    """Shows CTS/DSR/DCD/RI (inputs) and RTS/DTR (outputs).

    Output lines can optionally be toggled (diagnostics only).
    """

    control_requested = Signal(str, bool)

    def __init__(self, parent: QWidget | None = None, *, controllable: bool = False) -> None:
        super().__init__(parent)
        layout = QGridLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        self._labels: dict[str, QLabel] = {}
        self._toggles: dict[str, QCheckBox] = {}
        for column, name in enumerate(INPUT_SIGNALS + OUTPUT_SIGNALS):
            label = QLabel()
            label.setToolTip(SIGNAL_DESCRIPTIONS[name])
            label.setAlignment(Qt.AlignmentFlag.AlignCenter)
            layout.addWidget(label, 0, column)
            self._labels[name] = label
            if controllable and name in OUTPUT_SIGNALS:
                toggle = QCheckBox(f"{name} setzen")
                toggle.setToolTip(f"{name} manuell schalten (nur Diagnose, nur bei geöffnetem Port)")
                toggle.setEnabled(False)
                toggle.toggled.connect(lambda checked, n=name: self.control_requested.emit(n, checked))
                layout.addWidget(toggle, 1, column)
                self._toggles[name] = toggle
        self.set_status(ModemStatus())

    def set_controls_enabled(self, enabled: bool) -> None:
        for toggle in self._toggles.values():
            toggle.setEnabled(enabled)

    def set_status(self, status: ModemStatus) -> None:
        values = status.as_dict()
        for name, label in self._labels.items():
            value = values[name]
            kind = StatusKind.IDLE if value is None else (StatusKind.READY if value else StatusKind.IDLE)
            symbol = "●" if value else ("?" if value is None else "○")
            label.setText(f"{symbol} {name}\n{signal_text(value)}")
            label.setAccessibleName(f"{name} {signal_text(value)}")
            label.setStyleSheet(
                f"background:{kind.background}; color:{kind.foreground}; padding:4px 8px;"
                "border-radius:3px; font-weight:bold; min-width:56px;"
            )
            toggle = self._toggles.get(name)
            if toggle is not None and value is not None:
                toggle.blockSignals(True)
                toggle.setChecked(value)
                toggle.blockSignals(False)

    def text_of(self, name: str) -> str:
        return self._labels[name].text()
