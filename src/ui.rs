//! AntiMicroX layout (profile bar, element grid, sets 1-8, Quick Set / Names / Pref / Reset, edit dialogs),
//! retro-modern look. Every text is English and goes through `tr` (see i18n.rs).

use crate::engine::{self, Event, Shared, eval_stick, pad_down};
use crate::i18n::{self, tr, trf};
use crate::update::{self, State as Upd};
use crate::model::*;
use crate::winio;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Theme, Vec2, vec2};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};
use vigem_client::{XButtons, XGamepad};

// ================================================================= look

/// Retro terminal palette, modern spacing: ice-blue cyan for the real controller and "on",
/// amber for what padmimic itself sends (virtual pad).
struct Pal { panel: Color32, card: Color32, line: Color32, idle: Color32, hover: Color32, text: Color32, weak: Color32, acc: Color32, amber: Color32 }

fn pal(dark: bool) -> Pal {
    let c = Color32::from_rgb;
    if dark {
        Pal { panel: c(10, 12, 15), card: c(17, 20, 26), line: c(38, 46, 58), idle: c(26, 31, 40), hover: c(36, 44, 56), text: c(214, 226, 222), weak: c(118, 134, 140), acc: c(86, 214, 255), amber: c(255, 181, 71) }
    } else {
        Pal { panel: c(232, 226, 210), card: c(244, 239, 226), line: c(196, 186, 162), idle: c(226, 219, 200), hover: c(214, 205, 182), text: c(40, 38, 34), weak: c(120, 112, 96), acc: c(0, 112, 170), amber: c(190, 104, 0) }
    }
}

pub fn accent(dark: bool) -> Color32 {
    pal(dark).acc
}

/// Retro heading / readout text.
fn mono(s: f32) -> FontId {
    FontId::monospace(s)
}

/// Windows fonts: Segoe UI / Cascadia Mono for Latin, then fallbacks for Japanese, Chinese, Korean
/// and Arabic. The current language's CJK font goes first, because the same Han character is drawn
/// differently in Japanese and Chinese.
pub fn set_fonts(ctx: &egui::Context) {
    use egui::FontFamily::{Monospace, Proportional};
    let mut fonts = egui::FontDefinitions::default();
    let mut load = |name: &str, file: &str| {
        let ok = std::fs::read(format!(r"C:\Windows\Fonts\{file}")).map(|d| fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(d)))).is_ok();
        ok.then(|| name.to_string())
    };
    // Segoe UI Variable has no Arabic glyphs, so Arabic words always become shaping runs of their own
    // (needed for right-to-left order, see i18n::rtl); plain Segoe UI only if it is missing
    let ui_font = load("ui", "SegUIVar.ttf").or_else(|| load("ui", "segoeui.ttf"));
    let mono = [load("mono", "CascadiaMono.ttf"), load("mono2", "consola.ttf")];
    let (ja, zh, ko) = (load("ja", "YuGothM.ttc"), load("zh", "msyh.ttc"), load("ko", "malgun.ttf"));
    let ar = load("ar", "arial.ttf");
    let cjk = match i18n::cur() { 2 => [ja, zh, ko], 4 => [ko, ja, zh], _ => [zh, ja, ko] };
    let fallbacks: Vec<String> = cjk.into_iter().chain([ar]).flatten().collect();
    for (fam, first) in [(Proportional, vec![ui_font]), (Monospace, mono.to_vec())] {
        let list = fonts.families.get_mut(&fam).unwrap();
        for (i, f) in first.into_iter().flatten().enumerate() { list.insert(i, f) }
        list.extend(fallbacks.iter().cloned());
    }
    ctx.set_fonts(fonts);
}

pub fn style(ctx: &egui::Context) {
    set_fonts(ctx);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 7.0);
        s.spacing.button_padding = vec2(10.0, 5.0);
        s.spacing.interact_size.y = 24.0;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, mono(13.0));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(11.5));
        s.text_styles.insert(egui::TextStyle::Heading, mono(18.0));
        let v = &mut s.visuals;
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = CornerRadius::same(4);
        }
        v.window_corner_radius = CornerRadius::same(6);
        v.menu_corner_radius = CornerRadius::same(4);
    });
    for (theme, dark) in [(Theme::Light, false), (Theme::Dark, true)] {
        ctx.style_mut_of(theme, |s| {
            let c = pal(dark);
            let v = &mut s.visuals;
            v.panel_fill = c.panel;
            v.window_fill = c.card;
            v.window_stroke = Stroke::new(1.0, c.line);
            v.extreme_bg_color = c.idle;
            v.faint_bg_color = c.idle;
            v.override_text_color = Some(c.text);
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.line);
            for (w, fill, line) in [(&mut v.widgets.inactive, c.idle, c.line), (&mut v.widgets.hovered, c.hover, c.acc), (&mut v.widgets.active, c.hover, c.acc), (&mut v.widgets.open, c.hover, c.line)] {
                (w.weak_bg_fill, w.bg_fill, w.bg_stroke) = (fill, fill, Stroke::new(1.0, line));
            }
            v.selection.bg_fill = c.acc.gamma_multiply(0.35);
            v.selection.stroke = Stroke::new(1.0, c.acc);
            v.hyperlink_color = c.acc;
        });
    }
}

/// Soft neon halo around a rect.
fn glow(p: &egui::Painter, r: Rect, radius: f32, c: Color32) {
    for (w, a) in [(7.0, 0.07), (4.0, 0.14), (2.0, 0.3)] {
        p.rect_stroke(r.expand(w / 2.0), radius + w / 2.0, Stroke::new(w, c.gamma_multiply(a)), StrokeKind::Outside);
    }
}

fn glow_circle(p: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    for (w, a) in [(7.0, 0.07), (4.0, 0.14), (2.0, 0.3)] {
        p.circle_stroke(c, r + w / 2.0, Stroke::new(w, col.gamma_multiply(a)));
    }
}

fn card<R>(ui: &mut egui::Ui, title: &str, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let c = pal(ui.visuals().dark_mode);
    egui::Frame::new()
        .fill(c.card)
        .stroke(Stroke::new(1.0, c.line))
        .corner_radius(6)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                if !title.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("▌").font(mono(13.0)).color(c.acc));
                        ui.label(RichText::new(title.to_uppercase()).font(mono(12.5)).color(c.weak));
                    });
                }
                f(ui)
            })
            .inner
        })
        .inner
}

/// Retro power switch: LED + ON/OFF.
fn switch(ui: &mut egui::Ui, on: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(78.0, 28.0), Sense::click());
    let c = pal(ui.visuals().dark_mode);
    let t = ui.ctx().animate_bool_responsive(resp.id, on);
    let p = ui.painter();
    p.rect_filled(rect, 4.0, if resp.hovered() { c.hover } else { c.idle });
    p.rect_stroke(rect, 4.0, Stroke::new(1.0, if on { c.acc } else { c.line }), StrokeKind::Inside);
    let led = Pos2::new(rect.left() + 15.0, rect.center().y);
    let lc = if on { c.acc } else { c.weak.gamma_multiply(0.5) };
    if on { glow_circle(p, led, 4.0, c.acc.gamma_multiply(t)) }
    p.circle_filled(led, 4.0, lc);
    p.text(Pos2::new(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, tr(if on { "ON" } else { "OFF" }), mono(12.5), if on { c.acc } else { c.weak });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

#[derive(Clone, Copy, PartialEq)]
enum WinBtn { Min, Max, Restore, Close }

/// Minimize / maximize / close in the own title bar, drawn as lines (no font glyph needed); close lights red.
fn win_btn(ui: &mut egui::Ui, kind: WinBtn) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 26.0), Sense::click());
    let c = pal(ui.visuals().dark_mode);
    let hot = resp.hovered();
    let col = if kind == WinBtn::Close && hot { Color32::from_rgb(255, 90, 80) } else if hot { c.acc } else { c.weak };
    let p = ui.painter();
    if hot {
        p.rect_filled(rect, 4.0, col.gamma_multiply(0.15));
        p.rect_stroke(rect, 4.0, Stroke::new(1.0, col), StrokeKind::Inside);
    }
    let (m, s) = (rect.center(), Stroke::new(1.4, col));
    let box_at = |o: Vec2| Rect::from_center_size(m + o, vec2(9.0, 9.0));
    match kind {
        WinBtn::Min => { p.hline((m.x - 5.0)..=(m.x + 5.0), m.y + 1.0, s); }
        WinBtn::Max => { p.rect_stroke(box_at(Vec2::ZERO), 1.0, s, StrokeKind::Inside); }
        WinBtn::Restore => {
            p.rect_stroke(box_at(vec2(1.5, -1.5)), 1.0, s, StrokeKind::Inside);
            p.rect_filled(box_at(vec2(-1.5, 1.5)), 1.0, if hot { col.gamma_multiply(0.15) } else { c.panel });
            p.rect_stroke(box_at(vec2(-1.5, 1.5)), 1.0, s, StrokeKind::Inside);
        }
        WinBtn::Close => {
            p.line_segment([m + vec2(-4.5, -4.5), m + vec2(4.5, 4.5)], s);
            p.line_segment([m + vec2(-4.5, 4.5), m + vec2(4.5, -4.5)], s);
        }
    }
    resp.clicked()
}

/// The window has no Windows frame: resize from its edges, and draw a thin border.
fn window_edges(ctx: &egui::Context) {
    use egui::{CursorIcon as C, ResizeDirection as D};
    let r = ctx.content_rect();
    let c = pal(ctx.theme() == Theme::Dark);
    let maxed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    if !maxed {
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("frame")))
            .rect_stroke(r, 0.0, Stroke::new(1.0, c.line), StrokeKind::Inside);
    }
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()).filter(|_| !maxed) else { return };
    let m = 6.0;
    let (l, rt, t, b) = (pos.x < r.left() + m, pos.x > r.right() - m, pos.y < r.top() + m, pos.y > r.bottom() - m);
    let dir = match (l, rt, t, b) {
        (true, _, true, _) => Some((D::NorthWest, C::ResizeNorthWest)),
        (_, true, true, _) => Some((D::NorthEast, C::ResizeNorthEast)),
        (true, _, _, true) => Some((D::SouthWest, C::ResizeSouthWest)),
        (_, true, _, true) => Some((D::SouthEast, C::ResizeSouthEast)),
        (true, ..) => Some((D::West, C::ResizeWest)),
        (_, true, ..) => Some((D::East, C::ResizeEast)),
        (_, _, true, _) => Some((D::North, C::ResizeNorth)),
        (.., true) => Some((D::South, C::ResizeSouth)),
        _ => None,
    };
    if let Some((d, cur)) = dir {
        ctx.set_cursor_icon(cur);
        if ctx.input(|i| i.pointer.primary_pressed()) { ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(d)) }
    }
}

/// Language tiles: own name big, English name small under it. Returns the clicked language.
fn lang_grid(ui: &mut egui::Ui, current: usize, skip: &[usize]) -> Option<usize> {
    let c = pal(ui.visuals().dark_mode);
    let ids: Vec<usize> = (0..i18n::LANGS.len()).filter(|i| !skip.contains(i)).collect();
    let cols = if ui.available_width() > 420.0 { 3 } else { 2 };
    let w = (ui.available_width() - (cols - 1) as f32 * 8.0) / cols as f32;
    let mut hit = None;
    egui::Grid::new(("langs", skip.len())).spacing([8.0, 8.0]).show(ui, |ui| {
        for (n, &i) in ids.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 46.0), Sense::click());
            let on = i == current;
            let hot = resp.hovered();
            let p = ui.painter();
            if on { glow(p, rect, 4.0, c.acc) }
            p.rect_filled(rect, 4.0, if on { c.acc.gamma_multiply(0.2) } else if hot { c.hover } else { c.idle });
            p.rect_stroke(rect, 4.0, Stroke::new(1.0, if on || hot { c.acc } else { c.line }), StrokeKind::Inside);
            p.text(rect.center() - vec2(0.0, 7.0), Align2::CENTER_CENTER, i18n::native(i), FontId::proportional(15.0), if on { c.acc } else { c.text });
            let english = ["English", "German", "Japanese", "Chinese (Simplified)", "Korean", "Vietnamese", "Filipino", "Arabic"][i];
            p.text(rect.center() + vec2(0.0, 11.0), Align2::CENTER_CENTER, english, mono(10.0), c.weak);
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { hit = Some(i) }
            if n % cols == cols - 1 { ui.end_row() }
        }
    });
    hit
}

/// A grid cell like AntiMicro's button widgets: caption + current assignment, lit while pressed.
fn cell(ui: &mut egui::Ui, title: &str, body: &str, active: bool, size: Vec2) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let c = pal(ui.visuals().dark_mode);
    let p = ui.painter();
    let empty = body == "—";
    if active { glow(p, rect, 4.0, c.acc) }
    p.rect_filled(rect, 4.0, if active { c.acc.gamma_multiply(0.22) } else if resp.hovered() { c.hover } else { c.idle });
    p.rect_stroke(rect, 4.0, Stroke::new(1.0, if active || resp.hovered() { c.acc } else { c.line }), StrokeKind::Inside);
    let (tc, bc) = if active { (c.acc, c.acc) } else { (c.weak, if empty { c.weak.gamma_multiply(0.6) } else { c.text }) };
    let max_chars = ((size.x - 14.0) / 7.0).max(3.0) as usize;
    let body: String = if body.chars().count() > max_chars { body.chars().take(max_chars - 1).chain(['…']).collect() } else { body.into() };
    if title.is_empty() {
        p.text(rect.center(), Align2::CENTER_CENTER, body, FontId::proportional(13.0), bc);
    } else {
        p.text(rect.left_top() + vec2(8.0, 5.0), Align2::LEFT_TOP, title, mono(10.5), tc);
        p.text(rect.left_bottom() + vec2(8.0, -5.0), Align2::LEFT_BOTTOM, body, FontId::proportional(13.0), bc);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Retro fader/knob for every number: hover, hold and drag up/down (or left/right) to change it,
/// Shift for fine steps; a plain click turns it into a text field to type the value
/// (comma or dot), Enter takes it, Esc cancels.
struct Regler<'a> { text: &'a str, suffix: &'a str, log: bool, dec: Option<usize> }

fn rg(text: &str) -> Regler<'_> {
    Regler { text, suffix: "", log: false, dec: None }
}

