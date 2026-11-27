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

const SIM_FILL: Color32 = Color32::from_rgb(0xff, 0xcc, 0x00);
const REAL_FILL: Color32 = Color32::from_rgb(0x8b, 0x00, 0x00);

fn status_color(kind: StatusKind) -> Color32 {
    match kind {
        StatusKind::Ready => Color32::from_rgb(0x2e, 0x7d, 0x32),
        StatusKind::Warning => Color32::from_rgb(0xb5, 0x8a, 0x00),
        StatusKind::Error => Color32::from_rgb(0xb3, 0x26, 0x1e),
        StatusKind::Active => Color32::from_rgb(0x15, 0x65, 0xc0),
        StatusKind::Idle => Color32::GRAY,
    }
}

fn severity_color(severity: Severity) -> Color32 {
    match severity {
        Severity::Error => status_color(StatusKind::Error),
        Severity::Warning => status_color(StatusKind::Warning),
        Severity::Info => status_color(StatusKind::Active),
    }
}

fn status_line(ui: &mut Ui, kind: StatusKind, text: &str) {
    ui.horizontal_wrapped(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 5.0, status_color(kind));
        ui.label(text);
    });
}

/// Mode banner: yellow with dashed look for simulation, dark red for real transfers.
pub fn mode_banner(ui: &mut Ui, simulation: bool, port: &str) {
    let (fill, text_color, text) = if simulation {
        (SIM_FILL, Color32::BLACK, "SIMULATIONSMODUS - keine Verbindung zur Maschine".to_string())
    } else {
        (
            REAL_FILL,
            Color32::WHITE,
            format!("REALBETRIEB - Schnittstelle {}", if port.is_empty() { "(kein Port)" } else { port }),
        )
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(3.0, if simulation { Color32::BLACK } else { Color32::WHITE }))
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(text_color).strong());
        });
}

/// Start button in the colour of the operating mode (yellow: simulation, dark red: real machine).
fn mode_button(label: &str, simulation: bool) -> egui::Button<'static> {
    let (fill, text) = if simulation { (SIM_FILL, Color32::BLACK) } else { (REAL_FILL, Color32::WHITE) };
    egui::Button::new(RichText::new(label.to_string()).strong().color(text)).fill(fill).min_size(egui::vec2(0.0, 28.0))
}

fn error_panel(ui: &mut Ui, error: &UserFacingError) {
    egui::Frame::new()
        .fill(Color32::from_rgb(0xfd, 0xec, 0xea))
        .stroke(egui::Stroke::new(2.0, status_color(StatusKind::Error)))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.label(RichText::new(&error.title).color(Color32::from_rgb(0x8b, 0, 0)).strong());
            if !error.causes.is_empty() {
                ui.label(RichText::new("Mögliche Ursachen:").color(Color32::BLACK));
                for cause in &error.causes {
                    ui.label(RichText::new(format!("• {cause}")).color(Color32::BLACK));
                }
            }
            if !error.technical_details.is_empty() {
                egui::CollapsingHeader::new(RichText::new("Technische Details").color(Color32::BLACK))
                    .id_salt(("error-details", &error.title))
                    .show(ui, |ui| {
                        ui.label(RichText::new(&error.technical_details).monospace().color(Color32::BLACK));
                    });
            }
        });
}

fn findings_list(ui: &mut Ui, findings: &[Finding]) {
    if findings.is_empty() {
        ui.label(RichText::new("Keine Auffälligkeiten").color(status_color(StatusKind::Ready)));
        return;
    }
    for finding in findings {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("[{}]", finding.severity.label()))
                    .color(severity_color(finding.severity))
                    .strong(),
            );
            ui.label(&finding.message);
            let lines = finding.lines_text();
            if !lines.is_empty() {
                ui.label(RichText::new(format!("(Zeilen {lines})")).small());
            }
        });
    }
}

fn notice_box(ui: &mut Ui, text: &str, fill: Color32, stroke: Color32) {
    egui::Frame::new().fill(fill).stroke(egui::Stroke::new(2.0, stroke)).inner_margin(egui::Margin::same(8)).show(
        ui,
        |ui| {
            ui.label(RichText::new(text).color(Color32::BLACK));
        },
    );
}

/// Whole window for one frame.
pub fn show(app: &mut DeckelApp, ui: &mut Ui) {
    egui::Panel::top("header").show(ui, |ui| header(app, ui));
    egui::Panel::bottom("statusbar").show(ui, |ui| statusbar(app, ui));
    egui::Panel::left("nav").resizable(false).exact_size(200.0).show(ui, |ui| navigation(app, ui));
    egui::CentralPanel::default().show(ui, |ui| {
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match app.page {
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
        });
    });
    dialogs(app, ui.ctx());
    crate::ui_extra::dropped_files(app, ui.ctx());
}

