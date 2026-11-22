//! The screens before a run: the boss rush battle prep screen (best score, the Party row), Interlink's Party screen
//! (who is in each slot, AI or not, their lead's level and CP), and the Interlink screen's star caps. Port of those
//! parts of aLogs B16 `bossbar/live.py` and `fights.py` (cycle_boss). Each card's team and lead from their portraits,
//! the Special Offering and the Season Buffs are in party_icons.rs.

use crate::meter::{Frames, Meter, Out};
use crate::results::star_caps;
use crate::results_parse::{bot_sign, is_bot_name, names_alike};
use crate::sim::str_map_repr;
use crate::team::{Teammates, party_pill};
use alogs_gamelog::difflib::ratio;
use alogs_gamelog::{interlink_score, same_name};
use alogs_pil::Image;
use alogs_pyval::{Dict, Value, dict};
use alogs_reader::boss::Row;
use alogs_reader::prep;
use indexmap::IndexMap;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

type BoxF = (f64, f64, f64, f64);

/// Interlink's Party screen: the four slots' name pills (1P middle-left, 2P middle-right, 3P far left, 4P far right).
pub const PARTY_SLOTS: [BoxF; 4] = [(0.30, 0.17, 0.44, 0.21), (0.58, 0.185, 0.70, 0.225), (0.095, 0.222, 0.215, 0.262), (0.815, 0.209, 0.94, 0.249)];
/// ...and each slot's lead Aniimo card ('Irisalis / Lv.60 CP:3578'; CP only on a player's).
pub const PARTY_CARDS: [BoxF; 4] = [(0.33, 0.548, 0.44, 0.605), (0.598, 0.563, 0.71, 0.62), (0.116, 0.6, 0.228, 0.657), (0.842, 0.587, 0.952, 0.643)];
/// Egg Heist's Party screen: up to three players.
pub const EGG_PARTY_SLOTS: [BoxF; 3] = [(0.44, 0.197, 0.545, 0.226), (0.143, 0.2515, 0.2525, 0.278), (0.73, 0.2267, 0.8275, 0.256)];
pub const EGG_PARTY_CARDS: [BoxF; 3] = [(0.4505, 0.569, 0.5605, 0.626), (0.162, 0.6236, 0.272, 0.6806), (0.743, 0.5996, 0.853, 0.6566)];
/// The Interlink screen: each boss's stars as 'earned/cap', left, center, right.
pub const INTERLINK_STARS: [BoxF; 3] = [(0.18, 0.50, 0.28, 0.565), (0.44, 0.375, 0.53, 0.43), (0.71, 0.44, 0.78, 0.485)];
/// Where the panel goes when everyone's ready, of the height.
pub const PANEL_RAISED: f64 = -0.0352;

static CARD_LEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*([A-Za-z][A-Za-z' -]{1,24}?)\s*(?:/|$)").unwrap());
static CP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"CP\W{0,3}(\d{3,6})").unwrap());
static LV: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"L\s*v\s*\.?\s*(\d{1,2})").unwrap());

/// Longer than the Party screen is away within one lobby (in the owner's logs: 76 s at most, 693 gaps), so a Party
/// screen seen again after it is another lobby's even when the game log said nothing.
pub const LOBBY_AWAY_S: f64 = 180.0;

/// Where the seats' votes were cast: the game log's scene, how many fights and results the meter had seen, the last
/// look. Any of the three changed = another lobby.
#[derive(Clone, Debug, PartialEq)]
struct Lobby {
    space: Option<String>,
    runs: u64,
    at: f64,
}

/// The voting state behind `Meter::party` (Counters, sets and tuple keys Python keeps in the same dict). All of it
/// belongs to one lobby: a new lobby starts it again (Meter::lobby_change).
#[derive(Default)]
pub struct PartyState {
    /// each seat's names over the looks
    votes: BTreeMap<i64, IndexMap<String, i64>>,
    /// the seats whose player was counted as seen (Teammates::saw) once
    counted: BTreeSet<i64>,
    /// the seats the last look missed (kept for that one look)
    missed: BTreeSet<i64>,
    /// bot or player, by (seat, name), over the looks
    ai_signs: IndexMap<(i64, String), [i64; 2]>,
    /// each seat's names on the looks in a row that read it unlike its leading name (a new player once two agree)
    unlike: BTreeMap<i64, Vec<String>>,
    lobby: Option<Lobby>,
    /// fights started and results recorded: a party's votes never outlive a run
    pub runs: u64,
    /// Interlink results recorded: a Party read goes with the first result after it only (party_for_result)
    pub results: u64,
    /// when the last of them was
    pub result_at: f64,
    pub egg_party: bool,
    pub party_seen: f64,
    pub interlink_seen: f64,
    pub prep_read: Option<String>,
}

fn px(im: &Image, x: i64, y: i64) -> [u8; 3] {
    if x < 0 || y < 0 || x as usize >= im.width || y as usize >= im.height {
        return [0, 0, 0];
    }
    let p = im.getpixel(x as usize, y as usize);
    match p.len() {
        1 | 2 => [p[0], p[0], p[0]],
        _ => [p[0], p[1], p[2]],
    }
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(char::is_ascii_lowercase).collect()
}

