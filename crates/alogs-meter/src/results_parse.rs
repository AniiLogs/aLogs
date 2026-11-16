//! Reading end screens' text into values: labels, Holo-Battle Sim's stage line and team, Battle Data cards, the
//! votes over repeated reads, and matching result-screen columns to the Party screen. Port of the module-level
//! functions of aLogs B16 `bossbar/results.py`.

use crate::team::{Teammates, casefold};
use alogs_gamelog::difflib::ratio;
use alogs_gamelog::same_name;
use alogs_pil::{Filter, Image, Mode};
use alogs_pyval::{Dict, Value};
use indexmap::IndexMap;
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

pub const RESULT_STATS: [&str; 8] = [
    "Total Damage Dealt", "Total BREAK Damage", "Targets Defeated", "Most Healing", "Most Damage Taken", "Most Hits Taken",
    "Most Support Provided", "Best Strike Damage",
];
pub const EGG_MODES: [&str; 2] = ["Egg Heist Team Mode", "Egg Heist"];
pub const INTERLINK_NPCS: [&str; 4] = ["flora", "mario", "leonard", "tammy"];
pub const BATTLE_DATA_WAIT: f64 = 45.0;
pub const BATTLE_DATA_SETTLE: f64 = 6.0;
pub const SCREEN_READS: i64 = 10;
pub const SETTLE_S: f64 = 0.25;
pub const SCREEN_MOVED_ON: i64 = 240;

fn letters(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_lowercase()).collect()
}

/// The game label `text` most likely is, or `text` as read if none is close.
pub fn canon(text: &str, choices: &[&str], cutoff: f64) -> String {
    let t = letters(text);
    if t.is_empty() {
        return text.to_string();
    }
    let mut best: Option<(f64, &str)> = None;
    for c in choices {
        let r = ratio(&t, &letters(c));
        if best.is_none_or(|b| r > b.0) {
            best = Some((r, c));
        }
    }
    let (r, b) = best.unwrap();
    if r >= cutoff { b.to_string() } else { text.to_string() }
}

/// The number most tiles of an OCR read agree on ("60 60 60" -> 60).
pub fn first_num(s: Option<&str>) -> Option<i64> {
    let t = s.unwrap_or("").replace('O', "0");
    let nums: Vec<&str> = Regex::new(r"\d+").unwrap().find_iter(&t).map(|m| m.as_str()).collect();
    let mut best: Option<(usize, i64, &str)> = None;
    for (i, n) in nums.iter().enumerate() {
        let c = nums.iter().filter(|x| *x == n).count();
        let first = nums.iter().position(|x| x == n).unwrap();
        let key = (c, -(first as i64));
        if best.is_none_or(|b| key > (b.0, b.1)) {
            best = Some((key.0, key.1, n));
        }
        let _ = i;
    }
    best.and_then(|b| b.2.parse().ok())
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// Holo-Battle Sim's Victory line -> {time_s, trial, element, difficulty}, or None unless time and stage both read.
pub fn parse_sim(text: Option<&str>) -> Option<Value> {
    static T: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,2})\s*[:.]\s*(\d{2})\s*[:.]\s*(\d{2})").unwrap());
    let text = text.unwrap_or("");
    let t = T.captures(text)?;
    let (h, m, s): (i64, i64, i64) = (t[1].parse().ok()?, t[2].parse().ok()?, t[3].parse().ok()?);
    if m > 59 || s > 59 {
        return None;
    }
    let rest = &text[t.get(0).unwrap().end()..];
    let (trial, element, difficulty) = if let Some((kind, element, diff)) = sim_stage_of(rest) {
        let (k, e, d) = (cap(&kind), cap(&element), cap(&diff));
        (format!("{k} of {e}"), e, d)
    } else {
        let (element, kind, diff) = sim_stage(rest)?;
        if ["time", "battle"].contains(&element.to_lowercase().as_str()) {
            return None;
        }
        let (e, k, d) = (cap(&element), cap(&kind), cap(&diff));
        (format!("{e} {k}"), e, d)
    };
    Some(alogs_pyval::dict([
        ("time_s", Value::Int(h * 3600 + m * 60 + s)),
        ("trial", Value::Str(trial)),
        ("element", Value::Str(element)),
        ("difficulty", Value::Str(difficulty)),
    ]))
}

const DIFFS: [&str; 6] = ["easy", "common", "normal", "hard", "challenge", "trial"];

