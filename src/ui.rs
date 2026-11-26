//! AntiMicroX layout (profile bar, element grid, sets 1-8, Quick Set / Names / Pref / Reset, edit dialogs),
//! retro-modern look. Every text is English and goes through `tr` (see i18n.rs).

use crate::engine::{self, Event, Shared, eval_stick, pad_down};
use crate::i18n::{self, tr, trf};
use crate::update::{self, State as Upd};
use crate::model::*;
use crate::winio;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Theme, Vec2, vec2};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering::Relaxed};
use crate::icons::{self, Icon};
use std::time::{Duration, Instant};
use vigem_client::XGamepad;

// ================================================================= look

/// Accent colours to pick from (Settings → Appearance).
pub(crate) const ACCENTS: [(&str, [u8; 3]); 6] = [
    ("Blue", [36, 150, 255]), ("Violet", [132, 102, 255]), ("Pink", [255, 66, 140]),
    ("Orange", [255, 130, 36]), ("Green", [28, 190, 118]), ("Teal", [0, 180, 196]),
];

/// Zoom for Settings → Appearance → Size: small (default), medium, large.
pub(crate) const SIZES: [f32; 3] = [0.67, 0.8, 1.0];

static ACCENT: AtomicU8 = AtomicU8::new(0);
static DESIGN_ACCENT: AtomicU32 = AtomicU32::new(0);
fn set_design_accent(color: Option<[u8;3]>) -> bool {
    let packed=color.map(|[r,g,b]|0x01000000|((r as u32)<<16)|((g as u32)<<8)|b as u32).unwrap_or(0);
    DESIGN_ACCENT.swap(packed,Relaxed)!=packed
}
/// How much the panels cover the window background, in percent: 100 = solid, lower = glass.
static COVER: AtomicU8 = AtomicU8::new(100);
static MOTION: AtomicBool = AtomicBool::new(true);

pub(crate) fn set_look(accent: u8, cover: f32, motion: bool) {
    ACCENT.store(accent, Relaxed);
    COVER.store((cover.clamp(0.0, 1.0) * 100.0).round() as u8, Relaxed);
    MOTION.store(motion, Relaxed);
}

pub(crate) fn motion() -> bool {
    MOTION.load(Relaxed)
}

/// Black-and-white canvas with one accent, like a social timeline: cards and controls are a step
/// lighter (or darker), lines are hairlines. `amber` is the coral of the virtual pad output.
pub(crate) struct Pal { pub side: Color32, pub panel: Color32, pub card: Color32, pub line: Color32, pub idle: Color32, pub hover: Color32, pub text: Color32, pub weak: Color32, pub acc: Color32, pub amber: Color32 }

pub(crate) fn pal(dark: bool) -> Pal {
    pal_with(dark, COVER.load(Relaxed) as f32 / 100.0)
}

/// `cover` < 1 makes the surfaces see-through (premultiplied alpha), for aurora / Mica / Acrylic.
pub(crate) fn pal_with(dark: bool, cover: f32) -> Pal {
    let c = Color32::from_rgb;
    let design=DESIGN_ACCENT.load(Relaxed);
    let a=if design!=0 { [(design>>16) as u8,(design>>8) as u8,design as u8] } else { ACCENTS[ACCENT.load(Relaxed) as usize % ACCENTS.len()].1 };
    let acc = c(a[0], a[1], a[2]);
    let mut p = if dark {
        Pal { side: c(0, 0, 0), panel: c(0, 0, 0), card: c(22, 24, 28), line: c(47, 51, 54), idle: c(32, 35, 39), hover: c(45, 49, 54), text: c(231, 233, 234), weak: c(113, 118, 123), acc, amber: c(255, 122, 89) }
    } else {
        Pal { side: c(255, 255, 255), panel: c(255, 255, 255), card: c(246, 248, 249), line: c(232, 236, 238), idle: c(237, 241, 243), hover: c(224, 230, 233), text: c(15, 20, 25), weak: c(83, 100, 113), acc, amber: c(232, 88, 52) }
    };
    if cover < 1.0 {
        let glass = |x: Color32, f: f32| x.gamma_multiply(f.clamp(0.0, 1.0));
        p.side = glass(p.side, cover);
        p.panel = glass(p.panel, cover * 0.8);
        p.card = glass(p.card, cover + 0.3);
        p.idle = glass(p.idle, cover + 0.45);
        p.hover = glass(p.hover, cover + 0.5);
        p.line = glass(p.line, 0.8);
    }
    p
}

pub fn accent(dark: bool) -> Color32 {
    pal(dark).acc
}

/// Numeric readouts (digits line up).
pub(crate) fn mono(s: f32) -> FontId {
    FontId::monospace(s)
}

/// Semibold UI font for titles, buttons and selected items.
pub(crate) fn bold(s: f32) -> FontId {
    FontId::new(s, egui::FontFamily::Name("bold".into()))
}

pub(crate) const FRAME_60: Duration = Duration::from_nanos(16_666_667);

/// Pace the native UI once per frame, including frames that draw the Settings window.
#[derive(Default)]
struct FramePacer {
    frame: Option<u64>,
    next: Option<Instant>,
}

impl FramePacer {
    fn wait(&mut self, frame: u64) {
        if self.frame == Some(frame) { return; }
        self.frame = Some(frame);
        let now = Instant::now();
        let deadline = self.next.unwrap_or(now);
        if let Some(delay) = deadline.checked_duration_since(now) {
            std::thread::sleep(delay);
        }
        let actual = Instant::now();
        // Rebase after an idle interval instead of trying to render missed frames.
        self.next = Some(if actual.saturating_duration_since(deadline) >= FRAME_60 {
            actual + FRAME_60
        } else {
            deadline + FRAME_60
        });
    }
}

/// 0..1 → 0..1, fast start, soft landing.
pub(crate) fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Animated 0/1 that respects the "Animations" setting.
pub(crate) fn anim(ctx: &egui::Context, id: impl std::hash::Hash + std::fmt::Debug, on: bool, secs: f32) -> f32 {
    ctx.animate_bool_with_time(egui::Id::new(id), on, if motion() { secs } else { 0.0 })
}

pub(crate) fn anim_to(ctx: &egui::Context, id: impl std::hash::Hash + std::fmt::Debug, v: f32, secs: f32) -> f32 {
    ctx.animate_value_with_time(egui::Id::new(id), v, if motion() { secs } else { 0.0 })
}

/// Windows fonts: Segoe UI (regular + semibold) / Cascadia Mono for Latin, then fallbacks for
/// Japanese, Chinese, Korean and Arabic. The current language's CJK font goes first, because the
/// same Han character is drawn differently in Japanese and Chinese.
pub fn set_fonts(ctx: &egui::Context) {
    use egui::FontFamily::{Monospace, Name, Proportional};
    let mut fonts = egui::FontDefinitions::default();
    let mut load = |name: &str, file: &str| {
        let ok = std::fs::read(format!(r"C:\Windows\Fonts\{file}")).map(|d| fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(d)))).is_ok();
        ok.then(|| name.to_string())
    };
    // Segoe UI Variable has no Arabic glyphs, so Arabic words always become shaping runs of their own
    // (needed for right-to-left order, see i18n::rtl); plain Segoe UI only if it is missing
    let ui_font = load("ui", "SegUIVar.ttf").or_else(|| load("ui", "segoeui.ttf"));
    let ui_bold = load("uib", "seguisb.ttf").or_else(|| load("uib", "segoeuib.ttf"));
    let mono = [load("mono", "CascadiaMono.ttf"), load("mono2", "consola.ttf")];
    let (ja, zh, ko) = (load("ja", "YuGothB.ttc").or_else(|| load("ja", "YuGothM.ttc")), load("zh", "msyhbd.ttc").or_else(|| load("zh", "msyh.ttc")), load("ko", "malgunbd.ttf").or_else(|| load("ko", "malgun.ttf")));
    let ar = load("ar", "arial.ttf");
    let cjk = match i18n::cur() { 2 => [ja, zh, ko], 4 => [ko, ja, zh], _ => [zh, ja, ko] };
    let fallbacks: Vec<String> = cjk.into_iter().chain([ar]).flatten().collect();
    for (fam, first) in [(Proportional, vec![ui_font]), (Monospace, mono.to_vec())] {
        let list = fonts.families.get_mut(&fam).unwrap();
        for (i, f) in first.into_iter().flatten().enumerate() { list.insert(i, f) }
        list.extend(fallbacks.iter().cloned());
    }
    // Segoe UI Semibold has Arabic glyphs, which would break the right-to-left trick (see i18n::rtl):
    // Arabic gets the regular weight everywhere
    let mut b: Vec<String> = ui_bold.filter(|_| i18n::cur() != i18n::AR).into_iter().collect();
    b.extend(fonts.families[&Proportional].iter().cloned());
    fonts.families.insert(Name("bold".into()), b);
    ctx.set_fonts(fonts);
}

pub fn style(ctx: &egui::Context) {
    set_fonts(ctx);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.button_padding = vec2(14.0, 7.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.window_margin = egui::Margin::same(20);
        s.spacing.menu_margin = egui::Margin::same(8);
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, bold(13.5));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
        s.text_styles.insert(egui::TextStyle::Heading, bold(18.0));
        s.animation_time = 0.18;
        let v = &mut s.visuals;
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = CornerRadius::same(16); // pills at control height
            w.expansion = 0.0;
        }
        v.window_corner_radius = CornerRadius::same(22);
        v.menu_corner_radius = CornerRadius::same(14);
        v.window_shadow = egui::Shadow { offset: [0, 16], blur: 48, spread: 0, color: Color32::from_black_alpha(110) };
        v.popup_shadow = egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(80) };
    });
    apply_colors(ctx);
}

/// Colours for both themes; again after the accent or the glass level changed.
pub(crate) fn apply_colors(ctx: &egui::Context) {
    for (theme, dark) in [(Theme::Light, false), (Theme::Dark, true)] {
        ctx.style_mut_of(theme, |s| {
            let c = pal(dark);
            let solid = pal_with(dark, 1.0); // menus and sheets float over content: never see-through
            let v = &mut s.visuals;
            v.panel_fill = c.panel;
            v.window_fill = solid.card;
            v.window_stroke = Stroke::new(1.0, solid.line);
            v.extreme_bg_color = c.idle;
            v.faint_bg_color = c.idle;
            v.code_bg_color = c.idle;
            v.override_text_color = Some(c.text);
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.line);
            // flat controls: fill only, the accent shows up on focus and selection
            for (w, fill) in [(&mut v.widgets.inactive, c.idle), (&mut v.widgets.hovered, c.hover), (&mut v.widgets.active, c.hover), (&mut v.widgets.open, c.hover)] {
                (w.weak_bg_fill, w.bg_fill, w.bg_stroke) = (fill, fill, Stroke::NONE);
            }
            v.selection.bg_fill = c.acc.gamma_multiply(0.3);
            v.selection.stroke = Stroke::new(1.5, c.acc);
            v.hyperlink_color = c.acc;
        });
    }
}

fn card<R>(ui: &mut egui::Ui, title: &str, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let c = pal(ui.visuals().dark_mode);
    egui::Frame::new()
        .fill(c.card)
        .corner_radius(20)
        .inner_margin(18)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                if !title.is_empty() {
                    ui.label(RichText::new(title).font(bold(16.0)).color(c.text));
                    ui.add_space(2.0);
                }
                f(ui)
            })
            .inner
        })
        .inner
}

