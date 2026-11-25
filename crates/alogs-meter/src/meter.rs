//! The reading loop: the `Meter` and what every plug-in shares, and following fights on it. Port of `Meter.step` and
//! its capture/state handling in aLogs B16 `bossbar/live.py`, and of `FightsMixin` in `bossbar/fights.py`.
//!
//! The end-screen readers, the leaderboards plug-in, the screen readers and the diagnostics are in their own modules
//! (results, egg, sim, leaderboards, screens, party, menu, diag); how much of the picture each look copies in pacing.
//! Left out: the Dev switches' calm pacing (busy-half-pace), and the damage-number recorder (paused in B16).

use crate::env::Env;
use crate::fights::{CONFIRM_GAP, EARLY_GAP_S, EARLY_S, FIGHT_INTERVAL, Fight, LEVEL_CONFIRM, LEVEL_FIX_READS, LEVEL_SEEN_S, LEVEL_VOTE_S, LOW_CONFIRM_S, TOP_BAR};
use alogs_gamelog::{Event, scene_of};
use alogs_pil::Image;
use alogs_protocol::devflags::DevFlags;
use alogs_protocol::plugins::{Health, Manifest, SwitchWatch};
use alogs_pyval::{Value, dict};
use alogs_reader::bar::{self, Bar, Spot};
use alogs_reader::boss::{self, GRADE_BASE, Row};
use alogs_reader::ocr::Reader;
use alogs_reader::{parse, prep, xn};
use indexmap::IndexMap;
use std::sync::mpsc::Receiver;

pub const BAR_GONE_GRACE: f64 = 20.0;
pub const BAR_GONE_S: f64 = 3.0;
pub const TOP_PART: f64 = 0.42;
pub const HOMELAND_SCENE: i64 = 550;

/// The meter's command line (bossbar.py's arguments) and settings.
pub struct Args {
    pub cfg: Value,
    pub duration: f64,
    pub level: Option<i64>,
    pub boss_id: Option<String>,
    pub replay: bool,
    pub no_submit: bool,
    pub no_debug_frames: bool,
    pub no_overlay: bool,
    pub no_auto_click: bool,
    pub player_log: std::path::PathBuf,
}

/// Where the meter's output goes (the overlay and the console): `out(kind, data)`.
pub trait Out {
    fn out(&mut self, kind: &str, data: Value);
}

/// Where frames come from: the app's shared capture live, saved pictures in a replay, scripted ones in tests.
pub trait Frames {
    /// The game's picture (its top `top` share, or all of it), or None.
    fn grab(&mut self, hwnd: usize, top: Option<f64>) -> Option<Image>;
    /// The game window, if there is one.
    fn window(&mut self) -> Option<usize>;
    /// Where the game's picture starts on the screen (the window's client area, screen pixels), if known.
    fn origin(&mut self, _hwnd: usize) -> Option<(i32, i32)> {
        None
    }
    /// When the game showed the frame just read, if known.
    fn frame_time(&self) -> Option<f64> {
        None
    }
    /// The player switched screen capture off.
    fn switched_off(&mut self) -> bool {
        false
    }
    fn is_foreground(&self, _hwnd: usize) -> bool {
        true
    }
    /// The last picture was a copy of the screen (our overlays and anything on top may be in it).
    fn screen_copy(&self) -> bool {
        false
    }
    /// Ask every aLogs program to keep its overlays out of screen captures until then (unix ms).
    fn set_clean_until(&mut self, _ms: u64) {}
    /// Only these parts (shares of the client, 0-1) of the game's picture, pasted in place on a black frame, or None.
    fn grab_parts(&mut self, _hwnd: usize, _rects: &[(f64, f64, f64, f64)]) -> Option<Image> {
        None
    }
    /// Ask the app for pictures of the game window alone until then (unix ms). Windows 10: by the window copy, which
    /// draws no border.
    fn set_game_only_until(&mut self, _ms: u64) {}
    /// A script has the screen until then (unix ms): those pictures are wanted quickly (Windows 10: by window
    /// capture, with its border, not the slower window copy).
    fn set_game_fast_until(&mut self, _ms: u64) {}
    /// The other aLogs programs' windows on screen (Sanctum Helper's, the app's button), to cover in a screen copy.
    fn other_overlays(&mut self) -> Vec<(i32, i32, i32, i32)> {
        vec![]
    }
    /// Your HP bar's strip (`area` as shares of the client), from a capture reader of its own; None without one.
    fn hp_strip(&mut self, _hwnd: usize, _area: (f64, f64, f64, f64)) -> Option<Image> {
        None
    }
    /// Keep the app listening for the boss kill sound until `now` + 3 s.
    fn audio_want(&mut self, _now: f64) {}
    /// The app's newest audio cue: (its number, when it started (unix s), how well it matched), or None.
    fn audio_cue(&self) -> Option<(u32, f64, f32)> {
        None
    }
}

pub struct Meter {
    pub env: Env,
    pub args: Args,
    pub manifest: Manifest,
    pub health: Health,
    pub switches: SwitchWatch,
    pub dev: DevFlags,
    pub catalog: Vec<Row>,
    pub grades: IndexMap<String, Vec<f64>>,
    pub xn_tpl: xn::Templates,
    pub xn_cache: xn::CounterCache,
    pub ocr: Box<dyn Reader>,
    /// Live: the boss bar's text (title, timer, xN) read on a helper of its own, so a look never waits for it;
    /// the meta says whether the xN was missing and whether a title was asked for.
    pub ocr_bg: Option<crate::ocr_worker::OcrWorker<(bool, bool, Option<f64>)>>,
    /// Live: the screens' text read on a helper of its own, with the frame and time it was read from.
    pub screen_bg: Option<crate::ocr_worker::OcrWorker<(Image, f64)>>,
    pub fight: Option<Fight>,
    pub hwnd: Option<usize>,
    pub frame_size: (usize, usize),
    pub bar: Option<Bar>,
    pub bar_seen: f64,
    pub bar_spots: Vec<Spot>,
    /// The fight's bar ends, held once seen the same on a few looks (bar.rs `EndsHold`).
    pub ends: bar::EndsHold,
    pub last_text: f64,
    pub last_title: f64,
    pub switch_votes: Vec<(String, i64)>,
    /// Title reads before a fight starts: (boss, level, when); a fight starts on agreeing reads (`level_needs`).
    pub level_votes: Vec<(String, i64, f64)>,
    /// The level a fight of each boss was last settled on, in its space: (space, boss) -> (level, when).
    pub levels_seen: std::collections::HashMap<(Option<String>, String), (i64, f64)>,
    /// "fight level: unread" said for the bar showing now (once per bar).
    pub level_unread_said: bool,
    /// The level read from the title against the game's own font on the look the title's text crop was taken from
    /// (None: not read), and when.
    pub title_font: Option<(Option<i64>, f64)>,
    /// A contradiction between the two readers or with the fight said for this fight already.
    pub level_said: Option<(i64, i64)>,
    pub space: Option<String>,
    pub bossrush_space: bool,
    pub holosim_space: bool,
    pub rush_spaces: std::collections::BTreeSet<String>,
    pub sim_spaces: std::collections::BTreeSet<String>,
    pub duds: Vec<Dud>,
    pub dud_cand: Option<Dud>,
    pub hidden_drop: Option<(f64, i64, f64)>,
    /// What a bar with no fight yet has read: (frame time, fill, fill colour, edge colour) (keep_early).
    pub early: Vec<(f64, f64, [u8; 3], [u8; 3])>,
    /// The span and height of the bar those readings are of.
    pub early_geom: Option<(f64, i64)>,
    /// The overlay was told of a bar with no fight yet (emit_bar).
    pub bar_shown: bool,
    pub events: Option<Receiver<Event>>,
    pub catch_seen: f64,
    pub last_fight: Option<(Fight, f64)>,
    pub fight_ended_at: f64,
    /// Interlink results recorded (ps.results) when the last fight ended: a higher count since means its result was read
    pub results_at_fight_end: u64,
    pub awaiting: Option<f64>,
    pub after_fight: Option<(String, f64, f64)>,
    pub last_screen: Option<(i32, i32, usize, usize)>,
    pub origin: (i32, i32),
    pub uid: Option<String>,
    pub rush_level: Option<i64>,
    pub odd_level: Option<((String, i64), f64, f64)>,
    pub prep_boss: Option<(f64, Row)>,
    pub party: Option<Value>,
    pub not_bar_since: Option<f64>,
    pub quiet: bool,
    pub capture_off: bool,
    pub homeland_rest: bool,
    pub band_frame: bool,
    /// The last frame is whole (or holds every box a screen read needs); false for a top-part-only look.
    pub frame_full: bool,
    /// The last frame holds only a screen read's boxes (the rest black).
    pub parts_frame: bool,
    /// The top part's signature, every look.
    pub top_sig: Option<Vec<u8>>,
    pub pacing: crate::pacing::Pacing,
    pub brk_at: f64,
    pub last_step: f64,
    pub status_text: String,
    pub kill_sound_used: Option<u32>,
    pub frames: u64,
    /// The end-screen readers' state.
    pub rs: crate::results::ResultsState,
    pub ss: crate::screens::ScreensState,
    pub auto: crate::leaderboards::AutoState,
    pub sim: crate::sim::SimState,
    /// The Holo-Battle Sim run's levels, kept for its win (sim_stages.rs).
    pub stages: crate::sim_stages::Stages,
    pub egg: crate::egg::EggState,
    /// The end-of-fight watch and the hold from its fade (fight_end.rs).
    pub fight_end: crate::fight_end::FightEndState,
    /// An Interlink fight's pictures for its evidence, picked as it goes (fight_shots.rs).
    pub shots: crate::fight_shots::ShotPicker,
    /// The last such fight's evidence once it was over, for its summary.
    pub fight_evidence: Option<crate::fight_shots::FightEvidence>,
    /// Its pictures and log being written (tests wait for it).
    pub fight_evidence_job: Option<std::thread::JoinHandle<()>>,
    /// "I was in this run" for a fight run to its end whose result screen was never read (seat_note.rs).
    pub seat_note: crate::seat_note::SeatNoteState,
    pub ps: crate::party::PartyState,
    pub menu: crate::menu::MenuState,
    pub icon: crate::party_icons::IconState,
    pub diag: crate::diag::DiagState,
    pub hit: crate::hits::HitState,
    pub bars: crate::diag::BarDiag,
    pub ocr_check: crate::ocrcheck::OcrCheck,
    pub presence: Option<alogs_capture::presence::Presence>,
    /// Holding the player's keys and mouse buttons (Automation › hold inputs).
    pub hold: alogs_capture::input::InputBlock,
    /// The controller kept away while aLogs has control of the Sim's end or an Egg Heist end screen: the game told it
    /// is inactive, in front.
    pub told_inactive: alogs_capture::inactive::Inactive,
    /// The clientRunId inputs are held for until it's sent.
    pub send_waiting: Option<String>,
    /// The copy of that record (its payload as queued) the hold waits for: an older copy of the same run being sent
    /// doesn't end it. None: any copy of it does.
    pub send_waiting_copy: Option<Value>,
    /// What the leaderboard notice said for the last runs sent (clientRunId, what it announced), newest last, at most
    /// NOTICED_RUNS: a run's update only gets a notice when its standing got better since (Meter::sent_notice).
    pub noticed_runs: std::collections::VecDeque<(String, crate::payloads::Announced)>,
    pub presence_sent: Option<(String, &'static str, f64)>,
    /// When "You're being attacked" was last raised (the overlay shows it for 5 s).
    pub hit_at: f64,
    /// Queued records (their payload as queued, as json) whose arrival shows no notice: the notice of the run they
    /// belong to went up already (an Egg Heist rank after its match: one notice per run). At most NOTICED_RUNS.
    pub quiet_sends: std::collections::VecDeque<String>,
    /// The overlay's windows, to cover in a picture of the screen.
    pub own: Option<std::sync::Arc<std::sync::Mutex<crate::overlay::Own>>>,
    /// The app's newest audio cue (Frames::audio_cue), as of the last look.
    pub audio_cue: Option<(u32, f64, f32)>,
    /// A second OCR reader for the HP number (hit_watch), set by the binary.
    pub hp_ocr: Option<Box<dyn Reader>>,
    /// The game's head portraits, cards and Sim pictures (icons.py), once loaded.
    pub icons: Option<alogs_reader::icons::Icons>,
    /// None, 'loading', 'ready', 'failed'.
    pub icons_state: Option<String>,
    icons_job: Option<std::sync::mpsc::Receiver<Option<alogs_reader::icons::Icons>>>,
    /// The game's build as last read from Player.log's head (game_build), to log it once per change.
    pub game_build: Option<alogs_gamelog::GameBuild>,
    pub submitter: Option<std::sync::Arc<crate::submit::Submitter>>,
    /// Your in-game name (only ever from the game).
    pub character_name: Option<String>,
    pub font_reader: Option<alogs_reader::names::NameReader>,
    pub font_tried: bool,
    pub team: crate::team::Teammates,
    /// The Party screen this run started from, kept as proof.
    pub party_picture: Option<Image>,
    /// The Interlink bots' names (the game's own, else results_parse::INTERLINK_NPCS), read again with the schedule.
    pub bot_names: Vec<String>,
}

/// A bar-like shape that never named a boss (`dud_check`).
#[derive(Clone, Debug)]
pub struct Dud {
    pub span: f64,
    pub yc: i64,
    pub fill: Option<f64>,
    pub t0: f64,
    pub seen: f64,
}

fn same_spot_bars(span_a: f64, yc_a: f64, span_b: f64, yc_b: f64) -> bool {
    (span_a - span_b).abs() <= 3.0 && (yc_a - yc_b).abs() <= 3.0
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

fn get<'a>(r: &'a Row, k: &str) -> &'a str {
    r.get(k).map_or("", String::as_str)
}

