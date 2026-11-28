//! Drawing code. Records [`Action`]s; never performs I/O or long operations itself.

use egui::{Color32, RichText, ScrollArea, Ui};

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
    card, icon_text, icons, key_values, mode_badge, mode_button, notice, page_header, pill, primary_button, semibold,
    status_dot, Palette, Tone,
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

pub fn status_color(ui: &Ui, kind: StatusKind) -> Color32 {
    let p = Palette::of(ui);
    match kind {
        StatusKind::Ready => p.ok,
        StatusKind::Warning => p.warn,
        StatusKind::Error => p.error,
        StatusKind::Active => p.info,
        StatusKind::Idle => p.weak,
    }
}

fn severity_tone(severity: Severity) -> Tone {
    match severity {
        Severity::Error => Tone::Error,
        Severity::Warning => Tone::Warn,
        Severity::Info => Tone::Info,
    }
}

fn status_line(ui: &mut Ui, kind: StatusKind, text: &str) {
    let color = status_color(ui, kind);
    status_dot(ui, color, text);
}

pub fn page_icon(page: Page) -> egui_material_icons::MaterialIcon {
    match page {
        Page::Transfer => icons::ICON_UPLOAD,
        Page::Receive => icons::ICON_DOWNLOAD,
        Page::Programs => icons::ICON_INVENTORY_2,
        Page::Parameters => icons::ICON_TUNE,
        Page::Terminal => icons::ICON_TERMINAL,
        Page::Diagnostics => icons::ICON_MONITORING,
        Page::Settings => icons::ICON_SETTINGS,
        Page::Log => icons::ICON_HISTORY,
        Page::Commissioning => icons::ICON_CHECKLIST,
        Page::Help => icons::ICON_MENU_BOOK,
    }
}

pub fn error_panel(ui: &mut Ui, error: &UserFacingError) {
    notice(ui, Tone::Error, &error.title, |ui| {
        let p = Palette::of(ui);
        if !error.causes.is_empty() {
            ui.label(RichText::new("Mögliche Ursachen").color(p.weak));
            for cause in &error.causes {
                ui.label(format!("•  {cause}"));
            }
        }
        if !error.technical_details.is_empty() {
            egui::CollapsingHeader::new("Technische Details").id_salt(("error-details", &error.title)).show(ui, |ui| {
                ui.label(RichText::new(&error.technical_details).monospace());
            });
        }
    });
}

fn findings_list(ui: &mut Ui, findings: &[Finding]) {
    let p = Palette::of(ui);
    if findings.is_empty() {
        ui.label(RichText::new(icon_text(icons::ICON_CHECK_CIRCLE, "Keine Auffälligkeiten")).color(p.ok));
        return;
    }
    for finding in findings {
        let tone = severity_tone(finding.severity);
        let color = tone.color(&p);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tone.icon().codepoint).color(color));
            pill(ui, finding.severity.label(), color);
            ui.label(&finding.message);
            let lines = finding.lines_text();
            if !lines.is_empty() {
                ui.label(RichText::new(format!("Zeilen {lines}")).small().color(p.weak));
            }
        });
    }
}

/// Whole window for one frame.
pub fn show(app: &mut DeckelApp, ui: &mut Ui) {
    let p = Palette::of(ui);
    egui::Panel::top("header")
        .frame(
            egui::Frame::new()
                .fill(p.header)
                .inner_margin(egui::Margin::symmetric(18, 10))
                .stroke(egui::Stroke::new(1.0, p.border)),
        )
        .show(ui, |ui| header(app, ui));
    egui::Panel::bottom("statusbar")
        .frame(
            egui::Frame::new()
                .fill(p.header)
                .inner_margin(egui::Margin::symmetric(16, 6))
                .stroke(egui::Stroke::new(1.0, p.border)),
        )
        .show(ui, |ui| statusbar(app, ui));
    egui::Panel::left("nav")
        .resizable(false)
        .exact_size(224.0)
        .frame(
            egui::Frame::new()
                .fill(p.sidebar)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .stroke(egui::Stroke::new(1.0, p.border)),
        )
        .show(ui, |ui| navigation(app, ui));
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::symmetric(24, 16)))
        .show(ui, |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(1100.0));
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
    dialogs(app, ui.ctx());
    crate::ui_extra::dropped_files(app, ui.ctx());
}

fn logo(ui: &mut Ui) {
    let p = Palette::of(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::same(10), p.accent);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icons::ICON_PRECISION_MANUFACTURING.codepoint,
        egui::FontId::proportional(24.0),
        p.accent_text,
    );
}

