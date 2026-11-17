//! Notices for the player ("Chaos Mode chosen."): handed to the aLogs DPS meter while its overlay runs, so every
//! module's notices show in one style and spot; otherwise our own card in the game's top-right corner, dropping in
//! from above, staying a few seconds and sliding off to the right. Port of `SharedNotices.cs` and `ToastWindow.cs`.

use crate::overlay::canvas::{Canvas, Fill, Pen, Rgba, rounded_rect};
use crate::overlay::text;
use crate::settings::Settings;
use crate::{own_overlays, shared_capture, vision};
use alogs_capture::layered::Layered;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

type Rect = (i32, i32, i32, i32);

fn meter_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("aLogs").join("meter")
}

/// True when the meter took the notice (its status.json says "notices": true and was written in the last 5 s).
pub fn try_hand_off(head: &str, accent: Rgba, client: Rect) -> bool {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = meter_dir();
    let status = dir.join("status.json");
    let fresh = std::fs::metadata(&status).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|e| e.as_secs_f64() <= 5.0));
    if !fresh {
        return false;
    }
    let takes = std::fs::read_to_string(&status).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).is_some_and(|v| v["notices"] == serde_json::Value::Bool(true));
    if !takes {
        return false;
    }
    let folder = dir.join("notices");
    if std::fs::create_dir_all(&folder).is_err() {
        return false;
    }
    let notice = serde_json::json!({
        "head": head,
        "detail": "Sanctum Helper",
        "accent": format!("#{:02x}{:02x}{:02x}", accent.0, accent.1, accent.2),
        "screen": if client.2 > 0 { serde_json::json!([client.0, client.1, client.2, client.3]) } else { serde_json::Value::Null },
        "from": "sanctum",
    });
    // written whole, then renamed: the meter never reads half a file; the name keeps them in order
    let ticks = chrono::Utc::now().timestamp_micros() * 10 + 621_355_968_000_000_000;
    let name = format!("{ticks:020}-{:04}-sanctum", SEQ.fetch_add(1, Ordering::SeqCst) + 1);
    let tmp = folder.join(format!("{name}.tmp"));
    std::fs::write(&tmp, notice.to_string()).is_ok() && std::fs::rename(&tmp, folder.join(format!("{name}.json"))).is_ok()
}

const W: f64 = 340.0;
const H: f64 = 64.0;
const MARGIN: f64 = 18.0;
const DROP: f64 = 0.38;
const HOLD: f64 = 3.2;
const LEAVE: f64 = 0.34;

/// WPF's BackEase (EaseOut, amplitude 0.35).
fn back_out(t: f64) -> f64 {
    let u = 1.0 - t;
    1.0 - (u * u * u - u * 0.35 * (std::f64::consts::PI * u).sin())
}

/// Our own notification card, one at a time (later ones wait their turn).
pub struct Toasts {
    settings: Arc<RwLock<Settings>>,
    win: Option<Layered>,
    queue: VecDeque<(String, Rgba)>,
    current: Option<(String, Rgba, Instant, Rect)>,
    last_client: Rect,
    excluded: Option<bool>,
}

impl Toasts {
    pub fn new(settings: Arc<RwLock<Settings>>) -> Toasts {
        Toasts { settings, win: None, queue: VecDeque::new(), current: None, last_client: (0, 0, 0, 0), excluded: None }
    }

    /// A notice: to the meter if it takes it, else queued here over the game window (client area). None with
    /// "Notifications" off.
    pub fn notify(&mut self, head: &str, accent: Rgba, client: Rect) {
        self.notify_by(head, accent, client, try_hand_off);
    }

    /// notify, with the hand-over to the meter as `hand_off` (the tests' stand-in for try_hand_off).
    fn notify_by(&mut self, head: &str, accent: Rgba, client: Rect, hand_off: impl FnOnce(&str, Rgba, Rect) -> bool) {
        // the switch first, as the C# did: handed over before it was looked at, a notice showed with "Notifications"
        // off whenever the meter's overlay ran, which is the usual case
        if !self.settings.read().unwrap().show_notifications {
            return;
        }
        if hand_off(head, accent, client) || client.2 <= 0 {
            return;
        }
        self.queue.push_back((head.to_string(), accent));
        self.last_client = client;
    }

    fn apply_capture_mode(&mut self) {
        let Some(w) = &self.win else { return };
        let exclude = !self.settings.read().unwrap().recordable_overlay || shared_capture::clean_now();
        own_overlays::in_capture("toast", w.hwnd, self.current.is_some() && !exclude);
        if self.excluded != Some(exclude) {
            self.excluded = Some(exclude);
            w.hide_from_capture(exclude);
        }
    }