fn header(app: &mut DeckelApp, ui: &mut Ui) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new("DECKEL FP2A").heading().strong());
            ui.label("DIALOG 11");
        });
        ui.add_space(20.0);
        ui.vertical(|ui| {
            let (kind, text) = app.status.clone();
            status_line(ui, kind, &text);
            let mut link = app.profile.serial.summary();
            if app.profile.protocol.is_lsv2() {
                link.push_str(" | LSV2");
            }
            let port = app.port_label();
            ui.label(format!("{}  |  {link}", if port.is_empty() { "(kein Port)".to_string() } else { port }));
            let verified = app.profile.verification_status == VerificationStatus::Verified;
            ui.label(format!(
                "Profil: {}  ({})",
                app.profile.display_name,
                if verified { "verifiziert" } else { "NICHT verifiziert" }
            ));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            mode_banner(ui, app.simulation(), &app.port_label());
        });
    });
    ui.add_space(4.0);
}

fn statusbar(app: &mut DeckelApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        if let Some(task) = &app.task {
            ui.spinner();
            ui.label(format!("{} läuft", task.kind.label()));
            if ui.button("Abbrechen").clicked() {
                app.push(Action::Cancel);
            }
        }
        if let Some(message) = app.message.clone() {
            ui.label(message);
            if ui.small_button("x").on_hover_text("Meldung ausblenden").clicked() {
                app.message = None;
            }
        }
        if let Some(error) = &app.log.file_error {
            ui.label(RichText::new(error).color(status_color(StatusKind::Warning)));
        }
    });
}

fn navigation(app: &mut DeckelApp, ui: &mut Ui) {
    ui.add_space(8.0);
    for (index, page) in Page::ALL.into_iter().enumerate() {
        let selected = app.page == page;
        if ui.selectable_label(selected, format!("{}  {}", index + 1, page.label())).clicked() {
            app.push(Action::Navigate(page));
        }
    }
}

fn gate_hint(ui: &mut Ui, reasons: &[String]) {
    if !reasons.is_empty() {
        ui.label(RichText::new(format!("Gesperrt: {}", reasons.join("; "))).color(status_color(StatusKind::Warning)));
    }
}

