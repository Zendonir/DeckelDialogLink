//! Drawing code. Records [`Action`]s; never performs I/O or long operations itself.
//!
//! Arrangement as in the Python version (menu bar, header, numbered keys on the left,
//! content, status bar), the content area built like the DIALOG 11 operator panel:
//! mode line on top of a dark screen, softkeys F1-F8 below it.

use egui::{Color32, RichText, ScrollArea, Ui, Vec2};

use deckellink_core::appdata::ConnectionKind;
use deckellink_core::error::UserFacingError;
use deckellink_core::model::VerificationStatus;
use deckellink_core::nc::{Finding, Severity};
use deckellink_core::parameters::{ParameterDiff, PARAMETER_WRITE_NOTICE, WRITE_CONFIRMATION_PHRASE};
use deckellink_core::profile::safety_warnings;
use deckellink_core::scenarios::{get_scenario, scenarios};
use deckellink_core::transfer::{Direction, SendState};

use crate::app::{Action, DeckelApp, Dialog, Page, StatusKind};
use crate::applog::Level;
use crate::logic::{ascii_line, format_bytes, format_duration, hex_line, preview_text};
use crate::theme::{
    bold, fields, hardware_key, lamp, message, mode_banner, mode_line, mono_bold, nav_key, screen_box, screen_key, tag,
    Tone, CYAN, DIM, GREEN, HEADER, HEADER_DIM, HEADER_TEXT, HOUSING, HOUSING_DARK, KEY_TEXT, NAV, ORANGE, REAL_FILL,
    REAL_TEXT, RED, SCREEN, SIM_FILL, SIM_TEXT, TEXT, YELLOW,
};

pub const SAFETY_NOTICE: &str = "DeckelLink überträgt NC-Textdaten. Die Software prüft nicht, ob ein Programm \
geometrisch, technologisch oder maschinensicher ist. Prüfen Sie das Programm vollständig an der CNC und führen Sie \
den ersten Test ohne Werkstück beziehungsweise nach den betrieblichen Sicherheitsvorgaben durch.";
pub const SCOPE_NOTICE: &str = "DeckelLink überträgt ausschließlich Daten. Programmauswahl, Freigabe und Ausführung \
bleiben vollständig an der CNC-Steuerung. Es werden keine Zyklen gestartet, keine Achsen bewegt und keine \
Maschinenmodi umgeschaltet.";
pub const DRIP_FEED_NOTICE: &str = "NACHLADEBETRIEB: Die CNC arbeitet das Programm ab, während DeckelLink die Sätze \
nach Anforderung (Quittung) der Steuerung liefert. Den Programmstart nehmen Sie ausschließlich an der CNC vor. Ein \
Abbruch in DeckelLink, ein Kabel- oder PC-Fehler unterbricht nur die Datenzufuhr - die Maschine wird dadurch NICHT \
sicher angehalten. Halten Sie die Maschine bei Störungen an der Steuerung an. Der PC darf während des \
Nachladebetriebs nicht in den Energiesparmodus wechseln.";

/// Pages reachable via the numbered keys (order of the Python version).
pub const NAV_PAGES: [Page; 8] = [
    Page::Transfer,
    Page::Receive,
    Page::Programs,
    Page::Parameters,
    Page::Terminal,
    Page::Diagnostics,
    Page::Settings,
    Page::Log,
];

pub fn status_color(kind: StatusKind) -> Color32 {
    match kind {
        StatusKind::Ready => GREEN,
        StatusKind::Warning => ORANGE,
        StatusKind::Error => RED,
        StatusKind::Active => CYAN,
        StatusKind::Idle => DIM,
    }
}

fn severity_tone(severity: Severity) -> Tone {
    match severity {
        Severity::Error => Tone::Error,
        Severity::Warning => Tone::Warn,
        Severity::Info => Tone::Info,
    }
}

// ------------------------------------------------------------------ softkeys

/// Colour of a softkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStyle {
    Normal,
    /// Starts a transfer: yellow in simulation, dark red in real operation.
    Transfer,
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Softkey {
    pub label: String,
    pub action: Action,
    pub enabled: bool,
    pub style: KeyStyle,
}

fn key(label: &str, action: Action, enabled: bool) -> Option<Softkey> {
    Some(Softkey { label: label.into(), action, enabled, style: KeyStyle::Normal })
}

fn styled(label: &str, action: Action, enabled: bool, style: KeyStyle) -> Option<Softkey> {
    Some(Softkey { label: label.into(), action, enabled, style })
}

