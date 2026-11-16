//! Where the meter finds its files, and the settings the desktop app edits. Port of aLogs B16 `bossbar/settings.py`.
//!
//! settings.json is shared with the desktop app, which writes it from its menus; missing keys fall back to
//! [`defaults`], and keys `defaults` doesn't have are dropped as it's read (`merge`).

use alogs_pyval::{Dict, Value};
use std::path::{Path, PathBuf};

pub const APP_DIR: &str = "aLogs";

/// The meter's data folder: `ALOGS_DATA`, else `%LOCALAPPDATA%\aLogs\meter` (packaged) or `source_dir` (from source).
pub fn data_dir(frozen: bool, source_dir: &Path) -> PathBuf {
    if let Some(d) = std::env::var_os("ALOGS_DATA").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    if !frozen {
        return source_dir.to_path_buf();
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .or_else(|| std::env::var_os("HOME"))
        .unwrap_or_default();
    PathBuf::from(base).join(APP_DIR).join("meter")
}

pub fn settings_file(data: &Path) -> PathBuf {
    data.join("settings.json")
}
pub fn status_file(data: &Path) -> PathBuf {
    data.join("status.json")
}
/// Other modules' notices for the meter to show (Sanctum Helper).
pub fn notices_dir(data: &Path) -> PathBuf {
    data.join("notices")
}

const DEFAULTS_JSON: &str = r#"{
 "meter": {"enabled": true},
 "audio_cues": {"boss_kill": true},
 "app": {"welcome_done": false, "show_guide": true, "start_with_windows": false, "close_to_tray": true,
         "priority": "low", "auto_download": true, "beta_updates": false, "photos_auto": false,
         "photos_auto_since": 0, "load_details": false},
 "perf": {"on": false, "priority": "low", "look_interval": 0.05, "fight_rate": 30, "gpu_priority": "normal",
          "map_redraw": "every"},
 "sanctum": {"enabled": true, "map_redraw": "every"},
 "game": {"window_title": "Aniimo", "player_log": ""},
 "overlay": {"show": true, "dps_window": true, "hp_labels": true, "in_recordings": true, "backdrop": 0,
             "world_position": "map", "dps_locked": true, "dps_example": false, "dps_reset": 0,
             "rush_position": "corner", "result_seconds": 10},
 "capture": {"game_window_only": false, "look_interval": 0.05, "damage_numbers": false, "test_frames": false,
             "fight_rate": 30, "gpu_priority": "normal", "debug_frames": false, "end_screens": false,
             "nobar_frames": false, "keep_hours": 0},
 "results": {"auto_open_battle_data": true, "sim_gizmos": true, "sim_gate": false, "egg_auto_next": true,
             "hold_inputs": true, "hold_controller": false, "egg_fade_hold": false, "hold_seconds": 60},
 "leaderboard": {"submit": false, "automation": false, "api_url": "https://aniilogs-api.pages.dev/api", "token": "",
                 "link_character": true, "include_screenshots": true, "notify_sent": true},
 "notifications": {"place": "top-right", "motion": "fade", "locked": true, "button": true},
 "user_set": []
}"#;

/// `DEFAULTS`, the settings every key of which the meter knows.
pub fn defaults() -> Value {
    Value::json_loads(DEFAULTS_JSON).expect("DEFAULTS_JSON parses")
}

/// `_merge(base, extra)`: `base` with `extra`'s values for the keys `base` has, dicts merged inside.
pub fn merge(base: &Value, extra: &Value) -> Value {
    let Value::Dict(b) = base else { return base.clone() };
    let mut out = b.clone();
    if let Value::Dict(e) = extra {
        for (k, v) in e.iter() {
            match (out.get(k), v) {
                (Some(Value::Dict(_)), Value::Dict(_)) => {
                    let merged = merge(out.get(k).unwrap(), v);
                    out.insert(k.clone(), merged);
                }
                (Some(_), _) => out.insert(k.clone(), v.clone()),
                (None, _) => {}
            }
        }
    }
    Value::Dict(out)
}

