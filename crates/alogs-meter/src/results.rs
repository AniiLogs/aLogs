//! Reading end screens into runs: the result screen, Battle Data, the run's summary (logs/fights/<fight>.json), proof
//! pictures. Port of `ResultsMixin` in aLogs B16 `bossbar/results.py` (the Interlink half: result screen, Battle
//! Data, run_of, fight_summary, schedule_run, record_score, leader_ended_note, picture/game_alone/settled_frame) and
//! the helpers it uses from core.py (save_picture, star_caps, clear_stars, save_calibration).
//! Holo-Battle Sim, Egg Heist and Challenge Stats are in sim.rs and egg.rs.

use crate::fights::Fight;
use crate::meter::{Frames, Meter, Out};
use crate::results_parse::{self as rp, Votes, bd_complete, mark_you, own_name, party_slots, vote};
use crate::team::{self, json_unicode};
use alogs_gamelog::{interlink_score, scene_of};
use alogs_pil::ops::{Stat, expand};
use alogs_pil::{Filter, Image, Mode};
use alogs_pyval::{Dict, Value, dict};
use alogs_reader::boss::Row;
use alogs_reader::{prep, names::NameReader};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

pub const BD_ROWS: [(&str, f64); 4] = [("avg_dps", 0.5833), ("break", 0.6255), ("taken", 0.6667), ("heal", 0.7088)];
pub const LEADER_NOTE: (f64, f64, f64, f64) = (0.33, 0.140, 0.67, 0.195);
pub const CLEAR_STARS: i64 = 20;
pub const CENTER_STARS: i64 = 40;

/// The end-screen readers' state (the Python keeps these on the Meter, most made on first use).
#[derive(Default)]
pub struct ResultsState {
    pub result_seen: f64,
    pub result_reads: i64,
    pub result_votes: Votes,
    pub result_shot: Option<String>,
    pub result_last: f64,
    /// When the result screen counted as read: wholly read, and two looks in a row agreeing.
    pub result_ready: f64,
    /// The last look's own reads of the result screen (rating, score, time, each slot's name and value).
    pub result_look: Option<String>,
    pub result_check: Option<Value>,
    pub bd_seen: f64,
    pub bd_reads: i64,
    pub bd_votes: Votes,
    pub bd_shot: Option<String>,
    pub bd_cards: usize,
    pub bd_prev_sig: Option<Vec<Vec<Option<i64>>>>,
    pub bd_measured: bool,
    pub bd_stamp: Option<String>,
    pub bd_last: f64,
    pub bd_check: Option<Value>,
    pub battle_saved: f64,
    pub last_scores: IndexMap<String, (Option<i64>, Option<i64>)>,
    pub end_sig: Option<Vec<u8>>,
    pub leader_look: f64,
    /// The fight (its CSV name) whose leader ended the challenge.
    pub leader_ended: Option<String>,
    pub screen_check: Option<Value>,
    pub pending_run: Option<(PathBuf, f64)>,
    /// A sent run whose window for updates is open, and when to look at it next (its update, else its window's end).
    pub followup: Option<(PathBuf, f64)>,
    pub sh_checked: f64,
    pub sh_run_seen: f64,
    pub last_results: f64,
}

pub fn local(now: f64, fmt: &str) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_opt(now as i64, 0).single().map_or_else(String::new, |d| d.format(fmt).to_string())
}

pub(crate) fn rel(data: &Path, p: &Path) -> String {
    p.strip_prefix(data).map_or_else(|_| p.to_string_lossy().into_owned(), |r| r.to_string_lossy().into_owned())
}

/// A screen kept as a picture: a JPEG at most `width` px wide, in logs/<folder>/<name>.jpg.
pub fn save_picture(data: &Path, im: &Image, folder: &str, name: &str, width: usize) -> Option<PathBuf> {
    let d = data.join("logs").join(folder);
    std::fs::create_dir_all(&d).ok()?;
    let im = if im.width > width {
        im.resize(width, (im.height as f64 * width as f64 / im.width as f64).round_ties_even() as usize, Some(Filter::Lanczos))
    } else {
        im.clone()
    };
    let rgb = im.convert(Mode::Rgb);
    let path = d.join(format!("{name}.jpg"));
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 85).encode(&rgb.data, rgb.width as u16, rgb.height as u16, jpeg_encoder::ColorType::Rgb).ok()?;
    std::fs::write(&path, out).ok()?;
    Some(path)
}

fn read_json(p: &Path) -> Option<Value> {
    Value::json_loads(&std::fs::read_to_string(p).ok()?)
}

/// The side and center bosses' full ratings: the game's config (game_cycles.json rules), else what the Interlink
/// screen last showed (logs/star_caps.json), else 20/40.
pub fn star_caps(data: &Path) -> (i64, i64) {
    if let Some(c) = read_json(&data.join("game_cycles.json")) {
        let r = c.get("rules");
        if let (Some(a), Some(b)) = (alogs_pyval::py_int(r.get("starSide")), alogs_pyval::py_int(r.get("starCenter"))) {
            return (a, b);
        }
    }
    if let Some(c) = read_json(&data.join("logs").join("star_caps.json"))
        && let (Some(a), Some(b)) = (alogs_pyval::py_int(c.get("side")), alogs_pyval::py_int(c.get("center")))
    {
        return (a, b);
    }
    (CLEAR_STARS, CENTER_STARS)
}

fn get<'a>(r: &'a Row, k: &str) -> &'a str {
    r.get(k).map_or("", String::as_str)
}

fn num(s: Option<&str>) -> Option<i64> {
    let s = s?;
    if !s.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    s.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().ok()
}

fn opt_i(v: Option<i64>) -> Value {
    v.map_or(Value::None, Value::Int)
}

fn thousands(v: i64) -> String {
    let s = v.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if v < 0 { format!("-{out}") } else { out }
}

/// The run a Fight tracked, as the game keeps it.
pub struct Run {
    pub id: String,
    pub title: String,
    pub damage: i64,
    pub max_hp: i64,
    pub bosses: Option<Vec<Value>>,
}

impl Meter {
    /// The full rating of an Interlink boss: the center's cap for this week's center boss, the side cap otherwise.
    pub fn clear_stars(&self, puppet_id: Option<&str>) -> i64 {
        let (side, center) = star_caps(&self.env.data);
        let now = self.env.now();
        match puppet_id {
            Some(p) if self.env.schedule.cycle_slot(self.env.schedule.active_cycle_id(now).as_deref(), p).0 == Some("middle") => center,
            _ => side,
        }
    }

    /// The party read on this run's prep screen (recent enough), or None.
    pub fn party_for_result(&self, now: f64) -> Option<Vec<Value>> {
        let p = self.party.as_ref()?;
        if now - p.get("at").as_f64().unwrap_or(0.0) < 900.0 { p.get("members").as_list().cloned() } else { None }
    }

    /// The game-font reader, with the font cache in the data folder (read from the game first if needed); None
    /// when the game's font can't be read.
    pub fn name_reader(&mut self) -> Option<&NameReader> {
        if self.font_reader.is_none() && !self.font_tried {
            self.font_tried = true;
            self.font_reader = alogs_reader::gamefont::GameFont::load(&self.env.data.join("gamefont"), Some(&self.args.player_log), alogs_reader::gamefont::FONT_NAME)
                .ok()
                .map(NameReader::new);
        }
        self.font_reader.as_ref()
    }

