//! What goes to the AniiLogs leaderboards: each record's payload from a saved run, the lineup pairing, the last check
//! before sending, and the notice that says what was sent. Port of the payload half of aLogs B16 `bossbar/submit.py`.

use crate::team::{Teammates, casefold};
use alogs_gamelog::Clock;
use alogs_gamelog::difflib::{SequenceMatcher, ratio};
use alogs_pyval::{Dict, Value, dict};
use alogs_reader::boss::Schedule;
use std::collections::{BTreeSet, HashMap, HashSet};

pub const LINEUP_ALIKE: f64 = 0.5;
pub const LINEUP_WEAK: f64 = 0.35;
pub const LINEUP_CLEAR: f64 = 0.15;

/// File-name-safe text, at most 120 characters.
pub fn safe(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '_' }).take(120).collect()
}

fn cut(v: &Value, n: usize) -> Value {
    let s: String = v.as_str().map_or_else(|| if v.truthy() { v.py_str() } else { String::new() }, String::from).chars().take(n).collect();
    if s.is_empty() { Value::None } else { Value::Str(s) }
}
fn or_none<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if a.truthy() { a } else { b }
}
fn no_nones(pairs: Vec<(&str, Value)>) -> Value {
    dict(pairs.into_iter().filter(|(_, v)| !v.is_none()))
}
fn extend(d: &mut Dict, other: Value) {
    if let Value::Dict(o) = other {
        for (k, v) in o.iter() {
            d.insert(k.clone(), v.clone());
        }
    }
}

/// `{'slot', 'split'}` of a run's week (whichever is known).
pub fn slot_of(sched: &Schedule, cycle: &Value, boss_id: &Value) -> Value {
    let c = if cycle.is_none() { None } else { Some(cycle.py_str()) };
    let (slot, split) = sched.cycle_slot(c.as_deref(), &boss_id.py_str());
    no_nones(vec![("slot", slot.map_or(Value::None, |s| Value::Str(s.into()))), ("split", Value::from_json(&split))])
}

fn screen_checks(result: &Value, bd: &Value) -> Value {
    let checks = no_nones(vec![
        ("result", Some(result.get("screen_check")).filter(|v| v.truthy()).cloned().unwrap_or(Value::None)),
        ("battleData", Some(bd.get("screen_check")).filter(|v| v.truthy()).cloned().unwrap_or(Value::None)),
    ]);
    if checks.truthy() { dict([("screenCheck", checks)]) } else { Value::Dict(Dict::new()) }
}

fn fold(name: &str) -> String {
    Teammates::fold(name)
}