/// May the meter take control of the game for leaderboard proof (and do `what`, a `results.*` switch)? Only with
/// sending and the automation both on.
pub fn automation(cfg: &Value, what: Option<&str>) -> bool {
    let empty = Value::Dict(Dict::new());
    let lb = cfg.get("leaderboard").or(&empty);
    if !(lb.get("submit").truthy() && lb.get("automation").truthy()) {
        return false;
    }
    match what {
        None => true,
        Some(w) => cfg.get("results").or(&empty).get(w).truthy(),
    }
}

pub const NOTICE_KEYS: &[&str] = &["place", "motion", "locked"];

/// Where notices show was the leaderboards' (`leaderboard.notify_place`, ...) until notices became every plug-in's:
/// now `notifications.place`, ... A choice made there is carried over.
pub fn migrate_notices(stored: &mut Dict) {
    let Some(Value::Dict(lb)) = stored.get_str("leaderboard").cloned() else { return };
    if !NOTICE_KEYS.iter().any(|k| lb.get_str(&format!("notify_{k}")).is_some()) {
        return;
    }
    let mut lb = lb;
    let mut notes = match stored.get_str("notifications") {
        Some(Value::Dict(d)) => d.clone(),
        _ => Dict::new(),
    };
    for k in NOTICE_KEYS {
        let key = format!("notify_{k}");
        if let Some(v) = lb.get_str(&key).cloned() {
            lb = lb.iter().filter(|(kk, _)| kk.as_str() != Some(&key)).map(|(a, b)| (a.clone(), b.clone())).collect();
            if notes.get_str(k).is_none() {
                notes.insert((*k).into(), v);
            }
        }
    }
    stored.insert("leaderboard".into(), Value::Dict(lb));
    stored.insert("notifications".into(), Value::Dict(notes));
}

/// `load()` from a settings.json's text (`None`: missing or unreadable). Settings from before 0.1.28 and from B9 are
/// brought up to date.
pub fn load_from(text: Option<&str>) -> Value {
    let stored = text.and_then(Value::json_loads).unwrap_or_else(|| Value::Dict(Dict::new()));
    let Value::Dict(mut stored) = stored else {
        // json.load gave a list or a number: the Python's stored.get raises; the meter can't start with it either
        return defaults();
    };
    if let Some(Value::Dict(ov)) = stored.get_str("overlay").cloned() {
        let mut ov = ov;
        if ov.get_str("in_recordings").is_none()
            && let Some(h) = ov.get_str("hide_from_capture").cloned()
        {
            ov.insert("in_recordings".into(), Value::Bool(!h.truthy()));
        }
        if ov.at("world_position").py_eq(&"quests".into()) {
            ov.insert("world_position".into(), "right".into());
        }
        stored.insert("overlay".into(), Value::Dict(ov));
    }
    migrate_notices(&mut stored);
    merge(&defaults(), &Value::Dict(stored))
}

pub fn load(data: &Path) -> Value {
    let text = std::fs::read(settings_file(data)).ok().map(|b| String::from_utf8_lossy(&b).into_owned());
    load_from(text.as_deref())
}

/// settings.json's text as `save()` writes it.
pub fn save_text(settings: &Value) -> String {
    merge(&defaults(), settings).json_dumps_indent(1)
}

pub fn save(data: &Path, settings: &Value) -> std::io::Result<()> {
    write_whole(&settings_file(data), &save_text(settings))
}

/// status.json for the desktop app's dashboard (written about once a second). Errors are ignored, as in the Python.
pub fn write_status(data: &Path, status: &Value) {
    let _ = write_whole(&status_file(data), &status.json_dumps());
}

/// Written to `<path>.tmp`, then renamed over `path`.
pub fn write_whole(path: &Path, text: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}