/// `SIM_DIFF` at `i` of `cs` (case-insensitive, not followed by two letters): its length.
fn diff_at(cs: &[char], i: usize) -> Option<usize> {
    for d in DIFFS {
        let n = d.len();
        if i + n <= cs.len() && cs[i..i + n].iter().collect::<String>().to_lowercase() == d {
            let two = i + n + 1 < cs.len() && cs[i + n].is_ascii_alphabetic() && cs[i + n + 1].is_ascii_alphabetic();
            if !two {
                return Some(n);
            }
        }
    }
    None
}

fn word_end(cs: &[char], i: usize) -> bool {
    // \b before: start, or a non-word char before
    i == 0 || !(cs[i - 1].is_alphanumeric() || cs[i - 1] == '_')
}

/// `(Training|Trial|Crucible)\s*of\s*([A-Za-z]{3,12}).{0,6}?SIM_DIFF`, case-insensitive, first match.
fn sim_stage_of(rest: &str) -> Option<(String, String, String)> {
    let cs: Vec<char> = rest.chars().collect();
    for i in 0..cs.len() {
        if !word_end(&cs, i) {
            continue;
        }
        for kind in ["training", "trial", "crucible"] {
            let n = kind.len();
            if i + n > cs.len() || cs[i..i + n].iter().collect::<String>().to_lowercase() != kind {
                continue;
            }
            let mut j = i + n;
            while j < cs.len() && cs[j].is_whitespace() {
                j += 1;
            }
            if !(j + 2 <= cs.len() && cs[j..j + 2].iter().collect::<String>().to_lowercase() == "of") {
                continue;
            }
            j += 2;
            while j < cs.len() && cs[j].is_whitespace() {
                j += 1;
            }
            // element: greedy 12 down to 3 letters, then .{0,6}? lazily, then a difficulty
            let mut max_e = 0;
            while j + max_e < cs.len() && max_e < 12 && cs[j + max_e].is_ascii_alphabetic() {
                max_e += 1;
            }
            for e in (3..=max_e).rev() {
                for gap in 0..=6 {
                    let at = j + e + gap;
                    if at > cs.len() || (gap > 0 && cs[j + e + gap - 1] == '\n') {
                        break;
                    }
                    if let Some(dn) = diff_at(&cs, at) {
                        return Some((cs[i..i + n].iter().collect(), cs[j..j + e].iter().collect(), cs[at..at + dn].iter().collect()));
                    }
                }
            }
        }
    }
    None
}

/// `\b([A-Za-z]{3,12})\s*(Training|Trial).{0,6}?SIM_DIFF`, case-insensitive, first match.
fn sim_stage(rest: &str) -> Option<(String, String, String)> {
    let cs: Vec<char> = rest.chars().collect();
    for i in 0..cs.len() {
        if !word_end(&cs, i) || !cs[i].is_ascii_alphabetic() {
            continue;
        }
        let mut max_e = 0;
        while i + max_e < cs.len() && max_e < 12 && cs[i + max_e].is_ascii_alphabetic() {
            max_e += 1;
        }
        for e in (3..=max_e).rev() {
            let mut j = i + e;
            while j < cs.len() && cs[j].is_whitespace() {
                j += 1;
            }
            for kind in ["training", "trial"] {
                let n = kind.len();
                if j + n > cs.len() || cs[j..j + n].iter().collect::<String>().to_lowercase() != kind {
                    continue;
                }
                for gap in 0..=6 {
                    let at = j + n + gap;
                    if at > cs.len() || (gap > 0 && cs[j + n + gap - 1] == '\n') {
                        break;
                    }
                    if let Some(dn) = diff_at(&cs, at) {
                        return Some((cs[i..i + e].iter().collect(), cs[j..j + n].iter().collect(), cs[at..at + dn].iter().collect()));
                    }
                }
            }
        }
    }
    None
}

pub const SIM_CARDS: [(f64, f64, f64, f64); 5] =
    [(0.135, 0.385, 0.26, 0.45), (0.13, 0.555, 0.27, 0.61), (0.268, 0.575, 0.385, 0.63), (0.13, 0.745, 0.25, 0.80), (0.248, 0.745, 0.37, 0.80)];