    pub fn run_of(&self, f: &Fight) -> Run {
        let mut run = Run { id: get(&f.row, "puppetId").into(), title: f.title.clone(), damage: f.damage, max_hp: f.max_hp, bosses: None };
        if !f.bossrush || f.order.is_none() {
            return run;
        }
        let r1 = |v: f64| alogs_pil::resample::round_digits(v, 1);
        let fo = |v: Option<f64>| v.map_or(Value::None, Value::Float);
        let mut bosses = Vec::new();
        for (i, p) in f.phases.iter().enumerate() {
            let to = if i == f.phases.len() - 1 { Some(r1(f.elapsed)) } else { p.end_s };
            let secs = to.map(|t| r1(t - p.start_s));
            bosses.push(dict([
                ("id", Value::Str(get(&p.row, "puppetId").into())),
                ("name", Value::Str(p.title.clone())),
                ("max_hp", Value::Int(p.max_hp)),
                ("damage", Value::Int(p.damage)),
                ("defeated", Value::Bool(p.defeated)),
                ("from_s", Value::Float(p.start_s)),
                ("to_s", fo(to)),
                ("seconds", fo(secs)),
                ("dps", secs.filter(|s| *s > 1.0).map_or(Value::None, |s| Value::Int((p.damage as f64 / s).round_ties_even() as i64))),
                ("breaks", Value::Int(p.breaks)),
            ]));
        }
        let first = &f.phases[0];
        run.id = get(&first.row, "puppetId").into();
        run.title = first.title.clone();
        let (_, order, other) = self.env.schedule.stage_of(Some(&run.id), None, self.env.now());
        let row = if order == Some(2) {
            self.catalog.iter().find(|r| get(r, "table") == "puppet_data" && Some(get(r, "puppetId")) == other.as_deref() && !get(r, "fixed_hp_rate").is_empty())
        } else {
            None
        };
        if let Some(row) = row {
            let cal = alogs_reader::boss::load_calibration(&self.env.data);
            let hp = alogs_reader::boss::boss_max_hp(row, f.level, Some(&cal)).unwrap_or(0);
            bosses.insert(
                0,
                dict([
                    ("id", Value::Str(other.clone().unwrap_or_default())),
                    ("name", Value::Str(get(row, "bossTitle_en").into())),
                    ("max_hp", Value::Int(hp)),
                    ("damage", Value::Int(hp)),
                    ("defeated", Value::Bool(true)),
                    ("from_s", Value::None),
                    ("to_s", Value::None),
                    ("seconds", Value::None),
                    ("dps", Value::None),
                    ("breaks", Value::None),
                    ("seen", Value::Bool(false)),
                ]),
            );
            run.id = other.unwrap_or_default();
            let t = get(row, "bossTitle_en");
            run.title = if t.is_empty() { first.title.clone() } else { t.into() };
            run.damage = f.damage + hp;
            run.max_hp = f.max_hp + hp;
        }
        run.bosses = Some(bosses);
        run
    }

    fn summary_path(&self, f: &Fight) -> PathBuf {
        self.env.data.join("logs").join("fights").join(format!("{}.json", f.csv_name))
    }

    /// logs/fights/<fight>.json: the meter's numbers plus the game screens of this fight, merged as they appear.
    pub fn fight_summary(&mut self, f: &Fight, parts: Vec<(&str, Value)>) {
        let d = self.env.data.join("logs").join("fights");
        let _ = std::fs::create_dir_all(&d);
        let path = self.summary_path(f);
        let now = self.env.now();
        let mut s = match read_json(&path) {
            Some(Value::Dict(d)) => d,
            _ => {
                let run = self.run_of(f);
                let r1 = |v: f64| alogs_pil::resample::round_digits(v, 1);
                let rate = f.brk.rate(now).0;
                let mut meter = Dict::new();
                for (k, v) in [
                    ("max_hp", Value::Int(run.max_hp)),
                    ("damage", Value::Int(run.damage)),
                    ("seconds", Value::Float(r1(f.elapsed))),
                    ("dps", if f.elapsed > 1.0 { Value::Int((run.damage as f64 / f.elapsed).round_ties_even() as i64) } else { Value::None }),
                    ("segment_left", opt_i(f.seg)),
                    ("tracked_from_start", Value::Bool(f.started_at_top)),
                    ("csv", Value::Str(format!("{}.csv", f.csv_name))),
                    ("time_limit_s", match f.limit { Some(l) if l != 0 => Value::Int(l), _ => opt_i(self.overtime_limit(now)) }),
                    ("breaks", Value::Int(f.phases[..f.phases.len() - 1].iter().map(|p| p.breaks).sum::<i64>() + f.brk.breaks)),
                    ("break_gauge_points", f.brk.size.filter(|s| *s != 0.0).map_or(Value::None, |s| Value::Int(s.round_ties_even() as i64))),
                    ("break_rate_pct_s", rate.map_or(Value::None, |r| Value::Float(alogs_pil::resample::round_digits(r, 2)))),
                ] {
                    meter.insert(k.into(), v);
                }
                if let Some(b) = &run.bosses {
                    meter.insert("bosses".into(), Value::List(b.clone()));
                }
                let instance = if f.bossrush && self.space.is_some() && scene_of(self.space.as_deref()).is_none() { self.space.clone().map_or(Value::None, Value::Str) } else { Value::None };
                let mut s = Dict::new();
                for (k, v) in [
                    ("boss", Value::Str(run.title)),
                    ("puppetId", Value::Str(run.id)),
                    ("level", Value::Int(f.level)),
                    ("uid", self.uid.clone().map_or(Value::None, Value::Str)),
                    ("cycle", self.env.schedule.active_cycle_id(now).map_or(Value::None, Value::Str)),
                    ("started", Value::Str(local(f.t0, "%Y-%m-%d %H:%M:%S"))),
                    ("bossrush", Value::Bool(f.bossrush)),
                    ("instance", instance),
                    ("interlink_scene", Value::Bool(f.scene_ok)),
                    ("server", crate::submit::game_server().map_or(Value::None, Value::Str)),
                    ("meter", Value::Dict(meter)),
                ] {
                    s.insert(k.into(), v);
                }
                s
            }
        };
        for (k, v) in parts {
            s.insert(k.into(), v);
        }
        let write = |s: &Dict| {
            let _ = alogs_protocol::settings::write_whole(&path, &team::unescape_unicode(&Value::Dict(s.clone()).json_dumps_indent(1)));
        };
        write(&s);
        let has_result = s.get_str("result").is_some_and(|v| v.truthy());
        if self.submitter.is_some() && has_result && f.bossrush && s.get_str("submit").is_none() && !s.at("interlink_scene").truthy() {
            log::info!(target: "meter", "leaderboard: the game log didn't confirm an Interlink scene for this fight - kept in your records, not sent");
            s.insert("submit".into(), Value::Bool(false));
            write(&s);
        }
        if self.submitter.is_some() && has_result && f.bossrush && s.get_str("submit").is_none() {
            let bd = s.get_str("battle_data").is_some_and(|v| v.truthy());
            self.schedule_run(path, bd, now);
        }
    }

    /// When a finished run is decided and sent if its result screen doesn't read whole first (send_when_read):
    /// BATTLE_DATA_SETTLE after its Battle Data, else BATTLE_DATA_WAIT. Another run still waiting is sent first, on
    /// what it has, rather than dropped.
    pub fn schedule_run(&mut self, path: PathBuf, battle_data: bool, now: f64) {
        if let Some((p, _)) = self.rs.pending_run.clone()
            && p != path
        {
            self.decide_run(&p);
        }
        let mut due = now + if battle_data { rp::BATTLE_DATA_SETTLE } else { rp::BATTLE_DATA_WAIT };
        if let Some((p, d)) = &self.rs.pending_run
            && *p == path
        {
            due = due.min(*d);
        }
        self.rs.pending_run = Some((path, due));
    }

    /// This fight's result screen is wholly read (on this read or an earlier one of the same screen) and its run waits
    /// to be sent (fight_summary scheduled it: a run for the leaderboards, not decided yet): send it now.
    pub fn send_when_read(&mut self, f: &Fight) {
        let path = self.summary_path(f);
        if self.rs.result_seen > 0.0 && self.rs.result_ready >= self.rs.result_seen && self.rs.pending_run.as_ref().is_some_and(|(p, _)| *p == path) {
            log::info!(target: "meter", "leaderboard: the result screen is read - sending the run now");
            self.decide_run(&path);
        }
    }

    /// A later read of this fight's result screen (the "again" path of result_screen): written to its summary, the run
    /// sent once it is read whole, and for a run already sent, its update looked at BATTLE_DATA_SETTLE after this read
    /// (a number or name a later read corrects gets to the site; an unchanged read sends nothing).
    pub fn result_read_again(&mut self, f: &Fight, result: Value, now: f64) {
        self.fight_summary(f, vec![("result", result)]);
        self.send_when_read(f);
        self.sent_run_written(f, now);
    }

    /// Battle Data or a later result-screen read was written to a sent run's summary: its update is looked at
    /// BATTLE_DATA_SETTLE after the last write (Battle Data that never reads complete still gets there; a complete read
    /// sends at once, battle_data_done).
    fn sent_run_written(&mut self, f: &Fight, now: f64) {
        let path = self.summary_path(f);
        let open = read_json(&path).and_then(|v| v.as_dict().cloned()).is_some_and(|s| self.late_open(&s));
        if self.submitter.is_some() && open {
            self.followup_at(&path, now + rp::BATTLE_DATA_SETTLE);
        }
    }

