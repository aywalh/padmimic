//! Settings placement across the actual monitor work areas, including desktop gaps.
use eframe::egui::{Rect, pos2};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Desktop {
    monitors: Vec<(i32, i32, i32, i32)>,
    checked: Option<Instant>,
}

impl Desktop {
    pub fn settings_side(&mut self, outer: Rect, pixels_per_point: f32, smart: bool, width: f32, gap: f32) -> Option<bool> {
        let monitors = if smart {
            if self.checked.is_none_or(|at| at.elapsed() >= Duration::from_secs(1)) {
                self.monitors = crate::winio::monitor_work_areas();
                self.checked = Some(Instant::now());
            }
            self.monitors.clone()
        } else {
            vec![crate::winio::work_area((outer.center().x * pixels_per_point) as i32, (outer.center().y * pixels_per_point) as i32)]
        };
        let monitors = if monitors.is_empty() {
            vec![crate::winio::work_area((outer.center().x * pixels_per_point) as i32, (outer.center().y * pixels_per_point) as i32)]
        } else { monitors };
        let work: Vec<_> = monitors.into_iter().map(|(l,t,r,b)| Rect::from_min_max(pos2(l as f32, t as f32) / pixels_per_point, pos2(r as f32, b as f32) / pixels_per_point)).collect();
        choose_side(outer, &work, width, gap)
    }
}

fn choose_side(outer: Rect, work: &[Rect], width: f32, gap: f32) -> Option<bool> {
    let right = Rect::from_min_max(pos2(outer.right() + gap, outer.top()), pos2(outer.right() + gap + width, outer.bottom()));
    let left = Rect::from_min_max(pos2(outer.left() - gap - width, outer.top()), pos2(outer.left() - gap, outer.bottom()));
    if covered(right, work) { Some(true) } else if covered(left, work) { Some(false) } else { None }
}

/// A bounding box of the desktop is insufficient: it would count gaps as usable space.
fn covered(rect: Rect, work: &[Rect]) -> bool {
    let mut remaining = vec![rect];
    for area in work {
        let mut next = vec![];
        for piece in remaining {
            let cut = piece.intersect(*area);
            if !cut.is_positive() { next.push(piece); continue; }
            for rest in [
                Rect::from_min_max(piece.min, pos2(cut.left(), piece.bottom())),
                Rect::from_min_max(pos2(cut.right(), piece.top()), piece.max),
                Rect::from_min_max(pos2(cut.left(), piece.top()), pos2(cut.right(), cut.top())),
                Rect::from_min_max(pos2(cut.left(), cut.bottom()), pos2(cut.right(), piece.bottom())),
            ] {
                if rest.width() > 0.1 && rest.height() > 0.1 { next.push(rest); }
            }
        }
        remaining = next;
        if remaining.is_empty() { return true; }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;
    fn screen(x: f32, width: f32, height: f32) -> Rect { Rect::from_min_size(pos2(x, 0.), vec2(width, height)) }
    #[test]
    fn crosses_adjacent_monitors_and_returns_to_the_right() {
        let screens = [screen(0., 1920., 1080.), screen(1920., 1920., 1080.)];
        let main = |x| Rect::from_min_size(pos2(x, 50.), vec2(1000., 850.));
        assert_eq!(choose_side(main(900.), &screens, 440., 10.), Some(true));
        assert_eq!(choose_side(main(900.), &screens[..1], 440., 10.), Some(false));
        assert_eq!(choose_side(main(2800.), &screens, 440., 10.), Some(false));
        assert_eq!(choose_side(main(1800.), &screens, 440., 10.), Some(true));
    }
    #[test]
    fn rejects_gaps_and_uncovered_parts_of_offset_monitors() {
        let main = Rect::from_min_size(pos2(900., 50.), vec2(1000., 850.));
        assert_eq!(choose_side(main, &[screen(0.,1920.,1080.), screen(2200.,1920.,1080.)],440.,10.), Some(false));
        assert_eq!(choose_side(main, &[screen(0.,1920.,1080.), screen(1920.,1920.,700.)],440.,10.), Some(false));
        assert!(covered(screen(-100.,200.,800.), &[screen(-1920.,1920.,1080.), screen(0.,1920.,1080.)]));
        assert_eq!(choose_side(screen(0.,1900.,1080.), &[screen(0.,1920.,1080.)],440.,10.), None);
    }
}
