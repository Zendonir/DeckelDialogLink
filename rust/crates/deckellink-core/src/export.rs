//! Diagnostic package export (ZIP), as in the Python version.
//!
//! Contains logs, anonymised configuration, the active profile, system info and the last
//! transfer result. NC files and raw data are only included when explicitly selected.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{DateTime, FixedOffset};
use regex::{Captures, Regex};
use serde_json::{json, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::appdata::{AppConfig, AppPaths};
use crate::model::Profile;
use crate::profile::profile_to_json;
use crate::storage::{unique_path, StorageError};

fn path_res() -> Option<&'static (Regex, Regex)> {
    static RES: OnceLock<Option<(Regex, Regex)>> = OnceLock::new();
    RES.get_or_init(|| {
        let windows = Regex::new(r#"[A-Za-z]:\\[^\s"'<>|]*"#).ok()?;
        // Python: (?<![\w.])/(?:home|Users|...)/... - the look-behind is checked in `mask_paths`.
        let posix = Regex::new(r#"/(?:home|Users|root|tmp|var|mnt|media)/[^\s"'<>|]*"#).ok()?;
        Some((windows, posix))
    })
    .as_ref()
}

fn mask_one(value_with_tail: &str) -> String {
    let value = value_with_tail.trim_end_matches(['.', ',', ';', ':', ')']);
    let tail = value_with_tail.get(value.len()..).unwrap_or_default();
    let name = value.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or_default();
    format!("<PFAD>/{name}{tail}")
}

/// Replace absolute paths by `<PFAD>/<dateiname>`.
pub fn mask_paths(text: &str) -> String {
    let Some((windows, posix)) = path_res() else {
        return text.to_string();
    };
    let step1 = windows.replace_all(text, |c: &Captures<'_>| mask_one(c.get(0).map_or("", |m| m.as_str())));
    let mut out = String::with_capacity(step1.len());
    let mut last = 0usize;
    for m in posix.find_iter(&step1) {
        let before = step1.get(..m.start()).and_then(|s| s.chars().next_back());
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
            continue;
        }
        out.push_str(step1.get(last..m.start()).unwrap_or_default());
        out.push_str(&mask_one(m.as_str()));
        last = m.end();
    }
    out.push_str(step1.get(last..).unwrap_or_default());
    out
}

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    pub mask_paths: bool,
    pub include_nc_files: Vec<PathBuf>,
    pub include_raw_files: Vec<PathBuf>,
    pub last_result: Option<Value>,
}

pub fn system_info(version: &str) -> Value {
    json!({
        "application": "DeckelLink",
        "version": version,
        "implementation": "Rust",
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

pub fn anonymized_config(config: &AppConfig) -> Value {
    let mut map = config.to_json();
    let removed = if config.last_open_dir.is_empty() { "" } else { "<entfernt>" };
    map.insert("last_open_dir".into(), Value::String(removed.into()));
    Value::Object(map)
}

fn log_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().contains(".log")))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn zip_error(err: zip::result::ZipError) -> StorageError {
    StorageError::Io(std::io::Error::other(err.to_string()))
}

/// Write `DeckelLink_Diagnose_<timestamp>.zip` into `target_dir` (never overwrites).
pub fn export_diagnostics(
    paths: &AppPaths,
    config: &AppConfig,
    profile: &Profile,
    options: &ExportOptions,
    now: &DateTime<FixedOffset>,
    version: &str,
    target_dir: Option<&Path>,
) -> Result<PathBuf, StorageError> {
    let target_dir = target_dir.unwrap_or(&paths.exports);
    fs::create_dir_all(target_dir)?;
    let target = unique_path(target_dir, &format!("DeckelLink_Diagnose_{}", now.format("%Y%m%d_%H%M%S")), ".zip")?;
    let file = OpenOptions::new().write(true).create_new(true).open(&target)?;
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let clean = |text: String| if options.mask_paths { mask_paths(&text) } else { text };
    let pretty = |value: &Value| serde_json::to_string_pretty(value).unwrap_or_default();
    let add = |zip: &mut ZipWriter<fs::File>, name: &str, data: &[u8]| -> Result<(), StorageError> {
        zip.start_file(name, opts).map_err(zip_error)?;
        zip.write_all(data)?;
        Ok(())
    };
    add(&mut zip, "system.json", pretty(&system_info(version)).as_bytes())?;
    add(&mut zip, "config.json", clean(pretty(&anonymized_config(config))).as_bytes())?;
    add(&mut zip, "profile.json", pretty(&profile_to_json(profile)).as_bytes())?;
    if let Some(result) = &options.last_result {
        add(&mut zip, "last_result.json", clean(pretty(result)).as_bytes())?;
    }
    for log in log_files(&paths.logs) {
        let Ok(bytes) = fs::read(&log) else { continue };
        let name = log.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        add(&mut zip, &format!("logs/{name}"), clean(String::from_utf8_lossy(&bytes).into_owned()).as_bytes())?;
    }
    for (folder, files) in [("nc", &options.include_nc_files), ("raw", &options.include_raw_files)] {
        for path in files.iter().filter(|p| p.is_file()) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            add(&mut zip, &format!("{folder}/{name}"), &fs::read(path)?)?;
        }
    }
    let readme = format!(
        "DeckelLink Diagnosepaket\nPfade maskiert: {}\nNC-Dateien enthalten: {}\nRohdaten enthalten: {}\n",
        if options.mask_paths { "ja" } else { "nein" },
        options.include_nc_files.len(),
        options.include_raw_files.len()
    );
    add(&mut zip, "README.txt", readme.as_bytes())?;
    zip.finish().map_err(zip_error)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::builtin_profiles;

    #[test]
    fn masks_paths_like_python() {
        assert_eq!(mask_paths(r"Datei C:\Users\Max\NC\teil.nc."), "Datei <PFAD>/teil.nc.");
        assert_eq!(mask_paths("in /home/max/nc/teil.nc, ok"), "in <PFAD>/teil.nc, ok");
        assert_eq!(mask_paths("x.y/home/max/a"), "x.y/home/max/a");
        assert_eq!(mask_paths("ohne Pfad"), "ohne Pfad");
    }

    #[test]
    fn writes_zip_with_logs_and_masking() {
        let root = std::env::temp_dir().join(format!("deckellink-export-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = AppPaths::from_root(&root, None);
        paths.ensure().unwrap();
        fs::write(paths.logs.join("deckellink-rust.log"), "Datei /home/max/geheim/teil.nc geladen\n").unwrap();
        let config = AppConfig { last_open_dir: "/home/max/nc".into(), ..AppConfig::default() };
        let profile = builtin_profiles().0.into_iter().next().unwrap();
        let now = DateTime::parse_from_rfc3339("2026-11-27T10:00:00+01:00").unwrap();
        let options = ExportOptions { mask_paths: true, ..ExportOptions::default() };
        let first = export_diagnostics(&paths, &config, &profile, &options, &now, "0.8", None).unwrap();
        let second = export_diagnostics(&paths, &config, &profile, &options, &now, "0.8", None).unwrap();
        assert_ne!(first, second, "never overwritten");
        let mut archive = zip::ZipArchive::new(fs::File::open(&first).unwrap()).unwrap();
        let names: Vec<String> = archive.file_names().filter_map(|n| n.ok().map(|c| c.into_owned())).collect();
        for expected in ["system.json", "config.json", "profile.json", "logs/deckellink-rust.log", "README.txt"] {
            assert!(names.iter().any(|n| n == expected), "{names:?}");
        }
        let mut log = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("logs/deckellink-rust.log").unwrap(), &mut log).unwrap();
        assert_eq!(log, "Datei <PFAD>/teil.nc geladen\n");
        let mut cfg = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("config.json").unwrap(), &mut cfg).unwrap();
        assert!(cfg.contains("<entfernt>") && !cfg.contains("/home/max"));
        let _ = fs::remove_dir_all(&root);
    }
}
