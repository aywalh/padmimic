//! UI languages. The English text is the key; `lang/<code>.json` maps it to the translation and a
//! missing entry falls back to English, so a new text never breaks a language.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// (code, name in its own language). The index is the language id; 0 = English.
pub const LANGS: [(&str, &str); 8] = [
    ("en", "English"), ("de", "Deutsch"), ("ja", "日本語"), ("zh", "简体中文"),
    ("ko", "한국어"), ("vi", "Tiếng Việt"), ("fil", "Filipino"), ("ar", "العربية"),
];

const FILES: [&str; 8] = [
    "{}",
    include_str!("../lang/de.json"),
    include_str!("../lang/ja.json"),
    include_str!("../lang/zh.json"),
    include_str!("../lang/ko.json"),
    include_str!("../lang/vi.json"),
    include_str!("../lang/fil.json"),
    include_str!("../lang/ar.json"),
];

pub const AR: usize = 7;

static CUR: AtomicUsize = AtomicUsize::new(0);
static TABLES: OnceLock<Vec<HashMap<&'static str, &'static str>>> = OnceLock::new();

fn tables() -> &'static [HashMap<&'static str, &'static str>] {
    TABLES.get_or_init(|| {
        FILES.iter().enumerate().map(|(i, f)| {
            let m: HashMap<String, String> = serde_json::from_str(f).unwrap_or_default();
            m.into_iter()
                .map(|(k, v)| {
                    let v = if i == AR { rtl(&v) } else { v };
                    (&*Box::leak(k.into_boxed_str()), &*Box::leak(v.into_boxed_str()))
                })
                .collect()
        }).collect()
    })
}

/// Current language id.
pub fn cur() -> usize {
    CUR.load(Relaxed)
}

pub fn set(id: usize) {
    CUR.store(id.min(LANGS.len() - 1), Relaxed);
}

pub fn id_of(code: &str) -> Option<usize> {
    LANGS.iter().position(|l| l.0 == code)
}

/// A language's own name, ready to draw (Arabic joined and right-to-left).
pub fn native(id: usize) -> String {
    if id == AR { rtl(LANGS[id].1) } else { LANGS[id].1.into() }
}

/// Text in the current language.
pub fn tr(en: &str) -> &str {
    tr_in(cur(), en)
}

/// Text in a given language (the first-start question is asked in the user's language).
pub fn tr_in(id: usize, en: &str) -> &str {
    tables().get(id).and_then(|t| t.get(en).copied()).unwrap_or(en)
}

/// Translated text with each `{}` replaced by the next argument.
pub fn trf(en: &str, args: &[&dyn Display]) -> String {
    let mut out = String::new();
    let mut parts = tr(en).split("{}");
    out.push_str(parts.next().unwrap_or(""));
    for (i, p) in parts.enumerate() {
        if let Some(a) = args.get(i) { out.push_str(&a.to_string()) }
        out.push_str(p);
    }
    out
}

/// Language of where the user is (Windows region, then the Windows display language). Only local
/// settings are read; nothing leaves the PC.
pub fn system() -> Option<usize> {
    use windows_sys::Win32::Globalization::{GetUserDefaultGeoName, GetUserDefaultLocaleName};
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultGeoName(buf.as_mut_ptr(), buf.len() as i32) };
    let region = if n > 1 { String::from_utf16_lossy(&buf[..n as usize - 1]) } else { String::new() };
    let by_region = match region.as_str() {
        "DE" | "AT" | "CH" | "LI" | "LU" => Some("de"),
        "JP" => Some("ja"),
        "CN" | "TW" | "HK" | "MO" | "SG" => Some("zh"),
        "KR" => Some("ko"),
        "VN" => Some("vi"),
        "PH" => Some("fil"),
        "SA" | "AE" | "EG" | "QA" | "KW" | "BH" | "OM" | "JO" | "LB" | "IQ" | "SY" | "YE" | "LY" | "TN" | "DZ" | "MA" | "SD" | "PS" => Some("ar"),
        _ => None,
    };
    if let Some(c) = by_region { return id_of(c) }
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    let locale = if n > 1 { String::from_utf16_lossy(&buf[..n as usize - 1]) } else { return None };
    let lang = locale.split('-').next().unwrap_or("");
    id_of(if lang == "tl" { "fil" } else { lang })
}

/// egui (HarfBuzz) joins Arabic letters and mirrors each Arabic word itself, but lays a line out
/// left to right with no bidi. So: put the line in visual right-to-left order here, then turn each
/// Arabic letter run back to logical order for the shaper. Needs a Latin font without Arabic glyphs
/// (see ui::set_fonts), so Arabic words reach the shaper as runs of their own.
fn rtl(s: &str) -> String {
    // ponytail: simplified bidi (strong R/L, neutrals between two L stay L, else R), enough for UI labels
    let chars: Vec<char> = s.chars().collect();
    let arabic = |c: char| matches!(c as u32, 0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF);
    let strong: Vec<Option<bool>> = chars.iter().map(|&c| if arabic(c) { Some(true) } else if c.is_alphanumeric() { Some(false) } else { None }).collect();
    let rtl_at = |i: usize| -> bool {
        strong[i].unwrap_or_else(|| {
            let prev = strong[..i].iter().rev().flatten().next();
            let next = strong[i + 1..].iter().flatten().next();
            !(prev == Some(&false) && next == Some(&false))
        })
    };
    let mut runs: Vec<(bool, String)> = vec![];
    for (i, &c) in chars.iter().enumerate() {
        let r = rtl_at(i);
        match runs.last_mut() {
            Some(run) if run.0 == r => run.1.push(c),
            _ => runs.push((r, c.to_string())),
        }
    }
    let mirror = |c: char| match c { '(' => ')', ')' => '(', '[' => ']', ']' => '[', '«' => '»', '»' => '«', c => c };
    let visual: String = runs.iter().rev().map(|(r, t)| if *r { t.chars().rev().map(mirror).collect() } else { t.clone() }).collect();
    let visual = visual.replace("}{", "{}"); // a mirrored placeholder
    // Arabic letter runs back to logical order: the shaper mirrors them again
    let mut out = String::new();
    let mut word = String::new();
    for c in visual.chars() {
        if arabic(c) { word.insert(0, c) } else { out.push_str(&std::mem::take(&mut word)); out.push(c) }
    }
    out + &word
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_format_and_rtl() {
        set(0);
        assert_eq!(tr("no such text"), "no such text");
        assert_eq!(trf("{} of {}", &[&1, &"x"]), "1 of x");
        // Latin and the placeholder keep their order, the Arabic run is mirrored
        // visual order: Latin and the placeholder on the left, Arabic words right, each word logical
        assert_eq!(rtl("ملف {} ViGEm"), "ViGEm {} ملف");
        assert_eq!(rtl("كلمة أخرى"), "أخرى كلمة");
        for f in FILES { assert!(serde_json::from_str::<HashMap<String, String>>(f).is_ok()) }
    }
}
