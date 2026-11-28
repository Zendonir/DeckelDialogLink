//! State of the DIALOG 11 simulator page (offline; no port, no transport).
//!
//! Behaviour follows the Python simulator window: main mode keys 1/2/3, auxiliary
//! modes MODE/INFO/HELP/EDIT/GRAF/DIA, menu softkeys F1-F8 from the menu tree, test run
//! (sequential/single block) with time playback, manual jog in mode 1 (simulator only)
//! and program editing in mode 3. The simulation runs in a worker thread.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use deckellink_core::clock::{Clock, SystemClock};
use deckellink_core::sim::geometry::{axis, set_axis, Vec3};
use deckellink_core::sim::menus::{self, MenuItem, BACK, NOT_SIMULATED};
use deckellink_core::sim::model::{SimConfig, SimResult, ToolData};
use deckellink_core::sim::playback::{format_duration, Frame, Playback};
use deckellink_core::sim::reference::DEMO_PROGRAM;

pub const SPEEDS: [(&str, f64); 7] =
    [("Echtzeit 1x", 1.0), ("2x", 2.0), ("5x", 5.0), ("10x", 10.0), ("50x", 50.0), ("200x", 200.0), ("1000x", 1000.0)];
pub const DEFAULT_SPEED: usize = 3;
pub const JOG_STEPS: [f64; 5] = [0.01, 0.1, 1.0, 10.0, 100.0];
pub const DEFAULT_JOG: usize = 3;
pub const MODE_NAMES: [&str; 3] = ["1 HANDFAHREN", "2 AUTOMATIK", "3 PROGRAMM"];
pub const AUX_CODES: [&str; 6] = ["MODE", "INFO", "HELP", "EDIT", "GRAF", "DIA"];
/// Delay after the last edit before the program is simulated again (seconds).
pub const EDIT_DELAY_S: f64 = 0.7;
/// Stack of the simulation thread (nested subprograms and parameter calls recurse).
const SIM_STACK_BYTES: usize = 32 * 1024 * 1024;

/// Right-hand display of the simulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimTab {
    Display,
    Messages,
    Setup,
    Reference,
    Notes,
    Screen,
}

impl SimTab {
    pub const ALL: [SimTab; 6] =
        [SimTab::Display, SimTab::Messages, SimTab::Setup, SimTab::Reference, SimTab::Notes, SimTab::Screen];

    pub fn label(self) -> &'static str {
        match self {
            SimTab::Display => "ANZEIGE",
            SimTab::Messages => "MELDUNGEN",
            SimTab::Setup => "WZ / NULLP.",
            SimTab::Reference => "INFO G/M",
            SimTab::Notes => "NOTIZEN",
            SimTab::Screen => "BILDSCHIRM",
        }
    }
}

/// Projection of the graphic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Xy,
    Xz,
    Yz,
    Iso,
}

impl ViewMode {
    pub const ALL: [ViewMode; 4] = [ViewMode::Xy, ViewMode::Xz, ViewMode::Yz, ViewMode::Iso];

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Xy => "Draufsicht XY",
            ViewMode::Xz => "Vorderansicht XZ",
            ViewMode::Yz => "Seitenansicht YZ",
            ViewMode::Iso => "3D-Ansicht",
        }
    }

    pub fn next(self) -> ViewMode {
        match self {
            ViewMode::Xy => ViewMode::Xz,
            ViewMode::Xz => ViewMode::Yz,
            ViewMode::Yz => ViewMode::Iso,
            ViewMode::Iso => ViewMode::Xy,
        }
    }
}

/// View parameters of the graphic.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub mode: ViewMode,
    pub yaw: f64,
    pub pitch: f64,
    pub zoom: f64,
    pub pan: (f32, f32),
}

impl Default for View {
    fn default() -> Self {
        Self { mode: ViewMode::Iso, yaw: -35.0, pitch: 30.0, zoom: 1.0, pan: (0.0, 0.0) }
    }
}

impl View {
    /// Model coordinates -> 2D view coordinates (y up, model units).
    pub fn project(&self, p: Vec3) -> (f64, f64) {
        let [x, y, z] = p;
        match self.mode {
            ViewMode::Xy => (x, y),
            ViewMode::Xz => (x, z),
            ViewMode::Yz => (y, z),
            ViewMode::Iso => {
                let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
                let rx = x * yaw.cos() - y * yaw.sin();
                let ry = x * yaw.sin() + y * yaw.cos();
                (rx, z * pitch.cos() + ry * pitch.sin())
            }
        }
    }

    pub fn fit(&mut self) {
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
    }
}

/// Modal input of the simulator page.
#[derive(Debug, Clone, PartialEq)]
pub enum SimDialog {
    Goto { text: String },
    Dia { kind: &'static str, values: Vec<String> },
    Find { text: String },
    Replace { old: String, new: String },
    CuttingData { diameter: f64, vc: f64, fz: f64, flutes: u32 },
}

/// Simulator operations recorded by the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum SimAction {
    SetMode(usize),
    Aux(&'static str),
    Back,
    Softkey(usize),
    Start,
    Stop,
    SingleBlock,
    Rewind,
    ToEnd,
    /// Jump to a fraction 0..1 of the run time.
    Seek(f64),
    Speed(usize),
    JogStep(usize),
    Tab(SimTab),
    View(ViewMode),
    ToggleRapid,
    Fit,
    LoadDemo,
    OpenFile,
    SaveToLibrary,
    SaveAs,
    Resimulate,
    /// Clicked line of the listing (1-based).
    SelectLine(usize),
    DialogOk,
    CloseDialog,
}

/// Request the page cannot fulfil itself (system dialogs belong to the app).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimRequest {
    OpenFile,
    SaveAs,
}

/// Editable tool row (number, radius, length) as text.
pub type ToolRow = [String; 3];
/// Editable zero point row (n, X, Y, Z) as text.
pub type ZeroRow = [String; 4];

#[derive(Debug)]
struct Job {
    receiver: Receiver<SimResult>,
    start: Vec3,
}