fn header(app: &mut DeckelApp, ui: &mut Ui) {
    let p = Palette::of(ui);
    ui.horizontal(|ui| {
        logo(ui);
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.label(semibold("DeckelLink", 19.0).color(p.text));
            ui.label(RichText::new("DECKEL FP2A  ·  DIALOG 11").size(12.0).color(p.weak));
        });
        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);
        let info_width = (ui.available_width() - crate::theme::BADGE_WIDTH - 16.0).max(160.0);
        ui.allocate_ui_with_layout(egui::vec2(info_width, 56.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            let (kind, text) = app.status.clone();
            let color = status_color(ui, kind);
            status_dot(ui, color, &text);
            ui.horizontal(|ui| {
                let port = app.port_label();
                let port = if port.is_empty() { "(kein Port)".to_string() } else { port };
                let serial = &app.profile.serial;
                let lsv2 = if app.profile.protocol.is_lsv2() { " · LSV2" } else { "" };
                let verified = app.profile.verification_status == VerificationStatus::Verified;
                let (color, status) = if verified { (p.ok, "verifiziert") } else { (p.warn, "nicht verifiziert") };
                if info_width > 560.0 {
                    pill(
                        ui,
                        &icon_text(
                            icons::ICON_CABLE,
                            &format!("{port}  ·  {} Bd {}{lsv2}", serial.baud_rate, serial.frame()),
                        ),
                        p.weak,
                    );
                }
                let profile = if info_width > 760.0 {
                    format!("{}  ·  {status}", app.profile.display_name)
                } else {
                    status.to_string()
                };
                pill(
                    ui,
                    &icon_text(if verified { icons::ICON_VERIFIED } else { icons::ICON_WARNING }, &profile),
                    color,
                )
                .on_hover_text(format!("Profil: {}", app.profile.display_name));
            });
        });
        ui.add_space(ui.available_width() - crate::theme::BADGE_WIDTH);
        mode_badge(ui, app.simulation(), &app.port_label());
    });
}

fn statusbar(app: &mut DeckelApp, ui: &mut Ui) {
    let p = Palette::of(ui);
    ui.horizontal(|ui| {
        if let Some(task) = &app.task {
            ui.spinner();
            ui.label(format!("{} läuft …", task.kind.label()));
            if ui.button(icon_text(icons::ICON_STOP_CIRCLE, "Abbrechen")).clicked() {
                app.push(Action::Cancel);
            }
            ui.separator();
        }
        if let Some(message) = app.message.clone() {
            ui.label(RichText::new(icons::ICON_INFO.codepoint).color(p.info));
            ui.label(message);
            if ui.small_button(icons::ICON_CANCEL.codepoint).on_hover_text("Meldung ausblenden").clicked() {
                app.message = None;
            }
        }
        if let Some(error) = &app.log.file_error {
            ui.label(RichText::new(error).color(p.warn));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format!("DeckelLink {}", env!("CARGO_PKG_VERSION"))).small().color(p.weak));
        });
    });
}

fn navigation(app: &mut DeckelApp, ui: &mut Ui) {
    let groups: [(&str, &[Page]); 3] = [
        ("Übertragung", &[Page::Transfer, Page::Receive, Page::Programs, Page::Parameters]),
        ("Analyse", &[Page::Terminal, Page::Diagnostics, Page::Log]),
        ("System", &[Page::Settings, Page::Commissioning, Page::Help]),
    ];
    for (title, pages) in groups {
        crate::theme::group_label(ui, title);
        for page in pages {
            if crate::theme::nav_item(ui, page_icon(*page), page.label(), app.page == *page).clicked() {
                app.push(Action::Navigate(*page));
            }
        }
    }
}

fn gate_hint(ui: &mut Ui, reasons: &[String]) {
    if reasons.is_empty() {
        return;
    }
    let p = Palette::of(ui);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(icons::ICON_LOCK.codepoint).color(p.warn));
        ui.label(RichText::new(reasons.join("  ·  ")).color(p.warn));
    });
}

fn progress_bar(ui: &mut Ui, percent: f64) {
    let p = Palette::of(ui);
    #[allow(clippy::cast_possible_truncation)]
    let fraction = (percent / 100.0) as f32;
    ui.add(egui::ProgressBar::new(fraction).show_percentage().fill(p.accent).desired_height(22.0).corner_radius(6.0));
}

