//! Real controller + keyboard/mouse -> AntiMicroX-style virtual buttons -> keyboard, mouse and a virtual
//! Xbox 360 pad. Runs on one time-critical thread; the UI only swaps in new `Arc<Profile>` / `Arc<Settings>`.

use crate::model::*;
use crate::{sony_input, winio};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU8, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{mem, ptr, thread};
use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};
use windows_sys::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Media::timeBeginPeriod;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};
use windows_sys::Win32::UI::Input::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// ---------------------------------------------------------------- shared state

#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub loop_hz: u32,
    pub pad_hz: u32, // state changes/s of the real controller
    pub slot: Option<u32>,
    pub virt_slot: Option<u32>,
}

#[derive(Default)]
pub struct Live {
    pub set: usize,
    pub phys: XGamepad,
    pub sony: Option<sony_input::Reading>,
    pub out: XGamepad,
    pub active: Vec<Elem>, // virtual buttons currently pressed (raw input)
    pub stats: Stats,
    pub pps: (f32, f32),
    pub status: String,
    pub vigem: bool,
    pub lat: Lat,
}

/// Timing histogram in 50 µs buckets up to 50 ms: cheap enough to fill every engine tick.
#[derive(Clone)]
pub struct Hist {
    b: Vec<u32>,
    pub n: u64,
    sum: f64,
    pub max: f64,
}

impl Default for Hist {
    fn default() -> Self {
        Hist { b: vec![0; 1000], n: 0, sum: 0.0, max: 0.0 }
    }
}

impl Hist {
    const STEP: f64 = 50.0; // µs
    pub fn add(&mut self, us: f64) {
        let i = ((us / Self::STEP) as usize).min(self.b.len() - 1);
        self.b[i] += 1;
        self.n += 1;
        self.sum += us;
        self.max = self.max.max(us);
    }
    pub fn avg(&self) -> f64 {
        if self.n == 0 { 0.0 } else { self.sum / self.n as f64 }
    }
    /// Upper edge of the bucket holding the p-th fraction of samples.
    pub fn pct(&self, p: f64) -> f64 {
        let target = ((self.n as f64 * p).ceil() as u64).max(1);
        let mut c = 0u64;
        for (i, &v) in self.b.iter().enumerate() {
            c += v as u64;
            if c >= target { return (i + 1) as f64 * Self::STEP }
        }
        0.0
    }
}

/// Latency meter, collected since `since` (the UI resets it by replacing it).
#[derive(Clone)]
pub struct Lat {
    pub since: Instant,
    pub pad: Hist,  // gap between new reports of the real controller
    pub tick: Hist, // actual engine loop period
    pub virt: Hist, // virtual pad update sent -> visible in XInput
}

impl Default for Lat {
    fn default() -> Self {
        Lat { since: Instant::now(), pad: Hist::default(), tick: Hist::default(), virt: Hist::default() }
    }
}

/// Status text while ViGEmBus is missing (the UI offers to install it).
pub const VIGEM_MISSING: &str = "ViGEmBus missing – keyboard/mouse output only";

pub enum Event {
    LoadProfile(String),
}

pub struct Shared {
    pub profile: Mutex<Arc<Profile>>,
    pub settings: Mutex<Arc<Settings>>,
    pub on: AtomicBool,
    pub live: Mutex<Live>,
    pub set_cmd: Mutex<Option<usize>>,
    pub events: Mutex<Vec<Event>>,
}

// ---------------------------------------------------------------- pure element math (AntiMicroX)

/// Result of evaluating a stick like `JoyControlStick`.
#[derive(Debug, Clone, Copy)]
pub struct StickEval {
    pub safe: bool,
    pub bearing: f64, // 0 = up, clockwise
    pub raw: f64,
    pub radial: f64, // 0..1 past dead zone
    pub dx: f64,
    pub dy: f64,
}

/// `x`, `y_up` in XInput convention (-32768..32767, up positive).
pub fn eval_stick(s: &Stick, x: i32, y_up: i32) -> StickEval {
    let (x, y) = (x as f64, -(y_up as f64)); // AntiMicro works in SDL space: y down positive
    let raw = (x * x + y * y).sqrt();
    let ang = x.atan2(-y);
    let (sn, cs) = (ang.sin(), ang.cos());
    let mut bearing = ang.to_degrees();
    if bearing < 0.0 { bearing += 360.0 }
    let full = (if sn != 0.0 { 1.0 / sn.abs() } else { 2.0 }).min(if cs != 0.0 { 1.0 / cs.abs() } else { 2.0 });
    let circ = (full - 1.0) * s.square as f64 / 100.0 + 1.0;
    let adj = |v: f64| if circ > 1.0 { v / circ } else { v };
    let (dead, max) = (s.dead as f64, s.max as f64);
    let zone = |v: f64, d: f64| ((adj(v).abs() - adj(d)) / (max - adj(d))).clamp(0.0, 1.0);
    StickEval {
        safe: raw * raw > dead * dead,
        bearing,
        raw,
        radial: zone(raw, dead),
        dx: zone(x, (dead * sn).abs().round()),
        dy: zone(y, (dead * cs).abs().round()),
    }
}

/// 8-way direction index (0 = up, clockwise) for a bearing, using the diagonal range.
fn dir8(bearing: f64, diag: u8) -> usize {
    let diag = diag.clamp(1, 90) as f64;
    let card = (360.0 - diag * 4.0) / 4.0;
    let init_right = card / 2.0;
    if bearing <= init_right || bearing >= 360.0 - init_right {
        return UP;
    }
    let mut edge = init_right;
    for (i, w) in [diag, card, diag, card, diag, card, diag].into_iter().enumerate() {
        edge += w;
        if bearing < edge {
            return i + 1;
        }
    }
    7
}

/// Which direction buttons are pressed for a stick/d-pad direction given the mode.
pub fn dir_buttons(mode: DirMode, d8: usize) -> Vec<usize> {
    match mode {
        DirMode::Standard if d8 % 2 == 1 => vec![(d8 + 7) % 8, (d8 + 1) % 8],
        DirMode::Standard | DirMode::EightWay => vec![d8],
        DirMode::FourCardinal => vec![],
        DirMode::FourDiagonal => vec![],
    }
}

pub fn stick_buttons(s: &Stick, e: &StickEval) -> Vec<usize> {
    if !e.safe {
        return vec![];
    }
    match s.mode {
        DirMode::FourCardinal => vec![(((e.bearing + 45.0) / 90.0) as usize % 4) * 2],
        DirMode::FourDiagonal => vec![((e.bearing / 90.0) as usize % 4) * 2 + 1],
        m => dir_buttons(m, dir8(e.bearing, s.diag)),
    }
}

/// XInput d-pad bits -> pressed d-pad buttons.
pub fn dpad_buttons(mode: DirMode, bits: u16) -> Vec<usize> {
    let (u, d, l, r) = (bits & XButtons::UP != 0, bits & XButtons::DOWN != 0, bits & XButtons::LEFT != 0, bits & XButtons::RIGHT != 0);
    let d8 = match (u, r, d, l) {
        (true, true, _, false) => 1, (false, true, true, _) => 3, (_, false, true, true) => 5, (true, _, false, true) => 7,
        (true, _, _, _) => 0, (_, true, _, _) => 2, (_, _, true, _) => 4, (_, _, _, true) => 6,
        _ => return vec![],
    };
    match mode {
        DirMode::Standard => [(u, UP), (r, RIGHT), (d, DOWN), (l, LEFT)].into_iter().filter(|x| x.0).map(|x| x.1).collect(),
        DirMode::EightWay => vec![d8],
        DirMode::FourCardinal => vec![[0, 0, 2, 2, 4, 4, 6, 6][d8]],
        DirMode::FourDiagonal => if d8 % 2 == 1 { vec![d8] } else { vec![] },
    }
}

/// `JoyAxis::calculateThrottledValue`
pub fn throttle(v: i32, t: i8) -> i32 {
    match t {
        -2 => -v.abs(),
        -1 => (v - 32767) / 2,
        1 => (v + 32767) / 2,
        2 => v.abs(),
        _ => v,
    }
}

pub fn axis_distance(v: i32, dead: i32, max: i32) -> f64 {
    ((v.abs() - dead) as f64 / (max - dead).max(1) as f64).clamp(0.0, 1.0)
}

/// `JoyButton::mouseEvent` curve part; `ease` is the time the high-end easing phase started.
pub fn curve(c: &MouseCfg, d: f64, ease: &mut Option<u64>, now: u64) -> f64 {
    match c.curve {
        Curve::Linear => d,
        Curve::Quadratic => d * d,
        Curve::Cubic => d * d * d,
        Curve::QuadraticExtreme => if d >= 0.95 { d * d * 1.5 } else { d * d },
        Curve::Power => d.powf(1.0 / (c.sens as f64).clamp(1e-3, 1e3)).clamp(0.0, 1.0),
        Curve::EnhancedPrecision => {
            if d <= 0.4 { d * 0.37 } else if d <= 0.75 { d - 0.252 } else { d * 2.008 - 1.008 }
        }
        Curve::EasingQuadratic | Curve::EasingCubic => {
            if d <= 0.4 {
                *ease = None;
                d * 0.38
            } else if d <= 0.75 {
                *ease = None;
                d - 0.248
            } else {
                let t0 = *ease.get_or_insert(now);
                let secs = (now - t0) as f64 / 1000.0;
                let dur = c.easing as f64;
                let m = if dur > 0.0 && secs < dur {
                    let e = secs / dur;
                    0.5 * if c.curve == Curve::EasingQuadratic { e * e } else { e * e * e } + 1.0
                } else {
                    1.5
                };
                m * d * 1.33067 - 0.496005
            }
        }
    }
}

