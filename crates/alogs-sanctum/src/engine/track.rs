//! Following the player on the minimap: in the Sanctum against the locked layout, on the island against the island
//! art (from the arrival spots, a portal icon, a whole-island search in the background, and the minimap's own scroll
//! between frames). Port of Engine.cs's TrackMinimap, MoveOnIsland, MinimapStep, OnGameLogChanged, RememberEntryDoor,
//! ExpectedExitDoor, SanctumEntry and IslandLandmark (Dev switches at their defaults: exit-same-door on).

use super::heading::{corner_rect, in_corner};
use super::{Core, GAME_LOG_SOURCE, Place};
use crate::cv::{self, Interp, Mat, MatF};
use crate::cv2;
use crate::detectors::{self, Anchor};
use crate::layout_db::Layout;
use crate::vision::{self, P2};
use std::time::{Duration, Instant};

fn since(t: Option<Instant>) -> f64 {
    t.map_or(f64::MAX, |t| t.elapsed().as_secs_f64())
}

pub fn door_name(kind: Option<&str>) -> &'static str {
    match kind {
        Some("entrance_main") => "orange-door",
        Some("entrance_side") => "blue-door",
        _ => "door",
    }
}

fn manhattan(a: P2, b: P2) -> f64 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}

impl Core {
    /// One fix: the HUD corner (from the heading loop's latest copy when fresh, else copied now), the run timer and
    /// HUD checks, then the Sanctum or island position. `full`: a whole-window frame already at hand.
    pub fn track_minimap(&mut self, full: Option<&Mat>) {
        let layout = self.shown_layout();
        let c = self.game.client;
        let (cw, ch) = (c.2 as usize, c.3 as usize);
        let ui = vision::ui_scale(cw, ch);
        let region = self.settings().minimap_region;
        let rc = detectors::to_client(region, Anchor::TopLeft, cw, ch);
        if rc.2 < 40 {
            return;
        }
        let corner = corner_rect(rc, cw, ch);
        let mut grabbed_at = Instant::now();
        let top = match full {
            Some(f) => f.roi(corner.0 as usize, corner.1 as usize, corner.2 as usize, corner.3 as usize),
            None => match self.sh.take_corner(corner, &mut grabbed_at) {
                Some(m) => m,
                None if crate::shared_capture::app_present() => {
                    let still = self.sh.key_showing.load(std::sync::atomic::Ordering::SeqCst);
                    let m = self.grab(corner, still);
                    // no picture from the app yet: an empty frame says nothing about the screen, and read as "run timer
                    // and HUD gone" it hid the on-screen map until the next picture (the map flickered, 16 Nov)
                    if self.grabber.no_picture {
                        return;
                    }
                    m
                }
                None => super::grab::screen_part(&self.sh, c, corner),
            },
        };
        // "N readings in a row" counts were tuned at ~20 ticks a second: they only advance on ticks 45 ms apart
        let slow_tick = self.last_slow_tick.is_none_or(|t| t.elapsed() >= Duration::from_millis(45));
        if slow_tick {
            self.last_slow_tick = Some(Instant::now());
        }
        {
            let small = cv::resize(&top, 0, 0, 1.0 / ui, 1.0 / ui, Interp::Area);
            self.update_in_run(&small);
            // the HUD is up if either the Esc button or the run timer shows; gone only after half a second of neither
            if self.last_timer_seen || self.hud_visible_in(&small) {
                self.hud_last_seen = Some(Instant::now());
                self.hud_visible = true;
            } else if since(self.hud_last_seen) > 0.5 {
                self.hud_visible = false;
            }
        }
        // No run timer: maybe the map screen. Looked at four times a second, only while the minimap has a fix to hold.
        if !self.last_timer_seen && self.minimap.last_position().is_some() && self.last_map_page_look.is_none_or(|t| t.elapsed() >= Duration::from_millis(250)) {
            self.last_map_page_look = Some(Instant::now());
            let head = self.grab(super::grab::map_title_rect(c.2, c.3), true);
            if !self.grabber.no_picture && self.map_screen_open(&head) {
                self.minimap.hold_still();
            }
        }
        if !self.in_run() {
            return;
        }
        // a chest, shop or other screen covering the HUD: the minimap is hidden, skip it instead of counting misses
        if self.last_timer_score < 0.5 && self.timer_template.is_some() {
            return;
        }
        let ic = in_corner(rc, corner);
        let mm = top.roi(ic.0, ic.1, ic.2.min(top.w.saturating_sub(ic.0)), ic.3.min(top.h.saturating_sub(ic.1)));
        let now = Instant::now();
        let mut log_now = self.last_minimap_log.is_none_or(|t| t.elapsed() > Duration::from_secs(3));
        if log_now {
            self.last_minimap_log = Some(now);
        }
        // Inside the Sanctum or out on the island? The game's log knows; the minimap's look is the fallback.
        let place = self.game_log_place();
        let island_like = if matches!(place, Place::Island | Place::Sanctum) { place == Place::Island } else { looks_outside(&mm) };
        if slow_tick {
            self.inside_streak = if island_like { 0 } else { self.inside_streak + 1 };
        }
        if self.on_island && self.inside_streak >= 3 {
            self.on_island = false;
            self.island_tracker().reset();
            crate::log::write("back in the Sanctum (minimap)");
        }
        let island_map = self.settings().island_map;
        // a run can start on the island, before any Sanctum layout is known: track there anyway
        let Some(layout) = layout.filter(|_| true) else {
            if !(island_like && island_map) {
                if self.hud_visible && !island_like {
                    self.guess_layout(&mm, ui, log_now);
                } else if island_like && self.outside_streak > 20 && !self.guesses.is_empty() && self.guess_task.is_none() {
                    self.clear_guesses();
                    crate::log::write("guess: reset (left the Sanctum)");
                }
                return;
            }
            return self.track_island(&mm, ui, now, slow_tick, &mut log_now, grabbed_at, island_like);
        };
        if slow_tick {
            self.outside_streak = if island_like { self.outside_streak + 1 } else { 0 };
        }
        if !island_like {
            let t0 = Instant::now();
            let pos = self.minimap.locate(&mm, &layout, ui);
            self.perf_locate.add(t0.elapsed());
            if log_now {
                crate::log::write(&format!("{} -> {} ({})", self.minimap.last_info, if pos.is_none() { "rejected" } else { "ok" }, layout.title()));
            }
            // A wrong layout never matches. Only frames that show plenty of floor count.
            if pos.is_some() {
                self.misfit_since = None;
            } else if !self.hud_visible || self.minimap.last_info.starts_with("minimap: too little") {
                // says nothing about the layout
            } else if self.misfit_since.is_none() {
                self.misfit_since = Some(now);
            }
            if self.misfit_since.is_some_and(|m| m.elapsed().as_secs_f64() > 30.0)
                && self.settings().forced_layout == 0
                && self.current_layout.is_some()
                && self.layout_source != GAME_LOG_SOURCE
            {
                crate::log::write(&format!("{} hasn't matched the minimap for 30 s: identifying the layout again, every difficulty", self.current_layout.as_ref().unwrap().title()));
                self.misfit_since = None;
                self.current_layout = None;
                self.layout_source.clear();
                self.last_pose = None;
                self.player = None;
                self.minimap.reset();
                self.settings_mut().run_layout = 0;
                self.difficulty_doubted = true;
                self.clear_guesses();
                self.sh.emit_status();
                return;
            }
            if log_now && self.settings().save_debug_frames {
                self.save_debug(&mm, if pos.is_none() { "minimap_rejected" } else { "minimap_ok" });
            }
            if let Some(p) = pos {
                if self.on_island {
                    crate::log::write("back in the Sanctum");
                }
                self.on_island = false;
                self.player = Some(p);
                self.minimap_time = Some(now);
                self.sanctum_ok_time = Some(now);
                self.player_time = Some(grabbed_at); // the overlay leads the fix by the picture's age
                return;
            }
        }
        self.track_island(&mm, ui, now, slow_tick, &mut log_now, grabbed_at, island_like);
    }

