//! Facing and camera, read from the minimap on a loop of their own once per displayed frame (~60 a second), so
//! turning shows up at once; the overlay glides between readings. Port of Engine.cs's AngleFilter, HeadingSmoother,
//! HeadingFeed, HeadingLoop and UpdateHeading (Dev switches off).

use super::Shared;
use crate::cv::Mat;
use crate::detectors::{self, Anchor};
use crate::vision::{self, HeadingDetector};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

fn short_way(a: f64, b: f64) -> f64 {
    ((a - b) % 360.0 + 540.0) % 360.0 - 180.0
}

/// One-euro filter for an angle in degrees (Casiez et al.): smooths a lot while the value holds still and less the
/// faster it moves, so a resting cone stops jittering and a turning one doesn't trail behind.
#[derive(Default)]
pub struct AngleFilter {
    value: f64,
    speed: f64,
    pending: Option<f64>,
    last: Option<Instant>,
}

impl AngleFilter {
    const MIN_CUTOFF: f64 = 1.5;
    const BETA: f64 = 0.06;
    const SPEED_CUTOFF: f64 = 4.0;
    // two readings in a row this far off, agreeing with each other, are a real jump (a flick of the camera)
    const SNAP_DEG: f64 = 20.0;
    const SNAP_AGREE: f64 = 12.0;

    pub fn reset(&mut self, angle: f64, now: Instant) -> f64 {
        self.value = angle;
        self.speed = 0.0;
        self.pending = None;
        self.last = Some(now);
        angle
    }

    pub fn next(&mut self, angle: f64, now: Instant) -> f64 {
        let dt = self.last.map_or(0.5, |l| now.saturating_duration_since(l).as_secs_f64()).clamp(1e-3, 0.5);
        self.last = Some(now);
        let d = short_way(angle, self.value);
        if d.abs() >= Self::SNAP_DEG {
            let agrees = self.pending.is_some_and(|p| short_way(angle, p).abs() < Self::SNAP_AGREE);
            self.pending = Some(angle);
            if agrees {
                self.speed = 0.0;
                self.pending = None;
                self.value = angle;
                return angle;
            }
        } else {
            self.pending = None;
        }
        self.speed += alpha(Self::SPEED_CUTOFF, dt) * (d / dt - self.speed);
        self.value = ((self.value + alpha(Self::MIN_CUTOFF + Self::BETA * self.speed.abs(), dt) * d) % 360.0 + 360.0) % 360.0;
        self.value
    }
}

fn alpha(cutoff: f64, dt: f64) -> f64 {
    1.0 / (1.0 + 1.0 / (2.0 * std::f64::consts::PI * cutoff * dt))
}

/// Light smoothing for the arrow's facing: readings come in 2.5-degree steps and flicker between neighbours, so a
/// change of up to 6 degrees is eased in (time constant tau); a bigger one is a real turn, taken at once.
#[derive(Default)]
pub struct HeadingSmoother {
    value: Option<f64>,
    last: Option<Instant>,
}

impl HeadingSmoother {
    const SNAP_DEG: f64 = 6.0;

    pub fn reset(&mut self) {
        self.value = None;
    }

    pub fn next(&mut self, angle: f64, now: Instant, tau: f64) -> f64 {
        let dt = self.last.map_or(0.5, |l| now.saturating_duration_since(l).as_secs_f64()).clamp(1e-3, 0.5);
        self.last = Some(now);
        let Some(v) = self.value else {
            self.value = Some(angle);
            return angle;
        };
        let d = short_way(angle, v);
        let out = if d.abs() > Self::SNAP_DEG || tau <= 0.0 { angle } else { ((v + (1.0 - (-dt / tau).exp()) * d) % 360.0 + 360.0) % 360.0 };
        self.value = Some(out);
        out
    }
}

/// The latest facing and camera directions, for the overlay to animate between frames (written by the heading loop,
/// read by the engine and the overlay).
#[derive(Clone, Copy, Debug, Default)]
pub struct HeadingOut {
    pub heading: Option<f64>,
    pub heading_time: Option<Instant>,
    pub camera: Option<f64>,
    pub camera_time: Option<Instant>,
    pub egg: bool,
    pub stamp: Option<Instant>,
}

impl HeadingOut {
    /// The facing, while fresh (3 s).
    pub fn current_heading(&self) -> Option<f64> {
        self.heading.filter(|_| self.heading_time.is_some_and(|t| t.elapsed() < Duration::from_secs(3)))
    }

