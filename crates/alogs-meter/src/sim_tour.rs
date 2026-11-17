//! Holo-Battle Sim's end with aLogs in control (Leaderboards › Automation): the exit gate watched for your own press,
//! control taken (inputs held, the controller kept away), and the Gizmo screen toured after a win: All clicked, the
//! list captured as it scrolls, read after control is given back. Port of those parts of aLogs B16
//! `bossbar/leaderboards.py` and `live.py` (the GIZMO_* spots).

use crate::meter::{Meter, Out};
use crate::results::local;
use crate::results_parse::{SIM_GIZMO_X, SIM_LOW};
use alogs_capture::win;
use alogs_pil::ops::{Stat, difference};
use alogs_pil::{Filter, Image, Mode};
use alogs_pyval::{Value, dict};
use alogs_reader::prep;
use indexmap::IndexMap;
use std::path::PathBuf;

pub const SIM_GATE_GONE: f64 = 3.0;
pub const SIM_PRESS_EARLY: f64 = 1.0;
pub const SIM_MOUSE_WAIT: f64 = 3.0;
pub const SIM_VICTORY_WAIT: f64 = 8.0;
pub const GIZMO_LIST_BOX: (f64, f64, f64, f64) = (0.215, 0.09, 0.97, 0.96);
pub const GIZMO_TITLE: (f64, f64, f64, f64) = (0.06, 0.03, 0.14, 0.085);
pub const GIZMO_ALL: (f64, f64) = (0.09, 0.16);
pub const GIZMO_LIST: (f64, f64, f64, f64) = (0.305, 0.09, 0.60, 0.96);
pub const GIZMO_BACK: (f64, f64) = (0.05, 0.056);
pub const SIM_PROOF_MAX: usize = 20;

/// aLogs has the Sim's end.
pub struct SimCtl {
    pub until: f64,
    pub why: String,
    pub held: bool,
    pub space: Option<String>,
    pub rewasd: bool,
    pub stage: Option<&'static str>,
    pub done: Option<f64>,
}

#[derive(Default)]
pub struct GateState {
    space: Option<String>,
    pub last: bool,
    first_up: Option<f64>,
    up_at: f64,
    used: f64,
    look: f64,
}

#[derive(Default)]
pub struct Tour {
    stage: &'static str,
    next: f64,
    tries: i64,
    read: IndexMap<String, Vec<String>>,
    scrolls: i64,
    shots: Vec<String>,
    want: Option<i64>,
    mouse_from: Option<f64>,
    wiggled: f64,
    title: String,
    waited: i64,
    backs: i64,
    all_clicked: bool,
    all_reads: Vec<i64>,
    all_looks: i64,
    all_n: Option<i64>,
    sig: Option<Image>,
    moved: Option<f64>,
    sigs: Vec<Image>,
    stuck: i64,
    views: Vec<Image>,
    view_k: f64,
}

/// The Gizmo list's text, small (64 x 72): two views with the same signature show the same rows at the same place.
pub fn gizmo_list_sig(im: &Image) -> Image {
    let (w, h) = (im.width as f64, im.height as f64);
    let u = w.min(h * 16.0 / 9.0);
    let ux = (w - u) / 2.0;
    let b = GIZMO_LIST_BOX;
    let c = im.crop_i((ux + u * b.0) as i64, (h * b.1) as i64, (ux + u * b.2) as i64, (h * b.3) as i64).convert(Mode::L);
    c.point(|v| if v < 130 { 255.0 } else { 0.0 }).resize(64, 72, Some(Filter::Box))
}

/// How far apart two Gizmo list signatures are (mean pixel difference): 0 the same view, 9.9+ a scroll apart.
pub fn gizmo_list_diff(a: &Image, b: &Image) -> f64 {
    Stat::new(&difference(a, b), None).mean()[0]
}

pub fn gizmo_list_same(a: Option<&Image>, b: Option<&Image>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if gizmo_list_diff(a, b) < 1.5)
}

