//! Captured controller artwork in Padmimic's own UI. No website code runs in the app.
use crate::i18n::tr;
use crate::model::Stick;
use crate::ui::{anim, bold, mono, pal};
use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke, TextureHandle, pos2,
    vec2,
};
use image::AnimationDecoder;
use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use vigem_client::{XButtons, XGamepad};

pub type Trail = [VecDeque<(f32, f32)>; 2];
#[derive(Clone, Copy)]
enum Layout {
    DualSense,
    DualShock,
    Xbox,
}
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub brand: u8,
    layout: Layout,
    thumbnail: &'static [u8],
    body: &'static [u8],
    controls: &'static [u8],
    accent: [u8; 3],
    part_luma: [u8; 17],
    animation: Option<&'static [u8]>,
}
include!("controller_catalog.rs");

impl Model {
    pub fn accent(&self) -> [u8; 3] {
        self.accent
    }
    fn color(&self) -> Color32 {
        Color32::from_rgb(self.accent[0], self.accent[1], self.accent[2])
    }
    pub fn matches_filter(&self, filter: u8) -> bool {
        filter == 0
            || (filter == 3 && self.brand == 1)
            || match self.layout {
                Layout::DualSense => filter == 1,
                Layout::DualShock => filter == 2,
                Layout::Xbox => false,
            }
    }
    pub fn tag(&self) -> &'static str {
        match self.layout {
            Layout::DualSense => "PS5",
            Layout::DualShock => "PS4",
            Layout::Xbox => "Xbox",
        }
    }
    pub fn animated(&self) -> bool {
        self.animation.is_some()
    }
}

/// Stable IDs take precedence over the old numeric preference. Legacy Switch Pro falls back
/// to DualSense because the captured catalog contains no Nintendo artwork.
pub fn is_auto(id: &str, legacy: u8) -> bool {
    id == "auto" || (id.is_empty() && legacy == 0)
}

pub fn live_index(id: &str, legacy: u8, sony: Option<crate::sony_input::Reading>) -> usize {
    if is_auto(id, legacy) {
        use crate::sony_input::SonyModel;
        return match sony.map(|input| input.model) {
            Some(SonyModel::DualSense) => 1,
            Some(SonyModel::DualShock4) => 2,
            None => 0,
        };
    }
    selected_index(id, legacy)
}

pub fn selected_index(id: &str, legacy: u8) -> usize {
    if let Some(id) = id.strip_prefix("builtin:") {
        return MODELS.iter().position(|m| m.id == id).unwrap_or(1);
    }
    match legacy {
        0..=2 => legacy as usize,
        3 => 1,
        _ => 1,
    }
}

