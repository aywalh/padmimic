//! AntiMicroX layout (profile bar, element grid, sets 1-8, Quick Set / Names / Pref / Reset, edit dialogs),
//! drawn Apple-style.

use crate::engine::{self, Event, Shared, eval_stick, pad_down};
use crate::model::*;
use crate::winio;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, Theme, Vec2, vec2};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};
use vigem_client::{XButtons, XGamepad};

// ================================================================= look

pub fn accent(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(10, 132, 255) } else { Color32::from_rgb(0, 122, 255) }
}

pub fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts.font_data.insert("ui".into(), Arc::new(egui::FontData::from_owned(data)));
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "ui".into());
    }
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 7.0);
        s.spacing.button_padding = vec2(10.0, 5.0);
        s.spacing.interact_size.y = 24.0;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(11.5));
        let v = &mut s.visuals;
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = CornerRadius::same(8);
            w.bg_stroke = Stroke::NONE;
        }
        v.window_corner_radius = CornerRadius::same(14);
        v.menu_corner_radius = CornerRadius::same(10);
        v.window_stroke = Stroke::NONE;
    });
    for (theme, dark) in [(Theme::Light, false), (Theme::Dark, true)] {
        ctx.style_mut_of(theme, |s| {
            let v = &mut s.visuals;
            let (panel, card, inactive, hover) = if dark {
                (Color32::from_rgb(28, 28, 30), Color32::from_rgb(44, 44, 46), Color32::from_rgb(58, 58, 60), Color32::from_rgb(72, 72, 74))
            } else {
                (Color32::from_rgb(242, 242, 247), Color32::WHITE, Color32::from_rgb(229, 229, 234), Color32::from_rgb(209, 209, 214))
            };
            v.panel_fill = panel;
            v.window_fill = card;
            v.extreme_bg_color = inactive;
            v.faint_bg_color = inactive;
            v.widgets.inactive.weak_bg_fill = inactive;
            v.widgets.inactive.bg_fill = inactive;
            v.widgets.hovered.weak_bg_fill = hover;
            v.widgets.hovered.bg_fill = hover;
            v.selection.bg_fill = accent(dark);
            v.selection.stroke.color = Color32::WHITE;
        });
    }
}

fn card<R>(ui: &mut egui::Ui, title: &str, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .corner_radius(14)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                if !title.is_empty() {
                    ui.label(RichText::new(title).strong().size(13.0));
                }
                f(ui)
            })
            .inner
        })
        .inner
}

fn switch(ui: &mut egui::Ui, on: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(46.0, 28.0), Sense::click());
    let t = ui.ctx().animate_bool_responsive(resp.id, on);
    let r = rect.height() / 2.0;
    let bg = if on { Color32::from_rgb(52, 199, 89) } else { ui.visuals().widgets.inactive.weak_bg_fill };
    ui.painter().rect_filled(rect, r, bg);
    let x = egui::lerp((rect.left() + r)..=(rect.right() - r), t);
    ui.painter().circle_filled(Pos2::new(x, rect.center().y), r - 2.5, Color32::WHITE);
    resp.clicked()
}