    #[allow(clippy::too_many_arguments)]
    fn track_island(&mut self, mm: &Mat, ui: f64, now: Instant, slow_tick: bool, log_now: &mut bool, grabbed_at: Instant, island_like: bool) {
        let isl = self.db.island().cloned();
        let Some(isl) = isl.filter(|_| self.settings().island_map && self.hud_visible && island_like) else {
            self.odo_prev = None;
            return;
        };
        // how far the player moved since the last frame, from how far the minimap scrolled
        let shrunk = cv::resize(mm, 0, 0, 1.0 / ui, 1.0 / ui, Interp::Area);
        let step = self.minimap_step(&shrunk);
        let dead = match self.island_player {
            Some(cur) if self.on_island && since(self.island_time) < 3.0 => Some((cur.0 + step.map_or(0.0, |s| s.0), cur.1 + step.map_or(0.0, |s| s.1))),
            _ => None,
        };
        // a Sanctum portal icon in view pins the position exactly, even on open ground
        if self.island_search.is_none()
            && let Some(lm) = self.island_landmark(mm, &shrunk, ui, &isl)
        {
            if *log_now || !self.on_island {
                crate::log::write(&format!("island: placed by the portal icon at ({:.0},{:.0})", lm.0, lm.1));
            }
            let mut t = self.island_tracker();
            t.reset();
            t.seed(lm);
            drop(t);
            self.island_arrival = None;
            self.island_probation = 0;
            self.on_island = true;
            self.island_player = Some(lm);
            self.island_time = Some(now);
            self.last_landmark = lm;
            self.last_landmark_at = Some(now);
            return;
        }
        if self.island_arrival.is_some() && self.island_search.is_none() {
            self.island_arrival_step(mm, ui, &isl, now, slow_tick);
            return;
        }
        // A whole-island search takes a second or two: it runs in the background; its result is picked up first.
        let ip;
        let mut from_search = false;
        if let Some(h) = &self.island_search {
            if !h.is_finished() {
                return;
            }
            ip = self.island_search.take().unwrap().join().ok().flatten();
            from_search = true;
            *log_now = true;
        } else if self.island_tracker().tracking() {
            let t0 = Instant::now();
            ip = self.island_tracker().locate(mm, &isl, ui, false);
            self.perf_locate.add(t0.elapsed());
        } else {
            if self.last_island_search.is_some_and(|t| t.elapsed() < Duration::from_millis(1200)) {
                return;
            }
            self.last_island_search = Some(Instant::now());
            let (shot, tracker, isl2) = (mm.clone(), self.island_tracker.clone(), isl.clone());
            self.island_search_run = self.game_log.run_id().map(str::to_string);
            self.island_search = Some(std::thread::spawn(move || tracker.lock().unwrap().locate(&shot, &isl2, ui, true)));
            return;
        }
        if *log_now {
            let info = self.island_tracker().last_info.clone();
            crate::log::write(&format!("{info} -> {}{}", if ip.is_none() { "rejected" } else { "ok" }, if self.odo_still { " (standing still)" } else { "" }));
            // the island's weak spots (mostly by the Sanctum doors): their minimap pictures, to see what's there
            let weak = ip.is_none() || self.island_tracker().last_score < 0.55;
            if weak && self.settings().save_debug_frames {
                self.save_debug_with(mm, if ip.is_none() { "island_rejected" } else { "island_weak" }, &info);
            }
        }
        let Some(ip) = ip else {
            if self.island_probation > 0 {
                self.island_probation = 0;
                crate::log::write("island candidate dropped (local tracking failed)");
            }
            if let (Some(d0), Some(_)) = (dead, step) {
                if self.odo_still {
                    self.island_time = Some(now); // standing: held, while the tracker keeps looking wider around it
                } else {
                    self.move_on_island(d0, now);
                }
            }
            return;
        };
        // just placed by a portal icon: an image match far from there is a look-alike, not a move
        if since(self.last_landmark_at) < 2.0 && manhattan(ip, self.last_landmark) > 60.0 {
            let mut t = self.island_tracker();
            t.reset();
            t.seed(self.last_landmark);
            return;
        }
        if !self.on_island {
            // a first whole-island hit is confirmed by tracking on from it: the next few local fixes must succeed
            if from_search {
                self.island_probation = 4;
                crate::log::write("island candidate: confirming by local tracking");
                return;
            }
            if self.island_probation > 1 {
                if slow_tick {
                    self.island_probation -= 1;
                    crate::log::write(&format!("island candidate: local fix {}", self.island_tracker().last_info));
                }
                return;
            }
            self.island_probation = 0;
        }
        if self.on_island
            && let Some(d1) = dead
        {
            let score = self.island_tracker().last_score;
            let agrees = manhattan(ip, d1) < 80.0;
            if !(score >= 0.75 || (score >= 0.5 && agrees)) {
                // a weak match that disagrees with how the player moved is a look-alike: follow the movement (standing
                // still: hold, and let the tracker keep looking wider)
                if self.odo_still {
                    self.island_time = Some(now);
                } else {
                    self.move_on_island(d1, now);
                }
                return;
            }
        }
        if !self.on_island {
            crate::log::write("on the island");
        }
        self.on_island = true;
        self.island_player = Some(ip);
        self.island_time = Some(grabbed_at);
    }

