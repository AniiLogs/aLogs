//! The reading loop: the `Meter` and what every plug-in shares, and following fights on it. Port of `Meter.step` and
//! its capture/state handling in aLogs B16 `bossbar/live.py`, and of `FightsMixin` in `bossbar/fights.py`.
//!
//! The end-screen readers, the leaderboards plug-in, the screen readers and the diagnostics are in their own modules
//! (results, egg, sim, leaderboards, screens, party, menu, diag); how much of the picture each look copies in pacing.
//! Left out: the Dev switches' calm pacing (busy-half-pace), and the damage-number recorder (paused in B16).

use crate::env::Env;
use crate::fights::{CONFIRM_GAP, FIGHT_INTERVAL, Fight, LEVEL_CONFIRM, TOP_BAR};
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
    pub ocr_bg: Option<crate::ocr_worker::OcrWorker<(bool, bool)>>,
    /// Live: the screens' text read on a helper of its own, with the frame and time it was read from.
    pub screen_bg: Option<crate::ocr_worker::OcrWorker<(Image, f64)>>,
    pub fight: Option<Fight>,
    pub hwnd: Option<usize>,
    pub frame_size: (usize, usize),
    pub bar: Option<Bar>,
    pub bar_seen: f64,
    pub bar_spots: Vec<Spot>,
    pub last_text: f64,
    pub last_title: f64,
    pub switch_votes: Vec<(String, i64)>,
    pub space: Option<String>,
    pub bossrush_space: bool,
    pub holosim_space: bool,
    pub rush_spaces: std::collections::BTreeSet<String>,
    pub sim_spaces: std::collections::BTreeSet<String>,
    pub duds: Vec<Dud>,
    pub dud_cand: Option<Dud>,
    pub hidden_drop: Option<(f64, i64, f64)>,
    pub events: Option<Receiver<Event>>,
    pub catch_seen: f64,
    pub last_fight: Option<(Fight, f64)>,
    pub fight_ended_at: f64,
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
    pub egg: crate::egg::EggState,
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
    pub focus_guard: alogs_capture::focusguard::FocusGuard,
    /// The clientRunId inputs are held for until it's sent.
    pub send_waiting: Option<String>,
    pub presence_sent: Option<(String, &'static str, f64)>,
    /// When "You're being attacked" was last raised (the overlay shows it for 5 s).
    pub hit_at: f64,
    /// When the game log last said you left a space (Egg Heist's fade after it is a load).
    pub egg_space_at: f64,
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
    pub submitter: Option<std::sync::Arc<crate::submit::Submitter>>,
    /// Your in-game name (only ever from the game).
    pub character_name: Option<String>,
    pub font_reader: Option<alogs_reader::names::NameReader>,
    pub font_tried: bool,
    pub team: crate::team::Teammates,
    /// The Party screen this run started from, kept as proof.
    pub party_picture: Option<Image>,
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
            last_text: 0.0,
            last_title: 0.0,
            switch_votes: vec![],
            space: None,
            bossrush_space: false,
            holosim_space: false,
            rush_spaces: Default::default(),
            sim_spaces: Default::default(),
            duds: vec![],
            dud_cand: None,
            hidden_drop: None,
            events,
            catch_seen: 0.0,
            last_fight: None,
            fight_ended_at: 0.0,
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
            egg: Default::default(),
            ps: Default::default(),
            menu: Default::default(),
            icon: Default::default(),
            diag: Default::default(),
            hit: Default::default(),
            bars: Default::default(),
            ocr_check: Default::default(),
            presence: None,
            hold: Default::default(),
            focus_guard: Default::default(),
            send_waiting: None,
            presence_sent: None,
            hit_at: 0.0,
            egg_space_at: 0.0,
            own: None,
            audio_cue: None,
            hp_ocr: None,
            icons: None,
            icons_state: None,
            icons_job: None,
            submitter: None,
            character_name: None,
            font_reader: None,
            font_tried: false,
            team: crate::team::Teammates::load(env.data.join("teammates.json")),
            party_picture: None,
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
        need_title || now - self.last_text >= 1.0
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
        let level = self.args.level.or(lv);
        let in_world = self.space.is_some() && scene_of(self.space.as_deref()).is_some();
        let bossrush = !in_world && (xn.is_some() || self.bossrush_space || self.fight.as_ref().is_some_and(|f| f.bossrush));
        let now = self.now();
        let row = if !title.is_empty() && level.is_some() {
            boss::pick_boss(&self.catalog, &title, bossrush, self.args.boss_id.as_deref(), &self.env.schedule, now).0
        } else {
            None
        };
        let Some(row) = row else {
            if self.fight.is_none() && !title.is_empty() && self.bar.is_some() {
                let lvs = level.map_or("None".to_string(), |l| l.to_string());
                self.status(out, format!("reading boss name... (\"{title}\" Lv {lvs})"));
            }
            return;
        };
        let level = level.unwrap();
        let pid = get(&row, "puppetId").to_string();
        if let Some(f) = &mut self.fight {
            if pid == get(&f.row, "puppetId") && level == f.level {
                self.switch_votes.clear();
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
        let how = format!("title read {}, xN {}", alogs_pyval::str_repr(title_text.unwrap_or("")), xn.map_or("None".into(), |x| x.to_string()));
        self.start_fight(out, row, level, &title, bossrush, &how);
        if bossrush {
            self.rush_level = Some(level);
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
        if let Some(b) = &self.bar {
            f.bar_geom = Some((b.span, b.yc as f64));
        }
        let msg = format!("fight started: {} Lv {level}, max HP {}{}", f.title, thousands(hp), if bossrush { " (Interlink)" } else { "" });
        log::info!(target: "meter", "fight details: puppetId {}, {how}, bossrush space {}{}{}", get(&row, "puppetId"),
            if self.bossrush_space { "True" } else { "False" },
            if f.column != GRADE_BASE { format!(", HP bars {}", f.column) } else { String::new() },
            partner.as_ref().map_or(String::new(), |p| format!(", two-boss fight: {} ({}) comes next", get(p, "bossTitle_en"), partner_id.clone().unwrap_or_default())));
        self.fight = Some(f);
        self.status(out, msg);
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
        self.start_fight(out, row, rl, &t, true, &format!("from the prep screen, xN {x}"));
    }

    pub fn end_fight(&mut self, out: &mut dyn Out, why: &str) {
        let now = self.now();
        if let Some(mut f) = self.fight.take() {
            self.fight_ended_at = now;
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
            if f.elapsed > 20.0 {
                self.awaiting = Some(now + 60.0);
            }
            if why != "meter closed" {
                self.after_fight = Some((f.csv_name.clone(), now + 45.0, 0.0));
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
                    self.holosim_space = self.sim_spaces.contains(&space);
                    self.space = Some(space);
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
                    // the fade that follows is a load, not a match's end (egg_fade)
                    self.egg_space_at = self.env.now();
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
                        self.holosim_space = true;
                    }
                    self.load_icons(); // the Victory screen's gizmo row needs them
                }
            }
        }
    }

    /// One look at the game: keep the bar locked, follow the fight, read its text.
    pub fn step(&mut self, frames: &mut dyn Frames, out: &mut dyn Out) {
        self.reload_settings(self.env.now());
        // holds end on time even without a picture
        if self.egg.hold.as_ref().is_some_and(|h| self.env.now() > h.0) {
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
                self.hold.stop();
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
        let home = !self.args.replay && scene_of(self.space.as_deref()) == Some(HOMELAND_SCENE);
        if home != self.homeland_rest {
            self.homeland_rest = home;
            log::info!(target: "meter", "{}", if home { "in a homeland: not capturing until you leave" } else { "left the homeland: capturing again" });
        }
        if home {
            self.bar = None;
            self.status(out, "In a homeland: the meter is resting".into());
            return;
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
        let now = self.now();
        let mut partial: Option<f64> = (self.fight.is_some() && self.bar.is_some() && now - self.bar_seen < 1.0 && !self.args.replay && self.sim.tour.is_none()).then_some(TOP_PART);
        self.band_frame = false;
        if partial.is_some() && !self.text_due(now) && self.frame_size.1 != 0 {
            let b = self.bar.as_ref().unwrap();
            let band = (b.yc as f64 + 0.06 * b.span + 4.0) / self.frame_size.1 as f64;
            if band < TOP_PART {
                partial = Some(band);
                self.band_frame = true;
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
        self.away_note(Some(&im), fg);
        if !self.quiet {
            self.hit_watch(frames, now);
        }
        let top_h = (self.frame_size.1 as f64 * TOP_PART).max(1.0);
        self.top_sig = Some(crate::team::top_sig_direct(&im, top_h));
        if self.frame_full {
            self.egg_fade(&im, now); // Egg Heist: ends a screen hold that's done
        }
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
        }
        if self.bar.as_ref().is_none_or(|b| !bar::verify_bar(&im, b)) {
            if partial.is_some() {
                self.diag.load.whole += 1;
                match frames.grab(hwnd, None) {
                    Some(i) => {
                        im = i;
                        self.frame_size = im.size();
                        // a whole frame now, as the Python's grab said: the result screen and the leader's line are
                        // read from it straight away, not only once the bar has been gone a second
                        self.frame_full = true;
                    }
                    None => return,
                }
            }
            let mut found = bar::bar_at(&im, &self.bar_spots).or_else(|| bar::find_bar(&im, false, 8));
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
            if let Some(fd) = found.clone() {
                if fd.world && (self.fight.as_ref().is_some_and(|f| f.bossrush) || self.bossrush_space) {
                    found = Some(bar::as_bossrush_bar(&fd));
                } else if !self.bossrush_space && scene_of(self.space.as_deref()).is_some() && !self.args.replay {
                    found = Some(bar::as_world_bar(&fd));
                }
            }
            if found.as_ref().is_some_and(|f| f.world) && self.quiet && self.fight.is_none() && !(self.bossrush_space || self.holosim_space) {
                found = None;
            }
            if let Some(fd) = found {
                let changed = self.bar.as_ref().is_none_or(|old| (old.span - fd.span).abs() > 3.0 || (old.yc - fd.yc).abs() > 3);
                if changed {
                    log::info!(target: "meter", "boss bar found: span {:.0}px, y {:.0}, frame {}x{}", fd.span, fd.yc, im.width, im.height);
                }
                let spot = Spot { frame: fd.frame, yc: fd.yc as f64, thickness: fd.thickness as f64, left_bracket: fd.left_bracket, span: fd.span, track_left: fd.track_left as f64 };
                if !self.bar_spots.iter().any(|s| (s.yc - spot.yc).abs() <= 2.0 && (s.track_left - spot.track_left).abs() <= 2.0 && s.frame == spot.frame) {
                    self.bar_spots.insert(0, spot);
                    self.bar_spots.truncate(4);
                    save_spots(&self.env.data.join("bar_spot.json"), &self.bar_spots);
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
                        let slide = self.dev.on("fight-bar-slide-in");
                        let at = frames.frame_time();
                        let grades = self.grades.clone();
                        let f = self.fight.as_mut().unwrap();
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
        let (fill, fill_rgb, edge_rgb) = bar::measure_fill_any(&im, &b, 70);
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
        if let Some((text, (xn_missing, title))) = self.ocr_bg.as_ref().and_then(|w| w.take()) {
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
                crops.insert("title".into(), prep::prep_text(&im, tb, scale, 190, 3.0, 24));
                if prep::has_red(&im, tb, scale) {
                    crops.insert("title_red".into(), prep::prep_red(&im, tb, scale, 190, 3.0, 24));
                }
            }
            self.last_text = now;
            if need_title {
                self.last_title = now;
            }
            if let Some(w) = &self.ocr_bg {
                w.submit(crops, (xnv.is_none(), need_title));
            } else {
                let text = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
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
            if now - self.brk_at >= CONFIRM_GAP {
                self.brk_at = now;
                let (gauge, kind, _) = bar::measure_break(&im, &b);
                self.fight.as_mut().unwrap().brk.feed(gauge, kind, now);
            }
            let slide = self.dev.on("fight-bar-slide-in");
            let at = frames.frame_time();
            let grades = self.grades.clone();
            let f = self.fight.as_mut().unwrap();
            let bosses = f.phases.len();
            f.update(&self.env, Some(fill), fill_rgb, edge_rgb, xnv, timer, &grades, self.args.duration, at, slide);
            if f.phases.len() != bosses {
                let seg = f.seg;
                self.boss_handed_over(out, &format!("its bar came back at x{}", seg.map_or("None".into(), |s| s.to_string())));
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