/// Softkeys F1-F8 of the current page.
pub fn softkeys(app: &DeckelApp) -> [Option<Softkey>; 8] {
    let idle = !app.busy();
    let cancel = styled("ABBRUCH", Action::Cancel, app.busy(), KeyStyle::Cancel);
    match app.page {
        Page::Transfer => {
            let gate = app.send_gate();
            [
                key("DATEI\nÖFFNEN", Action::OpenFileDialog, idle),
                key("PRÜFEN", Action::Recheck, idle && app.program.is_some()),
                if app.simulation() { key("BEISPIEL-\nPROGRAMM", Action::LoadSample, idle) } else { None },
                key("EMPFANGEN", Action::Navigate(Page::Receive), true),
                styled(
                    if app.simulation() { "SIMULATION\nSTARTEN" } else { "AN MASCHINE\nSENDEN" },
                    Action::RequestSend { drip_feed: false },
                    gate.allowed,
                    KeyStyle::Transfer,
                ),
                if app.profile.protocol.is_lsv2() {
                    key("NACHLADE-\nBETRIEB", Action::RequestSend { drip_feed: true }, gate.allowed)
                } else {
                    None
                },
                None,
                cancel,
            ]
        }
        Page::Receive => {
            let stored = app.receive_summary.as_ref().and_then(|s| s.stored_path.clone());
            [
                styled("EMPFANG\nSTARTEN", Action::StartReceive, app.receive_gate().allowed, KeyStyle::Transfer),
                stored.map(|path| Softkey {
                    label: "ZUM SENDEN\nLADEN".into(),
                    action: Action::LoadArchived(path),
                    enabled: idle,
                    style: KeyStyle::Normal,
                }),
                key("NC-\nPROGRAMME", Action::Navigate(Page::Programs), true),
                None,
                None,
                None,
                None,
                cancel,
            ]
        }
        Page::Programs => {
            [key("AKTUALI-\nSIEREN", Action::RefreshArchive, true), None, None, None, None, None, None, None]
        }
        Page::Parameters => {
            let gate = app.parameter_gate();
            [
                styled("PARAMETER\nAUSLESEN", Action::ParamRead, app.receive_gate().allowed, KeyStyle::Transfer),
                key("DATEI\nWÄHLEN", Action::ParamOpenFileDialog, idle),
                if app.param.unlocked {
                    key("SCHREIBEN\nSPERREN", Action::ParamLock, idle)
                } else {
                    key("FREI-\nSCHALTEN", Action::ParamRequestUnlock, idle)
                },
                None,
                styled("PARAMETER\nSCHREIBEN", Action::ParamRequestWrite, gate.allowed, KeyStyle::Transfer),
                None,
                None,
                cancel,
            ]
        }
        Page::Terminal => [
            key(if app.terminal_hex { "ANZEIGE\nASCII" } else { "ANZEIGE\nHEX" }, Action::ToggleHex, true),
            key("LEEREN", Action::ClearTerminal, true),
            None,
            None,
            None,
            None,
            None,
            None,
        ],
        Page::Diagnostics => {
            let gate = app.receive_gate().allowed;
            [
                key("PORTS\nSUCHEN", Action::RefreshPorts, true),
                key("LOOPBACK-\nTEST", Action::StartLoopback, gate),
                key("FORMAT\nMITLESEN", Action::StartFormatDetect, gate),
                key("DIAGNOSE-\nPAKET", Action::ExportDiagnostics, true),
                None,
                None,
                None,
                cancel,
            ]
        }
        Page::Settings => {
            let builtin = app.store.is_builtin(&app.profile.id);
            [
                key("SIMULATION", Action::SetSimulation, idle && !app.simulation()),
                key("REAL-\nBETRIEB", Action::RequestRealMode, idle && app.simulation()),
                key("PROFIL\nBEARBEITEN", Action::EditProfile, idle && app.extra.draft.is_none()),
                key("PROFIL\nKOPIEREN", Action::DuplicateProfile, idle),
                key("PROFIL\nLÖSCHEN", Action::RequestDeleteProfile, idle && !builtin),
                key("PROFIL\nEXPORT", Action::ExportProfileDialog, true),
                key("PROFIL\nIMPORT", Action::ImportProfileDialog, idle),
                None,
            ]
        }
        Page::Log => [
            key("ALLE", Action::LogFilter(Level::Debug), true),
            key("INFO", Action::LogFilter(Level::Info), true),
            key("WARNUNG", Action::LogFilter(Level::Warning), true),
            key("FEHLER", Action::LogFilter(Level::Error), true),
            None,
            None,
            None,
            None,
        ],
        Page::Commissioning => {
            let can_verify = app.extra.commissioning.as_ref().is_some_and(|r| r.can_mark_verified());
            [
                key("PROTOKOLL\nEXPORT", Action::CommExport, true),
                key("ALS VERI-\nFIZIERT", Action::CommRequestVerify, idle && can_verify),
                key("DIAGNOSE", Action::Navigate(Page::Diagnostics), true),
                key("HANDBUCH", Action::Navigate(Page::Help), true),
                None,
                None,
                None,
                None,
            ]
        }
        Page::Help => {
            let count = crate::ui_extra::HELP_DOCUMENTS.len();
            let current = app.extra.help_doc;
            [
                key("VORHERIGES\nKAPITEL", Action::HelpDoc(current.saturating_sub(1)), current > 0),
                key("NÄCHSTES\nKAPITEL", Action::HelpDoc(current + 1), current + 1 < count),
                None,
                None,
                None,
                None,
                None,
                None,
            ]
        }
    }
}

const F_KEYS: [egui::Key; 8] = [
    egui::Key::F1,
    egui::Key::F2,
    egui::Key::F3,
    egui::Key::F4,
    egui::Key::F5,
    egui::Key::F6,
    egui::Key::F7,
    egui::Key::F8,
];

fn softkey_row(app: &mut DeckelApp, ui: &mut Ui) {
    let keys = softkeys(app);
    let gap = 8.0;
    let width = ((ui.available_width() - gap * 7.0) / 8.0).max(60.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (index, slot) in keys.iter().enumerate() {
            let number = format!("F{}", index + 1);
            match slot {
                Some(softkey) => {
                    let fill = match softkey.style {
                        KeyStyle::Transfer if app.simulation() => Some((SIM_FILL, SIM_TEXT)),
                        KeyStyle::Transfer => Some((REAL_FILL, REAL_TEXT)),
                        KeyStyle::Cancel => Some((Color32::from_rgb(0xb3, 0x26, 0x1e), Color32::WHITE)),
                        KeyStyle::Normal => None,
                    };
                    if hardware_key(ui, Vec2::new(width, 58.0), &number, &softkey.label, false, softkey.enabled, fill)
                        .clicked()
                    {
                        app.push(softkey.action.clone());
                    }
                }
                None => {
                    hardware_key(ui, Vec2::new(width, 58.0), &number, "", false, false, None);
                }
            }
        }
    });
}

/// F1-F8, Esc (cancel), Ctrl+1..8 (pages), Ctrl+O (open), Ctrl +/-/0 (font size). Not while a dialog is open.
fn keyboard(app: &mut DeckelApp, ctx: &egui::Context) {
    if app.dialog.is_some() {
        return;
    }
    let keys = softkeys(app);
    for (index, f_key) in F_KEYS.iter().enumerate() {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, *f_key)) {
            if let Some(Some(softkey)) = keys.get(index) {
                if softkey.enabled {
                    app.push(softkey.action.clone());
                }
            }
        }
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) && app.busy() {
        app.push(Action::Cancel);
    }
    let digits = [
        egui::Key::Num1,
        egui::Key::Num2,
        egui::Key::Num3,
        egui::Key::Num4,
        egui::Key::Num5,
        egui::Key::Num6,
        egui::Key::Num7,
        egui::Key::Num8,
    ];
    for (page, digit) in NAV_PAGES.iter().zip(digits) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, digit)) {
            app.push(Action::Navigate(*page));
        }
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) && !app.busy() {
        app.push(Action::OpenFileDialog);
    }
    if ctx.input_mut(|i| {
        i.consume_key(egui::Modifiers::COMMAND, egui::Key::Plus)
            || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Equals)
    }) {
        app.push(Action::FontScale(10));
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Minus)) {
        app.push(Action::FontScale(-10));
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Num0)) {
        app.push(Action::FontScale(0));
    }
}

// ------------------------------------------------------------------ window

/// Whole window for one frame.
pub fn show(app: &mut DeckelApp, ui: &mut Ui) {
    keyboard(app, ui.ctx());
    egui::Panel::top("menu")
        .frame(egui::Frame::new().fill(HOUSING).inner_margin(egui::Margin::symmetric(6, 2)))
        .show(ui, |ui| menu_bar(app, ui));
    egui::Panel::top("header")
        .frame(egui::Frame::new().fill(HEADER).inner_margin(egui::Margin::symmetric(16, 10)))
        .show(ui, |ui| header(app, ui));
    egui::Panel::bottom("statusbar")
        .frame(egui::Frame::new().fill(HOUSING_DARK).inner_margin(egui::Margin::symmetric(12, 4)))
        .show(ui, |ui| statusbar(app, ui));
    egui::Panel::left("nav")
        .resizable(false)
        .exact_size(230.0)
        .frame(egui::Frame::new().fill(NAV).inner_margin(egui::Margin::symmetric(0, 8)))
        .show(ui, |ui| navigation(app, ui));
    egui::CentralPanel::default().frame(egui::Frame::new().fill(HOUSING).inner_margin(egui::Margin::same(12))).show(
        ui,
        |ui| {
            let softkey_height = 70.0;
            let screen_height = (ui.available_height() - softkey_height).max(200.0);
            ui.allocate_ui(Vec2::new(ui.available_width(), screen_height), |ui| screen(app, ui));
            ui.add_space(10.0);
            softkey_row(app, ui);
        },
    );
    dialogs(app, ui.ctx());
    crate::ui_extra::dropped_files(app, ui.ctx());
}

