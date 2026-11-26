//! Thin Win32 helpers: key/mouse injection, key names, cursor, foreground app, system toggles.

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::{mem, ptr};
use windows_sys::Win32::Foundation::{CloseHandle, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Real key / mouse-button state as the low-level hooks saw it (a swallowed key never reaches GetAsyncKeyState).
pub static DOWN: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
/// Keys the hooks swallow: they go to the game only through the virtual pad, never twice.
pub static BLOCK: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];

pub fn down(vk: u16) -> bool {
    let i = vk as usize & 0xFF;
    if BLOCK[i].load(Relaxed) { DOWN[i].load(Relaxed) } else { unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 } }
}

/// Record a real key/button event; true = swallow it.
fn hook_key(vk: u32, is_down: bool) -> bool {
    let generic = match vk { 0xA0 | 0xA1 => 0x10, 0xA2 | 0xA3 => 0x11, 0xA4 | 0xA5 => 0x12, v => v };
    let (a, b) = (vk as usize & 0xFF, generic as usize & 0xFF);
    DOWN[a].store(is_down, Relaxed);
    DOWN[b].store(is_down, Relaxed);
    if !(BLOCK[a].load(Relaxed) || BLOCK[b].load(Relaxed)) { return false }
    // never swallow input meant for padmimic's own window, or it can't be clicked while on
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
    pid != unsafe { GetCurrentProcessId() }
}

pub unsafe extern "system" fn kb_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        let k = &*(l as *const KBDLLHOOKSTRUCT);
        // injected = our own SendInput output, it must pass untouched
        if code == HC_ACTION as i32 && k.flags & LLKHF_INJECTED == 0 && hook_key(k.vkCode, matches!(w as u32, WM_KEYDOWN | WM_SYSKEYDOWN)) {
            return 1;
        }
        CallNextHookEx(ptr::null_mut(), code, w, l)
    }
}

pub unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        let m = &*(l as *const MSLLHOOKSTRUCT);
        let x = if (m.mouseData >> 16) as u16 == 1 { 5 } else { 6 };
        let ev = match w as u32 {
            WM_LBUTTONDOWN => Some((1, true)), WM_LBUTTONUP => Some((1, false)),
            WM_RBUTTONDOWN => Some((2, true)), WM_RBUTTONUP => Some((2, false)),
            WM_MBUTTONDOWN => Some((4, true)), WM_MBUTTONUP => Some((4, false)),
            WM_XBUTTONDOWN => Some((x, true)), WM_XBUTTONUP => Some((x, false)),
            _ => None,
        };
        if let (true, Some((vk, d))) = (code == HC_ACTION as i32 && m.flags & LLMHF_INJECTED == 0, ev) {
            let target = WindowFromPoint(m.pt);
            let mut target_pid = 0;
            GetWindowThreadProcessId(target, &mut target_pid);
            if target_pid == GetCurrentProcessId() {
                // Record physical state but never block clicks meant for any of our windows.
                DOWN[vk as usize].store(d, Relaxed);
            } else if hook_key(vk, d) { return 1 }
        }
        CallNextHookEx(ptr::null_mut(), code, w, l)
    }
}

fn send(inputs: &[INPUT]) {
    if !inputs.is_empty() {
        unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), mem::size_of::<INPUT>() as i32) };
    }
}

fn is_extended(vk: u16) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x5D | 0x6F | 0x90 | 0xA3 | 0xA5)
}

fn key_input(vk: u16, up: bool) -> INPUT {
    // scancodes: games reading raw/DirectInput ignore VK-only events
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    let mut flags = if scan != 0 { KEYEVENTF_SCANCODE } else { 0 };
    if up { flags |= KEYEVENTF_KEYUP }
    if is_extended(vk) { flags |= KEYEVENTF_EXTENDEDKEY }
    let mut i: INPUT = unsafe { mem::zeroed() };
    i.r#type = INPUT_KEYBOARD;
    i.Anonymous.ki = KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 };
    i
}

pub fn key(vk: u16, up: bool) {
    send(&[key_input(vk, up)]);
}

fn mouse_input(dx: i32, dy: i32, data: i32, flags: u32) -> INPUT {
    let mut i: INPUT = unsafe { mem::zeroed() };
    i.r#type = INPUT_MOUSE;
    i.Anonymous.mi = MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 };
    i
}

