//! The meter's background housekeeping: old screenshots, per-frame fight logs and test pictures cleared every hour,
//! and the week schedule, boss list and Interlink HP bars re-read from the game's data every few hours (and the
//! schedule sent to the site by a developer account). Port of aLogs B16 `bossbar/core.py` (clean_old_*,
//! screenshot_cleanup, refresh_cycles, upload_cycles).

use crate::meter::Meter;
use alogs_reader::boss::{self, Row};
use serde_json::Value as J;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

pub const IMAGE_TYPES: [&str; 5] = [".png", ".jpg", ".jpeg", ".webp", ".bmp"];
pub const TEXT_DIAGNOSTICS: [&str; 3] = ["battle_data", "nobar", "bar_lost"];
pub const TEST_FRAME_DAYS: i64 = 7;
pub const TEST_FRAME_BYTES: u64 = 3 << 30;
pub const FIGHT_LOG_DAYS: f64 = 30.0;
pub const LEFTOVER_HOURS: f64 = 6.0;
pub const KEEP_FOLDERS: [&str; 3] = ["submit_queue", "fights", "characters"];

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn mtime(p: &Path) -> Option<(f64, u64)> {
    let m = std::fs::metadata(p).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64();
    Some((t, m.len()))
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// The meter's screenshots older than keep_hours (None or < 0: keep everything; 0: LEFTOVER_HOURS), except proof
/// a queued record still has to send and the test pictures and character proofs. (files, bytes) removed. A fight's
/// compressed log in logs/screens (fight evidence) counts as one of its pictures.
pub fn clean_old_screenshots(data: &Path, keep_hours: Option<i64>) -> (u64, u64) {
    let Some(h) = keep_hours.filter(|h| *h >= 0) else { return (0, 0) };
    let cutoff = now() - if h == 0 { LEFTOVER_HOURS } else { h as f64 } * 3600.0;
    let root = data.join("logs");
    let mut needed = std::collections::HashSet::new();
    if let Ok(rd) = std::fs::read_dir(root.join("submit_queue")) {
        for e in rd.flatten() {
            if let Some(item) = std::fs::read_to_string(e.path()).ok().and_then(|s| serde_json::from_str::<J>(&s).ok()) {
                // every picture a queued record names (Submitter::enqueue_with_fight, link_character, the app's
                // disputes): kept until it is sent or set aside
                for key in ["evidence", "battle", "party", "proof"] {
                    if let Some(p) = item.get(key).and_then(J::as_str).filter(|p| !p.is_empty()) {
                        needed.insert(norm(&data.join(p)));
                    }
                }
                // a Holo-Battle Sim run's gizmo screens
                for p in item.get("gizmos").and_then(J::as_array).into_iter().flatten().filter_map(J::as_str).filter(|p| !p.is_empty()) {
                    needed.insert(norm(&data.join(p)));
                }
                // an Interlink run's fight evidence: its pictures and its compressed log (fight_shots.rs)
                let fight = item.get("fight").and_then(J::as_array).into_iter().flatten().filter_map(|e| e.get("file").and_then(J::as_str));
                for p in fight.chain(item.get("fightlog").and_then(J::as_str)).filter(|p| !p.is_empty()) {
                    needed.insert(norm(&data.join(p)));
                }
            }
        }
    }
    let keep_dirs: Vec<String> = ["characters", "test_frames"].iter().map(|k| norm(&root.join(k))).collect();
    let (mut files, mut size) = (0, 0);
    walk_bottom_up(&root, &mut |d: &Path, names: &[PathBuf]| {
        let dn = norm(d);
        if keep_dirs.iter().any(|k| dn.starts_with(k)) {
            return;
        }
        let base = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        for p in names {
            let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            // (a fight's compressed log kept to send beside its pictures goes with them)
            if !(IMAGE_TYPES.iter().any(|t| n.ends_with(t)) || (n.ends_with(".txt") && TEXT_DIAGNOSTICS.contains(&base.as_str())) || (n.ends_with(".csv.gz") && base == "screens")) {
                continue;
            }
            if needed.contains(&norm(p)) {
                continue;
            }
            if let Some((t, len)) = mtime(p)
                && t < cutoff
                && std::fs::remove_file(p).is_ok()
            {
                files += 1;
                size += len;
            }
        }
        if d != root && !KEEP_FOLDERS.contains(&base.as_str()) {
            let _ = std::fs::remove_dir(d); // only when it's empty now
        }
    });
    (files, size)
}

fn walk_bottom_up(d: &Path, f: &mut dyn FnMut(&Path, &[PathBuf])) {
    let Ok(rd) = std::fs::read_dir(d) else { return };
    let (mut dirs, mut names) = (vec![], vec![]);
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() { dirs.push(p) } else { names.push(p) }
    }
    for sub in &dirs {
        walk_bottom_up(sub, f);
    }
    f(d, &names);
}

