//! The leaderboards plug-in's run handling: deciding and sending runs, late Battle Data, your bests, and the automatic
//! "View Battle Data" click. Port of those parts of `LeaderboardsMixin` in aLogs B16 `bossbar/leaderboards.py`.
//! Holding the player's inputs meanwhile (InputBlock, HidHide) is TODO(port).

use crate::meter::{Meter, Out};
use crate::payloads;
use crate::team;
use alogs_pil::ops::expand;
use alogs_pil::{Filter, Image, Mode};
use alogs_pyval::{Dict, Value};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

pub const LATE_BATTLE_DATA_S: f64 = 180.0;
pub const RESULT_PAUSE: f64 = 5.0;
pub const BATTLE_DATA_PAUSE: f64 = 5.0;
pub const HOLD_MIN: f64 = RESULT_PAUSE + BATTLE_DATA_PAUSE;

/// The automatic Battle Data session's state.
#[derive(Default)]
pub struct AutoState {
    pub auto_until: f64,
    pub clicks: i64,
    pub unread: i64,
    pub last_click: f64,
    pub hold_started: f64,
    pub bd_hold: bool,
}

fn read_json(p: &Path) -> Option<Dict> {
    match Value::json_loads(&std::fs::read_to_string(p).ok()?)? {
        Value::Dict(d) => Some(d),
        _ => None,
    }
}

fn write_json(p: &Path, s: &Dict) {
    let _ = alogs_protocol::settings::write_whole(p, &team::unescape_unicode(&Value::Dict(s.clone()).json_dumps_indent(1)));
}

impl Meter {
    pub fn run_path(&self, csv_name: &str) -> PathBuf {
        self.env.data.join("logs").join("fights").join(format!("{csv_name}.json"))
    }

    /// A sent run whose Battle Data can still follow it.
    pub fn late_open(&self, s: &Dict) -> bool {
        s.at("sent").truthy() && s.at("late_result").truthy() && !s.at("late_sent").truthy() && self.env.now() <= s.at("late_until").as_f64().unwrap_or(0.0)
    }

    /// Is this fight's run going to the leaderboards and not sent yet (or inside its late window)?
    pub fn run_to_send(&self, csv_name: &str) -> bool {
        let Some(s) = read_json(&self.run_path(csv_name)) else { return false };
        (!matches!(s.get_str("submit"), Some(Value::Bool(false))) && !s.at("sent").truthy()) || self.late_open(&s)
    }

    /// Which of your bests on this boss this week the run beats ('score', 'clear', 'dps', 'break', 'taken', 'heal').
    pub fn beats_best(&self, s: &Dict) -> Vec<String> {
        let empty = Value::Dict(Dict::new());
        let res = s.get_str("result").filter(|v| v.truthy()).unwrap_or(&empty);
        let players = res.get("players").as_list().cloned().unwrap_or_default();
        let cards = s.get_str("battle_data").map_or(Value::None, |b| b.get("players").clone()).as_list().cloned().unwrap_or_default();
        let me = players.iter().position(|p| p.get("self").truthy()).or(if players.len() <= 1 { Some(0) } else { None });
        let card = match me {
            Some(i) => payloads::card_of(&cards, players.get(i).unwrap_or(&empty), i),
            None => empty.clone(),
        };
        let n = |v: &Value| v.as_i64();
        let clear = if res.get("cleared").truthy() { n(res.get("time_s")) } else { None };
        let mine: Vec<(&str, Value)> = vec![
            ("score", Value::List(vec![Value::Int(n(res.get("rating_stars")).unwrap_or(0)), Value::Int(n(res.get("score")).unwrap_or(0))])),
            ("clear", clear.map_or(Value::None, |c| Value::Int(-c))),
            ("dps", card.get("avg_dps").clone()),
            ("break", card.get("break").clone()),
            ("taken", card.get("taken").clone()),
            ("heal", card.get("heal").clone()),
        ];
        let boss = s.at("puppetId").as_str().map(String::from);
        let cycle = s.at("cycle").as_str().map(String::from);
        let (_, order, partner) = match &boss {
            Some(b) => self.env.schedule.stage_of(Some(b), cycle.as_deref(), self.env.now()),
            None => (None, None, None),
        };
        let who = if order == Some(2) { partner } else { boss };
        let key = format!("{}|{}|{}", self.uid.clone().unwrap_or_else(|| "None".into()), s.at("cycle").py_str(), who.unwrap_or_else(|| "None".into()));
        let path = self.env.data.join("logs").join("bests.json");
        let mut bests = read_json(&path).unwrap_or_default();
        let old = bests.get_str(&key).and_then(|v| v.as_dict().cloned()).unwrap_or_default();
        let gt = |a: &Value, b: &Value| -> bool {
            match (a, b) {
                (Value::List(x), Value::List(y)) => x.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect::<Vec<_>>() > y.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect::<Vec<_>>(),
                _ => a.as_f64().unwrap_or(f64::MIN) > b.as_f64().unwrap_or(f64::MIN),
            }
        };
        let zero = |v: &Value| v.is_none() || v.as_f64() == Some(0.0) || v.repr() == "[0, 0]";
        let better: Vec<String> = mine.iter().filter(|(k, v)| !zero(v) && old.get_str(k).is_none_or(|o| o.is_none() || gt(v, o))).map(|(k, _)| (*k).to_string()).collect();
        if !better.is_empty() {
            let mut d = old.clone();
            for (k, v) in &mine {
                if better.iter().any(|b| b == k) {
                    d.insert(Value::Str((*k).into()), v.clone());
                }
            }
            bests.insert(Value::Str(key), Value::Dict(d));
            let _ = alogs_protocol::settings::write_whole(&path, &Value::Dict(bests).json_dumps());
        }
        better
    }

