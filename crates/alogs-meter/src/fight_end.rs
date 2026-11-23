//! Holding the player's inputs from an Interlink fight's fade to black, so a press can't skip the result screen before
//! aLogs has read it (issue 154: a result screen up well under a second, dismissed before the meter saw it, and the
//! run never recorded).
//!
//! The game fades to black many times (loads, cut-ins, a two-boss fight's change), so the fade is looked for only in a
//! short window at the end of a fight the meter follows: the fight's own countdown in its last ARM_LEFT_S seconds (up
//! to ARM_AFTER_S past zero), or the boss on its last bar at ARM_FILL or less and seen in the last KILL_AFTER_S. Outside
//! that window nothing of this runs: no fade check, no extra looks. While it is open every look is a whole frame.
//!
//! Two black looks in a row with the fight's bar not on them start the hold the result screen starts today (the same
//! gates: Automation's automatic Battle Data, "hold inputs" or "hold the controller", the game in front). The result
//! screen's own hold (start_auto) then takes it over and everything carries on as before. The fight's bar seen again
//! (the fight goes on: a fade mid-fight, a two-boss change) releases it at once; so do the game leaving
//! or entering a scene, the game going to the background, and FADE_HOLD_S without a result screen.

use crate::meter::Meter;
use alogs_pil::{Image, Mode};

/// The countdown's seconds left from which the window opens. From the logs of 21 Nov: the clock's zero came 0.4 s
/// before the bar's last reading and 1.6 to 2.5 s before the result screen; the meter's countdown is the game's clock
/// as read about once a second and carried on between reads, so 5 s leaves room for a read late by a second or two.
pub const ARM_LEFT_S: f64 = 5.0;
/// How long past the countdown's zero the window stays open: 2.2 to 2.5 s from zero to the result screen in the
/// owner's runs, and up to 1.5 s more waiting for the game window alone on Windows 10.
pub const ARM_AFTER_S: f64 = 6.0;
/// The last bar this full or less: the boss is about to go down. In the owner's 15 killed Interlink runs with a fight
/// file, the first reading at 5% or less came 1.3 to 14.5 s before the bar's end (and the bar's last reading was under
/// 2.3% in 13 of them); 20% would have opened the window up to 40 s early.
pub const ARM_FILL: f64 = 0.05;
/// How long after the boss bar's last sight (at ARM_FILL or less) the window stays open: the kill's result screen
/// came 4.7 s after the bar's last reading (21 Nov), plus the wait for the game window alone.
pub const KILL_AFTER_S: f64 = 10.0;
/// Black looks in a row that make a fade (two: a single black picture can be a capture hiccup).
pub const FADE_LOOKS: u32 = 2;
/// The fade's hold ends by itself after this long without a result screen (the result screen came within 1.5 s of
/// the bar's end in every run with one; its own hold takes over from there).
pub const FADE_HOLD_S: f64 = 6.0;

/// The end-of-fight watch.
#[derive(Default)]
pub struct FightEndState {
    /// Open: (since, why it opened).
    pub armed: Option<(f64, String)>,
    /// The fade's hold: when it started.
    pub held: Option<f64>,
    /// The fade's hold hid the controller.
    pub hid: bool,
    /// Black looks in a row.
    pub blacks: u32,
    /// The darkest look while open: (median, 95th percentile) of its brightness.
    pub darkest: Option<(u8, u8)>,
    /// Fade checks made (a look at a frame while the watch is open).
    pub checks: u64,
}

impl FightEndState {
    /// Is the watch open or the fade's hold on (every look a whole frame)?
    pub fn watching(&self) -> bool {
        self.armed.is_some() || self.held.is_some()
    }
}

/// Is this picture faded to black? With its brightness's median and 95th percentile (on a picture 64 px wide): median
/// under 14, 95% under 40.
pub fn black(im: &Image) -> (bool, u8, u8) {
    let small = im.reduce((im.width / 64).max(1)).convert(Mode::L);
    let mut lum = small.data.clone();
    if lum.is_empty() {
        return (false, 255, 255);
    }
    lum.sort_unstable();
    let (med, p95) = (lum[lum.len() / 2], lum[((lum.len() as f64 * 0.95) as usize).min(lum.len() - 1)]);
    (med < 14 && p95 < 40, med, p95)
}

impl Meter {
    /// The gates today's result-screen hold has (start_auto): Automation with the automatic Battle Data on, a live
    /// meter, and something to hold.
    fn fight_end_gates(&self) -> bool {
        let r = self.args.cfg.get("results");
        !self.args.replay && !self.args.no_auto_click && (r.get("hold_inputs").truthy() || r.get("hold_controller").truthy())
    }