fn transfer_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Übertragen - NC-Programm an die CNC senden");
    ui.horizontal_wrapped(|ui| {
        let idle = !app.busy();
        if ui.add_enabled(idle, egui::Button::new("Datei öffnen …")).clicked() {
            app.push(Action::OpenFileDialog);
        }
        if ui.add_enabled(idle && app.program.is_some(), egui::Button::new("Erneut prüfen")).clicked() {
            app.push(Action::Recheck);
        }
        if app.simulation() && ui.add_enabled(idle, egui::Button::new("Beispielprogramm laden")).clicked() {
            app.push(Action::LoadSample);
        }
        if !app.config.recent_files.is_empty() {
            egui::ComboBox::from_id_salt("recent").selected_text("Zuletzt geöffnet").width(220.0).show_ui(ui, |ui| {
                for path in app.config.recent_files.clone() {
                    if ui.add_enabled(idle, egui::Button::selectable(false, &path)).clicked() {
                        app.push(Action::LoadFile(path.into()));
                    }
                }
            });
        }
    });
    ui.separator();
    if let Some(error) = app.send_error.clone() {
        error_panel(ui, &error);
    }
    let Some(program) = app.program.clone() else {
        ui.label("Keine Datei geladen. Die Originaldatei wird nie verändert; gesendet werden aufbereitete Kopien.");
        return;
    };
    egui::Grid::new("file-info").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        ui.label("Datei:");
        ui.label(RichText::new(program.file_name()).strong());
        ui.end_row();
        ui.label("Pfad:");
        ui.label(program.path.display().to_string());
        ui.end_row();
        ui.label("Größe:");
        ui.label(format!(
            "{} ({} Zeilen, längste Zeile {} Zeichen)",
            format_bytes(program.original.len()),
            program.analysis.line_count,
            program.analysis.longest_line
        ));
        ui.end_row();
        ui.label("Programmende:");
        ui.label(if program.analysis.has_program_end { "gefunden" } else { "NICHT gefunden" });
        ui.end_row();
        ui.label("SHA-256:");
        ui.monospace(program.analysis.sha256.get(..16).unwrap_or_default());
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Prüfung").strong());
    findings_list(ui, &program.analysis.findings);
    if program.analysis.has_warnings() {
        let mut ack = app.ack_warnings;
        if ui.checkbox(&mut ack, "Warnungen gelesen und bestätigt").changed() {
            app.ack_warnings = ack;
        }
    }
    if app.profile.transfer.append_program_end && !program.analysis.has_program_end {
        let mut append = app.append_end;
        if ui.checkbox(&mut append, "Fehlendes Programmende beim Senden ergänzen (nur diese Übertragung)").changed() {
            app.push(Action::SetAppendEnd(append));
        }
    }
    if let Some(err) = &program.transform_error {
        ui.label(RichText::new(format!("Aufbereitung: {err}")).color(status_color(StatusKind::Error)));
    }
    if let Some(payload) = &program.payload {
        egui::CollapsingHeader::new(format!(
            "Transferdaten ({}, {} Zeilen) - Vorschau",
            format_bytes(payload.data.len()),
            payload.line_count
        ))
        .show(ui, |ui| {
            for note in &payload.applied {
                ui.label(format!("• {note}"));
            }
            ScrollArea::vertical().max_height(240.0).id_salt("preview").show(ui, |ui| {
                ui.monospace(preview_text(&payload.data, 400));
            });
        });
    }
    ui.separator();
    let gate = app.send_gate();
    ui.horizontal_wrapped(|ui| {
        let label = if app.simulation() { "Simulation starten" } else { "An Maschine senden" };
        if ui.add_enabled(gate.allowed, mode_button(label, app.simulation())).clicked() {
            app.push(Action::RequestSend { drip_feed: false });
        }
        if app.profile.protocol.is_lsv2()
            && ui.add_enabled(gate.allowed, egui::Button::new("Nachladebetrieb (LSV2) …")).clicked()
        {
            app.push(Action::RequestSend { drip_feed: true });
        }
        if app.busy() && ui.button("Abbrechen").clicked() {
            app.push(Action::Cancel);
        }
    });
    gate_hint(ui, &gate.reasons);
    ui.add_space(6.0);
    status_line(ui, state_kind(app.send_state), app.send_state.label());
    if let Some(progress) = &app.send_progress {
        #[allow(clippy::cast_possible_truncation)]
        let fraction = (progress.percent() / 100.0) as f32;
        ui.add(egui::ProgressBar::new(fraction).show_percentage());
        ui.label(format!(
            "{} von {} | Zeile {} von {} | {}",
            format_bytes(progress.bytes_done),
            format_bytes(progress.bytes_total),
            progress.current_line,
            progress.total_lines,
            format_duration(progress.elapsed_s)
        ));
    }
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
    ui.heading("Empfangen - Programm von der CNC übernehmen");
    ui.label("Ablauf: Empfang hier starten, dann die Programmausgabe an der CNC starten. Empfangene Programme werden als neue Datei gespeichert - nie überschrieben.");
    let gate = app.receive_gate();
    ui.horizontal(|ui| {
        if ui.add_enabled(gate.allowed, egui::Button::new(RichText::new("Empfang starten").strong())).clicked() {
            app.push(Action::StartReceive);
        }
        if app.busy() && ui.button("Abbrechen").clicked() {
            app.push(Action::Cancel);
        }
    });
    gate_hint(ui, &gate.reasons);
    ui.separator();
    status_line(ui, StatusKind::Active, app.receive_state.label());
    if let Some(progress) = &app.receive_progress {
        ui.label(format!(
            "{} empfangen | Dauer {} | seit {:.1} s keine Daten",
            format_bytes(progress.bytes_received),
            format_duration(progress.elapsed_s),
            progress.idle_s
        ));
    }
    if let Some(error) = app.receive_error.clone() {
        error_panel(ui, &error);
    }
    if let Some(summary) = app.receive_summary.clone() {
        ui.add_space(6.0);
        let (kind, text) = if summary.complete {
            (StatusKind::Ready, "Programm vollständig empfangen".to_string())
        } else if summary.bytes == 0 {
            (StatusKind::Error, format!("Keine Daten empfangen ({})", summary.end_reason))
        } else {
            (StatusKind::Warning, "Programm möglicherweise UNVOLLSTÄNDIG - nicht ungeprüft verwenden".to_string())
        };
        status_line(ui, kind, &text);
        ui.label(format!("{} - {}, {}", summary.state.label(), summary.end_reason, format_bytes(summary.bytes)));
        if let Some(path) = &summary.stored_path {
            ui.horizontal_wrapped(|ui| {
                ui.label("Gespeichert:");
                ui.monospace(path.display().to_string());
                if ui.add_enabled(!app.busy(), egui::Button::new("Im Übertragen öffnen")).clicked() {
                    app.push(Action::LoadArchived(path.clone()));
                }
            });
        }
        for note in &summary.notes {
            ui.label(format!("• {note}"));
        }
        for finding in &summary.findings {
            ui.label(finding);
        }
    }
}

