//! Beta Menu > Dev: the switches of the core, the DPS meter and the leaderboards (Sanctum Helper keeps its own, in its
//! program). Each is a proposed change, to test alone; their list is app/ui/dev-options.json, and docs/plugins.md says how
//! to add one.
//!
//! All off at every start, so off means behaviour as in 0.1.36. The one exception is a switch a tester made persistent:
//! that starts on, through restarts and updates, until they undo it or its revision goes up, which says its behaviour
//! changed. Only persistent choices are saved (with their revision), in dev-persistent.json.
//!
//! Beta builds only: on a release build nothing is loaded and every switch stays off, and the meter isn't even told where
//! the list of what's on is (ALOGS_DEV_ON, read by bossbar/devflags.py).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

const OPTIONS: &str = include_str!("../../ui/dev-options.json");

/// Set once at start on a beta build; a plain read in the code the switches guard.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State { on: BTreeSet::new(), persistent: BTreeMap::new(), changed: Vec::new() });

struct State {
    on: BTreeSet<String>,
    persistent: BTreeMap<String, u64>,
    changed: Vec<String>, // persistent until this start, when their revision went up: the tab says which
}

fn options() -> &'static Vec<Value> {
    static LIST: OnceLock<Vec<Value>> = OnceLock::new();
    LIST.get_or_init(|| serde_json::from_str(OPTIONS).unwrap_or_default())
}

fn revisions(list: &[Value]) -> BTreeMap<String, u64> {
    list.iter().filter_map(|o| Some((o["id"].as_str()?.to_string(), o["revision"].as_u64()?))).collect()
}

/// What a saved file means now: the ones that still hold, and those let go because their revision changed. One that no
/// longer exists just goes. Unreadable: none. The bool says the file needs saving again.
fn load_saved(text: Option<&str>, current: &BTreeMap<String, u64>) -> (BTreeMap<String, u64>, Vec<String>, bool) {
    let saved: BTreeMap<String, u64> = text.and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
    let (mut kept, mut changed) = (BTreeMap::new(), Vec::new());
    for (id, revision) in &saved {
        match current.get(id) {
            Some(now) if now == revision => {
                kept.insert(id.clone(), *revision);
            }
            Some(_) => changed.push(id.clone()),
            None => {}
        }
    }
    let dirty = kept.len() != saved.len();
    (kept, changed, dirty)
}

