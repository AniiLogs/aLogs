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
//!
//! The tab also asks how each switch went ("How did it go?"): a verdict and a note per switch, kept here with how long
//! the switch has been on while the game was open (Sanctum Helper's too, under "sanctum:<id>"), in dev-verdicts.json.
//! Sent from the tab as dev-feedback.json inside the logs of an ordinary report, so it needs nothing new on the server.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
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
    drop(state);
    let saved = fs::read_to_string(feedback_file()).ok();
    *FEEDBACK.lock().unwrap() = load_feedback(saved.as_deref(), &revisions(options()));
    ACTIVE.store(true, Ordering::SeqCst);
    // How long each switch has been on, counted only while the game is open: a switch on overnight in the tray says
    // nothing about how it plays.
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(TICK_S));
        if crate::capture::find_game().is_null() {
            continue;
        }
        let (on, _) = switches_now();
        if on.is_empty() {
            continue;
        }
        let mut all = FEEDBACK.lock().unwrap();
        tally(&mut all, &on, TICK_S);
        save_feedback(&all);
    });
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
    let mut view = json!({ "beta": true, "options": options(), "on": state.on, "persistent": state.persistent.keys().collect::<Vec<_>>(), "changed": state.changed });
    drop(state);
    view["feedback"] = json!(*FEEDBACK.lock().unwrap());
    view
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

// ---------------------------------------------------------------- how each switch went
/// What a tester says about one switch, and how long it has been on with the game open. It holds for one revision of
/// the switch: a raised revision is a different change, and what was said about the old one says nothing about it.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
struct Feedback {
    revision: u64,
    plugin: String,
    title: String,
    seconds: u64,         // on, with the game open
    verdict: String,      // "" (none yet), or one of VERDICTS
    note: String,
    sent_at: u64,         // unix seconds; 0 = never sent
    sent_verdict: String, // what went up then: the same verdict and note aren't sent twice
    sent_note: String,
}

impl Feedback {
    fn to_send(&self) -> bool {
        !self.verdict.is_empty() && (self.sent_at == 0 || self.sent_verdict != self.verdict || self.sent_note != self.note)
    }
}

/// A switch that is on: its key here (Sanctum Helper's start with SANCTUM), revision, plug-in and title.
type OnSwitch = (String, u64, String, String);

const VERDICTS: [&str; 4] = ["better", "same", "worse", "broke"];
const SANCTUM: &str = "sanctum:";
const TICK_S: u64 = 60;
const NOTE_MAX: usize = 500;

static FEEDBACK: Mutex<BTreeMap<String, Feedback>> = Mutex::new(BTreeMap::new());
/// What the last dev-feedback.json held, so that exactly that is marked as sent once the report went up.
static LAST_REPORT: Mutex<Vec<(String, String, String)>> = Mutex::new(Vec::new());

fn feedback_file() -> PathBuf {
    crate::data_dir().join("dev-verdicts.json")
}

fn save_feedback(all: &BTreeMap<String, Feedback>) {
    if let Err(e) = write_whole(&feedback_file(), &json!(all).to_string()) {
        crate::app_log(&format!("dev: saving the verdicts: {e}"));
    }
}

/// What a saved file means now. One about an earlier revision of a switch, or a switch that's gone, is dropped. Sanctum
/// Helper's are kept: its revisions are only known once it runs, and `at` drops an outdated one then.
fn load_feedback(text: Option<&str>, current: &BTreeMap<String, u64>) -> BTreeMap<String, Feedback> {
    let saved: BTreeMap<String, Feedback> = text.and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
    saved.into_iter().filter(|(key, f)| key.starts_with(SANCTUM) || current.get(key) == Some(&f.revision)).collect()
}

/// The record of a switch at this revision: started afresh when what's there is about another revision.
fn at<'a>(all: &'a mut BTreeMap<String, Feedback>, key: &str, revision: u64, plugin: &str, title: &str) -> &'a mut Feedback {
    let found = all.entry(key.to_string()).or_default();
    if found.revision != revision {
        *found = Feedback { revision, ..Feedback::default() };
    }
    (found.plugin, found.title) = (plugin.to_string(), title.to_string());
    found
}

fn tally(all: &mut BTreeMap<String, Feedback>, on: &[OnSwitch], seconds: u64) {
    for (key, revision, plugin, title) in on {
        at(all, key, *revision, plugin, title).seconds += seconds;
    }
}

