//! The overlay's window: a click-through, top-most layered window that covers only the map panel when it can (a
//! whole-screen transparent window costs a full copy per redraw), glides the on-screen map between fixes and turns
//! the arrow and cone at display rate, steps out of screen captures when it must, and lets the player drag the
//! on-screen map where they want it. Port of `OverlayWindow` and OverlayView's Animate (Dev switches off). Runs on
//! the thread that calls `tick` (it pumps the window's messages).

use super::canvas::Canvas;
use super::view::{self, OverlayView, R};
use crate::capture::magnifier;
use crate::engine::OverlayFrame;
use crate::engine::heading::HeadingOut;
use crate::layout_db::LayoutDb;
use crate::settings::Settings;
use crate::vision::{self, P2};
use crate::{own_overlays, shared_capture};
use alogs_capture::layered::{self, Layered, Mouse};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

type Rect = (i32, i32, i32, i32);

fn contains(r: R, p: P2) -> bool {
    p.0 >= r.0 && p.0 <= r.0 + r.2 && p.1 >= r.1 && p.1 <= r.1 + r.3
}

/// One display-rate step of an angle toward its target (the short way round); true when it moved enough to redraw.
fn turn(shown: &mut Option<f64>, target: Option<f64>, a: f64) -> bool {
    let Some(t) = target else {
        let had = shown.is_some();
        *shown = None;
        return had;
    };
    let Some(cur) = *shown else {
        *shown = Some(t);
        return true;
    };
    let d = ((t - cur) % 360.0 + 540.0) % 360.0 - 180.0;
    if d.abs() < 0.3 {
        return false;
    }
    *shown = Some((cur + d * a + 360.0) % 360.0);
    true
}

#[cfg(windows)]
fn primary_screen() -> Rect {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe { (0, 0, GetSystemMetrics(SM_CXSCREEN).max(1), GetSystemMetrics(SM_CYSCREEN).max(1)) }
}

#[cfg(not(windows))]
fn primary_screen() -> Rect {
    (0, 0, 1920, 1080)
}

struct Placement {
    old: (Option<f64>, Option<f64>, f64),
    dragging: Option<(P2, R)>,
    enter_was_down: bool,
    esc_was_down: bool,
}

pub struct OverlayWindow {
    win: Layered,
    pub view: OverlayView,
    settings: Arc<RwLock<Settings>>,
    live: Option<OverlayFrame>,
    /// Where the window is (screen px), and what it last drew there.
    want: Rect,
    dirty: bool,
    hidden_drawn: bool,
    parked: bool,
    force_exclude: bool,
    excluded: Option<bool>,
    over_game: bool,
    shown: bool,
    placement: Option<Placement>,
    // the glide and the turning
    shown_at: Option<P2>,
    shown_layout: Option<i64>,
    shown_heading: Option<f64>,
    shown_camera: Option<f64>,
    last_tick: Instant,
    last_redraw: Instant,
    hidden_since: Option<Instant>,
    /// Called when placement ends (saved or cancelled).
    pub on_placement_ended: Option<Box<dyn Fn() + Send>>,
}

impl OverlayWindow {
    pub fn new(db: Arc<LayoutDb>, settings: Arc<RwLock<Settings>>) -> Option<OverlayWindow> {
        let win = Layered::new()?;
        win.click_through(true);
        let mut w = OverlayWindow {
            win,
            view: OverlayView::new(db),
            settings,
            live: None,
            want: (0, 0, 0, 0),
            dirty: false,
            hidden_drawn: true,
            parked: true,
            force_exclude: false,
            excluded: None,
            over_game: false,
            shown: false,
            placement: None,
            shown_at: None,
            shown_layout: None,
            shown_heading: None,
            shown_camera: None,
            last_tick: Instant::now(),
            last_redraw: Instant::now(),
            hidden_since: None,
            on_placement_ended: None,
        };
        w.apply_capture_mode(false);
        Some(w)
    }

    pub fn hwnd(&self) -> usize {
        self.win.hwnd
    }

    pub fn is_placing(&self) -> bool {
        self.placement.is_some()
    }