/// iOS-style switch: the knob stretches while pressed and springs across.
pub(crate) fn switch(ui: &mut egui::Ui, on: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(48.0, 28.0), Sense::click());
    let c = pal(ui.visuals().dark_mode);
    let t = ease(anim(ui.ctx(), resp.id, on, 0.22));
    let press = anim(ui.ctx(), resp.id.with("press"), resp.is_pointer_button_down_on(), 0.12);
    let p = ui.painter();
    let off = if resp.hovered() { c.hover } else { c.idle };
    p.rect_filled(rect, 14.0, off.lerp_to_gamma(c.acc, t));
    let kw = 24.0 + 6.0 * press;
    let x = egui::lerp(rect.left() + 2.0..=rect.right() - 2.0 - kw, t);
    let knob = Rect::from_min_size(Pos2::new(x, rect.top() + 2.0), vec2(kw, 24.0));
    p.rect_filled(knob.translate(vec2(0.0, 1.0)), 12.0, Color32::from_black_alpha(45));
    p.rect_filled(knob, 12.0, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Round icon button (title bar, headers): soft circle on hover.
pub(crate) fn icon_btn(ui: &mut egui::Ui, icon: Icon, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let c = pal(ui.visuals().dark_mode);
    let h = anim(ui.ctx(), resp.id, resp.hovered(), 0.12);
    let p = ui.painter();
    if h > 0.0 { p.circle_filled(rect.center(), size / 2.0, c.hover.gamma_multiply(h)); }
    icons::draw(p, icon, rect.shrink(size * 0.25), if h > 0.5 { c.text } else { c.weak.lerp_to_gamma(c.text, h) });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Bold pill button; `filled` = accent background (the main action).
pub(crate) fn pill(ui: &mut egui::Ui, text: &str, filled: bool) -> egui::Response {
    let c = pal(ui.visuals().dark_mode);
    let font = bold(14.0);
    let g = ui.painter().layout_no_wrap(text.to_string(), font, c.text);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 36.0, 36.0), Sense::click());
    let h = anim(ui.ctx(), resp.id, resp.hovered(), 0.12);
    let press = anim(ui.ctx(), resp.id.with("p"), resp.is_pointer_button_down_on(), 0.08);
    let r = rect.shrink(press * 1.5);
    let p = ui.painter();
    if filled {
        p.rect_filled(r, 18.0, c.acc.lerp_to_gamma(Color32::WHITE, 0.12 * h));
        p.galley_with_override_text_color(r.center() - g.size() / 2.0, g, Color32::WHITE);
    } else {
        p.rect_filled(r, 18.0, c.idle.lerp_to_gamma(c.hover, h));
        p.galley(r.center() - g.size() / 2.0, g, c.text);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Settings row: text on the left, switch on the right, the whole row is clickable.
pub(crate) fn row_switch(ui: &mut egui::Ui, on: &mut bool, text: &str) -> egui::Response {
    let c = pal(ui.visuals().dark_mode);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    let galley = ui.painter().layout(text.to_string(), FontId::proportional(14.5), c.text, rect.width() - 80.0);
    ui.painter().galley(Pos2::new(rect.left() + 2.0, rect.center().y - galley.size().y / 2.0), galley, c.text);
    let sw = Rect::from_min_size(Pos2::new(rect.right() - 50.0, rect.center().y - 14.0), vec2(48.0, 28.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(sw).id_salt(("rs", text)));
    let clicked = switch(&mut child, *on);
    if clicked || resp.clicked() { *on = !*on }
    resp
}

/// Segmented control with a thumb that slides to the chosen option.
pub(crate) fn segmented(ui: &mut egui::Ui, id: &str, sel: &mut u8, options: &[&str]) {
    let c = pal(ui.visuals().dark_mode);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(420.0), 36.0), Sense::hover());
    let n = options.len() as f32;
    let w = (rect.width() - 6.0) / n;
    let p = ui.painter();
    p.rect_filled(rect, 18.0, c.idle);
    let x = anim_to(ui.ctx(), ("seg", id), *sel as f32, 0.22);
    let thumb = Rect::from_min_size(Pos2::new(rect.left() + 3.0 + x * w, rect.top() + 3.0), vec2(w, 30.0));
    p.rect_filled(thumb.translate(vec2(0.0, 1.0)), 15.0, Color32::from_black_alpha(30));
    p.rect_filled(thumb, 15.0, if ui.visuals().dark_mode { c.hover.lerp_to_gamma(Color32::WHITE, 0.08) } else { Color32::WHITE });
    for (i, o) in options.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(rect.left() + 3.0 + i as f32 * w, rect.top() + 3.0), vec2(w, 30.0));
        let resp = ui.interact(r, ui.id().with((id, i)), Sense::click());
        let on = *sel as usize == i;
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *o, if on { bold(13.0) } else { FontId::proportional(13.0) }, if on { c.text } else { c.weak });
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { *sel = i as u8 }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum WinBtn { Min, Max, Restore, Close }

/// Minimize / maximize / close in the own title bar, drawn as lines (no font glyph needed); close turns red.
pub(crate) fn win_btn(ui: &mut egui::Ui, kind: WinBtn) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 32.0), Sense::click());
    let c = pal(ui.visuals().dark_mode);
    let hot = resp.hovered();
    let red = kind == WinBtn::Close && hot;
    let col = if red { Color32::WHITE } else if hot { c.text } else { c.weak };
    let p = ui.painter();
    if hot { p.rect_filled(rect, 16.0, if red { Color32::from_rgb(232, 65, 66) } else { c.hover }); }
    let (m, s) = (rect.center(), Stroke::new(1.2, col));
    let box_at = |o: Vec2| Rect::from_center_size(m + o, vec2(9.0, 9.0));
    match kind {
        WinBtn::Min => { p.hline((m.x - 5.0)..=(m.x + 5.0), m.y + 1.0, s); }
        WinBtn::Max => { p.rect_stroke(box_at(Vec2::ZERO), 1.0, s, StrokeKind::Inside); }
        WinBtn::Restore => {
            p.rect_stroke(box_at(vec2(1.5, -1.5)), 1.0, s, StrokeKind::Inside);
            p.rect_filled(box_at(vec2(-1.5, 1.5)), 1.0, if hot { c.hover } else { c.panel });
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
fn window_edges(ctx: &egui::Context, own_max: bool) {
    use egui::{CursorIcon as C, ResizeDirection as D};
    let r = ctx.content_rect();
    let maxed = own_max || ctx.input(|i| i.viewport().maximized.unwrap_or(false));
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
pub(crate) fn lang_grid(ui: &mut egui::Ui, current: usize, skip: &[usize]) -> Option<usize> {
    let c = pal(ui.visuals().dark_mode);
    let ids: Vec<usize> = (0..i18n::LANGS.len()).filter(|i| !skip.contains(i)).collect();
    let cols = if ui.available_width() > 420.0 { 3 } else { 2 };
    let w = (ui.available_width() - (cols - 1) as f32 * 8.0) / cols as f32;
    let mut hit = None;
    egui::Grid::new(("langs", skip.len())).spacing([8.0, 8.0]).show(ui, |ui| {
        for (n, &i) in ids.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 54.0), Sense::click());
            let on = i == current;
            let h = anim(ui.ctx(), resp.id, resp.hovered(), 0.12);
            let p = ui.painter();
            p.rect_filled(rect, 16.0, if on { c.acc.gamma_multiply(0.18) } else { c.idle.lerp_to_gamma(c.hover, h) });
            if on { p.rect_stroke(rect, 16.0, Stroke::new(1.5, c.acc), StrokeKind::Inside); }
            p.text(rect.center() - vec2(0.0, 8.0), Align2::CENTER_CENTER, i18n::native(i), FontId::proportional(15.5), if on { c.acc } else { c.text });
            let english = ["English", "German", "Japanese", "Chinese (Simplified)", "Korean", "Vietnamese", "Filipino", "Arabic"][i];
            p.text(rect.center() + vec2(0.0, 12.0), Align2::CENTER_CENTER, english, FontId::proportional(11.5), c.weak);
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { hit = Some(i) }
            if n % cols == cols - 1 { ui.end_row() }
        }
    });
    hit
}

/// A grid cell like AntiMicro's button widgets: caption + current assignment, lit while pressed,
/// with an optional icon in front of the body (element settings) and small marks
/// (turbo, toggle) in the top right corner. Lifts on hover, sinks when pressed, glows while active.
fn cell_ex(ui: &mut egui::Ui, icon: Option<Icon>, marks: &[Icon], title: &str, body: &str, active: bool, size: Vec2) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let c = pal(ui.visuals().dark_mode);
    let ctx = ui.ctx().clone();
    let h = anim(&ctx, resp.id, resp.hovered(), 0.14);
    let a = anim(&ctx, resp.id.with("a"), active, 0.08);
    let press = anim(&ctx, resp.id.with("p"), resp.is_pointer_button_down_on(), 0.07);
    let r = rect.shrink(press * 2.0);
    let p = ui.painter();
    let empty = body == "—";
    p.rect_filled(r, 14.0, c.idle.lerp_to_gamma(c.hover, h).lerp_to_gamma(c.acc.gamma_multiply(0.35), a));
    if a > 0.0 { p.rect_stroke(r, 14.0, Stroke::new(1.5, c.acc.gamma_multiply(a)), StrokeKind::Inside); }
    let (tc, bc) = (c.weak.lerp_to_gamma(c.acc, a), if empty { c.weak.gamma_multiply(0.55) } else { c.text });
    let room = size.x - 20.0 - if icon.is_some() { 20.0 } else { 0.0 };
    let max_chars = (room / 7.2).max(3.0) as usize;
    let body: String = if body.chars().count() > max_chars { body.chars().take(max_chars - 1).chain(['…']).collect() } else { body.into() };
    if title.is_empty() {
        let g = p.layout_no_wrap(body, if icon.is_some() { bold(13.5) } else { FontId::proportional(13.5) }, bc);
        let iw = if icon.is_some() { 20.0 } else { 0.0 };
        let x0 = r.center().x - (g.size().x + iw) / 2.0;
        if let Some(i) = icon { icons::draw(p, i, Rect::from_center_size(Pos2::new(x0 + 7.0, r.center().y), vec2(15.0, 15.0)), c.weak.lerp_to_gamma(c.text, h)); }
        p.galley(Pos2::new(x0 + iw, r.center().y - g.size().y / 2.0), g, bc);
    } else {
        p.text(r.left_top() + vec2(12.0, 7.0), Align2::LEFT_TOP, title, FontId::proportional(11.5), tc);
        p.text(r.left_bottom() + vec2(12.0, -7.0), Align2::LEFT_BOTTOM, body, FontId::proportional(14.0), bc);
    }
    for (i, m) in marks.iter().enumerate() {
        icons::draw(p, *m, Rect::from_min_size(Pos2::new(r.right() - 20.0 - i as f32 * 15.0, r.top() + 7.0), vec2(12.0, 12.0)), c.acc);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

const SIDEBAR_ICON_X: f32 = 26.0;
const SIDEBAR_LABEL_X: f32 = 52.0;
const HEADER_CENTER_Y: f32 = 36.0;
const SIDEBAR_TOP: i8 = 14;
const TITLE_BAR_TOP: i8 = 10;

/// Sidebar: a column of entries with one pill that glides to the selected entry.
/// Returns the clicked index. `compact` shows icons only.
fn nav_list(ui: &mut egui::Ui, id: &str, items: &[(Icon, &str)], sel: Option<usize>, compact: bool) -> Option<usize> {
    let c = pal(ui.visuals().dark_mode);
    let h = 48.0;
    let w = ui.available_width();
    // the pill is painted first, under the entries
    let pill_layer = ui.painter().add(Shape::Noop);
    let mut hit = None;
    let mut selected_rect = None;
    for (i, (icon, text)) in items.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
        let on = sel == Some(i);
        if on { selected_rect = Some(rect); }
        let hv = anim(ui.ctx(), resp.id, resp.hovered() && !on, 0.12);
        let p = ui.painter();
        if hv > 0.0 { p.rect_filled(rect.shrink2(vec2(0.0, 2.0)), 22.0, c.hover.gamma_multiply(hv * 0.7)); }
        let col = if on { c.text } else { c.text.gamma_multiply(0.86) };
        let ir = if compact { Rect::from_center_size(rect.center(), vec2(24.0, 24.0)) } else { Rect::from_center_size(Pos2::new(rect.left() + SIDEBAR_ICON_X, rect.center().y), vec2(24.0, 24.0)) };
        icons::draw(p, *icon, ir, if on { c.acc } else { col });
        if !compact {
            p.text(Pos2::new(rect.left() + SIDEBAR_LABEL_X, rect.center().y), Align2::LEFT_CENTER, *text, if on { bold(16.5) } else { FontId::proportional(16.5) }, col);
        }
        let resp = if compact { resp.on_hover_text(*text) } else { resp };
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { hit = Some(i) }
    }
    if let Some(selected) = selected_rect {
        // Allocated rows include item spacing; use the same bounds as their icon and text.
        let y = anim_to(ui.ctx(), (id, "pill"), selected.top(), 0.26);
        let a = anim(ui.ctx(), (id, "pill_on"), true, 0.2);
        let r = Rect::from_min_size(Pos2::new(selected.left(), y + 2.0), vec2(selected.width(), selected.height() - 4.0));
        ui.painter().set(pill_layer, egui::epaint::RectShape::filled(r, 22.0, c.acc.gamma_multiply(0.16 * a)));
    }
    hit
}

