//! aLogs' plug-ins as the meter process sees them (plugins.json, the list the app reads too): whose switch is on, who
//! needs the game read, who owns which notice, and each plug-in's health. Port of aLogs B16 `bossbar/plugins.py`.

use alogs_pyval::{Dict, Value};
use std::collections::{HashMap, VecDeque};

/// The list of plug-ins: this crate's plugins.json, the one copy (the app builds it in too, and the installer ships it
/// beside the meter, crates/alogs-app/resources/meter), for builds and tests that don't have one next to them.
pub const BUNDLED: &str = include_str!("../plugins.json");

pub struct Manifest {
    pub raw: Value,
    /// `PLUGINS`: id -> plug-in, in the list's order.
    pub plugins: Vec<(String, Value)>,
}

impl Manifest {
    pub fn parse(text: &str) -> Option<Manifest> {
        let raw = Value::json_loads(text)?;
        let plugins = raw
            .get("plugins")
            .as_list()?
            .iter()
            .map(|p| Some((p.get("id").as_str()?.to_string(), p.clone())))
            .collect::<Option<Vec<_>>>()?;
        Some(Manifest { raw, plugins })
    }

    /// `_load()`: plugins.json from the first folder that has a readable one.
    pub fn load(folders: &[&std::path::Path]) -> Option<Manifest> {
        folders.iter().find_map(|f| std::fs::read_to_string(f.join("plugins.json")).ok().and_then(|t| Manifest::parse(&t)))
    }

    pub fn bundled() -> Manifest {
        Manifest::parse(BUNDLED).expect("bundled plugins.json")
    }

    pub fn plugin(&self, pid: &str) -> Option<&Value> {
        self.plugins.iter().find(|(id, _)| id == pid).map(|(_, p)| p)
    }

    /// The plug-in's own switch: `"meter"` is the DPS meter's (the app's), the others a setting.
    pub fn switched_on(&self, cfg: &Value, pid: &str, meter_on: bool) -> bool {
        let p = self.plugin(pid).expect("known plug-in");
        let sw = p.get("switch");
        if sw.py_eq(&"meter".into()) {
            return meter_on;
        }
        setting(cfg, &sw.py_str(), p.get("default")).truthy()
    }

    /// `{notice kind: shown?}`: a kind shows while the plug-in that owns it is on and running and its own notice
    /// setting (if any) is on. The core's only follow their setting.
    pub fn notice_owners(&self, cfg: &Value, meter_on: bool, health: &Health) -> Dict {
        let mut out = Dict::new();
        if let Some(core) = self.raw.get("core").get("notices").as_dict() {
            for (kind, ptr) in core.iter() {
                let v = setting(cfg, &ptr.py_str(), &Value::Bool(true));
                out.insert(kind.clone(), Value::Bool(!matches!(v, Value::Bool(false))));
            }
        }
        for (pid, p) in &self.plugins {
            let on = self.switched_on(cfg, pid, meter_on) && !health.failed(pid);
            if let Some(n) = p.get("notices").as_dict() {
                for (kind, ptr) in n.iter() {
                    let shown = on
                        && (ptr.is_none()
                            || !matches!(setting(cfg, &ptr.py_str(), &Value::Bool(true)), Value::Bool(false)));
                    out.insert(kind.clone(), Value::Bool(shown));
                }
            }
        }
        out
    }

    /// What the meter process does: `"on"` (the DPS meter is on), `"quiet"` (it's off but another plug-in that needs
    /// the game read is on) or `"rest"` (none is: nothing is read; notices still show).
    pub fn meter_mode(&self, meter_off: bool, cfg: &Value, health: &Health) -> &'static str {
        if !meter_off && !health.failed("dps") {
            return "on";
        }
        let reader = |p: &Value| p.get("needs").as_list().is_some_and(|n| n.iter().any(|x| x.py_eq(&"reader".into())));
        let any = self
            .plugins
            .iter()
            .filter(|(pid, p)| pid != "dps" && reader(p))
            .any(|(pid, _)| self.switched_on(cfg, pid, true) && !health.failed(pid));
        if any { "quiet" } else { "rest" }
    }

    /// The plug-in's log name (`meter.<log>`).
    pub fn log_name(&self, pid: &str) -> String {
        self.plugin(pid).map(|p| p.get("log")).filter(|l| !l.is_none()).map_or(pid.to_string(), Value::py_str)
    }
}

