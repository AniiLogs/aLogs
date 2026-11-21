//! The Party screen's pictures, matched to the game's own (icons.py's cache): each card's lead Aniimo and its team
//! of three, the leader's Season Buffs and Special Offering, and your own team from the fight's right side when the
//! Party screen left a slot blank. Port of those parts of aLogs B16 `bossbar/live.py`. The Python reads these on
//! background threads; here they run in the reading loop (matching is much faster in Rust).

use crate::meter::Meter;
use crate::party::{EGG_PARTY_CARDS, PANEL_RAISED, PARTY_CARDS};
use alogs_pil::ops::{Stat, difference};
use alogs_pil::{Filter, Image, Mode};
use alogs_pyval::{Dict, Value, dict};
use alogs_reader::icons::{Icons, Match};
use std::collections::HashMap;

type BoxF = (f64, f64, f64, f64);

pub const TEAM_PITCH: f64 = 0.0503;
pub const TEAM_WINDOW: BoxF = (-0.030, 0.064, -0.012, 0.100);
pub const TEAM_SQUARE: (f64, f64, f64) = (-0.0345, 0.0472, 0.0405);
pub const TEAM_BADGE: (f64, f64) = (74.0, 42.0);
pub const TEAM_PORTRAIT: f64 = 204.0;
pub const TEAM_COMPARE: BoxF = (-75.0, -52.0, 11.0, 45.0);
pub const TEAM_COMPARE_FADED: BoxF = (-72.0, -56.0, 28.0, 56.0);
pub const LEAD_OFFSET: (f64, f64) = (-6.0, -152.0);
pub const LEAD_PORTRAIT: f64 = 256.0;
pub const LEAD_COMPARE: BoxF = (-90.0, -100.0, 86.0, 90.0);
pub const SEASON_BUFFS: [BoxF; 2] = [(0.525, 0.867, 0.550, 0.911), (0.5595, 0.867, 0.5845, 0.911)];
pub const SEASON_LEVEL: BoxF = (0.0153, 0.900, 0.0273, 0.923);
pub const OFFERING: BoxF = (0.4338, 0.865, 0.4575, 0.909);
pub const HUD_TEAM: (f64, [f64; 3]) = (0.9219, [0.3278, 0.4282, 0.5287]);
pub const HUD_SIDE: f64 = 0.0495;
pub const HUD_WINDOW: BoxF = (-0.0083, -0.0338, 0.0117, 0.0032);
pub const HUD_EVERY_S: f64 = 4.0;
pub const HUD_AGREE: i64 = 2;
pub const TILE_THUMB: usize = 24;
pub const TILE_SAME: f64 = 2.0;
pub const CIRCLE_EVERY_S: f64 = 0.5;
pub const CIRCLE_RETRY_S: f64 = 3.0;
pub const PANEL_SPOT: BoxF = (0.405, 0.885, 0.43, 0.90);
pub const PANEL_BLUE: f64 = 170.0;

#[derive(Default)]
pub struct IconState {
    /// Each tile's last sure read: (window, thumbnail, answer).
    tile_memo: HashMap<(usize, String), ((i64, i64, i64, i64), Image, Option<Match>)>,
    hud_at: f64,
    hud_votes: HashMap<(usize, String), i64>,
    circle_on: bool,
    circle_at: f64,
    circle_prev: Option<Image>,
    circle_read: Option<Image>,
    circle_tried: f64,
    panel_sy: f64,
    /// The off-list icon last said in the log, and the week: (icon, week).
    off_list_said: Option<(String, Option<String>)>,
}

/// Whether a portrait match counts: clearly ahead of the next Aniimo.
pub fn tile_sure(r: &Option<Match>) -> bool {
    r.as_ref().is_some_and(|r| (r.1 >= 0.55 && r.2 >= 0.15) || (r.1 >= 0.68 && r.2 >= 0.10))
}

fn ibox(b: BoxF) -> (i64, i64, i64, i64) {
    (b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64)
}