/// 1 L, 2 R, 3 M, 4 X1, 5 X2
pub fn mouse_button(b: u8, up: bool) {
    let (d, u, data) = match b {
        1 => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 0),
        2 => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, 0),
        3 => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, 0),
        4 => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, XBUTTON1 as i32),
        _ => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, XBUTTON2 as i32),
    };
    send(&[mouse_input(0, 0, data, if up { u } else { d })]);
}

pub fn mouse_move(dx: i32, dy: i32) {
    send(&[mouse_input(dx, dy, 0, MOUSEEVENTF_MOVE)]);
}

/// 0 up, 1 down, 2 left, 3 right; one notch
pub fn wheel(dir: u8) {
    let (flag, data) = match dir {
        0 => (MOUSEEVENTF_WHEEL, 120),
        1 => (MOUSEEVENTF_WHEEL, -120),
        2 => (MOUSEEVENTF_HWHEEL, -120),
        _ => (MOUSEEVENTF_HWHEEL, 120),
    };
    send(&[mouse_input(0, 0, data, flag)]);
}

pub fn type_text(s: &str) {
    let mut v = Vec::new();
    for u in s.encode_utf16() {
        for up in [false, true] {
            let mut i: INPUT = unsafe { mem::zeroed() };
            i.r#type = INPUT_KEYBOARD;
            i.Anonymous.ki = KEYBDINPUT { wVk: 0, wScan: u, dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 };
            v.push(i);
        }
    }
    send(&v);
}

pub fn cursor() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

pub fn set_cursor(x: i32, y: i32) {
    unsafe { SetCursorPos(x, y) };
}

pub fn screen() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

pub fn key_name(vk: u16) -> String {
    let fixed = match vk {
        0 => "–", 1 => "Mouse L", 2 => "Mouse R", 4 => "Mouse M", 5 => "Mouse 4", 6 => "Mouse 5",
        0x08 => "Backspace", 0x09 => "Tab", 0x0D => "Enter", 0x10 => "Shift", 0x11 => "Ctrl", 0x12 => "Alt", 0x13 => "Pause",
        0x14 => "Caps", 0x1B => "Esc", 0x20 => "Space", 0x21 => "PgUp", 0x22 => "PgDn", 0x23 => "End", 0x24 => "Home",
        0x25 => "←", 0x26 => "↑", 0x27 => "→", 0x28 => "↓", 0x2C => "Print", 0x2D => "Ins", 0x2E => "Del",
        0x5B => "Win", 0x5C => "Win R", 0x5D => "Menu", 0x90 => "Num", 0x91 => "Scroll",
        0xA0 => "Shift L", 0xA1 => "Shift R", 0xA2 => "Ctrl L", 0xA3 => "Ctrl R", 0xA4 => "Alt", 0xA5 => "AltGr",
        0x6A => "Num *", 0x6B => "Num +", 0x6D => "Num -", 0x6E => "Num ,", 0x6F => "Num /",
        _ => "",
    };
    if !fixed.is_empty() { return fixed.into() }
    if (0x70..=0x87).contains(&vk) { return format!("F{}", vk - 0x6F) }
    if (0x60..=0x69).contains(&vk) { return format!("Num {}", vk - 0x60) }
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as i32;
    let mut buf = [0u16; 32];
    let n = unsafe { GetKeyNameTextW(scan << 16, buf.as_mut_ptr(), 32) };
    if n > 0 { String::from_utf16_lossy(&buf[..n as usize]) } else { format!("VK {vk:#04x}") }
}

/// Executable file name of the foreground window, lowercase.
pub fn foreground_exe() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() { return None }
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 { return None }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(|s| s.to_lowercase())
    }
}

/// Windows "Enhance pointer precision" (mouse acceleration). Returns the previous state.
pub fn set_pointer_precision(on: bool) -> bool {
    let mut p = [0i32; 3]; // threshold1, threshold2, acceleration
    unsafe {
        SystemParametersInfoW(SPI_GETMOUSE, 0, p.as_mut_ptr() as *mut _, 0);
        let was = p[2] != 0;
        p[2] = on as i32;
        SystemParametersInfoW(SPI_SETMOUSE, 0, p.as_mut_ptr() as *mut _, 0);
        was
    }
}