impl<'a> Regler<'a> {
    fn suffix(mut self, s: &'a str) -> Self { self.suffix = s; self }
    fn log(mut self) -> Self { self.log = true; self }
    fn dec(mut self, d: usize) -> Self { self.dec = Some(d); self }

    fn show<N: egui::emath::Numeric>(self, ui: &mut egui::Ui, v: &mut N, range: std::ops::RangeInclusive<N>) -> egui::Response {
        let (lo, hi) = (range.start().to_f64(), range.end().to_f64());
        let log = self.log && lo > 0.0;
        let to_t = |x: f64| if log { (x / lo).ln() / (hi / lo).ln() } else { (x - lo) / (hi - lo).max(1e-12) };
        let from_t = |t: f64| {
            let t = t.clamp(0.0, 1.0);
            let x = if log { lo * (hi / lo).powf(t) } else { lo + t * (hi - lo) };
            if N::INTEGRAL { x.round() } else { x }
        };
        let dec = self.dec.unwrap_or(if N::INTEGRAL { 0 } else if hi - lo <= 1.0 { 3 } else if hi - lo <= 20.0 { 2 } else { 1 });
        let fmt = |x: f64| format!("{x:.dec$}").replace('.', ",");
        let parse = |s: &str| s.trim().trim_end_matches(self.suffix.trim()).trim().replace(',', ".").parse::<f64>().ok();

        let w = ui.available_width().clamp(180.0, 460.0);
        let id = ui.auto_id_with(("regler", self.text)); // stable while scrolling, unlike the position
        let (rect, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
        let edit_id = id.with("edit");
        let enabled = ui.is_enabled();
        let mut resp = ui.interact(rect, id, if enabled { Sense::click_and_drag() } else { Sense::hover() });
        let c = pal(ui.visuals().dark_mode);
        let mut val = v.to_f64().clamp(lo, hi);

        // typing mode
        let editing: Option<String> = ui.data(|d| d.get_temp(edit_id));
        if let Some(mut s) = editing {
            let te = ui.put(rect, egui::TextEdit::singleline(&mut s).id(edit_id).font(mono(13.0)).horizontal_align(egui::Align::Center));
            if te.lost_focus() {
                if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    if let Some(x) = parse(&s) { val = from_t(to_t(x.clamp(lo, hi))); resp.mark_changed() }
                }
                ui.data_mut(|d| d.remove::<String>(edit_id));
            } else {
                ui.data_mut(|d| d.insert_temp(edit_id, s));
            }
            if resp.changed() { *v = N::from_f64(val) }
            return resp;
        }
        if resp.clicked() {
            ui.data_mut(|d| d.insert_temp(edit_id, fmt(val)));
            ui.memory_mut(|m| m.request_focus(edit_id));
        }
        if resp.drag_started() { ui.data_mut(|d| d.insert_temp(id, to_t(val))); }
        if resp.dragged() {
            let d = resp.drag_delta();
            let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
            let t: f64 = ui.data(|m| m.get_temp(id)).unwrap_or(to_t(val)) + (d.x - d.y) as f64 / 260.0 * fine;
            ui.data_mut(|m| m.insert_temp(id, t.clamp(0.0, 1.0)));
            let nv = from_t(t);
            if nv != val { val = nv; resp.mark_changed() }
        }
        if resp.changed() { *v = N::from_f64(val) } // untouched values stay exactly as loaded

        // look: LED segment meter with label and readout
        let p = ui.painter();
        let hot = enabled && (resp.hovered() || resp.dragged());
        let dim = |x: Color32| if enabled { x } else { x.gamma_multiply(0.45) };
        p.rect_filled(rect, 4.0, if hot { c.hover } else { c.idle });
        p.rect_stroke(rect, 4.0, Stroke::new(1.0, if hot { c.acc } else { c.line }), StrokeKind::Inside);
        let t = to_t(val).clamp(0.0, 1.0) as f32;
        let meter = Rect::from_min_max(Pos2::new(rect.left() + rect.width() * 0.5, rect.top() + 9.0), Pos2::new(rect.right() - 74.0, rect.bottom() - 9.0));
        let n = ((meter.width() / 6.0) as usize).max(4);
        let seg = meter.width() / n as f32;
        for i in 0..n {
            let r = Rect::from_min_size(Pos2::new(meter.left() + i as f32 * seg, meter.top()), vec2(seg - 2.0, meter.height()));
            let lit = (i as f32 + 0.5) / n as f32 <= t;
            p.rect_filled(r, 1.0, dim(if lit { c.acc } else { c.line }));
        }
        p.text(rect.left_center() + vec2(9.0, 0.0), Align2::LEFT_CENTER, self.text, FontId::proportional(13.0), dim(c.text));
        p.text(rect.right_center() - vec2(9.0, 0.0), Align2::RIGHT_CENTER, format!("{}{}", fmt(val), self.suffix), mono(12.5), dim(if hot { c.acc } else { c.text }));
        resp.on_hover_cursor(egui::CursorIcon::ResizeVertical).on_hover_text(tr("Drag = adjust (Shift = fine) · Click = type a number"))
    }
}

// ================================================================= text

pub fn act_text(a: Act) -> String {
    match a {
        Act::Key(vk) => winio::key_name(vk),
        Act::Mouse(b) => tr(["", "Mouse L", "Mouse R", "Mouse M", "Mouse 4", "Mouse 5"][b.min(5) as usize]).into(),
        Act::Wheel(w) => format!("{} {}", tr("Wheel"), ["↑", "↓", "←", "→"][w.min(3) as usize]),
        Act::Move(d) => format!("{} {}", tr("Mouse"), ["", "↑", "↓", "←", "→"][d.min(4) as usize]),
        Act::Pad(o) => format!("🎮 {}", o.label()),
    }
}

fn secs(ms: u32) -> String {
    format!("{:.2}s", ms as f32 / 1000.0)
}

pub fn slot_text(s: &Slot) -> String {
    match s {
        Slot::Out(a) => act_text(*a),
        Slot::Pause(ms) => format!("{} {}", tr("Pause"), secs(*ms)),
        Slot::Hold(ms) => format!("{} {}", tr("Hold"), secs(*ms)),
        Slot::Cycle => tr("Cycle").into(),
        Slot::Distance(p) => format!("{} {p}%", tr("Distance")),
        Slot::Release(ms) => format!("{} {}", tr("Release"), secs(*ms)),
        Slot::MouseMod(p) => format!("{} {p}%", tr("Mouse mod")),
        Slot::PressTime(ms) => format!("{} {}", tr("Press time"), secs(*ms)),
        Slot::Delay(ms) => format!("{} {}", tr("Delay"), secs(*ms)),
        Slot::SetChange(s) => format!("Set {}", s + 1),
        Slot::Text(t) => format!("{} “{t}”", tr("Text")),
        Slot::Exec { path, .. } => format!("{} {}", tr("Run"), path.rsplit(['\\', '/']).next().unwrap_or("")),
        Slot::Load(p) => format!("{} {}", tr("Profile"), stem(p)),
    }
}

pub fn summary(b: &Button) -> String {
    if !b.action.is_empty() {
        return b.action.clone();
    }
    let mut s = if b.slots.is_empty() { "—".to_string() } else { b.slots.iter().map(slot_text).collect::<Vec<_>>().join(" · ") };
    if b.turbo { s = format!("⚡ {s}") }
    if b.toggle { s = format!("⇆ {s}") }
    if let Some((set, _)) = b.set { s = format!("{s} ▸S{}", set + 1) }
    s
}

fn stem(path: &str) -> String {
    std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

const INSTALLING: &str = "Installing ViGEmBus…";

const BTN_NAMES: [&str; 11] = ["A", "B", "X", "Y", "LB", "RB", "Back", "Start", "Guide", "L3", "R3"];
const STICK_NAMES: [&str; 2] = ["Left stick", "Right stick"];
const TRIG_NAMES: [&str; 2] = ["LT", "RT"];

fn elem_name(p: &Set, e: Elem) -> String {
    let custom = p.button(e).map(|b| b.name.clone()).unwrap_or_default();
    if !custom.is_empty() {
        return custom;
    }
    match e {
        Elem::Btn(i) => BTN_NAMES[i as usize].into(),
        Elem::DPad(d) => format!("D-Pad {}", DIRS[d as usize]),
        Elem::Stick(s, d) => format!("{} {}", tr(STICK_NAMES[s as usize]), DIRS[d as usize]),
        Elem::StickMod(s) => format!("{} {}", tr(STICK_NAMES[s as usize]), tr("Modifier")),
        Elem::Trig(t, pos) => format!("{} {}", TRIG_NAMES[t as usize], if pos { "+" } else { "−" }),
        Elem::Key(i) => p.keys.get(i as usize).map(|k| format!("⌨ {}", winio::key_name(k.vk))).unwrap_or_default(),
    }
}

// ================================================================= pickers

/// Keyboard as (vk, width in key units); 0 = gap.
const KB_MAIN: [&[(u16, f32)]; 6] = [
    &[(0x1B, 1.0), (0, 1.0), (0x70, 1.0), (0x71, 1.0), (0x72, 1.0), (0x73, 1.0), (0, 0.5), (0x74, 1.0), (0x75, 1.0), (0x76, 1.0), (0x77, 1.0), (0, 0.5), (0x78, 1.0), (0x79, 1.0), (0x7A, 1.0), (0x7B, 1.0)],
    &[(0xDC, 1.0), (0x31, 1.0), (0x32, 1.0), (0x33, 1.0), (0x34, 1.0), (0x35, 1.0), (0x36, 1.0), (0x37, 1.0), (0x38, 1.0), (0x39, 1.0), (0x30, 1.0), (0xDB, 1.0), (0xDD, 1.0), (0x08, 2.0)],
    &[(0x09, 1.5), (0x51, 1.0), (0x57, 1.0), (0x45, 1.0), (0x52, 1.0), (0x54, 1.0), (0x5A, 1.0), (0x55, 1.0), (0x49, 1.0), (0x4F, 1.0), (0x50, 1.0), (0xBA, 1.0), (0xBB, 1.0), (0x0D, 1.5)],
    &[(0x14, 1.75), (0x41, 1.0), (0x53, 1.0), (0x44, 1.0), (0x46, 1.0), (0x47, 1.0), (0x48, 1.0), (0x4A, 1.0), (0x4B, 1.0), (0x4C, 1.0), (0xC0, 1.0), (0xDE, 1.0), (0xBF, 1.25)],
    &[(0xA0, 1.25), (0xE2, 1.0), (0x59, 1.0), (0x58, 1.0), (0x43, 1.0), (0x56, 1.0), (0x42, 1.0), (0x4E, 1.0), (0x4D, 1.0), (0xBC, 1.0), (0xBE, 1.0), (0xBD, 1.0), (0xA1, 2.75)],
    &[(0xA2, 1.5), (0x5B, 1.25), (0xA4, 1.25), (0x20, 5.75), (0xA5, 1.25), (0x5D, 1.25), (0xA3, 1.75)],
];
const KB_NAV: [&[(u16, f32)]; 6] = [
    &[(0x2C, 1.0), (0x91, 1.0), (0x13, 1.0)],
    &[(0x2D, 1.0), (0x24, 1.0), (0x21, 1.0)],
    &[(0x2E, 1.0), (0x23, 1.0), (0x22, 1.0)],
    &[],
    &[(0, 1.0), (0x26, 1.0)],
    &[(0x25, 1.0), (0x28, 1.0), (0x27, 1.0)],
];
const KB_NUM: [&[(u16, f32)]; 6] = [
    &[],
    &[(0x90, 1.0), (0x6F, 1.0), (0x6A, 1.0), (0x6D, 1.0)],
    &[(0x67, 1.0), (0x68, 1.0), (0x69, 1.0), (0x6B, 1.0)],
    &[(0x64, 1.0), (0x65, 1.0), (0x66, 1.0)],
    &[(0x61, 1.0), (0x62, 1.0), (0x63, 1.0)],
    &[(0x60, 2.0), (0x6E, 1.0)],
];

fn short_key(vk: u16) -> String {
    match vk {
        0x20 => tr("Space").into(), 0x0D => "Enter".into(), 0x09 => "Tab".into(), 0x14 => "Caps".into(), 0xA0 | 0xA1 => "Shift".into(),
        0x08 => "←─".into(), 0x2E => "Del".into(), 0x2D => "Ins".into(), 0x21 => "PgUp".into(), 0x22 => "PgDn".into(), 0x24 => "Home".into(),
        0x23 => "End".into(), 0x13 => "Pause".into(), 0x90 => "Num".into(), 0xA4 => "Alt".into(), 0xA5 => "AltGr".into(), 0x5B => "Win".into(), 0x5D => "Menu".into(),
        0xA2 | 0xA3 => "Ctrl".into(), 0x60..=0x69 => format!("{}", vk - 0x60), 0x6A => "*".into(), 0x6B => "+".into(),
        0x6D => "-".into(), 0x6E => ",".into(), 0x6F => "/".into(), 0x2C => "Print".into(), 0x91 => "Scroll".into(),
        _ => winio::key_name(vk),
    }
}

/// Clickable virtual keyboard (AntiMicro's VirtualKeyboardMouseWidget).
fn keyboard(ui: &mut egui::Ui, sel: Option<u16>, numpad: bool) -> Option<u16> {
    let units = 15.25 + 0.5 + 3.0 + if numpad { 0.5 + 4.0 } else { 0.0 };
    let u = (ui.available_width() / units).clamp(18.0, 40.0);
    let (rect, _) = ui.allocate_exact_size(vec2(units * u, 6.0 * u + 4.0), Sense::hover());
    let dark = ui.visuals().dark_mode;
    let mut hit = None;
    let old_pad = std::mem::replace(&mut ui.spacing_mut().button_padding, vec2(1.0, 1.0));
    let blocks: Vec<(f32, &[&[(u16, f32)]; 6])> = if numpad { vec![(0.0, &KB_MAIN), (15.75, &KB_NAV), (19.25, &KB_NUM)] } else { vec![(0.0, &KB_MAIN), (15.75, &KB_NAV)] };
    for (x0, rows) in blocks {
        for (r, row) in rows.iter().enumerate() {
            let mut x = x0;
            for &(vk, w) in row.iter() {
                if vk != 0 {
                    let kr = Rect::from_min_size(rect.min + vec2(x * u, r as f32 * u + if r > 0 { 4.0 } else { 0.0 }), vec2(w * u - 3.0, u - 3.0));
                    let on = sel == Some(vk);
                    let b = egui::Button::new(RichText::new(short_key(vk)).size((u * 0.36).clamp(9.0, 13.0)).color(if on { Color32::WHITE } else { ui.visuals().strong_text_color() }))
                        .fill(if on { accent(dark) } else { ui.visuals().faint_bg_color })
                        .corner_radius(5)
                        .truncate();
                    if ui.put(kr, b).on_hover_text(winio::key_name(vk)).clicked() { hit = Some(vk) }
                }
                x += w;
            }
        }
    }
    ui.spacing_mut().button_padding = old_pad;
    hit
}

fn chip(ui: &mut egui::Ui, label: &str, on: bool) -> bool {
    let dark = ui.visuals().dark_mode;
    let b = egui::Button::new(RichText::new(label).color(if on { Color32::WHITE } else { ui.visuals().strong_text_color() }))
        .fill(if on { accent(dark) } else { ui.visuals().faint_bg_color })
        .min_size(vec2(74.0, 26.0));
    ui.add(b).clicked()
}

/// Keyboard / mouse / virtual pad output picker.
fn act_picker(ui: &mut egui::Ui, tab: &mut u8, sel: Option<Act>, numpad: bool) -> Option<Act> {
    ui.horizontal(|ui| {
        for (i, l) in [("⌨", "Keyboard"), ("🖱", "Mouse"), ("🎮", "Controller")].into_iter().enumerate() {
            ui.selectable_value(tab, i as u8, format!("{} {}", l.0, tr(l.1)));
        }
    });
    let mut hit = None;
    match *tab {
        0 => hit = keyboard(ui, match sel { Some(Act::Key(k)) => Some(k), _ => None }, numpad).map(Act::Key),
        1 => {
            for (label, acts) in [
                (tr("Buttons"), vec![Act::Mouse(1), Act::Mouse(3), Act::Mouse(2), Act::Mouse(4), Act::Mouse(5)]),
                (tr("Wheel"), vec![Act::Wheel(0), Act::Wheel(1), Act::Wheel(2), Act::Wheel(3)]),
                (tr("Movement"), vec![Act::Move(1), Act::Move(2), Act::Move(3), Act::Move(4)]),
            ] {
                ui.horizontal_wrapped(|ui| {
                    ui.add_sized([70.0, 24.0], egui::Label::new(RichText::new(label).weak()));
                    for a in acts {
                        if chip(ui, &act_text(a), sel == Some(a)) { hit = Some(a) }
                    }
                });
            }
        }
        _ => {
            ui.horizontal_wrapped(|ui| {
                for o in Out::ALL {
                    if chip(ui, o.label(), sel == Some(Act::Pad(o))) { hit = Some(Act::Pad(o)) }
                }
            });
        }
    }
    hit
}

/// Seconds slider for a ms value: drag the bar, or the number box (hover, hold, slide left/right),
/// or click the number and type it ("0,02" and "0.02" both work).
fn secs_slider(ui: &mut egui::Ui, ms: &mut u32, min_ms: u32, max_ms: u32, text: String) {
    let mut s = *ms as f64 / 1000.0;
    if rg(&text).suffix(" s").log().dec(3).show(ui, &mut s, min_ms as f64 / 1000.0..=max_ms as f64 / 1000.0).changed() {
        *ms = ((s * 1000.0).round() as u32).clamp(min_ms, max_ms);
    }
}

fn ms_field(ui: &mut egui::Ui, label: &str, v: &mut u32, max: u32) -> bool {
    rg(label).suffix(" ms").log().show(ui, v, 1..=max).changed()
}

// ================================================================= controller drawing

/// Last stick positions (normalized, y up) for the motion trail in the live view.
pub type Trail = [std::collections::VecDeque<(f32, f32)>; 2];

/// Live view: the real controller in phosphor green, padmimic's virtual pad output in amber.
/// Sticks show dead zone and max zone of the current set plus a short motion trail.
fn controller(ui: &mut egui::Ui, g: &XGamepad, out: Option<&XGamepad>, sticks: &[Stick; 2], trail: &mut Trail) {
    let w = ui.available_width().min(400.0);
    let h = w * 0.56;
    let (resp, p) = ui.allocate_painter(vec2(ui.available_width(), h + 30.0), Sense::hover());
    let o = resp.rect.min + vec2((resp.rect.width() - w) / 2.0, 4.0);
    let at = |x: f32, y: f32| o + vec2(x * w, y * w);
    let c = pal(ui.visuals().dark_mode);
    let on = |b: u16| g.buttons.raw & b != 0;
    let von = |b: u16| out.is_some_and(|v| v.buttons.raw & b != 0);

    // backdrop: faint grid + scanlines (CRT feel, cheap)
    let area = Rect::from_min_size(o - vec2(0.0, 4.0), vec2(w, h + 4.0));
    let mut y = area.top();
    while y < area.bottom() {
        p.hline(area.x_range(), y, Stroke::new(1.0, c.line.gamma_multiply(0.18)));
        y += 3.0;
    }
    for i in 1..12 {
        let x = area.left() + area.width() * i as f32 / 12.0;
        p.vline(x, area.y_range(), Stroke::new(1.0, c.line.gamma_multiply(0.12)));
    }

    // body silhouette: right half as cubic curves, mirrored; filled as a fan (the shape is star-shaped
    // around its centre), outlined in neon
    let right: [[(f32, f32); 4]; 4] = [
        [(0.5, 0.135), (0.6, 0.135), (0.68, 0.115), (0.79, 0.115)],
        [(0.79, 0.115), (0.9, 0.115), (0.93, 0.33), (0.92, 0.47)],
        [(0.92, 0.47), (0.91, 0.56), (0.81, 0.58), (0.76, 0.52)],
        [(0.76, 0.52), (0.71, 0.46), (0.6, 0.43), (0.5, 0.43)],
    ];
    let mut outline: Vec<Pos2> = vec![];
    for seg in right.iter() {
        let pts = seg.map(|(x, y)| at(x, y));
        let b = egui::epaint::CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, Stroke::NONE);
        outline.extend(b.flatten(Some(0.4)));
    }
    let left: Vec<Pos2> = outline.iter().rev().map(|q| Pos2::new(2.0 * at(0.5, 0.0).x - q.x, q.y)).collect();
    outline.extend(left);
    let centre = at(0.5, 0.3);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(centre, c.card);
    for (i, q) in outline.iter().enumerate() {
        mesh.colored_vertex(*q, c.idle);
        let n = outline.len() as u32;
        mesh.add_triangle(0, 1 + i as u32, 1 + (i as u32 + 1) % n);
    }
    p.add(egui::Shape::mesh(mesh));
    p.add(egui::Shape::closed_line(outline.clone(), Stroke::new(5.0, c.acc.gamma_multiply(0.08))));
    p.add(egui::Shape::closed_line(outline, Stroke::new(1.5, c.acc.gamma_multiply(0.75))));

    let label = |pos: Pos2, s: &str, size: f32, col: Color32| { p.text(pos, Align2::CENTER_CENTER, s, mono(size * w), col); };

    // triggers: vertical meters outside the body, real (green) + virtual output (amber)
    for (x, v, vo, name) in [(0.035, g.left_trigger, out.map(|o| o.left_trigger), "LT"), (0.965, g.right_trigger, out.map(|o| o.right_trigger), "RT")] {
        let r = Rect::from_center_size(at(x, 0.3), vec2(0.03 * w, 0.34 * w));
        p.rect_filled(r, 2.0, c.idle);
        p.rect_stroke(r, 2.0, Stroke::new(1.0, c.line), StrokeKind::Inside);
        let fill = |val: u8, rr: Rect, col: Color32| {
            let top = rr.bottom() - rr.height() * val as f32 / 255.0;
            p.rect_filled(Rect::from_min_max(Pos2::new(rr.left(), top), rr.max), 2.0, col);
        };
        fill(v, r.shrink(2.0), c.acc);
        if v > 30 { glow(&p, r, 2.0, c.acc) }
        if let Some(vo) = vo {
            let side = Rect::from_min_size(Pos2::new(if x < 0.5 { r.right() + 3.0 } else { r.left() - 7.0 }, r.top()), vec2(4.0, r.height()));
            p.rect_filled(side, 1.0, c.idle);
            fill(vo, side, c.amber);
        }
        label(r.center_top() - vec2(0.0, 0.025 * w), name, 0.026, if v > 30 { c.acc } else { c.weak });
        label(r.center_bottom() + vec2(0.0, 0.025 * w), &format!("{v:03}"), 0.024, c.text);
    }

    // bumpers
    for (x0, b, name) in [(0.2, XButtons::LB, "LB"), (0.64, XButtons::RB, "RB")] {
        let r = Rect::from_min_size(at(x0, 0.07), vec2(0.16 * w, 0.04 * w));
        let lit = on(b);
        if lit { glow(&p, r, 6.0, c.acc) }
        p.rect_filled(r, 6.0, if lit { c.acc.gamma_multiply(0.35) } else { c.idle });
        p.rect_stroke(r, 6.0, Stroke::new(1.2, if lit { c.acc } else if von(b) { c.amber } else { c.line }), StrokeKind::Inside);
        label(r.center(), name, 0.024, if lit { c.acc } else { c.weak });
    }

    // sticks: well, max zone, dead zone, crosshair, trail, cap
    for (si, (cx, cy), x, y, click, vxy) in [
        (0usize, (0.3, 0.25), g.thumb_lx, g.thumb_ly, on(XButtons::LTHUMB), out.map(|o| (o.thumb_lx, o.thumb_ly))),
        (1, (0.62, 0.36), g.thumb_rx, g.thumb_ry, on(XButtons::RTHUMB), out.map(|o| (o.thumb_rx, o.thumb_ry))),
    ] {
        let ctr = at(cx, cy);
        let r = 0.075 * w;
        let s = &sticks[si];
        p.circle_filled(ctr, r, c.card);
        p.circle_stroke(ctr, r, Stroke::new(1.0, c.line));
        p.circle_stroke(ctr, r * (s.max as f32 / 32767.0).min(1.0), Stroke::new(1.0, c.weak.gamma_multiply(0.5)));
        p.circle_filled(ctr, r * s.dead as f32 / 32767.0, c.line.gamma_multiply(0.5));
        p.hline((ctr.x - r)..=(ctr.x + r), ctr.y, Stroke::new(1.0, c.line.gamma_multiply(0.6)));
        p.vline(ctr.x, (ctr.y - r)..=(ctr.y + r), Stroke::new(1.0, c.line.gamma_multiply(0.6)));
        let n = (x as f32 / 32767.0, y as f32 / 32767.0);
        let t = &mut trail[si];
        if t.front() != Some(&n) { t.push_front(n) }
        t.truncate(18);
        let pos = |q: &(f32, f32)| ctr + vec2(q.0, -q.1) * r;
        for (i, win) in t.iter().collect::<Vec<_>>().windows(2).enumerate() {
            let a = 1.0 - i as f32 / 18.0;
            p.line_segment([pos(win[0]), pos(win[1])], Stroke::new(2.0 * a, c.acc.gamma_multiply(0.5 * a)));
        }
        if let Some((vx, vy)) = vxy.filter(|v| *v != (0, 0)) {
            p.circle_stroke(pos(&(vx as f32 / 32767.0, vy as f32 / 32767.0)), r * 0.2, Stroke::new(1.5, c.amber));
        }
        let live = eval_stick(s, x as i32, y as i32).safe;
        let cap = pos(&n);
        if live || click { glow_circle(&p, cap, r * 0.22, c.acc) }
        p.circle_filled(cap, r * 0.22, if click { c.acc } else if live { c.acc.gamma_multiply(0.55) } else { c.weak.gamma_multiply(0.6) });
        p.circle_stroke(cap, r * 0.22, Stroke::new(1.2, c.acc));
    }

    // d-pad
    let dp = at(0.4, 0.37);
    let a = 0.028 * w;
    for (dx, dy, b) in [(0.0, -1.0, XButtons::UP), (0.0, 1.0, XButtons::DOWN), (-1.0, 0.0, XButtons::LEFT), (1.0, 0.0, XButtons::RIGHT)] {
        let r = Rect::from_center_size(dp + vec2(dx, dy) * a, vec2(a, a));
        let lit = on(b);
        if lit { glow(&p, r, 2.0, c.acc) }
        p.rect_filled(r, 2.0, if lit { c.acc } else { c.card });
        p.rect_stroke(r, 2.0, Stroke::new(1.0, if von(b) { c.amber } else { c.line }), StrokeKind::Inside);
    }
    p.rect_filled(Rect::from_center_size(dp, vec2(a, a)), 0.0, c.card);

    // face buttons
    let f = 0.042 * w;
    for (dx, dy, b, l, col) in [
        (0.0, 1.0, XButtons::A, "A", Color32::from_rgb(70, 220, 120)),
        (1.0, 0.0, XButtons::B, "B", Color32::from_rgb(255, 90, 80)),
        (-1.0, 0.0, XButtons::X, "X", Color32::from_rgb(70, 150, 255)),
        (0.0, -1.0, XButtons::Y, "Y", Color32::from_rgb(255, 210, 60)),
    ] {
        let ctr = at(0.71, 0.24) + vec2(dx, dy) * f;
        let r = 0.022 * w;
        let lit = on(b);
        if lit { glow_circle(&p, ctr, r, col) }
        p.circle_filled(ctr, r, if lit { col } else { c.card });
        p.circle_stroke(ctr, r, Stroke::new(1.5, if von(b) { c.amber } else { col.gamma_multiply(0.8) }));
        label(ctr, l, 0.024, if lit { Color32::BLACK } else { col });
    }

    // back / start / guide
    for (x, y, r, b) in [(0.445, 0.24, 0.013, XButtons::BACK), (0.555, 0.24, 0.013, XButtons::START), (0.5, 0.18, 0.022, XButtons::GUIDE)] {
        let ctr = at(x, y);
        let lit = on(b);
        if lit { glow_circle(&p, ctr, r * w, c.acc) }
        p.circle_filled(ctr, r * w, if lit { c.acc } else { c.card });
        p.circle_stroke(ctr, r * w, Stroke::new(1.0, if von(b) { c.amber } else { c.line }));
    }

    // readout strip
    let stick_txt = |x: i16, y: i16| format!("{:+.2} {:+.2}", x as f32 / 32767.0, y as f32 / 32767.0);
    let line = format!("LS {}   RS {}   LT {:03}  RT {:03}", stick_txt(g.thumb_lx, g.thumb_ly), stick_txt(g.thumb_rx, g.thumb_ry), g.left_trigger, g.right_trigger);
    p.text(Pos2::new(resp.rect.center().x, resp.rect.bottom() - 12.0), Align2::CENTER_CENTER, line, mono(12.0), c.weak);
    if out.is_some() {
        p.text(o + vec2(w, 2.0), Align2::RIGHT_TOP, format!("■ {}", tr("virtual")), mono(11.0), c.amber);
    }
    p.text(o + vec2(0.0, 2.0), Align2::LEFT_TOP, format!("■ {}", tr("real")), mono(11.0), c.acc); // legend in the top corners, clear of the readout
}

/// JoyControlStickStatusBox: zones, diagonal sectors and the live position.
fn stick_status(ui: &mut egui::Ui, s: &Stick, x: i16, y: i16) {
    let size = 170.0;
    let (resp, p) = ui.allocate_painter(vec2(size, size), Sense::hover());
    let c = resp.rect.center();
    let r = size / 2.0 - 4.0;
    let dark = ui.visuals().dark_mode;
    let k = r / 32767.0;
    p.circle_filled(c, r, ui.visuals().faint_bg_color);
    p.circle_stroke(c, s.max as f32 * k, (1.0, ui.visuals().weak_text_color()));
    p.circle_filled(c, s.dead as f32 * k, ui.visuals().widgets.hovered.weak_bg_fill);
    p.circle_stroke(c, s.mod_zone as f32 * k, (1.0, Color32::from_rgb(255, 159, 10)));
    let edges: Vec<f32> = match s.mode {
        DirMode::FourCardinal => vec![45.0, 135.0, 225.0, 315.0],
        DirMode::FourDiagonal => vec![0.0, 90.0, 180.0, 270.0],
        _ => {
            let d = s.diag.clamp(1, 90) as f32;
            let card = (360.0 - 4.0 * d) / 4.0;
            let mut e = card / 2.0;
            let mut v = vec![e];
            for w in [d, card, d, card, d, card, d] { e += w; v.push(e) }
            v
        }
    };
    for a in edges {
        let rad = a.to_radians();
        let dir = vec2(rad.sin(), -rad.cos());
        p.line_segment([c + dir * s.dead as f32 * k, c + dir * r], (1.0, ui.visuals().weak_text_color().gamma_multiply(0.5)));
    }
    let e = eval_stick(s, x as i32, y as i32);
    p.circle_filled(c + vec2(x as f32, -(y as f32)) * k, 5.0, if e.safe { accent(dark) } else { ui.visuals().weak_text_color() });
}

// ================================================================= dialogs

#[derive(Clone, Copy, PartialEq)]
enum MouseTarget { Btn(Elem), Stick(u8), DPad, Axis(u8) }

#[derive(Clone, PartialEq)]
enum Dlg {
    Button(Elem),
    Advanced(Elem),
    Stick(u8),
    DPad,
    Axis(u8),
    Mouse(MouseTarget),
    Names,
    Pref,
    QuickSet(Vec<Elem>),
    Settings,
    SaveAs(String),
    Reset,
    About,
    Latency,
    Update,
}

#[derive(Clone, Copy, PartialEq)]
enum Cap { Assign(Elem, Option<usize>), KeyBind(usize), Toggle }

#[derive(Clone, Copy, PartialEq)]
enum Tab { Controller, Kbm }

pub struct App {
    s: Arc<Shared>,
    st: Settings,
    st_sent: Arc<Settings>,
    p: Profile,
    p_sent: Arc<Profile>,
    saved: Profile,
    path: String,
    profiles: Vec<String>,
    tab: Tab,
    dlgs: Vec<Dlg>,
    cap: Option<(Cap, bool)>,
    pick_tab: u8,
    adv_page: u8,
    adv_sel: usize,
    set_page: u8,
    epp_prev: Option<bool>,
    auto_t: Instant,
    auto_last: String,
    toast: Option<(String, Instant)>,
    show_live: bool,
    trail: Trail,
    lang_prompt: Option<usize>, // first start: language to offer
    upd: Arc<std::sync::Mutex<Upd>>,
    lang_more: bool,
    vigem_msg: Arc<std::sync::Mutex<Option<String>>>, // Some while installing, then the result
}

impl App {
    pub fn new(s: Arc<Shared>, mut st: Settings) -> Self {
        // language: the saved one, else English and (once) offer the language of where the user is
        // dev hook for screenshots: PADMIMIC_LANG=ja shows that language without saving it
        let dev_lang = std::env::var("PADMIMIC_LANG").ok().and_then(|c| i18n::id_of(&c));
        let lang_prompt = match dev_lang.or(i18n::id_of(&st.lang)) {
            Some(l) => { i18n::set(l); None }
            None => {
                i18n::set(0);
                let sys = i18n::system().filter(|&l| l != 0);
                if sys.is_none() { st.lang = "en".into() }
                sys
            }
        };
        let path = if st.auto_load_last && Profile::load(&st.last_profile).is_some() { st.last_profile.clone() } else { String::new() };
        let p = Profile::load(&path).unwrap_or_default();
        *s.profile.lock().unwrap() = Arc::new(p.clone());
        *s.settings.lock().unwrap() = Arc::new(st.clone());
        let epp_prev = st.disable_epp.then(|| winio::set_pointer_precision(false));
        let (p_sent, st_sent) = (s.profile.lock().unwrap().clone(), s.settings.lock().unwrap().clone());
        let mut app = App {
            p_sent, st_sent, saved: p.clone(), p, s, st, path,
            profiles: vec![], tab: Tab::Controller, dlgs: vec![], cap: None, pick_tab: 0, adv_page: 0, adv_sel: 0, set_page: 0,
            epp_prev, auto_t: Instant::now(), auto_last: String::new(), toast: None, show_live: true, trail: Default::default(), lang_prompt, lang_more: false, upd: Arc::default(),
            vigem_msg: Arc::default(),
        };
        app.scan_profiles();
        update::cleanup();
        if app.st.check_updates { app.check_updates() }
        // dev hook for screenshots: PADMIMIC_SHOW=settings,languages,mouse,stick,latency,lang opens those at start
        for d in std::env::var("PADMIMIC_SHOW").unwrap_or_default().split(',') {
            match d {
                "settings" => app.open(Dlg::Settings),
                "languages" => { app.set_page = 1; app.open(Dlg::Settings) }
                "mouse" => app.open(Dlg::Mouse(MouseTarget::DPad)),
                "latency" => app.open(Dlg::Latency),
                "stick" => app.open(Dlg::Stick(0)),
                "lang" => app.lang_prompt = i18n::system().or(Some(1)),
                _ => {}
            }
        }
        app
    }