fn transfer_page(app: &mut DeckelApp, ui: &mut Ui) {
    page_header(
        ui,
        icons::ICON_UPLOAD,
        "NC-Programm übertragen",
        "Datei prüfen, aufbereiten und nach Bestätigung an die CNC senden. Die Originaldatei wird nie verändert.",
    );
    let idle = !app.busy();
    card(ui, Some(icons::ICON_FILE_OPEN), "Datei", |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(idle, egui::Button::new(icon_text(icons::ICON_FILE_OPEN, "Datei öffnen …"))).clicked()
            {
                app.push(Action::OpenFileDialog);
            }
            if ui
                .add_enabled(
                    idle && app.program.is_some(),
                    egui::Button::new(icon_text(icons::ICON_REFRESH, "Erneut prüfen")),
                )
                .clicked()
            {
                app.push(Action::Recheck);
            }
            if app.simulation()
                && ui.add_enabled(idle, egui::Button::new(icon_text(icons::ICON_SCIENCE, "Beispielprogramm"))).clicked()
            {
                app.push(Action::LoadSample);
            }
            if !app.config.recent_files.is_empty() {
                egui::ComboBox::from_id_salt("recent")
                    .selected_text(icon_text(icons::ICON_HISTORY, "Zuletzt geöffnet"))
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for path in app.config.recent_files.clone() {
                            if ui.add_enabled(idle, egui::Button::selectable(false, &path)).clicked() {
                                app.push(Action::LoadFile(path.into()));
                            }
                        }
                    });
            }
        });
        ui.add_space(6.0);
        match app.program.clone() {
            None => {
                ui.label(
                    RichText::new("Keine Datei geladen - Datei öffnen oder auf das Fenster ziehen.")
                        .color(Palette::of(ui).weak),
                );
            }
            Some(program) => {
                key_values(
                    ui,
                    "file-info",
                    &[
                        ("Datei", program.file_name()),
                        ("Pfad", program.path.display().to_string()),
                        (
                            "Größe",
                            format!(
                                "{}  ·  {} Zeilen  ·  längste Zeile {} Zeichen",
                                format_bytes(program.original.len()),
                                program.analysis.line_count,
                                program.analysis.longest_line
                            ),
                        ),
                        (
                            "Programmende",
                            if program.analysis.has_program_end { "gefunden".into() } else { "NICHT gefunden".into() },
                        ),
                        ("SHA-256", program.analysis.sha256.get(..16).unwrap_or_default().to_string()),
                    ],
                );
            }
        }
    });
    if let Some(error) = app.send_error.clone() {
        error_panel(ui, &error);
    }
    let Some(program) = app.program.clone() else { return };
    card(ui, Some(icons::ICON_FACT_CHECK), "Prüfung", |ui| {
        findings_list(ui, &program.analysis.findings);
        if program.analysis.has_warnings() {
            let mut ack = app.ack_warnings;
            if ui.checkbox(&mut ack, "Warnungen gelesen und bestätigt").changed() {
                app.ack_warnings = ack;
            }
        }
        if app.profile.transfer.append_program_end && !program.analysis.has_program_end {
            let mut append = app.append_end;
            if ui.checkbox(&mut append, "Fehlendes Programmende beim Senden ergänzen (nur diese Übertragung)").changed()
            {
                app.push(Action::SetAppendEnd(append));
            }
        }
        if let Some(err) = &program.transform_error {
            ui.label(RichText::new(format!("Aufbereitung: {err}")).color(Palette::of(ui).error));
        }
        if let Some(payload) = &program.payload {
            egui::CollapsingHeader::new(format!(
                "Transferdaten: {}  ·  {} Zeilen  (Vorschau)",
                format_bytes(payload.data.len()),
                payload.line_count
            ))
            .show(ui, |ui| {
                for note in &payload.applied {
                    ui.label(format!("•  {note}"));
                }
                egui::Frame::new()
                    .fill(Palette::of(ui).input)
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ScrollArea::vertical().max_height(260.0).id_salt("preview").show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.monospace(preview_text(&payload.data, 400));
                        });
                    });
            });
        }
    });
    card(ui, Some(icons::ICON_UPLOAD), "Übertragung", |ui| {
        let gate = app.send_gate();
        ui.horizontal_wrapped(|ui| {
            let label = if app.simulation() {
                icon_text(icons::ICON_PLAY_CIRCLE, "Simulation starten")
            } else {
                icon_text(icons::ICON_UPLOAD, "An Maschine senden")
            };
            if ui.add_enabled(gate.allowed, mode_button(&label, app.simulation())).clicked() {
                app.push(Action::RequestSend { drip_feed: false });
            }
            if app.profile.protocol.is_lsv2()
                && ui
                    .add_enabled(
                        gate.allowed,
                        egui::Button::new(icon_text(icons::ICON_SYNC, "Nachladebetrieb (LSV2) …"))
                            .min_size(egui::vec2(0.0, 36.0)),
                    )
                    .clicked()
            {
                app.push(Action::RequestSend { drip_feed: true });
            }
            if app.busy()
                && ui
                    .add(
                        egui::Button::new(icon_text(icons::ICON_STOP_CIRCLE, "Abbrechen"))
                            .min_size(egui::vec2(0.0, 36.0)),
                    )
                    .clicked()
            {
                app.push(Action::Cancel);
            }
        });
        gate_hint(ui, &gate.reasons);
        ui.add_space(4.0);
        status_line(ui, state_kind(app.send_state), app.send_state.label());
        if let Some(progress) = &app.send_progress {
            progress_bar(ui, progress.percent());
            ui.label(
                RichText::new(format!(
                    "{} von {}  ·  Zeile {} von {}  ·  {}",
                    format_bytes(progress.bytes_done),
                    format_bytes(progress.bytes_total),
                    progress.current_line,
                    progress.total_lines,
                    format_duration(progress.elapsed_s)
                ))
                .color(Palette::of(ui).weak),
            );
        }
    });
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

fn receive_page(app: &mut DeckelApp, ui: &mut Ui) {
    page_header(
        ui,
        icons::ICON_DOWNLOAD,
        "Programm von der CNC empfangen",
        "Empfang hier starten, dann die Programmausgabe an der CNC auslösen. Empfangene Programme werden als neue Datei gespeichert - nie überschrieben.",
    );
    card(ui, Some(icons::ICON_DOWNLOAD), "Empfang", |ui| {
        let gate = app.receive_gate();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(gate.allowed, primary_button(ui, &icon_text(icons::ICON_DOWNLOAD, "Empfang starten")))
                .clicked()
            {
                app.push(Action::StartReceive);
            }
            if app.busy()
                && ui
                    .add(
                        egui::Button::new(icon_text(icons::ICON_STOP_CIRCLE, "Abbrechen"))
                            .min_size(egui::vec2(0.0, 34.0)),
                    )
                    .clicked()
            {
                app.push(Action::Cancel);
            }
        });
        gate_hint(ui, &gate.reasons);
        ui.add_space(4.0);
        status_line(ui, StatusKind::Active, app.receive_state.label());
        if let Some(progress) = &app.receive_progress {
            ui.label(
                RichText::new(format!(
                    "{} empfangen  ·  Dauer {}  ·  seit {:.1} s keine Daten",
                    format_bytes(progress.bytes_received),
                    format_duration(progress.elapsed_s),
                    progress.idle_s
                ))
                .color(Palette::of(ui).weak),
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
        notice(ui, tone, &text, |ui| {
            ui.label(format!(
                "{}  ·  {}  ·  {}",
                summary.state.label(),
                summary.end_reason,
                format_bytes(summary.bytes)
            ));
            if let Some(path) = &summary.stored_path {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Gespeichert:");
                    ui.monospace(path.display().to_string());
                });
                if ui
                    .add_enabled(!app.busy(), egui::Button::new(icon_text(icons::ICON_UPLOAD, "Im Übertragen öffnen")))
                    .clicked()
                {
                    app.push(Action::LoadArchived(path.clone()));
                }
            }
            for note in &summary.notes {
                ui.label(format!("•  {note}"));
            }
            for finding in &summary.findings {
                ui.label(finding);
            }
        });
    }
}