    pub(crate) fn send_result(&mut self, payload: &Value, evidence: Option<&str>, battle: Option<&str>, party: Option<&str>, gizmos: Option<&[String]>) {
        if let Some(s) = &self.submitter {
            s.enqueue(payload, evidence, battle, party, gizmos);
        }
        // TODO(port): hold the player's inputs until it's sent (Automation › hold_inputs; InputBlock)
    }

    /// Battle Data opened after its run went without it: send the run again, once, with it.
    pub fn send_late_battle_data(&mut self, csv_name: &str) {
        let path = self.run_path(csv_name);
        let Some(mut s) = read_json(&path) else { return };
        let bd = s.at("battle_data").clone();
        if !self.late_open(&s) || !bd.get("players").truthy() {
            return;
        }
        let better = self.beats_best(&s);
        let mut all: std::collections::BTreeSet<String> = s.at("new_bests").as_list().map(|l| l.iter().map(Value::py_str).collect()).unwrap_or_default();
        all.extend(better.iter().cloned());
        s.insert("new_bests".into(), Value::List(all.into_iter().map(Value::Str).collect()));
        s.insert("late_sent".into(), Value::Bool(true));
        write_json(&path, &s);
        log::info!(target: "meter", "leaderboard: Battle Data opened after the run was sent - sending it again with its Battle Data{}",
            if better.is_empty() { String::new() } else { format!(" (a new best: {})", better.join(", ")) });
        let clock = alogs_gamelog::FixedClock(self.env.now());
        let p = payloads::run_payload(&Value::Dict(s.clone()), &self.uid.clone().map_or(Value::None, Value::Str), &self.env.schedule, &clock);
        let late = s.at("late_result").py_str();
        let b = bd.get("screenshot").as_str().map(String::from);
        let party = s.at("party_screenshot").as_str().map(String::from);
        self.send_result(&p, Some(&late), b.as_deref(), party.as_deref(), None);
    }

