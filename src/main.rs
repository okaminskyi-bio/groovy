mod app;
mod collab;
mod diff_engine;
mod docx;
mod git_ops;
mod highlight;
mod lint;
mod render;

use app::OlehGroovyEditorApp;
use eframe::{NativeOptions, egui};

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Oleh Groovy Editor")
            .with_inner_size([1460.0, 900.0])
            .with_min_inner_size([980.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Oleh Groovy Editor",
        options,
        Box::new(|cc| Ok(Box::new(OlehGroovyEditorApp::new(cc)))),
    )
}
