//! Holo-Battle Sim's Victory screen: the win, its team, Holo-gizmos and pictures, noted and sent to the leaderboards.
//! Port of `sim_result`, `sim_pictures`, `sim_species`, `sim_prep_for` (aLogs B16 `bossbar/results.py`) and
//! `sim_send` / `sim_release` (`leaderboards.py`). The Gizmo screen tour, the exit gate watch and taking control are
//! in sim_tour.rs.

use crate::meter::{Frames, Meter, Out};
use crate::payloads;
use crate::results::{local, rel};
use crate::results_parse::{
    SIM_CARD_MARGIN, SIM_CARD_SCORE, SIM_GIZMO_PITCH, SIM_GIZMO_X, SIM_GIZMOS, SIM_LOW, parse_sim, parse_sim_team, sim_badge_counts,
    sim_card_boxes, sim_card_names, sim_team_crops,
};
use alogs_pil::Image;
use alogs_pyval::{Dict, Value};
use alogs_reader::prep;
use indexmap::IndexMap;
use std::path::PathBuf;

#[derive(Default)]
pub struct SimState {
    pub seen: f64,
    pub first: Option<f64>,
    pub last: Option<String>,
    pub low: bool,
    pub pending: Option<(Value, Option<PathBuf>)>,
    /// The skills noted on the Party screen before a run, with "at".
    pub prep: Option<Dict>,
    pub test: bool,
    /// The team named from the cards' art (sim_species_team).
    pub species_team: Option<Value>,
    /// The Victory screen's cards still to be named from their art, once aLogs has given control back (sim_cards_finish).
    pub cards: Option<SimCards>,
    /// The frame size every Aniimo's card art is ready for (sim_cards_warm).
    pub warm: Option<(usize, usize)>,
    pub ctl: Option<crate::sim_tour::SimCtl>,
    pub tour: Option<crate::sim_tour::Tour>,
    pub gate: crate::sim_tour::GateState,
    pub press: alogs_capture::input::PressWatch,
    pub test_down: bool,
    /// The test key asks for the open fight to end (step does it).
    pub test_pending_end: bool,
}

/// A card whose own line names an Aniimo is that Aniimo when its art fits the card this well. Above SIM_CARD_SCORE
/// (0.6), which goes with a margin over every other Aniimo: here only the named one's art is tried, and another
/// Aniimo's can reach 0.66 on a card (17 Nov; the right ones gave 0.80 to 0.94). Under it the card is named the slow
/// way, so too high costs time and never a wrong name.
pub const SIM_CARD_NAMED: f64 = 0.7;
/// How many card arts `sim_cards_warm` makes ready a look.
pub const SIM_WARM_A_LOOK: usize = 40;

/// The Victory screen as it was read, kept until its team is named: the cards whose own line names an Aniimo whose art
/// is on the card are known at once (`known`); the others (a nickname, a line not read) need every Aniimo's art
/// tried, which waits until control is back.
pub struct SimCards {
    frame: Image,
    at: (f64, f64, f64),
    text: IndexMap<String, String>,
    names: Vec<String>,
    known: Vec<Option<String>>,
}

fn s(v: &str) -> Value {
    Value::Str(v.to_string())
}

fn jstr(j: &serde_json::Value, a: &str, b: &str) -> Value {
    j.get(a).and_then(|x| x.get(b)).and_then(serde_json::Value::as_str).map_or(Value::None, s)
}

fn set(d: &Value, k: &str, v: Value) -> Value {
    let mut d = d.as_dict().cloned().unwrap_or_default();
    d.insert(s(k), v);
    Value::Dict(d)
}

impl Meter {
    /// The game's head and card names, sorted (sim_result's `names`).
    fn icon_names(&self) -> Vec<String> {
        let Some(ic) = &self.icons else { return vec![] };
        let set: std::collections::BTreeSet<String> = ic.heads.values().chain(ic.cards.values()).map(|(n, _)| n.clone()).collect();
        set.into_iter().collect()
    }

    /// Each team card's Aniimo where the card's own line names it and that Aniimo's art is on the card: one art
    /// tried a card, a few ms, where trying every Aniimo's took 0.56 s with the player's inputs not yet held (17 Nov).
    /// None where the line isn't one of the game's names (a nickname, or not read) or the art disagrees.
    pub fn sim_species_named(&self, im: &Image, u: f64, ux: f64, h: f64, text: &IndexMap<String, String>, names: &[String]) -> Vec<Option<String>> {
        let Some(ic) = self.icons.as_ref().filter(|i| !i.cards.is_empty()) else { return vec![] };
        let named = sim_card_names(text, names);
        sim_card_boxes(u, ux, h)
            .into_iter()
            .zip(named)
            .map(|((window, art), name)| name.filter(|n| ic.card_of(im, window, art, 36, n).is_some_and(|score| score >= SIM_CARD_NAMED)))
            .collect()
    }

