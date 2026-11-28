//! Drawing of the page "Programmverwaltung" (inside the dark screen; actions on F1-F8).

use egui::{Color32, RichText, ScrollArea, Sense, Ui, Vec2};

use deckellink_core::library::EntryKind;
use deckellink_core::tooldb::PLACEHOLDERS;

use crate::app::{Action, DeckelApp};
use crate::lib_app::{tool_check_text, LibAction, LibTab, TOOL_COLUMNS};
use crate::logic::format_bytes;
use crate::theme::{message, mono_bold, screen_box, screen_key, Tone, DIM, GREEN, ORANGE, SCREEN_FIELD, TEXT, YELLOW};

fn lib(app: &mut DeckelApp, action: LibAction) {
    app.push(Action::Lib(action));
}

pub fn page(app: &mut DeckelApp, ui: &mut Ui) {
    ui.label(
        RichText::new(
            "Programme, Makros und Werkzeugdateien auf dem PC verwalten. Jede Änderung legt eine neue Version an, \
             Löschen verschiebt in den Papierkorb. An die Maschine geht eine Datei nur über die Seite Übertragen mit \
             der üblichen Bestätigung.",
        )
        .color(DIM),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        for (tab, label) in [(LibTab::Programs, "PROGRAMME"), (LibTab::Tools, "WERKZEUGE")] {
            let selected = app.lib.tab() == tab;
            let button =
                egui::Button::new(RichText::new(label).monospace().color(if selected { Color32::BLACK } else { TEXT }))
                    .fill(if selected { YELLOW } else { SCREEN_FIELD });
            if ui.add(button).clicked() {
                lib(app, LibAction::Tab(tab));
            }
        }
    });
    ui.add_space(6.0);
    match app.lib.tab() {
        LibTab::Programs => programs(app, ui),
        LibTab::Tools => tools(app, ui),
    }
}

fn status_line(ui: &mut Ui, status: &Option<(bool, String)>) {
    if let Some((ok, text)) = status {
        message(ui, if *ok { Tone::Ok } else { Tone::Error }, text, |_| {});
    }
}

fn programs(app: &mut DeckelApp, ui: &mut Ui) {
    status_line(ui, &app.lib.status.clone());
    let width = ui.available_width();
    let list_width = (width * 0.55).max(320.0);
    let height = ui.available_height().max(300.0);
    ui.horizontal_top(|ui| {
        let column = egui::Layout::top_down(egui::Align::Min);
        ui.allocate_ui_with_layout(Vec2::new(list_width, height), column, |ui| {
            ui.set_width(list_width);
            entry_list(app, ui);
        });
        let detail_width = (ui.available_width()).max(260.0);
        ui.allocate_ui_with_layout(Vec2::new(detail_width, height), column, |ui| {
            ui.set_width(detail_width);
            entry_detail(app, ui);
        });
    });
}

fn entry_list(app: &mut DeckelApp, ui: &mut Ui) {
    screen_box(ui, &format!("Programmverwaltung ({} Einträge)", app.lib.entries.len()), |ui| {
        if app.lib.entries.is_empty() {
            ui.label(RichText::new("Noch keine Einträge. F1 IMPORTIEREN übernimmt Dateien.").monospace().color(DIM));
            return;
        }
        let mut clicked = None;
        ScrollArea::both().id_salt("lib-list").auto_shrink([false, true]).max_height(440.0).show(ui, |ui| {
            egui::Grid::new("lib-entries").num_columns(6).spacing([12.0, 3.0]).striped(true).show(ui, |ui| {
                for title in ["Name", "Art", "Nr.", "Werkzeuge", "Größe", "Zuletzt gesendet"] {
                    ui.label(RichText::new(title).monospace().color(YELLOW));
                }
                ui.end_row();
                for entry in &app.lib.entries {
                    let selected = app.lib.selected.as_deref() == Some(entry.id.as_str());
                    let name = RichText::new(&entry.name).monospace();
                    let name =
                        if selected { name.color(Color32::BLACK).background_color(YELLOW) } else { name.color(TEXT) };
                    if ui.add(egui::Label::new(name).sense(Sense::click())).clicked() {
                        clicked = Some(entry.id.clone());
                    }
                    ui.label(RichText::new(entry.kind.label()).monospace().color(DIM));
                    ui.label(RichText::new(&entry.number).monospace().color(TEXT));
                    ui.label(RichText::new(entry.tools.join(" ")).monospace().color(TEXT));
                    ui.label(
                        RichText::new(format_bytes(usize::try_from(entry.size).unwrap_or(0))).monospace().color(DIM),
                    );
                    let sent = if entry.last_sent_at.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "{} ({})",
                            entry.last_sent_at.replace('T', " ").get(..19).unwrap_or(""),
                            entry.last_sent_mode
                        )
                    };
                    ui.label(RichText::new(sent).monospace().color(DIM));
                    ui.end_row();
                }
            });
        });
        if let Some(id) = clicked {
            lib(app, LibAction::Select(id));
        }
    });
}

