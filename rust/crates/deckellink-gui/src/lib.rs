//! DeckelLink desktop application (egui).
//!
//! Safety rules as in the Python version: nothing is sent automatically, every transfer is
//! confirmed immediately before it starts, simulation and real operation are visually
//! distinct, port I/O runs only in worker threads, originals are never modified.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing))]

pub mod app;
pub mod applog;
pub mod logic;
pub mod ui;
pub mod worker;

use deckellink_core::appdata::AppPaths;

/// eframe adapter around [`app::DeckelApp`].
#[derive(Debug)]
pub struct EframeApp {
    pub app: app::DeckelApp,
}

impl EframeApp {
    pub fn new(ctx: &egui::Context, paths: AppPaths) -> Self {
        Self { app: app::DeckelApp::new(ctx.clone(), paths) }
    }
}

impl eframe::App for EframeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::show(&mut self.app, ui);
        self.app.update();
    }
}