fn programs_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("NC-Programme - empfangene Programme");
    ui.horizontal(|ui| {
        if ui.button("Aktualisieren").clicked() {
            app.push(Action::RefreshArchive);
        }
        ui.label(format!("Ordner: {}", app.paths.received.display()));
    });
    ui.separator();
    if app.archive.is_empty() {
        ui.label("Noch keine Programme empfangen.");
        return;
    }
    egui::Grid::new("archive").striped(true).num_columns(5).spacing([14.0, 4.0]).show(ui, |ui| {
        for title in ["Datei", "Status", "Größe", "Geändert", ""] {
            ui.label(RichText::new(title).strong());
        }
        ui.end_row();
        for entry in app.archive.clone() {
            ui.label(entry.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            let (kind, text) = match (entry.folder, entry.complete()) {
                (_, Some(true)) => (StatusKind::Ready, "vollständig"),
                ("failed", _) | (_, Some(false)) => (StatusKind::Warning, "UNVOLLSTÄNDIG"),
                _ => (StatusKind::Idle, "ohne Metadaten"),
            };
            ui.label(RichText::new(text).color(status_color(kind)));
            ui.label(format_bytes(usize::try_from(entry.size).unwrap_or(usize::MAX)));
            ui.label(entry.modified.map(|m| m.format("%d.%m.%Y %H:%M").to_string()).unwrap_or_default());
            if ui.add_enabled(!app.busy(), egui::Button::new("Zum Senden laden")).clicked() {
                app.push(Action::LoadArchived(entry.path.clone()));
            }
            ui.end_row();
        }
    });
}

fn diff_view(ui: &mut Ui, title: &str, diff: &ParameterDiff) {
    ui.label(RichText::new(format!("Vergleich {title}: {}", diff.summary())).strong());
    if diff.diff_text.is_empty() {
        return;
    }
    ScrollArea::vertical().max_height(260.0).id_salt("diff").show(ui, |ui| {
        for line in diff.diff_text.lines() {
            let color = if line.starts_with('+') && !line.starts_with("+++") {
                Color32::from_rgb(0x2e, 0x7d, 0x32)
            } else if line.starts_with('-') && !line.starts_with("---") {
                Color32::from_rgb(0xb3, 0x26, 0x1e)
            } else {
                ui.visuals().text_color()
            };
            ui.label(RichText::new(line).monospace().color(color));
        }
    });
}

