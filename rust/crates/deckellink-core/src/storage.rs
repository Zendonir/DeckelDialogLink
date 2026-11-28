//! File helpers: checksums, safe file names, exclusive and atomic writes.
//!
//! Rule: files are never overwritten silently.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Datei existiert bereits: {0}")]
    Exists(String),
    #[error("Kein freier Dateiname für {0} in {1}")]
    NoFreeName(String, String),
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Reduce a file stem to safe ASCII characters (letters, digits, '-', '_', '.').
pub fn sanitize_stem(stem: &str, fallback: &str) -> String {
    let cleaned: String =
        stem.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
    let trimmed = cleaned.trim_matches(|c| c == '.' || c == '_').chars().take(80).collect::<String>();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed
    }
}

/// `<dir>/<stem><suffix>`, or `<stem>_001<suffix>` ... if taken.
pub fn unique_path(directory: &Path, stem: &str, suffix: &str) -> Result<PathBuf, StorageError> {
    let candidate = directory.join(format!("{stem}{suffix}"));
    if !candidate.exists() {
        return Ok(candidate);
    }
    for n in 1..=9999 {
        let candidate = directory.join(format!("{stem}_{n:03}{suffix}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(StorageError::NoFreeName(format!("{stem}{suffix}"), directory.display().to_string()))
}

/// Create `path` exclusively; fails if it exists.
pub fn write_new_file(path: &Path, data: &[u8]) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            return Err(StorageError::Exists(file_name(path)));
        }
        Err(err) => return Err(err.into()),
    };
    file.write_all(data)?;
    file.sync_all()?;
    Ok(())
}

/// Write via temporary file + rename. Refuses to replace an existing file unless `overwrite`.
pub fn atomic_write(path: &Path, data: &[u8], overwrite: bool) -> Result<(), StorageError> {
    if path.exists() && !overwrite {
        return Err(StorageError::Exists(file_name(path)));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".{}.tmp{}", file_name(path), std::process::id()));
    {
        let mut file = File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deckellink-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn checksum() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn names_and_exclusive_writes() {
        assert_eq!(sanitize_stem("TEIL 17/ä", "X"), "TEIL_17");
        assert_eq!(sanitize_stem("...", "X"), "X");
        let dir = temp_dir("storage");
        let first = unique_path(&dir, "A", ".nc").unwrap();
        write_new_file(&first, b"1").unwrap();
        assert!(matches!(write_new_file(&first, b"2"), Err(StorageError::Exists(_))));
        assert_eq!(fs::read(&first).unwrap(), b"1");
        let second = unique_path(&dir, "A", ".nc").unwrap();
        assert!(second.ends_with("A_001.nc"));
        assert!(atomic_write(&first, b"3", false).is_err());
        atomic_write(&first, b"3", true).unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"3");
        atomic_write(&dir.join("sub/new.json"), b"{}", false).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