    /// In screenshots and recordings unless the player hid overlays from them; its spot is shared so the meter and
    /// the app cover it, and where it is over what we read we read under it (magnifier). Only where the magnifier
    /// gives nothing does a map over what's read stay out of screen captures. Every overlay steps out while the
    /// meter takes a proof.
    pub fn apply_capture_mode(&mut self, screen_fallback: bool) {
        self.force_exclude |= screen_fallback;
        let s = self.settings.read().unwrap();
        let screen_copy = self.force_exclude || (shared_capture::in_use() && shared_capture::screen_copy());
        let exclude = !s.recordable_overlay
            || shared_capture::clean_now()
            || (magnifier::failed() && (self.force_exclude || s.mini_map_corner == "OverGame" || (self.over_game && screen_copy)));
        drop(s);
        own_overlays::in_capture("map", self.win.hwnd, self.shown && !exclude);
        if self.excluded == Some(exclude) {
            return;
        }
        self.excluded = Some(exclude);
        self.win.hide_from_capture(exclude);
    }

    /// A new frame from the engine.
    pub fn apply(&mut self, f: OverlayFrame) {
        self.live = Some(f.clone());
        if self.placement.is_some() {
            // keep covering the game while the user drags; follow it if it shows up or moves meanwhile
            if f.visible && f.client.2 > 0 && f.client != self.view.placement_client {
                self.view.placement_client = f.client;
                self.want = f.client;
            }
            self.dirty = true;
            return;
        }
        // A panel-style map only needs a window around the panel.
        let s = self.settings.read().unwrap().clone();
        let mut want = f.client;
        let mut offset = (0.0, 0.0);
        if f.visible
            && f.mini
            && f.client.2 > 0
            && let Some(l) = f.layout.and_then(|n| self.view.db.by_number(n).cloned())
        {
            let st = self.view.mini_style(&s, &f, &l);
            if ["Corner", "Disc", "Radar", "Compass", "Big", "Overlay"].contains(&st.as_str()) {
                let (cw, ch) = (f64::from(f.client.2), f64::from(f.client.3));
                let ui = vision::ui_scale(f.client.2 as usize, f.client.3 as usize);
                let mut m = 70.0 * ui; // room for labels, rim arrows, the caption
                let r = match st.as_str() {
                    "Big" => {
                        m = 24.0 * ui;
                        view::big_panel(&s, cw, ch, ui)
                    }
                    "Overlay" => {
                        m = 0.0;
                        view::see_through_area(cw, ch)
                    }
                    _ => self.view.mini_panel(&s, &st, cw, ch, ui),
                };
                let (x0, y0) = ((r.0 - m).max(0.0), (r.1 - m).max(0.0));
                let (x1, y1) = ((r.0 + r.2 + m).min(cw), (r.1 + r.3 + m).min(ch));
                if x1 > x0 && y1 > y0 {
                    want = (f.client.0 + x0 as i32, f.client.1 + y0 as i32, (x1 - x0).ceil() as i32, (y1 - y0).ceil() as i32);
                    offset = (f64::from(x0 as i32), f64::from(y0 as i32));
                }
            }
        }
        self.view.offset = offset;
        self.over_game = f.visible && want == f.client;
        self.shown = f.visible && want.2 > 0;
        own_overlays::set("map", if self.shown { Some(want) } else { None });
        self.apply_capture_mode(false);
        self.want = want;
        // hidden now and already drawn hidden: nothing to redraw
        if !f.visible && self.hidden_drawn {
            return;
        }
        self.hidden_drawn = !f.visible;
        self.dirty = true;
    }

    /// One display-rate step: the glide and turns, input while placing, a redraw when anything changed, messages.
    pub fn tick(&mut self, heading: HeadingOut) {
        layered::pump();
        if self.placement.is_some() {
            self.placement_input();
        }
        self.animate(heading);
        if self.dirty {
            self.dirty = false;
            self.redraw();
        }
        // the map still hidden 90 s on: let go of its pictures (decoded again when next shown)
        if self.hidden_since.is_some_and(|t| t.elapsed() > Duration::from_secs(90)) {
            self.view.free();
            self.hidden_since = None;
        }
    }