pub const SIM_SHARES: (f64, f64, f64, f64) = (0.352, 0.30, 0.40, 0.40);
pub const SIM_COINS: (f64, f64, f64, f64) = (0.795, 0.285, 0.84, 0.325);
pub const SIM_GIZMO_X: f64 = 0.464;
pub const SIM_GIZMO_PITCH: f64 = 0.0588;
pub const SIM_GIZMO_Y: (f64, f64) = (0.5795, 0.5985);
pub const SIM_GIZMOS: usize = 7;
pub const SIM_LOW: f64 = 0.0417;
pub const SIM_CARD_SCORE: f64 = 0.6;
pub const SIM_CARD_MARGIN: f64 = 0.08;
#[allow(clippy::type_complexity)]
pub const SIM_CARD_ART: [((f64, f64), (f64, f64, f64, f64), (f64, f64), (f64, f64)); 5] = [
    ((-1083.0, 384.0), (-520.0, -120.0, -40.0, 140.0), (-394.0, -193.0), (1024.0, 1680.0)),
    ((-1411.0, -111.0), (-246.0, 246.0, -85.0, 198.0), (-3.0, -51.0), (710.0, 1165.0)),
    ((-803.0, -111.0), (-232.0, 232.0, -102.0, 239.0), (-28.0, 24.0), (710.0, 1165.0)),
    ((-1455.0, -571.0), (-216.0, 216.0, -94.0, 218.0), (-38.0, -138.0), (710.0, 1165.0)),
    ((-888.0, -595.0), (-235.0, 235.0, -84.0, 195.0), (19.0, -180.0), (710.0, 1165.0)),
];

type BoxF = (f64, f64, f64, f64);

/// Each team card's (window, art rect) in screen px for `Icons::card`.
pub fn sim_card_boxes(u: f64, ux: f64, h: f64) -> Vec<(BoxF, BoxF)> {
    let (k, cx, cy) = (u / 4800.0, ux + u / 2.0, h / 2.0);
    SIM_CARD_ART
        .iter()
        .map(|&((wx, wy), (x0, x1, y0, y1), (ax, ay), (aw, ah))| {
            let window = (cx + (wx + x0) * k, cy - (wy + y1) * k, cx + (wx + x1) * k, cy - (wy + y0) * k);
            let art = (cx + (wx + ax - aw / 2.0) * k, cy - (wy + ay + ah / 2.0) * k, aw * k, ah * k);
            (window, art)
        })
        .collect()
}

/// The digits on the Holo-gizmo badges pasted in one line after a drawn label ("n: / 6 / 6 / 2 ...").
pub fn sim_badges(im: &Image, u: f64, ux: f64, h: f64, dy: f64) -> Image {
    let k = u / 2000.0;
    let mut out = Image::new(Mode::L, 140 + SIM_GIZMOS * 110, 120, &[255]);
    if let Some(f) = alogs_reader::text::Font::arial_bold() {
        f.draw(&mut out, 10.0, 25.0, "n:", 60.0, 0);
    }
    for i in 0..SIM_GIZMOS {
        let cx = SIM_GIZMO_X + SIM_GIZMO_PITCH * i as f64;
        let c = im
            .crop_i((ux + u * (cx - 0.0075)) as i64, (h * (SIM_GIZMO_Y.0 + dy)) as i64, (ux + u * (cx + 0.0075)) as i64, (h * (SIM_GIZMO_Y.1 + dy)) as i64)
            .convert(Mode::L);
        let c = c
            .resize(((c.width as f64 * 4.0 / k) as usize).max(1), ((c.height as f64 * 4.0 / k) as usize).max(1), Some(Filter::Lanczos))
            .point(|v| if v < 120 { 0.0 } else { 255.0 });
        let c = c.thumbnail(90, 90);
        out.paste(&c, 120 + i as i64 * 110, 15);
    }
    out
}

/// The Holo-gizmo row's counts, each badge read on its own against the game's digits.
pub fn sim_badge_counts(im: &Image, u: f64, ux: f64, h: f64, reader: &alogs_reader::names::NameReader, dy: f64, n: usize) -> Vec<Option<i64>> {
    (0..n)
        .map(|i| {
            let cx = SIM_GIZMO_X + SIM_GIZMO_PITCH * i as f64;
            let b = (ux + u * (cx - 0.0075), h * (SIM_GIZMO_Y.0 + dy), ux + u * (cx + 0.0075), h * (SIM_GIZMO_Y.1 + dy));
            reader.number(im, b, 0.05, true, false, false).filter(|v| 0 < *v && *v < 100)
        })
        .collect()
}