/// Each fight's per-frame CSV (logs/2026..._Boss.csv) after `days`.
pub fn clean_old_fight_logs(data: &Path, days: f64) -> (u64, u64) {
    let cutoff = now() - days * 86400.0;
    let (mut files, mut size) = (0, 0);
    let Ok(rd) = std::fs::read_dir(data.join("logs")) else { return (0, 0) };
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if !(n.ends_with(".csv") && n.len() >= 8 && n[..8].chars().all(|c| c.is_ascii_digit())) {
            continue;
        }
        if let Some((t, len)) = mtime(&e.path())
            && t < cutoff
            && std::fs::remove_file(e.path()).is_ok()
        {
            files += 1;
            size += len;
        }
    }
    (files, size)
}

/// Save test pictures: a day's folder goes after `days`, and the oldest days first while all are over `limit`.
pub fn clean_test_frames(data: &Path, days: i64, limit: u64) -> (u64, u64) {
    let root = data.join("logs").join("test_frames");
    let Ok(rd) = std::fs::read_dir(&root) else { return (0, 0) };
    let mut day_names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.len() == 8 && n.chars().all(|c| c.is_ascii_digit())).collect();
    day_names.sort();
    let cutoff = crate::results::local(now() - days as f64 * 86400.0, "%Y%m%d");
    let today = crate::results::local(now(), "%Y%m%d");
    let mut sizes = std::collections::HashMap::new();
    for n in &day_names {
        let (mut count, mut total) = (0u64, 0u64);
        walk_bottom_up(&root.join(n), &mut |_, names| {
            for p in names {
                count += 1;
                total += mtime(p).map_or(0, |m| m.1);
            }
        });
        sizes.insert(n.clone(), (count, total));
    }
    let (mut files, mut size) = (0, 0);
    let mut left: u64 = sizes.values().map(|v| v.1).sum();
    for n in &day_names {
        if *n >= cutoff && left <= limit {
            continue;
        }
        if *n == today && *n >= cutoff {
            break;
        }
        let _ = std::fs::remove_dir_all(root.join(n));
        files += sizes[n].0;
        size += sizes[n].1;
        left -= sizes[n].1;
    }
    (files, size)
}

/// At start and every hour: old screenshots, per-frame fight logs and test pictures.
pub fn start_cleanup(data: PathBuf, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            let cfg = alogs_protocol::settings::load(&data);
            let hours = alogs_pyval::py_int(cfg.get("capture").get("keep_hours")).unwrap_or(0);
            let (f, s) = clean_old_screenshots(&data, Some(hours));
            if f > 0 {
                log::info!(target: "meter", "removed {f} screenshots older than {} hours ({:.0} MB)", if hours == 0 { LEFTOVER_HOURS as i64 } else { hours }, s as f64 / 1048576.0);
            }
            let (f, s) = clean_old_fight_logs(&data, FIGHT_LOG_DAYS);
            if f > 0 {
                log::info!(target: "meter", "removed {f} per-frame fight logs older than {FIGHT_LOG_DAYS} days ({:.1} MB)", s as f64 / 1048576.0);
            }
            let (f, s) = clean_test_frames(&data, TEST_FRAME_DAYS, TEST_FRAME_BYTES);
            if f > 0 {
                log::info!(target: "meter", "removed {f} test pictures (older than {TEST_FRAME_DAYS} days or over {} GB, {:.0} MB)", TEST_FRAME_BYTES >> 30, s as f64 / 1048576.0);
            }
            for _ in 0..3600 {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    });
}

/// What a game-data read found, for the meter's loop to take in.
pub enum GameData {
    /// The game data is older than this week: aLogs' own boss list and schedule.
    Stale,
    Bosses(Vec<Row>),
    Grades(Vec<Row>),
    Cycles(PathBuf),
}

fn rows_of(v: &J) -> Vec<Row> {
    v.as_array().map(|a| a.iter().filter_map(|r| r.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), match v {
        J::String(s) => s.clone(),
        J::Null => String::new(),
        v => alogs_pyval::Value::from_json(v).py_str(),
    })).collect())).collect()).unwrap_or_default()
}

