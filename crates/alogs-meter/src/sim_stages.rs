//! Holo-Battle Sim: a record of a run's levels, sent with its win (`extra.stages`) so the site can check a run for
//! skipped levels. Raw facts with their times only: every change of the "n/N" counter at the top left, every area
//! start in the Sim's space from the game's log, every fight the meter followed there, and how many level reads were
//! made. Nothing here judges, warns or shows anything.
//!
//! A run can be paused, left and continued later (also after aLogs or the game restarted), so the record is made of
//! visits, one per entry into a Sim space, and kept on disk per character (`logs/sim_stages.json`, keyed by UID). A
//! visit continues the kept record when its first level read carries on from it (the same N, n not lower, not 1/N);
//! otherwise it starts a new record. Times are unix seconds with one decimal.

use crate::fights::Fight;
use crate::meter::Meter;
use alogs_pil::Image;
use alogs_reader::prep;
use indexmap::IndexMap;
use serde_json::{Map, Value as J, json};
use std::path::{Path, PathBuf};

/// The newest visits sent, and the most levels, area starts and fights kept a visit; past them `cut` is set.
pub const MAX_VISITS: usize = 12;
pub const MAX_LEVELS: usize = 40;
pub const MAX_AREAS: usize = 80;
pub const MAX_FIGHTS: usize = 80;
/// A record whose last visit is older than this is dropped when it is loaded.
pub const KEEP_S: f64 = 14.0 * 86400.0;
/// The level counter is read at most this often (the exit gate watch's own faster reads are used in its place), in
/// and out of fights; the first read after an area start doesn't wait for it.
pub const READ_EVERY_S: f64 = 1.0;
/// One read of the level after the last one is that level when it comes this soon after an area start (the area
/// start is its second witness): a level that is a fight almost from its first second gets few reads.
pub const AREA_WITNESS_S: f64 = 10.0;
/// The key of the counter's crop when it rides along with the boss bar's text read (in a fight, or a bar up).
pub const BAR_KEY: &str = "sim_level";
/// Read counts alone are written this often at most (levels, areas, fights and visits are written at once).
pub const SAVE_READS_S: f64 = 30.0;
pub const FILE: &str = "sim_stages.json";

