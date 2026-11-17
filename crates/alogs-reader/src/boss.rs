//! Boss data: the boss list, Interlink's HP bars, the week schedule, picking the boss on the bar, max HP and damage.
//! Port of the boss-data half of aLogs B16 `bossbar/reader.py`.

use crate::parse::norm;
use alogs_gamedata::hpcalc;
use alogs_gamelog::difflib::ratio;
use indexmap::IndexMap;
use serde_json::Value as J;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A boss_catalog.csv row (column -> text).
pub type Row = IndexMap<String, String>;

fn get<'a>(r: &'a Row, k: &str) -> &'a str {
    r.get(k).map_or("", String::as_str)
}

pub fn read_csv(path: &Path) -> std::io::Result<Vec<Row>> {
    let mut rd = csv::Reader::from_path(path).map_err(std::io::Error::other)?;
    let headers = rd.headers().map_err(std::io::Error::other)?.clone();
    let mut out = Vec::new();
    for rec in rd.records() {
        let rec = rec.map_err(std::io::Error::other)?;
        out.push(headers.iter().zip(rec.iter()).map(|(h, v)| (h.to_string(), v.to_string())).collect());
    }
    Ok(out)
}

/// The shipped boss list with the game data's own rows taking the place of the same (table, puppetId) and adding
/// bosses new since aLogs was built.
pub fn load_catalog(res: &Path, game: Option<&[Row]>) -> std::io::Result<Vec<Row>> {
    let rows = read_csv(&res.join("boss_catalog.csv"))?;
    let Some(game) = game.filter(|g| !g.is_empty()) else { return Ok(rows) };
    let key = |r: &Row| (get(r, "table").to_string(), get(r, "puppetId").to_string());
    let mut fresh: IndexMap<(String, String), Row> = game.iter().map(|r| (key(r), r.clone())).collect();
    let mut out: Vec<Row> = rows
        .iter()
        .map(|r| {
            let mut m = r.clone();
            if let Some(f) = fresh.shift_remove(&key(r)) {
                for (k, v) in f {
                    m.insert(k, v);
                }
            }
            m
        })
        .collect();
    out.extend(fresh.into_values());
    Ok(out)
}

pub const GRADE_BASE: &str = "bossHpLossPercent";
pub const GRADE_COLUMNS: [&str; 3] = [GRADE_BASE, "bossHpLossPercent_2", "bossHpLossPercent_3"];

/// `{column: [share of max HP lost at grade 0, 1, ...]}`: the game data's rows when there are some, else the
/// shipped boss_rush_grades.csv.
pub fn load_grades(res: &Path, game: Option<&[Row]>) -> std::io::Result<IndexMap<String, Vec<f64>>> {
    let rows: Vec<Row> = match game.filter(|g| !g.is_empty()) {
        Some(g) => g.to_vec(),
        None => read_csv(&res.join("boss_rush_grades.csv"))?,
    };
    let mut out = IndexMap::new();
    for c in GRADE_COLUMNS {
        let v = rows
            .iter()
            .map(|r| alogs_pyval::parse_float(get(r, c)).ok_or_else(|| std::io::Error::other(format!("bad {c}"))))
            .collect::<std::io::Result<Vec<f64>>>()?;
        out.insert(c.to_string(), v);
    }
    Ok(out)
}

/// The bar counter at a boss's full HP (x21): one bar per grade above 0.
pub fn xn_top(grades: &IndexMap<String, Vec<f64>>) -> i64 {
    grades[GRADE_BASE].len() as i64 - 1
}

pub fn hp_formula(r: &Row) -> Option<J> {
    let v: J = serde_json::from_str(get(r, "fixed_hp_rate")).ok()?;
    v.as_object().map(|o| o.get("1").cloned().unwrap_or(J::Null))
}

