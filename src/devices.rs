//! Every connected controller and keyboard, through Raw Input: no 4-pad limit like XInput, and it also
//! sees pads XInput never shows (DualSense, DualShock 4, Switch Pro, generic HID pads).
//! A background thread rescans every 2 s; the input thread stamps each device when it sends input.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use std::{mem, ptr, thread};
use windows_sys::Win32::Devices::HumanInterfaceDevice::HidD_GetProductString;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows_sys::Win32::UI::Input::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind { Pad, Keyboard }

#[derive(Clone, Debug)]
pub struct Device {
    pub kind: Kind,
    pub name: String,
    pub vid_pid: Option<(u16, u16)>,
    /// Xbox-style pad that also shows up in XInput ("IG_" in its path).
    pub xinput: bool,
    /// A keyboard interface of a device that is also a mouse (gaming mice expose one for their side keys).
    pub mouse: bool,
    /// Raw Input handles of all its interfaces (gaming keyboards expose several).
    pub handles: Vec<isize>,
}

static LIST: Mutex<Vec<Device>> = Mutex::new(Vec::new());
static SEEN: Mutex<Option<HashMap<isize, Instant>>> = Mutex::new(None);
/// Keys held per keyboard interface, 256 bits each (party mode: one keyboard = one player).
static KEYS: Mutex<Option<HashMap<isize, [u64; 4]>>> = Mutex::new(None);

impl Device {
    /// Survives replugging (Raw Input handles don't): what settings store for party mode.
    pub fn id(&self) -> String {
        match self.vid_pid { Some((v, p)) => format!("{v:04x}:{p:04x}"), None => self.name.clone() }
    }
}

/// WM_INPUT from a keyboard: remember what this keyboard holds.
pub fn key(h: HANDLE, vk: u16, down: bool) {
    if h.is_null() || vk >= 256 { return }
    let mut m = KEYS.lock().unwrap();
    let k = m.get_or_insert_with(HashMap::new).entry(h as isize).or_default();
    let (i, bit) = (vk as usize / 64, 1u64 << (vk % 64));
    if down { k[i] |= bit } else { k[i] &= !bit }
}

/// Keys held on any interface of this keyboard.
pub fn held(d: &Device) -> [u64; 4] {
    let m = KEYS.lock().unwrap();
    let mut out = [0u64; 4];
    for h in &d.handles {
        if let Some(k) = m.as_ref().and_then(|m| m.get(h)) { for i in 0..4 { out[i] |= k[i] } }
    }
    out
}

pub fn is_held(keys: &[u64; 4], vk: u16) -> bool { vk < 256 && keys[vk as usize / 64] & (1 << (vk % 64)) != 0 }

pub fn start() { thread::spawn(|| loop { let l = scan(); *LIST.lock().unwrap() = l; thread::sleep(Duration::from_secs(2)) }); }

pub fn list() -> Vec<Device> { LIST.lock().unwrap().clone() }

/// Called from WM_INPUT on the input thread: cheap.
pub fn touch(h: HANDLE) {
    if h.is_null() { return }
    SEEN.lock().unwrap().get_or_insert_with(HashMap::new).insert(h as isize, Instant::now());
}

/// Drop cached state when Windows reports that an input interface was unplugged.
pub fn removed(handle: isize) {
    if let Some(seen) = SEEN.lock().unwrap().as_mut() { seen.remove(&handle); }
    if let Some(keys) = KEYS.lock().unwrap().as_mut() { keys.remove(&handle); }
}

/// Sent input within the last second.
pub fn active(d: &Device) -> bool {
    let seen = SEEN.lock().unwrap();
    let Some(m) = seen.as_ref() else { return false };
    d.handles.iter().any(|h| m.get(h).is_some_and(|t| t.elapsed() < Duration::from_secs(1)))
}

/// Raw Input usages padmimic listens to for activity: joystick, gamepad, multi-axis, keyboard.
pub const USAGES: [u16; 4] = [4, 5, 8, 6];

fn scan() -> Vec<Device> {
    let mut n = 0u32;
    let sz = mem::size_of::<RAWINPUTDEVICELIST>() as u32;
    unsafe { GetRawInputDeviceList(ptr::null_mut(), &mut n, sz) };
    let mut raw = vec![unsafe { mem::zeroed::<RAWINPUTDEVICELIST>() }; n as usize + 8];
    let mut cap = raw.len() as u32;
    let got = unsafe { GetRawInputDeviceList(raw.as_mut_ptr(), &mut cap, sz) };
    if got == u32::MAX { return vec![] }
    raw.truncate(got as usize);

    // the first collection of the first USB interface (MI_00 / COL01, or the only one) says what the device mainly is
    let primary = |low: &str| (low.contains("mi_00") || !low.contains("&mi_")) && (low.contains("col01") || !low.contains("&col"));
    let mice: Vec<_> = raw.iter().filter(|r| r.dwType == RIM_TYPEMOUSE)
        .map(|r| device_path(r.hDevice).to_ascii_lowercase()).filter(|p| primary(p))
        .filter_map(|p| parse_vid_pid(&p)).collect();
    let mut out: Vec<Device> = vec![];
    for r in raw {
        let kind = match r.dwType {
            RIM_TYPEKEYBOARD => Kind::Keyboard,
            RIM_TYPEHID if matches!(hid_usage(r.hDevice), Some((1, 4 | 5 | 8))) => Kind::Pad,
            _ => continue,
        };
        let path = device_path(r.hDevice);
        let low = path.to_ascii_lowercase();
        // remote desktop / virtual phantoms that every PC lists
        if low.contains("root#rdp") || low.contains("#rdp_") || low.is_empty() { continue }
        let vid_pid = parse_vid_pid(&low);
        let xinput = low.contains("ig_");
        let name = product(&path).unwrap_or_else(|| match (kind, vid_pid) {
            (Kind::Pad, Some((v, p))) => format!("Controller {v:04X}:{p:04X}"),
            (Kind::Pad, None) => "Controller".into(),
            (Kind::Keyboard, Some((v, p))) => format!("Keyboard {v:04X}:{p:04X}"),
            (Kind::Keyboard, None) => "Keyboard".into(),
        });
        // ponytail: one entry per VID/PID and kind; two identical keyboards merge into one row
        let same = |d: &&mut Device| d.kind == kind && vid_pid.is_some() && d.vid_pid == vid_pid;
        let kb_primary = kind == Kind::Keyboard && primary(&low);
        match out.iter_mut().find(|d| same(d)) {
            Some(d) => { d.handles.push(r.hDevice as isize); if kb_primary { d.mouse = false } }
            None => {
                let mouse = kind == Kind::Keyboard && !kb_primary && vid_pid.is_some_and(|v| mice.contains(&v));
                out.push(Device { kind, name, vid_pid, xinput, mouse, handles: vec![r.hDevice as isize] })
            }
        }
    }
    out.sort_by_key(|d| (d.kind == Kind::Keyboard, d.name.to_lowercase()));
    out
}