impl Meter {
    pub fn new(env: Env, args: Args, ocr: Box<dyn Reader>, events: Option<Receiver<Event>>) -> Meter {
        let catalog = boss::load_catalog(&env.res, None).unwrap_or_default();
        let grades = boss::load_grades(&env.res, None).unwrap_or_default();
        let xn_tpl = xn::load_templates(&env.res);
        let bar_spots = load_spots(&env.data.join("bar_spot.json"));
        let rush_level = last_rush_level(&env.data);
        Meter {
            manifest: Manifest::load(&[&env.res]).unwrap_or_else(Manifest::bundled),
            health: Health::new(),
            switches: SwitchWatch::default(),
            dev: DevFlags::from_env(),
            catalog,
            grades,
            xn_tpl,
            xn_cache: xn::CounterCache::default(),
            ocr,
            ocr_bg: None,
            screen_bg: None,
            fight: None,
            hwnd: None,
            frame_size: (0, 0),
            bar: None,
            bar_seen: 0.0,
            bar_spots,
            ends: Default::default(),
            last_text: 0.0,
            last_title: 0.0,
            switch_votes: vec![],
            level_votes: vec![],
            levels_seen: Default::default(),
            level_unread_said: false,
            title_font: None,
            level_said: None,
            space: None,
            bossrush_space: false,
            holosim_space: false,
            rush_spaces: Default::default(),
            sim_spaces: Default::default(),
            duds: vec![],
            dud_cand: None,
            hidden_drop: None,
            early: vec![],
            early_geom: None,
            bar_shown: false,
            events,
            catch_seen: 0.0,
            last_fight: None,
            fight_ended_at: 0.0,
            results_at_fight_end: 0,
            awaiting: None,
            after_fight: None,
            last_screen: None,
            origin: (0, 0),
            uid: None,
            rush_level,
            odd_level: None,
            prep_boss: None,
            party: None,
            not_bar_since: None,
            quiet: false,
            capture_off: false,
            homeland_rest: false,
            band_frame: false,
            frame_full: true,
            parts_frame: false,
            top_sig: None,
            pacing: Default::default(),
            brk_at: 0.0,
            last_step: 0.0,
            status_text: "starting".into(),
            kill_sound_used: None,
            frames: 0,
            rs: Default::default(),
            ss: Default::default(),
            auto: Default::default(),
            sim: Default::default(),
            stages: Default::default(),
            egg: Default::default(),
            fight_end: Default::default(),
            shots: Default::default(),
            fight_evidence: None,
            fight_evidence_job: None,
            seat_note: Default::default(),
            ps: Default::default(),
            menu: Default::default(),
            icon: Default::default(),
            diag: Default::default(),
            hit: Default::default(),
            bars: Default::default(),
            ocr_check: Default::default(),
            presence: None,
            hold: Default::default(),
            told_inactive: Default::default(),
            send_waiting: None,
            send_waiting_copy: None,
            noticed_runs: Default::default(),
            presence_sent: None,
            hit_at: 0.0,
            quiet_sends: Default::default(),
            own: None,
            audio_cue: None,
            hp_ocr: None,
            icons: None,
            icons_state: None,
            icons_job: None,
            game_build: None,
            submitter: None,
            character_name: None,
            font_reader: None,
            font_tried: false,
            team: crate::team::Teammates::load(env.data.join("teammates.json")),
            party_picture: None,
            bot_names: crate::results_parse::load_bot_names(&env.data),
            env,
            args,
        }
    }

    fn now(&self) -> f64 {
        self.env.now()
    }

    fn status(&mut self, out: &mut dyn Out, text: String) {
        self.status_text = text.clone();
        out.out("status", Value::Str(text));
    }

    /// Seconds between frames in a fight (30 a second, 60 only when picked).
    pub fn fight_interval(&self) -> f64 {
        let cfg = &self.args.cfg;
        let rate = cfg.get("capture").get("fight_rate").as_f64().unwrap_or(30.0);
        let chosen = cfg.get("perf").get("on").truthy()
            || cfg.get("user_set").as_list().is_some_and(|l| l.iter().any(|v| v.as_str() == Some("capture.fight_rate")));
        if rate == 60.0 && chosen { 1.0 / 60.0 } else { FIGHT_INTERVAL }
    }

    /// Seconds between reads of the name above the bar: 8, and 2 while a two-boss fight's partner can show up.
    pub fn title_gap(&self) -> f64 {
        // (a fight whose level isn't read again yet, or is being read as another: its name about once a second)
        if self.fight.as_ref().is_some_and(|f| f.level_reads < LEVEL_FIX_READS as u32) || !self.switch_votes.is_empty() {
            return 1.0;
        }
        if let Some(f) = &self.fight
            && let Some(p) = &f.partner
            && (f.partner_due() || self.switch_votes.last().is_some_and(|v| v.0 == get(p, "puppetId")))
        {
            return 2.0;
        }
        8.0
    }

    /// Time to read the bar's text: about once a second, the name every 8 s.
    pub fn text_due(&self, now: f64) -> bool {
        let need_title = self.fight.is_none() || now - self.last_title > self.title_gap();
        // (and only while the helper is free, as in the Python: a job handed to a busy helper is dropped, but its
        // crops were made, 25 to 30 ms a look at 4K, and the read was stamped as made)
        (need_title || now - self.last_text >= 1.0) && !self.ocr_bg.as_ref().is_some_and(|w| w.busy())
    }

    /// A boss bar spans at least ~28% of the 16:9 width; during a fight only the fight's own bar counts.
    pub fn plausible_bar(&mut self, found: &Bar, im: &Image) -> bool {
        let (w, h) = (im.width as f64, im.height as f64);
        let u = w.min(h * 16.0 / 9.0);
        if found.span < 0.2 * u {
            return false;
        }
        if let Some(f) = &mut self.fight
            && let Some((span, yc)) = f.bar_geom
            && ((found.yc as f64 - yc).abs() > 6f64.max(h * 0.004) || (found.span - span).abs() > span * 0.12)
        {
            if !f.foreign_logged {
                f.foreign_logged = true;
                log::info!(target: "meter", "ignored a different bar-like shape during the fight (span {:.0}px, y {:.0})", found.span, found.yc);
            }
            return false;
        }
        true
    }

    /// A bar-like shape that named nothing for 10 s with a fill that never moved is set aside.
    pub fn dud_check(&mut self, b: &Bar, fill: Option<f64>, xn: Option<i64>, now: f64) -> bool {
        if self.fight.is_some() || xn.is_some() {
            self.dud_cand = None;
            return false;
        }
        let renew = match &self.dud_cand {
            None => true,
            Some(c) => {
                !same_spot_bars(c.span, c.yc as f64, b.span, b.yc as f64)
                    || now - c.seen > 5.0
                    || matches!((fill, c.fill), (Some(f), Some(cf)) if (f - cf).abs() > 0.01)
            }
        };
        if renew {
            self.dud_cand = Some(Dud { span: b.span, yc: b.yc, fill, t0: now, seen: now });
            return false;
        }
        let c = self.dud_cand.as_mut().unwrap();
        c.seen = now;
        if now - c.t0 < 10.0 {
            return false;
        }
        let c = self.dud_cand.take().unwrap();
        self.duds.push(c);
        if self.duds.len() > 8 {
            self.duds.remove(0);
        }
        self.bar_spots.retain(|s| !same_spot_bars(s.span, s.yc, b.span, b.yc as f64));
        log::info!(target: "meter", "ignoring a bar-like shape (span {:.0}px, y {:.0}): no boss name in 10 s and its fill never moved", b.span, b.yc);
        true
    }

    pub fn is_dud(&mut self, im: &Image, found: &Bar) -> bool {
        for i in 0..self.duds.len() {
            let d = &self.duds[i];
            if !same_spot_bars(d.span, d.yc as f64, found.span, found.yc as f64) {
                continue;
            }
            let fill = bar::measure_fill_any(im, found, 70).0;
            if d.fill.is_some_and(|df| (fill - df).abs() > 0.01) {
                self.duds.remove(i);
                log::info!(target: "meter", "the ignored shape at y {:.0} changed - looking at it again", found.yc);
                return false;
            }
            return true;
        }
        false
    }

