//! Additional application features: profile editor, loopback test, data format detection,
//! diagnostic export, replay recordings and the commissioning assistant.

use std::path::PathBuf;

use deckellink_core::clock::{Clock, SystemClock};
use deckellink_core::commissioning::{export_report, CommissioningRecord, CommissioningStore, StepStatus};
use deckellink_core::diagnostics::{
    analyze_capture, capture_passively, make_pattern, run_loopback_test, FormatGuess, LoopbackPattern, LoopbackResult,
    LoopbackVerdict,
};
use deckellink_core::error::UserFacingError;
use deckellink_core::export::{export_diagnostics, ExportOptions};
use deckellink_core::model::{Profile, VerificationStatus};
use deckellink_core::profile::profile_to_json;
use deckellink_core::scenarios::REPLAY_SCENARIO_ID;
use deckellink_core::transport::Transport;
use serde_json::json;

use crate::app::{Action, DeckelApp, Dialog, StatusKind};
use crate::logic::{make_transport, replay_data, Purpose};
use crate::worker::{Outcome, TaskKind};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct ExtraState {
    /// Working copy of the profile being edited (saved as a user profile).
    pub draft: Option<Profile>,
    pub loopback_pattern: LoopbackPattern,
    pub loopback_length: usize,
    pub loopback_timeout_s: f64,
    pub loopback_result: Option<LoopbackResult>,
    pub format_duration_s: u64,
    pub format_result: Option<(usize, FormatGuess)>,
    pub format_error: Option<UserFacingError>,
    pub export_mask_paths: bool,
    pub export_include_file: bool,
    pub last_export: Option<PathBuf>,
    pub commissioning: Option<CommissioningRecord>,
    pub help_doc: usize,
}

impl Default for ExtraState {
    fn default() -> Self {
        Self {
            draft: None,
            loopback_pattern: LoopbackPattern::AsciiText,
            loopback_length: 256,
            loopback_timeout_s: 3.0,
            loopback_result: None,
            format_duration_s: 60,
            format_result: None,
            format_error: None,
            export_mask_paths: true,
            export_include_file: false,
            last_export: None,
            commissioning: None,
            help_doc: 0,
        }
    }
}

/// Technical sections of a profile (anything that changes the data on the wire).
fn technical(profile: &Profile) -> serde_json::Value {
    let mut value = profile_to_json(profile);
    if let Some(map) = value.as_object_mut() {
        for key in ["id", "display_name", "description", "verification_status", "machine"] {
            map.remove(key);
        }
    }
    value
}

impl DeckelApp {
    /// Transport for `purpose`, with the replay recording in the replay scenario.
    pub(crate) fn transport(&mut self, profile: &Profile, purpose: Purpose) -> Box<dyn Transport> {
        let replay = if self.config.simulation_mode {
            match replay_data(&self.config) {
                Ok(data) => data,
                Err(message) => {
                    self.log.warn(message.clone());
                    self.message = Some(message);
                    None
                }
            }
        } else {
            None
        };
        make_transport(&self.config, profile, purpose, replay.as_deref())
    }