/// The Victory screen's team, shares, coins and gizmo counts, prepared for one OCR call.
pub fn sim_team_crops(im: &Image, u: f64, ux: f64, h: f64) -> IndexMap<String, Image> {
    use alogs_reader::prep::{prep_bright, prep_white};
    let k = u / 2000.0;
    let b = |x0: f64, y0: f64, x1: f64, y1: f64| (ux + u * x0, h * y0, ux + u * x1, h * y1);
    let mut crops = IndexMap::new();
    for (i, bx) in SIM_CARDS.iter().enumerate() {
        crops.insert(format!("c{i}w"), prep_white(im, b(bx.0, bx.1, bx.2, bx.3), k, 4.0));
        crops.insert(format!("c{i}b"), prep_bright(im, b(bx.0, bx.1, bx.2, bx.3), k, 2.0));
    }
    crops.insert("share".into(), prep_white(im, b(SIM_SHARES.0, SIM_SHARES.1, SIM_SHARES.2, SIM_SHARES.3), k, 4.0));
    crops.insert("coins".into(), prep_bright(im, b(SIM_COINS.0, SIM_COINS.1, SIM_COINS.2, SIM_COINS.3), k, 2.0));
    crops.insert("gizmos".into(), sim_badges(im, u, ux, h, 0.0));
    let (x0, y0, x1, y1) = SIM_COINS;
    crops.insert("coinsL".into(), prep_bright(im, b(x0, y0 + SIM_LOW, x1, y1 + SIM_LOW), k, 2.0));
    crops.insert("gizmosL".into(), sim_badges(im, u, ux, h, SIM_LOW));
    crops
}

