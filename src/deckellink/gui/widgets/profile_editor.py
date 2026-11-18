"""Metadata-driven profile editor. Every communication parameter is editable."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QFormLayout,
    QLineEdit,
    QScrollArea,
    QSpinBox,
    QTabWidget,
    QVBoxLayout,
    QWidget,
)

from deckellink.domain.models import (
    DATA_BITS,
    STANDARD_BAUD_RATES,
    BccMode,
    CommentHandling,
    EndDetection,
    LineEnding,
    Parity,
    Profile,
    ProtocolMode,
    TabHandling,
    VerificationStatus,
    WhitespaceHandling,
)


@dataclass(frozen=True)
class FieldSpec:
    section: str
    key: str
    label: str
    kind: str  # int | bool | choice | text
    tooltip: str = ""
    minimum: int = 0
    maximum: int = 1_000_000
    choices: tuple[tuple[Any, str], ...] = ()
    suffix: str = ""


def _enum_choices(enum: Any) -> tuple[tuple[Any, str], ...]:
    return tuple((m.value, getattr(m, "label", m.value)) for m in enum)


MS = " ms"
FIELDS: tuple[FieldSpec, ...] = (
    # serial
    FieldSpec(
        "serial",
        "baud_rate",
        "Baudrate",
        "choice",
        "Übertragungsgeschwindigkeit. Muss mit der CNC-Einstellung übereinstimmen.",
        choices=tuple((b, str(b)) for b in STANDARD_BAUD_RATES),
    ),
    FieldSpec(
        "serial",
        "data_bits",
        "Datenbits",
        "choice",
        "Anzahl Datenbits pro Zeichen.",
        choices=tuple((b, str(b)) for b in DATA_BITS),
    ),
    FieldSpec("serial", "parity", "Parität", "choice", "Paritätsbit.", choices=_enum_choices(Parity)),
    FieldSpec(
        "serial",
        "stop_bits",
        "Stoppbits",
        "choice",
        "Anzahl Stoppbits.",
        choices=((1.0, "1"), (1.5, "1,5"), (2.0, "2")),
    ),
    FieldSpec("serial", "rts_cts", "Flusssteuerung RTS/CTS", "bool", "Hardware-Handshake über RTS/CTS."),
    FieldSpec("serial", "xon_xoff", "Flusssteuerung XON/XOFF", "bool", "Software-Handshake (DC1/DC3)."),
    FieldSpec("serial", "dsr_dtr", "Flusssteuerung DSR/DTR", "bool", "Hardware-Handshake über DSR/DTR."),
    FieldSpec("serial", "initial_rts", "RTS beim Öffnen setzen", "bool", "Initialer RTS-Zustand."),
    FieldSpec("serial", "initial_dtr", "DTR beim Öffnen setzen", "bool", "Initialer DTR-Zustand."),
    FieldSpec(
        "serial",
        "require_cts_before_send",
        "CTS vor dem Senden erforderlich",
        "bool",
        "Senden beginnt erst, wenn die Gegenstelle CTS setzt.",
    ),
    FieldSpec(
        "serial",
        "require_dsr_before_send",
        "DSR vor dem Senden erforderlich",
        "bool",
        "Senden beginnt erst, wenn die Gegenstelle DSR setzt.",
    ),
    FieldSpec(
        "serial",
        "handshake_timeout_ms",
        "Max. Wartezeit auf CTS/DSR",
        "int",
        "Maximale Wartezeit auf die Sendefreigabe.",
        0,
        3_600_000,
        suffix=MS,
    ),
    FieldSpec(
        "serial",
        "read_timeout_ms",
        "Read Timeout",
        "int",
        "Timeout eines Lesevorgangs.",
        1,
        600_000,
        suffix=MS,
    ),
    FieldSpec(
        "serial",
        "write_timeout_ms",
        "Write Timeout",
        "int",
        "Maximale Dauer ohne Sendefortschritt.",
        1,
        600_000,
        suffix=MS,
    ),
    FieldSpec(
        "serial",
        "inter_byte_timeout_ms",
        "Inter-Byte Timeout",
        "int",
        "0 = deaktiviert.",
        0,
        60_000,
        suffix=MS,
    ),
    # transfer
    FieldSpec(
        "transfer",
        "line_ending",
        "Zeilenende",
        "choice",
        "Zeilenende in den Transferdaten.",
        choices=_enum_choices(LineEnding),
    ),
    FieldSpec(
        "transfer",
        "program_start",
        "Startsequenz",
        "text",
        "Wird vor das Programm gesetzt. Leer lassen, solange nicht verifiziert (z. B. M82).",
    ),
    FieldSpec("transfer", "program_end", "Programmende", "text", "Erwartetes Programmende, Standard M30."),
    FieldSpec(
        "transfer",
        "append_program_end",
        "Fehlendes Programmende ergänzen dürfen",
        "bool",
        "Erlaubt das Ergänzen - zusätzlich ist pro Übertragung eine Bestätigung nötig.",
    ),
    FieldSpec(
        "transfer",
        "start_delay_ms",
        "Startverzögerung",
        "int",
        "Wartezeit nach Portöffnung.",
        0,
        600_000,
        suffix=MS,
    ),
    FieldSpec(
        "transfer",
        "end_delay_ms",
        "Endverzögerung",
        "int",
        "Wartezeit nach dem letzten Byte.",
        0,
        600_000,
        suffix=MS,
    ),
    FieldSpec(
        "transfer",
        "character_delay_ms",
        "Zeichenverzögerung",
        "int",
        "Pause nach jedem Zeichen (0 = aus).",
        0,
        10_000,
        suffix=MS,
    ),
    FieldSpec(
        "transfer",
        "packet_size",
        "Paketgröße",
        "int",
        "Bytes pro Paket (0 = aus).",
        0,
        65_536,
        suffix=" Bytes",
    ),
    FieldSpec(
        "transfer",
        "packet_delay_ms",
        "Paketverzögerung",
        "int",
        "Pause nach jedem Paket.",
        0,
        600_000,
        suffix=MS,
    ),
    FieldSpec("transfer", "normalize_line_endings", "Zeilenenden normalisieren", "bool", ""),
    FieldSpec("transfer", "remove_blank_lines", "Leerzeilen entfernen", "bool", ""),
    FieldSpec("transfer", "strip_null_bytes", "Nullbytes entfernen", "bool", ""),
    FieldSpec(
        "transfer",
        "skip_semicolon_lines",
        "Semikolonzeilen entfernen",
        "bool",
        "Zeilen, die mit ';' beginnen, werden nicht gesendet.",
    ),
    FieldSpec(
        "transfer",
        "tab_handling",
        "Tabulatoren",
        "choice",
        "",
        choices=(
            (TabHandling.REPLACE.value, "durch Leerzeichen ersetzen"),
            (TabHandling.REJECT.value, "ablehnen (Fehler)"),
            (TabHandling.KEEP.value, "unverändert senden"),
        ),
    ),
    FieldSpec(
        "transfer",
        "whitespace",
        "Leerzeichen am Zeilenrand",
        "choice",
        "",
        choices=(
            (WhitespaceHandling.TRAILING.value, "nachgestellte entfernen"),
            (WhitespaceHandling.BOTH.value, "führende und nachgestellte entfernen"),
            (WhitespaceHandling.NONE.value, "beibehalten"),
        ),
    ),
    FieldSpec(
        "transfer",
        "comment_handling",
        "Kommentare",
        "choice",
        "",
        choices=((CommentHandling.KEEP.value, "beibehalten"), (CommentHandling.REMOVE.value, "entfernen")),
    ),
    FieldSpec("transfer", "comment_open", "Kommentar Anfang", "text", "Zeichen, das einen Kommentar öffnet."),
    FieldSpec("transfer", "comment_close", "Kommentar Ende", "text", "Zeichen, das einen Kommentar schließt."),
    FieldSpec("transfer", "ensure_final_newline", "Abschließenden Zeilenumbruch ergänzen", "bool", ""),
    FieldSpec(
        "transfer",
        "max_line_length",
        "Warnung ab Zeilenlänge",
        "int",
        "0 = keine Prüfung.",
        0,
        100_000,
        suffix=" Zeichen",
    ),
    FieldSpec(
        "transfer",
        "max_program_bytes",
        "Warnung ab Dateigröße",
        "int",
        "0 = keine Prüfung.",
        0,
        100 * 1024 * 1024,
        suffix=" Bytes",
    ),
    FieldSpec("transfer", "close_port_after_send", "Port nach dem Senden schließen", "bool", ""),
    # receive
    FieldSpec(
        "receive",
        "end_detection",
        "Empfangsende erkennen durch",
        "choice",
        "",
        choices=(
            (EndDetection.BOTH.value, "Endsequenz oder Inaktivität"),
            (EndDetection.END_SEQUENCE.value, "nur Endsequenz"),
            (EndDetection.INACTIVITY.value, "nur Inaktivität"),
        ),
    ),
    FieldSpec("receive", "end_sequence", "Endsequenz", "text", "Standard M30."),
    FieldSpec(
        "receive",
        "inactivity_timeout_ms",
        "Empfangs-Inaktivitätstimeout",
        "int",
        "",
        50,
        3_600_000,
        suffix=MS,
    ),
    FieldSpec(
        "receive",
        "first_byte_timeout_ms",
        "Max. Wartezeit auf erste Daten",
        "int",
        "0 = unbegrenzt.",
        0,
        86_400_000,
        suffix=MS,
    ),
    FieldSpec("receive", "end_grace_ms", "Nachlaufzeit nach Endsequenz", "int", "", 0, 60_000, suffix=MS),
    FieldSpec(
        "receive",
        "max_receive_bytes",
        "Maximale Empfangsgröße",
        "int",
        "0 = unbegrenzt (Warnung).",
        0,
        100 * 1024 * 1024,
        suffix=" Bytes",
    ),
    FieldSpec("receive", "strip_null_bytes", "Nullbytes entfernen", "bool", ""),
    FieldSpec("receive", "normalize_line_endings", "Zeilenenden normalisieren", "bool", ""),
    FieldSpec("receive", "save_raw", "Rohdaten separat speichern", "bool", ""),
    FieldSpec(
        "receive",
        "program_name_pattern",
        "Muster für Programmname (Regex)",
        "text",
        "Optional. Gruppe 1 ergibt den Dateinamen. Leer = RECEIVED_Datum_Zeit.",
    ),
)

LIST_SEPARATOR = " | "
FIELDS = (
    *FIELDS,
    FieldSpec(
        "protocol",
        "mode",
        "Protokoll",
        "choice",
        "Klartext oder LSV2 (DIN 66019).",
        choices=tuple((m.value, m.label) for m in ProtocolMode),
    ),
    FieldSpec(
        "protocol",
        "transparent",
        "Transparentmodus (DLE STX ... DLE ETX)",
        "bool",
        "DLE-Rahmung mit DLE-Verdopplung im Text. Nicht verifiziert.",
    ),
    FieldSpec(
        "protocol",
        "bcc_mode",
        "Prüfzeichen (BCC)",
        "choice",
        "Bildung der XOR-Prüfsumme. Nicht verifiziert.",
        choices=(
            (BccMode.TEXT_ETX.value, "XOR über Text + ETX"),
            (BccMode.TRANSMITTED.value, "XOR über alle gesendeten Zeichen nach STX"),
            (BccMode.NONE.value, "kein BCC"),
        ),
    ),
    FieldSpec("protocol", "enq_interval_ms", "ENQ-Wiederholintervall", "int", "", 100, 60_000, suffix=" ms"),
    FieldSpec(
        "protocol",
        "link_timeout_ms",
        "Max. Wartezeit Verbindungsaufbau",
        "int",
        "So lange wird ENQ wiederholt, bis die CNC mit DLE 0 antwortet.",
        100,
        3_600_000,
        suffix=" ms",
    ),
    FieldSpec(
        "protocol", "ack_timeout_ms", "Quittungs-Timeout (Speicherübertragung)", "int", "", 50, 600_000, suffix=" ms"
    ),
    FieldSpec(
        "protocol",
        "drip_feed_ack_timeout_ms",
        "Quittungs-Timeout Nachladebetrieb",
        "int",
        "Maximale Bearbeitungszeit eines Satzes bis zur nächsten Anforderung. 0 = unbegrenzt.",
        0,
        86_400_000,
        suffix=" ms",
    ),
    FieldSpec("protocol", "max_retries", "Max. Wiederholungen", "int", "Bei NAK oder fehlender Quittung.", 0, 20),
    FieldSpec("protocol", "wack_poll_ms", "Wartezeit nach WACK", "int", "", 10, 60_000, suffix=" ms"),
    FieldSpec("protocol", "lines_per_telegram", "Zeilen pro Telegramm", "int", "", 1, 1000),
    FieldSpec("protocol", "max_telegram_length", "Max. Telegrammlänge", "int", "", 8, 4096, suffix=" Zeichen"),
    FieldSpec("protocol", "include_line_ending", "Zeilenende im Telegramm mitsenden", "bool", ""),
    FieldSpec(
        "protocol",
        "start_telegrams",
        "Starttelegramme",
        "list",
        "Optional, durch ' | ' getrennt. {name} = Programmname. Nicht verifiziert.",
    ),
    FieldSpec("protocol", "end_telegrams", "Endtelegramme", "list", "Optional, durch ' | ' getrennt."),
    FieldSpec("protocol", "send_eot_on_cancel", "Bei Abbruch EOT senden", "bool", ""),
)

FIELDS = (
    *FIELDS,
    FieldSpec(
        "parameters",
        "allow_write",
        "Schreiben von Maschinenparametern erlauben",
        "bool",
        "Voraussetzung für das Schreiben. Zusätzlich sind Freischaltung pro Sitzung, aktuelle Sicherung "
        "und getippte Bestätigung nötig.",
    ),
    FieldSpec(
        "parameters",
        "inactivity_timeout_ms",
        "Ende der Parameterausgabe nach Inaktivität",
        "int",
        "",
        200,
        3_600_000,
        suffix=" ms",
    ),
    FieldSpec(
        "parameters",
        "first_byte_timeout_ms",
        "Max. Wartezeit auf Parameterausgabe",
        "int",
        "0 = unbegrenzt.",
        0,
        86_400_000,
        suffix=" ms",
    ),
    FieldSpec(
        "parameters", "max_bytes", "Maximale Größe Parameterdatei", "int", "", 1, 100 * 1024 * 1024, suffix=" Bytes"
    ),
    FieldSpec(
        "parameters",
        "identification_pattern",
        "Erkennungsmuster Parameterdatei (Regex)",
        "text",
        "Optional. Dateien ohne Treffer werden nicht geschrieben. Format der DIALOG 11 nicht verifiziert.",
    ),
    FieldSpec(
        "parameters",
        "max_backup_age_min",
        "Max. Alter der Sicherung vor dem Schreiben",
        "int",
        "",
        1,
        1440,
        suffix=" min",
    ),
)

SECTION_TITLES = {
    "serial": "Schnittstelle",
    "transfer": "Senden",
    "receive": "Empfangen",
    "protocol": "Protokoll (LSV2)",
    "parameters": "Maschinenparameter",
}


class ProfileEditor(QWidget):
    changed = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._widgets: dict[tuple[str, str], QWidget] = {}
        self._profile: Profile | None = None
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        general = QFormLayout()
        self.name_edit = QLineEdit()
        self.name_edit.setMaxLength(120)
        self.name_edit.textEdited.connect(self.changed.emit)
        general.addRow("Anzeigename", self.name_edit)
        self.description_edit = QLineEdit()
        self.description_edit.textEdited.connect(self.changed.emit)
        general.addRow("Beschreibung", self.description_edit)
        self.verified_box = QCheckBox("An realer Maschine verifiziert")
        self.verified_box.setToolTip("Nur setzen, wenn alle Parameter an der realen FP2A erfolgreich geprüft wurden.")
        self.verified_box.toggled.connect(lambda _c: self.changed.emit())
        general.addRow("", self.verified_box)
        layout.addLayout(general)

        self.tabs = QTabWidget()
        forms: dict[str, QFormLayout] = {}
        for section, title in SECTION_TITLES.items():
            page = QWidget()
            form = QFormLayout(page)
            scroll = QScrollArea()
            scroll.setWidgetResizable(True)
            scroll.setWidget(page)
            self.tabs.addTab(scroll, title)
            forms[section] = form
        for spec in FIELDS:
            widget = self._make_widget(spec)
            widget.setToolTip(spec.tooltip)
            widget.setAccessibleName(spec.label)
            self._widgets[(spec.section, spec.key)] = widget
            if isinstance(widget, QCheckBox):
                forms[spec.section].addRow("", widget)
            else:
                forms[spec.section].addRow(spec.label, widget)
        layout.addWidget(self.tabs)

    def _make_widget(self, spec: FieldSpec) -> QWidget:
        if spec.kind == "bool":
            box = QCheckBox(spec.label)
            box.toggled.connect(lambda _c: self.changed.emit())
            return box
        if spec.kind == "int":
            spin = QSpinBox()
            spin.setRange(spec.minimum, spec.maximum)
            spin.setSuffix(spec.suffix)
            spin.setGroupSeparatorShown(True)
            spin.valueChanged.connect(lambda _v: self.changed.emit())
            return spin
        if spec.kind == "choice":
            combo = QComboBox()
            for value, label in spec.choices:
                combo.addItem(label, value)
            combo.currentIndexChanged.connect(lambda _i: self.changed.emit())
            return combo
        edit = QLineEdit()
        edit.textEdited.connect(lambda _t: self.changed.emit())
        return edit

    def widget(self, section: str, key: str) -> QWidget:
        return self._widgets[(section, key)]

    def load(self, profile: Profile) -> None:
        self._profile = profile
        data = profile.to_dict()
        self.blockSignals(True)
        self.name_edit.setText(profile.display_name)
        self.description_edit.setText(profile.description)
        self.verified_box.setChecked(profile.verification_status is VerificationStatus.VERIFIED)
        for (section, key), widget in self._widgets.items():
            value = data[section][key]
            if isinstance(widget, QCheckBox):
                widget.setChecked(bool(value))
            elif isinstance(widget, QSpinBox):
                widget.setValue(int(value))
            elif isinstance(widget, QComboBox):
                index = next(
                    (i for i in range(widget.count()) if widget.itemData(i) == value),
                    -1,
                )
                widget.setCurrentIndex(max(index, 0))
            elif isinstance(widget, QLineEdit):
                widget.setText(LIST_SEPARATOR.join(value) if isinstance(value, list) else str(value))
        self.blockSignals(False)

    def to_dict(self) -> dict[str, Any]:
        """Profile dictionary built from the form (validated by the caller)."""
        if self._profile is None:
            raise RuntimeError("Kein Profil geladen")
        data = self._profile.to_dict()
        data["display_name"] = self.name_edit.text().strip()
        data["description"] = self.description_edit.text().strip()
        data["verification_status"] = (
            VerificationStatus.VERIFIED.value if self.verified_box.isChecked() else VerificationStatus.UNVERIFIED.value
        )
        for (section, key), widget in self._widgets.items():
            if isinstance(widget, QCheckBox):
                data[section][key] = widget.isChecked()
            elif isinstance(widget, QSpinBox):
                data[section][key] = widget.value()
            elif isinstance(widget, QComboBox):
                data[section][key] = widget.currentData()
            elif isinstance(widget, QLineEdit):
                if isinstance(data[section][key], list):
                    data[section][key] = [part.strip() for part in widget.text().split("|") if part.strip()]
                else:
                    data[section][key] = widget.text()
        return data