// ---------------------------------------------------------------- per-tick output frame

#[derive(Default)]
struct Acc {
    b: u16,
    lt: u8,
    rt: u8,
    l: (f64, f64),
    r: (f64, f64),
}

impl Acc {
    fn push(&mut self, o: Out, v: f64) {
        let t = (v.clamp(0.0, 1.0) * 255.0) as u8;
        match o {
            Out::LT => self.lt = self.lt.max(t), Out::RT => self.rt = self.rt.max(t),
            Out::LUp => self.l.1 += v, Out::LDown => self.l.1 -= v, Out::LLeft => self.l.0 -= v, Out::LRight => self.l.0 += v,
            Out::RUp => self.r.1 += v, Out::RDown => self.r.1 -= v, Out::RLeft => self.r.0 -= v, Out::RRight => self.r.0 += v,
            o => self.b |= o.bit(),
        }
    }
    fn stick(&mut self, s: VStick) -> &mut (f64, f64) {
        if s == VStick::Left { &mut self.l } else { &mut self.r }
    }
    /// `walk` scales the left stick after the circle clamp, so walking diagonals stay as long as straight lines.
    fn gamepad(mut self, walk: f64) -> XGamepad {
        for v in [&mut self.l, &mut self.r] {
            let len = (v.0 * v.0 + v.1 * v.1).sqrt();
            if len > 1.0 { v.0 /= len; v.1 /= len; } // keyboard diagonals on the unit circle
        }
        self.l = (self.l.0 * walk, self.l.1 * walk);
        let ax = |v: f64| (v.clamp(-1.0, 1.0) * 32767.0) as i16;
        XGamepad {
            buttons: XButtons(self.b), left_trigger: self.lt, right_trigger: self.rt,
            thumb_lx: ax(self.l.0), thumb_ly: ax(self.l.1), thumb_rx: ax(self.r.0), thumb_ry: ax(self.r.1),
        }
    }
}

struct Spring {
    dir: u8,
    d: f64,
    w: u32,
    h: u32,
    rel: bool,
    release: u8,
}

struct Frame {
    keys: [bool; 256],
    mouse: [bool; 6],
    pad: Acc,
    moves: Vec<(u8, f64)>, // direction, px/s
    springs: Vec<Spring>,
    wheel: [u32; 4],
    text: Vec<String>,
    exec: Vec<(String, String)>,
    load: Option<String>,
    set_to: Option<u8>,
    set_req: Option<(Elem, u8, SetCond)>,
    mouse_mod: f64,
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            keys: [false; 256], mouse: [false; 6], pad: Acc::default(), moves: vec![], springs: vec![], wheel: [0; 4],
            text: vec![], exec: vec![], load: None, set_to: None, set_req: None, mouse_mod: 1.0,
        }
    }
}

// ---------------------------------------------------------------- button runtime (JoyButton)

#[derive(Default)]
struct Seq {
    pos: usize,
    wait: Option<u64>,
    clear_at: Option<u64>, // Pause: keep keys pressed for the key press time, then release
    active: Vec<Act>,
    done: bool,
    press_ms: u32,
    mouse_mod: Option<u16>,
    tap_end: Option<u64>,
    released_early: bool,
}

impl Seq {
    fn new(pos: usize, press_ms: u32) -> Self {
        Seq { pos, press_ms, ..Default::default() }
    }

    /// Advance through slots until something waits. `held_ms` = how long the button is held (None = released).
    fn step(&mut self, slots: &[Slot], held_ms: Option<u64>, now: u64, cycle: &mut usize, fx: &mut Frame) {
        if self.clear_at.is_some_and(|t| now >= t) {
            self.active.clear();
            self.clear_at = None;
        }
        if self.wait.is_some_and(|t| now < t) || self.done {
            return;
        }
        self.wait = None;
        while let Some(s) = slots.get(self.pos) {
            match s {
                Slot::Out(a) => self.active.push(*a),
                Slot::Pause(ms) => {
                    self.clear_at = Some(now + self.press_ms as u64);
                    self.wait = Some(now + self.press_ms as u64 + *ms as u64);
                    self.pos += 1;
                    return;
                }
                Slot::Delay(ms) => {
                    self.wait = Some(now + *ms as u64);
                    self.pos += 1;
                    return;
                }
                Slot::Hold(ms) => match held_ms {
                    Some(h) if h >= *ms as u64 => self.active.clear(),
                    Some(_) => return, // keep waiting for the hold
                    None => break,
                },
                Slot::Cycle => {
                    *cycle = self.pos + 1;
                    self.done = true;
                    return;
                }
                Slot::Release(_) | Slot::Distance(_) => break,
                Slot::MouseMod(p) => self.mouse_mod = Some(*p),
                Slot::PressTime(ms) => self.press_ms = *ms,
                Slot::SetChange(s) => fx.set_to = Some(*s),
                Slot::Text(t) => fx.text.push(t.clone()),
                Slot::Exec { path, args } => fx.exec.push((path.clone(), args.clone())),
                Slot::Load(p) => fx.load = Some(p.clone()),
            }
            self.pos += 1;
        }
        if self.pos >= slots.len() && slots.contains(&Slot::Cycle) {
            *cycle = 0;
        }
        self.done = true;
    }
}

#[derive(Default)]
struct Accel {
    d0: f64,
    t0: u64,
    multi: f64,
    start: u64,
    dur: f64, // ms
}

#[derive(Default)]
struct BtnRt {
    raw: bool,
    logical: bool,
    eff: bool,
    toggled: bool,
    turbo_on: bool,
    turbo_next: u64,
    press_at: u64,
    last_active: u64,
    cycle: usize,
    seq_start: usize,
    seq: Option<Seq>,
    rel: Option<Seq>,
    ease: Option<u64>,
    accel: Accel,
    wheel_next: [u64; 4],
}

impl BtnRt {
    fn fresh(raw: bool) -> Self {
        BtnRt { raw, ..Default::default() }
    }

    #[allow(clippy::too_many_arguments)]
    fn update(&mut self, e: Elem, c: &Button, p: bool, d: f64, analog: bool, now: u64, press_ms: u32, fx: &mut Frame) {
        let rising = p && !self.raw;
        self.raw = p;
        if c.toggle && rising { self.toggled = !self.toggled }
        let logical = if c.toggle { self.toggled } else { p };
        let (lr, lf) = (logical && !self.logical, !logical && self.logical);
        self.logical = logical;

        if let Some((s, cond)) = c.set {
            let fire = match cond { SetCond::WhileHeld => lr, SetCond::OneWay | SetCond::TwoWay => lf };
            if fire { fx.set_req = Some((e, s, cond)) }
        }

        let dd = if analog { d } else { 1.0 };
        let mut eff = logical;
        if c.turbo && logical {
            let t = c.turbo_interval() as f64;
            let (on, off) = match c.turbo_mode.unwrap_or(TurboMode::Normal) {
                TurboMode::Normal => (t / 2.0, t / 2.0),
                TurboMode::Gradient => (dd * t, (1.0 - dd) * t),
                TurboMode::Pulse => (t / 2.0, if dd > 0.0 { t / dd / 2.0 } else { t }),
            };
            if lr {
                self.turbo_on = true;
                self.turbo_next = now + on as u64;
            } else if now >= self.turbo_next {
                self.turbo_on = off < 1.0 || !self.turbo_on;
                self.turbo_next = now + if self.turbo_on { on } else { off } as u64;
            }
            eff = self.turbo_on;
        }
        let (er, ef) = (eff && !self.eff, !eff && self.eff);
        self.eff = eff;

        let zones = c.slots.iter().any(|s| matches!(s, Slot::Distance(_)));
        if er {
            if c.cycle_reset.is_some_and(|ms| now.saturating_sub(self.last_active) >= ms as u64) || self.cycle >= c.slots.len() {
                self.cycle = 0;
            }
            self.press_at = now;
            self.ease = None;
            self.wheel_next = [0; 4]; // like AntiMicro: a wheel slot scrolls one notch the moment it activates
            if !zones {
                self.seq_start = self.cycle;
                self.seq = Some(Seq::new(self.cycle, press_ms));
            }
        }
        if eff { self.last_active = now }
        let held = eff.then(|| now - self.press_at);

        let mut acts: Vec<Act> = vec![];
        let mut mouse_mod = None;
        if zones && eff {
            // distance zones: the zone whose threshold the element passed is active
            let mut zone = vec![];
            for s in &c.slots {
                match s {
                    Slot::Distance(pct) if dd * 100.0 >= *pct as f64 => zone.clear(),
                    Slot::Distance(_) => break,
                    Slot::Out(a) => zone.push(*a),
                    Slot::MouseMod(p) => mouse_mod = Some(*p),
                    _ => {}
                }
            }
            acts = zone;
        }

        if let Some(seq) = &mut self.seq {
            seq.step(&c.slots, held, now, &mut self.cycle, fx);
            if ef {
                seq.released_early = !seq.done;
                let rel = c.slots.iter().enumerate().skip(self.seq_start).find(|(_, s)| matches!(s, Slot::Release(_)));
                if let Some((i, Slot::Release(ms))) = rel {
                    if now - self.press_at >= *ms as u64 {
                        self.rel = Some(Seq::new(i + 1, seq.press_ms));
                    }
                }
            }
            if !eff && seq.done && seq.wait.is_none() {
                // a macro that finished after release still taps its last keys for the key press time
                if seq.released_early && seq.tap_end.is_none() { seq.tap_end = Some(now + seq.press_ms as u64) }
                if seq.tap_end.is_none_or(|t| now >= t) { self.seq = None }
            }
        }
        if let Some(rel) = &mut self.rel {
            let mut c2 = self.cycle;
            rel.step(&c.slots, Some(u64::MAX), now, &mut c2, fx);
            if rel.done && rel.wait.is_none() && rel.tap_end.is_none() { rel.tap_end = Some(now + rel.press_ms as u64) }
            if rel.tap_end.is_some_and(|t| now >= t) { self.rel = None }
        }
        for s in self.seq.iter().chain(self.rel.iter()) {
            acts.extend_from_slice(&s.active);
            mouse_mod = mouse_mod.or(s.mouse_mod);
        }
        if let Some(m) = mouse_mod { fx.mouse_mod = fx.mouse_mod.min(m as f64 / 100.0) }

        let accel = self.accel(c, d, analog, now);
        for a in acts {
            self.emit(a, c, dd, accel, now, fx);
        }
    }