/// Fader for every number: hover, hold and drag up/down (or left/right) to change it,
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
        let (rect, _) = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover());
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

        // look: label, thin track with knob, value
        let p = ui.painter();
        let hot = enabled && (resp.hovered() || resp.dragged());
        let dim = |x: Color32| if enabled { x } else { x.gamma_multiply(0.45) };
        p.rect_filled(rect, 19.0, if hot { c.hover } else { c.idle });
        let t = to_t(val).clamp(0.0, 1.0) as f32;
        let track = Rect::from_min_max(Pos2::new(rect.left() + rect.width() * 0.48, rect.center().y - 2.0), Pos2::new(rect.right() - 80.0, rect.center().y + 2.0));
        p.rect_filled(track, 2.0, dim(c.line));
        let x = track.left() + track.width() * t;
        p.rect_filled(Rect::from_min_max(track.min, Pos2::new(x, track.bottom())), 2.0, dim(c.acc));
        let kr = egui::lerp(7.0..=9.0, anim(ui.ctx(), id.with("k"), hot, 0.12));
        p.circle_filled(Pos2::new(x, track.center().y + 1.0), kr, Color32::from_black_alpha(50));
        p.circle_filled(Pos2::new(x, track.center().y), kr, dim(Color32::WHITE));
        p.text(rect.left_center() + vec2(16.0, 0.0), Align2::LEFT_CENTER, self.text, FontId::proportional(14.0), dim(c.text));
        p.text(rect.right_center() - vec2(16.0, 0.0), Align2::RIGHT_CENTER, format!("{}{}", fmt(val), self.suffix), bold(13.0), dim(if hot { c.acc } else { c.weak }));
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
        Act::Pad(o) => format!("{} {}", tr("Pad"), o.label()),
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

/// Assignment as text, with turbo / toggle spelled out (cells show those as icons instead).
pub fn summary(b: &Button) -> String {
    let mut s = summary_core(b);
    if b.turbo { s = format!("{} · {s}", tr("Turbo")) }
    if b.toggle { s = format!("{} · {s}", tr("Toggle")) }
    s
}

