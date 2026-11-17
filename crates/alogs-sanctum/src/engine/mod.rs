//! The background engine: finds the game, reads the party screen, tracks the minimap, works out the layout and lines
//! the layout map up while the overlay key is down; what to draw goes out as `OverlayFrame`s. Port of `Engine.cs`
//! (Dev switches at their defaults). Two threads: the main loop owns the run's state (`Core`); the heading loop
//! (heading.rs) reads the facing ~60 times a second and hands each HUD-corner copy to the main loop (`Shared`).

pub mod grab;
pub mod guess;
pub mod heading;
pub mod map;
pub mod run;
pub mod track;

use crate::capture::{GameWindow, Rect};
use crate::cv::{Mat, MatF};
use crate::detectors::DifficultyDetector;
use crate::features::{ArtFeatureCache, MapMatcher};
pub use crate::game_log::Place;
use crate::game_log::GameLog;
use crate::input::{Gamepad, KeyHook};
use crate::lang;
use crate::layout_db::{Layout, LayoutDb};
use crate::ocr::OcrService;
use crate::settings::Settings;
use crate::vision::{self, DoorLocator, FloorRefiner, IslandTracker, MapArtCache, MapPose, MinimapTracker, P2};
use heading::HeadingOut;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const GAME_LOG_SOURCE: &str = "game log";

/// What the overlay should draw right now.
#[derive(Clone, Debug, Default)]
pub struct OverlayFrame {
    /// The game's client area (screen px).
    pub client: Rect,
    pub visible: bool,
    /// The layout's number.
    pub layout: Option<i64>,
    pub pose: Option<MapPose>,
    pub player: Option<P2>,
    pub player_fresh: bool,
    pub difficulty: Option<String>,
    pub status: String,
    pub mini: bool,
    pub floor: usize,
    pub heading: Option<f64>,
    pub big: bool,
    pub velocity: Option<P2>,
    pub stamp: Option<Instant>,
    pub camera: Option<f64>,
}

type FrameFn = Box<dyn Fn(OverlayFrame) + Send + Sync>;
type NotifyFn = Box<dyn Fn(&str, Option<&str>) + Send + Sync>;
type PlainFn = Box<dyn Fn() + Send + Sync>;

/// What both threads (and the hook thread) share.
pub struct Shared {
    pub running: AtomicBool,
    pub settings: Arc<RwLock<Settings>>,
    window: Mutex<(bool, bool, Rect, usize)>,
    pub in_run: AtomicBool,
    pub key_showing: AtomicBool,
    pub mini_showing: AtomicBool,
    pub heading_paused: AtomicBool,
    pub heading_reads: AtomicI64,
    pub toggled: AtomicBool,
    pub in_egg_heist: AtomicBool,
    pub magnifier_said: AtomicBool,
    pub heading: Mutex<HeadingOut>,
    corner: Mutex<Corner>,
    corner_ready: Condvar,
    on_frame: Mutex<Vec<FrameFn>>,
    on_status: Mutex<Vec<PlainFn>>,
    on_notify: Mutex<Vec<NotifyFn>>,
    on_capture_fallback: Mutex<Vec<PlainFn>>,
    on_language: Mutex<Vec<PlainFn>>,
}

#[derive(Default)]
struct Corner {
    pic: Option<Mat>,
    at: Option<Instant>,
    seq: u64,
    tracked: u64,
}

/// A corner copy counts as fresh this long (the heading loop copies one every ~16 ms).
const CORNER_FRESH: Duration = Duration::from_millis(40);

impl Shared {
    /// (found, in front, client, handle).
    pub fn window(&self) -> (bool, bool, Rect, usize) {
        *self.window.lock().unwrap()
    }

    pub fn put_corner(&self, m: Mat, at: Instant) {
        let mut c = self.corner.lock().unwrap();
        c.pic = Some(m);
        c.at = Some(at);
        c.seq += 1;
        self.corner_ready.notify_all();
    }

    /// A copy of the heading loop's latest corner grab when it is fresh and of this size.
    pub fn take_corner(&self, corner: Rect, copied_at: &mut Instant) -> Option<Mat> {
        let mut c = self.corner.lock().unwrap();
        let at = c.at?;
        let pic = c.pic.as_ref()?;
        if at.elapsed() > CORNER_FRESH || pic.w != corner.2 as usize || pic.h != corner.3 as usize {
            return None;
        }
        let m = pic.clone();
        c.tracked = c.seq;
        *copied_at = at;
        Some(m)
    }

    /// The heading loop has copied a corner the tracking hasn't used yet.
    pub fn new_corner(&self) -> bool {
        let c = self.corner.lock().unwrap();
        c.pic.is_some() && c.seq != c.tracked && c.at.is_some_and(|t| t.elapsed() <= CORNER_FRESH)
    }