    /// Just arrived on the island: try the known arrival spots (boat landings at a run's start; out of the Sanctum,
    /// all ten doors, the one gone in by winning a near tie).
    fn island_arrival_step(&mut self, mm: &Mat, ui: f64, isl: &Layout, now: Instant, slow_tick: bool) {
        let arrival = self.island_arrival.clone().unwrap();
        let side = arrival == "entrance_side";
        let expected = if side { self.expected_exit_door(isl) } else { None };
        let spots: Vec<&crate::layout_db::Marker> = if side {
            isl.markers.iter().filter(|m| m.kind == "entrance_main" || m.kind == "entrance_side").collect()
        } else {
            isl.markers.iter().filter(|m| m.kind == arrival).collect()
        };
        let (mut best, mut best_score, mut best_kind, mut best_rank) = (None, 0.0, None::<String>, 0.0);
        let mut scores = String::new();
        {
            let mut t = self.island_tracker();
            for m in spots {
                t.reset();
                let spot = (m.img[0], m.img[1]);
                t.seed(spot);
                let found = t.locate(mm, isl, ui, false).is_some();
                if side {
                    if !scores.is_empty() {
                        scores += ", ";
                    }
                    let s = if found { format!("{:.2}", t.last_score) } else { "-".into() };
                    scores += &format!("{} ({:.0},{:.0}) {s}", door_name(Some(&m.kind)), m.img[0], m.img[1]);
                }
                let rank = t.last_score + if expected.is_some_and(|e| manhattan((m.img[0], m.img[1]), e) < 1.0) { 0.05 } else { 0.0 };
                if found && rank > best_rank {
                    best_rank = rank;
                    // the player stands at the portal / landing itself: start there
                    best = Some(spot);
                    best_score = t.last_score;
                    best_kind = Some(m.kind.clone());
                }
            }
            t.reset();
        }
        // a weak best match waits about a second for a better picture (the minimap fades in)
        if best.is_some() && side && best_score < 0.6 && self.island_arrival_tries < 20 {
            if slow_tick {
                self.island_arrival_tries += 1;
            }
            return;
        }
        if let Some(at) = best {
            self.island_tracker().seed(at);
            if arrival == "boat" {
                self.island_spawn = Some(at);
                self.island_door = None;
                self.island_door_kind = None;
                self.save_run_places();
            }
            if arrival != "boat" {
                let same = best_kind.is_some() && expected.is_some_and(|e| manhattan(e, at) < 1.0);
                crate::log::write(&format!(
                    "island: found at the {} exit ({best_score:.2}){}{}",
                    door_name(best_kind.as_deref()),
                    if same { ", the door you went in by" } else { "" },
                    if scores.is_empty() { String::new() } else { format!(" | {scores}") }
                ));
            } else {
                crate::log::write(&format!("island: found at the boat landing ({best_score:.2})"));
            }
            self.island_arrival = None;
            self.island_probation = 0;
            self.on_island = true;
            self.island_player = Some(at);
            self.island_time = Some(now);
            return;
        }
        // not there (yet): loading, or already walked off; fall back to searching after ~3 s
        if slow_tick {
            self.island_arrival_tries += 1;
            if self.island_arrival_tries > 40 {
                self.island_arrival = None;
            }
        }
    }

