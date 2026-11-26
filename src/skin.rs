//! Your own controller skins, loaded from `skins/<name>/skin.json` next to padmimic.exe.
//! User skins remain separate from the embedded controller catalog in pad.rs.
//!
//! skin.json:
//! ```json
//! { "name": "My pad", "size": [800, 560],
//!   "parts": [
//!     { "idle": "body.png" , "cx": 400, "cy": 280, "w": 800, "h": 560 },
//!     { "idle": "a.png", "active": "a_on.png", "cx": 610, "cy": 250, "w": 44, "h": 44, "button": 0 },
//!     { "idle": "lstick.png", "cx": 230, "cy": 220, "w": 90, "h": 90, "stick": 0, "travel": 22 }
//!   ] }
//! ```
//! Parts are drawn in list order (first = back). Coordinates are pixels in the `size` canvas,
//! (cx, cy) is the centre, `rot` turns the part in degrees, `flip` mirrors it. `button` uses the
//! standard gamepad numbering: 0 A, 1 B, 2 X, 3 Y, 4 LB, 5 RB, 6 LT, 7 RT, 8 Back, 9 Start,
//! 10 L3, 11 R3, 12 up, 13 down, 14 left, 15 right, 16 Guide. A pressed part shows `active`, or
//! glows in the accent colour when it has none. `stick` 0 / 1 moves the part with that stick by
//! up to `travel` pixels. Images: PNG, JPEG, BMP or GIF (decoded by Windows' own GDI+).

use crate::ui::pal;
use eframe::egui::{self, Color32, Mesh, Painter, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use vigem_client::{XButtons, XGamepad};

#[derive(Deserialize)]
struct File { #[serde(default)] name: String, size: [f32; 2], parts: Vec<PartFile> }

#[derive(Deserialize, Clone)]
struct PartFile {
    idle: String,
    #[serde(default)] active: Option<String>,
    cx: f32, cy: f32, w: f32, h: f32,
    #[serde(default)] rot: f32,
    #[serde(default)] flip: bool,
    #[serde(default)] button: Option<u8>,
    #[serde(default)] stick: Option<u8>,
    #[serde(default)] travel: f32,
}

#[derive(Clone)]
struct Part { f: PartFile, idle: TextureHandle, active: Option<TextureHandle> }

#[derive(Clone)]
pub struct Skin { pub id: String, pub name: String, size: [f32; 2], parts: Vec<Part> }

/// The skins folder next to the exe (created on demand).
pub fn dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("skins"))).unwrap_or_else(|| "skins".into())
}

/// Every skin in the folder; broken ones come back as "folder: reason".
pub fn scan(ctx: &egui::Context) -> (Vec<Skin>, Vec<String>) {
    let (mut ok, mut bad) = (vec![], vec![]);
    let Ok(rd) = std::fs::read_dir(dir()) else { return (ok, bad) };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.join("skin.json").is_file()).collect();
    dirs.sort();
    for d in dirs {
        let id = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match load(ctx, &d, &id) {
            Ok(s) => ok.push(s),
            Err(e) => bad.push(format!("{id}: {e}")),
        }
    }
    (ok, bad)
}

fn load(ctx: &egui::Context, d: &Path, id: &str) -> Result<Skin, String> {
    let text = std::fs::read_to_string(d.join("skin.json")).map_err(|e| e.to_string())?;
    let f: File = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if f.size[0] <= 0.0 || f.size[1] <= 0.0 { return Err("size must be > 0".into()) }
    let tex = |name: &str| -> Result<TextureHandle, String> {
        // only files inside the skin's own folder
        let p = d.join(name);
        if Path::new(name).components().any(|c| !matches!(c, std::path::Component::Normal(_))) { return Err(format!("{name}: path leaves the skin folder")) }
        let (w, h, rgba) = decode(&p).ok_or_else(|| format!("{name}: cannot read image"))?;
        Ok(ctx.load_texture(format!("skin/{id}/{name}"), egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba), TextureOptions::LINEAR))
    };
    let mut parts = vec![];
    for pf in f.parts {
        let idle = tex(&pf.idle)?;
        let active = pf.active.as_deref().filter(|a| !a.is_empty()).map(tex).transpose()?;
        parts.push(Part { f: pf, idle, active });
    }
    Ok(Skin { id: id.into(), name: if f.name.is_empty() { id.into() } else { f.name }, size: f.size, parts })
}

