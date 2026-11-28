//! Profile repository: built-in profiles plus user profiles in `<data>/profiles`.
//!
//! Same rules as the Python version: built-in profiles are never overwritten (saving one
//! creates a user copy with a new id), every replaced or deleted file is backed up first,
//! imported profiles never become built-in.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset};

use crate::model::{Profile, VerificationStatus};
use crate::profile::{builtin_profiles, parse_profile, parse_profile_text, profile_to_json};
use crate::storage::{atomic_write, unique_path, StorageError};

pub const PREFERRED_DEFAULT_ID: &str = "dialog11_7n1_rtscts";

/// Copy `source` to `backups/YYYY-MM-DD/NAME_HH-MM-SS.ext` before it changes (never deletes).
pub fn backup_file(backup_dir: &Path, source: &Path, now: &DateTime<FixedOffset>) -> Result<PathBuf, StorageError> {
    let day_dir = backup_dir.join(now.format("%Y-%m-%d").to_string());
    fs::create_dir_all(&day_dir)?;
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let suffix = source.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let target = unique_path(&day_dir, &format!("{stem}_{}", now.format("%H-%M-%S")), &suffix)?;
    fs::copy(source, &target)?;
    Ok(target)
}

#[derive(Debug, Clone)]
pub struct ProfileStore {
    pub user_dir: PathBuf,
    pub backup_dir: PathBuf,
    profiles: Vec<Profile>,
    paths: HashMap<String, PathBuf>,
    warnings: HashMap<String, Vec<String>>,
    /// Files that could not be loaded (German messages).
    pub problems: Vec<String>,
}

fn to_text(profile: &Profile) -> String {
    serde_json::to_string_pretty(&profile_to_json(profile)).unwrap_or_default() + "\n"
}

impl ProfileStore {
    pub fn load(user_dir: &Path, backup_dir: &Path) -> Self {
        let mut store = Self {
            user_dir: user_dir.to_path_buf(),
            backup_dir: backup_dir.to_path_buf(),
            profiles: Vec::new(),
            paths: HashMap::new(),
            warnings: HashMap::new(),
            problems: Vec::new(),
        };
        store.reload();
        store
    }

    pub fn reload(&mut self) {
        self.profiles.clear();
        self.paths.clear();
        self.warnings.clear();
        self.problems.clear();
        let (builtin, problems) = builtin_profiles();
        self.problems.extend(problems.iter().map(ToString::to_string));
        self.profiles.extend(builtin);
        let mut files: Vec<PathBuf> = fs::read_dir(&self.user_dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        for path in files {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let parsed = fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| parse_profile_text(&text, &name, false).map_err(|e| e.to_string()));
            match parsed {
                Ok((profile, _)) if self.get(&profile.id).is_some() => self
                    .problems
                    .push(format!("Profil-ID '{}' ist bereits vergeben ({name}); Datei wird ignoriert.", profile.id)),
                Ok((profile, warnings)) => {
                    self.paths.insert(profile.id.clone(), path);
                    self.warnings.insert(profile.id.clone(), warnings);
                    self.profiles.push(profile);
                }
                Err(err) => self.problems.push(format!("{name}: {err}")),
            }
        }
        self.sort();
    }

    fn sort(&mut self) {
        let (mut builtin, mut user): (Vec<Profile>, Vec<Profile>) = self.profiles.drain(..).partition(|p| p.builtin);
        user.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        builtin.append(&mut user);
        self.profiles = builtin;
    }

    /// Built-in profiles first, then user profiles sorted by name.
    pub fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn is_builtin(&self, id: &str) -> bool {
        self.get(id).is_some_and(|p| p.builtin)
    }

    pub fn path_for(&self, id: &str) -> Option<&PathBuf> {
        self.paths.get(id)
    }

    pub fn warnings_for(&self, id: &str) -> Vec<String> {
        self.warnings.get(id).cloned().unwrap_or_default()
    }