    fn move_on_island(&mut self, at: P2, now: Instant) {
        let mut t = self.island_tracker();
        t.reset();
        t.seed(at);
        drop(t);
        self.island_player = Some(at);
        self.island_time = Some(now);
    }

    /// The player's movement since the last call (island map px), from the minimap's scroll between frames (phase
    /// correlation of its middle, the player marker and the white cone blanked). `small`: the minimap at 1080p size.
    pub fn minimap_step(&mut self, small: &Mat) -> Option<P2> {
        const SIDE: usize = 128;
        let (cx, cy) = (small.w / 2, small.h / 2);
        if cx < SIDE / 2 || cy < SIDE / 2 || cx + SIDE / 2 > small.w || cy + SIDE / 2 > small.h {
            return None;
        }
        let part = small.roi(cx - SIDE / 2, cy - SIDE / 2, SIDE, SIDE);
        let hsv = cv::bgr2hsv(&part);
        let mut blank = cv::in_range(&hsv, &[0, 0, 201], &[180, 44, 255]); // the white cone
        cv2::circle(&mut blank, (SIDE / 2) as i64, (SIDE / 2) as i64, 18, 255); // the player marker
        let gray = cv::bgr2gray(&part);
        let mut cur = gray.to_f32(1.0);
        let (mut sum, mut n) = (0f64, 0usize);
        for (v, b) in cur.data.iter().zip(&blank.data) {
            if *b == 0 {
                sum += f64::from(*v);
                n += 1;
            }
        }
        let mean = if n > 0 { (sum / n as f64) as f32 } else { 0.0 };
        for (v, b) in cur.data.iter_mut().zip(&blank.data) {
            if *b != 0 {
                *v = mean;
            }
        }
        self.odo_still = false;
        let prev = self.odo_prev.replace(cur.clone())?;
        let (x, y, response) = cv2::phase_correlate(&prev, &cur, None::<&MatF>);
        if response < 0.08 || x.abs() + y.abs() > 24.0 {
            // open ground (a field, one path): nothing to measure a scroll by. A picture that hasn't changed means the
            // player stands still, not "unknown": the position is held instead of going stale.
            let (mut changed, mut n) = (0usize, 0usize);
            for ((a, b), m) in prev.data.iter().zip(&cur.data).zip(&blank.data) {
                if *m == 0 {
                    changed += usize::from((a - b).abs() > STILL_LEVELS);
                    n += 1;
                }
            }
            if n > 0 && (changed as f64) <= STILL_SHARE * n as f64 {
                self.odo_still = true;
                return Some((0.0, 0.0));
            }
            return None;
        }
        // the map scrolls the opposite way to the player; 1.2 minimap px per island map px
        Some((-x / 1.2, -y / 1.2))
    }

