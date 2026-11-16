//! Following a boss fight on its HP bar: the countdown, the break gauge, one run on one bar (two phases on a two-boss
//! middle fight). Port of `Timer`, `BreakTracker` and `Fight` in aLogs B16 `bossbar/fights.py`.

use crate::env::Env;
use alogs_reader::boss::{self, GRADE_BASE, GradeInfo, Row};
use indexmap::IndexMap;
use std::collections::VecDeque;

pub const KILL_SOUND_WINDOW: (f64, f64) = (2.5, 6.0);
pub const SLIDE_IN_RISE: f64 = 0.15;
pub const KILL_TO_COUNTDOWN: f64 = 3.0;
pub const FIGHT_INTERVAL: f64 = 1.0 / 30.0;
pub const LEVEL_CONFIRM: f64 = 3.0;
pub const XN_STALE: f64 = 40.0;
pub const OVERTIME_S: i64 = 60;
pub const MAX_CLOCK: f64 = 900.0;
pub const CONFIRM_GAP: f64 = 0.09;
pub const HANDOFF_XN: i64 = 3;
pub const TOP_BAR: f64 = 0.15;
pub const FIGHT_CSV: [&str; 14] = [
    "wall_s", "timer_s", "timer_ocr", "xn_ocr", "segment", "fill_pct", "fill_rgb", "edge_rgb", "team_damage", "avg_dps", "dps_5s", "note",
    "break_left_pct", "break_phase",
];

/// The countdown smoothed against the wall clock: readings are trusted only when they agree with elapsed time.
#[derive(Clone, Debug)]
pub struct Timer {
    pub duration: f64,
    pub val: Option<f64>,
    pub at: Option<f64>,
    pending: Vec<(f64, f64)>,
}

impl Timer {
    pub fn new(duration: f64) -> Timer {
        Timer { duration, val: None, at: None, pending: vec![] }
    }
    pub fn predict(&self, now: f64) -> Option<f64> {
        Some((self.val? - (now - self.at?)).max(0.0))
    }
    fn keep3(&mut self, r: f64, now: f64) {
        self.pending.push((r, now));
        if self.pending.len() > 3 {
            self.pending.remove(0);
        }
    }
    /// "ok", "resync" or None (ignored).
    pub fn feed(&mut self, reading: Option<f64>, now: f64) -> Option<&'static str> {
        let r = reading?;
        if !(0.0 <= r && r <= self.duration.max(MAX_CLOCK)) {
            return None;
        }
        let Some(p) = self.predict(now) else {
            self.keep3(r, now);
            let n = self.pending.len();
            if n >= 2 && (self.pending[n - 1].0 - (self.pending[n - 2].0 - (now - self.pending[n - 2].1))).abs() <= 2.5 {
                (self.val, self.at) = (Some(r), Some(now));
                self.pending.clear();
                return Some("ok");
            }
            return None;
        };
        if (r - p).abs() <= 2.5 {
            (self.val, self.at) = (Some(r), Some(now));
            self.pending.clear();
            return Some("ok");
        }
        self.keep3(r, now);
        if self.pending.len() == 3 {
            let [(r1, t1), (r2, t2), (r3, t3)] = [self.pending[0], self.pending[1], self.pending[2]];
            if (r2 - (r1 - (t2 - t1))).abs() <= 2.0 && (r3 - (r2 - (t3 - t2))).abs() <= 2.0 && r3 < p {
                (self.val, self.at) = (Some(r3), Some(t3));
                self.pending.clear();
                return Some("resync");
            }
        }
        None
    }
}

/// The break gauge over a fight: how fast the team drains it, how many breaks.
#[derive(Clone, Debug)]
pub struct BreakTracker {
    pub size: Option<f64>,
    pub phase: &'static str,
    start: Option<(f64, f64)>,
    prev: Option<f64>,
    peak: f64,
    recent: VecDeque<(f64, f64)>,
    pub breaks: i64,
    drained: f64,
    drain_time: f64,
    pub last_after: Option<f64>,
    pub left: Option<f64>,
    pub kind: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BreakInfo {
    pub left: Option<f64>,
    pub phase: &'static str,
    pub kind: Option<&'static str>,
    pub rate: Option<f64>,
    pub rate_now: Option<f64>,
    pub breaks: i64,
    pub last_after: Option<f64>,
    pub size: Option<f64>,
}

impl BreakTracker {
    pub fn new(size: Option<f64>) -> BreakTracker {
        BreakTracker {
            size,
            phase: "drain",
            start: None,
            prev: None,
            peak: 0.0,
            recent: VecDeque::new(),
            breaks: 0,
            drained: 0.0,
            drain_time: 0.0,
            last_after: None,
            left: None,
            kind: None,
        }
    }