fn programs_page(app: &mut DeckelApp, ui: &mut Ui) {
    page_header(
        ui,
        icons::ICON_INVENTORY_2,
        "Empfangene NC-Programme",
        &format!("Ordner: {}", app.paths.received.display()),
    );
    card(ui, None, "", |ui| {
        ui.horizontal(|ui| {
            if ui.button(icon_text(icons::ICON_REFRESH, "Aktualisieren")).clicked() {
                app.push(Action::RefreshArchive);
            }
            ui.label(RichText::new(format!("{} Programme", app.archive.len())).color(Palette::of(ui).weak));
        });
        ui.add_space(6.0);
        if app.archive.is_empty() {
            ui.label(RichText::new("Noch keine Programme empfangen.").color(Palette::of(ui).weak));
            return;
        }
        let p = Palette::of(ui);
        egui::Grid::new("archive").striped(true).num_columns(5).spacing([18.0, 8.0]).show(ui, |ui| {
            for title in ["Datei", "Status", "Größe", "Geändert", ""] {
                ui.label(RichText::new(title).small().color(p.weak));
            }
            ui.end_row();
            for entry in app.archive.clone() {
                ui.label(entry.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                match (entry.folder, entry.complete()) {
                    (_, Some(true)) => pill(ui, "vollständig", p.ok),
                    ("failed", _) | (_, Some(false)) => pill(ui, "UNVOLLSTÄNDIG", p.warn),
                    _ => pill(ui, "ohne Metadaten", p.weak),
                };
                ui.label(format_bytes(usize::try_from(entry.size).unwrap_or(usize::MAX)));
                ui.label(entry.modified.map(|m| m.format("%d.%m.%Y %H:%M").to_string()).unwrap_or_default());
                if ui
                    .add_enabled(!app.busy(), egui::Button::new(icon_text(icons::ICON_UPLOAD, "Zum Senden laden")))
                    .clicked()
                {
                    app.push(Action::LoadArchived(entry.path.clone()));
                }
                ui.end_row();
            }
        });
    });
}

fn diff_view(ui: &mut Ui, title: &str, diff: &ParameterDiff) {
    let p = Palette::of(ui);
    ui.label(semibold(format!("Vergleich {title}"), 14.0));
    ui.label(RichText::new(diff.summary()).color(p.weak));
    if diff.diff_text.is_empty() {
        return;
    }
    egui::Frame::new()
        .fill(p.input)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ScrollArea::vertical().max_height(280.0).id_salt("diff").show(ui, |ui| {
                ui.set_width(ui.available_width());
                for line in diff.diff_text.lines() {
                    let color = if line.starts_with('+') && !line.starts_with("+++") {
                        p.ok
                    } else if line.starts_with('-') && !line.starts_with("---") {
                        p.error
                    } else {
                        p.weak
                    };
                    ui.label(RichText::new(line).monospace().color(color));
                }
            });
        });
}

