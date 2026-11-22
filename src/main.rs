#![windows_subsystem = "windows"]
mod engine;
mod i18n;
mod model;
mod setup;
mod ui;
mod update;
mod winio;

use engine::{Live, Shared};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

fn main() -> eframe::Result {
    // padmimic-setup.exe (same exe, other name) installs, --uninstall removes
    if let Some(m) = setup::mode() { return setup::run(m) }
    // profiles/settings live next to the exe, wherever it is started from
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let _ = std::env::set_current_dir(dir);
    }
    let st = model::Settings::load();
    let s = Arc::new(Shared {
        profile: Mutex::default(),
        settings: Mutex::new(Arc::new(st.clone())),
        on: AtomicBool::new(false),
        live: Mutex::new(Live { status: "Connecting…".into(), ..Default::default() }),
        set_cmd: Mutex::default(),
        events: Mutex::default(),
    });
    let app = ui::App::new(s.clone(), st);
    engine::spawn(s);
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([820.0, 860.0]).with_min_inner_size([420.0, 520.0])
            .with_decorations(false), // own retro title bar in ui::header
        ..Default::default()
    };
    eframe::run_native("padmimic", opts, Box::new(|cc| {
        ui::style(&cc.egui_ctx);
        Ok(Box::new(app))
    }))
}