/// A grid cell like AntiMicro's button widgets: caption + current assignment, lit while pressed.
fn cell(ui: &mut egui::Ui, title: &str, body: &str, active: bool, size: Vec2) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let dark = ui.visuals().dark_mode;
    let fill = if active { accent(dark) } else if resp.hovered() { ui.visuals().widgets.hovered.weak_bg_fill } else { ui.visuals().faint_bg_color };
    let p = ui.painter();
    p.rect_filled(rect, 10.0, fill);
    let (tc, bc) = if active { (Color32::from_white_alpha(200), Color32::WHITE) } else { (ui.visuals().weak_text_color(), ui.visuals().strong_text_color()) };
    let max_chars = ((size.x - 14.0) / 7.0).max(3.0) as usize;
    let body: String = if body.chars().count() > max_chars { body.chars().take(max_chars - 1).chain(['…']).collect() } else { body.into() };
    if title.is_empty() {
        p.text(rect.center(), Align2::CENTER_CENTER, body, FontId::proportional(13.0), bc);
    } else {
        p.text(rect.left_top() + vec2(8.0, 5.0), Align2::LEFT_TOP, title, FontId::proportional(11.0), tc);
        p.text(rect.left_bottom() + vec2(8.0, -5.0), Align2::LEFT_BOTTOM, body, FontId::proportional(13.0), bc);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ================================================================= text

pub fn act_text(a: Act) -> String {
    match a {
        Act::Key(vk) => winio::key_name(vk),
        Act::Mouse(b) => ["", "Maus L", "Maus R", "Maus M", "Maus 4", "Maus 5"][b.min(5) as usize].into(),
        Act::Wheel(w) => ["Rad ↑", "Rad ↓", "Rad ←", "Rad →"][w.min(3) as usize].into(),
        Act::Move(d) => ["", "Maus ↑", "Maus ↓", "Maus ←", "Maus →"][d.min(4) as usize].into(),
        Act::Pad(o) => format!("🎮 {}", o.label()),
    }
}

fn secs(ms: u32) -> String {
    format!("{:.2}s", ms as f32 / 1000.0)
}

pub fn slot_text(s: &Slot) -> String {
    match s {
        Slot::Out(a) => act_text(*a),
        Slot::Pause(ms) => format!("Pause {}", secs(*ms)),
        Slot::Hold(ms) => format!("Halten {}", secs(*ms)),
        Slot::Cycle => "Zyklus".into(),
        Slot::Distance(p) => format!("Distanz {p}%"),
        Slot::Release(ms) => format!("Loslassen {}", secs(*ms)),
        Slot::MouseMod(p) => format!("Maus-Mod {p}%"),
        Slot::PressTime(ms) => format!("Druckzeit {}", secs(*ms)),
        Slot::Delay(ms) => format!("Verzögerung {}", secs(*ms)),
        Slot::SetChange(s) => format!("Set {}", s + 1),
        Slot::Text(t) => format!("Text „{t}“"),
        Slot::Exec { path, .. } => format!("Start {}", path.rsplit(['\\', '/']).next().unwrap_or("")),
        Slot::Load(p) => format!("Profil {}", stem(p)),
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

const INSTALLING: &str = "ViGEmBus wird installiert…";

const BTN_NAMES: [&str; 11] = ["A", "B", "X", "Y", "LB", "RB", "Back", "Start", "Guide", "L3", "R3"];
const STICK_NAMES: [&str; 2] = ["Linker Stick", "Rechter Stick"];
const TRIG_NAMES: [&str; 2] = ["LT", "RT"];

fn elem_name(p: &Set, e: Elem) -> String {
    let custom = p.button(e).map(|b| b.name.clone()).unwrap_or_default();
    if !custom.is_empty() {
        return custom;
    }
    match e {
        Elem::Btn(i) => BTN_NAMES[i as usize].into(),
        Elem::DPad(d) => format!("D-Pad {}", DIRS[d as usize]),
        Elem::Stick(s, d) => format!("{} {}", STICK_NAMES[s as usize], DIRS[d as usize]),
        Elem::StickMod(s) => format!("{} Modifier", STICK_NAMES[s as usize]),
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
        0x20 => "Leer".into(), 0x0D => "Enter".into(), 0x09 => "Tab".into(), 0x14 => "Caps".into(), 0xA0 | 0xA1 => "Shift".into(),
        0x08 => "←─".into(), 0x2E => "Entf".into(), 0x2D => "Einfg".into(), 0x21 => "PgUp".into(), 0x22 => "PgDn".into(), 0x24 => "Pos1".into(),
        0x23 => "Ende".into(), 0x13 => "Pause".into(), 0x90 => "Num".into(), 0xA4 => "Alt".into(), 0xA5 => "AltGr".into(), 0x5B => "Win".into(), 0x5D => "Menü".into(),
        0xA2 | 0xA3 => "Strg".into(), 0x60..=0x69 => format!("{}", vk - 0x60), 0x6A => "*".into(), 0x6B => "+".into(),
        0x6D => "-".into(), 0x6E => ",".into(), 0x6F => "/".into(), 0x2C => "Druck".into(), 0x91 => "Roll".into(),
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
        for (i, l) in ["⌨ Tastatur", "🖱 Maus", "🎮 Controller"].into_iter().enumerate() {
            ui.selectable_value(tab, i as u8, l);
        }
    });
    let mut hit = None;
    match *tab {
        0 => hit = keyboard(ui, match sel { Some(Act::Key(k)) => Some(k), _ => None }, numpad).map(Act::Key),
        1 => {
            for (label, acts) in [
                ("Tasten", vec![Act::Mouse(1), Act::Mouse(3), Act::Mouse(2), Act::Mouse(4), Act::Mouse(5)]),
                ("Rad", vec![Act::Wheel(0), Act::Wheel(1), Act::Wheel(2), Act::Wheel(3)]),
                ("Bewegung", vec![Act::Move(1), Act::Move(2), Act::Move(3), Act::Move(4)]),
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

fn ms_field(ui: &mut egui::Ui, label: &str, v: &mut u32, max: u32) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let c = ui.add(egui::DragValue::new(v).range(0..=max).speed(5.0).suffix(" ms")).changed();
        ui.weak(secs(*v));
        c
    })
    .inner
}

// ================================================================= controller drawing

fn controller(ui: &mut egui::Ui, g: &XGamepad) {
    let w = ui.available_width().min(280.0);
    let (resp, p) = ui.allocate_painter(vec2(ui.available_width(), w * 0.58), Sense::hover());
    let o = resp.rect.min + vec2((resp.rect.width() - w) / 2.0, 0.0);
    let at = |x: f32, y: f32| o + vec2(x * w, y * w);
    let dark = ui.visuals().dark_mode;
    let (body, idle, cap) = if dark {
        (Color32::from_rgb(58, 58, 60), Color32::from_rgb(86, 86, 90), Color32::from_rgb(30, 30, 32))
    } else {
        (Color32::from_rgb(229, 229, 234), Color32::from_rgb(199, 199, 204), Color32::WHITE)
    };
    let txt = ui.visuals().strong_text_color();
    let acc = accent(dark);
    let on = |b: u16| g.buttons.raw & b != 0;
    let font = |s: f32| FontId::proportional(s * w);
    for (x, v, l) in [(0.2, g.left_trigger, "LT"), (0.66, g.right_trigger, "RT")] {
        let r = Rect::from_min_size(at(x, 0.0), vec2(0.14 * w, 0.065 * w));
        p.rect_filled(r, 6.0, idle);
        p.rect_filled(Rect::from_min_max(r.min, Pos2::new(r.min.x + r.width() * v as f32 / 255.0, r.max.y)), 6.0, acc);
        p.text(r.center(), Align2::CENTER_CENTER, l, font(0.03), txt);
    }
    for (x, b, l) in [(0.17, XButtons::LB, "LB"), (0.65, XButtons::RB, "RB")] {
        let r = Rect::from_min_size(at(x, 0.08), vec2(0.18 * w, 0.05 * w));
        p.rect_filled(r, 10.0, if on(b) { acc } else { idle });
        p.text(r.center(), Align2::CENTER_CENTER, l, font(0.028), if on(b) { Color32::WHITE } else { txt });
    }
    p.rect_filled(Rect::from_min_max(at(0.13, 0.14), at(0.87, 0.44)), 0.1 * w, body);
    p.circle_filled(at(0.24, 0.42), 0.14 * w, body);
    p.circle_filled(at(0.76, 0.42), 0.14 * w, body);
    let stick = |c: Pos2, x: i16, y: i16, click: bool| {
        let r = 0.06 * w;
        p.circle_filled(c, r, idle);
        p.circle_filled(c + vec2(x as f32, -(y as f32)) / 32767.0 * r * 0.55, r * 0.64, if click { acc } else { cap });
    };
    stick(at(0.28, 0.26), g.thumb_lx, g.thumb_ly, on(XButtons::LTHUMB));
    stick(at(0.6, 0.38), g.thumb_rx, g.thumb_ry, on(XButtons::RTHUMB));
    let d = 0.036 * w;
    for (dx, dy, b) in [(0.0, -1.0, XButtons::UP), (0.0, 1.0, XButtons::DOWN), (-1.0, 0.0, XButtons::LEFT), (1.0, 0.0, XButtons::RIGHT)] {
        p.rect_filled(Rect::from_center_size(at(0.4, 0.38) + vec2(dx, dy) * d, vec2(d, d)), 3.0, if on(b) { acc } else { idle });
    }
    let f = 0.046 * w;
    for (dx, dy, b, l, col) in [
        (0.0, 1.0, XButtons::A, "A", Color32::from_rgb(52, 199, 89)),
        (1.0, 0.0, XButtons::B, "B", Color32::from_rgb(255, 69, 58)),
        (-1.0, 0.0, XButtons::X, "X", Color32::from_rgb(10, 132, 255)),
        (0.0, -1.0, XButtons::Y, "Y", Color32::from_rgb(255, 204, 0)),
    ] {
        let c = at(0.72, 0.26) + vec2(dx, dy) * f;
        p.circle_filled(c, 0.027 * w, if on(b) { col } else { idle });
        p.text(c, Align2::CENTER_CENTER, l, font(0.03), if on(b) { Color32::WHITE } else { txt });
    }
    for (x, y, r, b) in [(0.44, 0.26, 0.017, XButtons::BACK), (0.56, 0.26, 0.017, XButtons::START), (0.5, 0.19, 0.028, XButtons::GUIDE)] {
        p.circle_filled(at(x, y), r * w, if on(b) { acc } else { idle });
    }
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
    vigem_msg: Arc<std::sync::Mutex<Option<String>>>, // Some while installing, then the result
}

impl App {
    pub fn new(s: Arc<Shared>, st: Settings) -> Self {
        let path = if st.auto_load_last && Profile::load(&st.last_profile).is_some() { st.last_profile.clone() } else { String::new() };
        let p = Profile::load(&path).unwrap_or_default();
        *s.profile.lock().unwrap() = Arc::new(p.clone());
        *s.settings.lock().unwrap() = Arc::new(st.clone());
        let epp_prev = st.disable_epp.then(|| winio::set_pointer_precision(false));
        let (p_sent, st_sent) = (s.profile.lock().unwrap().clone(), s.settings.lock().unwrap().clone());
        let mut app = App {
            p_sent, st_sent, saved: p.clone(), p, s, st, path,
            profiles: vec![], tab: Tab::Controller, dlgs: vec![], cap: None, pick_tab: 0, adv_page: 0, adv_sel: 0, set_page: 0,
            epp_prev, auto_t: Instant::now(), auto_last: String::new(), toast: None, show_live: true,
            vigem_msg: Arc::default(),
        };
        app.scan_profiles();
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
                self.toast(format!("Profil „{}“ geladen", stem(path)));
            }
            None => self.toast(format!("Profil {path} konnte nicht geladen werden")),
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
                self.toast("Gespeichert");
            }
            Err(e) => self.toast(format!("Speichern fehlgeschlagen: {e}")),
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
        ui.horizontal(|ui| {
            ui.label(RichText::new("padmimic").size(20.0).strong());
            ui.menu_button("App", |ui| {
                if ui.button("Minimieren").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true)) }
                if ui.button("Beenden").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close) }
            });
            ui.menu_button("Optionen", |ui| {
                if ui.button("Einstellungen…").clicked() { self.open(Dlg::Settings); ui.close() }
                if ui.button("Latenz-Messung…").clicked() { self.open(Dlg::Latency); ui.close() }
            });
            ui.menu_button("Hilfe", |ui| {
                if ui.button("Über padmimic").clicked() { self.open(Dlg::About); ui.close() }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if switch(ui, on) { self.s.on.store(!on, Relaxed) }
                ui.label(if on { "Aktiv" } else { "Aus" });
            });
        });
    }

    fn profile_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let dirty = self.p != self.saved;
            let name = if self.path.is_empty() { "<Neu>".to_string() } else { stem(&self.path) };
            let mut pick: Option<String> = None;
            egui::ComboBox::from_id_salt("profile").width(170.0).selected_text(format!("{name}{}", if dirty { " •" } else { "" })).show_ui(ui, |ui| {
                if ui.selectable_label(self.path.is_empty(), "<Neu>").clicked() { pick = Some(String::new()) }
                for p in &self.profiles {
                    if ui.selectable_label(*p == self.path, stem(p)).clicked() { pick = Some(p.clone()) }
                }
            });
            match pick {
                Some(p) if p.is_empty() => { self.p = Profile::default(); self.saved = self.p.clone(); self.path.clear() }
                Some(p) => self.load(&p),
                None => {}
            }
            if ui.button("Laden").on_hover_text("Profil erneut von der Festplatte laden").clicked() && !self.path.is_empty() {
                let p = self.path.clone();
                self.load(&p);
            }
            if ui.button("Speichern").clicked() {
                if self.path.is_empty() { self.open(Dlg::SaveAs(String::new())) } else { let p = self.path.clone(); self.save_to(p) }
            }
            if ui.button("Speichern unter").clicked() { self.open(Dlg::SaveAs(stem(&self.path))) }
            if ui.button("Entfernen").on_hover_text("Aus der Liste entfernen (Datei bleibt)").clicked() && !self.path.is_empty() {
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
        if cell(ui, title, &summary(b), active.contains(&e), size).on_hover_text(elem_name(set, e)).clicked() {
            self.open(Dlg::Button(e));
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
        let live_phys = self.s.live.lock().unwrap().phys;
        if self.show_live {
            card(ui, "", |ui| controller(ui, &live_phys));
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
                                card(ui, if sname.is_empty() { STICK_NAMES[s as usize] } else { &sname }, |ui| {
                                    let centre = active.iter().any(|e| matches!(e, Elem::Stick(x, _) if *x == s));
                                    self.dir_grid(ui, |d| Elem::Stick(s, d), "⚙ Stick", Dlg::Stick(s), active, centre);
                                    let w = ui.available_width();
                                    self.btn_cell(ui, Elem::StickMod(s), "Modifier", vec2(w, 40.0), active);
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
                                card(ui, "Trigger", |ui| {
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
                                                self.btn_cell(ui, Elem::Trig(t, true), "Drücken", vec2(w, 44.0), active);
                                            }
                                        });
                                    }
                                });
                            }
                            _ => {
                                card(ui, "Buttons", |ui| {
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
        card(ui, "Tastatur & Maus → Belegung", |ui| {
            ui.weak("Echte Tasten/Maustasten lösen Aktionen aus – z. B. Controller-Buttons für Maus-&-Tastatur-Spieler.");
            let mut remove = None;
            let n = self.p.sets[self.cur()].keys.len();
            for i in 0..n {
                ui.horizontal(|ui| {
                    let vk = self.p.sets[self.cur()].keys[i].vk;
                    let label = if self.capturing(Cap::KeyBind(i)) { "Taste drücken…".into() } else { format!("⌨ {}", winio::key_name(vk)) };
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
            if ui.button("＋ Belegung").clicked() {
                self.set().keys.push(KeyBind { vk: 0x20, btn: Button::out(Act::Pad(Out::A)) });
            }
        });
        ui.add_space(6.0);
        let aim = &mut self.p.mouse_aim;
        card(ui, "Maus → Stick", |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut aim.enabled, "Aktiv");
                ui.selectable_value(&mut aim.stick, VStick::Right, "Rechter Stick");
                ui.selectable_value(&mut aim.stick, VStick::Left, "Linker Stick");
            });
            ui.add(egui::Slider::new(&mut aim.sens, 0.05..=5.0).text("Sensitivität"));
            ui.add(egui::Slider::new(&mut aim.smooth, 0.0..=0.95).text("Glättung"));
            ui.add(egui::Slider::new(&mut aim.deadzone, 0.0..=0.5).text("Anti-Deadzone"));
            ui.add(egui::Slider::new(&mut aim.window_ms, 0.125..=8.0).logarithmic(true).suffix(" ms").text("Mess-Fenster"))
                .on_hover_text("Zeitraum, über den die Mausbewegung gemittelt wird. ≈ 1000 ÷ Maus-Abfragerate: 1 ms bei 1000 Hz, 2 ms bei 500 Hz, 0,25 ms bei 4000 Hz. Kleiner = direkter, zu klein = zittrig.");
        });
    }

    fn bottom(&mut self, ui: &mut egui::Ui) {
        let cur = self.cur();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Sets").weak());
            for i in 0..8 {
                let name = &self.p.set_names[i];
                let r = ui.selectable_label(cur == i, format!("{}", i + 1)).on_hover_text(if name.is_empty() { format!("Set {}", i + 1) } else { name.clone() });
                if r.clicked() { *self.s.set_cmd.lock().unwrap() = Some(i) }
            }
            ui.separator();
            if ui.button("Quick Set").clicked() {
                let held = self.s.live.lock().unwrap().active.clone();
                self.open(Dlg::QuickSet(held));
            }
            if ui.button("Namen").clicked() { self.open(Dlg::Names) }
            if ui.button("Pref").clicked() { self.open(Dlg::Pref) }
            if ui.button("Zurücksetzen").clicked() { self.open(Dlg::Reset) }
        });
        let (st, status, vigem) = { let l = self.s.live.lock().unwrap(); (l.stats, l.status.clone(), l.vigem) };
        if !vigem && status.starts_with("ViGEmBus fehlt") {
            let msg = self.vigem_msg.lock().unwrap().clone();
            let busy = msg.as_deref() == Some(INSTALLING);
            ui.horizontal(|ui| {
                let b = ui.add_enabled(!busy, egui::Button::new(if busy { INSTALLING } else { "ViGEmBus installieren" }))
                    .on_hover_text("Treiber für den virtuellen Xbox-Controller (von Nefarius, signiert). Windows fragt einmal nach Admin-Rechten. padmimic verbindet sich danach von selbst.");
                if b.clicked() {
                    let m = self.vigem_msg.clone();
                    *m.lock().unwrap() = Some(INSTALLING.into());
                    std::thread::spawn(move || {
                        let r = match winio::install_vigem() {
                            Ok(false) => "ViGEmBus installiert – verbindet sich in wenigen Sekunden".into(),
                            Ok(true) => "ViGEmBus installiert – bitte Windows neu starten".into(),
                            Err(e) => e,
                        };
                        *m.lock().unwrap() = Some(r);
                    });
                }
                if let Some(m) = msg.filter(|m| m != INSTALLING) { ui.small(m); }
            });
        }
        ui.horizontal(|ui| {
            ui.small(status);
            ui.small(format!(
                "· Controller: {} · {} Upd/s · Engine {} Hz · Virtuell: {}",
                st.slot.map_or("nicht gefunden".into(), |i| format!("Slot {}", i + 1)), st.pad_hz, st.loop_hz,
                st.virt_slot.map_or("–".into(), |i| format!("Slot {}", i + 1))
            ));
            if let Some((t, at)) = &self.toast {
                if at.elapsed() < Duration::from_secs(3) { ui.small(RichText::new(t).color(accent(ui.visuals().dark_mode))); }
            }
        });
    }

    // ------------------------------------------------------------- dialogs

    fn dialogs(&mut self, ctx: &egui::Context) {
        let list = self.dlgs.clone();
        let mut close = vec![];
        for (i, d) in list.iter().enumerate() {
            let mut open = true;
            let (title, width) = match d {
                Dlg::Button(e) => (format!("Belegung – {}", elem_name(&self.p.sets[self.cur()], *e)), 640.0),
                Dlg::Advanced(e) => (format!("Erweitert – {}", elem_name(&self.p.sets[self.cur()], *e)), 660.0),
                Dlg::Stick(s) => (STICK_NAMES[*s as usize].to_string(), 520.0),
                Dlg::DPad => ("D-Pad".into(), 380.0),
                Dlg::Axis(t) => (format!("Achse {}", TRIG_NAMES[*t as usize]), 420.0),
                Dlg::Mouse(_) => ("Maus-Einstellungen".into(), 460.0),
                Dlg::Names => ("Set-Namen".into(), 320.0),
                Dlg::Pref => ("Profil-Einstellungen".into(), 360.0),
                Dlg::QuickSet(_) => ("Quick Set".into(), 320.0),
                Dlg::Settings => ("Einstellungen".into(), 600.0),
                Dlg::SaveAs(_) => ("Speichern unter".into(), 320.0),
                Dlg::Reset => ("Zurücksetzen".into(), 320.0),
                Dlg::About => ("Über padmimic".into(), 340.0),
                Dlg::Latency => ("Latenz-Messung".into(), 560.0),
            };
            let mut done = false;
            egui::Window::new(title)
                .id(egui::Id::new(("dlg", i, format!("{:?}", std::mem::discriminant(d)))))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(width)
                .pivot(Align2::CENTER_TOP)
                .default_pos(Pos2::new(ctx.content_rect().center().x + i as f32 * 18.0, 40.0 + i as f32 * 18.0))
                .show(ctx, |ui| {
                    let max_h = ctx.content_rect().height() - 110.0;
                    egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(max_h).show(ui, |ui| done = match d.clone() {
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
                        Dlg::Latency => self.dlg_latency(ui),
                    });
                });
            if !open || done { close.push(d.clone()) }
        }
        self.dlgs.retain(|d| !close.contains(d));
    }

    fn ok_row(ui: &mut egui::Ui) -> bool {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::Button::new(RichText::new("Fertig").color(Color32::WHITE)).fill(accent(ui.visuals().dark_mode)).min_size(vec2(84.0, 28.0))).clicked()
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
        ui.weak("Klicke eine Taste, Maus- oder Controller-Ausgabe – oder „Aufnehmen“ und drücke eine echte Taste bzw. einen Controller-Button.");
        let first = b.slots.iter().find_map(|s| if let Slot::Out(a) = s { Some(*a) } else { None });
        if let Some(a) = act_picker(ui, &mut tab, first, numpad) {
            b.slots = vec![Slot::Out(a)];
        }
        ui.separator();
        let mut cap = false;
        ui.horizontal(|ui| {
            cap = ui.button(if capturing { "Drücken…" } else { "⏺ Aufnehmen" }).clicked();
            if ui.button("Leeren").clicked() { b.slots.clear() }
            ui.checkbox(&mut b.toggle, "Toggle");
            ui.checkbox(&mut b.turbo, "Turbo");
        });
        egui::Grid::new("names").num_columns(2).show(ui, |ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut b.name);
            ui.end_row();
            ui.label("Aktion:");
            ui.text_edit_singleline(&mut b.action);
            ui.end_row();
            ui.label("Slots:");
            ui.label(RichText::new(summary(&Button { action: String::new(), ..b.clone() })).strong());
            ui.end_row();
        });
        let done = ui.horizontal(|ui| {
            open_adv = ui.button("Erweitert…").clicked();
            open_mouse = ui.button("Maus-Einstellungen…").clicked();
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
                for (i, l) in ["Zuweisungen", "Toggle", "Turbo", "Set-Auswahl"].into_iter().enumerate() {
                    ui.selectable_value(&mut page, i as u8, l);
                }
            });
            ui.separator();
            ui.vertical(|ui| match page {
                0 => {
                    egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                        if b.slots.is_empty() { ui.weak("Keine Slots"); }
                        for (i, s) in b.slots.iter().enumerate() {
                            if ui.selectable_label(sel == i, format!("{}. {}", i + 1, slot_text(s))).clicked() { sel = i }
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Einfügen").clicked() {
                            let at = (sel + 1).min(b.slots.len());
                            b.slots.insert(at, Slot::Out(Act::Key(0x20)));
                            sel = at;
                        }
                        if ui.button("Löschen").clicked() && sel < b.slots.len() {
                            b.slots.remove(sel);
                            sel = sel.saturating_sub(1);
                        }
                        if ui.button("Alle löschen").clicked() { b.slots.clear() }
                        if ui.button("↑").clicked() && sel > 0 && sel < b.slots.len() { b.slots.swap(sel, sel - 1); sel -= 1 }
                        if ui.button("↓").clicked() && sel + 1 < b.slots.len() { b.slots.swap(sel, sel + 1); sel += 1 }
                    });
                    if let Some(s) = b.slots.get_mut(sel) {
                        ui.separator();
                        let mut ty = s.type_index();
                        egui::ComboBox::from_label("Slot-Typ").selected_text(Slot::TYPES[ty]).show_ui(ui, |ui| {
                            for (i, t) in Slot::TYPES.iter().enumerate() { ui.selectable_value(&mut ty, i, *t); }
                        });
                        if ty != s.type_index() { *s = Slot::from_type(ty) }
                        let (help, _) = slot_help(s);
                        ui.weak(help);
                        match s {
                            Slot::Out(a) => {
                                if let Some(n) = act_picker(ui, &mut tab, Some(*a), numpad) { *a = n }
                                if ui.button(if capturing_slot == Some(sel) { "Drücken…" } else { "⏺ Aufnehmen" }).clicked() { cap_slot = Some(sel) }
                            }
                            Slot::Pause(ms) => { ms_field(ui, "Zeit:", ms, 600_000); }
                            Slot::Hold(ms) => { ms_field(ui, "Zeit:", ms, 600_000); }
                            Slot::Delay(ms) => { ms_field(ui, "Zeit:", ms, 600_000); }
                            Slot::Release(ms) => { ms_field(ui, "Zeit:", ms, 600_000); }
                            Slot::PressTime(ms) => { ms_field(ui, "Zeit:", ms, 600_000); }
                            Slot::Distance(p) => { ui.add(egui::Slider::new(p, 1..=100).suffix(" %").text("Distanz")); }
                            Slot::MouseMod(p) => { ui.add(egui::Slider::new(p, 1..=400).suffix(" %").text("Maus-Tempo")); }
                            Slot::Cycle => {
                                let mut on = b.cycle_reset.is_some();
                                let mut v = b.cycle_reset.unwrap_or(1000) as f32 / 1000.0;
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut on, "Zyklus automatisch zurücksetzen nach");
                                    ui.add_enabled(on, egui::DragValue::new(&mut v).range(0.01..=60.0).speed(0.05).suffix(" s"));
                                });
                                b.cycle_reset = on.then_some((v * 1000.0) as u32);
                            }
                            Slot::SetChange(n) => {
                                egui::ComboBox::from_label("Set").selected_text(set_label(&set_names, *n as usize)).show_ui(ui, |ui| {
                                    for i in 0..8u8 { ui.selectable_value(n, i, set_label(&set_names, i as usize)); }
                                });
                            }
                            Slot::Text(t) => { ui.text_edit_singleline(t); }
                            Slot::Exec { path, args } => {
                                egui::Grid::new("exec").num_columns(2).show(ui, |ui| {
                                    ui.label("Programm:");
                                    ui.text_edit_singleline(path);
                                    ui.end_row();
                                    ui.label("Argumente:");
                                    ui.text_edit_singleline(args);
                                    ui.end_row();
                                });
                            }
                            Slot::Load(p) => {
                                egui::ComboBox::from_label("Profil").selected_text(stem(p)).show_ui(ui, |ui| {
                                    for pr in &profiles { ui.selectable_value(p, pr.clone(), stem(pr)); }
                                });
                            }
                        }
                    }
                }
                1 => {
                    ui.checkbox(&mut b.toggle, "Aktiviert");
                    ui.weak("Erster Druck hält die Belegung gedrückt, der nächste lässt sie los.");
                }
                2 => {
                    ui.checkbox(&mut b.turbo, "Aktiviert");
                    let mut m = b.turbo_mode.unwrap_or(TurboMode::Normal);
                    egui::ComboBox::from_label("Modus").selected_text(match m { TurboMode::Normal => "Normal", TurboMode::Gradient => "Gradient", TurboMode::Pulse => "Puls" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut m, TurboMode::Normal, "Normal");
                        ui.selectable_value(&mut m, TurboMode::Gradient, "Gradient");
                        ui.selectable_value(&mut m, TurboMode::Pulse, "Puls");
                    });
                    b.turbo_mode = (m != TurboMode::Normal).then_some(m);
                    let mut tens = b.turbo_interval() / 10;
                    let l = format!("Verzögerung {}  ·  Rate {:.1}/s", secs(tens * 10), 100.0 / tens as f32);
                    ui.add(egui::Slider::new(&mut tens, 1..=400).show_value(false).text(l));
                    b.turbo_ms = tens * 10;
                    ui.weak("Gradient: Druckdauer folgt dem Achsenweg. Puls: Frequenz folgt dem Achsenweg.");
                }
                _ => {
                    let label = |v: Option<(u8, SetCond)>| match v {
                        None => "Deaktiviert".to_string(),
                        Some((s, c)) => format!("{} {}", set_label(&set_names, s as usize), match c { SetCond::OneWay => "einseitig", SetCond::TwoWay => "zweiseitig", SetCond::WhileHeld => "solange gehalten" }),
                    };
                    egui::ComboBox::from_label("Set wechseln").width(240.0).selected_text(label(b.set)).show_ui(ui, |ui| {
                        ui.selectable_value(&mut b.set, None, "Deaktiviert");
                        for s in 0..8u8 {
                            for c in [SetCond::OneWay, SetCond::TwoWay, SetCond::WhileHeld] {
                                ui.selectable_value(&mut b.set, Some((s, c)), label(Some((s, c))));
                            }
                        }
                    });
                    ui.weak("Einseitig: wechselt beim Loslassen. Zweiseitig: derselbe Button wechselt im Ziel-Set zurück. Solange gehalten: zurück beim Loslassen.");
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
        egui::ComboBox::new(id, "Vorlagen").selected_text("Auswählen…").show_ui(ui, |ui| {
            for (i, it) in items.iter().enumerate() {
                if ui.selectable_label(false, *it).clicked() { pick = Some(i) }
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
        egui::ComboBox::new(id, "Modus").selected_text(DirMode::ALL.iter().find(|x| x.0 == *m).unwrap().1).show_ui(ui, |ui| {
            for (v, l) in DirMode::ALL { ui.selectable_value(m, v, l); }
        });
    }

    fn dlg_stick(&mut self, ui: &mut egui::Ui, si: u8) -> bool {
        let phys = self.s.live.lock().unwrap().phys;
        let (x, y) = if si == 0 { (phys.thumb_lx, phys.thumb_ly) } else { (phys.thumb_rx, phys.thumb_ry) };
        let mut open_mod = false;
        let mut open_mouse = false;
        let s = &mut self.set().sticks[si as usize];
        const PRESETS: [&str; 10] = ["Maus (normal)", "Maus (horizontal invertiert)", "Maus (vertikal invertiert)", "Maus (beide invertiert)", "Pfeiltasten", "W | A | S | D", "Nummernblock", "Keine", "🎮 Virtueller linker Stick", "🎮 Virtueller rechter Stick"];
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
                    ui.weak("Distanz:"); ui.label(format!("{:.0}", e.raw)); ui.end_row();
                    ui.weak("Richtung:"); ui.label(format!("{:.0}°", e.bearing)); ui.end_row();
                    ui.weak("% Safe Zone:"); ui.label(format!("{:.0} %", e.radial * 100.0)); ui.end_row();
                });
            });
            ui.vertical(|ui| {
                Self::dir_mode_combo(ui, "stickmode", &mut s.mode);
                ui.add(egui::Slider::new(&mut s.dead, 1..=32737).text("Tote Zone"));
                ui.add(egui::Slider::new(&mut s.max, 1..=32737).text("Max. Zone"));
                ui.add_enabled(matches!(s.mode, DirMode::Standard | DirMode::EightWay), egui::Slider::new(&mut s.diag, 1..=90).suffix("°").text("Diagonal-Bereich"));
                ui.add(egui::Slider::new(&mut s.square, 0..=100).suffix(" %").text("Square Stick"));
                ui.add(egui::Slider::new(&mut s.delay, 0..=1000).step_by(10.0).suffix(" ms").text("Stick-Verzögerung"));
                ui.add(egui::Slider::new(&mut s.mod_zone, 1..=32737).text("Modifier-Zone"));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut s.mod_inv, "Modifier-Zone invertieren");
                    open_mod = ui.button(format!("Modifier: {}", summary(&s.modifier))).clicked();
                });
                ui.horizontal(|ui| {
                    ui.label("Analog →");
                    ui.selectable_value(&mut s.analog, None, "Aus");
                    ui.selectable_value(&mut s.analog, Some(VStick::Left), "🎮 L-Stick");
                    ui.selectable_value(&mut s.analog, Some(VStick::Right), "🎮 R-Stick");
                });
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut s.name);
                });
            });
        });
        let done = ui.horizontal(|ui| {
            open_mouse = ui.button("Maus-Einstellungen…").clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| Self::ok_row(ui)).inner
        }).inner;
        if open_mod { self.open(Dlg::Button(Elem::StickMod(si))) }
        if open_mouse { self.open(Dlg::Mouse(MouseTarget::Stick(si))) }
        done
    }

    fn dlg_dpad(&mut self, ui: &mut egui::Ui) -> bool {
        let mut open_mouse = false;
        let d = &mut self.set().dpad;
        const PRESETS: [&str; 8] = ["Maus (normal)", "Maus (horizontal invertiert)", "Maus (vertikal invertiert)", "Maus (beide invertiert)", "Pfeiltasten", "W | A | S | D", "Nummernblock", "Keine"];
        if let Some(i) = Self::presets(ui, "dpadpre", &PRESETS) { Self::apply_dir_preset(&mut d.dirs, i); }
        let mut m = d.mode.unwrap_or(DirMode::Standard);
        Self::dir_mode_combo(ui, "dpadmode", &mut m);
        d.mode = (m != DirMode::Standard).then_some(m);
        ui.add(egui::Slider::new(&mut d.delay, 0..=1000).step_by(10.0).suffix(" ms").text("D-Pad-Verzögerung"));
        ui.horizontal(|ui| { ui.label("Name:"); ui.text_edit_singleline(&mut d.name); });
        let done = ui.horizontal(|ui| {
            open_mouse = ui.button("Maus-Einstellungen…").clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| Self::ok_row(ui)).inner
        }).inner;
        if open_mouse { self.open(Dlg::Mouse(MouseTarget::DPad)) }
        done
    }

    fn dlg_axis(&mut self, ui: &mut egui::Ui, t: u8) -> bool {
        let raw = { let l = self.s.live.lock().unwrap(); if t == 0 { l.phys.left_trigger } else { l.phys.right_trigger } };
        let (mut open_p, mut open_n, mut open_mouse) = (false, false, false);
        let a = &mut self.set().triggers[t as usize];
        const PRESETS: [&str; 13] = ["Maus (horizontal)", "Maus (horizontal invertiert)", "Maus (vertikal)", "Maus (vertikal invertiert)", "Pfeile ↑ | ↓", "Pfeile ← | →", "W | S", "A | D", "Num 8 | 2", "Num 4 | 6", "Keine", "🎮 Virtueller LT", "🎮 Virtueller RT"];
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
        let names = ["Negativ halb", "Negativ", "Normal", "Positiv", "Positiv halb"];
        egui::ComboBox::from_label("Throttle").selected_text(names[(a.throttle + 2) as usize]).show_ui(ui, |ui| {
            for (i, n) in names.iter().enumerate() { ui.selectable_value(&mut a.throttle, i as i8 - 2, *n); }
        });
        ui.add(egui::Slider::new(&mut a.dead, 0..=32737).text("Tote Zone"));
        ui.add(egui::Slider::new(&mut a.max, 1..=32737).text("Max. Zone"));
        let v = engine::throttle(raw as i32 * 32767 / 255, a.throttle);
        ui.horizontal(|ui| {
            ui.label("Aktueller Wert:");
            ui.add(egui::ProgressBar::new((v as f32 + 32767.0) / 65534.0).text(format!("{v}")).desired_width(220.0));
        });
        ui.horizontal(|ui| {
            open_n = ui.button(format!("− {}", summary(&a.n))).clicked();
            open_p = ui.button(format!("+ {}", summary(&a.p))).clicked();
        });
        ui.horizontal(|ui| {
            ui.label("Analog →");
            ui.selectable_value(&mut a.analog, None, "Aus");
            ui.selectable_value(&mut a.analog, Some(VTrigger::Left), "🎮 LT");
            ui.selectable_value(&mut a.analog, Some(VTrigger::Right), "🎮 RT");
        });
        ui.horizontal(|ui| { ui.label("Name:"); ui.text_edit_singleline(&mut a.name); });
        let done = ui.horizontal(|ui| {
            open_mouse = ui.button("Maus-Einstellungen…").clicked();
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
            ui.label("Modus:");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut m.mode, MouseMode::Cursor, "Cursor");
                ui.selectable_value(&mut m.mode, MouseMode::Spring, "Spring");
            });
            ui.end_row();
            ui.label("Beschleunigung:");
            egui::ComboBox::from_id_salt("curve").selected_text(Curve::ALL.iter().find(|c| c.0 == m.curve).unwrap().1).show_ui(ui, |ui| {
                for (c, l) in Curve::ALL { ui.selectable_value(&mut m.curve, c, l); }
            });
            ui.end_row();
        });
        card(ui, "Maus-Tempo", |ui| {
            let mut together = m.speed_x == m.speed_y;
            ui.checkbox(&mut together, "Gemeinsam ändern");
            let ox = m.speed_x;
            let (lx, ly) = (format!("Horizontal ({} px/s)", m.speed_x as u32 * 20), format!("Vertikal ({} px/s)", m.speed_y as u32 * 20));
            ui.add(egui::Slider::new(&mut m.speed_x, 1..=300).text(lx));
            ui.add(egui::Slider::new(&mut m.speed_y, 1..=300).text(ly));
            if together { if m.speed_x != ox { m.speed_y = m.speed_x } else { m.speed_x = m.speed_y } }
            ui.add(egui::Slider::new(&mut m.wheel_x, 1..=100).text("Rad horizontal (Rasten/s)"));
            ui.add(egui::Slider::new(&mut m.wheel_y, 1..=100).text("Rad vertikal (Rasten/s)"));
            ui.add_enabled(m.curve == Curve::Power, egui::Slider::new(&mut m.sens, 0.001..=1000.0).logarithmic(true).text("Sensitivität"));
            ui.add_enabled(matches!(m.curve, Curve::EasingQuadratic | Curve::EasingCubic), egui::Slider::new(&mut m.easing, 0.2..=5.0).suffix(" s").text("Easing-Dauer"));
        });
        card(ui, "", |ui| {
            ui.checkbox(&mut m.accel, RichText::new("Delta-Beschleunigung").strong());
            ui.add_enabled_ui(m.accel, |ui| {
                ui.add(egui::Slider::new(&mut m.accel_mult, 1.0..=200.0).suffix("×").text("Multiplikator"));
                ui.add(egui::Slider::new(&mut m.accel_start, 0.0..=100.0).suffix(" %").text("Start"));
                ui.add(egui::Slider::new(&mut m.accel_min, 0.0..=100.0).suffix(" %").text("Min. Schwelle"));
                ui.add(egui::Slider::new(&mut m.accel_max, 0.0..=100.0).suffix(" %").text("Max. Schwelle"));
                ui.add(egui::Slider::new(&mut m.accel_dur, 0.0..=5.0).suffix(" s").text("Extra-Dauer"));
                ui.horizontal(|ui| {
                    ui.label("Kurve:");
                    for (c, l) in [(AccelCurve::Linear, "Linear"), (AccelCurve::EaseOutSine, "Sinus"), (AccelCurve::EaseOutQuad, "Quad"), (AccelCurve::EaseOutCubic, "Kubisch")] {
                        ui.selectable_value(&mut m.accel_curve, c, l);
                    }
                });
            });
        });
        card(ui, "Spring", |ui| {
            ui.add_enabled_ui(m.mode == MouseMode::Spring, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Breite");
                    ui.add(egui::DragValue::new(&mut m.spring_w).range(0..=16384));
                    ui.label("Höhe");
                    ui.add(egui::DragValue::new(&mut m.spring_h).range(0..=16384));
                    ui.weak("0 = Bildschirm");
                });
                ui.add(egui::Slider::new(&mut m.spring_release, 0..=100).suffix(" %").text("Release-Radius"));
                ui.checkbox(&mut m.spring_rel, "Relativ");
            });
        });
        ui.weak(format!("Maus-Status: X {:.0} px/s · Y {:.0} px/s", pps.0, pps.1));
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
                ui.label(format!("Set {}", i + 1));
                ui.text_edit_singleline(&mut self.p.set_names[i]);
                ui.end_row();
            }
        });
        Self::ok_row(ui)
    }

    fn dlg_pref(&mut self, ui: &mut egui::Ui) -> bool {
        let mut tens = (self.p.key_press_ms / 10).clamp(1, 100);
        let l = format!("Tasten-Druckzeit {} ms", tens * 10);
        ui.add(egui::Slider::new(&mut tens, 1..=100).show_value(false).text(l));
        self.p.key_press_ms = tens * 10;
        ui.horizontal(|ui| { ui.label("Profilname:"); ui.text_edit_singleline(&mut self.p.name); });
        Self::ok_row(ui)
    }

    fn dlg_quickset(&mut self, ui: &mut egui::Ui, held: &[Elem]) -> bool {
        ui.label("Drücke einen Button oder bewege eine Achse am Controller (oder eine belegte Taste). Der passende Editor öffnet sich.");
        ui.ctx().request_repaint();
        let active = self.s.live.lock().unwrap().active.clone();
        if let Some(e) = active.into_iter().find(|e| !held.contains(e)) {
            self.open(Dlg::Button(e));
            return true;
        }
        ui.add_space(4.0);
        ui.button("Abbrechen").clicked()
    }

    fn dlg_settings(&mut self, ui: &mut egui::Ui) -> bool {
        let mut page = self.set_page;
        let toggle_cap = self.capturing(Cap::Toggle);
        let mut cap_toggle = false;
        let st = &mut self.st;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(120.0);
                for (i, l) in ["Allgemein", "Auto-Profil", "Maus", "Latenz"].into_iter().enumerate() {
                    ui.selectable_value(&mut page, i as u8, l);
                }
            });
            ui.separator();
            ui.vertical(|ui| match page {
                0 => {
                    ui.horizontal(|ui| { ui.label("Profil-Ordner:"); ui.text_edit_singleline(&mut st.profile_dir); });
                    ui.add(egui::Slider::new(&mut st.recent_count, 1..=100).text("Letzte Profile"));
                    ui.checkbox(&mut st.auto_load_last, "Zuletzt geöffnetes Profil automatisch laden");
                    ui.checkbox(&mut st.startup, "Mit Windows starten");
                    ui.checkbox(&mut st.hide_empty, "Leere Buttons ausblenden");
                    ui.checkbox(&mut st.numpad, "Nummernblock in der Tastatur anzeigen");
                    ui.horizontal(|ui| {
                        ui.label("An/Aus-Hotkey:");
                        let l = if toggle_cap { "Drücken…".into() } else { match st.toggle { Input::Key(k) => winio::key_name(k), Input::Pad(o) => format!("🎮 {}", o.label()) } };
                        cap_toggle = ui.button(l).clicked();
                    });
                    card(ui, "Tastenwiederholung", |ui| {
                        ui.checkbox(&mut st.key_repeat, "Aktiv");
                        ui.add_enabled(st.key_repeat, egui::Slider::new(&mut st.key_repeat_delay, 0..=1000).suffix(" ms").text("Verzögerung"));
                        ui.add_enabled(st.key_repeat, egui::Slider::new(&mut st.key_repeat_rate, 5..=250).suffix("/s").text("Rate"));
                    });
                }
                1 => {
                    ui.weak("Lädt automatisch ein Profil, sobald das Programm im Vordergrund ist.");
                    ui.checkbox(&mut st.auto_profile, "Aktiv");
                    let mut remove = None;
                    for (i, a) in st.auto_profiles.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut a.exe).hint_text("spiel.exe").desired_width(140.0));
                            ui.label("→");
                            egui::ComboBox::from_id_salt(("ap", i)).width(160.0).selected_text(stem(&a.profile)).show_ui(ui, |ui| {
                                for p in &self.profiles { ui.selectable_value(&mut a.profile, p.clone(), stem(p)); }
                            });
                            if ui.small_button("×").clicked() { remove = Some(i) }
                        });
                    }
                    if let Some(i) = remove { st.auto_profiles.remove(i); }
                    ui.horizontal(|ui| {
                        if ui.button("＋ Hinzufügen").clicked() { st.auto_profiles.push(AutoProfile { exe: String::new(), profile: String::new() }) }
                        if ui.button("Aktuelles Vordergrund-Programm").on_hover_text("Nach 3 s wird das aktive Programm eingetragen").clicked() {
                            st.auto_profiles.push(AutoProfile { exe: "…".into(), profile: self.path.clone() });
                        }
                    });
                }
                2 => {
                    ui.checkbox(&mut st.disable_epp, "Windows „Zeigerbeschleunigung verbessern“ deaktivieren");
                    card(ui, "Glättung", |ui| {
                        ui.checkbox(&mut st.smoothing, "Aktiv");
                        ui.add_enabled(st.smoothing, egui::Slider::new(&mut st.history, 1..=100).text("Verlaufsgröße"));
                        ui.add_enabled(st.smoothing, egui::Slider::new(&mut st.weight, 0.0..=1.0).text("Gewichtung"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Maus-Refresh:");
                        egui::ComboBox::from_id_salt("mref").selected_text(format!("{} ms", st.mouse_refresh)).show_ui(ui, |ui| {
                            for v in 1..=16 { ui.selectable_value(&mut st.mouse_refresh, v, format!("{v} ms")); }
                        });
                    });
                }
                _ => {
                    ui.horizontal(|ui| {
                        ui.label("Controller-Polling:");
                        egui::ComboBox::from_id_salt("hz").selected_text(format!("{} Hz", st.hz)).show_ui(ui, |ui| {
                            for hz in [125, 250, 500, 1000, 2000, 4000, 8000] { ui.selectable_value(&mut st.hz, hz, format!("{hz} Hz")); }
                        });
                    });
                    ui.weak("Wie oft Controller, Tastatur und Maus gelesen und Ausgaben gesendet werden (hochauflösender Timer, Thread mit höchster Priorität).");
                    ui.weak("Echten Controller mit HidHide verstecken und padmimic.exe dort freigeben, damit Spiele nur den virtuellen Pad sehen.");
                }
            });
        });
        self.set_page = page;
        if cap_toggle { self.cap = Some((Cap::Toggle, false)) }
        let mut done = false;
        ui.horizontal(|ui| {
            if ui.button("Standard").clicked() { self.st = Settings { recent: self.st.recent.clone(), last_profile: self.st.last_profile.clone(), ..Settings::default() } }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| done = Self::ok_row(ui));
        });
        done
    }

    fn dlg_save_as(&mut self, ui: &mut egui::Ui, idx: usize) -> bool {
        let Some(Dlg::SaveAs(name)) = self.dlgs.get_mut(idx) else { return true };
        ui.horizontal(|ui| { ui.label("Name:"); ui.text_edit_singleline(name); });
        let name = name.trim().to_string();
        if Self::ok_row(ui) && !name.is_empty() {
            let path = std::path::Path::new(&self.st.profile_dir).join(format!("{name}.json")).display().to_string();
            self.save_to(path);
            return true;
        }
        false
    }

    fn dlg_reset(&mut self, ui: &mut egui::Ui) -> bool {
        ui.label("Was soll zurückgesetzt werden?");
        let mut done = false;
        ui.horizontal(|ui| {
            if ui.button("Aktuelles Set leeren").clicked() { *self.set() = Set::default(); done = true }
            if ui.button("Ganzes Profil auf Standard").clicked() { self.p = Profile::default(); done = true }
            if ui.button("Abbrechen").clicked() { done = true }
        });
        done
    }

    fn dlg_about(&mut self, ui: &mut egui::Ui) -> bool {
        ui.label(RichText::new(format!("padmimic {}", env!("CARGO_PKG_VERSION"))).strong());
        ui.label("Controller, Tastatur & Maus → Tastatur, Maus und virtueller Xbox-360-Controller.");
        ui.weak("Belegungslogik (Slots, Turbo, Sets, Zonen, Kurven) nach dem Vorbild von AntiMicroX.");
        Self::ok_row(ui)
    }

    fn dlg_latency(&mut self, ui: &mut egui::Ui) -> bool {
        let (lat, stats) = { let l = self.s.live.lock().unwrap(); (l.lat.clone(), l.stats) };
        ui.weak("Zum Vergleichen: „Zurücksetzen“, 10–20 s den Stick kreisen lassen, Werte notieren. Dann z. B. antimicro starten und dasselbe wiederholen.");
        ui.label(format!(
            "Läuft seit {:.0} s · Engine-Ziel {} Hz · erreicht {} Hz · Genauigkeit ±{:.2} ms (ein Takt)",
            lat.since.elapsed().as_secs_f32(), self.st.hz, stats.loop_hz, 1000.0 / self.st.hz.max(1) as f32
        ));
        let ms = |us: f64| format!("{:.2} ms", us / 1000.0);
        egui::Grid::new("lat").num_columns(5).striped(true).spacing([16.0, 6.0]).show(ui, |ui| {
            for h in ["", "Werte", "Ø", "99 %", "Max"] { ui.strong(h); }
            ui.end_row();
            for (name, help, h) in [
                ("Controller-Daten", "Abstand zwischen zwei neuen Meldungen des echten Controllers. Zählt nur, während du ihn bewegst. Klein und gleichmäßig = gut.", &lat.pad),
                ("Engine-Takt", "Echter Abstand zwischen zwei Durchläufen von padmimic. Hohe Max-Werte heißen: Windows oder ein anderes Programm hat die Engine kurz aufgehalten.", &lat.tick),
                ("Weg zum virtuellen Pad", "Vom Senden an ViGEm, bis der virtuelle Controller die Änderung über XInput zeigt. So früh kann ein Spiel sie frühestens sehen.", &lat.virt),
            ] {
                ui.label(name).on_hover_text(help);
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
            ui.label(format!("Der Controller liefert beim Bewegen ≈ {:.0} neue Meldungen pro Sekunde.", 1e6 / lat.pad.avg()));
        }
        if stats.virt_slot.is_none() {
            ui.weak("Kein virtuelles Pad (ViGEmBus nicht installiert oder nicht erkannt): die dritte Zeile bleibt leer.");
        }
        let mut done = false;
        ui.horizontal(|ui| {
            if ui.button("Zurücksetzen").clicked() { self.s.live.lock().unwrap().lat = engine::Lat::default() }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| done = Self::ok_row(ui));
        });
        done
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
    if names[i].is_empty() { format!("Set {}", i + 1) } else { format!("Set {} ({})", i + 1, names[i]) }
}

fn slot_help(s: &Slot) -> (&'static str, ()) {
    (match s {
        Slot::Out(_) => "Taste, Maustaste, Mausbewegung, Rad oder Controller-Ausgabe. Aufeinanderfolgende Slots werden gleichzeitig gedrückt.",
        Slot::Cycle => "Slots nach dem Zyklus laufen beim nächsten Druck. Mehrere Zyklen teilen eine Sequenz in Abschnitte.",
        Slot::Delay(_) => "Verzögert den nächsten Slot. Bereits aktive Slots bleiben gedrückt.",
        Slot::Distance(_) => "Folgende Slots sind nur aktiv, wenn die Achse so weit über die tote Zone bewegt wird.",
        Slot::Exec { .. } => "Startet ein Programm, wenn der Slot erreicht wird.",
        Slot::Hold(_) => "Folgende Slots laufen nur, wenn der Button so lange gehalten wird.",
        Slot::Load(_) => "Lädt ein Profil, wenn der Slot erreicht wird.",
        Slot::MouseMod(_) => "Ändert alle Maus-Tempos um diesen Prozentsatz, solange aktiv – z. B. langsamer beim Zielen.",
        Slot::Pause(_) => "Pause zwischen Tastendrücken: vorherige Tasten werden losgelassen.",
        Slot::PressTime(_) => "Wie lange Tasten nach diesem Slot gedrückt gehalten werden.",
        Slot::Release(_) => "Folgende Slots laufen beim Loslassen, wenn der Button mindestens so lange gehalten wurde.",
        Slot::SetChange(_) => "Wechselt zum gewählten Set, wenn der Slot erreicht wird.",
        Slot::Text(_) => "Tippt den ganzen Text, wenn der Slot erreicht wird.",
    }, ())
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.capture();
        self.background();
        let on = self.s.on.load(Relaxed);
        let busy = on || self.cap.is_some() || !self.dlgs.is_empty() || self.show_live;
        ui.ctx().request_repaint_after(Duration::from_millis(if busy { 16 } else { 250 }));
        let active = self.s.live.lock().unwrap().active.clone();

        let bar = egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(14, 10));
        egui::Panel::top("top").frame(bar).show(ui, |ui| {
            self.header(ui);
            self.profile_bar(ui);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Controller, "🎮 Controller");
                ui.selectable_value(&mut self.tab, Tab::Kbm, "⌨ Tastatur & Maus");
                if self.tab == Tab::Controller {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.checkbox(&mut self.show_live, "Live-Ansicht"); });
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
        self.sync();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(prev) = self.epp_prev.take() { winio::set_pointer_precision(prev); }
        self.s.on.store(false, Relaxed);
        std::thread::sleep(Duration::from_millis(20)); // let the engine release held keys
        self.st.save();
    }
}