    /// The fight's numbers for the overlay.
    pub fn emit_fight(&mut self, out: &mut dyn Out, b: &Bar) {
        let now = self.now();
        self.last_screen = Some((self.origin.0, self.origin.1, self.frame_size.0, self.frame_size.1));
        let Some(f) = &self.fight else { return };
        let si = f.seg_info.as_ref();
        let opt = |v: Option<i64>| v.map_or(Value::None, Value::Int);
        let bi = f.brk.info(now);
        let fo = |v: Option<f64>| v.map_or(Value::None, Value::Float);
        let data = dict([
            ("title", Value::Str(f.title.clone())),
            ("level", Value::Int(f.level)),
            ("max_hp", Value::Int(f.max_hp)),
            ("boss_hp", Value::Int(f.boss_hp)),
            ("xn", opt(f.seg)),
            ("fill", Value::Float(f.seg_fill)),
            ("damage", Value::Int(f.damage)),
            ("elapsed", Value::Float(f.elapsed)),
            ("avg", Value::Float(f.avg)),
            ("recent", Value::Float(f.recent)),
            ("boss", f.boss_of().map_or(Value::None, |(a, b)| Value::List(vec![Value::Int(a), Value::Int(b)]))),
            ("bossrush", Value::Bool(f.bossrush)),
            ("holosim", Value::Bool(self.holosim_space)),
            ("seg_size", opt(si.map(|s| s.size_hp))),
            ("seg_left", opt(si.map(|s| s.left_hp))),
            (
                "bar",
                Value::List(vec![
                    Value::Float(f64::from(self.origin.0) + b.left_bracket),
                    Value::Float(f64::from(self.origin.1) + b.yc as f64),
                    Value::Float(bar::label_span(b.left_bracket, b.span, self.frame_size.0 as f64)),
                ]),
            ),
            (
                "break",
                dict([
                    ("left", fo(bi.left)),
                    ("phase", Value::Str(bi.phase.into())),
                    ("kind", bi.kind.map_or(Value::None, |k| Value::Str(k.into()))),
                    ("rate", fo(bi.rate)),
                    ("rate_now", fo(bi.rate_now)),
                    ("breaks", Value::Int(bi.breaks)),
                    ("last_after", fo(bi.last_after)),
                    ("size", fo(bi.size)),
                ]),
            ),
            ("screen", screen_value(self.last_screen)),
        ]);
        out.out("fight", data);
    }

    /// The boss a run on this boss's stage starts with (a two-boss fight's second boss gives its stage's first).
    pub fn stage_first(&self, row: Option<Row>) -> Option<Row> {
        let row = row?;
        let (_, order, first) = self.env.schedule.stage_of(Some(get(&row, "puppetId")), None, self.now());
        if order != Some(2) {
            return Some(row);
        }
        let first = first.unwrap_or_default();
        Some(
            self.catalog
                .iter()
                .find(|r| get(r, "table") == "puppet_data" && get(r, "puppetId") == first && !get(r, "bossTitle_en").is_empty() && !get(r, "fixed_hp_rate").is_empty())
                .cloned()
                .unwrap_or(row),
        )
    }

    /// Match the boss on the bar from its title read.
    pub fn identify(&mut self, out: &mut dyn Out, title_text: Option<&str>, xn: Option<i64>, red_text: Option<&str>) {
        let (lv, title) = parse::parse_title_pair(title_text, red_text);
        let now = self.now();
        // the level: the game's own digits read from the title (the first opinion) and the text reader's (the second).
        // Both agreeing settle it at once; they disagreeing leave it unread (read again); one alone is voted
        let font = self.title_font.take().filter(|f| now - f.1 <= 2.0).and_then(|f| f.0);
        let (level, read_by, both) = match (self.args.level, font, lv) {
            (Some(l), ..) => (Some(l), "given on the command line", true),
            (None, Some(a), Some(b)) if a == b => (Some(a), "font digits and text reader agree", true),
            (None, Some(a), Some(b)) => {
                if self.level_said != Some((a, b)) {
                    self.level_said = Some((a, b));
                    log::info!(target: "meter", "fight level: font digits read {a}, text reader {b} - read again");
                }
                (None, "", false)
            }
            (None, Some(a), None) => (Some(a), "font digits", false),
            (None, None, Some(b)) => (Some(b), "text reader", false),
            _ => (None, "", false),
        };
        let in_world = self.space.is_some() && scene_of(self.space.as_deref()).is_some();
        let bossrush = !in_world && (xn.is_some() || self.bossrush_space || self.fight.as_ref().is_some_and(|f| f.bossrush));
        let row = if !title.is_empty() && level.is_some() {
            boss::pick_boss(&self.catalog, &title, bossrush, self.args.boss_id.as_deref(), &self.env.schedule, now).0
        } else {
            None
        };
        let Some(row) = row else {
            if self.fight.is_none() && !title.is_empty() && self.bar.is_some() {
                let lvs = level.map_or("None".to_string(), |l| l.to_string());
                if level.is_none() && !self.level_unread_said {
                    self.level_unread_said = true;
                    log::info!(target: "meter", "fight level: unread (running on the bar alone; title read {})", alogs_pyval::str_repr(title_text.unwrap_or("")));
                }
                self.status(out, format!("reading boss name... (\"{title}\" Lv {lvs})"));
            }
            return;
        };
        let level = level.unwrap();
        let pid = get(&row, "puppetId").to_string();
        if let Some(f) = &mut self.fight {
            if pid == get(&f.row, "puppetId") && level == f.level {
                self.switch_votes.clear();
                f.level_reads += 1;
                return;
            }
            // the same boss read at another level: on LEVEL_FIX_READS agreeing reads (one read the two readers agree
            // on counts for all of them) its level was wrong, also a level carried into an Interlink fight: each
            // boss's HP and the damage are put right (Fight::correct_level)
            if pid == get(&f.row, "puppetId") && self.args.level.is_none() {
                let votes = if both { LEVEL_FIX_READS } else { 1 };
                for _ in 0..votes {
                    self.switch_votes.push((pid.clone(), level));
                }
                while self.switch_votes.len() > LEVEL_FIX_READS {
                    self.switch_votes.remove(0);
                }
                if self.switch_votes.len() < LEVEL_FIX_READS || self.switch_votes.iter().any(|v| *v != self.switch_votes[0]) {
                    return;
                }
                self.switch_votes.clear();
                let cal = boss::load_calibration(&self.env.data);
                let hps: Vec<i64> = f.phases.iter().map(|p| boss::boss_max_hp(&p.row, level, Some(&cal)).unwrap_or(0)).collect();
                if hps.iter().any(|h| *h <= 0) {
                    log::info!(target: "meter", "fight level: the title reads Lv {level}, the fight stays at Lv {} (no HP known for Lv {level})", f.level);
                    return;
                }
                let size = boss::boss_break_points(&self.env.res, &row, level as f64);
                log::info!(target: "meter", "fight level corrected: Lv {} -> Lv {level}, max HP {} -> {} ({read_by}{})", f.level, thousands(f.max_hp),
                    thousands(hps.iter().sum::<i64>()), if f.phases.len() > 1 { ", both bosses" } else { "" });
                f.correct_level(level, &hps, size);
                self.levels_seen.insert((self.space.clone(), pid), (level, now));
                if f.bossrush {
                    self.rush_level = Some(level);
                }
                return;
            }
            if f.partner.as_ref().is_some_and(|p| get(p, "puppetId") == pid) {
                if level != f.level {
                    return;
                }
                self.switch_votes.push((pid.clone(), level));
                if self.switch_votes.len() > 3 {
                    self.switch_votes.remove(0);
                }
                let need = if f.partner_due() { 1 } else { 2 };
                let n = self.switch_votes.len();
                if n < need || !self.switch_votes[n - need..].iter().all(|v| *v == (pid.clone(), level)) {
                    return;
                }
                let why = format!("its name read {}", alogs_pyval::str_repr(title_text.unwrap_or("")));
                f.next_boss(&self.env, row, &why);
                self.boss_handed_over(out, &format!("title read {}, xN {}", alogs_pyval::str_repr(title_text.unwrap_or("")), xn.map_or("None".into(), |x| x.to_string())));
                return;
            }
            if f.phases[..f.phases.len() - 1].iter().any(|p| get(&p.row, "puppetId") == pid) {
                return;
            }
            self.switch_votes.push((pid.clone(), level));
            if self.switch_votes.len() > 3 {
                self.switch_votes.remove(0);
            }
            if self.switch_votes.len() < 3 || self.switch_votes.iter().any(|v| *v != self.switch_votes[0]) {
                return;
            }
            self.end_fight(out, "different boss");
        } else if bossrush && self.bossrush_space && self.args.level.is_none() && self.rush_level.is_some_and(|r| r != level) {
            let odd = (pid.clone(), level);
            if self.odd_level.as_ref().is_none_or(|o| o.0 != odd || now - o.2 > 2.0) {
                self.odd_level = Some((odd, now, now));
            }
            let o = self.odd_level.as_mut().unwrap();
            o.2 = now;
            if now - o.1 < LEVEL_CONFIRM {
                let rl = self.rush_level.unwrap();
                self.status(out, format!("reading boss level... (\"{title}\" Lv {level}, Interlink bosses are Lv {rl})"));
                return;
            }
        }
        self.odd_level = None;
        // the level is voted: a fight starts on agreeing reads, and runs on its bar alone until then (replay_early)
        let mut source = read_by.to_string();
        if self.fight.is_none() && self.args.level.is_none() {
            let need = if both { 1 } else { self.level_needs(&pid, level, bossrush, now) };
            source = match need {
                _ if both => read_by.to_string(),
                1 if bossrush => format!("{read_by}, the Interlink run's level"),
                1 => format!("{read_by}, as the last fight of this boss here"),
                n => format!("{read_by}, {n} reads agree"),
            };
            self.level_votes.retain(|v| now - v.2 <= LEVEL_VOTE_S);
            self.level_votes.push((pid.clone(), level, now));
            let n = self.level_votes.len();
            if n < need || !self.level_votes[n - need..].iter().all(|v| v.0 == pid && v.1 == level) {
                self.status(out, format!("reading boss level... (\"{title}\" Lv {level})"));
                return;
            }
            self.level_votes.clear();
        }
        let how = format!("title read {}, xN {}", alogs_pyval::str_repr(title_text.unwrap_or("")), xn.map_or("None".into(), |x| x.to_string()));
        self.levels_seen.insert((self.space.clone(), pid), (level, now));
        log::info!(target: "meter", "fight level: {level} ({source})");
        self.start_fight(out, row, level, &title, bossrush, &how);
        if bossrush {
            self.rush_level = Some(level);
        }
    }

