//! The leaderboards plug-in's run handling: deciding and sending runs, late Battle Data, your bests, and the automatic
//! "View Battle Data" click. Port of those parts of `LeaderboardsMixin` in aLogs B16 `bossbar/leaderboards.py`.
//! Holding the player's inputs meanwhile: alogs-capture's input module (InputBlock, HidHide).

use crate::meter::{Meter, Out};
use crate::payloads;
use crate::team;
use alogs_pil::ops::expand;
use alogs_pil::{Filter, Image, Mode};
use alogs_pyval::{Dict, Value};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

/// How long after a run is sent what follows it (Battle Data, the result screen's later reads) is sent as its update.
pub const LATE_BATTLE_DATA_S: f64 = 180.0;
/// At most this many updates of a run after its first send.
pub const MAX_FOLLOWUPS: i64 = 3;
pub const RESULT_PAUSE: f64 = 5.0;
pub const BATTLE_DATA_PAUSE: f64 = 5.0;
pub const HOLD_MIN: f64 = RESULT_PAUSE + BATTLE_DATA_PAUSE;
/// Inputs held at most this long while a result is sent.
pub const SEND_HOLD_S: f64 = 10.0;
/// A run's fight evidence (fight_shots.rs) rides with its Battle Data update, or with any other update of it; with
/// none by this long after the first send, it goes in an update of its own. The automatic Battle Data session reads
/// Battle Data about 5 to 12 s after the result screen (RESULT_PAUSE, then BATTLE_DATA_SETTLE), and a Battle Data read
/// started before this pushes the look later (sent_run_written): so in the usual case one update carries both.
pub const FIGHT_EVIDENCE_WAIT_S: f64 = 20.0;
/// Looked at again this often while it can't go yet (a copy of the run still waits in the queue, or the automatic
/// Battle Data session runs).
pub const FIGHT_EVIDENCE_RECHECK_S: f64 = 5.0;

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

/// The run's fight evidence still to go: recorded in its summary, not attached to an update yet, not given up on.
pub fn fight_pending(s: &Dict) -> bool {
    (s.at("fight_shots").as_list().is_some_and(|l| !l.is_empty()) || matches!(s.at("fight_log"), Value::Dict(_)))
        && s.get_str("fight_evidence").is_none()
        && !s.at("fight_skipped").truthy()
}

/// A run payload's signature: kept in the run's json as `sent_sig`, so an update goes only when it differs.
pub fn payload_sig(p: &Value) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.json_dumps().hash(&mut h);
    format!("{:016x}", h.finish())
}

impl Meter {
    pub fn run_path(&self, csv_name: &str) -> PathBuf {
        self.env.data.join("logs").join("fights").join(format!("{csv_name}.json"))
    }

    /// A sent run whose Battle Data (or a later read of its result screen) can still follow it as an update: inside
    /// its window (LATE_BATTLE_DATA_S from the first send), MAX_FOLLOWUPS at most.
    pub fn late_open(&self, s: &Dict) -> bool {
        s.at("sent").truthy() && s.at("followups").as_i64().unwrap_or(0) < MAX_FOLLOWUPS && self.env.now() <= s.at("late_until").as_f64().unwrap_or(0.0)
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
        self.send_result_held(payload, evidence, battle, party, gizmos, true, &[], None);
    }

    /// Queue a record (with an Interlink run's fight evidence, when given); with `hold` (and holding_for_send), the
    /// player's inputs are held until this copy is sent.
    #[allow(clippy::too_many_arguments)]
    fn send_result_held(&mut self, payload: &Value, evidence: Option<&str>, battle: Option<&str>, party: Option<&str>, gizmos: Option<&[String]>, hold: bool,
                        fight: &[(i64, String)], fightlog: Option<&str>) {
        let Some(s) = self.submitter.clone() else { return };
        let queued = s.enqueue_with_fight(payload, evidence, battle, party, gizmos, fight, fightlog);
        if !hold || !self.holding_for_send() {
            return;
        }
        self.send_waiting = Some(payload.get("clientRunId").py_str());
        self.send_waiting_copy = queued;
        self.hold.start(SEND_HOLD_S);
        log::info!(target: "meter", "holding inputs until this is sent (at most {SEND_HOLD_S} s)");
    }

