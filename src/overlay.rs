//! Independent transparent live-controller viewport. No main-window panels or backlight.
use crate::{engine::Shared, model::OverlaySettings, skin::Skin};
use eframe::egui::{self, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, Vec2, ViewportCommand, ViewportId, WindowLevel, vec2};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TITLE: &str = "padmimic · Controller Overlay";

#[derive(Default)]
struct Events {
    closed: bool,
    geometry: Option<Rect>,
    geometry_changed: Option<Instant>,
    requested_geometry: Option<([f32; 2], Option<[f32; 2]>)>,
    builder_geometry: Option<([f32; 2], Option<[f32; 2]>)>,
    last_geometry: Option<Rect>,
    region_key: Option<(usize, i32, i32, u32)>,
    mask: Option<(usize, HitMask)>,
}

#[derive(Default)]
pub struct Window {
    events: Arc<Mutex<Events>>,
    input_watch: Mutex<Option<Arc<()>>>,
}

fn viewport_id() -> ViewportId { ViewportId::from_hash_of("controller-overlay") }

impl Window {
    /// A stationary overlay should not compete with the main window for GPU presents.
    fn watch_input(&self, ctx: &egui::Context, shared: &Arc<Shared>) {
        let mut watch = self.input_watch.lock().unwrap();
        if watch.is_some() { return; }
        let lease = Arc::new(());
        let alive = Arc::downgrade(&lease);
        *watch = Some(lease);
        let ctx = ctx.clone();
        let shared = shared.clone();
        std::thread::spawn(move || {
            let mut last = None;
            while alive.upgrade().is_some() {
                let phys = shared.live.lock().unwrap().phys;
                let settings = shared.settings.lock().unwrap().clone();
                let changed = last.as_ref().is_none_or(|(previous, previous_settings)| {
                    *previous != phys || !Arc::ptr_eq(previous_settings, &settings)
                });
                if changed {
                    ctx.request_repaint_of(viewport_id());
                    last = Some((phys, settings));
                }
                std::thread::sleep(crate::ui::FRAME_60);
            }
        });
    }

    /// Apply native window events before drawing settings, so UI edits take precedence.
    pub fn poll(&self, settings: &mut OverlaySettings) {
        let mut events = self.events.lock().unwrap();
        if std::mem::take(&mut events.closed) { settings.enabled = false; }
        // Let Windows own the drag/resize loop. Save once the geometry settles.
        if settings.enabled && events.geometry_changed.is_some_and(|at| at.elapsed() < Duration::from_millis(180)) {
            return;
        }
        if let Some(rect) = events.geometry.take() {
            settings.size = [rect.width(), rect.height()];
            settings.position = Some([rect.left(), rect.top()]);
            events.requested_geometry = Some((settings.size, settings.position));
        }
    }