    /// Where the camera looks, while fresh (1.5 s).
    pub fn current_camera(&self) -> Option<f64> {
        self.camera.filter(|_| self.camera_time.is_some_and(|t| t.elapsed() < Duration::from_millis(1500)))
    }

    /// Twined with an egg (the minimap shows the egg marker, not the arrow).
    pub fn egg_now(&self) -> bool {
        self.egg && self.stamp.is_some_and(|t| t.elapsed() < Duration::from_millis(1500))
    }
}

/// Everything only the heading thread touches.
pub struct HeadingState {
    detector: HeadingDetector,
    camera_filter: AngleFilter,
    smoother: HeadingSmoother,
    heading: Option<f64>,
    heading_time: Option<Instant>,
    camera: Option<f64>,
    camera_time: Option<Instant>,
    arrow_retry: Option<Instant>,
    last_arrow_seen: Option<Instant>,
    heading_from_arrow: bool,
    egg_votes: i32,
    egg_form: bool,
    last_egg_seen: Option<Instant>,
    arrow_since: Option<Instant>,
}

/// Egg form ends once the arrow has been read this long without a miss...
const EGG_ARROW_BACK_S: f64 = 0.3;
/// ...or no egg marker was seen this long.
const EGG_GONE_S: f64 = 2.0;

fn since(t: Option<Instant>, now: Instant) -> f64 {
    t.map_or(f64::MAX, |t| now.saturating_duration_since(t).as_secs_f64())
}

impl HeadingState {
    pub fn new(icons: &Path) -> HeadingState {
        HeadingState {
            detector: HeadingDetector::new(&icons.join("player_arrow.png"), icons),
            camera_filter: AngleFilter::default(),
            smoother: HeadingSmoother::default(),
            heading: None,
            heading_time: None,
            camera: None,
            camera_time: None,
            arrow_retry: None,
            last_arrow_seen: None,
            heading_from_arrow: false,
            egg_votes: 0,
            egg_form: false,
            last_egg_seen: None,
            arrow_since: None,
        }
    }

    /// Egg form from this reading's egg score (None: the arrow was read). On once two readings agree (0.6 and up);
    /// once on, a reading of 0.52 and up still counts (the marker dims and is crossed by the map's own marks), and it
    /// goes off only when the arrow has been read steadily for EGG_ARROW_BACK_S or no egg was seen for EGG_GONE_S.
    /// With 0.6 s and one score for both, the marker flipped between egg and arrow up to once a second while twined
    /// (25 Nov: 32 flips in two matches, two of them 17 ms apart).
    fn egg_step(&mut self, score: Option<f64>, now: Instant) -> bool {
        let keep = if self.egg_form { 0.52 } else { 0.6 };
        self.egg_votes = match score {
            Some(e) if e >= keep => (self.egg_votes + 1).min(3),
            Some(_) => (self.egg_votes - 1).max(0),
            None => 0,
        };
        if self.egg_votes >= 2 || (self.egg_form && score.is_some_and(|e| e >= keep)) {
            self.last_egg_seen = Some(now);
        }
        let arrow_back = since(self.arrow_since, now) >= EGG_ARROW_BACK_S && self.arrow_since.is_some();
        self.egg_votes >= 2 || (self.egg_form && !arrow_back && since(self.last_egg_seen, now) < EGG_GONE_S)
    }