/// The switches on right now, the app's and a running Sanctum Helper's, and the keys of those made persistent.
fn switches_now() -> (Vec<OnSwitch>, BTreeSet<String>) {
    let (on, mut persistent) = {
        let state = STATE.lock().unwrap();
        (state.on.clone(), state.persistent.keys().cloned().collect::<BTreeSet<String>>())
    };
    let text = |o: &Value, field: &str| o[field].as_str().unwrap_or_default().to_string();
    let mut list: Vec<OnSwitch> = options()
        .iter()
        .filter(|o| o["id"].as_str().is_some_and(|id| on.contains(id)))
        .map(|o| (text(o, "id"), o["revision"].as_u64().unwrap_or(0), text(o, "plugin"), text(o, "title")))
        .collect();
    let (theirs, kept) = crate::sanctum::dev_switches();
    list.extend(theirs.into_iter().map(|(id, revision, title)| (format!("{SANCTUM}{id}"), revision, "sanctum".to_string(), title)));
    persistent.extend(kept.into_iter().map(|id| format!("{SANCTUM}{id}")));
    (list, persistent)
}

/// dev-feedback.json: each verdict not sent yet, with how long its switch was on, and everything that is on now.
fn report(all: &BTreeMap<String, Feedback>, on: &[OnSwitch], persistent: &BTreeSet<String>, build: &str, now: u64) -> Option<Value> {
    let is_on = |key: &str| on.iter().any(|(k, ..)| k == key);
    let switches: Vec<Value> = all
        .iter()
        .filter(|(_, f)| f.to_send())
        .map(|(key, f)| {
            json!({
                "switch": key, "plugin": f.plugin, "title": f.title, "revision": f.revision, "verdict": f.verdict, "note": f.note,
                "minutes_on": f.seconds / 60, "on_now": is_on(key), "persistent": persistent.contains(key),
            })
        })
        .collect();
    if switches.is_empty() {
        return None;
    }
    let on_now: Vec<Value> = on.iter().map(|(key, revision, ..)| json!({ "switch": key, "revision": revision })).collect();
    Some(json!({ "build": build, "made": now, "minutes_on": "with the game open", "switches": switches, "on_now": on_now }))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// For collect_logs: the feedback to send as text, or None when there is nothing to send (or this is a release build).
pub fn feedback_report(build: &str) -> Option<String> {
    if !ACTIVE.load(Ordering::SeqCst) {
        return None;
    }
    let (on, persistent) = switches_now();
    let all = FEEDBACK.lock().unwrap();
    let made = report(&all, &on, &persistent, build, unix_now())?;
    *LAST_REPORT.lock().unwrap() =
        all.iter().filter(|(_, f)| f.to_send()).map(|(key, f)| (key.clone(), f.verdict.clone(), f.note.clone())).collect();
    serde_json::to_string_pretty(&made).ok()
}

fn clean_note(note: &str) -> String {
    note.trim().chars().take(NOTE_MAX).collect()
}

/// The tab saves a verdict and its note as they're chosen and typed. `key` is the switch's id, or "sanctum:<id>".
#[tauri::command]
pub fn dev_feedback_set(key: String, plugin: String, title: String, revision: u64, verdict: String, note: String) -> Result<(), String> {
    if !ACTIVE.load(Ordering::SeqCst) {
        return Err("Dev switches are only on a beta build.".into());
    }
    if !verdict.is_empty() && !VERDICTS.contains(&verdict.as_str()) {
        return Err(format!("No verdict called {verdict}."));
    }
    let (revision, plugin, title) = match key.strip_prefix(SANCTUM) {
        Some(id) if !id.is_empty() && plugin == "sanctum" => (revision, plugin, title.chars().take(120).collect()),
        Some(_) => return Err(format!("No switch called {key}.")),
        None => {
            let option = options().iter().find(|o| o["id"] == key.as_str()).ok_or_else(|| format!("No switch called {key}."))?;
            let text = |field: &str| option[field].as_str().unwrap_or_default().to_string();
            (option["revision"].as_u64().unwrap_or(0), text("plugin"), text("title"))
        }
    };
    let mut all = FEEDBACK.lock().unwrap();
    let found = at(&mut all, &key, revision, &plugin, &title);
    (found.verdict, found.note) = (verdict, clean_note(&note));
    save_feedback(&all);
    Ok(())
}

/// The report holding the last dev-feedback.json went up: what was in it is marked sent (a verdict or note changed
/// since then is not, and goes with the next one).
#[tauri::command]
pub fn dev_feedback_sent() -> Result<(), String> {
    let sent = std::mem::take(&mut *LAST_REPORT.lock().unwrap());
    let mut all = FEEDBACK.lock().unwrap();
    mark_sent(&mut all, &sent, unix_now());
    save_feedback(&all);
    crate::app_log(&format!("dev: feedback sent for {} switches", sent.len()));
    Ok(())
}

fn mark_sent(all: &mut BTreeMap<String, Feedback>, sent: &[(String, String, String)], now: u64) {
    for (key, verdict, note) in sent {
        if let Some(found) = all.get_mut(key) {
            (found.sent_at, found.sent_verdict, found.sent_note) = (now, verdict.clone(), note.clone());
        }
    }
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
        assert!(!on("kill-sound-log"));
        assert!(dev_set("kill-sound-log".into(), true).is_err());
        assert!(dev_persistent("kill-sound-log".into(), true).is_err());
        assert_eq!(dev_state()["beta"], false);
        assert!(dev_feedback_set("kill-sound-log".into(), "core".into(), "x".into(), 1, "better".into(), String::new()).is_err());
        assert!(feedback_report("0.0.0").is_none(), "a release build sends no dev feedback");
    }

    fn sw(key: &str, revision: u64) -> OnSwitch {
        (key.to_string(), revision, "core".to_string(), format!("{key} title"))
    }

    #[test]
    fn time_on_counts_per_switch_and_starts_again_with_a_new_revision() {
        let mut all = BTreeMap::new();
        tally(&mut all, &[sw("a", 1), sw("sanctum:x", 0)], 60);
        tally(&mut all, &[sw("a", 1)], 60);
        assert_eq!((all["a"].seconds, all["sanctum:x"].seconds), (120, 60));
        assert_eq!(all["a"].title, "a title");
        at(&mut all, "a", 1, "core", "a title").verdict = "better".into();
        tally(&mut all, &[sw("a", 1)], 60);
        assert_eq!((all["a"].seconds, all["a"].verdict.as_str()), (180, "better"), "the count doesn't touch a verdict");
        tally(&mut all, &[sw("a", 2)], 60);
        assert_eq!((all["a"].seconds, all["a"].verdict.as_str(), all["a"].revision), (60, "", 2), "revision 2 is another change");
    }

    #[test]
    fn saved_verdicts_hold_while_the_revision_does() {
        let mut all = BTreeMap::new();
        tally(&mut all, &[sw("a", 1), sw("b", 1), sw("gone", 1), sw("sanctum:x", 7)], 60);
        let text = json!(all).to_string();
        let kept = load_feedback(Some(&text), &current());
        assert_eq!(kept.keys().collect::<Vec<_>>(), ["a", "sanctum:x"], "b is at revision 2 now, gone is gone, Sanctum Helper's wait for it");
        assert_eq!(kept["a"], all["a"]);
        for text in [None, Some(""), Some("not json"), Some("[1,2]")] {
            assert!(load_feedback(text, &current()).is_empty(), "{text:?}");
        }
        // a file from before a field existed still reads
        assert_eq!(load_feedback(Some(r#"{"a":{"revision":1,"seconds":120}}"#), &current())["a"].seconds, 120);
    }

    #[test]
    fn only_new_verdicts_are_sent() {
        let mut all = BTreeMap::new();
        let nothing = BTreeSet::new();
        tally(&mut all, &[sw("a", 1), sw("b", 2), sw("c", 1)], 3900);
        assert!(report(&all, &[], &nothing, "0.1.37 (B4)", 5).is_none(), "time on alone is nothing to send");
        at(&mut all, "a", 1, "core", "a title").verdict = "better".into();
        let b = at(&mut all, "b", 2, "core", "b title");
        (b.verdict, b.note) = ("broke".into(), "the bar froze".into());
        let made = report(&all, &[sw("a", 1), sw("c", 1)], &BTreeSet::from(["a".to_string()]), "0.1.37 (B4)", 5).unwrap();
        assert_eq!(made["build"], "0.1.37 (B4)");
        let sent = made["switches"].as_array().unwrap();
        assert_eq!(sent.len(), 2, "c has no verdict: {made}");
        assert_eq!(sent[0], json!({ "switch": "a", "plugin": "core", "title": "a title", "revision": 1, "verdict": "better", "note": "",
            "minutes_on": 65, "on_now": true, "persistent": true }));
        assert_eq!((&sent[1]["note"], &sent[1]["on_now"], &sent[1]["persistent"]), (&json!("the bar froze"), &json!(false), &json!(false)));
        assert_eq!(made["on_now"].as_array().unwrap().len(), 2, "what else was on is said too: {made}");

        mark_sent(&mut all, &[("a".into(), "better".into(), String::new()), ("b".into(), "broke".into(), "the bar froze".into())], 9);
        assert!(report(&all, &[], &nothing, "x", 10).is_none(), "the same verdicts aren't sent twice");
        at(&mut all, "b", 2, "core", "b title").note = "the bar froze in Interlink".into();
        let again = report(&all, &[], &nothing, "x", 11).unwrap();
        assert_eq!(again["switches"].as_array().unwrap().len(), 1, "a changed note goes up again: {again}");
        // sent while the tester went on typing: what went up is what's marked, so the newer verdict still goes
        at(&mut all, "a", 1, "core", "a title").verdict = "worse".into();
        mark_sent(&mut all, &[("a".into(), "better".into(), String::new())], 12);
        assert!(all["a"].to_send());
    }

    #[test]
    fn a_note_is_kept_short() {
        assert_eq!(clean_note("  ok  "), "ok");
        assert_eq!(clean_note(&"é".repeat(900)).chars().count(), NOTE_MAX);
    }
}