    /// Does a record sent now hold the player's inputs until it is sent? Only with Automation and "hold inputs" on,
    /// the game in front, and not while the automatic Battle Data session runs: its own hold lasts until Battle Data
    /// is read, and a send must neither shorten nor end it.
    pub fn holding_for_send(&self) -> bool {
        let r = self.args.cfg.get("results");
        if self.args.replay || !alogs_protocol::settings::automation(&self.args.cfg, None) || matches!(r.get("hold_inputs"), Value::Bool(false)) {
            return false;
        }
        self.auto.auto_until == 0.0 && !self.hwnd.is_some_and(|w| !alogs_capture::win::is_foreground(w))
    }

    /// Once the sender has nothing waiting: the result is sent, its notice is going up, so inputs come back.
    pub fn result_sent_check(&mut self) {
        if self.send_waiting.is_some() && self.submitter.as_ref().is_none_or(|s| s.pending() == 0) {
            self.send_waiting = None;
            self.send_waiting_copy = None;
            self.stop_send_hold();
        }
    }

    /// The send's hold ends; the automatic Battle Data session's, if one runs now, stays until Battle Data is read.
    fn stop_send_hold(&mut self) {
        if self.auto.auto_until == 0.0 {
            self.hold.stop();
        }
    }

    /// Battle Data read complete after its run was sent: the run's update goes at once (if the site's copy differs),
    /// holding the inputs until it is sent as a first send does.
    pub fn send_late_battle_data(&mut self, csv_name: &str) {
        let path = self.run_path(csv_name);
        self.send_followup(&path, true);
    }

    /// Send a sent run again, as an update of the same record, when what the site should have (its payload) differs
    /// from what was last sent; inside the run's window, MAX_FOLLOWUPS at most. A Battle Data picture not sent yet
    /// goes with the result screen as its evidence (the site takes it only so); otherwise no pictures go, and the site
    /// keeps the ones it has. A copy of the run still queued is replaced: its pictures go with this one.
    ///
    /// The run's fight evidence (its pictures and log, fight_shots.rs) goes with the first update that goes anyway
    /// (Battle Data's, as a rule), else alone from FIGHT_EVIDENCE_WAIT_S after the first send, or as the window closes
    /// (`closing`): once, never while a copy of the run still waits in the queue (so never with the first copy), and
    /// only with the result screen as evidence (the site takes it only so).
    pub fn send_followup(&mut self, path: &Path, hold: bool) -> bool {
        self.send_followup_ex(path, hold, false)
    }

