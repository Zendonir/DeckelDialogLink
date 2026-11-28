//! State and actions of the page "Programmverwaltung" (programs, macros, tool files and the
//! tool table). Sending always goes through the transfer page with its confirmation.

use std::path::{Path, PathBuf};

use deckellink_core::clock::{Clock, SystemClock};
use deckellink_core::library::{EntryKind, LibraryEntry};
use deckellink_core::tooldb::{check_program_tools, export_tool_file, Tool, ToolExportTemplate};
use serde_json::Value;

use crate::app::{Action, DeckelApp, Dialog, Page};

pub const TOOL_COLUMNS: [&str; 9] =
    ["T-Nr.", "Bezeichnung", "Typ", "Ø mm", "Länge mm", "Schneiden", "F mm/min", "S 1/min", "Bemerkung"];
pub const PREVIEW_LIMIT: usize = 100_000;
const TEMPLATE_KEYS: [&str; 3] = ["tool_export_header", "tool_export_line", "tool_export_footer"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibTab {
    Programs,
    Tools,
}

/// Editable tool row (all columns as text).
pub type ToolRow = [String; 9];

#[derive(Debug, Clone, PartialEq)]
pub enum LibAction {
    Tab(LibTab),
    Refresh,
    Select(String),
    Import,
    NewVersion,
    SaveMeta,
    RestoreVersion(PathBuf),
    RequestDelete,
    ConfirmDelete,
    Send { drip_feed: bool },
    Simulate { edit: bool },
    ToolAdd,
    ToolRemove(usize),
    ToolSave,
    ToolReload,
    TemplateSave,
    ToolExport,
}

#[derive(Debug, Clone, Default)]
pub struct LibState {
    pub tab: Option<LibTab>,
    pub entries: Vec<LibraryEntry>,
    pub selected: Option<String>,
    pub name: String,
    pub kind: Option<EntryKind>,
    pub number: String,
    pub description: String,
    pub preview: String,
    pub versions: Vec<PathBuf>,
    pub tool_rows: Vec<ToolRow>,
    pub template: ToolExportTemplate,
    pub status: Option<(bool, String)>,
    pub tool_status: Option<(bool, String)>,
}

impl LibState {
    pub fn tab(&self) -> LibTab {
        self.tab.unwrap_or(LibTab::Programs)
    }

    pub fn selected_entry(&self) -> Option<&LibraryEntry> {
        let id = self.selected.as_deref()?;
        self.entries.iter().find(|e| e.id == id)
    }
}

pub fn tool_row(tool: &Tool) -> ToolRow {
    let num = deckellink_core::sim::py_g;
    [
        tool.number.to_string(),
        tool.name.clone(),
        tool.kind.clone(),
        num(tool.diameter),
        num(tool.length),
        tool.flutes.to_string(),
        num(tool.feed),
        num(tool.speed),
        tool.note.clone(),
    ]
}

fn parse_float(text: &str) -> Result<f64, String> {
    let text = text.trim().replace(',', ".");
    if text.is_empty() {
        return Ok(0.0);
    }
    text.parse::<f64>().map_err(|_| format!("'{text}' ist keine Zahl"))
}

fn parse_int(text: &str) -> Result<i64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(0);
    }
    text.parse::<i64>().map_err(|_| format!("'{text}' ist keine ganze Zahl"))
}

/// Tools of the table rows; `strict` reports the first invalid row.
pub fn table_tools(rows: &[ToolRow], strict: bool) -> Result<Vec<Tool>, String> {
    let mut tools = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let [number, name, kind, diameter, length, flutes, feed, speed, note] = row;
        let parsed = (|| -> Result<Tool, String> {
            let number = number.trim();
            if number.is_empty() {
                return Err("Werkzeugnummer fehlt".into());
            }
            Ok(Tool {
                number: parse_int(number)?,
                name: name.trim().to_string(),
                kind: if kind.trim().is_empty() { "Fräser".into() } else { kind.trim().to_string() },
                diameter: parse_float(diameter)?,
                length: parse_float(length)?,
                flutes: parse_int(flutes)?,
                feed: parse_float(feed)?,
                speed: parse_float(speed)?,
                note: note.trim().to_string(),
            })
        })();
        match parsed {
            Ok(tool) => tools.push(tool),
            Err(err) if strict => return Err(format!("Zeile {}: ungültiger Wert ({err})", index + 1)),
            Err(_) => {}
        }
    }
    Ok(tools)
}