/// Each Party member (by index) paired with the run's result-screen player it is: the name its lineup card goes by.
pub fn lineup_names(party: &[Value], players: &[Value]) -> HashMap<usize, String> {
    let sim = |a: &str, b: &str| ratio(&fold(a), &fold(b));
    let run = |a: &str, b: &str| SequenceMatcher::new(&fold(a), &fold(b)).matching_blocks().iter().map(|x| x.2).max().unwrap_or(0);
    let cards: Vec<usize> = (0..party.len()).filter(|&i| matches!(party[i], Value::Dict(_))).collect();
    let rows: Vec<usize> = (0..players.len()).filter(|&i| matches!(players[i], Value::Dict(_)) && players[i].get("name").truthy()).collect();
    let name = |v: &Value| v.get("name").as_str().unwrap_or("").to_string();
    let mut pair: HashMap<usize, usize> = HashMap::new();
    let mut used: HashSet<usize> = HashSet::new();
    let free_cards = |pair: &HashMap<usize, usize>| cards.iter().copied().filter(|m| !pair.contains_key(m) && party[*m].get("name").truthy()).collect::<Vec<_>>();
    let free_rows = |used: &HashSet<usize>| rows.iter().copied().filter(|p| !used.contains(p)).collect::<Vec<_>>();
    let me = rows.iter().copied().find(|&p| players[p].get("self").truthy());
    let mine = cards.iter().copied().find(|&m| party[m].get("self").truthy());
    if let (Some(me), Some(mine)) = (me, mine) {
        pair.insert(mine, me);
        used.insert(me);
    }
    loop {
        let mut best: Option<(f64, bool, usize, usize)> = None;
        for m in free_cards(&pair) {
            for p in free_rows(&used) {
                let k = (sim(&name(&party[m]), &name(&players[p])), party[m].get("slot").py_eq(players[p].get("slot")));
                if best.is_none_or(|b| k > (b.0, b.1)) {
                    best = Some((k.0, k.1, m, p));
                }
            }
        }
        match best {
            Some(b) if b.0 >= LINEUP_ALIKE => {
                pair.insert(b.2, b.3);
                used.insert(b.3);
            }
            _ => break,
        }
    }
    loop {
        let mut found: Option<(f64, usize, usize)> = None;
        let (fc, fr) = (free_cards(&pair), free_rows(&used));
        for &m in &fc {
            for &p in &fr {
                let s = sim(&name(&party[m]), &name(&players[p]));
                let mut near: Vec<f64> = fr.iter().filter(|&&q| q != p).map(|&q| sim(&name(&party[m]), &name(&players[q]))).collect();
                near.extend(fc.iter().filter(|&&o| o != m).map(|&o| sim(&name(&party[o]), &name(&players[p]))));
                if s >= LINEUP_WEAK && run(&name(&party[m]), &name(&players[p])) >= 2 && near.iter().all(|o| s >= o + LINEUP_CLEAR) && found.is_none_or(|f| s > f.0) {
                    found = Some((s, m, p));
                }
            }
        }
        let Some((_, m, p)) = found else { break };
        pair.insert(m, p);
        used.insert(p);
    }
    for &m in &cards {
        let p = free_rows(&used).into_iter().find(|&p| players[p].get("slot").py_eq(party[m].get("slot")));
        if let Some(p) = p
            && !pair.contains_key(&m)
            && players[p].get("sure").truthy()
            && name(&players[p]).chars().count() == 1
        {
            pair.insert(m, p);
            used.insert(p);
        }
    }
    let mut out: HashMap<usize, String> = HashMap::new();
    for &m in &cards {
        let mut n = name(&party[m]);
        if let Some(&p) = pair.get(&m) {
            let got = name(&players[p]);
            if got != n {
                log::info!(target: "meter", "lineup: slot {}'s {:?} sent as the result screen's {:?}", party[m].get("slot").as_i64().unwrap_or(0) + 1, n, got);
            }
            n = got;
        }
        out.insert(m, n);
    }
    let mut order = cards.clone();
    let pill_off = |m: usize| {
        let d = party[m].as_dict().unwrap();
        let pill = d.get_str("pill").unwrap_or(d.at("slot"));
        !pill.py_eq(d.at("slot"))
    };
    order.sort_by_key(|&m| (!pair.contains_key(&m), pill_off(m)));
    let mut seen: HashMap<String, i64> = HashMap::new();
    for m in order {
        let key = fold(&out[&m]);
        if key.is_empty() {
            continue;
        }
        let slot = party[m].get("slot").as_i64().unwrap_or(0);
        if let Some(s) = seen.get(&key) {
            log::info!(target: "meter", "lineup: {:?} is slot {}'s already: slot {} sent unnamed", out[&m], s + 1, slot + 1);
            out.insert(m, String::new());
        } else {
            seen.insert(key, slot);
        }
    }
    out
}

/// Every Aniimo's name (case-folded) from the icon cache's heads, or None when it can't be read.
pub fn species_names(icons_json: &std::path::Path) -> Option<BTreeSet<String>> {
    let v = Value::json_loads(&std::fs::read_to_string(icons_json).ok()?)?;
    let heads = v.get("heads").as_dict()?;
    let s: BTreeSet<String> = heads.values().filter(|n| n.truthy()).map(|n| casefold(n.py_str().trim())).collect();
    if s.is_empty() { None } else { Some(s) }
}

fn send_fold(name: &Value) -> String {
    fold(&name.py_str().split_whitespace().collect::<String>())
}

