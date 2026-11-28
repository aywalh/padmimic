//! PlayStation controller report -> Xbox 360 state. Pure logic, no Windows.

// Xbox-360-Tastenbits (ViGEm Common.h, XUSB_BUTTON)
pub const UP: u16 = 0x0001;
pub const DOWN: u16 = 0x0002;
pub const LEFT: u16 = 0x0004;
pub const RIGHT: u16 = 0x0008;
pub const START: u16 = 0x0010;
pub const BACK: u16 = 0x0020;
pub const LS: u16 = 0x0040;
pub const RS: u16 = 0x0080;
pub const LB: u16 = 0x0100;
pub const RB: u16 = 0x0200;
pub const GUIDE: u16 = 0x0400;
pub const A: u16 = 0x1000;
pub const B: u16 = 0x2000;
pub const X: u16 = 0x4000;
pub const Y: u16 = 0x8000;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Pad {
    pub buttons: u16,
    pub lt: u8,
    pub rt: u8,
    pub lx: f32, // -1..1, oben/rechts positiv
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SonyModel {
    DualShock4,
    DualSense,
}

impl SonyModel {
    pub fn from_usb_ids(vendor: u16, product: u16) -> Option<Self> {
        match (vendor, product) {
            (0x054c, 0x05c4 | 0x09cc) => Some(Self::DualShock4),
            (0x054c, 0x0ce6 | 0x0df2) => Some(Self::DualSense),
            _ => None,
        }
    }
}

struct Layout {
    lx: usize,
    ly: usize,
    rx: usize,
    ry: usize,
    l2: usize,
    r2: usize,
    b0: usize,
    b1: usize,
    b2: usize,
}

/// Vollreport (USB 0x01, Bluetooth 0x31 mit 1 Byte Versatz)
const FULL: Layout = Layout {
    lx: 1,
    ly: 2,
    rx: 3,
    ry: 4,
    l2: 5,
    r2: 6,
    b0: 8,
    b1: 9,
    b2: 10,
};
/// Kurzreport, den der DualSense per Bluetooth schickt, bevor er umgeschaltet ist
const SHORT: Layout = Layout {
    lx: 1,
    ly: 2,
    rx: 3,
    ry: 4,
    b0: 5,
    b1: 6,
    b2: 7,
    l2: 8,
    r2: 9,
};

/// 0..255 -> -1..1 mit Totzone
pub fn axis(v: u8, deadzone: f32) -> f32 {
    let x = (v as f32 - 128.0) / if v < 128 { 128.0 } else { 127.0 };
    if x.abs() < deadzone { 0.0 } else { x }
}

/// None = kein Eingabe-Report (oder zu kurz)
pub fn decode(r: &[u8], model: SonyModel, bluetooth: bool, deadzone: f32) -> Option<Pad> {
    let (l, o) = match (model, bluetooth, *r.first()?) {
        (SonyModel::DualShock4, true, 0x11) => (&SHORT, 2),
        (SonyModel::DualShock4, _, 0x01) => (&SHORT, 0),
        (SonyModel::DualSense, true, 0x31) => (&FULL, 1),
        (SonyModel::DualSense, true, 0x01) => (&SHORT, 0),
        (SonyModel::DualSense, false, 0x01) => (&FULL, 0),
        _ => return None,
    };
    let b = |i: usize| r.get(i + o).copied();
    let (b0, b1, b2) = (b(l.b0)?, b(l.b1)?, b(l.b2)?);
    let mut buttons = match b0 & 0x0F {
        0 => UP,
        1 => UP | RIGHT,
        2 => RIGHT,
        3 => DOWN | RIGHT,
        4 => DOWN,
        5 => DOWN | LEFT,
        6 => LEFT,
        7 => UP | LEFT,
        _ => 0,
    };
    for (byte, bit, xb) in [
        (b0, 0x10, X),
        (b0, 0x20, A),
        (b0, 0x40, B),
        (b0, 0x80, Y),
        (b1, 0x01, LB),
        (b1, 0x02, RB),
        (b1, 0x10, BACK),
        (b1, 0x20, START),
        (b1, 0x40, LS),
        (b1, 0x80, RS),
        (b2, 0x01, GUIDE),
    ] {
        if byte & bit != 0 {
            buttons |= xb;
        }
    }
    Some(Pad {
        buttons,
        lt: b(l.l2)?,
        rt: b(l.r2)?,
        lx: axis(b(l.lx)?, deadzone),
        ly: -axis(b(l.ly)?, deadzone), // DualSense oben = 0, Xbox oben = positiv
        rx: axis(b(l.rx)?, deadzone),
        ry: -axis(b(l.ry)?, deadzone),
    })
}

/// Touchpad click is present on both models; microphone mute is DualSense-specific.
fn extra_buttons(report: &[u8], model: SonyModel, bluetooth: bool) -> u8 {
    let index = match (model, bluetooth, report.first()) {
        (SonyModel::DualShock4, true, Some(0x11)) => 9,
        (SonyModel::DualSense, true, Some(0x31)) => 11,
        (SonyModel::DualSense, false, _) => 10,
        _ => 7,
    };
    let flags = report.get(index).copied().unwrap_or_default() >> 1;
    flags & if model == SonyModel::DualSense { 3 } else { 1 }
}

use std::mem::{size_of, zeroed};
use std::sync::Mutex;
use vigem_client::{XButtons, XGamepad};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::UI::Input::*;

#[derive(Clone, Copy)]
pub struct Reading {
    pub model: SonyModel,
    pub bluetooth: bool,
    pub pad: XGamepad,
    pub extra: u8,
    pub packet: u32,
}

struct Device {
    handle: usize,
    reading: Reading,
}

static DEVICES: Mutex<Vec<Device>> = Mutex::new(Vec::new());

pub fn current() -> Option<Reading> {
    // Read from the device most recently used. Idle report traffic keeps its position.
    DEVICES.lock().unwrap().last().map(|device| device.reading)
}

pub fn removed(handle: usize) {
    DEVICES
        .lock()
        .unwrap()
        .retain(|device| device.handle != handle);
}

unsafe fn identify(handle: HANDLE) -> Option<SonyModel> {
    let mut info: RID_DEVICE_INFO = unsafe { zeroed() };
    let mut size = size_of::<RID_DEVICE_INFO>() as u32;
    info.cbSize = size;
    let result = unsafe {
        GetRawInputDeviceInfoW(
            handle,
            RIDI_DEVICEINFO,
            (&mut info as *mut RID_DEVICE_INFO).cast(),
            &mut size,
        )
    };
    if result == u32::MAX || info.dwType != RIM_TYPEHID {
        return None;
    }
    let hid = unsafe { info.Anonymous.hid };
    SonyModel::from_usb_ids(
        hid.dwVendorId.try_into().ok()?,
        hid.dwProductId.try_into().ok()?,
    )
}

fn actuated(old: Reading, new: Reading) -> bool {
    let (a, b) = (old.pad, new.pad);
    a.buttons != b.buttons
        || old.extra != new.extra
        || a.left_trigger.abs_diff(b.left_trigger) > 3
        || a.right_trigger.abs_diff(b.right_trigger) > 3
        || [
            (a.thumb_lx, b.thumb_lx),
            (a.thumb_ly, b.thumb_ly),
            (a.thumb_rx, b.thumb_rx),
            (a.thumb_ry, b.thumb_ry),
        ]
        .into_iter()
        .any(|(before, after)| before.abs_diff(after) > 1024)
}

pub unsafe fn raw_input(input: HRAWINPUT) {
    let mut buf = [0usize; 1024];
    let mut size = size_of::<[usize; 1024]>() as u32;
    let header_size = size_of::<RAWINPUTHEADER>();
    let read = unsafe {
        GetRawInputData(
            input,
            RID_INPUT,
            buf.as_mut_ptr().cast(),
            &mut size,
            header_size as u32,
        )
    };
    if read == u32::MAX || (read as usize) < header_size + 8 {
        return;
    }
    let raw = unsafe { &*buf.as_ptr().cast::<RAWINPUTHEADER>() };
    if raw.dwType != RIM_TYPEHID || raw.hDevice.is_null() {
        return;
    }
    let mut devices = DEVICES.lock().unwrap();
    let handle = raw.hDevice as usize;
    let index = devices.iter().position(|device| device.handle == handle);
    let model = match index {
        Some(i) => devices[i].reading.model,
        None => match unsafe { identify(raw.hDevice) } {
            Some(model) => model,
            None => return,
        },
    };
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), read as usize) };
    let len = u32::from_ne_bytes(bytes[header_size..header_size + 4].try_into().unwrap()) as usize;
    let count =
        u32::from_ne_bytes(bytes[header_size + 4..header_size + 8].try_into().unwrap()) as usize;
    if len == 0 || count > (bytes.len() - header_size - 8) / len {
        return;
    }
    let old = index.map(|i| devices[i].reading);
    let mut next = old;
    for report in bytes[header_size + 8..].chunks_exact(len).take(count) {
        let bluetooth = len > 64 || matches!(report.first(), Some(0x11 | 0x31)) || len < 64;
        let Some(p) = decode(report, model, bluetooth, 0.0) else {
            continue;
        };
        let scale = |x: f32| (x * if x < 0.0 { 32768.0 } else { 32767.0 }).round() as i16;
        next = Some(Reading {
            model,
            bluetooth,
            pad: XGamepad {
                buttons: XButtons(p.buttons),
                left_trigger: p.lt,
                right_trigger: p.rt,
                thumb_lx: scale(p.lx),
                thumb_ly: scale(p.ly),
                thumb_rx: scale(p.rx),
                thumb_ry: scale(p.ry),
            },
            extra: extra_buttons(report, model, bluetooth),
            packet: next.map_or(1, |sample| sample.packet.wrapping_add(1)),
        });
    }
    let Some(reading) = next else { return };
    match index {
        Some(i) if old.is_some_and(|prior| actuated(prior, reading)) => {
            devices.remove(i);
            devices.push(Device { handle, reading });
        }
        Some(i) => devices[i].reading = reading,
        None => devices.insert(0, Device { handle, reading }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dualshock_usb_and_bluetooth_have_the_same_controls() {
        let usb = [1, 255, 128, 128, 0, 0x21, 0x43, 3, 65, 170];
        let mut bt = [0u8; 78];
        bt[0] = 0x11;
        bt[3..12].copy_from_slice(&usb[1..]);
        let p = decode(&usb, SonyModel::DualShock4, false, 0.0).unwrap();
        assert_eq!(decode(&bt, SonyModel::DualShock4, true, 0.0), Some(p));
        assert_eq!(p.buttons, A | UP | RIGHT | LB | RB | LS | GUIDE);
        assert_eq!((p.lt, p.rt, p.lx, p.ry), (65, 170, 1.0, 1.0));
        assert_eq!(extra_buttons(&bt, SonyModel::DualShock4, true), 1);
        assert!(decode(&bt[..10], SonyModel::DualShock4, true, 0.0).is_none());
    }

    #[test]
    fn dualsense_supports_full_and_short_bluetooth_reports() {
        let usb = [1, 128, 128, 128, 128, 9, 72, 0, 0x88, 0x80, 7];
        let short = [1, 128, 128, 128, 128, 0x88, 0x80, 7, 9, 72];
        let mut bt = [0u8; 78];
        bt[0] = 0x31;
        bt[2..12].copy_from_slice(&usb[1..]);
        let expected = decode(&usb, SonyModel::DualSense, false, 0.0).unwrap();
        assert_eq!(
            decode(&short, SonyModel::DualSense, true, 0.0),
            Some(expected)
        );
        assert_eq!(decode(&bt, SonyModel::DualSense, true, 0.0), Some(expected));
        assert_eq!(expected.buttons, Y | RS | GUIDE);
        assert_eq!(extra_buttons(&bt, SonyModel::DualSense, true), 3);
    }
}
