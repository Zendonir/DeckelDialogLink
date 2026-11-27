//! Drawing code for the profile editor, diagnostics tools, commissioning assistant and help.

use egui::{Color32, RichText, ScrollArea, Ui};

use deckellink_core::commissioning::{StepStatus, STEPS};
use deckellink_core::diagnostics::{LoopbackPattern, LoopbackVerdict};
use deckellink_core::model::{
    BccMode, CommentHandling, EndDetection, LineEnding, Parity, ProtocolMode, StopBits, TabHandling,
    VerificationStatus, WhitespaceHandling,
};
use deckellink_core::scenarios::REPLAY_SCENARIO_ID;

use crate::app::{Action, DeckelApp, Page};

const BAUD_RATES: [u32; 9] = [110, 300, 600, 1200, 2400, 4800, 9600, 19200, 38400];

/// Built-in manual: repository documents compiled into the program.
pub const HELP_DOCUMENTS: [(&str, &str); 12] = [
    ("Übersicht und Bedienung", include_str!("../../../../README.md")),
    ("Sicherheitskonzept", include_str!("../../../../docs/SAFETY.md")),
    ("Inbetriebnahme", include_str!("../../../../docs/COMMISSIONING.md")),
    ("Verifikations-Checkliste", include_str!("../../../../docs/VERIFICATION_CHECKLIST.md")),
    ("DIALOG 11 - bekannte/angenommene Eigenschaften", include_str!("../../../../docs/DIALOG11.md")),
    ("Kabel, Adapter, Netzwerkadapter", include_str!("../../../../docs/CABLE.md")),
    ("Maschinenparameter", include_str!("../../../../docs/PARAMETERS.md")),
    ("DIALOG-11-Simulator", include_str!("../../../../docs/SIMULATOR.md")),
    ("Programm- und Werkzeugverwaltung", include_str!("../../../../docs/LIBRARY.md")),
    ("Fehlersuche", include_str!("../../../../docs/TROUBLESHOOTING.md")),
    ("Installation und Datenordner", include_str!("../../../../docs/INSTALLATION.md")),
    ("Rust-Neubau", include_str!("../../../README.md")),
];

fn warn_color() -> Color32 {
    Color32::from_rgb(0xb5, 0x8a, 0x00)
}

// ------------------------------------------------------------------ editor

fn num_u64(ui: &mut Ui, label: &str, value: &mut u64, max: u64) {
    ui.label(label);
    ui.add(egui::DragValue::new(value).range(0..=max));
    ui.end_row();
}

fn flag(ui: &mut Ui, label: &str, value: &mut bool) {
    ui.label("");
    ui.checkbox(value, label);
    ui.end_row();
}

fn text(ui: &mut Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.text_edit_singleline(value);
    ui.end_row();
}

fn choice<T: Copy + PartialEq>(
    ui: &mut Ui,
    label: &str,
    id: &str,
    value: &mut T,
    options: &[T],
    name: impl Fn(T) -> String,
) {
    ui.label(label);
    egui::ComboBox::from_id_salt(id).selected_text(name(*value)).show_ui(ui, |ui| {
        for option in options {
            ui.selectable_value(value, *option, name(*option));
        }
    });
    ui.end_row();
}

fn telegrams(ui: &mut Ui, label: &str, list: &mut Vec<String>) {
    ui.label(label);
    let mut joined = list.join("\n");
    if ui.add(egui::TextEdit::multiline(&mut joined).desired_rows(2).hint_text("eine Zeile je Telegramm")).changed() {
        *list = joined.lines().map(str::to_string).filter(|l| !l.is_empty()).collect();
    }
    ui.end_row();
}