    fn scan_profiles(&mut self) {
        let mut v: Vec<String> = std::fs::read_dir(&self.st.profile_dir).into_iter().flatten().flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .map(|p| p.display().to_string())
            .collect();
        for r in &self.st.recent {
            if !v.contains(r) && std::path::Path::new(r).exists() { v.push(r.clone()) }
        }
        v.sort();
        self.profiles = v;
    }

    fn check_updates(&self) {
        let u = self.upd.clone();
        *u.lock().unwrap() = Upd::Checking;
        std::thread::spawn(move || {
            let r = match update::check() {
                // dev hook for testing the whole update path without clicking
                Ok(Some(r)) if std::env::var_os("PADMIMIC_AUTOUPDATE").is_some() => match update::install(&r) {
                    Ok(()) => Upd::Restarting,
                    Err(e) => Upd::Failed(e),
                },
                Ok(Some(r)) => Upd::Available(r),
                Ok(None) => Upd::Latest,
                Err(e) => Upd::Failed(e),
            };
            *u.lock().unwrap() = r;
        });
    }

    fn install_update(&self, r: update::Release) {
        let u = self.upd.clone();
        *u.lock().unwrap() = Upd::Installing;
        std::thread::spawn(move || {
            *u.lock().unwrap() = match update::install(&r) { Ok(()) => Upd::Restarting, Err(e) => Upd::Failed(e) };
        });
    }