fn parameters_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Maschinenparameter");
    ui.label(
        "Parameter werden nur als Datei übertragen: Auslesen nach Start der Parameterausgabe an der CNC, Schreiben nur \
         in eine an der CNC freigegebene Parametereingabe. Das Parameterformat der DIALOG 11 ist nicht verifiziert.",
    );
    ui.separator();
    ui.label(RichText::new("1. Aktuelle Sicherung auslesen").strong());
    let read_gate = app.receive_gate();
    ui.horizontal(|ui| {
        if ui.add_enabled(read_gate.allowed, egui::Button::new("Parameter auslesen")).clicked() {
            app.push(Action::ParamRead);
        }
        if app.busy() && ui.button("Abbrechen").clicked() {
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
            ui.label(format!(
                "Aktuelle Sicherung: {}, {}, SHA-256 {}…, ausgelesen {}\n{summary}",
                backup.path_name,
                format_bytes(backup.data.len()),
                backup.sha256().get(..16).unwrap_or_default(),
                backup.read_at.format("%H:%M:%S")
            ));
        }
        None => {
            ui.label("Aktuelle Sicherung: keine (in dieser Sitzung noch nicht ausgelesen)");
        }
    }
    if let Some((ok, text)) = app.param.verify.clone() {
        status_line(ui, if ok { StatusKind::Ready } else { StatusKind::Error }, &text);
    }
    if let Some(error) = app.param.error.clone() {
        error_panel(ui, &error);
    }
    egui::CollapsingHeader::new(format!("Gespeicherte Parametersätze ({})", app.param.sets.len())).show(ui, |ui| {
        egui::Grid::new("param-sets").striped(true).num_columns(4).show(ui, |ui| {
            for set in app.param.sets.clone() {
                ui.label(set.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                ui.label(if set.complete() { "vollständig" } else { "UNVOLLSTÄNDIG" });
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
    ui.separator();
    ui.label(RichText::new("2. Parameterdatei zum Schreiben wählen").strong());
    if ui.add_enabled(!app.busy(), egui::Button::new("Datei wählen …")).clicked() {
        app.push(Action::ParamOpenFileDialog);
    }
    if let Some(candidate) = app.param.candidate.clone() {
        ui.label(format!(
            "{} - {}, SHA-256 {}…",
            candidate.name,
            format_bytes(candidate.data.len()),
            candidate.analysis.sha256.get(..16).unwrap_or_default()
        ));
        if let Some(info) = candidate.structure() {
            ui.label(info.summary());
        }
        findings_list(ui, &candidate.analysis.findings);
    }
    if let Some((title, diff)) = app.param.diff.clone() {
        diff_view(ui, &title, &diff);
    }
    ui.separator();
    ui.label(RichText::new("3. Schreiben (gesperrt, bis alle Bedingungen erfüllt sind)").strong());
    ui.horizontal(|ui| {
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
    });
    let gate = app.parameter_gate();
    if ui.add_enabled(gate.allowed, egui::Button::new(RichText::new("Parameter schreiben …").strong())).clicked() {
        app.push(Action::ParamRequestWrite);
    }
    gate_hint(ui, &gate.reasons);
    if let Some(progress) = &app.param.progress {
        #[allow(clippy::cast_possible_truncation)]
        ui.add(egui::ProgressBar::new((progress.percent() / 100.0) as f32).show_percentage());
    }
}

fn terminal_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Terminal - übertragene Bytes");
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.terminal_hex, "Hexadezimal");
        if ui.button("Leeren").clicked() {
            app.push(Action::ClearTerminal);
        }
        ui.label(format!("{} Einträge (max. {})", app.traffic.len(), app.config.terminal_history_lines));
    });
    ui.label(RichText::new("Nur zur Anzeige in dieser Sitzung. In die Protokolldatei gelangen Rohdaten nur bei aktiviertem Rohdatenprotokoll (Einstellungen).").small());
    ui.separator();
    ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).id_salt("terminal").show(ui, |ui| {
        for event in &app.traffic {
            let (dir, color) = match event.direction {
                Direction::Tx => ("TX", Color32::from_rgb(0x15, 0x65, 0xc0)),
                Direction::Rx => ("RX", Color32::from_rgb(0x2e, 0x7d, 0x32)),
            };
            let text = if app.terminal_hex { hex_line(&event.data) } else { ascii_line(&event.data) };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(event.timestamp.format("%H:%M:%S%.3f").to_string()).monospace().small());
                ui.label(RichText::new(dir).monospace().color(color).strong());
                ui.label(RichText::new(text).monospace());
            });
        }
    });
}

fn line_state(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "EIN",
        Some(false) => "aus",
        None => "unbekannt",
    }
}

fn diagnostics_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Diagnose");
    ui.horizontal(|ui| {
        ui.label(RichText::new("Serielle Anschlüsse").strong());
        if ui.button("Aktualisieren").clicked() {
            app.push(Action::RefreshPorts);
        }
    });
    if app.ports.is_empty() {
        ui.label("Keine seriellen Anschlüsse gefunden (oder noch nicht gesucht).");
    }
    for port in &app.ports {
        ui.label(format!("• {}", port.label()));
    }
    ui.separator();
    ui.label(RichText::new("Modemleitungen (letzter Stand während einer Übertragung)").strong());
    match app.modem {
        Some(m) => {
            ui.label(format!(
                "CTS {} | DSR {} | DCD {} | RI {} | RTS {} | DTR {}",
                line_state(m.cts),
                line_state(m.dsr),
                line_state(m.dcd),
                line_state(m.ri),
                line_state(m.rts),
                line_state(m.dtr)
            ));
        }
        None => {
            ui.label("Noch keine Werte - werden während einer Übertragung angezeigt.");
        }
    }
    ui.separator();
    ui.label(RichText::new("Profil").strong());
    ui.label(format!(
        "{} ({}) - {}",
        app.profile.display_name,
        app.profile.id,
        app.profile.verification_status.label()
    ));
    for warning in safety_warnings(&app.profile) {
        ui.label(RichText::new(format!("Warnung: {warning}")).color(status_color(StatusKind::Warning)));
    }
    for problem in &app.store.problems {
        ui.label(RichText::new(format!("Profilproblem: {problem}")).color(status_color(StatusKind::Error)));
    }
    ui.separator();
    ui.label(RichText::new("Programm").strong());
    ui.label(format!("DeckelLink {} (Rust)", env!("CARGO_PKG_VERSION")));
    ui.label(format!("Datenordner: {}", app.paths.root.display()));
    ui.label(format!("Protokolle: {}", app.paths.logs.display()));
    crate::ui_extra::diagnostics_tools(app, ui);
}