/// A boss with only Interlink entries met in the world: its species with the standard wild-Alpha HP formula.
pub fn wild_copy(row: &Row) -> Row {
    let mut r = row.clone();
    r.insert("hp_estimated".into(), "wild".into());
    r.insert("prop_species_adj".into(), "0".into());
    r.insert("fixed_hp_rate".into(), r#"{"1": 3101, "2": 75, "3": 1, "4": 1, "5": 1}"#.into());
    r
}

/// Out in the world, the entries a boss on the bar can be (the wild one preferred).
pub fn world_rows(rows: &[Row]) -> Vec<Row> {
    let rows: Vec<Row> = rows.iter().filter(|r| !get(r, "fixed_hp_rate").is_empty()).cloned().collect();
    let big = |r: &Row| get(r, "puppetId").starts_with("8000") || get(r, "puppetId").starts_with("1108");
    if !rows.is_empty() && rows.iter().all(big) {
        return vec![wild_copy(&rows[0])];
    }
    let rows: Vec<Row> = rows.into_iter().filter(|r| !big(r)).collect();
    let wild: Vec<Row> = rows.iter().filter(|r| hp_formula(r).is_some_and(|f| f.as_f64() == Some(3101.0))).cloned().collect();
    let named: Vec<Row> =
        wild.iter().filter(|r| norm(Some(get(r, "name_en"))) == norm(Some(get(r, "bossTitle_en")))).cloned().collect();
    if !named.is_empty() {
        return named;
    }
    let w21: Vec<Row> = wild.iter().filter(|r| get(r, "puppetId").starts_with("21")).cloned().collect();
    if !w21.is_empty() {
        return w21;
    }
    if !wild.is_empty() {
        return wild;
    }
    rows
}

/// `difflib.get_close_matches(word, possibilities, n=1, cutoff)`: the best, ties to the larger string.
pub fn close_match(word: &str, possibilities: &BTreeSet<String>, cutoff: f64) -> Option<String> {
    let mut best: Option<(f64, &String)> = None;
    for x in possibilities {
        let s = ratio(x, word);
        if s >= cutoff && best.is_none_or(|b| (s, x) > (b.0, b.1)) {
            best = Some((s, x));
        }
    }
    best.map(|b| b.1.clone())
}

/// The boss rush week schedule (boss_rush_cycles.json), read again at each question as the Python does.
#[derive(Clone, Debug)]
pub struct Schedule {
    pub path: PathBuf,
}

fn ids_of(c: &J) -> Vec<J> {
    let mut v: Vec<J> = c["bossIds"].as_array().cloned().unwrap_or_default();
    if let Some(two) = c.get("bossIds2").and_then(J::as_array) {
        v.extend(two.iter().filter(|x| !x.is_null() && x.as_f64() != Some(0.0)).cloned());
    }
    v
}

fn jstr(v: &J) -> String {
    match v {
        J::String(s) => s.clone(),
        J::Null => "None".into(),
        v => alogs_pyval::Value::from_json(v).py_str(),
    }
}

impl Schedule {
    pub fn new(path: PathBuf) -> Schedule {
        Schedule { path }
    }
    fn cycles(&self) -> Option<Vec<J>> {
        let t = std::fs::read_to_string(&self.path).ok()?;
        serde_json::from_str::<J>(&t).ok()?.as_array().cloned()
    }
    fn running(c: &J, now: f64) -> bool {
        c["start"].as_f64().is_some_and(|s| s <= now) && c["end"].as_f64().is_some_and(|e| now <= e)
    }

    /// Boss ids of the week running now.
    pub fn active_bosses(&self, now: f64) -> BTreeSet<i64> {
        let mut out = BTreeSet::new();
        for c in self.cycles().unwrap_or_default() {
            if Self::running(&c, now) {
                out.extend(ids_of(&c).iter().filter_map(J::as_i64));
            }
        }
        out
    }

    /// The running week's bosses by their spot on the map: left, middle, right.
    pub fn active_slots(&self, now: f64) -> IndexMap<&'static str, String> {
        for c in self.cycles().unwrap_or_default() {
            let ids = c["bossIds"].as_array().cloned().unwrap_or_default();
            if Self::running(&c, now) && ids.len() == 3 {
                return ["left", "middle", "right"].into_iter().zip(ids.iter().map(jstr)).collect();
            }
        }
        IndexMap::new()
    }

    /// Id of the week running now, or None when the schedule doesn't cover today.
    pub fn active_cycle_id(&self, now: f64) -> Option<String> {
        self.cycles()?.iter().find(|c| Self::running(c, now)).map(|c| jstr(&c["cycle"]))
    }

    /// Of these boss ids, the one in the most recent week.
    pub fn latest_cycle_boss(&self, ids: &BTreeSet<i64>) -> Option<i64> {
        let mut cycles = self.cycles()?;
        // sorted(key=end, reverse=True) keeps equal ends in their order
        cycles.sort_by(|a, b| b["end"].as_f64().unwrap_or(0.0).total_cmp(&a["end"].as_f64().unwrap_or(0.0)));
        for c in cycles {
            if let Some(hit) = ids_of(&c).iter().filter_map(J::as_i64).find(|b| ids.contains(b)) {
                return Some(hit);
            }
        }
        None
    }

    /// Where a boss fights in a week: (slot, the week's split).
    pub fn cycle_slot(&self, cycle: Option<&str>, boss_id: &str) -> (Option<&'static str>, J) {
        let Some(cycles) = self.cycles() else { return (None, J::Null) };
        let cyc = cycle.map_or("None".to_string(), str::to_string);
        let Some(c) = cycles.iter().find(|c| jstr(&c["cycle"]) == cyc) else { return (None, J::Null) };
        let ids = c["bossIds"].as_array().cloned().unwrap_or_default();
        let ids2 = c.get("bossIds2").and_then(J::as_array).cloned().unwrap_or_else(|| vec![J::Null; 3]);
        for ((slot, a), b) in ["left", "middle", "right"].into_iter().zip(&ids).zip(&ids2) {
            if boss_id == jstr(a) || boss_id == jstr(b) {
                return (Some(slot), c.get("split").cloned().unwrap_or(J::Null));
            }
        }
        (None, c.get("split").cloned().unwrap_or(J::Null))
    }

    /// A boss's place in a week's fights: (slot, order, partner).
    pub fn stage_of(&self, puppet_id: Option<&str>, cycle: Option<&str>, now: f64) -> (Option<&'static str>, Option<i64>, Option<String>) {
        let Some(pid) = puppet_id else { return (None, None, None) };
        let Some(cycles) = self.cycles() else { return (None, None, None) };
        let mut cycle = cycle.map(str::to_string);
        if cycle.is_none() {
            cycle = self.active_cycle_id(now);
            if self.cycle_slot(cycle.as_deref(), pid).0.is_none() {
                let weeks: Vec<&J> = cycles.iter().filter(|c| ids_of(c).iter().any(|b| jstr(b) == pid)).collect();
                // max(weeks, key=end): the first of the latest
                let mut best: Option<&J> = None;
                for w in weeks {
                    if best.is_none_or(|b| w["end"].as_f64().unwrap_or(0.0) > b["end"].as_f64().unwrap_or(0.0)) {
                        best = Some(w);
                    }
                }
                if let Some(b) = best {
                    cycle = Some(jstr(&b["cycle"]));
                }
            }
        }
        let Some(slot) = self.cycle_slot(cycle.as_deref(), pid).0 else { return (None, None, None) };
        let cyc = cycle.unwrap_or_default();
        let c = cycles.iter().find(|c| jstr(&c["cycle"]) == cyc).unwrap();
        let i = ["left", "middle", "right"].iter().position(|s| *s == slot).unwrap();
        let first = &c["bossIds"][i];
        let second = c.get("bossIds2").and_then(|v| v.get(i)).cloned().unwrap_or(J::Null);
        let truthy = !(second.is_null() || second.as_f64() == Some(0.0) || second.as_str() == Some(""));
        if !truthy {
            return (Some(slot), None, None);
        }
        if jstr(first) == pid { (Some(slot), Some(1), Some(jstr(&second))) } else { (Some(slot), Some(2), Some(jstr(first))) }
    }

    /// The grade column a boss's HP bars follow (by its spawn order in a two-boss fight).
    pub fn grade_column(&self, puppet_id: Option<&str>, now: f64) -> &'static str {
        match self.stage_of(puppet_id, None, now).1 {
            Some(1) => "bossHpLossPercent_2",
            Some(2) => "bossHpLossPercent_3",
            _ => GRADE_BASE,
        }
    }
}