#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub n: i64,
    pub of: i64,
    pub t: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimFight {
    /// When the fight ended.
    pub t: f64,
    /// Its length, seconds.
    pub s: f64,
    pub damage: i64,
    /// The boss's puppet id.
    pub id: String,
    pub defeated: bool,
    pub breaks: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Visit {
    pub entered_at: f64,
    pub left_at: Option<f64>,
    /// False when the meter started in the middle of the visit.
    pub from_start: bool,
    pub levels: Vec<Level>,
    pub areas: Vec<f64>,
    pub fights: Vec<SimFight>,
    /// Level reads made, and how many of them read a plausible counter.
    pub made: u64,
    pub counter: u64,
}

impl Visit {
    fn last_t(&self) -> f64 {
        let mut t = self.entered_at.max(self.left_at.unwrap_or(0.0));
        t = self.levels.iter().map(|l| l.t).chain(self.areas.iter().copied()).chain(self.fights.iter().map(|f| f.t)).fold(t, f64::max);
        t
    }
    fn saw_something(&self) -> bool {
        !self.levels.is_empty() || !self.areas.is_empty() || !self.fights.is_empty() || self.made > 0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub visits: Vec<Visit>,
    /// Something was left out to keep it bounded.
    pub cut: bool,
}

fn r1(t: f64) -> f64 {
    (t * 10.0).round() / 10.0
}

impl Record {
    /// N, from the newest level recorded.
    pub fn of(&self) -> Option<i64> {
        self.last_level().map(|l| l.of)
    }
    pub fn last_level(&self) -> Option<&Level> {
        self.visits.iter().rev().find_map(|v| v.levels.last())
    }
    fn areas(&self) -> usize {
        self.visits.iter().map(|v| v.areas.len()).sum()
    }
    fn levels(&self) -> usize {
        self.visits.iter().map(|v| v.levels.len()).sum()
    }
    fn fights(&self) -> usize {
        self.visits.iter().map(|v| v.fights.len()).sum()
    }
    pub fn last_t(&self) -> f64 {
        self.visits.last().map_or(0.0, Visit::last_t)
    }

    /// The record as sent (`extra.stages`) and as kept on disk.
    pub fn to_json(&self) -> J {
        let visits: Vec<J> = self
            .visits
            .iter()
            .map(|v| {
                let mut o = Map::new();
                o.insert("enteredAt".into(), json!(r1(v.entered_at)));
                if let Some(l) = v.left_at {
                    o.insert("leftAt".into(), json!(r1(l)));
                }
                o.insert("fromStart".into(), json!(v.from_start));
                o.insert("levels".into(), J::Array(v.levels.iter().map(|l| json!({"n": l.n, "of": l.of, "t": r1(l.t)})).collect()));
                o.insert("areas".into(), J::Array(v.areas.iter().map(|t| json!({"t": r1(*t)})).collect()));
                o.insert(
                    "fights".into(),
                    J::Array(
                        v.fights
                            .iter()
                            .map(|f| json!({"t": r1(f.t), "s": r1(f.s), "damage": f.damage, "id": f.id, "defeated": f.defeated, "breaks": f.breaks}))
                            .collect(),
                    ),
                );
                o.insert("reads".into(), json!({"made": v.made, "counter": v.counter}));
                J::Object(o)
            })
            .collect();
        let mut o = Map::new();
        o.insert("of".into(), self.of().map_or(J::Null, |n| json!(n)));
        o.insert("visits".into(), J::Array(visits));
        if self.cut {
            o.insert("cut".into(), json!(true));
        }
        J::Object(o)
    }

    pub fn from_json(j: &J) -> Option<Record> {
        let f = |o: &J, k: &str| o.get(k).and_then(J::as_f64);
        let i = |o: &J, k: &str| o.get(k).and_then(J::as_i64);
        let list = |o: &J, k: &str| o.get(k).and_then(J::as_array).cloned().unwrap_or_default();
        let visits = j.get("visits")?.as_array()?;
        let mut rec = Record { visits: vec![], cut: j.get("cut").and_then(J::as_bool).unwrap_or(false) };
        for v in visits {
            rec.visits.push(Visit {
                entered_at: f(v, "enteredAt")?,
                left_at: f(v, "leftAt"),
                from_start: v.get("fromStart").and_then(J::as_bool).unwrap_or(true),
                levels: list(v, "levels").iter().filter_map(|l| Some(Level { n: i(l, "n")?, of: i(l, "of")?, t: f(l, "t")? })).collect(),
                areas: list(v, "areas").iter().filter_map(|a| f(a, "t")).collect(),
                fights: list(v, "fights")
                    .iter()
                    .filter_map(|x| {
                        Some(SimFight {
                            t: f(x, "t")?,
                            s: f(x, "s").unwrap_or(0.0),
                            damage: i(x, "damage").unwrap_or(0),
                            id: x.get("id").and_then(J::as_str).unwrap_or("").to_string(),
                            defeated: x.get("defeated").and_then(J::as_bool).unwrap_or(false),
                            breaks: i(x, "breaks").unwrap_or(0),
                        })
                    })
                    .collect(),
                made: v.get("reads").and_then(|r| r.get("made")).and_then(J::as_u64).unwrap_or(0),
                counter: v.get("reads").and_then(|r| r.get("counter")).and_then(J::as_u64).unwrap_or(0),
            });
        }
        Some(rec)
    }
}

/// The counter's "n/N" in a read, as the exit gate watch reads it.
pub fn parse_counter(text: &str) -> Option<(i64, i64)> {
    static RX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(\d{1,2})\s*/\s*(\d{1,2})").unwrap());
    let c = RX.captures(text)?;
    Some((c[1].parse().ok()?, c[2].parse().ok()?))
}

/// Whether a visit whose first level is `first` carries on the record `kept` (its visits before this one).
pub fn continues(kept: &[Visit], first: (i64, i64)) -> bool {
    let (n, of) = first;
    if n == 1 {
        return false;
    }
    match kept.iter().rev().find_map(|v| v.levels.last()) {
        Some(l) => l.of == of && n >= l.n,
        None => true,
    }
}

/// The meter's side of it: the record held for one character, whether a visit is open, the read waiting for its
/// second look.
#[derive(Default)]
pub struct Stages {
    pub rec: Option<Record>,
    /// Whose record is held; None until the game's log gave the UID (it is then merged with the kept one).
    pub uid: Option<String>,
    /// The record's last visit is the one going on now.
    pub open: bool,
    /// The open visit's first level was read (it continued the record or started a new one).
    decided: bool,
    /// A plausible new counter read once, waiting for the next look to agree: (n, N, when).
    cand: Option<(i64, i64, f64)>,
    pub last_read: f64,
    /// An area start came since the last read: the next read is made as soon as a picture allows.
    pub force: bool,
    /// The last area start in the open visit, until a read after it confirmed a level from one look.
    area_at: Option<f64>,
    /// The last area start seen in the log: (space, when, from the look back at the meter's start).
    last_enter: Option<(String, f64, bool)>,
    enters: u64,
    dirty: bool,
    reads_dirty: bool,
    saved_at: f64,
}

impl Stages {
    fn visit(&mut self) -> Option<&mut Visit> {
        if !self.open {
            return None;
        }
        self.rec.as_mut()?.visits.last_mut()
    }

    /// A visit starts. Returns the log line.
    pub fn visit_start(&mut self, entered_at: f64, from_start: bool) -> String {
        let rec = self.rec.get_or_insert_with(Record::default);
        let kept = match rec.last_level() {
            Some(l) => format!("a kept record at {}/{} ({} visit{}): it continues if the counter carries on", l.n, l.of, rec.visits.len(), if rec.visits.len() == 1 { "" } else { "s" }),
            None if !rec.visits.is_empty() => format!("a kept record with no level yet ({} visit{}): it continues unless the counter starts over", rec.visits.len(), if rec.visits.len() == 1 { "" } else { "s" }),
            None => "no kept record: a new one".to_string(),
        };
        if rec.visits.len() >= MAX_VISITS {
            rec.visits.remove(0);
            rec.cut = true;
        }
        rec.visits.push(Visit { entered_at, from_start, ..Default::default() });
        let n = rec.visits.len();
        self.open = true;
        self.decided = false;
        self.cand = None;
        self.area_at = None;
        self.dirty = true;
        format!("Holo-Battle Sim: visit {n} starts{}; {kept}", if from_start { "" } else { " (the meter started in the middle of it)" })
    }

    pub fn visit_end(&mut self, now: f64) {
        if let Some(v) = self.visit() {
            v.left_at = Some(now);
            self.dirty = true;
        }
        self.open = false;
        self.cand = None;
        self.force = false;
        self.area_at = None;
    }

    /// Whether a level read is due: a second after the last one, or at once after an area start.
    pub fn due(&self, now: f64) -> bool {
        self.force || now - self.last_read >= READ_EVERY_S
    }

    pub fn area(&mut self, t: f64) {
        if !self.open {
            return;
        }
        self.force = true;
        self.area_at = Some(t);
        let Some(v) = self.visit() else { return };
        if v.areas.len() < MAX_AREAS {
            v.areas.push(t);
        } else if let Some(r) = self.rec.as_mut() {
            r.cut = true;
        }
        self.dirty = true;
    }

    pub fn fight(&mut self, f: SimFight) {
        let Some(v) = self.visit() else { return };
        if v.fights.len() < MAX_FIGHTS {
            v.fights.push(f);
        } else if let Some(r) = self.rec.as_mut() {
            r.cut = true;
        }
        self.dirty = true;
    }

    /// One level read (the OCR's text, None when nothing came back). Returns the log line of a level change.
    pub fn read(&mut self, text: Option<&str>, now: f64) -> Option<String> {
        self.last_read = now;
        self.force = false;
        let decided = self.decided;
        let of_now = self.rec.as_ref().and_then(Record::of);
        let v = self.visit()?;
        v.made += 1;
        self.reads_dirty = true;
        let got = text.and_then(parse_counter).filter(|&(n, of)| of >= 2 && 1 <= n && n <= of && (!decided || of_now.is_none_or(|o| o == of)));
        let Some((n, of)) = got else {
            self.cand = None;
            return None;
        };
        let v = self.visit()?;
        v.counter += 1;
        if v.levels.last().is_some_and(|l| l.n == n && l.of == of) {
            self.cand = None;
            return None;
        }
        // the level after the last one, read once soon after an area start: the area start is the second look
        let next = self.rec.as_ref().and_then(Record::last_level).is_some_and(|l| l.of == of && n == l.n + 1);
        if next && self.area_at.is_some_and(|a| now >= a && now - a <= AREA_WITNESS_S) {
            self.cand = None;
            self.area_at = None;
            return Some(format!("{} (one read, after an area start)", self.level(n, of, now)));
        }
        match self.cand {
            Some((cn, cof, t)) if cn == n && cof == of => {
                self.cand = None;
                Some(self.level(n, of, t))
            }
            _ => {
                self.cand = Some((n, of, now));
                None
            }
        }
    }

    fn level(&mut self, n: i64, of: i64, t: f64) -> String {
        let rec = self.rec.as_mut().expect("a visit is open");
        let mut note = String::new();
        if !self.decided {
            self.decided = true;
            let k = rec.visits.len() - 1;
            if k > 0 {
                if continues(&rec.visits[..k], (n, of)) {
                    note = " - continues the kept record".into();
                } else {
                    let old = rec.last_level().map_or("no level".into(), |l| format!("{}/{}", l.n, l.of));
                    note = format!(" - a new record (the kept one at {old} dropped)");
                    let v = rec.visits.pop().unwrap();
                    *rec = Record { visits: vec![v], cut: false };
                }
            }
        } else if n == 1 && rec.levels() > 0 {
            // the counter started over within the visit: a new run, from here
            let since = rec.last_level().map_or(0.0, |l| l.t);
            let v = rec.visits.last().unwrap();
            let fresh = Visit {
                entered_at: v.entered_at,
                left_at: None,
                from_start: v.from_start,
                areas: v.areas.iter().copied().filter(|a| *a > since).collect(),
                fights: v.fights.iter().filter(|f| f.t > since).cloned().collect(),
                ..Default::default()
            };
            note = " - the counter started over: a new record".into();
            *rec = Record { visits: vec![fresh], cut: false };
        }
        let prev = rec.last_level().map(|l| l.t);
        let visit_no = rec.visits.len();
        let areas = rec.areas();
        let v = rec.visits.last_mut().unwrap();
        if v.levels.len() < MAX_LEVELS {
            v.levels.push(Level { n, of, t });
        } else {
            rec.cut = true;
        }
        self.dirty = true;
        let after = prev.map_or("the record's first level".into(), |p| format!("{:.0} s after the last level", t - p));
        format!("Holo-Battle Sim: level {n}/{of} (visit {visit_no}, area start {areas}, {after}){note}")
    }

    /// The record for the win's `extra.stages`, cleared here. None when nothing was seen.
    pub fn take(&mut self) -> Option<(J, String)> {
        let rec = self.rec.take();
        self.open = false;
        self.decided = false;
        self.cand = None;
        self.force = false;
        self.area_at = None;
        self.dirty = true;
        let rec = rec.filter(|r| r.visits.iter().any(Visit::saw_something))?;
        let line = format!(
            "Holo-Battle Sim: the run's levels go out with it: {} visits, {} levels seen, {} areas, {} fights{}",
            rec.visits.len(),
            rec.levels(),
            rec.areas(),
            rec.fights(),
            if rec.cut { " (cut to fit)" } else { "" }
        );
        Some((rec.to_json(), line))
    }
}

/// Every character's kept record: `{uid: record}`.
fn read_file(path: &Path) -> Map<String, J> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<J>(&t).ok()).and_then(|j| j.as_object().cloned()).unwrap_or_default()
}