fn thumb(im: &Image, b: BoxF) -> Image {
    let w = ibox(b);
    im.crop_i(w.0, w.1, w.2, w.3).convert(Mode::Rgb).resize(TILE_THUMB, TILE_THUMB, Some(Filter::Bilinear))
}

fn same(a: Option<&Image>, b: &Image) -> bool {
    a.is_some_and(|a| a.width == b.width && a.height == b.height && Stat::new(&difference(a, b), None).mean().iter().cloned().fold(0.0, f64::max) <= TILE_SAME)
}

fn opt_s(v: &Option<String>) -> Value {
    v.clone().map_or(Value::None, Value::Str)
}

fn who(m: &Value) -> (String, String) {
    let f = |k: &str| {
        let v = m.get(k);
        let s = if v.is_none() { String::new() } else { v.py_str() };
        crate::team::casefold(&s.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect::<String>())
    };
    (f("name"), f("aniimo"))
}

fn who_v(w: &(String, String)) -> Value {
    Value::List(vec![Value::Str(w.0.clone()), Value::Str(w.1.clone())])
}

fn who_of(v: &Value) -> Option<(String, String)> {
    let l = v.as_list()?;
    Some((l.first()?.py_str(), l.get(1)?.py_str()))
}

/// What a look at the Special Offering's circle gave.
#[derive(Debug, Clone)]
pub enum OfferingRead {
    /// Nothing sure.
    None,
    /// A sure match of an icon none of whose offerings the week's pick list holds: a misread, taken as no read.
    OffList(String),
    Some(Value),
}

impl PartialEq for OfferingRead {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (OfferingRead::None, OfferingRead::None) => true,
            (OfferingRead::OffList(a), OfferingRead::OffList(b)) => a == b,
            (OfferingRead::Some(a), OfferingRead::Some(b)) => a.py_eq(b),
            _ => false,
        }
    }
}

impl OfferingRead {
    pub fn value(self) -> Option<Value> {
        if let OfferingRead::Some(v) = self { Some(v) } else { None }
    }
}

/// The Special Offering whose icon shows in `b`. An icon several offerings share is told apart by the week's pick
/// list (`cycle`: the Interlink week); when that can't decide, the names are joined with " or " and no id is sent.
pub fn read_offering(ic: &Icons, im: &Image, b: BoxF, cycle: Option<&str>) -> OfferingRead {
    let Some(r) = ic.buff(im, b, "offering", false) else { return OfferingRead::None };
    if !(r.1 >= 0.7 && r.2 >= 0.15) {
        return OfferingRead::None;
    }
    offering_of(ic, &r.0, cycle)
}

/// The offering for a matched icon in a week (read_offering after the match).
pub fn offering_of(ic: &Icons, icon: &str, cycle: Option<&str>) -> OfferingRead {
    match ic.offerings_for(icon, cycle) {
        Some(ids) => OfferingRead::Some(offering_value(ic, &ids)),
        None => OfferingRead::OffList(icon.to_string()),
    }
}

/// `{id, name}` for one offering; `{name: "A or B"}` (or the one name two ids share) when it's still one of several.
pub fn offering_value(ic: &Icons, ids: &[String]) -> Value {
    let names: std::collections::BTreeSet<String> = ids.iter().filter_map(|k| ic.buffs.get(k).and_then(|b| b.get("name")).and_then(|n| n.as_str()).map(String::from)).collect();
    let name = Value::Str(names.into_iter().collect::<Vec<_>>().join(" or "));
    match ids {
        [id] => dict([("id", Value::Int(id.parse().unwrap_or(0))), ("name", name)]),
        _ => dict([("name", name)]),
    }
}

fn set_buffs(party: &mut Dict, season: Vec<Value>, offering: Value) {
    party.insert("buff_reads".into(), dict([("season", Value::List(season.clone())), ("offering", offering.clone())]));
    let mut buffs: Vec<Value> = season.into_iter().filter(Value::truthy).collect();
    if offering.truthy() {
        buffs.push(offering);
    }
    party.insert("buffs".into(), Value::List(buffs));
}

