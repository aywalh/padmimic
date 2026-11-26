//! Original vector mark: two mirrored P's.
use eframe::egui::{Color32, Painter, Pos2, Shape, Stroke, pos2, vec2};
use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::OnceLock;

/// Time the mark needs to draw itself.
pub const DRAW_SECONDS: f32 = 0.30;
/// Start screen: the mark draws in the middle, then the screen fades out.
pub const SPLASH_SECONDS: f32 = 0.67;
const FADE_FROM: f32 = 0.42;

/// The two-P mark centred on `center`; `scale` 1.0 is about 41 x 42 points. `seconds` since it began drawing.
pub fn mark(p: &Painter, center: Pos2, scale: f32, ink: Color32, seconds: f32) {
    let draw = smooth(seconds / DRAW_SECONDS);
    if draw <= 0.0 { return }
    let origin = center - vec2(20.5, 21.0) * scale;
    let stroke = Stroke::new(3.3 * scale, ink);
    static MARK: OnceLock<Vec<Pos2>> = OnceLock::new();
    let path = MARK.get_or_init(|| {
        let mut points = vec![pos2(4.0, 35.0), pos2(4.0, 21.0)];
        for i in 1..=24 {
            let angle = -PI + 3.0 * FRAC_PI_2 * i as f32 / 24.0;
            points.push(pos2(11.0 + 7.0 * angle.cos(), 21.0 + 7.0 * angle.sin()));
        }
        points.push(pos2(4.0, 28.0));
        points
    });
    trace(p, path.iter().map(|&pt| origin + pt.to_vec2() * scale), stroke, draw);
    trace(p, path.iter().map(|pt| origin + vec2(41.0 - pt.x, 42.0 - pt.y) * scale), stroke, draw);
}

/// Start screen over the whole window at `seconds` after launch; false once it is gone.
pub fn splash(p: &Painter, full: eframe::egui::Rect, bg: Color32, ink: Color32, seconds: f32) -> bool {
    if seconds >= SPLASH_SECONDS { return false }
    let fade = 1.0 - smooth((seconds - FADE_FROM) / (SPLASH_SECONDS - FADE_FROM));
    p.rect_filled(full, 0.0, bg.gamma_multiply(fade));
    let pop = smooth(seconds / 0.2);
    mark(p, full.center(), 1.45 * (0.92 + 0.08 * pop), ink.gamma_multiply(pop * fade), seconds - 0.05);
    true
}

fn smooth(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn trace(p: &Painter, points: impl Iterator<Item = Pos2>, stroke: Stroke, progress: f32) {
    let mut points: Vec<_> = points.collect();
    if progress < 1.0 {
        let total: f32 = points.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
        let mut remaining = total * progress;
        for i in 1..points.len() {
            let length = points[i - 1].distance(points[i]);
            if remaining < length {
                points[i] = points[i - 1].lerp(points[i], remaining / length);
                points.truncate(i + 1);
                break;
            }
            remaining -= length;
        }
    }
    if let Some(&first) = points.first() {
        p.circle_filled(first, stroke.width * 0.5, stroke.color);
    }
    if points.len() > 1 {
        p.circle_filled(*points.last().unwrap(), stroke.width * 0.5, stroke.color);
        p.add(Shape::line(points, stroke));
    }
}