fn menu_bar(app: &mut DeckelApp, ui: &mut Ui) {
    let idle = !app.busy();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.style_mut().override_font_id = Some(egui::FontId::proportional(13.5));
        ui.visuals_mut().override_text_color = Some(KEY_TEXT);
        ui.menu_button("Datei", |ui| {
            ui.visuals_mut().override_text_color = None;
            ui.style_mut().override_font_id = Some(egui::FontId::proportional(13.5));
            if ui.add_enabled(idle, egui::Button::new("NC-Programm öffnen …   Strg+O")).clicked() {
                app.push(Action::OpenFileDialog);
            }
            ui.menu_button("Zuletzt geöffnet", |ui| {
                if app.config.recent_files.is_empty() {
                    ui.add_enabled(false, egui::Button::new("(keine)"));
                }
                for (index, path) in app.config.recent_files.clone().into_iter().enumerate() {
                    if ui.add_enabled(idle, egui::Button::new(format!("{}  {path}", (index + 1) % 10))).clicked() {
                        app.push(Action::LoadFile(path.into()));
                    }
                }
            });
            ui.separator();
            if ui.button("Beenden").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.menu_button("Ansicht", |ui| {
            ui.visuals_mut().override_text_color = None;
            ui.style_mut().override_font_id = Some(egui::FontId::proportional(13.5));
            for (text, delta) in
                [("Schrift größer   Strg++", 10), ("Schrift kleiner   Strg+-", -10), ("Schrift Standard   Strg+0", 0)]
            {
                if ui.button(text).clicked() {
                    app.push(Action::FontScale(delta));
                }
            }
        });
        ui.menu_button("Werkzeuge", |ui| {
            ui.visuals_mut().override_text_color = None;
            ui.style_mut().override_font_id = Some(egui::FontId::proportional(13.5));
            for (text, page) in [
                ("Loopback-Test …", Page::Diagnostics),
                ("Datenformat ermitteln (nur Mitlesen) …", Page::Diagnostics),
                ("Diagnosepaket exportieren …", Page::Diagnostics),
                ("Inbetriebnahme-Assistent …", Page::Commissioning),
            ] {
                if ui.button(text).clicked() {
                    app.push(Action::Navigate(page));
                }
            }
        });
        ui.menu_button("Hilfe", |ui| {
            ui.visuals_mut().override_text_color = None;
            ui.style_mut().override_font_id = Some(egui::FontId::proportional(13.5));
            for (text, doc) in [("Handbuch …", 0usize), ("Sicherheitskonzept …", 1), ("Inbetriebnahme …", 2)] {
                if ui.button(text).clicked() {
                    app.push(Action::HelpDoc(doc));
                    app.push(Action::Navigate(Page::Help));
                }
            }
        });
    });
}

fn header(app: &mut DeckelApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(bold("DECKEL FP2A", 20.0).color(Color32::WHITE));
            ui.label(RichText::new("DIALOG 11").size(13.0).color(HEADER_DIM).font(egui::FontId::proportional(13.0)));
        });
        ui.add_space(28.0);
        let info_width = (ui.available_width() - crate::theme::BANNER_WIDTH - 16.0).max(200.0);
        ui.allocate_ui_with_layout(Vec2::new(info_width, 54.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let (kind, text) = app.status.clone();
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 14.0), egui::Sense::hover());
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(rect.center(), Vec2::splat(10.0)),
                    1.0,
                    status_color(kind),
                );
                ui.label(bold(text, 13.5).color(HEADER_TEXT));
            });
            let port = app.port_label();
            let mut link = app.profile.serial.summary();
            if app.profile.protocol.is_lsv2() {
                link.push_str(" | LSV2");
            }
            let proportional =
                |text: String, color: Color32| RichText::new(text).color(color).font(egui::FontId::proportional(12.5));
            ui.label(proportional(
                format!("{}  |  {link}", if port.is_empty() { "(kein Port)".to_string() } else { port }),
                HEADER_DIM,
            ));
            let verified = app.profile.verification_status == VerificationStatus::Verified;
            ui.label(proportional(
                format!(
                    "Profil: {}  ({})",
                    app.profile.display_name,
                    if verified { "verifiziert" } else { "NICHT verifiziert" }
                ),
                if verified { HEADER_DIM } else { Color32::from_rgb(0xff, 0xd8, 0x6b) },
            ));
        });
        ui.add_space(ui.available_width() - crate::theme::BANNER_WIDTH);
        mode_banner(ui, app.simulation(), &app.port_label());
    });
}

fn statusbar(app: &mut DeckelApp, ui: &mut Ui) {
    let text = |t: &str| RichText::new(t).color(KEY_TEXT).font(egui::FontId::proportional(12.5));
    ui.horizontal(|ui| {
        if let Some(task) = &app.task {
            ui.spinner();
            ui.label(text(&format!("{} läuft …  (Esc = Abbrechen)", task.kind.label())));
            ui.separator();
        }
        if let Some(message) = app.message.clone() {
            ui.label(text(&message));
            if ui.small_button(RichText::new("×").color(KEY_TEXT)).on_hover_text("Meldung ausblenden").clicked() {
                app.message = None;
            }
        }
        if let Some(error) = &app.log.file_error {
            ui.label(RichText::new(error).color(Color32::from_rgb(0x8a, 0x61, 0x00)));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(text(&format!("DeckelLink {}", env!("CARGO_PKG_VERSION"))));
        });
    });
}

fn navigation(app: &mut DeckelApp, ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 2.0;
    for (index, page) in NAV_PAGES.iter().enumerate() {
        if nav_key(ui, index + 1, page.label(), app.page == *page).clicked() {
            app.push(Action::Navigate(*page));
        }
    }
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new("HILFSFUNKTIONEN").size(11.0).color(HEADER_DIM).font(egui::FontId::proportional(11.0)));
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        let width = (ui.available_width() - 20.0) / 2.0;
        if hardware_key(ui, Vec2::new(width, 34.0), "", "HELP", app.page == Page::Help, true, None).clicked() {
            app.push(Action::Navigate(Page::Help));
        }
        if hardware_key(ui, Vec2::new(width, 34.0), "", "INBETRIEBN.", app.page == Page::Commissioning, true, None)
            .clicked()
        {
            app.push(Action::Navigate(Page::Commissioning));
        }
    });
}

