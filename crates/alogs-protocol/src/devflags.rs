//! Dev switches (Beta Menu > Dev): whether a proposed change is turned on. Port of aLogs B16 `bossbar/devflags.py`.
//!
//! The app writes the switches that are on to a small file and, only on a beta build, tells the meter where it is
//! (`ALOGS_DEV_ON`). Without it every switch is off and nothing is read. `on()` looks at the file at most once a
//! second and never fails.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

pub const CHECK: Duration = Duration::from_secs(1);

pub struct DevFlags {
    path: Option<PathBuf>,
    on: BTreeSet<String>,
    stamp: Option<(SystemTime, u64)>,
    next: Option<Instant>,
}

impl DevFlags {
    /// As the meter starts: the file `ALOGS_DEV_ON` names, if any.
    pub fn from_env() -> DevFlags {
        DevFlags::new(std::env::var_os("ALOGS_DEV_ON").filter(|p| !p.is_empty()).map(PathBuf::from))
    }

    /// `use(path)`: read this file from now on (`None`: as on a release build).
    pub fn new(path: Option<PathBuf>) -> DevFlags {
        DevFlags { path, on: BTreeSet::new(), stamp: None, next: None }
    }

    /// Is this switch on? False on anything unexpected.
    pub fn on(&mut self, option: &str) -> bool {
        if self.path.is_none() {
            return false;
        }
        let now = Instant::now();
        if self.next.is_none_or(|n| now >= n) {
            self.next = Some(now + CHECK);
            self.look();
        }
        self.on.contains(option)
    }

    /// The switches that are on now.
    pub fn all_on(&self) -> &BTreeSet<String> {
        &self.on
    }

    fn read(&self) -> Option<((SystemTime, u64), Option<BTreeSet<String>>)> {
        let path = self.path.as_ref()?;
        let meta = std::fs::metadata(path).ok()?;
        let stamp = (meta.modified().ok()?, meta.len());
        if Some(stamp) == self.stamp {
            return Some((stamp, None));
        }
        let text = std::fs::read_to_string(path).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let list = v.get("on")?.as_array()?;
        let found = list.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
        Some((stamp, Some(found)))
    }

    /// The file, if it changed. Gone or unreadable: all off.
    fn look(&mut self) {
        let found = match self.read() {
            Some((_, None)) => return,
            Some((stamp, Some(found))) => {
                self.stamp = Some(stamp);
                found
            }
            None => {
                self.stamp = None;
                BTreeSet::new()
            }
        };
        if found != self.on {
            let names: Vec<&str> = found.iter().map(String::as_str).collect();
            let list = if names.is_empty() { "none".to_string() } else { names.join(", ") };
            log::info!(target: "meter", "dev switches on: {list}");
            self.on = found;
        }
    }
}
