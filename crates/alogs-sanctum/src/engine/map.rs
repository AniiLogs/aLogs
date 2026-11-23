//! What the overlay shows: the full map lined up over the game's open map (doors, then the pan, one door or the last
//! pose, refined on the explored floor; features as the last resort), the island map, the on-screen and big maps; and
//! the party screen's difficulty. Port of Engine.cs's ShowMap, ShowIslandMap, ShowMini, ShowBig, HideMini,
//! ReadPartyScreen, MapScreenOpen, PanImage and SaveDebug (Dev switches off).

use super::{Core, GAME_LOG_SOURCE, OverlayFrame};
use crate::cv::{self, Interp, Mat, MatF};
use crate::cv2;
use crate::detectors::{self, Anchor};
use crate::lang;
use crate::layout_db::Layout;
use crate::vision::{self, MapPose, P2};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const PAN_SCALE: f64 = 0.25;
/// Longer than this without a party screen read and its next read is a new visit.
pub const PARTY_AWAY_SECONDS: f64 = 3.0;

fn since(t: Option<Instant>) -> f64 {
    t.map_or(f64::MAX, |t| t.elapsed().as_secs_f64())
}

fn dist(a: P2, b: P2) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// The middle of the map screen, small and gray, for following pans (the title, zoom bar and edges don't move).
pub fn pan_image(frame: &Mat) -> MatF {
    let crop = frame.roi(frame.w / 8, frame.h / 8, frame.w * 3 / 4, frame.h * 3 / 4);
    let small = cv::resize(&cv::bgr2gray(&crop), 0, 0, PAN_SCALE, PAN_SCALE, Interp::Area);
    small.to_f32(1.0)
}

pub fn new_party_visit(last_seen: Option<Instant>, now: Instant) -> bool {
    last_seen.is_none_or(|t| now.saturating_duration_since(t).as_secs_f64() > PARTY_AWAY_SECONDS)
}

impl Core {
    /// Is the in-game map screen showing? `head`: the map title corner of the game window.
    pub fn map_screen_open(&self, head: &Mat) -> bool {
        let c = self.game.client;
        let f = 1.0 / vision::ui_scale(c.2 as usize, c.3 as usize);
        let small = cv::resize(head, 0, 0, f, f, Interp::Area);
        self.doors.is_map_open(&small, 0)
    }

    fn frame(&self, visible: bool, layout: Option<&Layout>, status: &str) -> OverlayFrame {
        OverlayFrame { client: self.game.client, visible, layout: layout.map(|l| l.number), difficulty: self.effective_difficulty(), status: status.into(), ..Default::default() }
    }

    /// The on-screen map while the full map is closed, only while the minimap keeps finding the player.
    pub fn show_mini(&mut self) {
        let mut layout = self.shown_layout();
        let (mut me, mut when, mut floor) = (self.player, self.player_time, self.minimap.floor);
        let island_map = self.settings().island_map;
        if self.on_island && island_map
            && let Some(isl) = self.db.island()
        {
            layout = Some(isl.clone());
            me = self.island_player;
            when = self.island_time;
            floor = 0;
        }
        // outside the Sanctum the Sanctum map is wrong: step aside
        if self.outside_streak >= 3 && !island_map {
            return self.hide_mini();
        }
        // while the HUD is up keep showing the last fix; hide it in menus and on the map
        let fresh = me.is_some() && self.hud_visible && self.in_run() && since(when) < 60.0;
        if self.settings().mini_map_mode == "Off" || layout.is_none() || !fresh {
            return self.hide_mini();
        }
        self.sh.mini_showing.store(true, Ordering::SeqCst);
        let h = *self.sh.heading.lock().unwrap();
        let vel = if self.on_island { self.island_tracker.try_lock().map_or((0.0, 0.0), |t| t.velocity()) } else { self.minimap.velocity() };
        let f = OverlayFrame {
            player: me,
            player_fresh: true,
            mini: true,
            floor,
            heading: h.current_heading(),
            velocity: Some(vel),
            stamp: when,
            camera: h.current_camera(),
            ..self.frame(true, layout.as_ref(), "")
        };
        self.emit(f);
    }