    pub(crate) fn handle_extra(&mut self, action: Action) {
        let now = SystemClock::default().now();
        match action {
            Action::EditProfile => {
                if !self.busy() {
                    self.extra.draft = Some(self.profile.clone());
                }
            }
            Action::CancelEdit => self.extra.draft = None,
            Action::SaveDraft => self.save_draft(),
            Action::DuplicateProfile => match self.store.duplicate(&self.profile.id, &now) {
                Ok((copy, _)) => {
                    self.notify(format!("Profil '{}' angelegt", copy.display_name));
                    let id = copy.id.clone();
                    self.select_profile(&id);
                }
                Err(err) => self.message = Some(format!("Kopie nicht möglich: {err}")),
            },
            Action::RequestDeleteProfile => {
                if self.store.is_builtin(&self.profile.id) {
                    self.message = Some("Mitgelieferte Standardprofile können nicht gelöscht werden.".into());
                } else if !self.busy() {
                    self.dialog = Some(Dialog::DeleteProfile);
                }
            }
            Action::ConfirmDeleteProfile => {
                self.dialog = None;
                let id = self.profile.id.clone();
                match self.store.delete(&id, &now) {
                    Ok(()) => {
                        self.notify(format!("Profil '{id}' gelöscht (Sicherung im Ordner backups)"));
                        let (fallback, _) = self.store.resolve("");
                        let fallback_id = fallback.id.clone();
                        self.select_profile(&fallback_id);
                    }
                    Err(err) => self.message = Some(format!("Löschen nicht möglich: {err}")),
                }
            }
            Action::ExportProfileDialog => {
                let picked = rfd::FileDialog::new()
                    .set_title("Profil exportieren")
                    .set_file_name(format!("{}.json", self.profile.id))
                    .add_filter("Profile", &["json"])
                    .save_file();
                if let Some(target) = picked {
                    // The system dialog already asked before replacing; a backup is made first.
                    match self.store.export_profile(&self.profile.id, &target, true, &now) {
                        Ok(backup) => self.notify(format!(
                            "Profil exportiert: {}{}",
                            target.display(),
                            backup
                                .map(|b| format!(" (vorherige Datei gesichert: {})", b.display()))
                                .unwrap_or_default()
                        )),
                        Err(err) => self.message = Some(format!("Export fehlgeschlagen: {err}")),
                    }
                }
            }
            Action::StartLoopback => self.start_loopback(),
            Action::StartFormatDetect => self.start_format_detect(),
            Action::ExportDiagnostics => self.export_diagnostics(),
            Action::ReplayFileDialog => {
                let picked =
                    rfd::FileDialog::new().set_title("Aufzeichnung wählen").set_directory(&self.paths.raw).pick_file();
                if let Some(path) = picked {
                    self.config.simulation_replay_file = path.to_string_lossy().into_owned();
                    self.config.simulation_scenario = REPLAY_SCENARIO_ID.into();
                    self.save_config();
                }
            }
            Action::CommSetStatus(id, status) => {
                if let Some(record) = &mut self.extra.commissioning {
                    record.set_status(&id, status, &now);
                }
                self.save_commissioning();
            }
            Action::CommSave => self.save_commissioning(),
            Action::CommExport => {
                let Some(record) = &self.extra.commissioning else { return };
                match export_report(record, &self.profile, &self.paths.exports, &now, VERSION) {
                    Ok(path) => self.notify(format!("Inbetriebnahme-Protokoll gespeichert: {}", path.display())),
                    Err(err) => self.message = Some(format!("Protokoll konnte nicht gespeichert werden: {err}")),
                }
            }
            Action::CommRequestVerify => {
                if self.extra.commissioning.as_ref().is_some_and(CommissioningRecord::can_mark_verified) {
                    self.dialog = Some(Dialog::MarkVerified);
                }
            }
            Action::CommConfirmVerify => {
                self.dialog = None;
                self.mark_verified();
            }
            Action::DroppedFile(path) => {
                if self.busy() {
                    return;
                }
                if self.page == crate::app::Page::Parameters {
                    self.push(Action::ParamSelectFile(path));
                } else {
                    self.push(Action::LoadFile(path));
                    self.page = crate::app::Page::Transfer;
                }
            }
            other => self.log.warn(format!("Unbehandelte Aktion: {other:?}")),
        }
    }