    /// How many agreeing title reads start a fight at this level: one when it is the level this boss was settled on
    /// here within LEVEL_SEEN_S (or an Interlink run's level), three when it is another one (a single read of 6 after
    /// fights at 65 is a misread until it is read again), else two.
    fn level_needs(&self, pid: &str, level: i64, bossrush: bool, now: f64) -> usize {
        if bossrush && self.rush_level == Some(level) {
            return 1;
        }
        match self.levels_seen.get(&(self.space.clone(), pid.to_string())) {
            Some(&(l, at)) if now - at <= LEVEL_SEEN_S => {
                if l == level {
                    1
                } else {
                    3
                }
            }
            _ => 2,
        }
    }

    pub fn boss_handed_over(&mut self, out: &mut dyn Out, how: &str) {
        self.switch_votes.clear();
        let Some(f) = &self.fight else { return };
        let msg = format!("next boss: {} Lv {}, max HP {} (boss {} of 2, Interlink)", f.title, f.level, thousands(f.boss_hp), f.phases.len());
        log::info!(target: "meter", "fight details: puppetId {}, boss {} of 2, {how}, HP bars {}", get(&f.row, "puppetId"), f.phases.len(), f.column);
        self.status(out, msg);
    }

    pub fn start_fight(&mut self, out: &mut dyn Out, row: Row, level: i64, title: &str, bossrush: bool, how: &str) {
        let cal = boss::load_calibration(&self.env.data);
        let hp = boss::boss_max_hp(&row, level, Some(&cal)).unwrap_or(0);
        let now = self.now();
        self.ps.runs += 1; // the Party screen's votes are this lobby's: a fight ends them (Meter::lobby_change)
        self.level_unread_said = false;
        self.level_said = None;
        let (_, order, partner_id) = if bossrush { self.env.schedule.stage_of(Some(get(&row, "puppetId")), None, now) } else { (None, None, None) };
        let partner = if order == Some(1) {
            self.catalog
                .iter()
                .find(|r| get(r, "table") == "puppet_data" && Some(get(r, "puppetId")) == partner_id.as_deref() && !get(r, "fixed_hp_rate").is_empty())
                .cloned()
        } else {
            None
        };
        let t = get(&row, "bossTitle_en");
        let name = if t.is_empty() { title.to_string() } else { t.to_string() };
        let mut f = Fight::new(&self.env, &name, level, row.clone(), hp, bossrush, !self.args.no_debug_frames, partner.clone(), order);
        f.scene_ok = bossrush && self.bossrush_space;
        if let Some(b) = self.bar.clone() {
            // (its place is kept once its ends are held: see ends_look)
            f.bar_geom = Some((b.span, b.yc as f64));
        }
        f.low_confirm = LOW_CONFIRM_S;
        self.replay_early(&mut f);
        let msg = format!("fight started: {} Lv {level}, max HP {}{}", f.title, thousands(hp), if bossrush { " (Interlink)" } else { "" });
        log::info!(target: "meter", "fight details: puppetId {}, {how}, bossrush space {}{}{}", get(&row, "puppetId"),
            if self.bossrush_space { "True" } else { "False" },
            if f.column != GRADE_BASE { format!(", HP bars {}", f.column) } else { String::new() },
            partner.as_ref().map_or(String::new(), |p| format!(", two-boss fight: {} ({}) comes next", get(p, "bossTitle_en"), partner_id.clone().unwrap_or_default())));
        self.fight = Some(f);
        // an Interlink fight followed live with submissions on: its evidence pictures are picked as it goes
        let submits = self.submitter.as_ref().is_some_and(|s| s.config.lock().unwrap().is_some());
        self.shots.start(bossrush && submits && !self.args.replay);
        self.seat_note_start(bossrush, submits);
        self.shots.limit_s = self.overtime_limit(now).map_or(if self.args.duration > 0.0 { self.args.duration } else { crate::fight_shots::DEFAULT_LIMIT_S }, |l| l as f64);
        self.status(out, msg);
    }

    /// The fight is over: its kept pictures are written (JPEG) and its log compressed, off the meter's loop; what its
    /// summary records of them is kept for the summary (fight_summary).
    pub fn fight_evidence_done(&mut self, f: &Fight) {
        if !self.shots.on {
            return;
        }
        let (forced, ms) = (self.shots.forced, self.shots.copy_ms);
        let shots = self.shots.finish();
        let ev = crate::fight_shots::evidence_of(&f.csv_name, &shots, f.rows);
        log::info!(target: "meter", "fight evidence: {} pictures kept ({forced} looks made top-part looks for them, copies {ms:.1} ms in all), {} log rows",
            shots.len(), f.rows);
        self.fight_evidence = Some(ev);
        let (data, name) = (self.env.data.clone(), f.csv_name.clone());
        let csv = data.join("logs").join(format!("{name}.csv"));
        self.fight_evidence_job = Some(std::thread::spawn(move || crate::fight_shots::write_evidence(&data, &name, shots, csv)));
    }

    /// A fight's bar ends are held (seen the same on a few looks, centred): their place is kept, and looked at first
    /// by the next search (`bar_at`). Kept at every find, three of the four places held on 17 Nov were of things that
    /// were no bar (85% of all finds are); kept at a fight's start, a first find 19 px short at the right end stayed
    /// for days and came back as the first find of the next fights. A held place replaces one at the same place (this
    /// frame size and row, either end within 20 px): the other bar kind's place (its ends 62 px further out) stays.
    fn remember_spot(&mut self, b: &Bar) {
        let spot = Spot { frame: b.frame, yc: b.yc as f64, thickness: b.thickness as f64, left_bracket: b.left_bracket, span: b.span, track_left: b.track_left as f64 };
        let same = |s: &Spot, near: f64| {
            s.frame == spot.frame
                && (s.yc - spot.yc).abs() <= 3.0
                && ((s.left_bracket - spot.left_bracket).abs() <= near || (s.left_bracket + s.span - spot.left_bracket - spot.span).abs() <= near)
        };
        if self.bar_spots.first().is_some_and(|s| same(s, 1.0) && (s.span - spot.span).abs() <= 1.0 && (s.thickness - spot.thickness).abs() <= 1.0) {
            return; // (already the first: nothing to write)
        }
        self.bar_spots.retain(|s| !same(s, 20.0));
        self.bar_spots.insert(0, spot);
        self.bar_spots.truncate(4);
        save_spots(&self.env.data.join("bar_spot.json"), &self.bar_spots);
    }

    /// Is a fight known to have started on a bar at this place (bar_spot.json holds only such places)?
    pub fn known_spot(&self, b: &Bar) -> bool {
        self.bar_spots.iter().any(|s| s.frame == b.frame && (s.yc - b.yc as f64).abs() <= 2.0 && (s.track_left - b.track_left as f64).abs() <= 2.0)
    }

    /// A boss bar is up and its fight hasn't started (its boss's name is still being read): the overlay is told at
    /// once, so the HP % stands on the bar from the bar's first look and not from the name's read (the owner, 17
    /// Nov: "I want the hp labels to appear right away"). The % is the bar's own fill; the HP numbers need the
    /// boss and come with the fight. Only at a place where a fight started before: 85% of what the finder takes
    /// for a bar is no bar (menu art, orange things in the world), and a label must not flash on those. Not for
    /// Interlink: its fights start with their bar, and its % is of all the bar's layers, not of the one showing.
    pub fn emit_bar(&mut self, out: &mut dyn Out, b: &Bar, fill: f64, fill_rgb: [u8; 3]) {
        let live = fill_rgb[0] >= 225 && (80..=140).contains(&fill_rgb[1]) && (75..=135).contains(&fill_rgb[2]);
        if !live || !b.world || self.bossrush_space || self.args.replay || !self.known_spot(b) {
            return;
        }
        self.last_screen = Some((self.origin.0, self.origin.1, self.frame_size.0, self.frame_size.1));
        out.out("bar", dict([
            ("fill", Value::Float(fill)),
            ("bar", Value::List(vec![
                Value::Float(f64::from(self.origin.0) + b.left_bracket),
                Value::Float(f64::from(self.origin.1) + b.yc as f64),
                Value::Float(bar::label_span(b.left_bracket, b.span, self.frame_size.0 as f64)),
            ])),
            ("screen", screen_value(self.last_screen)),
        ]));
        self.bar_shown = true;
    }

    /// The bar the overlay was told of is gone and no fight came of it: the overlay takes its label away.
    pub fn bar_gone_check(&mut self, out: &mut dyn Out) {
        if !self.bar_shown || (self.bar.is_some() && self.fight.is_none()) {
            return;
        }
        self.bar_shown = false;
        if self.fight.is_none() {
            out.out("bar_gone", Value::None);
        }
    }

    /// A bar with no fight yet (its boss's name is still being read): what it reads is kept, EARLY_S at most, for
    /// the fight that starts on it. Another bar (a different place or size), or EARLY_GAP_S without this one,
    /// starts the keeping over.
    pub fn keep_early(&mut self, b: &Bar, at: f64, fill: f64, fill_rgb: [u8; 3], edge_rgb: [u8; 3]) {
        let same = self.early_geom.is_some_and(|(span, yc)| (span - b.span).abs() <= span * 0.03 && (yc - b.yc).abs() <= 6)
            && self.early.last().is_some_and(|l| (0.0..=EARLY_GAP_S).contains(&(at - l.0)));
        if !same {
            self.early.clear();
        }
        self.early_geom = Some((b.span, b.yc));
        self.early.push((at, fill, fill_rgb, edge_rgb));
        let from = self.early.iter().position(|r| at - r.0 <= EARLY_S).unwrap_or(0);
        self.early.drain(..from);
    }

    /// The fight starts at its bar's first sight, not when its boss's name is read: what the bar read before that is
    /// put through the fight as if it had been there. Thrown away, a hit landed before the name was read cost the
    /// fight its real start: the damage was all counted (it is read off the bar) but the clock began at the first
    /// hit seen after, so the DPS came out too high (17 Nov: a bar first seen at 55% gave 187,157 damage in 9 s).
    /// Not for Interlink, whose fights start with their bar and take their time from the game's timer.
    pub fn replay_early(&mut self, f: &mut Fight) {
        let early = std::mem::take(&mut self.early);
        self.early_geom = None;
        let Some(first) = early.first().map(|r| r.0) else { return };
        if f.bossrush || self.env.now() - first > EARLY_S + EARLY_GAP_S {
            return;
        }
        (f.created, f.t0, f.last_at) = (first, first, first);
        let grades = self.grades.clone();
        let n = early.len();
        for (at, fill, fill_rgb, edge_rgb) in early {
            f.update(&self.env, Some(fill), fill_rgb, edge_rgb, None, None, &grades, self.args.duration, Some(at), true);
        }
        log::info!(target: "meter", "fight: counted from its bar's first sight, {:.2} s before its name was read ({n} readings)", self.env.now() - first);
    }

