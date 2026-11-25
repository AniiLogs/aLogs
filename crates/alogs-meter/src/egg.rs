//! Egg Heist's screens: the end screen (one record per match, two reads alike), the rank screens (Rank Info, the
//! post-match rank, Egg Emperor's eggs) and boss rush's Challenge Stats. Port of those parts of aLogs B16
//! `bossbar/results.py`.
//!
//! Holding the player's inputs while a screen is read, as for Interlink and the Holo-Battle Sim: from the moment the
//! end screen (or the rank screen after it) is spotted, keyboard, mouse buttons and the controller (the game told it
//! is inactive) are held while it is read and sent, and let go once it is sent; at most results.hold_seconds
//! (egg_screen_hold, egg_sent, egg_hold_end). When aLogs presses Next itself, the hold carries on through the loading
//! to the rank screen, a new time limit from the end screen's send. One notice per match: its rank goes silently.

use crate::eggheist as e;
use crate::meter::{Frames, Meter, Out};
use crate::payloads;
use crate::results::{local, rel};
use crate::results_parse::{EGG_MODES, Votes, canon, confirmed, first_num, vote};
use crate::screens::{Heads, rank_screen};
use crate::sim::str_map_repr;
use alogs_pil::ops::expand;
use alogs_pil::{Filter, Image, Mode};
use alogs_pyval::{Dict, Value, dict};
use alogs_reader::prep;
use indexmap::IndexMap;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

pub const EGG_MAX_READS: i64 = 6;
pub const EGG_GONE_S: f64 = 4.0;
pub const EGG_SENT_S: f64 = 60.0;
pub const EGG_PARTY_S: f64 = 1800.0;
pub const EGG_RANK_WAIT_S: f64 = 90.0;
pub const EGG_NEXT_TRIES: i64 = 4;
pub const EGG_NEXT_GAP_S: f64 = 1.5;
pub const EGG_NEXT_S: f64 = 30.0;
/// The end screen's Next button by its height in each layout.
pub const EGG_NEXT_SPOTS: [(&str, f64, f64, f64); 2] = [("controller", 0.858, 0.90, 0.879), ("keyboard", 0.893, 0.935, 0.9144)];

pub struct EggRead {
    votes: Votes,
    reads: i64,
    seen: f64,
    im: Image,
}

pub struct EggPending {
    at: f64,
    payload: Value,
    shot: Option<String>,
    party: Option<String>,
}

pub struct EggClick {
    at: f64,
    last: f64,
    tries: i64,
}

#[derive(Default)]
pub struct EggState {
    pub read: Option<EggRead>,
    pub sent_at: f64,
    pub pending: Option<EggPending>,
    pub rank_read_at: f64,
    pub end_seen: f64,
    pub points: Option<String>,
    pub click: Option<EggClick>,
    pub rank_frame: Option<(Image, f64)>,
    pub seen: f64,
    pub gain_seen: f64,
    pub stats_seen: f64,
    pub season: Option<String>,
    /// Inputs held while an end or rank screen is read and sent.
    pub hold: Option<EggHold>,
    /// When an Egg Heist record last went with a notice to come: a rank record within EGG_RANK_WAIT_S of it belongs to
    /// the same run (or the same look at Rank Info) and goes silently.
    pub noticed_at: f64,
    /// What each kind of rank record last sent ('egg_rank', 'egg_gain').
    pub last: IndexMap<String, String>,
}

/// Egg Heist's hold: one per screen visit (the end screen, on through the rank screen when aLogs presses Next).
#[derive(Clone, Debug, PartialEq)]
pub struct EggHold {
    /// Let go at this time whatever happens (results.hold_seconds from its start, or from the end screen's send).
    pub until: f64,
    /// "end screen" or "rank screen": what it is held for now.
    pub screen: &'static str,
    /// The end screen is sent and aLogs pressed Next: held on through the loading to the rank screen.
    pub on_to_rank: bool,
    /// Keyboard and mouse buttons are held (Ctrl+Shift+F12 lets them go, and with them the rest).
    pub held: bool,
}

static POINTS_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\d{1,6}\s*/\s*\d{1,6}\s*$").unwrap());
static EGGS_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[xX×]\s*\d{1,6}\s*$").unwrap());

fn opt(v: Option<i64>) -> Value {
    v.map_or(Value::None, Value::Int)
}