/// What `sim_team_crops` read -> {team, mvpShares, gizmoCounts, coins, low}.
pub fn parse_sim_team(t: &IndexMap<String, String>, names: &[String], species: &[Option<String>]) -> Value {
    static LV: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"L\W?v\W*(\d{1,3})\W+(\S.{1,24})").unwrap());
    static PLAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z][a-z]+(?:[ '\-][A-Z][a-z]+)?$").unwrap());
    let norm = |x: &str| letters(x);
    let get = |k: &str| t.get(k).map_or("", String::as_str);
    let mut team = Vec::new();
    for i in 0..SIM_CARDS.len() {
        let mut reads: Vec<(i64, String)> = Vec::new();
        for v in [get(&format!("c{i}w")), get(&format!("c{i}b"))] {
            if let Some(m) = LV.captures(v) {
                let lv: i64 = m[1].parse().unwrap();
                if (1..=100).contains(&lv) {
                    reads.push((lv, m[2].trim().to_string()));
                }
            }
        }
        let art = species.get(i).cloned().flatten();
        if let Some(art) = art.filter(|a| !a.is_empty()) {
            let mut e = Dict::new();
            e.insert("name".into(), Value::Str(art.clone()));
            e.insert("level".into(), reads.first().map_or(Value::None, |r| Value::Int(r.0)));
            e.insert("mvp".into(), Value::Bool(i == 0));
            if !reads.is_empty() && reads.iter().all(|(_, r)| ratio(&norm(r), &norm(&art)) < 0.7) {
                e.insert("nickname".into(), Value::Str(reads[0].1.chars().take(24).collect()));
            }
            team.push(Value::Dict(e));
            continue;
        }
        if reads.is_empty() {
            continue;
        }
        let mut best: Option<(f64, &String)> = None;
        for (_, r) in &reads {
            for n in names {
                let s = ratio(&norm(r), &norm(n));
                if best.is_none_or(|b| (s, n) > (b.0, b.1)) {
                    best = Some((s, n));
                }
            }
        }
        let (score, name) = best.map_or((0.0, None), |b| (b.0, Some(b.1.clone())));
        let name = if score < 0.7 {
            if species.iter().any(|s| s.as_ref().is_some_and(|x| !x.is_empty())) {
                continue;
            }
            let plain: Vec<&String> = reads.iter().map(|r| &r.1).filter(|r| PLAIN.is_match(r)).collect();
            if plain.is_empty() {
                continue;
            }
            plain[0].clone()
        } else {
            name.unwrap()
        };
        team.push(alogs_pyval::dict([("name", Value::Str(name)), ("level", Value::Int(reads[0].0)), ("mvp", Value::Bool(i == 0))]));
    }
    let mut shares = Vec::new();
    let tail = Regex::new(r"\s*(%|[0oO]\s*/\s*[0oO]|[0oO]/[0oO])\s*$").unwrap();
    for part in get("share").split(" / ") {
        let p = tail.replace(part.trim(), "").into_owned();
        let n: Vec<&str> = Regex::new(r"\d{1,3}").unwrap().find_iter(&p).map(|m| m.as_str()).collect();
        if let Some(last) = n.last()
            && last.parse::<i64>().unwrap() <= 100
        {
            shares.push(last.parse::<i64>().unwrap());
        }
    }
    let right = |sfx: &str| -> (Vec<i64>, Option<i64>) {
        let coins: String = get(&format!("coins{sfx}")).chars().filter(|c| c.is_ascii_digit()).collect();
        let g = get(&format!("gizmos{sfx}"));
        let after = g.split_once(':').map_or(g, |x| x.1);
        let counts = Regex::new(r"\d{1,2}").unwrap().find_iter(after).map(|m| m.as_str().parse().unwrap()).take(SIM_GIZMOS).collect();
        (counts, if !coins.is_empty() && coins.len() <= 7 { coins.parse().ok() } else { None })
    };
    let ((mut counts, mut coins), (counts_l, coins_l)) = (right(""), right("L"));
    let low = (counts_l.len(), coins_l.is_some()) > (counts.len(), coins.is_some());
    if low {
        counts = counts_l;
        coins = coins_l;
    }
    alogs_pyval::dict([
        ("team", Value::List(team)),
        (
            "mvpShares",
            if shares.len() == 3 {
                alogs_pyval::dict([("damage", Value::Int(shares[0])), ("taken", Value::Int(shares[1])), ("healing", Value::Int(shares[2]))])
            } else {
                Value::None
            },
        ),
        ("gizmoCounts", Value::List(counts.into_iter().map(Value::Int).collect())),
        ("coins", coins.map_or(Value::None, Value::Int)),
        ("low", Value::Bool(low)),
    ])
}

const BD_KEYS: [&str; 4] = ["avg_dps", "break", "taken", "heal"];

/// Battle Data read whole: a card for every Party player, each with its average DPS and BREAK.
pub fn bd_complete(cards: &[Value], party: Option<&[Value]>) -> bool {
    !cards.is_empty() && cards.len() >= party.map_or(0, |p| p.len()) && cards.iter().all(|c| !c.get("avg_dps").is_none() && !c.get("break").is_none())
}

/// Battle Data cards from OCR text: each card's four values, in order.
pub fn parse_battle_data(text: Option<&str>) -> Vec<Value> {
    static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^/]*Average\s*D").unwrap());
    let label = |p: &str| -> Option<&'static str> {
        if p.contains("BREAK") {
            Some("break")
        } else if p.contains("Damage") {
            Some("taken")
        } else if p.contains("Heal") {
            Some("heal")
        } else {
            None
        }
    };
    let text = text.unwrap_or("");
    let segs: Vec<&str> = SPLIT.split(text).skip(1).collect();
    let mut cards: Vec<IndexMap<&'static str, Option<i64>>> = Vec::new();
    let tail_num = Regex::new(r"\s(\d+)$").unwrap();
    for seg in segs {
        let mut vals: Vec<i64> = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut spill: Option<Vec<(&'static str, Option<i64>)>> = None;
        for part in seg.split('/').map(str::trim) {
            let mut p = part.replace(',', "");
            if p == "o" || p == "O" {
                p = "0".into();
            }
            if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
                let v: i64 = p.parse().unwrap_or(i64::MAX);
                match spill.as_mut() {
                    Some(s) => {
                        if let Some(gap) = s.iter_mut().find(|x| x.1.is_none()) {
                            gap.1 = Some(v);
                        }
                    }
                    None => vals.push(v),
                }
            } else if p.chars().any(|c| c.is_ascii_alphabetic()) {
                let key = label(&p);
                let m = tail_num.captures(&p).map(|c| c[1].parse::<i64>().unwrap_or(i64::MAX));
                if let Some(k) = key
                    && seen.contains(k)
                    && spill.is_none()
                {
                    spill = Some(vec![]);
                }
                if let Some(s) = spill.as_mut() {
                    if let Some(k) = key {
                        s.push((k, m));
                    }
                    continue;
                }
                if let Some(k) = key {
                    seen.insert(k);
                }
                if let Some(v) = m {
                    vals.push(v);
                }
            }
        }
        let card: IndexMap<&'static str, Option<i64>> = BD_KEYS.iter().enumerate().map(|(i, k)| (*k, vals.get(i).copied())).collect();
        cards.push(card);
        let had: Vec<usize> = cards.iter().map(|c| BD_KEYS.iter().filter(|k| c[*k].is_some()).count()).collect();
        for (key, value) in spill.unwrap_or_default() {
            let short: Vec<usize> = (0..cards.len().saturating_sub(1)).rev().filter(|&i| cards[i][key].is_none()).collect();
            // min(short, key=had): the first of the fewest
            let owner = short.iter().copied().fold(None, |b: Option<usize>, i| match b {
                Some(j) if had[j] <= had[i] => Some(j),
                _ => Some(i),
            });
            if let (Some(o), Some(v)) = (owner, value) {
                cards[o].insert(key, Some(v));
            }
        }
    }
    if cards.iter().any(|c| BD_KEYS.iter().all(|k| c[*k].is_none())) {
        return vec![];
    }
    for c in cards.iter_mut() {
        if BD_KEYS.iter().filter(|k| c[*k].is_some()).count() == 3 && c["heal"].is_none() {
            c.insert("heal", Some(0));
        }
    }
    cards.into_iter().map(|c| Value::Dict(c.into_iter().map(|(k, v)| (Value::Str(k.into()), v.map_or(Value::None, Value::Int))).collect())).collect()
}

