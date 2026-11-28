//! Structure of DIALOG 11 machine parameter files (as output by the CNC).
//!
//! Observed format (output of a real FP2A, not an official specification):
//!
//! * group header `%MPGnn (NAME)` (machine parameters) or `%MDGnn (NAME)`,
//! * parameter lines `Mnnnnn value` (value may be empty),
//! * group end `?` followed by a line with a 4-digit hexadecimal checksum,
//! * group 00 `(KOPF)` holds software versions, machine type, number and date,
//! * the file ends with EOT (0x04).
//!
//! The checksum algorithm is unknown. DeckelLink therefore never computes
//! checksums; it only checks that each group has one and detects groups whose
//! content changed while the checksum stayed the same.

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use regex::Regex;

use crate::nc::checks::{split_eot_trailer, split_lines};
use crate::nc::{Finding, Severity};

pub const GROUP_END: &str = "?";
pub const MAX_REPORTED: usize = 200;

/// Characters Python's `\s` matches in ASCII text (`str.isspace`), minus CR/LF which never occur in a line.
const PY_SPACE: &str = r"[\t\x0B\x0C\x1C-\x1F ]";

fn group_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r"^%M([DP])G([0-9]{{2}}) \((.*)\){PY_SPACE}*$")).ok()).as_ref()
}

fn param_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^M([0-9]{5})(?: (.*))?$").ok()).as_ref()
}