    pub fn on_game_log_changed(&mut self, place: Place, new_run: bool) {
        crate::log::write(&format!("game log: {place:?}{}", if new_run { " (new run)" } else { "" }));
        let forced = self.settings().forced_layout;
        if new_run && forced == 0 && self.current_layout.is_some() && self.settings().run_id.as_deref() != self.game_log.run_id() {
            crate::log::write("game log: new run, forgetting the last layout");
            self.reset_run();
        }
        // Into a Sanctum with its layout still unknown: the candidates' map art is loaded in the background, once the
        // log has had its moment to name the one layout that's needed (named.rs).
        if place == Place::Sanctum && self.current_layout.is_none() && forced == 0 {
            self.preload_due = Some(Instant::now() + super::named::NAME_WAIT);
        }
        match place {
            Place::Sanctum if self.on_island => {
                self.remember_entry_door();
                self.on_island = false;
                self.island_tracker().reset();
                crate::log::write("back in the Sanctum (game log)");
            }
            Place::Island => {
                self.inside_streak = 0;
                self.outside_streak = self.outside_streak.max(3);
                // the player arrives somewhere known: out of the blue door at an exit portal, or at a boat landing
                self.island_arrival = Some(if new_run { "boat" } else { "entrance_side" }.into());
                self.island_arrival_tries = 0;
            }
            _ => {}
        }
        self.sh.emit_status();
    }

    /// Going into a Sanctum from the island: the nearest island door to the last position (within 300 px).
    fn remember_entry_door(&mut self) {
        let Some(isl) = self.db.island() else { return };
        let Some(p) = self.island_player.filter(|_| since(self.island_time) <= 30.0) else { return };
        let door = isl
            .markers
            .iter()
            .filter(|m| m.kind == "entrance_main" || m.kind == "entrance_side")
            .min_by(|a, b| manhattan((a.img[0], a.img[1]), p).total_cmp(&manhattan((b.img[0], b.img[1]), p)));
        match door.filter(|d| manhattan((d.img[0], d.img[1]), p) <= 300.0) {
            None => {
                self.island_door = None;
                self.island_door_kind = None;
                crate::log::write(&format!("island: went into the Sanctum away from any door ({:.0},{:.0})", p.0, p.1));
                self.save_run_places();
            }
            Some(d) => {
                self.island_door = Some((d.img[0], d.img[1]));
                self.island_door_kind = Some(d.kind.clone());
                crate::log::write(&format!("island: went into the Sanctum by the {} at ({:.0},{:.0})", door_name(Some(&d.kind)), d.img[0], d.img[1]));
                self.save_run_places();
                if let Some(l) = self.current_layout.clone().filter(|l| !l.is_island()) {
                    let e = self.sanctum_entry(&l);
                    self.minimap.seed(e);
                }
            }
        }
    }