fn write_whole(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

fn persist_file() -> PathBuf {
    crate::data_dir().join("dev-persistent.json")
}

/// What the meter reads: the switches on now.
pub fn on_file() -> PathBuf {
    crate::data_dir().join("dev-on.json")
}

fn save_persistent(state: &State) {
    if let Err(e) = write_whole(&persist_file(), &json!(state.persistent).to_string()) {
        crate::app_log(&format!("dev: saving persistent switches: {e}"));
    }
}

fn share_on(state: &State) {
    if let Err(e) = write_whole(&on_file(), &json!({ "on": state.on }).to_string()) {
        crate::app_log(&format!("dev: writing what's on: {e}"));
    }
}

/// At start, before the meter. On a beta: what's persistent turns on. On a release: nothing, and a list left by an earlier
/// beta goes; the persistent choices stay on disk for the next beta, unread.
pub fn start() {
    if crate::update::build_beta() == 0 {
        let _ = fs::remove_file(on_file());
        return;
    }
    let text = fs::read_to_string(persist_file()).ok();
    let (persistent, changed, dirty) = load_saved(text.as_deref(), &revisions(options()));
    let mut state = STATE.lock().unwrap();
    state.on = persistent.keys().cloned().collect();
    state.persistent = persistent;
    for id in &changed {
        crate::app_log(&format!("dev: {id} no longer persistent: it changed in this build"));
    }
    state.changed = changed;
    if !state.on.is_empty() {
        crate::app_log(&format!("dev: on at start: {}", state.on.iter().cloned().collect::<Vec<_>>().join(", ")));
    }
    if dirty {
        save_persistent(&state);
    }
    share_on(&state);
    ACTIVE.store(true, Ordering::SeqCst);
}

/// Is this switch on? Always false on a release build.
pub fn on(id: &str) -> bool {
    ACTIVE.load(Ordering::Relaxed) && STATE.lock().unwrap().on.contains(id)
}

fn known(id: &str) -> Result<u64, String> {
    if !ACTIVE.load(Ordering::SeqCst) {
        return Err("Dev switches are only on a beta build.".into());
    }
    revisions(options()).get(id).copied().ok_or_else(|| format!("No switch called {id}."))
}

/// The tab's view: nothing but `beta: false` on a release build.
#[tauri::command]
pub fn dev_state() -> Value {
    if !ACTIVE.load(Ordering::SeqCst) {
        return json!({ "beta": false, "options": [], "on": [], "persistent": [], "changed": [] });
    }
    let state = STATE.lock().unwrap();
    json!({ "beta": true, "options": options(), "on": state.on, "persistent": state.persistent.keys().collect::<Vec<_>>(), "changed": state.changed })
}

#[tauri::command]
pub fn dev_set(id: String, on: bool) -> Result<(), String> {
    known(&id)?;
    let mut state = STATE.lock().unwrap();
    if on {
        state.on.insert(id.clone());
    } else {
        state.on.remove(&id);
    }
    share_on(&state);
    crate::app_log(&format!("dev: {id} {}", if on { "on" } else { "off" }));
    Ok(())
}

/// Making one persistent turns it on now as well; undoing that leaves it as it is until the next start.
#[tauri::command]
pub fn dev_persistent(id: String, persistent: bool) -> Result<(), String> {
    let revision = known(&id)?;
    let mut state = STATE.lock().unwrap();
    if persistent {
        state.persistent.insert(id.clone(), revision);
        state.on.insert(id.clone());
        share_on(&state);
    } else {
        state.persistent.remove(&id);
    }
    state.changed.retain(|x| x != &id);
    save_persistent(&state);
    crate::app_log(&format!("dev: {id} {}", if persistent { "persistent" } else { "not persistent" }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current() -> BTreeMap<String, u64> {
        BTreeMap::from([("a".to_string(), 1), ("b".to_string(), 2)])
    }

    #[test]
    fn the_list_is_complete() {
        let list = options();
        assert!(!list.is_empty(), "dev-options.json didn't read");
        for option in list {
            for field in ["id", "plugin", "section", "area", "title", "what", "gain", "risk", "tested"] {
                assert!(option[field].as_str().is_some_and(|s| !s.is_empty()), "{option}: {field}");
            }
            assert!(option["revision"].as_u64().is_some(), "{option}: revision");
            assert!(["core", "dps", "leaderboards"].contains(&option["plugin"].as_str().unwrap()), "{option}: plugin");
            assert!(["No known risk", "Low risk", "High risk"].contains(&option["section"].as_str().unwrap()), "{option}: section");
            assert!(["CPU", "Smoothness", "Accuracy", "Memory", "Speed"].contains(&option["area"].as_str().unwrap()), "{option}: area");
        }
        assert_eq!(revisions(list).len(), list.len(), "two switches share an id");
    }

    #[test]
    fn nothing_saved_means_all_off() {
        for text in [None, Some(""), Some("not json"), Some("[1,2]"), Some("{\"a\":\"x\"}")] {
            let (kept, changed, _) = load_saved(text, &current());
            assert!(kept.is_empty() && changed.is_empty(), "{text:?}");
        }
    }

    #[test]
    fn a_persistent_switch_holds_while_its_revision_does() {
        let (kept, changed, dirty) = load_saved(Some(r#"{"a":1,"b":2}"#), &current());
        assert_eq!(kept, current());
        assert!(changed.is_empty() && !dirty);
    }

    #[test]
    fn a_raised_revision_lets_go_and_says_so() {
        let (kept, changed, dirty) = load_saved(Some(r#"{"a":1,"b":1}"#), &current());
        assert_eq!(kept, BTreeMap::from([("a".to_string(), 1)]));
        assert_eq!(changed, vec!["b".to_string()]);
        assert!(dirty, "the file is saved without it, so it isn't said again at the next start");
    }

    #[test]
    fn a_switch_taken_out_just_goes() {
        let (kept, changed, dirty) = load_saved(Some(r#"{"gone":3,"a":1}"#), &current());
        assert_eq!(kept.len(), 1);
        assert!(changed.is_empty(), "it can't be named: it has no title any more");
        assert!(dirty);
    }

    #[test]
    fn written_whole_and_read_back() {
        let dir = std::env::temp_dir().join(format!("alogs-dev-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dev-persistent.json");
        write_whole(&path, &json!(BTreeMap::from([("a".to_string(), 1u64)])).to_string()).unwrap();
        let (kept, _, _) = load_saved(fs::read_to_string(&path).ok().as_deref(), &current());
        assert_eq!(kept.get("a"), Some(&1));
        assert!(!path.with_extension("tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn off_until_started() {
        // The tests never call start(), as a release build doesn't: every switch reads off and can't be set
        assert!(!on("capture-log-once"));
        assert!(dev_set("capture-log-once".into(), true).is_err());
        assert!(dev_persistent("capture-log-once".into(), true).is_err());
        assert_eq!(dev_state()["beta"], false);
    }
}
