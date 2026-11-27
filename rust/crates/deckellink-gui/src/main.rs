//! DeckelLink desktop application.
#![cfg_attr(windows, windows_subsystem = "windows")]

use deckellink_core::appdata::AppPaths;
use deckellink_gui::EframeApp;

fn main() -> eframe::Result {
    let paths = AppPaths::resolve(None);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DeckelLink - DECKEL FP2A / DIALOG 11")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([900.0, 600.0]),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native("DeckelLink", options, Box::new(move |cc| Ok(Box::new(EframeApp::new(&cc.egui_ctx, paths)))))
}