fn parameters_page(app: &mut DeckelApp, ui: &mut Ui) {
    page_header(
        ui,
        icons::ICON_TUNE,
        "Maschinenparameter",
        "Nur als Datei: Auslesen nach Start der Parameterausgabe an der CNC, Schreiben nur in eine an der CNC freigegebene Parametereingabe. Format der DIALOG 11 nicht verifiziert.",
    );
    card(ui, Some(icons::ICON_DOWNLOAD), "1  ·  Aktuelle Sicherung auslesen", |ui| {
        let read_gate = app.receive_gate();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    read_gate.allowed,
                    primary_button(ui, &icon_text(icons::ICON_DOWNLOAD, "Parameter auslesen")),
                )
                .clicked()
            {
                app.push(Action::ParamRead);
            }
            if app.busy() && ui.button(icon_text(icons::ICON_STOP_CIRCLE, "Abbrechen")).clicked() {
                app.push(Action::Cancel);
            }
            if app.param.received_bytes > 0 && app.busy() {
                ui.label(format!("{} empfangen", format_bytes(app.param.received_bytes)));
            }
        });
        if let Some((kind, text)) = app.param.status.clone() {
            status_line(ui, kind, &text);
        }
        match &app.param.backup {
            Some(backup) => {
                let summary = backup.structure().map(|s| s.summary()).unwrap_or_default();
                key_values(
                    ui,
                    "backup-info",
                    &[
                        ("Sicherung", backup.path_name.clone()),
                        ("Größe", format_bytes(backup.data.len())),
                        ("SHA-256", format!("{}…", backup.sha256().get(..16).unwrap_or_default())),
                        ("Ausgelesen", backup.read_at.format("%H:%M:%S").to_string()),
                        ("Struktur", summary),
                    ],
                );
            }
            None => {
                ui.label(RichText::new("Noch keine Sicherung in dieser Sitzung.").color(Palette::of(ui).weak));
            }
        }
        if let Some((ok, text)) = app.param.verify.clone() {
            notice(ui, if ok { Tone::Ok } else { Tone::Error }, &text, |_| {});
        }
        egui::CollapsingHeader::new(format!("Gespeicherte Parametersätze ({})", app.param.sets.len())).show(ui, |ui| {
            egui::Grid::new("param-sets").striped(true).num_columns(4).spacing([14.0, 6.0]).show(ui, |ui| {
                for set in app.param.sets.clone() {
                    let p = Palette::of(ui);
                    ui.label(set.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                    if set.complete() {
                        pill(ui, "vollständig", p.ok);
                    } else {
                        pill(ui, "UNVOLLSTÄNDIG", p.warn);
                    }
                    if ui.button("Mit Sicherung vergleichen").clicked() {
                        app.push(Action::ParamCompare(set.path.clone()));
                    }
                    if ui.add_enabled(!app.busy(), egui::Button::new("Zum Schreiben wählen")).clicked() {
                        app.push(Action::ParamSelectFile(set.path.clone()));
                    }
                    ui.end_row();
                }
            });
        });
    });
    if let Some(error) = app.param.error.clone() {
        error_panel(ui, &error);
    }
    card(ui, Some(icons::ICON_DESCRIPTION), "2  ·  Parameterdatei zum Schreiben wählen", |ui| {
        if ui.add_enabled(!app.busy(), egui::Button::new(icon_text(icons::ICON_FILE_OPEN, "Datei wählen …"))).clicked()
        {
            app.push(Action::ParamOpenFileDialog);
        }
        if let Some(candidate) = app.param.candidate.clone() {
            key_values(
                ui,
                "candidate",
                &[
                    ("Datei", candidate.name.clone()),
                    ("Größe", format_bytes(candidate.data.len())),
                    ("SHA-256", format!("{}…", candidate.analysis.sha256.get(..16).unwrap_or_default())),
                    ("Struktur", candidate.structure().map(|s| s.summary()).unwrap_or_default()),
                ],
            );
            findings_list(ui, &candidate.analysis.findings);
        }
        if let Some((title, diff)) = app.param.diff.clone() {
            ui.add_space(6.0);
            diff_view(ui, &title, &diff);
        }
    });
    card(ui, Some(icons::ICON_LOCK), "3  ·  Schreiben (gesperrt, bis alle Bedingungen erfüllt sind)", |ui| {
        let mut unlocked = app.param.unlocked;
        if ui
            .add_enabled(
                !app.busy(),
                egui::Checkbox::new(&mut unlocked, "Schreibfunktion für diese Sitzung freischalten"),
            )
            .on_hover_text("Wird beim Beenden von DeckelLink automatisch zurückgesetzt.")
            .changed()
        {
            app.push(if unlocked { Action::ParamRequestUnlock } else { Action::ParamLock });
        }
        let gate = app.parameter_gate();
        if ui
            .add_enabled(
                gate.allowed,
                mode_button(&icon_text(icons::ICON_SAVE, "Parameter schreiben …"), app.simulation()),
            )
            .clicked()
        {
            app.push(Action::ParamRequestWrite);
        }
        gate_hint(ui, &gate.reasons);
        if let Some(progress) = &app.param.progress {
            progress_bar(ui, progress.percent());
        }
    });
}

fn terminal_page(app: &mut DeckelApp, ui: &mut Ui) {
    page_header(
        ui,
        icons::ICON_TERMINAL,
        "Terminal",
        "Übertragene Bytes dieser Sitzung. In die Protokolldatei gelangen Rohdaten nur bei aktiviertem Rohdatenprotokoll.",
    );
    let p = Palette::of(ui);
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.terminal_hex, "Hexadezimal");
        if ui.button(icon_text(icons::ICON_DELETE, "Leeren")).clicked() {
            app.push(Action::ClearTerminal);
        }
        ui.label(
            RichText::new(format!("{} Einträge (max. {})", app.traffic.len(), app.config.terminal_history_lines))
                .color(p.weak),
        );
    });
    ui.add_space(6.0);
    let console =
        if ui.visuals().dark_mode { Color32::from_rgb(0x09, 0x0c, 0x10) } else { Color32::from_rgb(0x1b, 0x23, 0x2d) };
    egui::Frame::new()
        .fill(console)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .max_height(560.0)
                .id_salt("terminal")
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if app.traffic.is_empty() {
                        ui.label(
                            RichText::new("Noch keine Daten.").monospace().color(Color32::from_rgb(0x8c, 0x98, 0xa6)),
                        );
                    }
                    for event in &app.traffic {
                        let (dir, color) = match event.direction {
                            Direction::Tx => ("TX", Color32::from_rgb(0x58, 0xa6, 0xff)),
                            Direction::Rx => ("RX", Color32::from_rgb(0x3f, 0xb9, 0x50)),
                        };
                        let text = if app.terminal_hex { hex_line(&event.data) } else { ascii_line(&event.data) };
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(event.timestamp.format("%H:%M:%S%.3f").to_string())
                                    .monospace()
                                    .color(Color32::from_rgb(0x6e, 0x7a, 0x88)),
                            );
                            ui.label(RichText::new(dir).monospace().color(color));
                            ui.label(RichText::new(text).monospace().color(Color32::from_rgb(0xe3, 0xe8, 0xee)));
                        });
                    }
                });
        });
}