    pub fn show(&self, ctx: &egui::Context, settings: &OverlaySettings, shared: Arc<Shared>, skin: Option<Skin>) {
        if !settings.enabled {
            self.input_watch.lock().unwrap().take();
            let mut events = self.events.lock().unwrap();
            events.last_geometry = None;
            events.region_key = None;
            events.builder_geometry = None;
            events.requested_geometry = None;
            return;
        }
        self.watch_input(ctx, &shared);
        let geometry = (settings.size, settings.position);
        let (size, position) = {
            let mut state = self.events.lock().unwrap();
            if state.requested_geometry != Some(geometry) || state.builder_geometry.is_none() {
                state.requested_geometry = Some(geometry);
                state.builder_geometry = Some(geometry);
                state.geometry = None;
            }
            state.builder_geometry.unwrap()
        };
        let settings = settings.clone();
        let events = self.events.clone();
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_transparent(true)
            .with_has_shadow(false)
            .with_active(false)
            .with_taskbar(false)
            .with_resizable(!settings.locked)
            .with_mouse_passthrough(settings.locked)
            .with_window_level(if settings.always_on_top { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal })
            .with_inner_size(size)
            .with_min_inner_size([160., 140.]);
        if let Some([x, y]) = position { builder = builder.with_position(Pos2::new(x, y)); }
        ctx.show_viewport_deferred(viewport_id(), builder, move |vui, _| {
            let ctx = vui.ctx().clone();
            // Read the engine on each child frame, including while the main app is minimized.
            let phys = shared.live.lock().unwrap().phys;
            let st = shared.settings.lock().unwrap().clone();
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(vui, |ui| {
                let rect = ui.max_rect();
                let picture = rect.shrink(8.);
                // Test mouse hits against the controller without clipping the rendered window.
                // Recompute the hit mask only when the model, size or DPI changes.
                let mi = crate::pad::selected_index(&st.pad_skin, st.pad_model);
                let ppp = ctx.pixels_per_point();
                let width = (rect.width() * ppp).round().max(1.) as i32;
                let height = (rect.height() * ppp).round().max(1.) as i32;
                let key = (if skin.is_some() { usize::MAX } else { mi }, width, height, (ppp * 1000.).round() as u32);
                {
                    let mut state = events.lock().unwrap();
                    if state.region_key != Some(key) {
                        if skin.is_some() {
                            if crate::winio::overlay_hit_test(TITLE, None) { state.region_key = Some(key); }
                        } else {
                            if state.mask.as_ref().is_none_or(|(id, _)| *id != mi) {
                                state.mask = Some((mi, HitMask::new(&crate::pad::overlay_pixels(mi))));
                            }
                            let image = crate::pad::overlay_image_rect(picture);
                            let image = Rect::from_min_max((image.min - rect.min).to_pos2() * ppp, (image.max - rect.min).to_pos2() * ppp);
                            let rectangles = state.mask.as_ref().unwrap().1.rectangles(image, width, height, (8. * ppp).ceil() as i32);
                            if crate::winio::overlay_hit_test(TITLE, Some(rectangles)) { state.region_key = Some(key); }
                        }
                    }
                }
                if let Some(skin) = &skin {
                    skin.paint(ui.painter(), picture, &phys);
                } else {
                    crate::pad::paint_overlay(ui.painter(), picture, crate::pad::selected_index(&st.pad_skin, st.pad_model), &phys);
                }
                if !settings.locked {
                    let resizing = resize_edges(&ctx, rect);
                    let drag = ui.interact(rect, ui.id().with("move-overlay"), Sense::drag());
                    if !resizing {
                        drag.clone().on_hover_cursor(CursorIcon::Grab);
                        if drag.drag_started_by(egui::PointerButton::Primary) {
                            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                        }
                    }
                    drag.context_menu(|ui| {
                        if ui.button(crate::i18n::tr("Close overlay")).clicked() {
                            events.lock().unwrap().closed = true;
                            ctx.request_repaint_of(ViewportId::ROOT);
                            ui.close();
                        }
                    });
                }
            });
            let (closed, geometry) = ctx.input(|i| (i.viewport().close_requested(), i.viewport().inner_rect));
            let changed = {
                let mut events = events.lock().unwrap();
                if closed { events.closed = true; }
                let changed = geometry.is_some() && geometry != events.last_geometry;
                if changed {
                    events.geometry = geometry;
                    events.last_geometry = geometry;
                    events.geometry_changed = Some(Instant::now());
                }
                closed || changed
            };
            if closed { ctx.request_repaint_of(ViewportId::ROOT); }
            else if changed { ctx.request_repaint_after_for(Duration::from_millis(200), ViewportId::ROOT); }

        });
    }
}

/// Dilated artwork alpha, expressed as horizontal runs. This preserves the controller's
/// outline feedback while passing clicks through holes and the surrounding empty canvas.
struct HitMask {
    width: usize,
    rows: Vec<Vec<(i32, i32)>>,
}
impl HitMask {
    fn new(image: &egui::ColorImage) -> Self {
        let [width, height] = image.size;
        let raw: Vec<Vec<(i32, i32)>> = (0..height).map(|y| {
            let mut runs = vec![];
            let mut x = 0;
            while x < width {
                if image.pixels[y * width + x].a() <= 12 { x += 1; continue; }
                let left = x;
                while x < width && image.pixels[y * width + x].a() > 12 { x += 1; }
                runs.push(((left as i32 - 4).max(0), (x as i32 + 4).min(width as i32)));
            }
            runs
        }).collect();
        let rows = (0..height).map(|y| {
            merge_runs(raw[y.saturating_sub(4)..(y + 5).min(height)].iter().flatten().copied().collect())
        }).collect();
        Self { width, rows }
    }
    fn rectangles(&self, image: Rect, width: i32, height: i32, border: i32) -> Vec<[i32; 4]> {
        let mut rects = vec![];
        for y in 0..height {
            let mut runs = vec![];
            if y < border || y >= height - border {
                runs.push((0, width));
            } else {
                // Thin invisible edge strips retain native resize handles.
                runs.extend([(0, border), (width - border, width)]);
                let sy = ((y as f32 - image.top()) / image.height() * self.rows.len() as f32).floor() as i32;
                if sy >= 0 && (sy as usize) < self.rows.len() {
                    for &(l, r) in &self.rows[sy as usize] {
                        let left = (image.left() + l as f32 / self.width as f32 * image.width()).floor() as i32;
                        let right = (image.left() + r as f32 / self.width as f32 * image.width()).ceil() as i32;
                        runs.push((left.clamp(0, width), right.clamp(0, width)));
                    }
                }
            }
            for (left, right) in merge_runs(runs) {
                if right > left { rects.push([left, y, right, y + 1]); }
            }
        }
        rects
    }
}
fn merge_runs(mut runs: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    runs.sort_unstable();
    let mut merged: Vec<(i32, i32)> = vec![];
    for (left, right) in runs {
        if let Some(last) = merged.last_mut().filter(|last| left <= last.1) { last.1 = last.1.max(right); }
        else { merged.push((left, right)); }
    }
    merged
}