/// Is the exit gate's button there: its dark blue-grey pill, not the gate's floating label?
pub fn sim_gate_pill(im: &Image, b: (f64, f64, f64, f64), u: f64) -> bool {
    let band = im.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64).convert(Mode::Rgb);
    let w = 400usize;
    let h = ((band.height as f64 * w as f64 / band.width.max(1) as f64).round_ties_even() as usize).max(1);
    let band = band.resize(w, h, Some(Filter::Box));
    let scale = w as f64 / (b.2 - b.0);
    let need = 0.04 * u * scale;
    let (mut rows, mut gap, mut best) = (0usize, 0usize, 0usize);
    for y in 0..h {
        let mut n = 0usize;
        for x in 0..w {
            let i = (y * w + x) * 3;
            let (r, g, bb) = (band.data[i] as i32, band.data[i + 1] as i32, band.data[i + 2] as i32);
            if (55..=100).contains(&r) && (55..=100).contains(&g) && bb <= 125 && bb - r >= 8 && bb - g >= 8 {
                n += 1;
            }
        }
        if n as f64 >= need {
            rows += 1;
            gap = 0;
        } else {
            gap += 1;
            if gap as f64 > 0.008 * u * scale {
                rows = 0;
            }
        }
        best = best.max(rows);
    }
    best as f64 >= 0.012 * u * scale
}

/// The picks a series' rows stand for: an upgraded Core ("...: Final") counts 3, one with α 2.
pub fn sim_count(rows: &[String]) -> i64 {
    rows.iter().map(|n| if n.ends_with(": Final") { 3 } else if n.ends_with(" α") { 2 } else { 1 }).sum()
}

/// One view's names added to the list read so far: the longest end of `rows` the view starts with is the overlap.
pub fn sim_stitch(rows: &[String], view: &[String]) -> Vec<String> {
    let base = |names: &[String]| -> Vec<String> { names.iter().map(|n| n.split(':').next().unwrap_or("").trim_end_matches(" α").to_string()).collect() };
    for k in (1..=rows.len().min(view.len())).rev() {
        if base(&rows[rows.len() - k..]) == base(&view[..k]) {
            return rows.iter().chain(&view[k..]).cloned().collect();
        }
    }
    rows.iter().chain(view).cloned().collect()
}

fn norm(x: &str) -> String {
    x.to_lowercase().chars().filter(char::is_ascii_lowercase).collect()
}

/// The gizmo names in one view of the list, matched to the game data's names (`names`: name -> series id).
pub fn sim_names(text: &str, names: &[(String, String)], sid: Option<&str>) -> Vec<String> {
    let names: Vec<&String> = names.iter().filter(|(_, s)| sid.is_none_or(|x| x == s)).map(|(n, _)| n).collect();
    let tags = regex::Regex::new(r"\b(Normal|Uncommon|Core)\b").unwrap();
    let fin = regex::Regex::new(r"(?i):?\s*final\s*$").unwrap();
    let alpha_re = regex::Regex::new(r"\s(?:α|a)\s*$").unwrap();
    let mut out = vec![];
    for line in text.split(" / ") {
        let line = tags.replace_all(line, "").trim().to_string();
        let final_ = norm(&line).ends_with("final");
        let mut head = if final_ { fin.replace(&line, "").into_owned() } else { line.split(':').next().unwrap_or("").to_string() };
        if norm(&head).chars().count() < 4 || names.is_empty() {
            continue;
        }
        let alpha = !final_ && alpha_re.is_match(&head);
        if alpha {
            head = alpha_re.replace(&head, "").into_owned();
        }
        let h = norm(&head);
        let (score, name) = names.iter().map(|n| (alogs_gamelog::difflib::ratio(&h, &norm(n)), (*n).clone())).fold((-1.0, String::new()), |b, c| if (c.0, &c.1) > (b.0, &b.1) { c } else { b });
        if score < 0.85 {
            continue;
        }
        out.push(if final_ && !norm(&name).ends_with("final") { format!("{name}: Final") } else if alpha { format!("{name} α") } else { name });
    }
    out
}