fn line_pill(ui: &mut Ui, name: &str, value: Option<bool>) {
    let p = Palette::of(ui);
    match value {
        Some(true) => pill(ui, &format!("{name}  EIN"), p.ok),
        Some(false) => pill(ui, &format!("{name}  aus"), p.weak),
        None => pill(ui, &format!("{name}  ?"), p.weak),
    };
}

fn diagnostics_page(app: &mut DeckelApp, ui: &mut Ui) {
    page_header(
        ui,
        icons::ICON_MONITORING,
        "Diagnose",
        "Anschlüsse, Leitungszustände, Loopback-Test, Datenformat und Diagnosepaket.",
    );
    card(ui, Some(icons::ICON_SETTINGS_INPUT_COMPONENT), "Serielle Anschlüsse", |ui| {
        if ui.button(icon_text(icons::ICON_REFRESH, "Suchen")).clicked() {
            app.push(Action::RefreshPorts);
        }
        if app.ports.is_empty() {
            ui.label(
                RichText::new("Keine seriellen Anschlüsse gefunden (oder noch nicht gesucht).")
                    .color(Palette::of(ui).weak),
            );
        }
        for port in &app.ports {
            ui.label(icon_text(icons::ICON_SETTINGS_INPUT_COMPONENT, &port.label()));
        }
    });
    card(ui, Some(icons::ICON_CABLE), "Modemleitungen (letzter Stand während einer Übertragung)", |ui| {
        match app.modem {
            Some(m) => {
                ui.horizontal_wrapped(|ui| {
                    line_pill(ui, "CTS", m.cts);
                    line_pill(ui, "DSR", m.dsr);
                    line_pill(ui, "DCD", m.dcd);
                    line_pill(ui, "RI", m.ri);
                    line_pill(ui, "RTS", m.rts);
                    line_pill(ui, "DTR", m.dtr);
                });
            }
            None => {
                ui.label(
                    RichText::new("Noch keine Werte - werden während einer Übertragung angezeigt.")
                        .color(Palette::of(ui).weak),
                );
            }
        }
    });
    crate::ui_extra::diagnostics_tools(app, ui);
    card(ui, Some(icons::ICON_INFO), "Programm und Profil", |ui| {
        key_values(
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
            notice(ui, Tone::Warn, &warning, |_| {});
        }
        for problem in app.store.problems.clone() {
            notice(ui, Tone::Error, &format!("Profilproblem: {problem}"), |_| {});
        }
    });
}