    fn sweep_late_results(&self) {
        let d = self.env.data.join("logs").join("screens");
        let Ok(rd) = std::fs::read_dir(&d) else { return };
        let now = std::time::SystemTime::now();
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with("late_")
                && e.metadata().and_then(|m| m.modified()).is_ok_and(|t| now.duration_since(t).is_ok_and(|x| x.as_secs_f64() > LATE_BATTLE_DATA_S + 60.0))
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// Battle Data is in (or the wait ran out): send the run, with which of your bests it beat.
    pub fn decide_run(&mut self, path: &Path) {
        let Some(mut s) = read_json(path) else { return };
        if s.get_str("submit").is_some() {
            return;
        }
        let better = self.beats_best(&s);
        s.insert("submit".into(), Value::Bool(true));
        s.insert("new_bests".into(), Value::List(better.iter().cloned().map(Value::Str).collect()));
        write_json(path, &s);
        self.rs.pending_run = None;
        log::info!(target: "meter", "leaderboard: sending this run ({})",
            if better.is_empty() { "no new best; for your Battle History".to_string() } else { format!("a new best: {}", better.join(", ")) });
        self.send_run(path);
    }

    /// Send a run once: its numbers, the result screen and Battle Data.
    pub fn send_run(&mut self, path: &Path) {
        let Some(mut s) = read_json(path) else { return };
        if s.at("sent").truthy() {
            return;
        }
        s.insert("sent".into(), Value::Bool(true));
        self.sweep_late_results();
        let shot = s.at("result").get("screenshot").as_str().map(String::from);
        let screenshots = self.submitter.as_ref().is_some_and(|x| x.config.lock().unwrap().as_ref().is_some_and(|c| c.screenshots));
        if let Some(shot) = &shot
            && !s.at("battle_data").get("screenshot").truthy()
            && screenshots
        {
            let name = Path::new(shot).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let late = format!("logs/screens/late_{name}");
            if std::fs::copy(self.env.data.join(shot), self.env.data.join(&late)).is_ok() {
                s.insert("late_result".into(), Value::Str(late));
                s.insert("late_until".into(), Value::Float(self.env.now() + LATE_BATTLE_DATA_S));
            }
        }
        write_json(path, &s);
        let clock = alogs_gamelog::FixedClock(self.env.now());
        let p = payloads::run_payload(&Value::Dict(s.clone()), &self.uid.clone().map_or(Value::None, Value::Str), &self.env.schedule, &clock);
        let b = s.at("battle_data").get("screenshot").as_str().map(String::from);
        let party = s.at("party_screenshot").as_str().map(String::from);
        self.send_result(&p, shot.as_deref(), b.as_deref(), party.as_deref(), None);
    }

    /// A record reached the leaderboards: its notice over the game, unless turned off.
    pub fn sent_notice(&mut self, out: &mut dyn Out, payload: &Value, reply: &Value) {
        let on = self.args.cfg.get("leaderboard").as_dict().and_then(|d| d.get_str("notify_sent")).is_none_or(|v| v.truthy());
        if !on {
            return;
        }
        if let Some((a, b, c)) = payloads::sent_notice(payload, Some(reply)) {
            out.out("toast", Value::List(vec![Value::Str(a), Value::Str(b), Value::Str(c)]));
        }
    }

    /// A run whose Battle Data never showed up: decide on the result screen alone.
    pub fn check_pending_run(&mut self, now: f64) {
        if let Some((p, due)) = self.rs.pending_run.clone()
            && now > due
        {
            self.decide_run(&p);
        }
    }

    /// The meter closing while a run waits for its Battle Data: decide it now on what it has.
    pub fn send_waiting_run(&mut self) {
        if let Some((p, _)) = self.rs.pending_run.clone() {
            log::info!(target: "meter", "leaderboard: the meter is closing - sending the run that was waiting for its Battle Data");
            self.decide_run(&p);
        }
    }

    pub fn start_auto(&mut self, now: f64) {
        let r = alogs_protocol::settings::load(&self.env.data);
        let secs = alogs_pyval::py_int(r.get("results").get("hold_seconds")).filter(|v| *v != 0).unwrap_or(60);
        let a = &mut self.auto;
        a.auto_until = now + HOLD_MIN.max(120f64.min(secs as f64));
        a.clicks = 0;
        a.unread = 0;
        a.last_click = now + RESULT_PAUSE - 1.0;
        a.hold_started = now;
        a.bd_hold = false;
        // TODO(port): hold inputs / hide controllers for the session
    }

    pub fn auto_active(&mut self) -> bool {
        if self.auto.auto_until != 0.0 && self.env.now() > self.auto.auto_until {
            self.end_auto("time limit");
        }
        self.auto.auto_until != 0.0
    }

    pub fn end_auto(&mut self, why: &str) {
        if self.auto.auto_until == 0.0 {
            return;
        }
        self.auto.auto_until = 0.0;
        log::info!(target: "meter", "automatic Battle Data ended: {why}");
    }

    /// Click the result screen's "View Battle Data" button (only while the game is in front and it reads so).
    pub fn open_battle_data(&mut self, out: &mut dyn Out, im: &Image, u: f64, ux: f64, h: f64, foreground: bool) {
        self.auto.last_click = self.env.now();
        if !foreground {
            self.end_auto("game is not the active window");
            return;
        }
        let b = (ux + u * 0.53, h * 0.875, ux + u * 0.68, h * 0.925);
        let c = im.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64).convert(Mode::L);
        let kk = 2.0 * 2000.0 / u;
        let c = c.resize(((c.width as f64 * kk) as usize).max(1), ((c.height as f64 * kk) as usize).max(1), Some(Filter::Bicubic)).point(|v| if v < 120 { 0.0 } else { 255.0 });
        let mut crops = IndexMap::new();
        crops.insert("b".to_string(), expand(&c, 20, &[255]));
        let label = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new()).get("b").cloned().unwrap_or_default();
        if !label.to_lowercase().contains("battle") {
            self.auto.unread += 1;
            if self.auto.unread >= 6 {
                let why = format!("button reads {} after {} tries", alogs_pyval::str_repr(&label), self.auto.unread);
                self.end_auto(&why);
            } else {
                log::info!(target: "meter", "View Battle Data not readable yet ({}), trying again", alogs_pyval::str_repr(&label));
            }
            return;
        }
        let x = f64::from(self.origin.0) + ux + u * 0.6;
        let y = f64::from(self.origin.1) + h * 0.9;
        self.auto.clicks += 1;
        log::info!(target: "meter", "auto-click {}: View Battle Data at screen ({x:.0}, {y:.0})", self.auto.clicks);
        self.hide_end_note(out);
        if !self.args.replay {
            alogs_capture::win::click_at(x as i32, y as i32, false);
        }
    }
}
