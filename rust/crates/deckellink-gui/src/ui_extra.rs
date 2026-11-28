//! Drawing code for the profile editor, diagnostics tools, commissioning assistant and help.

use egui::{Color32, RichText, Ui};

use deckellink_core::commissioning::{StepStatus, STEPS};
use deckellink_core::diagnostics::{LoopbackPattern, LoopbackVerdict};
use deckellink_core::model::{
    BccMode, CommentHandling, EndDetection, LineEnding, Parity, ProtocolMode, StopBits, TabHandling,
    VerificationStatus, WhitespaceHandling,
};
use deckellink_core::scenarios::REPLAY_SCENARIO_ID;

use crate::app::{Action, DeckelApp};
use crate::theme::{fields, lamp, message, mono_bold, screen_box, screen_key, Tone, DIM, GREEN, ORANGE, TEXT, YELLOW};

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
    ORANGE
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

/// Editor for a working copy of the profile (opened with F3 on the settings page).
pub fn profile_editor(app: &mut DeckelApp, ui: &mut Ui) {
    let Some(mut draft) = app.extra.draft.clone() else { return };
    ui.add_space(8.0);
    screen_box(ui, "Profil bearbeiten", |ui| {
        if app.store.is_builtin(&draft.id) {
            ui.label(
                RichText::new(
                    "Mitgelieferte Profile werden nie überschrieben - beim Speichern entsteht ein Benutzerprofil.",
                )
                .color(warn_color()),
            );
        }
        ui.label(
            RichText::new(
                "Alle Werte sind Annahmen, bis sie an der FP2A geprüft sind. Technische Änderungen setzen den Status auf 'nicht verifiziert'.",
            )
            .color(DIM),
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
                choice(
                    ui,
                    "Stoppbits:",
                    "stop",
                    &mut s.stop_bits,
                    &[StopBits::One, StopBits::OnePointFive, StopBits::Two],
                    |b| b.label().to_string(),
                );
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
                choice(ui, "Leerzeichen:", "ws", &mut t.whitespace, WhitespaceHandling::ALL, |v| {
                    v.as_str().to_string()
                });
                choice(ui, "Kommentare:", "comments", &mut t.comment_handling, CommentHandling::ALL, |v| {
                    v.as_str().to_string()
                });
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
                choice(ui, "Ende erkennen:", "end", &mut r.end_detection, EndDetection::ALL, |v| {
                    v.as_str().to_string()
                });
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
                ui.checkbox(
                    &mut q.allow_write,
                    RichText::new("Schreiben von Maschinenparametern erlauben").color(warn_color()),
                );
                ui.end_row();
                num_u64(ui, "Inaktivität (ms):", &mut q.inactivity_timeout_ms, 3_600_000);
                num_u64(ui, "Warten auf erstes Byte (ms):", &mut q.first_byte_timeout_ms, 86_400_000);
                num_u64(ui, "Max. Größe (Bytes):", &mut q.max_bytes, 100_000_000);
                text(ui, "Erkennungsmuster (Regex):", &mut q.identification_pattern);
                num_u64(ui, "Max. Alter der Sicherung (min):", &mut q.max_backup_age_min, 1440);
            });
        });
        ui.horizontal(|ui| {
            if ui.add(screen_key("Abbrechen")).clicked() {
                app.push(Action::CancelEdit);
            }
            if ui.add(screen_key("Speichern")).clicked() {
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
        if ui.add_enabled(!app.busy(), screen_key("Aufzeichnung wählen")).clicked() {
            app.push(Action::ReplayFileDialog);
        }
    });
}

// ------------------------------------------------------------- diagnostics

