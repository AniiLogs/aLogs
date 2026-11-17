//! Are we in an Egg Heist run? The run timer's stopwatch (white, or red under 3 minutes) in the HUD corner; the Esc
//! button for the HUD; the "Victory Conditions" panel that gives a Pathfinder challenge away; the island's look on
//! the minimap. Port of Engine.cs's UpdateInRun, CheckChallenge, ChallengeFound, ChallengeText, HudVisible,
//! EscVisible, Lit, ValueChannel and LooksOutside (Dev switches off).

use super::Core;
use crate::cv::{self, Mat};
use crate::cv2;
use crate::ocr::OcrService;
use crate::vision;
use std::time::{Duration, Instant};

pub fn value_channel(bgr: &Mat) -> Mat {
    cv::bgr2hsv(bgr).channel(2)
}

/// Is a matched HUD icon lit? Its white parts are 250+ (HSV value) in play; a third of the way into a fade to black,
/// under 170.
pub fn lit(value: &Mat, at: (usize, usize, usize, usize)) -> bool {
    let (x1, y1) = ((at.0 + at.2).min(value.w), (at.1 + at.3).min(value.h));
    if x1 <= at.0 || y1 <= at.1 {
        return false;
    }
    (at.1..y1).any(|y| (at.0..x1).any(|x| value.data[y * value.w + x] >= 170))
}

fn read_bgr(path: &std::path::Path) -> Option<Mat> {
    let m = vision::imread(path)?;
    Some(match m.c {
        4 => cv::bgra2bgr(&m),
        1 => cv::gray2bgr(&m),
        _ => m,
    })
}

fn clamp_rect(r: (usize, usize, usize, usize), w: usize, h: usize) -> (usize, usize, usize, usize) {
    let (x0, y0) = (r.0.min(w), r.1.min(h));
    (x0, y0, (r.0 + r.2).min(w) - x0, (r.1 + r.3).min(h) - y0)
}

/// Does the minimap show the island rather than the Sanctum? The Sanctum's always has dark void and walls (HSV value
/// under 90, 12-100% of the disc) and never green; the island's has almost no dark and is often green.
pub fn looks_outside(mm: &Mat) -> bool {
    let hsv = cv::bgr2hsv(mm);
    let mut disc = Mat::new(hsv.w, hsv.h, 1);
    let (cx, cy) = ((hsv.w / 2) as i64, (hsv.h / 2) as i64);
    let r = (hsv.w.min(hsv.h) as f64 * 0.42) as i64;
    cv2::circle(&mut disc, cx, cy, r, 255);
    cv2::circle(&mut disc, cx, cy, r / 5, 0);
    let n = disc.count_nonzero().max(1) as f64;
    let dark = cv::bitwise_and(&cv::in_range(&hsv, &[0, 0, 0], &[180, 255, 89]), &disc).count_nonzero() as f64;
    let green = cv::bitwise_and(&cv::in_range(&hsv, &[30, 40, 90], &[95, 255, 255]), &disc).count_nonzero() as f64;
    dark / n < 0.06 || green / n > 0.25
}

/// The "Victory Conditions" title in the HUD corner at 1080p size (x 45-295, y 295-335 measured), or None.
pub fn challenge_text(ocr: &OcrService, small: &Mat) -> Option<String> {
    let band = clamp_rect((20, 280, 300, 70), small.w, small.h);
    if band.2 < 100 || band.3 < 30 {
        return None;
    }
    let text = ocr.read(&small.roi(band.0, band.1, band.2, band.3)).to_lowercase();
    // "victory", not "condition": Egg Heist's own panel there is "Escape Condition"
    text.contains("victory").then_some(text)
}

/// The masked colour match of the red stopwatch, cleaned as Sanctum Helper cleans it (NaN -> -1, over 1 -> 0).
fn match_red(area: &Mat, tpl: &Mat, mask: &Mat) -> (f32, (usize, usize)) {
    let mut r = cv2::match_template_masked(area, tpl, mask);
    for v in r.data.iter_mut() {
        if v.is_nan() {
            *v = -1.0;
        }
        if *v > 1.0 {
            *v = 0.0;
        }
    }
    let (_, max, _, loc) = r.min_max_loc();
    (max, loc)
}

impl Core {
    fn icons(&self) -> std::path::PathBuf {
        self.db.root.join("icons")
    }