    pub fn feed(&mut self, g: Option<f64>, kind: Option<&'static str>, now: f64) {
        let Some(g) = g else { return };
        let prev = self.prev;
        self.prev = Some(g);
        self.left = Some(g);
        self.kind = kind;
        match self.phase {
            "drain" => {
                if self.start.is_none() {
                    self.start = Some((now, g));
                    self.recent.clear();
                }
                if prev.is_some_and(|p| g > p + 0.12) {
                    self.phase = "refill";
                    self.peak = g;
                    self.start = None;
                } else if g <= 0.02 && prev.is_some_and(|p| p <= 0.5) {
                    let (t0, g0) = self.start.unwrap();
                    if now - t0 > 1.0 {
                        self.drained += g0;
                        self.drain_time += now - t0;
                    }
                    self.breaks += 1;
                    self.last_after = if g0 > 0.9 { Some(now - t0) } else { None };
                    match self.last_after {
                        Some(a) => log::info!(target: "meter", "break #{} after {:.0} s", self.breaks, a),
                        None => log::info!(target: "meter", "break #{}", self.breaks),
                    }
                    self.phase = "broken";
                    self.start = None;
                } else {
                    self.recent.push_back((now, g));
                    while self.recent.front().is_some_and(|r| now - r.0 > 5.0) {
                        self.recent.pop_front();
                    }
                }
            }
            "broken" => {
                if g >= 0.05 {
                    self.phase = "refill";
                    self.peak = g;
                }
            }
            _ => {
                self.peak = self.peak.max(g);
                if g >= 0.97 || g < self.peak - 0.03 {
                    self.phase = "drain";
                    self.start = Some((now, g));
                    self.recent.clear();
                }
            }
        }
    }

    /// (% per second over the fight, % per second over the last ~5 s), None while unknown.
    pub fn rate(&self, now: f64) -> (Option<f64>, Option<f64>) {
        let (mut drained, mut t) = (self.drained, self.drain_time);
        if self.phase == "drain"
            && let (Some(s), Some(left)) = (self.start, self.left)
        {
            drained += (s.1 - left).max(0.0);
            t += now - s.0;
        }
        let overall = if t > 3.0 { Some(drained / t * 100.0) } else { None };
        let ((ta, ga), (tb, gb)) = if self.recent.len() > 1 { (self.recent[0], self.recent[self.recent.len() - 1]) } else { ((0.0, 0.0), (0.0, 0.0)) };
        let recent = if tb - ta >= 2.0 { Some((ga - gb).max(0.0) / (tb - ta) * 100.0) } else { None };
        (overall, recent)
    }