/// The dark display: mode line, page content.
fn screen(app: &mut DeckelApp, ui: &mut Ui) {
    egui::Frame::new()
        .fill(SCREEN)
        .stroke(egui::Stroke::new(3.0, Color32::from_rgb(0x5f, 0x6b, 0x73)))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let number =
                NAV_PAGES.iter().position(|p| *p == app.page).map(|i| format!("{} ", i + 1)).unwrap_or_default();
            let sub = match app.page {
                Page::Transfer => app.send_state.label().to_string(),
                Page::Receive => app.receive_state.label().to_string(),
                Page::Help => crate::ui_extra::HELP_DOCUMENTS
                    .get(app.extra.help_doc)
                    .map(|(t, _)| (*t).to_string())
                    .unwrap_or_default(),
                _ => app.task.as_ref().map(|t| format!("{} läuft", t.kind.label())).unwrap_or_default(),
            };
            let path = if sub.is_empty() {
                format!("{number}{}", app.page.label().to_uppercase())
            } else {
                format!("{number}{}  ›  {}", app.page.label().to_uppercase(), sub.to_uppercase())
            };
            let right = format!("{}  {}", app.profile.serial.frame(), if app.simulation() { "SIM" } else { "REAL" });
            mode_line(ui, &path, &right);
            ui.add_space(8.0);
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(1200.0));
                match app.page {
                    Page::Transfer => transfer_page(app, ui),
                    Page::Receive => receive_page(app, ui),
                    Page::Programs => programs_page(app, ui),
                    Page::Parameters => parameters_page(app, ui),
                    Page::Terminal => terminal_page(app, ui),
                    Page::Diagnostics => diagnostics_page(app, ui),
                    Page::Settings => settings_page(app, ui),
                    Page::Log => log_page(app, ui),
                    Page::Commissioning => crate::ui_extra::commissioning_page(app, ui),
                    Page::Help => crate::ui_extra::help_page(app, ui),
                }
            });
        });
}

// ------------------------------------------------------------------ shared screen parts

pub fn error_panel(ui: &mut Ui, error: &UserFacingError) {
    message(ui, Tone::Error, &error.title, |ui| {
        if !error.causes.is_empty() {
            ui.label(RichText::new("Mögliche Ursachen:").color(DIM));
            for cause in &error.causes {
                ui.label(format!("- {cause}"));
            }
        }
        if !error.technical_details.is_empty() {
            egui::CollapsingHeader::new("Technische Details").id_salt(("error-details", &error.title)).show(ui, |ui| {
                ui.label(RichText::new(&error.technical_details).color(DIM));
            });
        }
    });
}

fn findings_list(ui: &mut Ui, findings: &[Finding]) {
    if findings.is_empty() {
        ui.label(RichText::new("KEINE AUFFÄLLIGKEITEN").color(GREEN));
        return;
    }
    for finding in findings {
        let tone = severity_tone(finding.severity);
        ui.horizontal_wrapped(|ui| {
            tag(ui, tone.tag(), tone.color());
            ui.label(&finding.message);
            let lines = finding.lines_text();
            if !lines.is_empty() {
                ui.label(RichText::new(format!("(Zeilen {lines})")).color(DIM));
            }
        });
    }
}

fn gate_hint(ui: &mut Ui, reasons: &[String]) {
    if !reasons.is_empty() {
        ui.label(RichText::new(format!("GESPERRT: {}", reasons.join("; "))).color(ORANGE));
    }
}

fn progress_bar(ui: &mut Ui, percent: f64) {
    #[allow(clippy::cast_possible_truncation)]
    let fraction = (percent / 100.0) as f32;
    ui.add(
        egui::ProgressBar::new(fraction)
            .show_percentage()
            .fill(Color32::from_rgb(0x2b, 0x8a, 0x3e))
            .desired_height(20.0)
            .corner_radius(0.0),
    );
}

fn state_kind(state: SendState) -> StatusKind {
    match state {
        SendState::Completed | SendState::Ready => StatusKind::Ready,
        SendState::Failed => StatusKind::Error,
        SendState::Cancelled => StatusKind::Warning,
        SendState::Idle => StatusKind::Idle,
        _ => StatusKind::Active,
    }
}

// ------------------------------------------------------------------ pages

fn transfer_page(app: &mut DeckelApp, ui: &mut Ui) {
    if let Some(error) = app.send_error.clone() {
        error_panel(ui, &error);
    }
    let Some(program) = app.program.clone() else {
        screen_box(ui, "NC-Programm", |ui| {
            ui.label("Keine Datei geladen.");
            ui.label(
                RichText::new(
                    "F1 DATEI ÖFFNEN oder Datei auf das Fenster ziehen. Die Originaldatei wird nie verändert.",
                )
                .color(DIM),
            );
        });
        return;
    };
    ui.columns(2, |cols| {
        let [left, right] = cols else { return };
        screen_box(left, "NC-Programm", |ui| {
            fields(
                ui,
                "file",
                &[
                    ("Datei", program.file_name()),
                    ("Pfad", program.path.display().to_string()),
                    ("Größe", format_bytes(program.original.len())),
                    ("Zeilen", program.analysis.line_count.to_string()),
                    ("Längste Zeile", format!("{} Zeichen", program.analysis.longest_line)),
                    (
                        "Programmende",
                        if program.analysis.has_program_end { "gefunden".into() } else { "NICHT gefunden".into() },
                    ),
                    ("SHA-256", program.analysis.sha256.get(..16).unwrap_or_default().to_string()),
                ],
            );
        });
        screen_box(right, "Prüfung", |ui| {
            findings_list(ui, &program.analysis.findings);
            if program.analysis.has_warnings() {
                let mut ack = app.ack_warnings;
                if ui.checkbox(&mut ack, "Warnungen gelesen und bestätigt").changed() {
                    app.ack_warnings = ack;
                }
            }
            if app.profile.transfer.append_program_end && !program.analysis.has_program_end {
                let mut append = app.append_end;
                if ui.checkbox(&mut append, "Fehlendes Programmende ergänzen (nur diese Übertragung)").changed() {
                    app.push(Action::SetAppendEnd(append));
                }
            }
            if let Some(err) = &program.transform_error {
                ui.label(RichText::new(format!("Aufbereitung: {err}")).color(RED));
            }
        });
    });
    screen_box(ui, "Übertragung", |ui| {
        lamp(ui, status_color(state_kind(app.send_state)), &app.send_state.label().to_uppercase());
        gate_hint(ui, &app.send_gate().reasons);
        if let Some(progress) = &app.send_progress {
            progress_bar(ui, progress.percent());
            ui.label(
                RichText::new(format!(
                    "{} von {}  |  Zeile {} von {}  |  {}",
                    format_bytes(progress.bytes_done),
                    format_bytes(progress.bytes_total),
                    progress.current_line,
                    progress.total_lines,
                    format_duration(progress.elapsed_s)
                ))
                .color(DIM),
            );
        }
    });
    if let Some(payload) = &program.payload {
        screen_box(
            ui,
            &format!("Transferdaten  {}  {} Zeilen", format_bytes(payload.data.len()), payload.line_count),
            |ui| {
                for note in &payload.applied {
                    ui.label(RichText::new(format!("- {note}")).color(DIM));
                }
                ScrollArea::vertical().max_height(260.0).id_salt("preview").show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (index, line) in preview_text(&payload.data, 400).lines().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{:>4}", index + 1)).color(DIM));
                            ui.label(RichText::new(line).color(TEXT));
                        });
                    }
                });
            },
        );
    }
}