    pub fn send_followup_ex(&mut self, path: &Path, hold: bool, closing: bool) -> bool {
        let Some(mut s) = read_json(path) else { return false };
        if self.submitter.is_none() || !self.late_open(&s) {
            return false;
        }
        let uid = self.uid.clone().map_or(Value::None, Value::Str);
        let clock = self.env.record_clock();
        let p = payloads::run_payload(&Value::Dict(s.clone()), &uid, &self.env.schedule, &clock);
        let changed = s.at("sent_sig").as_str() != Some(payload_sig(&p).as_str());
        let (fight, fightlog) = self.fight_to_attach(path, &mut s, &p, changed, closing);
        let attach = !fight.is_empty() || fightlog.is_some();
        if !changed && !attach {
            return false;
        }
        let better = self.beats_best(&s);
        let mut all: std::collections::BTreeSet<String> = s.at("new_bests").as_list().map(|l| l.iter().map(Value::py_str).collect()).unwrap_or_default();
        all.extend(better.iter().cloned());
        s.insert("new_bests".into(), Value::List(all.into_iter().map(Value::Str).collect()));
        let p = payloads::run_payload(&Value::Dict(s.clone()), &uid, &self.env.schedule, &clock);
        let n = s.at("followups").as_i64().unwrap_or(0) + 1;
        s.insert("followups".into(), Value::Int(n));
        s.insert("sent_sig".into(), Value::Str(payload_sig(&p)));
        let (mut ev, mut bt, mut pt) = (None::<String>, None::<String>, None::<String>);
        let (mut fight, mut fightlog) = (fight, fightlog);
        let data = self.env.data.clone();
        let on_disk = |rel: &str| !rel.is_empty() && data.join(rel).is_file();
        let bd_shot = s.at("battle_data").get("screenshot").as_str().map(String::from).filter(|b| on_disk(b));
        if self.sends_screenshots()
            && let Some(b) = bd_shot.filter(|b| s.at("sent_battle").as_str() != Some(b.as_str()))
        {
            ev = self.result_evidence(&s);
            s.insert("sent_battle".into(), Value::Str(b.clone()));
            bt = Some(b);
            pt = s.at("party_screenshot").as_str().map(String::from);
        }
        if attach && ev.is_none() {
            ev = self.result_evidence(&s); // (the fight's evidence goes only with it)
        }
        if let Some(old) = self.submitter.as_ref().and_then(|sub| sub.queued_item(&p)) {
            let keep = alogs_pyval::py_int(alogs_protocol::settings::load(&data).get("capture").get("keep_hours")).unwrap_or(0) != 0;
            for (k, slot) in [("evidence", &mut ev), ("battle", &mut bt), ("party", &mut pt)] {
                let Some(o) = old.get(k).as_str().filter(|o| !o.is_empty()).map(String::from) else { continue };
                if slot.is_none() {
                    *slot = Some(o).filter(|o| on_disk(o));
                } else if slot.as_deref() != Some(o.as_str()) && !keep {
                    let _ = std::fs::remove_file(data.join(&o)); // replaced in the queue: nothing else names it
                }
            }
            // a queued update that carries the fight's evidence: this one carries it instead
            if fight.is_empty() && fightlog.is_none() {
                fight = old
                    .get("fight")
                    .as_list()
                    .map(|l| l.iter().filter_map(|e| Some((e.get("n").as_i64()?, e.get("file").as_str()?.to_string()))).filter(|(_, f)| on_disk(f)).collect())
                    .unwrap_or_default();
                fightlog = old.get("fightlog").as_str().map(String::from).filter(|f| on_disk(f));
            }
        }
        write_json(path, &s);
        let mut what = if bt.is_some() { "with its Battle Data picture".to_string() } else { "numbers only".to_string() };
        if !fight.is_empty() || fightlog.is_some() {
            what += &format!(", with the fight's evidence: {} pictures{}", fight.len(), if fightlog.is_some() { " and its log" } else { "" });
        }
        log::info!(target: "meter", "leaderboard: what follows the sent run changed - sending its update {n} of at most {MAX_FOLLOWUPS} ({what}){}",
            if better.is_empty() { String::new() } else { format!(", a new best: {}", better.join(", ")) });
        self.send_result_held(&p, ev.as_deref(), bt.as_deref(), pt.as_deref(), None, hold, &fight, fightlog.as_deref());
        true
    }

    /// The result screen to send as an update's evidence: as it was first sent (still on disk while its first copy waits
    /// in the queue), else a copy of the one kept for this (the kept one stays for a later update).
    fn result_evidence(&self, s: &Dict) -> Option<String> {
        let data = &self.env.data;
        let on_disk = |rel: &str| !rel.is_empty() && data.join(rel).is_file();
        let shot = s.at("result").get("screenshot").as_str().map(String::from).filter(|r| on_disk(r));
        shot.or_else(|| {
            let late = s.at("late_result").as_str().map(String::from).filter(|l| on_disk(l))?;
            let name = Path::new(&late).file_name()?.to_string_lossy().into_owned();
            let copy = format!("logs/screens/resend_{}", name.trim_start_matches("late_"));
            std::fs::copy(data.join(&late), data.join(&copy)).is_ok().then_some(copy)
        })
    }