impl Meter {
    /// A look's offering as taken: an off-list icon is no read (said once per icon and week), so the looks after it
    /// decide; the game forces a pick from the week's list, so a real read comes.
    pub fn offering_taken(&mut self, read: OfferingRead, cycle: Option<&str>) -> Option<Value> {
        if let OfferingRead::OffList(icon) = &read {
            let key = (icon.clone(), cycle.map(String::from));
            if self.icon.off_list_said.as_ref() != Some(&key) {
                log::info!(target: "meter", "Special Offering: icon {icon} matched, but none of its offerings is on week {}'s pick list: taken as a misread, not as a read",
                    cycle.unwrap_or("?"));
                self.icon.off_list_said = Some(key);
            }
        }
        read.value()
    }

    fn unit(u: f64) -> f64 {
        TEAM_PITCH * u / 240.0
    }

    fn tile_centre(t: f64, b: f64, u: f64, ux: f64, h: f64) -> (f64, f64) {
        let (sx, sy, side) = TEAM_SQUARE;
        (ux + u * (t + sx + side / 2.0), h * (b + sy) + u * side / 2.0)
    }

    fn card_shift(ic: &Icons, im: &Image, a: f64, b: f64, u: f64, ux: f64, h: f64) -> (f64, f64) {
        let unit = Self::unit(u);
        let (dx, r) = TEAM_BADGE;
        let mut best = (0.0, 0.0, 0.0);
        for i in [1.0, 0.0, 2.0] {
            let (px, py) = Self::tile_centre(a + TEAM_PITCH * i, b, u, ux, h);
            let (s, x, y) = ic.badge(im, px + dx * unit, py, r * unit, (30.0 * unit).round_ties_even() as i64, Some((90.0 * unit).round_ties_even() as i64),
                ((5.0 * unit).round_ties_even() as usize).max(2));
            if s > best.0 {
                best = (s, x - px - dx * unit, y - py);
            }
            if best.0 >= 0.6 {
                break;
            }
        }
        if best.0 >= 0.4 { (best.1, best.2) } else { (0.0, 0.0) }
    }

    fn portrait_read(&mut self, im: &Image, window: BoxF, square: (f64, f64, f64), fade: &str, memo: (usize, String)) -> Option<Match> {
        let th = thumb(im, window);
        if let Some(last) = self.icon.tile_memo.get(&memo)
            && last.0 == ibox(window)
            && same(Some(&last.1), &th)
        {
            return last.2.clone();
        }
        let r = self.icons.as_ref()?.head(im, window, square, 40, fade);
        if tile_sure(&r) {
            self.icon.tile_memo.insert(memo, (ibox(window), th, r.clone()));
        } else {
            self.icon.tile_memo.remove(&memo);
        }
        r
    }

    #[allow(clippy::too_many_arguments)]
    fn team_tile(&mut self, im: &Image, t: f64, b: f64, u: f64, ux: f64, h: f64, shift: (f64, f64), memo: (usize, String)) -> Option<Match> {
        let ic = self.icons.as_ref()?;
        let unit = Self::unit(u);
        let (mut px, mut py) = Self::tile_centre(t, b, u, ux, h);
        px += shift.0;
        py += shift.1;
        let (dx, r) = TEAM_BADGE;
        let found = ic.badge(im, px + dx * unit, py, r * unit, ((8.0 * unit).round_ties_even() as i64).max(4), None, 2);
        if found.0 >= 0.4 {
            px = found.1 - dx * unit;
            py = found.2;
        }
        let fade = ic.has_fade("team");
        let (x0, y0, x1, y1) = if fade { TEAM_COMPARE_FADED } else { TEAM_COMPARE };
        let half = TEAM_PORTRAIT * unit / 2.0;
        let window = (px + x0 * unit, py + y0 * unit, px + x1 * unit, py + y1 * unit);
        self.portrait_read(im, window, (px - half, py - half, 2.0 * half), if fade { "team" } else { "" }, memo)
    }