    /// Compared on brightness only (HSV value): the timer turns red under 3 minutes, red and white the same
    /// brightness. `small`: the HUD corner at 1080p size.
    pub fn update_in_run(&mut self, small: &Mat) {
        if self.timer_template.is_none() {
            self.timer_template = Some(read_bgr(&self.icons().join("hud_timer.png")).map(|t| value_channel(&t)));
        }
        if self.red_timer.is_none() {
            self.red_timer = Some(read_bgr(&self.icons().join("hud_timer_red.png")).map(|t| {
                let mut mask = Mat::new(t.w, t.h, 1);
                cv2::circle(&mut mask, (t.w / 2) as i64, (t.h / 2) as i64, (t.w / 2) as i64 - 2, 255);
                (t, mask)
            }));
        }
        let now = Instant::now();
        let mut seen = true;
        if let Some(Some(tpl)) = &self.timer_template {
            let area = clamp_rect((15, 340, 140, 320), small.w, small.h);
            seen = false;
            if area.2 > tpl.w && area.3 > tpl.h {
                let part = small.roi(area.0, area.1, area.2, area.3);
                let v = value_channel(&part);
                let res = cv2::match_template(&v, tpl);
                let (_, m, _, at) = res.min_max_loc();
                let mut max = f64::from(m);
                // the match ignores brightness: only a lit stopwatch counts (a fade to black still matched)
                if !lit(&v, (at.0, at.1, tpl.w, tpl.h)) {
                    max = 0.0;
                }
                // under 3 minutes: a red disc with a white running figure, matched in colour inside its circle
                if max < 0.7
                    && let Some(Some((rt, rm))) = &self.red_timer
                    && part.w > rt.w
                    && part.h > rt.h
                {
                    let (red_max, red_at) = match_red(&part, rt, rm);
                    let rp = part.roi(red_at.0, red_at.1, rt.w, rt.h);
                    let rv = value_channel(&rp);
                    if lit(&rv, (0, 0, rv.w, rv.h)) {
                        max = max.max(f64::from(red_max));
                    }
                }
                self.last_timer_score = max;
                // seen at 0.7, lost below 0.55: scores hover near 0.7 over bright floor
                seen = max >= if self.last_timer_seen { 0.55 } else { 0.7 };
            }
        }
        if seen != self.last_timer_seen {
            self.last_timer_seen = seen;
            crate::log::write(&format!("run timer {} ({:.2})", if seen { "seen" } else { "lost" }, self.last_timer_score));
        }
        if seen {
            self.check_challenge(small, now);
        }
        // tracking the player on the island means we're in a run, timer or not
        if !seen && self.on_island && self.island_tracker_tracking() {
            seen = true;
        }
        let since_run = self.last_run_seen.map_or(f64::MAX, |t| now.saturating_duration_since(t).as_secs_f64());
        if seen {
            if since_run > 30.0 {
                crate::log::write("Egg Heist run detected (timer on screen)");
            }
            self.last_run_seen = Some(now);
        } else if self.current_layout.is_some()
            && self.settings().forced_layout == 0
            && since_run > 90.0
            && self.last_no_run_check.elapsed() > Duration::from_secs(5)
        {
            // gone for 90 s with the game in front: the run is over
            self.last_no_run_check = now;
            crate::log::write("no Egg Heist run on screen for 90 s: forgetting the layout");
            self.reset_run();
        } else if self.current_layout.is_none() && self.guesses.is_empty() && self.arts.loaded() > 0 && since_run > 90.0 {
            // no run and no layout: let the map art go, so it isn't held while idle
            self.release_maps(|_| false);
        }
        if !self.in_run() {
            self.hide_mini();
        }
        self.sh.in_run.store(self.in_run(), std::sync::atomic::Ordering::SeqCst);
    }

    /// A Pathfinder challenge loads like an Egg Heist island and has the same stopwatch: its "Victory Conditions"
    /// panel is read once a second for the first 30 s of an island area while the timer is up.
    fn check_challenge(&mut self, small: &Mat, now: Instant) {
        if !self.ocr.available() || self.game_log.place != super::Place::Island {
            return;
        }
        let Some(run) = self.game_log.run_id().map(str::to_string) else { return };
        if self.challenge_area.as_deref() != Some(&run) {
            self.challenge_area = Some(run);
            self.challenge_area_at = now;
            self.next_challenge_read = now;
        }
        if now.saturating_duration_since(self.challenge_area_at) > Duration::from_secs(30) || now < self.next_challenge_read {
            return;
        }
        self.next_challenge_read = now + Duration::from_secs(1);
        if let Some(text) = challenge_text(&self.ocr, small) {
            crate::log::write(&format!("challenge on screen (\"{text}\"): not Egg Heist, hiding the maps"));
            self.game_log.not_egg_heist();
            self.on_island = false;
            self.island_tracker().reset();
            self.last_run_seen = None;
            self.hide_mini();
        }
    }

    /// The HUD is up: its Esc button, lit (a dimmed one matches as well as a lit one).
    pub fn hud_visible_in(&mut self, small: &Mat) -> bool {
        if self.hud_template.is_none() {
            self.hud_template = Some(read_bgr(&self.icons().join("hud_esc.png")));
        }
        let Some(Some(t)) = &self.hud_template else { return true };
        if small.w < t.w || small.h < t.h {
            return true;
        }
        let res = cv2::match_template(small, t);
        let (_, max, _, at) = res.min_max_loc();
        if max < 0.7 {
            return false;
        }
        let v = value_channel(&small.roi(at.0, at.1, t.w, t.h));
        lit(&v, (0, 0, v.w, v.h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn island_look_and_lit_icons() {
        // all dark: the Sanctum; all green: the island
        let mut m = Mat::new(100, 100, 3);
        m.data.fill(10);
        assert!(!looks_outside(&m));
        for p in m.data.chunks_exact_mut(3) {
            p.copy_from_slice(&[40, 160, 60]);
        }
        assert!(looks_outside(&m));
        let mut v = Mat::new(4, 4, 1);
        v.data.fill(100);
        assert!(!lit(&v, (0, 0, 4, 4)));
        v.data[5] = 200;
        assert!(lit(&v, (0, 0, 4, 4)) && !lit(&v, (2, 2, 2, 2)));
    }
}