fn most_common(c: &IndexMap<String, i64>) -> String {
    let mut best: Option<(&String, i64)> = None;
    for (k, &n) in c {
        if best.is_none_or(|b| n > b.1) {
            best = Some((k, n));
        }
    }
    best.map(|b| b.0.clone()).unwrap_or_default()
}

fn sorted_desc(mut v: Vec<(f64, String)>) -> Vec<(f64, String)> {
    v.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    v
}

fn md(m: &Dict, k: &str) -> Value {
    m.at(k).clone()
}

/// (name, lead's species), folded: who a Party screen card is as far as its team goes.
fn who(m: &Dict) -> (String, String) {
    let f = |k: &str| {
        let v = m.at(k);
        let s = if v.is_none() { String::new() } else { v.py_str() };
        crate::team::casefold(&s.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect::<String>())
    };
    (f("name"), f("aniimo"))
}

impl Meter {
    /// The boss name on the battle prep screen matched to one of this week's three bosses; '' when it clearly matches none.
    pub fn cycle_boss(&self, text: &str) -> String {
        let now = self.env.now();
        let ids: BTreeSet<String> = self.env.schedule.active_bosses(now).iter().map(i64::to_string).collect();
        let titled = |r: &&Row| r["table"] == "puppet_data" && !r["bossTitle_en"].is_empty();
        let names: BTreeSet<String> = self.catalog.iter().filter(titled).filter(|r| ids.contains(&r["puppetId"])).map(|r| r["bossTitle_en"].clone()).collect();
        let t = norm(text);
        if names.is_empty() {
            let every: BTreeSet<String> = self.catalog.iter().filter(titled).map(|r| r["bossTitle_en"].clone()).collect();
            let scored = sorted_desc(every.into_iter().map(|n| (ratio(&t, &norm(&n)), n)).collect());
            if let Some(f) = scored.first()
                && (f.0 == 1.0 || (f.0 >= 0.85 && (scored.len() < 2 || f.0 - scored[1].0 >= 0.1)))
            {
                return f.1.clone();
            }
            return text.to_string();
        }
        let scored = sorted_desc(names.iter().map(|n| (ratio(&t, &norm(n)), n.clone())).collect());
        let (best, name) = scored[0].clone();
        let runner_up = scored.get(1).map_or(0.0, |s| s.0);
        if best >= 0.6 && best - runner_up >= 0.1 {
            return name;
        }
        let fits: Vec<&String> = names.iter().filter(|n| t.chars().count() >= 3 && norm(n).contains(&t)).collect();
        if fits.len() == 1 {
            return fits[0].clone();
        }
        log::info!(target: "meter", "battle prep screen: boss name {} clearly matches none of this week's bosses ({:?})", alogs_pyval::str_repr(text), &scored[..scored.len().min(2)]);
        String::new()
    }

    fn cycle_row(&self, boss: &str) -> Option<Row> {
        let ids = self.env.schedule.active_bosses(self.env.now());
        self.catalog.iter().find(|r| r["table"] == "puppet_data" && r["bossTitle_en"] == boss && r["puppetId"].parse::<i64>().is_ok_and(|i| ids.contains(&i))).cloned()
    }

    /// Battle prep screen of one boss: its name top-left, 'Best score: 24459 | (star) 15' top-right.
    #[allow(clippy::too_many_arguments)]
    pub fn prep_screen(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64, u: f64, ux: f64, h: f64, pill: &str) {
        if now - self.egg.stats_seen < 2.0 {
            return;
        }
        self.egg.stats_seen = now;
        self.keep_sample(im, "prep", None, None);
        let mut crops = IndexMap::new();
        crops.insert("n".to_string(), prep::prep_text(im, (ux + u * 0.065, h * 0.025, ux + u * 0.3, h * 0.085), u / 2000.0, 190, 3.0, 24));
        crops.insert("label".to_string(), prep::prep_label(im, u, ux, h));
        let t = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
        let name = t.get("n").cloned().unwrap_or_default();
        let label = t.get("label").map_or(String::new(), |l| l.to_lowercase());
        let slot = [("left", &["left"][..]), ("middle", &["middle", "midd", "iddle"][..]), ("right", &["right", "ight"][..])]
            .iter().find(|(_, words)| words.iter().any(|w| label.contains(w))).map(|(s, _)| *s);
        let slots = self.env.schedule.active_slots(now);
        let slot_id = slot.and_then(|s| slots.get(s).cloned());
        let slot_row = slot_id.and_then(|id| self.catalog.iter().find(|r| r["puppetId"] == id && r["table"] == "puppet_data" && !r["bossTitle_en"].is_empty()).cloned());
        static NUMS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d[\d,]*").unwrap());
        let after = pill.split_once(':').map_or(pill, |x| x.1);
        let nums: Vec<i64> = NUMS.find_iter(after).filter_map(|m| m.as_str().replace(',', "").parse().ok()).collect();
        let score = nums.first().copied();
        let stars = if nums.len() > 1 { nums.last().copied() } else { None };
        let mut boss = match &slot_row {
            Some(r) => r["bossTitle_en"].clone(),
            None => {
                let a = self.cycle_boss(name.trim_matches(|c| c == ' ' || c == '/' || c == '|'));
                if a.is_empty() { self.cycle_boss(label.rsplit('/').next().unwrap_or("").trim()) } else { a }
            }
        };
        self.read_party(im, now, u, ux, h);
        let row = self.stage_first(slot_row.or_else(|| self.cycle_row(&boss)));
        if let Some(r) = &row {
            self.prep_boss = Some((now, r.clone()));
            if !r["bossTitle_en"].is_empty() {
                boss = r["bossTitle_en"].clone();
            }
        }
        let two_boss = row.as_ref().is_some_and(|r| self.env.schedule.stage_of(Some(&r["puppetId"]), None, now).1.is_some());
        let cs = row.as_ref().map(|r| self.clear_stars(Some(&r["puppetId"])));
        let score = interlink_score(score, stars, cs, two_boss);
        log::info!(target: "meter", "battle prep screen: OCR pill {}, name {} -> {boss}: best score {}, stars {}", alogs_pyval::str_repr(pill), alogs_pyval::str_repr(&name),
            score.map_or("None".into(), |v| v.to_string()), stars.map_or("None".into(), |v| v.to_string()));
        let key = format!("{boss}|{score:?}|{stars:?}");
        let agreed = self.ps.prep_read.as_ref() == Some(&key);
        self.ps.prep_read = Some(key);
        if score.is_some() && stars.is_some() && !boss.is_empty() && agreed {
            self.record_score(frames, out, im, "battle_prep", &boss, score, stars);
        }
    }