/// Native resizing works on every edge. The artwork always keeps its own aspect ratio.
fn resize_edges(ctx: &egui::Context, rect: Rect) -> bool {
    use egui::ResizeDirection as D;
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return false };
    let Some((direction, cursor)) = resize_direction(rect, pos) else { return false };
    ctx.set_cursor_icon(cursor);
    // Only show a small corner hint while manipulating the window; no permanent chrome.
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("overlay-resize-hint")));
    let end = rect.right_bottom() - vec2(3., 3.);
    if matches!(direction, D::SouthEast) {
        p.line_segment([end - vec2(10., 0.), end - vec2(0., 10.)], Stroke::new(1., Color32::from_white_alpha(140)));
    }
    if ctx.input(|i| i.pointer.primary_pressed()) { ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction)); }
    true
}

fn resize_direction(rect: Rect, pos: Pos2) -> Option<(egui::ResizeDirection, CursorIcon)> {
    use egui::{CursorIcon as C, ResizeDirection as D};
    if !rect.contains(pos) { return None; }
    let (l, r, t, b) = (pos.x < rect.left() + 10., pos.x > rect.right() - 10., pos.y < rect.top() + 10., pos.y > rect.bottom() - 10.);
    match (l, r, t, b) {
        (true, _, true, _) => Some((D::NorthWest, C::ResizeNorthWest)),
        (_, true, true, _) => Some((D::NorthEast, C::ResizeNorthEast)),
        (true, _, _, true) => Some((D::SouthWest, C::ResizeSouthWest)),
        (_, true, _, true) => Some((D::SouthEast, C::ResizeSouthEast)),
        (true, ..) => Some((D::West, C::ResizeWest)),
        (_, true, ..) => Some((D::East, C::ResizeEast)),
        (_, _, true, _) => Some((D::North, C::ResizeNorth)),
        (.., true) => Some((D::South, C::ResizeSouth)),
        _ => None,
    }
}

pub fn default_size(width: f32) -> Vec2 { vec2(width, width * 500. / 576.) }

#[cfg(test)]
mod tests {
    use super::*;

