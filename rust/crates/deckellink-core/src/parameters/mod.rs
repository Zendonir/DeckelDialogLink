//! Machine parameter files: analysis, comparison and the write gate.
//!
//! DeckelLink treats machine parameters as data files:
//!
//! * reading = receiving the parameter output the operator starts at the CNC,
//! * writing = sending a parameter file into a parameter input that the authorised
//!   operator has enabled at the CNC (key switch / code number).
//!
//! No remote parameter commands are sent. The file structure is checked against
//! the format observed on a real FP2A (see [`fileformat`]); it is not an official
//! specification. Parameter data is transferred with minimal changes (only NUL
//! removal and line-ending normalisation).
//!
//! The write blocks (profile `parameters.allow_write` off by default, unlock per
//! session only, current backup, diff, typed confirmation, verification by reading
//! again) are identical to the Python version and must not be weakened.

pub mod archive;
pub mod difflib;
pub mod fileformat;
pub mod transfer;

use std::cmp::Reverse;

use chrono::{DateTime, FixedOffset};

use crate::error::TransformError;
use crate::model::{CommentHandling, EndDetection, Profile, TabHandling, TransferSettings, WhitespaceHandling};
use crate::nc::analyzer::analyze;
use crate::nc::transformer::transform;
use crate::nc::{AnalysisResult, Finding, Severity, TransformResult};
use crate::profile::compile_pattern;
use crate::storage::sha256_hex;
use fileformat::{compatibility_problems, decode_ascii, parse_parameter_file, ParameterFileInfo};

/// Text that must be typed before parameters are written.
pub const WRITE_CONFIRMATION_PHRASE: &str = "PARAMETER SCHREIBEN";
pub const MAX_DIFF_LINES: usize = 400;

/// Safety notice shown before the write function is unlocked (same text as Python).
pub const PARAMETER_WRITE_NOTICE: &str = "MASCHINENPARAMETER: Falsche Parameter können Kollisionen, Schäden an \
Maschine und Werkstück sowie Gefahren für Personen verursachen. DeckelLink prüft Parameter nicht inhaltlich - das \
Parameterformat der DIALOG 11 ist nicht verifiziert. Schreiben Sie nur Parameter, deren Bedeutung Sie kennen, nach \
den Vorgaben des Maschinenherstellers und mit Freigabe der verantwortlichen Person. Lesen Sie die Parameter nach dem \
Schreiben erneut aus und vergleichen Sie sie, bevor die Maschine verfährt.";

/// Clearly artificial parameter list for the simulator (same bytes as the Python version).
///
/// This is NOT the DIALOG 11 parameter format (which is not verified) - it only exercises
/// the data path. Structure as observed on a real DIALOG 11; the checksums are placeholders.
pub fn sample_parameters() -> Vec<u8> {
    fn group(number: u32, name: &str, lines: &[String]) -> String {
        format!("%MPG{number:02} ({name})\r\n{}?\r\nC{number:03}\r\n", lines.concat())
    }
    let header: Vec<String> =
        ["M65000 SIMULATION", "M65002 SIM     0.000", "M65033 SIMULATOR", "M65034 0000-0000", "M65035 01.01.26"]
            .iter()
            .map(|line| format!("{line}\r\n"))
            .collect();
    let values = |group_no: u32, factor: u32| -> Vec<String> {
        (1..=20u32).map(|n| format!("M{group_no:02}{n:03} {}\r\n", (n * factor) % 1000)).collect()
    };
    let mut text = group(0, "KOPF", &header);
    text.push_str(&group(1, "SIMULATION", &values(1, 37)));
    text.push_str(&group(2, "SIMULATION", &values(2, 53)));
    text.push_str("\x04\r\n");
    text.into_bytes()
}