/// Tool check text of an entry (as in the Python version).
pub fn tool_check_text(entry: &LibraryEntry, tools: &[Tool]) -> (bool, String) {
    if entry.kind == EntryKind::Tools {
        return (
            false,
            "Werkzeugdatei: Beim Einlesen an der Steuerung werden Werkzeugdaten überschrieben. Nur Dateien senden, \
             deren Format von der Steuerung stammt bzw. geprüft ist."
                .into(),
        );
    }
    if entry.tools.is_empty() {
        return (true, "Keine Werkzeugaufrufe gefunden.".into());
    }
    let missing = check_program_tools(&entry.tools, tools);
    if missing.is_empty() {
        (true, format!("Alle Werkzeuge in der Werkzeugtabelle ({})", entry.tools.join(", ")))
    } else {
        (false, format!("Nicht in der Werkzeugtabelle: {} (verwendet: {})", missing.join(", "), entry.tools.join(", ")))
    }
}

impl DeckelApp {
    fn now(&self) -> chrono::DateTime<chrono::FixedOffset> {
        SystemClock::default().now()
    }

    /// Reload the entry list (keeps the selection when possible).
    pub fn refresh_library(&mut self, select: Option<String>) {
        self.lib.entries = self.library.entries();
        let wanted = select.or_else(|| self.lib.selected.clone());
        self.lib.selected = wanted.filter(|id| self.lib.entries.iter().any(|e| &e.id == id));
        self.show_selected_entry();
    }

    fn show_selected_entry(&mut self) {
        let Some(entry) = self.lib.selected_entry().cloned() else {
            self.lib.name.clear();
            self.lib.number.clear();
            self.lib.description.clear();
            self.lib.kind = None;
            self.lib.preview.clear();
            self.lib.versions.clear();
            return;
        };
        self.lib.name = entry.name.clone();
        self.lib.kind = Some(entry.kind);
        self.lib.number = entry.number.clone();
        self.lib.description = entry.description.clone();
        self.lib.preview = match entry.read() {
            Ok(data) => {
                let shown = data.get(..PREVIEW_LIMIT).unwrap_or(&data);
                let mut text = deckellink_core::library::decode_ascii(shown);
                if data.len() > PREVIEW_LIMIT {
                    text.push_str("\n… (gekürzt)");
                }
                text
            }
            Err(err) => format!("Datei nicht lesbar: {err}"),
        };
        self.lib.versions = self.library.versions(&entry);
    }

    pub fn load_tools(&mut self) {
        self.tool_db.load();
        self.lib.tool_rows = self.tool_db.tools.iter().map(tool_row).collect();
        let count = self.tool_db.tools.len();
        let mut text =
            if count > 0 { format!("{count} Werkzeug(e) geladen.") } else { "Noch keine Werkzeuge angelegt.".into() };
        let ok = self.tool_db.problems.is_empty();
        if !ok {
            text.push_str(&format!(" {}", self.tool_db.problems.join("; ")));
        }
        self.lib.tool_status = Some((ok, text));
        let get = |key: &str| self.config.extra.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        self.lib.template = ToolExportTemplate {
            header: get(TEMPLATE_KEYS[0]),
            line: get(TEMPLATE_KEYS[1]),
            footer: get(TEMPLATE_KEYS[2]),
        };
    }

    /// Notes for the send confirmation of a library entry (tool files overwrite machine data).
    pub fn library_send_notes(&self, path: &Path) -> Vec<String> {
        if path.parent() != Some(self.library.root.as_path()) {
            return Vec::new();
        }
        let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(entry) = self.library.get(&id).filter(|e| e.kind == EntryKind::Tools) else { return Vec::new() };
        let mut notes = vec![
            "WERKZEUGDATEI: Beim Einlesen an der Steuerung werden Werkzeugdaten (Längen, Radien) überschrieben. Werte \
             vor der Bearbeitung an der Maschine kontrollieren."
                .to_string(),
        ];
        if entry.source.contains("Vorlage") {
            notes
                .push("Diese Werkzeugdatei wurde aus einer Vorlage erzeugt - das Format ist NICHT verifiziert.".into());
        }
        notes
    }

    /// Record a completed transfer of a library entry.
    pub(crate) fn mark_library_sent(&mut self, path: &Path, simulated: bool) {
        if path.parent() != Some(self.library.root.as_path()) {
            return;
        }
        let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(entry) = self.library.get(&id) else { return };
        let now = self.now();
        if let Err(err) = self.library.mark_sent(&entry, &now, if simulated { "Simulator" } else { "real" }) {
            self.log.warn(format!("Programmverwaltung: Sendezeit nicht gespeichert: {err}"));
        }
        self.refresh_library(None);
    }

    /// Simulator tool table from the tool management (Python: radius = Ø/2).
    pub(crate) fn sim_tools_from_db(&mut self) {
        self.tool_db.load();
        if self.tool_db.tools.is_empty() {
            return;
        }
        let num = deckellink_core::sim::py_g;
        let mut rows: Vec<crate::sim_app::ToolRow> =
            self.tool_db.tools.iter().map(|t| [t.number.to_string(), num(t.radius()), num(t.length)]).collect();
        rows.extend((0..4).map(|_| Default::default()));
        self.sim.tools = rows;
    }