pub struct SimPage {
    pub text: String,
    pub source_name: String,
    pub mode: usize,
    pub menu_stack: Vec<&'static MenuItem>,
    pub single_block: bool,
    pub realtime: bool,
    pub graphics_on: bool,
    pub contour_on: bool,
    pub dripfeed_on: bool,
    pub show_tool: bool,
    pub show_rapid: bool,
    pub preset_offset: Vec3,
    pub manual_position: Vec3,
    pub manual_trace: Vec<Vec3>,
    pub playback: Option<Playback>,
    /// Increases with every new result (cache key of the graphic).
    pub generation: u64,
    pub time: f64,
    pub running: bool,
    pub stop_at: Option<f64>,
    pub speed: usize,
    pub jog: usize,
    pub tab: SimTab,
    pub screen_text: String,
    pub notes: String,
    pub tools: Vec<ToolRow>,
    pub zeros: Vec<ZeroRow>,
    pub parameter_text: String,
    pub rapid_feed: f64,
    pub start: Vec3,
    pub view: View,
    pub dialog: Option<SimDialog>,
    pub status: String,
    /// Line of the editor cursor (1-based, mode 3).
    pub cursor_line: usize,
    /// Char index the editor cursor should jump to (set by EDIT/section functions).
    pub pending_cursor: Option<(usize, usize)>,
    pub clipboard: Vec<String>,
    pub busy: bool,
    /// Library entry the text belongs to ("In Programmverwaltung speichern").
    pub library_entry: Option<String>,
    /// File whose transfer can be followed ("Übertragung mitverfolgen", display only).
    pub follow_path: Option<PathBuf>,
    pub follow_enabled: bool,
    /// Last original line already transmitted.
    pub follow_line: Option<usize>,
    clock: Box<dyn Clock>,
    last_tick: Option<f64>,
    dirty_since: Option<f64>,
    job: Option<Job>,
    queued: bool,
}

impl std::fmt::Debug for SimPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimPage").field("mode", &self.mode).field("time", &self.time).finish_non_exhaustive()
    }
}

impl Default for SimPage {
    fn default() -> Self {
        Self::new(Box::new(SystemClock::default()))
    }
}