/// The boss on the bar: `(row, how many different HP inputs the matching rows have)`.
pub fn pick_boss(catalog: &[Row], title: &str, bossrush: bool, boss_id: Option<&str>, sched: &Schedule, now: f64) -> (Option<Row>, usize) {
    let main: Vec<&Row> = {
        let m: Vec<&Row> = catalog.iter().filter(|r| get(r, "table") == "puppet_data").collect();
        if m.is_empty() { catalog.iter().collect() } else { m }
    };
    let pid = |r: &Row| get(r, "puppetId").parse::<i64>().unwrap_or(i64::MIN);
    let mut rows: Vec<Row>;
    if let Some(id) = boss_id.filter(|s| !s.is_empty()) {
        rows = main.iter().filter(|r| get(r, "puppetId") == id).map(|r| (*r).clone()).collect();
    } else {
        let t = norm(Some(title));
        rows = main.iter().filter(|r| !t.is_empty() && t == norm(Some(get(r, "bossTitle_en")))).map(|r| (*r).clone()).collect();
        if rows.is_empty() {
            rows = main.iter().filter(|r| !t.is_empty() && t == norm(Some(get(r, "name_en")))).map(|r| (*r).clone()).collect();
        }
        if rows.is_empty() && !t.is_empty() {
            let titles: BTreeSet<String> =
                main.iter().filter(|r| !get(r, "bossTitle_en").is_empty()).map(|r| norm(Some(get(r, "bossTitle_en")))).collect();
            if let Some(close) = close_match(&t, &titles, 0.8) {
                rows = main.iter().filter(|r| norm(Some(get(r, "bossTitle_en"))) == close).map(|r| (*r).clone()).collect();
            }
        }
        if bossrush {
            let rush: Vec<Row> = rows.iter().filter(|r| get(r, "in_boss_rush") == "yes").cloned().collect();
            if !rush.is_empty() {
                rows = rush.clone();
            }
            let active = sched.active_bosses(now);
            let mut in_active: Vec<Row> = rows.iter().filter(|r| active.contains(&pid(r))).cloned().collect();
            if in_active.is_empty() && rows.len() > 1 {
                let ids: BTreeSet<i64> = rows.iter().map(pid).collect();
                let latest = sched.latest_cycle_boss(&ids);
                in_active = rows.iter().filter(|r| Some(pid(r)) == latest).cloned().collect();
            }
            if in_active.is_empty() && !active.is_empty() {
                let week: Vec<&Row> = main
                    .iter()
                    .filter(|r| active.contains(&pid(r)) && get(r, "in_boss_rush") == "yes" && !get(r, "bossTitle_en").is_empty())
                    .copied()
                    .collect();
                let mut fits: Vec<&Row> = week.iter().filter(|r| !t.is_empty() && norm(Some(get(r, "bossTitle_en"))).contains(&t)).copied().collect();
                if fits.is_empty() {
                    fits = week.iter().filter(|r| !t.is_empty() && ratio(&t, &norm(Some(get(r, "bossTitle_en")))) >= 0.6).copied().collect();
                }
                let ids: BTreeSet<&str> = fits.iter().map(|r| get(r, "puppetId")).collect();
                if ids.len() == 1 {
                    in_active = fits.into_iter().cloned().collect();
                }
            }
            rows = if !active.is_empty() { in_active } else if !rush.is_empty() { rush } else { rows };
        } else {
            let not_rush: Vec<Row> = rows.iter().filter(|r| get(r, "in_boss_rush") != "yes").cloned().collect();
            rows = world_rows(if not_rush.is_empty() { &rows } else { &not_rush });
        }
    }
    let rows: Vec<Row> = rows.into_iter().filter(|r| !get(r, "fixed_hp_rate").is_empty()).collect();
    let kinds: BTreeSet<(String, String, String)> = rows
        .iter()
        .map(|r| (get(r, "species_hp").to_string(), get(r, "prop_species_adj").to_string(), get(r, "fixed_hp_rate").to_string()))
        .collect();
    (rows.into_iter().next(), kinds.len())
}