    pub fn info(&self, now: f64) -> BreakInfo {
        let (rate, rate_now) = self.rate(now);
        BreakInfo { left: self.left, phase: self.phase, kind: self.kind, rate, rate_now, breaks: self.breaks, last_after: self.last_after, size: self.size }
    }
}

/// One boss of a run.
#[derive(Clone, Debug)]
pub struct Phase {
    pub row: Row,
    pub title: String,
    pub max_hp: i64,
    pub damage: i64,
    pub defeated: bool,
    pub t0: f64,
    pub t1: Option<f64>,
    pub start_s: f64,
    pub end_s: Option<f64>,
    pub breaks: i64,
    pub hit_s: Option<f64>,
}

/// A CSV cell.
#[derive(Clone, Debug)]
pub enum Cell {
    N,
    I(i64),
    F(f64),
    S(String),
}

impl Cell {
    fn text(&self) -> String {
        match self {
            Cell::N => String::new(),
            Cell::I(i) => i.to_string(),
            Cell::F(f) => alogs_pyval::float_repr(*f),
            Cell::S(s) => s.clone(),
        }
    }
}

fn opt_f(v: Option<f64>) -> Cell {
    v.map_or(Cell::N, Cell::F)
}
fn opt_i(v: Option<i64>) -> Cell {
    v.map_or(Cell::N, Cell::I)
}
fn round1(v: f64) -> f64 {
    alogs_pil::resample::round_digits(v, 1)
}
fn round2(v: f64) -> f64 {
    alogs_pil::resample::round_digits(v, 2)
}
fn pyround(v: f64) -> i64 {
    v.round_ties_even() as i64
}
fn thousands(v: i64) -> String {
    let s = v.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if v < 0 { format!("-{out}") } else { out }
}

/// One run on one boss bar.
pub struct Fight {
    pub title: String,
    pub level: i64,
    pub row: Row,
    pub max_hp: i64,
    pub bossrush: bool,
    pub t0: f64,
    pub created: f64,
    pub damage: i64,
    pub samples: VecDeque<(f64, i64)>,
    pub partner: Option<Row>,
    pub order: Option<i64>,
    pub boss_hp: i64,
    pub done_hp: i64,
    pub phases: Vec<Phase>,
    pub joined: bool,
    pub hit: bool,
    pub first_damage: Option<i64>,
    pub last_at: f64,
    pub low_pending: Option<(f64, f64)>,
    pub rise_at: Option<f64>,
    pub away: f64,
    pub seg: Option<i64>,
    pub seg_fill: f64,
    pub seg_guess: bool,
    pub last_fill: Option<f64>,
    pub xn_votes: Vec<i64>,
    pub vote_at: f64,
    pub xn_anchor: Option<(i64, f64)>,
    pub timer: Option<Timer>,
    pub elapsed: f64,
    pub avg: f64,
    pub recent: f64,
    pub limit: Option<i64>,
    pub started_at_top: bool,
    pub refill_pending: i64,
    pub refill_at: f64,
    pub seg_info: Option<GradeInfo>,
    pub defeated: bool,
    pub column: String,
    pub brk: BreakTracker,
    pub row_at: f64,
    csv: Option<csv::Writer<std::fs::File>>,
    boss_col: bool,
    pub frames: Option<std::path::PathBuf>,
}

fn get<'a>(r: &'a Row, k: &str) -> &'a str {
    r.get(k).map_or("", String::as_str)
}

impl Fight {
    #[allow(clippy::too_many_arguments)]
    pub fn new(env: &Env, title: &str, level: i64, row: Row, max_hp: i64, bossrush: bool, debug: bool, partner: Option<Row>, order: Option<i64>) -> Fight {
        let now = env.now();
        let column = if bossrush { env.schedule.grade_column(Some(get(&row, "puppetId")), now).to_string() } else { GRADE_BASE.to_string() };
        let brk = BreakTracker::new(boss::boss_break_points(&env.res, &row, level as f64));
        let name = format!("{}_{}", env.local_stamp(now), title.chars().filter(|c| c.is_alphanumeric()).take(24).collect::<String>());
        let logs = env.data.join("logs");
        let _ = std::fs::create_dir_all(&logs);
        let boss_col = order.is_some();
        let csv = if env.write_files {
            std::fs::File::create(logs.join(format!("{name}.csv"))).ok().map(|f| {
                let mut w = csv::WriterBuilder::new().terminator(csv::Terminator::CRLF).from_writer(f);
                let mut head: Vec<String> = FIGHT_CSV.iter().map(|s| s.to_string()).collect();
                if boss_col {
                    head.push("boss".into());
                }
                let _ = w.write_record(&head);
                w
            })
        } else {
            None
        };
        let frames = if debug && env.write_files {
            let p = logs.join(&name);
            let _ = std::fs::create_dir_all(&p);
            Some(p)
        } else {
            None
        };
        Fight {
            title: title.to_string(),
            level,
            phases: vec![Phase {
                row: row.clone(),
                title: title.to_string(),
                max_hp,
                damage: 0,
                defeated: false,
                t0: now,
                t1: None,
                start_s: 0.0,
                end_s: None,
                breaks: 0,
                hit_s: None,
            }],
            row,
            max_hp,
            bossrush,
            t0: now,
            created: now,
            damage: 0,
            samples: VecDeque::new(),
            partner,
            order,
            boss_hp: max_hp,
            done_hp: 0,
            joined: false,
            hit: bossrush,
            first_damage: None,
            last_at: now,
            low_pending: None,
            rise_at: None,
            away: 0.0,
            seg: None,
            seg_fill: 1.0,
            seg_guess: false,
            last_fill: None,
            xn_votes: vec![],
            vote_at: 0.0,
            xn_anchor: None,
            timer: None,
            elapsed: 0.0,
            avg: 0.0,
            recent: 0.0,
            limit: None,
            started_at_top: false,
            refill_pending: 0,
            refill_at: 0.0,
            seg_info: None,
            defeated: false,
            column,
            brk,
            row_at: 0.0,
            csv,
            boss_col,
            frames,
        }
    }

