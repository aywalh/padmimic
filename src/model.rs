//! Profile / settings data. Mirrors AntiMicroX: every controller element owns virtual buttons,
//! every virtual button owns a list of slots, 8 sets per profile.

use serde::{Deserialize, Serialize};

/// Virtual Xbox 360 output (buttons, triggers, stick directions).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Out {
    A, B, X, Y, LB, RB, LT, RT, Back, Start, Guide, LS, RS,
    Up, Down, Left, Right,
    LUp, LDown, LLeft, LRight,
    RUp, RDown, RLeft, RRight,
}

impl Out {
    pub const ALL: [Out; 25] = [
        Out::A, Out::B, Out::X, Out::Y, Out::LB, Out::RB, Out::LT, Out::RT, Out::Back, Out::Start,
        Out::Guide, Out::LS, Out::RS, Out::Up, Out::Down, Out::Left, Out::Right, Out::LUp,
        Out::LDown, Out::LLeft, Out::LRight, Out::RUp, Out::RDown, Out::RLeft, Out::RRight,
    ];

    pub fn bit(self) -> u16 {
        use vigem_client::XButtons as X;
        match self {
            Out::A => X::A, Out::B => X::B, Out::X => X::X, Out::Y => X::Y, Out::LB => X::LB, Out::RB => X::RB,
            Out::Back => X::BACK, Out::Start => X::START, Out::Guide => X::GUIDE, Out::LS => X::LTHUMB,
            Out::RS => X::RTHUMB, Out::Up => X::UP, Out::Down => X::DOWN, Out::Left => X::LEFT, Out::Right => X::RIGHT,
            _ => 0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Out::A => "A", Out::B => "B", Out::X => "X", Out::Y => "Y", Out::LB => "LB", Out::RB => "RB",
            Out::LT => "LT", Out::RT => "RT", Out::Back => "Back", Out::Start => "Start", Out::Guide => "Guide",
            Out::LS => "L3", Out::RS => "R3",
            Out::Up => "D-Pad ↑", Out::Down => "D-Pad ↓", Out::Left => "D-Pad ←", Out::Right => "D-Pad →",
            Out::LUp => "L-Stick ↑", Out::LDown => "L-Stick ↓", Out::LLeft => "L-Stick ←", Out::LRight => "L-Stick →",
            Out::RUp => "R-Stick ↑", Out::RDown => "R-Stick ↓", Out::RLeft => "R-Stick ←", Out::RRight => "R-Stick →",
        }
    }
}

/// What a slot outputs while active.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Act {
    Key(u16),  // Windows virtual-key code
    Mouse(u8), // 1 L, 2 R, 3 M, 4 X1, 5 X2
    Wheel(u8), // 0 up, 1 down, 2 left, 3 right
    Move(u8),  // 1 up, 2 down, 3 left, 4 right (AntiMicro numbering)
    Pad(Out),
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum Slot {
    Out(Act),
    Pause(u32),     // ms: release everything, wait
    Hold(u32),      // ms: slots after only run if held that long
    Cycle,          // next press continues after this slot
    Distance(u8),   // %: slots after only active past this distance
    Release(u32),   // ms: slots after run on release (if held that long)
    MouseMod(u16),  // %: scales all mouse speeds while active
    PressTime(u32), // ms: how long keys after this are held in taps
    Delay(u32),     // ms: wait, keep active slots
    SetChange(u8),  // 0..7
    Text(String),
    Exec { path: String, args: String },
    Load(String), // profile path
}

