//! Own installer and uninstaller in padmimic's look. There is no second program: the release ships the
//! same exe twice, and a copy named `*setup*.exe` installs itself; `--uninstall` (the Windows "Apps"
//! entry) removes it. Per user, no admin rights: %LOCALAPPDATA%\Programs\padmimic, Start menu and
//! desktop shortcuts, the HKCU uninstall entry. Settings and profiles stay next to the exe as before,
//! so the self-update keeps working unchanged.

use crate::i18n::{self, tr, trf};
use crate::ui::{self, WinBtn, pal};
use crate::{model, update, winio};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, vec2};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vigem_client::{XButtons, XGamepad};

pub const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\padmimic";
const TITLE: &str = "padmimic setup";
/// Everything a quarter smaller than laid out (the app itself defaults to a third smaller).
const ZOOM: f32 = 0.75;
const NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode { Install, Uninstall }

pub fn mode() -> Option<Mode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--uninstall") { return Some(Mode::Uninstall) }
    let name = std::env::current_exe().ok()?.file_stem()?.to_string_lossy().to_lowercase();
    (name.contains("setup") || args.iter().any(|a| a == "--install")).then_some(Mode::Install)
}

fn default_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Programs").join("padmimic")
}

fn reg_value(name: &str) -> Option<String> {
    let out = cmd("reg", &["query", KEY, "/v", name]).ok()?;
    out.lines().find(|l| l.trim_start().starts_with(name))?.split("REG_SZ").nth(1).map(|s| s.trim().to_string())
}

/// The folder for the UI: the user's own path stays out of screenshots.
fn show_dir(d: &str) -> String {
    let base = default_dir().parent().and_then(|p| p.parent()).map(|p| p.display().to_string()).unwrap_or_default();
    match d.strip_prefix(&base) { Some(rest) if !base.is_empty() => format!("%LOCALAPPDATA%{rest}"), _ => d.into() }
}

fn cmd(exe: &str, args: &[&str]) -> Result<String, String> {
    let o = std::process::Command::new(exe).args(args).creation_flags(NO_WINDOW).output().map_err(|e| e.to_string())?;
    if o.status.success() { Ok(String::from_utf8_lossy(&o.stdout).into_owned()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
}

fn ps(script: &str) -> Result<(), String> {
    match update::powershell(script)? {
        (0, _) => Ok(()),
        (c, out) => Err(format!("PowerShell {c} {}", out.trim())),
    }
}

/// Path for a single-quoted PowerShell string.
fn q(p: &Path) -> String {
    p.display().to_string().replace('\'', "''")
}

// ================================================================= the steps

#[derive(Clone, PartialEq)]
enum St { Wait, Run, Ok, Skip, Warn(String) }

#[derive(Default)]
struct Job { steps: Vec<(&'static str, St)>, done: bool, fatal: Option<String> }

#[derive(Clone)]
struct Opts { dir: String, start_menu: bool, desktop: bool, startup: bool, vigem: bool, wipe: bool, lang: usize }

/// Close other running copies of this exe (they save their settings on the way out): WM_CLOSE to
/// their windows, then wait for them. Native, because a PowerShell start costs seconds.
fn close_running(exe: &Path) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, WaitForSingleObject};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE};
    struct Find { want: String, me: u32, pids: Vec<u32> }
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> windows_sys::core::BOOL {
        unsafe {
            let f = &mut *(l as *mut Find);
            let mut pid = 0;
            GetWindowThreadProcessId(h, &mut pid);
            if pid == f.me || IsWindowVisible(h) == 0 { return 1 }
            let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if p.is_null() { return 1 }
            let mut buf = [0u16; 1024];
            let mut n = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(p, 0, buf.as_mut_ptr(), &mut n) != 0;
            CloseHandle(p);
            if ok && String::from_utf16_lossy(&buf[..n as usize]).to_lowercase() == f.want {
                PostMessageW(h, WM_CLOSE, 0, 0);
                if !f.pids.contains(&pid) { f.pids.push(pid) }
            }
            1
        }
    }
    let mut f = Find { want: exe.display().to_string().to_lowercase(), me: std::process::id(), pids: vec![] };
    unsafe {
        EnumWindows(Some(each), &mut f as *mut Find as LPARAM);
        for pid in f.pids {
            let p = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if !p.is_null() { WaitForSingleObject(p, 6000); CloseHandle(p); }
        }
    }
    Ok(())
}

fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
    unsafe {
        let mut p: *mut u16 = std::ptr::null_mut();
        if SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p) != 0 { return None }
        let len = (0..).take_while(|&i| *p.add(i) != 0).count();
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        CoTaskMemFree(p as _);
        Some(PathBuf::from(s))
    }
}