fn entry_detail(app: &mut DeckelApp, ui: &mut Ui) {
    let Some(entry) = app.lib.selected_entry().cloned() else {
        screen_box(ui, "Eintrag", |ui| {
            ui.label(RichText::new("Links einen Eintrag wählen.").monospace().color(DIM));
        });
        return;
    };
    screen_box(ui, "Angaben", |ui| {
        egui::Grid::new("lib-meta").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new("Name:").monospace().color(DIM));
            ui.add(egui::TextEdit::singleline(&mut app.lib.name).desired_width(240.0));
            ui.end_row();
            ui.label(RichText::new("Art:").monospace().color(DIM));
            let current = app.lib.kind.unwrap_or(entry.kind);
            egui::ComboBox::from_id_salt("lib-kind").selected_text(current.label()).show_ui(ui, |ui| {
                for kind in EntryKind::ALL {
                    if ui.selectable_label(current == kind, kind.label()).clicked() {
                        app.lib.kind = Some(kind);
                    }
                }
            });
            ui.end_row();
            ui.label(RichText::new("Nummer:").monospace().color(DIM));
            ui.add(
                egui::TextEdit::singleline(&mut app.lib.number)
                    .hint_text("Programmnummer an der Steuerung, z. B. 17")
                    .desired_width(240.0),
            );
            ui.end_row();
            ui.label(RichText::new("Beschreibung:").monospace().color(DIM));
            ui.add(egui::TextEdit::singleline(&mut app.lib.description).desired_width(240.0));
            ui.end_row();
        });
        if ui.add(screen_key("Angaben speichern")).clicked() {
            lib(app, LibAction::SaveMeta);
        }
        ui.add_space(4.0);
        let (ok, text) = tool_check_text(&entry, &app.tool_db.tools);
        ui.add(egui::Label::new(RichText::new(text).monospace().color(if ok { GREEN } else { ORANGE })).wrap());
        ui.label(
            RichText::new(format!(
                "Datei {}  |  geändert {}  |  SHA-256 {}",
                entry.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                entry.updated_at.replace('T', " ").get(..19).unwrap_or(""),
                entry.sha256.get(..16).unwrap_or("")
            ))
            .monospace()
            .size(11.5)
            .color(DIM),
        );
    });
    screen_box(ui, "Inhalt", |ui| {
        ScrollArea::both().id_salt("lib-preview").max_height(220.0).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(&app.lib.preview).monospace().color(TEXT)).extend());
        });
    });
    screen_box(ui, "Frühere Versionen (neueste zuerst)", |ui| {
        if app.lib.versions.is_empty() {
            ui.label(RichText::new("keine").monospace().color(DIM));
        }
        let mut restore = None;
        for path in &app.lib.versions {
            ui.horizontal(|ui| {
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                ui.label(RichText::new(stem).monospace().color(TEXT));
                if ui.add(screen_key("wiederherstellen")).on_hover_text("Als neue Version übernehmen").clicked() {
                    restore = Some(path.clone());
                }
            });
        }
        if let Some(path) = restore {
            lib(app, LibAction::RestoreVersion(path));
        }
    });
}