struct Geometry {
    caps: [(f32, f32); 2],
    radius: f32,
    faces: [(f32, f32); 4],
    dpad: (f32, f32),
    step: f32,
    menus: [(f32, f32); 2],
    home: (f32, f32),
    triggers: [(f32, f32); 2],
    trigger_bounds: (f32, f32),
    bumpers: [(f32, f32); 2],
}
impl Layout {
    fn geometry(self) -> Geometry {
        match self {
            Self::DualSense => Geometry {
                caps: [(315., 299.), (482., 299.)],
                radius: 25.,
                faces: [(560., 265.), (598., 228.), (522., 228.), (560., 190.)],
                dpad: (238., 228.),
                step: 30.,
                menus: [(278., 170.), (521., 170.)],
                home: (400., 295.),
                triggers: [(235., 96.), (562., 96.)],
                trigger_bounds: (67., 124.),
                bumpers: [(240., 147.), (556., 147.)],
            },
            Self::DualShock => Geometry {
                caps: [(316., 309.), (483., 309.)],
                radius: 32.,
                faces: [(563., 274.), (600., 235.), (525., 235.), (563., 197.)],
                dpad: (237., 235.),
                step: 28.,
                menus: [(293., 183.), (506., 183.)],
                home: (400., 309.),
                triggers: [(242., 106.), (558., 106.)],
                trigger_bounds: (80., 132.),
                bumpers: [(242., 147.), (558., 147.)],
            },
            Self::Xbox => Geometry {
                caps: [(266., 220.), (467., 298.)],
                radius: 32.,
                faces: [(534., 254.), (571., 221.), (497., 220.), (534., 185.)],
                dpad: (331., 304.),
                step: 24.,
                menus: [(362., 218.), (438., 218.)],
                home: (400., 166.),
                triggers: [(264., 80.), (532., 80.)],
                trigger_bounds: (43., 120.),
                bumpers: [(281., 133.), (519., 133.)],
            },
        }
    }
}
const ASPECT: f32 = 500.0 / 576.0;
fn uv() -> Rect {
    Rect::from_min_max(Pos2::ZERO, pos2(1., 1.))
}
fn image(bytes: &[u8]) -> egui::ColorImage {
    let im = image::load_from_memory(bytes)
        .expect("validated embedded controller image")
        .into_rgba8();
    egui::ColorImage::from_rgba_unmultiplied(
        [im.width() as usize, im.height() as usize],
        im.as_raw(),
    )
}
fn load(ctx: &egui::Context, name: impl Into<String>, pixels: egui::ColorImage) -> TextureHandle {
    ctx.load_texture(name, pixels, egui::TextureOptions::LINEAR)
}
fn thumbnail_texture(ctx: &egui::Context, mi: usize) -> TextureHandle {
    let id = egui::Id::new(("controller-thumbnail", mi));
    if let Some(t) = ctx.data_mut(|d| d.get_temp::<TextureHandle>(id)) {
        return t;
    }
    let t = load(
        ctx,
        format!("controller-thumb-{mi}"),
        image(MODELS[mi].thumbnail),
    );
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

struct Clip {
    frames: Vec<(f64, egui::ColorImage)>,
    duration: f64,
}
fn decode_clip(bytes: &'static [u8]) -> Result<Clip, String> {
    let decoder =
        image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    // WebP encoders collapse a constant video preview into an ordinary still image.
    if !decoder.has_animation() {
        return Ok(Clip {
            frames: vec![(1.0, image(bytes))],
            duration: 1.0,
        });
    }
    let mut frames = Vec::new();
    let mut duration = 0.;
    for frame in decoder.into_frames() {
        let frame = frame.map_err(|e| e.to_string())?;
        let (num, den) = frame.delay().numer_denom_ms();
        duration += (num as f64 / den.max(1) as f64 / 1000.).max(0.01);
        let im = frame.into_buffer();
        frames.push((
            duration,
            egui::ColorImage::from_rgba_unmultiplied(
                [im.width() as usize, im.height() as usize],
                im.as_raw(),
            ),
        ));
    }
    if frames.is_empty() {
        return Err("empty controller animation".into());
    }
    Ok(Clip { frames, duration })
}
struct Entry {
    mi: usize,
    body: TextureHandle,
    controls: TextureHandle,
    still: egui::ColorImage,
    clip: Option<Clip>,
    receiver: Option<mpsc::Receiver<Result<Clip, String>>>,
    attempted: bool,
    frame: Option<usize>,
}
type Cache = Arc<Mutex<VecDeque<Entry>>>;
fn player(ctx: &egui::Context, mi: usize, animated: bool) -> (TextureHandle, TextureHandle) {
    let id = egui::Id::new("controller-playback-cache");
    let cache = ctx
        .data_mut(|d| d.get_temp::<Cache>(id))
        .unwrap_or_else(|| {
            let c = Cache::default();
            ctx.data_mut(|d| d.insert_temp(id, c.clone()));
            c
        });
    let mut cache = cache.lock().unwrap();
    let index = cache.iter().position(|e| e.mi == mi);
    let mut entry = if let Some(i) = index {
        cache.remove(i).unwrap()
    } else {
        let still = image(MODELS[mi].body);
        Entry {
            mi,
            body: load(ctx, format!("controller-body-{mi}"), still.clone()),
            controls: load(
                ctx,
                format!("controller-controls-{mi}"),
                image(MODELS[mi].controls),
            ),
            still,
            clip: None,
            receiver: None,
            attempted: false,
            frame: None,
        }
    };
    if animated && !entry.attempted {
        if let Some(bytes) = MODELS[mi].animation {
            // At most one background decode, even when rapidly moving across cards.
            static DECODING: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if DECODING
                .compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::AcqRel,
                    std::sync::atomic::Ordering::Relaxed,
                )
                .is_ok()
            {
                entry.attempted = true;
                let (tx, rx) = mpsc::channel();
                entry.receiver = Some(rx);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = decode_clip(bytes);
                    let _ = tx.send(result);
                    DECODING.store(false, std::sync::atomic::Ordering::Release);
                    ctx.request_repaint();
                });
            }
        } else {
            entry.attempted = true;
        }
    }
    if let Some(rx) = &entry.receiver {
        match rx.try_recv() {
            Ok(result) => {
                entry.clip = result.ok();
                entry.receiver = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => entry.receiver = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    if let Some(clip) = &entry.clip {
        if animated {
            let time = ctx.input(|i| i.time).rem_euclid(clip.duration);
            let frame = clip
                .frames
                .iter()
                .position(|(end, _)| *end > time)
                .unwrap_or(0);
            if entry.frame != Some(frame) {
                entry
                    .body
                    .set(clip.frames[frame].1.clone(), egui::TextureOptions::LINEAR);
                entry.frame = Some(frame);
            }
            if clip.frames.len() > 1 {
                // Wake for the next source frame instead of polling at a fixed 12.5 Hz.
                let delay = (clip.frames[frame].0 - time).max(0.001);
                ctx.request_repaint_after(Duration::from_secs_f64(delay));
            }
        } else if entry.frame.take().is_some() {
            entry
                .body
                .set(entry.still.clone(), egui::TextureOptions::LINEAR);
        }
    }
    let result = (entry.body.clone(), entry.controls.clone());
    cache.push_front(entry);
    // Selected preview + hovered card only; no animation textures for offscreen cards.
    cache.truncate(2);
    result
}

fn soft_glow_pixels() -> egui::ColorImage {
    let mut rgba = Vec::with_capacity(256 * 256 * 4);
    for y in 0..256 {
        for x in 0..256 {
            let d = ((x as f32 - 127.5) / 127.5).powi(2) + ((y as f32 - 127.5) / 127.5).powi(2);
            let alpha = ((-3.4 * d).exp() * (1.0 - d).max(0.0) * 145.0).round() as u8;
            rgba.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([256, 256], &rgba)
}
fn glow(p: &Painter, r: Rect, mi: usize) {
    let id = egui::Id::new("controller-soft-backlight");
    let cached = p.ctx().data_mut(|d| d.get_temp::<TextureHandle>(id));
    let texture = cached.unwrap_or_else(|| {
        let t = load(p.ctx(), "controller-soft-backlight", soft_glow_pixels());
        p.ctx().data_mut(|d| d.insert_temp(id, t.clone()));
        t
    });
    let glow = Rect::from_center_size(
        r.center() + vec2(0., r.height() * 0.12),
        vec2(r.width() * 1.20, r.height() * 1.28),
    );
    p.image(texture.id(), glow, uv(), MODELS[mi].color());
}

fn frame(r: Rect) -> (Pos2, f32, Rect) {
    let k = (r.width() / 576.).min(r.height() / 500.);
    let rect = Rect::from_center_size(r.center(), vec2(576., 500.) * k);
    (rect.min - vec2(112., 20.) * k, k, rect)
}
pub fn thumbnail(p: &Painter, r: Rect, mi: usize) {
    let p = &p.with_clip_rect(r.intersect(p.clip_rect()));
    glow(p, r, mi);
    let (_, _, r) = frame(r);
    p.image(thumbnail_texture(p.ctx(), mi).id(), r, uv(), Color32::WHITE);
    neutral_home_button(p, r, MODELS[mi].layout);
}

// A neutral physical home button covers the manufacturer emblem at render time.
// Source artwork and its provenance remain intact, including animated frames.
fn neutral_home_button(p: &Painter, r: Rect, layout: Layout) {
    let (o, k, _) = frame(r);
    let (x, y) = layout.geometry().home;
    let center = o + vec2(x, y) * k;
    let radius = match layout {
        Layout::DualSense => 16.,
        Layout::DualShock => 13.,
        Layout::Xbox => 22.,
    } * k;
    p.circle_filled(center, radius, Color32::from_rgb(39, 41, 43));
    p.circle_filled(center + vec2(0., 0.8 * k), radius * 0.90, Color32::from_rgb(20, 22, 24));
    p.circle_stroke(center, radius * 0.93, Stroke::new((0.65 * k).max(0.3), Color32::from_rgb(48, 50, 52)));
}

#[derive(Clone, Copy)]
struct PressStyle {
    fill: Color32,
    ink: Color32,
}
fn press_style(model: &Model, part: usize) -> PressStyle {
    let dark = Color32::from_rgb(18, 21, 29);
    let light = Color32::from_rgb(247, 249, 255);
    if model.part_luma[part] >= 48 {
        PressStyle {
            fill: dark,
            ink: light,
        }
    } else {
        PressStyle {
            fill: light,
            ink: dark,
        }
    }
}
fn control_uv(slot: usize) -> Rect {
    Rect::from_min_max(
        pos2(slot as f32 / 10., 0.),
        pos2((slot + 1) as f32 / 10., 1.),
    )
}
fn part_image(
    p: &Painter,
    controls: &TextureHandle,
    slot: usize,
    center: Pos2,
    scale: f32,
    color: Color32,
) {
    p.image(
        controls.id(),
        Rect::from_center_size(center, vec2(128., 128.) * scale),
        control_uv(slot),
        color,
    );
}
fn button_ring(p: &Painter, center: Pos2, radius: f32, fill: Color32, accent: Color32) {
    // Two contrasting contours remain visible over both white and dark artwork.
    p.circle_filled(center, radius + 2.5, Color32::from_rgb(10, 12, 18));
    p.circle_filled(center, radius + 1.3, Color32::from_rgb(250, 251, 255));
    p.circle_filled(center, radius, fill);
    p.circle_stroke(center, radius + 2.5, Stroke::new(1.5, accent));
}

/// Adaptive physical feedback uses actual part silhouettes, with separate virtual outlines.
fn paint(p: &Painter, r: Rect, mi: usize, g: &XGamepad, out: Option<&XGamepad>) {
    paint_controller(p, r, mi, g, out, true);
}

/// Alpha is also used for the native overlay window shape, so empty areas accept no clicks.
pub(crate) fn overlay_pixels(mi: usize) -> egui::ColorImage { image(MODELS[mi].body) }
pub(crate) fn overlay_image_rect(r: Rect) -> Rect { frame(r).2 }

/// Same live artwork and feedback, with no backlight, labels or app background.
pub(crate) fn paint_overlay(p: &Painter, r: Rect, mi: usize, g: &XGamepad, extra: u8) {
    paint_controller(p, r, mi, g, None, false);
    paint_sony_buttons(p, r, mi, extra);
}

fn paint_sony_buttons(p: &Painter, r: Rect, mi: usize, extra: u8) {
    if extra == 0 { return; }
    let model = &MODELS[mi];
    let (o, k, _) = frame(r);
    if extra & 1 != 0 {
        let y = match model.layout { Layout::DualSense => 192., Layout::DualShock => 203., Layout::Xbox => return };
        let touchpad = Rect::from_center_size(o + vec2(400., y) * k, vec2(122., 48.) * k);
        p.rect_stroke(touchpad, 8. * k, Stroke::new(1.5, model.color()), egui::StrokeKind::Inside);
    }
    if extra & 2 != 0 && matches!(model.layout, Layout::DualSense) {
        p.rect_filled(Rect::from_center_size(o + vec2(400., 323.) * k, vec2(17., 5.) * k), 2. * k, model.color());
    }
}

fn paint_controller(p: &Painter, r: Rect, mi: usize, g: &XGamepad, out: Option<&XGamepad>, backlight: bool) {
    // Let the soft halo fade into the parent card instead of cutting it at the image bounds.
    if backlight { glow(p, r, mi); }
    let p = &p.with_clip_rect(r.intersect(p.clip_rect()));
    let (o, k, rect) = frame(r);
    let at = |(x, y): (f32, f32)| o + vec2(x, y) * k;
    let (body, controls) = player(p.ctx(), mi, crate::ui::motion());
    p.image(body.id(), rect, uv(), Color32::WHITE);
    neutral_home_button(p, rect, MODELS[mi].layout);
    let model = &MODELS[mi];
    let geo = model.layout.geometry();
    let accent = model.color();
    let on = |bit| g.buttons.raw & bit != 0;
    let von = |bit| out.is_some_and(|g| g.buttons.raw & bit != 0);
    let virtual_color = pal(p.ctx().theme() == egui::Theme::Dark).amber;

    for (i, bit) in [XButtons::A, XButtons::B, XButtons::X, XButtons::Y]
        .into_iter()
        .enumerate()
    {
        let c = at(geo.faces[i]);
        let radius = 16. * k;
        if on(bit) {
            let style = press_style(model, i);
            button_ring(p, c + vec2(0., 1.2), radius * 0.94, style.fill, accent);
            let c = c + vec2(0., 1.2);
            let s = 6.5 * k;
            let st = Stroke::new((1.5 * k).max(1.), style.ink);
            match model.layout {
                Layout::Xbox => {
                    p.text(
                        c,
                        Align2::CENTER_CENTER,
                        ["A", "B", "X", "Y"][i],
                        bold((14. * k).max(9.)),
                        style.ink,
                    );
                }
                _ => match i {
                    0 => {
                        p.line_segment([c + vec2(-s, -s), c + vec2(s, s)], st);
                        p.line_segment([c + vec2(-s, s), c + vec2(s, -s)], st);
                    }
                    1 => {
                        p.circle_stroke(c, s, st);
                    }
                    2 => {
                        p.rect_stroke(
                            Rect::from_center_size(c, vec2(s * 1.7, s * 1.7)),
                            0.,
                            st,
                            egui::StrokeKind::Inside,
                        );
                    }
                    _ => {
                        p.add(Shape::closed_line(
                            vec![
                                c + vec2(0., -s),
                                c + vec2(s, s * 0.8),
                                c + vec2(-s, s * 0.8),
                            ],
                            st,
                        ));
                    }
                },
            }
        }
        if von(bit) {
            p.circle_stroke(c, radius + 4., Stroke::new(1.5, virtual_color));
        }
    }
    for (i, (dx, dy, bit)) in [
        (0., -1., XButtons::UP),
        (1., 0., XButtons::RIGHT),
        (0., 1., XButtons::DOWN),
        (-1., 0., XButtons::LEFT),
    ]
    .into_iter()
    .enumerate()
    {
        let c = at((geo.dpad.0 + dx * geo.step, geo.dpad.1 + dy * geo.step));
        let rect = Rect::from_center_size(c, vec2(22., 24.) * k);
        if on(bit) {
            let style = press_style(model, 4 + i);
            p.rect_filled(rect.expand(2.), 5. * k, style.ink);
            p.rect_filled(rect, 5. * k, style.fill);
            p.rect_stroke(
                rect.expand(2.),
                5. * k,
                Stroke::new(1.5, accent),
                egui::StrokeKind::Outside,
            );
        }
        if von(bit) {
            p.rect_stroke(
                rect.expand(4.),
                5. * k,
                Stroke::new(1.5, virtual_color),
                egui::StrokeKind::Outside,
            );
        }
    }
    for (i, (c, bit)) in [
        (geo.menus[0], XButtons::BACK),
        (geo.menus[1], XButtons::START),
        (geo.home, XButtons::GUIDE),
    ]
    .into_iter()
    .enumerate()
    {
        let c = at(c);
        if on(bit) {
            button_ring(p, c, 9. * k, press_style(model, 8 + i).fill, accent);
        }
        if von(bit) {
            p.circle_stroke(c, 11. * k + 3., Stroke::new(1.5, virtual_color));
        }
    }
    for (i, value) in [g.left_trigger, g.right_trigger].into_iter().enumerate() {
        let c = at(geo.triggers[i]);
        let style = press_style(model, 13 + i);
        if value > 0 {
            let press = 2. * k * value as f32 / 255.;
            let moved = c + vec2(0., press);
            part_image(p, &controls, 6 + i, c, k, accent);
            let clip = Rect::from_min_max(
                at((
                    geo.triggers[i].0 - 38.,
                    geo.trigger_bounds.1
                        - (geo.trigger_bounds.1 - geo.trigger_bounds.0) * value as f32 / 255.,
                )) + vec2(0., press),
                at((geo.triggers[i].0 + 38., geo.trigger_bounds.1)) + vec2(0., press),
            );
            part_image(
                &p.with_clip_rect(clip.intersect(p.clip_rect())),
                &controls,
                2 + i,
                moved,
                k,
                style.fill,
            );
            if backlight {
                // A numeric cue makes even a small analog press unambiguous on white triggers.
                let label = c;
                let text = format!(
                    "{} {}%",
                    if matches!(model.layout, Layout::Xbox) {
                        ["LT", "RT"][i]
                    } else {
                        ["L2", "R2"][i]
                    },
                    (value as f32 * 100. / 255.).round() as u8
                );
                let galley = p.layout_no_wrap(text, FontId::proportional(10.), Color32::WHITE);
                let badge_size = galley.size() + vec2(8., 4.);
                let offset = 35. * k + badge_size.x / 2. + 4.;
                let label = label + vec2(if i == 0 { -offset } else { offset }, 0.);
                let label = pos2(
                    label.x.clamp(
                        r.left() + badge_size.x / 2. + 2.,
                        r.right() - badge_size.x / 2. - 2.,
                    ),
                    label.y,
                );
                let badge = Rect::from_center_size(label, badge_size);
                p.rect_filled(badge, 5., Color32::from_rgb(13, 16, 23));
                p.rect_stroke(badge, 5., Stroke::new(1., accent), egui::StrokeKind::Inside);
                p.galley(badge.center() - galley.size() / 2., galley, Color32::WHITE);
            }
        }
        if out.is_some_and(|out| {
            if i == 0 {
                out.left_trigger > 0
            } else {
                out.right_trigger > 0
            }
        }) {
            part_image(p, &controls, 6 + i, c, k, virtual_color);
        }

        // Real curved shoulder-button mask, fitted to this controller family.
        let bit = if i == 0 { XButtons::LB } else { XButtons::RB };
        let c = at(geo.bumpers[i]);
        if on(bit) {
            let style = press_style(model, 15 + i);
            part_image(p, &controls, 8 + i, c, k, accent);
            part_image(p, &controls, 4 + i, c, k, style.ink);
            part_image(p, &controls, 4 + i, c + vec2(0., 0.75), k * 0.98, style.fill);
        }
        if von(bit) {
            part_image(p, &controls, 8 + i, c, k, virtual_color);
        }
    }
    for (i, (x, y, bit)) in [
        (g.thumb_lx, g.thumb_ly, XButtons::LTHUMB),
        (g.thumb_rx, g.thumb_ry, XButtons::RTHUMB),
    ]
    .into_iter()
    .enumerate()
    {
        let c = at(geo.caps[i]);
        let axes = vec2(axis_value(x), -axis_value(y));
        let travel = geo.radius * 0.55 * k;
        let moved = c + axes * travel;
        let style = press_style(model, 11 + i);
        if x != 0 || y != 0 || on(bit) {
            p.circle_filled(c, (geo.radius + 2.) * k, Color32::from_rgb(9, 11, 17));
            p.circle_stroke(c, (geo.radius + 3.) * k, Stroke::new(1., style.ink));
            p.circle_filled(
                moved + vec2(0., 2.),
                (geo.radius + 2.) * k,
                Color32::from_black_alpha(190),
            );
            part_image(p, &controls, i, moved, k, Color32::WHITE);
            if x != 0 || y != 0 {
                p.circle_stroke(moved, geo.radius * k + 1., Stroke::new(2., accent));
                p.line_segment([c, moved], Stroke::new(1.4, style.ink));
            }
            if on(bit) {
                button_ring(
                    p,
                    moved + vec2(0., 1.5),
                    geo.radius * 0.82 * k,
                    style.fill,
                    accent,
                );
                p.text(
                    moved + vec2(0., 1.5),
                    Align2::CENTER_CENTER,
                    ["L3", "R3"][i],
                    bold((14. * k).max(10.)),
                    style.ink,
                );
            }
        }
        if let Some(out) = out {
            let (x, y) = if i == 0 {
                (out.thumb_lx, out.thumb_ly)
            } else {
                (out.thumb_rx, out.thumb_ry)
            };
            if x != 0 || y != 0 || von(bit) {
                p.circle_stroke(
                    c + vec2(axis_value(x), -axis_value(y)) * travel,
                    geo.radius * k + 4.,
                    Stroke::new(1.5, virtual_color),
                );
            }
        }
    }
}

fn axis_value(v: i16) -> f32 {
    (v as f32 / 32767.).clamp(-1., 1.)
}

pub(crate) fn controller(
    ui: &mut egui::Ui,
    model: u8,
    g: &XGamepad,
    out: Option<&XGamepad>,
    sticks: &[Stick; 2],
    trail: &mut Trail,
) {
    controller_sized(ui, model, g, out, sticks, trail, 440., 0);
}
pub(crate) fn controller_sized(
    ui: &mut egui::Ui,
    model: u8,
    g: &XGamepad,
    out: Option<&XGamepad>,
    sticks: &[Stick; 2],
    trail: &mut Trail,
    maximum_width: f32,
    sony_extra: u8,
) {
    // Production always renders the real input snapshot. Test substitution is debug-only.
    #[cfg(debug_assertions)]
    let sample = snapshot_input(g);
    #[cfg(debug_assertions)]
    let g = &sample;
    let mi = model as usize % MODELS.len();
    let w = ui.available_width().min(maximum_width);
    let (resp, p) =
        ui.allocate_painter(vec2(ui.available_width(), w * ASPECT + 38.), Sense::hover());
    let area = Rect::from_center_size(resp.rect.center() - vec2(0., 19.), vec2(w, w * ASPECT));
    paint(&p, area, mi, g, out);
    paint_sony_buttons(&p, area, mi, sony_extra);
    let (o, k, _) = frame(area);
    let geo = MODELS[mi].layout.geometry();
    let c = pal(ui.visuals().dark_mode);
    for i in 0..2 {
        let (x, y) = if i == 0 {
            (g.thumb_lx, g.thumb_ly)
        } else {
            (g.thumb_rx, g.thumb_ry)
        };
        let n = (axis_value(x), axis_value(y));
        let center = o + vec2(geo.caps[i].0, geo.caps[i].1) * k;
        let radius = (geo.radius + 8.) * k;
        p.circle_stroke(
            center,
            radius * (sticks[i].max as f32 / 32767.).clamp(0., 1.),
            Stroke::new(1., c.line),
        );
        p.circle_stroke(
            center,
            radius * (sticks[i].dead as f32 / 32767.).clamp(0., 1.),
            Stroke::new(1., c.acc.gamma_multiply(0.35)),
        );
        if trail[i].front() != Some(&n) {
            trail[i].push_front(n)
        }
        trail[i].truncate(18);
        for (j, (a, b)) in trail[i].iter().zip(trail[i].iter().skip(1)).enumerate() {
            let f = 1. - j as f32 / 18.;
            p.line_segment(
                [
                    center + vec2(a.0, -a.1) * radius,
                    center + vec2(b.0, -b.1) * radius,
                ],
                Stroke::new(1.5 * f, c.acc.gamma_multiply(0.6 * f)),
            );
        }
    }
    let line = format!(
        "LS {:+.2} {:+.2}   RS {:+.2} {:+.2}   LT {:03}  RT {:03}",
        axis_value(g.thumb_lx),
        axis_value(g.thumb_ly),
        axis_value(g.thumb_rx),
        axis_value(g.thumb_ry),
        g.left_trigger,
        g.right_trigger
    );
    p.text(
        pos2(resp.rect.center().x, resp.rect.bottom() - 12.),
        Align2::CENTER_CENTER,
        line,
        mono(11.5),
        c.weak,
    );
    if out.is_some() {
        p.text(
            pos2(resp.rect.right() - 10., resp.rect.top() + 10.),
            Align2::RIGHT_TOP,
            tr("virtual"),
            FontId::proportional(11.),
            c.amber,
        );
    }
    p.text(
        resp.rect.min + vec2(10., 10.),
        Align2::LEFT_TOP,
        tr("real"),
        FontId::proportional(11.),
        c.acc,
    );
}
pub(crate) fn paint_model(p: &Painter, _id: egui::Id, r: Rect, mi: usize, g: &XGamepad) {
    paint(p, r, mi, g, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_or_unknown_preview_flags_never_replace_real_input() {
        let mut real = XGamepad::default();
        real.buttons.raw = XButtons::LB | XButtons::RTHUMB;
        real.left_trigger = 177;
        real.thumb_lx = -24000;
        real.thumb_ry = 28000;
        assert!(preview_input(&real, true, "") == real);
        assert!(preview_input(&real, true, "unknown") == real);
        assert!(preview_input(&real, false, "all") == real);
        assert!(preview_input(&real, true, "shoulders") != real);
    }
    #[test]
    fn active_parts_contrast_with_light_and_dark_artwork() {
        let luminance = |c: Color32| {
            let linear = |v: u8| {
                let v = v as f32 / 255.;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
        };
        for model in MODELS {
            for part in 0..17 {
                let idle = model.part_luma[part] as f32 / 255.;
                let active = luminance(press_style(model, part).fill);
                let contrast = (idle.max(active) + 0.05) / (idle.min(active) + 0.05);
                assert!(
                    contrast >= 3.7,
                    "{} part {} contrast {}",
                    model.id,
                    part,
                    contrast
                );
            }
        }
        let white = MODELS.iter().find(|m| m.id == "ps5-blue-robot").unwrap();
        assert!(press_style(white, 11).fill.r() < 30);
        assert!(press_style(white, 13).fill.r() < 30);
        let black = MODELS.iter().find(|m| m.id == "ps5-realistic").unwrap();
        assert!(press_style(black, 11).fill.r() > 240);
    }
    #[test]
    fn shoulder_masks_and_soft_backlight_are_present() {
        for model in MODELS {
            let atlas = image::load_from_memory(model.controls)
                .unwrap()
                .into_rgba8();
            assert_eq!(atlas.dimensions(), (1280, 128));
            for slot in 2..10 {
                let alpha: u32 = atlas
                    .enumerate_pixels()
                    .filter(|(x, _, _)| *x >= slot * 128 && *x < (slot + 1) * 128)
                    .map(|(_, _, p)| p[3] as u32)
                    .sum();
                assert!(alpha > 1000, "{} slot {}", model.id, slot);
            }
        }
        let glow = soft_glow_pixels();
        assert_eq!(glow.size, [256, 256]);
        assert!(glow.pixels[127 * 256 + 127].a() > 140);
        assert_eq!(glow.pixels[0].a(), 0);
        assert_eq!(
            glow.pixels[127 * 256 + 40].a(),
            glow.pixels[40 * 256 + 127].a()
        );
    }
    #[test]
    fn legacy_preferences_keep_the_manual_colour_and_allow_opt_out() {
        let mut settings: crate::model::Settings = serde_json::from_str(r#"{"accent":2}"#).unwrap();
        assert!(settings.pad_accent);
        assert_eq!(settings.accent, 2);
        settings.pad_accent = false;
        let restored: crate::model::Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(!restored.pad_accent);
        assert_eq!(restored.accent, 2);
    }
    #[test]
    fn catalog_covers_the_capture_and_keeps_selection_stable() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../assets/controllers/catalog.json")).unwrap();
        let captured = catalog["items"].as_array().unwrap();
        assert_eq!(MODELS.len(), 39);
        assert_eq!(captured.len(), MODELS.len());
        let mut ids = std::collections::HashSet::new();
        for (i, m) in MODELS.iter().enumerate() {
            assert!(ids.insert(m.id));
            assert_eq!(captured[i]["id"].as_str(), Some(m.id));
            let mut settings = crate::model::Settings::default();
            settings.pad_skin = format!("builtin:{}", m.id);
            let restored: crate::model::Settings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(selected_index(&restored.pad_skin, restored.pad_model), i);
            for bytes in [m.thumbnail, m.body, m.controls] {
                let image = image::load_from_memory(bytes).unwrap().into_rgba8();
                assert!(
                    image.pixels().any(|p| p[3] == 0),
                    "{} needs transparency",
                    m.id
                );
                assert!(image.pixels().any(|p| p[3] == 255));
            }
        }
        assert_eq!(
            (1..=3)
                .map(|f| MODELS.iter().filter(|m| m.matches_filter(f)).count())
                .collect::<Vec<_>>(),
            [23, 9, 7]
        );
        assert_eq!(
            (0..4).map(|n| selected_index("", n)).collect::<Vec<_>>(),
            [0, 1, 2, 1]
        );
        assert_eq!(selected_index("builtin:missing", 255), 1);
    }
    #[test]
    fn shipped_catalog_has_no_animated_controller() {
        assert!(MODELS.iter().all(|model| !model.animated()));
    }

    #[test]
    fn popout_uses_live_feedback_without_creating_a_glow() {
        let ctx = egui::Context::default();
        crate::ui::set_fonts(&ctx);
        let mut pressed = XGamepad::default();
        pressed.thumb_lx = 24000;
        pressed.thumb_ry = -17000;
        pressed.buttons.raw = XButtons::LTHUMB | XButtons::RB;
        pressed.right_trigger = 230;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_overlay(ui.painter(), Rect::from_min_size(Pos2::ZERO, vec2(576., 500.)), 1, &pressed, 0);
        });
        assert!(!output.shapes.is_empty());
        assert!(ctx.data_mut(|d| d.get_temp::<TextureHandle>(egui::Id::new("controller-soft-backlight"))).is_none());
        output.textures_delta.clear();
    }

    #[test]
    fn live_input_paths_render_and_player_cache_stays_bounded() {
        let ctx = egui::Context::default();
        crate::ui::set_fonts(&ctx);
        let mut pressed = XGamepad::default();
        pressed.buttons.raw = u16::MAX;
        pressed.left_trigger = 255;
        pressed.right_trigger = 170;
        pressed.thumb_lx = i16::MIN;
        pressed.thumb_ly = i16::MAX;
        pressed.thumb_rx = i16::MAX;
        pressed.thumb_ry = i16::MIN;
        for mi in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let ctx = ui.ctx();
                let p = ctx.layer_painter(egui::LayerId::background());
                paint(
                    &p,
                    Rect::from_min_size(Pos2::ZERO, vec2(576., 500.)),
                    mi,
                    &pressed,
                    Some(&pressed),
                );
            });
            assert!(!output.shapes.is_empty());
            output.textures_delta.clear();
        }
        let cache = ctx
            .data_mut(|d| d.get_temp::<Cache>(egui::Id::new("controller-playback-cache")))
            .unwrap();
        assert_eq!(cache.lock().unwrap().len(), 2);
    }
}

/// Shop-style card: picture on a soft stage, name and tag below. Hover lifts the card and plays
/// a demo input on the picture; the chosen card has an accent ring and a check badge.
pub(crate) fn tile(
    ui: &mut egui::Ui,
    w: f32,
    name: &str,
    tag: &str,
    chosen: bool,
    picture: impl FnOnce(&Painter, egui::Rect, Option<XGamepad>),
) -> egui::Response {
    let c = pal(ui.visuals().dark_mode);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, w * 0.66 + 58.0), Sense::click());
    let ctx = ui.ctx().clone();
    let h = crate::ui::ease(anim(&ctx, resp.id, resp.hovered(), 0.18));
    let s = anim(&ctx, resp.id.with("sel"), chosen, 0.2);
    let r = rect.translate(vec2(0.0, -3.0 * h));
    let p = ui.painter();
    p.add(
        egui::epaint::Shadow {
            offset: [0, (4.0 + 6.0 * h) as i8],
            blur: (10.0 + 14.0 * h) as u8,
            spread: 0,
            color: Color32::from_black_alpha((30.0 + 40.0 * h) as u8),
        }
        .as_shape(r, 18.0),
    );
    p.rect_filled(r, 18.0, c.card);
    let stage = egui::Rect::from_min_max(
        r.min + vec2(8.0, 8.0),
        pos2(r.right() - 8.0, r.bottom() - 50.0),
    );
    p.rect_filled(stage, 13.0, c.idle.lerp_to_gamma(c.hover, 0.5 * h));
    let demo = (resp.hovered() && crate::ui::motion()).then(|| {
        ctx.request_repaint();
        crate::setup::demo(ctx.input(|i| i.time), false).0
    });
    picture(p, stage.shrink(10.0), demo);
    let mut title = egui::text::LayoutJob::simple(name.to_owned(), bold(15.0), c.text, w - 32.0);
    title.wrap.max_rows = 1;
    title.wrap.overflow_character = Some('…');
    let galley = p.layout_job(title);
    p.galley(
        pos2(r.left() + 16.0, r.bottom() - 32.0 - galley.size().y / 2.0),
        galley,
        c.text,
    );
    p.text(
        pos2(r.left() + 16.0, r.bottom() - 14.0),
        Align2::LEFT_CENTER,
        tag,
        FontId::proportional(12.0),
        c.weak,
    );
    if s > 0.01 {
        p.rect_stroke(
            r,
            18.0,
            Stroke::new(2.0 * s, c.acc),
            egui::StrokeKind::Inside,
        );
        let b = pos2(r.right() - 22.0, r.top() + 22.0);
        p.circle_filled(b, 11.0 * s, c.acc);
        crate::icons::draw(
            p,
            crate::icons::Icon::Check,
            egui::Rect::from_center_size(b, vec2(12.0, 12.0) * s),
            Color32::WHITE,
        );
    }
    resp.on_hover_text(name)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Opt-in screenshot hook for visual QA, matching the existing PADMIMIC_SHOW dev hook.