fn parse_number(text: &str) -> Option<f64> {
    let text = text.trim().replace(',', ".");
    if text.is_empty() {
        return Some(0.0);
    }
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `S = vc·1000 / (π·D)`, `F = S·fz·z`.
pub fn cutting_data(diameter: f64, vc: f64, fz: f64, flutes: u32) -> (f64, f64) {
    if diameter <= 0.0 {
        return (0.0, 0.0);
    }
    let speed = vc * 1000.0 / (std::f64::consts::PI * diameter);
    (speed, speed * fz * f64::from(flutes))
}

/// Byte offset of the start of `line` (1-based) in `text`, or the text length.
fn line_start(text: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    text.match_indices('\n').nth(line - 2).map_or(text.len(), |(i, _)| i + 1)
}

/// Char index for a byte offset.
pub fn char_index(text: &str, byte: usize) -> usize {
    text.get(..byte).map_or(0, |t| t.chars().count())
}

impl SimPage {
    pub fn new(clock: Box<dyn Clock>) -> Self {
        let mut page = Self {
            text: DEMO_PROGRAM.to_string(),
            source_name: "Demoprogramm".into(),
            mode: 2,
            menu_stack: Vec::new(),
            single_block: false,
            realtime: false,
            graphics_on: true,
            contour_on: false,
            dripfeed_on: false,
            show_tool: true,
            show_rapid: true,
            preset_offset: [0.0; 3],
            manual_position: [0.0, 0.0, 100.0],
            manual_trace: Vec::new(),
            playback: None,
            generation: 0,
            time: 0.0,
            running: false,
            stop_at: None,
            speed: DEFAULT_SPEED,
            jog: DEFAULT_JOG,
            tab: SimTab::Display,
            screen_text: String::new(),
            notes: String::new(),
            tools: (1..=8)
                .map(|n| [n.to_string(), "5".into(), "0".into()])
                .chain((0..4).map(|_| Default::default()))
                .collect(),
            zeros: (1..=5).map(|n| [n.to_string(), "0".into(), "0".into(), "0".into()]).collect(),
            parameter_text: String::new(),
            rapid_feed: 5000.0,
            start: [0.0, 0.0, 100.0],
            view: View::default(),
            dialog: None,
            status: "Bereit".into(),
            cursor_line: 1,
            pending_cursor: None,
            clipboard: Vec::new(),
            busy: false,
            library_entry: None,
            follow_path: None,
            follow_enabled: false,
            follow_line: None,
            clock,
            last_tick: None,
            dirty_since: None,
            job: None,
            queued: false,
        };
        page.set_mode(2);
        page
    }

    pub fn result(&self) -> Option<&SimResult> {
        self.playback.as_ref().map(|p| &p.result)
    }

    pub fn menu(&self) -> Option<&'static MenuItem> {
        self.menu_stack.last().copied()
    }

    // ------------------------------------------------------------------ configuration
    pub fn config(&self) -> SimConfig {
        let mut config = SimConfig { rapid_feed: self.rapid_feed, start_position: self.start, ..SimConfig::default() };
        for [number, radius, length] in &self.tools {
            let (Ok(n), Some(r), Some(l)) = (number.trim().parse::<i64>(), parse_number(radius), parse_number(length))
            else {
                continue;
            };
            if radius.trim().is_empty() {
                continue;
            }
            config.tools.insert(n, ToolData { radius: r, length: l, name: String::new() });
        }
        for [number, x, y, z] in &self.zeros {
            if let (Ok(n), Some(x), Some(y), Some(z)) =
                (number.trim().parse::<i64>(), parse_number(x), parse_number(y), parse_number(z))
            {
                config.zero_points.insert(n, [x, y, z]);
            }
        }
        for line in self.parameter_text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_uppercase();
                if key.starts_with('P') {
                    config.parameter_table.insert(key, value.trim().to_string());
                }
            }
        }
        config
    }

    // ------------------------------------------------------------------ simulation thread
    /// Simulate the current text again (in a worker thread; at most one at a time).
    pub fn resimulate(&mut self) {
        self.dirty_since = None;
        if self.job.is_some() {
            self.queued = true;
            return;
        }
        let config = self.config();
        let text = self.text.clone();
        let start = config.start_position;
        let (sender, receiver) = channel();
        let spawned =
            std::thread::Builder::new().name("dialog11-sim".into()).stack_size(SIM_STACK_BYTES).spawn(move || {
                let result = deckellink_core::sim::simulate(&text, &config);
                // The page may be gone; nothing to do then.
                let _ignored = sender.send(result);
            });
        match spawned {
            Ok(_) => {
                self.job = Some(Job { receiver, start });
                self.busy = true;
                self.status = "Simulation läuft …".into();
            }
            Err(err) => self.status = format!("Simulation konnte nicht gestartet werden: {err}"),
        }
    }

    /// Simulate synchronously (tests and small programs).
    pub fn simulate_now(&mut self) {
        let config = self.config();
        let result = deckellink_core::sim::simulate(&self.text, &config);
        self.apply_result(result, config.start_position);
    }

    fn apply_result(&mut self, result: SimResult, start: Vec3) {
        let errors = result.errors().len();
        let warnings = result.warnings().len();
        self.status =
            format!("Simulation fertig: {} Bewegungen, {errors} Fehler, {warnings} Warnungen", result.moves.len());
        let playback = Playback::new(result, start);
        self.time = if self.mode == 2 { self.time.min(playback.total) } else { 0.0 };
        self.playback = Some(playback);
        self.generation = self.generation.wrapping_add(1);
    }

    /// Poll the simulation thread, debounce edits and advance a running test.
    /// Returns true while something is going on (the UI should repaint).
    pub fn update(&mut self) -> bool {
        if let Some(job) = &self.job {
            match job.receiver.try_recv() {
                Ok(result) => {
                    let start = job.start;
                    self.job = None;
                    self.busy = false;
                    if self.queued {
                        self.queued = false;
                        self.resimulate();
                    } else {
                        self.apply_result(result, start);
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.busy = false;
                    self.status = "Simulation abgebrochen (interner Fehler im Simulator)".into();
                }
            }
        }
        let now = self.clock.monotonic();
        if self.dirty_since.is_some_and(|t| now - t >= EDIT_DELAY_S) {
            self.resimulate();
        }
        if self.running {
            let dt = self.last_tick.map_or(0.0, |t| (now - t).clamp(0.0, 0.25));
            self.last_tick = Some(now);
            let factor = SPEEDS.get(self.speed).map_or(1.0, |s| s.1);
            self.advance(dt * factor);
        } else {
            self.last_tick = None;
        }
        self.running || self.busy || self.dirty_since.is_some()
    }

    /// The program text was edited (mode 3): simulate again after a short pause.
    pub fn text_changed(&mut self) {
        if self.mode == 3 {
            self.dirty_since = Some(self.clock.monotonic());
        }
    }

    pub fn load_text(&mut self, text: String, name: &str) {
        self.library_entry = None;
        self.follow_path = None;
        self.follow_enabled = false;
        self.follow_line = None;
        self.text = text;
        self.source_name = name.to_string();
        self.time = 0.0;
        self.pause();
        self.resimulate();
    }

    pub fn load_demo(&mut self) {
        self.load_text(DEMO_PROGRAM.to_string(), "Demoprogramm");
        self.parameter_text.clear();
    }

    // ------------------------------------------------------------------ modes and menus
    pub fn set_mode(&mut self, mode: usize) {
        let Some(root) = menus::main_mode(mode) else { return };
        self.mode = mode;
        self.menu_stack = vec![root];
        self.pause();
        if mode == 1 {
            if let Some(frame) = self.frame() {
                self.manual_position = frame.position;
            }
            self.manual_trace = vec![self.manual_position];
        } else {
            self.manual_trace.clear();
        }
    }

    pub fn mode_hint(&self) -> &'static str {
        match self.mode {
            1 => "Handfahren: Achsen nur im Simulator verfahren (12 INKREMENT, 13 FAHREN) - keine Maschine!",
            3 => "Programm bearbeiten - die Grafik wird nach Änderungen neu berechnet. Die Originaldatei bleibt unverändert.",
            _ => "Automatik/Testlauf - aktueller Satz ist markiert. Start/Stopp/Einzelsatz über die Tasten unten.",
        }
    }

    /// Auxiliary mode key (MODE, INFO, HELP, EDIT, GRAF, DIA).
    pub fn aux_mode(&mut self, code: &str) -> Option<SimRequest> {
        let item = menus::aux_mode(code)?;
        if !item.is_menu() {
            return self.run_action(item.action);
        }
        let root = self.menu_stack.first().copied().or_else(|| menus::main_mode(self.mode));
        self.menu_stack = root.into_iter().chain(std::iter::once(item)).collect();
        None
    }

    pub fn menu_back(&mut self) {
        if self.menu_stack.len() > 1 {
            self.menu_stack.pop();
        }
    }

    pub fn softkey(&mut self, number: usize) -> Option<SimRequest> {
        let menu = self.menu()?;
        let item = number.checked_sub(1).and_then(|i| menu.slots().get(i).copied().flatten())?;
        if !item.available {
            self.show_screen(format!("{}\n\n{}\n\n{NOT_SIMULATED}", item.title(), item.description));
            self.status = format!("{}: {NOT_SIMULATED}", item.title());
            return None;
        }
        if item.is_menu() {
            self.menu_stack.push(item);
            return None;
        }
        if item.action == BACK {
            self.menu_back();
            return None;
        }
        self.run_action(item.action)
    }

    pub fn mode_path(&self) -> String {
        self.menu_stack.iter().map(|m| m.title()).collect::<Vec<_>>().join(" › ")
    }

    pub fn mode_flags(&self) -> Vec<String> {
        let mut flags = Vec::new();
        if self.mode == 2 {
            flags.push(if self.running { "TESTLAUF LÄUFT" } else { "TESTLAUF HALT" }.to_string());
            flags.push(if self.single_block { "EINZELSATZ" } else { "FOLGESATZ" }.to_string());
            if self.realtime {
                flags.push("ECHTZEIT".into());
            }
        }
        if self.mode == 1 {
            flags.push(format!("Schritt {} mm", self.jog_step()));
        }
        flags.push(if self.graphics_on { "GRAFIK EIN" } else { "GRAFIK AUS" }.into());
        if self.contour_on {
            flags.push("KONTUR EIN".into());
        }
        if self.dripfeed_on {
            flags.push("NACHLADEN".into());
        }
        if let Some(line) = self.follow_line {
            flags.push(format!("ÜBERTRAGEN BIS ZEILE {line}"));
        }
        flags.push("SIMULATION".into());
        flags
    }

    pub fn jog_step(&self) -> f64 {
        JOG_STEPS.get(self.jog).copied().unwrap_or(1.0)
    }

    pub fn show_screen(&mut self, text: String) {
        self.screen_text = text;
        self.tab = SimTab::Screen;
    }

    /// Run a menu function. Returns a request for system dialogs.
    pub fn run_action(&mut self, action: &str) -> Option<SimRequest> {
        let (name, arg) = action.split_once(':').unwrap_or((action, ""));
        match name {
            "preset" => self.preset(),
            "goto" => {
                let current = self.display_position(self.manual_position);
                let text = format!(
                    "X{} Y{} Z{}",
                    deckellink_core::sim::py_g(current[0]),
                    deckellink_core::sim::py_g(current[1]),
                    deckellink_core::sim::py_g(current[2])
                );
                self.dialog = Some(SimDialog::Goto { text });
            }
            "cutting_data" => {
                self.dialog = Some(SimDialog::CuttingData { diameter: 10.0, vc: 100.0, fz: 0.05, flutes: 3 });
            }
            "jog" => {
                let (axis_text, sign) = arg.split_once(':').unwrap_or(("0", "+"));
                let index = axis_text.parse::<usize>().unwrap_or(0);
                let delta = if sign == "+" { 1.0 } else { -1.0 } * self.jog_step();
                self.jog_axis(index, delta);
            }
            "jog_step" => self.jog = (self.jog + 1) % JOG_STEPS.len(),
            "toggle_single" => self.single_block = !self.single_block,
            "toggle_realtime" => {
                self.realtime = !self.realtime;
                self.speed = if self.realtime { 0 } else { DEFAULT_SPEED };
            }
            "select" | "library" | "library_macros" => return Some(SimRequest::OpenFile),
            "toggle_graphics" | "toggle_screen" => self.graphics_on = !self.graphics_on,
            "start_stop" => {
                if self.running {
                    self.pause();
                } else {
                    self.start();
                }
            }
            "test_fast" => {
                self.speed = SPEEDS.len() - 1;
                self.realtime = false;
                self.start();
            }
            "edit_main" => self.goto_section(None),
            "edit_sub" => self.goto_section(Some("%*")),
            "edit_macro" => self.goto_section(Some("%%")),
            "tools" | "parameters" | "zero_points" => self.tab = SimTab::Setup,
            "contour" => {
                self.contour_on = !self.contour_on;
                self.status =
                    "Konturrechner: freie Konturelemente (UW, UR, FA) werden im Simulator nicht berechnet".into();
            }
            "notes" => self.tab = SimTab::Notes,
            "messages" => self.tab = SimTab::Messages,
            "transfer_in" => {
                self.show_screen(
                    "DATEN-TRANSFER › EINLESEN\n\nAn der Steuerung: Programm von einem Partnergerät einlesen.\n\
                     In DeckelLink entspricht das der Seite 'Übertragen' (Senden).\n\n\
                     Im Simulator: Programm aus einer Datei anwählen."
                        .into(),
                );
                return Some(SimRequest::OpenFile);
            }
            "transfer_out" => {
                self.show_screen(
                    "DATEN-TRANSFER › AUSLESEN\n\nAn der Steuerung: Programm an ein Partnergerät ausgeben.\n\
                     In DeckelLink entspricht das der Seite 'Empfangen'.\n\n\
                     Im Simulator: Programm als neue Datei speichern."
                        .into(),
                );
                return Some(SimRequest::SaveAs);
            }
            "toggle_dripfeed" => {
                self.dripfeed_on = !self.dripfeed_on;
                self.show_screen(format!(
                    "DATEN-TRANSFER › NACHLADEN {}\n\n\
                     An der Steuerung: Nachladefunktion aktivieren (F3), EINLESEN wählen (F1) und starten (F8), dann \
                     in Hauptbetriebsart 2 das Programm anwählen und mit Zyklus-Start abarbeiten.\n\n\
                     In DeckelLink: Profil mit LSV2-Protokoll wählen und auf der Seite 'Übertragen' den \
                     Nachladebetrieb starten. Die CNC fordert die Sätze über Quittungen an; DeckelLink startet die \
                     Bearbeitung nie.\n\nIm Simulator: START (F8) zeigt den Ablauf als Testlauf.",
                    if self.dripfeed_on { "EIN" } else { "AUS" }
                ));
            }
            "config_io" => self.show_screen(
                "DATEN-TRANSFER › KONFIG I/O\n\nAn der Steuerung: Schnittstellenparameter für bis zu 7 Partnergeräte \
                 (Gerätenummer, Kanal, Baudrate bis 19200, Datenbits, Stoppbits, Parität, Protokoll).\n\n\
                 Die Werte müssen mit dem DeckelLink-Profil übereinstimmen (Einstellungen → Profil). Bitte an der \
                 Steuerung ablesen und in der Verifikations-Checkliste eintragen."
                    .into(),
            ),
            "transfer_start" => {
                if self.dripfeed_on {
                    self.set_mode(2);
                    self.start();
                    self.status = "Nachladebetrieb (Simulation): Programm läuft als Testlauf".into();
                } else {
                    self.status = "Im Simulator gibt es keine Übertragung - EINLESEN/AUSLESEN wählen".into();
                }
            }
            "screen_text" => self.graphics_on = false,
            "screen_graphics" => self.graphics_on = true,
            "toggle_tool" => self.show_tool = !self.show_tool,
            "toggle_rapid" => self.show_rapid = !self.show_rapid,
            "clear" => self.rewind(),
            "reference" => self.tab = SimTab::Reference,
            "find" => self.dialog = Some(SimDialog::Find { text: String::new() }),
            "replace" => {
                if self.mode == 3 {
                    self.dialog = Some(SimDialog::Replace { old: String::new(), new: String::new() });
                } else {
                    self.status = "EDIT ist nur in Hauptbetriebsart 3 PROGRAMM möglich".into();
                }
            }
            "copy" | "cut" | "delete" | "paste" => self.edit_lines(name),
            "view" => {
                self.view.mode = match arg {
                    "xy" => ViewMode::Xy,
                    "xz" => ViewMode::Xz,
                    "yz" => ViewMode::Yz,
                    "iso" => ViewMode::Iso,
                    _ => self.view.mode.next(),
                };
            }
            "rotate" => self.view.yaw += arg.parse::<f64>().unwrap_or(0.0),
            "zoom" => {
                if arg == "in" {
                    self.view.zoom = (self.view.zoom * 1.5).min(200.0);
                } else {
                    self.view.fit();
                }
            }
            "structure" => {
                let root = arg.parse::<usize>().ok().and_then(menus::main_mode);
                self.show_screen(format!(
                    "Menüstruktur des Simulators nach der DIALOG 11\n\n{}",
                    menus::structure_text(root)
                ));
            }
            "dia" => self.open_dia(arg),
            _ => self.status = format!("Funktion '{action}' ist im Simulator nicht belegt"),
        }
        None
    }

    // ------------------------------------------------------------------ menu functions
    pub fn preset(&mut self) {
        self.preset_offset = self.manual_position;
        self.status = "PRESET: Anzeige an dieser Position auf 0 gesetzt (nur Simulator)".into();
    }

    /// Position shown on the display (mode 1 relative to PRESET).
    pub fn display_position(&self, position: Vec3) -> Vec3 {
        if self.mode != 1 {
            return position;
        }
        [position[0] - self.preset_offset[0], position[1] - self.preset_offset[1], position[2] - self.preset_offset[2]]
    }

    /// FAHREN (simulator only): "X10 Y20 Z5" relative to the displayed zero.
    pub fn goto(&mut self, text: &str) -> bool {
        let mut target = self.display_position(self.manual_position);
        let upper = text.to_uppercase();
        let mut found = false;
        let mut rest = upper.as_str();
        while let Some(position) = rest.find(['X', 'Y', 'Z']) {
            let Some(letter) = rest.get(position..position + 1) else { break };
            let after = rest.get(position + 1..).unwrap_or("").trim_start();
            let number: String = after
                .chars()
                .enumerate()
                .take_while(|(i, c)| {
                    c.is_ascii_digit() || *c == '.' || *c == ',' || (*i == 0 && (*c == '-' || *c == '+'))
                })
                .map(|(_, c)| c)
                .collect();
            if let Some(value) = parse_number(&number).filter(|_| number.chars().any(|c| c.is_ascii_digit())) {
                let index = ["X", "Y", "Z"].iter().position(|a| *a == letter).unwrap_or(0);
                set_axis(&mut target, index, value);
                found = true;
            }
            rest = rest.get(position + 1..).unwrap_or("");
        }
        if !found {
            self.status = "Keine gültige Position (z. B. X10 Y20 Z5)".into();
            return false;
        }
        let position =
            [target[0] + self.preset_offset[0], target[1] + self.preset_offset[1], target[2] + self.preset_offset[2]];
        self.manual_position = position;
        self.manual_trace.push(position);
        true
    }

    pub fn jog_axis(&mut self, index: usize, delta: f64) {
        let mut position = self.manual_position;
        let value = axis(&position, index) + delta;
        set_axis(&mut position, index, value);
        self.manual_position = position;
        self.manual_trace.push(position);
        let name = ["X", "Y", "Z"].get(index).copied().unwrap_or("?");
        self.status = format!(
            "Simulator-Achse {name} um {}{} mm verfahren (keine Maschine)",
            if delta >= 0.0 { "+" } else { "" },
            deckellink_core::sim::py_g(delta)
        );
    }

    fn goto_section(&mut self, prefix: Option<&str>) {
        if self.mode != 3 {
            self.set_mode(3);
        }
        let mut target = 1;
        if let Some(prefix) = prefix {
            let found = self.text.lines().position(|line| {
                let line = line.trim();
                line.starts_with(prefix) && (prefix != "%*" || !line.starts_with("%%"))
            });
            match found {
                Some(index) => target = index + 1,
                None => {
                    self.show_screen(format!(
                        "Kein Abschnitt '{prefix}n' im Programm.\n\nNeuer Abschnitt: am Ende eine Zeile nur mit \
                         '{prefix}1' einfügen und darunter die Sätze schreiben (Simulator-Konvention, siehe Handbuch)."
                    ));
                    return;
                }
            }
        }
        let start = char_index(&self.text, line_start(&self.text, target));
        self.cursor_line = target;
        self.pending_cursor = Some((start, start));
    }

    fn open_dia(&mut self, kind: &str) {
        if self.mode != 3 {
            self.set_mode(3);
        }
        if let Some(form) = menus::dia_form(kind) {
            self.dialog = Some(SimDialog::Dia { kind: form.kind, values: vec![String::new(); form.fields.len()] });
        }
    }

    pub fn next_block_number(&self) -> i64 {
        let numbers = self.text.lines().filter_map(|line| {
            let line = line.trim_start().to_uppercase();
            let digits: String = line.strip_prefix('N')?.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<i64>().ok()
        });
        numbers.max().map_or(10, |n| n.saturating_add(10))
    }

    /// Block of the open DIA form.
    pub fn dia_block(kind: &str, values: &[String], number: Option<i64>) -> String {
        let Some(form) = menus::dia_form(kind) else { return String::new() };
        let value = |address: &str| {
            form.fields.iter().position(|(a, _)| *a == address).and_then(|i| values.get(i)).cloned().unwrap_or_default()
        };
        menus::build_block(form, &value, number)
    }

    /// Insert a block after the cursor line (DIA).
    pub fn insert_block(&mut self, block: &str) {
        let end = line_start(&self.text, self.cursor_line + 1);
        let insert_at = if end > 0 && self.text.as_bytes().get(end - 1) == Some(&b'\n') { end - 1 } else { end };
        self.text.insert_str(insert_at, &format!("\n{block}"));
        self.cursor_line += 1;
        let cursor = char_index(&self.text, insert_at + 1 + block.len());
        self.pending_cursor = Some((cursor, cursor));
        self.text_changed();
    }

    /// EDIT › SUCHEN: next occurrence after the cursor line (wraps around).
    pub fn edit_find(&mut self, needle: &str) -> bool {
        if needle.is_empty() {
            return false;
        }
        let from = line_start(&self.text, self.cursor_line + 1);
        let hit =
            self.text.get(from..).and_then(|t| t.find(needle)).map(|i| i + from).or_else(|| self.text.find(needle));
        match hit {
            Some(byte) => {
                let line = self.text.get(..byte).map_or(0, |t| t.matches('\n').count()) + 1;
                self.cursor_line = line;
                let start = char_index(&self.text, byte);
                self.pending_cursor = Some((start, start + needle.chars().count()));
                self.status = format!("'{needle}' gefunden (Zeile {line})");
                true
            }
            None => {
                self.status = format!("'{needle}' nicht gefunden");
                false
            }
        }
    }

    /// EDIT › ERSETZEN (mode 3 only): replace all occurrences.
    pub fn edit_replace(&mut self, old: &str, new: &str) -> usize {
        if self.mode != 3 {
            self.status = "EDIT ist nur in Hauptbetriebsart 3 PROGRAMM möglich".into();
            return 0;
        }
        if old.is_empty() {
            return 0;
        }
        let count = self.text.matches(old).count();
        if count > 0 {
            self.text = self.text.replace(old, new);
            self.text_changed();
        }
        self.status = format!("{count} Stelle(n) ersetzt");
        count
    }

    /// EDIT › KOPIEREN/SCHIEBEN/LÖSCHEN/EINFÜGEN on the cursor line.
    pub fn edit_lines(&mut self, operation: &str) {
        if self.mode != 3 && operation != "copy" {
            self.status = "EDIT ist nur in Hauptbetriebsart 3 PROGRAMM möglich".into();
            return;
        }
        let mut lines: Vec<String> = self.text.lines().map(ToString::to_string).collect();
        let index = self.cursor_line.saturating_sub(1).min(lines.len().saturating_sub(1));
        match operation {
            "copy" => {
                self.clipboard = lines.get(index).cloned().into_iter().collect();
                self.status = "Satz kopiert".into();
                return;
            }
            "cut" | "delete" => {
                if index < lines.len() {
                    let removed = lines.remove(index);
                    if operation == "cut" {
                        self.clipboard = vec![removed];
                    }
                }
                self.status = if operation == "cut" { "Satz ausgeschnitten" } else { "Satz gelöscht" }.into();
            }
            _ => {
                if self.clipboard.is_empty() {
                    self.status = "Nichts zum Einfügen (zuerst KOPIEREN oder SCHIEBEN)".into();
                    return;
                }
                let at = (index + 1).min(lines.len());
                for (offset, line) in self.clipboard.iter().enumerate() {
                    lines.insert(at + offset, line.clone());
                }
                self.status = "Sätze eingefügt".into();
            }
        }
        let trailing = self.text.ends_with('\n');
        self.text = lines.join("\n");
        if trailing {
            self.text.push('\n');
        }
        self.text_changed();
    }

    // ------------------------------------------------------------------ playback
    pub fn frame(&self) -> Option<Frame> {
        self.playback.as_ref().map(|p| p.frame(self.time))
    }

    pub fn total(&self) -> f64 {
        self.playback.as_ref().map_or(0.0, |p| p.total)
    }

    pub fn start(&mut self) {
        let Some(total) = self.playback.as_ref().map(|p| p.total) else { return };
        if self.single_block {
            self.single_block_step();
            return;
        }
        if self.time >= total {
            self.time = 0.0;
        }
        self.stop_at = None;
        self.running = true;
    }

    pub fn single_block_step(&mut self) {
        let Some(playback) = &self.playback else { return };
        if self.time >= playback.total {
            self.time = 0.0;
        }
        self.stop_at = Some(playback.next_block_end(self.time));
        self.running = true;
    }

    pub fn pause(&mut self) {
        self.running = false;
        self.last_tick = None;
    }

    pub fn rewind(&mut self) {
        self.pause();
        self.time = 0.0;
    }

    pub fn to_end(&mut self) {
        self.pause();
        self.time = self.total();
    }

    pub fn seek(&mut self, fraction: f64) {
        self.pause();
        self.time = self.total() * fraction.clamp(0.0, 1.0);
    }

    /// Advance simulation time (timer and tests).
    pub fn advance(&mut self, seconds: f64) {
        let total = self.total();
        self.time = (self.time + seconds).min(total);
        if let Some(stop) = self.stop_at {
            if self.time >= stop {
                self.time = stop;
                self.stop_at = None;
                self.pause();
            }
        }
        if self.time >= total {
            self.pause();
        }
    }

    /// Listing line clicked: in mode 2 (halted) jump to that block.
    pub fn select_line(&mut self, line: usize) {
        self.cursor_line = line;
        if self.mode != 2 || self.running {
            return;
        }
        if let Some(time) = self.playback.as_ref().and_then(|p| p.time_of_line(line)) {
            self.time = time;
        }
    }

    /// Show the transmitted state of a running transfer (line already sent), NOT the
    /// position of the machine.
    pub fn follow_transfer(&mut self, line: usize) {
        let Some(playback) = &self.playback else { return };
        let time = playback.time_until_line(line);
        self.pause();
        self.follow_line = Some(line);
        self.time = time;
    }

    pub fn time_text(&self) -> String {
        format!("{} / {}", format_duration(self.time), format_duration(self.total()))
    }

    pub fn summary(&self) -> String {
        let Some(result) = self.result() else { return "Noch keine Simulation.".into() };
        format!(
            "{}: {} Sätze ausgeführt, {} Bewegungen, Vorschubweg {:.0} mm, Eilgang {:.0} mm, Laufzeit geschätzt {}.\n\
             Ende: {}\n{} Fehler, {} Warnungen.",
            self.source_name,
            result.executed_blocks,
            result.moves.len(),
            result.feed_length(),
            result.rapid_length(),
            format_duration(result.total_time_s()),
            if result.end_reason.is_empty() { "-" } else { &result.end_reason },
            result.errors().len(),
            result.warnings().len()
        )
    }

    // ------------------------------------------------------------------ dialogs
    /// Confirm the open dialog.
    pub fn dialog_ok(&mut self) {
        let Some(dialog) = self.dialog.take() else { return };
        match dialog {
            SimDialog::Goto { text } => {
                if !self.goto(&text) {
                    self.dialog = Some(SimDialog::Goto { text });
                }
            }
            SimDialog::Dia { kind, values } => {
                let block = Self::dia_block(kind, &values, Some(self.next_block_number()));
                self.insert_block(&block);
            }
            SimDialog::Find { text } => {
                self.edit_find(&text);
            }
            SimDialog::Replace { old, new } => {
                self.edit_replace(&old, &new);
            }
            SimDialog::CuttingData { .. } => {}
        }
    }

    /// Text for saving: CR LF line ends, 7-bit ASCII required.
    pub fn export_bytes(&self) -> Result<Vec<u8>, String> {
        if !self.text.is_ascii() {
            return Err("Das Programm enthält Zeichen außerhalb von 7-Bit-ASCII.".into());
        }
        Ok(self.text.replace("\r\n", "\n").replace('\n', "\r\n").into_bytes())
    }
}

