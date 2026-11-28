//! Command line of the desktop program and the headless self test (`--smoke-test`).
//!
//! Same options as the Python version: `--data-dir`, `--simulate`, `--smoke-test`,
//! `--smoke-report`, `--version` and an optional NC file that is loaded and checked on start
//! (nothing is sent automatically). The self test drives the real application logic without
//! a window and always in simulation mode; it never opens a port.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use deckellink_core::appdata::AppPaths;
use deckellink_core::transfer::{ReceiveState, SendState};
use serde_json::{json, Value};

use crate::app::{Action, DeckelApp, Page};

pub const USAGE: &str = "deckellink [--data-dir ORDNER] [--simulate] [--smoke-test [--smoke-report DATEI]] [NC-DATEI]";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub data_dir: Option<PathBuf>,
    pub simulate: bool,
    pub smoke_test: bool,
    pub smoke_report: Option<PathBuf>,
    pub version: bool,
    pub file: Option<PathBuf>,
}

/// Parse the command line (without the program name).
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--data-dir" => options.data_dir = Some(iter.next().ok_or("--data-dir ohne Ordner")?.into()),
            "--smoke-report" => options.smoke_report = Some(iter.next().ok_or("--smoke-report ohne Datei")?.into()),
            "--simulate" => options.simulate = true,
            "--smoke-test" => options.smoke_test = true,
            "--version" => options.version = true,
            other if other.starts_with("--") => return Err(format!("Unbekannte Option {other}\n{USAGE}")),
            other => {
                if options.file.is_some() {
                    return Err(format!("Nur eine Datei möglich\n{USAGE}"));
                }
                options.file = Some(other.into());
            }
        }
    }
    Ok(options)
}

/// One frame without window (as in the headless tests).
fn frame(ctx: &egui::Context, app: &mut DeckelApp) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 900.0))),
        ..egui::RawInput::default()
    };
    let mut output = ctx.run_ui(input, |ui| crate::ui::show(app, ui));
    output.textures_delta.clear();
    app.update();
}

fn act(ctx: &egui::Context, app: &mut DeckelApp, action: Action) {
    app.push(action);
    frame(ctx, app);
}

fn wait(ctx: &egui::Context, app: &mut DeckelApp, done: impl Fn(&DeckelApp) -> bool) -> bool {
    let start = Instant::now();
    while !done(app) {
        if start.elapsed() > Duration::from_secs(120) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
        frame(ctx, app);
    }
    true
}

/// Run the self test; returns the JSON report (`"ok": true` when every step passed).
pub fn run_smoke_test(paths: AppPaths) -> Value {
    let mut steps: Vec<Value> = Vec::new();
    let mut step = |name: &str, ok: bool, detail: Value| {
        steps.push(json!({"step": name, "ok": ok, "detail": detail}));
    };
    let ctx = egui::Context::default();
    let mut app = DeckelApp::new(ctx.clone(), paths);
    // never the real port: simulation for this run only (not saved)
    app.config.simulation_mode = true;
    app.config.simulation_scenario = "send_ok".into();
    frame(&ctx, &mut app);
    step("profiles_loaded", app.store.profiles().len() >= 2, json!({"active": app.profile.id}));

    act(&ctx, &mut app, Action::LoadSample);
    let program = app.program.clone();
    let checked = program.as_ref().is_some_and(|p| !p.analysis.has_errors());
    step("validated", checked, json!({"findings": program.as_ref().map_or(0, |p| p.analysis.findings.len())}));

    act(&ctx, &mut app, Action::RequestSend { drip_feed: false });
    let confirmation = app.dialog.is_some() && !app.busy();
    act(&ctx, &mut app, Action::ConfirmSend { suppress_safety_notice: false });
    let finished = wait(&ctx, &mut app, |a| !a.busy());
    step(
        "simulated_send",
        confirmation && finished && app.send_state == SendState::Completed,
        json!({"bytes": app.last_send.as_ref().map_or(0, |r| r.bytes_sent)}),
    );
    let unchanged = program.as_ref().is_some_and(|p| std::fs::read(&p.path).is_ok_and(|d| d == p.original));
    step("original_unchanged", unchanged, Value::Null);

    app.config.simulation_scenario = "receive_ok".into();
    act(&ctx, &mut app, Action::StartReceive);
    let finished = wait(&ctx, &mut app, |a| !a.busy());
    let stored = app.receive_summary.as_ref().and_then(|s| s.stored_path.clone());
    step(
        "simulated_receive",
        finished && app.receive_state == ReceiveState::Completed && stored.as_ref().is_some_and(|p| p.is_file()),
        json!({"file": stored.map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))}),
    );

    act(&ctx, &mut app, Action::Navigate(Page::Simulator));
    let simulated = wait(&ctx, &mut app, |a| a.sim.playback.is_some() || !a.sim.busy);
    let end = app.sim.result().map(|r| r.end_reason.clone()).unwrap_or_default();
    step("dialog11_simulator", simulated && end.starts_with("M30"), json!({"end": end}));

    act(&ctx, &mut app, Action::Navigate(Page::Library));
    let library_ok = app.library.ensure().is_ok() && app.library.root.is_dir();
    step("library", library_ok, json!({"entries": app.lib.entries.len()}));

    for page in Page::ALL {
        act(&ctx, &mut app, Action::Navigate(page));
    }
    step("pages_drawn", true, json!({"pages": Page::ALL.len()}));

    let ok = steps.iter().all(|s| s.get("ok").and_then(Value::as_bool).unwrap_or(false));
    json!({"ok": ok, "version": env!("CARGO_PKG_VERSION"), "steps": steps})
}

/// Write the report (new file or replace an earlier report of the self test).
pub fn write_report(path: &Path, report: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    deckellink_core::storage::atomic_write(path, text.as_bytes(), true).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_like_python() {
        let args: Vec<String> = ["--data-dir", "X", "--simulate", "prog.nc"].iter().map(ToString::to_string).collect();
        let options = parse_args(&args).unwrap();
        assert_eq!(options.data_dir, Some(PathBuf::from("X")));
        assert!(options.simulate && !options.smoke_test);
        assert_eq!(options.file, Some(PathBuf::from("prog.nc")));
        assert!(parse_args(&["--port".to_string()]).is_err());
        assert!(parse_args(&["a".to_string(), "b".to_string()]).is_err());
    }

    #[test]
    fn smoke_test_passes_in_a_fresh_data_folder() {
        let root = std::env::temp_dir().join(format!("deckellink-smoke-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let report = run_smoke_test(AppPaths::from_root(&root, None));
        assert_eq!(report["ok"], json!(true), "{report:#}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