/// A normal launch never requests a screenshot or writes an image.
#[cfg(debug_assertions)]
pub(crate) fn dev_snapshot(ctx: &egui::Context) {
    let Some(path) = std::env::var_os("PADMIMIC_SNAPSHOT").filter(|p| !p.is_empty()) else {
        return;
    };
    if ctx.cumulative_frame_nr() == 12 {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
    }
    let screenshot = ctx.input(|i| {
        i.events.iter().find_map(|e| {
            if let egui::Event::Screenshot { image, .. } = e {
                Some(image.clone())
            } else {
                None
            }
        })
    });
    if let Some(image) = screenshot {
        let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        if image::save_buffer(
            std::path::Path::new(&path),
            &rgba,
            image.size[0] as u32,
            image.size[1] as u32,
            image::ColorType::Rgba8,
        )
        .is_ok()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

#[cfg(debug_assertions)]
fn snapshot_input(real: &XGamepad) -> XGamepad {
    let enabled = std::env::var_os("PADMIMIC_SNAPSHOT").is_some_and(|p| !p.is_empty());
    let mode = std::env::var("PADMIMIC_DEMO_INPUT").unwrap_or_default();
    preview_input(real, enabled, &mode)
}
#[cfg(any(test, debug_assertions))]
fn preview_input(real: &XGamepad, capture_enabled: bool, mode: &str) -> XGamepad {
    if !capture_enabled || !matches!(mode, "neutral" | "sticks" | "shoulders" | "face" | "all") {
        return *real;
    }
    let mut g = XGamepad::default();
    if mode == "shoulders" || mode == "all" {
        g.buttons.raw |= XButtons::LB | XButtons::RB;
        g.left_trigger = 192;
        g.right_trigger = 128;
    }
    if mode == "sticks" || mode == "all" {
        g.buttons.raw |= XButtons::LTHUMB | XButtons::RTHUMB;
        g.thumb_lx = 24000;
        g.thumb_ly = -16000;
        g.thumb_rx = -22000;
        g.thumb_ry = 14000;
    }
    if mode == "face" || mode == "all" {
        g.buttons.raw |= XButtons::A | XButtons::B | XButtons::X | XButtons::Y | XButtons::UP;
    }
    g
}
