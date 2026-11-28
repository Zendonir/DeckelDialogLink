//! Commissioning assistant: steps of `docs/COMMISSIONING.md`, progress per profile, report.
//!
//! A profile can only be marked "verified" when every required step is done and a tester
//! is named. Nothing here changes machine settings; the assistant documents what the
//! operator did. File format identical to the Python version.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, SecondsFormat};
use serde_json::{json, Map, Value};

use crate::model::Profile;
use crate::storage::{atomic_write, sanitize_stem, unique_path, write_new_file, StorageError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepStatus {
    #[default]
    Open,
    Done,
    NotApplicable,
}

impl StepStatus {
    pub const ALL: [StepStatus; 3] = [StepStatus::Open, StepStatus::Done, StepStatus::NotApplicable];

    pub fn value(self) -> &'static str {
        match self {
            StepStatus::Open => "open",
            StepStatus::Done => "done",
            StepStatus::NotApplicable => "na",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StepStatus::Open => "offen",
            StepStatus::Done => "erledigt",
            StepStatus::NotApplicable => "nicht zutreffend",
        }
    }

    fn parse(text: &str) -> Self {
        Self::ALL.into_iter().find(|s| s.value() == text).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub required: bool,
    /// loopback | format | receive | send | parameters | help | ""
    pub action: &'static str,
}

const fn step(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    required: bool,
    action: &'static str,
) -> Step {
    Step { id, title, description, required, action }
}

pub const STEPS: [Step; 14] = [
    step(
        "connector",
        "Maschinenanschluss identifizieren",
        "Steckerform, Geschlecht und Belegung aus den Maschinenunterlagen ermitteln. Nichts ausprobieren.",
        true,
        "help",
    ),
    step(
        "cnc_settings",
        "Einstellungen an der CNC dokumentieren",
        "Baudrate, Datenbits, Parität, Stoppbits und Handshake der DIALOG 11 ablesen und notieren.",
        true,
        "",
    ),
    step(
        "profile",
        "Profil auswählen bzw. anlegen",
        "Profil mit den dokumentierten Werten wählen oder als Kopie anlegen (Einstellungen).",
        true,
        "",
    ),
    step(
        "cable",
        "Kabel ohne Maschine prüfen",
        "Durchgang und Belegung mit Messgerät gegen die Dokumentation prüfen; COM-Port/Adapter notieren.",
        true,
        "",
    ),
    step(
        "loopback",
        "Loopback-Test bestanden",
        "Kabel von der Maschine trennen, TX/RX brücken, Loopback-Test im Realbetrieb.",
        true,
        "loopback",
    ),
    step(
        "format",
        "Datenformat durch Mitlesen bestätigt (optional)",
        "CNC gibt ein Programm aus, DeckelLink liest nur mit und schätzt Parität/Datenbits.",
        false,
        "format",
    ),
    step(
        "receive",
        "Programm von der CNC empfangen",
        "Zuerst Empfangsrichtung: ein bekanntes Programm an der CNC ausgeben und empfangen.",
        true,
        "receive",
    ),
    step(
        "raw",
        "Rohbytes analysiert",
        "HEX-Ansicht/Terminal prüfen: Zeichen korrekt, Zeilenende, Zeichen vor dem Programm und nach M30.",
        true,
        "",
    ),
    step("backup", "Empfangene Datei gesichert", "Datei unter NC-Programme prüfen und zusätzlich sichern.", true, ""),
    step(
        "send_small",
        "Kleines, ungefährliches Testprogramm gesendet",
        "Kurzes Programm ohne kritische Bewegungen unter neuer Programmnummer senden.",
        true,
        "send",
    ),
    step(
        "verify_cnc",
        "Programm an der CNC vollständig geprüft",
        "Programm an der Steuerung mit dem Original vergleichen (Zeichen, Zeilen, Programmende).",
        true,
        "",
    ),
    step(
        "no_autostart",
        "Erster Lauf nach betrieblichen Vorgaben",
        "Ausführung nur durch den Bediener an der CNC, ohne Werkstück bzw. Probelauf/Einzelsatz.",
        true,
        "",
    ),
    step(
        "lsv2",
        "Nachladebetrieb (LSV2) geprüft (nur falls genutzt)",
        "Nach Abschnitt 12 der Inbetriebnahme-Anleitung; sonst 'nicht zutreffend'.",
        false,
        "help",
    ),
    step(
        "parameters",
        "Parameter-Sicherung erstellt (optional)",
        "Maschinenparameter auslesen und als Sicherung ablegen.",
        false,
        "parameters",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepRecord {
    pub status: StepStatus,
    pub note: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommissioningRecord {
    pub profile_id: String,
    pub tester: String,
    pub machine_info: String,
    pub started_at: String,
    pub steps: BTreeMap<String, StepRecord>,
}

fn iso_seconds(when: &DateTime<FixedOffset>) -> String {
    when.to_rfc3339_opts(SecondsFormat::Secs, false)
}

impl CommissioningRecord {
    pub fn new(profile_id: &str, now: &DateTime<FixedOffset>) -> Self {
        Self { profile_id: profile_id.into(), started_at: iso_seconds(now), ..Self::default() }
    }

    pub fn step(&self, id: &str) -> StepRecord {
        self.steps.get(id).cloned().unwrap_or_default()
    }

    pub fn set_status(&mut self, id: &str, status: StepStatus, now: &DateTime<FixedOffset>) {
        let record = self.steps.entry(id.to_string()).or_default();
        record.status = status;
        record.updated_at = iso_seconds(now);
    }

    pub fn set_note(&mut self, id: &str, note: &str) {
        self.steps.entry(id.to_string()).or_default().note = note.to_string();
    }

    pub fn missing_required(&self) -> Vec<Step> {
        STEPS.iter().filter(|s| s.required && self.step(s.id).status != StepStatus::Done).copied().collect()
    }

    pub fn can_mark_verified(&self) -> bool {
        self.missing_required().is_empty() && !self.tester.trim().is_empty()
    }

    pub fn done_count(&self) -> usize {
        STEPS.iter().filter(|s| self.step(s.id).status != StepStatus::Open).count()
    }

    pub fn to_json(&self) -> Value {
        let steps: Map<String, Value> = self
            .steps
            .iter()
            .map(|(k, v)| (k.clone(), json!({"status": v.status.value(), "note": v.note, "updated_at": v.updated_at})))
            .collect();
        json!({
            "profile_id": self.profile_id,
            "tester": self.tester,
            "machine_info": self.machine_info,
            "started_at": self.started_at,
            "steps": steps,
        })
    }

    pub fn from_json(data: &Value, profile_id: &str) -> Self {
        let text = |key: &str| data.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        let mut record = Self {
            profile_id: profile_id.into(),
            tester: text("tester"),
            machine_info: text("machine_info"),
            started_at: text("started_at"),
            steps: BTreeMap::new(),
        };
        if let Some(steps) = data.get("steps").and_then(Value::as_object) {
            for (key, value) in steps {
                if !STEPS.iter().any(|s| s.id == key) || !value.is_object() {
                    continue;
                }
                let field = |name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
                record.steps.insert(
                    key.clone(),
                    StepRecord {
                        status: StepStatus::parse(&field("status")),
                        note: field("note"),
                        updated_at: field("updated_at"),
                    },
                );
            }
        }
        record
    }
}

/// Progress per profile in `<data>/commissioning/<profile_id>.json` (previous version kept).
#[derive(Debug, Clone)]
pub struct CommissioningStore {
    pub directory: PathBuf,
}

impl CommissioningStore {
    pub fn new(root: &Path) -> Self {
        Self { directory: root.join("commissioning") }
    }

    pub fn path_for(&self, profile_id: &str) -> PathBuf {
        self.directory.join(format!("{}.json", sanitize_stem(profile_id, "profil")))
    }

    /// Stored record or a new one; unreadable files yield a new record (reported as second value).
    pub fn load(&self, profile_id: &str, now: &DateTime<FixedOffset>) -> (CommissioningRecord, Option<String>) {
        let path = self.path_for(profile_id);
        if !path.is_file() {
            return (CommissioningRecord::new(profile_id, now), None);
        }
        match fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
        {
            Ok(value) if value.is_object() => (CommissioningRecord::from_json(&value, profile_id), None),
            Ok(_) => (
                CommissioningRecord::new(profile_id, now),
                Some("Inbetriebnahme-Datei hat ein ungültiges Format".into()),
            ),
            Err(err) => {
                (CommissioningRecord::new(profile_id, now), Some(format!("Inbetriebnahme-Datei unlesbar: {err}")))
            }
        }
    }

    pub fn save(&self, record: &CommissioningRecord) -> Result<PathBuf, StorageError> {
        let path = self.path_for(&record.profile_id);
        fs::create_dir_all(&self.directory)?;
        if path.is_file() {
            fs::copy(&path, path.with_extension("json.bak"))?;
        }
        let text = serde_json::to_string_pretty(&record.to_json()).map_err(|e| StorageError::Io(e.into()))? + "\n";
        atomic_write(&path, text.as_bytes(), true)?;
        Ok(path)
    }
}

pub fn report_markdown(
    record: &CommissioningRecord,
    profile: &Profile,
    now: &DateTime<FixedOffset>,
    version: &str,
) -> String {
    let or_dash = |s: &str| if s.is_empty() { "-".to_string() } else { s.to_string() };
    let mut lines = vec![
        format!("# Inbetriebnahme-Protokoll DeckelLink {version}"),
        String::new(),
        format!("- Erstellt: {}", iso_seconds(now)),
        format!("- Prüfer: {}", or_dash(&record.tester)),
        format!("- Maschine/Steuerungsstand: {}", or_dash(&record.machine_info)),
        format!(
            "- Profil: {} (`{}`), Status: {}",
            profile.display_name,
            profile.id,
            profile.verification_status.label()
        ),
        format!("- Schnittstelle: {}, Protokoll: {}", profile.serial.summary(), profile.protocol.mode.label()),
        format!("- Begonnen: {}", or_dash(&record.started_at)),
        String::new(),
        "| # | Schritt | Pflicht | Status | Notiz | Zeitpunkt |".into(),
        "|---|---|---|---|---|---|".into(),
    ];
    for (index, step) in STEPS.iter().enumerate() {
        let rec = record.step(step.id);
        let note = rec.note.replace('|', "/").replace('\n', " ");
        lines.push(format!(
            "| {} | {} | {} | {} | {note} | {} |",
            index + 1,
            step.title,
            if step.required { "ja" } else { "nein" },
            rec.status.label(),
            rec.updated_at
        ));
    }
    lines.push(String::new());
    let missing = record.missing_required();
    if missing.is_empty() {
        lines.push("Alle Pflichtschritte sind erledigt.".into());
    } else {
        lines.push(format!(
            "Offene Pflichtschritte: {}",
            missing.iter().map(|s| s.title).collect::<Vec<_>>().join(", ")
        ));
    }
    lines.push(String::new());
    lines.push(
        "Hinweis: Dieses Protokoll dokumentiert die Inbetriebnahme der Datenübertragung. Es ersetzt keine Prüfung der NC-Programme oder der Maschine nach betrieblichen Vorgaben."
            .into(),
    );
    lines.join("\n") + "\n"
}

pub fn export_report(
    record: &CommissioningRecord,
    profile: &Profile,
    directory: &Path,
    now: &DateTime<FixedOffset>,
    version: &str,
) -> Result<PathBuf, StorageError> {
    let stem = format!("Inbetriebnahme_{}_{}", sanitize_stem(&profile.id, "profil"), now.format("%Y%m%d_%H%M%S"));
    fs::create_dir_all(directory)?;
    let target = unique_path(directory, &stem, ".md")?;
    write_new_file(&target, report_markdown(record, profile, now, version).as_bytes())?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::builtin_profiles;

    #[test]
    fn record_roundtrip_and_verification_gate() {
        let now = DateTime::parse_from_rfc3339("2026-11-27T09:00:00+01:00").unwrap();
        let mut record = CommissioningRecord::new("p", &now);
        assert_eq!(record.missing_required().len(), 11);
        for s in STEPS.iter().filter(|s| s.required) {
            record.set_status(s.id, StepStatus::Done, &now);
        }
        assert!(!record.can_mark_verified(), "tester required");
        record.tester = "M. Muster".into();
        assert!(record.can_mark_verified());
        record.set_note("raw", "CR LF | ok");
        let dir = std::env::temp_dir().join(format!("deckellink-comm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = CommissioningStore::new(&dir);
        store.save(&record).unwrap();
        store.save(&record).unwrap();
        assert!(store.path_for("p").with_extension("json.bak").is_file());
        let (loaded, problem) = store.load("p", &now);
        assert!(problem.is_none());
        assert_eq!(loaded, record);
        let profile = builtin_profiles().0.into_iter().next().unwrap();
        let report = report_markdown(&record, &profile, &now, "0.8");
        assert!(report.contains("| 8 | Rohbytes analysiert | ja | erledigt | CR LF / ok | 2026-11-27T09:00:00+01:00 |"));
        assert!(report.contains("Alle Pflichtschritte sind erledigt."));
        let path = export_report(&record, &profile, &dir, &now, "0.8").unwrap();
        assert!(path.is_file());
        let _ = fs::remove_dir_all(&dir);
    }
}