    /// Interlink: the prep screen named the boss, so its fight starts on the first frame of its bar.
    pub fn instant_start(&mut self, out: &mut dyn Out, xn: Option<i64>, now: f64, low: bool) {
        let (Some(x), Some(rl)) = (xn, self.rush_level) else { return };
        if self.fight.is_some() || self.prep_boss.as_ref().is_none_or(|p| now - p.0 > 180.0) {
            return;
        }
        if low && x < boss::xn_top(&self.grades) - 1 {
            return;
        }
        let row = self.prep_boss.take().unwrap().1;
        let t = get(&row, "bossTitle_en").to_string();
        log::info!(target: "meter", "fight level: {rl} (carried from the last Interlink fight)");
        self.start_fight(out, row, rl, &t, true, &format!("from the prep screen, xN {x}"));
    }

    pub fn end_fight(&mut self, out: &mut dyn Out, why: &str) {
        let now = self.now();
        // (its last seconds or its last bar nearly empty, as the fight's end watch knows them, as of the bar's last
        // sight: the fight can be ended seconds after the bar went, by its grace or the game log's scene change)
        let ending = self.fight_ending(if self.bar_seen > 0.0 { self.bar_seen.min(now) } else { now }).is_some();
        if let Some(mut f) = self.fight.take() {
            self.fight_ended_at = now;
            self.results_at_fight_end = self.ps.results;
            let unknown = f.brk.unknown_summary();
            if !unknown.is_empty() {
                log::info!(target: "meter", "break gauge: unknown on {unknown} this fight");
            }
            let left = if f.boss_hp != 0 { (f.max_hp - f.damage) as f64 / f.boss_hp as f64 } else { 1.0 };
            if why.starts_with("caught") && !f.bossrush && f.hit && f.timer.as_ref().is_none_or(|t| t.predict(now).is_none()) {
                // the boss kill sound, when heard, dates the blow itself
                let heard = self.kill_sound();
                let (kill, how) = match heard {
                    Some(at) => (at, "by its sound".to_string()),
                    None => (self.catch_seen - crate::fights::KILL_TO_COUNTDOWN, format!("{:.0} s before the Catch Countdown", crate::fights::KILL_TO_COUNTDOWN)),
                };
                if kill - f.t0 > f.elapsed {
                    log::info!(target: "meter", "fight {}: the bar was last read at {:.1}s; the kill, {how}, at {:.1}s", f.title, f.elapsed, kill - f.t0);
                    f.elapsed = kill - f.t0;
                }
            }
            if why.starts_with("caught") || (why == "boss bar gone" && left < 0.15) {
                f.mark_defeated(&self.env, why);
                log::info!(target: "meter", "fight {}: boss defeated - final hit counted, damage set to max HP {}", f.title, thousands(f.max_hp));
            }
            if !f.hit {
                log::info!(target: "meter", "fight {}: nobody hit it - no result shown", f.title);
            }
            let unattended = f.away > 0.5 * 1f64.max(now - f.created);
            if unattended {
                log::info!(target: "meter", "fight {}: unattended ({:.0}s of it with the game in the background) - no result shown", f.title, f.away);
            }
            if self.last_screen.is_some() && why != "meter closed" && f.hit && !unattended {
                out.out(
                    "fight_end",
                    dict([
                        ("damage", Value::Int(f.damage)),
                        ("max_hp", Value::Int(f.max_hp)),
                        ("elapsed", Value::Float(f.elapsed)),
                        ("defeated", Value::Bool(f.defeated)),
                        ("title", Value::Str(f.title.clone())),
                        ("boss", f.boss_of().map_or(Value::None, |(a, b)| Value::List(vec![Value::Int(a), Value::Int(b)]))),
                        ("avg", Value::Float(if f.elapsed > 1.0 { f.damage as f64 / f.elapsed } else { 0.0 })),
                        ("bossrush", Value::Bool(f.bossrush)),
                        ("holosim", Value::Bool(self.holosim_space)),
                        ("screen", screen_value(self.last_screen)),
                    ]),
                );
            }
            let mut msg = format!("fight over ({why}): {} Lv {} - {} team damage in {:.0}s", f.title, f.level, thousands(f.damage), f.elapsed);
            if f.defeated {
                msg += " - defeated";
            }
            if f.elapsed > 1.0 {
                msg += &format!(" = {} DPS", thousands((f.damage as f64 / f.elapsed).round_ties_even() as i64));
            }
            self.status(out, msg);
            f.ended = Some(now);
            f.close(&self.env);
            self.fight_evidence_done(&f);
            if why != "meter closed" {
                self.seat_note_end(&f, ending, now);
            }
            if f.elapsed > 20.0 {
                self.awaiting = Some(now + 60.0);
            }
            if why != "meter closed" {
                self.after_fight = Some((f.csv_name.clone(), now + 45.0, 0.0));
                self.stages_fight(&f, now); // (a Sim room's only)
            }
            self.last_fight = Some((f, now));
        }
        self.switch_votes.clear();
    }

    /// The game log's events: scenes entered and left, Interlink and Sim scenes, your UID.
    pub fn game_events(&mut self, out: &mut dyn Out) {
        let Some(rx) = &self.events else { return };
        let evs: Vec<Event> = rx.try_iter().collect();
        let said = |kind: &str, sid: Option<&str>, delay: Option<f64>| {
            log::info!(target: "meter", "game log: {kind} {}{}", sid.unwrap_or(""), delay.map_or(String::new(), |d| format!(" (seen {d:.1}s after the game wrote it)")));
        };
        for e in evs {
            match e {
                Event::Uid(u) => {
                    if self.uid.as_ref() != Some(&u) {
                        said("uid", Some(&u), None);
                    }
                    self.set_uid(&u);
                }
                Event::Enter(space, delay) => {
                    said("enter", Some(&space), delay);
                    // someone's home is theirs: only before any UID
                    if self.uid.is_none()
                        && let Some(c) = regex::Regex::new(r"^\d+-(\d+)-\d+$").unwrap().captures(&space)
                    {
                        let u = c[1].to_string();
                        self.set_uid(&u);
                    }
                    self.bossrush_space = self.rush_spaces.contains(&space);
                    let was_sim = self.holosim_space;
                    self.holosim_space = self.sim_spaces.contains(&space);
                    self.stages_enter(&space, delay, was_sim);
                    self.space = Some(space);
                    self.fight_end_off("the game entered another scene");
                    self.duds.clear();
                    self.dud_cand = None;
                    // the timer check may already have ended this fight; the log only ends it if it is still running
                    if self.fight.as_ref().is_some_and(|f| self.env.now() - f.created > 5.0) {
                        self.end_fight(out, "scene changed (game log)");
                    }
                    self.bar = None;
                }
                Event::Leave(space, delay) => {
                    said("leave", Some(&space), delay);
                    self.fight_end_off("the game left the fight's scene");
                }
                Event::BossRush(sid, delay) => {
                    said("bossrush", sid.as_deref(), delay);
                    let sid = sid.or_else(|| self.space.clone());
                    if let Some(s) = &sid {
                        self.rush_spaces.insert(s.clone());
                    }
                    if sid.is_some() && sid == self.space {
                        self.bossrush_space = true;
                        if let Some(f) = self.fight.as_mut()
                            && f.bossrush
                        {
                            f.scene_ok = true;
                        }
                    }
                }
                Event::HoloSim(sid, delay) => {
                    said("holosim", sid.as_deref(), delay);
                    let sid = sid.or_else(|| self.space.clone());
                    if let Some(s) = &sid {
                        self.sim_spaces.insert(s.clone());
                    }
                    if sid.is_some() && sid == self.space {
                        let was_sim = self.holosim_space;
                        self.holosim_space = true;
                        self.stages_holosim(was_sim);
                    }
                    self.load_icons(); // the Victory screen's gizmo row needs them
                }
            }
        }
    }