fn tools(app: &mut DeckelApp, ui: &mut Ui) {
    status_line(ui, &app.lib.tool_status.clone());
    screen_box(ui, "Werkzeugtabelle", |ui| {
        ui.label(
            RichText::new(
                "Werkzeugtabelle von DeckelLink. Sie wird vom Simulator (Radius = Ø/2) und für die Werkzeugprüfung der \
                 Programme verwendet. Speichern legt vorher eine Sicherung an.",
            )
            .color(DIM),
        );
        let mut remove = None;
        ScrollArea::horizontal().id_salt("tool-table").show(ui, |ui| {
            egui::Grid::new("tool-grid").num_columns(TOOL_COLUMNS.len() + 1).spacing([6.0, 3.0]).show(ui, |ui| {
                for title in TOOL_COLUMNS {
                    ui.label(RichText::new(title).monospace().color(YELLOW));
                }
                ui.label("");
                ui.end_row();
                for (index, row) in app.lib.tool_rows.iter_mut().enumerate() {
                    for (column, cell) in row.iter_mut().enumerate() {
                        let width = match column {
                            1 | 8 => 130.0,
                            2 => 90.0,
                            0 | 5 => 52.0,
                            _ => 72.0,
                        };
                        ui.add_sized([width, 22.0], egui::TextEdit::singleline(cell).font(egui::TextStyle::Monospace));
                    }
                    if ui.add(screen_key("entf.")).clicked() {
                        remove = Some(index);
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(index) = remove {
            lib(app, LibAction::ToolRemove(index));
        }
    });
    screen_box(ui, "Werkzeugdatei für die Steuerung (Vorlage, NICHT verifiziert)", |ui| {
        message(ui, Tone::Warn, "Format der DIALOG 11 nicht bekannt", |ui| {
            ui.label(format!(
                "Empfohlen: Werkzeugdatei an der Steuerung ausgeben, auf der Seite Empfangen aufnehmen und hier als \
                 „Werkzeugdatei“ importieren. Diese Datei kann unverändert zurückgesendet werden. Eine Vorlage nur nach \
                 diesem Muster einrichten. Platzhalter: {}",
                PLACEHOLDERS.join(" ")
            ));
        });
        egui::Grid::new("tool-template").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new("Kopfzeilen:").monospace().color(DIM));
            ui.add(
                egui::TextEdit::multiline(&mut app.lib.template.header)
                    .desired_rows(2)
                    .font(egui::TextStyle::Monospace),
            );
            ui.end_row();
            ui.label(RichText::new("Zeile je Werkzeug:").monospace().color(DIM));
            ui.add(
                egui::TextEdit::singleline(&mut app.lib.template.line)
                    .desired_width(420.0)
                    .font(egui::TextStyle::Monospace),
            );
            ui.end_row();
            ui.label(RichText::new("Schlusszeilen:").monospace().color(DIM));
            ui.add(
                egui::TextEdit::multiline(&mut app.lib.template.footer)
                    .desired_rows(2)
                    .font(egui::TextStyle::Monospace),
            );
            ui.end_row();
        });
    });
    ui.label(
        mono_bold(
            "F1 hinzufügen · F2 speichern · F3 verwerfen · F4 Vorlage speichern · F5 Werkzeugdatei erzeugen",
            12.0,
        )
        .color(DIM),
    );
}

/// Confirmation before moving an entry into the recycle bin.
pub fn delete_dialog(app: &mut DeckelApp, ui: &mut Ui) {
    crate::ui::dialog_title(ui, "In Papierkorb");
    let name = app.lib.selected_entry().map(|e| e.name.clone()).unwrap_or_default();
    ui.label(format!(
        "'{name}' in den Papierkorb verschieben?\n(Ordner 'deleted' in der Programmverwaltung, nichts wird endgültig \
         gelöscht.)"
    ));
    let (cancel, confirm) = crate::ui::dialog_keys(ui, "ABBRUCH", "IN PAPIERKORB", true, None);
    if cancel {
        app.push(Action::CloseDialog);
    } else if confirm {
        lib(app, LibAction::ConfirmDelete);
    }
}