    /// Does the run's fight evidence go with this update (send_followup_ex)? When it is pending and an update goes
    /// anyway (`changed`), the window closes, or its own wait is over with no automatic Battle Data session running;
    /// never while a copy of the run waits in the queue. Then what is on disk of it ((part, picture), log), recorded in
    /// the summary as `fight_evidence` (what makes this payload differ: extra.fightShots / fightLog). Without the result
    /// screen to send it with ("Attach the screenshots as proof" off, or no picture kept) it is given up on, once.
    fn fight_to_attach(&mut self, path: &Path, s: &mut Dict, p: &Value, changed: bool, closing: bool) -> (Vec<(i64, String)>, Option<String>) {
        let none = (vec![], None);
        if !fight_pending(s) {
            return none;
        }
        let now = self.env.now();
        let own_turn = now >= s.at("fight_due").as_f64().unwrap_or(0.0) && self.auto.auto_until == 0.0;
        if !(changed || closing || own_turn) || self.submitter.as_ref().and_then(|sub| sub.queued_item(p)).is_some() {
            return none;
        }
        let data = self.env.data.clone();
        let on_disk = |rel: &str| !rel.is_empty() && data.join(rel).is_file();
        let mut fight = vec![];
        let mut shots = vec![];
        for e in s.at("fight_shots").as_list().cloned().unwrap_or_default() {
            let (Some(n), Some(file)) = (e.get("n").as_i64(), e.get("file").as_str().map(String::from)) else { continue };
            if on_disk(&file) {
                shots.push(alogs_pyval::dict([("n", Value::Int(n)), ("t", e.get("t").clone())]));
                fight.push((n, file));
            }
        }
        let fl = s.at("fight_log").clone();
        let fightlog = fl.get("file").as_str().map(String::from).filter(|f| on_disk(f));
        let result = s.at("result").get("screenshot").as_str().is_some_and(on_disk) || s.at("late_result").as_str().is_some_and(on_disk);
        let why = if !self.sends_screenshots() {
            Some("\"Attach the screenshots as proof\" is off")
        } else if !result {
            Some("no result screen kept to send it with")
        } else if fight.is_empty() && fightlog.is_none() {
            Some("none of it was written")
        } else {
            None
        };
        if let Some(why) = why {
            log::info!(target: "meter", "leaderboard: the fight's evidence is not sent: {why}");
            s.insert("fight_skipped".into(), Value::Bool(true));
            write_json(path, s);
            self.drop_fight_files(s);
            return none;
        }
        let mut e = Dict::new();
        e.insert("shots".into(), Value::List(shots));
        if fightlog.is_some() {
            e.insert("log".into(), alogs_pyval::dict([("rows", fl.get("rows").clone())]));
        }
        s.insert("fight_evidence".into(), Value::Dict(e));
        (fight, fightlog)
    }

    /// The run's fight evidence files, removed (unless Keep screenshots is on): it isn't going to be sent.
    fn drop_fight_files(&self, s: &Dict) {
        if alogs_pyval::py_int(alogs_protocol::settings::load(&self.env.data).get("capture").get("keep_hours")).unwrap_or(0) != 0 {
            return;
        }
        let shots = s.at("fight_shots").as_list().cloned().unwrap_or_default();
        for f in shots.iter().map(|e| e.get("file")).chain([s.at("fight_log").get("file")]) {
            if let Some(f) = f.as_str() {
                let _ = std::fs::remove_file(self.env.data.join(f));
            }
        }
    }

    /// Is "Attach the screenshots as proof" on (with submissions on)?
    fn sends_screenshots(&self) -> bool {
        self.submitter.as_ref().is_some_and(|x| x.config.lock().unwrap().as_ref().is_some_and(|c| c.screenshots))
    }

    /// When to look at the sent run's window next (its update's settle time): another run's open window is settled first.
    pub fn followup_at(&mut self, path: &Path, due: f64) {
        if let Some((p, _)) = self.rs.followup.clone()
            && p != path
        {
            self.settle_followup();
        }
        self.rs.followup = Some((path.to_path_buf(), due));
    }

    /// The sent run's update is due: sent if anything changed, then its window is looked at again where it ends.
    pub fn check_followup(&mut self, now: f64) {
        let Some((p, due)) = self.rs.followup.clone() else { return };
        if now <= due {
            return;
        }
        self.send_followup(&p, false);
        match read_json(&p) {
            Some(s) if self.late_open(&s) => {
                let until = s.at("late_until").as_f64().unwrap_or(0.0);
                // the fight's evidence still to go: looked at when its own wait is over (again shortly while it can't)
                let next = if fight_pending(&s) { s.at("fight_due").as_f64().unwrap_or(0.0).max(now + FIGHT_EVIDENCE_RECHECK_S).min(until) } else { until };
                self.rs.followup = Some((p, next));
            }
            _ => self.end_followup(&p),
        }
    }

