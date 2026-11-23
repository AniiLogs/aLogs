//! The run's island places, kept across a restart of Sanctum Helper (aLogs restarted, an update): the boat landing
//! the run began at and the island door last gone into a Sanctum by, with the run they belong to. Without them a
//! restarted helper starts every Sanctum at its orange door and looks for the player at all ten island doors again.
//! Its own file (`%LOCALAPPDATA%\SanctumHelper\run-places.json`), so nothing aLogs reads changes.

use super::{minutes_since, utc_now_text, Core, P2};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Runs last at most 20 minutes; older places belong to a run that's over.
const KEEP_MINUTES: f64 = 25.0;

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct RunPlaces {
    pub run_id: Option<String>,
    pub saved: String,
    pub spawn: Option<[f64; 2]>,
    pub door: Option<[f64; 2]>,
    pub door_kind: Option<String>,
}

fn path() -> PathBuf {
    crate::settings::dir().join("run-places.json")
}

impl RunPlaces {
    pub fn load() -> Option<RunPlaces> {
        serde_json::from_str(&std::fs::read_to_string(path()).ok()?).ok()
    }

    /// Written to a temp file and renamed, so a half-written file is never read.
    pub fn save(&self) {
        let p = path();
        let Ok(json) = serde_json::to_string(self) else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }

    pub fn forget() {
        let _ = std::fs::remove_file(path());
    }

    /// For the run going now: the same run as far as both know (either id unknown counts, as for the saved layout),
    /// saved in the last 25 minutes, with something in it.
    pub fn usable(&self, run_id: Option<&str>, age_minutes: Option<f64>) -> bool {
        let same_run = self.run_id.is_none() || run_id.is_none() || self.run_id.as_deref() == run_id;
        same_run && age_minutes.is_some_and(|a| (0.0..KEEP_MINUTES).contains(&a)) && (self.spawn.is_some() || self.door.is_some())
    }
}

fn pt(p: Option<[f64; 2]>) -> Option<P2> {
    p.map(|p| (p[0], p[1]))
}

impl Core {
    /// Keeps the run's boat landing and the door gone in by for a restart.
    pub(super) fn save_run_places(&self) {
        RunPlaces {
            run_id: self.game_log.run_id().map(str::to_string),
            saved: utc_now_text(),
            spawn: self.island_spawn.map(|p| [p.0, p.1]),
            door: self.island_door.map(|p| [p.0, p.1]),
            door_kind: self.island_door_kind.clone(),
        }
        .save();
    }

    /// Restarted mid-run: the run's boat landing and the door gone in by come back.
    pub(super) fn restore_run_places(&mut self) {
        let Some(p) = RunPlaces::load() else { return };
        if !p.usable(self.game_log.run_id(), minutes_since(&p.saved)) {
            return;
        }
        self.island_spawn = pt(p.spawn);
        self.island_door = pt(p.door);
        self.island_door_kind = p.door_kind.clone().filter(|_| p.door.is_some());
        crate::log::write(&format!(
            "island places restored: {}{}",
            if p.spawn.is_some() { "boat landing" } else { "no boat landing" },
            match (&self.island_door, self.island_door_kind.as_deref()) {
                (Some(_), Some(k)) => format!(", went in by the {}", super::track::door_name(Some(k))),
                _ => String::new(),
            }
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::RunPlaces;

    fn places(run: Option<&str>) -> RunPlaces {
        RunPlaces { run_id: run.map(str::to_string), spawn: Some([1.0, 2.0]), ..Default::default() }
    }

    #[test]
    fn same_run_and_fresh_only() {
        assert!(places(Some("a")).usable(Some("a"), Some(3.0)));
        assert!(!places(Some("a")).usable(Some("b"), Some(3.0)), "another run's places");
        assert!(places(None).usable(Some("b"), Some(3.0)), "run unknown when saved");
        assert!(places(Some("a")).usable(None, Some(3.0)), "game log not read now");
        assert!(!places(Some("a")).usable(Some("a"), Some(30.0)), "too old");
        assert!(!places(Some("a")).usable(Some("a"), None), "time unreadable");
        assert!(!places(Some("a")).usable(Some("a"), Some(-1.0)), "from the future (clock changed)");
        assert!(!RunPlaces { run_id: Some("a".into()), ..Default::default() }.usable(Some("a"), Some(1.0)), "nothing kept");
    }

    #[test]
    fn round_trips_and_reads_partial_files() {
        let p = RunPlaces { run_id: Some("r".into()), saved: "2026-11-23T00:00:00Z".into(), spawn: Some([1.5, 2.5]), door: Some([3.0, 4.0]), door_kind: Some("entrance_side".into()) };
        let back: RunPlaces = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        let partial: RunPlaces = serde_json::from_str(r#"{"Spawn":[1,2]}"#).unwrap();
        assert_eq!(partial.spawn, Some([1.0, 2.0]));
        assert!(partial.door.is_none());
    }
}