fn is_checksum(text: &str) -> bool {
    text.len() == 4 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parameter group kind: `P` (%MPG) or `D` (%MDG).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupKind {
    P,
    D,
}

impl GroupKind {
    pub fn letter(self) -> char {
        match self {
            GroupKind::P => 'P',
            GroupKind::D => 'D',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GroupKind::P => "MPG",
            GroupKind::D => "MDG",
        }
    }

    /// (first software version key, count, machine type, machine number, date)
    fn header_keys(self) -> (u32, u32, &'static str, &'static str, &'static str) {
        match self {
            GroupKind::P => (65001, 30, "65033", "65034", "65035"),
            GroupKind::D => (1, 30, "00033", "00034", "00035"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterGroup {
    pub kind: GroupKind,
    pub number: u32,
    pub name: String,
    pub line: usize,
    pub lines: Vec<String>,
    pub checksum: Option<String>,
}

impl ParameterGroup {
    pub fn key(&self) -> String {
        format!("%M{}G{:02}", self.kind.letter(), self.number)
    }

    pub fn label(&self) -> String {
        format!("{} ({})", self.key(), self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterFileInfo {
    pub kind: Option<GroupKind>,
    pub groups: Vec<ParameterGroup>,
    pub header: HashMap<String, String>,
    pub eot: bool,
    pub findings: Vec<Finding>,
}

impl ParameterFileInfo {
    pub fn kind_label(&self) -> &'static str {
        self.kind.map_or("unbekannt", GroupKind::label)
    }

    fn header_value(&self, key: &str) -> String {
        self.header.get(key).map(|v| v.trim().to_string()).unwrap_or_default()
    }

    /// All non-empty software version entries of the header group, in order.
    pub fn software_versions(&self) -> Vec<String> {
        let Some(kind) = self.kind else {
            return Vec::new();
        };
        let (first, count, ..) = kind.header_keys();
        (first..first + count)
            .map(|n| self.header_value(&format!("{n:05}")))
            .filter(|value| !value.is_empty())
            .collect()
    }

    pub fn software_version(&self) -> String {
        self.software_versions().into_iter().next().unwrap_or_default()
    }

    pub fn machine_type(&self) -> String {
        self.kind.map(|k| self.header_value(k.header_keys().2)).unwrap_or_default()
    }

    pub fn machine_number(&self) -> String {
        self.kind.map(|k| self.header_value(k.header_keys().3)).unwrap_or_default()
    }

    pub fn date(&self) -> String {
        self.kind.map(|k| self.header_value(k.header_keys().4)).unwrap_or_default()
    }

    pub fn has_header(&self) -> bool {
        self.groups.iter().any(|g| g.number == 0)
    }

    pub fn group(&self, key: &str) -> Option<&ParameterGroup> {
        self.groups.iter().find(|g| g.key() == key)
    }

    pub fn summary(&self) -> String {
        if self.groups.is_empty() {
            return "Keine Parametergruppen erkannt".into();
        }
        let mut parts = vec![format!("{}-Format, {} Gruppe(n)", self.kind_label(), self.groups.len())];
        let software = self.software_version();
        if !software.is_empty() {
            parts.push(format!("Software {software}"));
        }
        let machine = self.machine_type();
        if !machine.is_empty() {
            parts.push(format!("Maschine {machine}"));
        }
        let number = self.machine_number();
        if !number.is_empty() {
            parts.push(format!("Nr. {number}"));
        }
        parts.join(", ")
    }
}

fn finding(code: &str, severity: Severity, message: String, lines: &[usize]) -> Finding {
    Finding {
        code: code.into(),
        severity,
        message,
        lines: lines.iter().take(MAX_REPORTED).copied().collect(),
        count: lines.len(),
    }
}

/// Python `bytes.decode("ascii", errors="replace")`.
pub fn decode_ascii(data: &[u8]) -> String {
    data.iter().map(|b| if b.is_ascii() { char::from(*b) } else { '\u{FFFD}' }).collect()
}

struct OpenGroup {
    kind: GroupKind,
    number: u32,
    name: String,
    line: usize,
}

/// Parse `data`; `None` if it contains no parameter group header at all.
pub fn parse_parameter_file(data: &[u8]) -> Option<ParameterFileInfo> {
    let group_re = group_re()?;
    let param_re = param_re()?;
    let cleaned: Vec<u8> = data.iter().copied().filter(|b| *b != 0).collect();
    let (body, trailer) = split_eot_trailer(&cleaned);
    let texts: Vec<(usize, String)> = split_lines(body)
        .into_iter()
        .map(|line| (line.number, decode_ascii(&line.content).trim_end_matches([' ', '\t']).to_string()))
        .collect();
    if !texts.iter().any(|(_, text)| group_re.is_match(text)) {
        return None;
    }

    let mut groups: Vec<ParameterGroup> = Vec::new();
    let (mut outside, mut bad_lines, mut unterminated, mut missing_checksum) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut kinds: BTreeSet<GroupKind> = BTreeSet::new();
    let mut header: HashMap<String, String> = HashMap::new();
    let mut current: Option<OpenGroup> = None;
    let mut content: Vec<String> = Vec::new();
    let mut awaiting: Option<(OpenGroup, Vec<String>)> = None;

    let make = |open: OpenGroup, lines: Vec<String>, checksum: Option<String>| ParameterGroup {
        kind: open.kind,
        number: open.number,
        name: open.name,
        line: open.line,
        lines,
        checksum,
    };

    for (number, text) in texts {
        if let Some((open, lines)) = awaiting.take() {
            if is_checksum(&text) {
                groups.push(make(open, lines, Some(text.to_uppercase())));
                continue;
            }
            missing_checksum.push(open.line);
            groups.push(make(open, lines, None));
        }
        if text.is_empty() {
            continue;
        }
        if let Some(caps) = group_re.captures(&text) {
            if let Some(open) = current.take() {
                unterminated.push(open.line);
                groups.push(make(open, std::mem::take(&mut content), None));
            }
            let kind = if caps.get(1).map(|m| m.as_str()) == Some("D") { GroupKind::D } else { GroupKind::P };
            let group_no = caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok()).unwrap_or(0);
            let name = caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();
            kinds.insert(kind);
            current = Some(OpenGroup { kind, number: group_no, name, line: number });
            content = Vec::new();
            continue;
        }
        let Some(open) = &current else {
            outside.push(number);
            continue;
        };
        if text == GROUP_END {
            if let Some(open) = current.take() {
                awaiting = Some((open, std::mem::take(&mut content)));
            }
            continue;
        }
        let Some(caps) = param_re.captures(&text) else {
            bad_lines.push(number);
            continue;
        };
        // Parameter numbers may repeat within a group (e.g. compensation tables).
        if open.number == 0 {
            let key = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            let value = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
            header.entry(key).or_insert(value);
        }
        content.push(text);
    }
    if let Some((open, lines)) = awaiting.take() {
        missing_checksum.push(open.line);
        groups.push(make(open, lines, None));
    }
    if let Some(open) = current.take() {
        unterminated.push(open.line);
        groups.push(make(open, content, None));
    }

    let mut findings = Vec::new();
    if !outside.is_empty() {
        findings.push(finding(
            "PARAM_DATA_OUTSIDE_GROUP",
            Severity::Error,
            format!("{} Zeile(n) stehen außerhalb einer Parametergruppe (%MPGnn/%MDGnn).", outside.len()),
            &outside,
        ));
    }
    if !bad_lines.is_empty() {
        findings.push(finding(
            "PARAM_LINE_FORMAT",
            Severity::Error,
            format!("{} Zeile(n) entsprechen nicht dem Format 'Mnnnnn Wert'.", bad_lines.len()),
            &bad_lines,
        ));
    }
    if !unterminated.is_empty() {
        findings.push(finding(
            "PARAM_GROUP_UNTERMINATED",
            Severity::Error,
            format!("{} Gruppe(n) ohne Gruppenende '?'.", unterminated.len()),
            &unterminated,
        ));
    }
    if !missing_checksum.is_empty() {
        findings.push(finding(
            "PARAM_CHECKSUM_MISSING",
            Severity::Error,
            format!("{} Gruppe(n) ohne Prüfsummenzeile (4 Hex-Ziffern nach '?').", missing_checksum.len()),
            &missing_checksum,
        ));
    }
    let keys: Vec<String> = groups.iter().map(ParameterGroup::key).collect();
    let duplicates: BTreeSet<&String> = keys.iter().filter(|k| keys.iter().filter(|x| x == k).count() > 1).collect();
    if !duplicates.is_empty() {
        let list: Vec<&str> = duplicates.iter().map(|s| s.as_str()).collect();
        findings.push(finding(
            "PARAM_DUPLICATE_GROUP",
            Severity::Warning,
            format!("Gruppen mehrfach vorhanden: {}.", list.join(", ")),
            &[],
        ));
    }
    if kinds.len() > 1 {
        findings.push(finding(
            "PARAM_MIXED_KINDS",
            Severity::Error,
            "Die Datei mischt %MPG- und %MDG-Gruppen.".into(),
            &[],
        ));
    }
    if !groups.iter().any(|g| g.number == 0) {
        findings.push(finding(
            "PARAM_NO_HEADER",
            Severity::Warning,
            "Kopfgruppe 00 (KOPF) mit Softwarestand fehlt - der Softwarestand kann nicht geprüft werden.".into(),
            &[],
        ));
    }
    if trailer.is_empty() {
        findings.push(finding(
            "PARAM_NO_EOT",
            Severity::Info,
            "Die Datei endet nicht mit EOT (0x04), wie es bei Ausgaben der Steuerung beobachtet wurde.".into(),
            &[],
        ));
    }
    let kind = if kinds.len() == 1 { kinds.iter().next().copied() } else { None };
    Some(ParameterFileInfo { kind, groups, header, eot: !trailer.is_empty(), findings })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupChange {
    pub key: String,
    pub label: String,
    pub checksum_changed: bool,
}

/// Groups of `new` whose parameter lines differ from the same group in `old`.
pub fn changed_groups(old: &ParameterFileInfo, new: &ParameterFileInfo) -> Vec<GroupChange> {
    new.groups
        .iter()
        .filter_map(|group| {
            let before = old.group(&group.key())?;
            (before.lines != group.lines).then(|| GroupChange {
                key: group.key(),
                label: group.label(),
                checksum_changed: before.checksum != group.checksum,
            })
        })
        .collect()
}

fn or_dash(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}

/// Reasons why `candidate` must not be written to the machine `backup` was read from.
pub fn compatibility_problems(backup: &ParameterFileInfo, candidate: &ParameterFileInfo) -> Vec<String> {
    let mut reasons = Vec::new();
    if backup.kind != candidate.kind {
        reasons.push(format!(
            "Dateiformat {} passt nicht zur Sicherung ({})",
            candidate.kind_label(),
            backup.kind_label()
        ));
    }
    if !candidate.has_header() {
        reasons.push("Parameterdatei hat keine Kopfgruppe - Softwarestand nicht prüfbar".into());
    } else if !backup.has_header() {
        reasons.push("Sicherung hat keine Kopfgruppe - Softwarestand nicht prüfbar".into());
    } else {
        if candidate.software_versions() != backup.software_versions() {
            reasons.push(format!(
                "Softwarestand der Datei ({}) passt nicht zur Maschine laut Sicherung ({})",
                or_dash(&candidate.software_version()),
                or_dash(&backup.software_version())
            ));
        }
        if candidate.machine_number() != backup.machine_number() {
            reasons.push(format!(
                "Maschinennummer der Datei ({}) passt nicht zur Sicherung ({})",
                or_dash(&candidate.machine_number()),
                or_dash(&backup.machine_number())
            ));
        }
    }
    for change in changed_groups(backup, candidate) {
        if !change.checksum_changed {
            reasons.push(format!(
                "Gruppe {} wurde geändert, die Prüfsumme aber nicht (Prüfsummenverfahren unbekannt - die Steuerung \
                 würde die Gruppe verwerfen)",
                change.label
            ));
        }
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"%MPG00 (KOPF)\r\nM65001 V1.2\r\nM65034 4711\r\n?\r\nAB12\r\n%MPG01 (ACHSEN)\r\nM01001 5\r\nM01002\r\n?\r\n00ff\r\n\x04";

    #[test]
    fn parses_structure() {
        let info = parse_parameter_file(SAMPLE).unwrap();
        assert_eq!(info.kind, Some(GroupKind::P));
        assert_eq!(info.groups.len(), 2);
        assert_eq!(info.groups[1].checksum.as_deref(), Some("00FF"));
        assert_eq!(info.software_version(), "V1.2");
        assert_eq!(info.machine_number(), "4711");
        assert!(info.eot && info.findings.is_empty());
        assert_eq!(info.summary(), "MPG-Format, 2 Gruppe(n), Software V1.2, Nr. 4711");
        assert!(parse_parameter_file(b"N10 G0\r\nM30\r\n").is_none());
    }

    #[test]
    fn reports_problems() {
        let data = b"X\r\n%MPG01 (A)\r\nfoo\r\n?\r\nZZ\r\n%MDG01 (B)\r\nM00001 1\r\n";
        let info = parse_parameter_file(data).unwrap();
        let codes: Vec<&str> = info.findings.iter().map(|f| f.code.as_str()).collect();
        assert_eq!(
            codes,
            vec![
                "PARAM_DATA_OUTSIDE_GROUP",
                "PARAM_LINE_FORMAT",
                "PARAM_GROUP_UNTERMINATED",
                "PARAM_CHECKSUM_MISSING",
                "PARAM_MIXED_KINDS",
                "PARAM_NO_HEADER",
                "PARAM_NO_EOT"
            ]
        );
        assert_eq!(info.kind, None);
        assert_eq!(info.findings[0].lines, vec![1, 5]);
    }

    #[test]
    fn compatibility() {
        let backup = parse_parameter_file(SAMPLE).unwrap();
        let changed =
            parse_parameter_file(&String::from_utf8_lossy(SAMPLE).replace("M01001 5", "M01001 6").into_bytes())
                .unwrap();
        let problems = compatibility_problems(&backup, &changed);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("%MPG01 (ACHSEN)"));
        let other = String::from_utf8_lossy(SAMPLE).replace("V1.2", "V2.0").replace("4711", "");
        let problems = compatibility_problems(&backup, &parse_parameter_file(other.as_bytes()).unwrap());
        assert_eq!(problems.len(), 3);
        assert!(problems[2].contains("%MPG00 (KOPF)"));
        assert!(problems[1].contains("(-)"));
    }
}