fn settings_page(app: &mut DeckelApp, ui: &mut Ui) {
    page_header(ui, icons::ICON_SETTINGS, "Einstellungen", "Betriebsart, Verbindung, Profil und Darstellung.");
    card(ui, Some(icons::ICON_SHIELD), "Betriebsart", |ui| {
        let idle = !app.busy();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    idle && !app.simulation(),
                    egui::Button::selectable(app.simulation(), icon_text(icons::ICON_SCIENCE, "Simulation")),
                )
                .clicked()
            {
                app.push(Action::SetSimulation);
            }
            if ui
                .add_enabled(
                    idle && app.simulation(),
                    egui::Button::selectable(!app.simulation(), icon_text(icons::ICON_BOLT, "Realbetrieb …")),
                )
                .clicked()
            {
                app.push(Action::RequestRealMode);
            }
        });
        if app.simulation() {
            ui.add_space(4.0);
            let current = get_scenario(&app.config.simulation_scenario);
            ui.horizontal(|ui| {
                ui.label("Szenario");
                egui::ComboBox::from_id_salt("scenario").selected_text(current.title).width(420.0).show_ui(ui, |ui| {
                    for scenario in scenarios() {
                        if ui.selectable_label(scenario.id == current.id, scenario.title).clicked() {
                            app.config.simulation_scenario = scenario.id.into();
                            app.push(Action::SaveConfig);
                        }
                    }
                });
            });
            ui.label(
                RichText::new(format!("{} ({})", current.description, current.kind.label()))
                    .small()
                    .color(Palette::of(ui).weak),
            );
            crate::ui_extra::replay_selection(app, ui);
        }
    });
    card(ui, Some(icons::ICON_CABLE), "Verbindung für den Realbetrieb", |ui| {
        ui.add_enabled_ui(!app.busy(), |ui| {
            let mut changed = false;
            egui::Grid::new("connection").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                ui.label("Verbindungsart");
                egui::ComboBox::from_id_salt("conn-kind")
                    .selected_text(app.config.connection_kind.label())
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        for kind in ConnectionKind::ALL {
                            changed |=
                                ui.selectable_value(&mut app.config.connection_kind, kind, kind.label()).changed();
                        }
                    });
                ui.end_row();
                match app.config.connection_kind {
                    ConnectionKind::Serial => {
                        ui.label("COM-Port");
                        ui.horizontal(|ui| {
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut app.config.port)
                                        .desired_width(160.0)
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
                            if ui.button(icon_text(icons::ICON_REFRESH, "Suchen")).clicked() {
                                app.push(Action::RefreshPorts);
                            }
                        });
                        ui.end_row();
                    }
                    ConnectionKind::Tcp | ConnectionKind::Rfc2217 => {
                        ui.label("Adapter (IP/Name)");
                        changed |= ui
                            .add(egui::TextEdit::singleline(&mut app.config.network_host).desired_width(220.0))
                            .lost_focus();
                        ui.end_row();
                        ui.label("TCP-Port");
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
    card(ui, Some(icons::ICON_DESCRIPTION), "Profil", |ui| {
        ui.add_enabled_ui(!app.busy(), |ui| {
            egui::ComboBox::from_id_salt("profile")
                .selected_text(app.profile.display_name.clone())
                .width(460.0)
                .show_ui(ui, |ui| {
                    for profile in app.store.profiles().to_vec() {
                        if ui.selectable_label(profile.id == app.profile.id, &profile.display_name).clicked() {
                            app.push(Action::SelectProfile(profile.id.clone()));
                        }
                    }
                });
        });
        let p = app.profile.clone();
        key_values(
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
            ui.label(RichText::new(&p.description).small().color(Palette::of(ui).weak));
        }
        ui.add_space(4.0);
        crate::ui_extra::profile_tools(app, ui);
    });
    card(ui, Some(icons::ICON_TUNE), "Darstellung und Protokoll", |ui| {
        let mut changed = false;
        let mut theme_changed = false;
        egui::Grid::new("display").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
            ui.label("Darstellung");
            ui.horizontal(|ui| {
                for (value, label) in [("dunkel", "Dunkel"), ("hell", "Hell"), ("system", "Wie System")] {
                    if ui.selectable_value(&mut app.config.ui_theme, value.to_string(), label).changed() {
                        theme_changed = true;
                    }
                }
            });
            ui.end_row();
            ui.label("Schriftgröße (%)");
            changed |= ui.add(egui::DragValue::new(&mut app.config.font_scale_percent).range(80..=200)).changed();
            ui.end_row();
            ui.label("Protokollstufe");
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
        if theme_changed {
            ui.ctx().set_theme(crate::theme::preference(&app.config.ui_theme));
        }
        if changed || theme_changed {
            app.push(Action::SaveConfig);
        }
        ui.label(
            RichText::new(format!("Datenordner: {}", app.paths.root.display())).small().color(Palette::of(ui).weak),
        );
    });
}

fn log_page(app: &mut DeckelApp, ui: &mut Ui) {
    page_header(
        ui,
        icons::ICON_HISTORY,
        "Protokoll",
        "Ereignisse dieser Sitzung. Es werden keine NC-Inhalte protokolliert.",
    );
    let p = Palette::of(ui);
    ui.horizontal(|ui| {
        ui.label("Anzeigen ab");
        for level in [Level::Debug, Level::Info, Level::Warning, Level::Error] {
            ui.selectable_value(&mut app.log_filter, level, level.as_str());
        }
    });
    ui.add_space(6.0);
    card(ui, None, "", |ui| {
        ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).max_height(600.0).id_salt("log").show(
            ui,
            |ui| {
                for entry in app.log.entries.iter().filter(|e| e.level >= app.log_filter) {
                    let color = match entry.level {
                        Level::Error => p.error,
                        Level::Warning => p.warn,
                        Level::Debug => p.weak,
                        Level::Info => p.text,
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&entry.time).monospace().color(p.weak));
                        ui.label(RichText::new(format!("{:<7}", entry.level.as_str())).monospace().color(color));
                        ui.label(RichText::new(&entry.message).color(color));
                    });
                }
            },
        );
    });
}

// ------------------------------------------------------------------------ dialogs

fn dialogs(app: &mut DeckelApp, ctx: &egui::Context) {
    let Some(dialog) = app.dialog.clone() else { return };
    let modal = egui::Modal::new(egui::Id::unique("deckellink-dialog")).show(ctx, |ui| {
        ui.set_max_width(660.0);
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

/// Dialog title row.
pub fn dialog_title(ui: &mut Ui, icon: egui_material_icons::MaterialIcon, title: &str) {
    let p = Palette::of(ui);
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon.codepoint).size(24.0).color(p.accent));
        ui.label(semibold(title, 19.0).color(p.text));
    });
    ui.add_space(8.0);
}

/// Dialog footer: secondary button left, primary on the right.
pub fn dialog_buttons(ui: &mut Ui, cancel: &str, add_primary: impl FnOnce(&mut Ui)) -> bool {
    ui.add_space(8.0);
    ui.separator();
    let mut cancelled = false;
    ui.horizontal(|ui| {
        cancelled = ui.add(egui::Button::new(cancel).min_size(egui::vec2(110.0, 34.0))).clicked();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add_primary);
    });
    cancelled
}

