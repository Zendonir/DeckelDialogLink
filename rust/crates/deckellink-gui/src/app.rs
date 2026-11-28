//! Application state and actions. Drawing code (in `ui`) only records [`Action`]s;
//! they are processed after each frame, which keeps all behaviour testable without a window.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use deckellink_core::appdata::{AppConfig, AppPaths, ArchivedProgram, ReceiveArchive, StoredReceive};
use deckellink_core::clock::{Clock, SystemClock};
use deckellink_core::error::UserFacingError;
use deckellink_core::model::Profile;
use deckellink_core::parameters::archive::{ParameterArchive, StoredParameterSet};
use deckellink_core::parameters::transfer::{read_parameters, write_parameters};
use deckellink_core::parameters::{
    diff_parameters, evaluate_parameter_write_gate, parameter_profile, GateInput, ParameterBackupInfo,
    ParameterCandidate, ParameterDiff, ParameterWriteGate,
};
use deckellink_core::profilestore::ProfileStore;
use deckellink_core::scenarios::get_scenario;
use deckellink_core::storage::{unique_path, write_new_file};
use deckellink_core::transfer::lsv2_transfer::{Lsv2ProgramReceiver, Lsv2ProgramSender};
use deckellink_core::transfer::receiver::{process_received, ProgramReceiver, ReceivedProgram};
use deckellink_core::transfer::sender::ProgramSender;
use deckellink_core::transfer::{
    Direction, Event, ReceiveProgress, ReceiveResult, ReceiveState, SendResult, SendState, TrafficEvent,
    TransferProgress,
};
use deckellink_core::transport::serial::{available_ports, PortInfo};
use deckellink_core::transport::ModemStatus;

use crate::applog::{AppLog, Level, LOG_FILE};
use crate::logic::{
    connection_target, evaluate_receive_gate, evaluate_send_gate, hex_line, port_label, sample_file, Gate,
    LoadedProgram, Purpose,
};
use crate::worker::{Outcome, RunningTask, TaskKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Transfer,
    Receive,
    Programs,
    Parameters,
    Terminal,
    Diagnostics,
    Settings,
    Log,
    Commissioning,
    Help,
}

impl Page {
    pub const ALL: [Page; 10] = [
        Page::Transfer,
        Page::Receive,
        Page::Programs,
        Page::Parameters,
        Page::Terminal,
        Page::Diagnostics,
        Page::Settings,
        Page::Log,
        Page::Commissioning,
        Page::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Page::Transfer => "Übertragen",
            Page::Receive => "Empfangen",
            Page::Programs => "NC-Programme",
            Page::Parameters => "Maschinenparameter",
            Page::Terminal => "Terminal",
            Page::Diagnostics => "Diagnose",
            Page::Settings => "Einstellungen",
            Page::Log => "Protokoll",
            Page::Commissioning => "Inbetriebnahme",
            Page::Help => "Hilfe",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Ready,
    Warning,
    Error,
    Active,
    Idle,
}

/// User intentions recorded by the UI and processed after the frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Navigate(Page),
    OpenFileDialog,
    LoadFile(PathBuf),
    LoadSample,
    Recheck,
    SetAppendEnd(bool),
    RequestSend {
        drip_feed: bool,
    },
    ConfirmSend {
        suppress_safety_notice: bool,
    },
    CloseConfirm,
    Cancel,
    StartReceive,
    RefreshArchive,
    LoadArchived(PathBuf),
    ParamRead,
    ParamSelectFile(PathBuf),
    ParamOpenFileDialog,
    ParamCompare(PathBuf),
    ParamRequestUnlock,
    ParamConfirmUnlock,
    ParamLock,
    ParamRequestWrite,
    ParamConfirmWrite,
    CloseDialog,
    SelectProfile(String),
    ImportProfileDialog,
    ImportProfile(PathBuf),
    RequestRealMode,
    ConfirmRealMode,
    SetSimulation,
    SaveConfig,
    RefreshPorts,
    ClearTerminal,
    EditProfile,
    CancelEdit,
    SaveDraft,
    DuplicateProfile,
    RequestDeleteProfile,
    ConfirmDeleteProfile,
    ExportProfileDialog,
    StartLoopback,
    StartFormatDetect,
    ExportDiagnostics,
    ReplayFileDialog,
    CommSetStatus(String, deckellink_core::commissioning::StepStatus),
    CommSave,
    CommExport,
    CommRequestVerify,
    CommConfirmVerify,
    DroppedFile(PathBuf),
    ToggleHex,
    LogFilter(Level),
    HelpDoc(usize),
    /// Font size step in percent (0 = back to 100 %).
    FontScale(i64),
}

