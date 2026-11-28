//! DeckelLink desktop application.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

use deckellink_core::appdata::AppPaths;
use deckellink_gui::smoke::{parse_args, run_smoke_test, write_report};
use deckellink_gui::EframeApp;

// stdout is the interface of --version and --smoke-test
#[allow(clippy::print_stdout)]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse_args(&args) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    if options.version {
        println!("DeckelLink {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let paths = AppPaths::resolve(options.data_dir.as_deref());
    if options.smoke_test {
        let report = run_smoke_test(paths);
        println!("{report}");
        if let Some(path) = &options.smoke_report {
            if let Err(err) = write_report(path, &report) {
                eprintln!("Bericht nicht geschrieben: {err}");
                return ExitCode::from(1);
            }
        }
        let ok = report.get("ok").and_then(serde_json::Value::as_bool).unwrap_or(false);
        return if ok { ExitCode::SUCCESS } else { ExitCode::from(1) };
    }
    let viewport = egui::ViewportBuilder::default()
        .with_title("DeckelLink - DECKEL FP2A / DIALOG 11")
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size([900.0, 600.0]);
    let native = eframe::NativeOptions { viewport, ..eframe::NativeOptions::default() };
    let result = eframe::run_native(
        "DeckelLink",
        native,
        Box::new(move |cc| {
            let mut app = EframeApp::new(&cc.egui_ctx, paths);
            if options.simulate {
                // this session only, the saved operating mode stays unchanged
                app.app.config.simulation_mode = true;
            }
            if let Some(file) = &options.file {
                // load and check only - nothing is sent automatically
                app.app.load_file(file);
            }
            Ok(Box::new(app))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("DeckelLink konnte nicht gestartet werden: {err}");
            ExitCode::from(1)
        }
    }
}
