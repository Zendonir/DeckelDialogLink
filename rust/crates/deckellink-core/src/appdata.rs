//! Per-user application data: directories, settings and the archive of received programs.
//!
//! Directory layout, `settings.json` and archive metadata are the same as in the Python
//! version, so both versions can work with the same data during the transition.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, Local};
use regex::Regex;
use serde_json::{json, Map, Value};

use crate::model::Profile;
use crate::parameters::archive::{finding_json, isoformat};
use crate::parameters::fileformat::decode_ascii;
use crate::profile::profile_to_json;
use crate::storage::{atomic_write, sanitize_stem, sha256_hex, unique_path, write_new_file, StorageError};
use crate::transfer::receiver::ReceivedProgram;
use crate::transfer::ReceiveResult;

pub const APP_NAME: &str = "DeckelLink";
pub const ENV_HOME: &str = "DECKELLINK_HOME";
pub const CONFIG_FILE: &str = "settings.json";
pub const NC_SUFFIX: &str = ".nc";

/// User-specific directories (never the program directory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub logs: PathBuf,
    pub received: PathBuf,
    pub backups: PathBuf,
    pub failed: PathBuf,
    pub raw: PathBuf,
    pub exports: PathBuf,
    pub profiles: PathBuf,
    pub simulation: PathBuf,
}

impl AppPaths {
    pub fn from_root(root: &Path, logs: Option<PathBuf>) -> Self {
        Self {
            root: root.to_path_buf(),
            config: root.join("config"),
            logs: logs.unwrap_or_else(|| root.join("logs")),
            received: root.join("received"),
            backups: root.join("backups"),
            failed: root.join("failed"),
            raw: root.join("raw"),
            exports: root.join("exports"),
            profiles: root.join("profiles"),
            simulation: root.join("simulation"),
        }
    }

    /// Explicit override > `DECKELLINK_HOME` > platform directories (as Python's `platformdirs`).
    pub fn resolve(override_root: Option<&Path>) -> Self {
        if let Some(root) = override_root {
            return Self::from_root(root, None);
        }
        if let Some(env) = std::env::var_os(ENV_HOME).filter(|v| !v.is_empty()) {
            return Self::from_root(Path::new(&env), None);
        }
        let (data, logs) = platform_dirs();
        Self::from_root(&data, Some(logs))
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [
            &self.root,
            &self.config,
            &self.logs,
            &self.received,
            &self.backups,
            &self.failed,
            &self.raw,
            &self.exports,
            &self.profiles,
            &self.simulation,
        ] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// `platformdirs.user_data_dir(APP_NAME, appauthor=False)` and `user_log_dir(...)`.
fn platform_dirs() -> (PathBuf, PathBuf) {
    if cfg!(windows) {
        let base = env_path("LOCALAPPDATA").unwrap_or_else(|| PathBuf::from("."));
        let data = base.join(APP_NAME);
        let logs = data.join("Logs");
        (data, logs)
    } else if cfg!(target_os = "macos") {
        let home = env_path("HOME").unwrap_or_else(|| PathBuf::from("."));
        (home.join("Library/Application Support").join(APP_NAME), home.join("Library/Logs").join(APP_NAME))
    } else {
        let home = env_path("HOME").unwrap_or_else(|| PathBuf::from("."));
        let data = env_path("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")).join(APP_NAME);
        let logs = env_path("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state")).join(APP_NAME).join("log");
        (data, logs)
    }
}

pub const LOG_LEVELS: [&str; 4] = ["DEBUG", "INFO", "WARNING", "ERROR"];
pub const MAX_RECENT_FILES: usize = 10;

/// Connection kind for real transfers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionKind {
    Serial,
    Tcp,
    Rfc2217,
}

impl ConnectionKind {
    pub const ALL: [ConnectionKind; 3] = [ConnectionKind::Serial, ConnectionKind::Tcp, ConnectionKind::Rfc2217];

    pub fn as_str(self) -> &'static str {
        match self {
            ConnectionKind::Serial => "serial",
            ConnectionKind::Tcp => "tcp",
            ConnectionKind::Rfc2217 => "rfc2217",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConnectionKind::Serial => "Serieller Anschluss (COM-Port)",
            ConnectionKind::Tcp => "Netzwerkadapter (TCP-Rohdaten)",
            ConnectionKind::Rfc2217 => "Netzwerkadapter (RFC 2217)",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == text)
    }
}

