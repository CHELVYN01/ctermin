// Sembunyikan jendela console pada build release; mode debug tetap memakainya untuk log.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod layout;
mod terminal;
mod ui;

use anyhow::anyhow;
use app::CterminApp;
use eframe::egui;

fn main() -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ctermin")
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size([320.0, 200.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ctermin",
        options,
        Box::new(|cc| Ok(Box::new(CterminApp::new(&cc.egui_ctx)))),
    )
    .map_err(|err| anyhow!("Gagal membuka jendela ctermin: {err}"))
}
