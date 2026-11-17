//! Notices other programs (Sanctum Helper) hand to aLogs: one JSON file each in the meter's `notices` folder, written
//! whole then renamed, named from .NET ticks (20 digits) so they sort by time. Port of `take_notices` in aLogs B16
//! `bossbar/notice_window.py`, and the writing side of Sanctum Helper's notices.

use alogs_pyval::{Value, py_int};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub head: String,
    pub detail: String,
    pub accent: String,
    /// The game's client area `[x, y, w, h]` it belongs to, if it said.
    pub screen: Option<[i64; 4]>,
    /// The notice kind in plugins.json ("sanctum"); "notice" when it didn't say.
    pub kind: String,
}

pub const DEFAULT_ACCENT: &str = "#5cb8ff";

fn cut(s: &str) -> String {
    s.chars().take(120).collect()
}

/// `v > 0`, or the TypeError Python raises for a str, list, dict or None.
fn gt0(v: &Value) -> Result<bool, ()> {
    match v {
        Value::Str(_) | Value::List(_) | Value::Dict(_) | Value::None => Err(()),
        v => Ok(v.as_f64().unwrap() > 0.0),
    }
}

/// One notice file's contents as the meter takes it: `Ok(None)` when it has no head, `Err` when the Python drops it
/// on an error (or the file isn't a JSON object, where its `d.get` would fail).
pub fn parse(text: &str) -> Result<Option<Notice>, ()> {
    let d = Value::json_loads(text).ok_or(())?;
    if d.as_dict().is_none() {
        return Err(());
    }
    let screen = match d.get("screen") {
        Value::List(l) if l.len() == 4 && gt0(&l[2])? && gt0(&l[3])? => {
            let mut s = [0i64; 4];
            for (i, v) in l.iter().enumerate() {
                s[i] = match v {
                    Value::Float(f) if !f.is_finite() => return Err(()),
                    Value::List(_) | Value::Dict(_) | Value::None => return Err(()),
                    v => py_int(v).ok_or(())?,
                };
            }
            Some(s)
        }
        _ => None,
    };
    let re = regex::Regex::new(r"^#[0-9a-fA-F]{6}$").unwrap();
    let accent_s = d.get("accent").py_str();
    let accent = if re.is_match(&accent_s) { accent_s } else { DEFAULT_ACCENT.to_string() };
    if !d.get("head").truthy() {
        return Ok(None);
    }
    Ok(Some(Notice {
        head: cut(&d.get("head").py_str()),
        detail: cut(&d.get("detail").or(&Value::Str(String::new())).py_str()),
        accent,
        screen,
        kind: d.get("from").or(&Value::Str("notice".into())).py_str(),
    }))
}

/// Takes every notice waiting in `dir`, oldest first, and deletes the files (also the ones it couldn't read).
pub fn take(dir: &Path) -> Vec<Notice> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let path = dir.join(&name);
        if let Ok(bytes) = std::fs::read(&path)
            && let Ok(text) = String::from_utf8(bytes)
            && let Ok(Some(n)) = parse(&text)
        {
            out.push(n);
        }
        let _ = std::fs::remove_file(&path);
    }
    out
}

/// .NET ticks (100 ns since 0001-01-01) for a time, as Sanctum Helper names its notice files.
pub fn dotnet_ticks(t: std::time::SystemTime) -> u128 {
    const UNIX_EPOCH_TICKS: u128 = 621_355_968_000_000_000;
    let d = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    UNIX_EPOCH_TICKS + d.as_nanos() / 100
}

/// Writes a notice the way Sanctum Helper does: `<20-digit ticks>-<pid>.json`, whole then renamed.
pub fn write(dir: &Path, n: &Notice) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let screen = n.screen.map_or(Value::None, |s| Value::List(s.iter().map(|&v| Value::Int(v)).collect()));
    let body = alogs_pyval::dict([
        ("head", Value::Str(n.head.clone())),
        ("detail", Value::Str(n.detail.clone())),
        ("accent", Value::Str(n.accent.clone())),
        ("screen", screen),
        ("from", Value::Str(n.kind.clone())),
    ]);
    let ticks = dotnet_ticks(std::time::SystemTime::now());
    let path = dir.join(format!("{ticks:020}-{}.json", std::process::id()));
    crate::settings::write_whole(&path, &body.json_dumps())?;
    Ok(path)
}