    /// Key held with the in-game map closed: a big map centred on the player, following them live (Sanctum only).
    pub fn show_big(&mut self) {
        let layout = self.shown_layout();
        let fresh = self.player.is_some() && since(self.player_time) < 60.0;
        let Some(layout) = layout.filter(|_| fresh) else {
            let f = self.frame(false, None, "");
            return self.emit(f);
        };
        let h = *self.sh.heading.lock().unwrap();
        let f = OverlayFrame {
            player: self.player,
            player_fresh: true,
            mini: true,
            floor: self.minimap.floor,
            heading: h.current_heading(),
            big: true,
            velocity: Some(self.minimap.velocity()),
            stamp: self.player_time,
            camera: h.current_camera(),
            ..self.frame(true, Some(&layout), "")
        };
        self.emit(f);
    }

    pub fn hide_mini(&mut self) {
        if !self.sh.mini_showing.swap(false, Ordering::SeqCst) {
            return;
        }
        let f = OverlayFrame { client: self.game.client, ..Default::default() };
        self.emit(f);
    }

    /// Reads the party screen's difficulty; true while the party screen is up.
    pub fn read_party_screen(&mut self) -> bool {
        let c = self.game.client;
        let region = self.settings().party_goal_region;
        let rc = detectors::to_client(region, Anchor::TopCenter, c.2 as usize, c.3 as usize);
        if rc.2 < 20 || rc.3 < 10 {
            return false;
        }
        let goal = self.grab((rc.0 as i32, rc.1 as i32, rc.2 as i32, rc.3 as i32), false);
        let Some(d) = self.difficulty.detect_goal(&self.ocr, &goal) else { return false };
        // being on the party screen means any previous run is over
        if self.current_layout.is_some() && self.settings().forced_layout == 0 {
            self.reset_run();
        }
        self.difficulty_doubted = false;
        // the notice says the difficulty each time the party screen comes up, and whenever it changes there
        let now = Instant::now();
        let came_back = new_party_visit(self.party_seen, now);
        self.party_seen = Some(now);
        if Some(&d) != self.detected_difficulty.as_ref() || came_back {
            if Some(&d) != self.detected_difficulty.as_ref() {
                crate::log::write(&format!("difficulty: {d} (\"{}\")", self.difficulty.last_text));
            } else {
                crate::log::write(&format!("party screen again: {d}"));
            }
            self.detected_difficulty = Some(d.clone());
            self.sh.notify(&lang::f("{0} Mode chosen.", &[&lang::difficulty(Some(&d))]), Some(&d));
            self.sh.emit_status();
        }
        true
    }