impl Meter {
    fn gizmo_names(&self) -> Vec<(String, String)> {
        self.icons.as_ref().and_then(|i| i.sim_gizmos.as_object()).map(|o| o.iter().map(|(n, v)| (n.clone(), v.get(0).map_or(String::new(), |s| s.as_str().map_or_else(|| s.to_string(), String::from)))).collect()).unwrap_or_default()
    }

    fn sim_rarity(&self, name: &str) -> Value {
        let g = self.icons.as_ref().map(|i| &i.sim_gizmos);
        let get = |n: &str| g.and_then(|g| g.get(n)).and_then(|v| v.get(1)).and_then(|r| r.as_str()).map(|s| Value::Str(s.into()));
        get(name).or_else(|| get(name.split(':').next().unwrap_or("").trim())).unwrap_or(Value::None)
    }

    /// aLogs takes control for the Sim's end: inputs held (and the controller kept away) as set, for at most the
    /// automation's seconds. Kept when it's already on.
    pub fn sim_control(&mut self, now: f64, why: &str) {
        if self.sim.ctl.is_some() {
            return;
        }
        let r = self.args.cfg.get("results").clone();
        let seconds = crate::leaderboards::HOLD_MIN.max(120f64.min(alogs_pyval::py_int(r.get("hold_seconds")).filter(|v| *v != 0).unwrap_or(60) as f64));
        let held = r.get("hold_inputs").truthy();
        let mut rewasd = false;
        if held {
            self.hold.start(seconds);
            self.hold.freeze_moves(seconds);
            rewasd = alogs_capture::rewasd::hold(alogs_capture::rewasd::config(&self.env.data).as_ref(), true);
            if !rewasd
                && let Some(h) = self.hwnd
                && !self.args.replay
            {
                self.focus_guard.take(h, seconds);
            }
        }
        if r.get("hold_controller").truthy() && alogs_capture::input::hidhide_available() {
            alogs_capture::input::hide_controllers(true);
        }
        log::info!(target: "meter", "Holo-Battle Sim: aLogs has control ({why}), for at most {seconds} s");
        self.sim.ctl = Some(SimCtl { until: now + seconds, why: why.to_string(), held, space: self.space.clone(), rewasd, stage: None, done: None });
    }

    /// Control given back.
    pub fn sim_release(&mut self, why: &str) {
        let Some(ctl) = self.sim.ctl.take() else { return };
        self.hold.stop();
        self.focus_guard.stop();
        if ctl.rewasd {
            alogs_capture::rewasd::hold(alogs_capture::rewasd::config(&self.env.data).as_ref(), false);
        }
        if alogs_capture::input::controllers_hidden() {
            alogs_capture::input::hide_controllers(false);
        }
        log::info!(target: "meter", "Holo-Battle Sim: control given back ({why})");
    }

    /// Every frame while aLogs has the Sim's end: control comes back the moment it isn't going as planned.
    pub fn sim_check(&mut self, now: f64) {
        let Some(ctl) = &self.sim.ctl else { return };
        if self.sim.tour.is_some() {
            return;
        }
        if now > ctl.until {
            self.sim_release("time limit");
        } else if ctl.held && !self.hold.active() && now - ctl.until < -1.0 {
            self.sim_release("you took control back (Ctrl+F12)");
        } else if ctl.stage == Some("gate") && ctl.done.is_some_and(|d| now - d > SIM_VICTORY_WAIT) {
            self.sim_release(&format!("no Victory screen {SIM_VICTORY_WAIT} s after your press at the gate"));
        }
    }