    fn open(&mut self, d: Dlg) {
        if !self.dlgs.contains(&d) { self.dlgs.push(d) }
    }

    fn toast(&mut self, t: impl Into<String>) {
        self.toast = Some((t.into(), Instant::now()));
    }

    fn cur(&self) -> usize {
        self.s.live.lock().unwrap().set.min(7)
    }

    fn set(&mut self) -> &mut Set {
        let c = self.cur();
        &mut self.p.sets[c]
    }

    fn load(&mut self, path: &str) {
        match Profile::load(path) {
            Some(p) => {
                self.p = p.clone();
                self.saved = p;
                self.path = path.into();
                self.remember(path);
                self.toast(trf("Profile “{}” loaded", &[&stem(path)]));
            }
            None => self.toast(trf("Profile {} could not be loaded", &[&path])),
        }
    }

    fn remember(&mut self, path: &str) {
        self.st.last_profile = path.into();
        self.st.recent.retain(|r| r != path);
        self.st.recent.insert(0, path.into());
        self.st.recent.truncate(self.st.recent_count.max(1));
    }

    fn save_to(&mut self, path: String) {
        match self.p.save(&path) {
            Ok(()) => {
                self.saved = self.p.clone();
                self.remember(&path);
                self.path = path;
                self.scan_profiles();
                self.toast(tr("Saved"));
            }
            Err(e) => self.toast(trf("Saving failed: {}", &[&e])),
        }
    }

    fn capture(&mut self) {
        let Some((c, armed)) = self.cap else { return };
        let phys = self.s.live.lock().unwrap().phys;
        let key = (1..255u16).find(|&k| winio::down(k));
        let pad = Out::ALL.into_iter().find(|&o| pad_down(&phys, o));
        match (armed, key.is_some() || pad.is_some()) {
            (false, false) => self.cap = Some((c, true)), // wait until the click that started capture is released
            (true, true) => {
                let act = key.map(|k| match k { 1 => Act::Mouse(1), 2 => Act::Mouse(2), 4 => Act::Mouse(3), 5 => Act::Mouse(4), 6 => Act::Mouse(5), k => Act::Key(k) });
                match c {
                    Cap::Assign(e, slot) => {
                        if let (Some(a), Some(b)) = (act.or(pad.map(Act::Pad)), self.set().button_mut(e)) {
                            match slot {
                                Some(i) if i < b.slots.len() => b.slots[i] = Slot::Out(a),
                                _ => b.slots = vec![Slot::Out(a)],
                            }
                        }
                    }
                    Cap::KeyBind(i) => {
                        if let (Some(k), Some(kb)) = (key, self.set().keys.get_mut(i)) { kb.vk = k }
                    }
                    Cap::Toggle => self.st.toggle = key.map(Input::Key).or(pad.map(Input::Pad)).unwrap_or(self.st.toggle),
                }
                self.cap = None;
            }
            _ => {}
        }
    }

    fn capturing(&self, c: Cap) -> bool {
        self.cap.is_some_and(|x| x.0 == c)
    }

    // ------------------------------------------------------------- main window parts