/// Application settings (`config/settings.json`). Unknown keys of the file are preserved.
#[derive(Debug, Clone, PartialEq)]
pub struct AppConfig {
    pub port: String,
    pub connection_kind: ConnectionKind,
    pub network_host: String,
    pub network_port: u32,
    /// Safe default: the application starts in simulation mode.
    pub simulation_mode: bool,
    pub simulation_scenario: String,
    pub active_profile_id: String,
    pub show_safety_notice: bool,
    pub log_level: String,
    pub raw_traffic_logging: bool,
    pub terminal_history_lines: u64,
    pub last_open_dir: String,
    pub recent_files: Vec<String>,
    pub font_scale_percent: u64,
    /// Recording used by the replay scenario ("" = sample program).
    pub simulation_replay_file: String,
    /// All keys of the file, including those only the Python version knows.
    pub extra: Map<String, Value>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: String::new(),
            connection_kind: ConnectionKind::Serial,
            network_host: String::new(),
            network_port: 4001,
            simulation_mode: true,
            simulation_scenario: "send_ok".into(),
            active_profile_id: "dialog11_7n1_rtscts".into(),
            show_safety_notice: true,
            log_level: "INFO".into(),
            raw_traffic_logging: false,
            terminal_history_lines: 2000,
            last_open_dir: String::new(),
            recent_files: Vec::new(),
            font_scale_percent: 100,
            simulation_replay_file: String::new(),
            extra: Map::new(),
        }
    }
}

