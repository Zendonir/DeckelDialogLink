//! Versioned storage of machine parameter sets (never overwritten).
//!
//! Same layout and metadata as the Python version: `<root>/parameters/PARAM_<date>_<time>.par`,
//! metadata `.json` next to it, raw bytes under `parameters/raw/`.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, Timelike};
use serde_json::{json, Map, Value};

use crate::model::Profile;
use crate::nc::Finding;
use crate::storage::{sha256_hex, unique_path, write_new_file, StorageError};
use crate::transfer::receiver::ReceivedProgram;
use crate::transfer::ReceiveResult;

pub const PARAM_SUFFIX: &str = ".par";
pub const APPLICATION: &str = concat!("DeckelLink ", env!("CARGO_PKG_VERSION"));

/// Python `datetime.isoformat()` for an aware timestamp.
pub fn isoformat(when: &DateTime<FixedOffset>) -> String {
    let micros = when.nanosecond() / 1000 % 1_000_000;
    if micros == 0 {
        when.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
    } else {
        format!("{}.{micros:06}{}", when.format("%Y-%m-%dT%H:%M:%S"), when.format("%:z"))
    }
}

/// Python `Finding.to_dict()` (at most 50 line numbers).
pub fn finding_json(finding: &Finding) -> Value {
    json!({
        "code": finding.code,
        "severity": finding.severity.as_str(),
        "message": finding.message,
        "lines": finding.lines.iter().take(50).collect::<Vec<_>>(),
        "count": finding.count,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredParameterSet {
    pub path: PathBuf,
    pub meta_path: PathBuf,
    pub raw_path: Option<PathBuf>,
    pub metadata: Map<String, Value>,
}

impl StoredParameterSet {
    pub fn complete(&self) -> bool {
        self.metadata.get("complete").and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn sha256(&self) -> String {
        self.metadata.get("sha256_normalized").and_then(Value::as_str).unwrap_or_default().to_string()
    }

    pub fn read_at(&self) -> Option<DateTime<FixedOffset>> {
        self.metadata.get("received_at").and_then(Value::as_str).and_then(|s| DateTime::parse_from_rfc3339(s).ok())
    }

    fn sort_key(&self) -> (String, String) {
        let received = self.metadata.get("received_at").and_then(Value::as_str).unwrap_or_default().to_string();
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        (received, name)
    }
}

/// Stores parameter sets under `<root>/parameters` (raw data under `parameters/raw`).
#[derive(Debug, Clone)]
pub struct ParameterArchive {
    pub directory: PathBuf,
    pub raw_directory: PathBuf,
}

impl ParameterArchive {
    pub fn new(root: &Path) -> Self {
        let directory = root.join("parameters");
        let raw_directory = directory.join("raw");
        Self { directory, raw_directory }
    }

    /// Store a received parameter set; `Ok(None)` if nothing was received. Never overwrites.
    pub fn store(
        &self,
        result: &ReceiveResult,
        program: &ReceivedProgram,
        profile: &Profile,
    ) -> Result<Option<StoredParameterSet>, StorageError> {
        if result.raw.is_empty() {
            return Ok(None);
        }
        let when = result.first_byte_at.unwrap_or(result.started_at);
        let stem = format!("PARAM_{}", when.format("%Y%m%d_%H%M%S"));
        let path = write_unique(&self.directory, &stem, PARAM_SUFFIX, &program.normalized)?;
        let file_stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(stem);
        let raw_path = write_unique(&self.raw_directory, &file_stem, ".raw", &program.raw)?;
        let findings: Vec<Value> = program.analysis.findings.iter().map(finding_json).collect();
        let metadata = json!({
            "type": "machine_parameters",
            "application": APPLICATION,
            "received_at": isoformat(&when),
            "profile_id": profile.id,
            "port": result.port,
            "simulated": result.simulated,
            "raw_bytes": program.raw.len(),
            "normalized_bytes": program.normalized.len(),
            "sha256_raw": sha256_hex(&program.raw),
            "sha256_normalized": sha256_hex(&program.normalized),
            "end_reason": result.end_reason.value(),
            "state": result.state.value(),
            "complete": program.complete && program.decode_errors == 0,
            "decode_errors": program.decode_errors,
            "findings": findings,
            "note": "Vollständigkeit nur anhand des Übertragungsendes beurteilt (Parameterformat nicht verifiziert).",
        });
        let meta_path = path.with_extension("json");
        let mut text = serde_json::to_string_pretty(&metadata).map_err(|e| StorageError::Io(e.into()))?;
        text.push('\n');
        write_new_file(&meta_path, text.as_bytes())?;
        let metadata = match metadata {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        Ok(Some(StoredParameterSet { path, meta_path, raw_path: Some(raw_path), metadata }))
    }

    /// All stored sets, newest first. Unreadable metadata yields an empty map (the set stays listed).
    pub fn list_sets(&self) -> Vec<StoredParameterSet> {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return Vec::new();
        };
        let mut sets: Vec<StoredParameterSet> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.to_string_lossy().ends_with(PARAM_SUFFIX))
            .map(|path| {
                let meta_path = path.with_extension("json");
                let metadata = fs::read_to_string(&meta_path)
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .and_then(|value| match value {
                        Value::Object(map) => Some(map),
                        _ => None,
                    })
                    .unwrap_or_default();
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let raw = self.raw_directory.join(format!("{stem}.raw"));
                let raw_path = raw.is_file().then_some(raw);
                StoredParameterSet { path, meta_path, raw_path, metadata }
            })
            .collect();
        sets.sort_by_key(|set| std::cmp::Reverse(set.sort_key()));
        sets
    }
}

fn write_unique(directory: &Path, stem: &str, suffix: &str, data: &[u8]) -> Result<PathBuf, StorageError> {
    fs::create_dir_all(directory)?;
    let mut last = None;
    for _ in 0..5 {
        let path = unique_path(directory, stem, suffix)?;
        match write_new_file(&path, data) {
            Ok(()) => return Ok(path),
            Err(StorageError::Exists(name)) => last = Some(name),
            Err(other) => return Err(other),
        }
    }
    Err(StorageError::NoFreeName(last.unwrap_or_else(|| format!("{stem}{suffix}")), directory.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::builtin_profiles;
    use crate::transfer::receiver::process_received;
    use crate::transfer::{EndReason, ReceiveState};

    fn result(raw: &[u8]) -> ReceiveResult {
        let at = DateTime::parse_from_rfc3339("2026-11-27T10:15:30.000250+01:00").unwrap();
        ReceiveResult {
            state: ReceiveState::Completed,
            raw: raw.to_vec(),
            end_reason: EndReason::Inactivity,
            started_at: at,
            duration_s: 1.0,
            profile_id: "p".into(),
            port: "SIMULATOR".into(),
            simulated: true,
            first_byte_at: Some(at),
            last_byte_at: Some(at),
            error: None,
        }
    }

    #[test]
    fn stores_without_overwriting() {
        let dir = std::env::temp_dir().join(format!("deckellink-param-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let archive = ParameterArchive::new(&dir);
        let profile = crate::parameters::parameter_profile(&builtin_profiles().0.into_iter().next().unwrap());
        let r = result(b"%MPG00 (KOPF)\r\n?\r\n0000\r\n");
        let program = process_received(&r, &profile);
        let first = archive.store(&r, &program, &profile).unwrap().unwrap();
        let second = archive.store(&r, &program, &profile).unwrap().unwrap();
        assert!(first.path.ends_with("PARAM_20261127_101530.par"));
        assert!(second.path.ends_with("PARAM_20261127_101530_001.par"));
        assert!(second.raw_path.as_ref().unwrap().ends_with("raw/PARAM_20261127_101530_001.raw"));
        assert_eq!(first.metadata["received_at"], "2026-11-27T10:15:30.000250+01:00");
        assert!(first.complete());
        let sets = archive.list_sets();
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0].path, second.path);
        assert_eq!(sets[0].sha256(), first.sha256());
        assert!(sets[0].read_at().is_some());
        assert!(archive.store(&result(b""), &program, &profile).unwrap().is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