    /// The run's clock when the leader's Special Offering read as Overtime, else None.
    pub fn overtime_limit(&self, now: f64) -> Option<i64> {
        let p = self.party.as_ref()?;
        let recent = now - p.get("at").as_f64().unwrap_or(0.0) < 900.0;
        let picked = recent && p.get("buffs").as_list().is_some_and(|b| b.iter().any(|x| x.get("name").as_str() == Some("Overtime")));
        picked.then(|| self.args.duration as i64 + crate::fights::OVERTIME_S)
    }

    /// This week's Interlink boss with this name, or None.
    pub fn week_row(&self, name: &str) -> Option<Row> {
        let active = self.env.schedule.active_bosses(self.env.now());
        self.catalog
            .iter()
            .find(|r| get(r, "table") == "puppet_data" && get(r, "bossTitle_en") == name && get(r, "puppetId").parse::<i64>().is_ok_and(|p| active.contains(&p)))
            .cloned()
    }

    /// One row in logs/scores.csv per new value (plus a picture), for the leaderboards.
    #[allow(clippy::too_many_arguments)]
    pub fn record_score(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, screen: &str, boss: &str, score: Option<i64>, stars: Option<i64>) {
        if self.rs.last_scores.get(boss) == Some(&(score, stars)) {
            return;
        }
        self.rs.last_scores.insert(boss.to_string(), (score, stars));
        let now = self.env.now();
        let shot = self.picture(frames, im, "screens", &format!("{screen}_{}", local(now, "%Y%m%d_%H%M%S")), screen == "challenge_stats" || screen == "battle_prep", true);
        let path = self.env.data.join("logs").join("scores.csv");
        let new = !path.exists();
        let _ = std::fs::create_dir_all(self.env.data.join("logs"));
        if let Ok(fh) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let mut w = csv::WriterBuilder::new().terminator(csv::Terminator::CRLF).from_writer(fh);
            if new {
                let _ = w.write_record(["time", "uid", "cycle", "screen", "boss", "score", "stars", "screenshot"]);
            }
            let s = |v: Option<i64>| v.map_or(String::new(), |x| x.to_string());
            let _ = w.write_record([
                local(now, "%Y-%m-%d %H:%M:%S"),
                self.uid.clone().unwrap_or_default(),
                self.env.schedule.active_cycle_id(now).unwrap_or_default(),
                screen.to_string(),
                boss.to_string(),
                s(score),
                s(stars),
                shot.as_ref().map_or(String::new(), |p| rel(&self.env.data, p)),
            ]);
        }
        let mut msg = format!("score recorded: {boss} - best score {}", score.map_or("None".into(), thousands));
        if let Some(st) = stars {
            msg += &format!(", {st} stars");
        }
        out.out("status", Value::Str(msg));
        let Some(sub) = self.submitter.clone() else { return };
        if screen != "challenge_stats" && screen != "battle_prep" {
            return;
        }
        let mut boss = boss.to_string();
        let mut row = None;
        if screen == "battle_prep" {
            row = self.week_row(&boss);
            let Some(r) = &row else {
                log::info!(target: "meter", "leaderboard: prep-screen best for {} not sent - not one of this week's bosses", alogs_pyval::str_repr(&boss));
                return;
            };
            boss = get(r, "bossTitle_en").into();
        }
        let cycle = self.env.schedule.active_cycle_id(now).map_or(Value::None, Value::Str);
        let bid = row.as_ref().map_or(Value::None, |r| Value::Str(get(r, "puppetId").into()));
        let clock = alogs_gamelog::FixedClock(now);
        let p = crate::payloads::score_payload(screen, &boss, &opt_i(score), &opt_i(stars), &self.uid.clone().map_or(Value::None, Value::Str), &cycle, &bid, &self.env.schedule, &clock);
        let ev = shot.map(|p| rel(&self.env.data, &p));
        sub.enqueue(&p, ev.as_deref(), None, None, None);
    }

    pub fn hide_end_note(&mut self, out: &mut dyn Out) {
        if self.rs.end_sig.is_some() {
            out.out("end_hide", Value::None);
            self.rs.end_sig = None;
        }
    }

    /// Between fights: hide the end-screen note the moment the screen under it changes.
    pub fn watch_end_note(&mut self, out: &mut dyn Out, im: &Image) {
        if let Some(sig) = &self.rs.end_sig
            && team::screen_changed(&team::screen_sig(im), sig, 24.0)
        {
            self.hide_end_note(out);
        }
    }

    /// Interlink: the game's line when the party leader ends a run early.
    pub fn leader_ended_note(&mut self, im: &Image, now: f64) {
        if now - self.rs.leader_look < 1.0 || !self.frame_full {
            return;
        }
        let f = self.fight.as_ref().or(self.last_fight.as_ref().map(|x| &x.0));
        let Some(f) = f else { return };
        if !f.bossrush || (self.fight.is_none() && now - self.fight_ended_at > 20.0) {
            return;
        }
        let name = f.csv_name.clone();
        self.rs.leader_look = now;
        let (w, h) = (im.width as f64, im.height as f64);
        let u = w.min(h * 16.0 / 9.0);
        let ux = (w - u) / 2.0;
        let (x0, y0, x1, y1) = LEADER_NOTE;
        let b = (ux + u * x0, h * y0, ux + u * x1, h * y1);
        let c = im.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64).convert(Mode::L);
        let hist = c.histogram(None);
        let white = hist[230..].iter().sum::<u64>() as f64 / (c.width * c.height).max(1) as f64;
        if !(0.01..=0.15).contains(&white) || Stat::new(&c, None).mean()[0] > 140.0 {
            return;
        }
        let mut crops = IndexMap::new();
        crops.insert("n".to_string(), prep::prep_white(im, b, u / 2000.0, 4.0));
        let t = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new()).get("n").cloned().unwrap_or_default();
        let re = regex::Regex::new(r"(?i)leader\W+has\W+ended|ended\W+the\W+challenge").unwrap();
        if re.is_match(&t) {
            if self.rs.leader_ended.as_ref() != Some(&name) {
                log::info!(target: "meter", "Interlink: the team leader ended the challenge ({})", alogs_pyval::str_repr(&t));
            }
            self.rs.leader_ended = Some(name);
        }
    }

    /// A screen the meter just read, kept as a picture when Keep screenshots says so or as proof until sent.
    pub fn picture(&mut self, frames: &mut dyn Frames, im: &Image, folder: &str, name: &str, proof: bool, fresh: bool) -> Option<PathBuf> {
        let keep = alogs_pyval::py_int(self.args.cfg.get("capture").get("keep_hours")).unwrap_or(0);
        let sending = proof && self.submitter.as_ref().is_some_and(|s| s.config.lock().unwrap().as_ref().is_some_and(|c| c.screenshots));
        if keep == 0 && !sending {
            return None;
        }
        let im = if proof && fresh {
            let settled = self.settled_frame(frames, im);
            self.game_alone(frames, &settled)
        } else {
            im.clone()
        };
        if proof {
            self.proof_with_overlay(&im, name);
        }
        save_picture(&self.env.data, &im, folder, name, if sending { 3840 } else { 1920 })
    }

    /// Next to each proof (the game alone, which is what's sent), a copy with the DPS window drawn where it was on
    /// screen a moment later (its result note comes up just after), for the player's own troubleshooting: with the test
    /// pictures (logs/test_frames/<day>/proof_overlay), never sent. Only while Save test pictures is on.
    pub fn proof_with_overlay(&self, im: &Image, name: &str) {
        let Some(own) = self.own.clone() else { return };
        if self.args.replay || !self.args.cfg.get("capture").get("test_frames").truthy() {
            return;
        }
        let (origin, im, name) = (self.origin, im.clone(), name.to_string());
        let dir = self.env.data.join("logs").join("test_frames").join(local(self.env.now(), "%Y%m%d")).join("proof_overlay");
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let Some((dps, x, y)) = own.lock().unwrap().dps_now.clone() else { return };
            let mut copy = im.convert(alogs_pil::Mode::Rgba);
            copy.alpha_composite(&dps.convert(alogs_pil::Mode::Rgba), i64::from(x - origin.0), i64::from(y - origin.1));
            let rgb = copy.convert(alogs_pil::Mode::Rgb);
            let mut out = vec![];
            let ok = jpeg_encoder::Encoder::new(&mut out, 92).encode(&rgb.data, rgb.width as u16, rgb.height as u16, jpeg_encoder::ColorType::Rgb).is_ok();
            if ok && std::fs::create_dir_all(&dir).is_ok() {
                if let Err(e) = std::fs::write(dir.join(format!("{name}_overlay.jpg")), out) {
                    log::info!(target: "meter", "proof copy with the overlay not saved ({e})");
                }
            }
        });
    }

    /// A proof is of the game, not of what's on top of it: with a screen copy, the game window alone is taken too.
    pub fn game_alone(&mut self, frames: &mut dyn Frames, im: &Image) -> Image {
        self.rs.screen_check = None;
        if self.args.replay {
            return im.clone();
        }
        if !frames.screen_copy() {
            self.rs.screen_check = Some(dict([("source", Value::Str("window".into()))]));
            return im.clone();
        }
        let copy = self.hwnd.and_then(alogs_capture::win::window_copy);
        let bad = copy.as_ref().is_none_or(|c| c.size() != im.size() || Stat::new(&c.resize(64, 36, Some(Filter::Box)), None).mean().into_iter().fold(0.0, f64::max) < 3.0);
        if bad {
            self.rs.screen_check = Some(dict([("source", Value::Str("screen".into())), ("copy", Value::Str("failed".into()))]));
            return im.clone();
        }
        let copy = copy.unwrap();
        let (worst, o25, o40) = rp::screen_difference(im, &copy);
        self.rs.screen_check = Some(dict([
            ("source", Value::Str("screen".into())),
            ("copy", Value::Str("ok".into())),
            ("worst", Value::Int(i64::from(worst))),
            ("over25", Value::Int(o25 as i64)),
            ("over40", Value::Int(o40 as i64)),
        ]));
        log::info!(target: "meter", "proof: screen against the game window alone: {{'worst': {worst}, 'over25': {o25}, 'over40': {o40}}}");
        if o40 as i64 <= rp::SCREEN_MOVED_ON { copy } else { im.clone() }
    }

    /// The screen once it has stopped moving (fresh whole frames until two in a row match, at most three looks).
    pub fn settled_frame(&mut self, frames: &mut dyn Frames, im: &Image) -> Image {
        if self.args.replay {
            return im.clone();
        }
        let now = self.env.now();
        frames.set_clean_until(((now + 3.0) * 1000.0) as u64);
        // (a boxes-only frame is nothing to compare with: the first whole frame after it stands in, as in the Python)
        let first = (!self.ss.read_parts).then(|| team::screen_sig(im));
        let (mut last, mut sig) = (im.clone(), first.clone());
        for _ in 0..3 {
            std::thread::sleep(std::time::Duration::from_secs_f64(rp::SETTLE_S));
            let Some(fresh) = self.hwnd.and_then(|h| frames.grab(h, None)) else { break };
            let fsig = team::screen_sig(&fresh);
            if first.as_ref().is_some_and(|f| team::screen_changed(&fsig, f, 24.0)) {
                break;
            }
            last = fresh;
            if sig.as_ref().is_some_and(|s| !team::screen_changed(&fsig, s, 4.0)) {
                break;
            }
            sig = Some(fsig);
        }
        frames.set_clean_until(((self.env.now() + 0.1) * 1000.0) as u64);
        last
    }

    /// The result screen's four player-name boxes: prepared, and grey crops for the Latin model.
    pub fn result_names(im: &Image, u: f64, ux: f64, h: f64) -> (IndexMap<String, Image>, IndexMap<String, Image>) {
        let (mut crops, mut grey) = (IndexMap::new(), IndexMap::new());
        for i in 0..4 {
            let dx = i as f64 * 0.1765;
            let b = (ux + u * (0.13 + dx), h * 0.618, ux + u * (0.24 + dx), h * 0.655);
            crops.insert(format!("name{i}"), prep::prep_text(im, b, u / 2000.0, 130, 3.0, 24));
            grey.insert(format!("name{i}"), prep::grey_crop(im, b, 48));
        }
        (crops, grey)
    }

    fn you_if_verified(&self) -> Option<String> {
        if team::character_verified(&self.env.data, self.uid.as_deref()) { self.character_name.clone() } else { None }
    }

    /// The result screen right after a boss: rating, score, time and each player's highlight stat.
    #[allow(clippy::too_many_arguments)]
    pub fn result_screen(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64, u: f64, ux: f64, h: f64, banner: &str, names: Option<(IndexMap<String, Image>, IndexMap<String, Image>)>) {
        let again = now - self.rs.result_seen < 30.0;
        if again && (self.rs.result_reads >= rp::SCREEN_READS || now - self.rs.result_last < 0.45) {
            return;
        }
        if !again {
            self.rs.result_seen = now;
            self.rs.result_reads = 0;
            self.rs.result_votes = Votes::new();
            self.rs.result_shot = None;
            self.rs.result_look = None;
        }
        self.rs.result_last = now;
        self.rs.result_reads += 1;
        if self.fight.is_some() {
            self.end_fight(out, "result screen");
        }
        let k = u / 2000.0;
        let b = |x0: f64, y0: f64, x1: f64, y1: f64| (ux + u * x0, h * y0, ux + u * x1, h * y1);
        let mut crops: IndexMap<String, Image> = IndexMap::new();
        crops.insert("rating".into(), prep::prep_short(im, b(0.20, 0.33, 0.275, 0.41), k, 3));
        crops.insert("score".into(), prep::prep_text(im, b(0.43, 0.325, 0.57, 0.415), k, 190, 1.0, 24));
        crops.insert("time".into(), prep::prep_short(im, b(0.68, 0.33, 0.79, 0.41), k, 3));
        let (named, grey) = names.unwrap_or_else(|| Self::result_names(im, u, ux, h));
        for i in 0..4 {
            let dx = i as f64 * 0.1765;
            crops.insert(format!("name{i}"), named[&format!("name{i}")].clone());
            crops.insert(format!("stat{i}"), prep::prep_text(im, b(0.21 + dx, 0.575, 0.33 + dx, 0.625), k, 160, 3.0, 24));
        }
        let pad: IndexMap<String, usize> = (0..4).map(|i| (format!("name{i}"), 24)).collect();
        let t = self.ocr.read(&crops, &pad, &grey);
        let tg = |k: &str| t.get(k).map(String::as_str);
        let (sure, stars, sure_time, values): (Option<i64>, Option<i64>, Option<i64>, Vec<Option<i64>>) = match self.name_reader() {
            Some(r) => (
                r.number(im, b(0.43, 0.325, 0.57, 0.415), 0.05, false, false, false),
                r.number(im, b(0.20, 0.33, 0.275, 0.41), 0.05, false, false, false),
                r.number(im, b(0.68, 0.33, 0.79, 0.41), 0.05, false, false, true),
                (0..4).map(|i| {
                    let dx = i as f64 * 0.1765;
                    r.number(im, b(0.21 + dx, 0.600, 0.33 + dx, 0.624), 0.05, false, false, false)
                })
                .collect(),
            ),
            None => (None, None, None, vec![None; 4]),
        };
        // this look's own reads (not the votes), to tell whether two looks in a row agree (result_ready)
        let mut look: Vec<Value> = vec![
            opt_i(stars.or_else(|| rp::first_num(tg("rating")))),
            opt_i(sure.or_else(|| num(tg("score")))),
            opt_i(sure_time.or_else(|| rp::first_num(tg("time")))),
        ];
        let mut v = |key: &str, val: Value| vote(&mut self.rs.result_votes, key, val);
        let rating = v("rating", look[0].clone()).as_i64();
        let score = v("score", look[1].clone()).as_i64();
        let secs = v("time", look[2].clone()).as_i64();
        let fidx = self.last_fight.as_ref().filter(|(_, at)| now - at < 180.0).is_some();
        let run = if fidx { Some(self.run_of(&self.last_fight.as_ref().unwrap().0)) } else { None };
        let full = self.clear_stars(run.as_ref().map(|r| r.id.as_str()));
        let score = interlink_score(score, rating, Some(full), run.as_ref().is_some_and(|r| r.bosses.is_some()));
        let num_re = regex::Regex::new(r"^[\d,.\s]+$").unwrap();
        let mut players: Vec<Dict> = Vec::new();
        for (i, sv) in values.iter().enumerate() {
            let name_read = Value::Str(tg(&format!("name{i}")).unwrap_or("").trim_matches(|c| " /|".contains(c)).to_string());
            look.push(name_read.clone());
            let name = vote(&mut self.rs.result_votes, &format!("name{i}"), name_read);
            let stat = tg(&format!("stat{i}")).unwrap_or("");
            let parts: Vec<&str> = stat.split(" / ").map(str::trim).filter(|p| !p.is_empty()).collect();
            let value_read = opt_i(sv.or_else(|| parts.iter().rev().find(|p| num_re.is_match(p)).and_then(|p| num(Some(p)))));
            look.push(value_read.clone());
            let value = vote(&mut self.rs.result_votes, &format!("value{i}"), value_read);
            let label = vote(&mut self.rs.result_votes, &format!("label{i}"), Value::Str(rp::canon(parts.iter().find(|p| !num_re.is_match(p)).copied().unwrap_or(""), &rp::RESULT_STATS, 0.6)));
            let lbl = label.py_str();
            if name.truthy() || !value.is_none() || rp::RESULT_STATS.contains(&lbl.as_str()) {
                players.push(Dict::from_iter([
                    (Value::Str("slot".into()), Value::Int(i as i64)),
                    (Value::Str("name".into()), name),
                    (Value::Str("stat".into()), label),
                    (Value::Str("value".into()), value),
                ]));
            }
        }
        let party = self.party_for_result(now);
        let sure_slots: std::collections::BTreeSet<i64> = (0..4).filter(|i| self.ocr_sure(&named[&format!("name{i}")])).collect();
        let agreed = rp::agreed_slots(&self.rs.result_votes, |i| format!("name{i}"));
        let you = self.you_if_verified();
        party_slots(&self.team, &mut players, party.as_deref(), &sure_slots, you.as_deref(), &agreed);
        let mine = you.clone().or_else(|| {
            let names: Vec<String> = players.iter().map(|p| p.at("name").py_str()).collect();
            own_name(self.character_name.as_deref(), &names, party.as_deref())
        });
        match &mine {
            Some(m) if Some(m) != self.character_name.as_ref() => {
                log::info!(target: "meter", "character for uid {}: {m}", self.uid.clone().unwrap_or_else(|| "None".into()));
                self.character_name = Some(m.clone());
                team::remember_character(&self.env.data, self.uid.as_deref(), Some(m), false);
                if let (Some(s), Some(u)) = (&self.submitter, &self.uid) {
                    s.link_character(u, m, None);
                }
            }
            None => log::info!(target: "meter", "result screen: not sure which slot is you (a party with other players, and your name not known yet), so the character name is left as it is"),
            _ => {}
        }
        let limit = {
            let lf = self.last_fight.as_ref().filter(|_| fidx).map(|x| &x.0);
            lf.and_then(|f| f.limit.filter(|l| *l != 0)).or_else(|| self.overtime_limit(now)).unwrap_or(self.args.duration as i64)
        };
        let mut cleared = false;
        if fidx {
            let f = &mut self.last_fight.as_mut().unwrap().0;
            cleared = secs.is_some_and(|s| 0 < s && s < limit) && f.bossrush && rating.unwrap_or(0) >= full;
            if cleared {
                f.elapsed = secs.unwrap() as f64;
                log::info!(target: "meter", "result screen: cleared in {}s", secs.unwrap());
            } else if secs.is_some_and(|s| s < limit) {
                let left = if run.as_ref().is_some_and(|r| r.bosses.is_some()) {
                    format!("{:.0}% of {}, boss {} of 2,", if f.boss_hp != 0 { (f.max_hp - f.damage) as f64 / f.boss_hp as f64 * 100.0 } else { 0.0 }, f.title, f.order.unwrap_or(0))
                } else {
                    format!("{:.0}%", if f.max_hp != 0 { (f.max_hp - f.damage) as f64 / f.max_hp as f64 * 100.0 } else { 0.0 })
                };
                log::info!(target: "meter", "result screen: ended at {}s without a clear (rating {}, {left} HP left on the meter)", secs.unwrap(), rating.map_or("None".into(), |r| r.to_string()));
            }
        }
        let ended_by_leader = fidx && self.rs.leader_ended.as_ref() == Some(&self.last_fight.as_ref().unwrap().0.csv_name);
        let boss = run.as_ref().map_or("(unknown - meter saw no fight)".to_string(), |r| r.title.clone());
        let players_v = |players: &Vec<Dict>| Value::List(players.iter().cloned().map(Value::Dict).collect());
        log::info!(target: "meter", "result screen read {}: banner {}, -> {boss}: rating {rating:?}, score {score:?}, time {secs:?}, players {}", self.rs.result_reads, alogs_pyval::str_repr(banner), players_v(&players).repr());
        // wholly read, and this look read what the look before it (of this same screen) read: a first look can catch
        // the screen still fading in, with a name read wrong
        let look = Value::List(look).json_dumps();
        let agrees = self.rs.result_look.as_deref() == Some(look.as_str());
        self.rs.result_look = Some(look);
        let whole = rating.is_some() && score.is_some() && secs.is_some() && !players.is_empty() && players.iter().all(|p| !p.at("value").is_none());
        if whole && !agrees && self.rs.result_ready < self.rs.result_seen {
            log::info!(target: "meter", "result screen read {}: read whole, waiting for a look that agrees with it", self.rs.result_reads);
        }
        if whole && agrees && self.rs.result_ready < self.rs.result_seen {
            self.rs.result_ready = now;
            // the run's proof is of this look's screen, taken before the click below changes it
            if again {
                self.retake_result_picture(frames, im, now);
            }
            // click_when_ready: the automatic Battle Data click with this frame, at once
            if self.auto_active() && self.auto.clicks == 0 && !self.args.replay {
                let fg = self.hwnd.is_some_and(|w| frames.is_foreground(w));
                self.open_battle_data(out, im, u, ux, h, fg);
            }
        }
        let best = banner.to_lowercase().contains("best");
        let result = |shot: &Option<String>, check: &Option<Value>, players: Value, reads: Option<i64>| {
            let mut d = Dict::new();
            for (k, v) in [
                ("new_best", Value::Bool(best)),
                ("rating_stars", opt_i(rating)),
                ("score", opt_i(score)),
                ("time_s", opt_i(secs)),
                ("cleared", Value::Bool(cleared)),
                ("star_cap", Value::Int(full)),
                ("players", players),
                ("ended_by_leader", Value::Bool(ended_by_leader)),
                ("screenshot", shot.clone().map_or(Value::None, Value::Str)),
            ] {
                d.insert(k.into(), v);
            }
            if let Some(r) = reads {
                d.insert("reads".into(), Value::Int(r));
            }
            d.insert("screen_check".into(), check.clone().unwrap_or(Value::None));
            Value::Dict(d)
        };
        if again {
            mark_you(&mut players, party.as_deref(), self.character_name.as_deref());
            if fidx {
                let r = result(&self.rs.result_shot, &self.rs.result_check, players_v(&players), Some(self.rs.result_reads));
                let f = self.last_fight.take().unwrap();
                self.result_read_again(&f.0, r, now);
                self.last_fight = Some(f);
            }
            return;
        }
        let stamp = local(now, "%Y%m%d_%H%M%S");
        let shot = self.picture(frames, im, "screens", &format!("result_{stamp}"), true, true);
        self.rs.result_shot = shot.as_ref().map(|p| rel(&self.env.data, p));
        self.rs.result_check = self.rs.screen_check.clone();
        // logs/results.csv
        let path = self.env.data.join("logs").join("results.csv");
        let new = !path.exists();
        if let Ok(fh) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let mut w = csv::WriterBuilder::new().terminator(csv::Terminator::CRLF).from_writer(fh);
            if new {
                let _ = w.write_record(["time", "uid", "cycle", "boss", "level", "new_best", "rating_stars", "score", "time_s", "players", "meter_damage", "meter_seconds", "meter_dps", "meter_csv", "screenshot"]);
            }
            let fi = self.last_fight.as_ref().filter(|_| fidx).map(|x| &x.0);
            let dmg = run.as_ref().map(|r| r.damage);
            let s = |v: Option<i64>| v.map_or(String::new(), |x| x.to_string());
            let _ = w.write_record([
                local(now, "%Y-%m-%d %H:%M:%S"),
                self.uid.clone().unwrap_or_default(),
                self.env.schedule.active_cycle_id(now).unwrap_or_default(),
                boss.clone(),
                s(fi.map(|f| f.level)),
                if best { "True".into() } else { "False".into() },
                s(rating),
                s(score),
                s(secs),
                json_unicode(&players_v(&players)),
                s(dmg),
                s(fi.map(|f| f.elapsed.round_ties_even() as i64)),
                s(fi.filter(|f| f.elapsed > 1.0).and_then(|f| dmg.map(|d| (d as f64 / f.elapsed).round_ties_even() as i64))),
                fi.map_or(String::new(), |f| format!("{}.csv", f.csv_name)),
                self.rs.result_shot.clone().unwrap_or_default(),
            ]);
        }
        mark_you(&mut players, party.as_deref(), self.character_name.as_deref());
        let mut party_shot = None;
        if fidx
            && party.is_some()
            && let Some(pic) = self.party_picture.clone()
        {
            party_shot = self.picture(frames, &pic, "screens", &format!("party_{stamp}"), true, false);
        }
        if fidx {
            let r = result(&self.rs.result_shot, &self.rs.result_check, players_v(&players), None);
            let mut parts = vec![("result", r)];
            if let Some(p) = &party {
                parts.push(("party", Value::List(p.clone())));
                let pv = self.party.clone().unwrap();
                parts.push(("team", pv.get("team").clone()));
                parts.push(("teams", pv.get("teams").clone()));
                parts.push(("buffs", pv.get("buffs").clone()));
            }
            if let Some(ps) = &party_shot {
                parts.push(("party_screenshot", Value::Str(rel(&self.env.data, ps))));
            }
            let f = self.last_fight.take().unwrap();
            self.fight_summary(&f.0, parts);
            self.send_when_read(&f.0);
            self.last_fight = Some(f);
            if best && score.is_some() {
                self.record_score(frames, out, im, "new_best", &boss, score, rating);
            }
        }
        out.out("status", Value::Str(format!("result recorded: {boss} - score {}, {} stars, {}s", score.map_or("None".into(), |s| s.to_string()), rating.map_or("None".into(), |s| s.to_string()), secs.map_or("None".into(), |s| s.to_string()))));
        self.rs.end_sig = Some(team::screen_sig(im));
    }

    /// The result screen read ready: its proof again, from this look (the first look's can still be fading in). The
    /// first look's picture is gone unless Keep screenshots keeps pictures; without a new one the first stays.
    fn retake_result_picture(&mut self, frames: &mut dyn Frames, im: &Image, now: f64) {
        let stamp = local(now, "%Y%m%d_%H%M%S");
        let Some(p) = self.picture(frames, im, "screens", &format!("result_{stamp}"), true, true) else { return };
        let new = rel(&self.env.data, &p);
        let keep = alogs_pyval::py_int(self.args.cfg.get("capture").get("keep_hours")).unwrap_or(0) != 0;
        if let Some(old) = self.rs.result_shot.replace(new.clone())
            && old != new
            && !keep
        {
            let _ = std::fs::remove_file(self.env.data.join(old));
        }
        self.rs.result_check = self.rs.screen_check.clone();
    }

    /// Is this name box's read in the name cache for good? (`sure_read`: only the real OCR keeps one.)
    pub fn ocr_sure(&self, img: &Image) -> bool {
        self.ocr.sure(img)
    }

    /// How many cards Battle Data shows, counted from pixels (0 when they don't line up like cards).
    pub fn bd_card_count(im: &Image, u: f64, ux: f64, h: f64) -> usize {
        let runs = light_runs(im, u, ux, h, 400, 0.0);
        let cards: Vec<(f64, f64)> = runs.into_iter().filter(|(a, b)| b - a >= 0.12).collect();
        let n = cards.len();
        if !(1..=4).contains(&n) {
            return 0;
        }
        for (i, (a, b)) in cards.iter().enumerate() {
            if b - a > 0.19 || ((a + b) / 2.0 - (0.5 + (i as f64 - (n as f64 - 1.0) / 2.0) * 0.1943)).abs() > 0.02 {
                return 0;
            }
        }
        n
    }

    /// Whether Battle Data's n cards are where they rest, not on their way in.
    pub fn bd_cards_in_place(im: &Image, u: f64, ux: f64, h: f64, n: usize) -> bool {
        if n == 0 {
            return false;
        }
        let runs = light_runs(im, u, ux, h, 1600, 0.12);
        if runs.len() != n {
            return false;
        }
        let (y0, y1) = ((h * 0.70) as i64, (h * 0.80) as i64);
        for (i, (a, b)) in runs.iter().enumerate() {
            if ((a + b) / 2.0 - (0.5 + (i as f64 - (n as f64 - 1.0) / 2.0) * 0.1943)).abs() > 0.002 {
                return false;
            }
            let x = (ux + u * (a + 0.01)) as i64;
            let col = im.crop_i(x, y0, x + 1, y1).convert(Mode::Rgb);
            let light: Vec<usize> = (0..(y1 - y0) as usize)
                .filter(|&k| {
                    let p = &col.data[3 * k..3 * k + 3];
                    let (mn, mx) = (*p.iter().min().unwrap(), *p.iter().max().unwrap());
                    mn >= 170 && mx - mn <= 50
                })
                .collect();
            let Some(&lastk) = light.last() else { return false };
            if !(0.742..=0.749).contains(&((y0 as f64 + lastk as f64) / h)) {
                return false;
            }
        }
        true
    }

    /// Each card's values by the game's own digits (a frame whose cards aren't in place gives none).
    pub fn battle_data_font(&mut self, im: &Image, u: f64, ux: f64, h: f64, read: &mut [Dict]) {
        let n = read.len();
        if self.name_reader().is_none() {
            return;
        }
        if !Self::bd_cards_in_place(im, u, ux, h, n) {
            log::info!(target: "meter", "Battle Data: cards not in place yet, no values from this frame");
            for card in read.iter_mut() {
                for (k, _) in BD_ROWS {
                    card.insert(k.into(), Value::None);
                }
            }
            return;
        }
        let reader = self.font_reader.as_ref().unwrap();
        let k = 3840.0 / u;
        let mut big: Option<Image> = None;
        let (y0, y1) = (h * 0.54, h * 0.74);
        for (i, card) in read.iter_mut().enumerate() {
            let c = ux + u * (0.5 + (i as f64 - (n as f64 - 1.0) / 2.0) * 0.1943);
            for (key, y) in BD_ROWS {
                let b = (c + u * 0.015, h * (y - 0.016), c + u * 0.081, h * (y + 0.016));
                let mut v = reader.number(im, b, 0.05, true, true, false);
                if v.is_none() && k > 1.01 {
                    let bg = big.get_or_insert_with(|| {
                        let band = im.crop_i(ux as i64, y0 as i64, (ux + u) as i64, y1 as i64);
                        band.resize((band.width as f64 * k).round_ties_even() as usize, (band.height as f64 * k).round_ties_even() as usize, Some(Filter::Lanczos))
                    });
                    let (ox, oy) = ((ux as i64) as f64, (y0 as i64) as f64);
                    v = reader.number(bg, ((b.0 - ox) * k, (b.1 - oy) * k, (b.2 - ox) * k, (b.3 - oy) * k), 0.05, true, true, false);
                }
                card.insert(key.into(), opt_i(v));
            }
        }
    }

    /// Battle Data's name tabs of n cards: prepared, and grey crops.
    pub fn bd_names(im: &Image, n: usize, u: f64, ux: f64, h: f64) -> (IndexMap<String, Image>, IndexMap<String, Image>) {
        let (mut a, mut g) = (IndexMap::new(), IndexMap::new());
        for i in 0..n {
            let c = 0.5 + (i as f64 - (n as f64 - 1.0) / 2.0) * 0.1943;
            let b = (ux + u * (c - 0.045), h * 0.512, ux + u * (c + 0.045), h * 0.547);
            a.insert(i.to_string(), prep::prep_white(im, b, u / 2000.0, 4.0));
            g.insert(i.to_string(), prep::grey_crop(im, b, 48));
        }
        (a, g)
    }

    /// The fight waiting for Battle Data: its team DPS checked against the meter's and the boss's max HP measured.
    pub fn bd_measure(&mut self, mut msg: String, vals: &[i64], now: f64) -> String {
        if self.awaiting.is_none_or(|a| now > a) || vals.is_empty() {
            return msg;
        }
        let Some((f, _)) = self.last_fight.as_ref() else { return msg };
        let game_dps: i64 = vals.iter().sum();
        let t = f.elapsed;
        let loss = if f.max_hp != 0 { f.damage as f64 / f.max_hp as f64 } else { 0.0 };
        let run = self.run_of(f);
        if t > 1.0 {
            msg += &format!(" vs meter {} ({} Lv {})", thousands((run.damage as f64 / t).round_ties_even() as i64), run.title, f.level);
        }
        let to_end = f.ended.unwrap_or(now) - f.t0;
        time_check(&self.env.data, f.bossrush, f.elapsed, to_end, &run.title, run.damage, game_dps, now);
        let mut calibrate = f.bossrush && f.started_at_top && loss > 0.02 && t > 60.0;
        if calibrate && run.bosses.is_some() {
            calibrate = false;
            log::info!(target: "meter", "Battle Data: no max HP measured from a two-boss fight: its team DPS is the run's, not one boss's");
        }
        if calibrate {
            let measured = game_dps as f64 * t / loss;
            if let Some(predicted) = alogs_reader::boss::boss_max_hp(&f.row, f.level, None)
                && predicted != 0
                && (0.5..=2.0).contains(&(measured / predicted as f64))
            {
                let kept = save_calibration(&self.env.data, get(&f.row, "puppetId"), f.level, measured, predicted, &format!("{} game {game_dps} T {t:.0}", local(now, "%Y-%m-%d %H:%M")));
                msg += &format!(" | max HP measured {} (predicted {}); using {} from now on", thousands(measured.round_ties_even() as i64), thousands(predicted), thousands(kept));
            }
        }
        self.awaiting = None;
        msg
    }

    /// Battle Data on screen: each card's four values and its player, voted over the reads of one appearance.
    #[allow(clippy::too_many_arguments)]
    pub fn battle_data(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64, u: f64, ux: f64, h: f64, head: &str, names: Option<(IndexMap<String, Image>, IndexMap<String, Image>)>) {
        if self.fight.is_some() {
            self.end_fight(out, "Battle Data screen");
        }
        let shown_already = now - self.rs.bd_seen < 6.0;
        self.rs.bd_seen = now;
        let party = self.party_for_result(now).unwrap_or_default();
        let budget = rp::SCREEN_READS * if party.len() > self.rs.bd_cards { 2 } else { 1 };
        let again = shown_already && now - self.rs.battle_saved < 30.0 && self.rs.bd_reads < budget && now - self.rs.bd_last >= 0.5;
        if (shown_already || now - self.rs.battle_saved < 30.0) && !again {
            return;
        }
        self.rs.bd_last = now;
        if !again {
            self.rs.battle_saved = now;
            self.rs.bd_reads = 0;
            self.rs.bd_votes = Votes::new();
            self.rs.bd_shot = None;
            self.rs.bd_cards = 0;
            self.rs.bd_prev_sig = None;
            self.rs.bd_measured = false;
            self.rs.bd_stamp = None;
            log::info!(target: "meter", "Battle Data screen detected (header OCR {})", alogs_pyval::str_repr(head));
        }
        self.rs.bd_reads += 1;
        let rows = im.crop_i((ux + u * 0.125) as i64, (h * 0.564) as i64, (ux + u * 0.875) as i64, (h * 0.73) as i64).convert(Mode::L);
        let k = 1f64.max(2.0 * 2000.0 / u);
        let rows = rows.resize((rows.width as f64 * k) as usize, (rows.height as f64 * k) as usize, Some(Filter::Lanczos));
        let mut crops = IndexMap::new();
        crops.insert("d".to_string(), expand(&rows, 20, &[255]));
        let text = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new()).get("d").cloned().unwrap_or_default();
        let parsed = rp::parse_battle_data(Some(&text));
        log::info!(target: "meter", "Battle Data read {}: OCR {} -> {}", self.rs.bd_reads, alogs_pyval::str_repr(&text), Value::List(parsed.clone()).repr());
        let before = self.rs.bd_cards;
        let seen = Self::bd_card_count(im, u, ux, h);
        let n = 4.min(before.max(seen).max(parsed.len()));
        self.rs.bd_cards = n;
        let mut read: Vec<Dict> = (0..n).map(|i| parsed.get(i).and_then(|c| c.as_dict().cloned()).unwrap_or_default()).collect();
        self.battle_data_font(im, u, ux, h, &mut read);
        if n != before {
            let mut said = if seen > 0 {
                if seen >= parsed.len() { "counted on screen".to_string() } else { format!("OCR's count (only {seen} counted on screen)") }
            } else {
                "OCR's count (not counted on screen)".to_string()
            };
            if !party.is_empty() && party.len() != n {
                said += &format!(", but the Party screen had {} players", party.len());
            }
            log::info!(target: "meter", "Battle Data cards: {said} -> {n} cards");
        }
        let keys = ["avg_dps", "break", "taken", "heal"];
        for (i, c) in read.iter().enumerate() {
            for kk in keys {
                vote(&mut self.rs.bd_votes, &format!("{i}|{kk}"), c.at(kk).clone());
            }
        }
        let mut cards: Vec<Dict> = (0..n).map(|i| keys.iter().map(|kk| (Value::Str((*kk).into()), rp::confirmed(&self.rs.bd_votes, &format!("{i}|{kk}")))).collect()).collect();
        let sig: Vec<Vec<Option<i64>>> = read.iter().map(|c| keys.iter().map(|kk| c.at(kk).as_i64()).collect()).collect();
        let agreed = !read.is_empty() && self.rs.bd_prev_sig.as_ref() == Some(&sig);
        self.rs.bd_prev_sig = Some(sig);
        if agreed {
            for (i, c) in cards.iter_mut().enumerate() {
                for kk in keys {
                    c.insert(Value::Str(kk.into()), read[i].at(kk).clone());
                }
            }
        }
        let (bcrops, grey) = match names {
            Some(nm) if nm.0.len() == n => nm,
            _ => Self::bd_names(im, n, u, ux, h),
        };
        let pad: IndexMap<String, usize> = bcrops.keys().map(|k| (k.clone(), 0)).collect();
        let nread = if n > 0 { self.ocr.read(&bcrops, &pad, &grey) } else { IndexMap::new() };
        for (i, c) in cards.iter_mut().enumerate() {
            let nm = nread.get(&i.to_string()).map_or("", |s| s.trim_matches(|ch| " /|".contains(ch))).to_string();
            let voted = vote(&mut self.rs.bd_votes, &format!("{i}|name"), if nm.is_empty() { Value::None } else { Value::Str(nm) });
            c.insert(Value::Str("slot".into()), Value::Int(i as i64));
            c.insert(Value::Str("name".into()), if voted.truthy() { voted } else { Value::Str(String::new()) });
        }
        let sure: std::collections::BTreeSet<i64> = bcrops.iter().filter(|(_, im)| self.ocr_sure(im)).filter_map(|(k, _)| k.parse().ok()).collect();
        let agreed_names = rp::agreed_slots(&self.rs.bd_votes, |i| format!("{i}|name"));
        let you = self.you_if_verified();
        party_slots(&self.team, &mut cards, Some(&party), &sure, you.as_deref(), &agreed_names);
        let vals: Vec<i64> = cards.iter().filter_map(|c| c.at("avg_dps").as_i64()).collect();
        let lf = self.last_fight.as_ref().is_some_and(|(_, at)| now - at < 180.0);
        let cards_v = Value::List(cards.iter().cloned().map(Value::Dict).collect());
        let team_dps = if vals.is_empty() { Value::None } else { Value::Int(vals.iter().sum()) };
        let complete = agreed && bd_complete(&cards.iter().cloned().map(Value::Dict).collect::<Vec<_>>(), Some(&party));
        if again {
            if agreed && !vals.is_empty() && vals.len() == cards.len() && !self.rs.bd_measured {
                self.rs.bd_measured = true;
                let stamp = self.rs.bd_stamp.clone().unwrap_or_else(|| local(now, "%Y%m%d_%H%M%S"));
                let shot = self.picture(frames, im, "battle_data", &stamp, lf, true);
                if let Some(s) = &shot {
                    self.rs.bd_shot = Some(rel(&self.env.data, s));
                }
                self.rs.bd_check = self.rs.screen_check.clone();
                let msg = format!(
                    "Battle Data screen saved: {} - game team DPS {} (players {})",
                    self.rs.bd_shot.clone().unwrap_or_else(|| "numbers only (no picture kept)".into()),
                    thousands(vals.iter().sum()),
                    vals.iter().map(|v| thousands(*v)).collect::<Vec<_>>().join(", ")
                );
                let msg = self.bd_measure(msg, &vals, now);
                out.out("status", Value::Str(msg.clone()));
                self.bd_note(&stamp, &msg, &cards, &text);
            }
            if lf {
                let bd = dict([
                    ("players", cards_v.clone()),
                    ("team_avg_dps", team_dps.clone()),
                    ("screenshot", self.rs.bd_shot.clone().map_or(Value::None, Value::Str)),
                    ("reads", Value::Int(self.rs.bd_reads)),
                    ("screen_check", self.rs.bd_check.clone().unwrap_or(Value::None)),
                ]);
                let f = self.last_fight.take().unwrap();
                self.fight_summary(&f.0, vec![("battle_data", bd)]);
                self.sent_run_written(&f.0, now);
                self.last_fight = Some(f);
            }
            if complete {
                self.battle_data_done(lf, now);
            }
            return;
        }
        let stamp = local(now, "%Y%m%d_%H%M%S");
        self.rs.bd_stamp = Some(stamp.clone());
        let _ = std::fs::create_dir_all(self.env.data.join("logs").join("battle_data"));
        let shot = self.picture(frames, im, "battle_data", &stamp, lf, true);
        self.rs.bd_shot = shot.as_ref().map(|s| rel(&self.env.data, s));
        self.rs.bd_check = self.rs.screen_check.clone();
        let mut msg = format!("Battle Data screen saved: {}", self.rs.bd_shot.clone().unwrap_or_else(|| "numbers only (no picture kept)".into()));
        if !vals.is_empty() {
            msg += &format!(" - game team DPS {} (players {})", thousands(vals.iter().sum()), vals.iter().map(|v| thousands(*v)).collect::<Vec<_>>().join(", "));
        }
        let msg = self.bd_measure(msg, &vals, now);
        out.out("status", Value::Str(msg.clone()));
        self.rs.end_sig = Some(team::screen_sig(im));
        if lf {
            let bd = dict([("players", cards_v), ("team_avg_dps", team_dps), ("screenshot", self.rs.bd_shot.clone().map_or(Value::None, Value::Str))]);
            let f = self.last_fight.take().unwrap();
            self.fight_summary(&f.0, vec![("battle_data", bd)]);
            self.sent_run_written(&f.0, now);
            self.last_fight = Some(f);
        }
        if complete {
            self.battle_data_done(lf, now);
        }
        self.bd_note(&stamp, &msg, &cards, &text);
    }

    /// Both pictures are in and two reads agree: a run not sent yet goes now, the automatic session stops (and with it
    /// its hold), a sent run's update with its Battle Data goes at once.
    fn battle_data_done(&mut self, lf: bool, now: f64) {
        if let Some((p, _)) = self.rs.pending_run.take() {
            self.rs.pending_run = Some((p, now));
        }
        if self.auto.auto_until != 0.0 {
            self.end_auto("Battle Data read");
        }
        if lf && self.submitter.is_some() {
            let csv = self.last_fight.as_ref().map(|f| f.0.csv_name.clone()).unwrap_or_default();
            self.send_late_battle_data(&csv);
        }
    }

    fn bd_note(&self, stamp: &str, msg: &str, cards: &[Dict], text: &str) {
        let mut s = format!("{msg}\n\n");
        for (i, c) in cards.iter().enumerate() {
            s += &format!(
                "player {}: average DPS {}, BREAK {}, damage taken {}, heal {}\n",
                i + 1,
                c.at("avg_dps").py_str(),
                c.at("break").py_str(),
                c.at("taken").py_str(),
                c.at("heal").py_str()
            );
        }
        s += &format!("\nraw OCR of the stat rows:\n{}\n", text.replace(" / ", "\n"));
        if let Some((f, _)) = &self.last_fight {
            s += &format!("\nmeter fight log: {}.csv\n", f.csv_name);
        }
        let _ = std::fs::write(self.env.data.join("logs").join("battle_data").join(format!("{stamp}.txt")), s);
    }

    /// Sanctum Helper's live status says an Egg Heist run is on (read about once a second).
    pub fn sanctum_in_run(&mut self, now: f64) -> bool {
        if now - self.rs.sh_checked >= 1.0 {
            self.rs.sh_checked = now;
            let p = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SanctumHelper").join("hosted").join("status.json");
            if let Some(st) = read_json(&p)
                && st.get("inRun").truthy()
                && self.env.now() * 1000.0 - alogs_pyval::py_float(st.get("time")).unwrap_or(0.0) < 5000.0
            {
                self.rs.sh_run_seen = now;
            }
        }
        now - self.rs.sh_run_seen < 30.0
    }
}