/// Profile section of the settings page: actions and the editor for a working copy.
pub fn profile_tools(app: &mut DeckelApp, ui: &mut Ui) {
    let idle = !app.busy();
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(idle && app.extra.draft.is_none(), egui::Button::new("Bearbeiten …")).clicked() {
            app.push(Action::EditProfile);
        }
        if ui.add_enabled(idle, egui::Button::new("Kopie anlegen")).clicked() {
            app.push(Action::DuplicateProfile);
        }
        let builtin = app.store.is_builtin(&app.profile.id);
        if ui
            .add_enabled(idle && !builtin, egui::Button::new("Löschen …"))
            .on_disabled_hover_text("Mitgelieferte Standardprofile können nicht gelöscht werden.")
            .clicked()
        {
            app.push(Action::RequestDeleteProfile);
        }
        if ui.button("Exportieren …").clicked() {
            app.push(Action::ExportProfileDialog);
        }
        if ui.add_enabled(idle, egui::Button::new("Importieren …")).clicked() {
            app.push(Action::ImportProfileDialog);
        }
    });
    let Some(mut draft) = app.extra.draft.clone() else { return };
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("Profil bearbeiten").strong());
        if app.store.is_builtin(&draft.id) {
            ui.label(
                RichText::new("Mitgelieferte Profile werden nie überschrieben - beim Speichern entsteht ein Benutzerprofil.")
                    .color(warn_color()),
            );
        }
        ui.label(
            RichText::new(
                "Alle Werte sind Annahmen, bis sie an der FP2A geprüft sind. Technische Änderungen setzen den Status auf 'nicht verifiziert'.",
            )
            .small(),
        );
        egui::Grid::new("editor-general").num_columns(2).show(ui, |ui| {
            text(ui, "Anzeigename:", &mut draft.display_name);
            text(ui, "Beschreibung:", &mut draft.description);
            ui.label("Status:");
            ui.label(draft.verification_status.label());
            ui.end_row();
        });
        egui::CollapsingHeader::new("Schnittstelle").default_open(true).show(ui, |ui| {
            egui::Grid::new("editor-serial").num_columns(2).show(ui, |ui| {
                let s = &mut draft.serial;
                choice(ui, "Baudrate:", "baud", &mut s.baud_rate, &BAUD_RATES, |b| b.to_string());
                choice(ui, "Datenbits:", "bits", &mut s.data_bits, &[5, 6, 7, 8], |b| b.to_string());
                choice(ui, "Parität:", "parity", &mut s.parity, Parity::ALL, |p| p.label().to_string());
                choice(ui, "Stoppbits:", "stop", &mut s.stop_bits, &[StopBits::One, StopBits::OnePointFive, StopBits::Two], |b| {
                    b.label().to_string()
                });
                flag(ui, "Hardware-Handshake RTS/CTS", &mut s.rts_cts);
                flag(ui, "Software-Handshake XON/XOFF", &mut s.xon_xoff);
                flag(ui, "DSR/DTR", &mut s.dsr_dtr);
                flag(ui, "RTS beim Öffnen setzen", &mut s.initial_rts);
                flag(ui, "DTR beim Öffnen setzen", &mut s.initial_dtr);
                flag(ui, "Vor dem Senden auf CTS warten", &mut s.require_cts_before_send);
                flag(ui, "Vor dem Senden auf DSR warten", &mut s.require_dsr_before_send);
                num_u64(ui, "Lese-Timeout (ms):", &mut s.read_timeout_ms, 600_000);
                num_u64(ui, "Schreib-Timeout (ms):", &mut s.write_timeout_ms, 600_000);
                num_u64(ui, "Handshake-Timeout (ms):", &mut s.handshake_timeout_ms, 3_600_000);
            });
        });
        egui::CollapsingHeader::new("Senden (Aufbereitung)").show(ui, |ui| {
            egui::Grid::new("editor-transfer").num_columns(2).show(ui, |ui| {
                let t = &mut draft.transfer;
                choice(ui, "Zeilenende:", "eol", &mut t.line_ending, LineEnding::ALL, |e| e.as_str().to_uppercase());
                text(ui, "Programmanfang:", &mut t.program_start);
                text(ui, "Programmende (z. B. M30,M2):", &mut t.program_end);
                flag(ui, "Fehlendes Programmende nach Rückfrage ergänzen", &mut t.append_program_end);
                flag(ui, "Leerzeilen entfernen", &mut t.remove_blank_lines);
                flag(ui, "Nullbytes entfernen", &mut t.strip_null_bytes);
                flag(ui, "Zeilen mit ';' überspringen", &mut t.skip_semicolon_lines);
                flag(ui, "Zeilenenden vereinheitlichen", &mut t.normalize_line_endings);
                choice(ui, "Tabulatoren:", "tabs", &mut t.tab_handling, TabHandling::ALL, |v| v.as_str().to_string());
                choice(ui, "Leerzeichen:", "ws", &mut t.whitespace, WhitespaceHandling::ALL, |v| v.as_str().to_string());
                choice(ui, "Kommentare:", "comments", &mut t.comment_handling, CommentHandling::ALL, |v| v.as_str().to_string());
                text(ui, "Kommentar-Beginn:", &mut t.comment_open);
                text(ui, "Kommentar-Ende:", &mut t.comment_close);
                flag(ui, "Zeilenende am Dateiende sicherstellen", &mut t.ensure_final_newline);
                flag(ui, "EOT (0x04) nach dem Programm senden", &mut t.send_eot_trailer);
                num_u64(ui, "Max. Zeilenlänge (0 = aus):", &mut t.max_line_length, 100_000);
                num_u64(ui, "Max. Programmgröße (Bytes, 0 = aus):", &mut t.max_program_bytes, 100_000_000);
                num_u64(ui, "Startverzögerung (ms):", &mut t.start_delay_ms, 60_000);
                num_u64(ui, "Endverzögerung (ms):", &mut t.end_delay_ms, 60_000);
                num_u64(ui, "Zeichenverzögerung (ms):", &mut t.character_delay_ms, 1000);
                num_u64(ui, "Paketgröße (Bytes, 0 = aus):", &mut t.packet_size, 65_536);
                num_u64(ui, "Paketpause (ms):", &mut t.packet_delay_ms, 60_000);
            });
        });
        egui::CollapsingHeader::new("Empfangen").show(ui, |ui| {
            egui::Grid::new("editor-receive").num_columns(2).show(ui, |ui| {
                let r = &mut draft.receive;
                choice(ui, "Ende erkennen:", "end", &mut r.end_detection, EndDetection::ALL, |v| v.as_str().to_string());
                text(ui, "Endsequenz:", &mut r.end_sequence);
                num_u64(ui, "Inaktivität (ms):", &mut r.inactivity_timeout_ms, 3_600_000);
                num_u64(ui, "Warten auf erstes Byte (ms):", &mut r.first_byte_timeout_ms, 86_400_000);
                num_u64(ui, "Nachlauf nach Endsequenz (ms):", &mut r.end_grace_ms, 600_000);
                num_u64(ui, "Max. Empfangsgröße (Bytes):", &mut r.max_receive_bytes, 100_000_000);
                flag(ui, "Nullbytes entfernen", &mut r.strip_null_bytes);
                flag(ui, "Zeilenenden vereinheitlichen", &mut r.normalize_line_endings);
                flag(ui, "Rohdaten zusätzlich speichern", &mut r.save_raw);
                flag(ui, "EOT (0x04) beendet den Empfang", &mut r.end_on_eot);
                text(ui, "Muster für Programmnamen (Regex):", &mut r.program_name_pattern);
            });
        });
        egui::CollapsingHeader::new("Protokoll (LSV2 - unverifiziert)").show(ui, |ui| {
            egui::Grid::new("editor-protocol").num_columns(2).show(ui, |ui| {
                let p = &mut draft.protocol;
                choice(ui, "Protokoll:", "mode", &mut p.mode, ProtocolMode::ALL, |v| v.label().to_string());
                flag(ui, "DLE-Transparenz", &mut p.transparent);
                choice(ui, "Prüfsumme (BCC):", "bcc", &mut p.bcc_mode, BccMode::ALL, |v| v.as_str().to_string());
                num_u64(ui, "ENQ-Intervall (ms):", &mut p.enq_interval_ms, 600_000);
                num_u64(ui, "Verbindungsaufbau max. (ms):", &mut p.link_timeout_ms, 3_600_000);
                num_u64(ui, "Quittungs-Timeout (ms):", &mut p.ack_timeout_ms, 600_000);
                num_u64(ui, "Quittungs-Timeout Nachladebetrieb (ms):", &mut p.drip_feed_ack_timeout_ms, 86_400_000);
                num_u64(ui, "Wiederholungen:", &mut p.max_retries, 100);
                num_u64(ui, "WACK-Abfrage (ms):", &mut p.wack_poll_ms, 600_000);
                num_u64(ui, "Zeilen je Telegramm:", &mut p.lines_per_telegram, 1000);
                num_u64(ui, "Max. Telegrammlänge:", &mut p.max_telegram_length, 65_536);
                flag(ui, "Zeilenende im Telegramm", &mut p.include_line_ending);
                telegrams(ui, "Starttelegramme:", &mut p.start_telegrams);
                telegrams(ui, "Endtelegramme:", &mut p.end_telegrams);
                flag(ui, "Bei Abbruch EOT senden", &mut p.send_eot_on_cancel);
            });
        });
        egui::CollapsingHeader::new("Maschinenparameter").show(ui, |ui| {
            egui::Grid::new("editor-params").num_columns(2).show(ui, |ui| {
                let q = &mut draft.parameters;
                ui.label("");
                ui.checkbox(&mut q.allow_write, RichText::new("Schreiben von Maschinenparametern erlauben").color(warn_color()));
                ui.end_row();
                num_u64(ui, "Inaktivität (ms):", &mut q.inactivity_timeout_ms, 3_600_000);
                num_u64(ui, "Warten auf erstes Byte (ms):", &mut q.first_byte_timeout_ms, 86_400_000);
                num_u64(ui, "Max. Größe (Bytes):", &mut q.max_bytes, 100_000_000);
                text(ui, "Erkennungsmuster (Regex):", &mut q.identification_pattern);
                num_u64(ui, "Max. Alter der Sicherung (min):", &mut q.max_backup_age_min, 1440);
            });
        });
        ui.horizontal(|ui| {
            if ui.button("Abbrechen").clicked() {
                app.push(Action::CancelEdit);
            }
            if ui.button(RichText::new("Speichern").strong()).clicked() {
                app.push(Action::SaveDraft);
            }
        });
    });
    if app.extra.draft.is_some() {
        app.extra.draft = Some(draft);
    }
}