fn confirm_send(app: &mut DeckelApp, ui: &mut Ui, drip_feed: bool, mut drip_ack: bool, mut suppress: bool) {
    dialog_title(ui, icons::ICON_UPLOAD, "Übertragung bestätigen");
    let simulation = app.simulation();
    mode_badge(ui, simulation, &app.port_label());
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
    key_values(
        ui,
        "confirm",
        &[
            ("Datei", program.file_name()),
            ("Transferdaten", format!("{} Bytes  ·  {} Zeilen", payload.data.len(), payload.line_count)),
            (
                "Profil",
                format!(
                    "{} ({})",
                    app.profile.display_name,
                    if verified { "verifiziert" } else { "NICHT verifiziert" }
                ),
            ),
            ("Schnittstelle", format!("{}  ·  {}", app.port_label(), app.profile.serial.summary())),
            ("Protokoll", app.profile.protocol.mode.label().to_string()),
        ],
    );
    for note in &payload.applied {
        ui.label(RichText::new(format!("Aufbereitung: {note}")).small().color(Palette::of(ui).weak));
    }
    if payload.appended_program_end {
        notice(ui, Tone::Warn, "Programmende wird auf Ihre Bestätigung hin ergänzt.", |_| {});
    }
    let warnings: Vec<String> = program.analysis.warnings().map(|w| w.message.clone()).collect();
    if !warnings.is_empty() {
        notice(ui, Tone::Warn, "Bestätigte Warnungen", |ui| {
            for warning in &warnings {
                ui.label(format!("•  {warning}"));
            }
        });
    }
    notice(ui, Tone::Info, "Nur Datenübertragung", |ui| {
        ui.label(SCOPE_NOTICE);
    });
    if drip_feed {
        notice(ui, Tone::Error, "Nachladebetrieb", |ui| {
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
        notice(ui, Tone::Warn, "Sicherheitshinweis", |ui| {
            ui.label(SAFETY_NOTICE);
            if ui.checkbox(&mut suppress, "Sicherheitshinweis künftig nicht mehr anzeigen").changed() {
                app.dialog = Some(Dialog::ConfirmSend { drip_feed, drip_ack, suppress });
            }
        });
    }
    let cancelled = dialog_buttons(ui, "Abbrechen", |ui| {
        let label = if drip_feed {
            icon_text(icons::ICON_SYNC, "Nachladebetrieb starten")
        } else if simulation {
            icon_text(icons::ICON_PLAY_CIRCLE, "Simulation starten")
        } else {
            icon_text(icons::ICON_UPLOAD, "Übertragung starten")
        };
        if ui.add_enabled(!drip_feed || drip_ack, mode_button(&label, simulation)).clicked() {
            app.push(Action::ConfirmSend { suppress_safety_notice: show_notice && suppress });
        }
    });
    if cancelled {
        app.push(Action::CloseConfirm);
    }
}

fn param_unlock(app: &mut DeckelApp, ui: &mut Ui) {
    dialog_title(ui, icons::ICON_LOCK_OPEN, "Schreiben von Maschinenparametern freischalten");
    notice(ui, Tone::Error, "Maschinenparameter", |ui| {
        ui.label(PARAMETER_WRITE_NOTICE);
    });
    ui.label("Schreibfunktion für diese Sitzung freischalten?");
    let cancelled = dialog_buttons(ui, "Nein", |ui| {
        if ui.add(primary_button(ui, "Ja, für diese Sitzung freischalten")).clicked() {
            app.push(Action::ParamConfirmUnlock);
        }
    });
    if cancelled {
        app.push(Action::CloseDialog);
    }
}

fn param_write(app: &mut DeckelApp, ui: &mut Ui, mut phrase: String, mut ack: bool) {
    dialog_title(ui, icons::ICON_SAVE, "Maschinenparameter schreiben");
    mode_badge(ui, app.simulation(), &app.port_label());
    ui.add_space(8.0);
    let gate = app.parameter_gate();
    if let (Some(candidate), Some(backup)) = (&app.param.candidate, &app.param.backup) {
        key_values(
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
                        "{}  ·  {}  ·  {}",
                        app.port_label(),
                        app.profile.display_name,
                        app.profile.protocol.mode.label()
                    ),
                ),
            ],
        );
    }
    notice(ui, Tone::Error, "Maschinenparameter", |ui| {
        ui.label(PARAMETER_WRITE_NOTICE);
    });
    ui.label("Vorher: Parametereingabe an der CNC freigeben (Schlüsselschalter/Schlüsselzahl) und Einlesen starten.");
    let mut changed =
        ui.checkbox(&mut ack, "Ich habe die Änderungen geprüft und bin zum Schreiben berechtigt.").changed();
    ui.label(format!("Zur Bestätigung eingeben: {WRITE_CONFIRMATION_PHRASE}"));
    changed |= ui.add(egui::TextEdit::singleline(&mut phrase).desired_width(260.0)).changed();
    if changed {
        app.dialog = Some(Dialog::ParamWrite { phrase: phrase.clone(), ack });
    }
    let simulation = app.simulation();
    let cancelled = dialog_buttons(ui, "Abbrechen", |ui| {
        let ready = gate.allowed && ack && phrase.trim() == WRITE_CONFIRMATION_PHRASE;
        if ui.add_enabled(ready, mode_button(&icon_text(icons::ICON_SAVE, "Parameter schreiben"), simulation)).clicked()
        {
            app.push(Action::ParamConfirmWrite);
        }
    });
    if cancelled {
        app.push(Action::CloseDialog);
    }
}

fn real_mode(app: &mut DeckelApp, ui: &mut Ui) {
    dialog_title(ui, icons::ICON_BOLT, "Realbetrieb aktivieren");
    mode_badge(ui, false, &crate::logic::real_port_label(&app.config));
    ui.add_space(8.0);
    ui.label(
        "Im Realbetrieb gehen Übertragungen an die konfigurierte Schnittstelle der Maschine. Jede Übertragung muss \
         weiterhin unmittelbar vorher bestätigt werden.",
    );
    notice(ui, Tone::Info, "Nur Datenübertragung", |ui| {
        ui.label(SCOPE_NOTICE);
    });
    let cancelled = dialog_buttons(ui, "Abbrechen", |ui| {
        if ui.add(mode_button(&icon_text(icons::ICON_BOLT, "Realbetrieb aktivieren"), false)).clicked() {
            app.push(Action::ConfirmRealMode);
        }
    });
    if cancelled {
        app.push(Action::CloseDialog);
    }
}