impl AppConfig {
    pub fn add_recent_file(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_string());
        self.recent_files.truncate(MAX_RECENT_FILES);
    }

    /// Build from JSON, ignoring invalid values (reported as German warnings, as in Python).
    pub fn from_json(data: &Map<String, Value>) -> (Self, Vec<String>) {
        let mut config = Self { extra: data.clone(), ..Self::default() };
        let mut warnings = Vec::new();

        for (key, value) in data {
            match key.as_str() {
                "port"
                | "network_host"
                | "simulation_scenario"
                | "active_profile_id"
                | "log_level"
                | "last_open_dir"
                | "connection_kind" => {
                    let Some(text) = value.as_str() else {
                        warnings.push(format!("Ungültiger Wert für {key}: {value}"));
                        continue;
                    };
                    let text = text.to_string();
                    match key.as_str() {
                        "port" => config.port = text,
                        "network_host" => config.network_host = text,
                        "simulation_scenario" => config.simulation_scenario = text,
                        "active_profile_id" => config.active_profile_id = text,
                        "log_level" => config.log_level = text,
                        "last_open_dir" => config.last_open_dir = text,
                        "simulation_replay_file" => config.simulation_replay_file = text,
                        _ => match ConnectionKind::parse(&text) {
                            Some(kind) => config.connection_kind = kind,
                            None => warnings.push(format!("Unbekannte Verbindungsart '{text}'")),
                        },
                    }
                }
                "simulation_mode" | "show_safety_notice" | "raw_traffic_logging" => {
                    let Some(flag) = value.as_bool() else {
                        warnings.push(format!("Ungültiger Wert für {key}: {value}"));
                        continue;
                    };
                    match key.as_str() {
                        "simulation_mode" => config.simulation_mode = flag,
                        "show_safety_notice" => config.show_safety_notice = flag,
                        _ => config.raw_traffic_logging = flag,
                    }
                }
                "network_port" | "terminal_history_lines" | "font_scale_percent" => {
                    let Some(number) = value.as_u64() else {
                        warnings.push(format!("Ungültiger Wert für {key}: {value}"));
                        continue;
                    };
                    match key.as_str() {
                        "network_port" => match u32::try_from(number) {
                            Ok(port) if (1..=65535).contains(&port) => config.network_port = port,
                            _ => warnings.push(format!("Ungültiger TCP-Port {number}")),
                        },
                        "terminal_history_lines" => config.terminal_history_lines = number.clamp(100, 100_000),
                        _ => config.font_scale_percent = number.clamp(80, 200),
                    }
                }
                "recent_files" => match value.as_array() {
                    Some(items) if items.iter().all(Value::is_string) => {
                        config.recent_files =
                            items.iter().filter_map(Value::as_str).take(MAX_RECENT_FILES).map(String::from).collect();
                    }
                    _ => warnings.push(format!("Ungültiger Wert für {key}: {value}")),
                },
                _ => {}
            }
        }
        if !LOG_LEVELS.contains(&config.log_level.as_str()) {
            warnings.push(format!("Unbekanntes Log-Level '{}'", config.log_level));
            config.log_level = "INFO".into();
        }
        (config, warnings)
    }

    pub fn to_json(&self) -> Map<String, Value> {
        let mut map = self.extra.clone();
        let known = json!({
            "port": self.port,
            "connection_kind": self.connection_kind.as_str(),
            "network_host": self.network_host,
            "network_port": self.network_port,
            "simulation_mode": self.simulation_mode,
            "simulation_scenario": self.simulation_scenario,
            "active_profile_id": self.active_profile_id,
            "show_safety_notice": self.show_safety_notice,
            "log_level": self.log_level,
            "raw_traffic_logging": self.raw_traffic_logging,
            "terminal_history_lines": self.terminal_history_lines,
            "last_open_dir": self.last_open_dir,
            "recent_files": self.recent_files,
            "font_scale_percent": self.font_scale_percent,
            "simulation_replay_file": self.simulation_replay_file,
        });
        if let Value::Object(known) = known {
            for (key, value) in known {
                map.insert(key, value);
            }
        }
        map
    }

    /// Load `config_dir/settings.json`; defaults (with a warning) if unreadable.
    pub fn load(config_dir: &Path) -> (Self, Vec<String>) {
        let path = config_dir.join(CONFIG_FILE);
        if !path.is_file() {
            return (Self::default(), Vec::new());
        }
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                return (
                    Self::default(),
                    vec![format!("Einstellungen konnten nicht gelesen werden ({err}); Standardwerte aktiv.")],
                )
            }
        };
        match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => Self::from_json(&map),
            Ok(_) => {
                (Self::default(), vec!["Einstellungsdatei hat ein ungültiges Format; Standardwerte aktiv.".into()])
            }
            Err(err) => (
                Self::default(),
                vec![format!("Einstellungen konnten nicht gelesen werden ({err}); Standardwerte aktiv.")],
            ),
        }
    }

    /// Save atomically; the previous version is kept as `settings.json.bak`.
    pub fn save(&self, config_dir: &Path) -> Result<(), StorageError> {
        let path = config_dir.join(CONFIG_FILE);
        if path.is_file() {
            fs::copy(&path, config_dir.join(format!("{CONFIG_FILE}.bak")))?;
        }
        let mut text =
            serde_json::to_string_pretty(&Value::Object(self.to_json())).map_err(|e| StorageError::Io(e.into()))?;
        text.push('\n');
        atomic_write(&path, text.as_bytes(), true)
    }
}

/// A stored received program.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredReceive {
    pub nc_path: PathBuf,
    pub meta_path: PathBuf,
    pub raw_path: Option<PathBuf>,
    pub complete: bool,
    pub metadata: Map<String, Value>,
}

/// An entry of the archive list.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchivedProgram {
    pub path: PathBuf,
    /// "received" or "failed"
    pub folder: &'static str,
    pub size: u64,
    pub modified: Option<DateTime<Local>>,
    pub metadata: Option<Map<String, Value>>,
}

impl ArchivedProgram {
    pub fn complete(&self) -> Option<bool> {
        self.metadata.as_ref().map(|m| m.get("complete").and_then(Value::as_bool).unwrap_or(false))
    }
}