fn one_each(rows: &mut [Value], what: &str, run: &str) {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&i| (!(matches!(rows[i], Value::Dict(_)) && rows[i].get("self").truthy()), i));
    let mut seen: HashSet<String> = HashSet::new();
    for i in order {
        let r = &rows[i];
        if !matches!(r, Value::Dict(_)) || !r.get("name").truthy() || r.get("ai").truthy() {
            continue;
        }
        let k = send_fold(r.get("name"));
        if seen.contains(&k) {
            log::info!(target: "meter", "send check: {what} row {} of {run} has a name another row has: sent unnamed", i + 1);
            let d: Dict = r.as_dict().unwrap().iter().filter(|(k, _)| k.as_str() != Some("name")).map(|(a, b)| (a.clone(), b.clone())).collect();
            rows[i] = Value::Dict(d);
        } else {
            seen.insert(k);
        }
    }
}

/// `payload` as it may go out: players and lineup slots named once each, every species one from the game's list.
pub fn send_check(payload: &Value, species: Option<&BTreeSet<String>>) -> Value {
    let mut out = Value::json_loads(&payload.json_dumps()).unwrap();
    let run = format!("{} {}", out.get("kind").py_str(), out.get("clientRunId").py_str());
    let Value::Dict(o) = &mut out else { return out };
    if let Some(Value::List(mut players)) = o.get_str("players").cloned() {
        one_each(&mut players, "player", &run);
        o.insert("players".into(), Value::List(players));
    }
    let mut extra = match o.get_str("extra") {
        Some(Value::Dict(e)) => e.clone(),
        _ => Dict::new(),
    };
    let had_extra = matches!(o.get_str("extra"), Some(Value::Dict(_)));
    let mut lineup = match extra.get_str("lineup") {
        Some(Value::List(l)) => Some(l.clone()),
        _ => None,
    };
    if let Some(l) = lineup.as_mut().filter(|l| !l.is_empty()) {
        one_each(l, "lineup", &run);
    }
    if let Some(species) = species.filter(|s| !s.is_empty()) {
        let ok = |n: &Value| n.as_str().is_some_and(|s| species.contains(&casefold(s.trim())));
        if let Some(l) = lineup.as_mut() {
            for (i, e) in l.iter_mut().enumerate() {
                let Value::Dict(d) = e else { continue };
                let a = d.at("aniimo").clone();
                if a.truthy() && !ok(&a) {
                    log::info!(target: "meter", "send check: lineup slot {} of {run}: its lead isn't an Aniimo's name: kept as its nickname only", i + 1);
                    if d.get_str("nickname").is_none() {
                        d.insert("nickname".into(), a);
                    }
                    *d = d.iter().filter(|(k, _)| k.as_str() != Some("aniimo")).map(|(k, v)| (k.clone(), v.clone())).collect();
                }
                if let Some(Value::List(team)) = d.get_str("team").cloned() {
                    let kept: Vec<Value> = team.iter().filter(|m| matches!(m, Value::Dict(_)) && ok(m.get("aniimo"))).cloned().collect();
                    if kept.len() != team.len() {
                        log::info!(target: "meter", "send check: lineup slot {} of {run}: {} team member(s) not an Aniimo: left out", i + 1, team.len() - kept.len());
                    }
                    if kept.is_empty() {
                        *d = d.iter().filter(|(k, _)| k.as_str() != Some("team")).map(|(k, v)| (k.clone(), v.clone())).collect();
                    } else {
                        d.insert("team".into(), Value::List(kept));
                    }
                }
            }
        }
        if let Some(Value::Dict(setup)) = extra.get_str("setup").cloned()
            && let Some(Value::List(team)) = setup.get_str("team")
        {
            let mut kept = Vec::new();
            for m in team {
                if matches!(m, Value::Dict(_)) && m.get("name").truthy() && !ok(m.get("name")) {
                    log::info!(target: "meter", "send check: a Holo-Battle Sim team member of {run} isn't an Aniimo: left out");
                    continue;
                }
                kept.push(m.clone());
            }
            let mut s = setup.clone();
            s.insert("team".into(), Value::List(kept));
            extra.insert("setup".into(), Value::Dict(s));
        }
    }
    if let Some(l) = lineup {
        extra.insert("lineup".into(), Value::List(l));
    }
    if had_extra {
        o.insert("extra".into(), Value::Dict(extra));
    }
    out
}