/// Simulation scenario extra: recording for the replay scenario.
pub fn replay_selection(app: &mut DeckelApp, ui: &mut Ui) {
    if app.config.simulation_scenario != REPLAY_SCENARIO_ID {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        let file = if app.config.simulation_replay_file.is_empty() {
            "keine (Beispielprogramm)".to_string()
        } else {
            app.config.simulation_replay_file.clone()
        };
        ui.label(format!("Aufzeichnung: {file}"));
        if ui.add_enabled(!app.busy(), egui::Button::new("Aufzeichnung wählen …")).clicked() {
            app.push(Action::ReplayFileDialog);
        }
    });
}

// ------------------------------------------------------------- diagnostics

pub fn diagnostics_tools(app: &mut DeckelApp, ui: &mut Ui) {
    let gate = app.receive_gate();
    ui.separator();
    ui.label(RichText::new("Loopback-Test (TX und RX am Kabel gebrückt, Maschine getrennt)").strong());
    ui.add_enabled_ui(!app.busy(), |ui| {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("loop-pattern").selected_text(app.extra.loopback_pattern.label()).show_ui(
                ui,
                |ui| {
                    for pattern in LoopbackPattern::ALL {
                        ui.selectable_value(&mut app.extra.loopback_pattern, pattern, pattern.label());
                    }
                },
            );
            ui.label("Bytes:");
            ui.add(egui::DragValue::new(&mut app.extra.loopback_length).range(1..=65_536));
            ui.label("Timeout (s):");
            ui.add(egui::DragValue::new(&mut app.extra.loopback_timeout_s).range(0.5..=120.0).speed(0.1));
            if ui.add_enabled(gate.allowed, egui::Button::new("Loopback-Test starten")).clicked() {
                app.push(Action::StartLoopback);
            }
        });
    });
    if let Some(result) = app.extra.loopback_result.clone() {
        let color = match result.verdict {
            LoopbackVerdict::Passed => Color32::from_rgb(0x2e, 0x7d, 0x32),
            LoopbackVerdict::Partial => warn_color(),
            LoopbackVerdict::Failed => Color32::from_rgb(0xb3, 0x26, 0x1e),
        };
        ui.label(RichText::new(result.summary()).color(color).strong());
        if let Some(error) = &result.error {
            ui.label(error.as_text());
        }
        for hint in &result.hints {
            ui.label(format!("- {hint}"));
        }
    }
    ui.separator();
    ui.label(RichText::new("Datenformat mitlesen (nur Empfang - es wird nichts gesendet)").strong());
    ui.add_enabled_ui(!app.busy(), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("Max. Dauer (s):");
            ui.add(egui::DragValue::new(&mut app.extra.format_duration_s).range(5..=600));
            if ui.add_enabled(gate.allowed, egui::Button::new("Mitlesen starten")).clicked() {
                app.push(Action::StartFormatDetect);
            }
        });
    });
    ui.label(
        RichText::new(
            "Danach an der CNC ein Programm ausgeben. Der Port wird mit 8N1 bei der Profil-Baudrate geöffnet.",
        )
        .small(),
    );
    if let Some((bytes, guess)) = &app.extra.format_result {
        ui.label(format!("{bytes} Bytes mitgelesen"));
        ui.monospace(guess.report());
    }
    if let Some(error) = &app.extra.format_error {
        ui.label(RichText::new(error.as_text()).color(Color32::from_rgb(0xb3, 0x26, 0x1e)));
    }
    ui.separator();
    ui.label(RichText::new("Diagnosepaket (ZIP mit Protokollen, Einstellungen, Profil)").strong());
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut app.extra.export_mask_paths, "Pfade maskieren");
        ui.add_enabled(
            app.program.is_some(),
            egui::Checkbox::new(&mut app.extra.export_include_file, "geladene NC-Datei beilegen"),
        );
        if ui.button("Diagnosepaket erstellen").clicked() {
            app.push(Action::ExportDiagnostics);
        }
    });
    if let Some(path) = &app.extra.last_export {
        ui.label(format!("Zuletzt erstellt: {}", path.display()));
    }
}