    #[allow(clippy::too_many_arguments)]
    fn lead_tile(&mut self, im: &Image, a: f64, b: f64, u: f64, ux: f64, h: f64, shift: (f64, f64), memo: (usize, String)) -> Option<Match> {
        let unit = Self::unit(u);
        let (px, py) = Self::tile_centre(a, b, u, ux, h);
        let (px, py) = (px + shift.0 + LEAD_OFFSET.0 * unit, py + shift.1 + LEAD_OFFSET.1 * unit);
        let fade = if self.icons.as_ref()?.has_fade("lead") { "lead" } else { "" };
        let (x0, y0, x1, y1) = LEAD_COMPARE;
        let half = LEAD_PORTRAIT * unit / 2.0;
        let window = (px + x0 * unit, py + y0 * unit, px + x1 * unit, py + y1 * unit);
        self.portrait_read(im, window, (px - half, py - half, 2.0 * half), fade, memo)
    }

    fn panel_at_rest(&self, im: &Image, u: f64, ux: f64, h: f64) -> bool {
        let (x0, y0, x1, y1) = PANEL_SPOT;
        let sy = self.icon.panel_sy;
        let spot = im.crop_i((ux + u * x0) as i64, (h * y0 + sy) as i64, (ux + u * x1) as i64, (h * y1 + sy) as i64);
        Stat::new(&spot.convert(Mode::Rgb), None).mean()[2] >= PANEL_BLUE
    }