fn hid_usage(h: HANDLE) -> Option<(u16, u16)> {
    let mut info: RID_DEVICE_INFO = unsafe { mem::zeroed() };
    info.cbSize = mem::size_of::<RID_DEVICE_INFO>() as u32;
    let mut size = info.cbSize;
    let r = unsafe { GetRawInputDeviceInfoW(h, RIDI_DEVICEINFO, &mut info as *mut _ as *mut _, &mut size) };
    if r == u32::MAX || info.dwType != RIM_TYPEHID { return None }
    let hid = unsafe { info.Anonymous.hid };
    Some((hid.usUsagePage, hid.usUsage))
}

fn device_path(h: HANDLE) -> String {
    let mut len = 0u32;
    unsafe { GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, ptr::null_mut(), &mut len) };
    let mut buf = vec![0u16; len as usize + 1];
    let r = unsafe { GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, buf.as_mut_ptr() as *mut _, &mut len) };
    if r == u32::MAX { return String::new() }
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())])
}

/// The name the device reports itself ("DualSense Wireless Controller"); needs no access rights.
fn product(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
    unsafe {
        let f = CreateFileW(wide.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, ptr::null(), OPEN_EXISTING, 0, ptr::null_mut());
        if f == INVALID_HANDLE_VALUE { return None }
        let mut buf = [0u16; 128];
        let ok = HidD_GetProductString(f, buf.as_mut_ptr() as *mut _, (buf.len() * 2) as u32);
        CloseHandle(f);
        if !ok { return None }
        let s = String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]);
        let s = s.trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

fn parse_vid_pid(low: &str) -> Option<(u16, u16)> {
    // USB: "vid_054c&pid_0ce6"; Bluetooth: "_vid&0002054c_pid&0ce6" (4 digits of vendor source first)
    let hex = |key: &str, skip: usize| low.find(key).and_then(|i| low.get(i + key.len() + skip..i + key.len() + skip + 4)).and_then(|s| u16::from_str_radix(s, 16).ok());
    Some((hex("vid_", 0).or_else(|| hex("vid&", 4))?, hex("pid_", 0).or_else(|| hex("pid&", 0))?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vid_pid_from_paths() {
        assert_eq!(parse_vid_pid(r"\\?\hid#vid_054c&pid_0ce6&mi_03#7&1a2b#{4d1e55b2}"), Some((0x054c, 0x0ce6)));
        assert_eq!(parse_vid_pid(r"\\?\hid#vid_045e&pid_02ff&ig_00#8&3c"), Some((0x045e, 0x02ff)));
        assert_eq!(parse_vid_pid(r"\\?\hid#{00001124-0000-1000-8000-00805f9b34fb}_vid&0002054c_pid&0ce6#9&2f"), Some((0x054c, 0x0ce6)));
        assert_eq!(parse_vid_pid(r"\\?\acpi#pnp0303#4&1d401fb5&0#{884b96c3}"), None);
    }

    #[test]
    fn keys_per_keyboard() {
        let (k1, k2) = (0x1001 as HANDLE, 0x1002 as HANDLE);
        key(k1, b'W' as u16, true);
        key(k2, 0x26, true);
        let d1 = Device { kind: Kind::Keyboard, name: "1".into(), vid_pid: None, xinput: false, mouse: false, handles: vec![0x1001] };
        let d2 = Device { handles: vec![0x1002], ..d1.clone() };
        assert!(is_held(&held(&d1), b'W' as u16) && !is_held(&held(&d1), 0x26), "keyboard 1 only holds W");
        assert!(is_held(&held(&d2), 0x26) && !is_held(&held(&d2), b'W' as u16), "keyboard 2 only holds ↑");
        key(k1, b'W' as u16, false);
        assert!(!is_held(&held(&d1), b'W' as u16));
    }

    #[test]
    fn scan_runs() {
        // whatever is plugged in: it must not crash, and phantoms are filtered
        for d in scan() {
            println!("{:?} {:?} xinput={} mouse={} interfaces={}", d.kind, d.name, d.xinput, d.mouse, d.handles.len());
            assert!(!d.handles.is_empty() && !d.name.is_empty());
        }
    }
}