    fn wait_corner(&self, ms: u64) {
        let c = self.corner.lock().unwrap();
        let _ = self.corner_ready.wait_timeout(c, Duration::from_millis(ms.clamp(1, 15)));
    }

    /// Every overlay frame goes out here: nothing is shown outside Egg Heist.
    pub fn emit(&self, mut f: OverlayFrame) {
        if f.visible && !self.in_egg_heist.load(Ordering::SeqCst) {
            f.visible = false;
        }
        for cb in self.on_frame.lock().unwrap().iter() {
            cb(f.clone());
        }
    }

    pub fn emit_status(&self) {
        for cb in self.on_status.lock().unwrap().iter() {
            cb();
        }
    }

    pub fn notify(&self, text: &str, difficulty: Option<&str>) {
        for cb in self.on_notify.lock().unwrap().iter() {
            cb(text, difficulty);
        }
    }

    pub fn emit_capture_fallback(&self) {
        for cb in self.on_capture_fallback.lock().unwrap().iter() {
            cb();
        }
    }

    fn emit_language(&self) {
        for cb in self.on_language.lock().unwrap().iter() {
            cb();
        }
    }
}

/// Count and average / max duration of something since the last report.
#[derive(Default)]
pub struct PerfStat {
    n: u64,
    total: Duration,
    max: Duration,
}

impl PerfStat {
    pub fn add(&mut self, d: Duration) {
        self.n += 1;
        self.total += d;
        self.max = self.max.max(d);
    }

    pub fn report(&mut self) -> String {
        let r = if self.n == 0 {
            "-".to_string()
        } else {
            format!("{:.1}/s avg {:.1} max {:.0} ms", self.n as f64 / 10.0, self.total.as_secs_f64() * 1000.0 / self.n as f64, self.max.as_secs_f64() * 1000.0)
        };
        *self = PerfStat::default();
        r
    }
}

/// What the status display reads (tray tooltip, settings window).
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub game_found: bool,
    pub client: Rect,
    pub detected_difficulty: Option<String>,
    pub layout: Option<i64>,
    pub layout_source: String,
    pub last_match_info: String,
    pub last_ocr_text: String,
    pub ocr_language: String,
    pub ocr_missing_language: Option<String>,
    pub minimap_score: f64,
    pub minimap_info: String,
    pub identifying_layout: bool,
    pub game_log_place: Option<Place>,
    pub in_run: bool,
    pub using_window_capture: bool,
}

/// The main loop's state.
pub struct Core {
    pub sh: Arc<Shared>,
    pub db: Arc<LayoutDb>,
    hook: Arc<KeyHook>,
    pub ocr: OcrService,
    pub difficulty: DifficultyDetector,
    pub arts: Arc<MapArtCache>,
    features: Arc<ArtFeatureCache>,
    pub matcher: MapMatcher,
    pub doors: DoorLocator,
    pub island_tracker: Arc<Mutex<IslandTracker>>,
    pub refiner: FloorRefiner,
    pub minimap: MinimapTracker,
    pub game: GameWindow,
    pub game_log: GameLog,
    pad: Gamepad,
    pub grabber: grab::Grabber,
    reset_requested: bool,
    game_gone_since: Option<Instant>,
    debug_cleared: bool,
    toggle_mode_was: bool,
    pad_polled: bool,
    pad_was_down: bool,
    pub game_found: bool,
    pub current_layout: Option<Layout>,
    pub layout_source: String,
    pub last_match_info: String,
    pub detected_difficulty: Option<String>,
    pub difficulty_doubted: bool,
    pub player: Option<P2>,
    pub player_time: Option<Instant>,
    pub last_pose: Option<MapPose>,
    pub last_pose_time: Option<Instant>,
    pub minimap_time: Option<Instant>,
    pub outside_streak: i32,
    pub inside_streak: i32,
    pub on_island: bool,
    pub island_player: Option<P2>,
    pub island_time: Option<Instant>,
    pub sanctum_ok_time: Option<Instant>,
    pub island_pose: Option<MapPose>,
    pub last_island_search: Option<Instant>,
    pub island_probation: i32,
    pub island_arrival: Option<String>,
    pub island_arrival_tries: i32,
    pub island_spawn: Option<P2>,
    pub island_door: Option<P2>,
    pub island_door_kind: Option<String>,
    pub island_search: Option<JoinHandle<Option<P2>>>,
    pub island_search_run: Option<String>,
    pub portal_icon: Option<Option<Mat>>,
    pub last_landmark: P2,
    pub last_landmark_at: Option<Instant>,
    pub odo_prev: Option<MatF>,
    pub last_slow_tick: Option<Instant>,
    pub hud_visible: bool,
    pub hud_last_seen: Option<Instant>,
    pub hud_template: Option<Option<Mat>>,
    pub last_map_page_look: Option<Instant>,
    pub last_minimap_log: Option<Instant>,
    pub misfit_since: Option<Instant>,
    pub guesses: guess::Guesses,
    pub guess_task: Option<()>,
    pub timer_template: Option<Option<Mat>>,
    pub red_timer: Option<Option<(Mat, Mat)>>,
    pub last_run_seen: Option<Instant>,
    pub last_no_run_check: Instant,
    pub last_timer_score: f64,
    pub last_timer_seen: bool,
    pub challenge_area: Option<String>,
    pub challenge_area_at: Instant,
    pub next_challenge_read: Instant,
    pub party_seen: Option<Instant>,
    pub held_since: Option<Instant>,
    pub last_match_log: Option<Instant>,
    pub last_wide_search: Option<Instant>,
    pub last_big_fix: Option<Instant>,
    pub fit: Option<MapPose>,
    pub fit_o: Option<P2>,
    pub fit_b: Option<P2>,
    pub prev_door_o: Option<P2>,
    pub fit_at: Option<Instant>,
    pub prev_gray: Option<MatF>,
    pub prev_gray_pose: Option<MapPose>,
    pub last_debug_save: Option<Instant>,
    pub perf_track: PerfStat,
    pub perf_locate: PerfStat,
    pub perf_ocr: PerfStat,
}