/// Votes over repeated reads: key -> (value -> count), each in first-seen order.
pub type Votes = IndexMap<String, IndexMap<String, (Value, i64)>>;

fn most(c: &IndexMap<String, (Value, i64)>) -> Option<(Value, i64)> {
    // max by (count, -first index): the most, and the first seen of equals
    let mut best: Option<(Value, i64)> = None;
    for (v, n) in c.values() {
        if best.as_ref().is_none_or(|b| *n > b.1) {
            best = Some((v.clone(), *n));
        }
    }
    best
}

/// One read's value for `key`; returns the value most reads so far agree on.
pub fn vote(votes: &mut Votes, key: &str, value: Value) -> Value {
    let empty = value.is_none() || value.as_str() == Some("");
    if !empty {
        let c = votes.entry(key.to_string()).or_default();
        let k = value.repr();
        c.entry(k).or_insert((value.clone(), 0)).1 += 1;
    }
    match votes.get(key).filter(|c| !c.is_empty()) {
        Some(c) => most(c).unwrap().0,
        None => value,
    }
}

/// The value most reads agree on, by at least two of them, else None.
pub fn confirmed(votes: &Votes, key: &str) -> Value {
    match votes.get(key).and_then(most) {
        Some((v, n)) if n >= 2 => v,
        _ => Value::None,
    }
}