    pub(crate) fn handle_lib(&mut self, action: LibAction) {
        let now = self.now();
        match action {
            LibAction::Tab(tab) => self.lib.tab = Some(tab),
            LibAction::Refresh => self.refresh_library(None),
            LibAction::Select(id) => {
                self.lib.selected = Some(id);
                self.show_selected_entry();
            }
            LibAction::Import => {
                let picked = rfd::FileDialog::new()
                    .set_title("Dateien in die Programmverwaltung übernehmen")
                    .add_filter("NC-Programme", &["nc", "NC", "txt", "TXT", "tap", "d11", "mpf", "MPF"])
                    .add_filter("Alle Dateien", &["*"])
                    .pick_files()
                    .unwrap_or_default();
                for path in picked {
                    self.library_import(&path);
                }
            }
            LibAction::NewVersion => {
                let Some(entry) = self.lib.selected_entry().cloned() else { return };
                let picked = rfd::FileDialog::new().set_title(format!("Neue Version für {}", entry.name)).pick_file();
                if let Some(path) = picked {
                    self.library_new_version(&entry, &path);
                }
            }
            LibAction::SaveMeta => {
                let Some(entry) = self.lib.selected_entry().cloned() else { return };
                let name = if self.lib.name.trim().is_empty() { entry.id.clone() } else { self.lib.name.clone() };
                let kind = self.lib.kind.unwrap_or(entry.kind);
                match self.library.update_meta(&entry, &name, kind, &self.lib.number, &self.lib.description) {
                    Ok(_) => self.lib.status = Some((true, "Angaben gespeichert.".into())),
                    Err(err) => self.lib.status = Some((false, format!("Nicht gespeichert: {err}"))),
                }
                self.refresh_library(Some(entry.id));
            }
            LibAction::RestoreVersion(path) => {
                let Some(entry) = self.lib.selected_entry().cloned() else { return };
                match self.library.restore_version(&entry, &path, &now) {
                    Ok(_) => self.lib.status = Some((true, "Version wiederhergestellt (als neue Version).".into())),
                    Err(err) => self.lib.status = Some((false, format!("Version wiederherstellen: {err}"))),
                }
                self.refresh_library(Some(entry.id));
            }
            LibAction::RequestDelete => {
                if self.lib.selected_entry().is_some() {
                    self.dialog = Some(Dialog::DeleteLibraryEntry);
                }
            }
            LibAction::ConfirmDelete => {
                self.dialog = None;
                let Some(entry) = self.lib.selected_entry().cloned() else { return };
                match self.library.delete(&entry, &now) {
                    Ok(target) => {
                        self.notify(format!(
                            "'{}' in den Papierkorb verschoben ({})",
                            entry.name,
                            target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                        ));
                        self.lib.selected = None;
                    }
                    Err(err) => self.lib.status = Some((false, format!("Löschen nicht möglich: {err}"))),
                }
                self.refresh_library(None);
            }
            LibAction::Send { drip_feed } => {
                let Some(entry) = self.lib.selected_entry().cloned() else { return };
                if self.busy() {
                    return;
                }
                self.load_file(&entry.path);
                self.page = Page::Transfer;
                if drip_feed {
                    if self.program.as_ref().is_some_and(|p| p.payload.is_some()) {
                        self.push(Action::RequestSend { drip_feed: true });
                    } else {
                        self.message = Some("Nachladebetrieb nicht möglich: Datei hat Prüffehler".into());
                    }
                }
            }
            LibAction::Simulate { edit } => {
                let Some(entry) = self.lib.selected_entry().cloned() else { return };
                match entry.read() {
                    Ok(data) => {
                        self.sim_tools_from_db();
                        let text = deckellink_core::library::decode_ascii(&data);
                        self.sim.load_text(text, &entry.name);
                        self.sim.library_entry = edit.then(|| entry.id.clone());
                        self.sim.set_mode(if edit { 3 } else { 2 });
                        self.page = Page::Simulator;
                    }
                    Err(err) => self.lib.status = Some((false, format!("Datei nicht lesbar: {err}"))),
                }
            }
            LibAction::ToolAdd => {
                let numbers: Vec<i64> =
                    table_tools(&self.lib.tool_rows, false).unwrap_or_default().iter().map(|t| t.number).collect();
                let number = (1..10_000).find(|n| !numbers.contains(n)).unwrap_or(1);
                self.lib.tool_rows.push(tool_row(&Tool::new(number)));
            }
            LibAction::ToolRemove(index) => {
                if index < self.lib.tool_rows.len() {
                    self.lib.tool_rows.remove(index);
                }
            }
            LibAction::ToolSave => {
                let result = table_tools(&self.lib.tool_rows, true)
                    .and_then(|tools| self.tool_db.save(&tools, &now).map_err(|e| e.0));
                self.lib.tool_status = Some(match result {
                    Ok(()) => (true, format!("{} Werkzeug(e) gespeichert.", self.tool_db.tools.len())),
                    Err(err) => (false, format!("Nicht gespeichert: {err}")),
                });
                if self.lib.tool_status.as_ref().is_some_and(|s| s.0) {
                    self.lib.tool_rows = self.tool_db.tools.iter().map(tool_row).collect();
                }
            }
            LibAction::ToolReload => self.load_tools(),
            LibAction::TemplateSave => {
                let template = self.lib.template.clone();
                for (key, value) in TEMPLATE_KEYS.iter().zip([template.header, template.line, template.footer]) {
                    self.config.extra.insert((*key).to_string(), Value::String(value));
                }
                self.save_config();
                self.lib.tool_status = Some((true, "Vorlage gespeichert (nicht verifiziert).".into()));
            }
            LibAction::ToolExport => self.export_tool_file(),
        }
    }