// ---------------------------------------------------------- commissioning

pub fn commissioning_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Inbetriebnahme-Assistent");
    let verified = app.profile.verification_status == VerificationStatus::Verified;
    ui.label(format!(
        "Profil: {} ({}). Das Profil kann erst als verifiziert markiert werden, wenn alle Pflichtschritte erledigt sind und ein Prüfer eingetragen ist.",
        app.profile.display_name,
        if verified { "verifiziert" } else { "nicht verifiziert" }
    ));
    ui.label(
        RichText::new("DeckelLink ändert hier keine Maschineneinstellungen - der Assistent dokumentiert, was der Bediener getan hat. Ergebnisse im Realbetrieb werden automatisch eingetragen.")
            .small(),
    );
    let Some(mut record) = app.extra.commissioning.clone() else { return };
    let mut changed = false;
    egui::Grid::new("comm-head").num_columns(2).show(ui, |ui| {
        ui.label("Prüfer:");
        changed |= ui.text_edit_singleline(&mut record.tester).lost_focus();
        ui.end_row();
        ui.label("Maschine/Steuerungsstand:");
        changed |= ui.text_edit_singleline(&mut record.machine_info).lost_focus();
        ui.end_row();
    });
    ui.label(format!("{} von {} Schritten bearbeitet", record.done_count(), STEPS.len()));
    ui.separator();
    for (index, step) in STEPS.iter().enumerate() {
        let current = record.step(step.id);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(format!(
                    "{}. {}{}",
                    index + 1,
                    step.title,
                    if step.required || step.title.contains('(') { "" } else { " (optional)" }
                ))
                .strong(),
            );
            ui.label(RichText::new(step.description).small());
            ui.horizontal_wrapped(|ui| {
                let mut status = current.status;
                egui::ComboBox::from_id_salt(("comm-status", step.id)).selected_text(status.label()).show_ui(
                    ui,
                    |ui| {
                        for option in StepStatus::ALL {
                            ui.selectable_value(&mut status, option, option.label());
                        }
                    },
                );
                if status != current.status {
                    app.push(Action::CommSetStatus(step.id.to_string(), status));
                }
                let mut note = current.note.clone();
                let response = ui.add(egui::TextEdit::singleline(&mut note).hint_text("Notiz").desired_width(360.0));
                if response.changed() {
                    record.set_note(step.id, &note);
                }
                changed |= response.lost_focus();
                let target = match step.action {
                    "loopback" | "format" => Some(Page::Diagnostics),
                    "receive" => Some(Page::Receive),
                    "send" => Some(Page::Transfer),
                    "parameters" => Some(Page::Parameters),
                    "help" => Some(Page::Help),
                    _ => None,
                };
                if let Some(page) = target {
                    if ui.button(format!("Öffnen: {}", page.label())).clicked() {
                        app.push(Action::Navigate(page));
                    }
                }
                if !current.updated_at.is_empty() {
                    ui.label(RichText::new(&current.updated_at).small());
                }
            });
        });
    }
    app.extra.commissioning = Some(record.clone());
    if changed {
        app.push(Action::CommSave);
    }
    ui.separator();
    let missing = record.missing_required();
    if !missing.is_empty() {
        ui.label(format!("Offene Pflichtschritte: {}", missing.iter().map(|s| s.title).collect::<Vec<_>>().join(", ")));
    }
    ui.horizontal(|ui| {
        if ui.button("Protokoll exportieren (Markdown)").clicked() {
            app.push(Action::CommExport);
        }
        if ui
            .add_enabled(
                record.can_mark_verified() && !app.busy(),
                egui::Button::new("Profil als verifiziert markieren …"),
            )
            .clicked()
        {
            app.push(Action::CommRequestVerify);
        }
    });
}