    // ponytail: delta acceleration measured over 10 ms windows instead of AntiMicro's per-mouse-event history
    fn accel(&mut self, c: &Button, d: f64, analog: bool, now: u64) -> f64 {
        let m = &c.mouse;
        if !m.accel || !analog {
            return 1.0;
        }
        let a = &mut self.accel;
        if now - a.t0 >= 10 {
            let travel = d - a.d0;
            let (mint, maxt) = (m.accel_min as f64 / 100.0, (m.accel_max as f64 / 100.0).max(m.accel_min as f64 / 100.0 + 0.01));
            if travel >= mint {
                let mag = m.accel_mult as f64;
                let minf = (mag * m.accel_start as f64 / 100.0).max(1.0);
                let slope = (mag - minf) / (maxt - mint);
                let lin = (slope * travel.min(maxt) + (minf - slope * mint)).min(mag);
                let x = if mag > minf { (lin - minf) / (mag - minf) } else { 0.0 };
                let shaped = match m.accel_curve {
                    AccelCurve::Linear => x,
                    AccelCurve::EaseOutSine => (x * std::f64::consts::FRAC_PI_2).sin(),
                    AccelCurve::EaseOutQuad => -x * (x - 2.0),
                    AccelCurve::EaseOutCubic => (x - 1.0).powi(3) + 1.0,
                };
                a.multi = (mag - minf) * shaped + minf;
                a.start = now;
                a.dur = (m.accel_dur as f64 * 1000.0 * shaped).max(10.0);
            }
            a.d0 = d;
            a.t0 = now;
        }
        if a.multi > 1.0 {
            let e = (now - a.start) as f64 / a.dur;
            if e < 1.0 {
                return (1.0 - a.multi) * e * e * e + a.multi;
            }
            a.multi = 0.0;
        }
        1.0
    }

    fn emit(&mut self, a: Act, c: &Button, d: f64, accel: f64, now: u64, fx: &mut Frame) {
        let m = &c.mouse;
        match a {
            Act::Key(vk) => fx.keys[vk as usize & 0xFF] = true,
            Act::Mouse(b) => fx.mouse[b.min(5) as usize] = true,
            Act::Pad(o) => fx.pad.push(o, d),
            Act::Wheel(w) => {
                let speed = if w < 2 { m.wheel_y } else { m.wheel_x } as f64 * d;
                if speed > 0.0 && now >= self.wheel_next[w as usize & 3] {
                    fx.wheel[w as usize & 3] += 1;
                    self.wheel_next[w as usize & 3] = now + (1000.0 / speed) as u64;
                }
            }
            Act::Move(dir) => match m.mode {
                MouseMode::Cursor => {
                    let speed = if dir <= 2 { m.speed_y } else { m.speed_x } as f64;
                    fx.moves.push((dir, curve(m, d, &mut self.ease, now) * accel * speed * 20.0));
                }
                MouseMode::Spring => fx.springs.push(Spring { dir, d, w: m.spring_w, h: m.spring_h, rel: m.spring_rel, release: m.spring_release }),
            },
        }
    }
}

/// Direction-change delay (stick delay / d-pad delay).
#[derive(Default)]
struct DirDelay {
    cur: Vec<usize>,
    pending: Option<(Vec<usize>, u64)>,
}

impl DirDelay {
    fn update(&mut self, new: Vec<usize>, delay: u32, now: u64) -> &[usize] {
        if new == self.cur || delay == 0 {
            self.cur = new;
            self.pending = None;
        } else {
            match &self.pending {
                Some((p, t)) if *p == new => if now - t >= delay as u64 { self.cur = new; self.pending = None },
                _ => self.pending = Some((new, now)),
            }
        }
        &self.cur
    }
}

/// Mouse -> virtual stick. Speed is measured in counts per ms over a fixed time window, so the stick
/// deflection is the same at any engine rate (a 1000 Hz mouse read at 2000 Hz used to give every
/// other tick zero movement: weaker and jittery aim).
#[derive(Default)]
struct AimState {
    acc: (f32, f32),
    acc_ms: f32,
    v: (f32, f32), // smoothed counts per ms
}

impl AimState {
    fn step(&mut self, d: (f32, f32), dt_ms: f32, a: &MouseAim) {
        self.acc.0 += d.0;
        self.acc.1 += d.1;
        self.acc_ms += dt_ms;
        // 5 % slack so timer jitter doesn't stretch a window by a whole tick
        if self.acc_ms >= a.window_ms.clamp(0.1, 50.0) * 0.95 {
            let k = 1.0 - a.smooth.clamp(0.0, 0.99).powf(self.acc_ms); // smoothing is defined per ms
            self.v.0 += (self.acc.0 / self.acc_ms - self.v.0) * k;
            self.v.1 += (self.acc.1 / self.acc_ms - self.v.1) * k;
            (self.acc, self.acc_ms) = ((0.0, 0.0), 0.0);
        }
    }

    /// Stick deflection (x, y up), None while the mouse rests.
    fn stick(&self, a: &MouseAim) -> Option<(f64, f64)> {
        let (x, y) = (self.v.0 * a.sens / 10.0, -self.v.1 * a.sens / 10.0);
        let len = (x * x + y * y).sqrt();
        if len <= 0.001 { return None }
        let out = (a.deadzone + (1.0 - a.deadzone) * len).min(1.0) / len;
        Some(((x * out) as f64, (y * out) as f64))
    }
}

// ---------------------------------------------------------------- engine

static DX: AtomicI32 = AtomicI32::new(0);
static DY: AtomicI32 = AtomicI32::new(0);

/// The pad that these held outputs give (diagonals on the circle, `walk` on the left stick): the Keyboard tab
/// previews it even while padmimic is off.
pub fn preview_pad(held: impl Iterator<Item = Out>, walk: f64) -> XGamepad {
    let mut acc = Acc::default();
    for o in held { acc.push(o, 1.0) }
    acc.gamepad(walk)
}

/// What the party pads show right now: slot_scan_thread must never take one for a real controller.
static PARTY_OUT: Mutex<Vec<XGamepad>> = Mutex::new(Vec::new());
/// Party players with a plugged-in pad, for the UI.
pub static PARTY_LIVE: AtomicU8 = AtomicU8::new(0);

/// Party mode: every chosen keyboard drives its own virtual Xbox pad (player 2, 3, 4) while padmimic is on.
/// Keys still reach the focused window too: split-screen games usually only read pads.
fn party_thread(s: Arc<Shared>) {
    let mut pads: Vec<(String, Xbox360Wired<Client>, XGamepad)> = vec![];
    let mut retry_at = Instant::now();
    let mut on_since: Option<Instant> = None;
    loop {
        thread::sleep(Duration::from_millis(4));
        let on = s.on.load(Relaxed);
        if !on { on_since = None } else if on_since.is_none() { on_since = Some(Instant::now()) }
        let want: Vec<String> = { let st = s.settings.lock().unwrap(); if on && st.kb_on { st.party.iter().take(3).cloned().collect() } else { vec![] } };
        pads.retain(|(id, ..)| want.contains(id)); // dropping a target unplugs it
        // the main virtual pad finds its slot as "the next new one": plug party pads only after it settled
        let settled = on_since.is_some_and(|t| t.elapsed() > Duration::from_secs(1)) && !LOOKING.load(Relaxed);
        if settled && Instant::now() >= retry_at {
            let missing: Vec<String> = want.iter().filter(|id| !pads.iter().any(|p| &p.0 == *id)).cloned().collect();
            for id in &missing {
                let pad = Client::connect().ok().and_then(|c| {
                    let mut t = Xbox360Wired::new(c, TargetId::XBOX360_WIRED);
                    (t.plugin().is_ok() && t.wait_ready().is_ok()).then_some(t)
                });
                match pad {
                    Some(t) => pads.push((id.clone(), t, XGamepad::default())),
                    None => { retry_at = Instant::now() + Duration::from_secs(3); break } // ViGEmBus missing / busy
                }
            }
        }
        let list = if pads.is_empty() { vec![] } else { crate::devices::list() };
        let mut outs = Vec::with_capacity(pads.len());
        for (id, t, last) in &mut pads {
            let keys = list.iter().find(|d| d.id() == *id).map(crate::devices::held).unwrap_or_default();
            let mut acc = Acc::default();
            for (vk, o) in PARTY_LAYOUT { if crate::devices::is_held(&keys, vk) { acc.push(o, 1.0) } }
            let g = acc.gamepad(1.0);
            if g != *last && t.update(&g).is_ok() { *last = g } // only on change: no packet churn
            outs.push(g);
        }
        PARTY_LIVE.store(pads.len() as u8, Relaxed);
        *PARTY_OUT.lock().unwrap() = outs;
    }
}