fn receive_page(app: &mut DeckelApp, ui: &mut Ui) {
    screen_box(ui, "Empfang", |ui| {
        ui.label("Ablauf: F1 EMPFANG STARTEN, danach die Programmausgabe an der CNC auslösen.");
        ui.label(
            RichText::new("Empfangene Programme werden als neue Datei gespeichert - nie überschrieben.").color(DIM),
        );
        ui.add_space(4.0);
        lamp(ui, CYAN, &app.receive_state.label().to_uppercase());
        gate_hint(ui, &app.receive_gate().reasons);
        if let Some(progress) = &app.receive_progress {
            fields(
                ui,
                "receive-progress",
                &[
                    ("Empfangen", format_bytes(progress.bytes_received)),
                    ("Dauer", format_duration(progress.elapsed_s)),
                    ("Ohne Daten seit", format!("{:.1} s", progress.idle_s)),
                ],
            );
        }
    });
    if let Some(error) = app.receive_error.clone() {
        error_panel(ui, &error);
    }
    if let Some(summary) = app.receive_summary.clone() {
        let (tone, text) = if summary.complete {
            (Tone::Ok, "Programm vollständig empfangen".to_string())
        } else if summary.bytes == 0 {
            (Tone::Error, format!("Keine Daten empfangen ({})", summary.end_reason))
        } else {
            (Tone::Warn, "Programm möglicherweise UNVOLLSTÄNDIG - nicht ungeprüft verwenden".to_string())
        };
        message(ui, tone, &text, |ui| {
            fields(
                ui,
                "receive-result",
                &[
                    ("Zustand", summary.state.label().to_string()),
                    ("Ende", summary.end_reason.clone()),
                    ("Größe", format_bytes(summary.bytes)),
                    (
                        "Gespeichert",
                        summary.stored_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "-".into()),
                    ),
                ],
            );
            for note in &summary.notes {
                ui.label(RichText::new(format!("- {note}")).color(DIM));
            }
            for finding in &summary.findings {
                ui.label(finding);
            }
        });
    }
}

fn programs_page(app: &mut DeckelApp, ui: &mut Ui) {
    screen_box(ui, &format!("Empfangene Programme ({})", app.archive.len()), |ui| {
        ui.label(RichText::new(format!("Ordner: {}", app.paths.received.display())).color(DIM));
        ui.add_space(4.0);
        if app.archive.is_empty() {
            ui.label("Noch keine Programme empfangen.");
            return;
        }
        egui::Grid::new("archive").num_columns(5).spacing([18.0, 4.0]).striped(true).show(ui, |ui| {
            for title in ["DATEI", "STATUS", "GRÖSSE", "GEÄNDERT", ""] {
                ui.label(RichText::new(title).color(YELLOW));
            }
            ui.end_row();
            for entry in app.archive.clone() {
                ui.label(entry.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                match (entry.folder, entry.complete()) {
                    (_, Some(true)) => tag(ui, "VOLLSTÄNDIG", GREEN),
                    ("failed", _) | (_, Some(false)) => tag(ui, "UNVOLLSTÄNDIG", ORANGE),
                    _ => tag(ui, "OHNE METADATEN", DIM),
                };
                ui.label(format_bytes(usize::try_from(entry.size).unwrap_or(usize::MAX)));
                ui.label(entry.modified.map(|m| m.format("%d.%m.%Y %H:%M").to_string()).unwrap_or_default());
                if ui.add_enabled(!app.busy(), screen_key("Zum Senden laden")).clicked() {
                    app.push(Action::LoadArchived(entry.path.clone()));
                }
                ui.end_row();
            }
        });
    });
}

fn diff_view(ui: &mut Ui, title: &str, diff: &ParameterDiff) {
    screen_box(ui, &format!("Vergleich {title}"), |ui| {
        ui.label(RichText::new(diff.summary()).color(YELLOW));
        if diff.diff_text.is_empty() {
            return;
        }
        ScrollArea::vertical().max_height(280.0).id_salt("diff").show(ui, |ui| {
            ui.set_width(ui.available_width());
            for line in diff.diff_text.lines() {
                let color = if line.starts_with('+') && !line.starts_with("+++") {
                    GREEN
                } else if line.starts_with('-') && !line.starts_with("---") {
                    RED
                } else {
                    DIM
                };
                ui.label(RichText::new(line).color(color));
            }
        });
    });
}

fn parameters_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.label(RichText::new(
        "Parameter nur als Datei: Auslesen nach Start der Parameterausgabe an der CNC, Schreiben nur in eine an der CNC freigegebene Parametereingabe. Format der DIALOG 11 nicht verifiziert.",
    ).color(DIM));
    ui.add_space(6.0);
    ui.columns(2, |cols| {
        let [left, right] = cols else { return };
        screen_box(left, "1  Sicherung (F1 Auslesen)", |ui| {
            if let Some((kind, text)) = app.param.status.clone() {
                lamp(ui, status_color(kind), &text);
            }
            if app.param.received_bytes > 0 && app.busy() {
                ui.label(format!("{} empfangen", format_bytes(app.param.received_bytes)));
            }
            match &app.param.backup {
                Some(backup) => fields(
                    ui,
                    "backup",
                    &[
                        ("Datei", backup.path_name.clone()),
                        ("Größe", format_bytes(backup.data.len())),
                        ("SHA-256", backup.sha256().get(..16).unwrap_or_default().to_string()),
                        ("Ausgelesen", backup.read_at.format("%H:%M:%S").to_string()),
                        ("Struktur", backup.structure().map(|s| s.summary()).unwrap_or_default()),
                    ],
                ),
                None => {
                    ui.label(RichText::new("Keine Sicherung in dieser Sitzung.").color(DIM));
                }
            }
            if let Some((ok, text)) = app.param.verify.clone() {
                message(ui, if ok { Tone::Ok } else { Tone::Error }, &text, |_| {});
            }
        });
        screen_box(right, "2  Parameterdatei (F2 Wählen)", |ui| match app.param.candidate.clone() {
            Some(candidate) => {
                fields(
                    ui,
                    "candidate",
                    &[
                        ("Datei", candidate.name.clone()),
                        ("Größe", format_bytes(candidate.data.len())),
                        ("SHA-256", candidate.analysis.sha256.get(..16).unwrap_or_default().to_string()),
                        ("Struktur", candidate.structure().map(|s| s.summary()).unwrap_or_default()),
                    ],
                );
                findings_list(ui, &candidate.analysis.findings);
            }
            None => {
                ui.label(RichText::new("Keine Datei gewählt.").color(DIM));
            }
        });
    });
    if let Some(error) = app.param.error.clone() {
        error_panel(ui, &error);
    }
    screen_box(ui, "3  Schreiben (F3 Freischalten, F5 Schreiben)", |ui| {
        if app.param.unlocked {
            lamp(ui, ORANGE, "SCHREIBFUNKTION FÜR DIESE SITZUNG FREIGESCHALTET");
        } else {
            lamp(ui, DIM, "SCHREIBFUNKTION GESPERRT");
        }
        gate_hint(ui, &app.parameter_gate().reasons);
        if let Some(progress) = &app.param.progress {
            progress_bar(ui, progress.percent());
        }
    });
    if let Some((title, diff)) = app.param.diff.clone() {
        diff_view(ui, &title, &diff);
    }
    screen_box(ui, &format!("Gespeicherte Parametersätze ({})", app.param.sets.len()), |ui| {
        egui::Grid::new("param-sets").num_columns(4).spacing([14.0, 4.0]).striped(true).show(ui, |ui| {
            for set in app.param.sets.clone() {
                ui.label(set.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                if set.complete() {
                    tag(ui, "VOLLSTÄNDIG", GREEN);
                } else {
                    tag(ui, "UNVOLLSTÄNDIG", ORANGE);
                }
                if ui.add(screen_key("Vergleichen")).clicked() {
                    app.push(Action::ParamCompare(set.path.clone()));
                }
                if ui.add_enabled(!app.busy(), screen_key("Zum Schreiben wählen")).clicked() {
                    app.push(Action::ParamSelectFile(set.path.clone()));
                }
                ui.end_row();
            }
        });
    });
}

fn terminal_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.label(RichText::new(format!(
        "{} Einträge (max. {})  |  Anzeige {}  |  Rohdaten gelangen nur bei aktiviertem Rohdatenprotokoll in die Protokolldatei.",
        app.traffic.len(),
        app.config.terminal_history_lines,
        if app.terminal_hex { "HEX" } else { "ASCII" }
    )).color(DIM));
    ui.add_space(4.0);
    screen_box(ui, "Datenverkehr", |ui| {
        ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).id_salt("terminal").show(ui, |ui| {
            ui.set_width(ui.available_width());
            if app.traffic.is_empty() {
                ui.label(RichText::new("Noch keine Daten.").color(DIM));
            }
            for event in &app.traffic {
                let (dir, color) = match event.direction {
                    Direction::Tx => ("TX", CYAN),
                    Direction::Rx => ("RX", GREEN),
                };
                let text = if app.terminal_hex { hex_line(&event.data) } else { ascii_line(&event.data) };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(event.timestamp.format("%H:%M:%S%.3f").to_string()).color(DIM));
                    ui.label(mono_bold(dir, 13.5).color(color));
                    ui.label(RichText::new(text).color(TEXT));
                });
            }
        });
    });
}