/// Program name from the data if the profile pattern finds one, else `RECEIVED_<timestamp>`.
pub fn derive_stem(data: &[u8], pattern: &str, when: &DateTime<FixedOffset>) -> String {
    if !pattern.is_empty() {
        if let Ok(re) = Regex::new(pattern) {
            let text = decode_ascii(data);
            if let Some(name) = re.captures(&text).and_then(|c| c.get(1)).map(|m| m.as_str().to_string()) {
                let name = sanitize_stem(&name, "");
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    format!("RECEIVED_{}", when.format("%Y%m%d_%H%M%S"))
}

fn read_metadata(path: &Path) -> Option<Map<String, Value>> {
    let text = fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Value>(&text).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// Versioned archive for received programs (never overwrites).
#[derive(Debug, Clone)]
pub struct ReceiveArchive {
    pub paths: AppPaths,
}

impl ReceiveArchive {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    fn write_unique(directory: &Path, stem: &str, suffix: &str, data: &[u8]) -> Result<PathBuf, StorageError> {
        fs::create_dir_all(directory)?;
        let mut stem = stem.to_string();
        for _ in 0..5 {
            let path = unique_path(directory, &stem, suffix)?;
            if suffix == NC_SUFFIX && path.with_extension("json").exists() {
                stem = format!("{}_x", path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
                continue;
            }
            match write_new_file(&path, data) {
                Ok(()) => return Ok(path),
                Err(StorageError::Exists(_)) => continue,
                Err(other) => return Err(other),
            }
        }
        Err(StorageError::NoFreeName(format!("{stem}{suffix}"), directory.display().to_string()))
    }

    /// Complete programs go to `received/`, incomplete ones to `failed/`; `Ok(None)` if nothing arrived.
    pub fn store(
        &self,
        result: &ReceiveResult,
        program: &ReceivedProgram,
        profile: &Profile,
    ) -> Result<Option<StoredReceive>, StorageError> {
        if result.raw.is_empty() {
            return Ok(None);
        }
        let when = result.first_byte_at.unwrap_or(result.started_at);
        let stem = derive_stem(&program.normalized, &profile.receive.program_name_pattern, &when);
        let target = if program.complete { &self.paths.received } else { &self.paths.failed };
        let nc_path = Self::write_unique(target, &stem, NC_SUFFIX, &program.normalized)?;
        let final_stem = nc_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(stem);
        let raw_path = if profile.receive.save_raw {
            Some(Self::write_unique(&self.paths.raw, &final_stem, ".raw", &program.raw)?)
        } else {
            None
        };
        let raw_relative =
            raw_path.as_ref().map(|p| p.strip_prefix(&self.paths.root).unwrap_or(p).to_string_lossy().into_owned());
        let serial = profile_to_json(profile).get("serial").cloned().unwrap_or(Value::Null);
        let metadata = json!({
            "application": crate::parameters::archive::APPLICATION,
            "received_at": isoformat(&when),
            "started_at": isoformat(&result.started_at),
            "last_byte_at": result.last_byte_at.as_ref().map(isoformat),
            "duration_s": (result.duration_s * 1000.0).round() / 1000.0,
            "profile_id": profile.id,
            "profile_verification_status": profile.verification_status.as_str(),
            "port": result.port,
            "simulated": result.simulated,
            "serial": serial,
            "raw_bytes": program.raw.len(),
            "normalized_bytes": program.normalized.len(),
            "sha256_raw": sha256_hex(&program.raw),
            "sha256_normalized": sha256_hex(&program.normalized),
            "end_reason": result.end_reason.value(),
            "state": result.state.value(),
            "complete": program.complete,
            "decode_errors": program.decode_errors,
            "notes": program.notes,
            "findings": program.analysis.findings.iter().map(finding_json).collect::<Vec<_>>(),
            "nc_file": nc_path.file_name().map(|n| n.to_string_lossy().into_owned()),
            "raw_file": raw_relative,
        });
        let meta_path = nc_path.with_extension("json");
        let mut text = serde_json::to_string_pretty(&metadata).map_err(|e| StorageError::Io(e.into()))?;
        text.push('\n');
        write_new_file(&meta_path, text.as_bytes())?;
        let metadata = match metadata {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        Ok(Some(StoredReceive { nc_path, meta_path, raw_path, complete: program.complete, metadata }))
    }

    /// All archived programs, newest first.
    pub fn list_programs(&self) -> Vec<ArchivedProgram> {
        let mut programs = Vec::new();
        for (folder, dir) in [("received", &self.paths.received), ("failed", &self.paths.failed)] {
            let Ok(entries) = fs::read_dir(dir) else { continue };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if !path.is_file() || !path.to_string_lossy().ends_with(NC_SUFFIX) {
                    continue;
                }
                let meta = entry.metadata().ok();
                programs.push(ArchivedProgram {
                    metadata: read_metadata(&path.with_extension("json")),
                    folder,
                    size: meta.as_ref().map_or(0, std::fs::Metadata::len),
                    modified: meta.and_then(|m| m.modified().ok()).map(DateTime::<Local>::from),
                    path,
                });
            }
        }
        programs.sort_by_key(|p| std::cmp::Reverse(p.modified));
        programs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::builtin_profiles;
    use crate::transfer::receiver::process_received;
    use crate::transfer::{EndReason, ReceiveState};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deckellink-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn config_roundtrip_keeps_unknown_keys() {
        let dir = temp("config");
        let mut map = Map::new();
        map.insert("simulation_mode".into(), Value::Bool(false));
        map.insert("network_port".into(), json!(70000));
        map.insert("tool_export_header".into(), json!("PYTHON"));
        map.insert("log_level".into(), json!("LAUT"));
        let (mut config, warnings) = AppConfig::from_json(&map);
        assert!(!config.simulation_mode);
        assert_eq!(config.network_port, 4001);
        assert_eq!(warnings.len(), 2);
        config.add_recent_file("a.nc");
        config.add_recent_file("b.nc");
        config.add_recent_file("a.nc");
        assert_eq!(config.recent_files, vec!["a.nc", "b.nc"]);
        config.save(&dir).unwrap();
        config.save(&dir).unwrap();
        assert!(dir.join("settings.json.bak").is_file());
        let (loaded, warnings) = AppConfig::load(&dir);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(loaded.recent_files, config.recent_files);
        assert_eq!(loaded.extra.get("tool_export_header"), Some(&json!("PYTHON")));
        fs::write(dir.join(CONFIG_FILE), "kaputt").unwrap();
        let (fallback, warnings) = AppConfig::load(&dir);
        assert!(fallback.simulation_mode && warnings.len() == 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_store_and_list() {
        let dir = temp("archive");
        let paths = AppPaths::from_root(&dir, None);
        paths.ensure().unwrap();
        let archive = ReceiveArchive::new(paths.clone());
        let profile = builtin_profiles().0.into_iter().next().unwrap();
        let at = DateTime::parse_from_rfc3339("2026-11-27T08:00:00+01:00").unwrap();
        let mut result = ReceiveResult {
            state: ReceiveState::Completed,
            raw: b"N10 G0\r\nM30\r\n".to_vec(),
            end_reason: EndReason::EndSequence,
            started_at: at,
            duration_s: 1.23456,
            profile_id: profile.id.clone(),
            port: "SIM".into(),
            simulated: true,
            first_byte_at: Some(at),
            last_byte_at: None,
            error: None,
        };
        let program = process_received(&result, &profile);
        let stored = archive.store(&result, &program, &profile).unwrap().unwrap();
        assert!(stored.complete && stored.nc_path.starts_with(&paths.received));
        assert!(stored.nc_path.ends_with("RECEIVED_20261127_080000.nc"));
        assert_eq!(stored.metadata["duration_s"], json!(1.235));
        assert_eq!(stored.metadata["serial"]["baud_rate"], json!(profile.serial.baud_rate));
        result.raw = b"N10 G0\r\n".to_vec();
        result.end_reason = EndReason::Inactivity;
        let incomplete = process_received(&result, &profile);
        let failed = archive.store(&result, &incomplete, &profile).unwrap().unwrap();
        assert!(failed.nc_path.starts_with(&paths.failed));
        let listed = archive.list_programs();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed.iter().filter(|p| p.complete() == Some(true)).count(), 1);
        assert_eq!(derive_stem(b"%PGM123\r\n", r"%PGM(\d+)", &at), "123");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_paths() {
        let paths = AppPaths::resolve(Some(Path::new("/x")));
        assert_eq!(paths.config, PathBuf::from("/x/config"));
        assert_eq!(paths.logs, PathBuf::from("/x/logs"));
    }
}