fn copy_exe(dir: &Path, exe: &Path) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if me.canonicalize().ok() == exe.canonicalize().ok() { return Ok(()) }
    if exe.exists() {
        // a copy that did not close keeps running from the renamed file; update::cleanup removes it later
        let old = exe.with_file_name("padmimic.old.exe");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).map_err(|e| e.to_string())?;
    }
    std::fs::copy(&me, exe).map(|_| ()).map_err(|e| e.to_string())
}

/// Start menu / desktop links: removed natively, created through the shell's COM object.
fn shortcuts(exe: &Path, start_menu: bool, desktop: bool) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs};
    let mut make = String::new();
    for (on, id) in [(start_menu, &FOLDERID_Programs), (desktop, &FOLDERID_Desktop)] {
        let Some(lnk) = known_folder(id).map(|f| f.join("padmimic.lnk")) else { continue };
        if on {
            make += &format!(
                "$l = $w.CreateShortcut('{}'); $l.TargetPath = '{e}'; $l.WorkingDirectory = '{d}'; $l.IconLocation = '{e},0'; $l.Description = 'Controller remapper'; $l.Save()
",
                q(&lnk), e = q(exe), d = q(exe.parent().unwrap_or(exe)),
            );
        } else {
            let _ = std::fs::remove_file(lnk);
        }
    }
    if make.is_empty() { return Ok(()) }
    ps(&format!("$w = New-Object -ComObject WScript.Shell
{make}"))
}

/// Autostart plus the installer's language, so the app does not ask again.
fn write_settings(dir: &Path, startup: bool, lang: usize) -> Result<(), String> {
    let f = dir.join(model::SETTINGS_FILE);
    let mut v: serde_json::Value = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if !v.is_object() { v = serde_json::json!({}) }
    v["startup"] = startup.into();
    v["lang"] = i18n::LANGS[lang].0.into();
    std::fs::write(&f, serde_json::to_string_pretty(&v).unwrap()).map_err(|e| e.to_string())
}

fn register(dir: &Path, exe: &Path) -> Result<(), String> {
    let e = exe.display().to_string();
    let d = dir.display().to_string();
    let size = (std::fs::metadata(exe).map(|m| m.len()).unwrap_or(0) / 1024).to_string();
    let uninstall = format!("\"{e}\" --uninstall");
    let url = update::repository_url().unwrap_or_default();
    for (name, ty, val) in [
        ("DisplayName", "REG_SZ", "padmimic"),
        ("DisplayVersion", "REG_SZ", env!("CARGO_PKG_VERSION")),
        ("Publisher", "REG_SZ", "padmimic"),
        ("DisplayIcon", "REG_SZ", &e),
        ("InstallLocation", "REG_SZ", &d),
        ("UninstallString", "REG_SZ", &uninstall),
        ("URLInfoAbout", "REG_SZ", &url),
        ("EstimatedSize", "REG_DWORD", &size),
        ("NoModify", "REG_DWORD", "1"),
        ("NoRepair", "REG_DWORD", "1"),
    ] {
        cmd("reg", &["add", KEY, "/v", name, "/t", ty, "/d", val, "/f"])?;
    }
    Ok(())
}

/// After a self-update: the Windows apps list shows the new version.
pub fn refresh_version(dir: &Path) {
    let _ = ps(&format!(
        "$k = 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\padmimic'; if ((Get-ItemProperty $k -ErrorAction SilentlyContinue).InstallLocation -eq '{}') {{ Set-ItemProperty $k DisplayVersion '{}' }}",
        q(dir), env!("CARGO_PKG_VERSION")
    ));
}

/// The running exe can't delete itself: a hidden PowerShell waits until this process is gone.
/// Only padmimic's own files go; a folder the user picked keeps everything else.
fn schedule_delete(dir: &Path, wipe: bool) -> Result<(), String> {
    let data = if wipe { "Remove-Item -LiteralPath \"$d\\settings.json\", \"$d\\profiles\" -Recurse -Force -ErrorAction SilentlyContinue" } else { "" };
    let script = format!(
        "Wait-Process -Id {pid} -ErrorAction SilentlyContinue
$d = '{d}'
for ($i = 0; $i -lt 20 -and (Test-Path -LiteralPath \"$d\\padmimic.exe\"); $i++) {{ Remove-Item -LiteralPath \"$d\\padmimic.exe\" -Force -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 300 }}
Remove-Item -LiteralPath \"$d\\padmimic.old.exe\", \"$d\\padmimic.new.exe\" -Force -ErrorAction SilentlyContinue
{data}
if (-not (Get-ChildItem -LiteralPath $d -Force -ErrorAction SilentlyContinue)) {{ Remove-Item -LiteralPath $d -Force -ErrorAction SilentlyContinue }}",
        pid = std::process::id(),
        d = q(dir),
    );
    // a child outlives its parent on Windows; but leave a job object if this process runs in one (a
    // job could take the helper down with us). A job that forbids breaking away refuses the first try.
    // No DETACHED_PROCESS: PowerShell without any console quits at once.
    let spawn = |flags: u32| std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .current_dir(std::env::temp_dir())
        .creation_flags(flags)
        .spawn();
    spawn(NO_WINDOW | 0x0100_0000).or_else(|_| spawn(NO_WINDOW)).map(|_| ()).map_err(|e| e.to_string())
}

fn run_job(mode: Mode, o: Opts, job: Arc<Mutex<Job>>, ctx: egui::Context) {
    let dir = PathBuf::from(o.dir.trim());
    let exe = dir.join("padmimic.exe");
    let labels: Vec<&'static str> = match mode {
        Mode::Install => vec!["Closing running padmimic", "Copying padmimic.exe", "Creating shortcuts", "Setting up autostart", "Adding to Windows apps", "Install ViGEmBus driver"],
        Mode::Uninstall => vec!["Closing running padmimic", "Removing shortcuts", "Removing autostart", "Removing from Windows apps", "Removing files"],
    };
    *job.lock().unwrap() = Job { steps: labels.iter().map(|&l| (l, St::Wait)).collect(), ..Default::default() };
    let set = |i: usize, s: St| {
        job.lock().unwrap().steps[i].1 = s;
        ctx.request_repaint();
    };
    // each step runs, then shows at least a moment, so the list reads like a terminal log
    let step = |i: usize, f: &mut dyn FnMut() -> Result<St, String>| -> bool {
        set(i, St::Run);
        let t = std::time::Instant::now();
        let r = f();
        std::thread::sleep(Duration::from_millis(220).saturating_sub(t.elapsed()));
        match r {
            Ok(s) => { set(i, s); true }
            Err(e) => { set(i, St::Warn(e)); false }
        }
    };
    let ok = |r: Result<(), String>| r.map(|_| St::Ok);
    match mode {
        Mode::Install => {
            step(0, &mut || ok(close_running(&exe)));
            if !step(1, &mut || ok(copy_exe(&dir, &exe))) {
                let mut j = job.lock().unwrap();
                j.fatal = Some(match &j.steps[1].1 { St::Warn(e) => e.clone(), _ => String::new() });
                j.done = true;
                return;
            }
            step(2, &mut || ok(shortcuts(&exe, o.start_menu, o.desktop)));
            step(3, &mut || {
                winio::set_startup_for(o.startup, &exe);
                write_settings(&dir, o.startup, o.lang).map(|_| if o.startup { St::Ok } else { St::Skip })
            });
            step(4, &mut || ok(register(&dir, &exe)));
            step(5, &mut || {
                if !o.vigem { return Ok(St::Skip) }
                match winio::install_vigem() {
                    Ok(true) => Ok(St::Warn(tr("Restart Windows to finish the ViGEmBus setup").into())),
                    Ok(false) => Ok(St::Ok),
                    Err(e) => Err(tr(&e).into()),
                }
            });
        }
        Mode::Uninstall => {
            step(0, &mut || ok(close_running(&exe)));
            step(1, &mut || ok(shortcuts(&exe, false, false)));
            step(2, &mut || { winio::set_startup_for(false, &exe); Ok(St::Ok) });
            step(3, &mut || { let _ = cmd("reg", &["delete", KEY, "/f"]); Ok(St::Ok) });
            step(4, &mut || ok(schedule_delete(&dir, o.wipe)));
        }
    }
    job.lock().unwrap().done = true;
    ctx.request_repaint();
}

// ================================================================= the window

#[derive(Clone, Copy, PartialEq)]
enum Page { Welcome, Options, Work, Done }

pub struct Setup {
    mode: Mode,
    page: Page,
    o: Opts,
    job: Arc<Mutex<Job>>,
    vigem_missing: bool,
    installed: Option<String>, // version already installed
    launch: bool,
    lang_open: bool,
    sticks: [model::Stick; 2],
    trail: ui::Trail,
    rounded: bool,
    shown: f32, // progress bar, eased
}

pub fn run(mode: Mode) -> eframe::Result {
    let dir = match mode {
        Mode::Install => reg_value("InstallLocation").map(PathBuf::from).unwrap_or_else(default_dir),
        Mode::Uninstall => std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(default_dir),
    };
    // language: the installed app's, else where the user is (dev hook: PADMIMIC_LANG)
    let saved = std::fs::read_to_string(dir.join(model::SETTINGS_FILE)).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["lang"].as_str().and_then(i18n::id_of));
    let lang = std::env::var("PADMIMIC_LANG").ok().and_then(|c| i18n::id_of(&c)).or(saved).or_else(i18n::system).unwrap_or(0);
    i18n::set(lang);
    let installed = reg_value("DisplayVersion").filter(|_| dir.join("padmimic.exe").exists());
    let vigem_missing = mode == Mode::Install && vigem_client::Client::connect().is_err();
    let app = Setup {
        mode,
        // dev hooks for screenshots and tests: PADMIMIC_SHOW=options opens that page, =auto starts right away
        page: if std::env::var("PADMIMIC_SHOW").is_ok_and(|v| v == "options") { Page::Options } else { Page::Welcome },
        o: Opts { dir: dir.display().to_string(), start_menu: true, desktop: true, startup: false, vigem: vigem_missing, wipe: false, lang },
        job: Arc::default(),
        vigem_missing,
        installed,
        launch: true,
        lang_open: false,
        sticks: Default::default(),
        trail: Default::default(),
        rounded: false,
        shown: 0.0,
    };
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title(TITLE).with_inner_size([860.0 * ZOOM, 480.0 * ZOOM])
            .with_resizable(false).with_maximize_button(false).with_decorations(false).with_transparent(true),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(TITLE, opts, Box::new(|cc| {
        ui::style(&cc.egui_ctx);
        cc.egui_ctx.set_zoom_factor(ZOOM);
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark); // the setup is always dark, like the app's default
        Ok(Box::new(app))
    }))
}