    fn cum<'a>(&self, grades: &'a IndexMap<String, Vec<f64>>) -> &'a Vec<f64> {
        match grades.get(&self.column) {
            Some(c) if !c.is_empty() => c,
            _ => &grades[GRADE_BASE],
        }
    }

    /// One frame of the fight. `slide_in`: the Dev switch fight-bar-slide-in is on.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        env: &Env,
        fill: Option<f64>,
        fill_rgb: [u8; 3],
        edge_rgb: [u8; 3],
        xn_ocr: Option<i64>,
        timer_ocr: Option<i64>,
        grades: &IndexMap<String, Vec<f64>>,
        duration: f64,
        at: Option<f64>,
        slide_in: bool,
    ) {
        let now = at.filter(|a| *a != 0.0).unwrap_or_else(|| env.now());
        let mut note = String::new();
        if self.timer.is_none() {
            self.timer = Some(Timer::new(duration));
        }
        let mut cum = self.cum(grades).clone();
        let top = cum.len() as i64 - 1;
        let xn_ocr = xn_ocr.filter(|x| 1 <= *x && *x <= top);
        let [r_, g_, b_] = fill_rgb;
        let fill = if r_ >= 225 && (80..=140).contains(&g_) && (75..=135).contains(&b_) { fill } else { None };
        if self.bossrush {
            if let Some(x) = xn_ocr.filter(|x| *x != 0)
                && now - self.vote_at >= CONFIRM_GAP
            {
                self.xn_votes.push(x);
                if self.xn_votes.len() > 4 {
                    self.xn_votes.remove(0);
                }
                self.vote_at = now;
            }
            let count = |v: &Vec<i64>, x: i64| v.iter().filter(|&&y| y == x).count();
            let agreed = xn_ocr.filter(|&x| x != 0 && count(&self.xn_votes, x) >= 2);
            if agreed.is_some() && agreed == self.seg {
                self.xn_anchor = Some((agreed.unwrap(), now));
            }
            if self.handoff_due(agreed, top) {
                let last = self.seg;
                let agreed = agreed.unwrap();
                let partner = self.partner.clone().unwrap();
                self.next_boss(env, partner, &format!("its bar came back at x{agreed} after x{}", last.map_or("None".into(), |v| v.to_string())));
                cum = self.cum(grades).clone();
                self.seg = Some(agreed);
                self.xn_anchor = Some((agreed, now));
                self.xn_votes = vec![agreed];
                self.vote_at = now;
                self.seg_fill = fill.unwrap_or(1.0);
                note = format!("segment x{agreed} (counter, boss {})", self.phases.len());
            } else if self.seg.is_none() {
                if xn_ocr == Some(top) || agreed.is_some_and(|a| count(&self.xn_votes, a) >= 3) {
                    let x = xn_ocr.unwrap();
                    self.seg = Some(x);
                    note = format!("segment x{x} (counter)");
                    self.xn_anchor = Some((x, now));
                    self.seg_guess = x != top;
                    if self.phases.len() == 1 && self.order != Some(2) {
                        self.started_at_top = x == top && fill.is_some_and(|f| f > 0.97);
                    }
                    self.seg_fill = fill.unwrap_or(1.0);
                }
            } else if let Some(a) = agreed.filter(|&a| {
                let s = self.seg.unwrap();
                a != s && ((a - s).abs() <= 3 || (a == top && self.seg_guess)) && (a < s || count(&self.xn_votes, a) >= 3)
            }) {
                note = format!("segment x{}->x{a} (counter)", self.seg.unwrap());
                self.xn_anchor = Some((a, now));
                self.seg_guess = false;
                self.seg = Some(a);
                self.seg_fill = fill.unwrap_or(1.0);
                self.refill_pending = 0;
            } else if let Some(f) = fill.filter(|&f| self.seg_fill < 0.25 && f > 0.7 && f - self.seg_fill > 0.45) {
                if self.refill_pending == 0 || now - self.refill_at >= CONFIRM_GAP {
                    self.refill_pending += 1;
                    self.refill_at = now;
                }
                let (mut anchor, at) = self.xn_anchor.map_or((None, 0.0), |(a, t)| (Some(a), t));
                if anchor.is_some() && now - at > XN_STALE {
                    anchor = None;
                }
                let seg = self.seg.unwrap();
                if self.refill_pending >= 2 && seg > 1 && anchor.is_none_or(|a| seg >= a) {
                    note = format!("segment x{seg}->x{} (bar refilled)", seg - 1);
                    self.seg = Some(seg - 1);
                    self.seg_fill = f;
                    self.refill_pending = 0;
                    let s = seg - 1;
                    self.xn_votes.retain(|v| *v <= s);
                } else if self.refill_pending >= 2 && seg > 1 {
                    note = format!("refill ignored, counter last x{}", anchor.map_or("None".into(), |a| a.to_string()));
                    self.seg_fill = f;
                    self.refill_pending = 0;
                }
            } else if fill.is_some() {
                self.refill_pending = 0;
            }
        }
        if let Some(f) = fill
            && !(self.bossrush && self.refill_pending != 0)
        {
            let lp = self.low_pending;
            let agrees = lp.is_some_and(|l| f <= l.0 + 0.02);
            if f < self.seg_fill - 0.15 && !(agrees && now - lp.unwrap().1 >= CONFIRM_GAP) {
                if !agrees {
                    self.low_pending = Some((f, now));
                }
            } else {
                self.seg_fill = self.seg_fill.min(f);
                self.low_pending = None;
            }
        }
        if !self.hit && !self.bossrush && fill.is_some_and(|f| f - self.seg_fill > SLIDE_IN_RISE) && slide_in {
            match self.rise_at {
                None => self.rise_at = Some(now),
                Some(r) if now - r >= CONFIRM_GAP => {
                    note = format!("{note} low reading {:.1}% dropped: the bar was sliding in", self.seg_fill * 100.0).trim().to_string();
                    self.seg_fill = fill.unwrap();
                    self.low_pending = None;
                    self.first_damage = None;
                    self.rise_at = None;
                    self.samples.clear();
                }
                _ => {}
            }
        } else if self.rise_at.is_some() {
            self.rise_at = None;
        }
        if fill.is_some() {
            self.last_fill = fill;
        }
        let timer = self.timer.as_mut().unwrap();
        if timer.feed(timer_ocr.map(|t| t as f64), now) == Some("resync") {
            note = format!("{note} timer resync").trim().to_string();
        }
        if self.limit.is_none() && self.bossrush && self.started_at_top && timer.val.is_some() && self.phases.len() == 1 {
            if timer.at.unwrap() - self.created <= 4.0 {
                let lim = (5.0 * (timer.val.unwrap() / 5.0).ceil()) as i64;
                self.limit = Some(lim);
                let d = duration as i64;
                if self.order.is_some() && lim != d && lim != d + OVERTIME_S {
                    note = format!("{note} {}:{:02} left at a top bar mid-run: no time limit", lim / 60, lim % 60).trim().to_string();
                    self.limit = Some(0);
                    self.started_at_top = false;
                } else {
                    note = format!("{note} time limit {}:{:02}", lim / 60, lim % 60).trim().to_string();
                }
            } else {
                self.limit = Some(0);
            }
        }
        let (dmg, seg) =
            boss::damage_dealt(self.boss_hp as f64, self.seg_fill, if self.bossrush { self.seg } else { None }, &cum, self.bossrush);
        let mut rose = false;
        if let Some(d) = dmg
            && !(self.bossrush && self.seg.is_none() && self.phases.len() > 1)
        {
            rose = d > self.phases.last().unwrap().damage;
            self.damage = self.done_hp + d;
            self.phases.last_mut().unwrap().damage = d;
        }
        if !self.hit {
            match self.first_damage {
                None => {
                    if fill.is_some() && self.low_pending.is_none() {
                        self.first_damage = Some(self.damage);
                    }
                }
                Some(fd) => {
                    if (self.damage - fd) as f64 > self.max_hp as f64 * 0.0012 {
                        self.hit = true;
                        self.t0 = (self.last_at + now) / 2.0;
                        note = format!("{note} first hit, {:.1}s after the bar showed up", self.t0 - self.created).trim().to_string();
                    }
                }
            }
        }
        self.last_at = now;
        self.seg_info = seg;
        if !note.is_empty() {
            log::info!(target: "meter", "fight: {note} (fill {:.1}%, damage {})", self.seg_fill * 100.0, thousands(self.damage));
        }
        self.samples.push_back((now, self.damage));
        while self.samples.len() > 1 && now - self.samples[1].0 >= 5.0 {
            self.samples.pop_front();
        }
        let tp = self.timer.as_ref().unwrap().predict(now);
        let clock = match self.limit {
            Some(l) if l != 0 => l as f64,
            _ => duration,
        };
        self.elapsed = match tp {
            Some(t) if t <= clock => clock - t,
            _ => {
                if self.hit {
                    now - self.t0
                } else {
                    0.0
                }
            }
        };
        if rose {
            self.phases.last_mut().unwrap().hit_s = Some(round1(self.elapsed));
        }
        if self.order == Some(2) && self.phases.len() == 1 && !self.joined && tp.is_some_and(|t| t <= clock) {
            self.joined = true;
            self.phases[0].start_s = round1(self.elapsed - (now - self.created)).max(0.0);
        }
        let avg = if self.elapsed > 1.0 { self.damage as f64 / self.elapsed } else { 0.0 };
        let ((t_a, d_a), (t_b, d_b)) = (self.samples[0], self.samples[self.samples.len() - 1]);
        self.recent = if t_b - t_a > 0.5 { (d_b - d_a) as f64 / (t_b - t_a) } else { avg };
        self.avg = avg;
        if note.is_empty() && now - self.row_at < CONFIRM_GAP {
            return;
        }
        self.row_at = now;
        let hex = |p: [u8; 3]| format!("{:02x}{:02x}{:02x}", p[0], p[1], p[2]);
        self.write(vec![
            Cell::F(round2(now - self.created)),
            opt_f(tp.map(round1)),
            opt_i(timer_ocr),
            opt_i(xn_ocr),
            opt_i(self.seg),
            opt_f(fill.map(|f| round2(f * 100.0))),
            Cell::S(hex(fill_rgb)),
            Cell::S(hex(edge_rgb)),
            Cell::I(self.damage),
            Cell::I(pyround(avg)),
            Cell::I(pyround(self.recent)),
            Cell::S(note),
            opt_f(self.brk.left.map(|l| round1(l * 100.0))),
            Cell::S(self.brk.phase.to_string()),
        ]);
    }

    /// (the boss on the bar, of how many) on a two-boss stage, else None.
    pub fn boss_of(&self) -> Option<(i64, i64)> {
        if self.bossrush { self.order.map(|o| (o, 2)) } else { None }
    }

    fn write(&mut self, mut cells: Vec<Cell>) {
        if self.boss_col {
            while cells.len() < FIGHT_CSV.len() {
                cells.push(Cell::N);
            }
            cells.push(opt_i(self.order));
        }
        if let Some(w) = self.csv.as_mut() {
            let _ = w.write_record(cells.iter().map(Cell::text));
        }
    }

    /// The boss went down: the bar vanishes before the last hit can be read, so the rest counts.
    pub fn mark_defeated(&mut self, env: &Env, why: &str) {
        if self.defeated {
            return;
        }
        let left = self.max_hp - self.damage;
        self.defeated = true;
        self.damage = self.max_hp;
        let bh = self.boss_hp;
        let p = self.phases.last_mut().unwrap();
        p.damage = bh;
        p.defeated = true;
        self.avg = if self.elapsed > 1.0 { self.damage as f64 / self.elapsed } else { 0.0 };
        let cells = vec![
            Cell::F(round2(env.now() - self.created)),
            Cell::N,
            Cell::N,
            Cell::N,
            opt_i(self.seg),
            Cell::F(0.0),
            Cell::S(String::new()),
            Cell::S(String::new()),
            Cell::I(self.damage),
            Cell::I(pyround(self.avg)),
            Cell::I(pyround(self.recent)),
            Cell::S(format!("defeated ({why}): final {} HP counted", thousands(left))),
        ];
        self.write(cells);
        if let Some(w) = self.csv.as_mut() {
            let _ = w.flush();
        }
    }

    /// A two-boss fight's first boss in its last three bars: its partner can take the bar any moment.
    pub fn partner_due(&self) -> bool {
        self.partner.is_some() && self.seg.is_some_and(|s| s <= 3)
    }

    /// The bar's own sign that a two-boss fight's first boss is down and its partner has the bar.
    pub fn handoff_due(&self, agreed: Option<i64>, top: i64) -> bool {
        self.partner.is_some()
            && agreed.is_some_and(|a| a >= top - HANDOFF_XN)
            && self.seg.is_some()
            && !self.seg_guess
            && (self.seg == Some(1) || (self.seg == Some(2) && self.seg_fill < 0.05))
    }

    /// A two-boss fight's hand-off to the first boss's partner: the run goes on, the bar's state starts over.
    pub fn next_boss(&mut self, env: &Env, row: Row, why: &str) {
        let now = env.now();
        let left = self.boss_hp - (self.damage - self.done_hp);
        let (bh, el, br) = (self.boss_hp, self.elapsed, self.brk.breaks);
        let p = self.phases.last_mut().unwrap();
        p.damage = bh;
        p.defeated = true;
        p.t1 = Some(now);
        p.end_s = Some(p.hit_s.unwrap_or(round1(el)));
        p.breaks = br;
        self.done_hp += self.boss_hp;
        let cal = boss::load_calibration(&env.data);
        let hp = boss::boss_max_hp(&row, self.level, Some(&cal)).unwrap_or(0);
        let t = get(&row, "bossTitle_en");
        self.title = if t.is_empty() { self.title.clone() } else { t.to_string() };
        self.boss_hp = hp;
        self.partner = None;
        self.order = Some(2);
        self.max_hp = self.done_hp + hp;
        self.damage = self.done_hp;
        self.defeated = false;
        self.phases.push(Phase {
            row: row.clone(),
            title: self.title.clone(),
            max_hp: hp,
            damage: 0,
            defeated: false,
            t0: now,
            t1: None,
            start_s: round1(self.elapsed),
            end_s: None,
            breaks: 0,
            hit_s: None,
        });
        self.row = row;
        self.seg = None;
        self.seg_fill = 1.0;
        self.seg_guess = false;
        self.seg_info = None;
        self.last_fill = None;
        self.xn_votes.clear();
        self.xn_anchor = None;
        self.refill_pending = 0;
        self.refill_at = 0.0;
        self.low_pending = None;
        self.column = env.schedule.grade_column(Some(get(&self.row, "puppetId")), now).to_string();
        self.brk = BreakTracker::new(boss::boss_break_points(&env.res, &self.row, self.level as f64));
        let msg = format!(
            "boss {} of 2: {}, max HP {} ({why}); the first boss is down, its last {} HP counted",
            self.phases.len(),
            self.title,
            thousands(hp),
            thousands(left)
        );
        log::info!(target: "meter", "fight: {msg} (run damage {}, {:.0}s)", thousands(self.damage), self.elapsed);
        let cells = vec![
            Cell::F(round2(now - self.created)),
            Cell::N,
            Cell::N,
            Cell::N,
            Cell::N,
            Cell::N,
            Cell::S(String::new()),
            Cell::S(String::new()),
            Cell::I(self.damage),
            Cell::I(pyround(self.avg)),
            Cell::I(pyround(self.recent)),
            Cell::S(msg),
        ];
        self.write(cells);
    }

    pub fn close(&mut self, env: &Env) {
        let (el, br, dmg, def) = (self.elapsed, self.brk.breaks, self.damage - self.done_hp, self.defeated);
        let p = self.phases.last_mut().unwrap();
        if p.t1.is_none() {
            p.t1 = Some(env.now());
            p.end_s = Some(round1(el));
            p.breaks = br;
            p.damage = dmg;
            p.defeated = def;
        }
        if let Some(mut w) = self.csv.take() {
            let _ = w.flush();
        }
    }
}