    fn animate(&mut self, heading: HeadingOut) {
        let now = Instant::now();
        let s_cap = self.settings.read().map(|s| s.redraw_cap).unwrap_or(false);
        // on a 120 Hz or faster screen, at most about 80 steps a second
        if s_cap && now.duration_since(self.last_tick) < Duration::from_millis(12) {
            return;
        }
        let dt = now.duration_since(self.last_tick).as_secs_f64().min(0.1);
        self.last_tick = now;
        let Some(fr) = self.live.clone().filter(|f| f.mini && f.visible && self.placement.is_none()) else {
            self.shown_at = None;
            self.shown_heading = None;
            self.shown_camera = None;
            return;
        };
        let Some(fix) = fr.player else {
            self.shown_at = None;
            return;
        };
        // a different map's coordinates aren't a walk: start from the new spot
        if fr.layout != self.shown_layout {
            self.shown_layout = fr.layout;
            self.shown_at = None;
        }
        self.view.egg = heading.egg_now();
        let live = heading.stamp.is_some_and(|t| t.elapsed() < Duration::from_millis(1500));
        // ~15 ms: readings come ~60 a second already smoothed, this only bridges display frames
        let ta = 1.0 - (-dt / 0.015).exp();
        let mut moved = turn(&mut self.shown_heading, if live { heading.current_heading() } else { fr.heading }, ta);
        moved |= turn(&mut self.shown_camera, if live { heading.current_camera() } else { fr.camera }, ta);
        // keep moving along the current velocity between fixes (up to 0.15 s ahead)
        let mut target = fix;
        if let (Some(v), Some(stamp)) = (fr.velocity, fr.stamp) {
            let ahead = now.saturating_duration_since(stamp).as_secs_f64().clamp(0.0, 0.15);
            target = (fix.0 + v.0 * ahead, fix.1 + v.1 * ahead);
        }
        match self.shown_at {
            Some(cur) if (cur.0 - target.0).abs() + (cur.1 - target.1).abs() <= 300.0 => {
                if (cur.0 - target.0).abs() + (cur.1 - target.1).abs() >= 0.3 {
                    // ~35 ms: smooths ticks and small corrections; the tracker already filters
                    let a = 1.0 - (-dt / 0.035).exp();
                    self.shown_at = Some((cur.0 + (target.0 - cur.0) * a, cur.1 + (target.1 - cur.1) * a));
                    moved = true;
                }
            }
            _ => {
                self.shown_at = Some(target); // first fix or a confirmed teleport: snap
                moved = true;
            }
        }
        if moved {
            self.dirty = true;
        }
    }

    fn redraw(&mut self) {
        let Some(f) = self.live.clone() else { return };
        let s = self.settings.read().unwrap().clone();
        let placing = self.placement.is_some();
        if !f.visible && !placing {
            if !self.parked {
                self.win.park();
                self.parked = true;
            }
            if self.hidden_since.is_none() {
                self.hidden_since = Some(Instant::now());
            }
            return;
        }
        self.hidden_since = None;
        let want = self.want;
        if want.2 <= 0 || want.3 <= 0 {
            return;
        }
        let Some(mut cv) = Canvas::new(want.2 as u32, want.3 as u32) else { return };
        self.view.render(&mut cv, &s, &f, self.shown_at, self.shown_heading, self.shown_camera);
        let pix = cv.finish();
        self.win.show_premultiplied(pix.data(), want.2, want.3, want.0, want.1);
        self.parked = false;
        self.last_redraw = Instant::now();
    }

    // ---- Placement: the player drags the on-screen map where they want it. The window takes the mouse only where it
    // draws something; it never takes keyboard focus (Enter / Esc are read with GetAsyncKeyState).

    pub fn begin_placement(&mut self) {
        if self.placement.is_some() {
            return;
        }
        let s = self.settings.read().unwrap();
        let old = (s.mini_map_x, s.mini_map_y, s.mini_map_size);
        drop(s);
        // over the game when it's up, else over the primary screen (a sample map)
        self.view.offset = (0.0, 0.0);
        let area = self.live.as_ref().filter(|f| f.visible && f.client.2 > 0).map_or_else(primary_screen, |f| f.client);
        self.view.placement_client = area;
        self.want = area;
        self.win.click_through(false);
        self.view.placing = true;
        self.placement = Some(Placement { old, dragging: None, enter_was_down: crate::input::key_held(0x0D), esc_was_down: crate::input::key_held(0x1B) });
        if self.live.is_none() {
            self.live = Some(OverlayFrame { client: area, ..Default::default() });
        }
        self.dirty = true;
        crate::log::write("mini map placement started");
    }

