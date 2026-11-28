#![windows_subsystem = "windows"]
mod brand;
mod devices;
mod docking;
mod engine;
mod i18n;
mod icons;
mod model;
mod overlay;
mod pad;
mod setup;
mod skin;
mod sony_input;
mod tray;
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
    let mut st = model::Settings::load();
    let variant = st.tray_variant;
    let tray = tray::Tray::start(variant);
    st.tray_variant = tray::next_variant(variant);
    st.save();
    let s = Arc::new(Shared {
        profile: Mutex::default(),
        settings: Mutex::new(Arc::new(st.clone())),
        on: AtomicBool::new(false),
        live: Mutex::new(Live { status: "Connecting…".into(), ..Default::default() }),
        set_cmd: Mutex::default(),
        events: Mutex::default(),
    });
    let zoom = ui::SIZES[st.size as usize % ui::SIZES.len()];
    let app = ui::App::new(s.clone(), st);
    engine::spawn(s);
    let mut opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1180.0 * zoom, 880.0 * zoom]).with_min_inner_size([520.0, 400.0])
            .with_decorations(false).with_transparent(true).with_icon(tray::window_icon(variant)), // own title bar in ui::top_bar
        ..Default::default()
    };
    // Two visible viewports can otherwise wait on separate GL swaps on the same thread.
    // Padmimic paces the root UI once per frame and the desktop compositor presents it.
    opts.glow_options.vsync = false;
    let result = eframe::run_native("padmimic", opts, Box::new(|cc| {
        ui::style(&cc.egui_ctx);
        cc.egui_ctx.set_zoom_factor(zoom);
        Ok(Box::new(app))
    }));
    drop(tray);
    result
}