fn summary_core(b: &Button) -> String {
    if !b.action.is_empty() {
        return b.action.clone();
    }
    let s = if b.slots.is_empty() { "—".to_string() } else { b.slots.iter().map(slot_text).collect::<Vec<_>>().join(" · ") };
    match b.set { Some((set, _)) => format!("{s} → S{}", set + 1), None => s }
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
        Elem::Key(i) => p.keys.get(i as usize).map(|k| winio::key_name(k.vk)).unwrap_or_default(),
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
                        .corner_radius(6)
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
        for (i, l) in ["Keyboard", "Mouse", "Controller"].into_iter().enumerate() {
            ui.selectable_value(tab, i as u8, tr(l));
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

pub(crate) use crate::pad::{Trail, controller};

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
    SaveAs(String),
    Reset,
    About,
    Latency,
    Update,
}

#[derive(Clone, Copy, PartialEq)]
enum Cap { Assign(Elem, Option<usize>), KeyBind(usize), Toggle }

#[derive(Clone, Copy, PartialEq)]
enum Tab { Controller, Kbm, Custom, Settings }

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
    // ---- look & motion
    brand_started: Option<Instant>,
    brand_hover: bool,            // pointer over the logo last frame
    launched: Option<Instant>,    // first frame, for the start screen
    normal: Option<Rect>,         // last window rect while not maximized
    page_t: Instant,             // last page change, for the page transition
    prev_tab: Tab,                // where "Settings" goes back to
    dlg_t: Vec<Instant>,          // when each open sheet appeared
    settings_open: bool,          // settings window beside the main window
    side_right: bool,
    dock_desktop: crate::docking::Desktop,
    side_applied: bool,           // material + corners set on the settings window
    set_detail: bool,             // narrow settings: a page is pushed over the list
    look_sent: Option<(u8, u8, u8, u32, bool, u8)>,
    material_ok: bool,            // Mica / Acrylic worked (Windows 11)
    show_settings: bool,          // dev hook: open settings on the first frame
    restore: Option<Rect>,        // own maximize: the window rect to go back to
    // ---- customization
    skins: Vec<crate::skin::Skin>,
    skin_errs: Vec<String>,
    skins_loaded: bool,
    cust_filter: u8,
    cust_search: String,
    frame_pacer: FramePacer,
    controller_overlay: crate::overlay::Window,
    taskbar_icon_applied: bool,
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
            brand_started: None, brand_hover: false, launched: None, normal: None, page_t: Instant::now(), prev_tab: Tab::Controller, dlg_t: vec![], settings_open: false, side_right: true, dock_desktop: crate::docking::Desktop::default(), side_applied: false,
            set_detail: false, look_sent: None, material_ok: true, show_settings: false, restore: None,
            skins: vec![], skin_errs: vec![], skins_loaded: false, cust_filter: 0, cust_search: String::new(), frame_pacer: FramePacer::default(), controller_overlay: crate::overlay::Window::default(), taskbar_icon_applied: false,
        };
        app.scan_profiles();
        update::cleanup();
        if app.st.check_updates { app.check_updates() }
        // dev hook for screenshots: PADMIMIC_SHOW=settings,languages,mouse,stick,latency,lang opens those at start
        for d in std::env::var("PADMIMIC_SHOW").unwrap_or_default().split(',') {
            match d {
                "settings" => app.show_settings = true,
                "appearance" => { app.set_detail = true; app.show_settings = true }
                "page" => { app.tab = Tab::Settings; app.set_detail = true }
                "languages" => { app.set_page = 2; app.set_detail = true; app.show_settings = true }
                "kbm" => app.tab = Tab::Kbm,
                "custom" => app.tab = Tab::Custom,
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
        if !self.dlgs.contains(&d) {
            self.dlgs.push(d);
            self.dlg_t.push(Instant::now());
        }
    }

    fn go(&mut self, t: Tab) {
        if self.tab != t {
            if t == Tab::Settings && self.tab != Tab::Settings { self.prev_tab = self.tab }
            self.tab = t;
            self.page_t = Instant::now();
        }
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

    fn page_title(&self) -> &'static str {
        match self.tab { Tab::Controller => tr("Controller"), Tab::Kbm => tr("Keyboard & mouse"), Tab::Custom => tr("Customization"), Tab::Settings => tr("Settings") }
    }

    /// Header of the content column: page title, save, live view, window buttons.
    /// The window has no Windows frame: empty space drags, double click maximizes. The drag area is
    /// registered before the widgets, so buttons on top still get their clicks.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let row = Rect::from_min_size(ui.cursor().min - vec2(22.0, 10.0), vec2(ui.available_width() + 30.0, 58.0));
        let bar = ui.interact(row, ui.id().with("titlebar"), Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let maxed = self.restore.is_some();
        if bar.double_clicked() {
            self.toggle_max(&ctx);
        } else if bar.drag_started_by(egui::PointerButton::Primary) {
            // dragging a maximized window: back to its old size, under the cursor at the same spot
            if let (Some(old), Some(outer), Some(at)) = (self.restore.take(), ctx.input(|i| i.viewport().outer_rect), ctx.input(|i| i.pointer.press_origin())) {
                let fx = (at.x - outer.left()) / outer.width().max(1.0);
                let screen = outer.min + at.to_vec2();
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(old.size()));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(screen.x - fx * old.width(), screen.y - at.y)));
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let c = pal(ui.visuals().dark_mode);
        ui.horizontal(|ui| {
            ui.set_min_height(2.0 * (HEADER_CENTER_Y - f32::from(TITLE_BAR_TOP)));
            ui.label(RichText::new(self.page_title()).font(bold(22.0)).color(c.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if win_btn(ui, WinBtn::Close) { ctx.send_viewport_cmd(egui::ViewportCommand::Close) }
                if win_btn(ui, if maxed { WinBtn::Restore } else { WinBtn::Max }) { self.toggle_max(&ctx) }
                if win_btn(ui, WinBtn::Min) { ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)) }
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.add_space(12.0);
                if self.tab == Tab::Controller {
                    if switch(ui, self.show_live) { self.show_live = !self.show_live }
                    ui.label(RichText::new(tr("Live view")).color(c.weak));
                    ui.add_space(6.0);
                }
                // the save pill only exists while there is something to save; it fades and slides in
                let dirty = self.p != self.saved;
                let a = ease(anim(&ctx, "save_pill", dirty, 0.25));
                if a > 0.01 {
                    ui.add_space((1.0 - a) * 16.0);
                    ui.scope(|ui| {
                        ui.set_opacity(a);
                        if pill(ui, tr("Save"), true).clicked() && dirty { self.save() }
                    });
                }
            });
        });
    }

    /// Maximize to the monitor's work area ourselves: with Windows' maximized flag a frameless
    /// window loses Mica / Acrylic and the whole UI turns solid.
    fn toggle_max(&mut self, ctx: &egui::Context) {
        if let Some(r) = self.restore.take() {
            self.look_sent = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(r.size()));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(r.min));
            return;
        }
        let Some(outer) = ctx.input(|i| i.viewport().outer_rect) else { return };
        let ppp = ctx.pixels_per_point();
        let (l, t, r, b) = winio::work_area((outer.center().x * ppp) as i32, (outer.center().y * ppp) as i32);
        let wa = Rect::from_min_max(Pos2::new(l as f32, t as f32) / ppp, Pos2::new(r as f32, b as f32) / ppp);
        self.restore = Some(self.normal.unwrap_or(outer));
        self.look_sent = None; // re-apply the material after the size jump
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(wa.min));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(wa.size()));
    }

    fn save(&mut self) {
        if self.path.is_empty() { self.open(Dlg::SaveAs(String::new())) } else { let p = self.path.clone(); self.save_to(p) }
    }

    /// Left column, timeline style: logo, pages, the big power pill, sets, the profile at the bottom.
    /// `compact` (narrow window) keeps only icons.
    fn sidebar(&mut self, ui: &mut egui::Ui, compact: bool) {
        let c = pal(ui.visuals().dark_mode);
        let ctx = ui.ctx().clone();
        let drag = ui.interact(Rect::from_min_size(ui.max_rect().min - vec2(12.0, 14.0), vec2(ui.available_width() + 24.0, 62.0)), ui.id().with("sidedrag"), Sense::click_and_drag());
        if drag.drag_started_by(egui::PointerButton::Primary) { ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag) }

        let height = 2.0 * (HEADER_CENTER_Y - f32::from(SIDEBAR_TOP));
        let (r, logo) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        // the start screen draws the mark; hovering it here draws it again
        let hover = logo.contains_pointer();
        let idle = self.brand_started.is_none_or(|s| s.elapsed().as_secs_f32() >= crate::brand::DRAW_SECONDS);
        if hover && !self.brand_hover && idle && motion() { self.brand_started = Some(Instant::now()) }
        self.brand_hover = hover;
        let draw = self.brand_started.map_or(crate::brand::DRAW_SECONDS, |s| s.elapsed().as_secs_f32());
        if draw < crate::brand::DRAW_SECONDS { ctx.request_repaint() }
        let center = Pos2::new(if compact { r.center().x } else { r.left() + SIDEBAR_ICON_X }, r.center().y);
        crate::brand::mark(ui.painter(), center, 0.94, c.text, draw);
        ui.add_space(64.0 - height);

        let items = [(Icon::Pad, tr("Controller")), (Icon::Keyboard, tr("Keyboard & mouse")), (Icon::Palette, tr("Customization")), (Icon::Gear, tr("Settings")), (Icon::Gauge, tr("Latency meter")), (Icon::Info, tr("About padmimic"))];
        let sel = if self.settings_open || self.tab == Tab::Settings { Some(3) } else { Some(match self.tab { Tab::Kbm => 1, Tab::Custom => 2, _ => 0 }) };
        match nav_list(ui, "nav", &items, sel, compact) {
            Some(0) => { self.settings_open = false; self.go(Tab::Controller) }
            Some(1) => { self.settings_open = false; self.go(Tab::Kbm) }
            Some(2) => { self.settings_open = false; self.go(Tab::Custom) }
            Some(3) => self.toggle_settings(&ctx),
            Some(4) => self.open(Dlg::Latency),
            Some(_) => self.open(Dlg::About),
            None => {}
        }
        ui.add_space(14.0);
        self.power(ui, compact);
        ui.add_space(20.0);
        self.sets(ui, compact);

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            self.profile_switcher(ui, compact);
            ui.add_space(8.0);
            if compact { return }
            let avail = match &*self.upd.lock().unwrap() { Upd::Available(r) => Some(r.version.clone()), _ => None };
            if let Some(v) = avail {
                if banner(ui, Icon::Up, &format!("{} v{v}", tr("Update")), c.amber).clicked() { self.open(Dlg::Update) }
            }
            let (status, vigem) = { let l = self.s.live.lock().unwrap(); (l.status.clone(), l.vigem) };
            if !vigem && status == engine::VIGEM_MISSING {
                let msg = self.vigem_msg.lock().unwrap().clone();
                let busy = msg.as_deref() == Some(INSTALLING);
                if let Some(m) = msg.clone().filter(|m| m != INSTALLING) { ui.label(RichText::new(tr(&m)).size(11.5).color(c.weak)); }
                let b = banner(ui, Icon::Pad, tr(if busy { INSTALLING } else { "Install ViGEmBus" }), c.acc)
                    .on_hover_text(tr("Driver for the virtual Xbox controller (by Nefarius, signed). Windows asks once for admin rights. padmimic connects by itself afterwards."));
                if b.clicked() && !busy {
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
            }
        });
    }

    /// A compact toggle with a single edge highlight as the state changes.
    fn power(&mut self, ui: &mut egui::Ui, compact: bool) {
        let c = pal(ui.visuals().dark_mode);
        let ctx = ui.ctx().clone();
        let on = self.s.on.load(Relaxed);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
        let t = anim(&ctx, resp.id.with("state"), on, 0.23);
        let hover = anim(&ctx, resp.id, resp.hovered(), 0.12);
        let p = ui.painter();
        let edge = c.line.lerp_to_gamma(c.acc, 0.43 * t);
        let fill = c.idle.lerp_to_gamma(c.hover, 0.55 * hover);
        let flash = (4.0 * t * (1.0 - t)).clamp(0.0, 1.0);
        if flash > 0.0 {
            for (spread, intensity) in [(3.0, 0.09), (1.5, 0.20), (0.5, 0.37)] {
                p.rect_stroke(rect.expand(spread), 12.0 + spread, Stroke::new(1.0, c.acc.gamma_multiply(flash * intensity)), StrokeKind::Inside);
            }
        }
        p.rect_filled(rect, 12.0, fill);
        p.rect_stroke(rect, 12.0, Stroke::new(1.0, edge), StrokeKind::Inside);

        let track_center = if compact { rect.center() } else { Pos2::new(rect.right() - 29.0, rect.center().y) };
        let track = Rect::from_center_size(track_center, vec2(38.0, 22.0));
        p.rect_filled(track, 11.0, c.hover.lerp_to_gamma(c.acc, t));
        p.rect_stroke(track, 11.0, Stroke::new(1.0, c.line.lerp_to_gamma(c.acc, t)), StrokeKind::Inside);
        let thumb = track_center + vec2(egui::lerp(-8.0..=8.0, t), 0.0);
        p.circle_filled(thumb, 7.0, c.weak.lerp_to_gamma(Color32::WHITE, t));

        let key = match self.st.toggle { Input::Key(k) => winio::key_name(k), Input::Pad(o) => format!("{} {}", tr("Pad"), o.label()) };
        let label = tr(if on { "ON" } else { "OFF" });
        if !compact {
            let state = p.text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, label, bold(13.0), c.text);
            let key = p.layout_no_wrap(key.clone(), mono(11.0), c.weak);
            let x = state.right() + 11.0;
            if x + key.size().x < track.left() - 7.0 {
                p.galley(Pos2::new(x, rect.center().y - key.size().y / 2.0), key, c.weak);
            }
        }
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let resp = if compact { resp.on_hover_text(format!("{label} · {key}")) } else { resp };
        if resp.clicked() {
            self.s.on.store(!on, Relaxed);
            ctx.request_repaint();
        }
    }

    /// Sets 1–8 as chips with one highlight that glides to the active set; the menu holds the set tools.
    fn sets(&mut self, ui: &mut egui::Ui, compact: bool) {
        let c = pal(ui.visuals().dark_mode);
        let cur = self.cur();
        ui.horizontal(|ui| {
            if !compact { ui.label(RichText::new(tr("Sets")).font(bold(14.0)).color(c.weak)); }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let m = icon_btn(ui, Icon::More, 28.0);
                egui::Popup::menu(&m).width(190.0).show(|ui| {
                    if ui.button(tr("Quick set")).clicked() {
                        let held = self.s.live.lock().unwrap().active.clone();
                        self.open(Dlg::QuickSet(held));
                    }
                    if ui.button(tr("Names")).clicked() { self.open(Dlg::Names) }
                    if ui.button(tr("Reset")).clicked() { self.open(Dlg::Reset) }
                });
            });
        });
        let cols = if compact { 1 } else { 4 };
        let gap = 6.0;
        let w = ((ui.available_width() - (cols - 1) as f32 * gap) / cols as f32).floor();
        let hgt = 36.0;
        let rows = 8 / cols;
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), rows as f32 * (hgt + gap) - gap), Sense::hover());
        let at = |i: usize| Rect::from_min_size(area.min + vec2((i % cols) as f32 * (w + gap), (i / cols) as f32 * (hgt + gap)), vec2(w, hgt));
        let p = ui.painter().clone();
        let mut hover = vec![0.0; 8];
        let mut hit = None;
        for i in 0..8 {
            let resp = ui.interact(at(i), ui.id().with(("set", i)), Sense::click());
            hover[i] = anim(ui.ctx(), resp.id, resp.hovered(), 0.12);
            let name = &self.p.set_names[i];
            let hint = if name.is_empty() { format!("{} {}", tr("Set"), i + 1) } else { name.clone() };
            if resp.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { hit = Some(i) }
        }
        for i in 0..8 { p.rect_filled(at(i), 18.0, c.idle.lerp_to_gamma(c.hover, hover[i])); }
        let x = anim_to(ui.ctx(), "set_x", at(cur).left(), 0.28);
        let y = anim_to(ui.ctx(), "set_y", at(cur).top(), 0.28);
        p.rect_filled(Rect::from_min_size(Pos2::new(x, y), vec2(w, hgt)), 18.0, c.acc);
        for i in 0..8 {
            let r = at(i);
            // text is white where the highlight covers the chip
            let cover = (1.0 - ((r.left() - x).abs() / w).min(1.0)) * (1.0 - ((r.top() - y).abs() / hgt).min(1.0));
            p.text(r.center(), Align2::CENTER_CENTER, format!("{}", i + 1), bold(14.0), c.text.lerp_to_gamma(Color32::WHITE, cover));
        }
        if let Some(i) = hit { *self.s.set_cmd.lock().unwrap() = Some(i) }
    }

    /// Profile at the bottom, like an account switcher: avatar, name, menu with every profile action.
    fn profile_switcher(&mut self, ui: &mut egui::Ui, compact: bool) {
        let c = pal(ui.visuals().dark_mode);
        let dirty = self.p != self.saved;
        let name = if self.path.is_empty() { tr("New").to_string() } else { stem(&self.path) };
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click());
        let h = anim(ui.ctx(), resp.id, resp.hovered(), 0.12);
        let p = ui.painter();
        if h > 0.0 { p.rect_filled(rect, 31.0, c.hover.gamma_multiply(h * 0.7)); }
        let av = Pos2::new(if compact { rect.center().x } else { rect.left() + 31.0 }, rect.center().y);
        p.circle_filled(av, 20.0, c.acc);
        let initial: String = name.chars().next().map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
        p.text(av, Align2::CENTER_CENTER, initial, bold(17.0), Color32::WHITE);
        if dirty { p.circle_filled(av + vec2(14.0, -14.0), 5.5, c.amber); p.circle_stroke(av + vec2(14.0, -14.0), 5.5, Stroke::new(2.0, c.side.to_opaque())); }
        if !compact {
            let max_w = rect.width() - 110.0;
            let g = p.layout(name.clone(), bold(15.0), c.text, max_w);
            p.galley(Pos2::new(av.x + 30.0, rect.center().y - 19.0), g, c.text);
            p.text(Pos2::new(av.x + 30.0, rect.center().y + 10.0), Align2::LEFT_CENTER, tr("Profile"), FontId::proportional(13.0), c.weak);
            icons::draw(p, Icon::More, Rect::from_center_size(Pos2::new(rect.right() - 24.0, rect.center().y), vec2(18.0, 18.0)), c.text);
        }
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let mut pick: Option<String> = None;
        egui::Popup::menu(&resp).width(260.0).show(|ui| {
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for p in &self.profiles {
                    if ui.selectable_label(*p == self.path, stem(p)).clicked() { pick = Some(p.clone()) }
                }
            });
            ui.separator();
            if ui.button(format!("+  {}", tr("New"))).clicked() { pick = Some(String::new()) }
            if ui.button(tr("Save")).clicked() { self.save() }
            if ui.button(tr("Save as")).clicked() { self.open(Dlg::SaveAs(stem(&self.path))) }
            if ui.button(tr("Load")).on_hover_text(tr("Load the profile from disk again")).clicked() && !self.path.is_empty() {
                let p = self.path.clone();
                self.load(&p);
            }
            if ui.button(tr("Profile settings")).clicked() { self.open(Dlg::Pref) }
            if ui.button(tr("Remove")).on_hover_text(tr("Remove from the list (the file stays)")).clicked() && !self.path.is_empty() {
                let p = self.path.clone();
                self.st.recent.retain(|r| *r != p);
                self.profiles.retain(|r| *r != p);
                self.path.clear();
            }
        });
        match pick {
            Some(p) if p.is_empty() => { self.p = Profile::default(); self.saved = self.p.clone(); self.path.clear() }
            Some(p) => self.load(&p),
            None => {}
        }
    }

    // ------------------------------------------------------------- settings: beside the window or inside

    /// Prefer the right side, considering all usable connected monitors when enabled.
    fn settings_beside(&mut self, ctx: &egui::Context) -> Option<bool> {
        if self.st.dock == 1 { return None }
        let (maxed, outer) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false), i.viewport().outer_rect));
        let outer = outer.filter(|_| !maxed)?;
        self.dock_desktop.settings_side(outer, ctx.pixels_per_point(), self.st.smart_docking, SIDE_W, SIDE_GAP)
    }

    fn toggle_settings(&mut self, ctx: &egui::Context) {
        if self.tab == Tab::Settings { let t = self.prev_tab; self.go(t); return }
        if self.settings_open { self.settings_open = false; return }
        match self.settings_beside(ctx) {
            Some(right) => { self.side_right = right; self.settings_open = true; self.side_applied = false }
            None => self.go(Tab::Settings),
        }
    }

    /// Animate the content of a fixed-size surface: avoid reallocating the native swapchain every frame.
    fn settings_side(&mut self, ctx: &egui::Context) {
        let mut desired_right = self.side_right;
        if self.settings_open {
            match self.settings_beside(ctx) {
                Some(right) => desired_right = right,
                None => { self.settings_open = false; self.go(Tab::Settings); }
            }
        }
        if ctx.input(|i| i.viewport().minimized.unwrap_or(false)) { self.settings_open = false }
        // Finish the fade-out on one side before revealing the panel on the other.
        let switching = self.settings_open && desired_right != self.side_right;
        let t = anim(ctx, "side", self.settings_open && !switching, 0.27);
        if t <= 0.0 {
            self.side_applied = false;
            if switching { self.side_right = desired_right; ctx.request_repaint(); }
            return;
        }
        if t < 1.0 { ctx.request_repaint(); }
        let e = t * t * (3.0 - 2.0 * t);
        let Some(outer) = ctx.input(|i| i.viewport().outer_rect) else { return };
        let x = if self.side_right { outer.right() + SIDE_GAP } else { outer.left() - SIDE_GAP - SIDE_W };
        let mut open = self.settings_open;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("settings"),
            egui::ViewportBuilder::default().with_title(SETTINGS_TITLE).with_decorations(false).with_transparent(true).with_taskbar(false)
                .with_has_shadow(false).with_resizable(false).with_position(Pos2::new(x, outer.top())).with_inner_size([SIDE_W, outer.height()]),
            |vui, _| {
                let vctx = vui.ctx().clone();
                if !self.side_applied && winio::own_window(SETTINGS_TITLE).is_some() {
                    winio::round_corners(SETTINGS_TITLE);
                    // same Mica / Acrylic as the main window, so both look alike over any background
                    winio::set_backdrop(SETTINGS_TITLE, self.material_kind(), vctx.theme() == Theme::Dark);
                    self.side_applied = true;
                }
                let c = pal(vctx.theme() == Theme::Dark);
                let full = vctx.content_rect();
                // plain fade in / out, nothing slides
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(vui, |ui| {
                    ui.set_opacity(e);
                    ui.painter().rect_filled(full, 18., c.side);
                    if self.aurora_on() { aurora(ui.painter(), full, vctx.input(|i| i.time) as f32, c.acc, vctx.theme() == Theme::Dark) }
                    let inner = Rect::from_min_max(full.min + vec2(20., 16.), full.max + vec2(-20., -16.));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(inner).id_salt("side"), |ui| {
                        ui.scope(|ui| {
                            ui.horizontal(|ui| {
                                ui.set_min_height(36.0);
                                ui.label(RichText::new(tr("Settings")).font(bold(22.0)));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if icon_btn(ui, Icon::Close, 34.0).clicked() { open = false }
                                });
                            });
                            ui.add_space(8.0);
                            self.settings_view(ui, true);
                        });
                    });
                });
                if vctx.input(|i| i.key_pressed(egui::Key::Escape) || i.viewport().close_requested()) && self.cap.is_none() { open = false }
            },
        );
        if !open { self.settings_open = false }
    }

    /// Settings: a list of pages; narrow = the page slides in over the list (with a back button),
    /// wide = list and page side by side.
    fn settings_view(&mut self, ui: &mut egui::Ui, narrow: bool) {
        let ctx = ui.ctx().clone();
        let full = ui.available_rect_before_wrap();
        if narrow {
            let t = ease(anim(&ctx, "push", self.set_detail, 0.3));
            let w = full.width();
            if t < 1.0 {
                ui.scope_builder(egui::UiBuilder::new().max_rect(full.translate(vec2(-t * w * 0.3, 0.0))).id_salt("set_list"), |ui| {
                    ui.set_opacity(1.0 - t * 0.7);
                    if let Some(i) = self.settings_list(ui) { self.set_page = i; self.set_detail = true }
                });
            }
            if t > 0.0 {
                let r = full.translate(vec2((1.0 - t) * w, 0.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(r).id_salt("set_page"), |ui| {
                    ui.horizontal(|ui| {
                        if icon_btn(ui, Icon::Back, 32.0).clicked() { self.set_detail = false }
                        ui.label(RichText::new(tr(SET_PAGES[self.set_page as usize % SET_PAGES.len()].1)).font(bold(18.0)));
                    });
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().id_salt(("side_scroll", self.set_page)).auto_shrink(false).show(ui, |ui| self.settings_page(ui));
                });
            }
            ui.allocate_rect(full, Sense::hover());
        } else {
            let list = Rect::from_min_size(full.min, vec2(260.0, full.height()));
            let page = Rect::from_min_max(Pos2::new(list.right() + 28.0, full.top()), Pos2::new(full.right().min(list.right() + 28.0 + 640.0), full.bottom()));
            ui.scope_builder(egui::UiBuilder::new().max_rect(list).id_salt("set_list"), |ui| {
                if let Some(i) = self.settings_list(ui) { self.set_page = i; self.page_t = Instant::now() }
            });
            ui.scope_builder(egui::UiBuilder::new().max_rect(page).id_salt("set_page"), |ui| {
                {
                    ui.label(RichText::new(tr(SET_PAGES[self.set_page as usize % SET_PAGES.len()].1)).font(bold(20.0)));
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().id_salt("set_scroll").auto_shrink(false).show(ui, |ui| self.settings_page(ui));
                }
            });
            ui.allocate_rect(full, Sense::hover());
        }
    }

    /// The page list: coloured icon tiles, names, chevrons, grouped like a phone's settings.
    fn settings_list(&mut self, ui: &mut egui::Ui) -> Option<u8> {
        let c = pal(ui.visuals().dark_mode);
        let mut hit = None;
        group(ui, |ui| {
            for (i, (icon, name)) in SET_PAGES.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::click());
                let h = anim(ui.ctx(), resp.id, resp.hovered(), 0.12);
                let sel = self.set_page as usize == i;
                let p = ui.painter();
                if h > 0.0 || sel { p.rect_filled(rect.expand2(vec2(8.0, -2.0)), 12.0, c.hover.gamma_multiply(if sel { 0.8 } else { h * 0.6 })); }
                let tile = Rect::from_center_size(Pos2::new(rect.left() + 16.0, rect.center().y), vec2(30.0, 30.0));
                let hue = [c.acc, Color32::from_rgb(142, 142, 147), Color32::from_rgb(40, 160, 255), Color32::from_rgb(255, 149, 0), Color32::from_rgb(52, 199, 89), Color32::from_rgb(175, 82, 222), Color32::from_rgb(64, 156, 255)][i];
                p.rect_filled(tile, 9.0, hue);
                icons::draw(p, *icon, tile.shrink(6.0), Color32::WHITE);
                p.text(Pos2::new(tile.right() + 14.0, rect.center().y), Align2::LEFT_CENTER, tr(name), FontId::proportional(15.0), c.text);
                icons::draw(p, Icon::Chevron, Rect::from_center_size(Pos2::new(rect.right() - 10.0, rect.center().y), vec2(14.0, 14.0)), c.weak);
                if i + 1 < SET_PAGES.len() { p.hline((tile.right() + 14.0)..=rect.right(), rect.bottom(), Stroke::new(1.0, c.line)); }
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { hit = Some(i as u8) }
            }
        });
        hit
    }

    /// Status line under the content: engine state on the left, numbers on the right.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let c = pal(ui.visuals().dark_mode);
        let (st, status) = { let l = self.s.live.lock().unwrap(); (l.stats, l.status.clone()) };
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr(&status)).size(12.0).color(c.weak));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!(
                    "pad {} · {} upd/s · engine {} Hz · virt {}",
                    st.slot.map_or("–".into(), |i| format!("#{}", i + 1)), st.pad_hz, st.loop_hz,
                    st.virt_slot.map_or("–".into(), |i| format!("#{}", i + 1))
                )).font(mono(11.0)).color(c.weak.gamma_multiply(0.8)));
            });
        });
    }

    // ------------------------------------------------------------- window background & overlays

    fn material_kind(&self) -> i32 {
        match self.st.backdrop { 2 => 2, 3 => 3, _ => 0 }
    }

    /// Own animated gradient: chosen, or standing in where Mica / Acrylic is not available.
    fn aurora_on(&self) -> bool {
        self.st.backdrop == 1 || (self.st.backdrop >= 2 && !self.material_ok)
    }

    /// Theme, accent, glass and the Windows material, applied whenever one of them changes.
    fn look(&mut self, ctx: &egui::Context) {
        let automatic = self.st.pad_accent && (self.st.pad_skin.is_empty() || self.st.pad_skin.starts_with("builtin:"));
        let color = automatic.then(|| crate::pad::MODELS[crate::pad::selected_index(&self.st.pad_skin,self.st.pad_model)].accent());
        let color_changed = set_design_accent(color);
        let key = (self.st.theme, self.st.accent, self.st.backdrop, (self.st.opacity * 100.0) as u32, self.st.motion, self.st.size);
        if (self.look_sent == Some(key) && !color_changed) || ctx.cumulative_frame_nr() < 2 { return }
        if self.look_sent.is_some_and(|k| k.5 != key.5) {
            // keep the window's look: grow / shrink it with the zoom
            let (old, new) = (SIZES[self.look_sent.unwrap().5 as usize % 3], SIZES[self.st.size as usize % 3]);
            if let Some(r) = ctx.input(|i| i.viewport().inner_rect) { ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(r.size() * old / new)) }
            ctx.set_zoom_factor(new);
        }
        self.look_sent = Some(key);
        ctx.set_theme(match self.st.theme { 0 => egui::ThemePreference::System, 2 => egui::ThemePreference::Light, _ => egui::ThemePreference::Dark });
        let dark = match self.st.theme { 0 => ctx.system_theme().is_none_or(|t| t == Theme::Dark), t => t != 2 };
        winio::round_corners("padmimic");
        let kind = self.material_kind();
        self.material_ok = winio::set_backdrop("padmimic", kind, dark) || kind == 0;
        self.side_applied = false;
        set_look(self.st.accent, if self.st.backdrop == 0 { 1.0 } else { self.st.opacity }, self.st.motion);
        apply_colors(ctx);
    }

    /// Toast at the bottom of the window.
    fn overlays(&mut self, ctx: &egui::Context) {
        let c = pal(ctx.theme() == Theme::Dark);
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("fx")));
        let full = ctx.content_rect();
        if let Some((text, at)) = &self.toast {
            let s = at.elapsed().as_secs_f32();
            if s < 3.0 {
                ctx.request_repaint();
                let a = if motion() { ease(s / 0.3) * (1.0 - ((s - 2.6) / 0.4).clamp(0.0, 1.0)) } else { 1.0 };
                let g = p.layout_no_wrap(text.clone(), bold(14.0), Color32::WHITE);
                let r = Rect::from_center_size(Pos2::new(full.center().x, full.bottom() - 64.0 + (1.0 - a) * 24.0), g.size() + vec2(40.0, 22.0));
                p.rect_filled(r.translate(vec2(0.0, 4.0)), 22.0, Color32::from_black_alpha((60.0 * a) as u8));
                p.rect_filled(r, 22.0, c.acc.gamma_multiply(a));
                p.galley_with_override_text_color(r.center() - g.size() / 2.0, g, Color32::WHITE.gamma_multiply(a));
            }
        }
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
        let marks: Vec<Icon> = [(b.toggle, Icon::Swap), (b.turbo, Icon::Bolt)].into_iter().filter(|m| m.0).map(|m| m.1).collect();
        let r = cell_ex(ui, None, &marks, title, &summary_core(b), active.contains(&e), size).on_hover_text(hint);
        if r.clicked() {
            self.open(Dlg::Button(e));
        }
        let mut clear = false;
        if used {
            r.context_menu(|ui| {
                if ui.button(tr("Remove assignment")).clicked() {
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
        let w = ((ui.available_width() - 16.0) / 3.0).clamp(60.0, 120.0);
        let sz = vec2(w, 50.0);
        const LAYOUT: [[i8; 3]; 3] = [[7, 0, 1], [6, -1, 2], [5, 4, 3]];
        egui::Grid::new(center).spacing([8.0, 8.0]).show(ui, |ui| {
            for row in LAYOUT {
                for d in row {
                    if d < 0 {
                        if cell_ex(ui, Some(Icon::Gear), &[], "", center, centre_active, sz).clicked() { self.open(dlg.clone()) }
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
            card(ui, &format!("{} · {hz} {}", tr("Live"), tr("upd/s")), |ui| self.live_view(ui, &phys, vigem.then_some(&out)));
            ui.add_space(12.0);
        }
        let full = ui.available_width();
        let cols = if full > 700.0 { 3 } else if full > 460.0 { 2 } else { 1 };
        let colw = (full - (cols - 1) as f32 * 12.0) / cols as f32;
        let blocks: Vec<u8> = vec![0, 1, 2, 3, 4];
        for chunk in blocks.chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for &b in chunk {
                    ui.allocate_ui(vec2(colw, 0.0), |ui| {
                        ui.set_width(colw);
                        match b {
                            0 | 1 => {
                                let s = b;
                                let sname = self.p.sets[self.cur()].sticks[s as usize].name.clone();
                                card(ui, if sname.is_empty() { tr(STICK_NAMES[s as usize]) } else { &sname }, |ui| {
                                    let centre = active.iter().any(|e| matches!(e, Elem::Stick(x, _) if *x == s));
                                    self.dir_grid(ui, |d| Elem::Stick(s, d), tr("Stick"), Dlg::Stick(s), active, centre);
                                    let w = ui.available_width();
                                    self.btn_cell(ui, Elem::StickMod(s), tr("Modifier"), vec2(w, 40.0), active);
                                });
                            }
                            2 => {
                                let name = self.p.sets[self.cur()].dpad.name.clone();
                                card(ui, if name.is_empty() { "D-Pad" } else { &name }, |ui| {
                                    let centre = active.iter().any(|e| matches!(e, Elem::DPad(_)));
                                    self.dir_grid(ui, Elem::DPad, "D-Pad", Dlg::DPad, active, centre);
                                });
                            }
                            3 => {
                                card(ui, tr("Triggers"), |ui| {
                                    for t in 0..2u8 {
                                        let a = &self.p.sets[self.cur()].triggers[t as usize];
                                        let neg = a.throttle < 0;
                                        let title = if a.name.is_empty() { TRIG_NAMES[t as usize].to_string() } else { a.name.clone() };
                                        let analog = match a.analog { Some(VTrigger::Left) => " → LT", Some(VTrigger::Right) => " → RT", None => "" };
                                        ui.horizontal(|ui| {
                                            let pressed = active.contains(&Elem::Trig(t, true)) || active.contains(&Elem::Trig(t, false));
                                            if cell_ex(ui, Some(Icon::Gear), &[], "", &format!("{title}{analog}"), pressed, vec2(ui.available_width() * 0.5, 50.0)).clicked() { self.open(Dlg::Axis(t)) }
                                            let w = ui.available_width();
                                            if neg {
                                                self.btn_cell(ui, Elem::Trig(t, false), "−", vec2(w / 2.0 - 4.0, 50.0), active);
                                                self.btn_cell(ui, Elem::Trig(t, true), "+", vec2(w / 2.0 - 4.0, 50.0), active);
                                            } else {
                                                self.btn_cell(ui, Elem::Trig(t, true), tr("Press"), vec2(w, 50.0), active);
                                            }
                                        });
                                    }
                                });
                            }
                            _ => {
                                card(ui, tr("Buttons"), |ui| {
                                    let w = ((ui.available_width() - 16.0) / 3.0).max(60.0);
                                    egui::Grid::new("buttons").spacing([8.0, 8.0]).show(ui, |ui| {
                                        for i in 0..11u8 {
                                            self.btn_cell(ui, Elem::Btn(i), BTN_NAMES[i as usize], vec2(w, 50.0), active);
                                            if i % 3 == 2 { ui.end_row() }
                                        }
                                    });
                                });
                            }
                        }
                    });
                }
            });
            ui.add_space(12.0);
        }
    }

    /// The chosen controller live: an own skin from skins/, else the built-in model.
    fn live_view(&mut self, ui: &mut egui::Ui, phys: &XGamepad, out: Option<&XGamepad>) {
        if !self.skins_loaded { self.reload_skins(ui.ctx()) }
        if self.tab == Tab::Controller {
            // pop the controller out into its own overlay window, or back in
            let at = Rect::from_min_size(Pos2::new(ui.max_rect().right() - 30.0, ui.max_rect().top() - 5.0), vec2(30.0, 30.0));
            let on = self.st.overlay.enabled;
            let tip = tr(if on { "Close the controller overlay" } else { "Pop the controller out into its own window" });
            if ui.put(at, |ui: &mut egui::Ui| icon_btn(ui, if on { Icon::Close } else { Icon::PopOut }, 30.0)).on_hover_text(tip).clicked() {
                self.st.overlay.enabled = !on;
            }
        }
        if let Some(s) = self.skins.iter().find(|s| s.id == self.st.pad_skin) {
            let w = ui.available_width().min(520.0);
            let hgt = w * s.aspect();
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), hgt), Sense::hover());
            s.paint(ui.painter(), Rect::from_center_size(r.center(), vec2(w, hgt)), phys);
        } else {
            let sticks = self.p.sets[self.cur()].sticks.clone();
            crate::pad::controller_sized(ui, crate::pad::selected_index(&self.st.pad_skin, self.st.pad_model) as u8, phys, out, &sticks, &mut self.trail, if self.tab == Tab::Custom { 300.0 } else { 440.0 });
        }
    }

    fn reload_skins(&mut self, ctx: &egui::Context) {
        (self.skins, self.skin_errs) = crate::skin::scan(ctx);
        self.skins_loaded = true;
    }

    /// Customization: live preview of the choice, filter, a shop-style grid of controllers and
    /// own skins, and the skins folder.
    fn custom_tab(&mut self, ui: &mut egui::Ui) {
        use crate::pad::{self, MODELS};
        let (phys, out, vigem) = { let l = self.s.live.lock().unwrap(); (l.phys, l.out, l.vigem) };
        let c = pal(ui.visuals().dark_mode);
        if !self.skins_loaded { self.reload_skins(ui.ctx()) }
        let title = self.skins.iter().find(|s| s.id == self.st.pad_skin).map(|s| s.name.clone()).unwrap_or_else(|| MODELS[pad::selected_index(&self.st.pad_skin, self.st.pad_model)].name.into());
        card(ui, &title, |ui| self.live_view(ui, &phys, vigem.then_some(&out)));

        ui.add_space(16.0);
        segmented(ui, "cust_filter", &mut self.cust_filter, &[tr("All"), "PS5", "PS4", "Xbox", tr("My skins")]);
        ui.add_space(14.0);
        let gap = 14.0;
        let full = ui.available_width();
        let cols = ((full + gap) / (230.0 + gap)).floor().max(1.0);
        let tw = ((full - gap * (cols - 1.0)) / cols).floor();
        let f = self.cust_filter;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.cust_search).hint_text(tr("Search controllers…")).desired_width((full - 100.0).max(100.0)));
            ui.weak(format!("{}", MODELS.len()));
        });
        ui.add_space(12.0);
        let search = self.cust_search.trim().to_lowercase();
        let selected = pad::selected_index(&self.st.pad_skin, self.st.pad_model);
        let mut visible = 0;
        let mut pick: Option<(u8, String)> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(gap, gap);
            if f < 4 {
                for (mi, md) in MODELS.iter().enumerate().filter(|(_, m)| m.matches_filter(f) && (m.name.to_lowercase().contains(&search) || m.id.contains(&search) || m.tag().to_lowercase().contains(&search))) {
                    visible += 1;
                    let chosen = !self.skins.iter().any(|s| s.id == self.st.pad_skin) && selected == mi;
                    let tag = if md.animated() { format!("{} · {}", md.tag(), tr("Animated")) } else { md.tag().into() };
                    let id = ui.id().with(("tile", mi));
                    if pad::tile(ui, tw, md.name, &tag, chosen, |p, r, demo| { if let Some(g) = demo { pad::paint_model(p, id, r, mi, &g) } else { pad::thumbnail(p, r, mi) } }).clicked() {
                        pick = Some((mi as u8, format!("builtin:{}", md.id)));
                    }
                }
            }
            if f == 0 || f == 4 {
                for s in self.skins.iter().filter(|s| s.name.to_lowercase().contains(&search)) {
                    visible += 1;
                    if pad::tile(ui, tw, &s.name, tr("My skin"), self.st.pad_skin == s.id, |p, r, demo| s.paint(p, r, &demo.unwrap_or_default())).clicked() {
                        pick = Some((self.st.pad_model, s.id.clone()));
                    }
                }
            }
        });
        if let Some((m, s)) = pick { self.st.pad_model = m; self.st.pad_skin = s; }
        if visible == 0 { ui.label(RichText::new(tr("No matching controllers.")).color(c.weak)); }

        ui.add_space(18.0);
        let mut reload = false;
        card(ui, tr("Your own skins"), |ui| {
            ui.label(RichText::new(tr("Add a folder with a skin.json and its images to the skins folder. Your own skins appear alongside the included controller designs.")).color(c.weak));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if pill(ui, tr("Open skins folder"), true).clicked() {
                    let d = crate::skin::dir();
                    let _ = std::fs::create_dir_all(&d);
                    let _ = std::process::Command::new("explorer").arg(&d).spawn();
                }
                if pill(ui, tr("Reload"), false).clicked() { reload = true }
            });
            for e in &self.skin_errs { ui.label(RichText::new(e).size(12.0).color(c.amber)); }
        });
        if reload { self.reload_skins(ui.ctx()) }
    }

    fn kbm_tab(&mut self, ui: &mut egui::Ui, active: &[Elem]) {
        card(ui, tr("Keyboard & mouse → assignment"), |ui| {
            ui.weak(tr("Real keys / mouse buttons trigger actions – e.g. controller buttons for mouse & keyboard players."));
            let mut remove = None;
            let n = self.p.sets[self.cur()].keys.len();
            for i in 0..n {
                ui.horizontal(|ui| {
                    let vk = self.p.sets[self.cur()].keys[i].vk;
                    let label = if self.capturing(Cap::KeyBind(i)) { tr("Press a key…").into() } else { winio::key_name(vk) };
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
            if ui.button(format!("+  {}", tr("Assignment"))).clicked() {
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

    // ------------------------------------------------------------- dialogs

    fn dialogs(&mut self, ctx: &egui::Context) {
        let list = self.dlgs.clone();
        self.dlg_t.resize(list.len(), Instant::now());
        let mut close = vec![];
        let solid = pal_with(ctx.theme() == Theme::Dark, 1.0);
        for (i, d) in list.iter().enumerate() {
            if *d == Dlg::Latency {
                if !self.latency_window(ctx) { close.push(d.clone()) }
                continue;
            }
            let (title, width) = match d {
                Dlg::Button(e) => (format!("{} – {}", tr("Assignment"), elem_name(&self.p.sets[self.cur()], *e)), 640.0),
                Dlg::Advanced(e) => (format!("{} – {}", tr("Advanced"), elem_name(&self.p.sets[self.cur()], *e)), 660.0),
                Dlg::Stick(s) => (tr(STICK_NAMES[*s as usize]).to_string(), 560.0),
                Dlg::DPad => ("D-Pad".into(), 380.0),
                Dlg::Axis(t) => (format!("{} {}", tr("Axis"), TRIG_NAMES[*t as usize]), 420.0),
                Dlg::Mouse(_) => (tr("Mouse settings").into(), 760.0),
                Dlg::Names => (tr("Set names").into(), 340.0),
                Dlg::Pref => (tr("Profile settings").into(), 400.0),
                Dlg::QuickSet(_) => (tr("Quick set").into(), 340.0),
                Dlg::SaveAs(_) => (tr("Save as").into(), 340.0),
                Dlg::Reset => (tr("Reset").into(), 380.0),
                Dlg::About => (tr("About padmimic").into(), 380.0),
                Dlg::Latency => (tr("Latency meter").into(), 560.0),
                Dlg::Update => (tr("Update").into(), 400.0),
            };
            // sheets: centred, dimmed backdrop, fade in while rising a little; not movable
            let e = if motion() { ease(self.dlg_t[i].elapsed().as_secs_f32() / 0.26) } else { 1.0 };
            if e < 1.0 { ctx.request_repaint() }
            let id = egui::Id::new(("sheet", i, format!("{:?}", std::mem::discriminant(d))));
            let area = egui::Modal::default_area(id).pivot(Align2::CENTER_CENTER).fixed_pos(ctx.content_rect().center() + vec2(0.0, (1.0 - e) * 22.0));
            let frame = egui::Frame::new().fill(solid.card).stroke(Stroke::new(1.0, solid.line)).corner_radius(26).inner_margin(24)
                .shadow(egui::Shadow { offset: [0, 18], blur: 50, spread: 0, color: Color32::from_black_alpha((120.0 * e) as u8) });
            let mut closed = false;
            let mut done = false;
            let r = egui::Modal::new(id).area(area).frame(frame).backdrop_color(Color32::from_black_alpha((110.0 * e) as u8)).show(ctx, |ui| {
                ui.set_opacity(e);
                ui.set_min_width(width);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&title).font(bold(19.0)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_btn(ui, Icon::Close, 32.0).clicked() { closed = true }
                    });
                });
                ui.add_space(6.0);
                let max_h = ctx.content_rect().height() - 160.0;
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
                    Dlg::SaveAs(_) => self.dlg_save_as(ui, i),
                    Dlg::Reset => self.dlg_reset(ui),
                    Dlg::About => self.dlg_about(ui),
                    Dlg::Latency => true, // own window, see latency_window
                    Dlg::Update => self.dlg_update(ui),
                });
            });
            let top = i + 1 == list.len();
            if closed || done || (top && r.should_close() && self.cap.is_none()) { close.push(d.clone()) }
        }
        let keep: Vec<bool> = self.dlgs.iter().map(|d| !close.contains(d)).collect();
        let mut k = keep.iter();
        self.dlg_t.retain(|_| *k.next().unwrap_or(&true));
        self.dlgs.retain(|d| !close.contains(d));
    }

    fn ok_row(ui: &mut egui::Ui) -> bool {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                pill(ui, tr("Done"), true).clicked()
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
            cap = ui.button(if capturing { tr("Press…") } else { tr("Record") }).clicked();
            // everything off: outputs, turbo, toggle, set change, name
            if ui.button(tr("Remove")).on_hover_text(tr("Remove the whole assignment")).clicked() { *b = Button::default() }
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
            ui.label(RichText::new(summary(&Button { action: String::new(), ..b.clone() })).font(bold(14.0)));
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
                                if ui.button(if capturing_slot == Some(sel) { tr("Press…") } else { tr("Record") }).clicked() { cap_slot = Some(sel) }
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
                    ui.selectable_value(&mut s.analog, Some(VStick::Left), "L-Stick");
                    ui.selectable_value(&mut s.analog, Some(VStick::Right), "R-Stick");
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
            ui.selectable_value(&mut a.analog, Some(VTrigger::Left), "LT");
            ui.selectable_value(&mut a.analog, Some(VTrigger::Right), "RT");
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

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        let toggle_cap = self.capturing(Cap::Toggle);
        let mut cap_toggle = false;
        let mut pick_lang = None;
        let mut check_now = false;
        let mut defaults = false;
        let material_ok = self.material_ok;
        let st = &mut self.st;
        match self.set_page {
            0 => {
                caption(ui, tr("Size"));
                group(ui, |ui| segmented(ui, "size", &mut st.size, &[tr("Small"), tr("Medium"), tr("Large")]));
                caption(ui, tr("Theme"));
                group(ui, |ui| segmented(ui, "theme", &mut st.theme, &[tr("System"), tr("Dark"), tr("Light")]));
                caption(ui, tr("Accent colour"));
                group(ui, |ui| {
                    row_switch(ui, &mut st.pad_accent, tr("Match controller colours"));
                    ui.add_enabled_ui(!st.pad_accent, |ui| accent_picker(ui, &mut st.accent));
                });
                caption(ui, tr("Background"));
                group(ui, |ui| {
                    backdrop_picker(ui, &mut st.backdrop);
                    if st.backdrop >= 2 && !material_ok { ui.label(RichText::new(tr("Needs Windows 11 – Aurora is shown instead.")).size(12.0).weak()); }
                    if st.backdrop != 0 {
                        let mut pct = ((1.0 - st.opacity) * 100.0).round();
                        if rg(tr("Transparency")).suffix(" %").dec(0).show(ui, &mut pct, 0.0..=80.0).changed() { st.opacity = 1.0 - pct / 100.0 }
                    }
                });
                caption(ui, tr("Settings open"));
                group(ui, |ui| {
                    segmented(ui, "dock", &mut st.dock, &[tr("Beside the window"), tr("Inside the app")]);
                    ui.add_enabled_ui(st.dock == 0, |ui| { row_switch(ui, &mut st.smart_docking, tr("Smart positioning across monitors")); });
                    ui.label(RichText::new(tr("Switches left only at the desktop edge. Turn off to use each monitor separately.")).size(12.).weak());
                });
                ui.add_space(8.0);
                group(ui, |ui| { row_switch(ui, &mut st.motion, tr("Animations")); });
            }
            1 => {
                group(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr("Profile folder:"));
                        ui.add(egui::TextEdit::singleline(&mut st.profile_dir).desired_width(f32::INFINITY));
                    });
                    rg(tr("Recent profiles")).show(ui, &mut st.recent_count, 1..=100);
                });
                group(ui, |ui| {
                    row_switch(ui, &mut st.auto_load_last, tr("Load the last profile on start"));
                    row_switch(ui, &mut st.startup, tr("Start with Windows"));
                    row_switch(ui, &mut st.hide_empty, tr("Hide empty buttons"));
                    row_switch(ui, &mut st.numpad, tr("Show the numpad on the keyboard"));
                });
                group(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(40.0);
                        ui.label(tr("On/off hotkey:"));
                        let l = if toggle_cap { tr("Press…").to_string() } else { match st.toggle { Input::Key(k) => winio::key_name(k), Input::Pad(o) => format!("{} {}", tr("Pad"), o.label()) } };
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| cap_toggle = ui.button(l).clicked());
                    });
                    row_switch(ui, &mut st.check_updates, tr("Check for updates on start"));
                    ui.horizontal(|ui| {
                        check_now = ui.button(tr("Check now")).clicked();
                        let status = match &*self.upd.lock().unwrap() {
                            Upd::Checking => tr("Checking…").to_string(),
                            Upd::Latest => tr("padmimic is up to date").to_string(),
                            Upd::Available(r) => format!("v{}", r.version),
                            Upd::Failed(e) => tr(e).to_string(),
                            _ => String::new(),
                        };
                        ui.weak(status);
                    });
                });
                caption(ui, tr("Key repeat"));
                group(ui, |ui| {
                    row_switch(ui, &mut st.key_repeat, tr("Active"));
                    ui.add_enabled_ui(st.key_repeat, |ui| rg(tr("Delay")).suffix(" ms").show(ui, &mut st.key_repeat_delay, 0..=1000));
                    ui.add_enabled_ui(st.key_repeat, |ui| rg(tr("Rate")).suffix("/s").show(ui, &mut st.key_repeat_rate, 5..=250));
                });
                ui.add_space(10.0);
                defaults = pill(ui, tr("Defaults"), false).clicked();
            }
            2 => pick_lang = lang_grid(ui, i18n::cur(), &[]),
            3 => {
                ui.label(RichText::new(tr("Loads a profile automatically as soon as the program is in the foreground.")).weak());
                ui.add_space(4.0);
                group(ui, |ui| {
                    row_switch(ui, &mut st.auto_profile, tr("Active"));
                    let mut remove = None;
                    for (i, a) in st.auto_profiles.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut a.exe).hint_text("game.exe").desired_width(120.0));
                            ui.label("→");
                            egui::ComboBox::from_id_salt(("ap", i)).width(130.0).selected_text(stem(&a.profile)).show_ui(ui, |ui| {
                                for p in &self.profiles { ui.selectable_value(&mut a.profile, p.clone(), stem(p)); }
                            });
                            if icon_btn(ui, Icon::Trash, 28.0).clicked() { remove = Some(i) }
                        });
                    }
                    if let Some(i) = remove { st.auto_profiles.remove(i); }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(format!("+  {}", tr("Add"))).clicked() { st.auto_profiles.push(AutoProfile { exe: String::new(), profile: String::new() }) }
                        if ui.button(tr("Current foreground program")).on_hover_text(tr("The active program is filled in after 3 s")).clicked() {
                            st.auto_profiles.push(AutoProfile { exe: "…".into(), profile: self.path.clone() });
                        }
                    });
                });
            }
            4 => {
                group(ui, |ui| { row_switch(ui, &mut st.disable_epp, tr("Turn off Windows “Enhance pointer precision”")); });
                caption(ui, tr("Smoothing"));
                group(ui, |ui| {
                    row_switch(ui, &mut st.smoothing, tr("Active"));
                    ui.add_enabled_ui(st.smoothing, |ui| rg(tr("History size")).show(ui, &mut st.history, 1..=100));
                    ui.add_enabled_ui(st.smoothing, |ui| rg(tr("Weight")).show(ui, &mut st.weight, 0.0..=1.0));
                });
                ui.add_space(8.0);
                group(ui, |ui| { rg(tr("Mouse refresh")).suffix(" ms").show(ui, &mut st.mouse_refresh, 1..=16); });
            }
            6 => {
                group(ui, |ui| {
                    row_switch(ui, &mut st.overlay.enabled, tr("Show controller overlay"));
                    row_switch(ui, &mut st.overlay.always_on_top, tr("Always on top"));
                    row_switch(ui, &mut st.overlay.locked, tr("Lock overlay (pass mouse clicks through)"));
                });
                ui.label(RichText::new(tr("Uses your selected controller with live inputs. Transparent background, no glow.")).weak());
                caption(ui, tr("Size and position"));
                group(ui, |ui| {
                    let mut width = st.overlay.size[0];
                    if rg(tr("Width")).suffix(" px").show(ui, &mut width, 160.0..=1600.0).changed() {
                        let size = crate::overlay::default_size(width);
                        st.overlay.size = [size.x, size.y];
                    }
                    if ui.button(tr("Reset size and position")).clicked() {
                        st.overlay.size = OverlaySettings::default().size;
                        // Put a lost/offscreen overlay back beside the main window's top-left.
                        let position = ui.ctx().input(|i| i.raw.viewports.get(&egui::ViewportId::ROOT).and_then(|v| v.inner_rect).map(|r| r.min + vec2(40., 40.)));
                        st.overlay.position = Some(position.map_or([100., 100.], |p| [p.x, p.y]));
                    }
                });
                ui.label(RichText::new(tr("Left-drag to move. Drag any edge or corner to resize. Unlock the overlay first.")).weak());
                ui.add_space(8.0);
                caption(ui, tr("Streaming"));
                ui.label(RichText::new(tr("For Discord, share the screen containing the overlay. Sharing only a game window may not include a separate overlay.")).weak());
            }
            _ => {
                group(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(40.0);
                        ui.label(tr("Controller polling:"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            egui::ComboBox::from_id_salt("hz").selected_text(format!("{} Hz", st.hz)).show_ui(ui, |ui| {
                                for hz in [125, 250, 500, 1000, 2000, 4000, 8000] { ui.selectable_value(&mut st.hz, hz, format!("{hz} Hz")); }
                            });
                        });
                    });
                });
                ui.add_space(6.0);
                ui.label(RichText::new(tr("How often controller, keyboard and mouse are read and outputs are sent (high-resolution timer, highest thread priority).")).weak());
                ui.label(RichText::new(tr("Hide the real controller with HidHide and allow padmimic.exe there, so games only see the virtual pad.")).weak());
            }
        }
        if cap_toggle { self.cap = Some((Cap::Toggle, false)) }
        if let Some(l) = pick_lang { self.set_lang(ui.ctx(), l) }
        if check_now { self.check_updates() }
        if defaults {
            let s = &self.st;
            self.st = Settings {
                recent: s.recent.clone(), last_profile: s.last_profile.clone(), lang: s.lang.clone(),
                theme: s.theme, accent: s.accent, pad_accent: s.pad_accent, backdrop: s.backdrop, opacity: s.opacity, dock: s.dock, smart_docking: s.smart_docking, tray_variant: s.tray_variant, motion: s.motion, size: s.size,
                ..Settings::default()
            }
        }
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
                ui.label(RichText::new(i18n::tr_in(id, "Change language?")).size(20.0));
                ui.label(RichText::new("Change language?").small().color(c.weak));
                ui.add_space(10.0);
                let big = egui::Button::new(RichText::new(i18n::native(id)).size(17.0).color(Color32::WHITE)).fill(c.acc).min_size(vec2(260.0, 38.0));
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
                        let b = egui::Button::new(RichText::new(tr("Update now")).color(Color32::WHITE)).fill(accent(ui.visuals().dark_mode)).min_size(vec2(110.0, 28.0));
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
        ui.label(RichText::new(format!("padmimic {}", env!("CARGO_PKG_VERSION"))).font(bold(16.0)));
        ui.label(tr("Controller, keyboard & mouse → keyboard, mouse and a virtual Xbox 360 controller."));
        ui.weak(tr("Mapping logic (slots, turbo, sets, zones, curves) modelled on AntiMicroX."));
        ui.weak(tr("Open source under the MIT License."));
        if let Some(url) = update::repository_url() { ui.hyperlink_to("GitHub", url); }
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
                window_edges(&ctx, false);
                let c = pal(ctx.theme() == Theme::Dark);
                egui::CentralPanel::default().frame(egui::Frame::new().fill(c.panel).inner_margin(14)).show(vui, |ui| {
                    // own small title bar: drag anywhere on it, close on the right
                    let row = Rect::from_min_size(ui.cursor().min - vec2(14.0, 14.0), vec2(ui.available_width() + 28.0, 44.0));
                    let bar = ui.interact(row, ui.id().with("latbar"), Sense::click_and_drag());
                    if bar.drag_started_by(egui::PointerButton::Primary) { ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag) }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr("Latency meter")).font(bold(18.0)));
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

