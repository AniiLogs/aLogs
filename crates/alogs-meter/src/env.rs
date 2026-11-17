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

impl Env {
    pub fn new(res: PathBuf, data: PathBuf) -> Env {
        let schedule = Schedule::new(res.join("boss_rush_cycles.json"));
        Env { res, data, schedule, write_files: true, fixed_now: Cell::new(None) }
    }

    /// `time.time()`.
    pub fn now(&self) -> f64 {
        self.fixed_now.get().unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()))
    }

    /// `time.strftime('%Y%m%d_%H%M%S')` for a time, in the PC's time zone.
    pub fn local_stamp(&self, t: f64) -> String {
        use chrono::TimeZone;
        chrono::Local.timestamp_opt(t as i64, 0).single().map_or_else(String::new, |d| d.format("%Y%m%d_%H%M%S").to_string())
    }
}