fn line_state(ui: &mut Ui, name: &str, value: Option<bool>) {
    match value {
        Some(true) => tag(ui, &format!("{name} EIN"), GREEN),
        Some(false) => tag(ui, &format!("{name} AUS"), DIM),
        None => tag(ui, &format!("{name} ?"), DIM),
    };
}

fn diagnostics_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.columns(2, |cols| {
        let [left, right] = cols else { return };
        screen_box(left, "Serielle Anschlüsse (F1)", |ui| {
            if app.ports.is_empty() {
                ui.label(RichText::new("Keine gefunden (oder noch nicht gesucht).").color(DIM));
            }
            for port in &app.ports {
                ui.label(port.label());
            }
        });
        screen_box(right, "Modemleitungen", |ui| match app.modem {
            Some(m) => {
                ui.horizontal_wrapped(|ui| {
                    line_state(ui, "CTS", m.cts);
                    line_state(ui, "DSR", m.dsr);
                    line_state(ui, "DCD", m.dcd);
                    line_state(ui, "RI", m.ri);
                    line_state(ui, "RTS", m.rts);
                    line_state(ui, "DTR", m.dtr);
                });
            }
            None => {
                ui.label(RichText::new("Werte erscheinen während einer Übertragung.").color(DIM));
            }
        });
    });
    crate::ui_extra::diagnostics_tools(app, ui);
    screen_box(ui, "Programm und Profil", |ui| {
        fields(
            ui,
            "program-info",
            &[
                ("Version", format!("DeckelLink {} (Rust)", env!("CARGO_PKG_VERSION"))),
                ("Profil", format!("{} ({})", app.profile.display_name, app.profile.id)),
                ("Status", app.profile.verification_status.label().to_string()),
                ("Datenordner", app.paths.root.display().to_string()),
                ("Protokolle", app.paths.logs.display().to_string()),
            ],
        );
        for warning in safety_warnings(&app.profile) {
            message(ui, Tone::Warn, &warning, |_| {});
        }
        for problem in app.store.problems.clone() {
            message(ui, Tone::Error, &format!("Profilproblem: {problem}"), |_| {});
        }
    });
}