    /// Ctrl+Shift+F11 on a Victory screen: read it again as a new win and tour the Gizmo screen, nothing sent.
    pub fn sim_test_key(&mut self) {
        let down = alogs_capture::input::key_down(0x7A) && alogs_capture::input::key_down(0x11) && alogs_capture::input::key_down(0x10);
        let was = std::mem::replace(&mut self.sim.test_down, down);
        if !down || was || self.sim.tour.is_some() || self.sim.ctl.is_some() {
            return;
        }
        self.sim.test = true;
        self.sim.seen = 0.0;
        self.sim.last = None;
        self.sim.first = None;
        self.sim.test_pending_end = true;
        log::info!(target: "meter", "Holo-Battle Sim test (Ctrl+Shift+F11): reading the Victory screen again and touring the Gizmo screen; nothing is sent");
    }

    /// The exit gate's "End Holo-Battle Sim" button on the last level: aLogs presses nothing, it watches for your own
    /// press of it (Y or F) and takes control from that press, so the Victory screen can't be skipped.
    pub fn sim_gate(&mut self, im: &Image, now: f64, foreground: bool) {
        if !self.holosim_space || self.args.replay || self.hwnd.is_none() || !foreground || !alogs_protocol::settings::automation(&self.args.cfg, Some("sim_gate")) || self.sim.tour.is_some() {
            return;
        }
        if self.sim.gate.space != self.space {
            self.sim.gate = GateState { space: self.space.clone(), ..Default::default() };
        }
        if self.sim.gate.last {
            self.sim.press.start();
        }
        let g = &self.sim.gate;
        if let Some((_, at)) = self.sim.press.last()
            && let Some(first) = g.first_up
            && at >= first - SIM_PRESS_EARLY
            && at > g.used
            && now - g.up_at <= SIM_GATE_GONE
            && self.sim.ctl.is_none()
        {
            self.sim.gate.used = at;
            self.sim.press.stop();
            self.sim_control(now, "your press at the exit gate");
            if let Some(c) = self.sim.ctl.as_mut() {
                c.stage = Some("gate");
                c.done = Some(at);
            }
            self.sim.gate.first_up = None;
            self.sim.gate.up_at = 0.0;
            log::info!(target: "meter", "Holo-Battle Sim: waiting for the Victory screen");
            return;
        }
        if now - self.sim.gate.look < 0.15 {
            return;
        }
        self.sim.gate.look = now;
        let (w, h) = (im.width as f64, im.height as f64);
        let u = w.min(h * 16.0 / 9.0);
        let ux = (w - u) / 2.0;
        let k = u / 2000.0;
        if !self.sim.gate.last {
            let mut c = IndexMap::new();
            c.insert("l".to_string(), prep::prep_bright(im, (ux + u * 0.02, h * 0.015, ux + u * 0.20, h * 0.08), k, 2.0));
            let t = self.ocr.read(&c, &IndexMap::new(), &IndexMap::new()).get("l").cloned().unwrap_or_default();
            let m = regex::Regex::new(r"(\d{1,2})\s*/\s*(\d{1,2})").unwrap().captures(&t).map(|m| (m[1].to_string(), m[2].to_string()));
            self.sim.gate.last = m.is_some_and(|(a, b)| a == b && a.parse::<i64>().unwrap_or(0) > 1);
            if !self.sim.gate.last {
                return;
            }
            log::info!(target: "meter", "Holo-Battle Sim: last level ({}): the exit gate is watched", alogs_pyval::str_repr(&t));
        }
        if self.sim.ctl.is_some() {
            return;
        }
        let b = (ux + u * 0.50, h * 0.40, ux + u * 0.90, h * 0.66);
        let up = sim_gate_pill(im, b, u) && {
            let mut c = IndexMap::new();
            c.insert("b".to_string(), prep::prep_bright(im, b, k, 2.0));
            norm(&self.ocr.read(&c, &IndexMap::new(), &IndexMap::new()).get("b").cloned().unwrap_or_default()).contains("endholo")
        };
        let gt = &mut self.sim.gate;
        if up {
            if gt.first_up.is_none() {
                gt.first_up = Some(now);
                log::info!(target: "meter", "Holo-Battle Sim: the exit gate button is up; watching for your Y or F");
            }
            gt.up_at = now;
        } else if gt.first_up.is_some() && now - gt.up_at > SIM_GATE_GONE {
            gt.first_up = None;
            gt.up_at = 0.0;
        }
    }

