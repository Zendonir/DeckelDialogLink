//! Tool management ("Werkzeugverwaltung").
//!
//! The tool table is kept on the PC (`tools.json`, same format as the Python version).
//! It feeds the simulator and the tool check of programs. The format in which the
//! DIALOG 11 reads or outputs tool data is NOT known; a tool file for the machine can
//! therefore only be produced from a user-defined template (marked unverified) - or a
//! tool file received from the machine is resent unchanged.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset};
use serde_json::{json, Value};

use crate::profilestore::backup_file;
use crate::storage::atomic_write;

pub const TOOL_FILE: &str = "tools.json";
pub const PLACEHOLDERS: [&str; 9] = ["{T}", "{NAME}", "{R}", "{D}", "{L}", "{Z}", "{F}", "{S}", "{TYP}"];

/// Invalid tool data or template (German message).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ToolDataError(pub String);

/// Python `repr(float)`.
pub fn py_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let sci = format!("{value:e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exponent.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if negative { "-" } else { "" };
    if (-4..16).contains(&exp) {
        let count = i32::try_from(digits.len()).unwrap_or(0);
        let text = if exp < 0 {
            let zeros = "0".repeat(usize::try_from(-exp - 1).unwrap_or(0));
            format!("0.{zeros}{digits}")
        } else if exp + 1 >= count {
            let zeros = "0".repeat(usize::try_from(exp + 1 - count).unwrap_or(0));
            format!("{digits}{zeros}.0")
        } else {
            let split = usize::try_from(exp + 1).unwrap_or(0);
            format!("{}.{}", digits.get(..split).unwrap_or(""), digits.get(split..).unwrap_or(""))
        };
        return format!("{sign}{text}");
    }
    let (first, rest) = digits.split_at(digits.len().min(1));
    let mantissa = if rest.is_empty() { first.to_string() } else { format!("{first}.{rest}") };
    format!("{sign}{mantissa}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub number: i64,
    pub name: String,
    pub kind: String,
    pub diameter: f64,
    pub length: f64,
    pub flutes: i64,
    pub feed: f64,
    pub speed: f64,
    pub note: String,
}

impl Tool {
    pub fn new(number: i64) -> Self {
        Self {
            number,
            name: String::new(),
            kind: "Fräser".into(),
            diameter: 10.0,
            length: 0.0,
            flutes: 0,
            feed: 0.0,
            speed: 0.0,
            note: String::new(),
        }
    }

    pub fn radius(&self) -> f64 {
        self.diameter / 2.0
    }

    pub fn problems(&self) -> Vec<String> {
        let n = self.number;
        let mut out = Vec::new();
        if n <= 0 || n > 9999 {
            out.push(format!("T{n}: Werkzeugnummer muss zwischen 1 und 9999 liegen"));
        }
        if !self.diameter.is_finite() || self.diameter < 0.0 || self.diameter > 1000.0 {
            out.push(format!("T{n}: Durchmesser {} ist unplausibel", py_repr(self.diameter)));
        }
        if !self.length.is_finite() || self.length.abs() > 2000.0 {
            out.push(format!("T{n}: Länge {} ist unplausibel", py_repr(self.length)));
        }
        if self.feed < 0.0 || self.speed < 0.0 || self.flutes < 0 {
            out.push(format!("T{n}: negative Werte sind nicht zulässig"));
        }
        out
    }

    pub fn to_json(&self) -> Value {
        json!({
            "number": self.number,
            "name": self.name,
            "kind": self.kind,
            "diameter": self.diameter,
            "length": self.length,
            "flutes": self.flutes,
            "feed": self.feed,
            "speed": self.speed,
            "note": self.note,
        })
    }

    fn from_json(item: &Value) -> Result<Tool, String> {
        let object = item.as_object().ok_or_else(|| "Eintrag ist kein Objekt".to_string())?;
        let int = |key: &str| -> Result<Option<i64>, String> {
            match object.get(key) {
                None => Ok(None),
                Some(Value::Number(n)) => n
                    .as_i64()
                    .or_else(|| n.as_f64().map(crate::sim::py_int))
                    .map(Some)
                    .ok_or_else(|| format!("{key}: {n}")),
                Some(Value::String(s)) => s.trim().parse::<i64>().map(Some).map_err(|_| format!("{key}: '{s}'")),
                Some(Value::Bool(b)) => Ok(Some(i64::from(*b))),
                Some(other) => Err(format!("{key}: {other}")),
            }
        };
        let float = |key: &str| -> Result<Option<f64>, String> {
            match object.get(key) {
                None => Ok(None),
                Some(Value::Number(n)) => n.as_f64().map(Some).ok_or_else(|| format!("{key}: {n}")),
                Some(Value::String(s)) => s.trim().parse::<f64>().map(Some).map_err(|_| format!("{key}: '{s}'")),
                Some(Value::Bool(b)) => Ok(Some(f64::from(u8::from(*b)))),
                Some(other) => Err(format!("{key}: {other}")),
            }
        };
        let text = |key: &str| -> Option<String> {
            object.get(key).map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
        };
        let number = int("number")?.ok_or_else(|| "Werkzeug ohne Nummer".to_string())?;
        let defaults = Tool::new(number);
        Ok(Tool {
            number,
            name: text("name").unwrap_or(defaults.name),
            kind: text("kind").unwrap_or(defaults.kind),
            diameter: float("diameter")?.unwrap_or(defaults.diameter),
            length: float("length")?.unwrap_or(defaults.length),
            flutes: int("flutes")?.unwrap_or(defaults.flutes),
            feed: float("feed")?.unwrap_or(defaults.feed),
            speed: float("speed")?.unwrap_or(defaults.speed),
            note: text("note").unwrap_or(defaults.note),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ToolDatabase {
    pub path: PathBuf,
    pub backup_dir: Option<PathBuf>,
    pub tools: Vec<Tool>,
    pub problems: Vec<String>,
}

impl ToolDatabase {
    pub fn new(path: &Path, backup_dir: Option<&Path>) -> Self {
        Self {
            path: path.to_path_buf(),
            backup_dir: backup_dir.map(Path::to_path_buf),
            tools: Vec::new(),
            problems: Vec::new(),
        }
    }

    pub fn load(&mut self) -> &[Tool] {
        self.tools.clear();
        self.problems.clear();
        if !self.path.is_file() {
            return &self.tools;
        }
        let parsed = fs::read_to_string(&self.path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()));
        let data = match parsed {
            Ok(data) => data,
            Err(err) => {
                self.problems.push(format!("Werkzeugdatei nicht lesbar: {err}"));
                return &self.tools;
            }
        };
        let items = data.get("tools").and_then(Value::as_array).cloned().unwrap_or_default();
        for item in &items {
            match Tool::from_json(item) {
                Ok(tool) => self.tools.push(tool),
                Err(err) => self.problems.push(format!("Werkzeug übersprungen: {err}")),
            }
        }
        self.tools.sort_by_key(|t| t.number);
        &self.tools
    }

    /// Validate and save (backup of the previous file first).
    pub fn save(&mut self, tools: &[Tool], now: &DateTime<FixedOffset>) -> Result<(), ToolDataError> {
        let mut problems: Vec<String> = tools.iter().flat_map(Tool::problems).collect();
        let numbers: Vec<i64> = tools.iter().map(|t| t.number).collect();
        let mut duplicates: Vec<i64> =
            numbers.iter().copied().filter(|n| numbers.iter().filter(|m| *m == n).count() > 1).collect();
        duplicates.sort_unstable();
        duplicates.dedup();
        if !duplicates.is_empty() {
            let list: Vec<String> = duplicates.iter().map(|n| format!("T{n}")).collect();
            problems.push(format!("Doppelte Werkzeugnummern: {}", list.join(", ")));
        }
        if !problems.is_empty() {
            return Err(ToolDataError(problems.join("; ")));
        }
        if self.path.is_file() {
            if let Some(dir) = &self.backup_dir {
                backup_file(dir, &self.path, now)
                    .map_err(|e| ToolDataError(format!("Sicherung fehlgeschlagen: {e}")))?;
            }
        }
        let mut sorted = tools.to_vec();
        sorted.sort_by_key(|t| t.number);
        let payload = json!({"schema_version": 1, "tools": sorted.iter().map(Tool::to_json).collect::<Vec<_>>()});
        let text = serde_json::to_string_pretty(&payload).map_err(|e| ToolDataError(e.to_string()))?;
        atomic_write(&self.path, text.as_bytes(), true).map_err(|e| ToolDataError(e.to_string()))?;
        self.tools = sorted;
        Ok(())
    }

    pub fn get(&self, number: i64) -> Option<&Tool> {
        self.tools.iter().find(|t| t.number == number)
    }
}

/// User-defined text layout for a tool file (unverified for the DIALOG 11).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolExportTemplate {
    pub header: String,
    pub line: String,
    pub footer: String,
}

impl ToolExportTemplate {
    pub fn configured(&self) -> bool {
        !self.line.trim().is_empty()
    }
}

/// Tool calls of a program that are missing in the tool table.
pub fn check_program_tools(used: &[String], tools: &[Tool]) -> Vec<String> {
    used.iter()
        .filter(|label| {
            let number = label.trim_start_matches('T').parse::<i64>().ok();
            !number.is_some_and(|n| tools.iter().any(|t| t.number == n))
        })
        .cloned()
        .collect()
}

/// Python `f"{value:.3f}".rstrip("0").rstrip(".")` with "-0" -> "0".
pub fn fmt_number(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-0" {
        "0".into()
    } else {
        text.to_string()
    }
}

/// Python `str.splitlines()` for template text ("\r\n" is one line break).
fn splitlines(text: &str) -> Vec<String> {
    const BREAKS: [char; 10] =
        ['\n', '\r', '\u{b}', '\u{c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}', '\u{2029}'];
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if BREAKS.contains(&c) {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            lines.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Build a tool file from `template` (CR LF, 7-bit ASCII).
pub fn export_tool_file(tools: &[Tool], template: &ToolExportTemplate) -> Result<Vec<u8>, ToolDataError> {
    if !template.configured() {
        return Err(ToolDataError(
            "Keine Vorlage hinterlegt. Das Werkzeugdatei-Format der DIALOG 11 ist nicht bekannt - bitte zuerst eine \
             Werkzeugdatei von der Steuerung auslesen und danach die Vorlage einrichten."
                .into(),
        ));
    }
    if tools.is_empty() {
        return Err(ToolDataError("Werkzeugtabelle ist leer.".into()));
    }
    let mut lines: Vec<String> = Vec::new();
    if !template.header.trim().is_empty() {
        lines.extend(splitlines(&template.header));
    }
    let mut sorted = tools.to_vec();
    sorted.sort_by_key(|t| t.number);
    for tool in &sorted {
        let values = [
            ("{T}", tool.number.to_string()),
            ("{NAME}", tool.name.clone()),
            ("{R}", fmt_number(tool.radius())),
            ("{D}", fmt_number(tool.diameter)),
            ("{L}", fmt_number(tool.length)),
            ("{Z}", tool.flutes.to_string()),
            ("{F}", fmt_number(tool.feed)),
            ("{S}", fmt_number(tool.speed)),
            ("{TYP}", tool.kind.clone()),
        ];
        let mut text = template.line.clone();
        for (key, value) in values {
            text = text.replace(key, &value);
        }
        lines.push(text);
    }
    if !template.footer.trim().is_empty() {
        lines.extend(splitlines(&template.footer));
    }
    let result = lines.join("\r\n") + "\r\n";
    if !result.is_ascii() {
        return Err(ToolDataError(
            "Die Werkzeugdatei enthält Zeichen außerhalb von 7-Bit-ASCII (z. B. Umlaute).".into(),
        ));
    }
    Ok(result.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_and_format_like_python() {
        for (value, text) in [
            (1500.0, "1500.0"),
            (0.5, "0.5"),
            (1e16, "1e+16"),
            (1.5e-5, "1.5e-05"),
            (-2.25, "-2.25"),
            (123456.789, "123456.789"),
        ] {
            assert_eq!(py_repr(value), text);
        }
        assert_eq!(fmt_number(5.0), "5");
        assert_eq!(fmt_number(-0.0001), "0");
        assert_eq!(fmt_number(2.5005), "2.501");
        assert_eq!(splitlines("a\r\nb\n\nc\n"), ["a", "b", "", "c"]);
    }

    #[test]
    fn save_validates_and_exports() {
        let root = std::env::temp_dir().join(format!("deckellink-tools-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let now = DateTime::parse_from_rfc3339("2026-11-28T10:00:00+01:00").unwrap();
        let mut db = ToolDatabase::new(&root.join(TOOL_FILE), Some(&root.join("backups")));
        let mut bad = Tool::new(0);
        bad.diameter = 1500.0;
        let err = db.save(&[bad, Tool::new(2), Tool::new(2)], &now).unwrap_err();
        assert_eq!(
            err.0,
            "T0: Werkzeugnummer muss zwischen 1 und 9999 liegen; T0: Durchmesser 1500.0 ist unplausibel; \
             Doppelte Werkzeugnummern: T2"
        );
        let mut t5 = Tool::new(5);
        t5.name = "VHM 8".into();
        t5.diameter = 8.0;
        db.save(&[t5, Tool::new(1)], &now).unwrap();
        db.save(&[Tool::new(1)], &now).unwrap();
        assert!(root.join("backups").read_dir().unwrap().count() == 1);
        let mut other = ToolDatabase::new(&root.join(TOOL_FILE), None);
        assert_eq!(other.load().len(), 1);
        let template =
            ToolExportTemplate { header: "%T\n".into(), line: "T{T} R{R} {NAME}".into(), footer: String::new() };
        let mut t5 = Tool::new(5);
        t5.name = "VHM".into();
        let data = export_tool_file(&[t5, Tool::new(1)], &template).unwrap();
        assert_eq!(data, b"%T\r\nT1 R5 \r\nT5 R5 VHM\r\n");
        assert_eq!(check_program_tools(&["T1".into(), "TT3".into()], &other.tools), ["TT3"]);
        let _ = fs::remove_dir_all(&root);
    }
}
