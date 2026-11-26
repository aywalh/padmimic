//! padmimic's own vector icons and logo, drawn with egui shapes on a 24×24 grid (like an SVG viewBox),
//! so nothing depends on emoji fonts and every glyph looks the same on every PC.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, vec2};
use std::f32::consts::TAU;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Icon { Pad, Keyboard, Gear, Gauge, Info, Up, Bolt, Swap, More, Back, Chevron, Close, Trash, Mouse, Palette, Globe, Apps, Chip, Check, Sliders, PopOut }

/// Points on a circle arc; angles in turns, 0 = up, clockwise.
fn arc(c: Pos2, r: f32, from: f32, to: f32) -> Vec<Pos2> {
    let n = (((to - from).abs() * 48.0).ceil() as usize).max(2);
    (0..=n).map(|i| {
        let a = (from + (to - from) * i as f32 / n as f32) * TAU;
        c + vec2(a.sin(), -a.cos()) * r
    }).collect()
}

pub fn draw(p: &Painter, icon: Icon, r: Rect, col: Color32) {
    let k = r.width().min(r.height()) / 24.0;
    let o = r.center() - vec2(12.0, 12.0) * k;
    let at = |x: f32, y: f32| o + vec2(x, y) * k;
    let s = Stroke::new(1.8 * k, col);
    let line = |pts: &[(f32, f32)]| { p.add(Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), s)); };
    let closed = |pts: &[(f32, f32)]| { p.add(Shape::closed_line(pts.iter().map(|&(x, y)| at(x, y)).collect(), s)); };
    let ring = |x: f32, y: f32, rad: f32| { p.circle_stroke(at(x, y), rad * k, s); };
    let dot = |x: f32, y: f32, rad: f32| { p.circle_filled(at(x, y), rad * k, col); };
    let rr = |x0: f32, y0: f32, x1: f32, y1: f32, rad: f32| { p.rect_stroke(Rect::from_min_max(at(x0, y0), at(x1, y1)), rad * k, s, StrokeKind::Middle); };
    let bow = |x: f32, y: f32, rad: f32, from: f32, to: f32| { p.add(Shape::line(arc(at(x, y), rad * k, from, to), s)); };
    match icon {
        Icon::Pad => {
            rr(2.5, 7.0, 21.5, 17.5, 5.0);
            line(&[(5.8, 12.2), (10.2, 12.2)]);
            line(&[(8.0, 10.0), (8.0, 14.4)]);
            dot(15.6, 10.9, 1.15);
            dot(17.9, 13.3, 1.15);
        }
        Icon::Keyboard => {
            rr(2.5, 6.0, 21.5, 18.0, 2.8);
            for x in [6.5, 9.5, 12.5, 15.5] { dot(x + 0.5, 9.6, 0.95); }
            for x in [8.0, 11.0, 14.0, 17.0] { dot(x - 0.5, 12.4, 0.95); }
            line(&[(8.5, 15.2), (15.5, 15.2)]);
        }
        Icon::Gear => {
            let mut pts = vec![];
            for i in 0..8 {
                let a = i as f32 / 8.0;
                for (da, rad) in [(-0.028, 9.6), (0.028, 9.6), (0.0625 - 0.03, 7.2), (0.0625 + 0.03, 7.2)] {
                    let t = (a + da) * TAU;
                    pts.push((12.0 + t.sin() * rad, 12.0 - t.cos() * rad));
                }
            }
            closed(&pts);
            ring(12.0, 12.0, 3.0);
        }
        Icon::Gauge => {
            bow(12.0, 13.5, 8.5, -0.36, 0.36);
            let t = 0.12 * TAU;
            line(&[(12.0, 13.5), (12.0 + t.sin() * 5.5, 13.5 - t.cos() * 5.5)]);
            dot(12.0, 13.5, 1.5);
        }
        Icon::Info => {
            ring(12.0, 12.0, 9.0);
            dot(12.0, 7.9, 1.2);
            line(&[(12.0, 11.0), (12.0, 16.8)]);
        }
        Icon::Up => {
            line(&[(12.0, 19.5), (12.0, 5.0)]);
            line(&[(6.0, 11.0), (12.0, 5.0), (18.0, 11.0)]);
        }
        Icon::Bolt => closed(&[(13.5, 2.5), (5.0, 13.5), (11.0, 13.5), (10.0, 21.5), (19.0, 10.0), (13.0, 10.0)]),
        Icon::Swap => {
            line(&[(4.0, 8.5), (19.0, 8.5)]);
            line(&[(15.0, 4.5), (19.0, 8.5), (15.0, 12.5)]);
            line(&[(20.0, 15.5), (5.0, 15.5)]);
            line(&[(9.0, 11.5), (5.0, 15.5), (9.0, 19.5)]);
        }
        Icon::More => { for x in [5.5, 12.0, 18.5] { dot(x, 12.0, 1.7); } }
        Icon::Back => line(&[(15.0, 5.0), (8.0, 12.0), (15.0, 19.0)]),
        Icon::Chevron => line(&[(9.5, 5.5), (16.0, 12.0), (9.5, 18.5)]),
        Icon::PopOut => {
            line(&[(11.0, 4.5), (6.5, 4.5), (4.5, 6.5), (4.5, 17.5), (6.5, 19.5), (17.5, 19.5), (19.5, 17.5), (19.5, 13.0)]);
            line(&[(11.5, 12.5), (19.5, 4.5)]);
            line(&[(14.0, 4.5), (19.5, 4.5), (19.5, 10.0)]);
        }
        Icon::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Icon::Trash => {
            line(&[(4.0, 6.8), (20.0, 6.8)]);
            line(&[(9.3, 6.8), (10.0, 4.0), (14.0, 4.0), (14.7, 6.8)]);
            closed(&[(6.2, 6.8), (7.2, 20.0), (16.8, 20.0), (17.8, 6.8)]);
            line(&[(10.0, 10.5), (10.0, 16.5)]);
            line(&[(14.0, 10.5), (14.0, 16.5)]);
        }
        Icon::Mouse => {
            rr(6.5, 3.0, 17.5, 21.0, 5.5);
            line(&[(12.0, 3.5), (12.0, 9.0)]);
        }
        Icon::Palette => {
            ring(12.0, 12.0, 9.0);
            for (x, y) in [(8.0, 9.5), (12.0, 7.0), (16.0, 9.5), (16.5, 14.0)] { dot(x, y, 1.4); }
        }
        Icon::Globe => {
            ring(12.0, 12.0, 9.0);
            let ell: Vec<Pos2> = (0..=48).map(|i| { let t = i as f32 / 48.0 * TAU; at(12.0 + t.sin() * 4.0, 12.0 - t.cos() * 9.0) }).collect();
            p.add(Shape::line(ell, s));
            line(&[(3.2, 12.0), (20.8, 12.0)]);
        }
        Icon::Apps => {
            rr(4.0, 4.0, 10.5, 10.5, 2.2);
            rr(13.5, 4.0, 20.0, 10.5, 2.2);
            rr(4.0, 13.5, 10.5, 20.0, 2.2);
            rr(13.5, 13.5, 20.0, 20.0, 2.2);
        }
        Icon::Chip => {
            rr(6.0, 6.0, 18.0, 18.0, 2.5);
            rr(9.5, 9.5, 14.5, 14.5, 1.0);
            for v in [9.0, 12.0, 15.0] {
                line(&[(v, 2.8), (v, 6.0)]);
                line(&[(v, 18.0), (v, 21.2)]);
                line(&[(2.8, v), (6.0, v)]);
                line(&[(18.0, v), (21.2, v)]);
            }
        }
        Icon::Check => line(&[(5.0, 12.5), (10.0, 17.5), (19.0, 7.0)]),
        Icon::Sliders => {
            line(&[(4.0, 8.0), (20.0, 8.0)]);
            line(&[(4.0, 16.0), (20.0, 16.0)]);
            dot(9.0, 8.0, 2.4);
            dot(15.0, 16.0, 2.4);
        }
    }
}

/// Padmimic monogram with aligned bowls and stems mirrored around its centre.
pub fn logo(p: &Painter, r: Rect, ink: Color32, t: f32) {
    let scale = r.width().min(r.height()) / 64.0;
    let origin = r.center() - vec2(32.0, 32.0) * scale;
    let stroke = Stroke::new(5.0 * scale, ink.gamma_multiply(t));
    for turned in [false, true] {
        let at = |x, y| origin + (if turned { vec2(64.0 - x, 64.0 - y) } else { vec2(x, y) }) * scale;
        let mut line = vec![at(11.0, 50.0), at(11.0, 32.0)];
        for i in 1..=24 {
            let angle = -std::f32::consts::PI + i as f32 * TAU * 0.75 / 24.0;
            line.push(at(20.0 + 9.0 * angle.cos(), 32.0 + 9.0 * angle.sin()));
        }
        line.push(at(11.0, 41.0));
        p.circle_filled(line[0], stroke.width * 0.5, stroke.color);
        p.circle_filled(*line.last().unwrap(), stroke.width * 0.5, stroke.color);
        p.add(Shape::line(line, stroke));
    }
}