    /// Each team card's Aniimo by its art, None where it isn't clear: the cards in `known` as they are, every Aniimo's
    /// art tried for the others.
    pub fn sim_species(&self, im: &Image, u: f64, ux: f64, h: f64, known: &[Option<String>]) -> Vec<Option<String>> {
        let Some(ic) = &self.icons else { return vec![] };
        if ic.cards.is_empty() {
            return vec![];
        }
        let mut out = vec![];
        let mut got = vec![];
        for (i, (window, art)) in sim_card_boxes(u, ux, h).into_iter().enumerate() {
            if let Some(name) = known.get(i).cloned().flatten() {
                got.push(format!("'{name}' (its name and its art)"));
                out.push(Some(name));
                continue;
            }
            let r = ic.card(im, window, art, 36);
            let sure = r.as_ref().is_some_and(|r| r.1 >= SIM_CARD_SCORE && r.2 >= SIM_CARD_MARGIN);
            out.push(if sure { r.as_ref().map(|r| r.0.clone()) } else { None });
            got.push(r.map_or("None".to_string(), |r| format!("('{}', {}, {})", r.0, Value::Float(alogs_pil::resample::round_digits(r.1, 2)).repr(), Value::Float(alogs_pil::resample::round_digits(r.2, 2)).repr())));
        }
        log::info!(target: "meter", "Holo-Battle Sim team by card art: [{}]", got.join(", "));
        out
    }

    /// In a Sim room with no fight on, every Aniimo's card art is made ready for the Victory screen's five card
    /// slots, SIM_WARM_A_LOOK arts a look (about 14 ms), so a card that needs every art tried (a nickname, a line not
    /// read) is named in about 23 ms, not 130. Once for a frame size; it takes some 40 looks.
    pub fn sim_cards_warm(&mut self) {
        let size = self.frame_size;
        if !self.holosim_space || self.args.replay || self.fight.is_some() || self.bar.is_some() || size.0 == 0 || size.1 == 0 || self.sim.warm == Some(size) {
            return;
        }
        let Some(ic) = self.icons.as_ref().filter(|i| !i.cards.is_empty()) else { return };
        let (w, h) = (size.0 as f64, size.1 as f64);
        let u = w.min(h * 16.0 / 9.0);
        let mut budget = SIM_WARM_A_LOOK;
        let left: usize = sim_card_boxes(u, (w - u) / 2.0, h).into_iter().map(|(window, art)| ic.warm_cards(window, art, 36, &mut budget)).sum();
        if left == 0 {
            log::info!(target: "meter", "Holo-Battle Sim: every Aniimo's card art is ready for the Victory screen ({} arts)", ic.cards.len());
            self.sim.warm = Some(size);
        }
    }

    /// The Victory screen's team named: the cards not known by their own line get every Aniimo's art tried. Slow
    /// (0.1 s a card and more), so it runs when the win is sent, with control already given back.
    pub fn sim_cards_finish(&mut self) {
        let Some(c) = self.sim.cards.take() else { return };
        let (u, ux, h) = c.at;
        let species = self.sim_species(&c.frame, u, ux, h, &c.known);
        self.sim.species_team = Some(parse_sim_team(&c.text, &c.names, &species).get("team").clone());
    }

