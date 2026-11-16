//! Sanctum Helper's settings (%LOCALAPPDATA%\SanctumHelper\settings.json, the one file aLogs edits too) and its log.
//! Port of `Settings.cs`: the same property names and defaults, the same version migrations, and a save that keeps
//! what aLogs changed in the file meanwhile for every setting not changed here.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub const CURRENT_VERSION: i64 = 5;

/// A rectangle in 1920x1080 reference pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RegionF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

fn default_layers() -> BTreeMap<String, bool> {
    [("doors", true), ("nest", true), ("key", true), ("challenge", true), ("legendary", true), ("luxurious", true), ("exquisite", false),
        ("simple", false), ("pot", false), ("pile", false)].into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Settings {
    pub version: i64,
    pub hold_key: i64,
    pub toggle_mode: bool,
    pub map_opacity: f64,
    pub marker_scale: f64,
    pub show_map_image: bool,
    pub show_labels: bool,
    pub show_view_cone: bool,
    pub caption_position: String,
    pub caption_layout: bool,
    pub caption_difficulty: bool,
    pub use_game_log: bool,
    pub show_notifications: bool,
    pub hold_button: i64,
    pub game_language: String,
    pub language: String,
    pub show_player: bool,
    pub layers: BTreeMap<String, bool>,
    pub recordable_overlay: bool,
    pub redraw_cap: bool,
    pub mini_map_mode: String,
    pub mini_map_corner: String,
    pub mini_map_x: Option<f64>,
    pub mini_map_y: Option<f64>,
    pub mini_map_size: f64,
    pub mini_map_zoom: f64,
    pub mini_map_opacity: f64,
    pub overlay_zoom: f64,
    pub big_map_zoom: f64,
    pub big_map_size: f64,
    pub overlay_opacity: f64,
    pub island_map: bool,
    pub difficulty: String,
    pub forced_layout: i64,
    pub track_minimap: bool,
    pub start_with_windows: bool,
    pub window_title: String,
    pub save_debug_frames: bool,
    pub run_layout: i64,
    pub run_id: Option<String>,
    pub run_layout_time: String,
    pub party_goal_region: RegionF,
    pub minimap_region: RegionF,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: CURRENT_VERSION, hold_key: 0xA2, toggle_mode: false, map_opacity: 0.55, marker_scale: 1.0, show_map_image: true, show_labels: true,
            show_view_cone: true, caption_position: "Below".into(), caption_layout: true, caption_difficulty: true, use_game_log: true,
            show_notifications: true, hold_button: 0, game_language: "Auto".into(), language: "Auto".into(), show_player: true, layers: default_layers(),
            recordable_overlay: true, redraw_cap: false, mini_map_mode: "Corner".into(), mini_map_corner: "TopRight".into(), mini_map_x: None,
            mini_map_y: None, mini_map_size: 300.0, mini_map_zoom: 0.33, mini_map_opacity: 0.85, overlay_zoom: 0.55, big_map_zoom: 0.8,
            big_map_size: 0.8, overlay_opacity: 0.65, island_map: true, difficulty: "Auto".into(), forced_layout: 0, track_minimap: true,
            start_with_windows: false, window_title: "Aniimo".into(), save_debug_frames: false, run_layout: 0, run_id: None,
            run_layout_time: "0001-01-01T00:00:00".into(), party_goal_region: RegionF { x: 740.0, y: 6.0, w: 400.0, h: 54.0 },
            minimap_region: RegionF { x: 75.0, y: 32.0, w: 210.0, h: 210.0 },
        }
    }
}

/// %LOCALAPPDATA%\SanctumHelper (or SANCTUM_DIR for tests).
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SANCTUM_DIR") {
        return PathBuf::from(d);
    }
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("SanctumHelper")
}

fn file_path() -> PathBuf {
    dir().join("settings.json")
}

/// The file as this process last read or wrote it, and as the settings in memory last matched it.
static LAST_JSON: Mutex<Option<String>> = Mutex::new(None);
static BASE_JSON: Mutex<Option<String>> = Mutex::new(None);
static SAVING: Mutex<()> = Mutex::new(());