impl crate::app::DeckelApp {
    /// Simulator operations (the page itself never touches ports or the transfer state).
    pub(crate) fn handle_sim(&mut self, action: SimAction) {
        let sim = &mut self.sim;
        let request = match action {
            SimAction::SetMode(mode) => {
                sim.set_mode(mode);
                None
            }
            SimAction::Aux(code) => sim.aux_mode(code),
            SimAction::Back => {
                sim.menu_back();
                None
            }
            SimAction::Softkey(number) => sim.softkey(number),
            SimAction::Start => {
                sim.start();
                None
            }
            SimAction::Stop => {
                sim.pause();
                None
            }
            SimAction::SingleBlock => {
                sim.single_block_step();
                None
            }
            SimAction::Rewind => {
                sim.rewind();
                None
            }
            SimAction::ToEnd => {
                sim.to_end();
                None
            }
            SimAction::Seek(fraction) => {
                sim.seek(fraction);
                None
            }
            SimAction::Speed(index) => {
                sim.speed = index.min(SPEEDS.len() - 1);
                sim.realtime = sim.speed == 0;
                None
            }
            SimAction::JogStep(index) => {
                sim.jog = index.min(JOG_STEPS.len() - 1);
                None
            }
            SimAction::Tab(tab) => {
                sim.tab = tab;
                None
            }
            SimAction::View(mode) => {
                sim.view.mode = mode;
                None
            }
            SimAction::ToggleRapid => {
                sim.show_rapid = !sim.show_rapid;
                None
            }
            SimAction::Fit => {
                sim.view.fit();
                None
            }
            SimAction::LoadDemo => {
                sim.load_demo();
                None
            }
            SimAction::OpenFile => Some(SimRequest::OpenFile),
            SimAction::SaveToLibrary => {
                self.sim_save_to_library();
                return;
            }
            SimAction::SaveAs => Some(SimRequest::SaveAs),
            SimAction::Resimulate => {
                sim.resimulate();
                None
            }
            SimAction::SelectLine(line) => {
                sim.select_line(line);
                if sim.mode == 3 {
                    let start = char_index(&sim.text, line_start(&sim.text, line));
                    sim.pending_cursor = Some((start, start));
                }
                None
            }
            SimAction::DialogOk => {
                sim.dialog_ok();
                None
            }
            SimAction::CloseDialog => {
                sim.dialog = None;
                None
            }
        };
        match request {
            Some(SimRequest::OpenFile) => self.sim_open_file(),
            Some(SimRequest::SaveAs) => self.sim_save_as(),
            None => {}
        }
    }