/// A settings.json value by its pointer (`"/leaderboard/submit"`); `default` when a step is missing.
pub fn setting(cfg: &Value, pointer: &str, default: &Value) -> Value {
    let mut at = cfg;
    for key in pointer.trim_matches('/').split('/') {
        match at.as_dict().and_then(|d| d.get_str(key)) {
            Some(v) => at = v,
            None => return default.clone(),
        }
    }
    at.clone()
}

/// Errors in a plug-in's code within [`FAIL_WINDOW`] s that stop it.
pub const FAILS: usize = 3;
pub const FAIL_WINDOW: f64 = 60.0;

/// Each plug-in's errors and whether it was stopped for them (`{id: why}`).
#[derive(Default)]
pub struct Health {
    errors: HashMap<String, VecDeque<f64>>,
    stopped: HashMap<String, String>,
    last: HashMap<String, String>,
    /// What `error` and `reset` logged, newest last (the Python logs them; tests read them).
    pub logged: Vec<(String, String)>,
}

impl Health {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn failed(&self, pid: &str) -> bool {
        self.stopped.contains_key(pid)
    }

    /// An error in the plug-in's code at `now` (unix seconds), in `where_`; `exc` is its `repr()`.
    pub fn error(&mut self, pid: &str, log_name: &str, exc: &str, where_: &str, now: f64) {
        let q = self.errors.entry(pid.to_string()).or_default();
        q.push_back(now);
        while q.front().is_some_and(|t| now - t > FAIL_WINDOW) {
            q.pop_front();
        }
        let n = q.len();
        let last: String = format!("{where_}: {exc}").chars().take(200).collect();
        self.last.insert(pid.to_string(), last.clone());
        let logger = format!("meter.{log_name}");
        log::error!(target: "meter", "{logger}: error in {where_} (the other plug-ins go on): {exc}");
        self.logged.push((logger.clone(), format!("error in {where_} (the other plug-ins go on)")));
        if n >= FAILS && !self.stopped.contains_key(pid) {
            self.stopped.insert(pid.to_string(), last);
            let msg = format!(
                "{n} errors in {} s: stopped until its switch is turned off and on again (or aLogs restarts)",
                FAIL_WINDOW as i64
            );
            log::error!(target: "meter", "{logger}: {msg}");
            self.logged.push((logger, msg));
        }
    }

    /// Its switch was turned off and on again: it gets another go.
    pub fn reset(&mut self, pid: &str) {
        if self.stopped.contains_key(pid) || self.errors.get(pid).is_some_and(|q| !q.is_empty()) {
            let msg = format!("plug-in {pid}: switched off and on again, running again");
            log::info!(target: "meter", "{msg}");
            self.logged.push(("meter".into(), msg));
        }
        self.stopped.remove(pid);
        self.errors.remove(pid);
    }

    /// `{'failed': why or None, 'last_error': ... or None}` for status.json.
    pub fn status(&self, pid: &str) -> Value {
        let opt = |v: Option<&String>| v.map_or(Value::None, |s| Value::Str(s.clone()));
        alogs_pyval::dict([("failed", opt(self.stopped.get(pid))), ("last_error", opt(self.last.get(pid)))])
    }

    /// `guarded(pid, fallback)`: run a plug-in's code; an error (or a panic) in it is the plug-in's (logged, counted),
    /// and a stopped plug-in's code doesn't run (it gives `fallback`).
    pub fn guarded<T>(
        &mut self,
        manifest: &Manifest,
        pid: &str,
        where_: &str,
        now: f64,
        fallback: T,
        f: impl FnOnce() -> Result<T, String>,
    ) -> T {
        if self.failed(pid) {
            return fallback;
        }
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        match r {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                self.error(pid, &manifest.log_name(pid), &e, where_, now);
                fallback
            }
            Err(p) => {
                let msg = p
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "panic".into());
                self.error(pid, &manifest.log_name(pid), &format!("panic({msg:?})"), where_, now);
                fallback
            }
        }
    }
}

/// Notices a plug-in's switch turned off and on again (in the settings the meter reloads), to give a stopped one
/// another go.
#[derive(Default)]
pub struct SwitchWatch {
    was: HashMap<String, bool>,
}

impl SwitchWatch {
    pub fn tick(&mut self, manifest: &Manifest, cfg: &Value, meter_on: bool, health: &mut Health) {
        for (pid, _) in &manifest.plugins {
            let on = manifest.switched_on(cfg, pid, meter_on);
            if on && self.was.get(pid) == Some(&false) {
                health.reset(pid);
            }
            self.was.insert(pid.clone(), on);
        }
    }
}
