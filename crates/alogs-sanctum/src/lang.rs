//! The app's and the game's languages: translations of the app's text, the game's own wording of its terms (to read
//! the party screen and to show difficulties as the game names them), and the game's language setting. Port of
//! `Lang.cs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

/// The game's languages, in the order of its language setting (0 = zh_CN ...), with their own names.
pub const LANGUAGES: [(&str, &str); 13] = [
    ("zh_CN", "简体中文"), ("zh_TW", "繁體中文"), ("en", "English"), ("ko_KR", "한국어"), ("ja_JP", "日本語"),
    ("vi_VN", "Tiếng Việt"), ("ru_RU", "Русский"), ("de_DE", "Deutsch"), ("fr_FR", "Français"), ("es_ES", "Español"),
    ("pt_PT", "Português"), ("id_ID", "Bahasa Indonesia"), ("th_TH", "ไทย"),
];

#[derive(Default)]
struct State {
    code: String,
    game_code: String,
    detected: Option<String>,
    map: HashMap<String, String>,
    terms: HashMap<String, HashMap<String, Vec<String>>>,
    root: PathBuf,
}

fn state() -> &'static RwLock<State> {
    static S: OnceLock<RwLock<State>> = OnceLock::new();
    S.get_or_init(|| RwLock::new(State { code: "en".into(), game_code: "en".into(), ..Default::default() }))
}

fn known(code: &str) -> bool {
    LANGUAGES.iter().any(|l| l.0 == code)
}

/// Picks the languages. Game: the setting, or with "Auto" the game's own setting, then Windows' display language,
/// then English. App: the setting, or with "Auto" the game's language.
pub fn init(assets: &Path, app_setting: &str, game_setting: &str) {
    {
        let mut s = state().write().unwrap();
        s.root = assets.join("lang");
        match std::fs::read_to_string(s.root.join("game_terms.json")).map_err(|e| e.to_string()).and_then(|t| {
            let v: serde_json::Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
            serde_json::from_value::<HashMap<String, HashMap<String, Vec<String>>>>(v["terms"].clone()).map_err(|e| e.to_string())
        }) {
            Ok(t) => s.terms = t,
            Err(e) => crate::log::write(&format!("game terms: {e}")),
        }
    }
    apply(app_setting, game_setting);
}

pub fn apply(app_setting: &str, game_setting: &str) {
    let detected = read_game_language();
    let mut s = state().write().unwrap();
    s.game_code = if game_setting != "Auto" && known(game_setting) {
        game_setting.to_string()
    } else {
        detected.clone().or_else(from_ui_culture).unwrap_or_else(|| "en".into())
    };
    s.detected = detected;
    s.code = if app_setting != "Auto" && known(app_setting) { app_setting.to_string() } else { s.game_code.clone() };
    s.map.clear();
    let file = s.root.join(format!("{}.json", s.code));
    if s.code != "en" && file.exists() {
        match std::fs::read_to_string(&file).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<HashMap<String, String>>(&t).map_err(|e| e.to_string())) {
            Ok(m) => s.map = m,
            Err(e) => crate::log::write(&format!("language {}: {e}", s.code)),
        }
    }
}

pub fn code() -> String {
    state().read().unwrap().code.clone()
}

/// The language the game shows its text in (for reading the party screen).
pub fn game_code() -> String {
    state().read().unwrap().game_code.clone()
}

/// The language the game shows according to its own settings (None: not found).
pub fn detected_game_code() -> Option<String> {
    state().read().unwrap().detected.clone()
}

/// A language's own name ("Deutsch", "日本語").
pub fn name_of(code: &str) -> String {
    LANGUAGES.iter().find(|l| l.0 == code).map_or(code.to_string(), |l| l.1.to_string())
}

/// The text in the current language (the English text when there is no translation).
pub fn t(english: &str) -> String {
    state().read().unwrap().map.get(english).filter(|t| !t.is_empty()).cloned().unwrap_or_else(|| english.to_string())
}

/// A translated format string filled in ({0}, {1} ...).
pub fn f(english: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut s = t(english);
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), &a.to_string());
    }
    s
}

/// A game term (Normal, Hard, Nightmare, Chaos, EggBoat, ...) in the game's wording for the current language.
pub fn term(key: &str, fallback: Option<&str>) -> String {
    let s = state().read().unwrap();
    s.terms.get(&s.code).and_then(|t| t.get(key)).and_then(|v| v.first()).cloned().unwrap_or_else(|| fallback.unwrap_or(key).to_string())
}