/// calibration.json in the meter's data folder: measured max HP per boss and level.
pub fn load_calibration(data: &Path) -> J {
    std::fs::read_to_string(data.join("calibration.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(J::Object(Default::default()))
}

/// Size of the boss's break gauge (from boss_break.json), or None when unknown.
pub fn boss_break_points(res: &Path, row: &Row, level: f64) -> Option<f64> {
    let t = std::fs::read_to_string(res.join("boss_break.json")).ok()?;
    let all: J = serde_json::from_str(&t).ok()?;
    let b = all.get(get(row, "propId"))?;
    let args: Vec<f64> = b["args"].as_array()?.iter().filter_map(J::as_f64).collect();
    hpcalc::break_points(b["formula"].as_i64()?, &args, level)
}

/// The boss's max HP at a level (the calibration's measured value when it has one).
pub fn boss_max_hp(row: &Row, level: i64, calibration: Option<&J>) -> Option<i64> {
    let f = |k: &str| {
        let v = get(row, k);
        if v.is_empty() { Some(0.0) } else { alogs_pyval::parse_float(v) }
    };
    let sp = f("species_hp")? + f("prop_species_adj")?;
    let rate: J = serde_json::from_str(get(row, "fixed_hp_rate")).ok()?;
    let predicted = hpcalc::max_hp(sp, &rate, level, 0.0);
    if let Some(cal) = calibration
        && let Some(m) = cal.get(format!("{}@{}", get(row, "puppetId"), level)).and_then(|c| c.get("measured"))
        && m.as_f64().is_some_and(|v| v != 0.0)
    {
        return m.as_i64().or_else(|| m.as_f64().map(|v| v as i64));
    }
    predicted
}

/// What `damage_dealt` adds about the grade being worked toward.
#[derive(Clone, Debug, PartialEq)]
pub struct GradeInfo {
    pub grade: i64,
    pub size_hp: i64,
    pub left_hp: i64,
}

/// Team damage so far (Interlink: through the grade table).
pub fn damage_dealt(max_hp: f64, fill: f64, xn: Option<i64>, cum: &[f64], bossrush: bool) -> (Option<i64>, Option<GradeInfo>) {
    let r = |v: f64| v.round_ties_even() as i64;
    let Some(xn) = xn.filter(|_| bossrush) else { return (Some(r((1.0 - fill) * max_hp)), None) };
    let g = cum.len() as i64 - xn;
    if !(1 <= g && g <= cum.len() as i64 - 1) {
        return (None, None);
    }
    let (lo, hi) = (cum[g as usize - 1], cum[g as usize]);
    (
        Some(r((lo + (1.0 - fill) * (hi - lo)) * max_hp)),
        Some(GradeInfo { grade: g, size_hp: r((hi - lo) * max_hp), left_hp: r(fill * (hi - lo) * max_hp) }),
    )
}