fn settings_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Einstellungen");
    ui.label(RichText::new("Betriebsart").strong());
    ui.horizontal(|ui| {
        let idle = !app.busy();
        if ui.add_enabled(idle && !app.simulation(), egui::Button::selectable(app.simulation(), "Simulation")).clicked()
        {
            app.push(Action::SetSimulation);
        }
        if ui
            .add_enabled(idle && app.simulation(), egui::Button::selectable(!app.simulation(), "Realbetrieb …"))
            .clicked()
        {
            app.push(Action::RequestRealMode);
        }
    });
    if app.simulation() {
        let current = get_scenario(&app.config.simulation_scenario);
        egui::ComboBox::from_label("Simulationsszenario").selected_text(current.title).width(380.0).show_ui(ui, |ui| {
            for scenario in scenarios() {
                if ui.selectable_label(scenario.id == current.id, scenario.title).clicked() {
                    app.config.simulation_scenario = scenario.id.into();
                    app.push(Action::SaveConfig);
                }
            }
        });
        ui.label(RichText::new(format!("{} ({})", current.description, current.kind.label())).small());
        crate::ui_extra::replay_selection(app, ui);
    }
    ui.separator();
    ui.label(RichText::new("Verbindung für den Realbetrieb").strong());
    ui.add_enabled_ui(!app.busy(), |ui| {
        let mut changed = false;
        egui::ComboBox::from_label("Verbindungsart").selected_text(app.config.connection_kind.label()).show_ui(
            ui,
            |ui| {
                for kind in ConnectionKind::ALL {
                    changed |= ui.selectable_value(&mut app.config.connection_kind, kind, kind.label()).changed();
                }
            },
        );
        match app.config.connection_kind {
            ConnectionKind::Serial => {
                ui.horizontal(|ui| {
                    ui.label("COM-Port:");
                    changed |= ui.text_edit_singleline(&mut app.config.port).lost_focus();
                    egui::ComboBox::from_id_salt("ports").selected_text("Auswahl").show_ui(ui, |ui| {
                        for port in app.ports.clone() {
                            if ui.selectable_label(app.config.port == port.device, port.label()).clicked() {
                                app.config.port = port.device.clone();
                                changed = true;
                            }
                        }
                    });
                    if ui.button("Suchen").clicked() {
                        app.push(Action::RefreshPorts);
                    }
                });
            }
            ConnectionKind::Tcp | ConnectionKind::Rfc2217 => {
                ui.horizontal(|ui| {
                    ui.label("Adapter (IP/Name):");
                    changed |= ui.text_edit_singleline(&mut app.config.network_host).lost_focus();
                    ui.label("TCP-Port:");
                    changed |= ui.add(egui::DragValue::new(&mut app.config.network_port).range(1..=65535)).changed();
                });
            }
        }
        if changed {
            app.push(Action::SaveConfig);
        }
    });
    ui.separator();
    ui.label(RichText::new("Profil").strong());
    ui.add_enabled_ui(!app.busy(), |ui| {
        egui::ComboBox::from_id_salt("profile").selected_text(app.profile.display_name.clone()).width(420.0).show_ui(
            ui,
            |ui| {
                for profile in app.store.profiles().to_vec() {
                    if ui.selectable_label(profile.id == app.profile.id, &profile.display_name).clicked() {
                        app.push(Action::SelectProfile(profile.id.clone()));
                    }
                }
            },
        );
    });
    crate::ui_extra::profile_tools(app, ui);
    let p = &app.profile;
    ui.label(format!(
        "{} | Protokoll {} | Zeilenende {} | Programmende '{}' | Status: {}",
        p.serial.summary(),
        p.protocol.mode.label(),
        p.transfer.line_ending.as_str().to_uppercase(),
        p.transfer.program_end,
        p.verification_status.label()
    ));
    if !p.description.is_empty() {
        ui.label(RichText::new(&p.description).small());
    }
    ui.separator();
    ui.label(RichText::new("Anzeige und Protokoll").strong());
    let mut changed = false;
    changed |= ui
        .checkbox(&mut app.config.show_safety_notice, "Sicherheitshinweis vor realen Übertragungen anzeigen")
        .changed();
    changed |= ui
        .checkbox(
            &mut app.config.raw_traffic_logging,
            "Rohdatenprotokoll in die Protokolldatei schreiben (enthält NC-Daten)",
        )
        .changed();
    ui.horizontal(|ui| {
        ui.label("Schriftgröße (%):");
        changed |= ui.add(egui::DragValue::new(&mut app.config.font_scale_percent).range(80..=200)).changed();
        ui.label("Protokollstufe:");
        egui::ComboBox::from_id_salt("loglevel").selected_text(app.config.log_level.clone()).show_ui(ui, |ui| {
            for level in deckellink_core::appdata::LOG_LEVELS {
                changed |= ui.selectable_value(&mut app.config.log_level, level.to_string(), level).changed();
            }
        });
    });
    if changed {
        app.push(Action::SaveConfig);
    }
    ui.label(RichText::new(format!("Datenordner: {}", app.paths.root.display())).small());
}