/// The light runs (card-grey) along the line between Battle Data's BREAK and Damage Taken rows, as shares of u.
fn light_runs(im: &Image, u: f64, ux: f64, h: f64, n: usize, min_len: f64) -> Vec<(f64, f64)> {
    let y = (h * 0.645) as i64;
    let row = im.crop_i(ux as i64, y, (ux + u) as i64, y + 1).convert(Mode::Rgb).resize(n, 1, Some(Filter::Box));
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for x in 0..=n {
        let light = x < n && {
            let p = &row.data[3 * x..3 * x + 3];
            let (mn, mx) = (*p.iter().min().unwrap(), *p.iter().max().unwrap());
            mn >= 190 && mx - mn <= 40
        };
        if light && start.is_none() {
            start = Some(x);
        } else if !light && let Some(s) = start {
            if min_len == 0.0 || (x - s) as f64 >= min_len * n as f64 {
                runs.push((s as f64 / n as f64, x as f64 / n as f64));
            }
            start = None;
        }
    }
    runs
}

/// calibration.json: a measured max HP for a boss at a level (the median of its last runs).
pub fn save_calibration(data: &Path, puppet_id: &str, level: i64, measured: f64, predicted: i64, note: &str) -> i64 {
    let path = data.join("calibration.json");
    let mut cal = match read_json(&path) {
        Some(Value::Dict(d)) => d,
        _ => Dict::new(),
    };
    let key = format!("{puppet_id}@{level}");
    let mut runs: Vec<Value> = cal.get_str(&key).and_then(|v| v.get("runs").as_list().cloned()).unwrap_or_default();
    runs.push(dict([("max_hp", Value::Int(measured.round_ties_even() as i64)), ("note", Value::Str(note.into()))]));
    let mut vals: Vec<i64> = runs.iter().filter_map(|r| r.get("max_hp").as_i64()).collect();
    vals.sort_unstable();
    let med = vals[vals.len() / 2];
    let keep: Vec<Value> = runs.iter().rev().take(10).rev().cloned().collect();
    cal.insert(Value::Str(key), dict([("predicted", Value::Int(predicted)), ("measured", Value::Int(med)), ("runs", Value::List(keep))]));
    let _ = std::fs::write(&path, Value::Dict(cal).json_dumps_indent(1));
    med
}