    /// The requested profile or the safe default (second value: fell back).
    pub fn resolve(&self, id: &str) -> (Profile, bool) {
        if let Some(profile) = self.get(id) {
            return (profile.clone(), false);
        }
        let fallback = self
            .get(PREFERRED_DEFAULT_ID)
            .or_else(|| self.profiles.iter().find(|p| p.builtin))
            .cloned()
            .unwrap_or_else(Profile::fallback);
        (fallback, !id.is_empty())
    }

    pub fn unique_id(&self, base: &str) -> String {
        let cleaned: String = base
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' { c } else { '_' })
            .take(56)
            .collect();
        let base = if cleaned.is_empty() { "profil".to_string() } else { cleaned };
        let taken = |id: &str| self.get(id).is_some() || self.user_dir.join(format!("{id}.json")).exists();
        let mut candidate = base.clone();
        let mut n = 1u32;
        while taken(&candidate) {
            candidate = format!("{base}_{n:02}");
            n += 1;
        }
        candidate
    }

    /// Persist as a user profile. A built-in id gets a user copy with a new id.
    pub fn save(&mut self, profile: &Profile, now: &DateTime<FixedOffset>) -> Result<(Profile, Vec<String>), String> {
        let mut profile = profile.clone();
        if self.is_builtin(&profile.id) || profile.id == Profile::fallback().id {
            profile.id = self.unique_id(&format!("{}_benutzer", profile.id));
            if !profile.display_name.contains("(angepasst)") {
                profile.display_name = format!("{} (angepasst)", profile.display_name);
            }
            profile.display_name = profile.display_name.chars().take(120).collect();
        }
        profile.builtin = false;
        // Validate the serialized form so stored files are always loadable.
        let data = profile_to_json(&profile);
        let (validated, warnings) =
            parse_profile(&data, &format!("{}.json", profile.id), false).map_err(|e| e.to_string())?;
        fs::create_dir_all(&self.user_dir).map_err(|e| e.to_string())?;
        let target = self.user_dir.join(format!("{}.json", validated.id));
        if target.exists() {
            backup_file(&self.backup_dir, &target, now).map_err(|e| e.to_string())?;
        }
        atomic_write(&target, to_text(&validated).as_bytes(), true).map_err(|e| e.to_string())?;
        self.profiles.retain(|p| p.id != validated.id);
        self.profiles.push(validated.clone());
        self.paths.insert(validated.id.clone(), target);
        self.warnings.insert(validated.id.clone(), warnings.clone());
        self.sort();
        Ok((validated, warnings))
    }

    pub fn duplicate(&mut self, id: &str, now: &DateTime<FixedOffset>) -> Result<(Profile, Vec<String>), String> {
        let source = self.get(id).cloned().ok_or_else(|| format!("Profil '{id}' nicht gefunden."))?;
        let mut copy = source.clone();
        copy.id = self.unique_id(&format!("{}_kopie", source.id));
        copy.display_name = format!("{} (Kopie)", source.display_name).chars().take(120).collect();
        copy.builtin = false;
        self.save(&copy, now)
    }

    /// Delete a user profile (backup kept). Built-in profiles cannot be deleted.
    pub fn delete(&mut self, id: &str, now: &DateTime<FixedOffset>) -> Result<(), String> {
        if self.is_builtin(id) {
            return Err("Mitgelieferte Standardprofile können nicht gelöscht werden.".into());
        }
        let path = self.paths.get(id).cloned().ok_or_else(|| format!("Profil '{id}' nicht gefunden."))?;
        backup_file(&self.backup_dir, &path, now).map_err(|e| e.to_string())?;
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        self.profiles.retain(|p| p.id != id);
        self.paths.remove(id);
        self.warnings.remove(id);
        Ok(())
    }

