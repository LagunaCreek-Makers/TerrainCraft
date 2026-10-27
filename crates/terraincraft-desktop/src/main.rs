//! TerrainCraft v0.9: native Rust egui desktop inspection and manufacturing export.
//! Deliberately no machine control or G-code generation.
mod app;
mod assembly;
mod canvas;
mod engraving;
mod export;
mod gpu_view;
mod history;
mod map;
mod project;
mod studio;

use eframe::egui;
use studio::Studio;

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200., 800.])
            .with_title("TerrainCraft"),
        ..Default::default()
    };
    eframe::run_native(
        "TerrainCraft",
        opts,
        Box::new(|_cc| Ok(Box::new(Studio::default()))),
    )
}