    /// The Victory screen's pictures matched to the game's own: the Training Style (Core gizmo), the Alpha
    /// Transformation, and each Holo-gizmo row icon's series. {} without the game's icons.
    pub fn sim_pictures(&self, im: &Image, u: f64, ux: f64, h: f64, dy: f64) -> Dict {
        let mut out = Dict::new();
        let Some(ic) = self.icons.as_ref().filter(|i| i.has_sim_art()) else {
            log::info!(target: "meter", "Holo-Battle Sim pictures: not read (the game's icons {})", self.icons_state.as_deref().unwrap_or("not loaded"));
            return out;
        };
        let b = |cx: f64, cy: f64, r: f64| (ux + u * (cx - r), h * (cy + dy) - u * r, ux + u * (cx + r), h * (cy + dy) + u * r);
        let core = ic.sim_picture(im, b(0.571, 0.41, 0.04), "core");
        if let Some(c) = &core
            && c.1 >= 0.3
            && c.2 >= 0.1
        {
            out.insert(s("style"), jstr(&ic.sim, &c.0, "name"));
            out.insert(s("core"), jstr(&ic.sim, &c.0, "core"));
        }
        let tf = ic.sim_picture(im, b(0.472, 0.41, 0.032), "transform");
        if let Some(t) = &tf
            && t.1 >= 0.35
            && t.2 >= 0.1
        {
            out.insert(s("transform"), jstr(&ic.sim, &t.0, "transform"));
        }
        let mut row: Option<Vec<String>> = Some(vec![]);
        for i in 0..SIM_GIZMOS {
            let cx = SIM_GIZMO_X + SIM_GIZMO_PITCH * i as f64;
            let Some(g) = ic.sim_glyph(im, (ux + u * (cx - 0.02), h * (0.512 + dy), ux + u * (cx + 0.02), h * (0.578 + dy))) else { break };
            let r = row.as_mut().unwrap();
            let num = |x: &str| x.parse::<i64>().unwrap_or(0);
            if g.1 < 0.45 || g.2 < 0.05 || r.last().is_some_and(|l| num(&g.0) <= num(l)) {
                row = None;
                break;
            }
            r.push(g.0);
        }
        if let Some(r) = row.as_ref().filter(|r| !r.is_empty()) {
            out.insert(s("gizmos"), Value::List(r.iter().map(|sid| alogs_pyval::dict([("series", jstr(&ic.sim, sid, "name")), ("sid", s(sid))])).collect()));
        }
        let m3 = |m: &Option<(String, f64, f64)>| m.as_ref().map_or("None".into(), |m| format!("('{}', {}, {})", m.0, Value::Float(m.1).repr(), Value::Float(m.2).repr()));
        let rows = match &row {
            Some(r) => Value::List(r.iter().map(|x| s(x)).collect()).repr(),
            None => "None".into(),
        };
        log::info!(target: "meter", "Holo-Battle Sim pictures{}: core {}, transform {}, series row {rows}", if dy != 0.0 { " (lower)" } else { "" }, m3(&core), m3(&tf));
        out
    }

    /// Each Aniimo's skills noted before this run, if seen in the last 2 hours.
    pub fn sim_prep_for(&self) -> Dict {
        let mut out = Dict::new();
        if let Some(p) = &self.sim.prep
            && self.env.now() - p.at("at").as_f64().unwrap_or(0.0) <= 7200.0
            && let Some(sk) = p.get_str("skills")
        {
            out.insert(s("skills"), sk.clone());
        }
        out
    }