pub fn spawn(s: Arc<Shared>) {
    thread::spawn(raw_mouse_thread);
    thread::spawn(slot_scan_thread);
    crate::devices::start();
    let ps = s.clone();
    thread::spawn(move || party_thread(ps));
    thread::spawn(move || run(s));
}

const NONE: u8 = 255;
/// XInput slot of the controller in use, picked by `slot_scan_thread`.
static SLOT_HINT: AtomicU8 = AtomicU8::new(NONE);
/// Our virtual pad's slot, never a candidate for the real controller.
static VIRT: AtomicU8 = AtomicU8::new(NONE);
/// Virtual pad just plugged in: the next slot that appears is ours.
static LOOKING: AtomicBool = AtomicBool::new(false);
/// Per XInput slot, for the devices list: 0 empty, 1 real pad, 2 our virtual pad, 3 a party pad.
static XSLOTS: [AtomicU8; 4] = [const { AtomicU8::new(0) }; 4];

/// XInput slots (0 empty, 1 real pad, 2 our virtual pad) and the one the engine reads.
pub fn xinput_slots() -> ([u8; 4], Option<usize>) {
    let used = SLOT_HINT.load(Relaxed);
    (XSLOTS.each_ref().map(|s| s.load(Relaxed)), (used != NONE).then_some(used as usize))
}
/// What our virtual pad shows right now; None = not plugged in.
static OUT: Mutex<Option<XGamepad>> = Mutex::new(None);

/// Follows the controller that is actually used: an idle or phantom pad in a lower slot (or our own
/// virtual pad, if it couldn't be identified) must never hide the one in your hands. Own thread,
/// because querying empty XInput slots is slow and would stall the engine.
fn slot_scan_thread() {
    let mut seen = [None::<u32>; 4];
    let mut active = [None::<Instant>; 4];
    let mut party = [false; 4];
    let (mut look_t, mut scanned): (Option<Instant>, bool) = (None, false);
    loop {
        // XInput can take seconds to list a new pad; a real one plugged in at the same moment is unlikely
        if !LOOKING.load(Relaxed) {
            look_t = None;
        } else if look_t.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(15) {
            LOOKING.store(false, Relaxed);
        }
        let (virt, out) = (VIRT.load(Relaxed), *OUT.lock().unwrap());
        let mut best: Option<(u32, Option<Instant>)> = None;
        for i in 0..4u32 {
            let k = i as usize;
            let reading = (i as u8 != virt).then(|| read_xinput(i)).flatten();
            let Some((pkt, g)) = reading else {
                XSLOTS[k].store(if i as u8 == virt { 2 } else { 0 }, Relaxed);
                (seen[k], active[k], party[k]) = (None, None, false);
                continue;
            };
            if seen[k].is_none() && look_t.is_some() && scanned && LOOKING.swap(false, Relaxed) {
                VIRT.store(i as u8, Relaxed); // appeared after our plug-in: ours
                continue;
            }
            // once a slot shows a moving party pad's exact state it stays a party slot until it disappears
            if g != XGamepad::default() && PARTY_OUT.lock().unwrap().contains(&g) { party[k] = true }
            XSLOTS[k].store(if party[k] { 3 } else { 1 }, Relaxed);
            // shows exactly our output: our own pad, unidentified
            let ours = party[k] || out == Some(g);
            if !ours && seen[k].is_some_and(|p| p != pkt) { active[k] = Some(Instant::now()) }
            seen[k] = Some(pkt);
            // with party pads around, an untouched slot may be one of them: only a moved pad is the real one
            let candidate = !ours && (PARTY_LIVE.load(Relaxed) == 0 || active[k].is_some());
            if candidate && best.is_none_or(|b| active[k] > b.1) { best = Some((i, active[k])) }
        }
        SLOT_HINT.store(best.map_or(NONE, |b| b.0 as u8), Relaxed);
        scanned = true; // from now on a slot without a previous reading is a newly plugged one
        thread::sleep(Duration::from_millis(200));
    }
}

fn read_xinput(slot: u32) -> Option<(u32, XGamepad)> {
    let mut st: XINPUT_STATE = unsafe { mem::zeroed() };
    if unsafe { XInputGetState(slot, &mut st) } != 0 {
        return None;
    }
    let g = st.Gamepad;
    Some((st.dwPacketNumber, XGamepad {
        buttons: XButtons(g.wButtons as u16),
        left_trigger: g.bLeftTrigger, right_trigger: g.bRightTrigger,
        thumb_lx: g.sThumbLX, thumb_ly: g.sThumbLY, thumb_rx: g.sThumbRX, thumb_ry: g.sThumbRY,
    }))
}

pub fn pad_down(g: &XGamepad, o: Out) -> bool {
    const T: i16 = 16000;
    match o {
        Out::LT => g.left_trigger > 64, Out::RT => g.right_trigger > 64,
        Out::LUp => g.thumb_ly > T, Out::LDown => g.thumb_ly < -T, Out::LLeft => g.thumb_lx < -T, Out::LRight => g.thumb_lx > T,
        Out::RUp => g.thumb_ry > T, Out::RDown => g.thumb_ry < -T, Out::RLeft => g.thumb_rx < -T, Out::RRight => g.thumb_rx > T,
        o => g.buttons.raw & o.bit() != 0,
    }
}

struct Engine {
    rt: HashMap<Elem, BtnRt>,
    dirs: [DirDelay; 3], // sticks 0/1, d-pad
    set: usize,
    while_held: Option<(Elem, usize)>,
    two_way: Option<(Elem, usize)>,
    keys: [Option<(u64, u64)>; 256], // down since, last repeat
    mouse: [bool; 6],
    px: (f64, f64),
    rem: (f64, f64),
    hist: (VecDeque<f64>, VecDeque<f64>),
    last_flush: u64,
    spring_origin: Option<(i32, i32)>,
    aim: AimState,
    pps: (f32, f32),
}

/// One input sample: (virtual button, pressed, distance 0..1, analog).
type Sample = (Elem, bool, f64, bool);

impl Engine {
    fn new() -> Self {
        Engine {
            rt: HashMap::new(), dirs: Default::default(), set: 0, while_held: None, two_way: None, keys: [None; 256],
            mouse: [false; 6], px: (0.0, 0.0), rem: (0.0, 0.0), hist: Default::default(), last_flush: 0,
            spring_origin: None, aim: AimState::default(), pps: (0.0, 0.0),
        }
    }

    /// Turn the real pad + keyboard into virtual button samples, and add analog passthrough to `acc`.
    fn sample(&mut self, set: &Set, phys: &XGamepad, sony: u8, keys: bool, now: u64, acc: &mut Acc) -> Vec<Sample> {
        let mut v: Vec<Sample> = Vec::with_capacity(48);
        for (i, o) in PAD_BUTTONS.iter().enumerate() {
            v.push((Elem::Btn(i as u8), phys.buttons.raw & o.bit() != 0, 1.0, false));
        }
        for i in 0..2 {
            v.push((Elem::Sony(i), sony & (1 << i) != 0, 1.0, false));
        }
        let dp = dpad_buttons(set.dpad.mode.unwrap_or(DirMode::Standard), phys.buttons.raw);
        let dp = self.dirs[2].update(dp, set.dpad.delay, now);
        for d in 0..8 {
            v.push((Elem::DPad(d as u8), dp.contains(&d), 1.0, false));
        }
        for (si, (x, y)) in [(phys.thumb_lx, phys.thumb_ly), (phys.thumb_rx, phys.thumb_ry)].into_iter().enumerate() {
            let s = &set.sticks[si];
            let e = eval_stick(s, x as i32, y as i32);
            let pressed = stick_buttons(s, &e);
            let pressed = self.dirs[si].update(pressed, s.delay, now);
            for d in 0..8 {
                let dist = match d { UP | DOWN => e.dy, LEFT | RIGHT => e.dx, _ => e.radial };
                v.push((Elem::Stick(si as u8, d as u8), pressed.contains(&d), dist, true));
            }
            let modp = e.safe && if s.mod_inv { e.raw < s.mod_zone as f64 } else { e.raw > s.mod_zone as f64 };
            v.push((Elem::StickMod(si as u8), modp, 1.0, false));
            if let (Some(vs), true) = (s.analog, e.safe && e.raw > 0.0) {
                let st = acc.stick(vs);
                st.0 += x as f64 / e.raw * e.radial;
                st.1 += y as f64 / e.raw * e.radial;
            }
        }
        for (ti, raw) in [phys.left_trigger, phys.right_trigger].into_iter().enumerate() {
            let a = &set.triggers[ti];
            let t = throttle(raw as i32 * 32767 / 255, a.throttle);
            let dist = axis_distance(t, a.dead, a.max);
            v.push((Elem::Trig(ti as u8, true), t > a.dead, dist, true));
            v.push((Elem::Trig(ti as u8, false), t < -a.dead, dist, true));
            if let (Some(vt), true) = (a.analog, t > a.dead) {
                acc.push(if vt == VTrigger::Left { Out::LT } else { Out::RT }, dist);
            }
        }
        for (i, k) in set.keys.iter().enumerate() {
            v.push((Elem::Key(i as u16), keys && winio::down(k.vk), 1.0, false));
        }
        v
    }