    /// Each player's team (cards: (slot, pill)) and the leader's buffs from the Party screen's pictures.
    pub fn read_party_icons(&mut self, im: &Image, cards: &[(i64, usize)], u: f64, ux: f64, h: f64) {
        self.load_icons();
        if self.icons_state.as_deref() != Some("ready") || self.party.is_none() {
            return;
        }
        let whos: HashMap<i64, (String, String)> = self.party.as_ref().unwrap().get("members").as_list().cloned().unwrap_or_default().iter()
            .map(|m| (m.get("slot").as_i64().unwrap_or(0), who(m))).collect();
        let boxes: &[BoxF] = if self.ps.egg_party { &EGG_PARTY_CARDS } else { &PARTY_CARDS };
        let mut teams: Vec<(i64, Vec<Option<String>>)> = vec![];
        let mut leads: HashMap<i64, Option<String>> = HashMap::new();
        let mut shifts = vec![];
        for &(slot, card) in cards {
            let Some(cb) = boxes.get(card) else { continue };
            let (a, b) = (cb.0, cb.1);
            let shift = Self::card_shift(self.icons.as_ref().unwrap(), im, a, b, u, ux, h);
            let mut team = vec![];
            for i in 0..3 {
                let r = self.team_tile(im, a + TEAM_PITCH * i as f64, b, u, ux, h, shift, (card, i.to_string()));
                team.push(if tile_sure(&r) { r.map(|r| r.0) } else { None });
            }
            let r = self.lead_tile(im, a, b, u, ux, h, shift, (card, "lead".into()));
            leads.insert(slot, if tile_sure(&r) { r.map(|r| r.0) } else { None });
            teams.push((slot, team));
            shifts.push(shift);
        }
        if self.ps.egg_party {
            self.party_teams(&teams, &whos, &leads);
            return;
        }
        let mut dys: Vec<f64> = shifts.iter().map(|s| s.1).collect();
        dys.sort_by(f64::total_cmp);
        let dy = if dys.is_empty() { 0.0 } else { dys[dys.len() / 2] };
        let cycle = self.env.schedule.active_cycle_id(self.env.now());
        let ic = self.icons.as_ref().unwrap();
        let at = |off: f64, b: BoxF| (ux + u * b.0, h * b.1 + off, ux + u * b.2, h * b.3 + off);
        let mut fits: HashMap<i64, f64> = HashMap::new();
        let mut fit = |off: f64| -> f64 { *fits.entry(off as i64).or_insert_with(|| ic.buff(im, at(off, SEASON_BUFFS[0]), "season", true).map_or(-1.0, |r| r.1)) };
        let mut sy = 0.0;
        if dy.abs() > 10.0 * h / 2160.0 && dys.iter().all(|d| (d - dy).abs() < 6.0 * h / 2160.0) {
            let cands: Vec<f64> = [1.7, 1.8, 1.9, 2.0, 2.1].iter().map(|f| (dy * f).round_ties_even()).collect();
            sy = cands[0];
            for &c in &cands[1..] {
                if fit(c) > fit(sy) {
                    sy = c;
                }
            }
        }
        if fit(sy) < 0.8 {
            let mut cands = vec![sy, (h * PANEL_RAISED).round_ties_even()];
            cands.extend([1.0, 2.0, 3.0, -1.0, -2.0, -3.0].iter().map(|k| sy + (h * k * 0.005).round_ties_even()));
            let mut best = cands[0];
            for &c in &cands[1..] {
                if fit(c) > fit(best) {
                    best = c;
                }
            }
            sy = best;
        }
        let mut season = vec![];
        for sq in SEASON_BUFFS {
            let r = ic.buff(im, at(sy, sq), "season", true);
            let Some(r) = r.filter(|r| r.1 >= 0.8 && r.2 >= 0.2) else {
                season.push(Value::None);
                continue;
            };
            let (x0, y0, x1, y1) = SEASON_LEVEL;
            let d = ic.digit(im, at(sy, (sq.0 + x0, y0, sq.0 + x1, y1)));
            let level = d.filter(|d| d.2 >= 0.055).and_then(|d| d.0.to_digit(10)).map(i64::from);
            let ids = if level.is_some() { ic.buffs_named(&r.0, "season", level) } else { vec![] };
            let bname = |k: &String| ic.buffs.get(k).and_then(|b| b.get("name")).and_then(|n| n.as_str()).unwrap_or("").to_string();
            let names: Vec<String> = ic.buffs_named(&r.0, "season", None).iter().map(bname).collect();
            season.push(if let Some(id) = ids.first() {
                dict([("id", Value::Int(id.parse().unwrap_or(0))), ("name", Value::Str(bname(id))), ("level", level.map_or(Value::None, Value::Int))])
            } else {
                // max(set(names), key=names.count)
                let set: std::collections::BTreeSet<&String> = names.iter().collect();
                let mut best: Option<(&String, usize)> = None;
                for n in set {
                    let c = names.iter().filter(|x| *x == n).count();
                    if best.is_none_or(|b| c > b.1) {
                        best = Some((n, c));
                    }
                }
                dict([("name", best.map_or(Value::None, |b| Value::Str(b.0.clone())))])
            });
        }
        self.icon.panel_sy = sy;
        let read = if self.icon.circle_on { OfferingRead::None } else { read_offering(ic, im, at(sy, OFFERING), cycle.as_deref()) };
        let at_rest = self.panel_at_rest(im, u, ux, h);
        let offering = self.offering_taken(read, cycle.as_deref());
        if !self.party_teams(&teams, &whos, &leads) {
            return;
        }
        let Some(Value::Dict(party)) = &mut self.party else { return };
        let last = party.at("buff_reads").clone();
        let last_season = last.get("season").as_list().cloned().unwrap_or_else(|| vec![Value::None; season.len()]);
        let kept = if !season.iter().any(Value::truthy) && at_rest { vec![Value::None; season.len()] } else { last_season };
        let merged: Vec<Value> = season.iter().zip(kept.iter().chain(std::iter::repeat(&Value::None))).map(|(n, o)| if n.truthy() { n.clone() } else { o.clone() }).collect();
        let off = offering.unwrap_or_else(|| last.get("offering").clone());
        set_buffs(party, merged, off);
        let leads_v: Vec<Value> = party.at("members").as_list().cloned().unwrap_or_default().iter().map(|m| m.get("aniimo").clone()).collect();
        log::info!(target: "meter", "Party screen icons: leads {}, teams {}, buffs {}", Value::List(leads_v).repr(), party.at("teams").repr(), party.at("buffs").repr());
    }