/// `{'lineup': [...]}` from the Party screen's members and their teams, for a run's extra.
pub fn lineup(summary: &Value) -> Value {
    let teams = summary.get("teams").as_list().cloned().unwrap_or_default();
    let party = summary.get("party").as_list().cloned().unwrap_or_default();
    let players = summary.get("result").get("players").as_list().cloned().unwrap_or_default();
    let names = lineup_names(&party, &players);
    let mut out = Vec::new();
    for (i, m) in party.iter().enumerate() {
        let card_name = names.get(&i).cloned().unwrap_or_else(|| m.get("name").as_str().unwrap_or("").to_string());
        let mut e = Dict::new();
        e.insert("slot".into(), m.get("slot").clone());
        e.insert("name".into(), cut(&Value::Str(card_name), 40));
        e.insert("ai".into(), m.get("ai").clone());
        e.insert("self".into(), if m.get("self").truthy() { Value::Bool(true) } else { Value::None });
        let lead_all = [("aniimo", cut(m.get("aniimo"), 40)), ("aniimoLevel", m.get("aniimo_level").clone()), ("cp", m.get("cp").clone())];
        e.insert("nickname".into(), cut(m.get("nickname"), 40));
        let lead: Vec<(&str, Value)> = lead_all.into_iter().filter(|(_, v)| !v.is_none()).collect();
        let slot = m.get("slot");
        let team = match slot.as_i64().filter(|_| slot.is_int() && !matches!(slot, Value::Bool(_))) {
            Some(s) if (s as usize) < teams.len() && teams[s as usize].truthy() => Some(teams[s as usize].clone()),
            _ => {
                if m.get("self").truthy() {
                    Some(summary.get("team").clone())
                } else {
                    None
                }
            }
        };
        let rest: Vec<Value> = team
            .as_ref()
            .and_then(|t| t.as_list().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|n| n.truthy())
            .map(|n| dict([("aniimo", Value::Str(n.py_str().chars().take(40).collect()))]))
            .collect();
        for (k, v) in &lead {
            e.insert((*k).into(), v.clone());
        }
        let has_lead = lead.iter().any(|(k, v)| *k == "aniimo" && v.truthy());
        if has_lead || !rest.is_empty() {
            let mut t = vec![dict(lead.clone())];
            t.extend(rest);
            e.insert("team".into(), Value::List(t));
        }
        out.push(Value::Dict(e.iter().filter(|(_, v)| !v.is_none()).map(|(k, v)| (k.clone(), v.clone())).collect()));
    }
    if out.is_empty() { Value::Dict(Dict::new()) } else { dict([("lineup", Value::List(out))]) }
}

fn bosses(meter: &Value) -> Value {
    let Some(b) = meter.get("bosses").as_list().filter(|b| !b.is_empty()) else { return Value::Dict(Dict::new()) };
    let list = b
        .iter()
        .take(2)
        .filter(|x| matches!(x, Value::Dict(_)))
        .map(|x| {
            dict([
                ("id", Value::Str(x.get("id").py_str())),
                ("name", cut(x.get("name"), 40)),
                ("maxHp", x.get("max_hp").clone()),
                ("damage", x.get("damage").clone()),
                ("defeated", Value::Bool(x.get("defeated").truthy())),
                ("seconds", x.get("seconds").clone()),
            ])
        })
        .collect();
    dict([("bosses", Value::List(list))])
}