    fn switch_set(&mut self, s: usize, samples: &[Sample]) {
        if s == self.set || s > 7 {
            return;
        }
        self.set = s;
        // held buttons must not fire again in the new set
        self.rt = samples.iter().map(|x| (x.0, BtnRt::fresh(x.1))).collect();
    }

    fn release_all(&mut self) {
        for vk in 0..256 {
            if self.keys[vk].take().is_some() { winio::key(vk as u16, true) }
        }
        for b in 1..6 {
            if std::mem::take(&mut self.mouse[b]) { winio::mouse_button(b as u8, true) }
        }
        self.rt.clear();
    }

    /// `dt_ms` = real time since the previous tick (the loop rate is a target, not a guarantee).
    #[allow(clippy::too_many_arguments)]
    fn tick(&mut self, p: &Profile, st: &Settings, phys: &XGamepad, on: bool, now: u64, dt_ms: f64, set_cmd: Option<usize>, live: &mut Live, events: &mut Vec<Event>) -> XGamepad {
        let set = &p.sets[self.set.min(7)];
        // keys mapped to the virtual pad must not also reach the game as keys: getting WASD as key *and*
        // as stick makes games flip between keyboard and controller mode every frame (slow, broken movement)
        // the two halves switch on / off separately: controller mappings, keyboard + mouse mappings
        let (pad_part, kb_part) = (st.pad_on, st.kb_on);
        let idle = XGamepad::default();
        let phys = if pad_part { phys } else { &idle };
        let mut block = [false; 256];
        if on && kb_part {
            for k in set.keys.iter().filter(|k| k.btn.slots.iter().any(|s| matches!(s, Slot::Out(Act::Pad(_))))) {
                block[k.vk as usize & 0xFF] = true;
            }
            if p.walk_vk != 0 { block[p.walk_vk as usize & 0xFF] = true } // walk key is a pad thing too
        }
        for (b, v) in winio::BLOCK.iter().zip(block) { b.store(v, Relaxed) }
        // system-wide hooks delay every key/mouse event on the PC: only while something is blocked,
        // raw mouse only while mouse aim runs (AntiMicro has neither, so idle padmimic must have neither)
        let aim_on = on && kb_part && p.mouse_aim.enabled;
        set_inputs(block.contains(&true), aim_on);
        let mut fx = Frame::default();
        let sony = if pad_part { live.sony.map_or(0, |sample| sample.extra) } else { 0 };
        let samples = self.sample(set, phys, sony, kb_part, now, &mut fx.pad);

        live.active.clear();
        live.active.extend(samples.iter().filter(|s| s.1).map(|s| s.0));

        if !on {
            // drop mouse movement made while off, or it fires as one big stick flick when switched on
            DX.swap(0, Relaxed);
            DY.swap(0, Relaxed);
            self.aim = AimState::default();
            self.release_all();
            if let Some(s) = set_cmd { self.switch_set(s, &samples) }
            return XGamepad::default();
        }

        let mut two_way_back = false;
        for &(e, pressed, d, analog) in &samples {
            let Some(cfg) = set.button(e) else { continue };
            let rt = self.rt.entry(e).or_default();
            if self.two_way.is_some_and(|t| t.0 == e) && rt.raw && !pressed && cfg.set.is_none() {
                two_way_back = true;
            }
            rt.update(e, cfg, pressed, d, analog, now, p.key_press_ms, &mut fx);
        }

        // mouse -> virtual stick (keyboard & mouse players)
        let aim = &p.mouse_aim;
        self.aim.step((DX.swap(0, Relaxed) as f32, DY.swap(0, Relaxed) as f32), dt_ms as f32, aim);
        if let (true, Some((x, y))) = (aim_on, self.aim.stick(aim)) {
            let s = fx.pad.stick(aim.stick);
            s.0 += x;
            s.1 += y;
        }

        self.output(&fx, st, now, dt_ms);
        live.pps = self.pps;
        for t in &fx.text { winio::type_text(t) }
        for (path, args) in &fx.exec {
            let _ = std::process::Command::new(path).args(args.split_whitespace()).spawn();
        }
        if let Some(l) = fx.load.clone() { events.push(Event::LoadProfile(l)) }

        // set changes
        let mut next = set_cmd;
        if let Some(s) = fx.set_to { next = Some(s as usize) }
        if let Some((e, s, cond)) = fx.set_req {
            match cond {
                SetCond::WhileHeld => self.while_held = Some((e, self.set)),
                SetCond::TwoWay => self.two_way = Some((e, self.set)),
                SetCond::OneWay => {}
            }
            next = Some(s as usize);
        }
        if let Some((e, back)) = self.while_held {
            if !samples.iter().any(|x| x.0 == e && x.1) && fx.set_req.is_none() {
                self.while_held = None;
                next = Some(back);
            }
        }
        if two_way_back {
            next = self.two_way.take().map(|t| t.1);
        }
        if let Some(s) = next { self.switch_set(s, &samples) }

        let fx_pad = std::mem::take(&mut fx.pad);
        let walking = kb_part && p.walk_vk != 0 && winio::down(p.walk_vk);
        fx_pad.gamepad(if walking { p.walk.clamp(0.05, 1.0) as f64 } else { 1.0 })
    }

    fn output(&mut self, fx: &Frame, st: &Settings, now: u64, dt_ms: f64) {
        let rep = 1000 / st.key_repeat_rate.max(1) as u64;
        for vk in 1..256 {
            match (fx.keys[vk], self.keys[vk]) {
                (true, None) => { winio::key(vk as u16, false); self.keys[vk] = Some((now, now)) }
                (false, Some(_)) => { winio::key(vk as u16, true); self.keys[vk] = None }
                (true, Some((since, last))) if st.key_repeat && now - since >= st.key_repeat_delay as u64 && now - last >= rep => {
                    winio::key(vk as u16, false);
                    self.keys[vk] = Some((since, now));
                }
                _ => {}
            }
        }
        for b in 1..6 {
            if fx.mouse[b] != self.mouse[b] {
                winio::mouse_button(b as u8, !fx.mouse[b]);
                self.mouse[b] = fx.mouse[b];
            }
        }
        for (dir, n) in fx.wheel.iter().enumerate() {
            for _ in 0..*n { winio::wheel(dir as u8) }
        }

        // cursor mode: integrate px/s over the real tick time, flush every refresh interval (AntiMicro smoothing + remainder)
        let dt = now.saturating_sub(self.last_flush).max(1) as f64;
        for &(dir, v) in &fx.moves {
            let v = v * fx.mouse_mod * dt_ms * 0.001;
            match dir { 1 => self.px.1 -= v, 2 => self.px.1 += v, 3 => self.px.0 -= v, _ => self.px.0 += v }
        }
        if fx.moves.is_empty() {
            self.px = (0.0, 0.0);
            self.rem = (0.0, 0.0);
            self.hist.0.clear();
            self.hist.1.clear();
            self.last_flush = now;
            self.pps = (0.0, 0.0);
        } else if dt >= st.mouse_refresh.max(1) as f64 {
            let n = if st.smoothing { st.history.clamp(1, 100) } else { 1 };
            let mut out = [0i32; 2];
            for (i, (px, rem, hist)) in [(self.px.0, &mut self.rem.0, &mut self.hist.0), (self.px.1, &mut self.rem.1, &mut self.hist.1)].into_iter().enumerate() {
                let mut f = px;
                if (*rem >= 0.0) == (f >= 0.0) { f += *rem }
                hist.push_front(f.clamp(-127.0, 127.0));
                hist.truncate(n);
                let (mut sum, mut wsum, mut w) = (0.0, 0.0, 1.0);
                for h in hist.iter() {
                    sum += h * w;
                    wsum += w;
                    w *= st.weight as f64;
                }
                let avg = sum / wsum;
                let whole = avg.trunc();
                *rem = avg - whole;
                out[i] = whole as i32;
            }
            if out != [0, 0] { winio::mouse_move(out[0], out[1]) }
            self.pps = ((out[0] as f64 * 1000.0 / dt) as f32, (out[1] as f64 * 1000.0 / dt) as f32);
            self.px = (0.0, 0.0);
            self.last_flush = now;
        }

        // spring mode: absolute position around the screen centre (or where it started, if relative)
        if fx.springs.is_empty() {
            if let Some((x, y)) = self.spring_origin.take() { winio::set_cursor(x, y) }
        } else {
            let (sw, sh) = winio::screen();
            let (mut dx, mut dy, mut w, mut h, mut rel, mut release) = (0f64, 0f64, 0, 0, false, 0u8);
            for s in &fx.springs {
                match s.dir { 1 => dy = dy.min(-s.d), 2 => dy = dy.max(s.d), 3 => dx = dx.min(-s.d), _ => dx = dx.max(s.d) }
                (w, h, rel, release) = (w.max(s.w), h.max(s.h), rel || s.rel, release.max(s.release));
            }
            let (w, h) = (if w == 0 { sw } else { w as i32 }, if h == 0 { sh } else { h as i32 });
            let origin = *self.spring_origin.get_or_insert(if rel { winio::cursor() } else { (sw / 2, sh / 2) });
            if dx.abs().max(dy.abs()) * 100.0 >= release as f64 {
                winio::set_cursor(origin.0 + (dx * w as f64 / 2.0) as i32, origin.1 + (dy * h as f64 / 2.0) as i32);
            }
        }
    }
}

