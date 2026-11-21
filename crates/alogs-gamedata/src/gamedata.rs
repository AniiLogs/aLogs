//! The boss rush (Interlink) week schedule, the boss list and Interlink's HP bars, read from the player's own game
//! install. Port of aLogs B16 `bossbar/gamedata.py` (everything but the build-time `write_*` and `__main__`).
//!
//! Rows are [`Value`]s shaped exactly like the Python's dicts, so `repr()` of a result can be compared with the
//! reference's and the JSON caches stay readable by both meters.

use crate::hpcalc;
use crate::pmdata::{PmData, PmError, Result};
use alogs_pyval::{Dict, Value, dict, py_float, py_int};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn err<T>(what: &str) -> Result<T> {
    Err(PmError(what.to_string()))
}

pub const DATA_REL: &str = r"Aniimo_Data\cvs\res\lua\LuaScripts\Common\Data\pmdata.bin";
pub const CYCLE_DROP: &[&str] = &["bossLv"];
pub const BOSS_TABLES: &[&str] = &["puppet_data", "puppet_data1", "puppet_data2", "puppet_data3"];
pub const GRADE_COLUMNS: &[&str] = &["bossHpLossPercent", "bossHpLossPercent_2", "bossHpLossPercent_3"];
pub const CATALOG_LEVELS: &[i64] = &[1, 10, 20, 30, 36, 40, 50, 60, 70, 80];

/// Written whole or not at all: a crash mid-write can't leave a cut-off cache.
pub fn write_json(path: &Path, data: &Value) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, data.json_dumps())?;
    std::fs::rename(&tmp, path)
}

fn table_or_empty(pm: &mut PmData, name: &str) -> Result<Value> {
    let t = pm.table(name)?;
    Ok(if t.truthy() { t } else { Value::Dict(Dict::new()) })
}

/// `d.get(k)` for a value the Python calls `.get` on: a dict, else the AttributeError the Python would raise.
fn get<'a>(d: &'a Value, k: &Value) -> Result<Option<&'a Value>> {
    if matches!(k, Value::List(_) | Value::Dict(_)) {
        return err("TypeError: unhashable type");
    }
    match d {
        Value::Dict(m) => Ok(m.get(k)),
        _ => err("AttributeError: object has no attribute 'get'"),
    }
}

fn get_or_none(d: &Value, k: &Value) -> Result<Value> {
    Ok(get(d, k)?.cloned().unwrap_or(Value::None))
}