/// One CSV field as Python's csv writer puts it (quoted when it holds a comma, a quote or a line break).
fn csv_field(v: &str) -> String {
    if v.contains([',', '"', '\n', '\r']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v.to_string() }
}

/// Which fight time the game's Battle Data uses: the time its team DPS implies (damage / DPS) next to the meter's two
/// candidates (to the last frame with the bar, and to the fight's end). One line per fight in logs/time_check.csv.
/// Port of `time_check` (fights.py).
#[allow(clippy::too_many_arguments)]
pub fn time_check(data: &std::path::Path, bossrush: bool, elapsed: f64, to_end: f64, title: &str, damage: i64, game_dps: i64, now: f64) {
    if game_dps == 0 || damage == 0 {
        return;
    }
    let game_s = damage as f64 / game_dps as f64;
    log::info!(target: "meter", "time check: the game's team DPS implies {game_s:.1}s; the meter counted {elapsed:.1}s (to the last bar reading) and {to_end:.1}s (to the end of the fight)");
    let path = data.join("logs").join("time_check.csv");
    let new = !path.exists();
    let r1 = |v: f64| format!("{:.1}", (v * 10.0).round_ties_even() / 10.0);
    let mut text = String::new();
    if new {
        text += "when,boss,interlink,damage,game_team_dps,game_seconds,meter_seconds,to_end_seconds\r\n";
    }
    let bossrush = if bossrush { "True" } else { "False" };
    let row = [local(now, "%Y-%m-%d %H:%M:%S"), csv_field(title), bossrush.into(), damage.to_string(), game_dps.to_string(), r1(game_s), r1(elapsed), r1(to_end)];
    text += &row.join(",");
    text += "\r\n";
    let _ = std::fs::create_dir_all(data.join("logs"));
    use std::io::Write;
    if let Ok(mut fh) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = fh.write_all(text.as_bytes());
    }
}