    /// Why the fight is about to end, if it is: its countdown's last seconds, or the boss on its last bar and nearly
    /// empty (not a two-boss fight's first boss: its partner comes next, in the same run).
    pub fn fight_ending(&self, now: f64) -> Option<String> {
        let f = self.fight.as_ref()?;
        if !f.bossrush {
            return None;
        }
        if let Some(t) = &f.timer
            && let (Some(v), Some(at)) = (t.val, t.at)
        {
            let left = v - (now - at);
            if (-ARM_AFTER_S..=ARM_LEFT_S).contains(&left) {
                let s = left.max(0.0).ceil() as i64;
                return Some(format!("{}:{:02} left", s / 60, s % 60));
            }
        }
        let first_of_two = f.partner.is_some() && f.phases.len() == 1;
        if f.hit && !first_of_two && f.seg == Some(1) && f.seg_fill <= ARM_FILL && now - self.bar_seen <= KILL_AFTER_S {
            return Some(format!("the boss's last bar at {:.0}%", f.seg_fill * 100.0));
        }
        None
    }

    /// Every look: open the watch when a followed Interlink fight is about to end, close it when that is over; end the
    /// fade's hold on its time limit or once the result is read.
    pub fn fight_end_tick(&mut self, now: f64) {
        if let Some(at) = self.fight_end.held {
            if now - at > FADE_HOLD_S {
                self.fight_end_release(&format!("no result screen within {FADE_HOLD_S} s"));
            } else if self.fight.is_none() && self.rs.result_ready >= at && !self.auto_active() {
                self.fight_end_release("the result is read");
            }
            return;
        }
        let why = if self.fight_end_gates() { self.fight_ending(now) } else { None };
        match (why, &self.fight_end.armed) {
            (Some(why), None) => {
                log::info!(target: "meter", "fight ending ({why}): watching for the fade");
                self.fight_end.armed = Some((now, why));
                self.fight_end.blacks = 0;
                self.fight_end.darkest = None;
            }
            (None, Some(_)) => {
                let why = if self.fight.is_none() { "the fight is over".to_string() } else { "its end did not come".to_string() };
                self.fight_end_off(&why);
            }
            _ => {}
        }
    }

    /// While the watch is open: this look's picture, for the fade. Two black looks in a row, the fight's bar not on
    /// them, start the hold (the game in front).
    pub fn fight_end_look(&mut self, im: &Image, now: f64, foreground: bool) {
        if self.fight_end.armed.is_none() || self.fight_end.held.is_some() {
            return;
        }
        self.fight_end.checks += 1;
        let (dark, med, p95) = black(im);
        if self.fight_end.darkest.is_none_or(|d| (med, p95) < d) {
            self.fight_end.darkest = Some((med, p95));
        }
        let bar_on = self.bar.as_ref().is_some_and(|b| alogs_reader::bar::verify_bar(im, b));
        self.fight_end.blacks = if dark && !bar_on { self.fight_end.blacks + 1 } else { 0 };
        if self.fight_end.blacks < FADE_LOOKS {
            return;
        }
        if !foreground {
            self.fight_end_off("faded to black with the game in the background: nothing held");
            return;
        }
        let r = self.args.cfg.get("results");
        if r.get("hold_inputs").truthy() {
            self.hold.start(FADE_HOLD_S);
        }
        self.fight_end.hid = false;
        if r.get("hold_controller").truthy() && !alogs_capture::input::controllers_hidden() && alogs_capture::input::hidhide_available() {
            self.fight_end.hid = alogs_capture::input::hide_controllers(true);
        }
        self.fight_end.held = Some(now);
        log::info!(target: "meter", "fade to black at the fight's end: holding inputs until the result is read (at most {FADE_HOLD_S} s)");
    }

    /// The fight's bar is on screen again: a fight still going is never held.
    pub fn fight_end_bar_back(&mut self) {
        if self.fight_end.held.is_some() {
            self.fight_end_release("the fight's bar is back: the fight goes on");
        }
    }

    /// The result screen is up and its own hold (the automatic Battle Data session) has started: it carries on.
    pub fn fight_end_handover(&mut self) {
        if self.fight_end.held.is_none() || !self.auto_active() {
            return;
        }
        self.fight_end.held = None;
        self.fight_end.armed = None;
        self.fight_end.hid = false;
        log::info!(target: "meter", "the result screen is up: its own hold carries on from the fade's");
    }

    /// The fade's hold ends now (the result screen's own hold, if one runs, is left alone); the watch closes and may
    /// open again.
    pub fn fight_end_release(&mut self, why: &str) {
        let Some(at) = self.fight_end.held.take() else {
            self.fight_end_off(why);
            return;
        };
        if !self.auto_active() {
            self.hold.stop();
            if self.fight_end.hid && alogs_capture::input::controllers_hidden() {
                alogs_capture::input::hide_controllers(false);
            }
        }
        self.fight_end.hid = false;
        self.fight_end.armed = None;
        self.fight_end.blacks = 0;
        log::info!(target: "meter", "fight-end input hold ended after {:.1} s: {why}", self.env.now() - at);
    }

    /// The watch closes without a fade (said with the darkest look it saw, to tell a fade the measure missed).
    pub fn fight_end_off(&mut self, why: &str) {
        if self.fight_end.held.is_some() {
            self.fight_end_release(why);
            return;
        }
        if self.fight_end.armed.take().is_none() {
            return;
        }
        let dark = self.fight_end.darkest.take().map_or(String::new(), |(m, p)| format!(" (darkest look: median {m}, 95% {p})"));
        self.fight_end.blacks = 0;
        log::info!(target: "meter", "fight-end watch over: {why}{dark}");
    }
}