/// The boss rush weeks, boss list and HP bars from the game's own data, at start and every 6 hours.
pub fn start_refresh(data: PathBuf, res: PathBuf, player_log: Option<PathBuf>, keep: Vec<(String, String)>, stop: Arc<AtomicBool>) -> Receiver<GameData> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            refresh_once(&data, &res, player_log.as_deref(), &keep, &tx);
            for _ in 0..6 * 3600 {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    });
    rx
}

/// A Python value as JSON.
fn j(v: &alogs_pyval::Value) -> J {
    serde_json::from_str(&v.json_dumps()).unwrap_or(J::Null)
}

fn refresh_once(data: &Path, res: &Path, player_log: Option<&Path>, keep: &[(String, String)], tx: &Sender<GameData>) {
    use alogs_gamedata::gamedata as gd;
    let cache = data.join("game_cycles.json");
    match gd::cached_cycles(&cache, player_log) {
        Ok((Some(pv), _)) => {
            let cycles = j(&pv);
            let shipped: Option<J> = std::fs::read_to_string(res.join("boss_rush_cycles.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
            let stale = cycles.as_array().is_some_and(|a| !a.is_empty())
                && !gd::covers_now(&pv, None).unwrap_or(false)
                && shipped.as_ref().is_some_and(|s| gd::covers_now(&alogs_pyval::Value::from_json(s), None).unwrap_or(false));
            if stale {
                let latest = cycles.as_array().and_then(|a| a.last()).map_or(J::Null, |c| c["cycle"].clone());
                log::warn!(target: "meter", "the game data at {} is older than this week (latest week {latest}): using aLogs' own boss list and week schedule instead",
                    gd::find_game_data(player_log).map_or("?".into(), |p| p.display().to_string()));
                let _ = tx.send(GameData::Stale);
                return;
            }
        }
        Ok(_) => {}
        Err(e) => log::warn!(target: "meter", "could not check the game data: {e}"),
    }
    match gd::cached_bosses(&data.join("game_bosses.json"), keep, player_log) {
        Ok((Some(bosses), changed)) => {
            let rows = rows_of(&j(&bosses));
            if changed {
                log::info!(target: "meter", "boss list read from the game data: {} rows", rows.len());
            }
            let _ = tx.send(GameData::Bosses(rows));
        }
        Ok(_) => {}
        Err(e) => log::warn!(target: "meter", "could not read the boss list from the game data: {e}"),
    }
    match gd::cached_grades(&data.join("game_grades.json"), player_log) {
        Ok((Some(grades), changed)) => {
            let rows = rows_of(&j(&grades));
            if changed {
                log::info!(target: "meter", "Interlink HP bars read from the game data: {} bars", rows.len().saturating_sub(1));
            }
            let _ = tx.send(GameData::Grades(rows));
        }
        Ok(_) => {}
        Err(e) => log::warn!(target: "meter", "could not read the Interlink HP bars from the game data: {e}"),
    }
    match gd::cached_cycles(&cache, player_log) {
        Ok((Some(pv), changed)) => {
            let cycles = j(&pv);
            let path = data.join("boss_rush_cycles.json");
            let on_disk: Option<J> = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok());
            if on_disk.as_ref() != Some(&cycles) {
                let _ = gd::write_json(&path, &pv);
                let latest = cycles.as_array().and_then(|a| a.last()).map_or(J::Null, |c| c["cycle"].clone());
                log::info!(target: "meter", "boss rush weeks read from the game data: {} (latest {latest})", cycles.as_array().map_or(0, Vec::len));
            }
            let _ = tx.send(GameData::Cycles(path));
            upload_cycles(data, res, &cycles, changed);
        }
        Ok(_) => {}
        Err(e) => log::warn!(target: "meter", "could not read the boss rush weeks from the game data: {e}"),
    }
}

/// Developer accounts: send the schedule to the site once per game data version (the site refuses others).
fn upload_cycles(data: &Path, res: &Path, cycles: &J, changed: bool) {
    let Some(cfg) = crate::submit::load_config(data) else { return };
    let marker = data.join("game_cycles.uploaded");
    let stamp = std::fs::read_to_string(data.join("game_cycles.json")).ok().and_then(|s| serde_json::from_str::<J>(&s).ok()).and_then(|v| v.get("stamp").and_then(J::as_str).map(String::from)).unwrap_or_default();
    let done = std::fs::read_to_string(&marker).map(|s| s.trim().to_string()).unwrap_or_default();
    if done == stamp && !changed {
        return;
    }
    let catalog = boss::load_catalog(res, None).unwrap_or_default();
    let names: std::collections::HashMap<String, String> = catalog.iter().filter(|r| r.get("table").map(String::as_str) == Some("puppet_data")).filter_map(|r| Some((r.get("puppetId")?.clone(), r.get("bossTitle_en")?.clone()))).collect();
    let boss = |b: &J| -> J {
        let id = match b { J::String(s) => s.clone(), v => v.to_string() };
        serde_json::json!({"id": id, "name": names.get(&id).cloned().unwrap_or_else(|| id.clone())})
    };
    let list: Vec<J> = cycles.as_array().map(|a| a.iter().filter(|c| !c["start"].is_null() && !c["end"].is_null()).map(|c| serde_json::json!({
        "id": c["cycle"], "season": c.get("season").cloned().unwrap_or(J::Null), "start": c["start"], "end": c["end"],
        "bosses": c["bossIds"].as_array().map(|b| b.iter().map(boss).collect::<Vec<_>>()).unwrap_or_default(),
        "bosses2": c.get("bossIds2").and_then(J::as_array).map(|b| b.iter().map(|x| if x.is_null() || x == &J::from(0) { J::Null } else { boss(x) }).collect::<Vec<_>>()).unwrap_or_else(|| vec![J::Null; 3]),
        "split": c.get("split").cloned().unwrap_or(J::Null),
    })).collect()).unwrap_or_default();
    let body = serde_json::json!({"cycles": list});
    let url = format!("{}/leaderboard/interlink/cycles", cfg.api_url);
    let version = std::env::var("ALOGS_VERSION").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "meter-0.1".into());
    match ureq::post(&url).set("authorization", &format!("Bearer {}", cfg.token)).set("content-type", "application/json").set("user-agent", &version)
        .timeout(std::time::Duration::from_secs(20)).send_string(&body.to_string())
    {
        Ok(r) => {
            log::info!(target: "meter", "boss rush weeks sent to the site: HTTP {}", r.status());
            let _ = std::fs::write(&marker, &stamp);
        }
        Err(ureq::Error::Status(403, _)) => {
            log::info!(target: "meter", "boss rush weeks: only developer accounts update the site");
            let _ = std::fs::write(&marker, &stamp);
        }
        Err(ureq::Error::Status(code, _)) => log::warn!(target: "meter", "boss rush weeks: the site answered HTTP {code}"),
        Err(e) => log::warn!(target: "meter", "boss rush weeks: could not reach the site ({e})"),
    }
}