/// A controller that plays by itself: sticks circle, buttons take turns; coral is padmimic's output.
pub(crate) fn demo(t: f64, busy: bool) -> (XGamepad, XGamepad) {
    let t = t as f32 * if busy { 2.4 } else { 1.0 };
    let s = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0) as i16;
    let seq = [XButtons::A, XButtons::X, XButtons::B, XButtons::Y, XButtons::RB, XButtons::UP, XButtons::RIGHT, XButtons::LB];
    let k = (t * 1.6) as usize;
    let g = XGamepad {
        buttons: XButtons { raw: seq[k % seq.len()] },
        left_trigger: ((t * 1.1).sin().max(0.0) * 255.0) as u8,
        right_trigger: ((t * 0.8 + 2.0).sin().max(0.0) * 255.0) as u8,
        thumb_lx: s(0.78 * (t * 1.3).cos()),
        thumb_ly: s(0.78 * (t * 1.3).sin()),
        thumb_rx: s(0.62 * (t * 0.9).sin()),
        thumb_ry: s(0.62 * (t * 1.8).sin()),
    };
    // output: a curved copy of the left stick and a second button on every other beat
    let curve = |v: i16| s((v as f32 / 32767.0).powi(3) * 1.6);
    let v = XGamepad {
        buttons: XButtons { raw: if k % 2 == 0 { seq[(k + 3) % seq.len()] } else { 0 } },
        thumb_lx: curve(g.thumb_lx),
        thumb_ly: curve(g.thumb_ly),
        ..Default::default()
    };
    (g, v)
}