    fn input_for_child(id: ViewportId) -> egui::RawInput {
        let mut input = egui::RawInput::default();
        input.viewport_id = id;
        input.viewports.insert(id, egui::ViewportInfo {
            inner_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(480., 417.))),
            ..Default::default()
        });
        input
    }

    #[test]
    fn transparent_child_reads_new_input_after_registration() {
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        crate::ui::set_fonts(&ctx);
        let shared = Arc::new(Shared {
            profile: Mutex::default(),
            settings: Mutex::new(Arc::new(crate::model::Settings::default())),
            on: std::sync::atomic::AtomicBool::new(false),
            live: Mutex::default(),
            set_cmd: Mutex::default(),
            events: Mutex::default(),
        });
        let window = Window::default();
        let config = OverlaySettings { enabled: true, ..Default::default() };
        let mut registration = ctx.run_ui(egui::RawInput::default(), |ui| {
            window.show(ui.ctx(), &config, shared.clone(), None);
        });
        let id = ViewportId::from_hash_of("controller-overlay");
        let viewport = &registration.viewport_output[&id];
        assert_eq!(viewport.builder.transparent, Some(true));
        assert_eq!(viewport.builder.decorations, Some(false));
        assert_eq!(viewport.builder.has_shadow, Some(false));
        assert_eq!(viewport.builder.window_level, Some(WindowLevel::AlwaysOnTop));
        let callback = viewport.viewport_ui_cb.clone().unwrap();
        let input = || {
            let mut input = egui::RawInput::default();
            input.viewport_id = id;
            input.viewports.insert(id, egui::ViewportInfo {
                inner_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(480., 417.))),
                ..Default::default()
            });
            input
        };
        let mut idle = ctx.run_ui(input(), |ui| callback(ui));
        {
            let mut live = shared.live.lock().unwrap();
            live.phys.buttons.raw = vigem_client::XButtons::A | vigem_client::XButtons::LB;
            live.phys.thumb_lx = 24000;
            live.phys.right_trigger = 200;
        }
        let mut pressed = ctx.run_ui(input(), |ui| callback(ui));
        assert!(pressed.shapes.len() > idle.shapes.len());
        // Interleave child repaints between a real egui press/release in the main UI.
        // Opening an overlay must never turn the root window into a modal background.
        let mut target = Rect::NOTHING;
        let mut root = ctx.run_ui(egui::RawInput::default(), |ui| {
            target = ui.button("Main-window control").rect;
            window.show(ui.ctx(), &config, shared.clone(), None);
        });
        let mut clicked = false;
        for down in [true, false] {
            let mut input = egui::RawInput::default();
            input.events = vec![egui::Event::PointerMoved(target.center()), egui::Event::PointerButton {
                pos: target.center(), button: egui::PointerButton::Primary, pressed: down, modifiers: egui::Modifiers::NONE,
            }];
            let mut frame = ctx.run_ui(input, |ui| {
                clicked |= ui.button("Main-window control").clicked();
                window.show(ui.ctx(), &config, shared.clone(), None);
            });
            frame.textures_delta.clear();
            if down { let mut child = ctx.run_ui(input_for_child(id), |ui| callback(ui)); child.textures_delta.clear(); }
        }
        assert!(clicked, "the main window must remain interactive with the overlay open");
        root.textures_delta.clear();
        registration.textures_delta.clear();
        idle.textures_delta.clear();
        pressed.textures_delta.clear();
    }

    #[test]
    fn transparent_canvas_passes_clicks_and_controller_and_resize_edges_remain_interactive() {
        let mut pixels = egui::ColorImage::filled([100, 100], Color32::TRANSPARENT);
        for y in 40..60 { for x in 40..60 { pixels.pixels[y * 100 + x] = Color32::WHITE; } }
        let mask = HitMask::new(&pixels);
        let rects = mask.rectangles(Rect::from_min_size(Pos2::ZERO, vec2(200., 200.)), 200, 200, 8);
        let hit = |x, y| rects.iter().any(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]);
        assert!(!hit(30, 30));
        assert!(!hit(100, 160));
        assert!(hit(100, 100));
        assert!(hit(1, 100));
        assert!(hit(199, 199));
        assert!(rects.windows(2).all(|pair| pair[0][1] < pair[1][1] || (pair[0][1] == pair[1][1] && pair[0][2] <= pair[1][0])));
    }

    #[test]
    fn corner_resize_does_not_start_window_drag() {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(480., 417.));
        assert!(resize_direction(rect, rect.center()).is_none());
        assert!(matches!(resize_direction(rect, rect.right_bottom() - vec2(1., 1.)), Some((egui::ResizeDirection::SouthEast, _))));
        assert!(matches!(resize_direction(rect, Pos2::new(1., 200.)), Some((egui::ResizeDirection::West, _))));
        assert!(resize_direction(rect, Pos2::new(-1., 100.)).is_none());
    }

    #[test]
    fn old_settings_keep_the_overlay_closed_and_geometry_round_trips() {
        let old: crate::model::Settings = serde_json::from_str(r#"{"theme":1,"pad_skin":"builtin:ps5-realistic"}"#).unwrap();
        assert!(!old.overlay.enabled);
        assert!(old.smart_docking);
        assert!(old.overlay.always_on_top);
        assert!(!old.overlay.locked);
        let window = Window::default();
        let mut cfg = old.overlay;
        cfg.enabled = true;
        window.events.lock().unwrap().geometry = Some(Rect::from_min_size(Pos2::new(120., 90.), default_size(600.)));
        window.events.lock().unwrap().closed = true;
        window.poll(&mut cfg);
        assert!(!cfg.enabled);
        assert_eq!(cfg.position, Some([120., 90.]));
        assert_eq!(cfg.size, [600., default_size(600.).y]);
        assert_eq!(serde_json::from_str::<OverlaySettings>(&serde_json::to_string(&cfg).unwrap()).unwrap(), cfg);
    }
}