    fn sim_open_file(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Programm für den Simulator öffnen")
            .add_filter("NC-Programme", &["nc", "NC", "txt", "TXT", "mpf", "MPF", "din", "DIN"])
            .add_filter("Alle Dateien", &["*"]);
        if !self.config.last_open_dir.is_empty() {
            dialog = dialog.set_directory(&self.config.last_open_dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.sim_load_path(&path);
        }
    }

    /// Load a file into the simulator (read only; the file is never changed).
    pub fn sim_load_path(&mut self, path: &std::path::Path) {
        match std::fs::read(path) {
            Ok(data) => {
                let name = path.file_name().map_or_else(|| "Programm".into(), |n| n.to_string_lossy().into_owned());
                self.sim.load_text(String::from_utf8_lossy(&data).replace('\u{4}', ""), &name);
                self.log.info(format!("Simulator: {name} geladen"));
            }
            Err(err) => self.message = Some(format!("Datei konnte nicht gelesen werden: {err}")),
        }
    }

    fn sim_save_as(&mut self) {
        let data = match self.sim.export_bytes() {
            Ok(data) => data,
            Err(err) => {
                self.message = Some(err);
                return;
            }
        };
        let picked = rfd::FileDialog::new()
            .set_title("Als neue Datei speichern")
            .set_file_name(suggested_name(&self.sim.source_name).to_string_lossy())
            .add_filter("NC-Programme", &["nc", "NC", "txt"])
            .save_file();
        let Some(path) = picked else { return };
        // Never replace an existing file (no silent overwrite, originals stay untouched).
        match deckellink_core::storage::write_new_file(&path, &data) {
            Ok(()) => self.notify(format!("Simulator-Programm gespeichert: {}", path.display())),
            Err(err) => {
                self.message = Some(format!(
                    "Nicht gespeichert: {err}. Bitte einen neuen Dateinamen wählen (kein Überschreiben)."
                ));
            }
        }
    }

    /// Open the program of the transfer page in the simulator.
    pub(crate) fn simulate_loaded_program(&mut self) {
        let Some(program) = &self.program else { return };
        let name = program.path.file_name().map_or_else(|| "Programm".into(), |n| n.to_string_lossy().into_owned());
        let text = String::from_utf8_lossy(&program.original).replace('\u{4}', "");
        let path = program.path.clone();
        self.sim.load_text(text, &name);
        self.sim.follow_path = Some(path);
        self.sim.follow_enabled = true;
        self.sim.set_mode(2);
        self.page = crate::app::Page::Simulator;
    }
}

impl crate::app::DeckelApp {
    /// Progress of a running transfer: move the simulator to the transmitted state when it
    /// shows the same file and following is switched on (display only).
    pub(crate) fn follow_in_simulator(&mut self, current_line: usize) {
        if !self.sim.follow_enabled || self.send_line_map.is_empty() {
            return;
        }
        if self.sim.follow_path.is_none() || self.sim.follow_path != self.sending_path {
            return;
        }
        let index = current_line.max(1).min(self.send_line_map.len()) - 1;
        if let Some(line) = self.send_line_map.get(index).copied() {
            self.sim.follow_transfer(line);
        }
    }
}

/// Suggested file name for "save as".
pub fn suggested_name(source: &str) -> PathBuf {
    let stem: String =
        source.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    PathBuf::from(format!("{}_sim.nc", if stem.is_empty() { "programm".into() } else { stem }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckellink_core::clock::FakeClock;

    fn page() -> SimPage {
        let mut page = SimPage::new(Box::new(FakeClock::default()));
        page.simulate_now();
        page
    }

    #[test]
    fn modes_menus_and_softkeys() {
        let mut page = page();
        assert_eq!(page.mode_path(), "2 AUTOMATIK");
        assert!(page.softkey(1).is_none());
        assert_eq!(page.mode_path(), "2 AUTOMATIK › 21 TESTLAUF");
        page.softkey(1);
        page.softkey(1);
        assert!(page.single_block);
        page.softkey(8);
        assert_eq!(page.mode_path(), "2 AUTOMATIK › 21 TESTLAUF");
        page.set_mode(1);
        page.softkey(8);
        page.softkey(1);
        assert_eq!(page.tab, SimTab::Screen);
        assert!(page.screen_text.contains(NOT_SIMULATED));
        assert_eq!(page.aux_mode("DIA"), None);
        assert_eq!(page.softkey(2), None);
        assert!(matches!(page.dialog, Some(SimDialog::Dia { kind: "G1", .. })));
        assert_eq!(page.mode, 3);
        assert_eq!(page.aux_mode("GRAF"), None);
        assert!(!page.graphics_on);
    }

    #[test]
    fn playback_single_block_and_seek() {
        let mut page = page();
        assert!(page.total() > 10.0);
        page.single_block = true;
        page.start();
        page.advance(1e6);
        assert!(!page.running);
        assert!(page.time > 0.0 && page.time < page.total());
        page.single_block = false;
        page.start();
        page.advance(1e6);
        assert!(!page.running);
        assert!((page.time - page.total()).abs() < 1e-9);
        page.seek(0.5);
        assert!((page.time - page.total() / 2.0).abs() < 1e-9);
        page.select_line(9);
        assert!(page.frame().is_some_and(|f| f.line == 9));
    }

    #[test]
    fn jog_goto_preset_and_editing() {
        let mut page = page();
        page.set_mode(1);
        let before = page.manual_position[0];
        page.run_action("jog:0:+");
        assert!((page.manual_position[0] - before - 10.0).abs() < 1e-9);
        page.preset();
        assert!(page.goto("X5 Y-2,5"));
        assert_eq!(page.display_position(page.manual_position)[..2], [5.0, -2.5]);
        assert!(!page.goto("hallo"));
        page.set_mode(3);
        page.cursor_line = 2;
        page.edit_lines("copy");
        page.edit_lines("paste");
        assert_eq!(page.text.lines().nth(2), Some("N10 G0 Z100"));
        page.edit_lines("delete");
        assert_eq!(page.text.lines().nth(1), Some("N10 G0 Z100"));
        // N10 and N270 of the demo program
        assert_eq!(page.edit_replace("G0 Z100", "G0 Z120"), 2);
        assert!(page.edit_find("Z120"));
        assert!(page.text.lines().nth(page.cursor_line - 1).is_some_and(|l| l.starts_with("N270")));
        page.cursor_line = 2;
        assert_eq!(page.next_block_number(), 290);
        let block = SimPage::dia_block("G1", &["10".into(), String::new(), String::new(), "200".into()], Some(300));
        assert_eq!(block, "N300 G1 X10 F200");
        page.insert_block(&block);
        assert_eq!(page.text.lines().nth(2), Some("N300 G1 X10 F200"));
        assert!((cutting_data(10.0, 100.0, 0.05, 3).0 - 3183.1).abs() < 0.1);
    }

    #[test]
    fn worker_thread_delivers_result() {
        let mut page = SimPage::new(Box::new(FakeClock::default()));
        page.resimulate();
        for _ in 0..500 {
            page.update();
            if page.playback.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(page.result().is_some_and(|r| r.end_reason.starts_with("M30")));
    }
}