// ---------------------------------------------------------------- widgets

fn primary(ui: &mut egui::Ui, text: &str, col: Color32) -> bool {
    let font = ui::bold(14.5);
    let w = ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE).size().x).max(96.0) + 44.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 42.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 21.0, if resp.hovered() { col.lerp_to_gamma(Color32::WHITE, 0.12) } else { col });
    p.text(rect.center(), Align2::CENTER_CENTER, text, font, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn secondary(ui: &mut egui::Ui, text: &str) -> bool {
    let c = pal(true);
    let font = ui::bold(14.5);
    let w = ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font.clone(), c.text).size().x) + 40.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 42.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 21.0, if resp.hovered() { c.hover } else { c.idle });
    p.text(rect.center(), Align2::CENTER_CENTER, text, font, c.text);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Option row: title (and hint under it), pill switch on the right; the whole row is clickable.
fn toggle(ui: &mut egui::Ui, on: &mut bool, title: &str, hint: &str) {
    let c = pal(true);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), if hint.is_empty() { 40.0 } else { 52.0 }), Sense::click());
    if resp.clicked() { *on = !*on }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let p = ui.painter();
    if resp.hovered() { p.rect_filled(rect, 10.0, c.idle); }
    let pill = Rect::from_min_size(Pos2::new(rect.right() - 54.0, rect.center().y - 12.0), vec2(44.0, 24.0));
    p.rect_filled(pill, 12.0, c.hover.lerp_to_gamma(c.acc, t));
    p.circle_filled(Pos2::new(egui::lerp(pill.left() + 12.0..=pill.right() - 12.0, t), pill.center().y), 9.0, Color32::WHITE);
    let x = rect.left() + 10.0;
    let y = if hint.is_empty() { rect.center().y } else { rect.center().y - 9.0 };
    p.text(Pos2::new(x, y), Align2::LEFT_CENTER, title, FontId::proportional(14.5), c.text);
    if !hint.is_empty() { p.text(Pos2::new(x, y + 19.0), Align2::LEFT_CENTER, hint, FontId::proportional(11.5), c.weak); }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
}