fn settings_page(app: &mut DeckelApp, ui: &mut Ui) {
    screen_box(ui, "Betriebsart (F1 Simulation, F2 Realbetrieb)", |ui| {
        if app.simulation() {
            lamp(ui, YELLOW, "SIMULATION - keine Verbindung zur Maschine");
            let current = get_scenario(&app.config.simulation_scenario);
            ui.horizontal(|ui| {
                ui.label("Szenario:");
                egui::ComboBox::from_id_salt("scenario").selected_text(current.title).width(440.0).show_ui(ui, |ui| {
                    for scenario in scenarios() {
                        if ui.selectable_label(scenario.id == current.id, scenario.title).clicked() {
                            app.config.simulation_scenario = scenario.id.into();
                            app.push(Action::SaveConfig);
                        }
                    }
                });
            });
            ui.label(RichText::new(format!("{} ({})", current.description, current.kind.label())).color(DIM));
            crate::ui_extra::replay_selection(app, ui);
        } else {
            lamp(ui, RED, &format!("REALBETRIEB - {}", crate::logic::real_port_label(&app.config)));
        }
    });
    screen_box(ui, "Verbindung für den Realbetrieb", |ui| {
        ui.add_enabled_ui(!app.busy(), |ui| {
            let mut changed = false;
            egui::Grid::new("connection").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
                ui.label("Verbindungsart:");
                egui::ComboBox::from_id_salt("conn-kind")
                    .selected_text(app.config.connection_kind.label())
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        for kind in ConnectionKind::ALL {
                            changed |=
                                ui.selectable_value(&mut app.config.connection_kind, kind, kind.label()).changed();
                        }
                    });
                ui.end_row();
                match app.config.connection_kind {
                    ConnectionKind::Serial => {
                        ui.label("COM-Port:");
                        ui.horizontal(|ui| {
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut app.config.port)
                                        .desired_width(140.0)
                                        .hint_text("z. B. COM3"),
                                )
                                .lost_focus();
                            egui::ComboBox::from_id_salt("ports").selected_text("Auswahl").show_ui(ui, |ui| {
                                for port in app.ports.clone() {
                                    if ui.selectable_label(app.config.port == port.device, port.label()).clicked() {
                                        app.config.port = port.device.clone();
                                        changed = true;
                                    }
                                }
                            });
                            if ui.add(screen_key("Suchen")).clicked() {
                                app.push(Action::RefreshPorts);
                            }
                        });
                        ui.end_row();
                    }
                    ConnectionKind::Tcp | ConnectionKind::Rfc2217 => {
                        ui.label("Adapter (IP/Name):");
                        changed |= ui
                            .add(egui::TextEdit::singleline(&mut app.config.network_host).desired_width(220.0))
                            .lost_focus();
                        ui.end_row();
                        ui.label("TCP-Port:");
                        changed |=
                            ui.add(egui::DragValue::new(&mut app.config.network_port).range(1..=65535)).changed();
                        ui.end_row();
                    }
                }
            });
            if changed {
                app.push(Action::SaveConfig);
            }
        });
    });
    screen_box(ui, "Profil (F3 Bearbeiten  F4 Kopieren  F5 Löschen  F6 Export  F7 Import)", |ui| {
        ui.add_enabled_ui(!app.busy(), |ui| {
            egui::ComboBox::from_id_salt("profile")
                .selected_text(app.profile.display_name.clone())
                .width(480.0)
                .show_ui(ui, |ui| {
                    for profile in app.store.profiles().to_vec() {
                        if ui.selectable_label(profile.id == app.profile.id, &profile.display_name).clicked() {
                            app.push(Action::SelectProfile(profile.id.clone()));
                        }
                    }
                });
        });
        let p = app.profile.clone();
        fields(
            ui,
            "profile-info",
            &[
                ("Schnittstelle", p.serial.summary()),
                ("Protokoll", p.protocol.mode.label().to_string()),
                ("Zeilenende", p.transfer.line_ending.as_str().to_uppercase()),
                ("Programmende", p.transfer.program_end.clone()),
                ("Status", p.verification_status.label().to_string()),
            ],
        );
        if !p.description.is_empty() {
            ui.label(RichText::new(&p.description).color(DIM));
        }
        crate::ui_extra::profile_editor(app, ui);
    });
    screen_box(ui, "Anzeige und Protokoll", |ui| {
        let mut changed = false;
        egui::Grid::new("display").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
            ui.label("Schriftgröße (%):");
            changed |= ui.add(egui::DragValue::new(&mut app.config.font_scale_percent).range(80..=200)).changed();
            ui.end_row();
            ui.label("Protokollstufe:");
            egui::ComboBox::from_id_salt("loglevel").selected_text(app.config.log_level.clone()).show_ui(ui, |ui| {
                for level in deckellink_core::appdata::LOG_LEVELS {
                    changed |= ui.selectable_value(&mut app.config.log_level, level.to_string(), level).changed();
                }
            });
            ui.end_row();
        });
        changed |= ui
            .checkbox(&mut app.config.show_safety_notice, "Sicherheitshinweis vor realen Übertragungen anzeigen")
            .changed();
        changed |= ui
            .checkbox(
                &mut app.config.raw_traffic_logging,
                "Rohdatenprotokoll in die Protokolldatei schreiben (enthält NC-Daten)",
            )
            .changed();
        if changed {
            app.push(Action::SaveConfig);
        }
        ui.label(RichText::new(format!("Datenordner: {}", app.paths.root.display())).color(DIM));
    });
}

fn log_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.label(
        RichText::new(format!("Anzeige ab {}  |  Es werden keine NC-Inhalte protokolliert.", app.log_filter.as_str()))
            .color(DIM),
    );
    ui.add_space(4.0);
    screen_box(ui, "Ereignisse", |ui| {
        ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).id_salt("log").show(ui, |ui| {
            for entry in app.log.entries.iter().filter(|e| e.level >= app.log_filter) {
                let color = match entry.level {
                    Level::Error => RED,
                    Level::Warning => ORANGE,
                    Level::Debug => DIM,
                    Level::Info => TEXT,
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&entry.time).color(DIM));
                    ui.label(RichText::new(format!("{:<7}", entry.level.as_str())).color(color));
                    ui.label(RichText::new(&entry.message).color(color));
                });
            }
        });
    });
}

// ------------------------------------------------------------------ dialogs

fn dialogs(app: &mut DeckelApp, ctx: &egui::Context) {
    let Some(dialog) = app.dialog.clone() else { return };
    let modal = egui::Modal::new(egui::Id::unique("deckellink-dialog")).show(ctx, |ui| {
        ui.set_max_width(680.0);
        match dialog {
            Dialog::ConfirmSend { drip_feed, drip_ack, suppress } => {
                confirm_send(app, ui, drip_feed, drip_ack, suppress)
            }
            Dialog::ParamUnlock => param_unlock(app, ui),
            Dialog::ParamWrite { phrase, ack } => param_write(app, ui, phrase, ack),
            Dialog::RealMode => real_mode(app, ui),
            Dialog::DeleteProfile => crate::ui_extra::delete_profile_dialog(app, ui),
            Dialog::MarkVerified => crate::ui_extra::mark_verified_dialog(app, ui),
        }
    });
    if modal.should_close() {
        app.push(Action::CloseDialog);
    }
}

/// Dialog title as a mode line.
pub fn dialog_title(ui: &mut Ui, title: &str) {
    mode_line(ui, &title.to_uppercase(), "");
    ui.add_space(8.0);
}

/// Dialog keys: cancel key left, confirming key right (style of the softkeys).
pub fn dialog_keys(
    ui: &mut Ui,
    cancel: &str,
    confirm: &str,
    enabled: bool,
    fill: Option<(Color32, Color32)>,
) -> (bool, bool) {
    ui.add_space(10.0);
    let mut result = (false, false);
    ui.horizontal(|ui| {
        result.0 = hardware_key(ui, Vec2::new(170.0, 42.0), "", cancel, false, true, None).clicked();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            result.1 = hardware_key(ui, Vec2::new(230.0, 42.0), "", confirm, false, enabled, fill).clicked();
        });
    });
    result
}

fn mode_fill(simulation: bool) -> Option<(Color32, Color32)> {
    Some(if simulation { (SIM_FILL, SIM_TEXT) } else { (REAL_FILL, REAL_TEXT) })
}