    /// Facing: the yellow arrow when it shows; otherwise (twined with an egg, the egg marker replaces it) the white
    /// view cone. While the arrow is missing it is only retried twice a second (its full search is the costly part).
    pub fn update(&mut self, mm: &Mat, ui: f64, show_view_cone: bool) -> HeadingOut {
        let now = Instant::now();
        let hint = self.heading.filter(|_| since(self.heading_time, now) < 0.5 && self.heading_from_arrow);
        let mut h = None;
        // arrow readings eased in over ~60 ms (they flicker between 2.5-degree steps); a real turn at once
        let mut tau = 0.06;
        if self.arrow_retry.is_none_or(|t| now >= t) {
            h = self.detector.detect(mm, ui, hint);
            self.arrow_retry = if h.is_none() { Some(now + Duration::from_millis(500)) } else { None };
        }
        self.heading_from_arrow = h.is_some();
        if h.is_some() {
            self.last_arrow_seen = Some(now);
        }
        // The view cone: drawn on our maps like the game does, and the facing when the arrow is gone. Read only when
        // drawn or the arrow is missing; a confidence of 0.2 keeps readings over walls.
        let (cone, conf) = if show_view_cone || h.is_none() { vision::from_view_cone(mm, ui) } else { (None, 0.0) };
        if let Some(c) = cone.filter(|_| conf >= 0.2) {
            let cam = if since(self.camera_time, now) < 1.0 { self.camera_filter.next(c, now) } else { self.camera_filter.reset(c, now) };
            self.camera = Some(cam);
            self.camera_time = Some(now);
            // a blink of the arrow keeps the last heading; only a lasting absence means egg form
            if h.is_none() && since(self.last_arrow_seen, now) > 0.6 {
                h = Some(cam);
                tau = 0.0;
            }
        }
        if let Some(hv) = h {
            if self.heading.is_none() || since(self.heading_time, now) >= 1.0 {
                self.smoother.reset();
            }
            self.heading = Some(self.smoother.next(hv, now, tau));
            self.heading_time = Some(now);
        }
        // Twined with an egg: no arrow and the egg marker where the arrow would be (twined 0.75-0.85, nearby egg
        // <= 0.44, normal minimaps <= 0.50).
        if self.heading_from_arrow {
            self.arrow_since.get_or_insert(now);
        } else {
            self.arrow_since = None;
        }
        let score = if h.is_none() || !self.heading_from_arrow {
            Some(if since(self.last_arrow_seen, now) > 0.4 { self.detector.egg_score(mm, ui) } else { 0.0 })
        } else {
            None
        };
        let egg = self.egg_step(score, now);
        if egg != self.egg_form {
            crate::log::write(if egg { "egg form: twined with an egg" } else { "egg form: back to normal" });
        }
        self.egg_form = egg;
        HeadingOut { heading: self.heading, heading_time: self.heading_time, camera: self.camera, camera_time: self.camera_time, egg, stamp: Some(now) }
    }
}

/// The HUD corner: the minimap, the Esc button and the run timer below them, from the left edge of the game's UI
/// area. (x, y, w, h) in client px.
pub fn corner_rect(rc: (usize, usize, usize, usize), cw: usize, ch: usize) -> (i32, i32, i32, i32) {
    let (hx, _, _, hh) = vision::hud(cw, ch);
    let ui = hh as f64 / 1080.0;
    let w = (cw - hx).min((rc.0 + rc.2).saturating_sub(hx).max((250.0 * ui) as usize));
    let h = ch.min((rc.1 + rc.3).max((650.0 * ui) as usize));
    (hx as i32, 0, w as i32, h as i32)
}

/// The minimap (client px) inside a corner copy.
pub fn in_corner(rc: (usize, usize, usize, usize), corner: (i32, i32, i32, i32)) -> (usize, usize, usize, usize) {
    (rc.0 - corner.0 as usize, rc.1 - corner.1 as usize, rc.2, rc.3)
}

/// 16 ms: one copy of the HUD corner per desktop frame at 60 Hz.
const PERIOD_MS: u64 = 16;

