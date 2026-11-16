//! Where Sanctum Helper's overlay windows are on screen (the map panel, the notification card), shared with the aLogs
//! app and meter so every reader of a picture of the screen covers them and they can stay in screenshots and
//! recordings. A spot just left is kept half a second; published on every change and once a second. Port of
//! `OwnOverlays.cs`.

use crate::shared_capture::{self, OWNER_SANCTUM, Rect};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
struct State {
    current: BTreeMap<String, Rect>,
    left: Vec<(Rect, Instant)>,
    published: Option<Instant>,
    /// each window's handle while it is in screen captures
    in_capture: BTreeMap<String, usize>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(State::default))
}

fn meets(a: Rect, b: Rect) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// Whether a window (by the same name as its spot) is in screen captures now.
pub fn in_capture(who: &str, hwnd: usize, shown: bool) {
    with(|s| {
        if shown {
            s.in_capture.insert(who.to_string(), hwnd);
        } else {
            s.in_capture.remove(who);
        }
    });
}

/// Our spots in screen captures now.
pub fn shown() -> Vec<Rect> {
    with(|s| s.current.iter().filter(|(k, _)| s.in_capture.contains_key(*k)).map(|(_, v)| *v).collect())
}

/// Our windows over that part of the screen and in screen captures.
pub fn over(screen: Rect) -> Vec<usize> {
    with(|s| s.current.iter().filter(|(k, v)| s.in_capture.contains_key(*k) && meets(**v, screen)).map(|(k, _)| s.in_capture[k]).collect())
}

pub fn set(who: &str, spot: Option<Rect>) {
    with(|s| {
        let now = Instant::now();
        let was = s.current.get(who).copied();
        match spot.filter(|r| r.2 > 0 && r.3 > 0) {
            Some(r) => {
                if was == Some(r) {
                    return;
                }
                if let Some(w) = was {
                    s.left.push((w, now + Duration::from_millis(500)));
                }
                s.current.insert(who.to_string(), r);
            }
            None => {
                let Some(w) = was else { return };
                s.left.push((w, now + Duration::from_millis(500)));
                s.current.remove(who);
            }
        }
        publish(s, now);
    });
}

/// Called often (each picture): publishes again once a second, and drops spots left long enough ago.
pub fn tick() {
    with(|s| {
        let now = Instant::now();
        if s.published.is_none_or(|p| now - p > Duration::from_secs(1)) || s.left.iter().any(|l| l.1 < now) {
            publish(s, now);
        }
    });
}

fn publish(s: &mut State, now: Instant) {
    s.left.retain(|l| l.1 >= now);
    s.published = Some(now);
    if !shared_capture::app_present() {
        return;
    }
    let all: Vec<Rect> = s.current.values().copied().chain(s.left.iter().map(|l| l.0)).collect();
    shared_capture::publish_overlays(OWNER_SANCTUM, &all);
}
