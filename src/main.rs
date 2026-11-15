#![windows_subsystem = "windows"]
mod engine;
mod model;
mod ui;
mod winio;

use engine::{Live, Shared};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

fn main() -> eframe::Result {
    // profiles/settings live next to the exe, wherever it is started from
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let _ = std::env::set_current_dir(dir);
    }
    let st = model::Settings::load();
    let s = Arc::new(Shared {
        profile: Mutex::default(),
        settings: Mutex::new(Arc::new(st.clone())),
        on: AtomicBool::new(false),
        live: Mutex::new(Live { status: "Verbinde…".into(), ..Default::default() }),
        set_cmd: Mutex::default(),
        events: Mutex::default(),
    });
    let app = ui::App::new(s.clone(), st);
    engine::spawn(s);
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([820.0, 860.0]).with_min_inner_size([420.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native("padmimic", opts, Box::new(|cc| {
        ui::style(&cc.egui_ctx);
        Ok(Box::new(app))
    }))
}