/// Download the pinned ViGEmBus release (Nefarius, signed), check its SHA-256 and run it elevated;
/// Windows asks once for admin rights. Ok(true) = installed, Windows restart recommended.
pub fn install_vigem() -> Result<bool, String> {
    use std::os::windows::process::CommandExt;
    const URL: &str = "https://github.com/nefarius/ViGEmBus/releases/download/v1.22.0/ViGEmBus_1.22.0_x64_x86_arm64.exe";
    const SHA256: &str = "89220A7865076B342892F98865F3499FB7C4CFD673159E89D352C360FD014C6A";
    let script = format!(
        "$ProgressPreference='SilentlyContinue'; [Net.ServicePointManager]::SecurityProtocol='Tls12'
$f = Join-Path $env:TEMP 'padmimic-vigembus-setup.exe'
try {{ Invoke-WebRequest -UseBasicParsing '{URL}' -OutFile $f }} catch {{ exit 10 }}
if ((Get-FileHash $f -Algorithm SHA256).Hash -ne '{SHA256}') {{ Remove-Item $f; exit 11 }}
try {{ $p = Start-Process $f -ArgumentList '/passive','/norestart' -Verb RunAs -Wait -PassThru }} catch {{ Remove-Item $f; exit 12 }}
Remove-Item $f
exit $p.ExitCode"
    );
    let code = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()
        .map_err(|e| format!("PowerShell could not start: {e}"))?
        .code()
        .unwrap_or(-1);
    match code {
        0 => Ok(false),
        3010 => Ok(true),
        10 => Err("ViGEmBus: download failed – check the internet connection".into()),
        11 => Err("ViGEmBus: wrong checksum – the file was not run".into()),
        12 | 1223 | 1602 => Err("ViGEmBus: admin prompt declined or cancelled".into()),
        c => Err(format!("ViGEmBus installer error {c}")),
    }
}

/// Start with Windows via HKCU Run key.
pub fn set_startup(on: bool) {
    set_startup_for(on, &std::env::current_exe().unwrap_or_default());
}

pub fn set_startup_for(on: bool, exe: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let exe = exe.display().to_string();
    let mut c = std::process::Command::new("reg");
    if on {
        c.args(["add", key, "/v", "padmimic", "/t", "REG_SZ", "/d", &format!("\"{exe}\""), "/f"]);
    } else {
        c.args(["delete", key, "/v", "padmimic", "/f"]);
    }
    let _ = c.creation_flags(0x0800_0000).output(); // CREATE_NO_WINDOW
}

/// Top-level window of this process with exactly this title (the app can have several viewports).
pub fn own_window(title: &str) -> Option<windows_sys::Win32::Foundation::HWND> {
    struct Find { pid: u32, title: Vec<u16>, hit: windows_sys::Win32::Foundation::HWND }
    unsafe extern "system" fn each(h: windows_sys::Win32::Foundation::HWND, l: LPARAM) -> windows_sys::core::BOOL {
        unsafe {
            let f = &mut *(l as *mut Find);
            let mut pid = 0;
            GetWindowThreadProcessId(h, &mut pid);
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(h, buf.as_mut_ptr(), 256).max(0) as usize;
            if pid == f.pid && buf[..n] == f.title[..] { f.hit = h; return 0 }
            1
        }
    }
    let mut f = Find { pid: unsafe { GetCurrentProcessId() }, title: title.encode_utf16().collect(), hit: ptr::null_mut() };
    unsafe { EnumWindows(Some(each), &mut f as *mut Find as LPARAM) };
    (!f.hit.is_null()).then_some(f.hit)
}

/// Windows 11 material behind a window: 0 none, 2 Mica, 3 Acrylic, 4 Mica Alt (DWM_SYSTEMBACKDROP_TYPE).
/// The window must be transparent (egui clears with alpha 0) for it to show. False on Windows 10.
pub fn set_backdrop(title: &str, kind: i32, dark: bool) -> bool {
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute};
    use windows_sys::Win32::UI::Controls::MARGINS;
    let Some(h) = own_window(title) else { return false };
    unsafe {
        let d: i32 = dark as i32;
        DwmSetWindowAttribute(h, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, &d as *const i32 as *const _, 4);
        // glass over the whole client area, extended from the bottom edge: a top (caption) margin would make
        // DWM draw its own minimize / maximize / close buttons behind ours
        let m = MARGINS { cxLeftWidth: 0, cxRightWidth: 0, cyTopHeight: 0, cyBottomHeight: if kind == 0 { 0 } else { 32767 } };
        DwmExtendFrameIntoClientArea(h, &m);
        DwmSetWindowAttribute(h, DWMWA_SYSTEMBACKDROP_TYPE as u32, &kind as *const i32 as *const _, 4) >= 0
    }
}