fn confirm_send(app: &mut DeckelApp, ui: &mut Ui, drip_feed: bool, mut drip_ack: bool, mut suppress: bool) {
    dialog_title(ui, if drip_feed { "Nachladebetrieb bestätigen" } else { "Übertragung bestätigen" });
    let simulation = app.simulation();
    mode_banner(ui, simulation, &app.port_label());
    ui.add_space(8.0);
    let Some(program) = app.program.clone() else {
        app.push(Action::CloseDialog);
        return;
    };
    let Some(payload) = program.payload.clone() else {
        app.push(Action::CloseDialog);
        return;
    };
    let verified = app.profile.verification_status == VerificationStatus::Verified;
    fields(
        ui,
        "confirm",
        &[
            ("Datei", program.file_name()),
            ("Transferdaten", format!("{} Bytes, {} Zeilen", payload.data.len(), payload.line_count)),
            (
                "Profil",
                format!(
                    "{} ({})",
                    app.profile.display_name,
                    if verified { "verifiziert" } else { "NICHT verifiziert" }
                ),
            ),
            ("Schnittstelle", format!("{} | {}", app.port_label(), app.profile.serial.summary())),
            ("Protokoll", app.profile.protocol.mode.label().to_string()),
        ],
    );
    for note in &payload.applied {
        ui.label(RichText::new(format!("Aufbereitung: {note}")).color(DIM));
    }
    if payload.appended_program_end {
        message(ui, Tone::Warn, "Programmende wird auf Ihre Bestätigung hin ergänzt.", |_| {});
    }
    let warnings: Vec<String> = program.analysis.warnings().map(|w| w.message.clone()).collect();
    if !warnings.is_empty() {
        message(ui, Tone::Warn, "Bestätigte Warnungen", |ui| {
            for warning in &warnings {
                ui.label(format!("- {warning}"));
            }
        });
    }
    ui.label(RichText::new(SCOPE_NOTICE).color(DIM));
    ui.add_space(4.0);
    if drip_feed {
        message(ui, Tone::Error, "Nachladebetrieb", |ui| {
            ui.label(DRIP_FEED_NOTICE);
            if ui
                .checkbox(
                    &mut drip_ack,
                    "Ich habe verstanden: Die Maschine muss bei Störungen an der CNC angehalten werden.",
                )
                .changed()
            {
                app.dialog = Some(Dialog::ConfirmSend { drip_feed, drip_ack, suppress });
            }
        });
    }
    let show_notice = app.config.show_safety_notice && !simulation;
    if show_notice {
        message(ui, Tone::Warn, "Sicherheitshinweis", |ui| {
            ui.label(SAFETY_NOTICE);
            if ui.checkbox(&mut suppress, "Sicherheitshinweis künftig nicht mehr anzeigen").changed() {
                app.dialog = Some(Dialog::ConfirmSend { drip_feed, drip_ack, suppress });
            }
        });
    }
    let label = if drip_feed {
        "NACHLADEBETRIEB STARTEN"
    } else if simulation {
        "SIMULATION STARTEN"
    } else {
        "ÜBERTRAGUNG STARTEN"
    };
    let (cancelled, confirmed) = dialog_keys(ui, "ABBRUCH", label, !drip_feed || drip_ack, mode_fill(simulation));
    if cancelled {
        app.push(Action::CloseConfirm);
    }
    if confirmed {
        app.push(Action::ConfirmSend { suppress_safety_notice: show_notice && suppress });
    }
}

fn param_unlock(app: &mut DeckelApp, ui: &mut Ui) {
    dialog_title(ui, "Parameter schreiben freischalten");
    message(ui, Tone::Error, "Maschinenparameter", |ui| {
        ui.label(PARAMETER_WRITE_NOTICE);
    });
    ui.label("Schreibfunktion für diese Sitzung freischalten?");
    let (cancelled, confirmed) = dialog_keys(ui, "NEIN", "JA, FREISCHALTEN", true, None);
    if cancelled {
        app.push(Action::CloseDialog);
    }
    if confirmed {
        app.push(Action::ParamConfirmUnlock);
    }
}

fn param_write(app: &mut DeckelApp, ui: &mut Ui, mut phrase: String, mut ack: bool) {
    dialog_title(ui, "Maschinenparameter schreiben");
    mode_banner(ui, app.simulation(), &app.port_label());
    ui.add_space(8.0);
    let gate = app.parameter_gate();
    if let (Some(candidate), Some(backup)) = (&app.param.candidate, &app.param.backup) {
        fields(
            ui,
            "param-write",
            &[
                (
                    "Datei",
                    format!(
                        "{} (SHA-256 {}…)",
                        candidate.name,
                        candidate.analysis.sha256.get(..16).unwrap_or_default()
                    ),
                ),
                ("Sicherung", backup.path_name.clone()),
                ("Änderungen", gate.diff.as_ref().map(ParameterDiff::summary).unwrap_or_default()),
                (
                    "Anschluss",
                    format!(
                        "{} | {} | {}",
                        app.port_label(),
                        app.profile.display_name,
                        app.profile.protocol.mode.label()
                    ),
                ),
            ],
        );
    }
    message(ui, Tone::Error, "Maschinenparameter", |ui| {
        ui.label(PARAMETER_WRITE_NOTICE);
    });
    ui.label("Vorher: Parametereingabe an der CNC freigeben (Schlüsselschalter/Schlüsselzahl) und Einlesen starten.");
    let mut changed =
        ui.checkbox(&mut ack, "Ich habe die Änderungen geprüft und bin zum Schreiben berechtigt.").changed();
    ui.label(format!("Zur Bestätigung eingeben: {WRITE_CONFIRMATION_PHRASE}"));
    changed |= ui.add(egui::TextEdit::singleline(&mut phrase).desired_width(280.0)).changed();
    if changed {
        app.dialog = Some(Dialog::ParamWrite { phrase: phrase.clone(), ack });
    }
    let ready = gate.allowed && ack && phrase.trim() == WRITE_CONFIRMATION_PHRASE;
    let (cancelled, confirmed) = dialog_keys(ui, "ABBRUCH", "PARAMETER SCHREIBEN", ready, mode_fill(app.simulation()));
    if cancelled {
        app.push(Action::CloseDialog);
    }
    if confirmed {
        app.push(Action::ParamConfirmWrite);
    }
}

fn real_mode(app: &mut DeckelApp, ui: &mut Ui) {
    dialog_title(ui, "Realbetrieb aktivieren");
    mode_banner(ui, false, &crate::logic::real_port_label(&app.config));
    ui.add_space(8.0);
    ui.label("Im Realbetrieb gehen Übertragungen an die konfigurierte Schnittstelle der Maschine. Jede Übertragung muss weiterhin unmittelbar vorher bestätigt werden.");
    ui.label(RichText::new(SCOPE_NOTICE).color(DIM));
    let (cancelled, confirmed) = dialog_keys(ui, "ABBRUCH", "REALBETRIEB AKTIVIEREN", true, mode_fill(false));
    if cancelled {
        app.push(Action::CloseDialog);
    }
    if confirmed {
        app.push(Action::ConfirmRealMode);
    }
}