    /// The Party row of the battle prep screen: four slots, each a name under a badge ('1P'..'4P' in colour for a
    /// player, a white 'Ai' for an AI teammate), your own name in green. Found by slot 1's yellow '1P' badge.
    pub fn read_party(&mut self, im: &Image, now: f64, u: f64, ux: f64, h: f64) {
        let x0 = (ux + u * 0.5125) as i64;
        let x1 = (x0 as f64 + u * 0.016) as i64;
        let mut top = (0, 0i64);
        for y in ((h * 0.58) as i64..(h * 0.72) as i64).step_by(2) {
            let yellow = (x0..x1).step_by(2).filter(|&x| { let p = px(im, x, y); p[0] >= 0xd0 && p[1] >= 0xb8 && p[2] <= 0x90 }).count();
            if yellow > top.0 {
                top = (yellow, y);
            }
        }
        if top.0 < 8 {
            return;
        }
        let yc = top.1 as f64 / h;
        let k = u / 2000.0;
        let mut crops = IndexMap::new();
        let mut kinds = vec![];
        let mut mine = vec![];
        for i in 0..4 {
            let x0 = (ux + u * (0.5125 + i as f64 * 0.0655)) as i64;
            let x1 = (x0 as f64 + u * 0.016) as i64;
            let (y0, y1) = ((h * (yc - 0.013)) as i64, (h * (yc + 0.013)) as i64);
            let pxs: Vec<[u8; 3]> = (x0..x1).step_by(2).flat_map(|x| (y0..y1).step_by(2).map(move |y| (x, y))).map(|(x, y)| px(im, x, y)).collect();
            let rng = |p: &[u8; 3]| (*p.iter().max().unwrap(), *p.iter().min().unwrap());
            let white = pxs.iter().filter(|p| { let (mx, mn) = rng(p); mn >= 0xd8 && mx - mn <= 0x22 }).count();
            let colour = pxs.iter().filter(|p| { let (mx, mn) = rng(p); mx >= 0xb0 && mx - mn >= 0x38 }).count();
            let n = pxs.len() as f64;
            kinds.push(if white > 3 * colour && white as f64 > 0.05 * n { Some("ai") } else if colour > 3 * white && colour as f64 > 0.05 * n { Some("player") } else { None });
            let c = ux + u * (0.5375 + i as f64 * 0.0655);
            let b = (c - u * 0.03, h * (yc + 0.051), c + u * 0.03, h * (yc + 0.077));
            crops.insert(format!("n{i}"), prep::prep_text(im, b, k, 190, 3.0, 24));
            let name_px: Vec<[u8; 3]> = ((b.0 as i64)..(b.2 as i64)).step_by(2).flat_map(|x| ((b.1 as i64)..(b.3 as i64)).step_by(2).map(move |y| (x, y))).map(|(x, y)| px(im, x, y)).collect();
            let green = name_px.iter().filter(|p| p[1] >= 0xb0 && i32::from(p[1]) - i32::from(p[0]) >= 0x30 && i32::from(p[1]) - i32::from(p[2]) >= 0x30).count();
            let bright = name_px.iter().filter(|p| *p.iter().min().unwrap() >= 0xd0).count();
            mine.push(green > 40 && green > 3 * bright);
        }
        let sizes: IndexMap<String, usize> = (0..4).map(|i| (format!("n{i}"), 24)).collect();
        let names = self.ocr.read(&crops, &sizes, &IndexMap::new());
        let members: Vec<Dict> = kinds.iter().enumerate().filter_map(|(i, kind)| {
            let kind = (*kind)?;
            let name = names.get(&format!("n{i}")).map_or("", String::as_str).trim_matches(|c| c == ' ' || c == '/' || c == '|').to_string();
            // no CP here: a name that isn't a bot's is a player's, whatever the mark; a bot's name leaves the mark as
            // this look's guess (the Party screen's cards decide)
            let bot = is_bot_name(&self.bot_names, &name);
            let sign = if bot && !mine[i] { Value::None } else { Value::Bool(false) };
            Some(dict([("slot", Value::Int(i as i64)), ("name", Value::Str(name)), ("ai", Value::Bool(kind == "ai" && bot && !mine[i])), ("ai_sign", sign), ("self", Value::Bool(mine[i]))]).as_dict().cloned().unwrap())
        }).filter(|m| m.at("name").truthy()).collect();
        self.set_party(members, now);
    }