/// Transfer settings for parameter data: no content changes except NUL/line endings.
pub fn parameter_transfer_settings(profile: &Profile) -> TransferSettings {
    TransferSettings {
        program_start: String::new(),
        program_end: String::new(),
        append_program_end: false,
        remove_blank_lines: false,
        skip_semicolon_lines: false,
        normalize_line_endings: true,
        strip_null_bytes: true,
        tab_handling: TabHandling::Keep,
        whitespace: WhitespaceHandling::None,
        comment_handling: CommentHandling::Keep,
        ensure_final_newline: false,
        max_line_length: 0,
        max_program_bytes: profile.parameters.max_bytes,
        ..profile.transfer.clone()
    }
}

/// Profile variant used for parameter transfers (inactivity end, no M30 logic).
pub fn parameter_profile(profile: &Profile) -> Profile {
    let p = &profile.parameters;
    let mut effective = profile.clone();
    effective.transfer = parameter_transfer_settings(profile);
    effective.receive.end_detection = EndDetection::Inactivity;
    effective.receive.inactivity_timeout_ms = p.inactivity_timeout_ms;
    effective.receive.first_byte_timeout_ms = p.first_byte_timeout_ms;
    effective.receive.max_receive_bytes = p.max_bytes;
    effective.receive.program_name_pattern = String::new();
    effective
}