fn utc_now_text() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// Minutes since a time saved as text (C#'s DateTime round-trip), or None when it can't be read.
fn minutes_since(text: &str) -> Option<f64> {
    let t = chrono::DateTime::parse_from_rfc3339(text).ok()?;
    Some((chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds() as f64 / 60.0)
}

impl Core {
    pub fn settings(&self) -> RwLockReadGuard<'_, Settings> {
        self.sh.settings.read().unwrap()
    }

    pub fn settings_mut(&self) -> RwLockWriteGuard<'_, Settings> {
        self.sh.settings.write().unwrap()
    }

    pub fn island_tracker(&self) -> MutexGuard<'_, IslandTracker> {
        self.island_tracker.lock().unwrap()
    }

    /// A whole-island search may hold the tracker for a second: not waited for here.
    pub fn island_tracker_tracking(&self) -> bool {
        self.island_tracker.try_lock().is_ok_and(|t| t.tracking())
    }

    pub fn emit(&self, f: OverlayFrame) {
        self.sh.emit(f);
    }

    /// The layout to show: the one picked by hand, else the run's.
    pub fn shown_layout(&self) -> Option<Layout> {
        let forced = self.settings().forced_layout;
        if forced != 0 { self.db.by_number(forced).cloned() } else { self.current_layout.clone() }
    }

    /// Is this an Egg Heist area? With the game's log readable, only while it has the player on the island or in a
    /// Sanctum; without it the screen decides.
    pub fn in_egg_heist(&self) -> bool {
        !(self.settings().use_game_log && self.game_log.available) || matches!(self.game_log.place, Place::Island | Place::Sanctum)
    }

    /// In a run: the run timer was seen in the last 15 s (it dips over bright grass on the island).
    pub fn in_run(&self) -> bool {
        self.in_egg_heist() && self.last_run_seen.is_some_and(|t| t.elapsed() < Duration::from_secs(15))
    }

    /// Where the game's log says the player is (Unknown without it).
    pub fn game_log_place(&self) -> Place {
        if self.settings().use_game_log { self.game_log.place } else { Place::Unknown }
    }

    pub fn effective_difficulty(&self) -> Option<String> {
        let d = self.settings().difficulty.clone();
        if d == "Auto" { self.detected_difficulty.clone() } else { Some(d) }
    }

    pub fn all_candidates(&self) -> Vec<Layout> {
        let forced = self.settings().forced_layout;
        if forced != 0
            && let Some(l) = self.db.by_number(forced)
        {
            return vec![l.clone()];
        }
        if self.layout_source == GAME_LOG_SOURCE
            && let Some(l) = &self.current_layout
        {
            return vec![l.clone()];
        }
        self.difficulty_candidates()
    }

    pub fn candidates(&self) -> Vec<Layout> {
        let forced = self.settings().forced_layout;
        if forced != 0
            && let Some(l) = self.db.by_number(forced)
        {
            return vec![l.clone()];
        }
        if let Some(l) = &self.current_layout {
            return vec![l.clone()];
        }
        self.difficulty_candidates()
    }

    fn difficulty_candidates(&self) -> Vec<Layout> {
        let c: Vec<Layout> = self.db.candidates(self.effective_difficulty().as_deref()).into_iter().cloned().collect();
        if c.is_empty() { self.db.candidates(None).into_iter().cloned().collect() } else { c }
    }

    /// Lets go of every layout's maps but the ones kept.
    pub fn release_maps(&mut self, keep: impl Fn(i64) -> bool + Copy) {
        self.arts.trim(keep);
        self.refiner.trim(keep);
        self.minimap.release(keep);
        self.features.trim(keep);
    }

    /// Forget the current run's layout (new run, or a wrong lock).
    pub fn reset_run(&mut self) {
        self.reset_requested = true;
    }

    pub fn grab(&mut self, sub: Rect, still: bool) -> Mat {
        let recordable = self.settings().recordable_overlay;
        let sh = self.sh.clone();
        self.grabber.grab(&sh, sub, still, recordable)
    }

    fn prepare_features(&self, l: &Layout) {
        let (f, l) = (self.features.clone(), l.clone());
        std::thread::spawn(move || f.prepare([&l]));
    }

    pub fn lock_layout(&mut self, l: &Layout, source: &str) {
        if self.current_layout.as_ref().is_some_and(|c| c.number == l.number) {
            return;
        }
        if self.current_layout.is_some() {
            // a different layout = a new run: forget everything tied to the old one
            self.last_pose = None;
            self.player = None;
            self.minimap.reset();
            self.prev_gray = None;
            self.prev_gray_pose = None;
        }
        self.current_layout = Some(l.clone());
        self.layout_source = source.to_string();
        self.misfit_since = None;
        // the layout says the difficulty when it belongs to just one: corrects a stale reading
        if self.settings().difficulty == "Auto" && l.difficulties.len() == 1 && Some(&l.difficulties[0]) != self.detected_difficulty.as_ref() {
            let d = l.difficulties[0].clone();
            crate::log::write(&format!("difficulty: {d} (from {}; was {})", l.title(), self.detected_difficulty.as_deref().unwrap_or("unknown")));
            self.detected_difficulty = Some(d.clone());
            self.sh.notify(&lang::f("{0} Mode chosen.", &[&lang::difficulty(Some(&d))]), Some(&d));
        }
        {
            let run_id = self.game_log.run_id().map(str::to_string);
            let mut s = self.settings_mut();
            s.run_layout = l.number;
            s.run_layout_time = utc_now_text();
            s.run_id = run_id;
            s.save();
        }
        crate::log::write(&format!("layout locked: {} via {source}", l.number));
        let n = l.number;
        self.release_maps(move |k| k == n);
        self.prepare_features(l);
        self.sh.notify(&lang::f("Sanctum found #{0}", &[&l.number]), None);
        self.sh.emit_status();
    }

    fn do_reset(&mut self) {
        self.reset_requested = false;
        self.sh.toggled.store(false, Ordering::SeqCst); // a new run starts with the overlay off in toggle mode
        self.current_layout = None;
        self.last_pose = None;
        self.player = None;
        self.minimap.reset();
        self.on_island = false;
        self.island_player = None;
        self.island_pose = None;
        {
            let mut t = self.island_tracker();
            t.reset();
            t.release();
        }
        self.clear_guesses();
        self.release_maps(|_| false);
        {
            let mut s = self.settings_mut();
            s.run_layout = 0;
            s.save();
        }
        self.sh.emit_status();
    }

    fn status(&self) -> Status {
        Status {
            game_found: self.game_found,
            client: self.game.client,
            detected_difficulty: self.detected_difficulty.clone(),
            layout: self.current_layout.as_ref().map(|l| l.number),
            layout_source: self.layout_source.clone(),
            last_match_info: self.last_match_info.clone(),
            last_ocr_text: self.difficulty.last_text.clone(),
            ocr_language: self.ocr.language.clone(),
            ocr_missing_language: self.ocr.missing_language.clone(),
            minimap_score: self.minimap.last_score,
            minimap_info: self.minimap.last_info.clone(),
            identifying_layout: self.identifying_layout(),
            game_log_place: Some(self.game_log_place()),
            in_run: self.in_run(),
            using_window_capture: self.settings().recordable_overlay && !self.grabber.print_window_failed,
        }
    }

    fn update_window_share(&self) {
        *self.sh.window.lock().unwrap() = (self.game_found, self.game.is_foreground(), self.game.client, self.game.handle);
        self.sh.in_egg_heist.store(self.in_egg_heist(), Ordering::SeqCst);
    }

    /// The main loop. `status`: refreshed for the display after every tick.
    fn run_loop(mut self, status: Arc<Mutex<Status>>, commands: Arc<Mutex<Vec<Command>>>) {
        let t0 = Instant::now();
        let ms = || t0.elapsed().as_millis() as u64;
        let (mut next_window, mut next_ocr, mut next_minimap, mut next_game_log, mut next_lang_check, mut next_perf_log) = (0u64, 0u64, 0u64, 0u64, 5000u64, 10000u64);
        let mut was_showing = false;
        while self.sh.running.load(Ordering::SeqCst) {
            let now = ms();
            for c in std::mem::take(&mut *commands.lock().unwrap()) {
                match c {
                    Command::ResetRun => self.reset_run(),
                    Command::GameLanguageChanged => {
                        let o = OcrService::new(lang::ocr_tag(&lang::game_code()));
                        crate::log::write(&format!(
                            "game language {}, OCR {}{}",
                            lang::game_code(),
                            o.language,
                            o.missing_language.as_ref().map_or(String::new(), |m| format!(" ({m} not installed)"))
                        ));
                        self.difficulty = DifficultyDetector::default();
                        self.ocr = o;
                    }
                }
            }
            if self.reset_requested {
                self.do_reset();
            }
            self.tick(now, &ms, &mut was_showing, &mut next_window, &mut next_ocr, &mut next_minimap, &mut next_game_log, &mut next_lang_check, &mut next_perf_log);
            *status.lock().unwrap() = self.status();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn tick(
        &mut self,
        now: u64,
        ms: &dyn Fn() -> u64,
        was_showing: &mut bool,
        next_window: &mut u64,
        next_ocr: &mut u64,
        next_minimap: &mut u64,
        next_game_log: &mut u64,
        next_lang_check: &mut u64,
        next_perf_log: &mut u64,
    ) {
        let sleep = |m: u64| std::thread::sleep(Duration::from_millis(m));
        let s = self.settings().clone();
        // debug frames turned off: the ones saved go
        if s.save_debug_frames {
            self.debug_cleared = false;
        } else if !self.debug_cleared {
            self.debug_cleared = true;
            let dir = crate::settings::dir().join("debug");
            if dir.exists() {
                std::thread::spawn(move || match std::fs::remove_dir_all(&dir) {
                    Ok(()) => crate::log::write("debug frames off: removed the saved ones"),
                    Err(e) => crate::log::write(&format!("debug frames: {e}")),
                });
            }
        }
        self.hook.poll();
        if s.toggle_mode != self.toggle_mode_was {
            self.toggle_mode_was = s.toggle_mode;
            self.sh.toggled.store(false, Ordering::SeqCst);
        }
        // the controller button works like the overlay key; read only with the game in front and in Egg Heist
        let mut pad_down = false;
        let pad_live = s.hold_button != 0 && self.game.is_foreground() && self.in_egg_heist();
        if pad_live {
            self.pad.poll();
            pad_down = self.pad.is_down(s.hold_button as i32);
            if pad_down && !self.pad_was_down && self.pad_polled && s.toggle_mode {
                self.sh.toggled.fetch_xor(true, Ordering::SeqCst);
            }
        }
        self.pad_was_down = pad_down;
        self.pad_polled = pad_live;
        // the game's language can be changed in game: follow it when set to Auto
        if now >= *next_lang_check {
            *next_lang_check = now + 5000;
            if s.game_language == "Auto"
                && let Some(gl) = lang::read_game_language()
                && Some(&gl) != lang::detected_game_code().as_ref()
            {
                crate::log::write(&format!("game language changed to {gl}"));
                self.sh.emit_language();
            }
        }
        if now >= *next_perf_log {
            *next_perf_log = now + 10000;
            crate::log::write(&format!(
                "perf: track {} · locate {} · ocr {} · capture {}{} · run timer {:.2}{}",
                self.perf_track.report(),
                self.perf_locate.report(),
                self.perf_ocr.report(),
                self.grabber.source,
                if self.grabber.source.starts_with("shared") { format!(" ({:.1} ms)", crate::shared_capture::copy_ms()) } else { String::new() },
                self.last_timer_score,
                if self.in_run() { " (in a run)" } else { "" }
            ));
        }
        if s.use_game_log && now >= *next_game_log {
            *next_game_log = now + 250;
            self.game_log.poll();
            for (place, new_run) in std::mem::take(&mut self.game_log.changes) {
                self.on_game_log_changed(place, new_run);
            }
        }
        if now >= *next_window {
            *next_window = now + 1000;
            let found = self.game.refresh(&s.window_title);
            if found != self.game_found {
                self.game_found = found;
                let c = self.game.client;
                crate::log::write(&if found { format!("game window found: {}x{} at {},{}", c.2, c.3, c.0, c.1) } else { "game window lost".into() });
                self.sh.emit_status();
            }
        }
        self.update_window_share();
        if !self.game_found {
            if *was_showing {
                self.emit(OverlayFrame::default());
                *was_showing = false;
            }
            self.sh.toggled.store(false, Ordering::SeqCst);
            // the game closed for a minute: the run is over, let its maps go (a minimized game isn't closed)
            if self.game.is_minimized() {
                self.game_gone_since = None;
            } else if self.game_gone_since.is_none() {
                self.game_gone_since = Some(Instant::now());
            }
            if self.game_gone_since.is_some_and(|t| t.elapsed() > Duration::from_secs(60)) && (self.current_layout.is_some() || self.arts.loaded() > 0) && !self.reset_requested {
                crate::log::write("game closed for a minute: forgetting the run and its maps");
                self.reset_requested = true;
            }
            return sleep(400);
        }
        self.game_gone_since = None;
        if !self.in_egg_heist() {
            self.sh.toggled.store(false, Ordering::SeqCst);
        }
        let held = if s.toggle_mode { self.sh.toggled.load(Ordering::SeqCst) } else { self.hook.is_down() || pad_down };
        let show = held && self.game.is_foreground() && self.in_egg_heist();
        self.sh.key_showing.store(show, Ordering::SeqCst);
        if show && !*was_showing {
            self.sh.mini_showing.store(false, Ordering::SeqCst);
            self.held_since = Some(Instant::now());
            let what = if pad_down { format!("controller {}", crate::input::button_name(s.hold_button as i32)) } else { crate::input::key_name(self.hook.key()) };
            crate::log::write(&format!("overlay key down ({what})"));
            // with no on-screen map the facing isn't read until the key shows something: wait for two fresh reads
            if self.sh.heading_paused.load(Ordering::SeqCst) && self.in_run() {
                let want = self.sh.heading_reads.load(Ordering::SeqCst) + 2;
                for _ in 0..25 {
                    if self.sh.heading_reads.load(Ordering::SeqCst) >= want {
                        break;
                    }
                    sleep(2);
                }
            }
        }
        if show {
            // the in-game map open: one window capture for the full-map overlay; closed: the big map
            let c0 = self.game.client;
            let big_ok = !self.on_island && (s.forced_layout != 0 || self.current_layout.is_some()) && self.in_run();
            let mut map_open = !big_ok && self.doors.last_map_open;
            let whole = (0, 0, c0.2, c0.3);
            if !map_open {
                let head = self.grab(grab::map_title_rect(c0.2, c0.3), true);
                // no picture yet is not "the map is closed": look again next tick
                if self.grabber.no_picture {
                    *was_showing = true;
                    return sleep(10);
                }
                map_open = self.map_screen_open(&head);
                if !map_open {
                    crate::shared_capture::forget(c0.2, c0.3, whole);
                }
            }
            if map_open {
                let frame = self.grab(whole, true);
                if self.grabber.no_picture {
                    *was_showing = true;
                    return sleep(10);
                }
                self.minimap.hold_still();
                self.show_map(Some(&frame));
            } else if !big_ok {
                self.show_map(None);
            } else {
                // a fix per new corner copy
                if self.sh.new_corner() || self.last_big_fix.is_none_or(|t| t.elapsed() >= Duration::from_millis(40)) {
                    self.last_big_fix = Some(Instant::now());
                    self.track_minimap(None);
                }
                self.show_big();
            }
            *was_showing = true;
            // next frame ~16 ms after this one started
            let spent = ms().saturating_sub(now);
            return sleep(16u64.saturating_sub(spent).max(1));
        }
        if *was_showing {
            let f = OverlayFrame { client: self.game.client, layout: self.current_layout.as_ref().map(|l| l.number), difficulty: self.effective_difficulty(), ..Default::default() };
            self.emit(f);
            *was_showing = false;
            let c = self.game.client;
            crate::shared_capture::forget(c.2, c.3, (0, 0, c.2, c.3));
        }
        // The party screen only exists outside a run: read it often there and never during one (also with the game
        // behind another window; not while minimized).
        let in_run_now = self.in_run() || matches!(self.game_log_place(), Place::Island | Place::Sanctum);
        if now >= *next_ocr && s.difficulty == "Auto" && self.ocr.available() && !in_run_now && !self.game.is_minimized() {
            let t = Instant::now();
            let on_party = self.read_party_screen();
            self.perf_ocr.add(t.elapsed());
            *next_ocr = now + if on_party { 500 } else if self.game.is_foreground() { 1000 } else { 1500 };
        }
        // the run's maps go once you've left, whatever is in front
        if !self.in_egg_heist() && (self.current_layout.is_some() || self.arts.loaded() > 0) && !self.reset_requested && self.last_run_seen.is_none_or(|t| t.elapsed() > Duration::from_secs(90)) {
            crate::log::write("out of Egg Heist for 90 s: letting the run's maps go");
            self.reset_run();
        }
        if !self.game.is_foreground() {
            self.hide_mini();
            return sleep(250);
        }
        if !self.in_egg_heist() && self.sh.mini_showing.load(Ordering::SeqCst) {
            crate::log::write("left Egg Heist: hiding the on-screen map");
            self.hide_mini();
        }
        // faster while the on-screen map follows the player, and in a run with the layout still unknown
        let following = s.mini_map_mode != "Off" && (self.current_layout.is_some() || self.on_island);
        let fast = following || self.in_run();
        let new_corner = self.sh.new_corner();
        if now >= *next_minimap && s.track_minimap && self.in_egg_heist() && (!fast || new_corner || now >= *next_minimap + 40) {
            *next_minimap = now + if fast { 25 } else { 350 };
            let t = Instant::now();
            self.track_minimap(None);
            self.show_mini();
            self.perf_track.add(t.elapsed());
        }
        // sleep until the next fix is due, or (while fast) until a new corner copy lands
        let t = ms();
        let due = if t < *next_minimap { *next_minimap } else { *next_minimap + 40 };
        if !self.in_egg_heist() {
            sleep(100);
        } else if fast && s.track_minimap {
            self.sh.wait_corner(due.saturating_sub(t));
        } else {
            sleep(15);
        }
    }
}

enum Command {
    ResetRun,
    GameLanguageChanged,
}

/// The engine: started once, runs until dropped.
pub struct Engine {
    sh: Arc<Shared>,
    status: Arc<Mutex<Status>>,
    commands: Arc<Mutex<Vec<Command>>>,
    core: Option<Core>,
    threads: Vec<JoinHandle<()>>,
}

impl Engine {
    /// `player_log`: the game's Player.log (None: where the game keeps it).
    pub fn new(db: Arc<LayoutDb>, settings: Arc<RwLock<Settings>>, hook: Arc<KeyHook>, player_log: Option<std::path::PathBuf>) -> Engine {
        let sh = Arc::new(Shared {
            running: AtomicBool::new(false),
            settings: settings.clone(),
            window: Mutex::new((false, false, (0, 0, 0, 0), 0)),
            in_run: AtomicBool::new(false),
            key_showing: AtomicBool::new(false),
            mini_showing: AtomicBool::new(false),
            heading_paused: AtomicBool::new(false),
            heading_reads: AtomicI64::new(0),
            toggled: AtomicBool::new(false),
            in_egg_heist: AtomicBool::new(true),
            magnifier_said: AtomicBool::new(false),
            heading: Mutex::new(HeadingOut::default()),
            corner: Mutex::new(Corner::default()),
            corner_ready: Condvar::new(),
            on_frame: Mutex::new(vec![]),
            on_status: Mutex::new(vec![]),
            on_notify: Mutex::new(vec![]),
            on_capture_fallback: Mutex::new(vec![]),
            on_language: Mutex::new(vec![]),
        });
        let ocr = OcrService::new(lang::ocr_tag(&lang::game_code()));
        if !ocr.available() {
            crate::log::write("Windows OCR is not available: difficulty must be set by hand");
        }
        let arts = Arc::new(MapArtCache::new(db.clone()));
        let features = Arc::new(ArtFeatureCache::new(arts.clone()));
        let icons = db.root.join("icons");
        let read = |n: &str| vision::imread(&icons.join(n)).map(|m| if m.c == 4 { crate::cv::bgra2bgr(&m) } else { m });
        let doors = DoorLocator::new(arts.clone(), read("map_header.png"), read("screen_door_orange.png"), read("screen_door_blue.png"));
        // toggle mode: only a press with the game in front and in Egg Heist flips it
        {
            let sh2 = sh.clone();
            hook.on_state_changed(move |down| {
                let (_, fg, _, _) = sh2.window();
                let toggle = sh2.settings.read().map(|s| s.toggle_mode).unwrap_or(false);
                if down && toggle && fg && sh2.in_egg_heist.load(Ordering::SeqCst) {
                    sh2.toggled.fetch_xor(true, Ordering::SeqCst);
                }
            });
        }
        let use_log = settings.read().unwrap().use_game_log;
        let mut game_log = GameLog::new(player_log);
        if use_log {
            game_log.poll();
            game_log.changes.clear(); // what it already held is the starting point, not news
        }
        let mut core = Core {
            sh: sh.clone(),
            db: db.clone(),
            hook,
            ocr,
            difficulty: DifficultyDetector::default(),
            arts: arts.clone(),
            features: features.clone(),
            matcher: MapMatcher::new(features),
            doors,
            island_tracker: Arc::new(Mutex::new(IslandTracker::new(arts.clone()))),
            refiner: FloorRefiner::new(arts.clone()),
            minimap: MinimapTracker::new(arts),
            game: GameWindow::default(),
            game_log,
            pad: Gamepad::default(),
            grabber: grab::Grabber::default(),
            reset_requested: false,
            game_gone_since: None,
            debug_cleared: false,
            toggle_mode_was: false,
            pad_polled: false,
            pad_was_down: false,
            game_found: false,
            current_layout: None,
            layout_source: String::new(),
            last_match_info: String::new(),
            detected_difficulty: None,
            difficulty_doubted: false,
            player: None,
            player_time: None,
            last_pose: None,
            last_pose_time: None,
            minimap_time: None,
            outside_streak: 0,
            inside_streak: 0,
            on_island: false,
            island_player: None,
            island_time: None,
            sanctum_ok_time: None,
            island_pose: None,
            last_island_search: None,
            island_probation: 0,
            island_arrival: None,
            island_arrival_tries: 0,
            island_spawn: None,
            island_door: None,
            island_door_kind: None,
            island_search: None,
            island_search_run: None,
            portal_icon: None,
            last_landmark: (0.0, 0.0),
            last_landmark_at: None,
            odo_prev: None,
            last_slow_tick: None,
            hud_visible: false,
            hud_last_seen: None,
            hud_template: None,
            last_map_page_look: None,
            last_minimap_log: None,
            misfit_since: None,
            guesses: guess::Guesses::default(),
            guess_task: None,
            timer_template: None,
            red_timer: None,
            last_run_seen: None,
            last_no_run_check: Instant::now(),
            last_timer_score: 0.0,
            last_timer_seen: false,
            challenge_area: None,
            challenge_area_at: Instant::now(),
            next_challenge_read: Instant::now(),
            party_seen: None,
            held_since: None,
            last_match_log: None,
            last_wide_search: None,
            last_big_fix: None,
            fit: None,
            fit_o: None,
            fit_b: None,
            prev_door_o: None,
            fit_at: None,
            prev_gray: None,
            prev_gray_pose: None,
            last_debug_save: None,
            perf_track: PerfStat::default(),
            perf_locate: PerfStat::default(),
            perf_ocr: PerfStat::default(),
        };
        core.toggle_mode_was = settings.read().unwrap().toggle_mode;
        // restarted mid-run: pick the run's layout back up (runs last at most 20 minutes)
        {
            let s = settings.read().unwrap();
            let same_run = s.run_id.is_none() || core.game_log.run_id().is_none() || s.run_id.as_deref() == core.game_log.run_id();
            if !same_run {
                crate::log::write("game log: a different run than the saved layout's, not restoring it");
            }
            let age = minutes_since(&s.run_layout_time);
            if same_run
                && s.run_layout != 0
                && let (Some(a), Some(saved)) = (age.filter(|a| *a < 25.0), db.by_number(s.run_layout))
            {
                core.current_layout = Some(saved.clone());
                core.layout_source = "saved run".into();
                crate::log::write(&format!("layout restored: {} (saved {a:.0} min ago)", saved.number));
            }
        }
        Engine { sh, status: Arc::new(Mutex::new(Status::default())), commands: Arc::new(Mutex::new(vec![])), core: Some(core), threads: vec![] }
    }

    pub fn on_frame(&self, f: impl Fn(OverlayFrame) + Send + Sync + 'static) {
        self.sh.on_frame.lock().unwrap().push(Box::new(f));
    }

    pub fn on_status_changed(&self, f: impl Fn() + Send + Sync + 'static) {
        self.sh.on_status.lock().unwrap().push(Box::new(f));
    }

    /// A short notice for the player: text, and the difficulty it's about (None for a Sanctum).
    pub fn on_notify(&self, f: impl Fn(&str, Option<&str>) + Send + Sync + 'static) {
        self.sh.on_notify.lock().unwrap().push(Box::new(f));
    }

    /// Window capture failed: the overlay can hide itself from captures again.
    pub fn on_capture_fallback(&self, f: impl Fn() + Send + Sync + 'static) {
        self.sh.on_capture_fallback.lock().unwrap().push(Box::new(f));
    }

    /// The game's own language setting changed (with the game language on Auto).
    pub fn on_language_detected(&self, f: impl Fn() + Send + Sync + 'static) {
        self.sh.on_language.lock().unwrap().push(Box::new(f));
    }

    pub fn start(&mut self) {
        let Some(core) = self.core.take() else { return };
        self.sh.running.store(true, Ordering::SeqCst);
        if let Some(l) = &core.current_layout {
            core.prepare_features(l);
        }
        let icons = core.db.root.join("icons");
        let (status, commands) = (self.status.clone(), self.commands.clone());
        self.threads.push(std::thread::Builder::new().name("SanctumHelper engine".into()).spawn(move || core.run_loop(status, commands)).unwrap());
        let sh = self.sh.clone();
        self.threads.push(std::thread::Builder::new().name("SanctumHelper heading".into()).spawn(move || heading::run(sh, icons)).unwrap());
    }

    /// Forget the current run's layout (new run, or a wrong lock).
    pub fn reset_run(&self) {
        self.commands.lock().unwrap().push(Command::ResetRun);
    }

    /// The game language setting changed: read the party screen in the new one.
    pub fn game_language_changed(&self) {
        self.commands.lock().unwrap().push(Command::GameLanguageChanged);
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    /// The latest facing and camera (for the overlay to animate between frames).
    pub fn heading(&self) -> HeadingOut {
        *self.sh.heading.lock().unwrap()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.sh.running.store(false, Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_times_and_party_visits() {
        assert!(minutes_since(&utc_now_text()).unwrap().abs() < 0.1);
        assert_eq!(minutes_since("0001-01-01T00:00:00"), None);
        assert!(map::new_party_visit(None, Instant::now()));
        let t = Instant::now();
        assert!(!map::new_party_visit(Some(t), t + Duration::from_secs(2)));
        assert!(map::new_party_visit(Some(t), t + Duration::from_secs(4)));
        let mut p = PerfStat::default();
        assert_eq!(p.report(), "-");
        p.add(Duration::from_millis(4));
        assert!(p.report().starts_with("0.1/s avg 4.0"));
    }
}