    /// Write a profile as JSON; an existing target is only replaced with `overwrite` (backup first).
    pub fn export_profile(
        &self,
        id: &str,
        target: &Path,
        overwrite: bool,
        now: &DateTime<FixedOffset>,
    ) -> Result<Option<PathBuf>, String> {
        let profile = self.get(id).ok_or_else(|| format!("Profil '{id}' nicht gefunden."))?;
        let mut backup = None;
        if target.exists() {
            if !overwrite {
                return Err(format!(
                    "Datei existiert bereits: {}",
                    target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                ));
            }
            backup = Some(backup_file(&self.backup_dir, target, now).map_err(|e| e.to_string())?);
        }
        atomic_write(target, to_text(profile).as_bytes(), true).map_err(|e| e.to_string())?;
        Ok(backup)
    }

    /// Validate a profile file and store it as a user profile (an existing id gets a new one).
    pub fn import_profile(
        &mut self,
        source: &Path,
        now: &DateTime<FixedOffset>,
    ) -> Result<(Profile, Vec<String>), String> {
        let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let text = fs::read_to_string(source).map_err(|e| format!("{name}: {e}"))?;
        let (mut profile, mut warnings) = parse_profile_text(&text, &name, false).map_err(|e| e.to_string())?;
        if self.get(&profile.id).is_some() || self.user_dir.join(format!("{}.json", profile.id)).exists() {
            let new_id = self.unique_id(&format!("{}_import", profile.id));
            warnings.push(format!("Profil-ID '{}' existiert bereits - importiert als '{new_id}'.", profile.id));
            profile.id = new_id;
            profile.display_name = format!("{} (Import)", profile.display_name).chars().take(120).collect();
        }
        profile.builtin = false;
        let (stored, save_warnings) = self.save(&profile, now)?;
        if stored.verification_status == VerificationStatus::Verified {
            warnings.push("Das importierte Profil ist als 'verifiziert' gekennzeichnet - Herkunft prüfen.".into());
        }
        for warning in save_warnings {
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
        Ok((stored, warnings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-27T10:11:12+01:00").unwrap()
    }

    #[test]
    fn builtin_never_overwritten() {
        let root = std::env::temp_dir().join(format!("deckellink-profiles-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut store = ProfileStore::load(&root.join("profiles"), &root.join("backups"));
        let builtin_count = store.profiles().len();
        let mut edited = store.get(PREFERRED_DEFAULT_ID).unwrap().clone();
        edited.serial.baud_rate = 9600;
        let (saved, _) = store.save(&edited, &now()).unwrap();
        assert_eq!(saved.id, "dialog11_7n1_rtscts_benutzer");
        assert!(saved.display_name.ends_with("(angepasst)"));
        assert_eq!(store.get(PREFERRED_DEFAULT_ID).unwrap().serial.baud_rate, 4800);
        // second save of the user profile replaces it with a backup
        let mut again = saved.clone();
        again.serial.baud_rate = 2400;
        store.save(&again, &now()).unwrap();
        assert!(root.join("backups/2026-11-27/dialog11_7n1_rtscts_benutzer_10-11-12.json").is_file());
        let (copy, _) = store.duplicate(&saved.id, &now()).unwrap();
        assert_eq!(copy.id, "dialog11_7n1_rtscts_benutzer_kopie");
        assert!(store.delete(PREFERRED_DEFAULT_ID, &now()).is_err());
        store.delete(&copy.id, &now()).unwrap();
        // export and re-import gets a new id
        let file = root.join("export.json");
        store.export_profile(&saved.id, &file, false, &now()).unwrap();
        assert!(store.export_profile(&saved.id, &file, false, &now()).is_err());
        let (imported, warnings) = store.import_profile(&file, &now()).unwrap();
        assert_eq!(imported.id, "dialog11_7n1_rtscts_benutzer_import");
        assert!(warnings.iter().any(|w| w.contains("existiert bereits")));
        let reloaded = ProfileStore::load(&root.join("profiles"), &root.join("backups"));
        assert_eq!(reloaded.profiles().len(), builtin_count + 2);
        assert!(reloaded.problems.is_empty(), "{:?}", reloaded.problems);
        assert_eq!(reloaded.resolve("fehlt").0.id, PREFERRED_DEFAULT_ID);
        let _ = fs::remove_dir_all(&root);
    }
}