// ================================================================= settings parts

const SETTINGS_TITLE: &str = "padmimic settings";
const SIDE_W: f32 = 440.0;
const SIDE_GAP: f32 = 10.0;
const SET_PAGES: [(Icon, &str); 7] = [
    (Icon::Palette, "Appearance"), (Icon::Sliders, "General"), (Icon::Globe, "Language"),
    (Icon::Apps, "Auto profile"), (Icon::Mouse, "Mouse"), (Icon::Chip, "Engine"),
    (Icon::Pad, "Controller overlay"),
];

/// Grouped block of settings rows on a rounded card.
fn group<R>(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let c = pal(ui.visuals().dark_mode);
    let r = egui::Frame::new().fill(c.card).corner_radius(18).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        f(ui)
    });
    ui.add_space(6.0);
    r.inner
}

/// Small heading above a group.
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).font(bold(13.0)).color(pal(ui.visuals().dark_mode).weak));
}

/// Coloured dots; the chosen one gets a ring and a check, hovering grows a dot a little.
fn accent_picker(ui: &mut egui::Ui, sel: &mut u8) {
    let c = pal(ui.visuals().dark_mode);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for (i, (name, rgb)) in ACCENTS.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(40.0, 44.0), Sense::click());
            let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            let h = anim(ui.ctx(), resp.id, resp.hovered(), 0.12);
            let on = *sel as usize == i;
            let o = anim(ui.ctx(), resp.id.with("on"), on, 0.2);
            let p = ui.painter();
            p.circle_filled(r.center(), 14.0 + 2.0 * h, col);
            if o > 0.0 {
                p.circle_stroke(r.center(), 18.5 + 2.0 * h, Stroke::new(2.0, col.gamma_multiply(o)));
                icons::draw(p, Icon::Check, Rect::from_center_size(r.center(), vec2(16.0 * o, 16.0 * o)), Color32::WHITE);
            }
            let _ = c;
            if resp.on_hover_text(tr(name)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { *sel = i as u8 }
        }
    });
}