/// Plug in the virtual pad and report the result to the UI. Its XInput slot is found by
/// `slot_scan_thread` (the slot that appears right after this). ViGEm's user index can't be used:
/// it counts only wired-style pads, so with a Bluetooth controller it named the real one.
fn connect_pad(s: &Shared, mirrors: bool) -> Option<Xbox360Wired<Client>> {
    VIRT.store(NONE, Relaxed);
    LOOKING.store(true, Relaxed);
    let pad = Client::connect().and_then(|c| {
        let mut t = Xbox360Wired::new(c, TargetId::XBOX360_WIRED);
        t.plugin()?;
        t.wait_ready()?;
        Ok(t)
    });
    match pad {
        Ok(p) => {
            let mut l = s.live.lock().unwrap();
            // the real pad can't be hidden from games without a driver (HidHide): say so instead of silently doubling input
            l.status = if mirrors {
                "Virtual pad connected – ⚠ your real controller is still visible to games (double input). Hide it with HidHide."
            } else {
                "Virtual Xbox 360 controller connected"
            }.into();
            l.vigem = true;
            Some(p)
        }
        Err(e) => {
            LOOKING.store(false, Relaxed);
            let mut l = s.live.lock().unwrap();
            let _ = e; // shown in English UI terms; the raw driver error helps nobody
            (l.status, l.vigem) = (VIGEM_MISSING.into(), false);
            None
        }
    }
}

fn run(s: Arc<Shared>) {
    let timer: HANDLE = unsafe {
        timeBeginPeriod(1);
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        CreateWaitableTimerExW(ptr::null(), ptr::null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS)
    };
    let (mut pad, mut virt): (Option<Xbox360Wired<Client>>, Option<u32>) = (None, None); // virt: from slot_scan_thread
    let mut vigem_t = Instant::now() - Duration::from_secs(10); // first tick may connect at once
    let mut p_seen: Option<Arc<Profile>> = None;
    let (mut want_pad, mut uses_pad, mut mirrors) = (None::<bool>, false, false);

    let t0 = Instant::now();
    let mut eng = Engine::new();
    let (mut toggle_was, mut last) = (false, XGamepad::default());
    let (mut slot, mut last_pkt, mut pkts, mut loops) = (None::<u32>, 0u32, 0u32, 0u32);
    let (mut stat_t, mut scan_t) = (Instant::now(), Instant::now() - Duration::from_secs(1));
    let mut events = vec![];
    let mut next = Instant::now();
    // latency meter state
    let mut prev_tick = Instant::now();
    let mut pad_t: Option<Instant> = None; // when the last new controller report was seen
    let mut virt_pkt = 0u32;
    let mut virt_wait: Option<Instant> = None; // virtual pad update sent, not yet visible in XInput
    let mut last_out_set = false;

    loop {
        let tick_t = Instant::now();
        let dt = tick_t - prev_tick;
        prev_tick = tick_t;
        let dt_ms = (dt.as_secs_f64() * 1000.0).min(50.0); // after sleep/hibernate don't integrate a huge step
        let mut pad_gap = None;

        let p = s.profile.lock().unwrap().clone(); // Arc clones: locks held for nanoseconds
        let st = s.settings.lock().unwrap().clone();

        // virtual pad only while switched on and the profile outputs to it: when off, the game must see
        // nothing but the real devices (an idle extra pad still counts as a second controller)
        if !p_seen.as_ref().is_some_and(|x| Arc::ptr_eq(x, &p)) {
            (uses_pad, mirrors) = (p.uses_pad(), p.mirrors_pad());
            p_seen = Some(p.clone());
        }
        let want = uses_pad && s.on.load(Relaxed);
        if want_pad != Some(want) {
            want_pad = Some(want);
            if !want {
                if pad.take().is_some() { slot = None } // dropping it unplugs the virtual pad
                virt_wait = None;
                LOOKING.store(false, Relaxed);
                VIRT.store(NONE, Relaxed);
                vigem_t = Instant::now() - Duration::from_secs(10); // switching on plugs it in at once
                let mut l = s.live.lock().unwrap();
                l.status = if uses_pad { "Off – virtual pad unplugged" } else { "No virtual pad – the controller goes straight to the game" }.into();
                l.vigem = false;
            }
        }
        // (re)connect, also picks up ViGEmBus installed while running; a failed connect returns at once
        if want && pad.is_none() && vigem_t.elapsed() > Duration::from_secs(2) {
            vigem_t = Instant::now();
            pad = connect_pad(&s, mirrors);
            if pad.is_some() {
                last = XGamepad::default();
                *OUT.lock().unwrap() = Some(last);
                slot = None; // rescan so the new virtual pad is never taken for the real one
            }
        }

        let mut phys = XGamepad::default();
        let v = VIRT.load(Relaxed);
        let found = (v != NONE).then_some(v as u32);
        if found != virt {
            virt = found;
            virt_pkt = virt.and_then(read_xinput).map_or(0, |v| v.0);
        }
        let hint = SLOT_HINT.load(Relaxed);
        let hint = (hint != NONE && Some(hint as u32) != virt).then_some(hint as u32);
        // after a failed read wait for a fresh scan instead of re-reading a gone slot every tick
        if hint != slot && scan_t.elapsed() > Duration::from_millis(600) {
            slot = hint;
        }
        let sony = sony_input::current();
        if let Some(sample) = sony {
            phys = sample.pad;
            if sample.packet != last_pkt {
                pkts += 1;
                last_pkt = sample.packet;
                let t = Instant::now();
                pad_gap = pad_t.map(|p| (t - p).as_secs_f64() * 1e6).filter(|&us| us < 50_000.0);
                pad_t = Some(t);
            }
        } else if let Some(i) = slot {
            match read_xinput(i) {
                Some((pkt, g)) => {
                    if pkt != last_pkt {
                        pkts += 1;
                        last_pkt = pkt;
                        let t = Instant::now();
                        // gaps over 50 ms are the pad resting, not its report rate
                        pad_gap = pad_t.map(|p| (t - p).as_secs_f64() * 1e6).filter(|&us| us < 50_000.0);
                        pad_t = Some(t);
                    }
                    phys = g;
                }
                None => (slot, scan_t) = (None, Instant::now()),
            }
        }

        let t = match st.toggle { Input::Key(k) => winio::down(k), Input::Pad(o) => pad_down(&phys, o) };
        if t && !toggle_was { s.on.fetch_xor(true, Relaxed); }
        toggle_was = t;

        let set_cmd = s.set_cmd.lock().unwrap().take();
        let on = s.on.load(Relaxed);
        let now = t0.elapsed().as_millis() as u64;
        let g = {
            let mut live = s.live.lock().unwrap();
            live.sony = sony;
            let g = eng.tick(&p, &st, &phys, on, now, dt_ms, set_cmd, &mut live, &mut events);
            live.phys = phys;
            live.set = eng.set;
            live.out = g;
            live.lat.tick.add(dt.as_secs_f64() * 1e6);
            if let Some(us) = pad_gap { live.lat.pad.add(us) }
            g
        };
        if !events.is_empty() { s.events.lock().unwrap().append(&mut events) }
        if g != last {
            if let Some(pad) = &mut pad {
                let _ = pad.update(&g);
                virt_wait.get_or_insert_with(Instant::now);
                *OUT.lock().unwrap() = Some(g);
            }
            last = g;
        }
        if pad.is_none() && last_out_set { *OUT.lock().unwrap() = None }
        last_out_set = pad.is_some();
        // read our own virtual pad back through XInput: how long until a game could see the update
        if let (Some(t0), Some(vi)) = (virt_wait, virt) {
            if let Some((pkt, _)) = read_xinput(vi).filter(|v| v.0 != virt_pkt) {
                virt_pkt = pkt;
                virt_wait = None;
                s.live.lock().unwrap().lat.virt.add(t0.elapsed().as_secs_f64() * 1e6);
            } else if t0.elapsed() > Duration::from_millis(100) {
                virt_wait = None; // never showed up (e.g. same state as before): don't count it
            }
        }

        loops += 1;
        if stat_t.elapsed() >= Duration::from_secs(1) {
            s.live.lock().unwrap().stats = Stats { loop_hz: loops, pad_hz: pkts, slot: if sony.is_none() { slot } else { None }, virt_slot: virt };
            (loops, pkts, stat_t) = (0, 0, Instant::now());
        }

        // fixed-rate schedule: wait until the next deadline, so work time doesn't lower the rate
        let period = Duration::from_nanos(1_000_000_000 / st.hz.clamp(125, 8000) as u64);
        next += period;
        let now_i = Instant::now();
        if next <= now_i {
            if now_i - next > period * 4 { next = now_i } // fell far behind (e.g. system sleep): resync
        } else {
            let wait = (next - now_i).as_nanos() as i64 / 100; // 100 ns units
            if timer.is_null() {
                thread::sleep(next - now_i);
            } else {
                unsafe {
                    SetWaitableTimer(timer, &-wait, 0, None, ptr::null(), 0);
                    WaitForSingleObject(timer, INFINITE);
                }
            }
        }
    }
}