/// pmdata.bin of the game that's running: the install the game's own log names (any folder, Steam or not), else the
/// newest copy in the Steam libraries.
pub fn find_game_data(player_log: Option<&Path>) -> Option<PathBuf> {
    let mut running: Vec<String> = Vec::new();
    if let Some(log) = player_log
        && let Ok(bytes) = std::fs::read(log)
    {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(400_000)]).into_owned();
        let re1 = regex::Regex::new(r#"([A-Za-z]:[/\\][^\r\n"']*?)[/\\]Aniimo_Data\b"#).unwrap();
        let re2 = regex::Regex::new(r#"([A-Za-z]:[/\\][^\r\n"']*?steamapps[/\\]common[/\\]Aniimo)[/\\"'\r\n]"#).unwrap();
        running.extend(re1.captures_iter(&head).map(|c| c[1].to_string()));
        running.extend(re2.captures_iter(&head).map(|c| c[1].to_string()));
    }
    let mut libraries: Vec<String> = Vec::new();
    for d in "CDEFGHIJ".chars() {
        for b in [r"\Program Files (x86)\Steam", r"\Program Files\Steam", r"\SteamLibrary", r"\Steam"] {
            libraries.push(format!("{d}:{b}"));
        }
    }
    if let Ok(vdf) = std::fs::read(r"C:\Program Files (x86)\Steam\steamapps\libraryfolders.vdf") {
        let vdf = String::from_utf8_lossy(&vdf);
        let re = regex::Regex::new(r#""path"\s*"([^"]+)""#).unwrap();
        libraries.extend(re.captures_iter(&vdf).map(|c| c[1].replace(r"\\", r"\")));
    }
    let others: Vec<String> = libraries.iter().map(|l| format!(r"{l}\steamapps\common\Aniimo")).collect();
    let found = |dirs: &[String]| -> Vec<PathBuf> {
        let paths: BTreeSet<String> = dirs
            .iter()
            .map(|d| format!(r"{}\{}", d.replace('/', r"\").trim_end_matches('\\'), DATA_REL).to_lowercase())
            .collect();
        paths.into_iter().map(PathBuf::from).filter(|p| p.exists()).collect()
    };
    for group in [found(&running), found(&others)] {
        if !group.is_empty() {
            return group.into_iter().max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
        }
    }
    None
}

fn now_secs() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn py_le(a: &Value, b: &Value) -> Result<bool> {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => Ok(x <= y),
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => Ok(x <= y),
            _ => err("TypeError: '<=' not supported"),
        },
    }
}

/// The week schedule has the week running now (an old copy of the game data doesn't).
pub fn covers_now(cycles: &Value, now: Option<f64>) -> Result<bool> {
    let now = Value::Float(now.filter(|n| *n != 0.0).unwrap_or_else(now_secs));
    let Some(list) = cycles.as_list() else { return Ok(false) };
    for c in list {
        let (s, e) = (get_or_none(c, &"start".into())?, get_or_none(c, &"end".into())?);
        if s.truthy() && e.truthy() && py_le(&s, &now)? && py_le(&now, &e)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn sorted_keys(d: &Value, key: impl Fn(&Value) -> Option<f64>) -> Result<Vec<Value>> {
    let Some(m) = d.as_dict() else { return err("TypeError: not a dict") };
    let mut keyed = Vec::new();
    for k in m.keys() {
        let Some(n) = key(k) else { return err("ValueError: invalid literal") };
        keyed.push((n, k.clone()));
    }
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0)); // stable, like sorted()
    Ok(keyed.into_iter().map(|(_, k)| k).collect())
}

/// `[{'cycle', 'season', 'split', 'start', 'end', 'bossIds', 'bossIds2'}]` from the game data.
pub fn read_cycles(pm: &mut PmData) -> Result<Value> {
    let cycles = table_or_empty(pm, "bossrush_cycle_data")?;
    let stages = table_or_empty(pm, "bossrush_guanka_data")?;
    let boss_id = |species: &Value, key: &str| -> Result<Value> {
        if stages.as_dict().is_none() {
            return Ok(Value::None);
        }
        let entry = match get(&stages, species)? {
            Some(e) if !e.is_none() => Some(e),
            _ => get(&stages, &Value::Str(species.py_str()))?,
        };
        Ok(match entry {
            Some(e @ Value::Dict(_)) => e.get(key).clone(),
            _ => Value::None,
        })
    };
    let mut out = Vec::new();
    for key in sorted_keys(&cycles, |k| py_int(k).map(|i| i as f64))? {
        let c = cycles.as_dict().unwrap().get(&key).unwrap();
        if !matches!(c, Value::Dict(_)) {
            continue;
        }
        let empty = Value::Dict(Dict::new());
        let beg = c.get("begDayTime").or(&empty).clone();
        let end = c.get("endDayTime").or(&empty).clone();
        let mut ids = Vec::new();
        for side in ["bossLeft", "bossMid", "bossRight"] {
            ids.push(boss_id(c.get(side), "bossId")?);
        }
        if !ids.iter().all(Value::truthy) {
            continue;
        }
        let start = match get(&beg, &Value::Int(1))? {
            Some(v) => v.clone(),
            None => get_or_none(&beg, &"1".into())?,
        };
        let stop = match get(&end, &Value::Int(3))? {
            Some(v) => v.clone(),
            None => get_or_none(&end, &"3".into())?,
        };
        let mut ids2 = Vec::new();
        for side in ["bossLeft", "bossMid", "bossRight"] {
            ids2.push(boss_id(c.get(side), "bossId_2")?);
        }
        out.push(dict([
            ("cycle", Value::Str(py_int(&key).unwrap().to_string())),
            ("season", c.get("seasonId").clone()),
            ("split", c.get("isRank").clone()),
            ("start", start),
            ("end", stop),
            ("bossIds", Value::List(ids)),
            ("bossIds2", Value::List(ids2)),
        ]));
    }
    Ok(Value::List(out))
}

/// The ids in a list or a `{1: id, 2: id ...}` dict, None when it holds anything else.
fn id_list(v: &Value) -> Option<BTreeSet<i64>> {
    let items: Vec<&Value> = match v {
        Value::List(l) => l.iter().collect(),
        Value::Dict(d) => d.values().collect(),
        _ => return None,
    };
    items.iter().map(|x| x.as_i64()).collect()
}

/// The Special Offerings a player can pick in each Interlink week: `{week: [buff id ...]}` (bossrush_cycle_data's
/// `specialBuffRef`, the game's own pick list). A week whose list isn't made only of Special Offerings
/// (bossrush_buff_data rows of type "Special") is left out.
pub fn read_offer_pools(pm: &mut PmData) -> Result<Value> {
    let buffs = table_or_empty(pm, "bossrush_buff_data")?;
    let special: BTreeSet<i64> = buffs
        .as_dict()
        .map(|d| d.iter().filter(|(_, r)| r.get("buffType").as_str() == Some("Special")).filter_map(|(k, _)| py_int(k)).collect())
        .unwrap_or_default();
    let cycles = table_or_empty(pm, "bossrush_cycle_data")?;
    let mut out = Dict::new();
    let Some(rows) = cycles.as_dict() else { return Ok(Value::Dict(out)) };
    for key in sorted_keys(&cycles, |k| py_int(k).map(|i| i as f64))? {
        let Some(n) = py_int(&key) else { continue };
        if let Some(ids) = id_list(rows.get(&key).unwrap().get("specialBuffRef")).filter(|ids| !ids.is_empty() && ids.is_subset(&special)) {
            out.insert(Value::Str(n.to_string()), Value::List(ids.iter().map(|i| Value::Int(*i)).collect()));
        }
    }
    Ok(Value::Dict(out))
}

/// Text id -> English, from the game's text tables (LuaScripts.xdf, a zip next to the Lua data folder).
pub struct EnglishText {
    text: Vec<u8>,
    at: Value,
}

impl EnglishText {
    pub fn open(pm_path: &Path) -> Result<EnglishText> {
        let up = (0..4).try_fold(pm_path.to_path_buf(), |p, _| p.parent().map(Path::to_path_buf));
        let Some(dir) = up else { return err("no LuaScripts.xdf") };
        let io = |e: std::io::Error| PmError(e.to_string());
        let file = std::fs::File::open(dir.join("LuaScripts.xdf")).map_err(io)?;
        let mut z = zip::ZipArchive::new(file).map_err(|e| PmError(e.to_string()))?;
        // the zip's own order, as namelist() gives it
        let mut ordered: Vec<(usize, String)> = Vec::new();
        for i in 0..z.len() {
            if let Ok(f) = z.by_index_raw(i) {
                ordered.push((i, f.name().to_string()));
            }
        }
        let Some((_, name)) = ordered.iter().find(|(_, n)| n.to_lowercase().ends_with("i18n/compress_en.bin")) else {
            return err("StopIteration");
        };
        let read = |z: &mut zip::ZipArchive<std::fs::File>, n: &str| -> Result<Vec<u8>> {
            use std::io::Read;
            let mut f = z.by_name(n).map_err(|e| PmError(format!("KeyError: {e}")))?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).map_err(io)?;
            Ok(buf)
        };
        let text = read(&mut z, name)?;
        let map_name = format!("{}NewTextMap_en.json", &name[..name.len() - "Compress_en.bin".len()]);
        let map = read(&mut z, &map_name)?;
        let at = Value::json_loads(&String::from_utf8_lossy(&map)).ok_or_else(|| PmError("JSONDecodeError".into()))?;
        Ok(EnglishText { text, at })
    }

    pub fn en(&self, text_id: &Value) -> Result<String> {
        let Some(id) = text_id.as_i64() else { return Ok(String::new()) };
        let key = Value::Str((id & 0xffff_ffff).to_string());
        let at = get_or_none(&self.at, &key)?;
        if !at.truthy() {
            return Ok(String::new());
        }
        let l = at.as_list().ok_or_else(|| PmError("TypeError".into()))?;
        let (Some(start), Some(len)) = (l.first().and_then(Value::as_i64), l.get(1).and_then(Value::as_i64)) else {
            return err("TypeError");
        };
        let n = self.text.len() as i64;
        let clip = |x: i64| if x < 0 { (x + n).max(0) } else { x.min(n) } as usize;
        let (a, b) = (clip(start), clip(start + len));
        Ok(if a < b { String::from_utf8_lossy(&self.text[a..b]).into_owned() } else { String::new() })
    }
}

/// Boss rows like boss_catalog.csv's (the fields the meter uses) from the game data: every boss-type creature
/// (monsterType 2) in the four puppet tables, plus the `(table, puppetId)` pairs in `keep`.
pub fn read_bosses(path: &Path, keep: &[(String, String)]) -> Result<Value> {
    let mut pm = PmData::open(path).map_err(|e| PmError(e.to_string()))?;
    let en = EnglishText::open(path)?;
    let props = table_or_empty(&mut pm, "property_data")?;
    let stages = table_or_empty(&mut pm, "bossrush_guanka_data")?;
    let rush = rush_set(&stages, false)?;
    let mut out = Vec::new();
    for table in BOSS_TABLES {
        let t = table_or_empty(&mut pm, table)?;
        let Some(rows) = t.as_dict() else { return err("AttributeError: 'list' object has no attribute 'items'") };
        for (pid, r) in rows.iter() {
            if !matches!(r, Value::Dict(_)) || !r.get("propId").truthy() {
                continue;
            }
            let Some(pid) = py_int(pid) else { return err("ValueError: invalid literal for int()") };
            let pid = pid.to_string();
            if !r.get("monsterType").py_eq(&Value::Int(2)) && !keep.iter().any(|(kt, kp)| kt == table && *kp == pid) {
                continue;
            }
            let prop_id = r.get("propId");
            let p = match get(&props, prop_id)? {
                Some(v) => v.clone(),
                None => get_or_none(&props, &Value::Str(prop_id.py_str()))?,
            };
            let p = if p.truthy() { p } else { Value::Dict(Dict::new()) };
            let rate = get_or_none(&p, &"fixed_hp_rate".into())?;
            let fixed = match &rate {
                Value::Dict(d) => Value::Dict(d.iter().map(|(k, v)| (Value::Str(k.py_str()), v.clone())).collect())
                    .json_dumps(),
                _ => String::new(),
            };
            let s = |k: &str, default: &str| match r.as_dict().unwrap().get_str(k) {
                Some(v) => v.py_str(),
                None => default.to_string(),
            };
            let adj = get_or_none(&p, &"species_hp_max_v".into())?;
            out.push(dict([
                ("puppetId", Value::Str(pid.clone())),
                ("table", Value::Str((*table).to_string())),
                ("uiName", r.get("uiName").or(&Value::Str(String::new())).clone()),
                ("name_en", Value::Str(en.en(r.get("name"))?)),
                ("bossTitle_en", Value::Str(en.en(r.get("BossShowName"))?)),
                ("monsterType", Value::Str(s("monsterType", ""))),
                ("bossType", Value::Str(s("bossType", ""))),
                ("lvType", Value::Str(s("lvType", ""))),
                ("propId", Value::Str(prop_id.py_str())),
                ("species_hp", Value::Str(s("species_hp_max_v", ""))),
                ("prop_species_adj", Value::Str(adj.or(&Value::Int(0)).py_str())),
                ("fixed_hp_rate", Value::Str(fixed)),
                ("in_boss_rush", Value::Str(if rush.contains(&pid) { "yes" } else { "" }.into())),
            ]));
        }
    }
    Ok(Value::List(out))
}

/// The stages' boss ids as strings: `str(v)` (`str(int(v))` with `as_int`) of every truthy bossId and bossId_2.
fn rush_set(stages: &Value, as_int: bool) -> Result<BTreeSet<String>> {
    let mut rush = BTreeSet::new();
    let Some(st) = stages.as_dict() else { return err("AttributeError: 'list' object has no attribute 'values'") };
    for s in st.values() {
        if !matches!(s, Value::Dict(_)) {
            continue;
        }
        for k in ["bossId", "bossId_2"] {
            let v = s.get(k);
            if v.truthy() {
                rush.insert(if as_int {
                    py_int(v).ok_or_else(|| PmError("ValueError".into()))?.to_string()
                } else {
                    v.py_str()
                });
            }
        }
    }
    Ok(rush)
}

/// Every Interlink boss id in the game data: bossrush_guanka_data bossId, plus bossId_2 where a stage has a second
/// boss.
pub fn boss_rush_ids(pm: &mut PmData) -> Result<BTreeSet<String>> {
    let stages = table_or_empty(pm, "bossrush_guanka_data")?;
    rush_set(&stages, true)
}

fn read_cache(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    Value::json_loads(&text)
}

fn stamp(path: &Path, prefix: &str) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64();
    Some(format!("{prefix}{}:{}", m.len(), mtime.trunc() as i64))
}

/// The boss list from the game data, decoded again only when the game's data file changed.
/// Returns `(rows, changed)` or `(None, false)`.
pub fn cached_bosses(cache: &Path, keep: &[(String, String)], player_log: Option<&Path>) -> Result<(Option<Value>, bool)> {
    let Some(path) = find_game_data(player_log) else { return Ok((None, false)) };
    let Some(st) = stamp(&path, "v2:") else { return err("OSError") }; // (v2: shared values read where the game reads them)
    if let Some(c) = read_cache(cache)
        && c.get("stamp").py_eq(&Value::Str(st.clone()))
        && let Some(b) = c.as_dict().and_then(|d| d.get_str("bosses"))
    {
        return Ok((Some(b.clone()), false));
    }
    let bosses = read_bosses(&path, keep)?;
    if bosses.as_list().map_or(0, Vec::len) < 50 {
        return Ok((None, false)); // a decode that went wrong: keep the shipped list
    }
    let _ = write_json(cache, &dict([("stamp", Value::Str(st)), ("bosses", bosses.clone())]));
    Ok((Some(bosses), true))
}

/// The schedule, decoded again only when the game's data file changed. Returns `(cycles, changed)` or `(None, false)`.
pub fn cached_cycles(cache: &Path, player_log: Option<&Path>) -> Result<(Option<Value>, bool)> {
    let Some(path) = find_game_data(player_log) else { return Ok((None, false)) };
    // (v4: the bots' names; v5: shared values read where the game reads them: seasons, splits, pick lists)
    let Some(st) = stamp(&path, "v5:") else { return err("OSError") };
    if let Some(c) = read_cache(cache)
        && c.get("stamp").py_eq(&Value::Str(st.clone()))
        && let Some(Value::List(list)) = c.as_dict().and_then(|d| d.get_str("cycles"))
        && let Some(out) = list
            .iter()
            .map(|cy| {
                cy.as_dict().map(|d| {
                    Value::Dict(
                        d.iter()
                            .filter(|(k, _)| !CYCLE_DROP.iter().any(|x| k.py_eq(&(*x).into())))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    )
                })
            })
            .collect::<Option<Vec<_>>>()
    {
        // a cache from an aLogs that still kept bossLv: the same schedule without it
        return Ok((Some(Value::List(out)), false));
    }
    let mut pm = PmData::open(&path).map_err(|e| PmError(e.to_string()))?;
    let cycles = read_cycles(&mut pm)?;
    if !cycles.truthy() {
        return Ok((None, false));
    }
    let rules = read_rush_rules(&path)?;
    let ids: Vec<i64> = rules.get("bots").as_list().map(|l| l.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
    let names = read_bot_names(&path, &ids).unwrap_or_default();
    let _ = write_json(
        cache,
        &dict([
            ("stamp", Value::Str(st)),
            ("cycles", cycles.clone()),
            ("rules", rules),
            ("bot_names", Value::List(names.into_iter().map(Value::Str).collect())),
        ]),
    );
    Ok((Some(cycles), true))
}

/// The Interlink bots' names (the AI teammates that fill a party: sys_config_data BossRushPlayerBotList), in English,
/// from avatar_robot_data's rows for those ids. Ids without a row or a name are left out.
pub fn read_bot_names(path: &Path, ids: &[i64]) -> Result<Vec<String>> {
    let mut pm = PmData::open(path).map_err(|e| PmError(e.to_string()))?;
    let en = EnglishText::open(path)?;
    let bots = table_or_empty(&mut pm, "avatar_robot_data")?;
    let mut out = Vec::new();
    for id in ids {
        let row = match get(&bots, &Value::Int(*id))? {
            Some(r) => r.clone(),
            None => get_or_none(&bots, &Value::Str(id.to_string()))?,
        };
        if !matches!(row, Value::Dict(_)) {
            continue;
        }
        let name = en.en(row.get("name"))?;
        if !name.trim().is_empty() && !out.contains(&name) {
            out.push(name.trim().to_string());
        }
    }
    Ok(out)
}

/// Interlink's rules from the game's own config (sys_config_data): the side bosses' full rating, the center boss's
/// multiplier, the fight's length and the AI teammates' ids. `{}` when the table can't be read.
pub fn read_rush_rules(path: &Path) -> Result<Value> {
    let cfg = match PmData::open(path).map_err(|e| PmError(e.to_string())).and_then(|mut pm| table_or_empty(&mut pm, "sys_config_data")) {
        Ok(c) => c,
        Err(_) => return Ok(Value::Dict(Dict::new())),
    };
    rush_rules_from(&cfg)
}

pub fn rush_rules_from(cfg: &Value) -> Result<Value> {
    let side = get_or_none(cfg, &"BossRushMaxStarSideBoss".into())?;
    let mult = get_or_none(cfg, &"BossRushStarMultiplierMidBoss".into())?;
    let bots = get_or_none(cfg, &"BossRushPlayerBotList".into())?;
    let center = match (side.as_i64(), mult.as_i64()) {
        (Some(a), Some(b)) if side.is_int() && mult.is_int() => Value::Int(a * b),
        _ => Value::None,
    };
    let bots = match &bots {
        Value::Dict(d) => Value::List(py_sorted(d.values().cloned().collect())?),
        _ => Value::None,
    };
    let duration = get_or_none(cfg, &"BossRushCombatDuration".into())?;
    let mut out = Dict::new();
    for (k, v) in [("starSide", side), ("starCenter", center), ("duration", duration), ("bots", bots)] {
        if !v.is_none() {
            out.insert(k.into(), v);
        }
    }
    Ok(Value::Dict(out))
}

/// `sorted(values)` for numbers or strings (a mix raises TypeError, as in Python).
fn py_sorted(mut v: Vec<Value>) -> Result<Vec<Value>> {
    if v.iter().all(Value::is_number) {
        v.sort_by(|a, b| a.as_f64().unwrap().total_cmp(&b.as_f64().unwrap()));
    } else if v.iter().all(|x| x.as_str().is_some()) {
        v.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    } else if v.len() > 1 {
        return err("TypeError: '<' not supported");
    }
    Ok(v)
}

/// `[{'grade', 'name', 'bossHpLossPercent', ...}]` by grade from a bossrush_grade_data table; `[]` when the table isn't
/// a ladder the meter can use (grades 0..N in order, every column a number from 0 up to 1, never going down).
pub fn grade_rows(table: &Value) -> Result<Value> {
    let empty = Value::Dict(Dict::new());
    let table = table.or(&empty);
    let mut rows: Vec<Value> = Vec::new();
    for k in sorted_keys(table, |k| py_float(k).map(f64::trunc))? {
        let r = table.as_dict().unwrap().get(&k).unwrap();
        if !matches!(r, Value::Dict(_)) {
            return Ok(Value::List(vec![]));
        }
        let base = r.get("bossHpLossPercent").clone();
        let name = r.get("gradeName").or(r.get("name")).or(&Value::Str(String::new())).py_str();
        let mut row = vec![("grade", Value::Int(py_float(&k).unwrap().trunc() as i64)), ("name", Value::Str(name))];
        for c in GRADE_COLUMNS {
            let v = r.get(c);
            row.push((c, if v.is_none() { base.clone() } else { v.clone() }));
        }
        rows.push(dict(row));
    }
    let grades: Vec<i64> = rows.iter().map(|r| r.get("grade").as_i64().unwrap()).collect();
    if rows.len() < 3 || grades != (0..rows.len() as i64).collect::<Vec<_>>() {
        return Ok(Value::List(vec![]));
    }
    for c in GRADE_COLUMNS {
        let v: Vec<&Value> = rows.iter().map(|r| r.get(c)).collect();
        if !v.iter().all(|x| matches!(x, Value::Int(_) | Value::Float(_))) {
            return Ok(Value::List(vec![]));
        }
        let f: Vec<f64> = v.iter().map(|x| x.as_f64().unwrap()).collect();
        if f[0] != 0.0 || f[f.len() - 1] != 1.0 || f.windows(2).any(|w| w[1] < w[0]) {
            return Ok(Value::List(vec![]));
        }
    }
    Ok(Value::List(rows))
}

pub fn read_grades(pm: &mut PmData) -> Result<Value> {
    let t = pm.table("bossrush_grade_data")?;
    grade_rows(&t)
}

fn by_grade(rows: &[Value]) -> Result<Value> {
    let mut d = Dict::new();
    for r in rows {
        let g = get_or_none(r, &"grade".into())?;
        d.insert(g, r.clone());
    }
    Ok(Value::Dict(d))
}

/// Interlink's HP bars from the game data, decoded again only when the game's data file changed.
pub fn cached_grades(cache: &Path, player_log: Option<&Path>) -> Result<(Option<Value>, bool)> {
    let Some(path) = find_game_data(player_log) else { return Ok((None, false)) };
    let Some(st) = stamp(&path, "") else { return err("OSError") };
    if let Some(c) = read_cache(cache)
        && c.get("stamp").py_eq(&Value::Str(st.clone()))
        && let Some(Value::List(g)) = c.as_dict().and_then(|d| d.get_str("grades"))
        && let Ok(t) = by_grade(g)
        && grade_rows(&t).is_ok_and(|r| r.truthy())
    {
        return Ok((Some(Value::List(g.clone())), false));
    }
    let mut pm = PmData::open(&path).map_err(|e| PmError(e.to_string()))?;
    let rows = read_grades(&mut pm)?;
    if !rows.truthy() {
        return Ok((None, false));
    }
    let _ = write_json(cache, &dict([("stamp", Value::Str(st)), ("grades", rows.clone())]));
    Ok((Some(rows), true))
}

fn read_csv(path: &Path) -> Result<Vec<Value>> {
    let mut rd = csv::Reader::from_path(path).map_err(|e| PmError(e.to_string()))?;
    let headers = rd.headers().map_err(|e| PmError(e.to_string()))?.clone();
    let mut out = Vec::new();
    for rec in rd.records() {
        let rec = rec.map_err(|e| PmError(e.to_string()))?;
        out.push(dict(headers.iter().zip(rec.iter()).map(|(h, v)| (h, Value::Str(v.to_string())))));
    }
    Ok(out)
}

/// boss_rush_grades.csv as grade_rows gives them (the columns as numbers).
pub fn shipped_grades(csv_path: &Path) -> Result<Value> {
    let mut rows = Vec::new();
    for r in read_csv(csv_path)? {
        let need = |k: &str| r.as_dict().unwrap().get_str(k).cloned().ok_or_else(|| PmError(format!("KeyError: {k:?}")));
        let grade = alogs_pyval::parse_int(need("grade")?.as_str().unwrap()).ok_or_else(|| PmError("ValueError".into()))?;
        let mut row = vec![("grade", Value::Int(grade)), ("name", need("name")?)];
        for c in GRADE_COLUMNS {
            let f = py_float(&need(c)?).ok_or_else(|| PmError("ValueError".into()))?;
            row.push((c, Value::Float(f)));
        }
        rows.push(dict(row));
    }
    grade_rows(&by_grade(&rows)?)
}

/// What differs between the shipped boss_rush_grades.csv and the game data's table, as lines.
pub fn grades_drift(pm: Option<&mut PmData>, csv_path: &Path) -> Result<Vec<String>> {
    let shipped = match shipped_grades(csv_path) {
        Ok(s) => s,
        Err(e) => return Ok(vec![format!("boss_rush_grades.csv unreadable: {e}")]),
    };
    if !shipped.truthy() {
        return Ok(vec!["boss_rush_grades.csv is not a 0-to-1 ladder of grades 0..N with the three columns".into()]);
    }
    let Some(pm) = pm else { return Ok(vec![]) };
    let game = read_grades(pm)?;
    if !game.truthy() {
        return Ok(vec!["the game data's bossrush_grade_data is not a 0-to-1 ladder".into()]);
    }
    let (s, g) = (shipped.as_list().unwrap(), game.as_list().unwrap());
    let mut out = Vec::new();
    if s.len() != g.len() {
        out.push(format!("{} grades shipped, {} in the game data", s.len(), g.len()));
    }
    for (a, b) in s.iter().zip(g) {
        let diff: Vec<String> = GRADE_COLUMNS
            .iter()
            .filter(|c| a.get(c).as_f64() != b.get(c).as_f64())
            .map(|c| format!("{c} {} -> {}", a.get(c).repr(), b.get(c).repr()))
            .collect();
        if !diff.is_empty() {
            out.push(format!("grade {}: {}", a.get("grade").repr(), diff.join("; ")));
        }
    }
    Ok(out)
}

/// boss_catalog.csv's rows: read_bosses' plus the max HP at a few levels.
pub fn catalog_rows(path: &Path, keep: &[(String, String)]) -> Result<Value> {
    let rows = read_bosses(path, keep)?;
    let mut out = Vec::new();
    for r in rows.as_list().unwrap() {
        let mut d = r.as_dict().unwrap().clone();
        let rate: Option<serde_json::Value> = serde_json::from_str(r.get("fixed_hp_rate").as_str().unwrap()).ok();
        let f = |k: &str| -> Result<f64> {
            let v = r.get(k);
            if !v.truthy() {
                return Ok(0.0);
            }
            py_float(v).ok_or_else(|| PmError(format!("ValueError: could not convert string to float: {}", v.repr())))
        };
        let species = f("species_hp")? + f("prop_species_adj")?;
        let truthy = rate.as_ref().is_some_and(|j| match j {
            serde_json::Value::Null => false,
            serde_json::Value::Bool(b) => *b,
            serde_json::Value::Number(n) => n.as_f64() != Some(0.0),
            serde_json::Value::String(s) => !s.is_empty(),
            serde_json::Value::Array(a) => !a.is_empty(),
            serde_json::Value::Object(o) => !o.is_empty(),
        });
        for lv in CATALOG_LEVELS {
            let hp = if truthy { hpcalc::max_hp(species, rate.as_ref().unwrap(), *lv, 0.0) } else { None };
            d.insert(Value::Str(format!("maxHP_lv{lv}")), Value::Str(hp.map(|h| h.to_string()).unwrap_or_default()));
        }
        out.push(Value::Dict(d));
    }
    Ok(Value::List(out))
}

pub fn shipped_catalog(csv_path: &Path) -> Result<Vec<Value>> {
    read_csv(csv_path)
}

fn keep_of(shipped: &[Value]) -> Vec<(String, String)> {
    shipped
        .iter()
        .filter(|r| !r.get("monsterType").py_eq(&"2".into()))
        .map(|r| (r.get("table").py_str(), r.get("puppetId").py_str()))
        .collect()
}

/// What differs between the shipped boss list and the game data, as lines (empty: nothing).
pub fn catalog_drift(path: Option<&Path>, csv_path: &Path, cycles_path: Option<&Path>) -> Result<Vec<String>> {
    let shipped = shipped_catalog(csv_path)?;
    let mut out = Vec::new();
    let Some(path) = path else {
        let cp = cycles_path.map(Path::to_path_buf).unwrap_or_else(|| csv_path.with_file_name("boss_rush_cycles.json"));
        let text = std::fs::read_to_string(&cp).map_err(|e| PmError(e.to_string()))?;
        let cycles = Value::json_loads(&text).ok_or_else(|| PmError("JSONDecodeError".into()))?;
        let marked: BTreeSet<String> = shipped
            .iter()
            .filter(|r| r.get("table").py_eq(&"puppet_data".into()) && r.get("in_boss_rush").py_eq(&"yes".into()))
            .map(|r| r.get("puppetId").py_str())
            .collect();
        for c in cycles.as_list().ok_or_else(|| PmError("TypeError".into()))? {
            let mut ids: Vec<Value> = c.get("bossIds").as_list().cloned().unwrap_or_default();
            if let Some(two) = c.get("bossIds2").as_list() {
                ids.extend(two.iter().filter(|x| x.truthy()).cloned());
            }
            for b in ids {
                if !marked.contains(&b.py_str()) {
                    out.push(format!("week {}: boss {} has no puppet_data row marked in_boss_rush", c.get("cycle").py_str(), b.py_str()));
                }
            }
        }
        return Ok(out);
    };
    let mut pm = PmData::open(path).map_err(|e| PmError(e.to_string()))?;
    let rush = boss_rush_ids(&mut pm)?;
    for r in &shipped {
        let pid = r.get("puppetId").py_str();
        let yes = r.get("in_boss_rush").py_eq(&"yes".into());
        if yes != rush.contains(&pid) {
            out.push(format!(
                "{} {} {}: in_boss_rush {}, the game's stages {} it",
                r.get("table").py_str(),
                pid,
                r.get("bossTitle_en").py_str(),
                r.get("in_boss_rush").repr(),
                if rush.contains(&pid) { "have" } else { "do not have" }
            ));
        }
    }
    let have_pd: BTreeSet<String> =
        shipped.iter().filter(|r| r.get("table").py_eq(&"puppet_data".into())).map(|r| r.get("puppetId").py_str()).collect();
    for b in rush.difference(&have_pd) {
        out.push(format!("boss {b} is in an Interlink stage but has no puppet_data row"));
    }
    let keep = keep_of(&shipped);
    let key = |r: &Value| (r.get("table").py_str(), r.get("puppetId").py_str());
    let mut game = std::collections::BTreeMap::new();
    for r in catalog_rows(path, &keep)?.as_list().unwrap() {
        game.insert(key(r), r.clone());
    }
    let mut have = std::collections::BTreeMap::new();
    for r in &shipped {
        have.insert(key(r), r.clone());
    }
    let all: BTreeSet<_> = game.keys().chain(have.keys()).cloned().collect();
    for k in all {
        match (have.get(&k), game.get(&k)) {
            (None, _) => out.push(format!("{} {}: in the game data, not in the shipped list", k.0, k.1)),
            (_, None) => out.push(format!("{} {}: in the shipped list, not in the game data", k.0, k.1)),
            (Some(a), Some(b)) => {
                let diff: Vec<String> = b
                    .as_dict()
                    .unwrap()
                    .iter()
                    .filter(|(f, v)| !a.as_dict().unwrap().get(f).is_some_and(|x| x.py_eq(v)) && !f.py_eq(&"in_boss_rush".into()))
                    .map(|(f, v)| {
                        let old = a.as_dict().unwrap().get(f).cloned().unwrap_or(Value::None);
                        format!("{} {} -> {}", f.py_str(), old.repr(), v.repr())
                    })
                    .collect();
                if !diff.is_empty() {
                    out.push(format!("{} {} {}: {}", k.0, k.1, a.get("bossTitle_en").py_str(), diff[..diff.len().min(4)].join("; ")));
                }
            }
        }
    }
    Ok(out)
}