/// Background choice as small window previews.
fn backdrop_picker(ui: &mut egui::Ui, sel: &mut u8) {
    let c = pal(ui.visuals().dark_mode);
    let dark = ui.visuals().dark_mode;
    let names = ["Solid", "Aurora", "Mica", "Acrylic"];
    let gap = 10.0;
    let cols = if ui.available_width() > 420.0 { 4 } else { 2 };
    let w = (ui.available_width() - gap * (cols - 1) as f32) / cols as f32;
    let t = ui.input(|i| i.time) as f32;
    egui::Grid::new("backdrops").spacing([gap, gap]).show(ui, |ui| {
        for (i, name) in names.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(w, 96.0), Sense::click());
            let on = *sel as usize == i;
            let h = anim(ui.ctx(), resp.id, resp.hovered(), 0.12);
            let p = ui.painter();
            let prev = Rect::from_min_max(r.min, Pos2::new(r.right(), r.bottom() - 24.0)).shrink(2.0);
            let base = if dark { Color32::from_rgb(10, 10, 12) } else { Color32::from_rgb(240, 241, 244) };
            p.rect_filled(prev, 12.0, base);
            let clip = p.with_clip_rect(prev.shrink(1.0));
            match i {
                1 => aurora(&clip, prev, t, c.acc, dark),
                2 => { // mica: wallpaper colour washed into the surface
                    clip.rect_filled(prev, 12.0, c.acc.gamma_multiply(0.18));
                    clip.circle_filled(prev.left_top() + vec2(10.0, 8.0), prev.width() * 0.6, Color32::from_rgb(120, 90, 200).gamma_multiply(0.18));
                }
                3 => { // acrylic: blurred shapes behind frosted glass
                    for (k, col) in [(0.0, Color32::from_rgb(255, 120, 80)), (1.0, Color32::from_rgb(60, 160, 255)), (2.0, Color32::from_rgb(80, 220, 140))] {
                        clip.circle_filled(prev.left_center() + vec2(prev.width() * (0.2 + 0.3 * k), (k - 1.0) * 10.0), 22.0, col.gamma_multiply(0.55));
                    }
                    clip.rect_filled(prev, 12.0, if dark { Color32::from_black_alpha(110) } else { Color32::from_white_alpha(110) });
                }
                _ => {}
            }
            // a tiny window on top: sidebar + two cards
            let win = prev.shrink2(vec2(prev.width() * 0.16, 14.0));
            let glass = if i == 0 { 1.0 } else { 0.55 };
            let card = if dark { Color32::from_rgb(28, 30, 34) } else { Color32::WHITE };
            clip.rect_filled(win, 6.0, card.gamma_multiply(glass));
            clip.rect_filled(Rect::from_min_size(win.min + vec2(5.0, 5.0), vec2(win.width() * 0.22, win.height() - 10.0)), 3.0, c.acc.gamma_multiply(0.5));
            clip.rect_filled(Rect::from_min_size(win.min + vec2(win.width() * 0.3, 5.0), vec2(win.width() * 0.64, 8.0)), 3.0, c.weak.gamma_multiply(0.5));
            p.rect_stroke(prev, 12.0, Stroke::new(if on { 2.0 } else { 1.0 }, if on { c.acc } else { c.line.lerp_to_gamma(c.weak, h) }), StrokeKind::Outside);
            p.text(Pos2::new(r.center().x, r.bottom() - 10.0), Align2::CENTER_CENTER, tr(name), if on { bold(13.0) } else { FontId::proportional(13.0) }, if on { c.text } else { c.weak });
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { *sel = i as u8 }
            if i % cols == cols - 1 { ui.end_row() }
        }
    });
    if *sel == 1 && motion() { ui.ctx().request_repaint_after(FRAME_60) }
}