/// The Battle Data card of a result-screen player: its slot's card (by position for summaries without slots).
pub fn card_of<'a>(cards: &'a [Value], player: &Value, i: usize) -> Value {
    let slot = player.get("slot");
    if slot.is_int() && !matches!(slot, Value::Bool(_)) && cards.iter().any(|c| c.get("slot").is_int()) {
        return cards.iter().find(|c| c.get("slot").py_eq(slot)).cloned().unwrap_or(Value::Dict(Dict::new()));
    }
    cards.get(i).cloned().unwrap_or(Value::Dict(Dict::new()))
}

fn thousands(v: &Value) -> Option<String> {
    match v {
        Value::Int(i) => {
            let s = i.abs().to_string();
            let mut out = String::new();
            for (k, c) in s.chars().enumerate() {
                if k > 0 && (s.len() - k) % 3 == 0 {
                    out.push(',');
                }
                out.push(c);
            }
            Some(if *i < 0 { format!("-{out}") } else { out })
        }
        _ => None,
    }
}

/// (title, details, accent) of the notice over the game when a record reached the leaderboards.
pub fn sent_notice(payload: &Value, reply: Option<&Value>) -> Option<(String, String, String)> {
    let kind = payload.get("kind").as_str().unwrap_or("");
    let stars = match payload.get("stars") {
        Value::Int(s) => Some(format!("{s} stars")),
        Value::Bool(b) => Some(format!("{} stars", if *b { "True" } else { "False" })),
        _ => None,
    };
    let boss = payload.get("boss").get("name").as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let verb = if reply.is_some_and(|r| r.get("updated").truthy()) { "updated" } else { "sent" };
    let s = |v: &Value| if v.truthy() { Some(v.py_str()) } else { None };
    let (head, parts, accent): (String, Vec<Option<String>>, &str) = match kind {
        "interlink_run" => (format!("Interlink run {verb}"), vec![boss, stars, thousands(payload.get("score"))], "#7fb2ff"),
        "interlink_best" => (format!("Interlink best {verb}"), vec![boss, stars, thousands(payload.get("score"))], "#7fb2ff"),
        "sim_run" => (format!("Holo-Battle Sim run {verb}"), vec![s(payload.get("trial")), s(payload.get("difficulty"))], "#9be27a"),
        "eggheist_match" => (format!("Egg Heist match {verb}"), vec![s(payload.get("difficulty")), thousands(payload.get("score"))], "#ffc861"),
        "eggheist_season" => (format!("Egg Heist rank {verb}"), vec![s(payload.get("rank").get("name")), thousands(payload.get("score"))], "#ffc861"),
        _ => return None,
    };
    Some((head, parts.into_iter().flatten().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" \u{b7} "), accent.into()))
}

fn played_at(clock: &dyn Clock, started: &Value) -> Value {
    let Some(s) = started.as_str().filter(|s| !s.is_empty()) else { return Value::None };
    let re = regex::Regex::new(r"^(\d{4})-(\d{1,2})-(\d{1,2}) (\d{1,2}):(\d{1,2}):(\d{1,2})$").unwrap();
    let Some(c) = re.captures(s) else { return Value::None };
    let n = |i: usize| c[i].parse::<u32>().unwrap();
    clock.local_to_unix(c[1].parse().unwrap(), n(2), n(3), n(4), n(5), n(6)).map_or(Value::None, |t| Value::Int(t as i64))
}