    /// After a win, open the Gizmo screen and read every gizmo picked: only with the automation on, the game in front
    /// and the game's gizmo names known. False when it doesn't start.
    pub fn sim_tour_start(&mut self, out: &mut dyn Out, now: f64) -> bool {
        let fg = self.hwnd.is_some_and(win::is_foreground);
        let known = self.icons.as_ref().is_some_and(|i| i.sim_gizmos.as_object().is_some_and(|o| !o.is_empty()));
        if !alogs_protocol::settings::automation(&self.args.cfg, Some("sim_gizmos")) || self.args.replay || !fg || !known {
            return false;
        }
        let gizmos = self.sim.pending.as_ref().map(|p| p.0.get("setup").get("gizmos").as_list().cloned().unwrap_or_default()).unwrap_or_default();
        self.sim_control(now, "the Victory screen");
        let test = self.sim.test;
        out.out("toast", dict([("head", Value::Str(format!("Scanning the Holo-Battle Sim results{}", if test { " (test)" } else { "" }))),
            ("detail", Value::Str("aLogs has control for a moment: reading your gizmos".into())), ("accent", Value::Str("#9be27a".into())),
            ("screen", Value::None), ("kind", Value::Str("notice".into()))]));
        let want = (!gizmos.is_empty() && gizmos.iter().all(|g| !g.get("count").is_none())).then(|| gizmos.iter().map(|g| g.get("count").as_i64().unwrap_or(0)).sum());
        log::info!(target: "meter", "Holo-Battle Sim: opening the Gizmo screen for {} gizmos", want.map_or("all its".to_string(), |w: i64| w.to_string()));
        self.sim.tour = Some(Tour { stage: "open", next: now + 0.3, want, ..Default::default() });
        true
    }

    fn sim_tour_names(&mut self, tour: &mut Tour) {
        let names = self.gizmo_names();
        let mut rows: Vec<String> = vec![];
        for view in &tour.views {
            let mut c = IndexMap::new();
            c.insert("n".to_string(), prep::prep_grey(view, (0.0, 0.0, view.width as f64, view.height as f64), tour.view_k, 2.0, 24));
            let text = self.ocr.read(&c, &IndexMap::new(), &IndexMap::new()).get("n").cloned().unwrap_or_default();
            rows = sim_stitch(&rows, &sim_names(&text, &names, None));
        }
        for n in &rows {
            let key = n.split(':').next().unwrap_or("").trim_end_matches(" α").trim();
            if let Some((_, sid)) = names.iter().find(|(name, _)| name == key) {
                tour.read.entry(sid.clone()).or_default().push(n.clone());
            }
        }
        if let Some(all) = tour.all_n
            && sim_count(&rows) != all
        {
            log::info!(target: "meter", "Holo-Battle Sim Gizmo screen: {} picks read in {} views, but All says {all}", sim_count(&rows), tour.views.len());
        }
    }