    /// The week's star caps from the Interlink screen ('14/20', '36/40', '20/20'), saved when they make sense.
    pub fn interlink_screen(&mut self, im: &Image, now: f64, u: f64, ux: f64, h: f64) {
        if now - self.ps.interlink_seen < 30.0 {
            return;
        }
        self.ps.interlink_seen = now;
        let k = u / 2000.0;
        let mut crops = IndexMap::new();
        for (i, s) in INTERLINK_STARS.iter().enumerate() {
            let b = (ux + u * s.0, h * s.1, ux + u * s.2, h * s.3);
            crops.insert(format!("b{i}"), prep::prep_bright(im, b, k, 2.0));
            crops.insert(format!("t{i}"), prep::prep_text(im, b, k, 190, 3.0, 24));
        }
        let t = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
        static CAP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/\s*(\d{2})(\D|$)").unwrap());
        let mut caps: Vec<Option<i64>> = vec![];
        for i in 0..3 {
            let found: Vec<i64> = [format!("b{i}"), format!("t{i}")].iter().flat_map(|key| {
                let s = t.get(key).cloned().unwrap_or_default();
                CAP.captures_iter(&s).filter_map(|c| c[1].parse().ok()).collect::<Vec<i64>>()
            }).collect();
            // max(set(found), key=found.count): the commonest, CPython's set order breaking ties (small ints: by value)
            let set: BTreeSet<i64> = found.iter().copied().collect();
            let mut best: Option<(i64, usize)> = None;
            for v in set {
                let n = found.iter().filter(|x| **x == v).count();
                if best.is_none_or(|b| n > b.1) {
                    best = Some((v, n));
                }
            }
            caps.push(best.map(|b| b.0));
        }
        let side = caps[0].or(caps[2]);
        let mut center = caps[1];
        if let Some(s) = side
            && center.is_none()
            && caps[0] == caps[2]
        {
            center = Some(2 * s);
        }
        let ok = match (side, center) {
            (Some(s), Some(c)) => caps[0].is_none_or(|v| v == s) && caps[2].is_none_or(|v| v == s) && c == 2 * s && (10..=60).contains(&s),
            _ => false,
        };
        let cr = |c: &Option<i64>| c.map_or("None".into(), |v| v.to_string());
        if !ok {
            log::info!(target: "meter", "Interlink screen: star caps not clear enough [{}] (OCR {})", caps.iter().map(cr).collect::<Vec<_>>().join(", "), str_map_repr(&t));
            return;
        }
        let (side, center) = (side.unwrap(), center.unwrap());
        let was = star_caps(&self.env.data);
        if was != (side, center) {
            log::info!(target: "meter", "Interlink screen: star caps {side} / {center} / {side} (were {} / {})", was.0, was.1);
            let v = dict([("side", Value::Int(side)), ("center", Value::Int(center)),
                ("cycle", self.env.schedule.active_cycle_id(now).map_or(Value::None, Value::Str)),
                ("seen", Value::Str(crate::results::local(now, "%Y-%m-%d %H:%M:%S")))]);
            let _ = crate::submit::write_json(&self.env.data.join("logs").join("star_caps.json"), &v, None);
        }
    }