    /// Each card's team reads into the party, the newest of each tile standing, and its lead's species. False with no party.
    fn party_teams(&mut self, teams: &[(i64, Vec<Option<String>>)], whos: &HashMap<i64, (String, String)>, leads: &HashMap<i64, Option<String>>) -> bool {
        let Some(Value::Dict(party)) = &mut self.party else { return false };
        let mut members: Vec<Value> = party.at("members").as_list().cloned().unwrap_or_default();
        let idx: HashMap<i64, usize> = members.iter().enumerate().map(|(i, m)| (m.get("slot").as_i64().unwrap_or(0), i)).collect();
        let now_who: HashMap<i64, (String, String)> = idx.iter().map(|(s, i)| (*s, who(&members[*i]))).collect();
        let mut out: Vec<Value> = party.at("teams").as_list().cloned().unwrap_or_else(|| vec![Value::None; 4]);
        let mut had = party.at("team_who").as_dict().cloned().unwrap_or_default();
        for (slot, team) in teams {
            let (then, now) = (whos.get(slot), now_who.get(slot));
            if now != then && !(then.is_some() && now.is_some() && then.unwrap().0 == now.unwrap().0 && then.unwrap().1.is_empty()) {
                continue;
            }
            let lead = leads.get(slot).cloned().flatten();
            let mi = idx.get(slot).copied();
            let other = mi.is_some_and(|i| lead.is_some() && members[i].get("aniimo").truthy() && members[i].get("aniimo").as_str() != lead.as_deref());
            if let (Some(i), Some(l)) = (mi, &lead) {
                let mut d = members[i].as_dict().cloned().unwrap();
                d.insert("aniimo".into(), Value::Str(l.clone()));
                members[i] = Value::Dict(d);
            }
            let s = *slot as usize;
            while out.len() <= s {
                out.push(Value::None);
            }
            let hk = slot.to_string();
            let last: Vec<Value> = if !other && had.get_str(&hk).and_then(who_of) == now.cloned() && out[s].truthy() {
                out[s].as_list().cloned().unwrap_or_default()
            } else {
                vec![Value::None; 3]
            };
            out[s] = Value::List(team.iter().zip(last.iter().chain(std::iter::repeat(&Value::None))).map(|(n, o)| if n.is_some() { opt_s(n) } else { o.clone() }).collect());
            let w = mi.map_or_else(|| whos[slot].clone(), |i| who(&members[i]));
            had.insert(Value::Str(hk), who_v(&w));
        }
        party.insert("members".into(), Value::List(members.clone()));
        if let Some(mine) = members.iter().find(|m| m.get("self").truthy()).and_then(|m| m.get("slot").as_i64()) {
            party.insert("team".into(), out.get(mine as usize).cloned().unwrap_or(Value::None));
        }
        if self.ps.egg_party {
            log::info!(target: "meter", "Egg Heist party: teams {}", Value::List(out.clone()).repr());
        }
        party.insert("teams".into(), Value::List(out));
        party.insert("team_who".into(), Value::Dict(had));
        true
    }