    /// One display-rate step of the card that's up (call ~60 times a second on the window thread).
    pub fn tick(&mut self) {
        if self.current.is_none() {
            let Some((t, a)) = self.queue.pop_front() else { return };
            if self.win.is_none() {
                self.win = Layered::new();
                if let Some(w) = &self.win {
                    w.click_through(true);
                }
            }
            let c = self.last_client;
            let ui = vision::ui_scale(c.2.max(1) as usize, c.3.max(1) as usize);
            let (w, h) = (((W + MARGIN + 8.0) * ui).ceil() as i32, ((H + MARGIN + 16.0) * ui).ceil() as i32);
            let spot = (c.0 + c.2 - w, c.1, w, h);
            own_overlays::set("toast", Some(spot));
            self.current = Some((t, a, Instant::now(), spot));
        }
        self.apply_capture_mode();
        own_overlays::tick();
        let (t, accent, start, spot) = self.current.clone().unwrap();
        let e = start.elapsed().as_secs_f64();
        let (mut dx, mut dy) = (0.0, 0.0);
        if e < DROP {
            dy = -(H + MARGIN + 10.0) * (1.0 - back_out(e / DROP));
        } else if e > DROP + HOLD {
            let k = ((e - DROP - HOLD) / LEAVE).min(1.0);
            dx = (W + MARGIN + 20.0) * k * k * k;
        }
        if e >= DROP + HOLD + LEAVE {
            self.current = None;
            if self.queue.is_empty() {
                own_overlays::set("toast", None);
                if let Some(w) = &self.win {
                    own_overlays::in_capture("toast", w.hwnd, false);
                    w.park();
                }
            }
            return;
        }
        let ui = vision::ui_scale(self.last_client.2.max(1) as usize, self.last_client.3.max(1) as usize);
        let Some(mut cv) = Canvas::new(spot.2 as u32, spot.3 as u32) else { return };
        cv.push_scale(ui, ui);
        let (x, y) = (f64::from(spot.2) / ui - MARGIN - W + dx, MARGIN + dy);
        // a soft shadow under the card
        for (i, a) in [(6.0, 18u8), (4.0, 26), (2.0, 34)] {
            cv.fill(rounded_rect(x - i + 1.0, y - i + 3.0, W + 2.0 * i - 2.0, H + 2.0 * i - 2.0, 12.0 + i), &Fill::Solid(Rgba(0, 0, 0, a)));
        }
        cv.draw(rounded_rect(x, y, W, H, 12.0), Some(&Fill::Solid(Rgba(20, 22, 31, 235))), Some(Pen::new(Rgba(255, 255, 255, 70), 1.0)));
        cv.fill(rounded_rect(x + 14.0, y + 10.0, 5.0, H - 20.0, 3.0), &Fill::Solid(accent));
        // the text, cut short with an ellipsis when it doesn't fit
        let room = W - 14.0 - 5.0 - 12.0 - 16.0;
        let mut shown = t.clone();
        while text::measure(&shown, 17.0).0 > room && shown.chars().count() > 1 {
            let mut c: Vec<char> = shown.trim_end_matches('…').chars().collect();
            c.pop();
            shown = c.into_iter().collect::<String>() + "…";
        }
        let th = text::measure(&shown, 17.0).1;
        if let Some(p) = text::path_at(&shown, 17.0, x + 14.0 + 5.0 + 12.0, y + (H - th) / 2.0) {
            cv.text_path(&p, Rgba::WHITE, None);
        }
        cv.pop_transform();
        let pix = cv.finish();
        if let Some(w) = &self.win {
            w.show_premultiplied(pix.data(), spot.2, spot.3, spot.0, spot.1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Notifications" off: no notice goes to the meter and none is queued here. On: the meter gets it when it takes
    /// notices, else it waits here for its card.
    #[test]
    fn nothing_is_shown_or_handed_over_with_notifications_off() {
        let settings = Arc::new(RwLock::new(Settings::default()));
        let mut toasts = Toasts::new(settings.clone());
        let (accent, client) = (Rgba(0x5c, 0xb8, 0xff, 255), (0, 0, 1920, 1080));
        let asked = std::cell::Cell::new(0);
        let meter = |takes: bool| { let asked = &asked; move |_: &str, _: Rgba, _: Rect| { asked.set(asked.get() + 1); takes } };

        settings.write().unwrap().show_notifications = false;
        toasts.notify_by("Chaos Mode chosen.", accent, client, meter(true));
        assert_eq!((asked.get(), toasts.queue.len()), (0, 0), "handed over or queued with the switch off");

        settings.write().unwrap().show_notifications = true;
        toasts.notify_by("Chaos Mode chosen.", accent, client, meter(true));
        assert_eq!((asked.get(), toasts.queue.len()), (1, 0), "the meter took it: not shown twice");
        toasts.notify_by("Chaos Mode chosen.", accent, client, meter(false));
        assert_eq!((asked.get(), toasts.queue.len()), (2, 1), "the meter didn't take it: our own card");
        toasts.notify_by("Chaos Mode chosen.", accent, (0, 0, 0, 0), meter(false));
        assert_eq!(toasts.queue.len(), 1, "no game window to show it over");
    }

    #[test]
    fn back_ease_overshoots_then_lands() {
        assert!(back_out(0.0).abs() < 1e-9);
        assert!((back_out(1.0) - 1.0).abs() < 1e-9);
        assert!((0.3..0.98).any_over(back_out));
    }

    trait AnyOver {
        fn any_over(self, f: fn(f64) -> f64) -> bool;
    }

    impl AnyOver for std::ops::Range<f64> {
        fn any_over(self, f: fn(f64) -> f64) -> bool {
            (0..100).map(|i| self.start + (self.end - self.start) * f64::from(i) / 100.0).any(|t| f(t) > 1.0)
        }
    }
}