/// Open modal dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    ConfirmSend { drip_feed: bool, drip_ack: bool, suppress: bool },
    ParamUnlock,
    ParamWrite { phrase: String, ack: bool },
    RealMode,
    DeleteProfile,
    MarkVerified,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReceiveSummary {
    pub state: ReceiveState,
    pub end_reason: String,
    pub bytes: usize,
    pub stored_path: Option<PathBuf>,
    pub complete: bool,
    pub notes: Vec<String>,
    pub findings: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ParamState {
    pub unlocked: bool,
    pub backup: Option<ParameterBackupInfo>,
    pub candidate: Option<ParameterCandidate>,
    pub sets: Vec<StoredParameterSet>,
    pub diff: Option<(String, ParameterDiff)>,
    pub last_written: Option<Vec<u8>>,
    pub verify: Option<(bool, String)>,
    pub status: Option<(StatusKind, String)>,
    pub error: Option<UserFacingError>,
    pub progress: Option<TransferProgress>,
    pub received_bytes: usize,
}

pub struct DeckelApp {
    pub ctx: egui::Context,
    pub paths: AppPaths,
    pub config: AppConfig,
    pub store: ProfileStore,
    pub profile: Profile,
    pub page: Page,
    pub log: AppLog,
    pub status: (StatusKind, String),
    pub task: Option<RunningTask>,
    pub actions: Vec<Action>,
    pub dialog: Option<Dialog>,
    pub message: Option<String>,
    // transfer page
    pub program: Option<LoadedProgram>,
    pub ack_warnings: bool,
    pub append_end: bool,
    pub send_state: SendState,
    pub send_progress: Option<TransferProgress>,
    pub send_error: Option<UserFacingError>,
    pub last_send: Option<SendResult>,
    // receive page
    pub receive_state: ReceiveState,
    pub receive_progress: Option<ReceiveProgress>,
    pub receive_summary: Option<ReceiveSummary>,
    pub receive_error: Option<UserFacingError>,
    // other pages
    pub archive: Vec<ArchivedProgram>,
    pub param: ParamState,
    pub traffic: VecDeque<TrafficEvent>,
    pub terminal_hex: bool,
    pub modem: Option<ModemStatus>,
    pub ports: Vec<PortInfo>,
    pub log_filter: Level,
    pub extra: crate::app_extra::ExtraState,
}

impl std::fmt::Debug for DeckelApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeckelApp").field("page", &self.page).field("profile", &self.profile.id).finish_non_exhaustive()
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

impl DeckelApp {
    pub fn new(ctx: egui::Context, paths: AppPaths) -> Self {
        let dir_error = paths.ensure().err();
        let (config, config_warnings) = AppConfig::load(&paths.config);
        let mut log = AppLog::new(Some(paths.logs.join(LOG_FILE)), Level::parse(&config.log_level));
        let store = ProfileStore::load(&paths.profiles, &paths.backups);
        let (profile, fell_back) = store.resolve(&config.active_profile_id);
        log.info(format!(
            "DeckelLink {} (Rust) gestartet - {} - Profil {}",
            env!("CARGO_PKG_VERSION"),
            if config.simulation_mode { "Simulationsmodus" } else { "REALBETRIEB" },
            profile.id
        ));
        if let Some(err) = dir_error {
            log.error(format!("Datenordner {} konnte nicht angelegt werden: {err}", paths.root.display()));
        }
        if fell_back {
            log.warn(format!(
                "Profil {} nicht verfügbar - Standardprofil {} aktiv",
                config.active_profile_id, profile.id
            ));
        }
        for warning in config_warnings.iter().chain(store.problems.iter()) {
            log.warn(warning.clone());
        }
        let mut app = Self {
            ctx,
            archive: Vec::new(),
            param: ParamState::default(),
            paths,
            store,
            profile,
            page: Page::Transfer,
            log,
            status: (StatusKind::Idle, "Bereit".into()),
            task: None,
            actions: Vec::new(),
            dialog: None,
            message: None,
            program: None,
            ack_warnings: false,
            append_end: false,
            send_state: SendState::Idle,
            send_progress: None,
            send_error: None,
            last_send: None,
            receive_state: ReceiveState::Idle,
            receive_progress: None,
            receive_summary: None,
            receive_error: None,
            traffic: VecDeque::new(),
            terminal_hex: false,
            modem: None,
            ports: Vec::new(),
            log_filter: Level::Info,
            extra: crate::app_extra::ExtraState::default(),
            config,
        };
        app.refresh_archive();
        app.refresh_parameter_sets();
        crate::theme::install(&app.ctx);
        app.apply_zoom();
        app.load_commissioning();
        app
    }

    pub fn busy(&self) -> bool {
        self.task.is_some()
    }

    pub fn simulation(&self) -> bool {
        self.config.simulation_mode
    }

    pub fn port_label(&self) -> String {
        port_label(&self.config)
    }

    pub fn target(&self) -> String {
        connection_target(&self.config)
    }

    pub fn send_gate(&self) -> Gate {
        evaluate_send_gate(self.program.as_ref(), self.simulation(), &self.target(), self.busy(), self.ack_warnings)
    }

    pub fn receive_gate(&self) -> Gate {
        evaluate_receive_gate(self.simulation(), &self.target(), self.busy())
    }

    pub fn parameter_gate(&self) -> ParameterWriteGate {
        evaluate_parameter_write_gate(&GateInput {
            profile: &self.profile,
            session_unlocked: self.param.unlocked,
            candidate: self.param.candidate.as_ref(),
            backup: self.param.backup.as_ref(),
            now: SystemClock::default().now(),
            port_label: &self.port_label(),
            simulation: self.simulation(),
            port: &self.target(),
            busy: self.busy(),
        })
    }

    pub fn push(&mut self, action: Action) {
        self.actions.push(action);
    }

    pub(crate) fn set_status(&mut self, kind: StatusKind, text: impl Into<String>) {
        self.status = (kind, text.into());
    }

    pub(crate) fn notify(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.log.info(text.clone());
        self.message = Some(text);
    }

    pub fn apply_zoom(&self) {
        #[allow(clippy::cast_precision_loss)]
        let factor = self.config.font_scale_percent.clamp(80, 200) as f32 / 100.0;
        self.ctx.set_zoom_factor(factor);
    }

    pub fn save_config(&mut self) {
        if let Err(err) = self.config.save(&self.paths.config) {
            self.log.error(format!("Einstellungen konnten nicht gespeichert werden: {err}"));
            self.message = Some(format!("Einstellungen konnten nicht gespeichert werden: {err}"));
        }
    }

    /// Process all recorded actions and poll the running task. Called once per frame.
    pub fn update(&mut self) {
        let actions = std::mem::take(&mut self.actions);
        for action in actions {
            self.handle(action);
        }
        self.poll_task();
        if self.busy() {
            self.ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle(&mut self, action: Action) {
        match action {
            Action::Navigate(page) => {
                if page == Page::Diagnostics && self.ports.is_empty() {
                    self.ports = available_ports();
                }
                self.page = page;
            }
            Action::OpenFileDialog => {
                if self.busy() {
                    return;
                }
                let mut dialog = rfd::FileDialog::new()
                    .set_title("NC-Programm öffnen")
                    .add_filter("NC-Programme", &["nc", "NC", "txt", "TXT", "mpf", "MPF", "din", "DIN"])
                    .add_filter("Alle Dateien", &["*"]);
                if !self.config.last_open_dir.is_empty() {
                    dialog = dialog.set_directory(&self.config.last_open_dir);
                }
                if let Some(path) = dialog.pick_file() {
                    self.load_file(&path);
                }
            }
            Action::LoadFile(path) => self.load_file(&path),
            Action::LoadSample => self.load_sample(),
            Action::Recheck => {
                if let Some(path) = self.program.as_ref().map(|p| p.path.clone()) {
                    self.load_file(&path);
                }
            }
            Action::SetAppendEnd(value) => {
                self.append_end = value;
                let profile = self.profile.clone();
                if let Some(program) = &mut self.program {
                    program.retransform(&profile, value);
                }
            }
            Action::RequestSend { drip_feed } => self.request_send(drip_feed),
            Action::ConfirmSend { suppress_safety_notice } => {
                let drip_feed = matches!(self.dialog, Some(Dialog::ConfirmSend { drip_feed: true, .. }));
                self.dialog = None;
                if suppress_safety_notice {
                    self.config.show_safety_notice = false;
                    self.save_config();
                    self.log.info("Sicherheitshinweis vom Benutzer abgeschaltet");
                }
                self.start_send(drip_feed);
            }
            Action::CloseConfirm | Action::CloseDialog => {
                if matches!(self.dialog, Some(Dialog::ConfirmSend { .. })) {
                    self.log.info("Übertragung nicht bestätigt");
                    self.message = Some("Übertragung nicht gestartet".into());
                }
                self.dialog = None;
            }
            Action::Cancel => {
                if let Some(task) = &self.task {
                    task.cancel();
                    self.log.warn(format!("{} durch Benutzer abgebrochen", task.kind.label()));
                }
            }
            Action::StartReceive => self.start_receive(),
            Action::RefreshArchive => self.refresh_archive(),
            Action::LoadArchived(path) => {
                self.load_file(&path);
                self.page = Page::Transfer;
            }
            Action::ParamRead => self.start_parameter_read(),
            Action::ParamSelectFile(path) => self.select_parameter_file(&path),
            Action::ParamOpenFileDialog => {
                let picked = rfd::FileDialog::new()
                    .set_title("Parameterdatei zum Schreiben wählen")
                    .set_directory(ParameterArchive::new(&self.paths.root).directory)
                    .add_filter("Parameterdateien", &["par", "txt", "TXT", "prm"])
                    .add_filter("Alle Dateien", &["*"])
                    .pick_file();
                if let Some(path) = picked {
                    self.select_parameter_file(&path);
                }
            }
            Action::ParamCompare(path) => self.compare_parameter_set(&path),
            Action::ParamRequestUnlock => self.dialog = Some(Dialog::ParamUnlock),
            Action::ParamConfirmUnlock => {
                self.dialog = None;
                self.param.unlocked = true;
                self.log.warn("Schreiben von Maschinenparametern für diese Sitzung FREIGESCHALTET");
            }
            Action::ParamLock => {
                self.param.unlocked = false;
                self.log.info("Schreiben von Maschinenparametern gesperrt");
            }
            Action::ParamRequestWrite => {
                let gate = self.parameter_gate();
                if gate.allowed {
                    self.dialog = Some(Dialog::ParamWrite { phrase: String::new(), ack: false });
                } else {
                    self.message = Some(format!("Parameter schreiben gesperrt: {}", gate.reason_text()));
                }
            }
            Action::ParamConfirmWrite => {
                let confirmed = matches!(
                    &self.dialog,
                    Some(Dialog::ParamWrite { phrase, ack: true })
                        if phrase.trim() == deckellink_core::parameters::WRITE_CONFIRMATION_PHRASE
                );
                self.dialog = None;
                if confirmed {
                    self.start_parameter_write();
                } else {
                    self.message = Some("Parameter wurden nicht geschrieben".into());
                }
            }
            Action::SelectProfile(id) => self.select_profile(&id),
            Action::ImportProfileDialog => {
                let picked =
                    rfd::FileDialog::new().set_title("Profil importieren").add_filter("Profile", &["json"]).pick_file();
                if let Some(path) = picked {
                    self.import_profile(&path);
                }
            }
            Action::ImportProfile(path) => self.import_profile(&path),
            Action::RequestRealMode => {
                if !self.busy() {
                    self.dialog = Some(Dialog::RealMode);
                }
            }
            Action::ConfirmRealMode => {
                self.dialog = None;
                if !self.busy() {
                    self.config.simulation_mode = false;
                    self.save_config();
                    self.log.warn(format!("REALBETRIEB aktiviert - Anschluss {}", self.port_label()));
                }
            }
            Action::SetSimulation => {
                if !self.busy() {
                    self.config.simulation_mode = true;
                    self.save_config();
                    self.log.info("Simulationsmodus aktiviert");
                }
            }
            Action::SaveConfig => {
                self.save_config();
                self.apply_zoom();
                self.log.min_level = Level::parse(&self.config.log_level);
                // Re-check a loaded file: the connection may have changed the gates, not the data.
            }
            Action::RefreshPorts => self.ports = available_ports(),
            Action::ClearTerminal => self.traffic.clear(),
            Action::ToggleHex => self.terminal_hex = !self.terminal_hex,
            Action::LogFilter(level) => self.log_filter = level,
            Action::HelpDoc(index) => {
                self.extra.help_doc = index.min(crate::ui_extra::HELP_DOCUMENTS.len().saturating_sub(1));
            }
            Action::FontScale(delta) => {
                let current = i64::try_from(self.config.font_scale_percent).unwrap_or(100);
                let value = if delta == 0 { 100 } else { (current + delta).clamp(80, 200) };
                self.config.font_scale_percent = u64::try_from(value).unwrap_or(100);
                self.apply_zoom();
                self.save_config();
                self.message = Some(format!("Schriftgröße {value} %"));
            }
            other => self.handle_extra(other),
        }
    }

    // ------------------------------------------------------------------ transfer

    pub fn load_file(&mut self, path: &Path) {
        if self.busy() {
            return;
        }
        self.send_error = None;
        self.send_progress = None;
        match std::fs::read(path) {
            Ok(original) => {
                self.ack_warnings = false;
                self.append_end = false;
                let program = LoadedProgram::new(path.to_path_buf(), original, &self.profile, false);
                self.log.info(format!(
                    "Datei geladen: {} ({} Bytes, {} Zeilen, Fehler {}, Warnungen {}, SHA-256 {})",
                    program.file_name(),
                    program.original.len(),
                    program.analysis.line_count,
                    program.analysis.errors().count(),
                    program.analysis.warnings().count(),
                    program.analysis.sha256.get(..16).unwrap_or_default()
                ));
                self.send_state = if program.analysis.has_errors() { SendState::Idle } else { SendState::Ready };
                if let Some(parent) = path.parent() {
                    self.config.last_open_dir = parent.to_string_lossy().into_owned();
                }
                self.config.add_recent_file(&path.to_string_lossy());
                self.save_config();
                self.program = Some(program);
            }
            Err(err) => {
                self.program = None;
                self.send_state = SendState::Idle;
                let mut error = UserFacingError::new(format!("Datei konnte nicht gelesen werden: {}", file_name(path)));
                error.causes = vec![
                    "Datei wurde verschoben oder gelöscht.".into(),
                    "Fehlende Zugriffsrechte oder Netzlaufwerk nicht erreichbar.".into(),
                ];
                error.technical_details = err.to_string();
                error.operation = "Datei laden".into();
                self.log.error(format!("Datei laden fehlgeschlagen: {} ({err})", path.display()));
                self.send_error = Some(error);
            }
        }
    }

    fn load_sample(&mut self) {
        let scenario = get_scenario(&self.config.simulation_scenario);
        let path = sample_file(&self.paths.simulation, scenario.id);
        let current = std::fs::read(&path).ok();
        let target = if current.as_deref() == Some(scenario.sample_program) {
            Ok(path)
        } else {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            unique_path(&self.paths.simulation, &stem, ".nc")
                .and_then(|p| write_new_file(&p, scenario.sample_program).map(|()| p))
        };
        match target {
            Ok(path) => self.load_file(&path),
            Err(err) => self.message = Some(format!("Beispielprogramm konnte nicht angelegt werden: {err}")),
        }
    }

    fn request_send(&mut self, drip_feed: bool) {
        let gate = self.send_gate();
        if !gate.allowed {
            self.message = Some(format!("Senden nicht möglich: {}", gate.reason_text()));
            return;
        }
        if drip_feed && !self.profile.protocol.is_lsv2() {
            self.message = Some("Nachladebetrieb erfordert ein Profil mit LSV2-Protokoll.".into());
            return;
        }
        self.dialog = Some(Dialog::ConfirmSend { drip_feed, drip_ack: false, suppress: false });
    }

    fn start_send(&mut self, drip_feed: bool) {
        let gate = self.send_gate();
        let Some(program) = self.program.as_ref() else { return };
        let Some(payload) = program.payload.clone() else { return };
        if !gate.allowed {
            self.message = Some(format!("Senden nicht möglich: {}", gate.reason_text()));
            return;
        }
        let profile = self.profile.clone();
        let name = program.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.log.info(format!(
            "Übertragung bestätigt: Datei {} Modus {} Nachladebetrieb {} Port {} Profil {}",
            program.file_name(),
            if self.simulation() { "Simulation" } else { "REAL" },
            drip_feed,
            self.port_label(),
            profile.id
        ));
        let purpose = if drip_feed { Purpose::DripFeed } else { Purpose::Send };
        let mut transport = self.transport(&profile, purpose);
        self.send_error = None;
        self.send_progress = None;
        let kind = if drip_feed { TaskKind::DripFeed } else { TaskKind::Send };
        self.spawn(kind, move |token, sink| {
            let clock = SystemClock::default();
            let result = if profile.protocol.is_lsv2() {
                let mut sender = Lsv2ProgramSender::new(transport.as_mut(), &profile, &clock, token, sink, drip_feed);
                sender.program_name = name;
                sender.send(&payload)
            } else {
                ProgramSender::new(transport.as_mut(), &profile, &clock, token, sink).send(&payload)
            };
            Outcome::Sent(result)
        });
    }

    fn start_receive(&mut self) {
        let gate = self.receive_gate();
        if !gate.allowed {
            self.message = Some(format!("Empfang nicht möglich: {}", gate.reason_text()));
            return;
        }
        let profile = self.profile.clone();
        let mut transport = self.transport(&profile, Purpose::Receive);
        let archive = ReceiveArchive::new(self.paths.clone());
        self.receive_error = None;
        self.receive_summary = None;
        self.receive_progress = None;
        self.page = Page::Receive;
        self.log.info(format!("Empfang gestartet (Profil {}, Port {})", profile.id, self.port_label()));
        self.spawn(TaskKind::Receive, move |token, sink| {
            let clock = SystemClock::default();
            let result = if profile.protocol.is_lsv2() {
                Lsv2ProgramReceiver::new(transport.as_mut(), &profile, &clock, token, sink).receive()
            } else {
                ProgramReceiver::new(transport.as_mut(), &profile, &clock, token, sink).receive()
            };
            let (program, stored) = if result.raw.is_empty() {
                (None, Ok(None))
            } else {
                let program = process_received(&result, &profile);
                let stored = archive.store(&result, &program, &profile).map_err(|e| e.to_string());
                (Some(program), stored)
            };
            Outcome::Received { result, program, stored }
        });
    }

    pub(crate) fn spawn<F>(&mut self, kind: TaskKind, job: F)
    where
        F: FnOnce(&deckellink_core::clock::CancelToken, &mut dyn deckellink_core::transfer::EventSink) -> Outcome
            + Send
            + 'static,
    {
        match RunningTask::spawn(kind, &self.ctx, job) {
            Ok(task) => {
                self.set_status(StatusKind::Active, format!("{} läuft …", kind.label()));
                self.task = Some(task);
            }
            Err(message) => {
                self.log.error(message.clone());
                self.message = Some(message);
            }
        }
    }

    fn poll_task(&mut self) {
        let Some(task) = &mut self.task else { return };
        let kind = task.kind;
        let (events, done) = task.poll();
        for event in events {
            self.apply_event(kind, event);
        }
        match done {
            None => {}
            Some(Ok(outcome)) => {
                self.task = None;
                self.finish(outcome);
            }
            Some(Err(message)) => {
                self.task = None;
                self.log.error(message.clone());
                self.set_status(StatusKind::Error, message.clone());
                self.message = Some(message);
            }
        }
    }

    fn apply_event(&mut self, kind: TaskKind, event: Event) {
        match event {
            Event::SendState(state) => {
                if matches!(kind, TaskKind::ParamWrite) {
                    self.param.status = Some((StatusKind::Active, state.label().into()));
                } else {
                    self.send_state = state;
                }
                self.set_status(StatusKind::Active, state.label());
            }
            Event::ReceiveState(state) => {
                if matches!(kind, TaskKind::ParamRead) {
                    self.param.status = Some((StatusKind::Active, format!("Parameter: {}", state.label())));
                } else {
                    self.receive_state = state;
                }
                let status = if state == ReceiveState::Listening { StatusKind::Warning } else { StatusKind::Active };
                self.set_status(status, state.label());
            }
            Event::Progress(progress) => {
                if matches!(kind, TaskKind::ParamWrite) {
                    self.param.progress = Some(progress);
                } else {
                    self.send_progress = Some(progress);
                }
            }
            Event::ReceiveProgress(progress) => {
                if matches!(kind, TaskKind::ParamRead) {
                    self.param.received_bytes = progress.bytes_received;
                } else {
                    self.receive_progress = Some(progress);
                }
            }
            Event::Traffic(traffic) => {
                if self.config.raw_traffic_logging {
                    let dir = if traffic.direction == Direction::Tx { "TX" } else { "RX" };
                    self.log.log(Level::Info, format!("Rohdaten {dir}: {}", hex_line(&traffic.data)));
                }
                self.traffic.push_back(traffic);
                let limit = usize::try_from(self.config.terminal_history_lines).unwrap_or(2000).max(100);
                while self.traffic.len() > limit {
                    self.traffic.pop_front();
                }
            }
            Event::Modem(status) => self.modem = Some(status),
        }
    }

    fn finish(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Sent(result) => self.send_finished(result),
            Outcome::Received { result, program, stored } => self.receive_finished(&result, program, stored),
            Outcome::ParamsRead { result, program, stored } => self.parameter_read_finished(&result, program, stored),
            Outcome::ParamsWritten(result) => self.parameter_write_finished(result),
            Outcome::Loopback(result) => self.loopback_finished(result),
            Outcome::Format(result) => self.format_finished(result),
        }
    }

    fn send_finished(&mut self, result: SendResult) {
        self.send_state = result.state;
        match result.state {
            SendState::Completed => {
                self.set_status(StatusKind::Ready, format!("Übertragung abgeschlossen ({} Bytes)", result.bytes_sent));
                self.log.info(format!(
                    "Übertragung abgeschlossen: {} Bytes in {:.1} s, SHA-256 {}",
                    result.bytes_sent,
                    result.duration_s,
                    result.sha256.get(..16).unwrap_or_default()
                ));
            }
            SendState::Cancelled => {
                self.set_status(StatusKind::Warning, "Übertragung abgebrochen");
                self.log.warn(format!("Übertragung abgebrochen nach {} Bytes", result.bytes_sent));
            }
            _ => {
                self.set_status(StatusKind::Error, "Übertragung fehlgeschlagen");
                if let Some(error) = &result.error {
                    self.log.error(format!("Übertragung fehlgeschlagen: {}", error.technical_details));
                }
            }
        }
        self.send_error = result.error.clone();
        self.last_send = Some(result);
    }

    fn receive_finished(
        &mut self,
        result: &ReceiveResult,
        program: Option<ReceivedProgram>,
        stored: Result<Option<StoredReceive>, String>,
    ) {
        self.receive_state = result.state;
        self.receive_error = result.error.clone();
        let mut summary = ReceiveSummary {
            state: result.state,
            end_reason: result.end_reason.label().into(),
            bytes: result.raw.len(),
            stored_path: None,
            complete: false,
            notes: Vec::new(),
            findings: Vec::new(),
        };
        if let Some(program) = &program {
            summary.complete = program.complete;
            summary.notes = program.notes.clone();
            summary.findings =
                program.analysis.findings.iter().map(|f| format!("[{}] {}", f.severity.label(), f.message)).collect();
        }
        match stored {
            Ok(Some(stored)) => {
                self.log.info(format!(
                    "Empfangen und gespeichert: {} ({} Bytes, vollständig: {})",
                    stored.nc_path.display(),
                    result.raw.len(),
                    stored.complete
                ));
                summary.stored_path = Some(stored.nc_path);
            }
            Ok(None) => self.log.warn(format!("Keine Daten empfangen ({})", result.end_reason.label())),
            Err(err) => {
                let mut error = UserFacingError::new("Empfangsdatei konnte nicht gespeichert werden.");
                error.technical_details = err.clone();
                error.operation = "Empfangsdatei speichern".into();
                self.log.error(format!("Empfangsdatei speichern fehlgeschlagen: {err}"));
                self.receive_error = Some(error);
            }
        }
        let kind = if summary.complete {
            StatusKind::Ready
        } else if summary.bytes == 0 || result.state == ReceiveState::Failed {
            StatusKind::Error
        } else {
            StatusKind::Warning
        };
        self.set_status(kind, format!("{} - {}", result.state.label(), result.end_reason.label()));
        if summary.complete {
            self.commissioning_auto("receive", "Empfang erfolgreich");
        }
        self.receive_summary = Some(summary);
        self.refresh_archive();
    }

    pub fn refresh_archive(&mut self) {
        self.archive = ReceiveArchive::new(self.paths.clone()).list_programs();
    }

    // ---------------------------------------------------------------- parameters

    pub fn refresh_parameter_sets(&mut self) {
        self.param.sets = ParameterArchive::new(&self.paths.root).list_sets();
    }

    fn start_parameter_read(&mut self) {
        let gate = self.receive_gate();
        if !gate.allowed {
            self.message = Some(format!("Parameter auslesen nicht möglich: {}", gate.reason_text()));
            return;
        }
        let profile = self.profile.clone();
        let mut transport = self.transport(&profile, Purpose::ParamRead);
        let archive = ParameterArchive::new(&self.paths.root);
        self.param.error = None;
        self.param.verify = if self.param.last_written.is_some() { self.param.verify.take() } else { None };
        self.param.received_bytes = 0;
        self.log.info(format!("Parameter auslesen gestartet (Profil {}, Port {})", profile.id, self.port_label()));
        self.spawn(TaskKind::ParamRead, move |token, sink| {
            let clock = SystemClock::default();
            let result = read_parameters(transport.as_mut(), &profile, &clock, token, sink);
            let (program, stored) = if result.raw.is_empty() {
                (None, Ok(None))
            } else {
                let program = process_received(&result, &parameter_profile(&profile));
                let stored = archive.store(&result, &program, &profile).map_err(|e| e.to_string());
                (Some(program), stored)
            };
            Outcome::ParamsRead { result, program, stored }
        });
    }

    fn parameter_read_finished(
        &mut self,
        result: &ReceiveResult,
        program: Option<ReceivedProgram>,
        stored: Result<Option<StoredParameterSet>, String>,
    ) {
        self.param.error = result.error.clone();
        let stored = match stored {
            Ok(stored) => stored,
            Err(err) => {
                let mut error = UserFacingError::new("Parametersatz konnte nicht gespeichert werden.");
                error.technical_details = err;
                error.operation = "Parameter speichern".into();
                self.param.error = Some(error);
                None
            }
        };
        match (stored, program) {
            (Some(stored), Some(program)) => {
                let complete = stored.complete();
                let name = file_name(&stored.path);
                self.param.status = Some((
                    if complete { StatusKind::Ready } else { StatusKind::Warning },
                    format!("Parameter gesichert: {name}{}", if complete { "" } else { " - UNVOLLSTÄNDIG/UNGEPRÜFT" }),
                ));
                self.set_status(StatusKind::Ready, format!("Parameter gesichert: {name}"));
                self.log.info(format!("Parameter gesichert: {name} ({} Bytes)", program.normalized.len()));
                self.param.backup = Some(ParameterBackupInfo {
                    path_name: name,
                    data: program.normalized.clone(),
                    read_at: SystemClock::default().now(),
                    profile_id: self.profile.id.clone(),
                    port: self.port_label(),
                    complete,
                });
                if let Some(written) = self.param.last_written.take() {
                    let check = diff_parameters(&written, &program.normalized, "geschrieben", "ausgelesen");
                    let identical = check.identical();
                    let text = if identical {
                        "Kontrolle: ausgelesene Parameter stimmen mit der geschriebenen Datei überein".to_string()
                    } else {
                        format!("Kontrolle: ABWEICHUNG zur geschriebenen Datei - {}", check.summary())
                    };
                    self.log.log(if identical { Level::Info } else { Level::Error }, text.clone());
                    self.param.verify = Some((identical, text));
                    self.param.diff = Some(("geschrieben <-> ausgelesen".into(), check));
                } else {
                    self.show_candidate_diff();
                }
            }
            _ => {
                self.param.status =
                    Some((StatusKind::Error, format!("Keine Parameter empfangen ({})", result.end_reason.label())));
                self.set_status(StatusKind::Error, "Keine Parameter empfangen");
            }
        }
        self.refresh_parameter_sets();
    }

    fn select_parameter_file(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(data) => {
                let candidate = ParameterCandidate::new(file_name(path), data, &self.profile);
                self.log.info(format!(
                    "Parameterdatei zum Schreiben gewählt: {} ({} Bytes, SHA-256 {}, Fehler {})",
                    candidate.name,
                    candidate.data.len(),
                    candidate.analysis.sha256.get(..16).unwrap_or_default(),
                    candidate.analysis.errors().count()
                ));
                self.param.candidate = Some(candidate);
                self.show_candidate_diff();
            }
            Err(err) => {
                let mut error =
                    UserFacingError::new(format!("Parameterdatei konnte nicht gelesen werden: {}", file_name(path)));
                error.technical_details = err.to_string();
                error.operation = "Parameterdatei lesen".into();
                self.param.error = Some(error);
            }
        }
    }

    fn show_candidate_diff(&mut self) {
        let (Some(candidate), Some(backup)) = (&self.param.candidate, &self.param.backup) else {
            return;
        };
        let diff = diff_parameters(&backup.data, &candidate.data, &backup.path_name, &candidate.name);
        self.param.diff = Some((format!("Sicherung <-> {}", candidate.name), diff));
    }

    fn compare_parameter_set(&mut self, path: &Path) {
        let Some(backup) = &self.param.backup else {
            self.message = Some("Vergleich nicht möglich - noch keine aktuelle Sicherung ausgelesen".into());
            return;
        };
        match std::fs::read(path) {
            Ok(data) => {
                let name = file_name(path);
                let diff = diff_parameters(&backup.data, &data, &backup.path_name, &name);
                self.param.diff = Some((format!("{} <-> {name}", backup.path_name), diff));
            }
            Err(err) => self.message = Some(format!("Parameterdatei konnte nicht gelesen werden: {err}")),
        }
    }

    fn start_parameter_write(&mut self) {
        let gate = self.parameter_gate();
        let (Some(candidate), Some(backup), Some(diff)) = (&self.param.candidate, &self.param.backup, &gate.diff)
        else {
            self.message = Some(format!("Parameter schreiben gesperrt: {}", gate.reason_text()));
            return;
        };
        let Some(payload) = candidate.payload.clone() else { return };
        if !gate.allowed {
            self.message = Some(format!("Parameter schreiben gesperrt: {}", gate.reason_text()));
            return;
        }
        self.log.warn(format!(
            "PARAMETER SCHREIBEN bestätigt: Datei {} SHA-256 {} Sicherung {} Änderungen {} Modus {} Port {} Profil {}",
            candidate.name,
            candidate.analysis.sha256,
            backup.path_name,
            diff.summary(),
            if self.simulation() { "Simulation" } else { "REAL" },
            self.port_label(),
            self.profile.id
        ));
        let written = candidate.data.clone();
        let profile = self.profile.clone();
        let mut transport = self.transport(&profile, Purpose::ParamWrite);
        self.param.error = None;
        self.param.progress = None;
        self.param.pending_written(written);
        self.spawn(TaskKind::ParamWrite, move |token, sink| {
            let clock = SystemClock::default();
            Outcome::ParamsWritten(write_parameters(transport.as_mut(), &profile, &payload, &clock, token, sink))
        });
    }

    fn parameter_write_finished(&mut self, result: SendResult) {
        if result.state == SendState::Completed {
            // The backup no longer reflects the machine state: a new read is required.
            self.param.backup = None;
            self.param.candidate = None;
            self.param.diff = None;
            self.param.verify = None;
            self.param.status = Some((
                StatusKind::Warning,
                "Parameter gesendet - jetzt zur Kontrolle erneut auslesen (Vergleich erfolgt automatisch)".into(),
            ));
            self.set_status(StatusKind::Warning, "Parameter gesendet - zur Kontrolle erneut auslesen");
            self.log.warn("Parameter gesendet - Kontrolle durch erneutes Auslesen erforderlich");
        } else {
            self.param.last_written = None;
            let kind = if result.state == SendState::Failed { StatusKind::Error } else { StatusKind::Warning };
            self.param.status = Some((kind, result.state.label().into()));
            self.set_status(kind, result.state.label());
            self.param.error = result.error;
        }
    }

    // ------------------------------------------------------------------ settings

    pub fn select_profile(&mut self, id: &str) {
        if self.busy() {
            return;
        }
        let Some(profile) = self.store.get(id).cloned() else { return };
        self.profile = profile;
        self.config.active_profile_id = id.to_string();
        self.save_config();
        self.log.info(format!("Profil gewählt: {id}"));
        // Re-analyse with the new profile; the original file stays untouched.
        if let Some(path) = self.program.as_ref().map(|p| p.path.clone()) {
            self.load_file(&path);
        }
        if let Some(candidate) = self.param.candidate.take() {
            self.param.candidate = Some(ParameterCandidate::new(candidate.name, candidate.data, &self.profile));
        }
        self.extra.draft = None;
        self.load_commissioning();
    }

    pub fn import_profile(&mut self, path: &Path) {
        match self.store.import_profile(path, &SystemClock::default().now()) {
            Ok((profile, warnings)) => {
                self.notify(format!("Profil '{}' importiert (ID {})", profile.display_name, profile.id));
                for warning in warnings {
                    self.log.warn(format!("Profil {}: {warning}", profile.id));
                }
            }
            Err(err) => self.message = Some(format!("Profil nicht importiert: {err}")),
        }
    }
}

impl ParamState {
    fn pending_written(&mut self, data: Vec<u8>) {
        self.last_written = Some(data);
        self.verify = None;
    }
}
