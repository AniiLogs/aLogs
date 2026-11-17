//! How much of the game's picture each look copies, and when a screen read is due. Between fights only the top part
//! is copied (where a boss bar appears; ~3x cheaper at 4K) unless a whole frame is needed; a screen read alone gets
//! just its boxes. A screen read from a copy of the screen (Windows 10) waits for window capture. Port of
//! `full_frame_due`, `screen_parts`, `screen_read_due`, `game_only` and `game_only_gone` in aLogs B16
//! `bossbar/live.py` (Dev switches off: no calm pacing, no quiet-top-only, no egg-top-only). Its `names_waiting` is
//! left out: the Rust meter reads new names in place (no `names_first`), so no screen ever waits for them.

use crate::meter::{Frames, Meter, TOP_PART};
use crate::screens::SCREEN_BOXES;

/// After a match's end screen: whole frames, and pictures of what comes next, to see the rank screen.
pub const EGG_AFTER_S: f64 = 45.0;
/// The screens read only from the game window alone when the picture is a copy of the screen.
pub const GAME_ONLY_SCREENS: [&str; 10] = ["result", "Holo-Battle Sim result", "challenge", "battle prep", "Egg Heist rank", "profile", "party", "Egg Heist", "menu", "Battle Data"];
pub const GAME_ONLY_HOLD_S: f64 = 3.0;
pub const GAME_ONLY_WAIT_S: f64 = 1.5;
/// After the Sim's last fight, how long its Victory screen is expected (the game window alone asked for meanwhile).
pub const SIM_VICTORY_SOON_S: f64 = 6.0;

/// The pace between looks while no fight is on, kept up with "How often the meter looks for a boss bar"
/// (capture.look_interval). `seen` is the setting as last looked at: once it changes (the app saved the settings, and
/// the meter loads them again within a second), the new value is the pace from the next look. The pace the meter
/// started with (the setting, or --interval) holds until then, as in the Python, whose settings reload set it. The
/// Rust meter read the setting once at start, so a change waited for the next start of aLogs.
pub fn look_interval(current: f64, seen: &mut Option<f64>, cfg: &alogs_pyval::Value) -> f64 {
    let now = cfg.get("capture").get("look_interval").as_f64().filter(|v| *v > 0.0);
    if now == *seen {
        return current;
    }
    *seen = now;
    now.unwrap_or(current)
}

/// What the next look copies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// Only the top part (between fights, nothing else needed).
    No,
    /// The whole frame.
    Whole,
    /// Only the boxes a screen read needs.
    Screen,
}

/// What the pacing keeps between looks.
#[derive(Default)]
pub struct Pacing {
    pub game_only_since: f64,
    pub game_only_asked: f64,
    pub game_only_spot: Option<(&'static str, f64)>,
    pub game_only_said: bool,
    pub parts_said: f64,
}

impl Meter {
    /// Time for the next screen read: 0.1 s after the last when the screen changed (seen on the top part every look
    /// has), about every 1.5 s when it didn't, at once in quick mode.
    pub fn screen_read_due(&mut self, now: f64, quick: bool) -> bool {
        let changed = match (&self.ss.screen_read_sig, &self.top_sig) {
            (Some(last), Some(cur)) => crate::team::screen_changed(cur, last, 6.0),
            _ => true,
        };
        let mut gap: f64 = if quick { 0.0 } else if changed { 0.1 } else { 1.5 };
        // a fight's bar hidden: 4 reads a second, not back to back; the automatic Battle Data click still reads at once
        let auto = self.auto_active();
        if quick && !auto {
            gap = 0.25;
        }
        // the automatic session waits for the result screen to be wholly read before its first click
        if auto && self.auto.clicks == 0 {
            gap = gap.min(0.25);
        }
        // Egg Heist's end and rank screens, read with the player's inputs held: as soon as the reads can agree
        if self.egg.hold.is_some() || self.egg.read.is_some() || self.egg.points.is_some() {
            gap = gap.min(0.25);
        }
        if !quick && self.idle() {
            gap = gap.max(1.0);
        }
        now - self.rs.last_results >= gap
    }

    /// Between fights, whether this look needs the whole frame: a screen read, the end-screen note, Egg Heist, the
    /// Sim's end, diagnostic frames.
    pub fn full_frame_due(&mut self, now: f64) -> Due {
        if self.frame_size.0 == 0 || self.frame_size.1 == 0 {
            return Due::Whole; // (the frame's size comes from a whole one)
        }
        if self.rs.end_sig.is_some() || self.egg.hold.is_some() || self.sim.tour.is_some() {
            return Due::Whole;
        }
        if self.sanctum_in_run(now) {
            return Due::Whole;
        }
        // the rank screen can come any moment after a match: whole frames on every look
        let egg_after = now - self.egg.sent_at < EGG_AFTER_S;
        if egg_after {
            return Due::Whole;
        }
        if self.sim.ctl.is_some() || (self.holosim_space && self.sim.gate.last) {
            return Due::Whole;
        }
        let cap = self.args.cfg.get("capture");
        if let Some(a) = &self.after_fight
            && cap.get("end_screens").truthy()
            && now - a.2 >= 3.0
        {
            return Due::Whole;
        }
        if cap.get("nobar_frames").truthy() && self.bossrush_space && now - self.bars.last_nobar >= 5.0 && self.bars.nobar_count < 30 {
            return Due::Whole;
        }
        if self.screen_read_due(now, false) {
            // only for a screen read: just its boxes, unless a screen is up (then whole frames until it's gone)
            return if now < self.ss.full_until { Due::Whole } else { Due::Screen };
        }
        Due::No
    }

