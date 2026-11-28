//! Headless UI flows: every frame is really drawn with egui (no window), actions are the
//! same the buttons record. Covers send/receive/cancel, parameter write locks and mode switch.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use deckellink_core::appdata::AppPaths;
use deckellink_core::parameters::{sample_parameters, WRITE_CONFIRMATION_PHRASE};
use deckellink_core::profile::{builtin_profiles, profile_to_json};
use deckellink_core::transfer::{ReceiveState, SendState};
use deckellink_gui::app::{Action, DeckelApp, Dialog, Page};
use deckellink_gui::ui;

struct Harness {
    ctx: egui::Context,
    app: DeckelApp,
    root: PathBuf,
}

impl Harness {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("deckellink-gui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let ctx = egui::Context::default();
        let app = DeckelApp::new(ctx.clone(), AppPaths::from_root(&root, None));
        Self { ctx, app, root }
    }

    /// Draw one frame and process the recorded actions (like eframe does).
    fn frame(&mut self) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 900.0))),
            ..egui::RawInput::default()
        };
        let app = &mut self.app;
        let mut output = self.ctx.run_ui(input, |ui| ui::show(app, ui));
        // headless: no renderer applies the texture changes
        output.textures_delta.clear();
        self.app.update();
    }

    /// Press a key (like the F keys of the control) and draw a frame.
    fn press(&mut self, key: egui::Key) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 900.0))),
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..egui::RawInput::default()
        };
        let app = &mut self.app;
        let mut output = self.ctx.run_ui(input, |ui| ui::show(app, ui));
        output.textures_delta.clear();
        self.app.update();
    }

    fn act(&mut self, action: Action) {
        self.app.push(action);
        self.frame();
    }

    fn wait_idle(&mut self) {
        let start = Instant::now();
        while self.app.busy() {
            assert!(start.elapsed() < Duration::from_secs(60), "task did not finish");
            std::thread::sleep(Duration::from_millis(20));
            self.frame();
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn all_pages_draw_and_start_in_simulation() {
    let mut h = Harness::new("pages");
    assert!(h.app.simulation(), "safe default: simulation mode");
    for page in Page::ALL {
        h.act(Action::Navigate(page));
        h.frame();
        assert_eq!(h.app.page, page);
    }
}

#[test]
fn send_requires_confirmation_then_completes() {
    let mut h = Harness::new("send");
    h.act(Action::LoadSample);
    assert!(h.app.program.is_some());
    assert!(h.app.send_gate().allowed, "{:?}", h.app.send_gate().reasons);
    h.act(Action::RequestSend { drip_feed: false });
    assert!(matches!(h.app.dialog, Some(Dialog::ConfirmSend { .. })));
    assert!(!h.app.busy(), "nothing is sent before the confirmation");
    h.frame();
    h.act(Action::CloseConfirm);
    assert!(h.app.dialog.is_none() && !h.app.busy());

    h.act(Action::RequestSend { drip_feed: false });
    h.act(Action::ConfirmSend { suppress_safety_notice: false });
    assert!(h.app.busy());
    h.wait_idle();
    assert_eq!(h.app.send_state, SendState::Completed, "{:?}", h.app.send_error);
    assert!(!h.app.traffic.is_empty(), "terminal shows the traffic");
    // the original file is never modified
    let program = h.app.program.clone().unwrap();
    assert_eq!(std::fs::read(&program.path).unwrap(), program.original);
}

#[test]
fn receive_stores_new_file() {
    let mut h = Harness::new("receive");
    h.app.config.simulation_scenario = "receive_ok".into();
    h.act(Action::StartReceive);
    assert_eq!(h.app.page, Page::Receive);
    h.wait_idle();
    assert_eq!(h.app.receive_state, ReceiveState::Completed);
    let summary = h.app.receive_summary.clone().unwrap();
    assert!(summary.complete, "{summary:?}");
    assert!(summary.stored_path.unwrap().is_file());
    assert_eq!(h.app.archive.len(), 1);
    h.act(Action::StartReceive);
    h.wait_idle();
    assert_eq!(h.app.archive.len(), 2, "never overwritten");
    h.act(Action::Navigate(Page::Programs));
}

#[test]
fn cancel_stops_transfer() {
    let mut h = Harness::new("cancel");
    h.app.config.simulation_scenario = "user_cancel".into();
    h.act(Action::LoadSample);
    h.act(Action::RequestSend { drip_feed: false });
    h.act(Action::ConfirmSend { suppress_safety_notice: false });
    std::thread::sleep(Duration::from_millis(100));
    h.act(Action::Cancel);
    h.wait_idle();
    assert_eq!(h.app.send_state, SendState::Cancelled);
}

#[test]
fn drip_feed_only_with_lsv2_and_acknowledgement() {
    let mut h = Harness::new("drip");
    h.act(Action::LoadSample);
    h.act(Action::RequestSend { drip_feed: true });
    assert!(h.app.dialog.is_none(), "plain profile: no drip-feed");
    h.act(Action::SelectProfile("dialog11_lsv2_7e1".into()));
    h.app.config.simulation_scenario = "lsv2_dripfeed".into();
    h.act(Action::RequestSend { drip_feed: true });
    assert!(matches!(h.app.dialog, Some(Dialog::ConfirmSend { drip_feed: true, drip_ack: false, .. })));
    h.frame();
    h.act(Action::ConfirmSend { suppress_safety_notice: false });
    h.wait_idle();
    assert_eq!(h.app.send_state, SendState::Completed, "{:?}", h.app.send_error);
}

#[test]
fn parameter_write_needs_every_lock() {
    let mut h = Harness::new("params");
    // profile allowing writes, imported as a user profile (built-in profiles stay locked)
    let mut profile = builtin_profiles().0.into_iter().find(|p| p.id == "dialog11_7n1_rtscts").unwrap();
    profile.id = "test_schreiben".into();
    profile.display_name = "Test mit Schreibfreigabe".into();
    profile.parameters.allow_write = true;
    let profile_file = h.root.join("import.json");
    std::fs::create_dir_all(&h.root).unwrap();
    std::fs::write(&profile_file, profile_to_json(&profile).to_string()).unwrap();
    let edited =
        String::from_utf8(sample_parameters()).unwrap().replace("M01001 37", "M01001 38").replace("C001", "C0F1");
    let candidate_file = h.root.join("NEU.par");
    std::fs::write(&candidate_file, edited).unwrap();

    h.act(Action::Navigate(Page::Parameters));
    h.act(Action::ParamSelectFile(candidate_file.clone()));
    let gate = h.app.parameter_gate();
    assert!(!gate.allowed);
    assert!(gate.reasons.iter().any(|r| r.contains("Profil erlaubt kein Schreiben")));

    h.act(Action::ImportProfile(profile_file));
    h.act(Action::SelectProfile("test_schreiben".into()));
    h.act(Action::ParamRead);
    h.wait_idle();
    assert!(h.app.param.backup.is_some(), "{:?}", h.app.param.status);
    assert!(h.app.param.diff.is_some(), "diff against the backup is shown");
    assert!(h.app.parameter_gate().reasons.iter().any(|r| r.contains("nicht freigeschaltet")));

    h.act(Action::ParamRequestUnlock);
    assert_eq!(h.app.dialog, Some(Dialog::ParamUnlock));
    h.frame();
    h.act(Action::ParamConfirmUnlock);
    assert!(h.app.parameter_gate().allowed, "{:?}", h.app.parameter_gate().reasons);

    // wrong phrase: nothing is written
    h.act(Action::ParamRequestWrite);
    h.app.dialog = Some(Dialog::ParamWrite { phrase: "parameter schreiben".into(), ack: true });
    h.frame();
    h.act(Action::ParamConfirmWrite);
    assert!(!h.app.busy() && h.app.param.last_written.is_none());

    h.act(Action::ParamRequestWrite);
    h.app.dialog = Some(Dialog::ParamWrite { phrase: WRITE_CONFIRMATION_PHRASE.into(), ack: true });
    h.act(Action::ParamConfirmWrite);
    assert!(h.app.busy());
    h.wait_idle();
    assert!(h.app.param.backup.is_none(), "backup invalid after writing");
    assert!(h.app.param.last_written.is_some());

    // verification read (the simulator always returns the sample, so a deviation is reported)
    h.act(Action::ParamRead);
    h.wait_idle();
    let (identical, text) = h.app.param.verify.clone().unwrap();
    assert!(!identical && text.contains("ABWEICHUNG"), "{text}");
}

#[test]
fn real_mode_needs_confirmation_and_target() {
    let mut h = Harness::new("real");
    h.act(Action::LoadSample);
    h.act(Action::RequestRealMode);
    assert_eq!(h.app.dialog, Some(Dialog::RealMode));
    h.frame();
    h.act(Action::ConfirmRealMode);
    assert!(!h.app.simulation());
    assert!(h.app.send_gate().reasons.iter().any(|r| r.contains("Kein COM-Port")));
    h.act(Action::RequestSend { drip_feed: false });
    assert!(h.app.dialog.is_none(), "blocked without a port");
    h.frame();
    // the setting is persisted
    let reloaded = DeckelApp::new(egui::Context::default(), AppPaths::from_root(&h.root, None));
    assert!(!reloaded.simulation());
    h.act(Action::SetSimulation);
    assert!(h.app.simulation());
}

#[test]
fn profile_editor_never_overwrites_builtin() {
    use deckellink_core::model::VerificationStatus;
    let mut h = Harness::new("editor");
    let builtin_id = h.app.profile.id.clone();
    h.act(Action::Navigate(Page::Settings));
    h.act(Action::EditProfile);
    let mut draft = h.app.extra.draft.clone().unwrap();
    draft.serial.baud_rate = 9600;
    draft.verification_status = VerificationStatus::Verified;
    h.app.extra.draft = Some(draft);
    h.frame();
    h.act(Action::SaveDraft);
    assert_ne!(h.app.profile.id, builtin_id, "a user copy is active");
    assert_eq!(h.app.profile.serial.baud_rate, 9600);
    assert_eq!(h.app.profile.verification_status, VerificationStatus::Unverified, "changed profile is unverified");
    assert_eq!(h.app.store.get(&builtin_id).unwrap().serial.baud_rate, 4800);
    assert!(h.app.store.path_for(&h.app.profile.id).unwrap().is_file());
    h.act(Action::RequestDeleteProfile);
    assert_eq!(h.app.dialog, Some(Dialog::DeleteProfile));
    h.frame();
    h.act(Action::ConfirmDeleteProfile);
    assert_eq!(h.app.profile.id, builtin_id);
    h.act(Action::RequestDeleteProfile);
    assert!(h.app.dialog.is_none(), "built-in profiles cannot be deleted");
}

#[test]
fn diagnostics_loopback_format_and_export() {
    let mut h = Harness::new("diag");
    h.act(Action::Navigate(Page::Diagnostics));
    h.act(Action::StartLoopback);
    h.wait_idle();
    let result = h.app.extra.loopback_result.clone().unwrap();
    assert_eq!(result.verdict, deckellink_core::diagnostics::LoopbackVerdict::Passed, "{}", result.summary());
    h.app.extra.format_duration_s = 5;
    h.act(Action::ClearTerminal);
    h.act(Action::StartFormatDetect);
    h.wait_idle();
    let (_, guess) = h.app.extra.format_result.clone().unwrap();
    assert_eq!(guess.frame(), "7E1");
    assert!(
        h.app.traffic.iter().all(|t| t.direction == deckellink_core::transfer::Direction::Rx),
        "format detection never sends"
    );
    h.act(Action::ExportDiagnostics);
    let zip = h.app.extra.last_export.clone().unwrap();
    assert!(zip.is_file() && zip.extension().unwrap() == "zip");
    h.frame();
}

#[test]
fn replay_and_dropped_files() {
    let mut h = Harness::new("replay");
    std::fs::create_dir_all(&h.root).unwrap();
    let recording = h.root.join("aufnahme.raw");
    std::fs::write(&recording, b"%AUFNAHME\r\nN10 G0 X1\r\nM30\r\n").unwrap();
    h.app.config.simulation_scenario = deckellink_core::scenarios::REPLAY_SCENARIO_ID.into();
    h.app.config.simulation_replay_file = recording.to_string_lossy().into_owned();
    h.act(Action::Navigate(Page::Settings));
    h.act(Action::StartReceive);
    h.wait_idle();
    let stored = h.app.receive_summary.clone().unwrap().stored_path.unwrap();
    assert_eq!(std::fs::read(stored).unwrap(), b"%AUFNAHME\r\nN10 G0 X1\r\nM30\r\n");
    // a dropped file is only loaded, never sent
    h.act(Action::DroppedFile(recording.clone()));
    h.frame();
    assert_eq!(h.app.page, Page::Transfer);
    assert_eq!(h.app.program.as_ref().unwrap().path, recording);
    assert!(!h.app.busy() && h.app.dialog.is_none());
}

#[test]
fn commissioning_marks_verified_copy() {
    use deckellink_core::commissioning::{StepStatus, STEPS};
    use deckellink_core::model::VerificationStatus;
    let mut h = Harness::new("commissioning");
    h.act(Action::Navigate(Page::Commissioning));
    h.act(Action::Navigate(Page::Help));
    h.act(Action::Navigate(Page::Commissioning));
    for step in STEPS.iter().filter(|s| s.required) {
        h.app.push(Action::CommSetStatus(step.id.to_string(), StepStatus::Done));
    }
    h.frame();
    h.act(Action::CommRequestVerify);
    assert!(h.app.dialog.is_none(), "tester missing");
    h.app.extra.commissioning.as_mut().unwrap().tester = "Prüfer".into();
    h.act(Action::CommSave);
    h.act(Action::CommExport);
    h.act(Action::CommRequestVerify);
    assert_eq!(h.app.dialog, Some(Dialog::MarkVerified));
    h.frame();
    let builtin_id = h.app.profile.id.clone();
    h.act(Action::CommConfirmVerify);
    assert_eq!(h.app.profile.verification_status, VerificationStatus::Verified);
    assert_ne!(h.app.profile.id, builtin_id, "built-in profile stays unverified");
    assert_eq!(h.app.store.get(&builtin_id).unwrap().verification_status, VerificationStatus::Unverified);
    assert!(h.app.profile.description.contains("Verifiziert am"));
    // reloaded record belongs to the verified copy
    assert_eq!(h.app.extra.commissioning.as_ref().unwrap().profile_id, h.app.profile.id);
}

#[test]
fn softkeys_via_function_keys() {
    let mut h = Harness::new("softkeys");
    // F5 SENDEN is locked without a file: nothing happens
    h.press(egui::Key::F5);
    assert!(h.app.dialog.is_none());
    let keys = ui::softkeys(&h.app);
    assert!(!keys[4].as_ref().unwrap().enabled, "send key locked without a file");
    // F3 loads the sample in simulation, F5 opens the confirmation (never sends directly)
    h.press(egui::Key::F3);
    assert!(h.app.program.is_some());
    h.press(egui::Key::F5);
    assert!(matches!(h.app.dialog, Some(Dialog::ConfirmSend { drip_feed: false, .. })));
    assert!(!h.app.busy(), "F keys never start a transfer without confirmation");
    // softkeys are ignored while a dialog is open
    h.press(egui::Key::F1);
    h.act(Action::CloseConfirm);
    // Ctrl+number selects the page like the numbered keys
    let input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Num6,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
        ..egui::RawInput::default()
    };
    let app = &mut h.app;
    let mut output = h.ctx.run_ui(input, |u| ui::show(app, u));
    output.textures_delta.clear();
    h.app.update();
    assert_eq!(h.app.page, Page::Diagnostics);
    // Diagnostics F2 runs the loopback test in simulation
    h.press(egui::Key::F2);
    h.wait_idle();
    assert!(h.app.extra.loopback_result.is_some());
    // every page has eight softkey positions and draws without errors
    for page in Page::ALL {
        h.act(Action::Navigate(page));
        assert_eq!(ui::softkeys(&h.app).len(), 8);
    }
}

#[test]
fn simulator_page_modes_softkeys_and_playback() {
    let mut h = Harness::new("simulator");
    h.act(Action::Navigate(Page::Simulator));
    let start = Instant::now();
    while h.app.sim.playback.is_none() {
        assert!(start.elapsed() < Duration::from_secs(30), "simulation did not finish");
        std::thread::sleep(Duration::from_millis(10));
        h.frame();
    }
    assert!(h.app.sim.result().is_some_and(|r| r.end_reason.starts_with("M30")));
    // F1 opens 21 TESTLAUF, F1 again 211, F7 2117 START/STOP starts the test run
    h.press(egui::Key::F1);
    h.press(egui::Key::F1);
    assert_eq!(h.app.sim.mode_path(), "2 AUTOMATIK › 21 TESTLAUF › 211 TESTLAUF");
    h.press(egui::Key::F7);
    assert!(h.app.sim.running);
    h.act(Action::Sim(deckellink_gui::sim_app::SimAction::ToEnd));
    assert!(!h.app.sim.running);
    // every view, every tab and every mode draws
    for mode in deckellink_gui::sim_app::ViewMode::ALL {
        h.act(Action::Sim(deckellink_gui::sim_app::SimAction::View(mode)));
    }
    for tab in deckellink_gui::sim_app::SimTab::ALL {
        h.act(Action::Sim(deckellink_gui::sim_app::SimAction::Tab(tab)));
    }
    for mode in [1, 3, 2] {
        h.act(Action::Sim(deckellink_gui::sim_app::SimAction::SetMode(mode)));
    }
    // the simulator never touches the transfer state
    assert_eq!(h.app.send_state, SendState::Idle);
    assert!(!h.app.busy());
}