    /// Now (another run, or the meter closing): the open window's last update if anything changed, and its end.
    pub fn settle_followup(&mut self) {
        if let Some((p, _)) = self.rs.followup.clone() {
            self.send_followup_ex(&p, false, true);
            self.end_followup(&p);
        }
    }

    /// A run's window is over: nothing follows it any more, and the result screen kept for its update is gone.
    fn end_followup(&mut self, path: &Path) {
        if self.rs.followup.as_ref().is_some_and(|(p, _)| p == path) {
            self.rs.followup = None;
        }
        let Some(mut s) = read_json(path) else { return };
        if let Some(late) = s.at("late_result").as_str().map(String::from) {
            let _ = std::fs::remove_file(self.env.data.join(&late));
            s.pop("late_result");
            write_json(path, &s);
        }
        if fight_pending(&s) {
            log::info!(target: "meter", "leaderboard: the fight's evidence is not sent: the run's window ended first");
            s.insert("fight_skipped".into(), Value::Bool(true));
            write_json(path, &s);
            self.drop_fight_files(&s);
        }
    }

    /// The meter starting: no run's window is open, so no result screen kept for an update is needed.
    pub fn forget_late_results(&self) {
        let Ok(rd) = std::fs::read_dir(self.env.data.join("logs").join("screens")) else { return };
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with("late_") {
                let _ = std::fs::remove_file(e.path());
            }
        }
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

    /// The result screen is wholly read (or Battle Data is in, or the wait ran out): send the run, with which of your
    /// bests it beat.
    pub fn decide_run(&mut self, path: &Path) {
        let Some(mut s) = read_json(path) else { return };
        if s.get_str("submit").is_some() {
            return;
        }
        let better = self.beats_best(&s);
        s.insert("submit".into(), Value::Bool(true));
        s.insert("new_bests".into(), Value::List(better.iter().cloned().map(Value::Str).collect()));
        write_json(path, &s);
        if self.rs.pending_run.as_ref().is_some_and(|(p, _)| p == path) {
            self.rs.pending_run = None;
        }
        log::info!(target: "meter", "leaderboard: sending this run ({})",
            if better.is_empty() { "no new best; for your Battle History".to_string() } else { format!("a new best: {}", better.join(", ")) });
        self.send_run(path);
    }