fn kicker(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(ui::bold(13.0)).color(pal(true).acc));
}

fn headline(ui: &mut egui::Ui, text: &str, col: Color32) {
    ui.label(RichText::new(text).font(ui::bold(23.0)).color(col));
}

fn info_row(ui: &mut egui::Ui, key: &str, val: &str) {
    let c = pal(true);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(96.0, 18.0), Sense::hover());
        ui.painter().text(r.left_center(), Align2::LEFT_CENTER, key, FontId::proportional(12.5), c.weak);
        ui.label(RichText::new(val).size(12.5).color(c.text));
    });
}

impl Setup {
    fn start(&mut self, ctx: &egui::Context) {
        self.page = Page::Work;
        self.shown = 0.0;
        let (mode, o, job, ctx) = (self.mode, self.o.clone(), self.job.clone(), ctx.clone());
        *job.lock().unwrap() = Job::default();
        std::thread::spawn(move || run_job(mode, o, job, ctx));
    }

    fn backdrop(&self, p: &egui::Painter, r: Rect) {
        p.rect_filled(r, 0.0, pal(true).panel);
    }

    fn title_bar(&mut self, ui: &mut egui::Ui, r: Rect) {
        let c = pal(true);
        let bar = Rect::from_min_size(r.min, vec2(r.width(), 56.0));
        let drag = ui.interact(bar, ui.id().with("drag"), Sense::click_and_drag());
        if drag.drag_started_by(egui::PointerButton::Primary) { ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag) }
        let p = ui.painter();
        let logo = Rect::from_min_size(Pos2::new(r.left() + 28.0, bar.center().y - 15.0), vec2(30.0, 30.0));
        crate::icons::logo(p, logo, c.text, 1.0);
        let name = p.text(Pos2::new(logo.right() + 10.0, bar.center().y), Align2::LEFT_CENTER, "padmimic", ui::bold(18.0), c.text);
        let label = tr(if self.mode == Mode::Install { "Setup" } else { "Uninstall" });
        let lr = p.text(Pos2::new(name.right() + 20.0, bar.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.0), c.weak);
        p.rect_filled(Rect::from_min_max(Pos2::new(name.right() + 10.0, bar.center().y - 11.0), Pos2::new(lr.right() + 10.0, bar.center().y + 11.0)), 11.0, c.idle);
        p.text(Pos2::new(name.right() + 20.0, bar.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.0), c.weak);
        let right = Rect::from_min_max(Pos2::new(r.right() - 330.0, bar.top() + 10.0), Pos2::new(r.right() - 14.0, bar.bottom() - 10.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
        if ui::win_btn(&mut ui, WinBtn::Close) { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close) }
        if ui::win_btn(&mut ui, WinBtn::Min) { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true)) }
        ui.add_space(6.0);
        if self.page != Page::Work {
            let b = egui::Button::new(RichText::new(format!("Aあ  {}", i18n::native(i18n::cur()))).size(13.0).color(c.text)).fill(c.idle);
            if ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { self.lang_open = true }
        }
    }

    /// (1) Welcome — (2) Options — (3) Install
    fn steps_bar(&self, p: &egui::Painter, left: f32, y: f32) {
        let c = pal(true);
        let names: &[&str] = if self.mode == Mode::Install { &["Welcome", "Options", "Install"] } else { &["Uninstall", "Done"] };
        let cur = match (self.mode, self.page) {
            (Mode::Install, Page::Welcome) => 0,
            (Mode::Install, Page::Options) => 1,
            (Mode::Install, _) => 2,
            (_, Page::Done) => 1,
            _ => 0,
        };
        let mut x = left;
        for (i, n) in names.iter().enumerate() {
            let dot = Pos2::new(x + 10.0, y);
            p.circle_filled(dot, 10.0, if i <= cur { c.acc } else { c.idle });
            p.text(dot, Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(11.5), if i <= cur { Color32::WHITE } else { c.weak });
            let r = p.text(Pos2::new(dot.x + 18.0, y), Align2::LEFT_CENTER, tr(n), FontId::proportional(13.0), if i == cur { c.text } else { c.weak });
            x = r.right() + 12.0;
            if i + 1 < names.len() {
                p.hline(x..=x + 28.0, y, Stroke::new(1.5, if i < cur { c.acc } else { c.line }));
                x += 40.0;
            }
        }
    }

    fn demo_panel(&mut self, ui: &mut egui::Ui, r: Rect, t: f64) {
        let c = pal(true);
        ui.painter().rect_filled(r, 22.0, c.card);
        let busy = self.page == Page::Work;
        let (g, v) = if self.page == Page::Done { (XGamepad { buttons: XButtons { raw: XButtons::A }, ..Default::default() }, XGamepad::default()) } else { demo(t, busy) };
        let inner = r.shrink2(vec2(14.0, 30.0)).translate(vec2(0.0, 10.0));
        let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        ui::controller(&mut cu, (t / 4.0) as u8 % crate::pad::MODELS.len() as u8, &g, Some(&v), &self.sticks, &mut self.trail);
        let p = ui.painter();
        p.text(Pos2::new(r.left() + 18.0, r.top() + 20.0), Align2::LEFT_CENTER, tr("Live preview"), FontId::proportional(12.5), c.weak);
        let dot = Pos2::new(r.right() - 22.0, r.top() + 20.0);
        let pulse = if busy { 1.0 } else { 0.55 + 0.45 * (t * 2.5).sin().abs() as f32 };
        p.circle_filled(dot, 4.0, c.acc.gamma_multiply(pulse));
        let mut x = r.left() + 18.0;
        for tag in ["8000 Hz", "keys · mouse", "Xbox 360", "MIT"] {
            let galley = p.layout_no_wrap(tag.into(), FontId::proportional(11.5), c.weak);
            let pill = Rect::from_min_size(Pos2::new(x, r.bottom() - 36.0), vec2(galley.size().x + 20.0, 22.0));
            p.rect_filled(pill, 11.0, c.idle);
            p.galley(Pos2::new(pill.left() + 10.0, pill.center().y - galley.size().y / 2.0), galley, c.weak);
            x = pill.right() + 6.0;
        }
    }

    fn page_welcome(&mut self, ui: &mut egui::Ui) {
        let c = pal(true);
        let ctx = ui.ctx().clone();
        if self.mode == Mode::Uninstall {
            kicker(ui, tr("Uninstall"));
            ui.add_space(6.0);
            headline(ui, tr("Remove padmimic?"), c.text);
            ui.add_space(6.0);
            ui.label(RichText::new(tr("Shortcuts, autostart and the entry in Windows apps are removed.")).size(14.5).color(c.weak));
            ui.add_space(18.0);
            toggle(ui, &mut self.o.wipe, tr("Also delete my profiles and settings"), tr("Otherwise they stay in the folder"));
            ui.add_space(14.0);
            info_row(ui, tr("Folder"), &show_dir(&self.o.dir));
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.horizontal(|ui| {
                    if primary(ui, tr("Remove"), c.amber) { self.start(&ctx) }
                    ui.add_space(4.0);
                    if secondary(ui, tr("Cancel")) { ctx.send_viewport_cmd(egui::ViewportCommand::Close) }
                });
            });
            return;
        }
        kicker(ui, "Controller remapper for Windows");
        ui.add_space(6.0);
        headline(ui, tr("Your controller."), Color32::from_rgb(242, 247, 245));
        headline(ui, tr("Plus keys, mouse & a virtual pad."), c.acc);
        ui.add_space(8.0);
        ui.label(RichText::new(tr("The real gamepad goes straight to the game – padmimic adds what you map on top. Built for low latency.")).size(13.5).color(c.weak));
        ui.add_space(10.0);
        info_row(ui, tr("Install to"), &show_dir(&self.o.dir));
        let v = env!("CARGO_PKG_VERSION");
        let ver = match &self.installed { Some(old) => trf("{} · installed: {}", &[&v, old]), None => v.to_string() };
        info_row(ui, tr("Version"), &ver);
        info_row(ui, "", tr("No admin rights needed · MIT License"));
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                let go = match &self.installed { Some(old) if update::newer(v, old) => "Update", Some(_) => "Reinstall", None => "Install" };
                if primary(ui, tr(go), c.acc) { self.start(&ctx) }
                ui.add_space(4.0);
                if secondary(ui, tr("Options")) { self.page = Page::Options }
            });
        });
    }

    fn page_options(&mut self, ui: &mut egui::Ui) {
        let c = pal(true);
        let ctx = ui.ctx().clone();
        kicker(ui, tr("Options"));
        ui.add_space(8.0);
        toggle(ui, &mut self.o.start_menu, tr("Start menu shortcut"), "");
        toggle(ui, &mut self.o.desktop, tr("Desktop shortcut"), "");
        toggle(ui, &mut self.o.startup, tr("Start with Windows"), "");
        if self.vigem_missing {
            toggle(ui, &mut self.o.vigem, tr("Install ViGEmBus driver"), tr("Needed for the virtual Xbox 360 controller · asks for admin rights once"));
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Folder")).size(12.5).color(c.weak));
            if ui.add(egui::Label::new(RichText::new(tr("Default")).size(12.5).color(c.acc)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.o.dir = default_dir().display().to_string();
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.o.dir).font(FontId::proportional(13.0)).desired_width(f32::INFINITY).margin(vec2(10.0, 9.0)));
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                if primary(ui, tr(if self.installed.is_some() { "Reinstall" } else { "Install" }), c.acc) && !self.o.dir.trim().is_empty() { self.start(&ctx) }
                ui.add_space(4.0);
                if secondary(ui, tr("Back")) { self.page = Page::Welcome }
            });
        });
    }

    fn page_work(&mut self, ui: &mut egui::Ui, t: f64) {
        let c = pal(true);
        let (steps, done) = { let j = self.job.lock().unwrap(); (j.steps.clone(), j.done) };
        let n = steps.len().max(1) as f32;
        let finished = steps.iter().filter(|s| !matches!(s.1, St::Wait | St::Run)).count() as f32;
        let running = steps.iter().any(|s| s.1 == St::Run) as u8 as f32 * 0.5;
        let target = if done { 1.0 } else { (finished + running) / n };
        self.shown += (target - self.shown) * 0.12;
        kicker(ui, tr(if self.mode == Mode::Install { "Installing" } else { "Removing" }));
        ui.add_space(2.0);
        ui.label(RichText::new(format!("{}%", (self.shown * 100.0).round() as u32)).font(ui::bold(44.0)).color(c.text));
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(bar, 3.0, c.idle);
        p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * self.shown, bar.height())), 3.0, c.acc);
        ui.add_space(14.0);
        for (label, st) in &steps {
            ui.horizontal(|ui| {
                // status dot: ring = waiting, pulsing = running, filled = done, coral = warning
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
                let (m, p) = (r.center(), ui.painter());
                match st {
                    St::Wait | St::Skip => { p.circle_stroke(m, 5.0, Stroke::new(1.5, c.line)); }
                    St::Run => { p.circle_filled(m, 5.0, c.acc.gamma_multiply(0.45 + 0.55 * (t * 6.0).sin().abs() as f32)); }
                    St::Ok => { p.circle_filled(m, 5.0, c.acc); }
                    St::Warn(_) => { p.circle_filled(m, 5.0, c.amber); }
                }
                let text_col = if matches!(st, St::Wait | St::Skip) { c.weak } else { c.text };
                ui.label(RichText::new(tr(label)).size(13.5).color(text_col));
            });
            if let St::Warn(e) = st { ui.label(RichText::new(format!("  {e}")).size(11.5).color(c.amber)); }
        }
    }

    fn page_done(&mut self, ui: &mut egui::Ui) {
        let c = pal(true);
        let ctx = ui.ctx().clone();
        let (fatal, warns) = {
            let j = self.job.lock().unwrap();
            (j.fatal.clone(), j.steps.iter().filter_map(|(l, s)| if let St::Warn(e) = s { Some(format!("{} – {e}", tr(l))) } else { None }).collect::<Vec<_>>())
        };
        if let Some(e) = fatal {
            kicker(ui, tr("Install"));
            ui.add_space(6.0);
            headline(ui, tr("Installation failed"), c.amber);
            ui.add_space(6.0);
            ui.label(RichText::new(e).size(12.5).color(c.weak));
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.horizontal(|ui| {
                    if primary(ui, tr("Try again"), c.acc) { self.start(&ctx) }
                    ui.add_space(4.0);
                    if secondary(ui, tr("Close")) { ctx.send_viewport_cmd(egui::ViewportCommand::Close) }
                });
            });
            return;
        }
        kicker(ui, tr("Done"));
        ui.add_space(6.0);
        if self.mode == Mode::Uninstall {
            headline(ui, tr("padmimic was removed."), c.text);
            ui.add_space(6.0);
            ui.label(RichText::new(tr("Thanks for trying it.")).size(14.5).color(c.weak));
        } else {
            headline(ui, tr("padmimic is installed."), c.text);
            ui.add_space(6.0);
            ui.label(RichText::new(tr("Find it in the Start menu. Turn it on with F8 or the switch at the top right.")).size(14.5).color(c.weak));
            ui.add_space(16.0);
            toggle(ui, &mut self.launch, tr("Launch padmimic now"), "");
        }
        for w in warns {
            ui.label(RichText::new(format!("! {w}")).size(12.0).color(c.amber));
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            let label = if self.mode == Mode::Install { "Finish" } else { "Close" };
            if primary(ui, tr(label), c.acc) {
                if self.mode == Mode::Install && self.launch {
                    let dir = PathBuf::from(self.o.dir.trim());
                    let _ = std::process::Command::new(dir.join("padmimic.exe")).current_dir(&dir).spawn();
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn lang_modal(&mut self, ctx: &egui::Context) {
        if !self.lang_open { return }
        let c = pal(true);
        let r = egui::Modal::new(egui::Id::new("setup_lang")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new(tr("Language")).font(ui::bold(17.0)).color(c.text));
            ui.add_space(6.0);
            ui::lang_grid(ui, i18n::cur(), &[])
        });
        if let Some(l) = r.inner {
            i18n::set(l);
            self.o.lang = l;
            ui::set_fonts(ctx);
            self.lang_open = false;
        }
        if r.should_close() { self.lang_open = false }
    }
}