    /// The full map: `frame` is the game window, or None when its map screen is known to be closed.
    pub fn show_map(&mut self, frame: Option<&Mat>) {
        let c = self.game.client;
        if frame.is_none() {
            self.doors.mark_closed();
        }
        let (island_map, forced) = {
            let s = self.settings();
            (s.island_map, s.forced_layout)
        };
        // out on the island the island map has door icons too (Sanctum entrances): the door method must not run
        if island_map
            && self.on_island
            && let Some(isl) = self.db.island().cloned()
        {
            if !self.show_island_map(frame, &isl) {
                let f = self.frame(false, None, "");
                self.emit(f);
            }
            return;
        }
        if !island_map && self.outside_streak >= 3 {
            let f = self.frame(false, None, "");
            return self.emit(f);
        }
        let cands: Vec<Layout> = self.candidates();
        // every layout is checked (a difficulty read stale); in a tie the difficulty's own layouts win
        let all: Vec<Layout> = if forced == 0 { self.db.candidates(None).into_iter().cloned().collect() } else { self.all_candidates() };
        let tracked = self.current_layout.as_ref().filter(|_| since(self.sanctum_ok_time) < 10.0).map(|l| l.number);
        let diff = self.effective_difficulty();
        let favoured: Option<Vec<i64>> = if self.difficulty_doubted || diff.is_none() { None } else { Some(self.db.candidates(diff.as_deref()).iter().map(|l| l.number).collect()) };
        let mut arrow = None;
        let all_refs: Vec<&Layout> = all.iter().collect();
        let mut pose = frame.and_then(|f| self.doors.locate(f, &all_refs, &mut arrow, tracked, favoured.as_deref()));
        self.last_match_info = self.doors.last_info.clone();
        if pose.is_none()
            && all.len() > 1
            && self.doors.last_map_open
            && self.doors.last_orange.is_some()
            && self.doors.last_blue.is_some()
            && let (Some(cur), Some(f)) = (self.current_layout.clone(), frame)
        {
            pose = self.doors.locate(f, &[&cur], &mut arrow, None, None);
            if pose.is_some() {
                self.last_match_info += &format!(" | kept {}", cur.title());
            }
        }
        let log_match = self.last_match_log.is_none_or(|t| t.elapsed() > Duration::from_secs(1));
        if log_match {
            self.last_match_log = Some(Instant::now());
        }
        // 1. a first guess: both doors > the pan since the last frame > one door at the last zoom > the last pose
        // 2. refine it on the explored floor; feature matching is the last resort
        let door_pose = pose.clone();
        let mut guess = door_pose.clone();
        let mut src = if door_pose.is_some() { "doors" } else { "" };
        let pan_now = if self.doors.last_map_open { frame.map(pan_image) } else { None };
        if guess.is_none()
            && let (Some(pn), Some(pg), Some(pp)) = (&pan_now, &self.prev_gray, &self.prev_gray_pose)
            && (pg.w, pg.h) == (pn.w, pn.h)
        {
            let (sx, sy, response) = cv2::phase_correlate(pg, pn, None::<&MatF>);
            if response > 0.08 {
                guess = Some(MapPose { tx: pp.tx + sx / PAN_SCALE, ty: pp.ty + sy / PAN_SCALE, inliers: 30, rotation: 0.0, ..pp.clone() });
                src = "pan";
            }
        }
        if guess.is_none()
            && self.doors.last_map_open
            && (self.doors.last_orange.is_some() || self.doors.last_blue.is_some())
            && let Some(lp) = self.last_pose.clone()
            && self.current_layout.as_ref().is_none_or(|c| c.number == lp.layout)
            && let Some(l) = self.db.by_number(lp.layout)
        {
            let seen_img = match self.doors.last_blue {
                Some(bp) => l.blue().map(|r| (bp, r.img.clone())),
                None => l.orange().map(|r| (self.doors.last_orange.unwrap(), r.img.clone())),
            };
            if let Some((seen, img)) = seen_img {
                guess = Some(MapPose { tx: seen.0 - lp.scale * img[0], ty: seen.1 - lp.scale * img[1], inliers: 30, rotation: 0.0, ..lp });
                src = "one door";
            }
        }
        if guess.is_none() && self.doors.last_map_open && self.last_pose.is_some() {
            guess = self.last_pose.clone();
            src = "last";
        }
        // While the doors stay put the last fit holds (checked again every 1.5 s); while they move at the same zoom
        // the fit moves with them; once they stop somewhere new or the zoom changes, a new fit is made.
        let mut reused = false;
        // a pan / one-door guess the floor fit didn't confirm doesn't place the player
        let mut seed_ok = true;
        let door_o = self.doors.last_orange;
        if src == "doors"
            && let (Some(fit), Some(dp), Some(o), Some(bl), Some(fo), Some(fb)) = (self.fit.clone(), &door_pose, door_o, self.doors.last_blue, self.fit_o, self.fit_b)
            && fit.layout == dp.layout
        {
            let tol = 2.5 * vision::ui_scale(c.2 as usize, c.3 as usize); // door icons are found in 2 px steps
            let same_zoom = (dist(o, bl) / dist(fo, fb).max(1.0) - 1.0).abs() < 0.015;
            let still = dist(o, fo) <= tol;
            let moving = self.prev_door_o.is_some_and(|po| dist(o, po) > tol);
            if same_zoom && still && since(self.fit_at) < 1.5 {
                pose = Some(fit);
                reused = true;
                self.last_match_info += " | doors still: last fit";
            } else if same_zoom && !still && moving {
                pose = Some(MapPose { tx: fit.tx + o.0 - fo.0, ty: fit.ty + o.1 - fo.1, ..fit });
                reused = true;
                self.last_match_info += " | doors moving: last fit moved along";
            }
        }
        self.prev_door_o = if src == "doors" { door_o } else { None };
        if let (Some(g), true, false, Some(f)) = (guess.clone(), self.doors.last_map_open, reused, frame)
            && let Some(gl) = self.db.by_number(g.layout).cloned()
        {
            let sure = src == "doors";
            let mut refined = self.refiner.refine(f, &g, &gl, if sure { 0.06 } else { 0.15 }, if sure { 40 } else { 120 });
            let need = if sure { 0.5 } else { 0.45 };
            if (refined.is_none() || self.refiner.last_score < need) && !sure && self.last_wide_search.is_none_or(|t| t.elapsed() > Duration::from_millis(500)) {
                // zoomed or panned a lot since the last fit: search much wider (slower, so not every frame)
                self.last_wide_search = Some(Instant::now());
                refined = self.refiner.refine(f, &g, &gl, 0.45, 320);
            }
            if let Some(r) = refined.filter(|_| self.refiner.last_score >= need) {
                self.last_match_info += &format!(" | {src} + {}", self.refiner.last_info);
                if sure {
                    self.fit = Some(r.clone());
                    self.fit_o = self.doors.last_orange;
                    self.fit_b = self.doors.last_blue;
                    self.fit_at = Some(Instant::now());
                }
                pose = Some(r);
            } else if sure {
                pose = door_pose.clone();
                self.fit = None;
                self.last_match_info += &format!(" | doors only ({})", self.refiner.last_info);
            } else if src == "pan" || src == "one door" {
                pose = Some(g);
                seed_ok = false;
                self.last_match_info += &format!(" | {src} only ({})", self.refiner.last_info);
            } else {
                self.last_match_info += &format!(" | lost ({})", self.refiner.last_info);
            }
        }
        if pose.is_none()
            && self.doors.last_map_open
            && cands.len() == 1
            && let Some(f) = frame
        {
            let refs: Vec<&Layout> = cands.iter().collect();
            pose = self.matcher.match_map(f, &refs, self.last_pose.as_ref());
            self.last_match_info += &format!(" | features {}", self.matcher.last_info);
        }
        if let (Some(p), Some(pn)) = (&pose, pan_now) {
            self.prev_gray = Some(pn);
            self.prev_gray_pose = Some(p.clone());
        }
        if log_match {
            let what = match &pose {
                None => "no match".to_string(),
                Some(p) => format!("{} scale {:.3}", self.db.by_number(p.layout).map_or(String::new(), |l| l.title()), p.scale),
            };
            crate::log::write(&format!("map: {what} | {} | candidates {}", self.last_match_info, cands.iter().map(|x| x.number.to_string()).collect::<Vec<_>>().join(",")));
        }
        if let (Some(p), true, Some(a)) = (&pose, seed_ok, arrow)
            && p.inliers >= 30
        {
            // the yellow arrow on the full map is the most exact player position there is
            let pl = ((a.0 - p.tx) / p.scale, (a.1 - p.ty) / p.scale);
            self.player = Some(pl);
            self.player_time = Some(Instant::now());
            self.minimap.seed(pl);
        }
        if let Some(p) = &pose {
            self.last_pose = Some(p.clone());
            self.last_pose_time = Some(Instant::now());
            if forced == 0
                && self.layout_source != GAME_LOG_SOURCE
                && let Some(l) = self.db.by_number(p.layout).cloned()
            {
                self.lock_layout(&l, "map");
            }
        } else if self.last_pose.is_some() && self.doors.last_map_open && since(self.last_pose_time) < 0.4 {
            pose = self.last_pose.clone(); // brief miss (animation, cursor over the map): keep the last fit
        } else if self.settings().save_debug_frames
            && let Some(f) = frame
        {
            self.save_debug(f, "nomatch");
        }
        let layout: Option<Layout> = pose
            .as_ref()
            .and_then(|p| self.db.by_number(p.layout).cloned())
            .or_else(|| self.current_layout.clone())
            .or_else(|| if cands.len() == 1 { Some(cands[0].clone()) } else { None });
        let status = if let Some(p) = &pose {
            format!("{}  ·  aligned", self.db.by_number(p.layout).map_or(String::new(), |l| l.title()))
        } else if let Some(l) = &layout {
            format!("{}  ·  open the full map to align", l.title())
        } else {
            match self.effective_difficulty() {
                None => "Open the in-game map. Tip: open the party screen once so I can read the difficulty.".into(),
                Some(d) => format!("{d}: open the in-game map so I can find the layout"),
            }
        };
        let fresh = self.player.is_some() && since(self.player_time) < 5.0;
        // without the map on screen stay invisible for a moment: the key may just be part of normal play
        let visible = pose.is_some() || (self.in_run() && since(self.held_since) > 0.7);
        let h = *self.sh.heading.lock().unwrap();
        let f = OverlayFrame {
            pose,
            player: self.player,
            player_fresh: fresh,
            floor: self.refiner.floor,
            heading: h.current_heading(),
            ..self.frame(visible, layout.as_ref(), &status)
        };
        self.emit(f);
    }

