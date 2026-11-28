//! Program management ("Programmverwaltung"): NC programs, macros and machine data files.
//!
//! Same folder layout as the Python version: every entry is stored as `<id>.nc` with
//! metadata `<id>.json` in the library folder. Nothing is overwritten silently: a new
//! content version moves the previous version into `versions/<id>/` first, and deleting
//! moves the files to `deleted/` (never a hard delete).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{DateTime, FixedOffset, SecondsFormat};
use regex::Regex;
use serde_json::{json, Value};

use crate::storage::{atomic_write, sha256_hex, unique_path, write_new_file};

pub const SUFFIX: &str = ".nc";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    Program,
    Macro,
    Tools,
    Other,
}

impl EntryKind {
    pub const ALL: [EntryKind; 4] = [EntryKind::Program, EntryKind::Macro, EntryKind::Tools, EntryKind::Other];

    pub fn value(self) -> &'static str {
        match self {
            EntryKind::Program => "program",
            EntryKind::Macro => "macro",
            EntryKind::Tools => "tools",
            EntryKind::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EntryKind::Program => "Hauptprogramm",
            EntryKind::Macro => "Makroprogramm",
            EntryKind::Tools => "Werkzeugdatei",
            EntryKind::Other => "Sonstige Datei",
        }
    }

    pub fn parse(text: &str) -> Option<EntryKind> {
        EntryKind::ALL.into_iter().find(|k| k.value() == text)
    }
}

/// Library operation failed (German message).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct LibraryError(pub String);

impl From<crate::storage::StorageError> for LibraryError {
    fn from(err: crate::storage::StorageError) -> Self {
        Self(err.to_string())
    }
}

impl From<std::io::Error> for LibraryError {
    fn from(err: std::io::Error) -> Self {
        Self(err.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
    pub number: String,
    pub description: String,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
    pub sha256: String,
    pub size: u64,
    pub tools: Vec<String>,
    pub last_sent_at: String,
    pub last_sent_mode: String,
    pub version_count: usize,
}

impl LibraryEntry {
    pub fn meta_path(&self) -> PathBuf {
        self.path.with_extension("json")
    }

    pub fn read(&self) -> Result<Vec<u8>, LibraryError> {
        Ok(fs::read(&self.path)?)
    }

    /// Metadata JSON (same keys and order as the Python version).
    pub fn to_meta(&self) -> Value {
        json!({
            "name": self.name,
            "kind": self.kind.value(),
            "number": self.number,
            "description": self.description,
            "source": self.source,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "sha256": self.sha256,
            "size": self.size,
            "tools": self.tools,
            "last_sent_at": self.last_sent_at,
            "last_sent_mode": self.last_sent_mode,
        })
    }
}

/// Python `isoformat(timespec="seconds")`.
pub fn iso_seconds(now: &DateTime<FixedOffset>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// Python `bytes.decode("ascii", errors="replace")`.
pub fn decode_ascii(data: &[u8]) -> String {
    data.iter().map(|b| if b.is_ascii() { char::from(*b) } else { '\u{fffd}' }).collect()
}

fn comment_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\([^()]*\)").ok()).as_ref()
}

fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Tool calls `T..`/`TT..` in program text (comments in brackets ignored).
///
/// Same result as the Python pattern `(?<![A-Z])(TT|T)\s*(\d+)(?![\d.])`.
pub fn tools_used(data: &[u8]) -> Vec<String> {
    let upper = decode_ascii(data).to_uppercase();
    let text = match comment_re() {
        Some(re) => re.replace_all(&upper, " ").into_owned(),
        None => upper,
    };
    let chars: Vec<char> = text.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut found: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let preceded = i.checked_sub(1).and_then(at).is_some_and(|c| c.is_ascii_uppercase());
        if at(i) != Some('T') || preceded {
            i += 1;
            continue;
        }
        // try "TT" first, then "T" (alternation order of the pattern)
        let attempt = |prefix_len: usize| -> Option<(usize, String)> {
            let mut j = i + prefix_len;
            while at(j).is_some_and(is_py_space) {
                j += 1;
            }
            let start = j;
            while at(j).is_some_and(|c| c.is_ascii_digit()) {
                j += 1;
            }
            if j == start || at(j) == Some('.') {
                return None;
            }
            Some((j, chars.get(start..j).map(|d| d.iter().collect()).unwrap_or_default()))
        };
        let result = if at(i + 1) == Some('T') { attempt(2).map(|r| ("TT", r)) } else { None }
            .or_else(|| attempt(1).map(|r| ("T", r)));
        match result {
            Some((prefix, (end, digits))) => {
                let number = digits.trim_start_matches('0');
                if !number.is_empty() {
                    let label = format!("{prefix}{number}");
                    if !found.contains(&label) {
                        found.push(label);
                    }
                }
                i = end;
            }
            None => i += 1,
        }
    }
    found
}

fn macro_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^[\s\x1c-\x1f]*%%[0-9]").ok()).as_ref()
}