    /// The Special Offering's circle, looked at twice a second while the Party screen is up: read when it changes.
    pub fn party_circle(&mut self, im: &Image, now: f64, u: f64, ux: f64, h: f64) {
        if self.party.is_none() || self.ps.egg_party || self.icons_state.as_deref() != Some("ready") || now - self.icon.circle_at < CIRCLE_EVERY_S {
            return;
        }
        self.icon.circle_on = true;
        self.icon.circle_at = now;
        let (x0, y0, x1, y1) = OFFERING;
        let sy = self.icon.panel_sy;
        let b = (ux + u * x0, h * y0 + sy, ux + u * x1, h * y1 + sy);
        let th = thumb(im, b);
        let prev = self.icon.circle_prev.replace(th.clone());
        if !same(prev.as_ref(), &th) {
            return;
        }
        let known = self.party.as_ref().unwrap().get("buff_reads").get("offering").truthy();
        if same(self.icon.circle_read.as_ref(), &th) && (known || now - self.icon.circle_tried < CIRCLE_RETRY_S) {
            return;
        }
        self.icon.circle_tried = now;
        if !self.panel_at_rest(im, u, ux, h) {
            return;
        }
        let cycle = self.env.schedule.active_cycle_id(now);
        let ic = self.icons.as_ref().unwrap();
        let mut read = read_offering(ic, im, b, cycle.as_deref());
        if !matches!(read, OfferingRead::Some(_)) {
            for s in [0.0, (h * PANEL_RAISED).round_ties_even()] {
                if s == self.icon.panel_sy {
                    continue;
                }
                let r = read_offering(ic, im, (ux + u * x0, h * y0 + s, ux + u * x1, h * y1 + s), cycle.as_deref());
                if matches!(r, OfferingRead::Some(_)) {
                    read = r;
                    self.icon.panel_sy = s;
                    break;
                }
                if read == OfferingRead::None {
                    read = r;
                }
            }
        }
        let offering = self.offering_taken(read, cycle.as_deref());
        let Some(Value::Dict(party)) = &mut self.party else { return };
        let last = party.at("buff_reads").clone();
        let season = last.get("season").as_list().cloned().unwrap_or_else(|| vec![Value::None; SEASON_BUFFS.len()]);
        let off = offering.unwrap_or(Value::None);
        set_buffs(party, season, off.clone());
        self.icon.circle_read = Some(th);
        if !off.py_eq(last.get("offering")) {
            log::info!(target: "meter", "Special Offering: {} (was {})", off.get("name").py_str(), last.get("offering").get("name").py_str());
        }
    }

    /// A look at the fight's right side while your own team has a blank slot.
    pub fn fight_team(&mut self, im: &Image, now: f64, u: f64, ux: f64, h: f64) {
        if self.icons_state.as_deref() != Some("ready") || now - self.icon.hud_at < HUD_EVERY_S {
            return;
        }
        let Some(party) = self.party.clone() else { return };
        let Some(mine) = party.get("members").as_list().and_then(|l| l.iter().find(|m| m.get("self").truthy()).cloned()) else { return };
        let slot = mine.get("slot").as_i64().unwrap_or(0) as usize;
        let teams = party.get("teams").as_list().cloned().unwrap_or_else(|| vec![Value::None; 4]);
        let mut team: Vec<Value> = teams.get(slot).and_then(|t| t.as_list().cloned()).unwrap_or_else(|| vec![Value::None; 3]);
        if team.iter().all(Value::truthy) {
            return;
        }
        self.icon.hud_at = now;
        let side = HUD_SIDE * u;
        let x = ux + u * HUD_TEAM.0;
        let (x0, y0, x1, y1) = HUD_WINDOW;
        let mut changed = false;
        for (i, row) in HUD_TEAM.1.iter().enumerate() {
            if team[i].truthy() {
                continue;
            }
            let y = h * row;
            let r = self.icons.as_ref().unwrap().head(im, (x + u * x0, y + h * y0, x + u * x1, y + h * y1), (x - side / 2.0, y - side / 2.0, side), 40, "");
            if !tile_sure(&r) {
                continue;
            }
            let r = r.unwrap();
            if team.iter().any(|t| t.as_str() == Some(r.0.as_str())) || mine.get("aniimo").as_str() == Some(r.0.as_str()) {
                continue;
            }
            let n = self.icon.hud_votes.entry((i, r.0.clone())).or_insert(0);
            *n += 1;
            if *n < HUD_AGREE {
                continue;
            }
            team[i] = Value::Str(r.0.clone());
            changed = true;
            log::info!(target: "meter", "Your team, slot {}: {}, from the fight (blank on the Party screen; fit {:.2}, ahead by {:.2})", i + 1, r.0, r.1, r.2);
        }
        if changed && let Some(Value::Dict(p)) = &mut self.party {
            let mut teams = teams;
            while teams.len() <= slot {
                teams.push(Value::None);
            }
            teams[slot] = Value::List(team.clone());
            p.insert("teams".into(), Value::List(teams));
            let mut tw = p.at("team_who").as_dict().cloned().unwrap_or_default();
            tw.insert(Value::Str(slot.to_string()), who_v(&who(&mine)));
            p.insert("team_who".into(), Value::Dict(tw));
            p.insert("team".into(), Value::List(team));
        }
    }
}