/// Coloured call to action in the sidebar (update, driver).
fn banner(ui: &mut egui::Ui, icon: Icon, text: &str, col: Color32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    let h = anim(ui.ctx(), resp.id, resp.hovered(), 0.12);
    let p = ui.painter();
    p.rect_filled(r, 22.0, col.gamma_multiply(0.18 + 0.1 * h));
    icons::draw(p, icon, Rect::from_center_size(Pos2::new(r.left() + 24.0, r.center().y), vec2(18.0, 18.0)), col);
    p.text(Pos2::new(r.left() + 42.0, r.center().y), Align2::LEFT_CENTER, text, bold(13.5), col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Aurora: a few large soft colour clouds drifting slowly, tinted by the accent.
pub(crate) fn aurora(p: &egui::Painter, r: Rect, t: f32, acc: Color32, dark: bool) {
    p.rect_filled(r, 0.0, if dark { Color32::from_rgb(4, 4, 8) } else { Color32::from_rgb(246, 246, 250) });
    let mix = |o: Color32, k: f32| acc.lerp_to_gamma(o, k);
    let cols = [acc, mix(Color32::from_rgb(255, 70, 160), 0.6), mix(Color32::from_rgb(0, 210, 255), 0.6), mix(Color32::from_rgb(120, 70, 255), 0.5)];
    let big = r.width().max(r.height());
    for (i, col) in cols.iter().enumerate() {
        let f = i as f32;
        let x = r.left() + r.width() * (0.5 + 0.42 * (t * 0.07 * (1.0 + f * 0.31) + f * 1.7).sin());
        let y = r.top() + r.height() * (0.5 + 0.42 * (t * 0.05 * (1.0 + f * 0.27) + f * 2.3).cos());
        let rad = big * (0.42 + 0.08 * (t * 0.11 + f).sin());
        let a = if dark { 0.5 } else { 0.32 };
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(Pos2::new(x, y), col.gamma_multiply(a));
        let n = 48u32;
        for k in 0..n {
            let ang = k as f32 / n as f32 * std::f32::consts::TAU;
            mesh.colored_vertex(Pos2::new(x + ang.cos() * rad, y + ang.sin() * rad), Color32::TRANSPARENT);
            mesh.add_triangle(0, 1 + k, 1 + (k + 1) % n);
        }
        p.add(Shape::mesh(mesh));
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
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4] // the window is transparent: Mica / Acrylic or our own background shows through
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame_pacer.wait(ctx.cumulative_frame_nr());
        if !self.taskbar_icon_applied && ctx.cumulative_frame_nr() >= 2 {
            self.taskbar_icon_applied = crate::tray::apply_taskbar_icon();
        }
        self.controller_overlay.poll(&mut self.st.overlay);
        self.capture();
        self.background();
        self.look(&ctx);
        let on = self.s.on.load(Relaxed);
        // while the game has focus nobody watches this window at 60 fps: its CPU/GPU time belongs to the game
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let frame_delay = if self.settings_open || self.cap.is_some() || !self.dlgs.is_empty() || (focused && (on || self.show_live || (self.aurora_on() && motion()))) {
            FRAME_60
        } else if on || self.show_live {
            Duration::from_millis(100)
        } else {
            Duration::from_millis(250)
        };
        ctx.request_repaint_after(frame_delay);
        // Windows' own maximize (snap to the top edge, Win+Up) drops Mica / Acrylic: use ours instead
        let (os_max, outer) = ctx.input(|i| (i.viewport().maximized == Some(true), i.viewport().outer_rect));
        if os_max {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            if self.restore.is_none() { self.toggle_max(&ctx) }
        } else if self.restore.is_none() { self.normal = outer }
        window_edges(&ctx, self.restore.is_some());
        let active = self.s.live.lock().unwrap().active.clone();
        if std::mem::take(&mut self.show_settings) { self.toggle_settings(&ctx) }

        let dark = ctx.theme() == Theme::Dark;
        let c = pal(dark);
        let full = ctx.content_rect();
        if self.aurora_on() { aurora(&ctx.layer_painter(egui::LayerId::background()), full, ctx.input(|i| i.time) as f32, c.acc, dark) }
        let compact = full.width() < 920.0;
        egui::Panel::left("side").exact_size(if compact { 84.0 } else { 268.0 }).resizable(false).show_separator_line(true)
            .frame(egui::Frame::new().fill(c.side).inner_margin(egui::Margin { left: 14, right: 14, top: SIDEBAR_TOP, bottom: 14 }))
            .show(ui, |ui| self.sidebar(ui, compact));
        let bar = egui::Frame::new().fill(c.panel).inner_margin(egui::Margin { left: 24, right: 8, top: TITLE_BAR_TOP, bottom: 6 });
        egui::Panel::top("top").frame(bar).show_separator_line(false).show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("bottom").frame(egui::Frame::new().fill(c.panel).inner_margin(egui::Margin::symmetric(24, 8))).show_separator_line(false).show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(c.panel).inner_margin(egui::Margin { left: 24, right: 20, top: 6, bottom: 4 })).show(ui, |ui| {
            // page change: the new page fades in and rises into place
            let t = if motion() { ease(self.page_t.elapsed().as_secs_f32() / 0.36) } else { 1.0 };
            if t < 1.0 { ctx.request_repaint() }
            let rise = (1.0 - t) * 28.0;
            ui.set_opacity(t);
            match self.tab {
                Tab::Settings => {
                    ui.add_space(rise);
                    let narrow = ui.available_width() < 700.0;
                    self.settings_view(ui, narrow)
                }
                tab => {
                    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                        ui.add_space(rise);
                        match tab { Tab::Controller => self.controller_tab(ui, &active), Tab::Custom => self.custom_tab(ui), _ => self.kbm_tab(ui, &active) }
                    });
                }
            }
        });
        self.settings_side(&ctx);
        self.dialogs(&ctx);
        self.lang_prompt(&ctx);
        self.overlays(&ctx);
        // start screen: the mark draws in the middle, then everything fades in from behind it
        if motion() {
            let t = self.launched.get_or_insert_with(Instant::now).elapsed().as_secs_f32();
            let bg = if dark { Color32::from_rgb(13, 13, 18) } else { Color32::from_rgb(246, 246, 250) };
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("splash")));
            if crate::brand::splash(&p, full, bg, c.text, t) { ctx.request_repaint() }
        }
        if self.st.overlay.enabled && !self.skins_loaded { self.reload_skins(&ctx); }
        let overlay_skin = self.skins.iter().find(|s| s.id == self.st.pad_skin).cloned();
        self.controller_overlay.show(&ctx, &self.st.overlay, self.s.clone(), overlay_skin);
        if matches!(*self.upd.lock().unwrap(), Upd::Restarting) { ctx.send_viewport_cmd(egui::ViewportCommand::Close) }
        self.sync();
        #[cfg(debug_assertions)]
        crate::pad::dev_snapshot(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(prev) = self.epp_prev.take() { winio::set_pointer_precision(prev); }
        self.s.on.store(false, Relaxed);
        std::thread::sleep(Duration::from_millis(20)); // let the engine release held keys
        self.controller_overlay.poll(&mut self.st.overlay);
        self.st.save();
    }
}