    pub fn library_import(&mut self, path: &Path) -> Option<LibraryEntry> {
        let now = self.now();
        match self.library.import_file(path, None, &now) {
            Ok(entry) => {
                self.log.info(format!("Programmverwaltung: {} übernommen als {}", entry.source, entry.id));
                self.lib.status = Some((true, format!("'{}' übernommen.", entry.name)));
                self.refresh_library(Some(entry.id.clone()));
                Some(entry)
            }
            Err(err) => {
                self.lib.status = Some((false, format!("Importieren: {err}")));
                None
            }
        }
    }

    pub fn library_new_version(&mut self, entry: &LibraryEntry, path: &Path) -> bool {
        let now = self.now();
        let result = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|data| self.library.save_version(entry, &data, &now).map_err(|e| e.0));
        let ok = result.is_ok();
        self.lib.status = Some(match result {
            Ok(_) => (true, "Neue Version gespeichert; die vorherige liegt unter 'Frühere Versionen'.".into()),
            Err(err) => (false, format!("Neue Version: {err}")),
        });
        self.refresh_library(Some(entry.id.clone()));
        ok
    }

    /// Save the simulator text as a new version of its library entry.
    pub(crate) fn sim_save_to_library(&mut self) {
        let Some(id) = self.sim.library_entry.clone() else { return };
        let Some(entry) = self.library.get(&id) else {
            self.message = Some("Eintrag existiert nicht mehr.".into());
            return;
        };
        let data = match self.sim.export_bytes() {
            Ok(data) => data,
            Err(err) => {
                self.message = Some(err);
                return;
            }
        };
        let now = self.now();
        match self.library.save_version(&entry, &data, &now) {
            Ok(_) => self.notify(format!("Als neue Version von '{}' gespeichert", entry.name)),
            Err(err) => self.message = Some(format!("Speichern fehlgeschlagen: {err}")),
        }
        self.refresh_library(Some(id));
    }

    fn export_tool_file(&mut self) {
        let now = self.now();
        let result = table_tools(&self.lib.tool_rows, true)
            .and_then(|tools| export_tool_file(&tools, &self.lib.template).map_err(|e| e.0))
            .and_then(|data| {
                self.library
                    .import_bytes(
                        "WERKZEUGE_VORLAGE.nc",
                        &data,
                        Some(EntryKind::Tools),
                        "Vorlage (nicht verifiziert)",
                        &now,
                    )
                    .map_err(|e| e.0)
            })
            .and_then(|entry| {
                self.library
                    .update_meta(&entry, &entry.name, entry.kind, "", "Aus Vorlage erzeugt - Format NICHT verifiziert")
                    .map_err(|e| e.0)
            });
        match result {
            Ok(entry) => {
                self.lib.tool_status =
                    Some((true, format!("Werkzeugdatei '{}' in der Programmverwaltung angelegt.", entry.name)));
                self.lib.tab = Some(LibTab::Programs);
                self.refresh_library(Some(entry.id));
            }
            Err(err) => self.lib.tool_status = Some((false, err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_rows_parse_and_report() {
        let rows = vec![tool_row(&Tool::new(3)), {
            let mut row = tool_row(&Tool::new(4));
            row[3] = "8,5".into();
            row
        }];
        let tools = table_tools(&rows, true).unwrap();
        assert!((tools[1].diameter - 8.5).abs() < 1e-12);
        let mut bad = rows.clone();
        bad[0][3] = "x".into();
        assert_eq!(table_tools(&bad, true).unwrap_err(), "Zeile 1: ungültiger Wert ('x' ist keine Zahl)");
        assert_eq!(table_tools(&bad, false).unwrap().len(), 1);
    }
}