/// A difficulty's name as the game shows it.
pub fn difficulty(d: Option<&str>) -> String {
    d.map_or_else(|| t("Unknown"), |d| term(d, Some(d)))
}

/// Every wording of a term in a language, for reading it off the screen.
pub fn variants(lang: &str, key: &str) -> Vec<String> {
    state().read().unwrap().terms.get(lang).and_then(|t| t.get(key)).cloned().unwrap_or_default()
}

/// Sets the game terms directly (tests).
pub fn set_terms(terms: HashMap<String, HashMap<String, Vec<String>>>) {
    state().write().unwrap().terms = terms;
}

/// The game's language setting as it is right now, from its saved preferences (Unity keeps them in the registry under
/// HKCU\Software\Aniimo\Aniimo; reading them doesn't touch the game). The value is saved as text ("2" plus a NUL).
#[cfg(windows)]
pub fn read_game_language() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    static NAME: RwLock<Option<String>> = RwLock::new(None);
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Aniimo\\Aniimo").ok()?;
    let held = NAME.read().unwrap().clone().and_then(|n| key.get_raw_value(&n).ok());
    let raw = match held {
        Some(r) => r,
        None => {
            let name = key.enum_values().filter_map(Result::ok).map(|(n, _)| n).find(|n| {
                let l = n.to_lowercase();
                l.contains("_language_h") && !l.contains("audio")
            })?;
            *NAME.write().unwrap() = Some(name.clone());
            key.get_raw_value(&name).ok()?
        }
    };
    use winreg::enums::RegType;
    let i: i64 = match raw.vtype {
        RegType::REG_DWORD if raw.bytes.len() >= 4 => i64::from(u32::from_le_bytes([raw.bytes[0], raw.bytes[1], raw.bytes[2], raw.bytes[3]]) as i32),
        RegType::REG_QWORD if raw.bytes.len() >= 8 => i64::from_le_bytes(raw.bytes[..8].try_into().ok()?),
        RegType::REG_SZ | RegType::REG_EXPAND_SZ => {
            let w: Vec<u16> = raw.bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&w).trim_end_matches(['\0', ' ']).trim().parse().unwrap_or(-1)
        }
        _ => String::from_utf8_lossy(&raw.bytes).trim_end_matches(['\0', ' ']).parse().unwrap_or(-1),
    };
    (0..LANGUAGES.len() as i64).contains(&i).then(|| LANGUAGES[i as usize].0.to_string())
}

#[cfg(not(windows))]
pub fn read_game_language() -> Option<String> {
    None
}

/// The game language for a culture name such as "de-DE" or "zh-Hant-TW".
pub fn from_culture(name: &str) -> Option<String> {
    if name.starts_with("zh") {
        let trad = name.contains("Hant") || name.ends_with("TW") || name.ends_with("HK") || name.ends_with("MO");
        return Some(if trad { "zh_TW" } else { "zh_CN" }.into());
    }
    let two = name.split('-').next().unwrap_or("").to_lowercase();
    LANGUAGES.iter().map(|l| l.0).find(|c| c.starts_with(&format!("{two}_")) || *c == two).map(str::to_string)
}

#[cfg(windows)]
fn from_ui_culture() -> Option<String> {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let (mut n, mut len) = (0u32, 0u32);
    unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut n, std::ptr::null_mut(), &mut len) == 0 || len == 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut n, buf.as_mut_ptr(), &mut len) == 0 {
            return None;
        }
        let first: Vec<u16> = buf.into_iter().take_while(|&c| c != 0).collect();
        from_culture(&String::from_utf16_lossy(&first))
    }
}

#[cfg(not(windows))]
fn from_ui_culture() -> Option<String> {
    None
}

/// Windows OCR language tag for a game language (None: Windows has no OCR for it, e.g. Thai).
pub fn ocr_tag(code: &str) -> Option<&'static str> {
    Some(match code {
        "zh_CN" => "zh-Hans-CN",
        "zh_TW" => "zh-Hant-TW",
        "ko_KR" => "ko",
        "ja_JP" => "ja",
        "ru_RU" => "ru",
        "de_DE" => "de-DE",
        "fr_FR" => "fr-FR",
        "es_ES" => "es-ES",
        "pt_PT" => "pt-PT",
        "vi_VN" => "vi",
        "id_ID" => "id",
        "en" => "en-US",
        _ => return None,
    })
}