/// A character's kept record, None when there is none or it is older than KEEP_S.
pub fn load(path: &Path, uid: &str, now: f64) -> Option<Record> {
    let rec = Record::from_json(read_file(path).get(uid)?)?;
    (now - rec.last_t() <= KEEP_S && !rec.visits.is_empty()).then_some(rec)
}

/// Writes a character's record (None: removes it), the whole file at once.
pub fn save(path: &Path, uid: &str, rec: Option<&Record>) {
    let mut all = read_file(path);
    match rec {
        Some(r) => {
            all.insert(uid.to_string(), r.to_json());
        }
        None => {
            if all.remove(uid).is_none() {
                return;
            }
        }
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = alogs_protocol::settings::write_whole(path, &J::Object(all).to_string());
}

impl Meter {
    fn stages_path(&self) -> PathBuf {
        self.env.data.join("logs").join(FILE)
    }

    /// The record of the character playing: loaded when the UID is first known (or changes); what was noted before
    /// the UID came is added to it.
    fn stages_sync(&mut self) {
        let Some(uid) = self.uid.clone() else { return };
        if self.stages.uid.as_ref() == Some(&uid) {
            return;
        }
        let path = self.stages_path();
        let had_uid = self.stages.uid.is_some();
        if had_uid {
            self.stages_save(true);
        }
        let kept = load(&path, &uid, self.env.now());
        let mem = self.stages.rec.take().filter(|_| !had_uid);
        self.stages.uid = Some(uid);
        self.stages.rec = match (kept, mem) {
            (Some(mut k), Some(m)) => {
                let first = m.visits.iter().find_map(|v| v.levels.first()).map(|l| (l.n, l.of));
                if first.is_none_or(|f| continues(&k.visits, f)) {
                    k.visits.extend(m.visits);
                    let over = k.visits.len().saturating_sub(MAX_VISITS);
                    if over > 0 {
                        k.visits.drain(..over);
                        k.cut = true;
                    }
                    k.cut |= m.cut;
                    Some(k)
                } else {
                    Some(m)
                }
            }
            (k, m) => m.or(k),
        };
        if had_uid {
            self.stages.open = false;
        }
        self.stages.dirty = true;
        self.stages_save(true);
    }

    fn stages_save(&mut self, force: bool) {
        let now = self.env.now();
        let due = self.stages.dirty || (self.stages.reads_dirty && (force || now - self.stages.saved_at >= SAVE_READS_S));
        if !due || !self.env.write_files {
            return;
        }
        let Some(uid) = self.stages.uid.clone() else { return };
        save(&self.stages_path(), &uid, self.stages.rec.as_ref().filter(|r| !r.visits.is_empty()));
        self.stages.dirty = false;
        self.stages.reads_dirty = false;
        self.stages.saved_at = now;
    }

    /// An area start in the game's log (Event::Enter), with the Sim flag before (`was_sim`) and after it.
    pub fn stages_enter(&mut self, space: &str, delay: Option<f64>, was_sim: bool) {
        if self.args.replay {
            return;
        }
        self.stages_sync();
        let now = self.env.now();
        self.stages.enters += 1;
        let back = delay.is_none() && self.stages.enters == 1;
        let t = now - delay.unwrap_or(0.0).max(0.0);
        self.stages.last_enter = Some((space.to_string(), t, back));
        if was_sim && !self.holosim_space {
            self.stages.visit_end(now);
        } else if self.holosim_space && !self.stages.open {
            let line = self.stages.visit_start(if back { now } else { t }, !back);
            log::info!(target: "meter", "{line}");
            if !back {
                self.stages.area(t);
            }
        } else if self.holosim_space && !back {
            self.stages.area(t);
        }
        self.stages_save(false);
    }

    /// The Sim's scene line (Event::HoloSim) made this space a Sim one: a visit starts with the area start before it.
    pub fn stages_holosim(&mut self, was_sim: bool) {
        if self.args.replay || was_sim || !self.holosim_space || self.stages.open {
            return;
        }
        self.stages_sync();
        let now = self.env.now();
        let (t, back) = match &self.stages.last_enter {
            Some((s, t, back)) if Some(s) == self.space.as_ref() => (*t, *back),
            _ => (now, true),
        };
        let line = self.stages.visit_start(if back { now } else { t }, !back);
        log::info!(target: "meter", "{line}");
        if !back {
            self.stages.area(t);
        }
        self.stages_save(false);
    }

    /// A fight followed to its end in a Sim room.
    pub fn stages_fight(&mut self, f: &Fight, now: f64) {
        if !self.holosim_space || self.args.replay {
            return;
        }
        self.stages_sync();
        self.stages.fight(SimFight {
            t: now,
            s: f.elapsed,
            damage: f.damage,
            id: f.row.get("puppetId").cloned().unwrap_or_default(),
            defeated: f.defeated,
            breaks: f.brk.breaks,
        });
        self.stages_save(false);
    }

    /// A read of the level counter's box (the exit gate watch's or our own).
    pub fn stages_feed(&mut self, text: Option<&str>, now: f64) {
        if !self.stages.open {
            return;
        }
        self.stages_sync();
        if let Some(line) = self.stages.read(text, now) {
            log::info!(target: "meter", "{line}");
        }
        self.stages_save(false);
    }

    /// About once a second (at once after an area start), in a Sim room with no boss bar or fight on and aLogs not
    /// holding the screen: reads the level counter, unless the exit gate watch read it in the last second. In a fight
    /// or with a bar up it is read along with the bar's text instead (`stages_bar_crop`).
    pub fn stages_look(&mut self, im: &Image, now: f64, foreground: bool, screen_copy: bool) {
        if !self.holosim_space || self.args.replay || !self.stages.open || self.fight.is_some() || self.bar.is_some() || self.sim.tour.is_some() || self.sim.ctl.is_some() {
            return;
        }
        // a copy of the screen with the game behind another window isn't a picture of the game
        if screen_copy && !foreground {
            return;
        }
        if !self.stages.due(now) {
            return;
        }
        let Some(crop) = self.stage_counter_crop(im) else { return };
        let mut c = IndexMap::new();
        c.insert("l".to_string(), crop);
        let text = self.ocr.read(&c, &IndexMap::new(), &IndexMap::new()).get("l").cloned().unwrap_or_default();
        self.stages_feed(Some(&text), now);
    }

    /// In a fight or with a boss bar up, on a look whose boss bar text is read: the counter's crop to read along with
    /// it (on the bar's background OCR helper live, so no look waits for it), when a level read is due and this look's
    /// picture holds the counter's box. The read is counted as made now; its text comes back to `stages_feed` with
    /// this time.
    pub fn stages_bar_crop(&mut self, im: &Image, now: f64, foreground: bool, screen_copy: bool) -> Option<Image> {
        if !self.holosim_space || self.args.replay || !self.stages.open || self.sim.tour.is_some() || self.sim.ctl.is_some() {
            return None;
        }
        if (screen_copy && !foreground) || !self.stages.due(now) {
            return None;
        }
        let crop = self.stage_counter_crop(im)?;
        self.stages.last_read = now; // (the pace holds while the read is on its way)
        self.stages.force = false;
        Some(crop)
    }

    /// The frame's measures: the frame's size (a fight's top-part picture is only the frame's top), or the picture's.
    fn stage_frame(&self, im: &Image) -> (f64, f64) {
        if self.frame_size.0 > 0 && self.frame_size.1 > 0 { (self.frame_size.0 as f64, self.frame_size.1 as f64) } else { (im.width as f64, im.height as f64) }
    }

    /// The level counter's box prepared for the OCR ("4/16" at the top left, as the exit gate watch reads it), or None
    /// when this look's picture doesn't hold the box (a fight's top-part picture, the frame's top 0.42, holds it; a
    /// band under a boss bar high up may not).
    pub fn stage_counter_crop(&self, im: &Image) -> Option<Image> {
        let (w, h) = self.stage_frame(im);
        let u = w.min(h * 16.0 / 9.0);
        let ux = (w - u) / 2.0;
        let b = (ux + u * 0.02, h * 0.015, ux + u * 0.20, h * 0.08);
        if (im.width as f64) < b.2.ceil() || (im.height as f64) < b.3.ceil() {
            return None;
        }
        Some(prep::prep_bright(im, b, u / 2000.0, 2.0))
    }

    /// The record for the win being sent (`extra.stages`), cleared. None when nothing was seen.
    pub fn stages_take(&mut self) -> Option<alogs_pyval::Value> {
        self.stages_sync();
        let got = self.stages.take();
        self.stages_save(true);
        let (j, line) = got?;
        log::info!(target: "meter", "{line}");
        Some(alogs_pyval::Value::from_json(&j))
    }
}
