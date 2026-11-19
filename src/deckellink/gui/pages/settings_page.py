"""Page "Einstellungen": connection, simulator, profiles and general options."""

from __future__ import annotations

from typing import Any

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QButtonGroup,
    QCheckBox,
    QComboBox,
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPlainTextEdit,
    QPushButton,
    QRadioButton,
    QScrollArea,
    QSpinBox,
    QVBoxLayout,
    QWidget,
)

from deckellink.config import LOG_LEVELS, AppConfig
from deckellink.domain.models import Profile, VerificationStatus
from deckellink.gui.widgets.error_panel import ErrorPanel
from deckellink.gui.widgets.profile_editor import ProfileEditor
from deckellink.simulation.scenarios import REPLAY_SCENARIO_ID, SCENARIOS, get_scenario
from deckellink.transport.network_transport import ConnectionKind
from deckellink.transport.serial_transport import PortInfo


class SettingsPage(QWidget):
    simulation_toggled = Signal(bool)
    port_selected = Signal(str)
    network_changed = Signal(str, str, int)
    refresh_ports_requested = Signal()
    scenario_selected = Signal(str)
    profile_selected = Signal(str)
    profile_save_requested = Signal(object)
    profile_duplicate_requested = Signal(str)
    profile_delete_requested = Signal(str)
    profiles_reload_requested = Signal()
    profile_export_requested = Signal(str)
    profile_import_requested = Signal()
    replay_file_requested = Signal()
    general_changed = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        outer = QVBoxLayout(self)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        content = QWidget()
        root = QVBoxLayout(content)
        scroll.setWidget(content)
        outer.addWidget(scroll)
        self._loading = False

        # --- connection ------------------------------------------------------------
        conn = QGroupBox("Verbindung")
        conn_form = QFormLayout(conn)
        mode_row = QHBoxLayout()
        self.sim_radio = QRadioButton("Simulationsmodus (keine Maschine)")
        self.real_radio = QRadioButton("Realbetrieb (COM-Port oder Netzwerkadapter)")
        group = QButtonGroup(self)
        group.addButton(self.sim_radio)
        group.addButton(self.real_radio)
        self.sim_radio.toggled.connect(self._mode_toggled)
        mode_row.addWidget(self.sim_radio)
        mode_row.addWidget(self.real_radio)
        mode_row.addStretch(1)
        conn_form.addRow("Betriebsart:", mode_row)
        self.kind_combo = QComboBox()
        for kind in ConnectionKind:
            self.kind_combo.addItem(kind.label, kind.value)
        self.kind_combo.setAccessibleName("Verbindungsart")
        self.kind_combo.setToolTip(
            "COM-Port: lokaler oder virtueller COM-Port (auch Treiber eines Netzwerkadapters). "
            "TCP: RS-232/Ethernet-Adapter im TCP-Server-Modus. RFC 2217: Adapter mit Telnet COM Port Control."
        )
        self.kind_combo.currentIndexChanged.connect(self._network_edited)
        conn_form.addRow("Verbindungsart:", self.kind_combo)
        port_row = QHBoxLayout()
        self.port_combo = QComboBox()
        self.port_combo.setEditable(True)
        self.port_combo.setToolTip("COM-Port des RS-232-Adapters, z. B. COM3")
        self.port_combo.setAccessibleName("COM-Port")
        self.port_combo.currentTextChanged.connect(self._port_changed)
        port_row.addWidget(self.port_combo, 1)
        self.refresh_ports = QPushButton("Ports suchen")
        self.refresh_ports.clicked.connect(self.refresh_ports_requested.emit)
        port_row.addWidget(self.refresh_ports)
        conn_form.addRow("COM-Port:", port_row)
        net_row = QHBoxLayout()
        self.host_edit = QLineEdit()
        self.host_edit.setPlaceholderText("z. B. 192.168.0.50")
        self.host_edit.setAccessibleName("IP-Adresse des Netzwerkadapters")
        self.host_edit.editingFinished.connect(self._network_edited)
        net_row.addWidget(self.host_edit, 1)
        net_row.addWidget(QLabel("TCP-Port:"))
        self.net_port_spin = QSpinBox()
        self.net_port_spin.setRange(1, 65535)
        self.net_port_spin.setAccessibleName("TCP-Port des Netzwerkadapters")
        self.net_port_spin.editingFinished.connect(self._network_edited)
        net_row.addWidget(self.net_port_spin)
        conn_form.addRow("Netzwerkadapter:", net_row)
        self.network_hint = QLabel()
        self.network_hint.setWordWrap(True)
        conn_form.addRow("", self.network_hint)
        self.scenario_combo = QComboBox()
        for scenario in SCENARIOS:
            self.scenario_combo.addItem(scenario.title, scenario.id)
        self.scenario_combo.currentIndexChanged.connect(self._scenario_changed)
        conn_form.addRow("Simulator-Szenario:", self.scenario_combo)
        replay_row = QHBoxLayout()
        self.replay_label = QLabel("–")
        self.replay_label.setWordWrap(True)
        replay_row.addWidget(self.replay_label, 1)
        self.replay_button = QPushButton("Aufzeichnung wählen …")
        self.replay_button.clicked.connect(self.replay_file_requested.emit)
        replay_row.addWidget(self.replay_button)
        self.replay_row_label = QLabel("Aufzeichnung:")
        conn_form.addRow(self.replay_row_label, replay_row)
        self.scenario_description = QLabel()
        self.scenario_description.setWordWrap(True)
        conn_form.addRow("", self.scenario_description)
        root.addWidget(conn)

        # --- profiles ----------------------------------------------------------------
        prof = QGroupBox("Kommunikationsprofil")
        prof_layout = QVBoxLayout(prof)
        row = QHBoxLayout()
        self.profile_combo = QComboBox()
        self.profile_combo.setAccessibleName("Aktives Profil")
        self.profile_combo.currentIndexChanged.connect(self._profile_changed)
        row.addWidget(self.profile_combo, 1)
        self.save_button = QPushButton("Profil &speichern")
        self.save_button.setToolTip(
            "Speichert die Änderungen. Mitgelieferte Standardprofile werden nie überschrieben - "
            "es entsteht eine benutzerdefinierte Kopie."
        )
        self.save_button.clicked.connect(self._save_clicked)
        self.duplicate_button = QPushButton("Duplizieren")
        self.duplicate_button.clicked.connect(lambda: self.profile_duplicate_requested.emit(self.current_profile_id()))
        self.delete_button = QPushButton("Löschen")
        self.delete_button.setToolTip("Nur benutzerdefinierte Profile; eine Sicherung wird angelegt.")
        self.delete_button.clicked.connect(lambda: self.profile_delete_requested.emit(self.current_profile_id()))
        self.export_button = QPushButton("Exportieren …")
        self.export_button.setToolTip("Profil als JSON-Datei speichern, z. B. um ein geprüftes Profil weiterzugeben.")
        self.export_button.clicked.connect(lambda: self.profile_export_requested.emit(self.current_profile_id()))
        self.import_button = QPushButton("Importieren …")
        self.import_button.setToolTip(
            "Profil aus JSON-Datei übernehmen (wird geprüft, als Benutzerprofil gespeichert)."
        )
        self.import_button.clicked.connect(self.profile_import_requested.emit)
        self.reload_button = QPushButton("Neu laden")
        self.reload_button.clicked.connect(self.profiles_reload_requested.emit)
        for button in (
            self.save_button,
            self.duplicate_button,
            self.delete_button,
            self.export_button,
            self.import_button,
            self.reload_button,
        ):
            row.addWidget(button)
        prof_layout.addLayout(row)
        self.profile_status = QLabel()
        self.profile_status.setWordWrap(True)
        prof_layout.addWidget(self.profile_status)
        self.profile_error = ErrorPanel()
        prof_layout.addWidget(self.profile_error)
        self.profile_warnings = QPlainTextEdit()
        self.profile_warnings.setReadOnly(True)
        self.profile_warnings.setMaximumHeight(110)
        self.profile_warnings.setAccessibleName("Profilwarnungen")
        prof_layout.addWidget(QLabel("Warnungen und Hinweise zum Profil:"))
        prof_layout.addWidget(self.profile_warnings)
        self.editor = ProfileEditor()
        self.editor.setMinimumHeight(380)
        prof_layout.addWidget(self.editor)
        root.addWidget(prof)

        # --- general -------------------------------------------------------------------
        general = QGroupBox("Allgemein")
        form = QFormLayout(general)
        self.safety_notice = QCheckBox("Sicherheitshinweis vor realer Übertragung anzeigen")
        self.raw_logging = QCheckBox("Rohdatenprotokoll (vollständiger Datenverkehr) in Datei schreiben")
        self.mask_paths = QCheckBox("Dateipfade in Diagnosepaketen maskieren")
        self.log_level = QComboBox()
        for level in LOG_LEVELS:
            self.log_level.addItem(level, level)
        self.history = QSpinBox()
        self.history.setRange(100, 100_000)
        self.history.setSuffix(" Einträge")
        self.retention = QSpinBox()
        self.retention.setRange(0, 3650)
        self.retention.setSpecialValueText("deaktiviert (Backups nie löschen)")
        self.retention.setSuffix(" Tage")
        self.retention.setToolTip("Automatisches Löschen alter Backups. Standard: deaktiviert.")
        form.addRow("", self.safety_notice)
        form.addRow("", self.raw_logging)
        form.addRow("", self.mask_paths)
        form.addRow("Log-Level:", self.log_level)
        form.addRow("Terminal-Historie:", self.history)
        form.addRow("Backup-Aufbewahrung:", self.retention)
        for widget in (self.safety_notice, self.raw_logging, self.mask_paths):
            widget.toggled.connect(self._general_changed)
        self.log_level.currentIndexChanged.connect(self._general_changed)
        self.history.valueChanged.connect(self._general_changed)
        self.retention.valueChanged.connect(self._general_changed)
        self.data_dir = QLabel()
        self.data_dir.setWordWrap(True)
        form.addRow("Datenordner:", self.data_dir)
        root.addWidget(general)
        root.addStretch(1)

    # --- loading -------------------------------------------------------------------
    def load_config(self, config: AppConfig, data_dir: str) -> None:
        self._loading = True
        self.sim_radio.setChecked(config.simulation_mode)
        self.real_radio.setChecked(not config.simulation_mode)
        self.port_combo.setEditText(config.port)
        self.kind_combo.setCurrentIndex(max(self.kind_combo.findData(config.connection_kind), 0))
        self.host_edit.setText(config.network_host)
        self.net_port_spin.setValue(config.network_port)
        index = self.scenario_combo.findData(config.simulation_scenario)
        self.scenario_combo.setCurrentIndex(max(index, 0))
        self.scenario_description.setText(get_scenario(config.simulation_scenario).description)
        self.set_replay_file(config.simulation_replay_file)
        self.safety_notice.setChecked(config.show_safety_notice)
        self.raw_logging.setChecked(config.raw_traffic_logging)
        self.mask_paths.setChecked(config.mask_paths_in_export)
        self.log_level.setCurrentIndex(max(self.log_level.findData(config.log_level), 0))
        self.history.setValue(config.terminal_history_lines)
        self.retention.setValue(config.backup_retention_days or 0)
        self.data_dir.setText(data_dir)
        self._update_mode_widgets()
        self._loading = False

    def apply_general(self, config: AppConfig) -> None:
        config.show_safety_notice = self.safety_notice.isChecked()
        config.raw_traffic_logging = self.raw_logging.isChecked()
        config.mask_paths_in_export = self.mask_paths.isChecked()
        config.log_level = str(self.log_level.currentData())
        config.terminal_history_lines = self.history.value()
        config.backup_retention_days = self.retention.value() or None

    def set_ports(self, ports: list[PortInfo], current: str) -> None:
        self._loading = True
        self.port_combo.clear()
        for port in ports:
            self.port_combo.addItem(port.label, port.device)
        self.port_combo.setEditText(current)
        self._loading = False

    def set_profiles(self, profiles: list[Profile], active_id: str) -> None:
        self._loading = True
        self.profile_combo.clear()
        for profile in profiles:
            tag = "Standard" if profile.builtin else "Benutzer"
            self.profile_combo.addItem(f"{profile.display_name}  [{tag}]", profile.id)
        index = self.profile_combo.findData(active_id)
        self.profile_combo.setCurrentIndex(max(index, 0))
        self._loading = False

    def show_profile(self, profile: Profile, warnings: list[str]) -> None:
        self.editor.load(profile)
        verified = profile.verification_status is VerificationStatus.VERIFIED
        origin = "mitgeliefertes Standardprofil (schreibgeschützt)" if profile.builtin else "Benutzerprofil"
        self.profile_status.setText(
            f"ID: {profile.id} - {origin} - "
            + ("✔ an Maschine verifiziert" if verified else "⚠ NICHT an realer Maschine verifiziert")
            + f"\n{profile.serial.summary}"
        )
        self.profile_warnings.setPlainText("\n".join(f"⚠ {w}" for w in warnings) or "Keine Warnungen.")
        self.delete_button.setEnabled(not profile.builtin)

    def current_profile_id(self) -> str:
        return str(self.profile_combo.currentData() or "")

    # --- slots ----------------------------------------------------------------------
    def _update_mode_widgets(self) -> None:
        sim = self.sim_radio.isChecked()
        replay = str(self.scenario_combo.currentData()) == REPLAY_SCENARIO_ID
        for widget in (self.replay_label, self.replay_button, self.replay_row_label):
            widget.setVisible(replay)
        self.replay_button.setEnabled(sim)
        kind = ConnectionKind(str(self.kind_combo.currentData() or "serial"))
        self.scenario_combo.setEnabled(sim)
        self.kind_combo.setEnabled(not sim)
        self.port_combo.setEnabled(not sim and not kind.is_network)
        self.refresh_ports.setEnabled(not sim and not kind.is_network)
        self.host_edit.setEnabled(not sim and kind.is_network)
        self.net_port_spin.setEnabled(not sim and kind.is_network)
        hints = {
            ConnectionKind.SERIAL: "",
            ConnectionKind.TCP: (
                "TCP-Rohdaten: Baudrate, Datenformat und Handshake im Adapter einstellen (müssen zum Profil "
                "passen). Modemleitungen (CTS/DSR) sind nicht sichtbar - Profile mit CTS-Prüfung sind hier "
                "nicht nutzbar. Viele Adapter erlauben nur eine gleichzeitige Verbindung."
            ),
            ConnectionKind.RFC2217: (
                "RFC 2217: DeckelLink überträgt Baudrate, Datenformat und Steuerleitungen an den Adapter; "
                "der Adapter muss RFC 2217 unterstützen."
            ),
        }
        self.network_hint.setText(hints[kind])
        self.network_hint.setVisible(bool(hints[kind]))

    def _network_edited(self, *_args: object) -> None:
        self._update_mode_widgets()
        if not self._loading:
            self.network_changed.emit(
                str(self.kind_combo.currentData()), self.host_edit.text().strip(), self.net_port_spin.value()
            )

    def _mode_toggled(self, _checked: bool) -> None:
        self._update_mode_widgets()
        if not self._loading:
            self.simulation_toggled.emit(self.sim_radio.isChecked())

    def _port_changed(self, text: str) -> None:
        if self._loading:
            return
        data = self.port_combo.currentData()
        device = (
            str(data)
            if data and self.port_combo.currentText() == self.port_combo.itemText(self.port_combo.currentIndex())
            else text.split(" - ", maxsplit=1)[0].strip()
        )
        self.port_selected.emit(device)

    def set_replay_file(self, path: str) -> None:
        self.replay_label.setText(path or "keine gewählt - es wird das Beispielprogramm verwendet")

    def _scenario_changed(self, _index: int) -> None:
        scenario_id = str(self.scenario_combo.currentData())
        self.scenario_description.setText(get_scenario(scenario_id).description)
        self._update_mode_widgets()
        if not self._loading:
            self.scenario_selected.emit(scenario_id)

    def _profile_changed(self, _index: int) -> None:
        if not self._loading:
            self.profile_selected.emit(self.current_profile_id())

    def _save_clicked(self) -> None:
        data: dict[str, Any] = self.editor.to_dict()
        self.profile_save_requested.emit(data)

    def _general_changed(self, *_args: object) -> None:
        if not self._loading:
            self.general_changed.emit()
