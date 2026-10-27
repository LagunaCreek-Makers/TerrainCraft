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

/// Catalogs and other data are read from `assets/` relative to the working directory.
/// When the app is started from a file manager (macOS Finder starts it in `/`), use the
/// `assets/` folder shipped next to the executable instead.
fn find_assets() {
    if std::path::Path::new("assets").is_dir() {
        return;
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    if let Some(dir) = exe_dir.filter(|d| d.join("assets").is_dir()) {
        let _ = std::env::set_current_dir(dir);
    }
}

fn main() -> eframe::Result<()> {
    find_assets();
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
