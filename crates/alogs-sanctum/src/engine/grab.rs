//! Copies of parts of the game's client area for the engine: the app's shared picture (cleaned of every aLogs overlay
//! when it is a copy of the screen), the window's own image, or the screen without our windows. Port of Engine.cs's
//! CoverOverlays, WithoutOurs, Clean, ScreenPart, Grab, MapTitleRect.

use super::Shared;
use crate::capture::{self, Rect, magnifier};
use crate::cv::Mat;
use crate::{own_overlays, shared_capture, vision};
use std::sync::atomic::Ordering;

fn on_screen(client: Rect, sub: Rect) -> Rect {
    (client.0 + sub.0, client.1 + sub.1, sub.2, sub.3)
}

/// A picture of the screen has every aLogs overlay window in it: covered in black where each one is (the meter's,
/// the app's button; and ours unless the picture was taken without them), so nothing reads them.
pub fn cover_overlays(m: &mut Mat, client: Rect, sub: Rect, ours_too: bool) {
    let mut spots = shared_capture::overlays(Some(shared_capture::OWNER_SANCTUM));
    if ours_too {
        spots.extend(own_overlays::shown());
    }
    for o in spots {
        let (x0, y0) = ((o.0 - client.0 - sub.0).max(0), (o.1 - client.1 - sub.1).max(0));
        let (x1, y1) = ((o.0 + o.2 - client.0 - sub.0).min(m.w as i32), (o.1 + o.3 - client.1 - sub.1).min(m.h as i32));
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y as usize * m.w + x as usize) * m.c;
                m.data[i..i + m.c].fill(0);
            }
        }
    }
}

thread_local! {
    /// This thread's last picture was taken without our windows.
    pub static MAGNIFIED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// That part of the screen without our windows, where one in screen captures is over it; None where none is, or
/// the magnifier gives nothing.
fn without_ours(sh: &Shared, screen: Rect) -> Option<Mat> {
    MAGNIFIED.with(|m| m.set(false));
    let ours = own_overlays::over(screen);
    if ours.is_empty() {
        return None;
    }
    let m = magnifier::grab(screen, &ours)?;
    MAGNIFIED.with(|c| c.set(true));
    if !sh.magnifier_said.swap(true, Ordering::SeqCst) {
        crate::log::write("a map over the game is in screen captures: the part under it is read without it (magnifier)");
    }
    Some(m)
}

/// A shared picture that is a copy of the screen, made safe to read.
pub fn clean(sh: &Shared, shared: Mat, client: Rect, sub: Rect) -> Mat {
    if let Some(mut c) = without_ours(sh, on_screen(client, sub)).filter(|c| c.w == shared.w && c.h == shared.h) {
        cover_overlays(&mut c, client, sub, false);
        return c;
    }
    let mut s = shared;
    cover_overlays(&mut s, client, sub, true);
    s
}

/// Our own copy of part of the screen (no aLogs app): without our windows where one is over it.
pub fn screen_part(sh: &Shared, client: Rect, sub: Rect) -> Mat {
    let r = on_screen(client, sub);
    without_ours(sh, r).unwrap_or_else(|| capture::grab_screen(r))
}

/// What the main loop's grabs keep between calls.
#[derive(Default)]
pub struct Grabber {
    /// The last grab had no picture from the app yet: what it gave is empty and says nothing about the screen.
    pub no_picture: bool,
    pub print_window_failed: bool,
    shared_screen_copy_said: bool,
    misses: u32,
    first_miss: Option<std::time::Instant>,
    pub source: &'static str,
}

impl Grabber {
    /// A part of the game's client area (client px). `still`: the in-game map page is read.
    pub fn grab(&mut self, sh: &Shared, sub: Rect, still: bool, recordable: bool) -> Mat {
        let (_, _, c, handle) = sh.window();
        if shared_capture::app_present() {
            let shared = shared_capture::try_grab(handle, c.2, c.3, sub, false, still);
            self.no_picture = shared.is_none();
            own_overlays::tick();
            let shared = shared.map(|m| {
                if shared_capture::screen_copy() {
                    if !self.shared_screen_copy_said {
                        self.shared_screen_copy_said = true;
                        crate::log::write("the aLogs app shares its Desktop Duplication picture: every aLogs overlay in it is covered before reading");
                    }
                    clean(sh, m, c, sub)
                } else {
                    m
                }
            });
            self.source = if shared_capture::screen_copy() { "shared desktop duplication" } else { "shared window capture" };
            return shared.unwrap_or_else(|| Mat::new(sub.2.max(1) as usize, sub.3.max(1) as usize, 3));
        }
        self.no_picture = false;
        let window = recordable && !self.print_window_failed;
        self.source = if window { "own window" } else { "own screen copy" };
        if window {
            if let Some(m) = capture::grab_window(handle, c.2, c.3, sub) {
                self.misses = 0;
                return m;
            }
            // a single black frame is normal (fades, loading screens); a long run of them (40 and 3 s) means the
            // window can't be captured at all (exclusive fullscreen)
            if self.misses == 0 {
                self.first_miss = Some(std::time::Instant::now());
            }
            self.misses += 1;
            if self.misses >= 40 && self.first_miss.is_some_and(|t| t.elapsed().as_millis() > 3000) {
                self.print_window_failed = true;
                crate::log::write("window capture keeps giving no image; using screen capture (the parts under our overlays read without them)");
                sh.emit_capture_fallback();
            }
        }
        screen_part(sh, c, sub)
    }
}

/// The map screen's title corner (client px): 400 x 200 at 1080p, scaled with the game's UI, at the left of its UI
/// area.
pub fn map_title_rect(cw: i32, ch: i32) -> Rect {
    let (hx, _, _, hh) = vision::hud(cw as usize, ch as usize);
    let f = 1080.0 / hh as f64;
    (hx as i32, 0, (cw - hx as i32).min((400.0 / f) as i32), ch.min((200.0 / f) as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_corner_and_cover() {
        assert_eq!(map_title_rect(1920, 1080), (0, 0, 400, 200));
        assert_eq!(map_title_rect(3440, 1440), (440, 0, 533, 266));
        let mut m = Mat::new(4, 3, 3);
        m.data.fill(9);
        // nothing published here: nothing covered
        cover_overlays(&mut m, (0, 0, 100, 100), (0, 0, 4, 3), false);
        assert!(m.data.iter().all(|&v| v == 9));
    }
}