/// The slots whose name most reads agree on, by at least two of them.
pub fn agreed_slots(votes: &Votes, key: impl Fn(usize) -> String) -> BTreeSet<i64> {
    (0..4).filter(|&i| votes.get(&key(i)).is_some_and(|c| c.values().map(|x| x.1).max().unwrap_or(0) >= 2)).map(|i| i as i64).collect()
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

/// Each result-screen (or Battle Data) column takes the Party screen's player of the same slot (see the Python's
/// long docstring in results.py: party_slots).
pub fn party_slots(team: &Teammates, players: &mut [Dict], party: Option<&[Value]>, sure: &BTreeSet<i64>, you: Option<&str>, agreed: &BTreeSet<i64>) {
    let Some(party) = party.filter(|p| !p.is_empty()) else {
        for p in players.iter_mut() {
            let name = s(p.at("name"));
            if !name.is_empty() && !p.at("self").truthy() {
                p.insert("name".into(), Value::Str(team.steady(&name)));
            }
        }
        return;
    };
    let mut by_slot: IndexMap<i64, Value> = IndexMap::new();
    for m in party {
        if m.get("name").truthy() {
            by_slot.insert(m.get("slot").as_i64().unwrap_or(-1), m.clone());
        }
    }
    let party: Vec<Value> = by_slot.values().cloned().collect();
    let sim = |a: &str, b: &str| ratio(&Teammates::fold(a), &Teammates::fold(b));
    let mut taken: BTreeSet<i64> = BTreeSet::new();
    let mine: BTreeSet<i64> = party
        .iter()
        .filter(|x| x.get("self").truthy() || you.is_some_and(|y| same_name(x.get("name").as_str(), Some(y))))
        .map(|x| x.get("slot").as_i64().unwrap_or(-1))
        .collect();
    let twins = |n: &str| Regex::new(r"[Il1|]").unwrap().replace_all(n, "l").into_owned();
    let give = |p: &mut Dict, m: &Value, taken: &mut BTreeSet<i64>| {
        let own = s(p.at("name"));
        let mname = s(m.get("name"));
        let pslot = p.at("slot").as_i64().unwrap_or(-1);
        if agreed.contains(&pslot) && !own.is_empty() && twins(&own) != twins(&mname) {
            let alike = sim(&own, &mname) >= 0.6;
            if own != mname {
                log::info!(target: "meter", "result: column {} keeps {:?}, read alike again and again, not the Party screen's {:?}", pslot + 1, own, mname);
            }
            let npc = INTERLINK_NPCS.contains(&casefold(&own).as_str());
            p.insert("ai".into(), Value::Bool(m.get("ai").truthy() && (alike || npc)));
            p.insert("self".into(), Value::Bool(m.get("self").truthy() && alike));
        } else {
            p.insert("name".into(), Value::Str(mname));
            p.insert("ai".into(), Value::Bool(m.get("ai").truthy()));
            p.insert("self".into(), Value::Bool(m.get("self").truthy()));
        }
        let ms = m.get("slot").as_i64().unwrap_or(-1);
        if mine.contains(&ms) {
            taken.extend(mine.iter().copied());
        } else {
            taken.insert(ms);
        }
    };
    // me: the player who reads as you, the most alike (first of equals)
    let mut me: Option<usize> = None;
    if let Some(y) = you {
        let mut best = -1.0;
        for (i, p) in players.iter().enumerate() {
            if same_name(p.at("name").as_str(), Some(y)) {
                let r = sim(&s(p.at("name")), y);
                if r > best {
                    best = r;
                    me = Some(i);
                }
            }
        }
    }
    let mut rest: Vec<usize> = Vec::new();
    for i in 0..players.len() {
        let own = s(players[i].at("name"));
        let slot = players[i].at("slot").as_i64().unwrap_or(-1);
        let m = by_slot.get(&slot).cloned();
        if Some(i) == me {
            let y = you.unwrap();
            if let Some(m) = &m
                && s(m.get("name")) != y
            {
                log::info!(target: "meter", "result: column {} reads as you, not the Party screen's {:?}", slot + 1, s(m.get("name")));
            }
            let p = &mut players[i];
            p.insert("name".into(), Value::Str(y.to_string()));
            p.insert("ai".into(), Value::Bool(false));
            p.insert("self".into(), Value::Bool(true));
            taken.extend(mine.iter().copied());
        } else if sure.contains(&slot) && own.chars().count() == 1 && own.chars().all(|c| c.is_ascii_alphanumeric()) {
            if let Some(m) = &m
                && s(m.get("name")) != own
            {
                log::info!(target: "meter", "result: column {} reads {:?} (sure), not the Party screen's {:?}", slot + 1, own, s(m.get("name")));
            }
            let p = &mut players[i];
            if p.get_str("ai").is_none() {
                p.insert("ai".into(), Value::Bool(false));
            }
            p.insert("sure".into(), Value::Bool(true));
        } else {
            rest.push(i);
        }
    }
    let named = |p: &Dict, taken: &BTreeSet<i64>| -> Option<(f64, Value)> {
        let slot = p.at("slot").as_i64().unwrap_or(-1);
        let m = by_slot.get(&slot);
        let pn = s(p.at("name"));
        let mut other: Option<(f64, &Value)> = None;
        for x in &party {
            let same_member = m.is_some_and(|mm| mm.get("slot").as_i64() == x.get("slot").as_i64());
            if same_member || taken.contains(&x.get("slot").as_i64().unwrap_or(-1)) {
                continue;
            }
            let r = sim(&s(x.get("name")), &pn);
            if other.is_none_or(|o| r > o.0) {
                other = Some((r, x));
            }
        }
        let (sc, o) = other.map_or((0.0, None), |o| (o.0, Some(o.1.clone())));
        if sc >= 0.8 && m.is_none_or(|mm| sc >= sim(&s(mm.get("name")), &pn) + 0.3) { o.map(|o| (sc, o)) } else { None }
    };
    loop {
        let mut best: Option<(f64, Value, usize)> = None;
        for &pi in &rest {
            if let Some((sc, o)) = named(&players[pi], &taken)
                && best.as_ref().is_none_or(|b| sc > b.0)
            {
                best = Some((sc, o, pi));
            }
        }
        let Some((_, other, pi)) = best else { break };
        let slot = players[pi].at("slot").as_i64().unwrap_or(-1);
        let m = by_slot.get(&slot);
        log::info!(target: "meter", "result: column {} reads {:?}, the Party screen's {:?} (slot {}), not slot {}'s {:?}", slot + 1, s(players[pi].at("name")),
            s(other.get("name")), other.get("slot").as_i64().unwrap_or(-1) + 1, slot + 1, m.map(|m| s(m.get("name"))));
        give(&mut players[pi], &other, &mut taken);
        rest.retain(|&x| x != pi);
    }
    for pi in rest {
        let slot = players[pi].at("slot").as_i64().unwrap_or(-1);
        let Some(m) = by_slot.get(&slot).cloned() else { continue };
        let mslot = m.get("slot").as_i64().unwrap_or(-1);
        if taken.contains(&mslot) {
            log::info!(target: "meter", "result: column {} keeps its read {:?}: slot {}'s {:?} is another column's", slot + 1, s(players[pi].at("name")), slot + 1, s(m.get("name")));
            let name = s(players[pi].at("name"));
            if !name.is_empty() {
                players[pi].insert("name".into(), Value::Str(team.steady(&name)));
            }
            continue;
        }
        let pn = s(players[pi].at("name"));
        if !pn.is_empty() && sim(&s(m.get("name")), &pn) < 0.3 {
            log::info!(target: "meter", "result: column {} reads {:?}, taken as slot {}'s {:?}", slot + 1, pn, slot + 1, s(m.get("name")));
        }
        give(&mut players[pi], &m, &mut taken);
    }
}

/// After `party_slots`, on the result screen: your row is the one it marked, else the first that reads as you.
pub fn mark_you(players: &mut [Dict], party: Option<&[Value]>, known: Option<&str>) {
    let party = party.unwrap_or(&[]);
    let you = party.iter().find(|m| m.get("self").truthy()).map(|m| s(m.get("name")));
    let mut mine = players.iter().any(|p| p.at("self").truthy());
    for p in players.iter_mut() {
        let name = s(p.at("name"));
        if !mine && ((known.is_some_and(|k| !k.is_empty()) && same_name(Some(&name), known)) || you.as_ref().is_some_and(|y| !y.is_empty() && name == *y)) {
            p.insert("self".into(), Value::Bool(true));
            mine = true;
        }
        let v = p.at("self").truthy();
        p.insert("self".into(), Value::Bool(v));
        if p.get_str("ai").is_none() {
            let ai = party.iter().any(|m| m.get("ai").truthy() && s(m.get("name")) == name);
            p.insert("ai".into(), Value::Bool(ai));
        }
    }
}

/// Which name on the result screen is the player running the meter, when it can be told.
pub fn own_name(known: Option<&str>, names: &[String], party: Option<&[Value]>) -> Option<String> {
    let names: Vec<&String> = names.iter().filter(|n| (2..=40).contains(&n.chars().count())).collect();
    if let Some(k) = known.filter(|k| !k.is_empty())
        && names.iter().any(|n| same_name(Some(k), Some(n)))
    {
        return Some(k.to_string());
    }
    let party = party?;
    let humans: Vec<String> = party.iter().filter(|m| !m.get("ai").truthy()).map(|m| s(m.get("name"))).collect();
    if !party.is_empty() && humans.len() == 1 {
        let h = &humans[0];
        return names.iter().find(|n| same_name(Some(h), Some(n))).map(|n| (*n).clone()).or_else(|| (2..=40).contains(&h.chars().count()).then(|| h.clone()));
    }
    None
}

/// How far a copy of the screen is from the game window alone, cell by cell on a 30 x 16 grid.
pub fn screen_difference(screen: &Image, window: &Image) -> (u8, usize, usize) {
    let cells = |im: &Image| im.convert(Mode::L).resize(320, 180, Some(Filter::Box)).crop_i(10, 11, 310, 169).resize(30, 16, Some(Filter::Box));
    let d = alogs_pil::ops::difference(&cells(screen), &cells(window));
    (d.data.iter().copied().max().unwrap_or(0), d.data.iter().filter(|v| **v > 25).count(), d.data.iter().filter(|v| **v > 40).count())
}