pub fn diagnostics_tools(app: &mut DeckelApp, ui: &mut Ui) {
    screen_box(ui, "Loopback-Test (F2)  -  TX und RX am Kabel gebrückt, Maschine getrennt", |ui| {
        ui.add_enabled_ui(!app.busy(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Muster:");
                egui::ComboBox::from_id_salt("loop-pattern")
                    .selected_text(app.extra.loopback_pattern.label())
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        for pattern in LoopbackPattern::ALL {
                            ui.selectable_value(&mut app.extra.loopback_pattern, pattern, pattern.label());
                        }
                    });
                ui.label("Bytes:");
                ui.add(egui::DragValue::new(&mut app.extra.loopback_length).range(1..=65_536));
                ui.label("Timeout (s):");
                ui.add(egui::DragValue::new(&mut app.extra.loopback_timeout_s).range(0.5..=120.0).speed(0.1));
            });
        });
        if let Some(result) = app.extra.loopback_result.clone() {
            let tone = match result.verdict {
                LoopbackVerdict::Passed => Tone::Ok,
                LoopbackVerdict::Partial => Tone::Warn,
                LoopbackVerdict::Failed => Tone::Error,
            };
            message(ui, tone, &result.summary(), |ui| {
                if let Some(error) = &result.error {
                    ui.label(error.as_text());
                }
                for hint in &result.hints {
                    ui.label(format!("- {hint}"));
                }
            });
        }
    });
    screen_box(ui, "Datenformat mitlesen (F3)  -  nur Empfang, es wird nichts gesendet", |ui| {
        ui.add_enabled_ui(!app.busy(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Max. Dauer (s):");
                ui.add(egui::DragValue::new(&mut app.extra.format_duration_s).range(5..=600));
            });
        });
        ui.label(
            RichText::new(
                "Danach an der CNC ein Programm ausgeben. Der Port wird mit 8N1 bei der Profil-Baudrate geöffnet.",
            )
            .color(DIM),
        );
        if let Some((bytes, guess)) = &app.extra.format_result {
            let tone = if guess.confidence == "keine" { Tone::Warn } else { Tone::Ok };
            message(
                ui,
                tone,
                &format!("Ergebnis {}  Sicherheit {}  ({bytes} Bytes)", guess.frame(), guess.confidence),
                |ui| {
                    ui.label(guess.report());
                },
            );
        }
        if let Some(error) = app.extra.format_error.clone() {
            crate::ui::error_panel(ui, &error);
        }
    });
    screen_box(ui, "Diagnosepaket (F4)  -  ZIP mit Protokollen, Einstellungen und Profil", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut app.extra.export_mask_paths, "Pfade maskieren");
            ui.add_enabled(
                app.program.is_some(),
                egui::Checkbox::new(&mut app.extra.export_include_file, "geladene NC-Datei beilegen"),
            );
        });
        if let Some(path) = &app.extra.last_export {
            ui.label(RichText::new(format!("Zuletzt erstellt: {}", path.display())).color(DIM));
        }
    });
}

// ---------------------------------------------------------- commissioning

