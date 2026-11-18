"""Options for the diagnostic package export."""

from __future__ import annotations

from pathlib import Path

from PySide6.QtWidgets import (
    QCheckBox,
    QDialog,
    QDialogButtonBox,
    QLabel,
    QVBoxLayout,
    QWidget,
)

from deckellink.diagnostics.export import ExportOptions


class ExportDialog(QDialog):
    def __init__(
        self,
        *,
        mask_paths: bool,
        nc_file: Path | None,
        raw_file: Path | None,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.setWindowTitle("Diagnosepaket exportieren")
        self._nc_file = nc_file
        self._raw_file = raw_file
        layout = QVBoxLayout(self)
        info = QLabel(
            "Das Paket enthält Logs, anonymisierte Einstellungen, das aktive Profil, Systeminformationen "
            "und das letzte Übertragungsergebnis. NC-Dateien und Rohdaten nur nach ausdrücklicher Auswahl."
        )
        info.setWordWrap(True)
        layout.addWidget(info)
        self.mask_box = QCheckBox("Dateipfade maskieren")
        self.mask_box.setChecked(mask_paths)
        layout.addWidget(self.mask_box)
        self.include_nc = QCheckBox(
            f"Aktuelle NC-Datei einschließen ({nc_file.name})" if nc_file else "Keine NC-Datei geladen"
        )
        self.include_nc.setEnabled(nc_file is not None)
        layout.addWidget(self.include_nc)
        self.include_raw = QCheckBox(
            f"Rohdaten des letzten Empfangs einschließen ({raw_file.name})" if raw_file else "Keine Rohdaten vorhanden"
        )
        self.include_raw.setEnabled(raw_file is not None)
        layout.addWidget(self.include_raw)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def options(self, last_result: dict[str, object] | None) -> ExportOptions:
        return ExportOptions(
            mask_paths=self.mask_box.isChecked(),
            include_nc_files=[self._nc_file] if self._nc_file and self.include_nc.isChecked() else [],
            include_raw_files=[self._raw_file] if self._raw_file and self.include_raw.isChecked() else [],
            last_result=dict(last_result) if last_result else None,
        )