// ---------------------------------------------------------------- help

fn inline(text: &str) -> String {
    let mut out = text.replace(['*', '`'], "");
    // [text](url) -> text (url)
    while let (Some(open), Some(mid)) = (out.find('['), out.find("](")) {
        let Some(close) = out.get(mid..).and_then(|rest| rest.find(')')).map(|c| c + mid) else { break };
        if open >= mid {
            break;
        }
        let label = out.get(open + 1..mid).unwrap_or_default().to_string();
        let url = out.get(mid + 2..close).unwrap_or_default().to_string();
        let replacement = if url.starts_with("http") { format!("{label} ({url})") } else { label };
        out.replace_range(open..=close, &replacement);
    }
    out
}

/// Minimal Markdown display (headings, lists, code blocks, tables).
pub fn markdown(ui: &mut Ui, source: &str) {
    let mut in_code = false;
    for line in source.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            ui.monospace(trimmed);
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(4.0);
        } else if trimmed == "---" || trimmed == "***" {
            ui.separator();
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            ui.add_space(6.0);
            ui.heading(inline(rest));
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            ui.add_space(6.0);
            ui.label(RichText::new(inline(rest)).size(17.0).strong());
        } else if let Some(rest) = trimmed.strip_prefix("### ").or_else(|| trimmed.strip_prefix("#### ")) {
            ui.label(RichText::new(inline(rest)).strong());
        } else if trimmed.starts_with('|') {
            if trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
                continue;
            }
            let cells: Vec<String> = trimmed.trim_matches('|').split('|').map(|c| inline(c.trim())).collect();
            ui.label(RichText::new(cells.join("  |  ")).monospace());
        } else if let Some(rest) =
            trimmed.trim_start().strip_prefix("- ").or_else(|| trimmed.trim_start().strip_prefix("* "))
        {
            let indent = trimmed.len() - trimmed.trim_start().len();
            ui.horizontal_wrapped(|ui| {
                ui.add_space(8.0 + 4.0 * f32::from(u8::try_from(indent).unwrap_or(0)));
                ui.label(format!("• {}", inline(rest)));
            });
        } else if trimmed.starts_with('>') {
            ui.label(RichText::new(inline(trimmed.trim_start_matches('>').trim())).italics());
        } else {
            ui.label(inline(trimmed));
        }
    }
}

