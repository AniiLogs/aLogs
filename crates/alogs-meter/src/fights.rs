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
/// Agreeing title reads that put a fight's level right (Meter::identify), read about once a second while needed.
pub const LEVEL_FIX_READS: usize = 3;
/// Title reads before a fight count together this long (Meter::identify).
pub const LEVEL_VOTE_S: f64 = 10.0;
/// The level a boss was settled on in a place counts for the next fight there this long (Meter::level_needs).
pub const LEVEL_SEEN_S: f64 = 600.0;
/// A break counts only once the gauge has stayed empty this long (a one-look misread, or a flash where the bar is,
/// reads empty for a moment and is back at once): from its first empty look to a later one still empty.
pub const BREAK_CONFIRM_S: f64 = 0.5;

/// The fill's colour is the boss bar's red: the bar itself was read on this look (else the fight CSV's fill_pct is
/// empty: the bar hidden by a skill animation, gone, or covered by a flash).
fn pct(g: Option<f64>) -> String {
    g.map_or("unknown".into(), |g| format!("{:.1}%", g * 100.0))
}

pub fn bar_red(rgb: [u8; 3]) -> bool {
    let [r, g, b] = rgb;
    r >= 225 && (80..=140).contains(&g) && (75..=135).contains(&b)
}
/// A reading more than 15 points under the last is taken once it has stood this long, every reading in between
/// agreeing (the live meter's rule; `Fight::low_confirm`). At CONFIRM_GAP, two readings 0.09 s apart were enough,
/// and since HP only drops a wrong one stayed for the rest of the fight: one row at 0.51% put a fight 219,000
/// damage ahead for 44.7 s (17 Nov).
pub const LOW_CONFIRM_S: f64 = 0.3;
/// A bar's readings from before its fight started are kept this long, and are one bar's while no longer than
/// EARLY_GAP_S apart (Meter::keep_early).
pub const EARLY_S: f64 = 10.0;
pub const EARLY_GAP_S: f64 = 2.0;
pub const HANDOFF_XN: i64 = 3;
/// The bar's counter taken for a drop of more than 3 bars once every vote has been under the meter's bar this long.
/// Without it a burst of several bars in a few seconds left the meter on its old bar for the rest of the fight (the
/// counter's later reads were too far off to take, and its refills were refused for the stale counter), so a kill
/// was saved with 6% of the boss's HP (19 Nov).
pub const XN_DROP_S: f64 = 1.0;
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
///
/// Live, each look comes with what the bar shows besides the gauge (`feed_look`, from `bar::measure_break`): the
/// gauge (or unknown), the lock icon, and the broken look (the game's "250%" badge over the bar's right end, or the HP
/// row's recent-damage piece in break yellow). Only looks on which the bar itself was read count; a look that can't
/// tell the lock icon keeps its last state.
///
/// Every bar but an Interlink one (`by_look`: the open world's, a Holo-Battle Sim room's, any other) is a state machine
/// on what is seen:
/// - `drain`: a gauge line, no broken look. A break needs the broken look: an empty gauge (no line, or only the
///   sliver at the track's start) with the broken look is a break, counted once, at that look, and the state is
///   `broken` (also when the drain to it wasn't seen). The sliver without the broken look is only a low reading, shown
///   as it is, however long it stands (the boss isn't broken yet: 22 Nov, it stood at 0.5 to 0.8% for 3 s).
/// - `broken`: the broken look, no line (or only the bright flashes at its end). The white line growing from the left
///   (3 to 30%) with the broken look still up is `refill`.
/// - `refill` (reached only from `broken`): the white line growing while the broken look is still up (the badge stays up until the gauge is full
///   again: 22 Nov, the badge went with the line at 98.5 to 99.6%).
/// - Back to `drain` (unbroken) only on a positive sighting: the bar read, a gauge line there, and the broken look gone.
///   Time passing, the bar away (a cutscene hides the whole HUD), the gauge unknown or a flash change nothing: after a
///   gap the state is what it was before it. So one broken spell is one break, however often the HUD goes and comes back.
/// - The lock icon (the boss can't be broken): while it is up nothing counts a break; an empty spell that saw it needs
///   the broken look.
///
/// An Interlink bar keeps its own rules (its broken state looks different): a break once the gauge has stayed empty
/// BREAK_CONFIRM_S, the refill on a jump up of more than 12 points that the next reading holds, back to the drain at
/// 97% or once it falls 3 points from its peak.
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
    /// An empty gauge not confirmed yet: (its first empty look, the drain's start then, the reading before it, the
    /// gauge shown before it, the lock icon seen meanwhile). The phase stays "drain" and nothing counts it until it is
    /// confirmed; a spell still pending when the fight ends is never counted.
    pending: Option<(f64, (f64, f64), Option<f64>, Option<f64>, bool)>,
    /// A jump up in the drain waiting for the next reading to hold it: (the reading before it, the gauge shown before).
    rise: Option<(Option<f64>, Option<f64>)>,
    /// The lock icon is up: the boss can't be broken now (as last seen: a look that can't tell keeps it).
    pub locked: bool,
    /// Looks whose gauge was unknown, by why ("under our cover", "no line found", "lock icon", "bar not read"), for
    /// the fight's end.
    pub unknown: Vec<(&'static str, u32)>,
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
    /// The lock icon is up (the boss can't be broken now).
    pub locked: bool,
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
            pending: None,
            rise: None,
            locked: false,
            unknown: vec![],
        }
    }

    fn note_unknown(&mut self, why: &'static str) {
        match self.unknown.iter_mut().find(|u| u.0 == why) {
            Some(u) => u.1 += 1,
            None => self.unknown.push((why, 1)),
        }
    }

    /// The looks whose gauge was unknown this fight, by why ("" when none).
    pub fn unknown_summary(&self) -> String {
        let n: u32 = self.unknown.iter().map(|u| u.1).sum();
        if n == 0 {
            return String::new();
        }
        format!("{n} looks ({})", self.unknown.iter().map(|(w, k)| format!("{w} {k}")).collect::<Vec<_>>().join(", "))
    }

    /// One look at the gauge, fed only when the fight's bar itself was read on that look (`bar_rgb`: the fill's
    /// colour): with the bar hidden or gone, what shows where the gauge was means nothing. `by_look`: any bar but an
    /// Interlink one (the open world's, a Holo-Battle Sim room's), whose broken look decides the phase.
    pub fn feed_look(&mut self, r: &alogs_reader::bar::BreakRead, bar_rgb: [u8; 3], by_look: bool, now: f64) {
        if !bar_red(bar_rgb) {
            self.note_unknown("bar not read");
            return;
        }
        if let Some(l) = r.locked
            && l != self.locked
        {
            self.locked = l;
            log::info!(target: "meter", "break gauge: lock icon {} (gauge {})", if l { "up" } else { "gone" }, pct(r.gauge));
        }
        if r.gauge.is_none() {
            self.note_unknown(r.unknown.unwrap_or("no line found"));
        }
        self.step(r.gauge, r.kind, (by_look && r.broken, self.locked, by_look), now);
    }

    /// A reading with nothing else known of the look (no lock icon, no broken look): the gauge's own rules only.
    pub fn feed(&mut self, g: Option<f64>, kind: Option<&'static str>, now: f64) {
        self.step(g, kind, (false, false, false), now);
    }

    fn step(&mut self, g: Option<f64>, kind: Option<&'static str>, look: (bool, bool, bool), now: f64) {
        let Some(g) = g else { return };
        let (broken, locked, by_look) = look;
        if self.prev.is_none() {
            log::info!(target: "meter", "break gauge: first read {:.1}%", g * 100.0);
        }
        let mut prev = self.prev;
        self.kind = kind;
        if let Some((mut first, start, before, shown, mut lock_seen)) = self.pending {
            if g > 0.05 {
                // back before it was confirmed: a misread, the drain carries on as if it never was
                self.pending = None;
                prev = before;
                self.left = shown;
            } else {
                self.prev = Some(g);
                lock_seen |= locked;
                if locked && !by_look {
                    first = now; // (an Interlink bar: the wait starts again once the icon is gone)
                }
                let empty = g <= 0.02 && !locked;
                // with the broken look at once; else once it stayed empty (on the open world's bar, an empty spell
                // that saw the lock icon needs the broken look)
                let waited = now - first >= BREAK_CONFIRM_S && !(by_look && lock_seen);
                if empty && ((by_look && broken) || waited) {
                    self.pending = None;
                    self.left = Some(g);
                    self.count_break(first, start);
                } else {
                    self.pending = Some((first, start, before, shown, lock_seen));
                }
                return;
            }
        }
        self.prev = Some(g);
        let shown = self.left;
        self.left = Some(g);
        match self.phase {
            "drain" => {
                if self.start.is_none() {
                    self.start = Some((now, g));
                    self.recent.clear();
                }
                // (back up to where it stood a moment ago is no refill: the low reading between was a misread)
                let stood = self.recent.iter().filter(|r| now - r.0 <= 2.0).map(|r| r.1).fold(0.0, f64::max);
                let jump = !by_look && prev.is_some_and(|p| g > p + 0.12) && g > stood + 0.03;
                if jump && self.rise.is_some() {
                    self.refill(g);
                } else if jump {
                    // a jump up: a refill once the next reading holds it
                    self.rise = Some((prev, shown));
                    (self.prev, self.left) = (prev, shown);
                } else if by_look && g <= 0.02 && broken && !locked {
                    // empty (or the sliver) with the broken look: the break, at this look (also when the drain to it
                    // wasn't seen: the gauge unread for a while); never while the lock icon is up
                    self.rise = None;
                    let start = self.start.unwrap();
                    self.count_break(now, start);
                } else if !by_look && g <= 0.02 && prev.is_some_and(|p| p <= 0.5) {
                    // (an Interlink bar) empty: a break once it stays so (the gauge shown and the drain's figures
                    // wait for that); never while the lock icon is up
                    self.rise = None;
                    let start = self.start.unwrap();
                    self.pending = Some((now, start, prev, shown, locked));
                    self.left = shown;
                } else {
                    self.rise = None; // (a jump that didn't hold: a misread)
                    self.recent.push_back((now, g));
                    while self.recent.front().is_some_and(|r| now - r.0 > 5.0) {
                        self.recent.pop_front();
                    }
                }
            }
            "broken" => {
                if by_look {
                    // the refill starts with the white line growing from the left; the bright flashes at the end of
                    // the broken state are no line; a line without the broken look is the drain again
                    if g >= 0.05 && !broken {
                        log::info!(target: "meter", "break gauge: unbroken again ({:.1}%)", g * 100.0);
                        self.drain_from(g, now);
                    } else if broken && kind == Some("white") && (0.03..=0.3).contains(&g) {
                        self.refill(g);
                    }
                } else if g >= 0.05 {
                    self.refill(g);
                }
            }
            _ => {
                if g >= 0.97 && self.peak < 0.97 {
                    log::info!(target: "meter", "break gauge: refill complete ({:.1}%)", g * 100.0);
                }
                self.peak = self.peak.max(g);
                if by_look {
                    // unbroken again: a line with the broken look gone
                    if !broken {
                        log::info!(target: "meter", "break gauge: unbroken again ({:.1}%)", g * 100.0);
                        self.drain_from(g, now);
                    }
                } else if g >= 0.97 || g < self.peak - 0.03 {
                    self.drain_from(g, now);
                }
            }
        }
    }

    fn refill(&mut self, g: f64) {
        log::info!(target: "meter", "break gauge: refill starting ({:.1}%)", g * 100.0);
        self.phase = "refill";
        self.peak = g;
        self.start = None;
        self.rise = None;
    }

    fn drain_from(&mut self, g: f64, now: f64) {
        self.phase = "drain";
        self.start = Some((now, g));
        self.recent.clear();
    }

    /// A break, dated from its first empty look `at`, the drain having started at `start`.
    fn count_break(&mut self, at: f64, start: (f64, f64)) {
        let (t0, g0) = start;
        if at - t0 > 1.0 {
            self.drained += g0;
            self.drain_time += at - t0;
        }
        self.breaks += 1;
        self.last_after = if g0 > 0.9 { Some(at - t0) } else { None };
        match self.last_after {
            Some(a) => log::info!(target: "meter", "break #{} after {:.0} s", self.breaks, a),
            None => log::info!(target: "meter", "break #{}", self.breaks),
        }
        self.phase = "broken";
        self.start = None;
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
        BreakInfo { left: self.left, phase: self.phase, kind: self.kind, rate, rate_now, breaks: self.breaks, last_after: self.last_after, size: self.size, locked: self.locked }
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
    /// Title reads that agreed with the fight's level since it started.
    pub level_reads: u32,
    pub last_at: f64,
    pub low_pending: Option<(f64, f64)>,
    /// How long a much lower reading must stand before it is taken (CONFIRM_GAP as in the Python; the live meter
    /// sets LOW_CONFIRM_S).
    pub low_confirm: f64,
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
    /// Since when every counter vote has been under the segment the meter holds (a drop of more than 3 bars).
    pub drop_since: Option<f64>,
    pub seg_info: Option<GradeInfo>,
    pub defeated: bool,
    pub column: String,
    pub brk: BreakTracker,
    pub row_at: f64,
    csv: Option<csv::Writer<std::fs::File>>,
    boss_col: bool,
    pub frames: Option<std::path::PathBuf>,
    /// The fight's file name without its .csv (logs/<name>.csv).
    pub csv_name: String,
    /// Data rows written to its CSV so far.
    pub rows: u64,
    /// The bar this fight follows: (span, yc).
    pub bar_geom: Option<(f64, f64)>,
    pub foreign_logged: bool,
    /// The bar's ends changed on this look (the meter sets it before `update`): its reading waits for one more look
    /// before it counts as damage, so a wrong end can't credit damage that wasn't dealt.
    pub ends_moved: bool,
    /// The game log confirmed an Interlink scene.
    pub scene_ok: bool,
    pub ended: Option<f64>,
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
            level_reads: 0,
            last_at: now,
            low_pending: None,
            low_confirm: CONFIRM_GAP,
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
            drop_since: None,
            seg_info: None,
            defeated: false,
            column,
            brk,
            row_at: 0.0,
            csv,
            boss_col,
            frames,
            csv_name: name,
            rows: 0,
            bar_geom: None,
            foreign_logged: false,
            ends_moved: false,
            scene_ok: false,
            ended: None,
        }
    }

    /// The fight's level was read wrong: each boss's HP is the right level's (`hps`, one per boss so far, in order), and
    /// the damage so far follows.
    ///
    /// The arithmetic: the damage on the boss being fought is its HP times a fraction the bar alone gives (one bar: the
    /// fill lost; Interlink's xN bars: the grade table's share for the bar counter and the fill, `boss::damage_dealt`),
    /// so it scales with that boss's HP exactly: new = old x new HP / old HP. A boss already beaten (the first of a
    /// two-boss fight) counts its whole HP, so it is its new HP. The fight's total is the beaten bosses' HP plus the
    /// current boss's damage; its HP the beaten bosses' plus the current boss's. The 5 s DPS samples, the first-hit mark
    /// and the averages scale with the total. Exact at any point, as the fraction doesn't depend on the HP.
    pub fn correct_level(&mut self, level: i64, hps: &[i64], break_size: Option<f64>) {
        let old_total = self.damage.max(1) as f64;
        let old_boss = self.boss_hp.max(1) as f64;
        let current = (self.damage - self.done_hp) as f64;
        let n = self.phases.len();
        let mut done = 0;
        for (i, p) in self.phases.iter_mut().enumerate() {
            p.max_hp = hps[i];
            if i + 1 < n {
                p.damage = hps[i];
                done += hps[i];
            }
        }
        let hp = hps[n - 1];
        let now_damage = (current * hp as f64 / old_boss).round() as i64;
        self.phases[n - 1].damage = now_damage;
        self.level = level;
        self.done_hp = done;
        self.boss_hp = hp;
        self.max_hp = done + hp;
        self.damage = done + now_damage;
        let k = self.damage as f64 / old_total;
        let scale = |v: i64| (v as f64 * k).round() as i64;
        for s in self.samples.iter_mut() {
            s.1 = scale(s.1);
        }
        self.first_damage = self.first_damage.map(scale);
        self.avg *= k;
        self.recent *= k;
        self.brk.size = break_size;
        self.level_reads = LEVEL_FIX_READS as u32;
    }

    fn cum<'a>(&self, grades: &'a IndexMap<String, Vec<f64>>) -> &'a Vec<f64> {
        match grades.get(&self.column) {
            Some(c) if !c.is_empty() => c,
            _ => &grades[GRADE_BASE],
        }
    }

    /// One frame of the fight. `slide_in`: a low first reading followed by a much fuller one before anyone has hit is
    /// dropped (the live meter always asks for it; it was the Dev switch fight-bar-slide-in).
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
        let ends_moved = std::mem::take(&mut self.ends_moved);
        if self.timer.is_none() {
            self.timer = Some(Timer::new(duration));
        }
        let mut cum = self.cum(grades).clone();
        let top = cum.len() as i64 - 1;
        let xn_ocr = xn_ocr.filter(|x| 1 <= *x && *x <= top);
        let fill = if bar_red(fill_rgb) { fill } else { None };
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
            // every vote under the bar the meter holds: the start of a drop the counter shows
            if let (Some(s), Some(_)) = (self.seg, xn_ocr) {
                if self.xn_votes.iter().all(|v| *v < s) {
                    self.drop_since.get_or_insert(now);
                } else {
                    self.drop_since = None;
                }
            }
            // (three of the last four votes, as for a rise. Never to x1, nor to a number whose digits are part of the
            // bar's own: "x1" read with a digit lost, from "x13" or "x8"'s neighbours, stood for seconds on end in Nov
            // runs, while every real burst of bars was near the top. The last bars are reached a step at a time.)
            let dropped = agreed.filter(|&a| {
                self.seg.is_some_and(|s| s - a > 3 && a > 1 && !s.to_string().contains(&a.to_string()))
                    && count(&self.xn_votes, a) >= 3
                    && self.drop_since.is_some_and(|d| now - d >= XN_DROP_S)
            });
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
                (a != s && ((a - s).abs() <= 3 || (a == top && self.seg_guess)) && (a < s || count(&self.xn_votes, a) >= 3)) || dropped == Some(a)
            }) {
                note = format!("segment x{}->x{a} (counter)", self.seg.unwrap());
                self.xn_anchor = Some((a, now));
                self.seg_guess = false;
                self.seg = Some(a);
                self.seg_fill = fill.unwrap_or(1.0);
                self.refill_pending = 0;
                self.drop_since = None;
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
            if ends_moved && f < self.seg_fill {
                // read at ends other than the last reading's: not damage until the next look at these ends (in a
                // burst of 8 re-finds in 0.7 s, 22 Nov, one row credited 5.7% of a boss's HP that wasn't dealt)
            } else if f < self.seg_fill - 0.15 && !(agrees && now - lp.unwrap().1 >= self.low_confirm) {
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
        if let Some(w) = self.csv.as_mut()
            && w.write_record(cells.iter().map(Cell::text)).is_ok()
        {
            self.rows += 1;
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
        self.drop_since = None;
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