pub fn commissioning_page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.label(
        RichText::new("Dokumentiert, was der Bediener geprüft hat - DeckelLink ändert hier keine Maschineneinstellungen. Ergebnisse im Realbetrieb werden automatisch eingetragen.")
            .color(DIM),
    );
    ui.add_space(6.0);
    let Some(mut record) = app.extra.commissioning.clone() else { return };
    let verified = app.profile.verification_status == VerificationStatus::Verified;
    let mut changed = false;
    screen_box(ui, "Profil und Prüfer", |ui| {
        fields(
            ui,
            "comm-profile",
            &[
                ("Profil", app.profile.display_name.clone()),
                ("Status", if verified { "verifiziert".into() } else { "nicht verifiziert".into() }),
                ("Fortschritt", format!("{} von {} Schritten bearbeitet", record.done_count(), STEPS.len())),
            ],
        );
        egui::Grid::new("comm-head").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
            ui.label(RichText::new("Prüfer:").color(DIM));
            changed |= ui.add(egui::TextEdit::singleline(&mut record.tester).desired_width(320.0)).lost_focus();
            ui.end_row();
            ui.label(RichText::new("Maschine/Steuerungsstand:").color(DIM));
            changed |= ui.add(egui::TextEdit::singleline(&mut record.machine_info).desired_width(320.0)).lost_focus();
            ui.end_row();
        });
    });
    screen_box(ui, "Schritte", |ui| {
        egui::Grid::new("comm-steps").num_columns(4).spacing([12.0, 6.0]).striped(true).show(ui, |ui| {
            for title in ["NR", "SCHRITT", "STATUS", "NOTIZ"] {
                ui.label(RichText::new(title).color(YELLOW));
            }
            ui.end_row();
            for (index, step) in STEPS.iter().enumerate() {
                let current = record.step(step.id);
                let color = match current.status {
                    StepStatus::Done => GREEN,
                    StepStatus::NotApplicable => DIM,
                    StepStatus::Open if step.required => ORANGE,
                    StepStatus::Open => DIM,
                };
                ui.label(mono_bold(format!("{:>2}", index + 1), 13.5).color(color));
                ui.vertical(|ui| {
                    ui.set_max_width(420.0);
                    let optional = if step.required || step.title.contains('(') { "" } else { " (optional)" };
                    ui.label(RichText::new(format!("{}{optional}", step.title)).color(TEXT));
                    ui.label(RichText::new(step.description).small().color(DIM));
                });
                let mut status = current.status;
                egui::ComboBox::from_id_salt(("comm-status", step.id))
                    .selected_text(status.label())
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for option in StepStatus::ALL {
                            ui.selectable_value(&mut status, option, option.label());
                        }
                    });
                if status != current.status {
                    app.push(Action::CommSetStatus(step.id.to_string(), status));
                }
                let mut note = current.note.clone();
                let response = ui.add(egui::TextEdit::singleline(&mut note).hint_text("Notiz").desired_width(260.0));
                if response.changed() {
                    record.set_note(step.id, &note);
                }
                changed |= response.lost_focus();
                ui.end_row();
            }
        });
    });
    app.extra.commissioning = Some(record.clone());
    if changed {
        app.push(Action::CommSave);
    }
    let missing = record.missing_required();
    if missing.is_empty() {
        lamp(ui, GREEN, "ALLE PFLICHTSCHRITTE ERLEDIGT - F2 ALS VERIFIZIERT MARKIEREN (Prüfer erforderlich)");
    } else {
        lamp(ui, ORANGE, &format!("OFFENE PFLICHTSCHRITTE: {}", missing.len()));
    }
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
            ui.label(mono_bold(inline(rest).to_uppercase(), 16.0).color(YELLOW));
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            ui.add_space(6.0);
            ui.add_space(4.0);
            ui.label(mono_bold(inline(rest), 14.5).color(YELLOW));
        } else if let Some(rest) = trimmed.strip_prefix("### ").or_else(|| trimmed.strip_prefix("#### ")) {
            ui.label(mono_bold(inline(rest), 13.5).color(TEXT));
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
    ui.columns(2, |cols| {
        let [index, body] = cols else { return };
        screen_box(index, "Inhalt", |ui| {
            for (number, (title, _)) in HELP_DOCUMENTS.iter().enumerate() {
                let active = app.extra.help_doc == number;
                let text =
                    RichText::new(format!("{:>2}  {title}", number + 1)).color(if active { YELLOW } else { TEXT });
                if ui.selectable_label(active, text).clicked() {
                    app.push(Action::HelpDoc(number));
                }
            }
        });
        let (_, text) =
            HELP_DOCUMENTS.get(app.extra.help_doc).or_else(|| HELP_DOCUMENTS.first()).copied().unwrap_or(("", ""));
        screen_box(body, "", |ui| markdown(ui, text));
    });
}

// ---------------------------------------------------------------- dialogs

pub fn delete_profile_dialog(app: &mut DeckelApp, ui: &mut Ui) {
    crate::ui::dialog_title(ui, "Profil löschen");
    ui.label(format!(
        "Benutzerprofil '{}' ({}) löschen? Eine Sicherung bleibt im Ordner 'backups'.",
        app.profile.display_name, app.profile.id
    ));
    let (cancelled, confirmed) = crate::ui::dialog_keys(ui, "ABBRUCH", "PROFIL LÖSCHEN", true, None);
    if cancelled {
        app.push(Action::CloseDialog);
    }
    if confirmed {
        app.push(Action::ConfirmDeleteProfile);
    }
}

pub fn mark_verified_dialog(app: &mut DeckelApp, ui: &mut Ui) {
    crate::ui::dialog_title(ui, "Profil als verifiziert markieren");
    let tester = app.extra.commissioning.as_ref().map(|r| r.tester.clone()).unwrap_or_default();
    ui.label(format!(
        "Profil '{}' als an der realen Maschine verifiziert kennzeichnen?\n\nPrüfer: {tester}\nMitgelieferte Standardprofile bleiben unverändert - es entsteht eine Kopie.",
        app.profile.display_name
    ));
    let (cancelled, confirmed) = crate::ui::dialog_keys(ui, "ABBRUCH", "ALS VERIFIZIERT MARKIEREN", true, None);
    if cancelled {
        app.push(Action::CloseDialog);
    }
    if confirmed {
        app.push(Action::CommConfirmVerify);
    }
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