    fn header(&mut self, ui: &mut egui::Ui) {
        let on = self.s.on.load(Relaxed);
        // own title bar (the window has no Windows frame): empty space drags, double click maximizes.
        // Registered before the widgets, so buttons on top still get their clicks.
        let row = Rect::from_min_size(ui.cursor().min - vec2(14.0, 10.0), vec2(ui.available_width() + 28.0, 44.0));
        let bar = ui.interact(row, ui.id().with("titlebar"), Sense::click_and_drag());
        let maxed = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        if bar.double_clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maxed));
        } else if bar.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.horizontal(|ui| {
            let c = pal(ui.visuals().dark_mode);
            // blinking block cursor after the name, like an old terminal prompt
            let blink = (ui.input(|i| i.time) * 1.6) as i64 % 2 == 0;
            ui.label(RichText::new(format!("PADMIMIC{}", if blink { "█" } else { " " })).font(mono(20.0)).color(c.acc));
            ui.menu_button(tr("Options"), |ui| {
                if ui.button(tr("Settings…")).clicked() { self.open(Dlg::Settings); ui.close() }
                if ui.button(tr("Latency meter…")).clicked() { self.open(Dlg::Latency); ui.close() }
            });
            ui.menu_button(tr("Help"), |ui| {
                if ui.button(tr("About padmimic")).clicked() { self.open(Dlg::About); ui.close() }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ctx = ui.ctx().clone();
                if win_btn(ui, WinBtn::Close) { ctx.send_viewport_cmd(egui::ViewportCommand::Close) }
                if win_btn(ui, if maxed { WinBtn::Restore } else { WinBtn::Max }) { ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maxed)) }
                if win_btn(ui, WinBtn::Min) { ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)) }
                ui.add_space(10.0);
                if switch(ui, on) { self.s.on.store(!on, Relaxed) }
                let avail = match &*self.upd.lock().unwrap() { Upd::Available(r) => Some(r.version.clone()), _ => None };
                if let Some(v) = avail {
                    let c = pal(ui.visuals().dark_mode);
                    let b = egui::Button::new(RichText::new(format!("⬆ {} v{v}", tr("Update"))).font(mono(12.5)).color(Color32::BLACK)).fill(c.amber);
                    if ui.add(b).clicked() { self.open(Dlg::Update) }
                }
            });
        });
    }

    fn profile_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let dirty = self.p != self.saved;
            let name = if self.path.is_empty() { format!("<{}>", tr("New")) } else { stem(&self.path) };
            let mut pick: Option<String> = None;
            egui::ComboBox::from_id_salt("profile").width(170.0).selected_text(format!("{name}{}", if dirty { " •" } else { "" })).show_ui(ui, |ui| {
                if ui.selectable_label(self.path.is_empty(), format!("<{}>", tr("New"))).clicked() { pick = Some(String::new()) }
                for p in &self.profiles {
                    if ui.selectable_label(*p == self.path, stem(p)).clicked() { pick = Some(p.clone()) }
                }
            });
            match pick {
                Some(p) if p.is_empty() => { self.p = Profile::default(); self.saved = self.p.clone(); self.path.clear() }
                Some(p) => self.load(&p),
                None => {}
            }
            if ui.button(tr("Load")).on_hover_text(tr("Load the profile from disk again")).clicked() && !self.path.is_empty() {
                let p = self.path.clone();
                self.load(&p);
            }
            if ui.button(tr("Save")).clicked() {
                if self.path.is_empty() { self.open(Dlg::SaveAs(String::new())) } else { let p = self.path.clone(); self.save_to(p) }
            }
            if ui.button(tr("Save as")).clicked() { self.open(Dlg::SaveAs(stem(&self.path))) }
            if ui.button(tr("Remove")).on_hover_text(tr("Remove from the list (the file stays)")).clicked() && !self.path.is_empty() {
                let p = self.path.clone();
                self.st.recent.retain(|r| *r != p);
                self.profiles.retain(|r| *r != p);
                self.path.clear();
            }
        });
    }

    fn btn_cell(&mut self, ui: &mut egui::Ui, e: Elem, title: &str, size: Vec2, active: &[Elem]) {
        let set = &self.p.sets[self.cur()];
        let Some(b) = set.button(e) else { return };
        if self.st.hide_empty && b.slots.is_empty() && b.action.is_empty() {
            ui.allocate_exact_size(size, Sense::hover());
            return;
        }
        let used = *b != Button::default();
        let hint = if used { format!("{}\n{}", elem_name(set, e), tr("Right-click: remove assignment")) } else { elem_name(set, e) };
        let r = cell(ui, title, &summary(b), active.contains(&e), size).on_hover_text(hint);
        if r.clicked() {
            self.open(Dlg::Button(e));
        }
        let mut clear = false;
        if used {
            r.context_menu(|ui| {
                if ui.button(format!("🗑 {}", tr("Remove assignment"))).clicked() {
                    clear = true;
                    ui.close();
                }
            });
        }
        if clear {
            if let Some(b) = self.set().button_mut(e) { *b = Button::default() }
        }
    }

    /// 3×3 grid around a stick / d-pad; the centre opens the element dialog.
    fn dir_grid(&mut self, ui: &mut egui::Ui, mk: impl Fn(u8) -> Elem, center: &str, dlg: Dlg, active: &[Elem], centre_active: bool) {
        let w = ((ui.available_width() - 16.0) / 3.0).clamp(60.0, 110.0);
        let sz = vec2(w, 44.0);
        const LAYOUT: [[i8; 3]; 3] = [[7, 0, 1], [6, -1, 2], [5, 4, 3]];
        egui::Grid::new(center).spacing([6.0, 6.0]).show(ui, |ui| {
            for row in LAYOUT {
                for d in row {
                    if d < 0 {
                        if cell(ui, "", center, centre_active, sz).clicked() { self.open(dlg.clone()) }
                    } else {
                        self.btn_cell(ui, mk(d as u8), DIRS[d as usize], sz, active);
                    }
                }
                ui.end_row();
            }
        });
    }

    fn controller_tab(&mut self, ui: &mut egui::Ui, active: &[Elem]) {
        let (phys, out, vigem, hz) = { let l = self.s.live.lock().unwrap(); (l.phys, l.out, l.vigem, l.stats.pad_hz) };
        if self.show_live {
            let sticks = self.p.sets[self.cur()].sticks.clone();
            let trail = &mut self.trail;
            card(ui, &format!("{} · {hz} {}", tr("Live"), tr("upd/s")), |ui| controller(ui, &phys, vigem.then_some(&out), &sticks, trail));
        }
        let full = ui.available_width();
        let cols = if full > 700.0 { 3 } else if full > 460.0 { 2 } else { 1 };
        let colw = (full - (cols - 1) as f32 * 10.0) / cols as f32;
        let blocks: Vec<u8> = vec![0, 1, 2, 3, 4];
        for chunk in blocks.chunks(cols) {
            ui.horizontal_top(|ui| {
                for &b in chunk {
                    ui.allocate_ui(vec2(colw, 0.0), |ui| {
                        ui.set_width(colw);
                        match b {
                            0 | 1 => {
                                let s = b;
                                let sname = self.p.sets[self.cur()].sticks[s as usize].name.clone();
                                card(ui, if sname.is_empty() { tr(STICK_NAMES[s as usize]) } else { &sname }, |ui| {
                                    let centre = active.iter().any(|e| matches!(e, Elem::Stick(x, _) if *x == s));
                                    self.dir_grid(ui, |d| Elem::Stick(s, d), &format!("⚙ {}", tr("Stick")), Dlg::Stick(s), active, centre);
                                    let w = ui.available_width();
                                    self.btn_cell(ui, Elem::StickMod(s), tr("Modifier"), vec2(w, 40.0), active);
                                });
                            }
                            2 => {
                                let name = self.p.sets[self.cur()].dpad.name.clone();
                                card(ui, if name.is_empty() { "D-Pad" } else { &name }, |ui| {
                                    let centre = active.iter().any(|e| matches!(e, Elem::DPad(_)));
                                    self.dir_grid(ui, Elem::DPad, "⚙ D-Pad", Dlg::DPad, active, centre);
                                });
                            }
                            3 => {
                                card(ui, tr("Triggers"), |ui| {
                                    for t in 0..2u8 {
                                        let a = &self.p.sets[self.cur()].triggers[t as usize];
                                        let neg = a.throttle < 0;
                                        let title = if a.name.is_empty() { TRIG_NAMES[t as usize].to_string() } else { a.name.clone() };
                                        let analog = match a.analog { Some(VTrigger::Left) => " → 🎮 LT", Some(VTrigger::Right) => " → 🎮 RT", None => "" };
                                        ui.horizontal(|ui| {
                                            let pressed = active.contains(&Elem::Trig(t, true)) || active.contains(&Elem::Trig(t, false));
                                            if cell(ui, "", &format!("⚙ {title}{analog}"), pressed, vec2(ui.available_width() * 0.5, 44.0)).clicked() { self.open(Dlg::Axis(t)) }
                                            let w = ui.available_width();
                                            if neg {
                                                self.btn_cell(ui, Elem::Trig(t, false), "−", vec2(w / 2.0 - 4.0, 44.0), active);
                                                self.btn_cell(ui, Elem::Trig(t, true), "+", vec2(w / 2.0 - 4.0, 44.0), active);
                                            } else {
                                                self.btn_cell(ui, Elem::Trig(t, true), tr("Press"), vec2(w, 44.0), active);
                                            }
                                        });
                                    }
                                });
                            }
                            _ => {
                                card(ui, tr("Buttons"), |ui| {
                                    let w = ((ui.available_width() - 12.0) / 3.0).max(60.0);
                                    egui::Grid::new("buttons").spacing([6.0, 6.0]).show(ui, |ui| {
                                        for i in 0..11u8 {
                                            self.btn_cell(ui, Elem::Btn(i), BTN_NAMES[i as usize], vec2(w, 44.0), active);
                                            if i % 3 == 2 { ui.end_row() }
                                        }
                                    });
                                });
                            }
                        }
                    });
                }
            });
            ui.add_space(4.0);
        }
    }

    fn kbm_tab(&mut self, ui: &mut egui::Ui, active: &[Elem]) {
        card(ui, tr("Keyboard & mouse → assignment"), |ui| {
            ui.weak(tr("Real keys / mouse buttons trigger actions – e.g. controller buttons for mouse & keyboard players."));
            let mut remove = None;
            let n = self.p.sets[self.cur()].keys.len();
            for i in 0..n {
                ui.horizontal(|ui| {
                    let vk = self.p.sets[self.cur()].keys[i].vk;
                    let label = if self.capturing(Cap::KeyBind(i)) { tr("Press a key…").into() } else { format!("⌨ {}", winio::key_name(vk)) };
                    if ui.add(egui::Button::new(label).min_size(vec2(130.0, 30.0))).clicked() { self.cap = Some((Cap::KeyBind(i), false)) }
                    ui.label("→");
                    let w = (ui.available_width() - 36.0).max(80.0);
                    self.btn_cell(ui, Elem::Key(i as u16), "", vec2(w, 30.0), active);
                    if ui.small_button("×").clicked() { remove = Some(i) }
                });
            }
            if let Some(i) = remove {
                self.set().keys.remove(i);
                self.cap = None;
            }
            if ui.button(format!("＋ {}", tr("Assignment"))).clicked() {
                self.set().keys.push(KeyBind { vk: 0x20, btn: Button::out(Act::Pad(Out::A)) });
            }
        });
        ui.add_space(6.0);
        let aim = &mut self.p.mouse_aim;
        card(ui, tr("Mouse → stick"), |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut aim.enabled, tr("Active"));
                ui.selectable_value(&mut aim.stick, VStick::Right, tr("Right stick"));
                ui.selectable_value(&mut aim.stick, VStick::Left, tr("Left stick"));
            });
            rg(tr("Sensitivity")).show(ui, &mut aim.sens, 0.05..=5.0);
            rg(tr("Smoothing")).show(ui, &mut aim.smooth, 0.0..=0.95);
            rg(tr("Anti-deadzone")).show(ui, &mut aim.deadzone, 0.0..=0.5);
            rg(tr("Sample window")).log().suffix(" ms").show(ui, &mut aim.window_ms, 0.125..=8.0)
                .on_hover_text(tr("Time the mouse movement is averaged over. ≈ 1000 ÷ mouse polling rate: 1 ms at 1000 Hz, 2 ms at 500 Hz, 0.25 ms at 4000 Hz. Smaller = more direct, too small = jittery."));
        });
    }

    fn bottom(&mut self, ui: &mut egui::Ui) {
        let cur = self.cur();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("Sets")).weak());
            for i in 0..8 {
                let name = &self.p.set_names[i];
                let r = ui.selectable_label(cur == i, format!("{}", i + 1)).on_hover_text(if name.is_empty() { format!("{} {}", tr("Set"), i + 1) } else { name.clone() });
                if r.clicked() { *self.s.set_cmd.lock().unwrap() = Some(i) }
            }
            ui.separator();
            if ui.button(tr("Quick set")).clicked() {
                let held = self.s.live.lock().unwrap().active.clone();
                self.open(Dlg::QuickSet(held));
            }
            if ui.button(tr("Names")).clicked() { self.open(Dlg::Names) }
            if ui.button(tr("Profile")).clicked() { self.open(Dlg::Pref) }
            if ui.button(tr("Reset")).clicked() { self.open(Dlg::Reset) }
        });
        let (st, status, vigem) = { let l = self.s.live.lock().unwrap(); (l.stats, l.status.clone(), l.vigem) };
        if !vigem && status == engine::VIGEM_MISSING {
            let msg = self.vigem_msg.lock().unwrap().clone();
            let busy = msg.as_deref() == Some(INSTALLING);
            ui.horizontal(|ui| {
                let b = ui.add_enabled(!busy, egui::Button::new(tr(if busy { INSTALLING } else { "Install ViGEmBus" })))
                    .on_hover_text(tr("Driver for the virtual Xbox controller (by Nefarius, signed). Windows asks once for admin rights. padmimic connects by itself afterwards."));
                if b.clicked() {
                    let m = self.vigem_msg.clone();
                    *m.lock().unwrap() = Some(INSTALLING.into());
                    std::thread::spawn(move || {
                        let r = match winio::install_vigem() {
                            Ok(false) => "ViGEmBus installed – connecting in a few seconds".into(),
                            Ok(true) => "ViGEmBus installed – please restart Windows".into(),
                            Err(e) => e,
                        };
                        *m.lock().unwrap() = Some(r);
                    });
                }
                if let Some(m) = msg.filter(|m| m != INSTALLING) { ui.small(tr(&m)); }
            });
        }
        ui.horizontal(|ui| {
            ui.small(tr(&status));
            ui.label(RichText::new(format!(
                "PAD {} · {} UPD/S · ENGINE {} HZ · VIRT {}",
                st.slot.map_or("–".into(), |i| format!("#{}", i + 1)), st.pad_hz, st.loop_hz,
                st.virt_slot.map_or("–".into(), |i| format!("#{}", i + 1))
            )).font(mono(11.0)).color(pal(ui.visuals().dark_mode).weak));
            if let Some((t, at)) = &self.toast {
                if at.elapsed() < Duration::from_secs(3) { ui.small(RichText::new(t.as_str()).color(accent(ui.visuals().dark_mode))); }
            }
        });
    }

    // ------------------------------------------------------------- dialogs

    fn dialogs(&mut self, ctx: &egui::Context) {
        let list = self.dlgs.clone();
        let mut close = vec![];
        for (i, d) in list.iter().enumerate() {
            if *d == Dlg::Latency {
                if !self.latency_window(ctx) { close.push(d.clone()) }
                continue;
            }
            let mut open = true;
            let (title, width) = match d {
                Dlg::Button(e) => (format!("{} – {}", tr("Assignment"), elem_name(&self.p.sets[self.cur()], *e)), 640.0),
                Dlg::Advanced(e) => (format!("{} – {}", tr("Advanced"), elem_name(&self.p.sets[self.cur()], *e)), 660.0),
                Dlg::Stick(s) => (tr(STICK_NAMES[*s as usize]).to_string(), 560.0),
                Dlg::DPad => ("D-Pad".into(), 380.0),
                Dlg::Axis(t) => (format!("{} {}", tr("Axis"), TRIG_NAMES[*t as usize]), 420.0),
                Dlg::Mouse(_) => (tr("Mouse settings").into(), 760.0),
                Dlg::Names => (tr("Set names").into(), 320.0),
                Dlg::Pref => (tr("Profile settings").into(), 380.0),
                Dlg::QuickSet(_) => (tr("Quick set").into(), 320.0),
                Dlg::Settings => (tr("Settings").into(), 640.0),
                Dlg::SaveAs(_) => (tr("Save as").into(), 320.0),
                Dlg::Reset => (tr("Reset").into(), 360.0),
                Dlg::About => (tr("About padmimic").into(), 360.0),
                Dlg::Latency => (tr("Latency meter").into(), 560.0),
                Dlg::Update => (tr("Update").into(), 380.0),
            };
            let mut done = false;
            egui::Window::new(title)
                .id(egui::Id::new(("dlg", i, format!("{:?}", std::mem::discriminant(d)))))
                .open(&mut open)
                .collapsible(false)
                .auto_sized() // grows with its content, so nothing is cut off
                .min_width(width)
                .pivot(Align2::CENTER_TOP)
                .default_pos(Pos2::new(ctx.content_rect().center().x + i as f32 * 18.0, 40.0 + i as f32 * 18.0))
                .show(ctx, |ui| {
                    let max_h = ctx.content_rect().height() - 110.0;
                    egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| done = match d.clone() {
                        Dlg::Button(e) => self.dlg_button(ui, e),
                        Dlg::Advanced(e) => self.dlg_advanced(ui, e),
                        Dlg::Stick(s) => self.dlg_stick(ui, s),
                        Dlg::DPad => self.dlg_dpad(ui),
                        Dlg::Axis(t) => self.dlg_axis(ui, t),
                        Dlg::Mouse(t) => self.dlg_mouse(ui, t),
                        Dlg::Names => self.dlg_names(ui),
                        Dlg::Pref => self.dlg_pref(ui),
                        Dlg::QuickSet(held) => self.dlg_quickset(ui, &held),
                        Dlg::Settings => self.dlg_settings(ui),
                        Dlg::SaveAs(_) => self.dlg_save_as(ui, i),
                        Dlg::Reset => self.dlg_reset(ui),
                        Dlg::About => self.dlg_about(ui),
                        Dlg::Latency => true, // own window, see latency_window
                        Dlg::Update => self.dlg_update(ui),
                    });
                });
            if !open || done { close.push(d.clone()) }
        }
        self.dlgs.retain(|d| !close.contains(d));
    }

    fn ok_row(ui: &mut egui::Ui) -> bool {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::Button::new(RichText::new(tr("Done")).color(Color32::WHITE)).fill(accent(ui.visuals().dark_mode)).min_size(vec2(84.0, 28.0))).clicked()
            })
            .inner
        })
        .inner
    }

    fn dlg_button(&mut self, ui: &mut egui::Ui, e: Elem) -> bool {
        let numpad = self.st.numpad;
        let capturing = self.capturing(Cap::Assign(e, None));
        let mut open_adv = false;
        let mut open_mouse = false;
        let mut tab = self.pick_tab;
        let Some(b) = self.set().button_mut(e) else { return true };
        ui.weak(tr("Click a key, mouse or controller output – or “Record” and press a real key or controller button."));
        let first = b.slots.iter().find_map(|s| if let Slot::Out(a) = s { Some(*a) } else { None });
        if let Some(a) = act_picker(ui, &mut tab, first, numpad) {
            b.slots = vec![Slot::Out(a)];
        }
        ui.separator();
        let mut cap = false;
        ui.horizontal(|ui| {
            cap = ui.button(if capturing { tr("Press…").to_string() } else { format!("⏺ {}", tr("Record")) }).clicked();
            // everything off: outputs, turbo, toggle, set change, name
            if ui.button(format!("🗑 {}", tr("Remove"))).on_hover_text(tr("Remove the whole assignment")).clicked() { *b = Button::default() }
            ui.checkbox(&mut b.toggle, tr("Toggle"));
            ui.checkbox(&mut b.turbo, tr("Turbo"));
        });
        egui::Grid::new("names").num_columns(2).show(ui, |ui| {
            ui.label(tr("Name:"));
            ui.text_edit_singleline(&mut b.name);
            ui.end_row();
            ui.label(tr("Action:"));
            ui.text_edit_singleline(&mut b.action);
            ui.end_row();
            ui.label(tr("Slots:"));
            ui.label(RichText::new(summary(&Button { action: String::new(), ..b.clone() })).strong());
            ui.end_row();
        });
        let done = ui.horizontal(|ui| {
            open_adv = ui.button(tr("Advanced…")).clicked();
            open_mouse = ui.button(tr("Mouse settings…")).clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| Self::ok_row(ui)).inner
        }).inner;
        self.pick_tab = tab;
        if cap { self.cap = Some((Cap::Assign(e, None), false)) }
        if open_adv { self.adv_sel = 0; self.open(Dlg::Advanced(e)) }
        if open_mouse { self.open(Dlg::Mouse(MouseTarget::Btn(e))) }
        done
    }

    fn dlg_advanced(&mut self, ui: &mut egui::Ui, e: Elem) -> bool {
        let numpad = self.st.numpad;
        let profiles = self.profiles.clone();
        let set_names = self.p.set_names.clone();
        let mut page = self.adv_page;
        let mut sel = self.adv_sel;
        let mut tab = self.pick_tab;
        let mut cap_slot = None;
        let capturing_slot = match self.cap { Some((Cap::Assign(x, Some(i)), _)) if x == e => Some(i), _ => None };
        let Some(b) = self.set().button_mut(e) else { return true };
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(120.0);
                for (i, l) in ["Assignments", "Toggle", "Turbo", "Set selection"].into_iter().enumerate() {
                    ui.selectable_value(&mut page, i as u8, tr(l));
                }
            });
            ui.add_space(10.0); // no vertical separator: it takes all available height and stretches the dialog
            ui.vertical(|ui| match page {
                0 => {
                    egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                        if b.slots.is_empty() { ui.weak(tr("No slots")); }
                        for (i, s) in b.slots.iter().enumerate() {
                            if ui.selectable_label(sel == i, format!("{}. {}", i + 1, slot_text(s))).clicked() { sel = i }
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button(tr("Insert")).clicked() {
                            let at = (sel + 1).min(b.slots.len());
                            b.slots.insert(at, Slot::Out(Act::Key(0x20)));
                            sel = at;
                        }
                        if ui.button(tr("Delete")).clicked() && sel < b.slots.len() {
                            b.slots.remove(sel);
                            sel = sel.saturating_sub(1);
                        }
                        if ui.button(tr("Delete all")).clicked() { b.slots.clear() }
                        if ui.button("↑").clicked() && sel > 0 && sel < b.slots.len() { b.slots.swap(sel, sel - 1); sel -= 1 }
                        if ui.button("↓").clicked() && sel + 1 < b.slots.len() { b.slots.swap(sel, sel + 1); sel += 1 }
                    });
                    if let Some(s) = b.slots.get_mut(sel) {
                        ui.separator();
                        let mut ty = s.type_index();
                        egui::ComboBox::from_label(tr("Slot type")).selected_text(tr(Slot::TYPES[ty])).show_ui(ui, |ui| {
                            for (i, t) in Slot::TYPES.iter().enumerate() { ui.selectable_value(&mut ty, i, tr(t)); }
                        });
                        if ty != s.type_index() { *s = Slot::from_type(ty) }
                        let (help, _) = slot_help(s);
                        ui.weak(tr(help));
                        match s {
                            Slot::Out(a) => {
                                if let Some(n) = act_picker(ui, &mut tab, Some(*a), numpad) { *a = n }
                                if ui.button(if capturing_slot == Some(sel) { tr("Press…").to_string() } else { format!("⏺ {}", tr("Record")) }).clicked() { cap_slot = Some(sel) }
                            }
                            Slot::Pause(ms) => { ms_field(ui, tr("Time"), ms, 600_000); }
                            Slot::Hold(ms) => { ms_field(ui, tr("Time"), ms, 600_000); }
                            Slot::Delay(ms) => { ms_field(ui, tr("Time"), ms, 600_000); }
                            Slot::Release(ms) => { ms_field(ui, tr("Time"), ms, 600_000); }
                            Slot::PressTime(ms) => { ms_field(ui, tr("Time"), ms, 600_000); }
                            Slot::Distance(p) => { rg(tr("Distance")).suffix(" %").show(ui, p, 1..=100); }
                            Slot::MouseMod(p) => { rg(tr("Mouse speed")).suffix(" %").show(ui, p, 1..=400); }
                            Slot::Cycle => {
                                let mut on = b.cycle_reset.is_some();
                                let mut v = b.cycle_reset.unwrap_or(1000) as f32 / 1000.0;
                                ui.checkbox(&mut on, tr("Reset the cycle automatically"));
                                ui.add_enabled_ui(on, |ui| rg(tr("Reset after")).suffix(" s").log().show(ui, &mut v, 0.01..=60.0));
                                b.cycle_reset = on.then_some((v * 1000.0) as u32);
                            }
                            Slot::SetChange(n) => {
                                egui::ComboBox::from_label(tr("Set")).selected_text(set_label(&set_names, *n as usize)).show_ui(ui, |ui| {
                                    for i in 0..8u8 { ui.selectable_value(n, i, set_label(&set_names, i as usize)); }
                                });
                            }
                            Slot::Text(t) => { ui.text_edit_singleline(t); }
                            Slot::Exec { path, args } => {
                                egui::Grid::new("exec").num_columns(2).show(ui, |ui| {
                                    ui.label(tr("Program:"));
                                    ui.text_edit_singleline(path);
                                    ui.end_row();
                                    ui.label(tr("Arguments:"));
                                    ui.text_edit_singleline(args);
                                    ui.end_row();
                                });
                            }
                            Slot::Load(p) => {
                                egui::ComboBox::from_label(tr("Profile")).selected_text(stem(p)).show_ui(ui, |ui| {
                                    for pr in &profiles { ui.selectable_value(p, pr.clone(), stem(pr)); }
                                });
                            }
                        }
                    }
                }
                1 => {
                    ui.checkbox(&mut b.toggle, tr("Enabled"));
                    ui.weak(tr("First press holds the assignment down, the next one releases it."));
                }
                2 => {
                    ui.checkbox(&mut b.turbo, tr("Enabled"));
                    let mut m = b.turbo_mode.unwrap_or(TurboMode::Normal);
                    egui::ComboBox::from_label(tr("Mode")).selected_text(tr(match m { TurboMode::Normal => "Normal", TurboMode::Gradient => "Gradient", TurboMode::Pulse => "Pulse" })).show_ui(ui, |ui| {
                        ui.selectable_value(&mut m, TurboMode::Normal, tr("Normal"));
                        ui.selectable_value(&mut m, TurboMode::Gradient, tr("Gradient"));
                        ui.selectable_value(&mut m, TurboMode::Pulse, tr("Pulse"));
                    });
                    b.turbo_mode = (m != TurboMode::Normal).then_some(m);
                    let mut ms = b.turbo_interval();
                    let l = format!("{}  ·  {} {:.1}/s", tr("Delay"), tr("Rate"), 1000.0 / ms as f32);
                    secs_slider(ui, &mut ms, 2, 4000, l);
                    if ms != b.turbo_interval() { b.turbo_ms = ms } // just looking keeps 0 = default
                    ui.weak(tr("Gradient: press length follows the axis travel. Pulse: frequency follows the axis travel."));
                }
                _ => {
                    let label = |v: Option<(u8, SetCond)>| match v {
                        None => tr("Disabled").to_string(),
                        Some((s, c)) => format!("{} {}", set_label(&set_names, s as usize), tr(match c { SetCond::OneWay => "one-way", SetCond::TwoWay => "two-way", SetCond::WhileHeld => "while held" })),
                    };
                    egui::ComboBox::from_label(tr("Change set")).width(240.0).selected_text(label(b.set)).show_ui(ui, |ui| {
                        ui.selectable_value(&mut b.set, None, tr("Disabled"));
                        for s in 0..8u8 {
                            for c in [SetCond::OneWay, SetCond::TwoWay, SetCond::WhileHeld] {
                                ui.selectable_value(&mut b.set, Some((s, c)), label(Some((s, c))));
                            }
                        }
                    });
                    ui.weak(tr("One-way: switches on release. Two-way: the same button switches back in the target set. While held: back on release."));
                }
            });
        });
        self.adv_page = page;
        self.adv_sel = sel;
        self.pick_tab = tab;
        if let Some(i) = cap_slot { self.cap = Some((Cap::Assign(e, Some(i)), false)) }
        Self::ok_row(ui)
    }

    fn presets(ui: &mut egui::Ui, id: &str, items: &[&str]) -> Option<usize> {
        let mut pick = None;
        egui::ComboBox::new(id, tr("Presets")).selected_text(tr("Choose…")).show_ui(ui, |ui| {
            for (i, it) in items.iter().enumerate() {
                if ui.selectable_label(false, tr(it)).clicked() { pick = Some(i) }
            }
        });
        pick
    }

    fn apply_dir_preset(dirs: &mut [Button; 8], i: usize) -> Option<()> {
        let m = |d| Act::Move(d);
        let k = |c: u8| Act::Key(c as u16);
        // (up, down, left, right, diagonals ↗ ↘ ↙ ↖)
        let (u, d, l, r, diag): (Act, Act, Act, Act, Option<[Act; 4]>) = match i {
            0 => (m(1), m(2), m(3), m(4), None),
            1 => (m(1), m(2), m(4), m(3), None),
            2 => (m(2), m(1), m(3), m(4), None),
            3 => (m(2), m(1), m(4), m(3), None),
            4 => (k(0x26), k(0x28), k(0x25), k(0x27), None),
            5 => (k(b'W'), k(b'S'), k(b'A'), k(b'D'), None),
            6 => (k(0x68), k(0x62), k(0x64), k(0x66), Some([k(0x69), k(0x63), k(0x61), k(0x67)])),
            _ => { *dirs = Default::default(); return None }
        };
        *dirs = Default::default();
        dirs[UP] = Button::out(u);
        dirs[DOWN] = Button::out(d);
        dirs[LEFT] = Button::out(l);
        dirs[RIGHT] = Button::out(r);
        if let Some(dg) = diag {
            for (slot, a) in [1, 3, 5, 7].into_iter().zip(dg) { dirs[slot] = Button::out(a) }
        }
        Some(())
    }

    fn dir_mode_combo(ui: &mut egui::Ui, id: &str, m: &mut DirMode) {
        egui::ComboBox::new(id, tr("Mode")).selected_text(tr(DirMode::ALL.iter().find(|x| x.0 == *m).unwrap().1)).show_ui(ui, |ui| {
            for (v, l) in DirMode::ALL { ui.selectable_value(m, v, tr(l)); }
        });
    }

    fn dlg_stick(&mut self, ui: &mut egui::Ui, si: u8) -> bool {
        let phys = self.s.live.lock().unwrap().phys;
        let (x, y) = if si == 0 { (phys.thumb_lx, phys.thumb_ly) } else { (phys.thumb_rx, phys.thumb_ry) };
        let mut open_mod = false;
        let mut open_mouse = false;
        let s = &mut self.set().sticks[si as usize];
        const PRESETS: [&str; 10] = ["Mouse (normal)", "Mouse (horizontal inverted)", "Mouse (vertical inverted)", "Mouse (both inverted)", "Arrow keys", "W | A | S | D", "Numpad", "None", "🎮 Virtual left stick", "🎮 Virtual right stick"];
        if let Some(i) = Self::presets(ui, "stickpre", &PRESETS) {
            match i {
                8 => { s.dirs = Default::default(); s.analog = Some(VStick::Left) }
                9 => { s.dirs = Default::default(); s.analog = Some(VStick::Right) }
                i => { Self::apply_dir_preset(&mut s.dirs, i); s.analog = None }
            }
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                stick_status(ui, s, x, y);
                let e = eval_stick(s, x as i32, y as i32);
                egui::Grid::new("stickvals").num_columns(2).show(ui, |ui| {
                    ui.weak("X / Y:"); ui.label(format!("{x} / {y}")); ui.end_row();
                    ui.weak(tr("Distance:")); ui.label(format!("{:.0}", e.raw)); ui.end_row();
                    ui.weak(tr("Direction:")); ui.label(format!("{:.0}°", e.bearing)); ui.end_row();
                    ui.weak(tr("% safe zone:")); ui.label(format!("{:.0} %", e.radial * 100.0)); ui.end_row();
                });
            });
            ui.vertical(|ui| {
                Self::dir_mode_combo(ui, "stickmode", &mut s.mode);
                rg(tr("Dead zone")).show(ui, &mut s.dead, 1..=32737);
                rg(tr("Max zone")).show(ui, &mut s.max, 1..=32737);
                ui.add_enabled_ui(matches!(s.mode, DirMode::Standard | DirMode::EightWay), |ui| rg(tr("Diagonal range")).suffix("°").show(ui, &mut s.diag, 1..=90));
                rg(tr("Square stick")).suffix(" %").show(ui, &mut s.square, 0..=100);
                rg(tr("Stick delay")).suffix(" ms").show(ui, &mut s.delay, 0..=1000);
                rg(tr("Modifier zone")).show(ui, &mut s.mod_zone, 1..=32737);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut s.mod_inv, tr("Invert modifier zone"));
                    open_mod = ui.button(format!("{}: {}", tr("Modifier"), summary(&s.modifier))).clicked();
                });
                ui.horizontal(|ui| {
                    ui.label(format!("{} →", tr("Analog")));
                    ui.selectable_value(&mut s.analog, None, tr("Off"));
                    ui.selectable_value(&mut s.analog, Some(VStick::Left), "🎮 L-Stick");
                    ui.selectable_value(&mut s.analog, Some(VStick::Right), "🎮 R-Stick");
                });
                ui.horizontal(|ui| {
                    ui.label(tr("Name:"));
                    ui.text_edit_singleline(&mut s.name);
                });
            });
        });
        let done = ui.horizontal(|ui| {
            open_mouse = ui.button(tr("Mouse settings…")).clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| Self::ok_row(ui)).inner
        }).inner;
        if open_mod { self.open(Dlg::Button(Elem::StickMod(si))) }
        if open_mouse { self.open(Dlg::Mouse(MouseTarget::Stick(si))) }
        done
    }

    fn dlg_dpad(&mut self, ui: &mut egui::Ui) -> bool {
        let mut open_mouse = false;
        let d = &mut self.set().dpad;
        const PRESETS: [&str; 8] = ["Mouse (normal)", "Mouse (horizontal inverted)", "Mouse (vertical inverted)", "Mouse (both inverted)", "Arrow keys", "W | A | S | D", "Numpad", "None"];
        if let Some(i) = Self::presets(ui, "dpadpre", &PRESETS) { Self::apply_dir_preset(&mut d.dirs, i); }
        let mut m = d.mode.unwrap_or(DirMode::Standard);
        Self::dir_mode_combo(ui, "dpadmode", &mut m);
        d.mode = (m != DirMode::Standard).then_some(m);
        rg(tr("D-pad delay")).suffix(" ms").show(ui, &mut d.delay, 0..=1000);
        ui.horizontal(|ui| { ui.label(tr("Name:")); ui.text_edit_singleline(&mut d.name); });
        let done = ui.horizontal(|ui| {
            open_mouse = ui.button(tr("Mouse settings…")).clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| Self::ok_row(ui)).inner
        }).inner;
        if open_mouse { self.open(Dlg::Mouse(MouseTarget::DPad)) }
        done
    }

    fn dlg_axis(&mut self, ui: &mut egui::Ui, t: u8) -> bool {
        let raw = { let l = self.s.live.lock().unwrap(); if t == 0 { l.phys.left_trigger } else { l.phys.right_trigger } };
        let (mut open_p, mut open_n, mut open_mouse) = (false, false, false);
        let a = &mut self.set().triggers[t as usize];
        const PRESETS: [&str; 13] = ["Mouse (horizontal)", "Mouse (horizontal inverted)", "Mouse (vertical)", "Mouse (vertical inverted)", "Arrows ↑ | ↓", "Arrows ← | →", "W | S", "A | D", "Num 8 | 2", "Num 4 | 6", "None", "🎮 Virtual LT", "🎮 Virtual RT"];
        if let Some(i) = Self::presets(ui, "axispre", &PRESETS) {
            let k = |c: u16| Act::Key(c);
            let pair = match i {
                0 => Some((Act::Move(3), Act::Move(4))), 1 => Some((Act::Move(4), Act::Move(3))),
                2 => Some((Act::Move(1), Act::Move(2))), 3 => Some((Act::Move(2), Act::Move(1))),
                4 => Some((k(0x26), k(0x28))), 5 => Some((k(0x25), k(0x27))), 6 => Some((k(b'W' as u16), k(b'S' as u16))),
                7 => Some((k(b'A' as u16), k(b'D' as u16))), 8 => Some((k(0x68), k(0x62))), 9 => Some((k(0x64), k(0x66))),
                _ => None,
            };
            a.analog = match i { 11 => Some(VTrigger::Left), 12 => Some(VTrigger::Right), _ => None };
            let (n, p) = pair.map_or((Button::default(), Button::default()), |(n, p)| (Button::out(n), Button::out(p)));
            a.n = n;
            a.p = p;
        }
        let names = ["Negative half", "Negative", "Normal", "Positive", "Positive half"];
        egui::ComboBox::from_label(tr("Throttle")).selected_text(tr(names[(a.throttle + 2) as usize])).show_ui(ui, |ui| {
            for (i, n) in names.iter().enumerate() { ui.selectable_value(&mut a.throttle, i as i8 - 2, tr(n)); }
        });
        rg(tr("Dead zone")).show(ui, &mut a.dead, 0..=32737);
        rg(tr("Max zone")).show(ui, &mut a.max, 1..=32737);
        let v = engine::throttle(raw as i32 * 32767 / 255, a.throttle);
        ui.horizontal(|ui| {
            ui.label(tr("Current value:"));
            ui.add(egui::ProgressBar::new((v as f32 + 32767.0) / 65534.0).text(format!("{v}")).desired_width(220.0));
        });
        ui.horizontal(|ui| {
            open_n = ui.button(format!("− {}", summary(&a.n))).clicked();
            open_p = ui.button(format!("+ {}", summary(&a.p))).clicked();
        });
        ui.horizontal(|ui| {
            ui.label(format!("{} →", tr("Analog")));
            ui.selectable_value(&mut a.analog, None, tr("Off"));
            ui.selectable_value(&mut a.analog, Some(VTrigger::Left), "🎮 LT");
            ui.selectable_value(&mut a.analog, Some(VTrigger::Right), "🎮 RT");
        });
        ui.horizontal(|ui| { ui.label(tr("Name:")); ui.text_edit_singleline(&mut a.name); });
        let done = ui.horizontal(|ui| {
            open_mouse = ui.button(tr("Mouse settings…")).clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| Self::ok_row(ui)).inner
        }).inner;
        if open_p { self.open(Dlg::Button(Elem::Trig(t, true))) }
        if open_n { self.open(Dlg::Button(Elem::Trig(t, false))) }
        if open_mouse { self.open(Dlg::Mouse(MouseTarget::Axis(t))) }
        done
    }

    fn mouse_targets(t: MouseTarget) -> Vec<Elem> {
        match t {
            MouseTarget::Btn(e) => vec![e],
            MouseTarget::Stick(s) => (0..8).map(|d| Elem::Stick(s, d)).collect(),
            MouseTarget::DPad => (0..8).map(Elem::DPad).collect(),
            MouseTarget::Axis(a) => vec![Elem::Trig(a, true), Elem::Trig(a, false)],
        }
    }

    fn dlg_mouse(&mut self, ui: &mut egui::Ui, t: MouseTarget) -> bool {
        let targets = Self::mouse_targets(t);
        let pps = self.s.live.lock().unwrap().pps;
        let Some(mut m) = self.set().button(targets[0]).map(|b| b.mouse.clone()) else { return true };
        let before = m.clone();
        egui::Grid::new("mtop").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Mode:"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut m.mode, MouseMode::Cursor, tr("Cursor"));
                ui.selectable_value(&mut m.mode, MouseMode::Spring, tr("Spring"));
            });
            ui.end_row();
            ui.label(tr("Curve:"));
            egui::ComboBox::from_id_salt("curve").selected_text(tr(Curve::ALL.iter().find(|c| c.0 == m.curve).unwrap().1)).show_ui(ui, |ui| {
                for (c, l) in Curve::ALL { ui.selectable_value(&mut m.curve, c, tr(l)); }
            });
            ui.end_row();
        });
        // two columns: the dialog fits without scrolling
        ui.columns(2, |cols| {
        let (left, right) = cols.split_at_mut(1);
        let (ui, ru) = (&mut left[0], &mut right[0]);
        card(ui, tr("Mouse speed"), |ui| {
            let mut together = m.speed_x == m.speed_y;
            ui.checkbox(&mut together, tr("Change together"));
            let ox = m.speed_x;
            let (lx, ly) = (format!("{} ({} px/s)", tr("Horizontal"), m.speed_x as u32 * 20), format!("{} ({} px/s)", tr("Vertical"), m.speed_y as u32 * 20));
            rg(&lx).show(ui, &mut m.speed_x, 1..=300);
            rg(&ly).show(ui, &mut m.speed_y, 1..=300);
            if together { if m.speed_x != ox { m.speed_y = m.speed_x } else { m.speed_x = m.speed_y } }
            rg(tr("Wheel horizontal")).suffix(tr(" notches/s")).show(ui, &mut m.wheel_x, 1..=100);
            rg(tr("Wheel vertical")).suffix(tr(" notches/s")).show(ui, &mut m.wheel_y, 1..=100);
            ui.add_enabled_ui(m.curve == Curve::Power, |ui| rg(tr("Sensitivity")).log().show(ui, &mut m.sens, 0.001..=1000.0));
            ui.add_enabled_ui(matches!(m.curve, Curve::EasingQuadratic | Curve::EasingCubic), |ui| rg(tr("Easing duration")).suffix(" s").show(ui, &mut m.easing, 0.2..=5.0));
        });
        card(ru, "", |ui| {
            ui.checkbox(&mut m.accel, RichText::new(tr("Delta acceleration")).strong());
            ui.add_enabled_ui(m.accel, |ui| {
                rg(tr("Multiplier")).suffix("×").show(ui, &mut m.accel_mult, 1.0..=200.0);
                rg(tr("Start")).suffix(" %").show(ui, &mut m.accel_start, 0.0..=100.0);
                rg(tr("Min threshold")).suffix(" %").show(ui, &mut m.accel_min, 0.0..=100.0);
                rg(tr("Max threshold")).suffix(" %").show(ui, &mut m.accel_max, 0.0..=100.0);
                rg(tr("Extra duration")).suffix(" s").show(ui, &mut m.accel_dur, 0.0..=5.0);
                ui.horizontal(|ui| {
                    ui.label(tr("Curve:"));
                    for (c, l) in [(AccelCurve::Linear, "Linear"), (AccelCurve::EaseOutSine, "Sine"), (AccelCurve::EaseOutQuad, "Quad"), (AccelCurve::EaseOutCubic, "Cubic")] {
                        ui.selectable_value(&mut m.accel_curve, c, tr(l));
                    }
                });
            });
        });
        ru.add_space(6.0);
        card(ru, tr("Spring"), |ui| {
            ui.add_enabled_ui(m.mode == MouseMode::Spring, |ui| {
                rg(tr("Width")).suffix(" px").show(ui, &mut m.spring_w, 0..=16384);
                rg(tr("Height")).suffix(" px").show(ui, &mut m.spring_h, 0..=16384);
                ui.weak(tr("Width/height 0 = whole screen"));
                rg(tr("Release radius")).suffix(" %").show(ui, &mut m.spring_release, 0..=100);
                ui.checkbox(&mut m.spring_rel, tr("Relative"));
            });
        });
        });
        ui.weak(format!("{}: X {:.0} px/s · Y {:.0} px/s", tr("Mouse status"), pps.0, pps.1));
        if m != before {
            for e in targets {
                if let Some(b) = self.set().button_mut(e) { b.mouse = m.clone() }
            }
        }
        Self::ok_row(ui)
    }

    fn dlg_names(&mut self, ui: &mut egui::Ui) -> bool {
        egui::Grid::new("setnames").num_columns(2).show(ui, |ui| {
            for i in 0..8 {
                ui.label(format!("{} {}", tr("Set"), i + 1));
                ui.text_edit_singleline(&mut self.p.set_names[i]);
                ui.end_row();
            }
        });
        Self::ok_row(ui)
    }

    fn dlg_pref(&mut self, ui: &mut egui::Ui) -> bool {
        secs_slider(ui, &mut self.p.key_press_ms, 1, 1000, tr("Key press time").into());
        ui.horizontal(|ui| { ui.label(tr("Profile name:")); ui.text_edit_singleline(&mut self.p.name); });
        Self::ok_row(ui)
    }

    fn dlg_quickset(&mut self, ui: &mut egui::Ui, held: &[Elem]) -> bool {
        ui.label(tr("Press a button or move an axis on the controller (or an assigned key). The matching editor opens."));
        ui.ctx().request_repaint();
        let active = self.s.live.lock().unwrap().active.clone();
        if let Some(e) = active.into_iter().find(|e| !held.contains(e)) {
            self.open(Dlg::Button(e));
            return true;
        }
        ui.add_space(4.0);
        ui.button(tr("Cancel")).clicked()
    }

    fn dlg_settings(&mut self, ui: &mut egui::Ui) -> bool {
        let mut page = self.set_page;
        let toggle_cap = self.capturing(Cap::Toggle);
        let mut cap_toggle = false;
        let mut pick_lang = None;
        let mut check_now = false;
        let st = &mut self.st;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(130.0);
                for (i, l) in ["General", "Language", "Auto profile", "Mouse", "Engine"].into_iter().enumerate() {
                    ui.selectable_value(&mut page, i as u8, tr(l));
                }
            });
            ui.add_space(10.0); // no vertical separator: it takes all available height and stretches the dialog
            ui.vertical(|ui| match page {
                0 => {
                    ui.horizontal(|ui| { ui.label(tr("Profile folder:")); ui.text_edit_singleline(&mut st.profile_dir); });
                    rg(tr("Recent profiles")).show(ui, &mut st.recent_count, 1..=100);
                    ui.checkbox(&mut st.auto_load_last, tr("Load the last profile on start"));
                    ui.checkbox(&mut st.startup, tr("Start with Windows"));
                    ui.checkbox(&mut st.hide_empty, tr("Hide empty buttons"));
                    ui.checkbox(&mut st.numpad, tr("Show the numpad on the keyboard"));
                    ui.horizontal(|ui| {
                        ui.label(tr("On/off hotkey:"));
                        let l = if toggle_cap { tr("Press…").to_string() } else { match st.toggle { Input::Key(k) => winio::key_name(k), Input::Pad(o) => format!("🎮 {}", o.label()) } };
                        cap_toggle = ui.button(l).clicked();
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut st.check_updates, tr("Check for updates on start"));
                        check_now = ui.button(tr("Check now")).clicked();
                        let status = match &*self.upd.lock().unwrap() {
                            Upd::Checking => tr("Checking…").to_string(),
                            Upd::Latest => tr("padmimic is up to date").to_string(),
                            Upd::Available(r) => format!("⬆ v{}", r.version),
                            Upd::Failed(e) => tr(e).to_string(),
                            _ => String::new(),
                        };
                        ui.weak(status);
                    });
                    card(ui, tr("Key repeat"), |ui| {
                        ui.checkbox(&mut st.key_repeat, tr("Active"));
                        ui.add_enabled_ui(st.key_repeat, |ui| rg(tr("Delay")).suffix(" ms").show(ui, &mut st.key_repeat_delay, 0..=1000));
                        ui.add_enabled_ui(st.key_repeat, |ui| rg(tr("Rate")).suffix("/s").show(ui, &mut st.key_repeat_rate, 5..=250));
                    });
                }
                1 => pick_lang = lang_grid(ui, i18n::cur(), &[]),
                2 => {
                    ui.weak(tr("Loads a profile automatically as soon as the program is in the foreground."));
                    ui.checkbox(&mut st.auto_profile, tr("Active"));
                    let mut remove = None;
                    for (i, a) in st.auto_profiles.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut a.exe).hint_text("game.exe").desired_width(140.0));
                            ui.label("→");
                            egui::ComboBox::from_id_salt(("ap", i)).width(160.0).selected_text(stem(&a.profile)).show_ui(ui, |ui| {
                                for p in &self.profiles { ui.selectable_value(&mut a.profile, p.clone(), stem(p)); }
                            });
                            if ui.small_button("×").clicked() { remove = Some(i) }
                        });
                    }
                    if let Some(i) = remove { st.auto_profiles.remove(i); }
                    ui.horizontal(|ui| {
                        if ui.button(format!("＋ {}", tr("Add"))).clicked() { st.auto_profiles.push(AutoProfile { exe: String::new(), profile: String::new() }) }
                        if ui.button(tr("Current foreground program")).on_hover_text(tr("The active program is filled in after 3 s")).clicked() {
                            st.auto_profiles.push(AutoProfile { exe: "…".into(), profile: self.path.clone() });
                        }
                    });
                }
                3 => {
                    ui.checkbox(&mut st.disable_epp, tr("Turn off Windows “Enhance pointer precision”"));
                    card(ui, tr("Smoothing"), |ui| {
                        ui.checkbox(&mut st.smoothing, tr("Active"));
                        ui.add_enabled_ui(st.smoothing, |ui| rg(tr("History size")).show(ui, &mut st.history, 1..=100));
                        ui.add_enabled_ui(st.smoothing, |ui| rg(tr("Weight")).show(ui, &mut st.weight, 0.0..=1.0));
                    });
                    rg(tr("Mouse refresh")).suffix(" ms").show(ui, &mut st.mouse_refresh, 1..=16);
                }
                _ => {
                    ui.horizontal(|ui| {
                        ui.label(tr("Controller polling:"));
                        egui::ComboBox::from_id_salt("hz").selected_text(format!("{} Hz", st.hz)).show_ui(ui, |ui| {
                            for hz in [125, 250, 500, 1000, 2000, 4000, 8000] { ui.selectable_value(&mut st.hz, hz, format!("{hz} Hz")); }
                        });
                    });
                    ui.weak(tr("How often controller, keyboard and mouse are read and outputs are sent (high-resolution timer, highest thread priority)."));
                    ui.weak(tr("Hide the real controller with HidHide and allow padmimic.exe there, so games only see the virtual pad."));
                }
            });
        });
        self.set_page = page;
        if cap_toggle { self.cap = Some((Cap::Toggle, false)) }
        if let Some(l) = pick_lang { self.set_lang(ui.ctx(), l) }
        if check_now { self.check_updates() }
        let mut done = false;
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(tr("Defaults")).clicked() { self.st = Settings { recent: self.st.recent.clone(), last_profile: self.st.last_profile.clone(), lang: self.st.lang.clone(), ..Settings::default() } }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| done = Self::ok_row(ui));
        });
        done
    }

    fn set_lang(&mut self, ctx: &egui::Context, l: usize) {
        i18n::set(l);
        self.st.lang = i18n::LANGS[l].0.into();
        set_fonts(ctx);
    }

    /// First start: offer the language of where the user is, without saying why.
    fn lang_prompt(&mut self, ctx: &egui::Context) {
        let Some(id) = self.lang_prompt else { return };
        let mut pick = None;
        egui::Modal::new(egui::Id::new("lang_prompt")).show(ctx, |ui| {
            let c = pal(ui.visuals().dark_mode);
            ui.set_width(400.0);
            ui.vertical_centered(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(i18n::tr_in(id, "Change language?")).font(mono(19.0)).color(c.acc));
                ui.label(RichText::new("Change language?").small().color(c.weak));
                ui.add_space(10.0);
                let big = egui::Button::new(RichText::new(i18n::native(id)).size(17.0).color(Color32::BLACK)).fill(c.acc).min_size(vec2(260.0, 38.0));
                if ui.add(big).clicked() { pick = Some(id) }
                if ui.add(egui::Button::new(i18n::tr_in(id, "Keep English")).min_size(vec2(260.0, 30.0))).clicked() { pick = Some(0) }
                ui.add_space(6.0);
                let more = format!("{} {}", if self.lang_more { "−" } else { "+" }, i18n::tr_in(id, "Other languages"));
                if ui.add(egui::Label::new(RichText::new(more).color(c.weak)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.lang_more = !self.lang_more;
                }
            });
            if self.lang_more {
                ui.add_space(4.0);
                if let Some(l) = lang_grid(ui, usize::MAX, &[id, 0]) { pick = Some(l) }
            }
        });
        if let Some(l) = pick {
            self.set_lang(ctx, l);
            self.lang_prompt = None;
        }
    }

    fn dlg_update(&mut self, ui: &mut egui::Ui) -> bool {
        let state = self.upd.lock().unwrap().clone();
        let mut done = false;
        match state {
            Upd::Available(r) => {
                ui.label(trf("padmimic {} is available (you have {}).", &[&r.version, &env!("CARGO_PKG_VERSION")]));
                ui.weak(tr("padmimic restarts after the update."));
                if !r.page.is_empty() { ui.hyperlink_to(tr("Release notes"), &r.page); }
                ui.horizontal(|ui| {
                    if ui.button(tr("Later")).clicked() { done = true }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let b = egui::Button::new(RichText::new(tr("Update now")).color(Color32::BLACK)).fill(accent(ui.visuals().dark_mode)).min_size(vec2(110.0, 28.0));
                        if ui.add(b).clicked() { self.install_update(r.clone()) }
                    });
                });
            }
            Upd::Installing => { ui.horizontal(|ui| { ui.spinner(); ui.label(tr("Downloading the update…")); }); }
            Upd::Failed(e) => { ui.label(tr(&e)); done = Self::ok_row(ui) }
            _ => done = true,
        }
        done
    }

    fn dlg_save_as(&mut self, ui: &mut egui::Ui, idx: usize) -> bool {
        let Some(Dlg::SaveAs(name)) = self.dlgs.get_mut(idx) else { return true };
        ui.horizontal(|ui| { ui.label(tr("Name:")); ui.text_edit_singleline(name); });
        let name = name.trim().to_string();
        if Self::ok_row(ui) && !name.is_empty() {
            let path = std::path::Path::new(&self.st.profile_dir).join(format!("{name}.json")).display().to_string();
            self.save_to(path);
            return true;
        }
        false
    }

    fn dlg_reset(&mut self, ui: &mut egui::Ui) -> bool {
        ui.label(tr("What should be reset?"));
        let mut done = false;
        ui.horizontal(|ui| {
            if ui.button(tr("Clear current set")).clicked() { *self.set() = Set::default(); done = true }
            if ui.button(tr("Whole profile to default")).clicked() { self.p = Profile::default(); done = true }
            if ui.button(tr("Cancel")).clicked() { done = true }
        });
        done
    }

    fn dlg_about(&mut self, ui: &mut egui::Ui) -> bool {
        ui.label(RichText::new(format!("padmimic {}", env!("CARGO_PKG_VERSION"))).strong());
        ui.label(tr("Controller, keyboard & mouse → keyboard, mouse and a virtual Xbox 360 controller."));
        ui.weak(tr("Mapping logic (slots, turbo, sets, zones, curves) modelled on AntiMicroX."));
        ui.weak(tr("Open source under the GNU GPL v3."));
        ui.hyperlink_to(format!("github.com/{}", update::REPO), format!("https://github.com/{}", update::REPO));
        Self::ok_row(ui)
    }

    /// Latency meter in its own window, so it can sit next to the game while padmimic stays small.
    fn latency_window(&mut self, ctx: &egui::Context) -> bool {
        let mut open = true;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("latency"),
            egui::ViewportBuilder::default().with_title(format!("padmimic · {}", tr("Latency meter"))).with_inner_size([600.0, 380.0]).with_min_inner_size([460.0, 300.0]).with_decorations(false),
            |vui, _| {
                let ctx = vui.ctx().clone();
                window_edges(&ctx);
                let c = pal(ctx.theme() == Theme::Dark);
                egui::CentralPanel::default().frame(egui::Frame::new().fill(c.panel).inner_margin(14)).show(vui, |ui| {
                    // own small title bar: drag anywhere on it, close on the right
                    let row = Rect::from_min_size(ui.cursor().min - vec2(14.0, 14.0), vec2(ui.available_width() + 28.0, 44.0));
                    let bar = ui.interact(row, ui.id().with("latbar"), Sense::click_and_drag());
                    if bar.drag_started_by(egui::PointerButton::Primary) { ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag) }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr("Latency meter").to_uppercase()).font(mono(15.0)).color(c.acc));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if win_btn(ui, WinBtn::Close) { open = false }
                        });
                    });
                    ui.add_space(4.0);
                    self.latency_body(ui);
                });
                if ctx.input(|i| i.viewport().close_requested()) { open = false }
            },
        );
        open
    }

    fn latency_body(&mut self, ui: &mut egui::Ui) {
        let (lat, stats) = { let l = self.s.live.lock().unwrap(); (l.lat.clone(), l.stats) };
        ui.weak(tr("To compare: “Reset”, circle the stick for 10–20 s, note the values. Then e.g. start antimicro and repeat."));
        ui.label(trf("Running {} s · engine target {} Hz · reached {} Hz · accuracy ±{} ms (one tick)", &[
            &format!("{:.0}", lat.since.elapsed().as_secs_f32()), &self.st.hz, &stats.loop_hz, &format!("{:.2}", 1000.0 / self.st.hz.max(1) as f32),
        ]));
        let ms = |us: f64| format!("{:.2} ms", us / 1000.0);
        egui::Grid::new("lat").num_columns(5).striped(true).spacing([16.0, 6.0]).show(ui, |ui| {
            for h in ["", tr("Samples"), "Ø", "99 %", tr("Max")] { ui.strong(h); }
            ui.end_row();
            for (name, help, h) in [
                ("Controller data", "Gap between two new reports of the real controller. Only counts while you move it. Small and even = good.", &lat.pad),
                ("Engine tick", "Real gap between two runs of padmimic. High max values mean Windows or another program held the engine up for a moment.", &lat.tick),
                ("Path to the virtual pad", "From sending to ViGEm until the virtual controller shows the change through XInput. The earliest a game can see it.", &lat.virt),
            ] {
                ui.label(tr(name)).on_hover_text(tr(help));
                if h.n == 0 {
                    for _ in 0..4 { ui.weak("–"); }
                } else {
                    ui.label(h.n.to_string());
                    ui.label(ms(h.avg()));
                    ui.label(ms(h.pct(0.99)));
                    ui.label(ms(h.max));
                }
                ui.end_row();
            }
        });
        if lat.pad.n > 0 {
            ui.label(trf("While moving, the controller sends ≈ {} new reports per second.", &[&format!("{:.0}", 1e6 / lat.pad.avg())]));
        }
        if stats.virt_slot.is_none() {
            ui.weak(tr("No virtual pad (ViGEmBus not installed or not detected): the third row stays empty."));
        }
        if ui.button(tr("Reset")).clicked() { self.s.live.lock().unwrap().lat = engine::Lat::default() }
    }

    // ------------------------------------------------------------- background work

    fn background(&mut self) {
        let events: Vec<Event> = std::mem::take(&mut *self.s.events.lock().unwrap());
        for e in events {
            match e { Event::LoadProfile(p) => self.load(&p) }
        }
        if self.auto_t.elapsed() >= Duration::from_secs(1) {
            self.auto_t = Instant::now();
            if let Some(exe) = winio::foreground_exe().filter(|e| e != "padmimic.exe") {
                // "current foreground program" placeholder from the settings page
                for a in self.st.auto_profiles.iter_mut().filter(|a| a.exe == "…") { a.exe = exe.clone() }
                if self.st.auto_profile && exe != self.auto_last {
                    let hit = self.st.auto_profiles.iter().find(|a| a.exe.eq_ignore_ascii_case(&exe)).map(|a| a.profile.clone());
                    if let Some(p) = hit.filter(|p| *p != self.path) { self.load(&p) }
                }
                self.auto_last = exe;
            }
        }
    }

    fn sync(&mut self) {
        if *self.p_sent != self.p {
            self.p_sent = Arc::new(self.p.clone());
            *self.s.profile.lock().unwrap() = self.p_sent.clone();
        }
        if *self.st_sent != self.st {
            if self.st.disable_epp != self.st_sent.disable_epp {
                if self.st.disable_epp { self.epp_prev = Some(winio::set_pointer_precision(false)) }
                else if let Some(prev) = self.epp_prev.take() { winio::set_pointer_precision(prev); }
            }
            if self.st.startup != self.st_sent.startup { winio::set_startup(self.st.startup) }
            if self.st.profile_dir != self.st_sent.profile_dir { self.scan_profiles() }
            self.st_sent = Arc::new(self.st.clone());
            *self.s.settings.lock().unwrap() = self.st_sent.clone();
            self.st.save();
        }
    }
}

