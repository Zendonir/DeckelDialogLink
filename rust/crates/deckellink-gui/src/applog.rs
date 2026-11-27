//! Application log: shown on the "Protokoll" page and appended to `logs/deckellink-rust.log`.
//!
//! No NC file contents are logged here; raw traffic only reaches the file when the user
//! explicitly enabled raw traffic logging.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use chrono::Local;

pub const LOG_FILE: &str = "deckellink-rust.log";
const MAX_ENTRIES: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warning,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warning => "WARNING",
            Level::Error => "ERROR",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "DEBUG" => Level::Debug,
            "WARNING" => Level::Warning,
            "ERROR" => Level::Error,
            _ => Level::Info,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub time: String,
    pub level: Level,
    pub message: String,
}

#[derive(Debug)]
pub struct AppLog {
    pub entries: Vec<LogEntry>,
    pub min_level: Level,
    file: Option<PathBuf>,
    pub file_error: Option<String>,
}

impl AppLog {
    pub fn new(file: Option<PathBuf>, min_level: Level) -> Self {
        Self { entries: Vec::new(), min_level, file, file_error: None }
    }

    pub fn log(&mut self, level: Level, message: impl Into<String>) {
        if level < self.min_level {
            return;
        }
        let now = Local::now();
        let entry = LogEntry { time: now.format("%H:%M:%S").to_string(), level, message: message.into() };
        if let Some(path) = &self.file {
            let line = format!("{} {:<7} {}\n", now.format("%Y-%m-%d %H:%M:%S"), level.as_str(), entry.message);
            let written = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut file| file.write_all(line.as_bytes()));
            if let Err(err) = written {
                // Logging must never stop the application; report once in the UI.
                if self.file_error.is_none() {
                    self.file_error = Some(format!("Protokolldatei nicht beschreibbar ({}): {err}", path.display()));
                }
            }
        }
        self.entries.push(entry);
        if self.entries.len() > MAX_ENTRIES {
            let excess = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..excess);
        }
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.log(Level::Info, message);
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.log(Level::Warning, message);
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.log(Level::Error, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_and_file() {
        let dir = std::env::temp_dir().join(format!("deckellink-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(LOG_FILE);
        let mut log = AppLog::new(Some(path.clone()), Level::Info);
        log.log(Level::Debug, "unsichtbar");
        log.info("Start");
        log.error("Fehler");
        assert_eq!(log.entries.len(), 2);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("INFO    Start") && !text.contains("unsichtbar"));
        let mut broken = AppLog::new(Some(dir.join("fehlt").join("x.log")), Level::Info);
        broken.info("a");
        assert!(broken.file_error.is_some());
        assert_eq!(broken.entries.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