fn log_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Protokoll");
    ui.horizontal(|ui| {
        ui.label("Anzeigen ab:");
        for level in [Level::Debug, Level::Info, Level::Warning, Level::Error] {
            ui.selectable_value(&mut app.log_filter, level, level.as_str());
        }
    });
    ui.separator();
    ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).id_salt("log").show(ui, |ui| {
        for entry in app.log.entries.iter().filter(|e| e.level >= app.log_filter) {
            let color = match entry.level {
                Level::Error => status_color(StatusKind::Error),
                Level::Warning => status_color(StatusKind::Warning),
                _ => ui.visuals().text_color(),
            };
            ui.label(
                RichText::new(format!("{} {:<7} {}", entry.time, entry.level.as_str(), entry.message))
                    .monospace()
                    .color(color),
            );
        }
    });
}

// ------------------------------------------------------------------------ dialogs

fn dialogs(app: &mut DeckelApp, ctx: &egui::Context) {
    let Some(dialog) = app.dialog.clone() else { return };
    let modal = egui::Modal::new(egui::Id::unique("deckellink-dialog")).show(ctx, |ui| {
        ui.set_max_width(640.0);
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

fn confirm_send(app: &mut DeckelApp, ui: &mut Ui, drip_feed: bool, mut drip_ack: bool, mut suppress: bool) {
    ui.heading("Übertragung bestätigen");
    let simulation = app.simulation();
    mode_banner(ui, simulation, &app.port_label());
    let Some(program) = app.program.clone() else {
        app.push(Action::CloseDialog);
        return;
    };
    let Some(payload) = program.payload.clone() else {
        app.push(Action::CloseDialog);
        return;
    };
    egui::Grid::new("confirm").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        ui.label("Datei:");
        ui.label(program.file_name());
        ui.end_row();
        ui.label("Transferdaten:");
        ui.label(format!("{} Bytes, {} Zeilen", payload.data.len(), payload.line_count));
        ui.end_row();
        let verified = app.profile.verification_status == VerificationStatus::Verified;
        ui.label("Profil:");
        ui.label(format!(
            "{} ({})",
            app.profile.display_name,
            if verified { "verifiziert" } else { "NICHT verifiziert" }
        ));
        ui.end_row();
        ui.label("Schnittstelle:");
        ui.label(format!("{} | {}", app.port_label(), app.profile.serial.summary()));
        ui.end_row();
        ui.label("Protokoll:");
        ui.label(app.profile.protocol.mode.label());
        ui.end_row();
    });
    for note in &payload.applied {
        ui.label(format!("Aufbereitung: {note}"));
    }
    if payload.appended_program_end {
        ui.label(RichText::new("Achtung: Programmende wird auf Ihre Bestätigung hin ergänzt.").strong());
    }
    let warnings: Vec<String> = program.analysis.warnings().map(|w| format!("- {}", w.message)).collect();
    if !warnings.is_empty() {
        ui.label(format!("Bestätigte Warnungen:\n{}", warnings.join("\n")));
    }
    ui.label(SCOPE_NOTICE);
    if drip_feed {
        notice_box(ui, DRIP_FEED_NOTICE, Color32::from_rgb(0xfd, 0xec, 0xea), Color32::from_rgb(0xb3, 0x26, 0x1e));
        if ui
            .checkbox(
                &mut drip_ack,
                "Ich habe verstanden: Die Maschine muss bei Störungen an der CNC angehalten werden.",
            )
            .changed()
        {
            app.dialog = Some(Dialog::ConfirmSend { drip_feed, drip_ack, suppress });
        }
    }
    let show_notice = app.config.show_safety_notice && !simulation;
    if show_notice {
        notice_box(
            ui,
            &format!("SICHERHEITSHINWEIS\n\n{SAFETY_NOTICE}"),
            Color32::from_rgb(0xff, 0xf8, 0xe1),
            Color32::from_rgb(0xb5, 0x8a, 0x00),
        );
        if ui.checkbox(&mut suppress, "Sicherheitshinweis künftig nicht mehr anzeigen").changed() {
            app.dialog = Some(Dialog::ConfirmSend { drip_feed, drip_ack, suppress });
        }
    }
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Abbrechen").clicked() {
            app.push(Action::CloseConfirm);
        }
        let label = if drip_feed {
            "Nachladebetrieb starten"
        } else if simulation {
            "Simulation starten"
        } else {
            "Übertragung starten"
        };
        if ui.add_enabled(!drip_feed || drip_ack, mode_button(label, simulation)).clicked() {
            app.push(Action::ConfirmSend { suppress_safety_notice: show_notice && suppress });
        }
    });
}

