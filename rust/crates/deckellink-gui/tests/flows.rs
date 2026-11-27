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