/// Work area (screen minus taskbar) of the monitor at this physical point: left, top, right, bottom.
pub fn work_area(x: i32, y: i32) -> (i32, i32, i32, i32) {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
    unsafe {
        let mut mi: MONITORINFO = mem::zeroed();
        mi.cbSize = mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST), &mut mi);
        let r = mi.rcWork;
        let m = mi.rcMonitor;
        // a window covering the whole monitor (auto-hidden taskbar) counts as fullscreen for DWM,
        // which then drops Mica / Acrylic: stay one pixel short of it
        let full = (r.left, r.top, r.right, r.bottom) == (m.left, m.top, m.right, m.bottom);
        (r.left, r.top, r.right, r.bottom - full as i32)
    }
}

/// Windows 11: round corners for a frameless window of this process (no-op on Windows 10).
pub fn round_corners(title: &str) {
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
    if let Some(h) = own_window(title) {
        let v = DWMWCP_ROUND;
        unsafe { DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &v as *const _ as *const _, 4) };
    }
}

/// Work areas of all active monitors; screen gaps and taskbars are excluded.
pub fn monitor_work_areas() -> Vec<(i32, i32, i32, i32)> {
    use windows_sys::Win32::Graphics::Gdi::*;
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut windows_sys::Win32::Foundation::RECT, data: LPARAM) -> windows_sys::core::BOOL {
        unsafe {
            let areas = &mut *(data as *mut Vec<(i32, i32, i32, i32)>);
            let mut info: MONITORINFO = mem::zeroed();
            info.cbSize = mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(monitor, &mut info) != 0 {
                let r = info.rcWork;
                areas.push((r.left, r.top, r.right, r.bottom));
            }
            1
        }
    }
    let mut areas = vec![];
    unsafe { EnumDisplayMonitors(ptr::null_mut(), ptr::null(), Some(each), &mut areas as *mut _ as LPARAM); }
    areas
}

/// Route clicks through empty artwork pixels without changing the DWM surface shape.
pub fn overlay_hit_test(title: &str, rectangles: Option<Vec<[i32; 4]>>) -> bool {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::{Dwm::*, Gdi::ScreenToClient};
    use windows_sys::core::w;
    struct HitTest {
        original: WNDPROC,
        rectangles: Option<Vec<[i32; 4]>>,
    }
    const PROPERTY: *const u16 = w!("PadmimicOverlayHitTest");

    unsafe extern "system" fn hit_test(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe {
            let data = GetPropW(hwnd, PROPERTY).cast::<HitTest>();
            if data.is_null() { return DefWindowProcW(hwnd, msg, w, l); }
            let original = (*data).original;
            if msg == WM_NCDESTROY {
                RemovePropW(hwnd, PROPERTY);
                drop(Box::from_raw(data));
            } else if msg == WM_NCHITTEST {
                if let Some(rectangles) = &(*data).rectangles {
                    let mut point = POINT { x: l as i16 as i32, y: (l >> 16) as i16 as i32 };
                    if ScreenToClient(hwnd, &mut point) != 0 {
                        let start = rectangles.partition_point(|rect| rect[3] <= point.y);
                        let hit = rectangles[start..].iter().take_while(|rect| rect[1] <= point.y)
                            .any(|rect| point.x >= rect[0] && point.x < rect[2]);
                        if !hit { return HTTRANSPARENT as LRESULT; }
                    }
                }
            }
            CallWindowProcW(original, hwnd, msg, w, l)
        }
    }

    let Some(hwnd) = own_window(title) else { return false };
    unsafe {
        let data = GetPropW(hwnd, PROPERTY).cast::<HitTest>();
        if !data.is_null() {
            (*data).rectangles = rectangles;
            return true;
        }
        let original = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        if original == 0 { return false; }
        let data = Box::into_raw(Box::new(HitTest {
            original: mem::transmute::<isize, WNDPROC>(original),
            rectangles,
        }));
        if SetPropW(hwnd, PROPERTY, data.cast()) == 0 {
            drop(Box::from_raw(data));
            return false;
        }
        if SetWindowLongPtrW(hwnd, GWLP_WNDPROC, hit_test as *const () as isize) == 0 {
            RemovePropW(hwnd, PROPERTY);
            drop(Box::from_raw(data));
            return false;
        }
        let color = DWMWA_COLOR_NONE;
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as u32, (&color as *const u32).cast(), 4);
    }
    true
}
