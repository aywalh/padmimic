//! Shared icon variants for the window and the Windows notification area.
use std::{mem, ptr};
use std::sync::OnceLock;
use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    w,
};

const MARKS: &[&[u8]] = &[
    include_bytes!("../assets/branding/monogramm.png"),
    include_bytes!("../assets/branding/spiegel.png"),
    include_bytes!("../assets/branding/pad.png"),
    include_bytes!("../assets/branding/twin-stick.png"),
    include_bytes!("../assets/branding/doppel.png"),
];
const CALLBACK: u32 = WM_APP + 1;
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

pub fn next_variant(index: usize) -> usize {
    (index % MARKS.len() + 1) % MARKS.len()
}

pub fn window_icon(index: usize) -> eframe::egui::IconData {
    let pixels = image::load_from_memory(MARKS[index % MARKS.len()]).expect("embedded app icon PNG").into_rgba8();
    eframe::egui::IconData { width: pixels.width(), height: pixels.height(), rgba: pixels.into_raw() }
}

/// Link the large taskbar icon to the exact icon registered with Explorer.
pub fn apply_taskbar_icon() -> bool {
    let Some(hwnd) = crate::winio::own_window("padmimic") else { return false };
    unsafe {
        let icon = crate::winio::own_window("PadmimicNotificationIcon")
            .map(|window| GetWindowLongPtrW(window, GWLP_USERDATA))
            .filter(|icon| *icon != 0)
            .unwrap_or_else(|| SendMessageW(hwnd, WM_GETICON, ICON_SMALL as usize, 0));
        if icon == 0 { return false; }
        // The tray or window owns the icon until after the eframe event loop ends.
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icon);
        true
    }
}

pub struct Tray {
    window: HWND,
    icon: HICON,
}

impl Tray {
    pub fn start(index: usize) -> Option<Self> {
        let data = MARKS[index % MARKS.len()];
        // The icon resource must be aligned to a DWORD boundary.
        let mut aligned = vec![0u32; data.len().div_ceil(4)];
        unsafe {
            let bytes = aligned.as_mut_ptr().cast::<u8>();
            ptr::copy_nonoverlapping(data.as_ptr(), bytes, data.len());
            let icon = CreateIconFromResourceEx(bytes, data.len() as u32, 1, 0x0003_0000, 32, 32, LR_DEFAULTCOLOR);
            if icon.is_null() { return None; }

            let class = w!("PadmimicNotificationIcon");
            let module = GetModuleHandleW(ptr::null());
            let wnd = WNDCLASSW {
                lpfnWndProc: Some(tray_proc),
                hInstance: module,
                lpszClassName: class,
                ..mem::zeroed()
            };
            RegisterClassW(&wnd);
            TASKBAR_CREATED.get_or_init(|| RegisterWindowMessageW(w!("TaskbarCreated")));
            let window = CreateWindowExW(
                WS_EX_TOOLWINDOW, class, class, WS_POPUP, 0, 0, 0, 0,
                ptr::null_mut(), ptr::null_mut(), module, ptr::null(),
            );
            if window.is_null() {
                DestroyIcon(icon);
                return None;
            }
            SetWindowLongPtrW(window, GWLP_USERDATA, icon as isize);
            let tray = Self { window, icon };
            if Shell_NotifyIconW(NIM_ADD, &notification(window, icon)) == 0 { return None; }
            Some(tray)
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &notification(self.window, self.icon));
            DestroyWindow(self.window);
            DestroyIcon(self.icon);
        }
    }
}

fn notification(window: HWND, icon: HICON) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        uFlags: NIF_ICON | NIF_TIP | NIF_MESSAGE,
        uCallbackMessage: CALLBACK,
        hIcon: icon,
        ..Default::default()
    };
    for (slot, unit) in data.szTip.iter_mut().zip("Padmimic".encode_utf16()) { *slot = unit; }
    data
}

unsafe extern "system" fn tray_proc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if message == CALLBACK && l as u32 == WM_LBUTTONUP {
            if let Some(main) = crate::winio::own_window("padmimic") {
                ShowWindow(main, SW_RESTORE);
                SetForegroundWindow(main);
            }
            return 0;
        }
        if TASKBAR_CREATED.get().is_some_and(|&registered| registered != 0 && message == registered) {
            let icon = GetWindowLongPtrW(window, GWLP_USERDATA) as HICON;
            if !icon.is_null() { Shell_NotifyIconW(NIM_ADD, &notification(window, icon)); }
            return 0;
        }
        DefWindowProcW(window, message, w, l)
    }
}