    fn slots(&self) -> &'static [BoxF] {
        if self.ps.egg_party { &EGG_PARTY_SLOTS } else { &PARTY_SLOTS }
    }

    fn cards(&self) -> &'static [BoxF] {
        if self.ps.egg_party { &EGG_PARTY_CARDS } else { &PARTY_CARDS }
    }

    /// The Party screen's name pills, prepared ({'n0'..}: box).
    pub fn party_names(&self, im: &Image, u: f64, ux: f64, h: f64) -> IndexMap<String, Image> {
        self.slots().iter().enumerate().map(|(i, s)| (format!("n{i}"), prep::prep_bright(im, (ux + u * s.0, h * s.1, ux + u * s.2, h * s.3), u / 2000.0, 2.0))).collect()
    }

    /// Which Party screen this is, by its top line. A line that didn't read leaves it as it was.
    pub fn party_layout(&mut self, top_line: &str) {
        let line = norm(top_line);
        if !line.is_empty() {
            self.ps.egg_party = line.contains("heist") || line.contains("operation");
        }
    }

    /// Each player in a slot of their own.
    fn own_slots(&self, mut members: Vec<Dict>, now: f64) -> Vec<Dict> {
        let recent: Vec<Dict> = match &self.party {
            Some(p) if now - p.get("at").as_f64().unwrap_or(0.0) < 60.0 && self.lobby_change(now).is_none() => p.get("members").as_list().map(|l| l.iter().filter_map(|m| m.as_dict().cloned()).collect()).unwrap_or_default(),
            _ => vec![],
        };
        for m in &mut members {
            if !m.at("cp").truthy() {
                continue;
            }
            let was: Vec<&Dict> = recent.iter().filter(|p| p.at("cp").py_eq(m.at("cp"))).collect();
            if was.len() == 1 {
                m.insert("slot".into(), was[0].at("slot").clone());
            }
        }
        let slot = |m: &Dict| m.at("slot").as_i64().unwrap_or(0);
        let mut order: Vec<usize> = (0..members.len()).collect();
        order.sort_by_key(|&i| !members[i].at("pill").py_eq(members[i].at("slot")));
        let mut kept: IndexMap<i64, usize> = IndexMap::new();
        for i in order {
            kept.entry(slot(&members[i])).or_insert(i);
        }
        for i in 0..members.len() {
            if kept.values().any(|&j| j == i) {
                continue;
            }
            let free: Vec<i64> = (0..self.slots().len() as i64).filter(|s| !kept.contains_key(s)).collect();
            if free.is_empty() {
                continue;
            }
            let pill = members[i].at("pill").as_i64();
            let s = pill.filter(|p| free.contains(p)).unwrap_or(free[0]);
            members[i].insert("slot".into(), Value::Int(s));
            kept.insert(s, i);
        }
        kept.values().map(|&i| members[i].clone()).collect()
    }

    /// Interlink's Party screen: each slot's name, and who is who (a player's card shows CP, an AI teammate's doesn't).
    pub fn party_screen(&mut self, im: &Image, now: f64, u: f64, ux: f64, h: f64) {
        if now - self.ps.party_seen < 2.0 {
            return;
        }
        self.ps.party_seen = now;
        self.keep_sample(im, "party", None, None);
        let k = u / 2000.0;
        let bx = |s: &BoxF| (ux + u * s.0, h * s.1, ux + u * s.2, h * s.3);
        let mut crops = self.party_names(im, u, ux, h);
        for (i, s) in self.cards().iter().enumerate() {
            crops.insert(format!("a{i}"), prep::prep_grey(im, bx(s), k, 3.0, 24));
        }
        crops.insert("boss".into(), prep::prep_text(im, bx(&(0.45, 0.005, 0.62, 0.04)), k, 190, 3.0, 24));
        let sizes: IndexMap<String, usize> = crops.keys().filter(|k| k.len() == 2 && k.starts_with('n')).map(|k| (k.clone(), 20)).collect();
        let mut t = self.ocr.read(&crops, &sizes, &IndexMap::new());
        let g = |t: &IndexMap<String, String>, k: &str| t.get(k).cloned().unwrap_or_default();
        if !(0..self.cards().len()).any(|i| !g(&t, &format!("a{i}")).trim().is_empty()) {
            let pills: Vec<Value> = (0..self.slots().len()).map(|i| t.get(&format!("n{i}")).map_or(Value::None, |s| Value::Str(s.clone()))).collect();
            log::info!(target: "meter", "Party screen: no player's card reads, not read as a party (pills {})", alogs_pyval::str_repr(&Value::List(pills).repr()));
            return;
        }
        for (i, spot) in self.cards().iter().enumerate() {
            let card = g(&t, &format!("a{i}"));
            let Some(cp) = CP.captures(&card).map(|c| c[1].to_string()) else { continue };
            if CARD_LEAD.is_match(&card) {
                continue;
            }
            let mut c = IndexMap::new();
            c.insert("again".to_string(), prep::prep_grey(im, bx(&(spot.0, spot.1 + PANEL_RAISED, spot.2, spot.3)), k, 3.0, 24));
            let again = self.ocr.read(&c, &IndexMap::new(), &IndexMap::new()).get("again").cloned().unwrap_or_default();
            if CARD_LEAD.is_match(&again) && CP.captures(&again).is_some_and(|c2| c2[1] == cp) {
                t.insert(format!("a{i}"), again);
            }
        }
        let known = self.character_name.clone();
        let yellow = |i: usize| -> bool {
            let Some(s) = self.slots().get(i) else { return false };
            let b = bx(s);
            let pxs: Vec<[u8; 3]> = ((b.0 as i64)..(b.2 as i64)).step_by(3).flat_map(|x| ((b.1 as i64)..(b.3 as i64)).step_by(3).map(move |y| (x, y))).map(|(x, y)| px(im, x, y)).collect();
            pxs.iter().filter(|p| p[0] >= 0xc8 && p[1] >= 0xa8 && i32::from(p[0]) - i32::from(p[2]) >= 0x50).count() as f64 > 0.025 * pxs.len() as f64
        };
        let old_members: Vec<Dict> = self.party.as_ref().and_then(|p| p.get("members").as_list().cloned()).unwrap_or_default().iter().filter_map(|m| m.as_dict().cloned()).collect();
        let fold = |v: &Value| crate::team::casefold(&(if v.is_none() { String::new() } else { v.py_str() }).chars().filter(|c| c.is_alphanumeric() || *c == '_').collect::<String>());
        // your own pill is drawn in yellow: on Interlink's Party screen only when exactly one is (measured on 13 4K
        // Party pictures, 22 Nov: your pill 3.6-5.3% yellow at this test, every other pill 1.4% at most, one picture
        // with none over 2.5%)
        let yellows: Vec<usize> = (0..self.slots().len()).filter(|&i| yellow(i)).collect();
        let own_pill = |i: usize| if self.ps.egg_party { yellows.contains(&i) } else { yellows == [i] };
        let mut members = vec![];
        for i in 0..4usize {
            let card = g(&t, &format!("a{i}"));
            if self.ps.egg_party && card.trim().is_empty() {
                continue;
            }
            let mut got = party_pill(&g(&t, &format!("n{i}")), i as i64);
            if got.is_none() && known.is_some() && i < self.slots().len() && own_pill(i) {
                got = Some((i as i64, known.clone().unwrap()));
            }
            // Interlink: a seat whose card was read (its lead or its CP) is kept when its name can't be read, without a
            // name: the site places a run's Party data by seat. Its seat is its place on the screen (PARTY_SLOTS are
            // 1P-4P in order, standing 3P 1P 2P 4P left to right).
            if got.is_none() && !self.ps.egg_party && i < self.slots().len() && (CARD_LEAD.is_match(&card) || CP.is_match(&card)) {
                got = Some((i as i64, String::new()));
            }
            let Some((slot, mut name)) = got else { continue };
            let pet = CARD_LEAD.captures(&card).map(|c| c[1].trim().to_string());
            let lv = LV.captures(&card).and_then(|c| c[1].parse::<i64>().ok());
            let cp = CP.captures(&card).and_then(|c| c[1].parse::<i64>().ok());
            let mine = known.as_deref().is_some_and(|k| same_name(Some(k), Some(&name))) || (known.is_none() && yellow(i));
            if mine && let Some(k) = &known {
                name = k.clone();
            }
            let nick = pet.clone();
            let empty = Dict::new();
            let old = old_members.iter().find(|m| m.at("slot").as_i64() == Some(slot)
                && ((cp.is_some() && m.at("cp").as_i64() == cp)
                    || (nick.as_ref().is_some_and(|n| fold(m.at("nickname")) == fold(&Value::Str(n.clone()))))
                    || (!(cp.is_some() && m.at("cp").truthy()) && m.at("name").as_str() == Some(name.as_str())))).unwrap_or(&empty);
            // a bot has a bot's name and no CP on its card (see bot_sign); a card that didn't read leaves the name as
            // this look's guess, for the looks that did to decide
            // (no name, no sign: a seat without its name is neither told a bot nor a player by its looks)
            let sign = if mine { Some(false) } else if name.is_empty() { None } else { bot_sign(&self.bot_names, &name, !card.trim().is_empty(), cp) };
            let ai = sign.unwrap_or_else(|| is_bot_name(&self.bot_names, &name));
            let m = dict([
                ("slot", Value::Int(slot)), ("name", Value::Str(name)), ("self", Value::Bool(mine)), ("pill", Value::Int(i as i64)), ("ai", Value::Bool(ai)),
                ("ai_sign", sign.map_or(Value::None, Value::Bool)),
                ("aniimo", md(old, "aniimo")),
                ("nickname", nick.map_or_else(|| md(old, "nickname"), Value::Str)),
                ("aniimo_level", lv.map_or_else(|| md(old, "aniimo_level"), Value::Int)),
                ("cp", cp.map_or_else(|| md(old, "cp"), Value::Int)),
            ]);
            members.push(m.as_dict().cloned().unwrap());
        }
        let bt = g(&t, "boss");
        let boss = self.cycle_boss(bt.trim_matches(|c| c == ' ' || c == '/' || c == '|'));
        let row = if boss.is_empty() { None } else { self.stage_first(self.cycle_row(&boss)) };
        if let Some(r) = row {
            self.prep_boss = Some((now, r));
        }
        log::info!(target: "meter", "Party screen: OCR {} -> boss {}", str_map_repr(&t), if boss.is_empty() { "?" } else { &boss });
        let mut members = self.own_slots(members, now);
        members.sort_by_key(|m| m.at("slot").as_i64().unwrap_or(0));
        self.set_party(members.clone(), now);
        if let Some(Value::Dict(p)) = &mut self.party {
            p.insert("egg".into(), Value::Bool(self.ps.egg_party));
        }
        if self.party.is_some() {
            self.party_picture = Some(im.clone());
        }
        let cards: Vec<(i64, usize)> = members.iter().map(|m| (m.at("slot").as_i64().unwrap_or(0), m.at("pill").as_i64().unwrap_or(0) as usize)).collect();
        self.read_party_icons(im, &cards, u, ux, h);
    }

    /// Why the Party screen seen now is another lobby than the one its votes were cast in, if it is: the game log's
    /// scene changed, a fight started or a result was recorded since, or the screen was away for longer than it is
    /// within one lobby.
    pub fn lobby_change(&self, now: f64) -> Option<String> {
        let l = self.ps.lobby.as_ref()?;
        if l.space != self.space {
            Some("the game log's scene changed".into())
        } else if l.runs != self.ps.runs {
            Some("a run since".into())
        } else if now - l.at > LOBBY_AWAY_S {
            Some(format!("the Party screen away for {:.0} s", now - l.at))
        } else {
            None
        }
    }

    /// This run's party, each slot's name voted over a few reads.
    pub fn set_party(&mut self, members: Vec<Dict>, now: f64) {
        self.screen_settled("party", now);
        if members.is_empty() {
            return;
        }
        let mut members = members;
        // every seat's memory belongs to one lobby
        let fresh = self.lobby_change(now);
        if let Some(why) = &fresh {
            log::info!(target: "meter", "party: another lobby ({why}): the seats are voted again");
            self.ps.votes.clear();
            self.ps.counted.clear();
            self.ps.missed.clear();
            self.ps.ai_signs.clear();
            self.ps.unlike.clear();
        }
        self.ps.lobby = Some(Lobby { space: self.space.clone(), runs: self.ps.runs, at: now });
        let party = self.party.clone().and_then(|p| p.as_dict().cloned());
        let at = party.as_ref().map_or(f64::MIN, |p| p.at("at").as_f64().unwrap_or(0.0));
        let slot = |m: &Dict| m.at("slot").as_i64().unwrap_or(0);
        let old: Vec<Dict> = party.as_ref().and_then(|p| p.at("members").as_list().cloned()).unwrap_or_default().iter().filter_map(|m| m.as_dict().cloned()).collect();
        let mut missed_now = BTreeSet::new();
        if party.is_some() && now - at < 60.0 && fresh.is_none() {
            let got: BTreeSet<i64> = members.iter().map(slot).collect();
            let mut keep: Vec<Dict> = old.iter().filter(|m| !got.contains(&slot(m)) && !self.ps.missed.contains(&slot(m))).cloned().collect();
            for m in &mut keep {
                m.insert("kept".into(), Value::Bool(true));
            }
            if !keep.is_empty() {
                members.extend(keep);
                members.sort_by_key(slot);
            }
            missed_now = old.iter().map(slot).filter(|s| !got.contains(s)).collect();
        }
        let same = fresh.is_none() && party.is_some() && now - at < 900.0 && old.iter().map(slot).collect::<Vec<_>>() == members.iter().map(slot).collect::<Vec<_>>();
        if !same {
            self.ps.votes.clear();
        }
        for m in &members {
            let (s, name) = (slot(m), m.at("name").py_str());
            if !m.at("kept").truthy() {
                self.new_player(s, &name);
            }
            // (a seat read without its name casts no vote: its name is the one its other looks read, if any)
            let seat = self.ps.votes.entry(s).or_default();
            if !name.is_empty() {
                *seat.entry(name).or_insert(0) += 1;
            }
        }
        for m in &mut members {
            let n = self.ps.votes.get(&slot(m)).map(most_common).unwrap_or_default();
            m.insert("name".into(), Value::Str(n));
        }
        if same {
            for m in &mut members {
                if !m.at("self").truthy()
                    && let Some(w) = old.iter().find(|w| slot(w) == slot(m))
                    && w.at("self").truthy()
                    && w.at("name").py_eq(m.at("name"))
                {
                    m.insert("self".into(), Value::Bool(true));
                    m.insert("ai".into(), Value::Bool(false));
                }
            }
        }
        let recent = party.is_some() && now - at < 900.0;
        if !recent || fresh.is_some() {
            self.ps.ai_signs.clear();
        }
        // bot or player, voted over this Party screen's looks: each look's sign (bot_sign) counts, the most win; the
        // looks that couldn't tell (a card that didn't read) and the battle prep's mark only break a tie. Never a bot
        // without a bot's name.
        for m in &mut members {
            let sign = m.pop("ai_sign");
            if m.at("kept").truthy() {
                continue;
            }
            let name = m.at("name").py_str();
            let tally = self.ps.ai_signs.entry((slot(m), Teammates::fold(&name))).or_insert([0, 0]);
            match sign {
                Some(Value::Bool(true)) => tally[0] += 1,
                Some(Value::Bool(false)) => tally[1] += 1,
                // (a member made without a sign: its flag is its sign)
                None => tally[if m.at("ai").truthy() { 0 } else { 1 }] += 1,
                _ => {}
            }
            let t = *tally;
            let bot = is_bot_name(&self.bot_names, &name);
            let ai = !m.at("self").truthy() && bot && (t[0] > t[1] || (t[0] == t[1] && m.at("ai").truthy()));
            m.insert("ai".into(), Value::Bool(ai));
        }
        // one player in one slot only
        let mut groups: IndexMap<String, Vec<usize>> = IndexMap::new();
        for (i, m) in members.iter().enumerate() {
            if m.at("name").truthy() {
                groups.entry(Teammates::fold(&m.at("name").py_str())).or_default().push(i);
            }
        }
        let mut drop = BTreeSet::new();
        for g in groups.values().filter(|g| g.len() > 1) {
            let score = |i: usize| {
                let m = &members[i];
                let pill = m.get_str("pill").cloned().unwrap_or_else(|| m.at("slot").clone());
                (pill.py_eq(m.at("slot")), m.at("self").truthy(), !m.at("kept").truthy(),
                 self.ps.votes.get(&slot(m)).and_then(|v| v.get(&m.at("name").py_str())).copied().unwrap_or(0))
            };
            let mut best = g[0];
            for &i in &g[1..] {
                if score(i) > score(best) {
                    best = i;
                }
            }
            for &i in g {
                if i == best {
                    continue;
                }
                let kept = members[i].at("kept").truthy();
                log::info!(target: "meter", "party: {} in slot {} too, kept in slot {}: slot {} left {}", alogs_pyval::str_repr(&members[i].at("name").py_str()),
                    slot(&members[i]) + 1, slot(&members[best]) + 1, slot(&members[i]) + 1, if kept { "out" } else { "blank" });
                if kept {
                    drop.insert(i);
                } else {
                    members[i].insert("name".into(), Value::Str(String::new()));
                    members[i].insert("ai".into(), Value::Bool(false));
                    members[i].insert("self".into(), Value::Bool(false));
                }
            }
        }
        let mut members: Vec<Dict> = members.into_iter().enumerate().filter(|(i, _)| !drop.contains(i)).map(|(_, mut m)| { m.pop("kept"); m }).collect();
        if !same {
            self.ps.counted.clear();
        }
        for m in &mut members {
            if m.at("self").truthy() || m.at("ai").truthy() || !m.at("name").truthy() {
                continue;
            }
            let name = m.at("name").py_str();
            let clear = self.ps.votes[&slot(m)].get(&name).copied().unwrap_or(0) >= 3;
            let steady = self.team.steady(&name);
            m.insert("name".into(), Value::Str(steady.clone()));
            if clear && !self.ps.counted.contains(&slot(m)) {
                self.team.saw(&steady);
                self.ps.counted.insert(slot(m));
            }
        }
        let triple = |ms: &[Dict]| ms.iter().map(|m| (m.at("name").repr(), m.at("ai").truthy(), m.at("self").truthy())).collect::<Vec<_>>();
        let changed = !same || triple(&members) != triple(&old);
        let lead = |ms: &[Dict]| ms.iter().find(|m| slot(m) == 0).map(|m| who(m).0);
        let keep_buffs = recent && lead(&members).is_some() && lead(&members) == lead(&old);
        let mut np = Dict::new();
        np.insert("at".into(), Value::Float(now));
        np.insert("results".into(), Value::Int(self.ps.results as i64));
        np.insert("members".into(), Value::List(members.iter().cloned().map(Value::Dict).collect()));
        if let Some(p) = &party
            && (same || keep_buffs)
        {
            for k in ["buffs", "buff_reads"] {
                if let Some(v) = p.get_str(k) {
                    np.insert(k.into(), v.clone());
                }
            }
        }
        if let Some(p) = &party
            && recent
        {
            let whos: BTreeMap<i64, (String, String)> = members.iter().map(|m| (slot(m), who(m))).collect();
            let mut had = Dict::new();
            if let Some(tw) = p.at("team_who").as_dict() {
                for (s, w) in tw.iter() {
                    let s_i = s.py_str().parse::<i64>().unwrap_or(-1);
                    let pair = w.as_list().map(|l| (l.first().map_or(String::new(), Value::py_str), l.get(1).map_or(String::new(), Value::py_str)));
                    if whos.get(&s_i).is_some_and(|x| Some(x) == pair.as_ref()) {
                        had.insert(s.clone(), w.clone());
                    }
                }
            }
            let old_teams = p.at("teams").as_list().cloned().unwrap_or_else(|| vec![Value::None; 4]);
            let teams: Vec<Value> = (0..4i64).map(|s| if had.get_str(&s.to_string()).is_some() { old_teams.get(s as usize).cloned().unwrap_or(Value::None) } else { Value::None }).collect();
            if let Some(mine) = members.iter().find(|m| m.at("self").truthy()).map(slot)
                && teams.get(mine as usize).is_some_and(Value::truthy)
            {
                np.insert("team".into(), teams[mine as usize].clone());
            }
            np.insert("teams".into(), Value::List(teams));
            np.insert("team_who".into(), Value::Dict(had));
        }
        self.ps.missed = missed_now;
        self.party = Some(Value::Dict(np));
        if changed {
            let line: Vec<String> = members.iter().map(|m| {
                let mut s = m.at("name").py_str();
                if m.at("ai").truthy() { s += " (AI)"; }
                if m.at("self").truthy() { s += " (you)"; }
                if m.at("aniimo").truthy() || m.at("nickname").truthy() {
                    let nick = m.at("nickname");
                    let as_ = if nick.truthy() && !nick.py_eq(m.at("aniimo")) { format!(" as {}", nick.repr()) } else { String::new() };
                    let cp = if m.at("cp").truthy() { format!(", CP {}", m.at("cp").py_str()) } else { String::new() };
                    s += &format!(" [{}{as_} Lv {}{cp}]", if m.at("aniimo").truthy() { m.at("aniimo").py_str() } else { "?".into() }, m.at("aniimo_level").py_str());
                }
                s
            }).collect();
            log::info!(target: "meter", "party: {}", line.join(", "));
        }
    }

    /// Within one lobby a player can leave and another take the seat: a look whose name for `seat` is unlike the
    /// seat's leading name (names_alike), twice in a row and alike each other, drops the seat's votes for the new
    /// name. One odd look (a misread, a banner over the name) leaves a settled name where it is.
    fn new_player(&mut self, seat: i64, name: &str) {
        let leader = self.ps.votes.get(&seat).filter(|v| !v.is_empty()).map(most_common);
        let Some(leader) = leader.filter(|l| !name.is_empty() && !names_alike(l, name)) else {
            self.ps.unlike.remove(&seat);
            return;
        };
        let streak = self.ps.unlike.entry(seat).or_default();
        if streak.last().is_some_and(|p| !names_alike(p, name)) {
            streak.clear();
        }
        streak.push(name.to_string());
        if streak.len() < 2 {
            return;
        }
        let new: Vec<String> = std::mem::take(streak);
        log::info!(target: "meter", "party: slot {} reads {:?} twice in a row, not its {:?}: a new player in that seat", seat + 1, name, leader);
        let mut votes = IndexMap::new();
        for n in &new[..new.len() - 1] {
            *votes.entry(n.clone()).or_insert(0) += 1;
        }
        self.ps.votes.insert(seat, votes);
        self.ps.counted.remove(&seat);
    }
}