fn set_label(names: &[String; 8], i: usize) -> String {
    if names[i].is_empty() { format!("{} {}", tr("Set"), i + 1) } else { format!("{} {} ({})", tr("Set"), i + 1, names[i]) }
}

fn slot_help(s: &Slot) -> (&'static str, ()) {
    (match s {
        Slot::Out(_) => "Key, mouse button, mouse movement, wheel or controller output. Consecutive slots are pressed together.",
        Slot::Cycle => "Slots after the cycle run on the next press. Several cycles split a sequence into parts.",
        Slot::Delay(_) => "Delays the next slot. Slots already active stay pressed.",
        Slot::Distance(_) => "Following slots are only active when the axis moves this far past the dead zone.",
        Slot::Exec { .. } => "Starts a program when the slot is reached.",
        Slot::Hold(_) => "Following slots only run if the button is held this long.",
        Slot::Load(_) => "Loads a profile when the slot is reached.",
        Slot::MouseMod(_) => "Scales all mouse speeds by this percentage while active – e.g. slower while aiming.",
        Slot::Pause(_) => "Pause between key presses: previous keys are released.",
        Slot::PressTime(_) => "How long keys after this slot are held down.",
        Slot::Release(_) => "Following slots run on release if the button was held at least this long.",
        Slot::SetChange(_) => "Switches to the chosen set when the slot is reached.",
        Slot::Text(_) => "Types the whole text when the slot is reached.",
    }, ())
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.capture();
        self.background();
        let on = self.s.on.load(Relaxed);
        // while the game has focus nobody watches this window at 60 fps: its CPU/GPU time belongs to the game
        let focused = ui.ctx().input(|i| i.viewport().focused.unwrap_or(true));
        let ms = if self.cap.is_some() || !self.dlgs.is_empty() { 16 } else if on || self.show_live { if focused { 16 } else { 100 } } else { 250 };
        ui.ctx().request_repaint_after(Duration::from_millis(ms));
        window_edges(ui.ctx());
        let active = self.s.live.lock().unwrap().active.clone();

        let bar = egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(14, 10));
        egui::Panel::top("top").frame(bar).show(ui, |ui| {
            self.header(ui);
            self.profile_bar(ui);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Controller, format!("🎮 {}", tr("Controller")));
                ui.selectable_value(&mut self.tab, Tab::Kbm, format!("⌨ {}", tr("Keyboard & mouse")));
                if self.tab == Tab::Controller {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.checkbox(&mut self.show_live, tr("Live view")); });
                }
            });
        });
        egui::Panel::bottom("bottom").frame(bar).show(ui, |ui| self.bottom(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(14, 2))).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| match self.tab {
                Tab::Controller => self.controller_tab(ui, &active),
                Tab::Kbm => self.kbm_tab(ui, &active),
            });
        });
        let ctx = ui.ctx().clone();
        self.dialogs(&ctx);
        self.lang_prompt(&ctx);
        if matches!(*self.upd.lock().unwrap(), Upd::Restarting) { ctx.send_viewport_cmd(egui::ViewportCommand::Close) }
        self.sync();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(prev) = self.epp_prev.take() { winio::set_pointer_precision(prev); }
        self.s.on.store(false, Relaxed);
        std::thread::sleep(Duration::from_millis(20)); // let the engine release held keys
        self.st.save();
    }
}
