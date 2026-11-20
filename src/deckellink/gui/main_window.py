"""Main window: coordinates pages, background tasks, profiles and storage."""

from __future__ import annotations

import logging
import queue
from collections.abc import Callable
from dataclasses import replace
from datetime import datetime
from pathlib import Path
from typing import Any

from PySide6.QtCore import Qt, QUrl
from PySide6.QtGui import (
    QCloseEvent,
    QDesktopServices,
    QDragEnterEvent,
    QDropEvent,
    QKeySequence,
    QShortcut,
    QShowEvent,
)
from PySide6.QtWidgets import (
    QApplication,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QListWidget,
    QMainWindow,
    QMessageBox,
    QScrollArea,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from deckellink.clock import Clock, ScaledClock, SystemClock
from deckellink.commissioning import CommissioningRecord, CommissioningStore, StepStatus, export_report
from deckellink.config import FONT_SCALE_MAX, FONT_SCALE_MIN, AppConfig, save_config
from deckellink.diagnostics.error_messages import describe_error
from deckellink.diagnostics.export import export_diagnostics
from deckellink.diagnostics.format_detect import (
    FormatGuess,
    analyze_capture,
    capture_passively,
    capture_settings,
    encode_frames_as_8n1,
)
from deckellink.diagnostics.loopback_test import LoopbackPattern, LoopbackVerdict, make_pattern, run_loopback_test
from deckellink.diagnostics.terminal import TerminalBuffer, TrafficFileLogger
from deckellink.domain.errors import ProfileValidationError, UserFacingError
from deckellink.domain.models import Parity, Profile, VerificationStatus
from deckellink.domain.results import (
    ReceivedProgram,
    ReceiveProgress,
    ReceiveResult,
    SendResult,
    TrafficEvent,
    TransferProgress,
)
from deckellink.domain.states import ReceiveState, SendState
from deckellink.gui.dialogs.commissioning_dialog import CommissioningDialog
from deckellink.gui.dialogs.confirm_transfer import ConfirmInfo, ConfirmOutcome, ask_confirmation
from deckellink.gui.dialogs.export_dialog import ExportDialog
from deckellink.gui.dialogs.format_detect_dialog import FormatDetectDialog
from deckellink.gui.dialogs.help_dialog import HelpDialog, show_about
from deckellink.gui.dialogs.loopback_dialog import LoopbackDialog
from deckellink.gui.dialogs.parameter_write_dialog import ParameterWriteInfo, ask_parameter_write
from deckellink.gui.pages.diagnostics_page import DiagnosticsPage
from deckellink.gui.pages.log_page import LogPage, QtLogHandler
from deckellink.gui.pages.parameters_page import ParametersPage
from deckellink.gui.pages.programs_page import ProgramsPage
from deckellink.gui.pages.receive_page import ReceivePage
from deckellink.gui.pages.settings_page import SettingsPage
from deckellink.gui.pages.terminal_page import TerminalPage
from deckellink.gui.pages.transfer_page import TransferPage
from deckellink.gui.style import StatusKind, apply_theme
from deckellink.gui.viewmodels import (
    LoadedProgram,
    evaluate_receive_gate,
    evaluate_send_gate,
)
from deckellink.gui.widgets.header import HeaderBar
from deckellink.gui.workers import Job, TaskCallbacks, TaskHandle
from deckellink.logging.setup import set_level
from deckellink.nc.analyzer import analyze
from deckellink.nc.transformer import TransformError, transform
from deckellink.parameters.archive import ParameterArchive, StoredParameterSet
from deckellink.parameters.service import (
    ParameterBackupInfo,
    ParameterCandidate,
    ParameterWriteGate,
    analyze_parameters,
    diff_parameters,
    evaluate_parameter_write_gate,
    parameter_profile,
    prepare_parameter_payload,
)
from deckellink.parameters.transfer import read_parameters, write_parameters
from deckellink.power import keep_system_awake
from deckellink.profiles.loader import builtin_profiles_dir
from deckellink.profiles.repository import ProfileRepository, ProfileRepositoryError
from deckellink.profiles.schema import parse_profile
from deckellink.safety import PARAMETER_WRITE_NOTICE
from deckellink.simulation.lsv2_cnc import Lsv2SimulatedCnc
from deckellink.simulation.scenarios import (
    REPLAY_SCENARIO_ID,
    SAMPLE_PARAMETERS,
    SAMPLE_PROGRAM,
    Scenario,
    get_scenario,
    lsv2_config_for,
)
from deckellink.storage.archive import ReceiveArchive, StoredReceive, export_copy
from deckellink.storage.backup import BackupManager
from deckellink.storage.files import sanitize_stem, unique_path, write_new_file
from deckellink.storage.paths import AppPaths
from deckellink.transfer.cancellation import CancellationToken
from deckellink.transfer.lsv2_transfer import Lsv2ProgramReceiver, Lsv2ProgramSender
from deckellink.transfer.receiver import ProgramReceiver, ReceiveCallbacks, process_received
from deckellink.transfer.sender import ProgramSender, SendCallbacks
from deckellink.transport.base import ModemStatus, Transport
from deckellink.transport.loopback import LoopbackTransport
from deckellink.transport.network_transport import ConnectionKind, connection_target, create_hardware_transport
from deckellink.transport.serial_transport import available_ports
from deckellink.transport.simulated_transport import SIMULATED_PORT_NAME, SimulatedTransport, SimulationConfig
from deckellink.version import APP_NAME, __version__

log = logging.getLogger(__name__)

NAV_ITEMS = (
    "Übertragen",
    "Empfangen",
    "NC-Programme",
    "Maschinenparameter",
    "Terminal",
    "Diagnose",
    "Einstellungen",
    "Protokoll",
)
NC_FILE_FILTER = "NC-Programme (*.nc *.NC *.txt *.TXT *.mpf *.MPF *.din *.DIN);;Alle Dateien (*)"

ConfirmFn = Callable[[ConfirmInfo, QWidget | None], ConfirmOutcome]

TASK_SEND = "Senden"
TASK_RECEIVE = "Empfangen"
TASK_LOOPBACK = "Loopback-Test"
TASK_MONITOR = "Diagnose"
TASK_PARAM_READ = "Parameter auslesen"
TASK_PARAM_WRITE = "Parameter schreiben"
TASK_FORMAT = "Datenformat ermitteln"
PROFILE_FILE_FILTER = "DeckelLink-Profile (*.json);;Alle Dateien (*)"
PARAMETER_FILE_FILTER = "Parameterdateien (*.par *.txt *.TXT *.prm);;Alle Dateien (*)"


class MainWindow(QMainWindow):
    def __init__(
        self,
        paths: AppPaths,
        config: AppConfig,
        *,
        sim_time_scale: float = 1.0,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.paths = paths.ensure()
        self.config = config
        self.sim_time_scale = sim_time_scale
        self.backups = BackupManager(paths.backups, retention_days=config.backup_retention_days)
        self.repo = ProfileRepository(builtin_profiles_dir(), paths.profiles, self.backups)
        self.archive = ReceiveArchive(paths)
        self.profile, fell_back = self.repo.resolve(config.active_profile_id)
        self.config.active_profile_id = self.profile.id
        self.terminal = TerminalBuffer(config.terminal_history_lines)
        self.traffic_logger: TrafficFileLogger | None = None
        self.program: LoadedProgram | None = None
        self.task: TaskHandle | None = None
        self._finishing: list[TaskHandle] = []
        self._active_transport: Transport | None = None
        self.retained_transport: Transport | None = None
        self._monitor_queue: queue.Queue[tuple[str, bool]] = queue.Queue()
        self.last_result: dict[str, Any] | None = None
        self.last_stored: StoredReceive | None = None
        self._last_failed = False
        self._busy_text = ""
        self.confirm: ConfirmFn = ask_confirmation
        self.confirm_parameter_write: Callable[[ParameterWriteInfo, QWidget | None], bool] = ask_parameter_write
        self.param_archive = ParameterArchive(paths.root)
        self.commissioning_store = CommissioningStore(paths.root)
        self.commissioning_dialog: CommissioningDialog | None = None
        self.commissioning_record: CommissioningRecord | None = None
        self.format_dialog: FormatDetectDialog | None = None
        self.param_unlocked = False  # session only, never persisted
        self.param_backup: ParameterBackupInfo | None = None
        self.param_candidate: ParameterCandidate | None = None
        self.last_written_parameters: bytes | None = None
        self.loopback_dialog: LoopbackDialog | None = None

        self.setWindowTitle(f"{APP_NAME} {__version__} - DECKEL FP2A / DIALOG 11")
        self.setMinimumSize(800, 560)
        self.resize(1280, 860)
        self._build_ui()
        self._build_menus()
        self.setAcceptDrops(True)

        self.log_handler = QtLogHandler()
        self.log_handler.emitter.record.connect(self.log_page.add_record)
        logging.getLogger().addHandler(self.log_handler)

        self.settings_page.load_config(config, str(paths.root))
        self._refresh_ports()
        self._refresh_profiles()
        self._apply_raw_logging(config.raw_traffic_logging)
        self.refresh_programs()
        self.refresh_parameter_sets()
        self._refresh_all()

        for problem in self.repo.problems:
            log.error("Profile file problem %s: %s", problem.path.name, problem.error)
        if self.repo.problems or fell_back:
            details = [f"{p.path.name}: {p.error}" for p in self.repo.problems]
            if fell_back:
                details.append(f"Gewähltes Profil nicht verfügbar - verwende '{self.profile.display_name}'.")
            self.settings_page.profile_error.show_error(
                UserFacingError(
                    "Nicht alle Profile konnten geladen werden. Es wird ein sicheres Standardprofil verwendet.",
                    tuple(details),
                    "\n".join(details),
                    "Profile laden",
                )
            )
            self.statusBar().showMessage("Profilfehler - siehe Einstellungen", 10000)
        log.info(
            "Main window ready (mode=%s, profile=%s)",
            "simulation" if config.simulation_mode else "real",
            self.profile.id,
        )

    # ------------------------------------------------------------------ UI
    def _build_ui(self) -> None:
        central = QWidget()
        outer = QVBoxLayout(central)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(0)
        self.header = HeaderBar()
        outer.addWidget(self.header)
        body = QHBoxLayout()
        body.setContentsMargins(0, 0, 0, 0)
        self.nav = QListWidget()
        self.nav.setObjectName("Navigation")
        self.nav.setFixedWidth(230)
        self.nav.setAccessibleName("Hauptnavigation")
        for index, name in enumerate(NAV_ITEMS, start=1):
            self.nav.addItem(f"{index}  {name}")
        body.addWidget(self.nav)
        self.stack = QStackedWidget()
        body.addWidget(self.stack, 1)
        outer.addLayout(body, 1)
        self.setCentralWidget(central)

        self.transfer_page = TransferPage()
        self.receive_page = ReceivePage()
        self.programs_page = ProgramsPage()
        self.parameters_page = ParametersPage()
        self.terminal_page = TerminalPage(self.terminal)
        self.diagnostics_page = DiagnosticsPage()
        self.settings_page = SettingsPage()
        self.log_page = LogPage()
        for page in (
            self.transfer_page,
            self.receive_page,
            self.programs_page,
            self.parameters_page,
            self.terminal_page,
            self.diagnostics_page,
            self.settings_page,
            self.log_page,
        ):
            self.stack.addWidget(self._scrollable(page))
        self.nav.currentRowChanged.connect(self.stack.setCurrentIndex)
        self.nav.setCurrentRow(0)

        tp = self.transfer_page
        tp.open_requested.connect(self.open_file)
        tp.check_requested.connect(self.recheck_file)
        tp.sample_requested.connect(self.load_sample)
        tp.send_requested.connect(self.start_send)
        tp.dripfeed_requested.connect(self.start_drip_feed)
        tp.cancel_requested.connect(self.cancel_task)
        tp.receive_requested.connect(lambda: self.nav.setCurrentRow(1))
        tp.options_changed.connect(self._transfer_options_changed)

        rp = self.receive_page
        rp.start_requested.connect(self.start_receive)
        rp.cancel_requested.connect(self.cancel_task)
        rp.open_folder_requested.connect(lambda: self._open_folder(self.paths.received))
        rp.load_for_send_requested.connect(self.load_for_send)

        pp = self.programs_page
        pp.refresh_requested.connect(self.refresh_programs)
        pp.open_folder_requested.connect(lambda: self._open_folder(self.paths.received))
        pp.export_requested.connect(self.export_program)
        pp.load_for_send_requested.connect(self.load_for_send)

        mp = self.parameters_page
        mp.read_requested.connect(self.start_parameter_read)
        mp.cancel_requested.connect(self.cancel_task)
        mp.open_folder_requested.connect(lambda: self._open_folder(self.param_archive.directory))
        mp.unlock_toggled.connect(self.set_parameter_write_unlocked)
        mp.compare_requested.connect(self.compare_parameter_set)
        mp.select_requested.connect(self.select_parameter_file)
        mp.load_external_requested.connect(self.load_external_parameter_file)
        mp.write_requested.connect(self.start_parameter_write)

        self.terminal_page.file_logging_toggled.connect(self._terminal_file_logging_toggled)

        dp = self.diagnostics_page
        dp.monitor_toggled.connect(self.toggle_monitor)
        dp.control_line_requested.connect(self.request_control_line)
        dp.loopback_requested.connect(self.open_loopback)
        dp.export_requested.connect(self.export_diagnostics_package)

        sp = self.settings_page
        sp.simulation_toggled.connect(self.set_simulation_mode)
        sp.port_selected.connect(self.set_port)
        sp.network_changed.connect(self.set_network_connection)
        sp.refresh_ports_requested.connect(self._refresh_ports)
        sp.scenario_selected.connect(self.set_scenario)
        sp.profile_selected.connect(self.select_profile)
        sp.profile_save_requested.connect(self.save_profile)
        sp.profile_duplicate_requested.connect(self.duplicate_profile)
        sp.profile_delete_requested.connect(self.delete_profile)
        sp.profiles_reload_requested.connect(self.reload_profiles)
        sp.profile_export_requested.connect(self.export_profile)
        sp.profile_import_requested.connect(self.import_profile)
        sp.replay_file_requested.connect(self.choose_replay_file)
        self.diagnostics_page.format_requested.connect(self.open_format_detection)
        sp.general_changed.connect(self._general_settings_changed)

        self.log_page.open_folder_requested.connect(lambda: self._open_folder(self.paths.logs))

        QShortcut(QKeySequence(Qt.Key.Key_Escape), self, self.cancel_task)
        for index in range(len(NAV_ITEMS)):
            QShortcut(QKeySequence(f"Ctrl+{index + 1}"), self, lambda i=index: self.nav.setCurrentRow(i))

    def _build_menus(self) -> None:
        bar = self.menuBar()
        file_menu = bar.addMenu("&Datei")
        open_action = file_menu.addAction("NC-Programm &öffnen …")
        open_action.setShortcut(QKeySequence.StandardKey.Open)
        open_action.triggered.connect(self.open_file)
        self.recent_menu = file_menu.addMenu("&Zuletzt geöffnet")
        file_menu.addSeparator()
        quit_action = file_menu.addAction("&Beenden")
        quit_action.setShortcut(QKeySequence("Ctrl+Q"))
        quit_action.triggered.connect(self.close)

        view_menu = bar.addMenu("&Ansicht")
        for text, shortcut, delta in (
            ("Schrift &größer", "Ctrl++", 10),
            ("Schrift &kleiner", "Ctrl+-", -10),
            ("Schrift &Standard", "Ctrl+0", 0),
        ):
            action = view_menu.addAction(text)
            action.setShortcut(QKeySequence(shortcut))
            action.triggered.connect(lambda _c=False, d=delta: self.change_font_scale(d))
        self.tools_menu = bar.addMenu("&Werkzeuge")
        loopback_action = self.tools_menu.addAction("&Loopback-Test …")
        loopback_action.triggered.connect(self.open_loopback)
        export_action = self.tools_menu.addAction("Diagnosepaket &exportieren …")
        export_action.triggered.connect(self.export_diagnostics_package)
        self.tools_menu.addSeparator()
        wizard_action = self.tools_menu.addAction("&Inbetriebnahme-Assistent …")
        wizard_action.triggered.connect(self.open_commissioning)
        format_action = self.tools_menu.addAction("&Datenformat ermitteln (nur Mitlesen) …")
        format_action.triggered.connect(self.open_format_detection)

        help_menu = bar.addMenu("&Hilfe")
        manual = help_menu.addAction("&Handbuch …")
        manual.setShortcut(QKeySequence(Qt.Key.Key_F1))
        manual.triggered.connect(lambda _checked=False: self.open_help("README.md"))
        safety = help_menu.addAction("&Sicherheitskonzept …")
        safety.triggered.connect(lambda: self.open_help("SAFETY.md"))
        commissioning = help_menu.addAction("&Inbetriebnahme …")
        commissioning.triggered.connect(lambda: self.open_help("COMMISSIONING.md"))
        help_menu.addSeparator()
        about = help_menu.addAction("Ü&ber DeckelLink …")
        about.triggered.connect(lambda: show_about(self))
        self._refresh_recent_menu()

    # ----------------------------------------------------------- menus / comfort
    def open_help(self, start: str = "README.md") -> HelpDialog:
        dialog = HelpDialog(self, start)
        dialog.setModal(False)
        dialog.show()
        self._help_dialog = dialog
        return dialog

    def _refresh_recent_menu(self) -> None:
        menu = self.recent_menu
        menu.clear()
        existing = [p for p in self.config.recent_files if Path(p).is_file()]
        if not existing:
            placeholder = menu.addAction("(keine)")
            placeholder.setEnabled(False)
            return
        for index, name in enumerate(existing, start=1):
            action = menu.addAction(f"&{index % 10}  {name}")
            action.triggered.connect(lambda _c=False, n=name: self.load_for_send(Path(n)))
        menu.addSeparator()
        clear = menu.addAction("Liste leeren")
        clear.triggered.connect(self.clear_recent_files)

    def clear_recent_files(self) -> None:
        self.config.recent_files = []
        self._save_config()
        self._refresh_recent_menu()

    def change_font_scale(self, delta: int) -> None:
        current = self.config.font_scale_percent
        value = 100 if delta == 0 else min(max(current + delta, FONT_SCALE_MIN), FONT_SCALE_MAX)
        self.config.font_scale_percent = value
        app = QApplication.instance()
        if isinstance(app, QApplication):
            apply_theme(app, value / 100.0)
        self._save_config()
        self.statusBar().showMessage(f"Schriftgröße {value} %", 4000)

    def open_initial_file(self, path: Path) -> None:
        """File passed on the command line / via file association: load and check only."""
        if path.is_file():
            self.load_for_send(path)
        else:
            self.statusBar().showMessage(f"Datei nicht gefunden: {path}", 10000)

    @staticmethod
    def _dropped_files(event: QDragEnterEvent | QDropEvent) -> list[Path]:
        mime = event.mimeData()
        if not mime.hasUrls():
            return []
        return [Path(url.toLocalFile()) for url in mime.urls() if url.isLocalFile()]

    def dragEnterEvent(self, event: QDragEnterEvent) -> None:
        if self._dropped_files(event):
            event.acceptProposedAction()
        else:
            event.ignore()

    def dropEvent(self, event: QDropEvent) -> None:
        files = [p for p in self._dropped_files(event) if p.is_file()]
        if not files:
            event.ignore()
            return
        event.acceptProposedAction()
        if self.busy:
            self.statusBar().showMessage("Datei nicht geladen: ein Vorgang läuft", 6000)
            return
        self.load_for_send(files[0])
        if len(files) > 1:
            self.statusBar().showMessage("Nur die erste der abgelegten Dateien wurde geladen", 6000)

    def showEvent(self, event: QShowEvent) -> None:
        super().showEvent(event)
        if not getattr(self, "_fitted", False):
            self._fitted = True
            screen = self.screen()
            if screen is not None:
                available = screen.availableGeometry()
                width = min(self.width(), available.width() - 20)
                height = min(self.height(), available.height() - 40)
                if width < self.width() or height < self.height():
                    self.resize(max(width, self.minimumWidth()), max(height, self.minimumHeight()))

    @staticmethod
    def _scrollable(page: QWidget) -> QScrollArea:
        """Pages scroll instead of forcing a wide window (small workshop screens)."""
        area = QScrollArea()
        area.setWidgetResizable(True)
        area.setFrameShape(QFrame.Shape.NoFrame)
        area.setWidget(page)
        return area

    # ------------------------------------------------------------- helpers
    @property
    def simulation(self) -> bool:
        return self.config.simulation_mode

    @property
    def busy(self) -> bool:
        return self.task is not None

    @property
    def port_label(self) -> str:
        if self.simulation:
            return SIMULATED_PORT_NAME
        kind = self.connection_kind
        if kind.is_network:
            return self.connection_target or "(kein Netzwerkadapter konfiguriert)"
        return self.config.port or "(kein Port)"

    @property
    def connection_kind(self) -> ConnectionKind:
        return ConnectionKind(self.config.connection_kind)

    @property
    def connection_target(self) -> str:
        return connection_target(
            self.connection_kind, self.config.port, self.config.network_host, self.config.network_port
        )

    def scenario(self) -> Scenario:
        return get_scenario(self.config.simulation_scenario)

    def _clock(self) -> Clock:
        return ScaledClock(self.sim_time_scale) if self.simulation else SystemClock()

    def _make_transport(self, purpose: str) -> Transport:
        serial = self.profile.serial
        if self.simulation:
            if purpose == "loopback":
                return LoopbackTransport(serial, self._clock())
            replay = self._replay_data() if self.scenario().id == REPLAY_SCENARIO_ID else None
            if purpose == "format":
                stream = replay if replay is not None else encode_frames_as_8n1(SAMPLE_PROGRAM * 4, Parity.EVEN)
                return SimulatedTransport(
                    capture_settings(serial), SimulationConfig(machine_program=stream), self._clock()
                )
            receiving = purpose in ("receive", "param_read")
            if self.profile.protocol.is_lsv2 and purpose != "monitor":
                lsv2_config = lsv2_config_for(self.scenario(), receive=receiving, drip_feed=purpose == "dripfeed")
                if purpose == "param_read":
                    lsv2_config = replace(lsv2_config, program=SAMPLE_PARAMETERS)
                if replay is not None and receiving:
                    lsv2_config = replace(lsv2_config, program=replay)
                return Lsv2SimulatedCnc(serial, self.profile.protocol, lsv2_config, self._clock())
            config = self.scenario().config
            if purpose == "param_read":
                config = replace(config, machine_program=SAMPLE_PARAMETERS)
            if replay is not None and receiving:
                config = replace(config, machine_program=replay)
            if purpose == "receive" and not config.machine_program:
                config = replace(config, machine_program=SAMPLE_PROGRAM)
            return SimulatedTransport(serial, config, self._clock())
        if purpose == "format":
            serial = capture_settings(serial)
        return create_hardware_transport(
            self.connection_kind, self.config.port, self.config.network_host, self.config.network_port, serial
        )

    def _replay_data(self) -> bytes | None:
        """Raw data of a recorded transfer for the replay scenario (None -> sample program)."""
        name = self.config.simulation_replay_file
        if not name:
            return None
        try:
            data = Path(name).read_bytes()
        except OSError as exc:
            log.warning("Replay file unreadable: %s", exc)
            self.statusBar().showMessage("Aufzeichnung nicht lesbar - Beispielprogramm wird verwendet", 8000)
            return None
        return data or None

    def _link_summary(self) -> str:
        summary = self.profile.serial.summary
        if self.profile.protocol.is_lsv2:
            summary += " | LSV2"
        return summary

    def _save_config(self) -> None:
        try:
            save_config(self.config, self.paths.config)
        except OSError as exc:
            log.error("Saving settings failed: %s", exc)
            self.statusBar().showMessage("Einstellungen konnten nicht gespeichert werden", 8000)

    def _open_folder(self, path: Path) -> None:
        path.mkdir(parents=True, exist_ok=True)
        QDesktopServices.openUrl(QUrl.fromLocalFile(str(path)))

    def _close_retained(self) -> None:
        if self.retained_transport is not None:
            self.retained_transport.close()
            self.retained_transport = None
            log.info("Retained port closed")

    # ----------------------------------------------------------- refreshing
    def _refresh_all(self) -> None:
        profile = self.profile
        self.header.set_mode(self.simulation, self.port_label)
        self.header.set_connection(self.port_label, self._link_summary())
        self.header.set_profile(profile.display_name, profile.verification_status is VerificationStatus.VERIFIED)
        connection = f"{self.port_label} | {self._link_summary()} | Profil: {profile.display_name}"
        self.transfer_page.set_connection_text(connection)
        self.receive_page.set_connection_text(connection)
        self.diagnostics_page.set_profile(profile, self.port_label)
        self.diagnostics_page.set_counters(self.terminal.tx_bytes, self.terminal.rx_bytes)
        self._refresh_gates()
        self._refresh_header_status()

    def _refresh_gates(self) -> None:
        gate = evaluate_send_gate(
            self.program,
            simulation=self.simulation,
            port=self.connection_target,
            busy=self.busy,
            warnings_acknowledged=self.transfer_page.ack_warnings.isChecked(),
        )
        self.transfer_page.apply_gate(
            gate, busy=self.busy, simulation=self.simulation, lsv2=self.profile.protocol.is_lsv2
        )
        self.receive_page.apply_gate(
            evaluate_receive_gate(simulation=self.simulation, port=self.connection_target, busy=self.busy),
            busy=self.busy,
        )
        monitoring = self.task is not None and self.task.name == TASK_MONITOR
        self.diagnostics_page.set_busy(self.busy, monitoring)
        self.settings_page.setEnabled(not self.busy)
        param_task = self.task is not None and self.task.name in (TASK_PARAM_READ, TASK_PARAM_WRITE)
        self.parameters_page.set_busy(self.busy, param_task)
        self._refresh_parameter_gate()

    def _refresh_header_status(self) -> None:
        if self.task is not None:
            if self.task.name == TASK_MONITOR:
                self.header.set_status(StatusKind.READY, "Verbunden (Diagnose)")
            else:
                self.header.set_status(StatusKind.ACTIVE, self._busy_text or f"{self.task.name} aktiv")
        elif self.retained_transport is not None and self.retained_transport.is_open:
            self.header.set_status(StatusKind.READY, "Verbunden - Port offen")
        elif self._last_failed:
            self.header.set_status(StatusKind.ERROR, "Fehler beim letzten Vorgang")
        elif self.simulation:
            self.header.set_status(StatusKind.READY, "Bereit (Simulator)")
        elif self.connection_target:
            self.header.set_status(StatusKind.READY, f"Bereit - {self.port_label}")
        else:
            self.header.set_status(StatusKind.IDLE, "Getrennt - keine Schnittstelle konfiguriert")

    def _refresh_ports(self) -> None:
        self.settings_page.set_ports(available_ports(), self.config.port)

    def _refresh_profiles(self) -> None:
        self.settings_page.set_profiles(self.repo.profiles, self.profile.id)
        self.settings_page.show_profile(self.profile, self.repo.warnings_for(self.profile.id))

    # ----------------------------------------------------------- tasks
    def _start_task(
        self,
        name: str,
        job: Job,
        *,
        finished: Callable[[Any], None],
        state: Callable[[Any], None] | None = None,
        progress: Callable[[Any], None] | None = None,
        modem: Callable[[ModemStatus], None] | None = None,
    ) -> TaskHandle:
        handle = TaskHandle(name, job)
        worker = handle.worker
        if state is not None:
            worker.state_changed.connect(state)
        if progress is not None:
            worker.progress.connect(progress)
        worker.traffic.connect(self._on_traffic)
        worker.modem.connect(modem or self._on_modem)
        worker.finished.connect(lambda result: self._task_finished(handle, finished, result))
        worker.crashed.connect(lambda error: self._task_crashed(handle, error))
        handle.thread.finished.connect(lambda: self._thread_finished(handle))
        self.task = handle
        self._last_failed = False
        self._busy_text = f"{name} läuft"
        handle.start()
        log.info("Task started: %s", name)
        self._refresh_all()
        return handle

    def _task_finished(self, handle: TaskHandle, callback: Callable[[Any], None], result: Any) -> None:
        self._release(handle)
        callback(result)
        self._flush_terminal()
        self._refresh_all()

    def _task_crashed(self, handle: TaskHandle, error: UserFacingError) -> None:
        self._release(handle)
        self._last_failed = True
        self.diagnostics_page.last_error.setText(error.title)
        page_panel = {
            TASK_SEND: self.transfer_page.error_panel,
            TASK_RECEIVE: self.receive_page.error_panel,
            TASK_PARAM_READ: self.parameters_page.error_panel,
            TASK_PARAM_WRITE: self.parameters_page.error_panel,
        }.get(handle.name, self.diagnostics_page.error_panel)
        page_panel.show_error(error)
        if handle.name == TASK_MONITOR:
            self.diagnostics_page.set_port_open(False)
        if handle.name == TASK_LOOPBACK and self.loopback_dialog is not None:
            self.loopback_dialog.set_running(False)
        self.statusBar().showMessage(error.title, 10000)
        self._flush_terminal()
        self._refresh_all()

    def _release(self, handle: TaskHandle) -> None:
        if self.task is handle:
            self.task = None
        self._finishing.append(handle)
        self._active_transport = None

    def _thread_finished(self, handle: TaskHandle) -> None:
        if handle in self._finishing:
            self._finishing.remove(handle)

    def cancel_task(self) -> None:
        if self.task is not None and self.task.name != TASK_MONITOR:
            log.warning("User requested cancel of %s", self.task.name)
            self._busy_text = "Abbruch angefordert …"
            self.task.cancel()
            self._refresh_header_status()

    def _on_traffic(self, event: TrafficEvent) -> None:
        entries = self.terminal.add(event)
        self._write_entries(entries)
        if self.task is not None and self.task.name == TASK_RECEIVE:
            self.receive_page.append_data(event.data)
        if self.task is not None and self.task.name == TASK_PARAM_READ:
            self.parameters_page.set_received_bytes(self.terminal.rx_bytes)
        self.diagnostics_page.set_counters(self.terminal.tx_bytes, self.terminal.rx_bytes)

    def _flush_terminal(self) -> None:
        self._write_entries(self.terminal.flush())

    def _write_entries(self, entries: list[Any]) -> None:
        if not entries:
            return
        self.terminal_page.append_entries(entries)
        if self.traffic_logger is not None:
            for entry in entries:
                self.traffic_logger.write(entry)

    def _on_modem(self, status: ModemStatus) -> None:
        self.transfer_page.signals.set_status(status)
        self.receive_page.signals.set_status(status)
        self.diagnostics_page.signals.set_status(status)

    def _record_state(self, text: str) -> None:
        self.diagnostics_page.last_state.setText(f"{datetime.now().strftime('%H:%M:%S')}  {text}")

    # ----------------------------------------------------------- files
    def open_file(self) -> None:
        if self.busy:
            return
        start_dir = self.config.last_open_dir or str(Path.home())
        name, _ = QFileDialog.getOpenFileName(self, "NC-Programm öffnen", start_dir, NC_FILE_FILTER)
        if name:
            self.load_file(Path(name))

    def load_file(self, path: Path) -> None:
        """Load the original bytes (never modified), analyse and prepare transfer data."""
        self.transfer_page.set_state(SendState.VALIDATING)
        self.transfer_page.error_panel.clear()
        try:
            original = path.read_bytes()
        except OSError as exc:
            error = describe_error(exc, operation="Datei laden")
            self.transfer_page.error_panel.show_error(error)
            self.program = None
            self.transfer_page.show_program(None, self.profile)
            self.transfer_page.set_state(SendState.IDLE)
            self._refresh_all()
            return
        self.config.last_open_dir = str(path.parent)
        self.config.add_recent_file(str(path))
        self._refresh_recent_menu()
        self._save_config()
        analysis = analyze(original, self.profile.transfer, self.profile.serial)
        self.program = LoadedProgram(path=path, original=original, analysis=analysis)
        self._retransform()
        log.info(
            "File loaded: %s (%d bytes, %d lines, errors=%d, warnings=%d, sha256=%s)",
            path.name,
            len(original),
            analysis.line_count,
            len(analysis.errors),
            len(analysis.warnings),
            analysis.sha256[:16],
        )
        self.transfer_page.show_program(self.program, self.profile)
        self.transfer_page.reset_progress()
        self.transfer_page.set_state(SendState.IDLE if analysis.has_errors else SendState.READY)
        self._record_state("Datei geprüft")
        self._refresh_all()

    def _retransform(self) -> None:
        program = self.program
        if program is None:
            return
        program.payload = None
        program.transform_error = None
        if program.analysis.has_errors:
            program.transform_error = "Prüffehler verhindern die Aufbereitung"
            return
        try:
            program.payload = transform(
                program.original,
                self.profile.transfer,
                append_program_end_confirmed=self.transfer_page.append_end.isChecked(),
            )
        except (TransformError, UnicodeEncodeError) as exc:
            program.transform_error = str(exc)

    def recheck_file(self) -> None:
        if self.program is not None:
            self.load_file(self.program.path)

    def load_sample(self) -> None:
        scenario = self.scenario()
        stem = sanitize_stem(f"SIM_{scenario.id}")
        path = unique_path(self.paths.simulation, stem, ".nc")
        write_new_file(path, scenario.sample_program)
        self.load_file(path)

    def load_for_send(self, path: Path) -> None:
        self.nav.setCurrentRow(0)
        self.load_file(path)

    def _transfer_options_changed(self) -> None:
        if self.program is not None:
            self._retransform()
            self.transfer_page.show_program(self.program, self.profile, reset_ack=False)
        self._refresh_gates()

    # ----------------------------------------------------------- send
    def start_drip_feed(self) -> None:
        """NC drip-feed (Nachladebetrieb) via LSV2: the CNC requests data block by block."""
        if not self.profile.protocol.is_lsv2:
            self.statusBar().showMessage("Nachladebetrieb erfordert ein Profil mit LSV2-Protokoll", 8000)
            return
        self.start_send(drip_feed=True)

    def start_send(self, drip_feed: bool = False) -> None:
        gate = evaluate_send_gate(
            self.program,
            simulation=self.simulation,
            port=self.connection_target,
            busy=self.busy,
            warnings_acknowledged=self.transfer_page.ack_warnings.isChecked(),
        )
        program = self.program
        if not gate.allowed or program is None or program.payload is None:
            self.statusBar().showMessage(f"Senden nicht möglich: {gate.reason_text}", 8000)
            return
        payload = program.payload
        profile = self.profile
        info = ConfirmInfo(
            simulation=self.simulation,
            file_name=program.path.name,
            file_path=str(program.path),
            transfer_bytes=len(payload.data),
            lines=payload.line_count,
            profile_name=profile.display_name,
            profile_verified=profile.verification_status is VerificationStatus.VERIFIED,
            port=self.port_label,
            serial_summary=profile.serial.summary,
            warnings=tuple(f.message for f in program.analysis.warnings),
            applied=payload.applied,
            show_safety_notice=self.config.show_safety_notice,
            appended_program_end=payload.appended_program_end,
            protocol=profile.protocol.mode.label,
            drip_feed=drip_feed,
        )
        outcome = self.confirm(info, self)
        if not outcome.accepted:
            log.info("Transfer not confirmed by user")
            self.statusBar().showMessage("Übertragung nicht gestartet", 5000)
            return
        if outcome.suppress_safety_notice:
            self.config.show_safety_notice = False
            self.settings_page.load_config(self.config, str(self.paths.root))
            self._save_config()
            log.info("Safety notice disabled by user")
        log.info(
            "Transfer confirmed: file=%s mode=%s drip_feed=%s port=%s profile=%s",
            program.path.name,
            "simulation" if self.simulation else "REAL",
            drip_feed,
            self.port_label,
            profile.id,
        )
        self._close_retained()
        transport = self._make_transport("dripfeed" if drip_feed else "send")
        self._active_transport = transport
        clock = self._clock()
        self.transfer_page.error_panel.clear()
        self.transfer_page.reset_progress()

        program_name = program.path.stem

        def job(cb: TaskCallbacks, token: CancellationToken) -> object:
            callbacks = SendCallbacks(
                on_state=cb.state, on_progress=cb.progress, on_traffic=cb.traffic, on_modem=cb.modem
            )
            if profile.protocol.is_lsv2:
                lsv2_sender = Lsv2ProgramSender(
                    transport,
                    profile,
                    drip_feed=drip_feed,
                    program_name=program_name,
                    clock=clock,
                    token=token,
                    callbacks=callbacks,
                )
                with keep_system_awake(drip_feed):
                    return (lsv2_sender.send(payload), transport)
            sender = ProgramSender(transport, profile, clock=clock, token=token, callbacks=callbacks)
            return (sender.send(payload), transport)

        self._start_task(
            TASK_SEND,
            job,
            finished=self._send_finished,
            state=self._send_state,
            progress=self._send_progress,
        )

    def _send_state(self, state: SendState) -> None:
        self.transfer_page.set_state(state)
        self._busy_text = state.label
        self._record_state(f"Senden: {state.label}")
        if state is SendState.WAITING_FOR_HANDSHAKE:
            self.header.set_status(StatusKind.WARNING, state.label)
        elif state is SendState.DRIP_FEEDING:
            self.header.set_status(StatusKind.ACTIVE, "Nachladebetrieb aktiv - CNC fordert Daten an")
        elif state.is_active:
            self.header.set_status(StatusKind.ACTIVE, state.label)

    def _send_progress(self, progress: TransferProgress) -> None:
        self.transfer_page.set_progress(progress)

    def _send_finished(self, payload: tuple[SendResult, Transport]) -> None:
        result, transport = payload
        self.transfer_page.show_result(result)
        self.last_result = result.to_dict()
        if result.port_left_open and transport.is_open:
            self.retained_transport = transport
        self._last_failed = result.state is SendState.FAILED
        if result.error is not None:
            self.diagnostics_page.last_error.setText(result.error.title)
        messages = {
            SendState.COMPLETED: f"Übertragung abgeschlossen: {result.bytes_sent} Bytes",
            SendState.CANCELLED: f"Übertragung abgebrochen nach {result.bytes_sent} Bytes",
            SendState.FAILED: "Übertragung fehlgeschlagen - siehe Meldung",
        }
        self.statusBar().showMessage(messages.get(result.state, result.state.label), 10000)

    # ----------------------------------------------------------- receive
    def start_receive(self) -> None:
        gate = evaluate_receive_gate(simulation=self.simulation, port=self.connection_target, busy=self.busy)
        if not gate.allowed:
            self.statusBar().showMessage(f"Empfang nicht möglich: {gate.reason_text}", 8000)
            return
        self._close_retained()
        profile = self.profile
        transport = self._make_transport("receive")
        self._active_transport = transport
        clock = self._clock()
        archive = self.archive
        self.receive_page.begin()
        self.nav.setCurrentRow(1)

        def job(cb: TaskCallbacks, token: CancellationToken) -> object:
            callbacks = ReceiveCallbacks(
                on_state=cb.state, on_progress=cb.progress, on_traffic=cb.traffic, on_modem=cb.modem
            )
            if profile.protocol.is_lsv2:
                result = Lsv2ProgramReceiver(
                    transport, profile, clock=clock, token=token, callbacks=callbacks
                ).receive()
            else:
                result = ProgramReceiver(transport, profile, clock=clock, token=token, callbacks=callbacks).receive()
            program: ReceivedProgram | None = None
            stored: StoredReceive | None = None
            store_error: UserFacingError | None = None
            if result.raw:
                program = process_received(result, profile)
                try:
                    stored = archive.store(result, program, profile)
                except OSError as exc:
                    store_error = describe_error(exc, operation="Empfangsdatei speichern")
            return (result, program, stored, store_error)

        self._start_task(
            TASK_RECEIVE,
            job,
            finished=self._receive_finished,
            state=self._receive_state,
            progress=self._receive_progress,
        )

    def _receive_state(self, state: ReceiveState) -> None:
        self.receive_page.set_state(state)
        self._busy_text = state.label
        self._record_state(f"Empfang: {state.label}")
        if state is ReceiveState.LISTENING:
            self.header.set_status(StatusKind.WARNING, state.label)
        elif state.is_active:
            self.header.set_status(StatusKind.ACTIVE, state.label)

    def _receive_progress(self, progress: ReceiveProgress) -> None:
        self.receive_page.set_progress(progress)

    def _receive_finished(
        self,
        payload: tuple[ReceiveResult, ReceivedProgram | None, StoredReceive | None, UserFacingError | None],
    ) -> None:
        result, program, stored, store_error = payload
        self.receive_page.show_result(result, program, stored)
        if store_error is not None:
            self.receive_page.error_panel.show_error(store_error)
        self.last_result = result.to_dict()
        if stored is not None:
            self.last_result["stored_file"] = stored.nc_path.name
            self.last_result["sha256_normalized"] = stored.metadata["sha256_normalized"]
        self.last_stored = stored
        self._last_failed = result.state is ReceiveState.FAILED or store_error is not None
        if result.state is ReceiveState.COMPLETED and program is not None and program.complete:
            self._commissioning_auto("receive", "Empfang erfolgreich")
        if result.error is not None:
            self.diagnostics_page.last_error.setText(result.error.title)
        self.refresh_programs()
        text = result.state.label
        if stored is not None:
            text += f" - gespeichert als {stored.nc_path.name}"
        self.statusBar().showMessage(text, 10000)

    # ----------------------------------------------------------- programs
    # ----------------------------------------------------------- machine parameters
    def refresh_parameter_sets(self) -> None:
        self.parameters_page.set_sets(self.param_archive.list_sets())

    def _refresh_parameter_gate(self) -> None:
        gate = self._parameter_gate()
        self.parameters_page.apply_gate(gate)

    def _parameter_gate(self) -> ParameterWriteGate:
        return evaluate_parameter_write_gate(
            profile=self.profile,
            session_unlocked=self.param_unlocked,
            candidate=self.param_candidate,
            backup=self.param_backup,
            now=datetime.now().astimezone(),
            port_label=self.port_label,
            simulation=self.simulation,
            port=self.connection_target,
            busy=self.busy,
        )

    def set_parameter_write_unlocked(self, enabled: bool, *, confirm: bool = True) -> None:
        if enabled and confirm:
            answer = QMessageBox.warning(
                self,
                "Schreiben von Maschinenparametern freischalten",
                PARAMETER_WRITE_NOTICE + "\n\nSchreibfunktion für diese Sitzung freischalten?",
                QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
                QMessageBox.StandardButton.No,
            )
            if answer != QMessageBox.StandardButton.Yes:
                enabled = False
        self.param_unlocked = enabled
        self.parameters_page.set_unlocked(enabled)
        log.warning("Parameter write %s for this session", "UNLOCKED" if enabled else "locked")
        self._refresh_parameter_gate()

    def start_parameter_read(self) -> None:
        gate = evaluate_receive_gate(simulation=self.simulation, port=self.connection_target, busy=self.busy)
        if not gate.allowed:
            self.statusBar().showMessage(f"Parameter auslesen nicht möglich: {gate.reason_text}", 8000)
            return
        self._close_retained()
        profile = self.profile
        transport = self._make_transport("param_read")
        clock = self._clock()
        archive = self.param_archive
        self.parameters_page.error_panel.clear()
        self.parameters_page.verify_label.setVisible(False)
        self.terminal.reset_counters()
        log.info("Parameter read started (profile=%s, port=%s)", profile.id, self.port_label)

        def job(cb: TaskCallbacks, token: CancellationToken) -> object:
            callbacks = ReceiveCallbacks(on_state=cb.state, on_traffic=cb.traffic, on_modem=cb.modem)
            result = read_parameters(transport, profile, clock=clock, token=token, callbacks=callbacks)
            program: ReceivedProgram | None = None
            stored: StoredParameterSet | None = None
            store_error: UserFacingError | None = None
            if result.raw:
                program = process_received(result, parameter_profile(profile))
                try:
                    stored = archive.store(result, program, profile)
                except OSError as exc:
                    store_error = describe_error(exc, operation="Parameter speichern")
            return (result, program, stored, store_error)

        self._start_task(TASK_PARAM_READ, job, finished=self._parameter_read_finished, state=self._parameter_state)

    def _parameter_state(self, state: object) -> None:
        label = getattr(state, "label", str(state))
        self.parameters_page.state.set_status(StatusKind.ACTIVE, label)
        self._busy_text = f"Parameter: {label}"
        self._record_state(f"Parameter: {label}")

    def _parameter_read_finished(
        self,
        payload: tuple[ReceiveResult, ReceivedProgram | None, StoredParameterSet | None, UserFacingError | None],
    ) -> None:
        result, program, stored, store_error = payload
        page = self.parameters_page
        page.progress.setRange(0, 1000)
        self.last_result = result.to_dict() | {"type": "parameter_read"}
        if result.error is not None:
            page.error_panel.show_error(result.error)
        if store_error is not None:
            page.error_panel.show_error(store_error)
        if stored is None or program is None:
            page.state.set_status(StatusKind.ERROR, f"Keine Parameter empfangen ({result.end_reason.label})")
            self._last_failed = True
        else:
            complete = stored.complete
            page.state.set_status(
                StatusKind.READY if complete else StatusKind.WARNING,
                f"Parameter gesichert: {stored.path.name}" + ("" if complete else " - UNVOLLSTÄNDIG/UNGEPRÜFT"),
            )
            page.progress.setValue(1000)
            self.param_backup = ParameterBackupInfo(
                path_name=stored.path.name,
                data=program.normalized,
                read_at=datetime.now().astimezone(),
                profile_id=self.profile.id,
                port=self.port_label,
                complete=complete,
            )
            page.backup_label.setText(
                f"Aktuelle Sicherung: {stored.path.name}, {len(program.normalized)} Bytes, "
                f"SHA-256 {stored.sha256[:16]}…, ausgelesen {datetime.now():%H:%M:%S}"
                + (f"\n{structure.summary}" if (structure := self.param_backup.structure) else "")
            )
            if self.last_written_parameters is not None:
                check = diff_parameters(self.last_written_parameters, program.normalized, "geschrieben", "ausgelesen")
                page.verify_label.setVisible(True)
                if check.identical:
                    page.verify_label.set_status(
                        StatusKind.READY, "Kontrolle: ausgelesene Parameter stimmen mit der geschriebenen Datei überein"
                    )
                else:
                    page.verify_label.set_status(
                        StatusKind.ERROR, f"Kontrolle: ABWEICHUNG zur geschriebenen Datei - {check.summary}"
                    )
                page.show_diff(check, "geschrieben ↔ ausgelesen")
                log.info("Parameter verification after write: identical=%s", check.identical)
                self.last_written_parameters = None
            elif self.param_candidate is not None:
                self._show_candidate_diff()
        self.refresh_parameter_sets()

    def compare_parameter_set(self, path: Path) -> None:
        if self.param_backup is None:
            self.parameters_page.show_diff(None, "nicht möglich - noch keine aktuelle Sicherung ausgelesen")
            return
        try:
            data = path.read_bytes()
        except OSError as exc:
            self.parameters_page.error_panel.show_error(describe_error(exc, operation="Parameterdatei lesen"))
            return
        diff = diff_parameters(self.param_backup.data, data, self.param_backup.path_name, path.name)
        self.parameters_page.show_diff(diff, f"{self.param_backup.path_name} ↔ {path.name}")

    def select_parameter_file(self, path: Path) -> None:
        try:
            data = path.read_bytes()
        except OSError as exc:
            self.parameters_page.error_panel.show_error(describe_error(exc, operation="Parameterdatei lesen"))
            return
        analysis = analyze_parameters(data, self.profile)
        payload = None
        transform_error = None
        if not analysis.has_errors:
            try:
                payload = prepare_parameter_payload(data, self.profile)
            except TransformError as exc:
                transform_error = str(exc)
        self.param_candidate = ParameterCandidate(path.name, data, analysis, payload, transform_error)
        log.info(
            "Parameter file selected for write: %s (%d bytes, sha256=%s, errors=%d)",
            path.name,
            len(data),
            analysis.sha256[:16],
            len(analysis.errors),
        )
        self.parameters_page.show_candidate(self.param_candidate)
        self._show_candidate_diff()
        self._refresh_parameter_gate()

    def _show_candidate_diff(self) -> None:
        candidate = self.param_candidate
        if candidate is None:
            return
        if self.param_backup is None:
            self.parameters_page.show_diff(None, "zum Schreiben - erst Parameter auslesen (Sicherung)")
            return
        diff = diff_parameters(self.param_backup.data, candidate.data, self.param_backup.path_name, candidate.name)
        self.parameters_page.show_diff(diff, f"Sicherung ↔ {candidate.name}")

    def load_external_parameter_file(self) -> None:
        name, _ = QFileDialog.getOpenFileName(
            self, "Parameterdatei zum Schreiben wählen", str(self.param_archive.directory), PARAMETER_FILE_FILTER
        )
        if name:
            self.select_parameter_file(Path(name))

    def start_parameter_write(self) -> None:
        gate = self._parameter_gate()
        candidate = self.param_candidate
        backup = self.param_backup
        if not gate.allowed or candidate is None or candidate.payload is None or backup is None or gate.diff is None:
            self.statusBar().showMessage(f"Parameter schreiben gesperrt: {gate.reason_text}", 10000)
            return
        info = ParameterWriteInfo(
            simulation=self.simulation,
            port=self.port_label,
            profile_name=self.profile.display_name,
            protocol=self.profile.protocol.mode.label,
            file_name=candidate.name,
            sha256=candidate.analysis.sha256,
            backup_name=backup.path_name,
            backup_age_min=(datetime.now().astimezone() - backup.read_at).total_seconds() / 60,
            diff=gate.diff,
        )
        if not self.confirm_parameter_write(info, self):
            log.info("Parameter write not confirmed")
            self.statusBar().showMessage("Parameter wurden nicht geschrieben", 6000)
            return
        log.warning(
            "PARAMETER WRITE confirmed: file=%s sha256=%s backup=%s changes=%s mode=%s port=%s profile=%s",
            candidate.name,
            candidate.analysis.sha256,
            backup.path_name,
            gate.diff.summary,
            "simulation" if self.simulation else "REAL",
            self.port_label,
            self.profile.id,
        )
        self._close_retained()
        profile = self.profile
        payload = candidate.payload
        written = candidate.data
        transport = self._make_transport("param_write")
        clock = self._clock()
        self.parameters_page.error_panel.clear()
        self.parameters_page.progress.setValue(0)

        def job(cb: TaskCallbacks, token: CancellationToken) -> object:
            callbacks = SendCallbacks(
                on_state=cb.state, on_progress=cb.progress, on_traffic=cb.traffic, on_modem=cb.modem
            )
            return write_parameters(transport, profile, payload, clock=clock, token=token, callbacks=callbacks)

        def finished(result: SendResult) -> None:
            page = self.parameters_page
            self.last_result = result.to_dict() | {"type": "parameter_write"}
            if result.state is SendState.COMPLETED:
                self.last_written_parameters = written
                # The backup no longer reflects the machine state: a new read is required.
                self.param_backup = None
                self.param_candidate = None
                page.show_candidate(None)
                page.backup_label.setText(
                    "Aktuelle Sicherung: keine - Parameter wurden geschrieben. Bitte jetzt erneut auslesen "
                    "und vergleichen (Kontrolle erfolgt automatisch)."
                )
                page.state.set_status(StatusKind.WARNING, "Parameter gesendet - jetzt zur Kontrolle erneut auslesen")
            else:
                self._last_failed = result.state is SendState.FAILED
                page.state.set_status(
                    StatusKind.ERROR if result.state is SendState.FAILED else StatusKind.WARNING, result.state.label
                )
                if result.error is not None:
                    page.error_panel.show_error(result.error)
                    self.diagnostics_page.last_error.setText(result.error.title)
            log.warning("PARAMETER WRITE finished: state=%s bytes=%d", result.state.value, result.bytes_sent)

        self._start_task(
            TASK_PARAM_WRITE,
            job,
            finished=finished,
            state=self._parameter_state,
            progress=self.parameters_page.set_progress,
        )

    def refresh_programs(self) -> None:
        self.programs_page.set_programs(self.archive.list_programs())

    def export_program(self, source: Path) -> None:
        start = str(Path(self.config.last_open_dir or Path.home()) / source.name)
        name, _ = QFileDialog.getSaveFileName(self, "Programm speichern unter", start, NC_FILE_FILTER)
        if not name:
            return
        target = Path(name)
        try:
            backup = export_copy(source, target, self.backups, overwrite=True)
        except Exception as exc:
            error = describe_error(exc, operation="Programm exportieren")
            QMessageBox.warning(self, "Speichern fehlgeschlagen", error.as_text())
            return
        message = f"Gespeichert: {target.name}"
        if backup is not None:
            message += f" (vorherige Datei gesichert als {backup.name})"
        self.statusBar().showMessage(message, 10000)

    # ----------------------------------------------------------- diagnostics
    def toggle_monitor(self, enabled: bool) -> None:
        if not enabled:
            if self.task is not None and self.task.name == TASK_MONITOR:
                self.task.cancel()
            return
        if self.busy:
            self.diagnostics_page.set_port_open(False)
            return
        if not self.simulation and not self.connection_target:
            self.diagnostics_page.set_port_open(False, "Kein COM-Port gewählt")
            return
        self._close_retained()
        transport = self._make_transport("monitor")
        clock = self._clock()
        commands = self._monitor_queue
        while not commands.empty():
            commands.get_nowait()
        self.diagnostics_page.error_panel.clear()

        def job(cb: TaskCallbacks, token: CancellationToken) -> object:
            transport.open()
            cb.state("open")
            try:
                while not token.is_cancelled:
                    while True:
                        try:
                            name, value = commands.get_nowait()
                        except queue.Empty:
                            break
                        if name == "RTS":
                            transport.set_control_lines(rts=value)
                        else:
                            transport.set_control_lines(dtr=value)
                        logging.getLogger(__name__).info("Control line %s set to %s", name, value)
                    cb.modem(transport.get_modem_status())
                    clock.sleep(0.2, token.event)
            finally:
                transport.close()
            return None

        self._start_task(
            TASK_MONITOR,
            job,
            finished=lambda _r: self.diagnostics_page.set_port_open(False),
            state=lambda _s: self.diagnostics_page.set_port_open(True, f"{transport.name} geöffnet"),
        )

    def request_control_line(self, name: str, value: bool) -> None:
        if self.task is None or self.task.name != TASK_MONITOR:
            return
        if name == "RTS" and self.profile.serial.rts_cts:
            log.warning("RTS set manually while RTS/CTS handshake is active - driver controls RTS")
            self.statusBar().showMessage("Hinweis: Bei aktivem RTS/CTS-Handshake steuert der Treiber RTS.", 8000)
        self._monitor_queue.put((name, value))

    def open_loopback(self) -> None:
        if self.loopback_dialog is None:
            dialog = LoopbackDialog(self.port_label, self.profile.serial.data_bits, self)
            dialog.run_requested.connect(self.run_loopback)
            dialog.cancel_requested.connect(self._cancel_loopback)
            self.loopback_dialog = dialog
            dialog.finished.connect(self._loopback_closed)
        self.loopback_dialog.show()
        self.loopback_dialog.raise_()

    def _loopback_closed(self, _code: int) -> None:
        self._cancel_loopback()
        if self.loopback_dialog is not None:
            self.loopback_dialog.deleteLater()
        self.loopback_dialog = None

    def _cancel_loopback(self) -> None:
        if self.task is not None and self.task.name == TASK_LOOPBACK:
            self.task.cancel()

    def run_loopback(self, pattern: str, length: int, timeout_s: float) -> None:
        if self.busy:
            if self.loopback_dialog is not None:
                self.loopback_dialog.set_running(False)
            self.statusBar().showMessage("Loopback nicht möglich: ein anderer Vorgang läuft", 6000)
            return
        if not self.simulation and not self.connection_target:
            if self.loopback_dialog is not None:
                self.loopback_dialog.set_running(False)
            self.statusBar().showMessage("Loopback nicht möglich: kein COM-Port gewählt", 6000)
            return
        self._close_retained()
        transport = self._make_transport("loopback")
        clock = self._clock()
        packets = make_pattern(LoopbackPattern(pattern), length, self.profile.serial.data_bits)

        def job(cb: TaskCallbacks, token: CancellationToken) -> object:
            return run_loopback_test(
                transport, packets, timeout_s=timeout_s, clock=clock, token=token, on_traffic=cb.traffic
            )

        def finished(result: Any) -> None:
            if self.loopback_dialog is not None:
                self.loopback_dialog.show_result(result)
            if result.verdict is LoopbackVerdict.PASSED:
                self._commissioning_auto("loopback", "Loopback bestanden")
            self.last_result = {"type": "loopback", "summary": result.summary()}

        self._start_task(TASK_LOOPBACK, job, finished=finished)

    def export_diagnostics_package(self) -> None:
        nc_file = self.program.path if self.program is not None else None
        raw_file = self.last_stored.raw_path if self.last_stored is not None else None
        dialog = ExportDialog(
            mask_paths=self.config.mask_paths_in_export, nc_file=nc_file, raw_file=raw_file, parent=self
        )
        if dialog.exec() != ExportDialog.DialogCode.Accepted:
            return
        options = dialog.options(self.last_result)
        try:
            target = export_diagnostics(self.paths, self.config, self.profile, options)
        except Exception as exc:
            self.diagnostics_page.error_panel.show_error(describe_error(exc, operation="Diagnosepaket exportieren"))
            return
        self.statusBar().showMessage(f"Diagnosepaket erstellt: {target}", 15000)
        self._open_folder(target.parent)

    # ----------------------------------------------------------- settings
    def set_simulation_mode(self, simulation: bool) -> None:
        if self.busy:
            return
        self._close_retained()
        self.config.simulation_mode = simulation
        self._save_config()
        log.warning("Operating mode changed to %s", "SIMULATION" if simulation else "REAL (serial port)")
        self.loopback_dialog_close()
        self._refresh_all()

    def loopback_dialog_close(self) -> None:
        if self.loopback_dialog is not None:
            self.loopback_dialog.close()

    def set_port(self, port: str) -> None:
        port = port.strip()
        if port == self.config.port:
            return
        self._close_retained()
        self.config.port = port
        self._save_config()
        log.info("Port selected: %s", port or "-")
        self._refresh_all()

    def set_network_connection(self, kind: str, host: str, tcp_port: int) -> None:
        """Connection type (COM / TCP / RFC 2217) and Ethernet adapter address."""
        if self.busy:
            return
        kind_value = ConnectionKind(kind)
        if (kind_value.value, host.strip(), tcp_port) == (
            self.config.connection_kind,
            self.config.network_host,
            self.config.network_port,
        ):
            return
        self._close_retained()
        self.config.connection_kind = kind_value.value
        self.config.network_host = host.strip()
        self.config.network_port = tcp_port
        self._save_config()
        log.info("Connection type %s (%s)", kind_value.value, self.port_label)
        self._refresh_all()

    # ----------------------------------------------------------- format detection
    def open_format_detection(self) -> None:
        if self.format_dialog is None:
            dialog = FormatDetectDialog(self.port_label, self.profile.serial.baud_rate, self)
            dialog.start_requested.connect(self.run_format_detection)
            dialog.cancel_requested.connect(self._stop_format_detection)
            dialog.adopt_requested.connect(self.adopt_detected_format)
            dialog.finished.connect(self._format_dialog_closed)
            self.format_dialog = dialog
        if not self.simulation and self.connection_kind is ConnectionKind.TCP:
            self.statusBar().showMessage("TCP-Rohdaten: Für das Mitlesen den Netzwerkadapter auf 8N1 stellen.", 12000)
        self.format_dialog.show()
        self.format_dialog.raise_()

    def _format_dialog_closed(self, _code: int) -> None:
        self._stop_format_detection()
        if self.format_dialog is not None:
            self.format_dialog.deleteLater()
        self.format_dialog = None

    def _stop_format_detection(self) -> None:
        if self.task is not None and self.task.name == TASK_FORMAT:
            self.task.cancel()

    def run_format_detection(self, duration_s: int) -> None:
        gate = evaluate_receive_gate(simulation=self.simulation, port=self.connection_target, busy=self.busy)
        if not gate.allowed:
            if self.format_dialog is not None:
                self.format_dialog.set_running(False)
            self.statusBar().showMessage(f"Mitlesen nicht möglich: {gate.reason_text}", 8000)
            return
        self._close_retained()
        transport = self._make_transport("format")
        clock = self._clock()
        raw_dir = self.paths.raw
        log.info("Passive format detection started on %s (receive only)", self.port_label)

        def job(cb: TaskCallbacks, token: CancellationToken) -> object:
            data = capture_passively(
                transport,
                duration_s=float(duration_s),
                max_bytes=64_000,
                clock=clock,
                token=token,
                on_traffic=cb.traffic,
                on_progress=cb.progress,
            )
            if data:
                target = unique_path(raw_dir, f"FORMAT_{datetime.now():%Y%m%d_%H%M%S}", ".raw")
                write_new_file(target, data)
            return analyze_capture(data)

        def finished(guess: Any) -> None:
            if self.format_dialog is not None:
                self.format_dialog.show_guess(guess)
            log.info("Format detection result: %s (%s)", guess.frame, guess.confidence)
            if guess.parity is not None:
                self._commissioning_auto("format", f"Ermittelt: {guess.frame} ({guess.confidence})")

        def progress(count: Any) -> None:
            if self.format_dialog is not None:
                self.format_dialog.set_received(int(count))

        self._start_task(TASK_FORMAT, job, finished=finished, progress=progress)

    def adopt_detected_format(self, guess: Any) -> None:
        if not isinstance(guess, FormatGuess) or guess.parity is None or guess.data_bits is None:
            return
        answer = QMessageBox.question(
            self,
            "Datenformat übernehmen",
            f"Eine Kopie des Profils '{self.profile.display_name}' mit Datenformat {guess.frame} anlegen und "
            "aktivieren? Das ermittelte Format ist ein Hinweis und muss durch einen Empfang bestätigt werden.",
        )
        if answer != QMessageBox.StandardButton.Yes:
            return
        serial = replace(
            self.profile.serial, data_bits=guess.data_bits, parity=guess.parity, stop_bits=guess.stop_bits or 1
        )
        copy = replace(
            self.profile,
            id=self.repo.unique_id(f"{self.profile.id}_{guess.frame.lower()}"),
            display_name=f"{self.profile.display_name} ({guess.frame} ermittelt)"[:120],
            serial=serial,
            verification_status=VerificationStatus.UNVERIFIED,
            builtin=False,
        )
        try:
            stored, _ = self.repo.save(copy)
        except (ProfileValidationError, OSError) as exc:
            self.settings_page.profile_error.show_error(describe_error(exc, operation="Profil anlegen"))
            return
        self._activate_profile(stored)
        self.statusBar().showMessage(f"Profil '{stored.display_name}' angelegt und aktiviert", 10000)

    # ----------------------------------------------------------- commissioning assistant
    def open_commissioning(self) -> None:
        if self.commissioning_dialog is not None:
            self.commissioning_dialog.close()
        record = self.commissioning_store.load(self.profile.id, datetime.now().astimezone())
        self.commissioning_record = record
        dialog = CommissioningDialog(record, self.profile, self)
        dialog.changed.connect(self._save_commissioning)
        dialog.action_requested.connect(self._commissioning_action)
        dialog.export_requested.connect(self.export_commissioning_report)
        dialog.mark_verified_requested.connect(self.mark_profile_verified)
        dialog.finished.connect(self._commissioning_closed)
        self.commissioning_dialog = dialog
        dialog.show()

    def _commissioning_closed(self, _code: int) -> None:
        self._save_commissioning()
        if self.commissioning_dialog is not None:
            self.commissioning_dialog.deleteLater()
        self.commissioning_dialog = None
        self.commissioning_record = None

    def _save_commissioning(self) -> None:
        if self.commissioning_record is not None:
            try:
                self.commissioning_store.save(self.commissioning_record)
            except OSError as exc:
                log.error("Saving commissioning record failed: %s", exc)

    def _commissioning_action(self, action: str) -> None:
        pages = {"receive": 1, "send": 0, "parameters": 3}
        if action == "loopback":
            self.open_loopback()
        elif action == "format":
            self.open_format_detection()
        elif action in pages:
            self.nav.setCurrentRow(pages[action])
            self.raise_()
        else:
            self.open_help("COMMISSIONING.md")

    def _commissioning_auto(self, step_id: str, note: str) -> None:
        """Real-machine results mark the matching step as done (never in simulation)."""
        if self.simulation or self.commissioning_dialog is None or self.commissioning_record is None:
            return
        if self.commissioning_record.profile_id != self.profile.id:
            return
        self.commissioning_dialog.set_status(step_id, StepStatus.DONE)
        record = self.commissioning_record.step(step_id)
        if not record.note:
            record.note = f"{note} ({datetime.now():%d.%m.%Y %H:%M})"
            self.commissioning_dialog.note_edits[step_id].setText(record.note)
        self._save_commissioning()

    def export_commissioning_report(self) -> None:
        if self.commissioning_record is None:
            return
        try:
            target = export_report(
                self.commissioning_record, self.profile, self.paths.exports, datetime.now().astimezone()
            )
        except OSError as exc:
            QMessageBox.warning(self, "Export fehlgeschlagen", describe_error(exc, operation="Protokoll").as_text())
            return
        self.statusBar().showMessage(f"Protokoll gespeichert: {target}", 15000)
        self._open_folder(target.parent)

    def mark_profile_verified(self) -> None:
        record = self.commissioning_record
        if record is None or not record.can_mark_verified():
            return
        answer = QMessageBox.question(
            self,
            "Profil als verifiziert markieren",
            f"Profil '{self.profile.display_name}' als an der realen Maschine verifiziert kennzeichnen?\n\n"
            f"Prüfer: {record.tester}\nMitgelieferte Standardprofile bleiben unverändert - es entsteht eine "
            "Kopie.",
        )
        if answer != QMessageBox.StandardButton.Yes:
            return
        stamp = datetime.now().astimezone().strftime("%d.%m.%Y")
        note = f" Verifiziert am {stamp} durch {record.tester}" + (
            f" ({record.machine_info})." if record.machine_info else "."
        )
        verified = replace(
            self.profile,
            verification_status=VerificationStatus.VERIFIED,
            description=(self.profile.description + note).strip(),
        )
        try:
            stored, _ = self.repo.save(verified)
        except (ProfileValidationError, OSError) as exc:
            self.settings_page.profile_error.show_error(describe_error(exc, operation="Profil speichern"))
            return
        record.profile_id = stored.id
        self.commissioning_store.save(record)
        log.warning("Profile %s marked as VERIFIED by %s", stored.id, record.tester)
        self._activate_profile(stored)
        if self.commissioning_dialog is not None:
            self.commissioning_dialog.close()
        self.statusBar().showMessage(f"Profil '{stored.display_name}' ist als verifiziert gekennzeichnet", 15000)

    # ----------------------------------------------------------- profile exchange / replay
    def export_profile(self, profile_id: str) -> None:
        profile = self.repo.get(profile_id)
        if profile is None:
            return
        start = str(Path(self.config.last_open_dir or Path.home()) / f"{profile.id}.json")
        name, _ = QFileDialog.getSaveFileName(self, "Profil exportieren", start, PROFILE_FILE_FILTER)
        if not name:
            return
        try:
            backup = self.repo.export_profile(profile_id, Path(name), overwrite=True)
        except (ProfileRepositoryError, OSError) as exc:
            self.settings_page.profile_error.show_error(describe_error(exc, operation="Profil exportieren"))
            return
        message = f"Profil exportiert: {Path(name).name}"
        if backup is not None:
            message += f" (vorherige Datei gesichert als {backup.name})"
        self.statusBar().showMessage(message, 10000)

    def import_profile(self) -> None:
        name, _ = QFileDialog.getOpenFileName(
            self, "Profil importieren", self.config.last_open_dir or str(Path.home()), PROFILE_FILE_FILTER
        )
        if not name:
            return
        try:
            stored, warnings = self.repo.import_profile(Path(name))
        except (ProfileValidationError, ProfileRepositoryError, OSError) as exc:
            self.settings_page.profile_error.show_error(describe_error(exc, operation="Profil importieren"))
            return
        self.settings_page.profile_error.clear()
        self._activate_profile(stored)
        notes = [w for w in warnings if "importiert als" in w or "verifiziert" in w]
        self.statusBar().showMessage(f"Profil importiert: {stored.display_name}. " + " ".join(notes), 15000)

    def choose_replay_file(self) -> None:
        name, _ = QFileDialog.getOpenFileName(
            self,
            "Aufzeichnung für den Simulator wählen",
            str(self.paths.raw),
            "Rohdaten/NC-Dateien (*.raw *.nc *.par *.txt);;Alle Dateien (*)",
        )
        if not name:
            return
        self.config.simulation_replay_file = name
        self._save_config()
        self.settings_page.set_replay_file(name)
        log.info("Replay file selected: %s", Path(name).name)

    def set_scenario(self, scenario_id: str) -> None:
        self.config.simulation_scenario = scenario_id
        self._save_config()
        log.info("Simulator scenario: %s", scenario_id)
        self._refresh_all()

    def _activate_profile(self, profile: Profile) -> None:
        self._close_retained()
        self.profile = profile
        self.config.active_profile_id = profile.id
        self._save_config()
        self._refresh_profiles()
        if self.program is not None:
            self.program.analysis = analyze(self.program.original, profile.transfer, profile.serial)
            self._retransform()
            self.transfer_page.show_program(self.program, profile)
        self._refresh_all()
        log.info("Active profile: %s", profile.id)

    def select_profile(self, profile_id: str) -> None:
        profile = self.repo.get(profile_id)
        if profile is not None:
            self.settings_page.profile_error.clear()
            self._activate_profile(profile)

    def save_profile(self, data: dict[str, Any]) -> None:
        try:
            candidate, _warnings = parse_profile(data, "Editor")
            stored, _ = self.repo.save(candidate)
        except ProfileValidationError as exc:
            self.settings_page.profile_error.show_error(describe_error(exc, operation="Profil speichern"))
            return
        except OSError as exc:
            self.settings_page.profile_error.show_error(describe_error(exc, operation="Profil speichern"))
            return
        self.settings_page.profile_error.clear()
        if stored.id != candidate.id:
            self.statusBar().showMessage(
                f"Standardprofil bleibt unverändert - Änderungen als '{stored.display_name}' gespeichert",
                10000,
            )
        else:
            self.statusBar().showMessage(f"Profil gespeichert: {stored.display_name}", 8000)
        self._activate_profile(stored)

    def duplicate_profile(self, profile_id: str) -> None:
        try:
            copy, _ = self.repo.duplicate(profile_id)
        except (ProfileRepositoryError, ProfileValidationError, OSError) as exc:
            self.settings_page.profile_error.show_error(describe_error(exc, operation="Profil duplizieren"))
            return
        self._activate_profile(copy)

    def delete_profile(self, profile_id: str) -> None:
        if self.repo.is_builtin(profile_id):
            self.statusBar().showMessage("Standardprofile können nicht gelöscht werden", 6000)
            return
        answer = QMessageBox.question(
            self,
            "Profil löschen",
            f"Profil '{profile_id}' löschen? Eine Sicherung wird im Backup-Ordner abgelegt.",
        )
        if answer != QMessageBox.StandardButton.Yes:
            return
        try:
            self.repo.delete(profile_id)
        except (ProfileRepositoryError, OSError) as exc:
            self.settings_page.profile_error.show_error(describe_error(exc, operation="Profil löschen"))
            return
        self._activate_profile(self.repo.default_profile())

    def reload_profiles(self) -> None:
        self.repo.reload()
        profile, _ = self.repo.resolve(self.profile.id)
        if self.repo.problems:
            details = tuple(f"{p.path.name}: {p.error}" for p in self.repo.problems)
            self.settings_page.profile_error.show_error(
                UserFacingError(
                    "Einige Profile sind ungültig und wurden nicht geladen.",
                    details,
                    "\n".join(details),
                    "Profile laden",
                )
            )
        else:
            self.settings_page.profile_error.clear()
        self._activate_profile(profile)

    def _general_settings_changed(self) -> None:
        old_raw = self.config.raw_traffic_logging
        self.settings_page.apply_general(self.config)
        set_level(self.config.log_level)
        self.terminal.resize(self.config.terminal_history_lines)
        self.terminal_page.apply_history_limit()
        self.backups.retention_days = self.config.backup_retention_days
        if old_raw != self.config.raw_traffic_logging:
            self._apply_raw_logging(self.config.raw_traffic_logging)
        self._save_config()

    def _terminal_file_logging_toggled(self, enabled: bool) -> None:
        if enabled != self.config.raw_traffic_logging:
            self.config.raw_traffic_logging = enabled
            self.settings_page.load_config(self.config, str(self.paths.root))
            self._apply_raw_logging(enabled)
            self._save_config()

    def _apply_raw_logging(self, enabled: bool) -> None:
        if enabled and self.traffic_logger is None:
            logger = TrafficFileLogger(self.paths.logs / f"traffic_{datetime.now():%Y%m%d}.log")
            try:
                logger.open()
            except OSError as exc:
                log.error("Cannot open traffic log: %s", exc)
                return
            self.traffic_logger = logger
        elif not enabled and self.traffic_logger is not None:
            self.traffic_logger.close()
            self.traffic_logger = None
        self.terminal_page.file_log.blockSignals(True)
        self.terminal_page.file_log.setChecked(self.traffic_logger is not None)
        self.terminal_page.file_log.blockSignals(False)

    # ----------------------------------------------------------- shutdown
    def shutdown(self) -> None:
        """Cancel tasks, wait for threads, close ports. Idempotent."""
        handles = ([self.task] if self.task is not None else []) + list(self._finishing)
        for handle in handles:
            handle.cancel()
        for handle in handles:
            if not handle.wait(8000):
                log.error("Background task %s did not stop in time", handle.name)
        self.task = None
        self._close_retained()
        if self.traffic_logger is not None:
            self.traffic_logger.close()
            self.traffic_logger = None
        logging.getLogger().removeHandler(self.log_handler)
        self.param_unlocked = False
        self._save_config()

    def closeEvent(self, event: QCloseEvent) -> None:
        if self.task is not None and self.task.name in (TASK_SEND, TASK_RECEIVE):
            answer = QMessageBox.question(
                self,
                "Übertragung läuft",
                "Eine Übertragung läuft noch. Abbrechen und DeckelLink beenden?",
            )
            if answer != QMessageBox.StandardButton.Yes:
                event.ignore()
                return
        self.shutdown()
        event.accept()