    /// One look at the game: keep the bar locked, follow the fight, read its text.
    pub fn step(&mut self, frames: &mut dyn Frames, out: &mut dyn Out) {
        self.reload_settings(self.env.now());
        self.bar_gone_check(out);
        // holds end on time even without a picture
        if self.egg.hold.as_ref().is_some_and(|h| self.env.now() > h.until) {
            self.egg_hold_end("time limit");
        }
        self.auto_active();
        self.result_sent_check();
        let meter_off = !self.args.replay && frames.switched_off();
        let cfg = self.args.cfg.clone();
        self.switches.tick(&self.manifest, &cfg, !meter_off, &mut self.health);
        let mode = self.manifest.meter_mode(meter_off, &cfg, &self.health);
        let (rest, quiet) = (mode == "rest", mode == "quiet");
        if quiet != self.quiet {
            self.quiet = quiet;
            log::info!(target: "meter", "{}", if quiet { "the DPS meter is off: reading results for the leaderboards only" } else { "the DPS meter shows again" });
        }
        if rest != self.capture_off {
            self.capture_off = rest;
            log::info!(target: "meter", "{}", if rest { "the DPS meter and the leaderboards are off: resting" } else { "reading the game again" });
            if rest {
                self.end_auto("the DPS meter and the leaderboards were switched off");
                self.egg_hold_end("the DPS meter and the leaderboards were switched off");
                self.fight_end_off("the DPS meter and the leaderboards were switched off");
                self.hold.stop();
                self.told_inactive.stop();
                if alogs_capture::input::controllers_hidden() {
                    alogs_capture::input::hide_controllers(false);
                }
                if self.fight.is_some() {
                    self.end_fight(out, "the DPS meter was switched off");
                }
                self.bar = None;
            }
        }
        if rest {
            self.status(out, "The DPS meter is off".into());
            return;
        }
        self.game_events(out);
        // A homeland has no boss fights: no bar is looked for there, and the loop looks once a second (the PC lagged
        // there, B4). Its screens are still read: the Egg Heist rank screen after a match queued from home, Rank Info.
        let home = !self.args.replay && scene_of(self.space.as_deref()) == Some(HOMELAND_SCENE);
        if home != self.homeland_rest {
            self.homeland_rest = home;
            log::info!(target: "meter", "{}", if home { "in a homeland: no boss bars looked for, screens still read" } else { "left the homeland: looking for boss bars again" });
        }
        if home {
            self.bar = None;
        }
        if !self.args.replay {
            self.load_icons(); // every reader of them comes after this
        }
        self.poll_icons();
        // out in the world, keep the app listening for the boss kill sound while a fight is on
        if !self.args.replay {
            if self.fight.as_ref().is_some_and(|f| !f.bossrush) && self.args.cfg.get("audio_cues").get("boss_kill").truthy() {
                frames.audio_want(self.env.now());
            }
            self.audio_cue = frames.audio_cue();
        }
        self.check_pending_run(self.now()); // (Battle Data waiting for its names counts)
        self.seat_note_check(self.now());
        self.check_followup(self.now());
        let now = self.now();
        self.fight_end_tick(now); // an Interlink fight about to end: its fade is watched for, on whole frames
        let mut partial: Option<f64> = (self.fight.is_some() && self.bar.is_some() && now - self.bar_seen < 1.0 && !self.args.replay && self.sim.tour.is_none()
            && !self.fight_end.watching())
        .then_some(TOP_PART);
        self.band_frame = false;
        if partial.is_some() && !self.text_due(now) && self.frame_size.1 != 0 {
            let b = self.bar.as_ref().unwrap();
            let band = (b.yc as f64 + 0.06 * b.span + 4.0) / self.frame_size.1 as f64;
            if band < TOP_PART {
                if self.shots.force_due(now) {
                    // a fight evidence picture is wanted: this look is a top-part one (once a second at most)
                    self.shots.forced_at(now);
                } else {
                    partial = Some(band);
                    self.band_frame = true;
                }
            }
        }
        self.hwnd = frames.window();
        let Some(hwnd) = self.hwnd else {
            self.status(out, "waiting for the game window".into());
            return;
        };
        // Where the frame sits on the screen, read with every frame as the Python did: our clicks and scrolls, the HP
        // labels and the covering of our own overlays are all placed by it. The Rust meter left it at (0, 0), which is
        // right only for a game at the top left of the main screen (17 Nov).
        if let Some(origin) = frames.origin(hwnd) {
            self.origin = origin;
        }
        // Between fights only the top part is copied unless a whole frame is due (full_frame_due); a screen read alone
        // gets just its boxes. The Gizmo tour reads whole frames.
        let live = !self.args.replay && self.sim.tour.is_none();
        let idle_look = live && partial.is_none() && self.fight.is_none() && self.bar.is_none();
        let due = if idle_look { self.full_frame_due(now) } else { crate::pacing::Due::Whole };
        let top_only = idle_look && due == crate::pacing::Due::No;
        let parts = if idle_look && due == crate::pacing::Due::Screen { self.screen_parts(now) } else { None };
        self.frame_full = partial.is_none() && !top_only;
        self.parts_frame = parts.is_some();
        if parts.is_some() {
            self.diag.load.parts += 1;
        } else if partial.is_some() || top_only {
            self.diag.load.part += 1;
        } else {
            self.diag.load.whole += 1;
        }
        let grabbed = match (&parts, top_only) {
            (Some(p), _) => frames.grab_parts(hwnd, p),
            (None, true) => frames.grab(hwnd, Some(TOP_PART)).map(|top| {
                // painted into a frame-sized picture (the rest black) for the code that expects a whole frame
                let (w, h) = self.frame_size;
                let mut canvas = Image::new(top.mode, w, h, &[0, 0, 0, 0][..top.mode.bands()]);
                canvas.paste(&top, 0, 0);
                canvas
            }),
            _ => frames.grab(hwnd, partial),
        };
        let Some(mut im) = grabbed else {
            self.status(out, "can't see the game window".into());
            return;
        };
        if partial.is_none() && !top_only && parts.is_none() {
            self.frame_size = im.size();
        }
        if !self.args.replay && frames.screen_copy() {
            self.mask_own(frames, &mut im); // our overlays can be in a picture of the screen
        }
        self.frames += 1;
        if let Some(f) = &mut self.fight
            && !self.args.replay
            && !frames.is_foreground(hwnd)
        {
            f.away += 1f64.min(now - if self.last_step == 0.0 { now } else { self.last_step });
        }
        self.last_step = now;
        let fg = self.args.replay || frames.is_foreground(hwnd);
        if self.fight_end.held.is_some() && !fg {
            self.fight_end_release("the game is not the active window");
        }
        if self.frame_full {
            self.fight_end_look(&im, now, fg); // (nothing unless the watch is open)
        }
        self.away_note(Some(&im), fg);
        if !self.quiet {
            self.hit_watch(frames, now);
        }
        let top_h = (self.frame_size.1 as f64 * TOP_PART).max(1.0);
        self.top_sig = Some(crate::team::top_sig_direct(&im, top_h));
        self.egg_hold_tick(now, fg); // Egg Heist: its hold's time limit, and the game going to the background
        self.sim_check(now);
        self.game_only_keep(frames, now);
        self.sim_cards_warm();
        if self.holosim_space && !self.args.replay {
            self.sim_test_key(); // Ctrl+Shift+F11: test the Sim end
            if std::mem::take(&mut self.sim.test_pending_end) && self.fight.is_some() {
                self.end_fight(out, "Holo-Battle Sim test");
            }
        }
        // the Gizmo tour first, before any bar search (its list's lines looked like boss bars)
        if self.sim.tour.is_some() && self.frame_full && self.sim_tour_step(frames, out, &im, now) {
            return;
        }
        if self.bar.is_none() && self.holosim_space {
            self.sim_gate(&im, now, fg);
            self.stages_look(&im, now, fg, !self.args.replay && frames.screen_copy()); // the run's level, once a second
        }
        // In a fight: its held ends (a new fight or frame size starts them over), this look's sight of them, and a
        // reading at them when their bracket check failed while the track still shows the fill's colour. A
        // Holo-Battle Sim room's bar is found and held the same way: it sits where the open world's does.
        self.ends_sync(now);
        let (mut held_read, mut sighted) = (None, false);
        let mut verified = self.bar.as_ref().is_some_and(|b| bar::verify_bar(&im, b));
        if self.fight.is_some()
            && let Some(b) = self.bar.clone()
        {
            (held_read, sighted) = self.ends_look(&im, &b, verified, now);
            if self.bar.as_ref().is_some_and(|nb| !bar::same_ends(nb, &b)) {
                verified = self.bar.as_ref().is_some_and(|nb| bar::verify_bar(&im, nb));
            }
        }
        if !verified && held_read.is_none() {
            if partial.is_some() {
                self.diag.load.whole += 1;
                match frames.grab(hwnd, None) {
                    Some(i) => {
                        im = i;
                        // this picture too: taken as it came, our own HP label ("42.2%", drawn on the bar) was read as
                        // the end of the fill, and the damage jumped a quarter of the boss's HP ahead and stood still
                        // for 8 to 11 s (3 of B19's 4 fights, 17 Nov). The Python covered every picture in its grab.
                        if !self.args.replay && frames.screen_copy() {
                            self.mask_own(frames, &mut im);
                        }
                        self.frame_size = im.size();
                        // a whole frame now, as the Python's grab said: the result screen and the leader's line are
                        // read from it straight away, not only once the bar has been gone a second
                        self.frame_full = true;
                    }
                    None => return,
                }
            }
            let mut found = if self.homeland_rest { None } else { bar::bar_at(&im, &self.bar_spots).or_else(|| bar::find_bar(&im, false, 8)) };
            if let Some(fd) = found.clone()
                && self.fight.is_none()
                && self.is_dud(&im, &fd)
            {
                found = None;
            }
            if let Some(fd) = found.clone()
                && !self.plausible_bar(&fd, &im)
            {
                found = None;
            }
            found = found.map(|fd| self.bar_kind(fd));
            if found.as_ref().is_some_and(|f| f.world) && self.quiet && self.fight.is_none() && !(self.bossrush_space || self.holosim_space) {
                found = None;
            }
            if self.fight.is_some()
                && let Some(fd) = found.clone()
            {
                found = self.ends_found(fd, now, sighted);
            }
            if let Some(fd) = found {
                let changed = self.bar.as_ref().is_none_or(|old| (old.span - fd.span).abs() > 3.0 || (old.yc - fd.yc).abs() > 3);
                if changed {
                    log::info!(target: "meter", "boss bar found: span {:.0}px, y {:.0}, frame {}x{}", fd.span, fd.yc, im.width, im.height);
                }
                // a title read still waiting was made for another shape (a false bar, lost a moment ago): it is not
                // this bar's. Taken as its first read it said "Lv None" and cost the real read a turn (15 times in
                // the 17 Nov logs).
                if self.bar.is_none()
                    && let Some(w) = &self.ocr_bg
                {
                    w.forget();
                }
                self.bar = Some(fd);
                self.end_auto("boss bar is back");
                self.bar_sample(&im, now);
            } else if self.fight.is_some() && {
                let f = self.fight.as_ref().unwrap();
                let grace = if f.bossrush && (self.bossrush_space || f.partner.is_some()) { BAR_GONE_GRACE * 4.0 } else { BAR_GONE_GRACE };
                now - self.bar_seen < grace
            } {
                self.watch_end_note(out, &im);
                self.leader_ended_note(&im, now);
                self.check_battle_data(frames, out, &im, now);
                if self.fight.is_some() && self.catch_seen >= self.bar_seen - 2.0 {
                    self.end_fight(out, "caught (Catch Countdown)");
                    return;
                }
                if let (Some(_), Some(b)) = (&self.fight, self.bar.clone()) {
                    let (fill, fill_rgb, edge_rgb) = bar::measure_fill_any(&im, &b, 70);
                    let live = fill_rgb[0] >= 225 && (80..=140).contains(&fill_rgb[1]) && (75..=135).contains(&fill_rgb[2]);
                    let last = self.fight.as_ref().unwrap().last_fill;
                    let mut ok = live && last.is_none_or(|l| fill <= l + 0.05);
                    if ok && last.is_some_and(|l| fill < l - 0.3) {
                        let run = match self.hidden_drop {
                            Some(h) if (h.0 - fill).abs() <= 0.02 => h,
                            _ => (fill, 0, 0.0),
                        };
                        let counts = now - run.2 >= CONFIRM_GAP;
                        self.hidden_drop = Some((fill, run.1 + i64::from(counts), if counts { now } else { run.2 }));
                        ok = self.hidden_drop.unwrap().1 >= 3;
                    }
                    if ok {
                        self.hidden_drop = None;
                        let slide = true;
                        let at = frames.frame_time();
                        let grades = self.grades.clone();
                        let moved = self.ends_moved(&b);
                        let f = self.fight.as_mut().unwrap();
                        f.ends_moved = moved;
                        f.update(&self.env, Some(fill), fill_rgb, edge_rgb, None, None, &grades, self.args.duration, at, slide);
                        self.emit_fight(out, &b);
                        self.bar_seen = now;
                    }
                }
                return;
            } else {
                if self.bar.is_some() {
                    log::info!(target: "meter", "boss bar lost ({})", bar::LAST_FAIL.lock().unwrap());
                    self.bars.lost_at = Some(now);
                    self.bars.lost_n = 0;
                }
                self.bar = None;
                if self.fight.is_some() {
                    self.end_fight(out, "boss bar gone");
                }
                if self.sim_tour_step(frames, out, &im, now) {
                    return;
                }
                self.save_end_screens(&im, now);
                self.save_nobar(&im, now);
                self.save_bar_lost(&im, now);
                self.watch_end_note(out, &im);
                self.leader_ended_note(&im, now);
                self.check_battle_data(frames, out, &im, now);
                self.status(out, "waiting for a boss bar".into());
                return;
            }
        }
        let b = self.bar.clone().unwrap();
        let (fill, fill_rgb, edge_rgb) = held_read.unwrap_or_else(|| bar::measure_fill_any(&im, &b, 70));
        let last = self.fight.as_ref().and_then(|f| f.last_fill);
        if last.is_some_and(|l| l < 0.5 && fill > l + 0.5) && fill_rgb[2] > fill_rgb[0] {
            let since = *self.not_bar_since.get_or_insert(now);
            if now - since >= BAR_GONE_S {
                log::info!(target: "meter", "boss bar lost (the locked spot reads {:.0}% in rgb{:?}, the fight was at {:.0}%)", fill * 100.0, fill_rgb, last.unwrap() * 100.0);
                self.not_bar_since = None;
                self.bar = None;
                if self.fight.as_ref().is_some_and(|f| f.partner.is_none()) {
                    self.end_fight(out, "boss bar gone");
                }
            }
            return;
        }
        self.not_bar_since = None;
        self.bar_seen = now;
        self.fight_end_bar_back(); // a hold from a fade the fight came back from ends at once
        let scale = b.span / bar::REF_SPAN;
        let need_title = self.fight.is_none() || now - self.last_title > self.title_gap();
        let xn_box = bar::box_at(&b, bar::XN_BOX);
        let top = boss::xn_top(&self.grades);
        let boxim = im.crop_i(xn_box.0 as i64, xn_box.1 as i64, xn_box.2 as i64, xn_box.3 as i64);
        let mut xnv = self.xn_cache.read(&boxim, &self.xn_tpl, top, 0.22);
        if self.fight.is_none() {
            let low = b.yc as f64 > TOP_BAR * self.frame_size.1 as f64;
            self.instant_start(out, xnv, now, low);
        }
        if self.dud_check(&b, Some(fill), xnv, now) {
            self.bar = None;
            self.status(out, "waiting for a boss bar".into());
            return;
        }
        let mut timer = None;
        // Text (timer, title, xN fallback) is read about once a second. Live, a background OCR helper does it so a frame
        // never waits for it; the result is used on the frame after it arrives. Replays read it in place.
        if let Some((text, (xn_missing, title, level_at))) = self.ocr_bg.as_ref().and_then(|w| w.take()) {
            if let Some(t) = level_at {
                self.stages_feed(text.get(crate::sim_stages::BAR_KEY).map(String::as_str), t); // a Sim run's level
            }
            timer = parse::parse_timer(text.get("timer").map(String::as_str));
            if xn_missing && xnv.is_none() {
                xnv = parse::parse_xn(text.get("xn").map(String::as_str), top);
            }
            if title {
                let (t, r) = (text.get("title").cloned(), text.get("title_red").cloned());
                self.identify(out, t.as_deref(), xnv, r.as_deref());
            }
        }
        if self.text_due(now) && !self.band_frame {
            let (w, h) = (self.frame_size.0 as f64, self.frame_size.1 as f64);
            let mut crops: IndexMap<String, Image> = IndexMap::new();
            crops.insert("timer".into(), prep::prep_text(&im, bar::timer_box(w, h, false), scale, 190, 3.0, 24));
            if xnv.is_none() {
                crops.insert("xn".into(), prep::prep_short(&im, xn_box, scale, 3));
            }
            let tb = bar::box_at(&b, bar::TITLE_BOX);
            if need_title {
                // the level by the game's own digits, from this same look (None when the font can't be loaded)
                let fl = self.name_reader().and_then(|r| r.level(&im, tb));
                self.title_font = Some((fl, now));
                crops.insert("title".into(), prep::prep_text(&im, tb, scale, 190, 3.0, 24));
                if prep::has_red(&im, tb, scale) {
                    crops.insert("title_red".into(), prep::prep_red(&im, tb, scale, 190, 3.0, 24));
                }
            }
            // a Holo-Battle Sim run's level counter rides along (once a second at most): a level can be all fight
            let level_at = self.stages_bar_crop(&im, now, fg, !self.args.replay && frames.screen_copy()).map(|c| {
                crops.insert(crate::sim_stages::BAR_KEY.into(), c);
                now
            });
            self.last_text = now;
            if need_title {
                self.last_title = now;
            }
            if let Some(w) = &self.ocr_bg {
                w.submit(crops, (xnv.is_none(), need_title, level_at));
            } else {
                let text = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
                if let Some(t) = level_at {
                    self.stages_feed(text.get(crate::sim_stages::BAR_KEY).map(String::as_str), t);
                }
                timer = parse::parse_timer(text.get("timer").map(String::as_str));
                if xnv.is_none() {
                    xnv = parse::parse_xn(text.get("xn").map(String::as_str), top);
                }
                if need_title {
                    let (t, r) = (text.get("title").cloned(), text.get("title_red").cloned());
                    self.identify(out, t.as_deref(), xnv, r.as_deref());
                }
            }
        }
        if self.fight.is_some() {
            self.snapshot(&im, &b, now);
            // (read only on a look on which the bar itself was read: feed_look counts the others)
            if now - self.brk_at >= CONFIRM_GAP {
                self.brk_at = now;
                let read = if crate::fights::bar_red(fill_rgb) { bar::measure_break(&im, &b) } else { bar::BreakRead::default() };
                let f = self.fight.as_mut().unwrap();
                // (every bar but an Interlink one goes by the broken look: the open world's, a Holo-Battle Sim
                // room's, any other fight's)
                let by_look = !f.bossrush;
                f.brk.feed_look(&read, fill_rgb, by_look, now);
            }
            // A low first reading that is followed by a much fuller one, before anyone has hit, is dropped: always
            // (it was the Dev switch fight-bar-slide-in; a wrong reading kept is no trade-off to choose).
            let slide = true;
            let at = frames.frame_time();
            let grades = self.grades.clone();
            let moved = self.ends_moved(&b);
            let f = self.fight.as_mut().unwrap();
            f.ends_moved = moved;
            let bosses = f.phases.len();
            f.update(&self.env, Some(fill), fill_rgb, edge_rgb, xnv, timer, &grades, self.args.duration, at, slide);
            let (handed, damage, elapsed, limit) = (f.phases.len() != bosses, f.damage, f.elapsed, f.limit.filter(|l| *l != 0));
            if handed {
                let seg = f.seg;
                self.boss_handed_over(out, &format!("its bar came back at x{}", seg.map_or("None".into(), |s| s.to_string())));
            }
            if self.shots.on {
                // a look holding the top part (a whole frame is cropped to it) is the picture a rise asked for; then
                // this look's own rise is weighed (fight_shots.rs)
                if !self.band_frame && self.shots.wants().is_some() {
                    let rows = ((self.frame_size.1 as f64 * TOP_PART) as usize).min(im.height);
                    self.shots.look(&im, rows, elapsed);
                }
                // the fight's expected length: its time limit as read off its timer, else as known at its start
                // (240 s with Overtime picked, else 180 s)
                let limit = limit.map_or(self.shots.limit_s, |l| l as f64);
                self.shots.damage(damage, elapsed, limit, !handed);
            }
            if partial.is_none() {
                self.test_frame(&im, "fight", now);
            }
            // an Interlink fight: your team from the fight's right side while the Party screen left a slot blank
            if self.fight.as_ref().is_some_and(|f| f.bossrush) && partial.is_none() {
                let (w, h) = (im.width as f64, im.height as f64);
                let u = w.min(h * 16.0 / 9.0);
                self.fight_team(&im, now, u, (w - u) / 2.0, h);
            }
            self.emit_fight(out, &b);
        } else {
            self.keep_early(&b, frames.frame_time().unwrap_or(now), fill, fill_rgb, edge_rgb);
            self.emit_bar(out, &b, fill, fill_rgb);
        }
    }
}