/// Message window of the input thread; null until it exists.
static INPUT_HWND: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(ptr::null_mut());
/// Last (hooks, raw mouse) state handed to the input thread, bit 0 / bit 1; 4 = nothing sent yet.
static INPUTS_SENT: AtomicU8 = AtomicU8::new(4);
const WM_SET_INPUTS: u32 = WM_APP + 1;

/// Ask the input thread to (un)install the low-level hooks and the raw mouse sink. Cheap when unchanged.
fn set_inputs(hooks: bool, raw: bool) {
    let want = hooks as u8 | (raw as u8) << 1;
    let hwnd = INPUT_HWND.load(Relaxed);
    if INPUTS_SENT.load(Relaxed) != want && !hwnd.is_null() && unsafe { PostMessageW(hwnd, WM_SET_INPUTS, want as usize, 0) } != 0 {
        INPUTS_SENT.store(want, Relaxed);
    }
}

/// Input thread: owns the low-level hooks (to swallow keys mapped to the pad) and the raw mouse sink
/// (mouse deltas for aim, also when the game locks the cursor). Both only exist while needed.
fn raw_mouse_thread() {
    unsafe {
        let class = windows_sys::w!("padmimic_raw");
        let hinst = GetModuleHandleW(ptr::null());
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst, lpszClassName: class, ..mem::zeroed() };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(0, class, class, 0, 0, 0, 0, 0, HWND_MESSAGE, ptr::null_mut(), hinst, ptr::null());
        // while hooked, every key and click on the system waits for this thread: keep it first in line
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        INPUT_HWND.store(hwnd, Relaxed);
        // pads and keyboards, read only to see which device is in use (nothing is swallowed)
        let rids = crate::devices::USAGES.map(|u| RAWINPUTDEVICE { usUsagePage: 1, usUsage: u, dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY, hwndTarget: hwnd });
        RegisterRawInputDevices(rids.as_ptr(), rids.len() as u32, mem::size_of::<RAWINPUTDEVICE>() as u32);
        let mut msg = mem::zeroed();
        while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
            DispatchMessageW(&msg);
        }
    }
}