pub fn last_json() -> Option<String> {
    LAST_JSON.lock().unwrap().clone()
}

impl Settings {
    pub fn load() -> Settings {
        let p = file_path();
        if let Ok(json) = std::fs::read_to_string(&p) {
            *LAST_JSON.lock().unwrap() = Some(json.clone());
            *BASE_JSON.lock().unwrap() = Some(json.clone());
            match serde_json::from_str::<Settings>(&json) {
                Ok(mut s) => {
                    if !json.contains("\"Version\"") {
                        s.version = 1;
                    }
                    if s.version < 2 && s.hold_key == 0x14 {
                        s.hold_key = 0xA2;
                    }
                    if s.version < 3 {
                        s.overlay_zoom = 0.55;
                        s.overlay_opacity = 0.65;
                    }
                    if s.version < 4 {
                        s.island_map = true;
                    }
                    if s.difficulty == "Beginner" {
                        s.difficulty = "Normal".into();
                    }
                    s.version = CURRENT_VERSION;
                    return s;
                }
                Err(e) => log(&format!("settings load failed: {e}")),
            }
        }
        Settings::default()
    }

    /// Written whole (temp file, then replaced), keeping what changed in the file since it was last read for every
    /// setting not changed here.
    pub fn save(&self) {
        let _g = SAVING.lock().unwrap();
        let _ = std::fs::create_dir_all(dir());
        let mut json = serde_json::to_string_pretty(self).unwrap_or_default();
        let merged = self.merge_on_disk();
        if let Some(m) = &merged {
            json = m.clone();
        }
        *LAST_JSON.lock().unwrap() = if merged.is_some() { None } else { Some(json.clone()) };
        if merged.is_none() {
            *BASE_JSON.lock().unwrap() = Some(json.clone());
        }
        let tmp = file_path().with_extension("json.tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, file_path());
        }
    }

    fn merge_on_disk(&self) -> Option<String> {
        let base = BASE_JSON.lock().unwrap().clone()?;
        let disk: Map<String, Value> = serde_json::from_str::<Value>(&std::fs::read_to_string(file_path()).ok()?).ok()?.as_object()?.clone();
        let before: Settings = serde_json::from_str(&base).ok()?;
        let (now_v, before_v) = (serde_json::to_value(self).ok()?, serde_json::to_value(&before).ok()?);
        let (now_o, before_o) = (now_v.as_object()?, before_v.as_object()?);
        let mut mine: Option<Map<String, Value>> = None;
        for (k, now) in now_o {
            let Some(there) = disk.get(k) else { continue };
            if Some(now) != before_o.get(k) {
                continue; // changed here since: ours stays
            }
            // the file has something else (compared the way it would be written here)
            let as_written = {
                let mut probe = now_o.clone();
                probe.insert(k.clone(), there.clone());
                serde_json::from_value::<Settings>(Value::Object(probe)).ok().and_then(|s| serde_json::to_value(s).ok()).and_then(|v| v.get(k).cloned())
            };
            if as_written.as_ref() == Some(now) {
                continue;
            }
            mine.get_or_insert_with(|| now_o.clone()).insert(k.clone(), there.clone());
        }
        mine.and_then(|m| serde_json::to_string_pretty(&Value::Object(m)).ok())
    }

    /// Layers added in later versions fall back to their default.
    pub fn layer(&self, kind: &str) -> bool {
        self.layers.get(kind).copied().unwrap_or_else(|| default_layers().get(kind).copied().unwrap_or(false))
    }
}

/// log.txt beside the settings (starts over at 2 MB).
pub fn log(msg: &str) {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let p = d.join("log.txt");
    if std::fs::metadata(&p).is_ok_and(|m| m.len() > 2_000_000) {
        let _ = std::fs::remove_file(&p);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{} {msg}", chrono::Local::now().format("%H:%M:%S%.3f"));
    }
}