    /// Full map on the island: feature matching against the island art. False when the screen isn't the island map.
    fn show_island_map(&mut self, frame: Option<&Mat>, isl: &Layout) -> bool {
        if !self.in_egg_heist() {
            self.on_island = false;
            let f = self.frame(false, Some(isl), "");
            self.emit(f);
            return true;
        }
        let mut arrow = None;
        if let Some(f) = frame {
            self.doors.locate(f, &[], &mut arrow, None, None); // map-open check + the player's arrow
        }
        let Some(f) = frame.filter(|_| self.doors.last_map_open) else {
            let fr = self.frame(false, Some(isl), "");
            self.emit(fr);
            return true;
        };
        let pose = self.matcher.match_map(f, &[isl], self.island_pose.as_ref());
        self.last_match_info = format!("island {}", self.matcher.last_info);
        if self.last_match_log.is_none_or(|t| t.elapsed() > Duration::from_secs(1)) {
            self.last_match_log = Some(Instant::now());
            let what = pose.as_ref().map_or("no match".to_string(), |p| format!("island scale {:.3} inliers {}", p.scale, p.inliers));
            crate::log::write(&format!("map: {what} | {}", self.matcher.last_info));
        }
        let Some(pose) = pose.filter(|p| self.on_island || p.inliers >= 25) else { return false };
        self.island_pose = Some(pose.clone());
        if let Some(a) = arrow {
            let ip = ((a.0 - pose.tx) / pose.scale, (a.1 - pose.ty) / pose.scale);
            self.island_player = Some(ip);
            self.island_time = Some(Instant::now());
            self.island_tracker().seed(ip);
        }
        self.on_island = true;
        let fr = OverlayFrame { pose: Some(pose), player: self.island_player, player_fresh: true, ..self.frame(true, Some(isl), "Egg Heist island · aligned") };
        self.emit(fr);
        true
    }