impl Slot {
    pub const TYPES: [&'static str; 13] = [
        "Key / mouse / pad", "Cycle", "Delay", "Distance", "Execute", "Hold", "Load profile",
        "Mouse mod", "Pause", "Press time", "Release", "Set change", "Text entry",
    ];
    pub fn type_index(&self) -> usize {
        match self {
            Slot::Out(_) => 0, Slot::Cycle => 1, Slot::Delay(_) => 2, Slot::Distance(_) => 3, Slot::Exec { .. } => 4,
            Slot::Hold(_) => 5, Slot::Load(_) => 6, Slot::MouseMod(_) => 7, Slot::Pause(_) => 8, Slot::PressTime(_) => 9,
            Slot::Release(_) => 10, Slot::SetChange(_) => 11, Slot::Text(_) => 12,
        }
    }
    pub fn from_type(i: usize) -> Slot {
        match i {
            1 => Slot::Cycle, 2 => Slot::Delay(100), 3 => Slot::Distance(50),
            4 => Slot::Exec { path: String::new(), args: String::new() }, 5 => Slot::Hold(500),
            6 => Slot::Load(String::new()), 7 => Slot::MouseMod(50), 8 => Slot::Pause(100), 9 => Slot::PressTime(100),
            10 => Slot::Release(0), 11 => Slot::SetChange(0), 12 => Slot::Text(String::new()),
            _ => Slot::Out(Act::Key(0x20)),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum SetCond { OneWay, TwoWay, WhileHeld }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum TurboMode { Normal, Gradient, Pulse }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum MouseMode { Cursor, Spring }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Curve { EnhancedPrecision, Linear, Quadratic, Cubic, QuadraticExtreme, Power, EasingQuadratic, EasingCubic }

impl Curve {
    pub const ALL: [(Curve, &'static str); 8] = [
        (Curve::EnhancedPrecision, "Enhanced precision"), (Curve::Linear, "Linear"), (Curve::Quadratic, "Quadratic"),
        (Curve::Cubic, "Cubic"), (Curve::QuadraticExtreme, "Quadratic extreme"), (Curve::Power, "Power function"),
        (Curve::EasingQuadratic, "Easing quadratic"), (Curve::EasingCubic, "Easing cubic"),
    ];
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum AccelCurve { Linear, EaseOutSine, EaseOutQuad, EaseOutCubic }

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct MouseCfg {
    pub mode: MouseMode,
    pub curve: Curve,
    pub speed_x: u16, // 1 = 20 px/s
    pub speed_y: u16,
    pub sens: f32,   // power curve exponent
    pub easing: f32, // s
    pub accel: bool, // delta acceleration
    pub accel_mult: f32,
    pub accel_start: f32, // %
    pub accel_min: f32,   // %
    pub accel_max: f32,   // %
    pub accel_dur: f32,   // s
    pub accel_curve: AccelCurve,
    pub spring_w: u32, // 0 = full screen
    pub spring_h: u32,
    pub spring_release: u8, // %
    pub spring_rel: bool,
    pub wheel_x: u16, // notches/s
    pub wheel_y: u16,
}

impl Default for MouseCfg {
    fn default() -> Self {
        MouseCfg {
            mode: MouseMode::Cursor, curve: Curve::EnhancedPrecision, speed_x: 50, speed_y: 50, sens: 1.0, easing: 0.5,
            accel: false, accel_mult: 2.0, accel_start: 0.0, accel_min: 10.0, accel_max: 100.0, accel_dur: 0.1,
            accel_curve: AccelCurve::Linear, spring_w: 0, spring_h: 0, spring_release: 0, spring_rel: false,
            wheel_x: 20, wheel_y: 20,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Button {
    pub name: String,
    pub action: String,
    pub slots: Vec<Slot>,
    pub toggle: bool,
    pub turbo: bool,
    pub turbo_mode: Option<TurboMode>, // None = Normal (keeps JSON small)
    pub turbo_ms: u32,                 // 0 = AntiMicro default 100 ms
    pub set: Option<(u8, SetCond)>,
    pub cycle_reset: Option<u32>, // ms
    pub mouse: MouseCfg,
}

impl Button {
    pub fn with(slots: Vec<Slot>) -> Self {
        Button { slots, ..Default::default() }
    }
    pub fn out(a: Act) -> Self {
        Self::with(vec![Slot::Out(a)])
    }
    pub fn turbo_interval(&self) -> u32 {
        if self.turbo_ms == 0 { 100 } else { self.turbo_ms }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum DirMode { Standard, EightWay, FourCardinal, FourDiagonal }

impl DirMode {
    pub const ALL: [(DirMode, &'static str); 4] = [
        (DirMode::Standard, "Standard"), (DirMode::EightWay, "8-way"),
        (DirMode::FourCardinal, "4-way (axes)"), (DirMode::FourDiagonal, "4-way (diagonal)"),
    ];
}

/// Direction order used for stick and d-pad buttons.
pub const DIRS: [&str; 8] = ["↑", "↗", "→", "↘", "↓", "↙", "←", "↖"];
pub const UP: usize = 0;
pub const RIGHT: usize = 2;
pub const DOWN: usize = 4;
pub const LEFT: usize = 6;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum VStick { Left, Right }

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Stick {
    pub name: String,
    pub dead: i32,
    pub max: i32,
    pub mode: DirMode,
    pub diag: u8,   // diagonal range in degrees
    pub square: u8, // square stick %
    pub delay: u32, // ms
    pub mod_zone: i32,
    pub mod_inv: bool,
    pub modifier: Button,
    pub dirs: [Button; 8],
    pub analog: Option<VStick>, // forward to a virtual stick (analog)
}

impl Default for Stick {
    fn default() -> Self {
        Stick {
            name: String::new(), dead: 8000, max: 32000, mode: DirMode::Standard, diag: 45, square: 0, delay: 0,
            mod_zone: 8000, mod_inv: false, modifier: Button::default(), dirs: Default::default(), analog: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct DPad {
    pub name: String,
    pub mode: Option<DirMode>, // None = Standard
    pub delay: u32,
    pub dirs: [Button; 8],
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum VTrigger { Left, Right }

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Axis {
    pub name: String,
    pub dead: i32,
    pub max: i32,
    pub throttle: i8, // -2 neg half, -1 neg, 0 normal, 1 pos, 2 pos half
    pub p: Button,
    pub n: Button,
    pub analog: Option<VTrigger>,
}

impl Default for Axis {
    fn default() -> Self {
        Axis { name: String::new(), dead: 6000, max: 32000, throttle: 0, p: Button::default(), n: Button::default(), analog: None }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct KeyBind {
    pub vk: u16,
    pub btn: Button,
}

/// Real controller buttons, in this order.
pub const PAD_BUTTONS: [Out; 11] = [Out::A, Out::B, Out::X, Out::Y, Out::LB, Out::RB, Out::Back, Out::Start, Out::Guide, Out::LS, Out::RS];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Set {
    pub buttons: [Button; 11],
    pub dpad: DPad,
    pub sticks: [Stick; 2],
    pub triggers: [Axis; 2],
    pub keys: Vec<KeyBind>,
}

impl Default for Set {
    /// Empty set: nothing mapped.
    fn default() -> Self {
        Set { buttons: Default::default(), dpad: DPad::default(), sticks: Default::default(), triggers: Default::default(), keys: vec![] }
    }
}

impl Set {
    fn all_buttons(&self) -> impl Iterator<Item = &Button> {
        self.buttons.iter()
            .chain(&self.dpad.dirs)
            .chain(self.sticks.iter().flat_map(|s| s.dirs.iter().chain([&s.modifier])))
            .chain(self.triggers.iter().flat_map(|t| [&t.p, &t.n]))
            .chain(self.keys.iter().map(|k| &k.btn))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct MouseAim {
    pub enabled: bool,
    pub stick: VStick,
    pub sens: f32,
    pub smooth: f32,
    pub deadzone: f32,
    /// ms of mouse movement averaged into one speed sample; ≈ 1000 / mouse polling Hz
    pub window_ms: f32,
}

impl Default for MouseAim {
    fn default() -> Self {
        MouseAim { enabled: false, stick: VStick::Right, sens: 1.0, smooth: 0.6, deadzone: 0.2, window_ms: 1.0 }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub key_press_ms: u32,
    pub set_names: [String; 8],
    pub sets: [Set; 8],
    pub mouse_aim: MouseAim,
}

impl Default for Profile {
    /// Like AntiMicro: nothing mapped, the real controller reaches the game untouched.
    fn default() -> Self {
        Profile { name: String::new(), key_press_ms: 100, set_names: Default::default(), sets: Default::default(), mouse_aim: MouseAim::default() }
    }
}

impl Profile {
    /// The virtual pad is only plugged in when something outputs to it; otherwise games see just
    /// the real controller (no double input, nothing between pad and game).
    pub fn uses_pad(&self) -> bool {
        self.mouse_aim.enabled
            || self.sets.iter().any(|s| {
                s.sticks.iter().any(|x| x.analog.is_some())
                    || s.triggers.iter().any(|t| t.analog.is_some())
                    || s.all_buttons().any(|b| b.slots.iter().any(|x| matches!(x, Slot::Out(Act::Pad(_)))))
            })
    }
    /// Real controller input forwarded to the virtual pad: the game then sees the real pad too.
    pub fn mirrors_pad(&self) -> bool {
        let pad = |b: &Button| b.slots.iter().any(|x| matches!(x, Slot::Out(Act::Pad(_))));
        self.sets.iter().any(|s| {
            s.sticks.iter().any(|x| x.analog.is_some() || x.dirs.iter().any(pad))
                || s.triggers.iter().any(|t| t.analog.is_some() || pad(&t.p) || pad(&t.n))
                || s.buttons.iter().chain(&s.dpad.dirs).any(pad)
        })
    }
    pub fn load(path: &str) -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap())
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct AutoProfile {
    pub exe: String, // e.g. "game.exe"
    pub profile: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Input {
    Key(u16),
    Pad(Out),
}

/// Global program settings (AntiMicroX "Edit Settings").
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub profile_dir: String,
    pub recent: Vec<String>,
    pub recent_count: usize,
    pub last_profile: String,
    pub auto_load_last: bool,
    pub startup: bool,
    pub hide_empty: bool,
    pub numpad: bool,
    pub hz: u32, // gamepad poll / engine rate
    pub key_repeat: bool,
    pub key_repeat_delay: u32, // ms
    pub key_repeat_rate: u32,  // per second
    pub disable_epp: bool,     // Windows "Enhance pointer precision"
    pub smoothing: bool,
    pub history: usize,
    pub weight: f32,
    pub mouse_refresh: u32, // ms
    pub auto_profile: bool,
    pub auto_profiles: Vec<AutoProfile>,
    pub toggle: Input,
    /// UI language code ("en", "de", …); empty = not chosen yet, ask on start.
    pub lang: String,
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            profile_dir: "profiles".into(), recent: vec![], recent_count: 5, last_profile: String::new(),
            auto_load_last: true, startup: false, hide_empty: false, numpad: true, hz: 1000,
            key_repeat: false, key_repeat_delay: 660, key_repeat_rate: 25, disable_epp: false,
            smoothing: false, history: 10, weight: 0.2, mouse_refresh: 1, auto_profile: false, auto_profiles: vec![],
            toggle: Input::Key(0x77),
            lang: String::new(),
            check_updates: true,
        }
    }
}

pub const SETTINGS_FILE: &str = "settings.json";

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(SETTINGS_FILE).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let _ = std::fs::write(SETTINGS_FILE, serde_json::to_string_pretty(self).unwrap());
    }
}

/// Identifies one virtual button inside a set.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Elem {
    Btn(u8),
    DPad(u8),
    Stick(u8, u8),
    StickMod(u8),
    Trig(u8, bool), // true = positive
    Key(u16),       // index into Set::keys
}

impl Set {
    pub fn button(&self, e: Elem) -> Option<&Button> {
        Some(match e {
            Elem::Btn(i) => &self.buttons[i as usize],
            Elem::DPad(d) => &self.dpad.dirs[d as usize],
            Elem::Stick(s, d) => &self.sticks[s as usize].dirs[d as usize],
            Elem::StickMod(s) => &self.sticks[s as usize].modifier,
            Elem::Trig(t, p) => { let a = &self.triggers[t as usize]; if p { &a.p } else { &a.n } }
            Elem::Key(i) => &self.keys.get(i as usize)?.btn,
        })
    }
    pub fn button_mut(&mut self, e: Elem) -> Option<&mut Button> {
        Some(match e {
            Elem::Btn(i) => &mut self.buttons[i as usize],
            Elem::DPad(d) => &mut self.dpad.dirs[d as usize],
            Elem::Stick(s, d) => &mut self.sticks[s as usize].dirs[d as usize],
            Elem::StickMod(s) => &mut self.sticks[s as usize].modifier,
            Elem::Trig(t, p) => { let a = &mut self.triggers[t as usize]; if p { &mut a.p } else { &mut a.n } }
            Elem::Key(i) => &mut self.keys.get_mut(i as usize)?.btn,
        })
    }
}