/// Python `repr()` of a string (quotes and escapes as Python prints them).
pub fn py_str_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if u32::from(c) < 0x20 || (0x7f..0xa0).contains(&u32::from(c)) => {
                out.push_str(&format!("\\x{:02x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Analysis of a parameter file (character checks plus optional identification pattern and structure).
pub fn analyze_parameters(data: &[u8], profile: &Profile) -> AnalysisResult {
    let mut result = analyze(data, &parameter_transfer_settings(profile), Some(&profile.serial));
    // Tabs and blank lines are kept unchanged in parameter files - informational only.
    for finding in &mut result.findings {
        if finding.code == "TABS" || finding.code == "SEMICOLON_LINES" {
            finding.severity = Severity::Info;
        }
    }
    let pattern = &profile.parameters.identification_pattern;
    if !data.is_empty() && !pattern.is_empty() {
        let text = decode_ascii(data);
        // An invalid pattern is rejected when the profile is loaded; treat it as "no match" here.
        let matched = compile_pattern(pattern).is_ok_and(|re| re.is_match(&text));
        if !matched {
            result.findings.push(Finding {
                code: "NOT_A_PARAMETER_FILE".into(),
                severity: Severity::Error,
                message: format!(
                    "Die Datei entspricht nicht dem im Profil hinterlegten Muster für Parameterdateien ({}).",
                    py_str_repr(pattern)
                ),
                lines: Vec::new(),
                count: 0,
            });
        }
    }
    if !data.is_empty() {
        if let Some(info) = parse_parameter_file(data) {
            result.findings.extend(info.findings);
        }
    }
    result.findings.sort_by_key(|f| Reverse(f.severity.rank()));
    result
}

pub fn prepare_parameter_payload(data: &[u8], profile: &Profile) -> Result<TransformResult, TransformError> {
    transform(data, &parameter_transfer_settings(profile), false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterDiff {
    pub changed_lines: usize,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub diff_text: String,
    pub truncated: bool,
}

impl ParameterDiff {
    pub fn identical(&self) -> bool {
        self.changed_lines == 0 && self.added_lines == 0 && self.removed_lines == 0
    }

    pub fn summary(&self) -> String {
        if self.identical() {
            return "Keine Unterschiede".into();
        }
        format!(
            "{} geänderte, {} hinzugefügte, {} entfernte Zeile(n)",
            self.changed_lines, self.added_lines, self.removed_lines
        )
    }
}

/// Python `str.splitlines()` for ASCII text (after CR/CRLF were turned into LF).
fn split_text_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .split(['\n', '\u{0B}', '\u{0C}', '\u{1C}', '\u{1D}', '\u{1E}', '\u{85}', '\u{2028}', '\u{2029}'])
        .map(String::from)
        .collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn diff_lines(data: &[u8]) -> Vec<String> {
    let cleaned: Vec<u8> = data.iter().copied().filter(|b| *b != 0).collect();
    split_text_lines(&decode_ascii(&cleaned).replace("\r\n", "\n").replace('\r', "\n"))
}

/// Line based comparison (line endings and NUL bytes are ignored).
pub fn diff_parameters(old: &[u8], new: &[u8], old_name: &str, new_name: &str) -> ParameterDiff {
    let a = diff_lines(old);
    let b = diff_lines(new);
    let (mut changed, mut added, mut removed) = (0usize, 0usize, 0usize);
    for code in difflib::SequenceMatcher::new(&a, &b, false).opcodes() {
        let (left, right) = (code.i2 - code.i1, code.j2 - code.j1);
        match code.tag {
            difflib::Tag::Replace => {
                let common = left.min(right);
                changed += common;
                removed += left - common;
                added += right - common;
            }
            difflib::Tag::Delete => removed += left,
            difflib::Tag::Insert => added += right,
            difflib::Tag::Equal => {}
        }
    }
    let diff = difflib::unified_diff(&a, &b, old_name, new_name, 1);
    let truncated = diff.len() > MAX_DIFF_LINES;
    let mut text = diff.iter().take(MAX_DIFF_LINES).cloned().collect::<Vec<_>>().join("\n");
    if truncated {
        text.push_str(&format!("\n... ({} weitere Diff-Zeilen)", diff.len() - MAX_DIFF_LINES));
    }
    ParameterDiff { changed_lines: changed, added_lines: added, removed_lines: removed, diff_text: text, truncated }
}

/// A parameter set read from the machine in the current session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterBackupInfo {
    pub path_name: String,
    pub data: Vec<u8>,
    pub read_at: DateTime<FixedOffset>,
    pub profile_id: String,
    pub port: String,
    pub complete: bool,
}

impl ParameterBackupInfo {
    pub fn sha256(&self) -> String {
        sha256_hex(&self.data)
    }

    pub fn structure(&self) -> Option<ParameterFileInfo> {
        parse_parameter_file(&self.data)
    }
}

/// A parameter file selected for writing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterCandidate {
    pub name: String,
    pub data: Vec<u8>,
    pub analysis: AnalysisResult,
    pub payload: Option<TransformResult>,
    pub transform_error: Option<String>,
}

impl ParameterCandidate {
    /// Analyse `data` and prepare the transfer bytes (only if the analysis has no errors).
    pub fn new(name: impl Into<String>, data: Vec<u8>, profile: &Profile) -> Self {
        let analysis = analyze_parameters(&data, profile);
        let (payload, transform_error) = if analysis.has_errors() {
            (None, None)
        } else {
            match prepare_parameter_payload(&data, profile) {
                Ok(payload) => (Some(payload), None),
                Err(err) => (None, Some(err.to_string())),
            }
        };
        Self { name: name.into(), data, analysis, payload, transform_error }
    }

    pub fn structure(&self) -> Option<ParameterFileInfo> {
        parse_parameter_file(&self.data)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterWriteGate {
    pub allowed: bool,
    pub reasons: Vec<String>,
    pub diff: Option<ParameterDiff>,
}

impl ParameterWriteGate {
    pub fn reason_text(&self) -> String {
        self.reasons.join("; ")
    }
}

/// Inputs of [`evaluate_parameter_write_gate`].
#[derive(Debug, Clone, Copy)]
pub struct GateInput<'a> {
    pub profile: &'a Profile,
    pub session_unlocked: bool,
    pub candidate: Option<&'a ParameterCandidate>,
    pub backup: Option<&'a ParameterBackupInfo>,
    pub now: DateTime<FixedOffset>,
    pub port_label: &'a str,
    pub simulation: bool,
    pub port: &'a str,
    pub busy: bool,
}

/// All conditions that must hold before a parameter write may even be offered.
pub fn evaluate_parameter_write_gate(input: &GateInput<'_>) -> ParameterWriteGate {
    let profile = input.profile;
    let mut reasons: Vec<String> = Vec::new();
    let mut diff = None;
    if !profile.parameters.allow_write {
        reasons.push("Profil erlaubt kein Schreiben von Maschinenparametern".into());
    }
    if !input.session_unlocked {
        reasons.push("Schreibfunktion für diese Sitzung nicht freigeschaltet".into());
    }
    let max_age = profile.parameters.max_backup_age_min;
    match input.backup {
        None => reasons.push(format!("Keine aktuelle Sicherung - Parameter zuerst auslesen (max. {max_age} min alt)")),
        Some(backup) => {
            let age_s = (input.now - backup.read_at).num_microseconds().map_or(f64::INFINITY, |us| us as f64 / 1e6);
            let age_min = age_s / 60.0;
            if backup.profile_id != profile.id || backup.port != input.port_label {
                reasons.push("Sicherung stammt von anderem Profil oder Anschluss - Parameter erneut auslesen".into());
            } else if age_min > max_age as f64 {
                reasons.push(format!("Sicherung ist {age_min:.0} min alt (max. {max_age} min) - erneut auslesen"));
            } else if !backup.complete {
                reasons.push("Letzte Sicherung ist unvollständig - erneut auslesen".into());
            }
        }
    }
    match input.candidate {
        None => reasons.push("Keine Parameterdatei zum Schreiben gewählt".into()),
        Some(candidate) => {
            if candidate.analysis.has_errors() {
                reasons.push("Parameterdatei enthält Prüffehler".into());
            }
            if candidate.payload.is_none() {
                reasons.push("Transferdaten konnten nicht erzeugt werden".into());
            }
            if let Some(backup) = input.backup {
                let d = diff_parameters(&backup.data, &candidate.data, &backup.path_name, &candidate.name);
                if d.identical() {
                    reasons.push("Datei ist identisch mit der aktuellen Sicherung - nichts zu schreiben".into());
                } else {
                    reasons.extend(structure_reasons(backup, candidate));
                }
                diff = Some(d);
            }
        }
    }
    if !input.simulation && input.port.is_empty() {
        reasons.push("Kein COM-Port gewählt und Simulator nicht aktiv".into());
    }
    if input.busy {
        reasons.push("Ein anderer Vorgang läuft".into());
    }
    ParameterWriteGate { allowed: reasons.is_empty(), reasons, diff }
}

/// Format, software version, machine number and checksum consistency against the backup.
fn structure_reasons(backup: &ParameterBackupInfo, candidate: &ParameterCandidate) -> Vec<String> {
    let Some(candidate_info) = candidate.structure() else {
        return vec!["Parameterdatei hat keine erkennbare Gruppenstruktur (%MPGnn/%MDGnn)".into()];
    };
    let Some(backup_info) = backup.structure() else {
        return vec!["Sicherung hat keine erkennbare Gruppenstruktur - Softwarestand nicht prüfbar".into()];
    };
    compatibility_problems(&backup_info, &candidate_info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::builtin_profiles;
    use chrono::Duration;

    pub(crate) const BACKUP: &[u8] =
        b"%MPG00 (KOPF)\r\nM65001 V1\r\nM65034 42\r\n?\r\n1111\r\n%MPG01 (A)\r\nM01001 5\r\n?\r\n2222\r\n\x04";

    fn profile(allow: bool) -> Profile {
        let mut p = builtin_profiles().0.into_iter().next().unwrap();
        p.parameters.allow_write = allow;
        p
    }

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-27T10:00:00+01:00").unwrap()
    }

    fn backup(p: &Profile, minutes_ago: i64) -> ParameterBackupInfo {
        ParameterBackupInfo {
            path_name: "PARAM.par".into(),
            data: BACKUP.to_vec(),
            read_at: now() - Duration::minutes(minutes_ago),
            profile_id: p.id.clone(),
            port: "COM3".into(),
            complete: true,
        }
    }

    fn gate(
        p: &Profile,
        unlocked: bool,
        c: Option<&ParameterCandidate>,
        b: Option<&ParameterBackupInfo>,
    ) -> ParameterWriteGate {
        evaluate_parameter_write_gate(&GateInput {
            profile: p,
            session_unlocked: unlocked,
            candidate: c,
            backup: b,
            now: now(),
            port_label: "COM3",
            simulation: false,
            port: "COM3",
            busy: false,
        })
    }

    #[test]
    fn write_gate_requires_everything() {
        let locked = profile(false);
        let g = gate(&locked, false, None, None);
        assert!(!g.allowed);
        assert_eq!(g.reasons.len(), 4);

        let p = profile(true);
        let good = String::from_utf8_lossy(BACKUP).replace("M01001 5", "M01001 6").replace("2222", "2223");
        let candidate = ParameterCandidate::new("NEU.par", good.into_bytes(), &p);
        let b = backup(&p, 5);
        let g = gate(&p, true, Some(&candidate), Some(&b));
        assert!(g.allowed, "{:?}", g.reasons);
        assert_eq!(g.diff.as_ref().unwrap().summary(), "2 geänderte, 0 hinzugefügte, 0 entfernte Zeile(n)");

        // still blocked without session unlock, with an old backup, or with an unchanged checksum
        assert!(!gate(&p, false, Some(&candidate), Some(&b)).allowed);
        let old = backup(&p, 61);
        assert!(gate(&p, true, Some(&candidate), Some(&old)).reasons[0].contains("61 min alt"));
        let same_checksum = String::from_utf8_lossy(BACKUP).replace("M01001 5", "M01001 6");
        let c2 = ParameterCandidate::new("X.par", same_checksum.into_bytes(), &p);
        assert!(gate(&p, true, Some(&c2), Some(&b)).reasons[0].contains("Prüfsumme aber nicht"));
        let c3 = ParameterCandidate::new("Y.par", BACKUP.to_vec(), &p);
        assert!(gate(&p, true, Some(&c3), Some(&b)).reasons[0].contains("identisch"));
    }

    #[test]
    fn sample_matches_python() {
        let sample = sample_parameters();
        assert_eq!(sample.len(), 655);
        assert_eq!(sha256_hex(&sample), "ce4d1feb27245c6b54322bfc2299f98aa175d494daf6059a3c92cbf13c8d00e9");
        let info = parse_parameter_file(&sample).unwrap();
        assert_eq!(info.groups.len(), 3);
        assert_eq!(info.machine_number(), "0000-0000");
    }

    #[test]
    fn diff_counts_and_text() {
        let d = diff_parameters(b"A\r\nB\r\nC\r\n", b"A\nX\nC\nD", "alt", "neu");
        assert_eq!((d.changed_lines, d.added_lines, d.removed_lines), (1, 1, 0));
        assert_eq!(d.diff_text, "--- alt\n+++ neu\n@@ -1,3 +1,4 @@\n A\n-B\n+X\n C\n+D");
        assert!(diff_parameters(b"A\r\n\x00", b"A\n", "a", "b").identical());
    }

    #[test]
    fn analysis_and_payload() {
        let mut p = profile(false);
        p.parameters.identification_pattern = "^%MPG".into();
        let a = analyze_parameters(BACKUP, &p);
        assert!(!a.has_errors(), "{:?}", a.findings);
        let a = analyze_parameters(b"N10 G0\r\n", &p);
        assert!(a.get("NOT_A_PARAMETER_FILE").unwrap().message.ends_with("('^%MPG')."));
        let payload = prepare_parameter_payload(b"M1\nM2\x00", &p).unwrap();
        assert_eq!(payload.data, b"M1\r\nM2");
        assert_eq!(py_str_repr("a'b\\"), "\"a'b\\\\\"");
    }
}