    /// Holo-Battle Sim's end screen: "Victory" with "Battle time 00:05:25 Earth Training - Hard" under it.
    pub fn sim_result(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64, u: f64, ux: f64, h: f64) {
        if now - self.sim.seen < 30.0 {
            return;
        }
        let k = u / 2000.0;
        let bx = (ux + u * 0.36, h * 0.198, ux + u * 0.65, h * 0.233);
        let tb = (ux + u * 0.43, h * 0.13, ux + u * 0.57, h * 0.20);
        let mut crops: IndexMap<String, Image> = IndexMap::new();
        crops.insert("w".into(), prep::prep_white(im, bx, k, 4.0));
        crops.insert("b".into(), prep::prep_bright(im, bx, k, 2.0));
        crops.insert("title".into(), prep::prep_bright(im, tb, k, 2.0));
        crops.insert("title2".into(), prep::prep_text(im, tb, k, 200, 3.0, 24));
        crops.extend(sim_team_crops(im, u, ux, h));
        let t = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
        let get = |k: &str| t.get(k).map(String::as_str);
        let Some(sim) = parse_sim(get("w")).or_else(|| parse_sim(get("b"))) else {
            log::info!(target: "meter", "Holo-Battle Sim end screen not read: {}", str_map_repr(&t));
            return;
        };
        let title = format!("{} {}", get("title").unwrap_or(""), get("title2").unwrap_or("")).to_lowercase();
        let names = self.icon_names();
        let team = parse_sim_team(&t, &names, &[]);
        self.sim.species_team = None;
        let n_team = team.get("team").as_list().map_or(0, Vec::len);
        let won = regex::Regex::new(r"victo|v[il1]\W?[c<(]?t?ory").unwrap().is_match(&title) || (!title.contains("defe") && n_team >= 3);
        let filled = |k: &str| get(k).is_some_and(|v| !v.is_empty());
        if won && !((filled("coins") || filled("coinsL")) && filled("share")) {
            let first = match self.sim.first {
                Some(f) if now - f <= 30.0 => f,
                _ => {
                    self.sim.first = Some(now);
                    now
                }
            };
            if now - first < 1.5 {
                return;
            }
        }
        self.sim.seen = now;
        log::info!(target: "meter", "Holo-Battle Sim: {} ({}), {} - {}, battle time {} s", if won { "victory" } else { "not a victory" },
            get("title").map_or("None".into(), alogs_pyval::str_repr), sim.get("trial").py_str(), sim.get("difficulty").py_str(), sim.get("time_s").py_str());
        if !won {
            self.sim_release("not a victory");
            return;
        }
        if self.fight.is_some() {
            self.end_fight(out, "Victory screen");
        }
        let key = format!("{:?}|{}|{}|{}", self.space, sim.get("trial").repr(), sim.get("difficulty").repr(), sim.get("time_s").repr());
        if self.sim.last.as_ref() == Some(&key) {
            if self.sim.ctl.as_ref().is_some_and(|c| c.stage == Some("spot")) {
                self.sim_release("this win is already recorded");
            }
            return;
        }
        self.sim.last = Some(key);
        let mut team = team.as_dict().cloned().unwrap_or_default();
        let mut low = team.pop("low").is_some_and(|v| v.truthy());
        let mut pictures = self.sim_pictures(im, u, ux, h, if low { SIM_LOW } else { 0.0 });
        if pictures.get_str("gizmos").is_none() {
            let other = self.sim_pictures(im, u, ux, h, if low { 0.0 } else { SIM_LOW });
            if other.get_str("gizmos").is_some() {
                pictures = other;
                low = !low;
            }
        }
        self.sim.low = low;
        let mut setup = team.clone();
        for (k, v) in pictures.iter().chain(self.sim_prep_for().iter()) {
            setup.insert(k.clone(), v.clone());
        }
        // each card's Aniimo: here only where its own line names it and the art agrees (a few ms); the cards left are
        // named from every Aniimo's art when the win is sent, after control (sim_cards_finish)
        let known = self.sim_species_named(im, u, ux, h, &t, &names);
        self.sim.cards = Some(SimCards { frame: im.clone(), at: (u, ux, h), text: t.clone(), names, known });
        let mut counts: Vec<Value> = setup.pop("gizmoCounts").and_then(|v| v.as_list().cloned()).unwrap_or_default();
        let gizmos: Vec<Value> = setup.at("gizmos").as_list().cloned().unwrap_or_default();
        if let Some(reader) = self.name_reader() {
            let shaped = sim_badge_counts(im, u, ux, h, reader, if low { SIM_LOW } else { 0.0 }, SIM_GIZMOS);
            let n = if gizmos.is_empty() { shaped.iter().position(Option::is_none).unwrap_or(shaped.len()) } else { gizmos.len() };
            if n > 0 && n <= shaped.len() && shaped[..n].iter().all(Option::is_some) {
                let sh: Vec<Value> = shaped[..n].iter().map(|v| Value::Int(v.unwrap())).collect();
                if Value::List(sh.clone()).repr() != Value::List(counts.clone()).repr() {
                    log::info!(target: "meter", "Holo-Battle Sim counts by the game's digits {} (OCR line {})", Value::List(sh.clone()).repr(), Value::List(counts.clone()).repr());
                }
                counts = sh;
            }
        }
        let g = if !gizmos.is_empty() && gizmos.len() == counts.len() {
            Value::List(gizmos.iter().zip(&counts).map(|(g, n)| set(g, "count", n.clone())).collect())
        } else if !gizmos.is_empty() {
            log::info!(target: "meter", "Holo-Battle Sim: {} gizmo series but {} counts read {}: counts left out", gizmos.len(), counts.len(), Value::List(counts.clone()).repr());
            Value::List(gizmos.iter().map(|g| set(g, "count", Value::None)).collect())
        } else if counts.is_empty() {
            Value::None
        } else {
            Value::List(counts.iter().map(|n| alogs_pyval::dict([("count", n.clone())])).collect())
        };
        setup.insert(s("gizmos"), g);
        log::info!(target: "meter", "Holo-Battle Sim setup: {}", Value::Dict(setup.clone()).repr());
        let mut sim = sim.as_dict().cloned().unwrap_or_default();
        sim.insert(s("setup"), Value::Dict(setup));
        // control first: the proof picture waits for the screen to stand still and is encoded (0.37 s on 17 Nov), and
        // came before the inputs were held. The tour's first step is a look later, so the picture still shows the
        // Victory screen as it was read.
        self.sim.pending = Some((Value::Dict(sim), None));
        let toured = self.sim_tour_start(out, now);
        let shot = self.picture(frames, im, "screens", &format!("sim_{}", local(now, "%Y%m%d_%H%M%S")), true, true);
        if let Some(p) = self.sim.pending.as_mut() {
            p.1 = shot;
        }
        if !toured {
            self.sim_send(out, None);
        }
    }