    fn sim_tour_end(&mut self, out: &mut dyn Out, why: &str) {
        let Some(mut tour) = self.sim.tour.take() else { return };
        if !tour.views.is_empty() {
            self.sim_release("the Gizmo screen is captured");
            self.sim_tour_names(&mut tour);
        }
        self.sim.seen = self.env.now();
        let mut got = 0;
        if let Some((sim, shot)) = self.sim.pending.take() {
            let mut sim = sim.as_dict().cloned().unwrap_or_default();
            let mut setup = sim.at("setup").as_dict().cloned().unwrap_or_default();
            let known: Vec<Value> = setup.at("gizmos").as_list().cloned().unwrap_or_default();
            if !tour.read.is_empty() && (known.is_empty() || !known.iter().all(|g| g.get("sid").truthy())) {
                let counts: Vec<Value> = known.iter().map(|g| g.get("count").clone()).collect();
                let mut sids: Vec<&String> = tour.read.keys().collect();
                sids.sort_by_key(|s| s.parse::<i64>().unwrap_or(0));
                let n = sids.len();
                let series = |s: &str| self.icons.as_ref().and_then(|i| i.sim.get(s)).and_then(|v| v.get("name")).and_then(|v| v.as_str()).map_or(Value::None, |x| Value::Str(x.into()));
                setup.insert("gizmos".into(), Value::List(sids.iter().enumerate().map(|(i, s)| dict([("series", series(s)), ("sid", Value::Str((*s).clone())),
                    ("count", if counts.len() == n { counts[i].clone() } else { Value::None })])).collect()));
            }
            let gizmos: Vec<Value> = setup.at("gizmos").as_list().cloned().unwrap_or_default().into_iter().map(|g| {
                let mut d = g.as_dict().cloned().unwrap_or_default();
                if let Some(rows) = tour.read.get(&d.at("sid").py_str()) {
                    d.insert("picked".into(), Value::List(rows.iter().map(|n| dict([("name", Value::Str(n.clone())), ("rarity", self.sim_rarity(n))])).collect()));
                    d.insert("complete".into(), Value::Bool(Value::Int(sim_count(rows)).py_eq(d.at("count"))));
                    got += sim_count(rows);
                }
                Value::Dict(d)
            }).collect();
            let cores: Vec<Value> = gizmos.iter().flat_map(|g| g.get("picked").as_list().cloned().unwrap_or_default()).filter(|p| p.get("rarity").as_str() == Some("Core")).map(|p| p.get("name").clone()).collect();
            if !gizmos.is_empty() {
                setup.insert("gizmos".into(), Value::List(gizmos));
            }
            if !cores.is_empty() {
                setup.insert("cores".into(), Value::List(cores));
            }
            sim.insert("setup".into(), Value::Dict(setup));
            sim.insert("gizmo_shots".into(), Value::List(tour.shots.iter().map(|s| Value::Str(s.clone())).collect()));
            self.sim.pending = Some((Value::Dict(sim), shot));
        }
        let of = tour.want.map_or(String::new(), |w| format!(" of {w}"));
        let read: Vec<String> = tour.read.iter().map(|(s, r)| format!("{s}: {}", Value::List(r.iter().map(|x| Value::Str(x.clone())).collect()).repr())).collect();
        log::info!(target: "meter", "Holo-Battle Sim Gizmo screen: {why}; read {got}{of} gizmos: {}", read.join("; "));
        let note = match tour.want {
            Some(w) if got != w => format!("{got} of {w} gizmos"),
            _ => format!("{got} gizmos"),
        };
        self.sim_send(out, Some(&note));
        self.sim_release("the Gizmo screen is read");
    }

    fn sim_tour_shot(&mut self, frames: &mut dyn crate::meter::Frames, im: &Image, what: &str) -> Option<String> {
        let n = self.sim.tour.as_ref()?.shots.len();
        if n >= SIM_PROOF_MAX {
            return None;
        }
        let name = format!("sim_gizmo_{}_{n:02}_{}", local(self.env.now(), "%Y%m%d_%H%M%S"), what.chars().filter(char::is_ascii_alphanumeric).collect::<String>());
        self.picture(frames, im, "screens", &name, true, false).map(|p: PathBuf| p.to_string_lossy().into_owned())
    }