pub fn help_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Hilfe - Handbuch");
    ui.label(format!(
        "DeckelLink {} (Rust). DeckelLink überträgt ausschließlich Daten; Programmauswahl, Freigabe und Ausführung bleiben an der CNC.",
        crate::app_extra::VERSION
    ));
    ui.horizontal_wrapped(|ui| {
        for (index, (title, _)) in HELP_DOCUMENTS.iter().enumerate() {
            if ui.selectable_label(app.extra.help_doc == index, *title).clicked() {
                app.extra.help_doc = index;
            }
        }
    });
    ui.separator();
    let (_, text) =
        HELP_DOCUMENTS.get(app.extra.help_doc).or_else(|| HELP_DOCUMENTS.first()).copied().unwrap_or(("", ""));
    ScrollArea::vertical()
        .id_salt(("help", app.extra.help_doc))
        .auto_shrink([false, false])
        .show(ui, |ui| markdown(ui, text));
}

// ---------------------------------------------------------------- dialogs

pub fn delete_profile_dialog(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Profil löschen");
    ui.label(format!(
        "Benutzerprofil '{}' ({}) löschen? Eine Sicherung bleibt im Ordner 'backups'.",
        app.profile.display_name, app.profile.id
    ));
    ui.horizontal(|ui| {
        if ui.button("Abbrechen").clicked() {
            app.push(Action::CloseDialog);
        }
        if ui.button("Löschen").clicked() {
            app.push(Action::ConfirmDeleteProfile);
        }
    });
}