/// interlink_run from a logs/fights/<fight>.json summary.
pub fn run_payload(summary: &Value, uid: &Value, sched: &Schedule, clock: &dyn Clock) -> Value {
    let empty = Value::Dict(Dict::new());
    let res = or_none(summary.get("result"), &empty).clone();
    let bd = or_none(summary.get("battle_data"), &empty).clone();
    let meter = or_none(summary.get("meter"), &empty).clone();
    let cards = bd.get("players").as_list().cloned().filter(|c| !c.is_empty()).unwrap_or_default();
    let mut listed: Vec<Value> = match res.get("players").as_list().filter(|l| !l.is_empty()) {
        Some(l) => l.clone(),
        None => cards.iter().map(|_| Value::Dict(Dict::new())).collect(),
    };
    let is_slot = |v: &Value| v.is_int() && !matches!(v, Value::Bool(_));
    let slots: BTreeSet<i64> = listed.iter().filter(|p| is_slot(p.get("slot"))).filter_map(|p| p.get("slot").as_i64()).collect();
    if !slots.is_empty() {
        let mut by: indexmap::IndexMap<i64, Value> = indexmap::IndexMap::new();
        for c in &cards {
            let s = c.get("slot");
            if is_slot(s) && (0..=3).contains(&s.as_i64().unwrap()) && !slots.contains(&s.as_i64().unwrap()) {
                by.insert(
                    s.as_i64().unwrap(),
                    dict([("slot", s.clone()), ("name", c.get("name").clone()), ("ai", c.get("ai").clone()), ("self", c.get("self").clone())]),
                );
            }
        }
        let mut extra: Vec<Value> = by.into_values().collect();
        if listed.iter().any(|p| p.get("self").truthy()) {
            extra = extra
                .into_iter()
                .map(|p| {
                    if p.get("self").truthy() {
                        let mut d = p.as_dict().unwrap().clone();
                        d.insert("name".into(), Value::None);
                        d.insert("self".into(), Value::None);
                        d.insert("unnamed".into(), Value::Bool(true));
                        Value::Dict(d)
                    } else {
                        p
                    }
                })
                .collect();
        }
        listed.extend(extra);
        listed.sort_by_key(|p| if is_slot(p.get("slot")) { p.get("slot").as_i64().unwrap() } else { 4 });
    }
    let mut players = Vec::new();
    for (i, p) in listed.iter().enumerate() {
        let c = card_of(&cards, p, i);
        let name_src = if p.get("name").truthy() {
            p.get("name").clone()
        } else if p.get("unnamed").truthy() {
            Value::None
        } else {
            c.get("name").clone()
        };
        players.push(no_nones(vec![
            ("name", cut(&name_src, 40)),
            ("stat", cut(p.get("stat"), 60)),
            ("value", p.get("value").clone()),
            ("ai", if matches!(p.get("ai"), Value::Bool(_)) { p.get("ai").clone() } else { Value::None }),
            ("self", if p.get("self").truthy() { Value::Bool(true) } else { Value::None }),
            ("avgDps", c.get("avg_dps").clone()),
            ("breakValue", c.get("break").clone()),
            ("damageTaken", c.get("taken").clone()),
            ("heal", c.get("heal").clone()),
        ]));
    }
    let secs = res.get("time_s").clone();
    let csv = meter.get("csv").as_str().filter(|s| !s.is_empty()).unwrap_or("run").to_string();
    let run_id = match csv.rfind('.') {
        Some(i) if i > 0 && !csv[i..].contains(['/', '\\']) => csv[..i].to_string(),
        _ => csv,
    };
    let mut out = Dict::new();
    out.insert("kind".into(), "interlink_run".into());
    out.insert("clientRunId".into(), Value::Str(run_id));
    if summary.get("server").truthy() {
        out.insert("server".into(), summary.get("server").clone());
    }
    out.insert("gameUid".into(), or_none(summary.get("uid"), uid).clone());
    out.insert("cycle".into(), summary.get("cycle").clone());
    out.insert("boss".into(), dict([("id", summary.get("puppetId").clone()), ("name", summary.get("boss").clone()), ("level", summary.get("level").clone())]));
    out.insert("score".into(), res.get("score").clone());
    out.insert("stars".into(), res.get("rating_stars").clone());
    out.insert("clearTimeS".into(), secs.clone());
    out.insert("cleared".into(), if secs.is_none() { Value::None } else { Value::Bool(res.get("cleared").truthy()) });
    out.insert("teamDps".into(), bd.get("team_avg_dps").clone());
    out.insert("meterDps".into(), meter.get("dps").clone());
    out.insert("meterDamage".into(), meter.get("damage").clone());
    out.insert("partySize".into(), if players.is_empty() { Value::None } else { Value::Int(players.len() as i64) });
    out.insert("players".into(), Value::List(players));
    out.insert("playedAt".into(), played_at(clock, summary.get("started")));
    if summary.get("buffs").truthy() {
        let b: Vec<Value> = summary
            .get("buffs")
            .as_list()
            .unwrap()
            .iter()
            .map(|b| Value::Dict(b.as_dict().map(|d| d.iter().filter(|(_, v)| !v.is_none()).map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()))
            .collect();
        out.insert("buffs".into(), Value::List(b));
    }
    let mut extra = Dict::new();
    extra.insert("newBest".into(), res.get("new_best").clone());
    extra.insert("newBests".into(), or_none(summary.get("new_bests"), &Value::List(vec![])).clone());
    extra.insert("trackedFromStart".into(), meter.get("tracked_from_start").clone());
    extra.insert("instanceId".into(), summary.get("instance").clone());
    extend(&mut extra, slot_of(sched, summary.get("cycle"), summary.get("puppetId")));
    extend(&mut extra, lineup(summary));
    if res.get("star_cap").truthy() {
        extra.insert("starCap".into(), res.get("star_cap").clone());
    }
    if res.get("ended_by_leader").truthy() {
        extra.insert("endedByLeader".into(), Value::Bool(true));
    }
    extend(&mut extra, screen_checks(&res, or_none(summary.get("battle_data"), &empty)));
    if meter.get("time_limit_s").truthy() {
        extra.insert("timeLimitS".into(), meter.get("time_limit_s").clone());
    }
    extend(&mut extra, bosses(&meter));
    out.insert("extra".into(), Value::Dict(extra));
    Value::Dict(out)
}

/// `time.strftime('%Y-W%W')` for a time, in the clock's zone.
fn week_of(clock: &dyn Clock, now: f64) -> String {
    use chrono::TimeZone;
    let _ = clock;
    chrono::Local.timestamp_opt(now as i64, 0).single().map_or_else(String::new, |d| d.format("%Y-W%W").to_string())
}

/// interlink_week (Challenge Stats) or interlink_best (battle prep).
#[allow(clippy::too_many_arguments)]
pub fn score_payload(screen: &str, boss: &str, score: &Value, stars: &Value, uid: &Value, cycle: &Value, boss_id: &Value, sched: &Schedule, clock: &dyn Clock) -> Value {
    let now = clock.now();
    let week = if cycle.truthy() { cycle.py_str() } else { week_of(clock, now) };
    if screen == "challenge_stats" {
        return dict([
            ("kind", Value::Str("interlink_week".into())),
            ("clientRunId", Value::Str(safe(&format!("week-{week}")))),
            ("gameUid", uid.clone()),
            ("cycle", cycle.clone()),
            ("score", score.clone()),
            ("stars", stars.clone()),
            ("playedAt", Value::Int(now as i64)),
        ]);
    }
    let who = if boss_id.truthy() { boss_id.py_str() } else { boss.to_string() };
    dict([
        ("kind", Value::Str("interlink_best".into())),
        ("clientRunId", Value::Str(safe(&format!("best-{week}-{who}")))),
        ("gameUid", uid.clone()),
        ("cycle", cycle.clone()),
        ("boss", dict([("id", boss_id.clone()), ("name", Value::Str(boss.chars().take(80).collect()))])),
        ("score", score.clone()),
        ("stars", stars.clone()),
        ("playedAt", Value::Int(now as i64)),
        ("extra", slot_of(sched, cycle, boss_id)),
    ])
}

/// sim_run: one Holo-Battle Sim win from its Victory screen.
pub fn sim_payload(sim: &Value, uid: &Value, played_at: Option<f64>, clock: &dyn Clock) -> Value {
    let at = played_at.filter(|p| *p != 0.0).unwrap_or_else(|| clock.now()) as i64;
    let who = if uid.truthy() { uid.py_str() } else { "x".into() };
    let mut extra = Dict::new();
    extra.insert("trial".into(), sim.get("trial").clone());
    extra.insert("element".into(), sim.get("element").clone());
    if sim.get("setup").truthy() {
        extra.insert("setup".into(), sim.get("setup").clone());
    }
    dict([
        ("kind", Value::Str("sim_run".into())),
        ("clientRunId", Value::Str(safe(&format!("sim-{who}-{at}")))),
        ("gameUid", uid.clone()),
        ("clearTimeS", sim.get("time_s").clone()),
        ("trial", sim.get("trial").clone()),
        ("difficulty", sim.get("difficulty").clone()),
        ("cleared", Value::Bool(true)),
        ("playedAt", Value::Int(at)),
        ("extra", Value::Dict(extra)),
    ])
}

/// eggheist_match: one Egg Heist match from the end screen.
pub fn egg_match_payload(m: &Value, season: &Value, uid: &Value, played_at: Option<f64>, clock: &dyn Clock) -> Value {
    let at = played_at.filter(|p| *p != 0.0).unwrap_or_else(|| clock.now()) as i64;
    let who = if uid.truthy() { uid.py_str() } else { "x".into() };
    let outcome = m.get("outcome").as_str().unwrap_or("");
    let diff = m.get("difficulty");
    let dtext = diff.as_str().unwrap_or("");
    let diff_ok = !dtext.is_empty() && dtext.chars().count() <= 20 && dtext.chars().all(|c| c.is_ascii_alphabetic() || c == ' ');
    let mut extra = Dict::new();
    for k in ["mode", "objective", "chests_today"] {
        if m.get(k).truthy() {
            extra.insert(k.into(), m.get(k).clone());
        }
    }
    if m.get("screen_check").truthy() {
        extra.insert("screenCheck".into(), dict([("result", m.get("screen_check").clone())]));
    }
    if m.get("party").truthy() {
        extend(&mut extra, lineup(m.get("party")));
    }
    dict([
        ("kind", Value::Str("eggheist_match".into())),
        ("clientRunId", Value::Str(safe(&format!("egg-{who}-{at}")))),
        ("gameUid", uid.clone()),
        ("cycle", season.clone()),
        ("score", m.get("loot").clone()),
        ("clearTimeS", m.get("time_s").clone()),
        ("cleared", if outcome == "success" || outcome == "failed" { Value::Bool(outcome == "success") } else { Value::None }),
        ("difficulty", if diff_ok { or_none(diff, &Value::None).clone() } else { Value::None }),
        ("defeats", m.get("defeats").clone()),
        ("playedAt", Value::Int(at)),
        ("extra", Value::Dict(extra)),
    ])
}

/// eggheist_season: the season standing as seen.
pub fn eggheist_payload(standing: &Value, season: &Value, uid: &Value, profile: Option<&Value>, clock: &dyn Clock) -> Value {
    use chrono::TimeZone;
    let now = clock.now();
    let s = if season.truthy() {
        season.py_str()
    } else {
        chrono::Local.timestamp_opt(now as i64, 0).single().map_or_else(String::new, |d| d.format("%Y").to_string())
    };
    let extra = profile.and_then(|p| p.as_dict()).map(|d| d.iter().filter(|(_, v)| !v.is_none()).map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
    dict([
        ("kind", Value::Str("eggheist_season".into())),
        ("clientRunId", Value::Str(safe(&format!("eggheist-{s}")))),
        ("gameUid", uid.clone()),
        ("cycle", season.clone()),
        (
            "rank",
            dict([
                ("tier", standing.get("tier").clone()),
                ("subtier", standing.get("subtier").clone()),
                ("name", Value::Str(standing.get("name").py_str().chars().take(40).collect())),
            ]),
        ),
        ("score", standing.get("points").clone()),
        ("stars", standing.get("stars").clone()),
        ("playedAt", Value::Int(now as i64)),
        ("extra", Value::Dict(extra)),
    ])
}

/// (UID|season, [tier, subtier, points, stars]) of an eggheist_season payload.
pub fn standing_key(payload: &Value) -> (String, Value) {
    let rank = payload.get("rank");
    (
        format!("{}|{}", payload.get("gameUid").py_str(), payload.get("cycle").py_str()),
        Value::List(vec![rank.get("tier").clone(), rank.get("subtier").clone(), payload.get("score").clone(), payload.get("stars").clone()]),
    )
}