    /// One step of the Gizmo tour on each new frame while it runs. True while the tour has the screen.
    pub fn sim_tour_step(&mut self, frames: &mut dyn crate::meter::Frames, out: &mut dyn Out, im: &Image, now: f64) -> bool {
        if self.sim.tour.is_none() {
            return false;
        }
        let ctl_ok = self.sim.ctl.as_ref().map(|c| (now <= c.until, c.held, c.space.clone()));
        let Some((in_time, held, space)) = ctl_ok else {
            self.sim_tour_end(out, "time limit");
            return false;
        };
        if !in_time {
            self.sim_tour_end(out, "time limit");
            return false;
        }
        if held && !self.hold.active() {
            self.sim_tour_end(out, "you took control back (Ctrl+F12)");
            return false;
        }
        if !self.hwnd.is_some_and(win::is_foreground) {
            self.sim_tour_end(out, "game is not the active window");
            return false;
        }
        if space.is_some() && self.space != space {
            self.sim_tour_end(out, "the game left the Victory screen");
            return false;
        }
        if now < self.sim.tour.as_ref().unwrap().next {
            return true;
        }
        let (w, h) = (im.width as f64, im.height as f64);
        let u = w.min(h * 16.0 / 9.0);
        let ux = (w - u) / 2.0;
        let k = u / 2000.0;
        let (ox, oy) = (f64::from(self.origin.0), f64::from(self.origin.1));
        let at = |x: f64, y: f64| ((ox + ux + u * x) as i32, (oy + h * y) as i32);
        let read1 = |m: &Self, key: &str, img: Image| -> String {
            let mut c = IndexMap::new();
            c.insert(key.to_string(), img);
            m.ocr.read(&c, &IndexMap::new(), &IndexMap::new()).get(key).cloned().unwrap_or_default()
        };
        let stage = self.sim.tour.as_ref().unwrap().stage;
        if stage == "open" {
            let spot = at(SIM_GIZMO_X, 0.545 + if self.sim.low { SIM_LOW } else { 0.0 });
            let prompt = norm(&read1(self, "p", prep::prep_bright(im, (ux + u * 0.38, h * 0.93, ux + u * 0.62, h * 0.98), k, 2.0)));
            let tour = self.sim.tour.as_mut().unwrap();
            if !prompt.contains("anywhere") && !prompt.contains("close") {
                let from = *tour.mouse_from.get_or_insert(now);
                if now - from > SIM_MOUSE_WAIT {
                    self.sim_tour_end(out, &format!("the game didn't switch to mouse mode (prompt read {})", alogs_pyval::str_repr(&prompt)));
                    return false;
                }
                if tour.wiggled == 0.0 || now - tour.wiggled > 0.8 {
                    log::info!(target: "meter", "Holo-Battle Sim: the Victory screen is in controller mode ({}): moving the mouse for mouse mode", alogs_pyval::str_repr(&prompt));
                    win::wiggle_at(spot.0, spot.1);
                    tour.wiggled = now;
                }
                tour.next = now + 0.25;
                return true;
            }
            tour.tries += 1;
            if tour.tries > 3 {
                let t = tour.title.clone();
                self.sim_tour_end(out, &format!("the Gizmo screen did not open (title read {})", alogs_pyval::str_repr(&t)));
                return false;
            }
            log::info!(target: "meter", "Holo-Battle Sim: clicking the gizmo row at ({}, {}) (try {}, mouse mode)", spot.0, spot.1, tour.tries);
            win::wiggle_at(spot.0, spot.1);
            win::click_at(spot.0, spot.1, true);
            tour.stage = "wait";
            tour.next = now + 0.3;
            tour.waited = 0;
            return true;
        }
        let g = GIZMO_TITLE;
        let title = read1(self, "t", prep::prep_grey(im, (ux + u * g.0, h * g.1, ux + u * g.2, h * g.3), k, 3.0, 24));
        let on_page = norm(&title).contains("gizmo");
        let tour = self.sim.tour.as_mut().unwrap();
        if stage == "wait" {
            tour.title = title;
            if on_page {
                tour.stage = "all";
                tour.next = now;
            } else {
                tour.waited += 1;
                tour.stage = if tour.waited >= 10 { "open" } else { "wait" };
                tour.next = now + 0.15;
            }
            return true;
        }
        if stage == "back" {
            if !on_page {
                self.sim_tour_end(out, "done");
                return true;
            }
            tour.backs += 1;
            if tour.backs > 4 {
                self.sim_tour_end(out, "the Gizmo screen did not close");
                return false;
            }
            if tour.backs % 2 == 1 {
                win::press_key(win::VK_ESCAPE);
            } else {
                let b = at(GIZMO_BACK.0, GIZMO_BACK.1);
                win::wiggle_at(b.0, b.1);
                win::click_at(b.0, b.1, true);
            }
            tour.next = now + 0.4;
            return true;
        }
        if !on_page {
            self.sim_tour_end(out, "left the Gizmo screen");
            return false;
        }
        if stage == "all" {
            if !tour.all_clicked {
                let a = at(GIZMO_ALL.0, GIZMO_ALL.1);
                win::wiggle_at(a.0, a.1);
                win::click_at(a.0, a.1, true);
                tour.all_clicked = true;
                tour.next = now + 0.3;
                return true;
            }
            let t = read1(self, "a", prep::prep_bright(im, (ux + u * 0.065, h * 0.14, ux + u * 0.135, h * 0.185), k, 2.0));
            let tour = self.sim.tour.as_mut().unwrap();
            if t.to_lowercase().contains("all")
                && let Some(m) = regex::Regex::new(r"(\d{1,3})").unwrap().captures(&t)
            {
                tour.all_reads.push(m[1].parse().unwrap_or(0));
            }
            let agreed = tour.all_reads.iter().copied().find(|n| tour.all_reads.iter().filter(|x| *x == n).count() >= 2);
            if agreed.is_none() && tour.all_reads.len() < 4 && tour.all_looks < 6 {
                tour.all_looks += 1;
                tour.next = now + 0.1;
                return true;
            }
            if let Some(a) = agreed {
                tour.all_n = Some(a);
                tour.want = Some(a);
            }
            tour.stage = "read";
            tour.next = now;
            return true;
        }
        // 'read': capture the list as it scrolls; the names are read after control is given back
        let (x0, y0, x1, y1) = GIZMO_LIST;
        let sig = gizmo_list_sig(im);
        if !gizmo_list_same(Some(&sig), tour.sig.as_ref()) && now - tour.moved.unwrap_or(now) < 0.6 {
            tour.moved.get_or_insert(now);
            tour.sig = Some(sig);
            tour.next = now + 0.05;
            return true;
        }
        tour.moved = None;
        tour.sig = Some(sig.clone());
        let same = tour.sigs.iter().any(|old| gizmo_list_diff(&sig, old) < 3.5);
        tour.stuck = if same { tour.stuck + 1 } else { 0 };
        if !same {
            tour.sigs.push(sig);
            tour.views.push(im.crop_i((ux + u * x0) as i64, (h * y0) as i64, (ux + u * x1) as i64, (h * y1) as i64));
            tour.view_k = k;
            let what = format!("all_{:02}", tour.scrolls);
            if let Some(shot) = self.sim_tour_shot(frames, im, &what) {
                self.sim.tour.as_mut().unwrap().shots.push(shot);
            }
        }
        let tour = self.sim.tour.as_mut().unwrap();
        if tour.stuck == 0 && tour.scrolls < 25 {
            tour.scrolls += 1;
            tour.next = now + 0.1;
            let p = at((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            win::scroll_at(p.0, p.1, -3, true);
            return true;
        }
        let b = at(GIZMO_BACK.0, GIZMO_BACK.1);
        win::wiggle_at(b.0, b.1);
        win::click_at(b.0, b.1, true);
        tour.stage = "back";
        tour.backs = 0;
        tour.next = now + 0.4;
        true
    }
}