fn machine_data_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^%M[DP]G[0-9][0-9]").ok()).as_ref()
}

pub fn guess_kind(data: &[u8], name: &str) -> EntryKind {
    let head = data.get(..4000).unwrap_or(data);
    let text = decode_ascii(head).to_uppercase();
    if macro_re().is_some_and(|re| re.is_match(&text)) || name.to_uppercase().starts_with("%%") {
        return EntryKind::Macro;
    }
    if machine_data_re().is_some_and(|re| re.is_match(&text)) {
        return EntryKind::Other;
    }
    EntryKind::Program
}

/// Python `sanitize_stem` of the library: `[^A-Za-z0-9_-]` -> `_`, at most 64 characters.
pub fn library_stem(stem: &str, fallback: &str) -> String {
    let cleaned: String = stem
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .take(64)
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Python `str(value)` for metadata values.
fn py_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        other => other.to_string(),
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct ProgramLibrary {
    pub root: PathBuf,
    pub versions_dir: PathBuf,
    pub deleted_dir: PathBuf,
}

impl ProgramLibrary {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf(), versions_dir: root.join("versions"), deleted_dir: root.join("deleted") }
    }

    pub fn ensure(&self) -> Result<(), LibraryError> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    // ------------------------------------------------------------------ reading
    /// All entries, sorted by kind and name (case-insensitive).
    pub fn entries(&self) -> Vec<LibraryEntry> {
        let Ok(dir) = fs::read_dir(&self.root) else { return Vec::new() };
        let mut paths: Vec<PathBuf> = dir
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(SUFFIX)))
            .collect();
        paths.sort();
        let mut entries: Vec<LibraryEntry> = paths.iter().map(|p| self.load(p)).collect();
        entries.sort_by(|a, b| (a.kind.value(), a.name.to_lowercase()).cmp(&(b.kind.value(), b.name.to_lowercase())));
        entries
    }

    pub fn get(&self, id: &str) -> Option<LibraryEntry> {
        let path = self.root.join(format!("{id}{SUFFIX}"));
        path.is_file().then(|| self.load(&path))
    }

    fn load(&self, path: &Path) -> LibraryEntry {
        let id = file_stem(path);
        let meta_path = path.with_extension("json");
        let meta: serde_json::Map<String, Value> = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let text = |key: &str| meta.get(key).map(py_str).unwrap_or_default();
        let name = meta.get("name").map(py_str).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
        let kind =
            EntryKind::parse(&meta.get("kind").map_or_else(|| "program".into(), py_str)).unwrap_or(EntryKind::Other);
        let tools = match meta.get("tools") {
            Some(Value::Array(items)) => items.iter().map(py_str).collect(),
            _ => Vec::new(),
        };
        let version_dir = self.versions_dir.join(&id);
        let version_count = Self::version_files(&version_dir).len();
        LibraryEntry {
            path: path.to_path_buf(),
            name,
            kind,
            number: text("number"),
            description: text("description"),
            source: text("source"),
            created_at: text("created_at"),
            updated_at: text("updated_at"),
            sha256: text("sha256"),
            size: fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            tools,
            last_sent_at: text("last_sent_at"),
            last_sent_mode: text("last_sent_mode"),
            version_count,
            id,
        }
    }

    fn version_files(directory: &Path) -> Vec<PathBuf> {
        let Ok(dir) = fs::read_dir(directory) else { return Vec::new() };
        let mut files: Vec<PathBuf> = dir
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(SUFFIX)))
            .collect();
        files.sort();
        files
    }

    /// Earlier versions of an entry, newest first.
    pub fn versions(&self, entry: &LibraryEntry) -> Vec<PathBuf> {
        let mut files = Self::version_files(&self.versions_dir.join(&entry.id));
        files.reverse();
        files
    }

    // ------------------------------------------------------------------ writing
    pub fn import_bytes(
        &self,
        name: &str,
        data: &[u8],
        kind: Option<EntryKind>,
        source: &str,
        now: &DateTime<FixedOffset>,
    ) -> Result<LibraryEntry, LibraryError> {
        if data.is_empty() {
            return Err(LibraryError("Leere Datei kann nicht übernommen werden.".into()));
        }
        self.ensure()?;
        let name_stem = file_stem(Path::new(name));
        let stem = library_stem(&name_stem, "PROGRAMM");
        let path = unique_path(&self.root, &stem, SUFFIX)?;
        write_new_file(&path, data)?;
        let id = file_stem(&path);
        let stamp = iso_seconds(now);
        let entry = LibraryEntry {
            name: if name_stem.is_empty() { id.clone() } else { name_stem },
            kind: kind.unwrap_or_else(|| guess_kind(data, name)),
            number: String::new(),
            description: String::new(),
            source: source.to_string(),
            created_at: stamp.clone(),
            updated_at: stamp,
            sha256: sha256_hex(data),
            size: u64::try_from(data.len()).unwrap_or(u64::MAX),
            tools: tools_used(data),
            last_sent_at: String::new(),
            last_sent_mode: String::new(),
            version_count: 0,
            path,
            id,
        };
        self.write_meta(&entry)?;
        Ok(entry)
    }

    pub fn import_file(
        &self,
        source: &Path,
        kind: Option<EntryKind>,
        now: &DateTime<FixedOffset>,
    ) -> Result<LibraryEntry, LibraryError> {
        let data = fs::read(source).map_err(|e| LibraryError(format!("Datei konnte nicht gelesen werden: {e}")))?;
        let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.import_bytes(&name, &data, kind, &name, now)
    }

    /// Store new content; the previous content is kept as a version first.
    pub fn save_version(
        &self,
        entry: &LibraryEntry,
        data: &[u8],
        now: &DateTime<FixedOffset>,
    ) -> Result<LibraryEntry, LibraryError> {
        if data.is_empty() {
            return Err(LibraryError("Leerer Inhalt wird nicht gespeichert.".into()));
        }
        let current = entry.read()?;
        if current == data {
            return Ok(entry.clone());
        }
        let directory = self.versions_dir.join(&entry.id);
        fs::create_dir_all(&directory)?;
        let version = unique_path(&directory, &now.format("%Y%m%d_%H%M%S").to_string(), SUFFIX)?;
        write_new_file(&version, &current)?;
        atomic_write(&entry.path, data, true)?;
        let mut updated = entry.clone();
        updated.sha256 = sha256_hex(data);
        updated.size = u64::try_from(data.len()).unwrap_or(u64::MAX);
        updated.tools = tools_used(data);
        updated.updated_at = iso_seconds(now);
        self.write_meta(&updated)?;
        Ok(self.get(&entry.id).unwrap_or(updated))
    }

    pub fn restore_version(
        &self,
        entry: &LibraryEntry,
        version: &Path,
        now: &DateTime<FixedOffset>,
    ) -> Result<LibraryEntry, LibraryError> {
        if version.parent() != Some(self.versions_dir.join(&entry.id).as_path()) {
            return Err(LibraryError("Version gehört nicht zu diesem Eintrag.".into()));
        }
        let data = fs::read(version)?;
        self.save_version(entry, &data, now)
    }

    /// Change name, kind, number and description (the file itself is not touched).
    pub fn update_meta(
        &self,
        entry: &LibraryEntry,
        name: &str,
        kind: EntryKind,
        number: &str,
        description: &str,
    ) -> Result<LibraryEntry, LibraryError> {
        let mut updated = entry.clone();
        updated.name = name.trim().to_string();
        updated.kind = kind;
        updated.number = number.trim().to_string();
        updated.description = description.trim().to_string();
        self.write_meta(&updated)?;
        Ok(updated)
    }

    pub fn mark_sent(
        &self,
        entry: &LibraryEntry,
        now: &DateTime<FixedOffset>,
        mode: &str,
    ) -> Result<LibraryEntry, LibraryError> {
        let mut updated = entry.clone();
        updated.last_sent_at = iso_seconds(now);
        updated.last_sent_mode = mode.to_string();
        self.write_meta(&updated)?;
        Ok(updated)
    }

    /// Move the entry (file, metadata, versions) into `deleted/` - never a hard delete.
    pub fn delete(&self, entry: &LibraryEntry, now: &DateTime<FixedOffset>) -> Result<PathBuf, LibraryError> {
        let mut target = self.deleted_dir.join(format!("{}_{}", entry.id, now.format("%Y%m%d_%H%M%S")));
        let mut n = 1;
        while target.exists() {
            let name = target.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            target = target.with_file_name(format!("{name}_{n}"));
            n += 1;
        }
        fs::create_dir_all(&target)?;
        let file_name = |p: &Path| p.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        fs::rename(&entry.path, target.join(file_name(&entry.path)))?;
        let meta = entry.meta_path();
        if meta.is_file() {
            fs::rename(&meta, target.join(file_name(&meta)))?;
        }
        let versions = self.versions_dir.join(&entry.id);
        if versions.is_dir() {
            fs::rename(&versions, target.join("versions"))?;
        }
        Ok(target)
    }

    fn write_meta(&self, entry: &LibraryEntry) -> Result<(), LibraryError> {
        let text = serde_json::to_string_pretty(&entry.to_meta()).map_err(|e| LibraryError(e.to_string()))?;
        atomic_write(&entry.meta_path(), text.as_bytes(), true)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-28T10:00:00+01:00").unwrap()
    }

    #[test]
    fn tool_calls_like_python() {
        assert_eq!(tools_used(b"N10 T1 M6\nN20 TT 12\n(T99) T1.5 XT3 T0 T007"), ["T1", "TT12", "T7"]);
        assert_eq!(tools_used(b"TTT5 T2."), Vec::<String>::new());
        assert_eq!(guess_kind(b"%%4\nN10", "x.nc"), EntryKind::Macro);
        assert_eq!(guess_kind(b"%MPG01\n", "x.nc"), EntryKind::Other);
        assert_eq!(guess_kind(b"N10 G0", "%%x"), EntryKind::Macro);
        assert_eq!(library_stem(" Prog ä.1 ", "P"), "Prog___1");
    }

    #[test]
    fn import_version_restore_delete() {
        let root = std::env::temp_dir().join(format!("deckellink-lib-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let library = ProgramLibrary::new(&root);
        let entry = library.import_bytes("TEIL 1.nc", b"N10 T1 M6\r\n", None, "TEIL 1.nc", &now()).unwrap();
        assert_eq!(entry.id, "TEIL_1");
        assert_eq!(entry.tools, ["T1"]);
        let second = library.import_bytes("TEIL 1.nc", b"N10\r\n", None, "", &now()).unwrap();
        assert_eq!(second.id, "TEIL_1_001");
        let updated = library.save_version(&entry, b"N10 T2 M6\r\n", &now()).unwrap();
        assert_eq!(updated.version_count, 1);
        assert_eq!(updated.tools, ["T2"]);
        let versions = library.versions(&updated);
        assert_eq!(versions.len(), 1);
        let restored = library.restore_version(&updated, &versions[0], &now()).unwrap();
        assert_eq!(restored.read().unwrap(), b"N10 T1 M6\r\n");
        assert_eq!(restored.version_count, 2);
        let meta: Value = serde_json::from_str(&fs::read_to_string(restored.meta_path()).unwrap()).unwrap();
        assert_eq!(meta["created_at"], "2026-11-28T10:00:00+01:00");
        let target = library.delete(&restored, &now()).unwrap();
        assert!(target.join("TEIL_1.nc").is_file() && target.join("versions").is_dir());
        assert_eq!(library.entries().len(), 1);
        assert!(library.save_version(&second, b"", &now()).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