    fn save_draft(&mut self) {
        let Some(mut draft) = self.extra.draft.clone() else { return };
        let original = self.store.get(&draft.id).cloned();
        let technical_change = original.as_ref().is_none_or(|o| technical(o) != technical(&draft));
        if technical_change && draft.verification_status == VerificationStatus::Verified {
            // A changed profile is no longer the one that was checked at the machine.
            draft.verification_status = VerificationStatus::Unverified;
            self.log.warn(format!("Profil {} geändert - Status auf 'nicht verifiziert' gesetzt", draft.id));
        }
        match self.store.save(&draft, &SystemClock::default().now()) {
            Ok((stored, warnings)) => {
                for warning in &warnings {
                    self.log.warn(format!("Profil {}: {warning}", stored.id));
                }
                self.notify(format!("Profil '{}' gespeichert (ID {})", stored.display_name, stored.id));
                self.extra.draft = None;
                let id = stored.id.clone();
                self.select_profile(&id);
            }
            Err(err) => self.message = Some(format!("Profil nicht gespeichert: {err}")),
        }
    }

    fn start_loopback(&mut self) {
        let gate = self.receive_gate();
        if !gate.allowed {
            self.message = Some(format!("Loopback nicht möglich: {}", gate.reason_text()));
            return;
        }
        let profile = self.profile.clone();
        let packets = make_pattern(self.extra.loopback_pattern, self.extra.loopback_length, profile.serial.data_bits);
        let timeout = self.extra.loopback_timeout_s;
        let mut transport = self.transport(&profile, Purpose::Loopback);
        self.extra.loopback_result = None;
        self.log.info(format!("Loopback-Test gestartet auf {}", self.port_label()));
        self.spawn(TaskKind::Loopback, move |token, sink| {
            let clock = SystemClock::default();
            Outcome::Loopback(run_loopback_test(transport.as_mut(), &packets, timeout, &clock, token, sink))
        });
    }

    pub(crate) fn loopback_finished(&mut self, result: LoopbackResult) {
        let summary = result.summary();
        let kind = match result.verdict {
            LoopbackVerdict::Passed => StatusKind::Ready,
            LoopbackVerdict::Partial => StatusKind::Warning,
            LoopbackVerdict::Failed => StatusKind::Error,
        };
        self.log.info(format!("Loopback-Test: {summary}"));
        self.status = (kind, format!("Loopback-Test: {}", result.verdict.label()));
        if result.verdict == LoopbackVerdict::Passed {
            self.commissioning_auto("loopback", "Loopback bestanden");
        }
        self.extra.loopback_result = Some(result);
    }

    fn start_format_detect(&mut self) {
        let gate = self.receive_gate();
        if !gate.allowed {
            self.message = Some(format!("Mitlesen nicht möglich: {}", gate.reason_text()));
            return;
        }
        let profile = self.profile.clone();
        let duration = f64::from(u32::try_from(self.extra.format_duration_s).unwrap_or(600));
        let mut transport = self.transport(&profile, Purpose::FormatDetect);
        self.extra.format_result = None;
        self.extra.format_error = None;
        self.log.info(format!("Datenformat mitlesen gestartet (nur Empfang) auf {}", self.port_label()));
        self.spawn(TaskKind::FormatDetect, move |token, sink| {
            let clock = SystemClock::default();
            let outcome = capture_passively(transport.as_mut(), duration, 4096, &clock, token, sink)
                .map(|data| (data.len(), analyze_capture(&data)));
            Outcome::Format(outcome)
        });
    }

    pub(crate) fn format_finished(&mut self, result: Result<(usize, FormatGuess), UserFacingError>) {
        match result {
            Ok((bytes, guess)) => {
                self.log.info(format!(
                    "Datenformat: {} ({}), {bytes} Bytes mitgelesen",
                    guess.frame(),
                    guess.confidence
                ));
                self.status = (StatusKind::Ready, format!("Datenformat: {}", guess.frame()));
                if guess.confidence != "keine" {
                    self.commissioning_auto("format", &format!("Ermittelt: {} ({})", guess.frame(), guess.confidence));
                }
                self.extra.format_result = Some((bytes, guess));
            }
            Err(error) => {
                self.status = (StatusKind::Error, "Mitlesen fehlgeschlagen".into());
                self.extra.format_error = Some(error);
            }
        }
    }