fn screen_value(s: Option<(i32, i32, usize, usize)>) -> Value {
    s.map_or(Value::None, |(x, y, w, h)| Value::List(vec![Value::Int(i64::from(x)), Value::Int(i64::from(y)), Value::Int(w as i64), Value::Int(h as i64)]))
}

fn load_spots(path: &std::path::Path) -> Vec<Spot> {
    let Some(v) = std::fs::read_to_string(path).ok().and_then(|t| Value::json_loads(&t)) else { return vec![] };
    v.as_list()
        .map(|l| {
            l.iter()
                .filter_map(|s| {
                    // (places kept before 17 Nov were of every find, most of them no bar: only a place marked as
                    // a fight's is taken, since the HP labels now show at such a place at once)
                    if !s.get("fight").truthy() {
                        return None;
                    }
                    let f = s.get("frame").as_list()?;
                    Some(Spot {
                        frame: (f.first()?.as_i64()? as usize, f.get(1)?.as_i64()? as usize),
                        yc: s.get("yc").as_f64()?,
                        thickness: s.get("thickness").as_f64()?,
                        left_bracket: s.get("left_bracket").as_f64()?,
                        span: s.get("span").as_f64()?,
                        track_left: s.get("track_left").as_f64()?,
                    })
                })
                // (a place that isn't centred on its frame is loaded all the same: bar_at skips it outside a Holo-Battle
                // Sim room, and the next held ends at that place replace it; a Sim room's may be one)
                .collect()
        })
        .unwrap_or_default()
}

fn save_spots(path: &std::path::Path, spots: &[Spot]) {
    let n = |v: f64| if v.fract() == 0.0 { Value::Int(v as i64) } else { Value::Float(v) };
    let v = Value::List(
        spots
            .iter()
            .map(|s| {
                dict([
                    ("yc", n(s.yc)),
                    ("track_left", n(s.track_left)),
                    ("left_bracket", n(s.left_bracket)),
                    ("span", n(s.span)),
                    ("thickness", n(s.thickness)),
                    ("fight", Value::Bool(true)),
                    ("frame", Value::List(vec![Value::Int(s.frame.0 as i64), Value::Int(s.frame.1 as i64)])),
                ])
            })
            .collect(),
    );
    let _ = alogs_protocol::settings::write_whole(path, &v.json_dumps());
}

/// The level of the last Interlink fight (logs/fights' newest Interlink summary), the season's boss level.
pub fn last_rush_level(data: &std::path::Path) -> Option<i64> {
    let dir = data.join("logs").join("fights");
    let mut files: Vec<_> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    for p in files.iter().rev() {
        let Some(s) = std::fs::read_to_string(p).ok().and_then(|t| Value::json_loads(&t)) else { continue };
        if s.get("bossrush").truthy() && s.get("level").truthy() {
            return s.get("level").as_i64();
        }
    }
    None
}