    /// A debug picture (PNG) every 3 s at most; the newest 200 stay, info.txt starts over past 2 MB.
    pub fn save_debug(&mut self, frame: &Mat, tag: &str) {
        let info = self.matcher.last_info.clone();
        self.save_debug_with(frame, tag, &info);
    }

    /// As save_debug, with the line for info.txt given (the island tracker's, for an island minimap).
    pub fn save_debug_with(&mut self, frame: &Mat, tag: &str, info_line: &str) {
        if self.last_debug_save.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
            return;
        }
        self.last_debug_save = Some(Instant::now());
        let dir = crate::settings::dir().join("debug");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut pics: Vec<(std::time::SystemTime, std::path::PathBuf)> = rd
                .filter_map(Result::ok)
                .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
                .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                .collect();
            pics.sort_by(|a, b| b.0.cmp(&a.0));
            for (_, p) in pics.into_iter().skip(199) {
                let _ = std::fs::remove_file(p);
            }
        }
        let info = dir.join("info.txt");
        if std::fs::metadata(&info).is_ok_and(|m| m.len() > 2_000_000) {
            let _ = std::fs::remove_file(&info);
        }
        let stamp = chrono::Local::now();
        let _ = write_png(&dir.join(format!("{}_{tag}.png", stamp.format("%Y%m%d_%H%M%S"))), frame);
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&info) {
            let _ = writeln!(f, "{} {tag} {info_line}", stamp.format("%H:%M:%S"));
        }
    }
}

/// A BGR (or gray) picture as PNG.
pub fn write_png(path: &std::path::Path, m: &Mat) -> Result<(), String> {
    let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), m.w as u32, m.h as u32);
    e.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = match m.c {
        1 => {
            e.set_color(png::ColorType::Grayscale);
            m.data.clone()
        }
        _ => {
            e.set_color(png::ColorType::Rgb);
            m.data.chunks_exact(m.c).flat_map(|p| [p[2], p[1], p[0]]).collect()
        }
    };
    e.write_header().and_then(|mut w| w.write_image_data(&data)).map_err(|e| e.to_string())
}