    /// Send a run's first copy: its numbers, the result screen and Battle Data. What follows it inside its window
    /// (LATE_BATTLE_DATA_S) goes as updates of the same record (send_followup); the first look at that is
    /// BATTLE_DATA_SETTLE from now, for the result screen's later reads.
    pub fn send_run(&mut self, path: &Path) {
        let Some(mut s) = read_json(path) else { return };
        if s.at("sent").truthy() {
            return;
        }
        let now = self.env.now();
        s.insert("sent".into(), Value::Bool(true));
        self.sweep_late_results();
        let shot = s.at("result").get("screenshot").as_str().map(String::from);
        let screenshots = self.sends_screenshots();
        let b = s.at("battle_data").get("screenshot").as_str().map(String::from);
        let fight = fight_pending(&s);
        if fight {
            s.insert("fight_due".into(), Value::Float(now + FIGHT_EVIDENCE_WAIT_S));
        }
        if let Some(shot) = &shot
            && (b.is_none() || fight)
            && screenshots
        {
            // kept for an update that brings a Battle Data picture (or the fight's evidence) once this copy (and its
            // picture) is sent
            let name = Path::new(shot).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let late = format!("logs/screens/late_{name}");
            if std::fs::copy(self.env.data.join(shot), self.env.data.join(&late)).is_ok() {
                s.insert("late_result".into(), Value::Str(late));
            }
        }
        s.insert("late_until".into(), Value::Float(now + LATE_BATTLE_DATA_S));
        if let Some(b) = b.as_ref().filter(|_| screenshots) {
            s.insert("sent_battle".into(), Value::Str(b.clone()));
        }
        let clock = self.env.record_clock();
        let p = payloads::run_payload(&Value::Dict(s.clone()), &self.uid.clone().map_or(Value::None, Value::Str), &self.env.schedule, &clock);
        s.insert("sent_sig".into(), Value::Str(payload_sig(&p)));
        write_json(path, &s);
        let party = s.at("party_screenshot").as_str().map(String::from);
        self.send_result(&p, shot.as_deref(), b.as_deref(), party.as_deref(), None);
        self.followup_at(path, now + crate::results_parse::BATTLE_DATA_SETTLE);
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

    /// The records the sender got to the leaderboards since the last look (Submitter::on_sent), each one's notice.
    /// The Python's sender called sent_notice itself, from its own thread; here they come in on the meter's loop.
    pub fn take_sent(&mut self, out: &mut dyn Out, sent: &std::sync::mpsc::Receiver<(Value, Value)>) {
        while let Ok((payload, reply)) = sent.try_recv() {
            self.sent_notice(out, &payload, &reply);
            self.result_sent(&payload);
        }
    }

    /// From the sender, once a result is sent: its notice is going up, so inputs come back. The Python's `result_sent`.
    /// Without it the hold only ended once the sender had nothing at all waiting (result_sent_check), so another
    /// record still on its way kept the inputs held for the hold's whole 10 s. Only the copy waited for ends it: a
    /// run's first copy reaching the site doesn't end the hold for its update.
    pub fn result_sent(&mut self, payload: &Value) {
        let mine = self.send_waiting.as_deref().is_some_and(|run| payload.get("clientRunId").py_str() == run)
            && self.send_waiting_copy.as_ref().is_none_or(|c| c.json_dumps() == payload.json_dumps());
        if mine {
            self.send_waiting = None;
            self.send_waiting_copy = None;
            self.stop_send_hold();
        }
    }

    /// A run whose result screen never read whole (and no Battle Data came): decide on what it has.
    pub fn check_pending_run(&mut self, now: f64) {
        if let Some((p, due)) = self.rs.pending_run.clone()
            && now > due
        {
            self.decide_run(&p);
        }
    }

    /// The meter closing while a run waits to be sent, or a sent run's window is open: the run is sent now on what it
    /// has, an update goes if anything changed, and the window ends.
    pub fn send_waiting_run(&mut self) {
        if let Some((p, _)) = self.rs.pending_run.clone() {
            log::info!(target: "meter", "leaderboard: the meter is closing - sending the run that was waiting to be sent");
            self.decide_run(&p);
        }
        self.settle_followup();
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
        let until = a.auto_until;
        if r.get("results").get("hold_inputs").truthy() {
            self.hold.start(until - now);
        }
        // (already hidden by the fight's fade: fight_end.rs)
        if r.get("results").get("hold_controller").truthy() && !alogs_capture::input::controllers_hidden() && alogs_capture::input::hidhide_available() {
            alogs_capture::input::hide_controllers(true);
        }
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
        self.hold.stop();
        if alogs_capture::input::controllers_hidden() {
            alogs_capture::input::hide_controllers(false);
        }
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
        if self.hold.active() {
            self.hold.freeze_moves(0.4); // the player's mouse can't pull the click away
        }
        if !self.args.replay {
            alogs_capture::win::click_at(x as i32, y as i32, false);
        }
    }

    /// Online / AFK on your profile while the game runs: sent when it changes and every 2 minutes, and "offline"
    /// once the game closes.
    pub fn presence_tick(&mut self, now: f64, game_open: bool, in_front: bool) {
        let (Some(sub), Some(uid)) = (self.submitter.clone(), self.uid.clone()) else { return };
        if self.args.replay {
            return;
        }
        let last = self.presence_sent.clone();
        if !game_open {
            if last.as_ref().is_some_and(|l| l.1 != "offline") {
                self.presence_sent = Some((uid.clone(), "offline", now));
                sub.presence(&uid, "offline", false);
            }
            return;
        }
        let p = self.presence.get_or_insert_with(|| alogs_capture::presence::Presence::new(now));
        let state = p.state(now, in_front);
        if let Some(l) = &last
            && l.0 == uid
            && l.1 == state
            && now - l.2 < 120.0
        {
            return;
        }
        if last.as_ref().is_none_or(|l| l.0 != uid || l.1 != state) {
            log::info!(target: "meter", "presence: {state}");
        }
        self.presence_sent = Some((uid.clone(), state, now));
        sub.presence(&uid, state, false);
    }
}