impl Meter {
    /// The running game's build, read from Player.log's head (one bounded read; the file is new at every game start),
    /// as a run's `game` (payloads::game_value). Logged when first known or changed. None while it isn't known.
    pub fn game_build_now(&mut self) -> Option<Value> {
        let g = alogs_gamelog::game_build(&self.args.player_log)?;
        if self.game_build.as_ref() != Some(&g) {
            let offered = match (g.patch, &g.patch_action) {
                (Some(p), Some(a)) => format!(" (offered {p}, {a})"),
                (Some(p), None) => format!(" (offered {p})"),
                (None, Some(a)) => format!(" ({a})"),
                (None, None) => String::new(),
            };
            log::info!(target: "meter", "game build {}{offered}", g.build);
            self.game_build = Some(g.clone());
        }
        Some(crate::payloads::game_value(&g))
    }

    /// The game's own pictures (icons.py's cache in <data>/icons), loaded once in the background: read from the game
    /// first when there's no cache or the game was updated since.
    pub fn load_icons(&mut self) {
        if self.icons_state.is_some() {
            return;
        }
        self.icons_state = Some("loading".into());
        let dir = self.env.data.join("icons");
        let log = self.args.player_log.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ic = alogs_reader::icons::Icons::load(&dir, Some(&log));
            if let Err(e) = &ic {
                log::info!(target: "meter", "game icons: not readable ({e})");
            }
            let _ = tx.send(ic.ok());
        });
        self.icons_job = Some(rx);
    }

    fn poll_icons(&mut self) {
        let Some(rx) = &self.icons_job else { return };
        if let Ok(ic) = rx.try_recv() {
            self.icons_job = None;
            match &ic {
                Some(i) => log::info!(target: "meter", "game icons: {} Aniimo, {} buffs (game build {})", i.heads.len(), i.buffs.len(), i.build),
                None => {}
            }
            self.icons_state = Some(if ic.is_some() { "ready" } else { "failed" }.into());
            self.icons = ic;
        }
    }
}

/// The boss kill sound counts when it started this many seconds before the Catch Countdown.
pub const KILL_SOUND_WINDOW: (f64, f64) = (2.5, 6.0);

impl Meter {
    /// When the boss kill sound started, if the app heard one in the seconds before the Catch Countdown.
    pub fn kill_sound(&mut self) -> Option<f64> {
        if !self.args.cfg.get("audio_cues").get("boss_kill").truthy() {
            return None;
        }
        let (n, at, score) = self.audio_cue?;
        if self.kill_sound_used == Some(n) {
            return None;
        }
        let before = self.catch_seen - at;
        if !(KILL_SOUND_WINDOW.0..=KILL_SOUND_WINDOW.1).contains(&before) {
            return None;
        }
        self.kill_sound_used = Some(n);
        log::info!(target: "meter", "boss kill sound heard {before:.1}s before the Catch Countdown (match {score:.2})");
        Some(at)
    }
}

impl Meter {
    /// The picture is of the screen and our overlay windows show in captures: cover where they are (and were a moment
    /// ago) in the reader's COVER colour, so the meter never reads its own numbers; the other aLogs programs' too.
    pub fn mask_own(&mut self, frames: &mut dyn Frames, im: &mut Image) {
        let mut rects: Vec<(i32, i32, i32, i32, f64)> = match &self.own {
            Some(o) => {
                let o = o.lock().unwrap();
                if o.in_capture { o.rects.clone() } else { vec![] }
            }
            None => vec![],
        };
        rects.extend(frames.other_overlays().into_iter().map(|r| (r.0, r.1, r.2, r.3, 0.0)));
        let t = self.env.now();
        let (ox, oy) = self.origin;
        let c = alogs_reader::bar::COVER;
        let fill: Vec<u8> = if im.mode == alogs_pil::Mode::Rgba { vec![c[0], c[1], c[2], 255] } else { c.to_vec() };
        for (x0, y0, x1, y1, until) in rects {
            if until != 0.0 && until < t {
                continue;
            }
            alogs_pil::draw::rectangle(im, (f64::from(x0 - ox), f64::from(y0 - oy), f64::from(x1 - ox - 1), f64::from(y1 - oy - 1)), &fill);
        }
    }
}

/// A fight's held bar ends may be replaced (once) this many seconds into it (bar.rs `EndsHold`).
pub const ENDS_REPLACE_S: f64 = 30.0;

/// Is this the fill's own colour at the track's left end (the bar is up and alive)?
fn live_fill(rgb: [u8; 3]) -> bool {
    rgb[0] >= 225 && (80..=140).contains(&rgb[1]) && (75..=135).contains(&rgb[2])
}

impl Meter {
    /// A bar found, with its track by where it is: in an Interlink fight always the Interlink track (never the open
    /// world's, whatever its right end showed: bar.rs `interlink_track`); out in the world and in a Holo-Battle Sim
    /// room the world track; else as its right end showed.
    pub fn bar_kind(&self, fd: Bar) -> Bar {
        let rush_fight = self.fight.as_ref().is_some_and(|f| f.bossrush);
        if rush_fight && !self.holosim_space {
            return bar::interlink_track(&fd);
        }
        if fd.world && (rush_fight || self.bossrush_space) {
            bar::as_bossrush_bar(&fd)
        } else if !self.bossrush_space && (scene_of(self.space.as_deref()).is_some() || self.holosim_space) && !self.args.replay {
            // (a Holo-Battle Sim room too: its bars are world bars. Left to the right-end test, a white
            // hit flash there made one an xN bar, whose track ends at 91% of the span)
            bar::as_world_bar(&fd)
        } else {
            fd
        }
    }

    /// The held ends are this fight's at this frame size: a new fight (or none), or another frame size, starts them
    /// over. An Interlink fight's bar is never read on the open-world track.
    pub fn ends_sync(&mut self, now: f64) {
        let id = self.fight.as_ref().map(|f| format!("{} {}", f.csv_name, f.created));
        if id != self.ends.fight || self.ends.frame != self.frame_size {
            let e = &self.ends;
            if let Some(h) = &e.held {
                let why = if id == e.fight { "the frame size changed: dropped" } else { "the fight is over" };
                log::info!(target: "meter", "boss bar ends: span {:.0}px held, seen on {} looks{}; {} looks read at them with a bracket not seen, {} search results not taken ({why})",
                    h.span, e.seen, if e.replaced { " (replaced once)" } else { "" }, e.held_reads, e.not_taken);
            }
            self.ends = bar::EndsHold::new(id, self.frame_size, now);
        }
        if self.fight.as_ref().is_some_and(|f| f.bossrush)
            && !self.holosim_space
            && let Some(b) = &self.bar
            && b.world
        {
            self.bar = Some(bar::interlink_track(b));
        }
    }

    /// The ends were held or replaced on this look: the bar is read there from now on, the fight's geometry is
    /// theirs (`plausible_bar`), and their place is kept for the next fights.
    fn ends_news(&mut self, news: bar::HoldNews) {
        if news == bar::HoldNews::Nothing {
            return;
        }
        let h = self.bar_kind(self.ends.held.clone().unwrap());
        self.ends.held = Some(h.clone());
        log::info!(target: "meter", "boss bar ends {}: span {:.0}px (brackets at x {:.0} and {:.0}), y {}, seen the same on {} looks",
            if news == bar::HoldNews::Held { "held" } else { "replaced" }, h.span, h.left_bracket, h.left_bracket + h.span, h.yc, self.ends.seen);
        if let Some(f) = &mut self.fight {
            f.bar_geom = Some((h.span, h.yc as f64));
        }
        self.remember_spot(&h);
        self.bar = Some(h);
    }

    /// This look's sight of the fight's bar ends (`b` the bar locked now, `verified` its bracket check): both its
    /// brackets seen on a centred bar, or (held ends that may still be replaced, their check failed) the ends found
    /// again from their track. Returns the reading at the held ends when their bracket check failed but their track
    /// still starts in the fill's colour (no whole frame and no search: a bracket hidden by a badge, an icon or a hit
    /// is no reason to look again), and whether a sight was counted.
    pub fn ends_look(&mut self, im: &Image, b: &Bar, verified: bool, now: f64) -> (Option<(f64, [u8; 3], [u8; 3])>, bool) {
        let early = now - self.ends.since <= ENDS_REPLACE_S;
        let sight = if verified && bar::centred(b) && bar::verify_strict(im, b) {
            Some(Bar { brackets_found: true, ..b.clone() })
        } else if self.ends.held.is_some() && !self.ends.replaced && early && !verified {
            bar::find_at(im, b).map(|f| self.bar_kind(f))
        } else {
            None
        };
        let counted = sight.as_ref().is_some_and(|s| s.brackets_found);
        let news = self.ends.look(sight.as_ref(), early);
        self.ends_news(news);
        let (Some(held), Some(now_bar)) = (self.ends.held.clone(), self.bar.clone()) else { return (None, counted) };
        let ok = if bar::same_ends(&now_bar, b) { verified } else { bar::verify_bar(im, &now_bar) };
        if ok || !bar::same_ends(&held, &now_bar) {
            return (None, counted);
        }
        let r = bar::measure_fill_any(im, &now_bar, 70);
        if !live_fill(r.1) {
            return (None, counted); // (the bar may be gone: looked for as before)
        }
        self.ends.held_reads += 1;
        (Some(r), counted)
    }

    /// A search result in a fight. Before any ends are held it is taken (and counts towards holding them); once they
    /// are, only when it agrees with them within a couple of px, or when, early in the fight, it has been seen often
    /// enough to replace them (bar.rs `EndsHold`). Otherwise None: the fight goes on at the held ends, through the
    /// bar-gone path's checks.
    fn ends_found(&mut self, fd: Bar, now: f64, sighted: bool) -> Option<Bar> {
        let early = now - self.ends.since <= ENDS_REPLACE_S;
        let had = self.ends.held.clone();
        let news = if sighted { bar::HoldNews::Nothing } else { self.ends.look(Some(&fd), early) };
        self.ends_news(news);
        if news != bar::HoldNews::Nothing {
            return self.ends.held.clone();
        }
        let Some(h) = had else { return Some(fd) };
        if bar::same_ends(&fd, &h) {
            return Some(h);
        }
        if self.ends.not_taken == 0 {
            log::info!(target: "meter", "a search found other bar ends (span {:.0}px, x {:.0} to {:.0}, y {}): not taken, the held ones are read on", fd.span, fd.left_bracket, fd.left_bracket + fd.span, fd.yc);
        }
        self.ends.not_taken += 1;
        None
    }

    /// Did the ends the bar is read at change since the fight's last reading (by more than bar::AGREE_PX)? The
    /// reading on such a look waits for one more look before it counts as damage (fights.rs `ends_moved`).
    pub fn ends_moved(&mut self, b: &Bar) -> bool {
        let e = (b.left_bracket, b.span, b.yc);
        let moved = self.ends.last_read.is_some_and(|l| {
            (l.0 - e.0).abs() > bar::AGREE_PX || (l.0 + l.1 - e.0 - e.1).abs() > bar::AGREE_PX || (l.2 - e.2).abs() > 3
        });
        self.ends.last_read = Some(e);
        moved
    }
}