/// The heading loop: one corner copy per cycle, for the arrow and cone here and for the position tracking.
pub fn run(sh: Arc<Shared>, icons: std::path::PathBuf) {
    let mut st = HeadingState::new(&icons);
    while sh.running.load(Ordering::SeqCst) {
        let t0 = Instant::now();
        let mut magnified = false;
        let step = (|| -> Option<()> {
            let (found, fg, client, handle) = sh.window();
            if !found || !fg || !sh.in_run.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(150));
                return None;
            }
            let (cw, ch) = (client.2 as usize, client.3 as usize);
            let ui = vision::ui_scale(cw, ch);
            let (region, mini_mode_on, show_cone) = {
                let s = sh.settings.read().unwrap();
                (s.minimap_region, s.mini_map_mode != "Off", s.show_view_cone)
            };
            let rc = detectors::to_client(region, Anchor::TopLeft, cw, ch);
            if rc.2 < 40 {
                std::thread::sleep(Duration::from_millis(250));
                return None;
            }
            let corner = corner_rect(rc, cw, ch);
            let grabbed = Instant::now();
            let top = if crate::shared_capture::app_present() {
                crate::shared_capture::try_grab(handle, cw as i32, ch as i32, corner, false, false)
            } else {
                magnified = true;
                Some(super::grab::screen_part(&sh, client, corner))
            };
            let Some(mut top) = top else {
                std::thread::sleep(Duration::from_millis(4)); // no fresh shared frame yet
                return None;
            };
            if crate::shared_capture::app_present() && crate::shared_capture::screen_copy() {
                top = super::grab::clean(&sh, top, client, corner);
            }
            sh.put_corner(top.clone(), grabbed);
            // the facing is drawn only on our maps: read it while the on-screen map is on or the key shows something
            let wanted = mini_mode_on || sh.mini_showing.load(Ordering::SeqCst) || sh.key_showing.load(Ordering::SeqCst);
            sh.heading_paused.store(!wanted, Ordering::SeqCst);
            if wanted {
                let ic = in_corner(rc, corner);
                let mm = top.roi(ic.0, ic.1, ic.2.min(top.w - ic.0), ic.3.min(top.h - ic.1));
                let out = st.update(&mm, ui, show_cone);
                *sh.heading.lock().unwrap() = out;
                sh.heading_reads.fetch_add(1, Ordering::SeqCst);
            }
            Some(())
        })();
        let _ = step;
        // a corner read without our map over it (magnifier) costs about a screen copy: at most 25 a second
        let period = if magnified { 40 } else { PERIOD_MS };
        let spent = t0.elapsed().as_millis() as u64;
        if spent < period {
            std::thread::sleep(Duration::from_millis(period - spent));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One reading every 16 ms: Some(score) with no arrow, None when the arrow was read.
    fn feed(st: &mut HeadingState, t0: Instant, from_ms: u64, reads: &[Option<f64>]) -> Vec<bool> {
        reads.iter().enumerate().map(|(i, r)| {
            let now = t0 + Duration::from_millis(from_ms + 16 * i as u64);
            if r.is_none() { st.arrow_since.get_or_insert(now); } else { st.arrow_since = None; }
            let egg = st.egg_step(*r, now);
            st.egg_form = egg;
            egg
        }).collect()
    }

    #[test]
    fn egg_form_holds_through_weak_reads_and_a_stray_arrow() {
        let t0 = Instant::now();
        let mut st = HeadingState::new(Path::new("no-icons"));
        // twined: two clear readings turn it on
        assert_eq!(feed(&mut st, t0, 0, &[Some(0.8), Some(0.8)]), [false, true]);
        // the marker dims and is crossed for a second (0.53-0.58, under the 0.6 that turns it on), then one stray
        // arrow reading and a 1.5 s stretch with no egg read at all: still an egg throughout
        let mut reads = [Some(0.55), Some(0.58), Some(0.53)].repeat(20);
        reads.push(None);
        reads.extend(vec![Some(0.3); 90]);
        assert!(feed(&mut st, t0, 32, &reads).iter().all(|e| *e));
        // the arrow back for good: off within EGG_ARROW_BACK_S and a reading
        let back = feed(&mut st, t0, 32 + 16 * reads.len() as u64, &[None; 30]);
        assert!(back[..18].iter().all(|e| *e) && !back[20], "{back:?}");
        // a normal minimap never turns it on (normal <= 0.50)
        let mut st = HeadingState::new(Path::new("no-icons"));
        assert!(feed(&mut st, t0, 0, &vec![Some(0.5); 200]).iter().all(|e| !*e));
    }

    #[test]
    fn filters_follow_turns_and_snap_on_flicks() {
        let t = Instant::now();
        let mut f = AngleFilter::default();
        f.reset(350.0, t);
        // small wobble across north stays near it
        let v = f.next(5.0, t + Duration::from_millis(16));
        assert!(short_way(v, 355.0).abs() < 10.0, "{v}");
        // a flick: caught up with within two readings
        f.next(120.0, t + Duration::from_millis(32));
        let v = f.next(122.0, t + Duration::from_millis(48));
        assert!(short_way(v, 122.0).abs() < 2.0, "{v}");
        let mut s = HeadingSmoother::default();
        assert_eq!(s.next(10.0, t, 0.06), 10.0);
        let v = s.next(14.0, t + Duration::from_millis(16), 0.06);
        assert!(v > 10.0 && v < 14.0);
        assert_eq!(s.next(40.0, t + Duration::from_millis(32), 0.06), 40.0);
        assert_eq!(corner_rect((75, 32, 210, 210), 1920, 1080), (0, 0, 285, 650));
        assert_eq!(corner_rect((540, 43, 280, 280), 3440, 1440), (440, 0, 380, 866));
    }
}
