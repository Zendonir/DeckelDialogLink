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
from PySide6.QtGui import QCloseEvent, QDesktopServices, QKeySequence, QShortcut
from PySide6.QtWidgets import (
    QFileDialog,
    QHBoxLayout,
    QListWidget,
    QMainWindow,
    QMessageBox,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from deckellink.clock import Clock, ScaledClock, SystemClock
from deckellink.config import AppConfig, save_config
from deckellink.diagnostics.error_messages import describe_error
from deckellink.diagnostics.export import export_diagnostics
from deckellink.diagnostics.loopback_test import LoopbackPattern, make_pattern, run_loopback_test
from deckellink.diagnostics.terminal import TerminalBuffer, TrafficFileLogger
from deckellink.domain.errors import ProfileValidationError, UserFacingError
from deckellink.domain.models import Profile, VerificationStatus
from deckellink.domain.results import (
    ReceivedProgram,
    ReceiveProgress,
    ReceiveResult,
    SendResult,
    TrafficEvent,
    TransferProgress,
)
from deckellink.domain.states import ReceiveState, SendState
from deckellink.gui.dialogs.confirm_transfer import ConfirmInfo, ConfirmOutcome, ask_confirmation
from deckellink.gui.dialogs.export_dialog import ExportDialog
from deckellink.gui.dialogs.loopback_dialog import LoopbackDialog
from deckellink.gui.pages.diagnostics_page import DiagnosticsPage
from deckellink.gui.pages.log_page import LogPage, QtLogHandler
from deckellink.gui.pages.programs_page import ProgramsPage
from deckellink.gui.pages.receive_page import ReceivePage
from deckellink.gui.pages.settings_page import SettingsPage
from deckellink.gui.pages.terminal_page import TerminalPage
from deckellink.gui.pages.transfer_page import TransferPage
from deckellink.gui.style import StatusKind
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
from deckellink.power import keep_system_awake
from deckellink.profiles.loader import builtin_profiles_dir
from deckellink.profiles.repository import ProfileRepository, ProfileRepositoryError
from deckellink.profiles.schema import parse_profile
from deckellink.simulation.lsv2_cnc import Lsv2SimulatedCnc
from deckellink.simulation.scenarios import SAMPLE_PROGRAM, Scenario, get_scenario, lsv2_config_for
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
from deckellink.transport.serial_transport import SerialTransport, available_ports
from deckellink.transport.simulated_transport import SIMULATED_PORT_NAME, SimulatedTransport
from deckellink.version import APP_NAME, __version__

log = logging.getLogger(__name__)

NAV_ITEMS = (
    "Übertragen",
    "Empfangen",
    "NC-Programme",
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
        self.loopback_dialog: LoopbackDialog | None = None

        self.setWindowTitle(f"{APP_NAME} {__version__} - DECKEL FP2A / DIALOG 11")
        self.resize(1280, 860)
        self._build_ui()

        self.log_handler = QtLogHandler()
        self.log_handler.emitter.record.connect(self.log_page.add_record)
        logging.getLogger().addHandler(self.log_handler)

        self.settings_page.load_config(config, str(paths.root))
        self._refresh_ports()
        self._refresh_profiles()
        self._apply_raw_logging(config.raw_traffic_logging)
        self.refresh_programs()
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
        self.nav.setFixedWidth(190)
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
        self.terminal_page = TerminalPage(self.terminal)
        self.diagnostics_page = DiagnosticsPage()
        self.settings_page = SettingsPage()
        self.log_page = LogPage()
        for page in (
            self.transfer_page,
            self.receive_page,
            self.programs_page,
            self.terminal_page,
            self.diagnostics_page,
            self.settings_page,
            self.log_page,
        ):
            self.stack.addWidget(page)
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

        self.terminal_page.file_logging_toggled.connect(self._terminal_file_logging_toggled)

        dp = self.diagnostics_page
        dp.monitor_toggled.connect(self.toggle_monitor)
        dp.control_line_requested.connect(self.request_control_line)
        dp.loopback_requested.connect(self.open_loopback)
        dp.export_requested.connect(self.export_diagnostics_package)

        sp = self.settings_page
        sp.simulation_toggled.connect(self.set_simulation_mode)
        sp.port_selected.connect(self.set_port)
        sp.refresh_ports_requested.connect(self._refresh_ports)
        sp.scenario_selected.connect(self.set_scenario)
        sp.profile_selected.connect(self.select_profile)
        sp.profile_save_requested.connect(self.save_profile)
        sp.profile_duplicate_requested.connect(self.duplicate_profile)
        sp.profile_delete_requested.connect(self.delete_profile)
        sp.profiles_reload_requested.connect(self.reload_profiles)
        sp.general_changed.connect(self._general_settings_changed)

        self.log_page.open_folder_requested.connect(lambda: self._open_folder(self.paths.logs))

        QShortcut(QKeySequence("Ctrl+O"), self, self.open_file)
        QShortcut(QKeySequence(Qt.Key.Key_Escape), self, self.cancel_task)
        for index in range(len(NAV_ITEMS)):
            QShortcut(QKeySequence(f"Ctrl+{index + 1}"), self, lambda i=index: self.nav.setCurrentRow(i))

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
        return self.config.port or "(kein Port)"

    def scenario(self) -> Scenario:
        return get_scenario(self.config.simulation_scenario)

    def _clock(self) -> Clock:
        return ScaledClock(self.sim_time_scale) if self.simulation else SystemClock()

    def _make_transport(self, purpose: str) -> Transport:
        serial = self.profile.serial
        if self.simulation:
            if purpose == "loopback":
                return LoopbackTransport(serial, self._clock())
            if self.profile.protocol.is_lsv2 and purpose in ("send", "receive", "dripfeed"):
                lsv2_config = lsv2_config_for(
                    self.scenario(), receive=purpose == "receive", drip_feed=purpose == "dripfeed"
                )
                return Lsv2SimulatedCnc(serial, self.profile.protocol, lsv2_config, self._clock())
            config = self.scenario().config
            if purpose == "receive" and not config.machine_program:
                config = replace(config, machine_program=SAMPLE_PROGRAM)
            return SimulatedTransport(serial, config, self._clock())
        return SerialTransport(self.config.port, serial)

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
        self.header.set_mode(self.simulation, self.config.port)
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
            port=self.config.port,
            busy=self.busy,
            warnings_acknowledged=self.transfer_page.ack_warnings.isChecked(),
        )
        self.transfer_page.apply_gate(
            gate, busy=self.busy, simulation=self.simulation, lsv2=self.profile.protocol.is_lsv2
        )
        self.receive_page.apply_gate(
            evaluate_receive_gate(simulation=self.simulation, port=self.config.port, busy=self.busy),
            busy=self.busy,
        )
        monitoring = self.task is not None and self.task.name == TASK_MONITOR
        self.diagnostics_page.set_busy(self.busy, monitoring)
        self.settings_page.setEnabled(not self.busy)

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
        elif self.config.port:
            self.header.set_status(StatusKind.READY, f"Bereit - {self.config.port}")
        else:
            self.header.set_status(StatusKind.IDLE, "Getrennt - kein COM-Port gewählt")

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
            port=self.config.port,
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
        gate = evaluate_receive_gate(simulation=self.simulation, port=self.config.port, busy=self.busy)
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
        if result.error is not None:
            self.diagnostics_page.last_error.setText(result.error.title)
        self.refresh_programs()
        text = result.state.label
        if stored is not None:
            text += f" - gespeichert als {stored.nc_path.name}"
        self.statusBar().showMessage(text, 10000)

    # ----------------------------------------------------------- programs
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
        if not self.simulation and not self.config.port:
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
        if not self.simulation and not self.config.port:
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
