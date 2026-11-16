//! Egg Heist season rank: the rank ladder and reading the game's "Rank Info" screen. Port of aLogs B16
//! `bossbar/eggheist.py`. What gets recorded is what the screen shows (rank name, Rank Points and, at Egg Emperor,
//! stars); the ladder only fills in the sub-tier the title doesn't spell out.

use alogs_pyval::{Value, dict};
use regex::Regex;
use std::sync::LazyLock;

/// (big rank name, sub-tier labels from lowest to highest, eggs per sub-tier, points per egg)
pub const RANKS: [(&str, &[&str], i64, i64); 7] = [
    ("Rookie Egg Hunter", &["III", "II", "I"], 3, 30),
    ("Rising Egg Scout", &["III", "II", "I"], 3, 60),
    ("Pro Egg Snatcher", &["IV", "III", "II", "I"], 4, 90),
    ("Elite Egg Seeker", &["IV", "III", "II", "I"], 5, 120),
    ("Expert Egg Raider", &["V", "IV", "III", "II", "I"], 6, 150),
    ("Egg Heist Master", &["V", "IV", "III", "II", "I"], 6, 180),
    ("Egg Emperor", &[""], 0, 180),
];
pub const TOP_TIER: i64 = RANKS.len() as i64;

/// [(tier 1-7, sub-tier index 1.., full name, points where it starts)] in order.
pub fn ladder() -> Vec<(i64, i64, String, i64)> {
    let mut tiers = vec![];
    for (bi, (name, labels, eggs, pt)) in RANKS.iter().enumerate() {
        for (si, label) in labels.iter().enumerate() {
            tiers.push((bi as i64 + 1, si as i64 + 1, format!("{name} {label}").trim().to_string(), *eggs, *pt));
        }
    }
    let mut out = vec![];
    let mut acc = 0;
    for (i, (tier, sub, name, eggs, _)) in tiers.iter().enumerate() {
        out.push((*tier, *sub, name.clone(), acc));
        if i + 1 < tiers.len() {
            acc += 0.max(eggs - if i == 0 { 0 } else { 1 }) * tiers[i].4 + tiers[i + 1].4;
        }
    }
    out
}

pub static LADDER: LazyLock<Vec<(i64, i64, String, i64)>> = LazyLock::new(ladder);

fn letters(s: &str) -> String {
    s.to_lowercase().chars().filter(char::is_ascii_lowercase).collect()
}

/// Big rank (1-7) from the screen title, tolerant of small OCR slips.
pub fn tier_of(title: Option<&str>) -> Option<i64> {
    let title = title.unwrap_or("");
    let t = letters(title);
    for (i, r) in RANKS.iter().enumerate() {
        if t.contains(&letters(r.0)) {
            return Some(i as i64 + 1);
        }
    }
    let low = title.to_lowercase();
    let mut best: Option<(usize, i64)> = None;
    for (i, r) in RANKS.iter().enumerate() {
        let hits = r.0.to_lowercase().split_whitespace().filter(|w| low.contains(w)).count();
        if hits >= 2 && best.is_none_or(|b| hits > b.0) {
            best = Some((hits, i as i64 + 1));
        }
    }
    best.map(|b| b.1)
}

fn opt(v: Option<i64>) -> Value {
    v.map_or(Value::None, Value::Int)
}

/// The standing to record: {tier, subtier, name, points, stars}; sub-tier from points when known.
pub fn standing(title: Option<&str>, points: Option<i64>, stars: Option<i64>) -> Option<Value> {
    let tier = tier_of(title)?;
    if tier == TOP_TIER {
        return Some(dict([("tier", Value::Int(tier)), ("subtier", Value::Int(1)), ("name", Value::Str(RANKS[6].0.into())), ("points", opt(points)), ("stars", opt(stars))]));
    }
    let rows: Vec<_> = LADDER.iter().filter(|r| r.0 == tier).collect();
    let mut row = rows[0];
    if let Some(p) = points {
        row = rows.iter().rev().find(|r| r.3 <= p).copied().unwrap_or(rows[0]);
    }
    let name = if points.is_some() { row.2.clone() } else { RANKS[tier as usize - 1].0.to_string() };
    Some(dict([("tier", Value::Int(tier)), ("subtier", Value::Int(row.1)), ("name", Value::Str(name)), ("points", opt(points)), ("stars", Value::None)]))
}

/// egg_rank_base_data.pointLimit by tier: what one match can add.
pub fn point_limit(tier: i64) -> Option<i64> {
    match tier {
        1 => Some(90),
        2 => Some(180),
        3 => Some(270),
        4..=7 => Some(360),
        _ => None,
    }
}

/// Could a match have added this? Yes when either is unknown, and for a loss.
pub fn gain_possible(gain: Option<i64>, tier: Option<i64>) -> bool {
    match (gain, tier.and_then(point_limit)) {
        (Some(g), Some(l)) => g <= l,
        _ => true,
    }
}

fn num(s: &str) -> Option<i64> {
    s.replace(',', "").parse().ok()
}

/// 'Rank Points 1277/2370' -> (1277, 2370).
pub fn parse_points(text: Option<&str>) -> (Option<i64>, Option<i64>) {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d[\d,]*)\s*/\s*(\d[\d,]*)").unwrap());
    let t = text.unwrap_or("").replace('O', "0").replace('o', "0");
    match RE.captures(&t) {
        Some(c) => (num(&c[1]), num(&c[2])),
        None => (None, None),
    }
}

/// Season start dates (as YYYY-MM-DD) -> the game's season badge.
pub const KNOWN_SEASONS: [(&str, &str); 1] = [("2026-09-15", "S1")];

/// The season running on `today` (YYYY-MM-DD), from the known start dates.
pub fn current_season(today: &str) -> Option<String> {
    KNOWN_SEASONS.iter().filter(|(start, _)| *start <= today).max().map(|(_, n)| n.to_string())
}

/// '+18' -> 18, a loss, '-18', -> -18.
pub fn parse_gain(text: Option<&str>) -> Option<i64> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([+\-\x{2212}\x{2013}])\s*(\d{1,4})").unwrap());
    let t = text.unwrap_or("").replace('O', "0");
    let c = RE.captures(&t)?;
    let n: i64 = c[2].parse().ok()?;
    Some(if &c[1] == "+" { n } else { -n })
}

/// 'S1' from the season badge, else from the date range under the season title.
pub fn parse_season(badge: Option<&str>, header: &str) -> Option<String> {
    static BADGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[S5$]\s*(\d{1,2})\b").unwrap());
    static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{2})/(\d{2})/(\d{4})\s*-").unwrap());
    if let Some(c) = BADGE.captures(badge.unwrap_or("")) {
        return Some(format!("S{}", c[1].parse::<i64>().ok()?));
    }
    let h = header.replace('O', "0");
    let c = DATE.captures(&h)?;
    let start = format!("{}-{}-{}", &c[3], &c[1], &c[2]);
    Some(KNOWN_SEASONS.iter().find(|(s, _)| *s == start).map_or_else(|| format!("season-{start}"), |(_, n)| n.to_string()))
}