impl eframe::App for Setup {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.rounded {
            self.rounded = true;
            crate::winio::round_corners(TITLE);
            // same glass as the app: Acrylic behind see-through surfaces (solid on Windows 10)
            let glass = crate::winio::set_backdrop(TITLE, 3, true);
            ui::set_look(0, if glass { 0.55 } else { 1.0 }, true);
            ui::apply_colors(&ctx);
            if std::env::var("PADMIMIC_SHOW").is_ok_and(|v| v == "auto") { self.start(&ctx) }
        }
        ctx.request_repaint_after(Duration::from_millis(16)); // the demo controller moves
        if self.page == Page::Work && self.job.lock().unwrap().done && self.shown > 0.985 { self.page = Page::Done }
        let t = ctx.input(|i| i.time);
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
            let r = ctx.content_rect();
            self.backdrop(ui.painter(), r);
            self.title_bar(ui, r);
            self.steps_bar(ui.painter(), r.left() + 28.0, r.top() + 76.0);
            let demo = Rect::from_min_max(Pos2::new(r.right() - 408.0, r.top() + 92.0), Pos2::new(r.right() - 28.0, r.bottom() - 28.0));
            self.demo_panel(ui, demo, t);
            let left = Rect::from_min_max(Pos2::new(r.left() + 28.0, r.top() + 104.0), Pos2::new(demo.left() - 36.0, r.bottom() - 28.0));
            let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(left).layout(egui::Layout::top_down(egui::Align::Min)));
            match self.page {
                Page::Welcome => self.page_welcome(&mut cu),
                Page::Options => self.page_options(&mut cu),
                Page::Work => self.page_work(&mut cu, t),
                Page::Done => self.page_done(&mut cu),
            }
            ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("edge")))
                .rect_stroke(r, 0.0, Stroke::new(1.0, pal(true).line), StrokeKind::Inside);
        });
        self.lang_modal(&ctx);
    }
}