    /// Where an exit should come out: the door gone in by, else this run's spawn's orange door.
    fn expected_exit_door(&self, isl: &Layout) -> Option<P2> {
        if let Some(d) = self.island_door {
            return Some(d);
        }
        let s = self.island_spawn?;
        isl.markers
            .iter()
            .filter(|m| m.kind == "entrance_main")
            .min_by(|a, b| manhattan((a.img[0], a.img[1]), s).total_cmp(&manhattan((b.img[0], b.img[1]), s)))
            .map(|m| (m.img[0], m.img[1]))
    }

    /// Where the Sanctum map starts: its orange door; its blue door when the island's blue door was gone in by.
    pub fn sanctum_entry(&self, l: &Layout) -> P2 {
        self.entry_spot(l).unwrap_or((0.0, 0.0))
    }

    /// The Sanctum door the player comes in at (sanctum_entry), None when the layout has neither door.
    pub fn entry_spot(&self, l: &Layout) -> Option<P2> {
        let room = if self.island_door_kind.as_deref() == Some("entrance_side") && l.rooms.iter().any(|r| r.kind == "blue") { l.blue() } else { l.orange() };
        room.map(|r| (r.img[0], r.img[1]))
    }

    /// The player's island position from a Sanctum exit-portal icon on the minimap (1.2 minimap px per map px), the
    /// portal that fits the last known position, else the one whose surroundings match clearly best.
    fn island_landmark(&mut self, mm: &Mat, small: &Mat, ui: f64, isl: &Layout) -> Option<P2> {
        let near = self.island_player.filter(|_| since(self.island_time) < 30.0);
        if self.portal_icon.is_none() {
            let p = vision::imread(&self.db.root.join("icons").join("mm_portal_blue.png")).map(|m| if m.c == 4 { cv::bgra2bgr(&m) } else { m });
            self.portal_icon = Some(p);
        }
        let icon = self.portal_icon.as_ref().unwrap().as_ref()?;
        if small.w <= icon.w || small.h <= icon.h {
            return None;
        }
        let res = cv2::match_template(small, icon);
        let (_, max, _, loc) = res.min_max_loc();
        if max < 0.75 {
            return None;
        }
        let dx = loc.0 as f64 + icon.w as f64 / 2.0 - small.w as f64 / 2.0;
        let dy = loc.1 as f64 + icon.h as f64 / 2.0 - small.h as f64 / 2.0;
        // icons of far-off places are pinned to the rim (~88 px out at 1080p)
        let r = (dx * dx + dy * dy).sqrt();
        if !(18.0..=85.0).contains(&r) {
            return None;
        }
        let cands: Vec<P2> = isl.markers.iter().filter(|m| m.kind == "entrance_side").map(|m| (m.img[0] - dx / 1.2, m.img[1] - dy / 1.2)).collect();
        if let Some(n) = near {
            let mut best = None;
            let mut best_d = 450.0;
            for p in cands {
                let d = manhattan(p, n);
                if d < best_d {
                    best_d = d;
                    best = Some(p);
                }
            }
            return best;
        }
        let mut t = self.island_tracker();
        let mut scored: Vec<(P2, f64)> = vec![];
        for p in cands {
            t.reset();
            t.seed(p);
            t.locate(mm, isl, ui, false);
            scored.push((p, t.last_score));
        }
        t.reset();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        if scored.is_empty() || scored[0].1 < 0.3 || (scored.len() > 1 && scored[0].1 - scored[1].1 < 0.05) {
            return None;
        }
        Some(scored[0].0)
    }
}

/// The minimap's middle stood still between two pictures: at most this share of its pixels (the cone and marker left
/// out) changed by more than STILL_LEVELS grey levels. A path line scrolling by while walking changes its two edges,
/// ~1.5% of the middle; a picture of the same spot changes nearly nothing.
const STILL_LEVELS: f32 = 10.0;
const STILL_SHARE: f64 = 0.003;

/// The island minimap is almost all map art; the Sanctum's always shows dark void and walls.
pub fn looks_outside(mm: &Mat) -> bool {
    super::run::looks_outside(mm)
}
