//! What the meter's logic needs from outside: the clock, where its files are, the week schedule.

use alogs_reader::boss::Schedule;
use std::cell::Cell;
use std::path::PathBuf;

pub struct Env {
    /// Read-only files shipped with the meter (boss_catalog.csv, xn_templates.json ...): the Python's RES.
    pub res: PathBuf,
    /// What the meter writes (logs, records, calibration): the Python's DATA / HERE.
    pub data: PathBuf,
    pub schedule: Schedule,
    /// Write per-fight CSVs and pictures (off in tests).
    pub write_files: bool,
    /// A fixed clock for tests (None: the PC's).
    pub fixed_now: Cell<Option<f64>>,
}

/// Env::record_clock's clock.
pub struct RecordClock {
    now: f64,
    fixed: bool,
}

impl alogs_gamelog::Clock for RecordClock {
    fn now(&self) -> f64 {
        self.now
    }
    fn local_to_unix(&self, y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Option<f64> {
        if self.fixed {
            alogs_gamelog::FixedClock(self.now).local_to_unix(y, mo, d, h, mi, s)
        } else {
            alogs_gamelog::SystemClock.local_to_unix(y, mo, d, h, mi, s)
        }
    }
}

impl Env {
    pub fn new(res: PathBuf, data: PathBuf) -> Env {
        let schedule = Schedule::new(res.join("boss_rush_cycles.json"));
        Env { res, data, schedule, write_files: true, fixed_now: Cell::new(None) }
    }

    /// `time.time()`.
    pub fn now(&self) -> f64 {
        self.fixed_now.get().unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()))
    }

    /// The clock records are built with (every payload builder's `clock`): now is `now()`; a local wall time (a run's
    /// "started") is read in the PC's time zone. Only with `fixed_now` (tests) is it the fixed UTC clock, so the
    /// goldens stay as they are. (FixedClock outside tests sent every "played at" off by the PC's time zone.)
    pub fn record_clock(&self) -> RecordClock {
        RecordClock { now: self.now(), fixed: self.fixed_now.get().is_some() }
    }

    /// `time.strftime('%Y%m%d_%H%M%S')` for a time, in the PC's time zone.
    pub fn local_stamp(&self, t: f64) -> String {
        use chrono::TimeZone;
        chrono::Local.timestamp_opt(t as i64, 0).single().map_or_else(String::new, |d| d.format("%Y%m%d_%H%M%S").to_string())
    }
}