    fn export_diagnostics(&mut self) {
        let last_result = self.last_send.as_ref().map(|r| {
            json!({
                "type": "send",
                "state": r.state.label(),
                "bytes_total": r.bytes_total,
                "bytes_sent": r.bytes_sent,
                "duration_s": r.duration_s,
                "port": r.port,
                "profile_id": r.profile_id,
                "simulated": r.simulated,
                "sha256": r.sha256,
                "error": r.error.as_ref().map(UserFacingError::as_text),
            })
        });
        let include_nc_files = if self.extra.export_include_file {
            self.program.as_ref().map(|p| vec![p.path.clone()]).unwrap_or_default()
        } else {
            Vec::new()
        };
        let options = ExportOptions {
            mask_paths: self.extra.export_mask_paths,
            include_nc_files,
            include_raw_files: Vec::new(),
            last_result,
        };
        match export_diagnostics(
            &self.paths,
            &self.config,
            &self.profile,
            &options,
            &SystemClock::default().now(),
            VERSION,
            None,
        ) {
            Ok(path) => {
                self.notify(format!("Diagnosepaket gespeichert: {}", path.display()));
                self.extra.last_export = Some(path);
            }
            Err(err) => self.message = Some(format!("Diagnosepaket konnte nicht erstellt werden: {err}")),
        }
    }

    // ------------------------------------------------------------ commissioning

    pub(crate) fn load_commissioning(&mut self) {
        let store = CommissioningStore::new(&self.paths.root);
        let (record, problem) = store.load(&self.profile.id, &SystemClock::default().now());
        if let Some(problem) = problem {
            self.log.error(problem);
        }
        self.extra.commissioning = Some(record);
    }

    pub(crate) fn save_commissioning(&mut self) {
        let Some(record) = &self.extra.commissioning else { return };
        if let Err(err) = CommissioningStore::new(&self.paths.root).save(record) {
            self.message = Some(format!("Inbetriebnahme-Stand nicht gespeichert: {err}"));
        }
    }

    /// Real-machine results mark the matching step as done (never in simulation).
    pub(crate) fn commissioning_auto(&mut self, step_id: &str, note: &str) {
        if self.simulation() {
            return;
        }
        let now = SystemClock::default().now();
        let Some(record) = &mut self.extra.commissioning else { return };
        if record.profile_id != self.profile.id {
            return;
        }
        record.set_status(step_id, StepStatus::Done, &now);
        if record.step(step_id).note.is_empty() {
            record.set_note(step_id, &format!("{note} ({})", now.format("%d.%m.%Y %H:%M")));
        }
        self.save_commissioning();
    }

    fn mark_verified(&mut self) {
        let Some(record) = self.extra.commissioning.clone() else { return };
        if !record.can_mark_verified() {
            return;
        }
        let now = SystemClock::default().now();
        let note = format!(
            " Verifiziert am {} durch {}{}",
            now.format("%d.%m.%Y"),
            record.tester,
            if record.machine_info.is_empty() { ".".to_string() } else { format!(" ({}).", record.machine_info) }
        );
        let mut verified = self.profile.clone();
        verified.verification_status = VerificationStatus::Verified;
        verified.description = format!("{}{note}", verified.description).trim().to_string();
        match self.store.save(&verified, &now) {
            Ok((stored, _)) => {
                let mut record = record;
                record.profile_id = stored.id.clone();
                if let Err(err) = CommissioningStore::new(&self.paths.root).save(&record) {
                    self.log.error(format!("Inbetriebnahme-Stand nicht gespeichert: {err}"));
                }
                self.log.warn(format!("Profil {} als VERIFIZIERT gekennzeichnet durch {}", stored.id, record.tester));
                let id = stored.id.clone();
                self.select_profile(&id);
                self.notify(format!("Profil '{}' ist als verifiziert gekennzeichnet", stored.display_name));
            }
            Err(err) => self.message = Some(format!("Profil nicht gespeichert: {err}")),
        }
    }
}