/// Image file → (width, height, RGBA) through GDI+.
pub fn decode(path: &Path) -> Option<(usize, usize, Vec<u8>)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Graphics::GdiPlus::*;
    const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x0026_200A;
    static TOKEN: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    TOKEN.get_or_init(|| {
        let mut t = 0usize;
        let input = GdiplusStartupInput { GdiplusVersion: 1, DebugEventCallback: 0, SuppressBackgroundThread: 0, SuppressExternalCodecs: 0 };
        unsafe { GdiplusStartup(&mut t, &input, std::ptr::null_mut()) };
        t
    });
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut bmp: *mut GpBitmap = std::ptr::null_mut();
        if GdipCreateBitmapFromFile(wide.as_ptr(), &mut bmp) != Ok || bmp.is_null() { return None }
        let (mut w, mut h) = (0u32, 0u32);
        GdipGetImageWidth(bmp as *mut GpImage, &mut w);
        GdipGetImageHeight(bmp as *mut GpImage, &mut h);
        let mut out = None;
        if w > 0 && h > 0 && w <= 8192 && h <= 8192 {
            let rect = Rect { X: 0, Y: 0, Width: w as i32, Height: h as i32 };
            let mut data = BitmapData::default();
            if GdipBitmapLockBits(bmp, &rect, ImageLockModeRead as u32, PIXEL_FORMAT_32BPP_ARGB, &mut data) == Ok {
                let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
                for y in 0..h as isize {
                    let row = std::slice::from_raw_parts((data.Scan0 as *const u8).offset(y * data.Stride as isize), w as usize * 4);
                    for px in row.chunks_exact(4) { rgba.extend([px[2], px[1], px[0], px[3]]) } // BGRA → RGBA
                }
                GdipBitmapUnlockBits(bmp, &mut data);
                out = Some((w as usize, h as usize, rgba));
            }
        }
        GdipDisposeImage(bmp as *mut GpImage);
        out
    }
}

/// Standard gamepad button number → is it down on this XInput state.
fn pressed(g: &XGamepad, b: u8) -> bool {
    let bit = match b {
        0 => XButtons::A, 1 => XButtons::B, 2 => XButtons::X, 3 => XButtons::Y, 4 => XButtons::LB, 5 => XButtons::RB,
        6 => return g.left_trigger > 30, 7 => return g.right_trigger > 30,
        8 => XButtons::BACK, 9 => XButtons::START, 10 => XButtons::LTHUMB, 11 => XButtons::RTHUMB,
        12 => XButtons::UP, 13 => XButtons::DOWN, 14 => XButtons::LEFT, 15 => XButtons::RIGHT, 16 => XButtons::GUIDE,
        _ => return false,
    };
    g.buttons.raw & bit != 0
}

impl Skin {
    pub fn aspect(&self) -> f32 {
        self.size[1] / self.size[0]
    }

    /// Draws the skin fitted into `r`.
    pub fn paint(&self, p: &Painter, r: Rect, g: &XGamepad) {
        let acc = pal(p.ctx().theme() == egui::Theme::Dark).acc;
        let k = (r.width() / self.size[0]).min(r.height() / self.size[1]);
        let o = r.center() - vec2(self.size[0], self.size[1]) * k / 2.0;
        for part in &self.parts {
            let f = &part.f;
            let mut c = o + vec2(f.cx, f.cy) * k;
            if let Some(s) = f.stick {
                let (x, y) = if s == 0 { (g.thumb_lx, g.thumb_ly) } else { (g.thumb_rx, g.thumb_ry) };
                c += vec2(x as f32, -(y as f32)) / 32767.0 * f.travel * k;
            }
            let down = f.button.is_some_and(|b| pressed(g, b));
            let tex = if down { part.active.as_ref().unwrap_or(&part.idle) } else { &part.idle };
            let tint = if down && part.active.is_none() { acc.lerp_to_gamma(Color32::WHITE, 0.35) } else { Color32::WHITE };
            p.add(quad(tex.id(), c, vec2(f.w, f.h) * k, f.rot, f.flip, tint));
        }
    }
}

/// Textured rectangle around `c`, turned by `rot` degrees, optionally mirrored.
fn quad(tex: egui::TextureId, c: Pos2, size: egui::Vec2, rot: f32, flip: bool, tint: Color32) -> Mesh {
    let mut m = Mesh::with_texture(tex);
    let (s, co) = rot.to_radians().sin_cos();
    let h = size / 2.0;
    for (dx, dy, u, v) in [(-1.0, -1.0, 0.0, 0.0), (1.0, -1.0, 1.0, 0.0), (1.0, 1.0, 1.0, 1.0), (-1.0, 1.0, 0.0, 1.0)] {
        let (x, y) = (dx * h.x, dy * h.y);
        let u = if flip { 1.0 - u } else { u };
        m.vertices.push(egui::epaint::Vertex { pos: c + vec2(x * co - y * s, x * s + y * co), uv: pos2(u, v), color: tint });
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    m
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_a_png() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/app.png");
        let (w, h, rgba) = super::decode(&p).expect("docs/app.png decodes");
        assert!(w > 0 && h > 0 && rgba.len() == w * h * 4);
        assert!(super::decode(std::path::Path::new("does-not-exist.png")).is_none());
    }

    #[test]
    fn loads_a_skin_folder_and_refuses_escaping_paths() {
        let d = std::env::temp_dir().join(format!("padmimic-skin-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::copy(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/app.png"), d.join("body.png")).unwrap();
        let ctx = eframe::egui::Context::default();
        std::fs::write(d.join("skin.json"), r#"{"name":"T","size":[100,70],"parts":[{"idle":"body.png","cx":50,"cy":35,"w":100,"h":70},{"idle":"body.png","cx":20,"cy":20,"w":10,"h":10,"button":0}]}"#).unwrap();
        let s = super::load(&ctx, &d, "t").expect("skin loads");
        assert_eq!((s.name.as_str(), s.parts.len()), ("T", 2));
        std::fs::write(d.join("skin.json"), r#"{"size":[1,1],"parts":[{"idle":"../x.png","cx":0,"cy":0,"w":1,"h":1}]}"#).unwrap();
        assert!(super::load(&ctx, &d, "t").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
