//! aLogs' plug-ins: the one list (bossbar/plugins.json) the app and the meter both read - each plug-in's name, page,
//! switch, whether it's opt-in, what it needs and which notices it owns. The button over the game switches the ones it
//! controls (General Settings › The button over the game: button.controls.<id>), and the app builds its plug-in
//! controls and statuses from it (`plugins`).

use serde_json::Value;
use std::sync::OnceLock;

static MANIFEST: OnceLock<Value> = OnceLock::new();

pub fn manifest() -> &'static Value {
    MANIFEST.get_or_init(|| serde_json::from_str(include_str!("../../../bossbar/plugins.json")).expect("plugins.json reads"))
}

#[derive(Clone, Debug)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    /// a settings.json pointer, or "meter" for the DPS meter's own switch (overlay_button.json)
    pub switch: String,
    pub default: bool,
    /// on only when the player turns it on (the button's first press never does)
    pub opt_in: bool,
}

pub fn list() -> Vec<Plugin> {
    manifest()["plugins"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|p| Plugin {
                    id: p["id"].as_str().unwrap_or_default().into(),
                    name: p["name"].as_str().unwrap_or_default().into(),
                    switch: p["switch"].as_str().unwrap_or_default().into(),
                    default: p["default"].as_bool().unwrap_or(false),
                    opt_in: p["opt_in"].as_bool().unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Its own switch, as it stands.
pub fn is_on(p: &Plugin, settings: &Value) -> bool {
    if p.switch == "meter" {
        crate::overlay_button::capture_on()
    } else {
        settings.pointer(&p.switch).and_then(Value::as_bool).unwrap_or(p.default)
    }
}

/// Turns its own switch (the meter's through the button's state, the others in settings.json).
pub fn set_on(p: &Plugin, on: bool) {
    if p.switch == "meter" {
        crate::overlay_button::set_capture(on);
    } else {
        crate::write_setting(&p.switch, Value::Bool(on));
    }
}

/// Whether the button over the game switches it (button.controls.<id>, every plug-in unless the player says).
pub fn controlled(p: &Plugin, settings: &Value) -> bool {
    settings.pointer(&format!("/button/controls/{}", p.id)).and_then(Value::as_bool).unwrap_or(true)
}

/// The list, for the app (its plug-in controls and statuses).
#[tauri::command]
pub fn plugins() -> Value {
    manifest().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_reads() {
        let ids: Vec<String> = list().iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids, ["dps", "sanctum", "leaderboards"]);
        let lb = list().into_iter().find(|p| p.id == "leaderboards").unwrap();
        assert!(lb.opt_in && !lb.default, "sending is opt-in");
        assert_eq!(lb.switch, "/leaderboard/submit");
        assert!(list().iter().all(|p| p.switch == "meter" || p.switch.starts_with('/')), "switches are pointers or the meter's");
        assert_eq!(controlled(&lb, &serde_json::json!({ "button": { "controls": { "leaderboards": false } } })), false);
        assert!(controlled(&lb, &serde_json::json!({})), "every plug-in by default");
    }
}