    fn sim_species_team(&mut self, sim: &mut Dict) {
        if let Some(team) = self.sim.species_team.take().filter(Value::truthy) {
            let setup = set(sim.at("setup"), "team", team);
            sim.insert(s("setup"), setup);
        }
    }

    /// Note and send the Holo-Battle Sim win waiting in `sim.pending`.
    pub fn sim_send(&mut self, out: &mut dyn Out, note: Option<&str>) {
        self.sim_cards_finish();
        let Some((sim, shot)) = self.sim.pending.take() else { return };
        let mut sim = sim.as_dict().cloned().unwrap_or_default();
        if self.sim.test {
            self.sim.test = false;
            self.sim_species_team(&mut sim);
            out.out("toast", alogs_pyval::dict([("head", s("Holo-Battle Sim test done")),
                ("detail", s(&format!("{} · nothing sent", note.unwrap_or("no Gizmo screen read")))),
                ("accent", s("#9be27a")), ("screen", Value::None), ("kind", s("notice"))]));
            self.sim_release("the test is done");
            return;
        }
        self.sim_species_team(&mut sim);
        let shots: Vec<String> = sim.pop("gizmo_shots").and_then(|v| v.as_list().cloned()).unwrap_or_default().iter()
            .filter(|p| p.truthy()).map(|p| rel(&self.env.data, std::path::Path::new(&p.py_str()))).collect();
        let mut setup = sim.at("setup").as_dict().cloned().unwrap_or_default();
        if let Some(gs) = setup.at("gizmos").as_list().cloned() {
            let gs = gs.into_iter().map(|g| { let mut d = g.as_dict().cloned().unwrap_or_default(); d.pop("sid"); Value::Dict(d) }).collect();
            setup.insert(s("gizmos"), Value::List(gs));
            sim.insert(s("setup"), Value::Dict(setup));
        }
        let shot_rel = shot.as_ref().map(|p| rel(&self.env.data, p));
        if self.env.write_files {
            let path = self.env.data.join("logs").join("sim_runs.csv");
            let _ = std::fs::create_dir_all(self.env.data.join("logs"));
            let new = !path.exists();
            if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                let mut w = csv::WriterBuilder::new().terminator(csv::Terminator::CRLF).from_writer(f);
                if new {
                    let _ = w.write_record(["time", "uid", "trial", "difficulty", "time_s", "screenshot"]);
                }
                let _ = w.write_record([local(self.env.now(), "%Y-%m-%d %H:%M:%S"), self.uid.clone().unwrap_or_else(|| "None".into()),
                    sim.at("trial").py_str(), sim.at("difficulty").py_str(), sim.at("time_s").py_str(), shot_rel.clone().unwrap_or_default()]);
            }
        }
        if self.submitter.is_some()
            && let Some(uid) = self.uid.clone()
        {
            let clock = self.env.record_clock();
            let mut run = sim.clone();
            if let Some(g) = self.game_build_now() {
                run.insert(s("game"), g);
            }
            let p = payloads::sim_payload(&Value::Dict(run), &Value::Str(uid), None, &clock);
            self.send_result(&p, shot_rel.as_deref(), None, None, Some(&shots));
        }
        let msg = format!("Holo-Battle Sim recorded: {} - {} in {} s{}", sim.at("trial").py_str(), sim.at("difficulty").py_str(), sim.at("time_s").py_str(),
            note.map_or(String::new(), |n| format!(" ({n})")));
        self.screen_settled("sim_result", self.env.now());
        out.out("status", Value::Str(msg));
        self.sim_release("the run is saved");
    }

}

/// A `{str: str}` dict's repr.
pub fn str_map_repr(t: &IndexMap<String, String>) -> String {
    Value::Dict(t.iter().fold(Dict::new(), |mut d, (k, v)| {
        d.insert(s(k), s(v));
        d
    }))
    .repr()
}