/// Runs on the input thread (hooks must be removed by the thread that set them).
unsafe fn apply_inputs(hwnd: HWND, want: usize) {
    use std::cell::Cell;
    thread_local! {
        static HOOKS: Cell<Option<(HHOOK, HHOOK)>> = const { Cell::new(None) };
        static RAW: Cell<bool> = const { Cell::new(false) };
    }
    unsafe {
        match (want & 1 != 0, HOOKS.get()) {
            (true, None) => {
                let hinst = GetModuleHandleW(ptr::null());
                HOOKS.set(Some((SetWindowsHookExW(WH_KEYBOARD_LL, Some(winio::kb_hook), hinst, 0), SetWindowsHookExW(WH_MOUSE_LL, Some(winio::mouse_hook), hinst, 0))));
            }
            (false, Some((k, m))) => {
                HOOKS.set(None);
                UnhookWindowsHookEx(k);
                UnhookWindowsHookEx(m);
                for d in &winio::DOWN { d.store(false, Relaxed) } // unseen while unhooked: don't keep stale presses
            }
            _ => {}
        }
        let raw = want & 2 != 0;
        if raw != RAW.get() {
            RAW.set(raw);
            let (flags, target) = if raw { (RIDEV_INPUTSINK, hwnd) } else { (RIDEV_REMOVE, ptr::null_mut()) };
            let rid = RAWINPUTDEVICE { usUsagePage: 1, usUsage: 2, dwFlags: flags, hwndTarget: target };
            RegisterRawInputDevices(&rid, 1, mem::size_of::<RAWINPUTDEVICE>() as u32);
        }
    }
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_SET_INPUTS {
            apply_inputs(h, w);
            return 0;
        }
        if msg == WM_INPUT_DEVICE_CHANGE && w as u32 == GIDC_REMOVAL {
            sony_input::removed(l as usize);
            crate::devices::removed(l as isize);
        }
        if msg == WM_INPUT {
            // header first: pad (HID) reports are bigger than RAWINPUT and only their device is needed
            let mut hdr: RAWINPUTHEADER = mem::zeroed();
            let mut hsize = mem::size_of::<RAWINPUTHEADER>() as u32;
            if GetRawInputData(l as HRAWINPUT, RID_HEADER, &mut hdr as *mut _ as *mut _, &mut hsize, hsize) != u32::MAX && hdr.dwType != RIM_TYPEMOUSE {
                crate::devices::touch(hdr.hDevice);
                if hdr.dwType == RIM_TYPEKEYBOARD {
                    let mut ri: RAWINPUT = mem::zeroed();
                    let mut size = mem::size_of::<RAWINPUT>() as u32;
                    if GetRawInputData(l as HRAWINPUT, RID_INPUT, &mut ri as *mut _ as *mut _, &mut size, hsize) != u32::MAX {
                        let k = ri.data.keyboard;
                        if k.VKey != 0xFF { crate::devices::key(hdr.hDevice, k.VKey, k.Flags as u32 & RI_KEY_BREAK == 0) }
                    }
                } else if hdr.dwType == RIM_TYPEHID {
                    sony_input::raw_input(l as HRAWINPUT);
                }
                return DefWindowProcW(h, msg, w, l);
            }
            let mut ri: RAWINPUT = mem::zeroed();
            let mut size = mem::size_of::<RAWINPUT>() as u32;
            let got = GetRawInputData(l as HRAWINPUT, RID_INPUT, &mut ri as *mut _ as *mut _, &mut size, mem::size_of::<RAWINPUTHEADER>() as u32);
            // injected moves (our own spring/cursor output) must not feed back into mouse aim
            if got != u32::MAX && ri.header.dwType == RIM_TYPEMOUSE && !ri.header.hDevice.is_null() && ri.data.mouse.usFlags & MOUSE_MOVE_ABSOLUTE == 0 {
                DX.fetch_add(ri.data.mouse.lLastX, Relaxed);
                DY.fetch_add(ri.data.mouse.lLastY, Relaxed);
            }
        }
        DefWindowProcW(h, msg, w, l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(b: &Button, presses: &[(u64, bool)], until: u64) -> Vec<(u64, Vec<u16>)> {
        let mut rt = BtnRt::default();
        let mut out = vec![];
        let mut p = false;
        for t in 0..=until {
            if let Some(&(_, v)) = presses.iter().find(|x| x.0 == t) { p = v }
            let mut fx = Frame::default();
            rt.update(Elem::Btn(0), b, p, 1.0, false, t, 10, &mut fx);
            let keys: Vec<u16> = (0..256).filter(|&k| fx.keys[k]).map(|k| k as u16).collect();
            if out.last().is_none_or(|l: &(u64, Vec<u16>)| l.1 != keys) { out.push((t, keys)) }
        }
        out
    }
    const A: u16 = b'A' as u16;
    const B: u16 = b'B' as u16;

    #[test]
    fn simple_and_toggle() {
        let b = Button::out(Act::Key(A));
        assert_eq!(run(&b, &[(1, true), (5, false)], 8), vec![(0, vec![]), (1, vec![A]), (5, vec![])]);
        let t = Button { toggle: true, ..b };
        assert_eq!(run(&t, &[(1, true), (2, false), (5, true), (6, false)], 8), vec![(0, vec![]), (1, vec![A]), (5, vec![])]);
    }

    #[test]
    fn turbo_normal() {
        let b = Button { turbo: true, turbo_ms: 10, ..Button::out(Act::Key(A)) };
        let r = run(&b, &[(0, true), (30, false)], 40);
        assert_eq!(r[..4], [(0, vec![A]), (5, vec![]), (10, vec![A]), (15, vec![])]);
        assert_eq!(r.last().unwrap().1, Vec::<u16>::new());
    }

    #[test]
    fn pause_macro_runs_after_release() {
        // A, pause 20 ms, B — tapped for 1 ms still plays fully (keys held for the 10 ms press time)
        let b = Button::with(vec![Slot::Out(Act::Key(A)), Slot::Pause(20), Slot::Out(Act::Key(B))]);
        assert_eq!(run(&b, &[(1, true), (2, false)], 60), vec![(0, vec![]), (1, vec![A]), (11, vec![]), (31, vec![B]), (41, vec![])]);
    }

    #[test]
    fn cycle_and_hold_and_release() {
        let c = Button::with(vec![Slot::Out(Act::Key(A)), Slot::Cycle, Slot::Out(Act::Key(B))]);
        assert_eq!(run(&c, &[(1, true), (3, false), (5, true), (7, false)], 9), vec![(0, vec![]), (1, vec![A]), (3, vec![]), (5, vec![B]), (7, vec![])]);
        let h = Button::with(vec![Slot::Out(Act::Key(A)), Slot::Hold(10), Slot::Out(Act::Key(B))]);
        assert_eq!(run(&h, &[(1, true), (20, false)], 22), vec![(0, vec![]), (1, vec![A]), (11, vec![B]), (20, vec![])]);
        let r = Button::with(vec![Slot::Release(0), Slot::Out(Act::Key(B))]);
        assert_eq!(run(&r, &[(1, true), (4, false)], 20), vec![(0, vec![]), (4, vec![B]), (14, vec![])]);
    }

    #[test]
    fn stick_math() {
        let s = Stick::default();
        let e = eval_stick(&s, 0, 32000);
        assert!(e.safe && e.bearing.abs() < 0.01 && (e.dy - 1.0).abs() < 1e-9 && e.dx == 0.0);
        assert_eq!(stick_buttons(&s, &e), vec![UP]);
        let diag = eval_stick(&s, 20000, -20000); // right-down
        assert!((diag.bearing - 135.0).abs() < 0.01);
        assert_eq!(stick_buttons(&s, &diag), vec![RIGHT, DOWN]);
        assert_eq!(stick_buttons(&Stick { mode: DirMode::EightWay, ..s.clone() }, &diag), vec![3]);
        assert_eq!(stick_buttons(&Stick { mode: DirMode::FourCardinal, ..s.clone() }, &diag), vec![DOWN]); // 135° -> down zone
        assert!(!eval_stick(&s, 3000, 3000).safe);
        assert_eq!(dir8(80.0, 45), RIGHT);
        assert_eq!(dir8(30.0, 45), 1);
    }

    #[test]
    fn dpad_and_axis() {
        let ur = XButtons::UP | XButtons::RIGHT;
        assert_eq!(dpad_buttons(DirMode::Standard, ur), vec![UP, RIGHT]);
        assert_eq!(dpad_buttons(DirMode::EightWay, ur), vec![1]);
        assert_eq!(dpad_buttons(DirMode::FourCardinal, ur), vec![UP]);
        assert_eq!(dpad_buttons(DirMode::FourDiagonal, XButtons::UP), Vec::<usize>::new());
        assert_eq!(throttle(-32767, 1), 0);
        assert_eq!(throttle(0, 1), 16383);
        assert_eq!(throttle(32767, -1), 0);
        assert_eq!(throttle(-5, 2), 5);
        assert!((axis_distance(19000, 6000, 32000) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn aim_same_at_every_engine_rate() {
        // 1000 Hz mouse, 5 counts per report, read by the engine at different rates
        let a = MouseAim::default();
        for hz in [1000u32, 2000, 4000, 8000] {
            let (mut st, mut lo, mut hi) = (AimState::default(), f64::MAX, 0f64);
            let per = hz / 1000;
            for t in 0..hz * 2 {
                st.step((if t % per == 0 { 5.0 } else { 0.0 }, 0.0), 1000.0 / hz as f32, &a);
                if t > hz {
                    let x = st.stick(&a).unwrap().0;
                    (lo, hi) = (lo.min(x), hi.max(x));
                }
            }
            assert!((lo - 0.6).abs() < 1e-3 && (hi - 0.6).abs() < 1e-3, "{hz} Hz: {lo}..{hi}");
        }
        // resting mouse -> no stick
        let mut st = AimState::default();
        for _ in 0..100 { st.step((0.0, 0.0), 1.0, &a) }
        assert!(st.stick(&a).is_none());
    }

    #[test]
    fn wheel_turbo_like_antimicro() {
        // held 1 s: plain wheel follows its speed (20/s), turbo 20 ms gives one notch per cycle (50/s)
        let notches = |b: &Button| {
            let mut rt = BtnRt::default();
            (0..1000u64).map(|t| {
                let mut fx = Frame::default();
                rt.update(Elem::DPad(2), b, true, 1.0, false, t, 10, &mut fx);
                fx.wheel[0]
            }).sum::<u32>()
        };
        let plain = Button::out(Act::Wheel(0));
        assert_eq!(notches(&plain), 20);
        assert_eq!(notches(&Button { turbo: true, turbo_ms: 20, ..plain }), 50);
    }

    #[test]
    fn keyboard_stick_circle_walk_and_socd() {
        let len = |g: XGamepad| ((g.thumb_lx as f64).powi(2) + (g.thumb_ly as f64).powi(2)).sqrt() / 32767.0;
        let stick = |outs: &[Out], walk: f64| { let mut a = Acc::default(); for &o in outs { a.push(o, 1.0) } a.gamepad(walk) };
        assert!((len(stick(&[Out::LUp, Out::LRight], 1.0)) - 1.0).abs() < 0.01, "diagonal on the circle");
        assert!((len(stick(&[Out::LUp, Out::LRight], 0.5)) - 0.5).abs() < 0.01, "walking diagonal = walking straight");
        assert!((len(stick(&[Out::LUp], 0.5)) - 0.5).abs() < 0.01);
        assert_eq!(stick(&[Out::LLeft, Out::LRight], 1.0).thumb_lx, 0, "A+D together = neutral");
    }

    /// Tests that tick the engine share winio::BLOCK: run them one at a time.
    static BLOCK_TESTS: Mutex<()> = Mutex::new(());

    #[test]
    fn controller_and_keyboard_switch_separately() {
        let _one = BLOCK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut p = Profile::default();
        p.sets[0].keys.push(KeyBind { vk: b'K' as u16, btn: Button::out(Act::Pad(Out::LLeft)) });
        p.sets[0].buttons[0] = Button::out(Act::Pad(Out::B)); // real A -> virtual B
        let pad_a = XGamepad { buttons: XButtons(XButtons::A), ..Default::default() };
        let (mut live, mut ev) = (Live::default(), vec![]);
        let run = |st: &Settings, live: &mut Live, ev: &mut Vec<Event>| Engine::new().tick(&p, st, &pad_a, true, 0, 1.0, None, live, ev);

        let both = Settings::default();
        assert!(run(&both, &mut live, &mut ev).buttons.raw & XButtons::B != 0, "controller part maps A -> B");
        assert!(winio::BLOCK[b'K' as usize].load(Relaxed), "keyboard part swallows mapped keys");

        let kb_only = Settings { pad_on: false, ..Settings::default() };
        assert_eq!(run(&kb_only, &mut live, &mut ev).buttons.raw & XButtons::B, 0, "controller part off: real pad ignored");
        assert!(winio::BLOCK[b'K' as usize].load(Relaxed), "keyboard part still on");

        let pad_only = Settings { kb_on: false, ..Settings::default() };
        assert!(run(&pad_only, &mut live, &mut ev).buttons.raw & XButtons::B != 0);
        assert!(!winio::BLOCK[b'K' as usize].load(Relaxed), "keyboard part off: keys reach the game untouched");
    }

    #[test]
    fn profile_uses_pad() {
        let mut p = Profile::default();
        assert!(!p.uses_pad(), "default profile must leave the real controller alone");
        p.sets[0].dpad.dirs[2] = Button::out(Act::Wheel(0));
        assert!(!p.uses_pad());
        p.sets[3].keys.push(KeyBind { vk: 0x20, btn: Button::out(Act::Pad(Out::A)) });
        assert!(p.uses_pad());
        assert!(!p.mirrors_pad(), "keyboard -> pad doesn't double the real controller");
        p.sets[1].sticks[0].analog = Some(VStick::Left);
        assert!(p.mirrors_pad());
    }

    #[test]
    fn pad_keys_blocked_only_while_on() {
        let _one = BLOCK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut p = Profile::default();
        p.sets[0].keys.push(KeyBind { vk: b'A' as u16, btn: Button::out(Act::Pad(Out::LLeft)) });
        p.sets[0].keys.push(KeyBind { vk: b'Q' as u16, btn: Button::out(Act::Key(b'E' as u16)) });
        let (mut e, st, mut live, mut ev) = (Engine::new(), Settings::default(), Live::default(), vec![]);
        e.tick(&p, &st, &XGamepad::default(), true, 0, 1.0, None, &mut live, &mut ev);
        assert!(winio::BLOCK[b'A' as usize].load(Relaxed), "A goes to the game only as stick");
        assert!(!winio::BLOCK[b'Q' as usize].load(Relaxed), "key -> key stays a normal key");
        e.tick(&p, &st, &XGamepad::default(), false, 1, 1.0, None, &mut live, &mut ev);
        assert!(!winio::BLOCK[b'A' as usize].load(Relaxed), "off = keyboard untouched");
    }

    #[test]
    fn hist() {
        let mut h = Hist::default();
        for us in [100.0, 200.0, 300.0, 4000.0] { h.add(us) }
        assert_eq!(h.n, 4);
        assert!((h.avg() - 1150.0).abs() < 1e-9 && h.max == 4000.0);
        assert_eq!(h.pct(0.5), 250.0); // 2nd sample sits in bucket 200..250
        assert_eq!(h.pct(0.99), 4050.0);
        h.add(1e9); // overflow lands in the last bucket instead of panicking
        assert_eq!(h.pct(1.0), 50_000.0);
    }

    #[test]
    fn curves() {
        let mut e = None;
        let c = |k| MouseCfg { curve: k, ..MouseCfg::default() };
        assert!((curve(&c(Curve::EnhancedPrecision), 1.0, &mut e, 0) - 1.0).abs() < 1e-9);
        assert!((curve(&c(Curve::EnhancedPrecision), 0.4, &mut e, 0) - 0.148).abs() < 1e-9);
        assert!((curve(&c(Curve::Quadratic), 0.5, &mut e, 0) - 0.25).abs() < 1e-9);
        // easing ramps from 1.0x to 1.5x of the high end over the easing duration
        let ec = c(Curve::EasingQuadratic);
        let start = curve(&ec, 1.0, &mut e, 0);
        let end = curve(&ec, 1.0, &mut e, 1000);
        assert!((start - 0.834665).abs() < 1e-6 && (end - (1.5 * 1.33067 - 0.496005)).abs() < 1e-6);
    }
}