fn thousands(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// The end screen's numbers by OCR: the whole read has to be a number.
fn whole(text: Option<&str>) -> Option<i64> {
    let v: String = text.unwrap_or("").chars().filter(|c| *c != ',' && !c.is_whitespace()).collect::<String>().replace('O', "0");
    if !v.is_empty() && v.len() <= 9 && v.chars().all(|c| c.is_ascii_digit()) { v.parse().ok() } else { None }
}

/// The number after the drawn label; a lone 1 often reads as I, l or |, a 0 as O.
fn digits(text: Option<&str>) -> Option<i64> {
    let t = text.unwrap_or("");
    let after = t.split_once(':').map_or(t, |x| x.1).trim();
    let raw: String = after.chars().map(|c| match c {
        'I' | 'l' | '|' | '!' => '1',
        'O' | 'o' => '0',
        c => c,
    }).filter(|c| *c != ' ').collect();
    if !raw.is_empty() && raw.chars().all(|c| c.is_ascii_digit()) { raw.parse().ok() } else { None }
}

impl Meter {
    fn season_now(&self) -> Option<String> {
        self.egg.season.clone().or_else(|| e::current_season(&today()))
    }

    /// A match's rank screen may still come: its hold is on, or the end screen was seen or sent less than
    /// EGG_RANK_WAIT_S ago and no rank screen has been read since. A match queued from a homeland comes back to it,
    /// and the rank screen shows there, so the homeland rest waits for this.
    pub fn egg_rank_due(&self, now: f64) -> bool {
        let after = self.egg.end_seen.max(self.egg.sent_at);
        self.egg.hold.is_some() || (after > 0.0 && now - after < EGG_RANK_WAIT_S && self.egg.rank_read_at < after)
    }

    /// The rank screen's line and numbers only count just after a match's end screen.
    pub fn rank_only_after_match(&self, heads: &Heads, now: f64) -> Heads {
        if now - self.egg.end_seen.max(self.egg.sent_at) < EGG_RANK_WAIT_S {
            return heads.clone();
        }
        heads.iter().filter(|(k, _)| !matches!(k.as_str(), "ep" | "es" | "et")).map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// The end screen is read: press its Next button (only on a button that reads Next, with the game in front).
    fn egg_next(&mut self, frames: &mut dyn Frames, im: &Image, now: f64, u: f64, ux: f64, h: f64) {
        let Some(click) = &mut self.egg.click else { return };
        if now - click.last < EGG_NEXT_GAP_S {
            return;
        }
        if now - click.at > EGG_NEXT_S || click.tries >= EGG_NEXT_TRIES {
            log::info!(target: "meter", "Egg Heist: Next not pressed through, after {} tries; it's left to the player", click.tries);
            self.egg.click = None;
            self.egg_next_given_up();
            return;
        }
        if self.hwnd.is_some_and(|w| !frames.is_foreground(w)) {
            self.egg.click = None;
            log::info!(target: "meter", "Egg Heist: Next not pressed, the game is not the active window");
            self.egg_next_given_up();
            return;
        }
        click.last = now;
        click.tries += 1;
        let tries = click.tries;
        let kk = 2.0 * 2000.0 / u;
        let mut crops = IndexMap::new();
        for (name, y0, y1, _) in EGG_NEXT_SPOTS {
            let c = im.crop_i((ux + u * 0.84) as i64, (h * y0) as i64, (ux + u * 0.94) as i64, (h * y1) as i64).convert(Mode::L);
            let c = c.resize(((c.width as f64 * kk) as usize).max(1), ((c.height as f64 * kk) as usize).max(1), Some(Filter::Bicubic)).point(|v| if v < 120 { 0.0 } else { 255.0 });
            crops.insert(name.to_string(), expand(&c, 20, &[255]));
        }
        let t = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
        let Some(spot) = EGG_NEXT_SPOTS.iter().find(|s| t.get(s.0).is_some_and(|v| v.to_lowercase().contains("next"))) else {
            log::info!(target: "meter", "Egg Heist: Next not readable yet ({}), trying again", str_map_repr(&t));
            return;
        };
        let x = f64::from(self.origin.0) + ux + u * 0.88;
        let y = f64::from(self.origin.1) + h * spot.3;
        log::info!(target: "meter", "Egg Heist: pressing Next ({} layout, try {tries}) at screen ({x:.0}, {y:.0})", spot.0);
        if self.hold.active() {
            self.hold.freeze_moves(0.4);
        }
        if !self.args.replay {
            alogs_capture::win::click_at(x as i32, y as i32, false);
        }
    }

    /// Egg Heist end screen ("Adventure Successful", or the failed one): one record per match in logs/egg_matches.csv.
    #[allow(clippy::too_many_arguments)]
    pub fn egg_result(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64, u: f64, ux: f64, h: f64, title: &str) {
        self.egg.end_seen = now;
        self.egg_next(frames, im, now, u, ux, h);
        if now - self.egg.seen < 0.25 || now - self.egg.sent_at < EGG_SENT_S {
            return;
        }
        self.egg.seen = now;
        let k = u / 2000.0;
        let bx = |a: f64, b: f64, c: f64, d: f64| (ux + u * a, h * b, ux + u * c, h * d);
        let mut crops = IndexMap::new();
        crops.insert("difficulty".to_string(), prep::prep_bright(im, bx(0.035, 0.2, 0.088, 0.232), k, 2.0));
        crops.insert("mode".to_string(), prep::prep_text(im, bx(0.09, 0.2, 0.235, 0.232), k, 190, 3.0, 24));
        crops.insert("time".to_string(), prep::prep_short(im, bx(0.262, 0.2, 0.31, 0.235), k, 3));
        crops.insert("goal".to_string(), prep::prep_text(im, bx(0.05, 0.258, 0.26, 0.292), k, 190, 3.0, 24));
        crops.insert("chests".to_string(), prep::prep_bright(im, bx(0.18, 0.81, 0.305, 0.84), k, 2.0));
        let mut t = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
        let goal = t.get("goal").map_or("", |s| s.trim()).to_string();
        let (loot, defeats) = if let Some(reader) = self.name_reader() {
            let dy = if goal.is_empty() { 0.0 } else { 0.04 };
            (reader.number(im, bx(0.03, 0.285 + dy, 0.15, 0.345 + dy), 0.05, false, false, false),
             reader.number(im, bx(0.03, 0.39 + dy, 0.075, 0.45 + dy), 0.05, false, false, false))
        } else {
            let numbers = |m: &Self, dy: f64| {
                let b = bx(0.03, 0.465 + dy, 0.075, 0.535 + dy);
                let mut c = IndexMap::new();
                c.insert("loot".to_string(), prep::prep_text(im, bx(0.03, 0.345 + dy, 0.15, 0.42 + dy), k, 190, 3.0, 24));
                c.insert("defeats".to_string(), prep::prep_labelled(im, b, k, "Defeats:", 2.0));
                c.insert("defeats2".to_string(), prep::prep_labelled(im, b, k, "Defeats:", 3.0));
                c.insert("defeats3".to_string(), prep::prep_labelled(im, b, k, "Defeats:", 5.0));
                m.ocr.read(&c, &IndexMap::new(), &IndexMap::new())
            };
            let shifts = if goal.is_empty() { (-0.04, 0.0) } else { (0.0, -0.04) };
            t.extend(numbers(self, shifts.0));
            if whole(t.get("loot").map(String::as_str)).is_none() {
                t.extend(numbers(self, shifts.1));
            }
            let g = |k: &str| t.get(k).map(String::as_str);
            (whole(g("loot")), digits(g("defeats")).or_else(|| digits(g("defeats2"))).or_else(|| digits(g("defeats3"))))
        };
        let g = |k: &str| t.get(k).map_or("", String::as_str);
        let low = title.to_lowercase();
        let outcome = if low.contains("fail") || low.contains("unsuccess") { Some("failed") } else if low.contains("success") { Some("success") } else { None };
        static TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,2})\s*:\s*(\d{2})").unwrap());
        static CHESTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+)\s*/\s*(\d+)").unwrap());
        let seconds = TIME.captures(g("time")).map(|m| m[1].parse::<i64>().unwrap() * 60 + m[2].parse::<i64>().unwrap());
        let chests = CHESTS.captures(g("chests")).map(|m| format!("{}/{}", &m[1], &m[2]));
        let dl = g("difficulty").to_lowercase();
        let difficulty = ["Chaos", "Nightmare", "Expert", "Hard", "Normal", "Easy"].iter().find(|d| dl.contains(&d.to_lowercase())).map(|d| d.to_string())
            .or_else(|| Some(g("difficulty").trim().to_string()).filter(|s| !s.is_empty()));
        let m = g("mode");
        let mode = m.split_once(':').map_or(m, |x| x.1).trim().to_string();
        let sv = |v: Option<String>| v.map_or(Value::None, Value::Str);
        let mtch = dict([
            ("outcome", sv(outcome.map(String::from))),
            ("difficulty", sv(difficulty)),
            ("mode", if mode.is_empty() { Value::None } else { Value::Str(canon(&mode, &EGG_MODES, 0.6)) }),
            ("time_s", opt(seconds)),
            ("objective", sv(Some(goal.clone()).filter(|g| !g.is_empty()))),
            ("loot", opt(loot)),
            ("defeats", opt(defeats)),
            ("chests_today", sv(chests)),
        ]);
        log::info!(target: "meter", "Egg Heist result: OCR {} -> {}", str_map_repr(&t), mtch.repr());
        if loot.is_none() && seconds.is_none() {
            return;
        }
        if self.egg.read.as_ref().is_none_or(|r| now - r.seen > EGG_GONE_S) {
            self.egg.read = Some(EggRead { votes: Votes::new(), reads: 0, seen: now, im: im.clone() });
        }
        let read = self.egg.read.as_mut().unwrap();
        read.seen = now;
        read.im = im.clone();
        read.reads += 1;
        for (k, v) in mtch.as_dict().unwrap().iter() {
            vote(&mut read.votes, &k.py_str(), v.clone());
        }
        let agreed = ["outcome", "loot", "defeats", "time_s"].iter().all(|k| !confirmed(&read.votes, k).is_none());
        if agreed || read.reads >= EGG_MAX_READS {
            // (Next is to be pressed before the send: a hold sent at once carries on to the rank screen)
            let next = !self.args.replay && alogs_protocol::settings::automation(&self.args.cfg, Some("egg_auto_next"));
            if next {
                self.egg.click = Some(EggClick { at: now, last: 0.0, tries: 0 });
            }
            self.egg_send(frames, out, now);
            if next && self.egg.click.is_some() {
                self.egg_next(frames, im, now, u, ux, h);
            }
        }
    }

    /// An end screen that left before its reads agreed goes with what did agree; pictures kept for a rank screen that
    /// never came go.
    pub fn egg_gone(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, now: f64) {
        if self.egg.read.as_ref().is_some_and(|r| now - r.seen > EGG_GONE_S) {
            self.egg_send(frames, out, now);
        }
        if self.egg.pending.as_ref().is_some_and(|p| now - p.at.max(self.egg.end_seen) > EGG_RANK_WAIT_S) {
            self.egg_update(None);
        }
    }

    /// The match's record again, now with the rank screen's picture; without one, only the copies go. Not held for:
    /// the rank record read from the same screen is (it goes first, having no pictures), and this update with three
    /// pictures took 2.5 s more of the player's controls (25 Nov); the queue sends it right after.
    fn egg_update(&mut self, rank_shot: Option<String>) {
        let Some(p) = self.egg.pending.take() else { return };
        if let Some(r) = rank_shot
            && self.submitter.is_some()
        {
            self.send_result_held(&p.payload, p.shot.as_deref(), Some(&r), p.party.as_deref(), None, false, &[], None);
            return;
        }
        for r in [p.shot, p.party].into_iter().flatten() {
            let _ = std::fs::remove_file(self.env.data.join(r));
        }
    }

    fn egg_send(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, now: f64) {
        let Some(read) = self.egg.read.take() else { return };
        let votes = read.votes;
        self.egg.sent_at = now;
        let mut m = Dict::new();
        for k in ["outcome", "difficulty", "mode", "time_s", "objective", "loot", "defeats", "chests_today"] {
            m.insert(k.into(), confirmed(&votes, k));
        }
        if m.at("loot").is_none() && m.at("time_s").is_none() {
            self.egg_hold_end("the end screen gave nothing to send");
            return;
        }
        if m.at("time_s").is_none()
            && let Some(times) = votes.get("time_s")
            && times.len() == 1
        {
            m.insert("time_s".into(), times.values().next().unwrap().0.clone());
        }
        self.rs.screen_check = None;
        let shot = self.picture(frames, &read.im, "screens", &format!("egg_result_{}", local(now, "%Y%m%d_%H%M%S")), true, true);
        m.insert("screen_check".into(), self.rs.screen_check.clone().unwrap_or(Value::None));
        // who played it: the Party screen read before it, once
        let mut with_party = false;
        if let Some(Value::Dict(p)) = &mut self.party
            && p.at("egg").truthy()
            && now - p.at("at").as_f64().unwrap_or(0.0) < EGG_PARTY_S
        {
            m.insert("party".into(), dict([("party", p.at("members").clone()), ("teams", p.at("teams").clone()), ("team", p.at("team").clone())]));
            p.insert("egg".into(), Value::Bool(false));
            with_party = true;
        }
        let shot_rel = shot.as_ref().map(|p| rel(&self.env.data, p));
        if self.env.write_files {
            let path = self.env.data.join("logs").join("egg_matches.csv");
            let _ = std::fs::create_dir_all(self.env.data.join("logs"));
            let new = !path.exists();
            if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                let mut w = csv::WriterBuilder::new().terminator(csv::Terminator::CRLF).from_writer(f);
                if new {
                    let _ = w.write_record(["time", "uid", "outcome", "difficulty", "mode", "time_s", "objective", "loot", "defeats", "chests_today", "screenshot"]);
                }
                let c = |k: &str| { let v = m.at(k); if v.is_none() { String::new() } else { v.py_str() } };
                let _ = w.write_record([local(self.env.now(), "%Y-%m-%d %H:%M:%S"), self.uid.clone().unwrap_or_else(|| "None".into()),
                    c("outcome"), c("difficulty"), c("mode"), c("time_s"), c("objective"), c("loot"), c("defeats"), c("chests_today"), shot_rel.clone().unwrap_or_default()]);
            }
        }
        if self.submitter.is_some()
            && let Some(uid) = self.uid.clone()
        {
            self.egg_update(None);
            // the Party screen's picture as more proof, when that screen was read before this match
            let picture = if with_party { self.party_picture.clone() } else { None };
            let party_shot = picture.and_then(|pic| self.picture(frames, &pic, "screens", &format!("party_{}", local(now, "%Y%m%d_%H%M%S")), true, false));
            let party_rel = party_shot.as_ref().map(|p| rel(&self.env.data, p));
            let spare = |data: &Path, p: &Option<std::path::PathBuf>| -> Option<String> {
                let p = p.as_ref()?;
                let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
                let copy = std::path::PathBuf::from(format!("{}.again{ext}", p.to_string_lossy()));
                std::fs::copy(p, &copy).ok()?;
                Some(rel(data, &copy))
            };
            let clock = self.env.record_clock();
            let season = self.egg.season.clone().map_or(Value::None, Value::Str);
            let mut run = m.clone();
            if let Some(g) = self.game_build_now() {
                run.insert("game".into(), g);
            }
            let payload = payloads::egg_match_payload(&Value::Dict(run), &season, &Value::Str(uid), Some(now), &clock);
            self.egg.pending = Some(EggPending { at: now, payload: payload.clone(), shot: spare(&self.env.data, &shot), party: spare(&self.env.data, &party_shot) });
            if self.send_result_held(&payload, shot_rel.as_deref(), None, party_rel.as_deref(), None, true, &[], None).is_some() {
                self.egg.noticed_at = now; // the match's notice: its rank goes silently
            }
        }
        self.egg_queued(now);
        self.screen_settled("egg_result", self.env.now());
        let (outcome, loot, difficulty) = (m.at("outcome").clone(), m.at("loot").as_i64(), m.at("difficulty").clone());
        let msg = format!("Egg Heist match recorded: {}{}{}",
            if outcome.as_str() == Some("success") { "escaped".to_string() } else { outcome.py_str() },
            loot.map_or(String::new(), |l| format!(", {} loot", thousands(l))),
            if difficulty.truthy() { format!(" ({})", difficulty.py_str()) } else { String::new() });
        out.out("status", Value::Str(msg));
    }

    /// Egg Heist "Rank Info": rank title, "Rank Points X/Y" or the star count (Egg Emperor), and the season badge.
    pub fn egg_rank_info(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64, u: f64, ux: f64, h: f64) {
        if now - self.egg.stats_seen < 5.0 {
            return;
        }
        self.egg.stats_seen = now;
        let k = u / 2000.0;
        let b = |a: f64, b: f64, c: f64, d: f64| (ux + u * a, h * b, ux + u * c, h * d);
        let mut crops = IndexMap::new();
        crops.insert("title".to_string(), prep::prep_text(im, b(0.10, 0.195, 0.365, 0.275), k, 190, 3.0, 24));
        crops.insert("points".to_string(), prep::prep_text(im, b(0.105, 0.80, 0.36, 0.86), k, 190, 3.0, 24));
        crops.insert("stars".to_string(), prep::prep_short(im, b(0.215, 0.715, 0.29, 0.785), k, 3));
        crops.insert("stars2".to_string(), prep::prep_short(im, b(0.215, 0.715, 0.30, 0.785), k, 3));
        crops.insert("stars3".to_string(), prep::prep_short(im, b(0.225, 0.72, 0.28, 0.78), k, 3));
        crops.insert("badge".to_string(), prep::prep_short(im, b(0.918, 0.06, 0.962, 0.125), k, 3));
        crops.insert("season".to_string(), prep::prep_text(im, b(0.67, 0.05, 0.91, 0.13), k, 190, 3.0, 24));
        let t = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
        let g = |k: &str| t.get(k).map(String::as_str);
        let (points, next_start) = e::parse_points(g("points"));
        let stars = if points.is_none() { ["stars", "stars2", "stars3"].iter().find_map(|k| first_num(g(k))) } else { None };
        let st = e::standing(g("title"), points, stars);
        let season = e::parse_season(g("badge"), g("season").unwrap_or("")).or_else(|| self.season_now());
        if season.is_some() {
            self.egg.season = season.clone();
        }
        log::info!(target: "meter", "Egg Heist Rank Info: OCR {} -> {}, season {}, next big rank at {}", str_map_repr(&t),
            st.as_ref().map_or("None".into(), Value::repr), season.as_deref().unwrap_or("None"), next_start.map_or("None".into(), |n| n.to_string()));
        let Some(st) = st else { return };
        if st.get("points").is_none() && st.get("stars").is_none() {
            return;
        }
        let key = format!("{season:?}|{}|{}|{}", st.get("name").repr(), st.get("points").repr(), st.get("stars").repr());
        if self.egg.last.get("egg_rank") == Some(&key) {
            return;
        }
        self.egg.last.insert("egg_rank".into(), key);
        let shot = self.picture(frames, im, "screens", &format!("egg_rank_{}", local(now, "%Y%m%d_%H%M%S")), true, true);
        let shot_rel = shot.as_ref().map(|p| rel(&self.env.data, p));
        if self.env.write_files {
            let path = self.env.data.join("logs").join("scores.csv");
            let new = !path.exists();
            let _ = std::fs::create_dir_all(self.env.data.join("logs"));
            if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                let mut w = csv::WriterBuilder::new().terminator(csv::Terminator::CRLF).from_writer(f);
                if new {
                    let _ = w.write_record(["time", "uid", "cycle", "screen", "boss", "score", "stars", "screenshot"]);
                }
                let c = |v: &Value| if v.is_none() { String::new() } else { v.py_str() };
                let _ = w.write_record([local(self.env.now(), "%Y-%m-%d %H:%M:%S"), self.uid.clone().unwrap_or_else(|| "None".into()),
                    season.clone().unwrap_or_default(), "egg_rank".into(), st.get("name").py_str(), c(st.get("points")), c(st.get("stars")), shot_rel.clone().unwrap_or_default()]);
            }
        }
        self.screen_settled("egg_rank", self.env.now());
        let msg = format!("Egg Heist rank recorded: {}{}{}", st.get("name").py_str(),
            st.get("points").as_i64().map_or(String::new(), |p| format!(" - {} rank points", thousands(p))),
            st.get("stars").as_i64().map_or(String::new(), |s| format!(" - {} stars", thousands(s))));
        out.out("status", Value::Str(msg));
        if self.submitter.is_some() {
            let profile = self.egg_profile(season.as_deref(), &[("nextRankAt", opt(next_start))]);
            let clock = self.env.record_clock();
            let p = payloads::eggheist_payload(&st, &season.map_or(Value::None, Value::Str), &self.uid.clone().map_or(Value::None, Value::Str), Some(&profile), &clock);
            // (a menu screen the player opened and leaves when they like: nothing is held for it)
            let p = payloads::with_game(p, self.game_build_now());
            self.egg_send_rank(&p, shot_rel.as_deref(), false, now);
        }
    }

    /// A rank record goes the leaderboards' way (held until sent with `hold` while Egg Heist's hold runs). Silent when
    /// an Egg Heist record with a notice went within EGG_RANK_WAIT_S: its match, or an earlier read of the same screen.
    fn egg_send_rank(&mut self, p: &Value, shot: Option<&str>, hold: bool, now: f64) {
        let Some(queued) = self.send_result_held(p, shot, None, None, None, hold, &[], None) else { return };
        if now - self.egg.noticed_at < EGG_RANK_WAIT_S {
            self.quiet_sends.push_back(queued.json_dumps());
            while self.quiet_sends.len() > crate::leaderboards::NOTICED_RUNS {
                self.quiet_sends.pop_front();
            }
            log::info!(target: "meter", "Egg Heist: the rank goes without a notice of its own (one notice per run)");
        } else {
            self.egg.noticed_at = now;
        }
    }

    /// The season's numbers for the player's profile, kept in logs/egg_profile.json so the rank screens add to them.
    pub fn egg_profile(&self, season: Option<&str>, values: &[(&str, Value)]) -> Value {
        let path = self.env.data.join("logs").join("egg_profile.json");
        let mut kept = std::fs::read_to_string(&path).ok().and_then(|s| Value::json_loads(&s)).and_then(|v| v.as_dict().cloned()).unwrap_or_default();
        let key = format!("{}|{}", self.uid.clone().unwrap_or_else(|| "None".into()), season.unwrap_or("None"));
        let mut mine = kept.at(&key).as_dict().cloned().unwrap_or_default();
        for (k, v) in values {
            if !v.is_none() {
                mine.insert((*k).into(), v.clone());
            }
        }
        kept.insert(Value::Str(key), Value::Dict(mine.clone()));
        let _ = std::fs::create_dir_all(self.env.data.join("logs"));
        let _ = crate::submit::write_json(&path, &Value::Dict(kept), None);
        Value::Dict(mine)
    }

    /// The rank screen's newest frame is kept while it's up; when it leaves before a second read, that frame is read.
    pub fn egg_rank_watch(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, heads: &Heads, now: f64) {
        if rank_screen(heads) {
            if !self.ss.read_parts {
                self.egg.rank_frame = Some((im.clone(), now));
            }
            return;
        }
        let Some((_, at)) = &self.egg.rank_frame else {
            self.egg.points = None;
            return;
        };
        if now - at < 0.4 {
            return;
        }
        let (frame, at) = self.egg.rank_frame.take().unwrap();
        self.egg.points = None;
        if self.egg.rank_read_at >= at || now - at > 10.0 {
            return;
        }
        let (w, h) = (frame.width as f64, frame.height as f64);
        let u = w.min(h * 16.0 / 9.0);
        log::info!(target: "meter", "Egg Heist: the rank screen left before a second read: reading the frame kept of it");
        self.egg_rank_gain(frames, out, &frame, now, u, (w - u) / 2.0, h, true);
    }

    /// Egg Heist's rank screen after a match: the rank title, the Rank Points bar "1337/1350" and what the match added.
    #[allow(clippy::too_many_arguments)]
    pub fn egg_rank_gain(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64, u: f64, ux: f64, h: f64, once: bool) {
        if now - self.egg.gain_seen < 5.0 {
            return;
        }
        let k = u / 2000.0;
        let b = |a: f64, bb: f64, c: f64, d: f64| (ux + u * a, h * bb, ux + u * c, h * d);
        let two = |m: &Self, hi: (f64, f64, f64, f64), lo: (f64, f64, f64, f64)| {
            let mut c = IndexMap::new();
            c.insert("high".to_string(), prep::prep_text(im, hi, k, 190, 3.0, 24));
            c.insert("low".to_string(), prep::prep_text(im, lo, k, 190, 3.0, 24));
            m.ocr.read(&c, &IndexMap::new(), &IndexMap::new())
        };
        let spots = two(self, b(0.72, 0.5374, 0.83, 0.5874), b(0.72, 0.555, 0.83, 0.605));
        let points_text = ["high", "low"].iter().filter_map(|s| spots.get(*s)).find(|v| POINTS_LINE.is_match(v)).cloned();
        let Some(points_text) = points_text else {
            let eggs = two(self, b(0.465, 0.7124, 0.55, 0.7824), b(0.465, 0.73, 0.55, 0.80));
            if let Some(count) = ["high", "low"].iter().filter_map(|s| eggs.get(*s)).find(|v| EGGS_LINE.is_match(v)).cloned() {
                self.egg_emperor_count(frames, out, im, now, u, ux, h, &count);
            }
            return;
        };
        if self.egg.points.as_ref() != Some(&points_text) && !once {
            self.egg.points = Some(points_text);
            return;
        }
        self.egg.points = None;
        self.egg.gain_seen = now;
        let mut crops = IndexMap::new();
        crops.insert("title".to_string(), prep::prep_bright(im, b(0.38, 0.215, 0.62, 0.265), k, 2.0));
        crops.insert("title2".to_string(), prep::prep_text(im, b(0.37, 0.20, 0.63, 0.275), k, 230, 3.0, 24));
        let sizes = ["gain4", "gain3.5", "gain5", "gain2"];
        for (name, size) in sizes.iter().zip([4.0, 3.5, 5.0, 2.0]) {
            if let Some(p) = prep::prep_pill(im, b(0.86, 0.53, 0.915, 0.605), k, size, 24) {
                crops.insert(name.to_string(), p);
            }
        }
        let t = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
        let g = |k: &str| t.get(k).map(String::as_str);
        let (points, bar_end) = e::parse_points(Some(&points_text));
        let mut gain = sizes.iter().find_map(|n| e::parse_gain(g(n)));
        let title = if e::tier_of(g("title")).is_some() { g("title") } else { g("title2") };
        let st = e::standing(title, points, None);
        let season = self.season_now();
        let limit = self.dev.on("egg-gain-limit");
        let tier = st.as_ref().and_then(|s| s.get("tier").as_i64());
        let within = |found: Option<i64>| -> Option<i64> {
            if !limit || tier.is_none() || e::gain_possible(found, tier) {
                return found;
            }
            log::info!(target: "meter", "Egg Heist: {:+} is above the {} a match can add at this rank: taken as a misread", found.unwrap(), e::point_limit(tier.unwrap()).unwrap_or(0));
            None
        };
        gain = within(gain);
        let before = self.egg_profile(season.as_deref(), &[]).get("bar").as_list().and_then(|l| l.first().and_then(Value::as_i64));
        if gain.is_none()
            && let (Some(b4), Some(p)) = (before, points)
            && p != b4
        {
            gain = within(Some(p - b4));
        }
        log::info!(target: "meter", "Egg Heist rank after a match: OCR {}, {} -> {}, +{}, bar to {}, season {}", str_map_repr(&t), alogs_pyval::str_repr(&points_text),
            st.as_ref().map_or("None".into(), Value::repr), gain.map_or("None".into(), |g| g.to_string()), bar_end.map_or("None".into(), |n| n.to_string()), season.as_deref().unwrap_or("None"));
        let (Some(st), Some(points)) = (st, points) else { return };
        let update = self.egg_rank_seen(frames, im, now);
        let key = format!("{season:?}|{points}|{gain:?}");
        if self.egg.last.get("egg_gain") == Some(&key) {
            self.egg_update_after(update);
            self.egg_queued(now);
            return;
        }
        self.egg.last.insert("egg_gain".into(), key);
        out.out("status", Value::Str(format!("Egg Heist: {}Rank Points ({}, {})", gain.map_or(String::new(), |g| format!("{g:+} ")), st.get("name").py_str(), thousands(points))));
        if self.submitter.is_some() {
            let mut vals = vec![];
            if let Some(g) = gain {
                vals.push(("lastGain", Value::Int(g)));
            }
            vals.push(("bar", Value::List(vec![Value::Int(points), opt(bar_end)])));
            vals.push(("barAt", Value::Int(self.env.now() as i64)));
            let profile = self.egg_profile(season.as_deref(), &vals);
            let clock = self.env.record_clock();
            let p = payloads::eggheist_payload(&st, &season.map_or(Value::None, Value::Str), &self.uid.clone().map_or(Value::None, Value::Str), Some(&profile), &clock);
            let p = payloads::with_game(p, self.game_build_now());
            self.egg_send_rank(&p, None, true, now);
        }
        self.egg_update_after(update);
        self.egg_queued(now);
    }

    /// The rank screen after a match is read: its picture goes with the match waiting for it (the rank record after it
    /// is what the hold waits for).
    /// Its picture is taken now; the match's update is queued by egg_update_after, after the rank record: queued
    /// first, the sender could start uploading it before the rank was queued.
    fn egg_rank_seen(&mut self, frames: &mut dyn Frames, im: &Image, now: f64) -> Option<Option<String>> {
        self.egg.rank_read_at = now;
        self.egg.click = None;
        self.egg.rank_frame = None;
        self.egg.pending.as_ref()?;
        let shot = self.picture(frames, im, "screens", &format!("egg_rank_{}", local(now, "%Y%m%d_%H%M%S")), true, true);
        Some(shot.map(|p| rel(&self.env.data, &p)))
    }

    /// The match update egg_rank_seen left for after the rank record (Some: a match was waiting for its rank screen).
    fn egg_update_after(&mut self, update: Option<Option<String>>) {
        if let Some(r) = update {
            self.egg_update(r);
        }
    }

    /// The post-match screen at Egg Emperor: the eggs collected ("x57") are the season's standing.
    #[allow(clippy::too_many_arguments)]
    pub fn egg_emperor_count(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64, u: f64, ux: f64, h: f64, count_text: &str) {
        if now - self.egg.gain_seen < 5.0 {
            return;
        }
        self.egg.gain_seen = now;
        let update = self.egg_rank_seen(frames, im, now);
        let k = u / 2000.0;
        let b = |a: f64, bb: f64, c: f64, d: f64| (ux + u * a, h * bb, ux + u * c, h * d);
        let mut crops = IndexMap::new();
        crops.insert("title".to_string(), prep::prep_text(im, b(0.37, 0.20, 0.63, 0.275), k, 230, 3.0, 24));
        crops.insert("title2".to_string(), prep::prep_bright(im, b(0.38, 0.215, 0.62, 0.265), k, 2.0));
        let t = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
        let g = |k: &str| t.get(k).map(String::as_str);
        let title = if e::tier_of(g("title")).is_some() { g("title") } else { g("title2") };
        let eggs = first_num(Some(count_text));
        let st = if e::tier_of(title) == Some(e::TOP_TIER) { e::standing(title, None, eggs) } else { None };
        let season = self.season_now();
        log::info!(target: "meter", "Egg Heist rank after a match (Egg Emperor): OCR {}, {} -> {}, season {}", str_map_repr(&t), alogs_pyval::str_repr(count_text),
            st.as_ref().map_or("None".into(), Value::repr), season.as_deref().unwrap_or("None"));
        let (Some(st), Some(eggs)) = (st, eggs) else {
            self.egg_update_after(update);
            return;
        };
        let key = format!("{season:?}|emperor|{eggs}");
        if self.egg.last.get("egg_gain") == Some(&key) {
            self.egg_update_after(update);
            self.egg_queued(now);
            return;
        }
        self.egg.last.insert("egg_gain".into(), key);
        out.out("status", Value::Str(format!("Egg Heist: Egg Emperor, {} eggs", thousands(eggs))));
        if self.submitter.is_some() {
            let mut profile = Dict::new();
            for (k, v) in self.egg_profile(season.as_deref(), &[]).as_dict().cloned().unwrap_or_default().iter() {
                if !matches!(k.py_str().as_str(), "bar" | "barAt" | "lastGain" | "nextRankAt") {
                    profile.insert(k.clone(), v.clone());
                }
            }
            let clock = self.env.record_clock();
            let p = payloads::eggheist_payload(&st, &season.map_or(Value::None, Value::Str), &self.uid.clone().map_or(Value::None, Value::Str), Some(&Value::Dict(profile)), &clock);
            let p = payloads::with_game(p, self.game_build_now());
            self.egg_send_rank(&p, None, true, now);
        }
        self.egg_update_after(update);
        self.egg_queued(now);
    }

    /// Esc menu in boss rush: 'Challenge Stats' with this week's Best Score and Current Best stars.
    pub fn challenge_stats(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64, u: f64, ux: f64, h: f64) {
        if now - self.egg.stats_seen < 5.0 {
            return;
        }
        self.egg.stats_seen = now;
        let k = u / 2000.0;
        let mut crops = IndexMap::new();
        crops.insert("score".to_string(), prep::prep_text(im, (ux + u * 0.31, h * 0.325, ux + u * 0.455, h * 0.415), k, 190, 1.0, 24));
        crops.insert("stars".to_string(), prep::prep_short(im, (ux + u * 0.555, h * 0.33, ux + u * 0.62, h * 0.41), k, 3));
        let text = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
        let s = text.get("score").map_or("", String::as_str);
        let score = if s.chars().any(|c| c.is_ascii_digit()) {
            s.replace(['O', 'o'], "0").chars().filter(char::is_ascii_digit).collect::<String>().parse::<i64>().ok()
        } else {
            None
        };
        let stars = first_num(text.get("stars").map(String::as_str));
        log::info!(target: "meter", "Challenge Stats screen: OCR {} -> best score {}, stars {}", str_map_repr(&text),
            score.map_or("None".into(), |v| v.to_string()), stars.map_or("None".into(), |v| v.to_string()));
        if score.is_some() {
            self.record_score(frames, out, im, "challenge_stats", "all (weekly best)", score, stars);
        }
    }

    /// Hand an Egg Heist rank or end screen to its reader (read_screen's Egg Heist part). True when it was the end screen.
    #[allow(clippy::too_many_arguments)]
    pub fn egg_screens(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64, u: f64, ux: f64, h: f64, heads: &Heads) -> bool {
        let g = |k: &str| heads.get(k).cloned().unwrap_or_default();
        if crate::screens::rank_line(heads) || POINTS_LINE.is_match(&g("ep")) {
            self.egg_rank_gain(frames, out, im, now, u, ux, h, false);
        } else if EGGS_LINE.is_match(&g("es")) {
            self.egg_emperor_count(frames, out, im, now, u, ux, h, &g("es"));
        }
        if g("eh").to_lowercase().contains("adventure") {
            self.test_frame(im, "egg_result", now);
            self.egg_result(frames, out, im, now, u, ux, h, &g("eh"));
            return true;
        }
        false
    }

    /// Does an Egg Heist screen spotted now get held? The leaderboards' Automation on, something to hold (keyboard and
    /// mouse buttons, or the controller), a live meter and the game in front: the gates of a send's hold
    /// (holding_for_send).
    fn egg_hold_allowed(&self, foreground: bool) -> bool {
        let r = self.args.cfg.get("results");
        let something = !matches!(r.get("hold_inputs"), Value::Bool(false)) || r.get("hold_controller").truthy();
        !self.args.replay && alogs_protocol::settings::automation(&self.args.cfg, None) && something && (self.hwnd.is_none() || foreground)
    }

    /// Hold now (again: for the whole time limit from now), as the Sim's end does (sim_control): keyboard and mouse
    /// buttons, and the controller, by telling the game it is inactive (it stays in front); HidHide's too when "Block
    /// the controller too" is on.
    fn egg_hold_take(&mut self, now: f64, screen: &'static str, on_to_rank: bool) -> f64 {
        let seconds = crate::leaderboards::hold_limit(&self.args.cfg);
        let r = self.args.cfg.get("results").clone();
        let held = !matches!(r.get("hold_inputs"), Value::Bool(false));
        if held {
            crate::overlay::control_mode("egg");
            self.hold.start(seconds);
            if let Some(h) = self.hwnd
                && !self.args.replay
            {
                self.told_inactive.take(h, seconds);
            }
        }
        if r.get("hold_controller").truthy() && !alogs_capture::input::controllers_hidden() && alogs_capture::input::hidhide_available() {
            alogs_capture::input::hide_controllers(true);
        }
        self.egg.hold = Some(EggHold { until: now + seconds, screen, on_to_rank, held });
        seconds
    }

    /// Lets go: keyboard, mouse and the controller back to the player at once.
    pub fn egg_hold_end(&mut self, why: &str) {
        if self.egg.hold.take().is_none() {
            return;
        }
        self.hold.stop();
        self.told_inactive.stop();
        if alogs_capture::input::controllers_hidden() {
            alogs_capture::input::hide_controllers(false);
        }
        log::info!(target: "meter", "Egg Heist input hold ended: {why}");
    }

    /// An Egg Heist end screen, or the rank screen after a match, is spotted: inputs held from now (before it is
    /// read) until what it shows is sent, so a press can't skip past it. The hold that carries on from the end screen
    /// to the rank screen goes on.
    pub fn egg_screen_hold(&mut self, spotted: Option<&str>, heads: &Heads, now: f64, foreground: bool) {
        if spotted != Some("Egg Heist") {
            return;
        }
        let end = heads.get("eh").is_some_and(|v| v.to_lowercase().contains("adventure"));
        if let Some(h) = &mut self.egg.hold {
            if !end && h.screen == "end screen" && h.on_to_rank {
                h.screen = "rank screen";
                log::info!(target: "meter", "Egg Heist: the rank screen is up; inputs stay held until it is read and sent");
            }
            return;
        }
        if !self.egg_hold_allowed(foreground) {
            return;
        }
        if end {
            if now - self.egg.sent_at < EGG_SENT_S {
                return;
            }
        } else if !(now - self.egg.sent_at.max(self.egg.end_seen) < EGG_RANK_WAIT_S && now - self.egg.rank_read_at > EGG_RANK_WAIT_S) {
            return;
        }
        let screen = if end { "end screen" } else { "rank screen" };
        let seconds = self.egg_hold_take(now, screen, false);
        log::info!(target: "meter", "Egg Heist: the {screen} is up: holding inputs until it is read and sent (at most {seconds} s)");
    }

    /// Something of this screen was queued (or nothing was to be): with nothing waiting to be sent, that's as sent.
    fn egg_queued(&mut self, now: f64) {
        if self.egg.hold.is_some() && self.send_waiting.is_none() {
            self.egg_sent(now);
        }
    }

    /// What the hold waited for is sent: the end screen's record with Next pressed by aLogs carries on to the rank
    /// screen (a new time limit from now); anything else lets go at once.
    pub fn egg_sent(&mut self, now: f64) {
        let Some(h) = self.egg.hold.clone() else { return };
        if h.screen == "end screen" && !h.on_to_rank && self.egg.click.is_some() {
            let seconds = self.egg_hold_take(now, "end screen", true);
            log::info!(target: "meter", "Egg Heist: the match is sent and aLogs presses Next: inputs stay held until the rank screen is read and sent (at most {seconds} s more)");
            return;
        }
        self.egg_hold_end(&format!("the {} is sent", h.screen));
    }

    /// aLogs gave up pressing Next: the player goes on, so a hold kept for the rank screen lets go.
    fn egg_next_given_up(&mut self) {
        if self.egg.hold.as_ref().is_some_and(|h| h.on_to_rank && h.screen == "end screen") {
            self.egg_hold_end("Next was not pressed: it's left to the player");
        }
    }

    /// Every look: the hold's time limit, the player taking control back, and the game going to the background, let
    /// go.
    pub fn egg_hold_tick(&mut self, now: f64, foreground: bool) {
        let Some(h) = &self.egg.hold else { return };
        if now > h.until {
            self.egg_hold_end("time limit");
        } else if h.held && !self.hold.active() && now - h.until < -1.0 {
            self.egg_hold_end("you took control back (Ctrl+F12)");
        } else if !foreground {
            self.egg_hold_end("the game is not the active window");
        }
    }
}