impl Meter {
    /// Take in what the game-data refresh found (the boss list, HP bars and weeks).
    pub fn take_game_data(&mut self, rx: &Receiver<GameData>) {
        while let Ok(g) = rx.try_recv() {
            match g {
                GameData::Stale => {
                    self.catalog = boss::load_catalog(&self.env.res, None).unwrap_or_default();
                    self.grades = boss::load_grades(&self.env.res, None).unwrap_or_default();
                    self.env.schedule = boss::Schedule::new(self.env.res.join("boss_rush_cycles.json"));
                }
                GameData::Bosses(rows) => {
                    let had: std::collections::HashSet<(String, String)> = self.catalog.iter().map(|r| (r["table"].clone(), r["puppetId"].clone())).collect();
                    self.catalog = boss::load_catalog(&self.env.res, Some(&rows)).unwrap_or_default();
                    let new: std::collections::BTreeSet<String> = self.catalog.iter().filter(|r| !had.contains(&(r["table"].clone(), r["puppetId"].clone())))
                        .map(|r| if r["bossTitle_en"].is_empty() { r.get("name_en").cloned().unwrap_or_default() } else { r["bossTitle_en"].clone() }).collect();
                    if !new.is_empty() && !had.is_empty() {
                        log::info!(target: "meter", "boss list: new {:?}", new.iter().take(10).collect::<Vec<_>>());
                    }
                }
                GameData::Grades(rows) => self.grades = boss::load_grades(&self.env.res, Some(&rows)).unwrap_or_default(),
                GameData::Cycles(p) => {
                    self.env.schedule = boss::Schedule::new(p);
                    self.bot_names = crate::results_parse::load_bot_names(&self.env.data);
                }
            }
        }
    }
}