pub fn mark_verified_dialog(app: &mut DeckelApp, ui: &mut Ui) {
    ui.heading("Profil als verifiziert markieren");
    let tester = app.extra.commissioning.as_ref().map(|r| r.tester.clone()).unwrap_or_default();
    ui.label(format!(
        "Profil '{}' als an der realen Maschine verifiziert kennzeichnen?\n\nPrüfer: {tester}\nMitgelieferte Standardprofile bleiben unverändert - es entsteht eine Kopie.",
        app.profile.display_name
    ));
    ui.horizontal(|ui| {
        if ui.button("Abbrechen").clicked() {
            app.push(Action::CloseDialog);
        }
        if ui.button(RichText::new("Als verifiziert markieren").strong()).clicked() {
            app.push(Action::CommConfirmVerify);
        }
    });
}

/// Files dropped onto the window are loaded (never sent).
pub fn dropped_files(app: &mut DeckelApp, ctx: &egui::Context) {
    let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
        i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
    });
    if let Some(path) = dropped.into_iter().next() {
        app.push(Action::DroppedFile(path));
    }
    let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
    if hovering {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop-overlay")));
        let rect = ctx.content_rect();
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(160));
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Datei loslassen zum Laden (es wird nichts gesendet)",
            egui::FontId::proportional(24.0),
            Color32::WHITE,
        );
    }
}