fn param_unlock(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Schreiben von Maschinenparametern freischalten");
    notice_box(ui, PARAMETER_WRITE_NOTICE, Color32::from_rgb(0xfd, 0xec, 0xea), Color32::from_rgb(0xb3, 0x26, 0x1e));
    ui.label("Schreibfunktion für diese Sitzung freischalten?");
    ui.horizontal(|ui| {
        if ui.button("Nein").clicked() {
            app.push(Action::CloseDialog);
        }
        if ui.button("Ja, für diese Sitzung freischalten").clicked() {
            app.push(Action::ParamConfirmUnlock);
        }
    });
}

fn param_write(app: &mut DeckelApp, ui: &mut Ui, mut phrase: String, mut ack: bool) {
    ui.heading("Maschinenparameter schreiben");
    mode_banner(ui, app.simulation(), &app.port_label());
    let gate = app.parameter_gate();
    if let (Some(candidate), Some(backup)) = (&app.param.candidate, &app.param.backup) {
        egui::Grid::new("param-write").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
            ui.label("Datei:");
            ui.label(format!(
                "{} (SHA-256 {}…)",
                candidate.name,
                candidate.analysis.sha256.get(..16).unwrap_or_default()
            ));
            ui.end_row();
            ui.label("Sicherung:");
            ui.label(&backup.path_name);
            ui.end_row();
            ui.label("Änderungen:");
            ui.label(gate.diff.as_ref().map(ParameterDiff::summary).unwrap_or_default());
            ui.end_row();
            ui.label("Anschluss:");
            ui.label(format!(
                "{} | Profil {} | {}",
                app.port_label(),
                app.profile.display_name,
                app.profile.protocol.mode.label()
            ));
            ui.end_row();
        });
    }
    notice_box(ui, PARAMETER_WRITE_NOTICE, Color32::from_rgb(0xfd, 0xec, 0xea), Color32::from_rgb(0xb3, 0x26, 0x1e));
    ui.label("Vorher: Parametereingabe an der CNC freigeben (Schlüsselschalter/Schlüsselzahl) und Einlesen starten.");
    let mut changed =
        ui.checkbox(&mut ack, "Ich habe die Änderungen geprüft und bin zum Schreiben berechtigt.").changed();
    ui.label(format!("Zur Bestätigung eingeben: {WRITE_CONFIRMATION_PHRASE}"));
    changed |= ui.text_edit_singleline(&mut phrase).changed();
    if changed {
        app.dialog = Some(Dialog::ParamWrite { phrase: phrase.clone(), ack });
    }
    ui.horizontal(|ui| {
        if ui.button("Abbrechen").clicked() {
            app.push(Action::CloseDialog);
        }
        let ready = gate.allowed && ack && phrase.trim() == WRITE_CONFIRMATION_PHRASE;
        if ui.add_enabled(ready, egui::Button::new(RichText::new("Parameter schreiben").strong())).clicked() {
            app.push(Action::ParamConfirmWrite);
        }
    });
}

fn real_mode(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Realbetrieb aktivieren");
    mode_banner(ui, false, &crate::logic::real_port_label(&app.config));
    ui.label(
        "Im Realbetrieb gehen Übertragungen an die konfigurierte Schnittstelle der Maschine. Jede Übertragung muss \
         weiterhin unmittelbar vorher bestätigt werden.",
    );
    ui.label(SCOPE_NOTICE);
    ui.horizontal(|ui| {
        if ui.button("Abbrechen").clicked() {
            app.push(Action::CloseDialog);
        }
        if ui.button(RichText::new("Realbetrieb aktivieren").strong()).clicked() {
            app.push(Action::ConfirmRealMode);
        }
    });
}