    pub fn end_placement(&mut self, keep: bool) {
        let Some(p) = self.placement.take() else { return };
        {
            let mut s = self.settings.write().unwrap();
            if keep {
                s.save();
            } else {
                (s.mini_map_x, s.mini_map_y, s.mini_map_size) = p.old;
            }
            let at = match (s.mini_map_x, s.mini_map_y) {
                (Some(x), Some(y)) => format!("{x:.3},{y:.3}"),
                _ => "default".into(),
            };
            crate::log::write(&format!("mini map placement {}: {at} size {:.0}", if keep { "saved" } else { "cancelled" }, s.mini_map_size));
        }
        self.view.placing = false;
        self.win.click_through(true);
        self.hidden_drawn = false;
        if let Some(l) = self.live.clone() {
            self.apply(l);
        }
        self.dirty = true;
        if let Some(cb) = &self.on_placement_ended {
            cb();
        }
    }

    fn placement_input(&mut self) {
        let (enter, esc) = (crate::input::key_held(0x0D), crate::input::key_held(0x1B));
        let (ew, xw) = self.placement.as_ref().map_or((true, true), |p| (p.enter_was_down, p.esc_was_down));
        if let Some(p) = self.placement.as_mut() {
            p.enter_was_down = enter;
            p.esc_was_down = esc;
        }
        if enter && !ew {
            return self.end_placement(true);
        }
        if esc && !xw {
            return self.end_placement(false);
        }
        let c = self.view.placement_client;
        for (h, kind, x, y) in layered::take_events() {
            if h != self.win.hwnd {
                continue;
            }
            let p = (f64::from(x - c.0), f64::from(y - c.1));
            match kind {
                Mouse::Down => {
                    if contains(self.view.done_button, p) {
                        return self.end_placement(true);
                    }
                    if contains(self.view.cancel_button, p) {
                        return self.end_placement(false);
                    }
                    if contains(self.view.reset_button, p) {
                        let mut s = self.settings.write().unwrap();
                        s.mini_map_x = None;
                        s.mini_map_y = None;
                        s.mini_map_size = Settings::default().mini_map_size;
                        self.dirty = true;
                        continue;
                    }
                    let r = self.view.panel_rect;
                    if contains((r.0 - 8.0, r.1 - 8.0, r.2 + 16.0, r.3 + 16.0), p)
                        && let Some(pl) = self.placement.as_mut()
                    {
                        pl.dragging = Some((p, r));
                    }
                }
                Mouse::Move => {
                    let Some((from, start)) = self.placement.as_ref().and_then(|pl| pl.dragging) else { continue };
                    let ui = vision::ui_scale(c.2.max(1) as usize, c.3.max(1) as usize);
                    let moved = (start.0 + p.0 - from.0, start.1 + p.1 - from.1, start.2, start.3);
                    let fit = view::fit(moved, f64::from(c.2), f64::from(c.3), ui);
                    let mut s = self.settings.write().unwrap();
                    s.mini_map_x = Some((fit.0 + fit.2 / 2.0) / f64::from(c.2));
                    s.mini_map_y = Some((fit.1 + fit.3 / 2.0) / f64::from(c.3));
                    self.dirty = true;
                }
                Mouse::Up => {
                    if let Some(pl) = self.placement.as_mut() {
                        pl.dragging = None;
                    }
                }
                Mouse::WheelUp | Mouse::WheelDown => {
                    // resize around the panel's centre (the stored position is the centre)
                    let r = self.view.panel_rect;
                    let mut s = self.settings.write().unwrap();
                    if s.mini_map_x.is_none() {
                        s.mini_map_x = Some((r.0 + r.2 / 2.0) / f64::from(c.2.max(1)));
                        s.mini_map_y = Some((r.1 + r.3 / 2.0) / f64::from(c.3.max(1)));
                    }
                    let step = if kind == Mouse::WheelUp { 20.0 } else { -20.0 };
                    s.mini_map_size = (s.mini_map_size + step).clamp(160.0, 720.0);
                    self.dirty = true;
                }
                Mouse::Right => return self.end_placement(true),
                Mouse::Double => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turning_takes_the_short_way() {
        let mut a = Some(350.0);
        assert!(turn(&mut a, Some(10.0), 0.5));
        assert!((a.unwrap() - 0.0).abs() < 1e-9);
        assert!(!turn(&mut a, Some(0.1), 0.5));
        assert!(turn(&mut a, None, 0.5) && a.is_none());
        assert!(contains((0.0, 0.0, 10.0, 10.0), (5.0, 10.0)));
    }
}