    /// The boxes a screen read needs, as shares of the frame: the top part and the boxes below it this read looks at.
    pub fn screen_parts(&mut self, now: f64) -> Option<Vec<(f64, f64, f64, f64)>> {
        let (w, h) = (self.frame_size.0 as f64, self.frame_size.1 as f64);
        if w == 0.0 || h == 0.0 {
            return None;
        }
        let u = w.min(h * 16.0 / 9.0);
        let ux = (w - u) / 2.0;
        let pad = 0.004;
        let skip: &[&str] = if now - self.ss.full_read_at < crate::screens::FULL_READ_S { crate::screens::place_skip(self.place(now)) } else { &[] };
        let mut out = vec![(0.0, 0.0, 1.0, TOP_PART)];
        for (name, _, (x0, y0, x1, y1)) in SCREEN_BOXES {
            if skip.contains(&name) || y1 <= TOP_PART {
                continue;
            }
            out.push((((ux + u * x0) / w - pad).max(0.0), (y0 - pad).max(0.0), ((ux + u * x1) / w + pad).min(1.0), (y1 + pad).min(1.0)));
        }
        Some(out)
    }

    /// This frame of a spotted screen should wait for the game window alone (the picture is a copy of the screen, so
    /// anything over the game, even a picture of a result screen, would be read): asks for it, and gives up waiting
    /// after GAME_ONLY_WAIT_S on a PC where it doesn't open.
    pub fn game_only(&mut self, frames: &mut dyn Frames, spotted: &'static str, now: f64) -> bool {
        if !GAME_ONLY_SCREENS.contains(&spotted) {
            return false;
        }
        if now - self.pacing.game_only_asked > 0.5 {
            self.pacing.game_only_asked = now;
            frames.set_game_only_until(((now + GAME_ONLY_HOLD_S) * 1000.0) as u64);
        }
        if !frames.screen_copy() {
            self.pacing.game_only_since = 0.0;
            self.pacing.game_only_spot = None;
            return false;
        }
        let last = self.pacing.game_only_spot.map_or(0.0, |s| s.1);
        if self.pacing.game_only_since == 0.0 || now - last > GAME_ONLY_HOLD_S {
            self.pacing.game_only_since = now;
        }
        self.pacing.game_only_spot = Some((spotted, now));
        if now - self.pacing.game_only_since < GAME_ONLY_WAIT_S {
            return true;
        }
        if !self.pacing.game_only_said {
            self.pacing.game_only_said = true;
            log::info!(target: "meter", "the game window alone did not come for the {spotted} screen: reading the screen");
        }
        false
    }

    /// Is a script on the screen, or its screen about to come? Then the game window alone is wanted the whole time,
    /// not only from the look that spots a screen.
    pub fn game_only_wanted(&self, now: f64) -> bool {
        // aLogs has the screen: the Sim's end and its Gizmo tour, the automatic Battle Data session (while its time
        // runs: it is closed lazily), Egg Heist's hold. Each of these ends by itself within a minute or two, so the
        // slower pictures of the game window alone can't stay on.
        let auto = self.auto.auto_until != 0.0 && now <= self.auto.auto_until;
        let script = self.sim.ctl.is_some() || self.sim.tour.is_some() || auto || self.egg.hold.is_some();
        // the Sim's Victory screen comes within a second or two of its last fight (17 Nov: 0.4 s). The last level is
        // known only with the gate watch on; without it every level's last fight counts.
        let last_level = self.sim.gate.last || !alogs_protocol::settings::automation(&self.args.cfg, Some("sim_gate"));
        let coming = self.holosim_space && last_level && self.fight.is_none() && self.fight_ended_at != 0.0 && now - self.fight_ended_at < SIM_VICTORY_SOON_S;
        script || coming
    }

    /// Asks for the game window alone while `game_only_wanted`, again every 0.5 s. On Windows 10 the picture is
    /// otherwise a copy of the screen, and the app hands out the game window alone only when a reader asks, for
    /// GAME_ONLY_HOLD_S from the last ask (by the window copy, which draws no border; by window capture with its
    /// border only where that copy shows nothing). Asked only by `game_only`, from a spotted screen's read, it came
    /// late and ran out early (17 Nov, app.log): the Sim's Victory screen was read once from the screen and then a
    /// second time (0.3 s); and the Gizmo tour, which reads no screens, let it run out, so the pictures changed
    /// back to the screen copy 1.5 s into the tour. Where the picture is always the window (Windows 11, or General
    /// Settings > Capture only the game window) asking changes nothing.
    pub fn game_only_keep(&mut self, frames: &mut dyn Frames, now: f64) {
        if self.args.replay || !self.game_only_wanted(now) || now - self.pacing.game_only_asked <= 0.5 {
            return;
        }
        self.pacing.game_only_asked = now;
        frames.set_game_only_until(((now + GAME_ONLY_HOLD_S) * 1000.0) as u64);
    }

    /// The screen showed one of these and, with the game window open, it's nothing like it: something on top, or a misread.
    pub fn game_only_gone(&mut self, frames: &dyn Frames, spotted: Option<&str>, now: f64) {
        if let Some((what, at)) = self.pacing.game_only_spot
            && spotted.is_none()
            && !frames.screen_copy()
            && now - at < GAME_ONLY_HOLD_S
        {
            self.pacing.game_only_spot = None;
            log::info!(target: "meter", "the screen showed a {what} screen and the game window doesn't: nothing read from it");
        }
    }
}
