//! Reads the game picture the aLogs app shares with its modules (`Local\aLogs.Capture.v2`, layout in alogs-capture's
//! shared.rs and docs/shared-capture.md), so the game is captured once for all tools. While the app runs, pictures
//! come only from here; standalone, `try_grab` gives None and the window is copied directly. We ask for the box
//! around everything read in the last two seconds. Port of `SharedCapture.cs` (Dev switches off).

use crate::cv::Mat;
use alogs_capture::shared::View;
use std::cell::Cell;
use std::sync::atomic::{Ordering, fence};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const NAME: &str = "Local\\aLogs.Capture.v2";
const MAGIC: u32 = 0x5043_4C41;
const HEADER: usize = 4096;
const SLOTS: usize = 16;
const SLOT_BASE: usize = 64;
const PICTURE_BASE: usize = 320;
const PICTURE: usize = 64;
const HEARTBEAT: usize = 2048;
const HEARTBEAT_STALE_MS: i64 = 5000;
const AREA_KEEP_MS: u64 = 2000;
const STILL_AGE_MS: f64 = 1500.0;
const CLEAN_UNTIL: usize = 1344;
const OVERLAY_BASE: usize = 1352;
const OVERLAY_BLOCK: usize = 144;
const OVERLAY_MAX: usize = 8;
const OVERLAY_STALE_MS: i64 = 3000;
pub const OWNER_SANCTUM: usize = 1;

/// (x, y, w, h).
pub type Rect = (i32, i32, i32, i32);

struct State {
    view: Option<Arc<View>>,
    next_open: u64,
    slot: Option<usize>,
    last_call: u64,
    interval: f64,
    announced: bool,
    last_used: u64,
    areas: Vec<(u64, Rect)>,
    bx: Rect,
    request: u32,
    screen_copy: bool,
    copy_ms: f64,
    enabled: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    view: None, next_open: 0, slot: None, last_call: 0, interval: 250.0, announced: false, last_used: 0, areas: Vec::new(),
    bx: (0, 0, 0, 0), request: 0, screen_copy: false, copy_ms: 0.0, enabled: true,
});

thread_local! {
    /// Which picture this thread's last copy came from.
    pub static LAST_PICTURE: Cell<(usize, u64)> = const { Cell::new((0, 0)) };
    static LAST_SUB: Cell<Rect> = const { Cell::new((0, 0, 0, 0)) };
    /// The last call found that same picture and part again and so copied nothing.
    pub static SAME_AS_LAST: Cell<bool> = const { Cell::new(false) };
    /// How old this thread's last copy was when taken (ms).
    pub static LAST_AGE_MS: Cell<i64> = const { Cell::new(0) };
}

fn tick() -> u64 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    // +10 s so a fresh process doesn't read 0 as "never"
    T0.get_or_init(Instant::now).elapsed().as_millis() as u64 + 10_000
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn pid() -> u32 {
    std::process::id()
}

fn intersect(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.0.max(b.0), a.1.max(b.1));
    let (x1, y1) = ((a.0 + a.2).min(b.0 + b.2), (a.1 + a.3).min(b.1 + b.3));
    if x1 <= x0 || y1 <= y0 { (0, 0, 0, 0) } else { (x0, y0, x1 - x0, y1 - y0) }
}

impl State {
    fn open(&mut self) -> bool {
        if self.view.is_some() {
            return true;
        }
        let t = tick();
        if t < self.next_open {
            return false;
        }
        self.next_open = t + 2000;
        match View::open(NAME, 0) {
            Some(v) if v.u32(0) == MAGIC => {
                self.view = Some(Arc::new(v));
                true
            }
            _ => {
                self.close();
                false
            }
        }
    }

    /// A picture being converted keeps its own handle on the mapping: it is let go once the last one is done.
    fn close(&mut self) {
        self.view = None;
        self.slot = None;
    }

    fn v(&self) -> &View {
        self.view.as_ref().unwrap()
    }

    /// Keeps our request slot alive: pictures are taken only while someone asks.
    fn request_slot(&mut self, now: i64) -> bool {
        let me = pid();
        if self.slot.is_none_or(|s| self.v().u32(SLOT_BASE + s * 16 + 12) != me) {
            self.slot = None;
            self.bx = (0, 0, 0, 0);
            for i in 0..SLOTS {
                let at = SLOT_BASE + i * 16;
                if (self.v().u64(at) as i64) < now || self.v().u32(at + 12) == me {
                    self.slot = Some(i);
                    break;
                }
            }
            let Some(s) = self.slot else { return false };
            self.v().set_u8(PICTURE_BASE + s * PICTURE + 60, 0); // no shrink (its last owner may have asked)
        }
        let a = SLOT_BASE + self.slot.unwrap() * 16;
        let v = self.v();
        v.set_u32(a + 12, me);
        v.set_u32(a + 8, self.interval as u32);
        v.set_u64(a, (now + 2500) as u64);
        true
    }

    fn ask_area(&mut self, area: Rect) {
        let t = tick();
        self.areas.retain(|a| t - a.0 <= AREA_KEEP_MS && a.1 != area);
        self.areas.push((t, area));
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, 0, 0);
        for (_, a) in &self.areas {
            x0 = x0.min(a.0);
            y0 = y0.min(a.1);
            x1 = x1.max(a.0 + a.2);
            y1 = y1.max(a.1 + a.3);
        }
        let want = (x0, y0, x1 - x0, y1 - y0);
        let at = PICTURE_BASE + self.slot.unwrap() * PICTURE;
        if want == self.bx && self.v().u32(at + 8) == self.request {
            return;
        }
        self.bx = want;
        self.request = self.request % u32::MAX + 1;
        let v = self.v();
        v.set_u16(at, x0 as u16);
        v.set_u16(at + 2, y0 as u16);
        v.set_u16(at + 4, x1 as u16);
        v.set_u16(at + 6, y1 as u16);
        v.set_u32(at + 8, self.request);
    }
}

fn seq(v: &View, at: usize) -> u64 {
    fence(Ordering::SeqCst);
    let s = v.u64(at);
    fence(Ordering::SeqCst);
    s
}

/// Part of the client (px) as shares of its size (0-65535).
fn share(sub: Rect, cw: i32, ch: i32) -> Rect {
    let (cw, ch) = (i64::from(cw), i64::from(ch));
    let sx0 = (i64::from(sub.0) * 65535 / cw) as i32;
    let sy0 = (i64::from(sub.1) * 65535 / ch) as i32;
    let sx1 = 65535.min((i64::from(sub.0 + sub.2) * 65535 + cw - 1) / cw) as i32;
    let sy1 = 65535.min((i64::from(sub.1 + sub.3) * 65535 + ch - 1) / ch) as i32;
    (sx0, sy0, sx1 - sx0, sy1 - sy0)
}

pub fn set_enabled(on: bool) {
    STATE.lock().unwrap().enabled = on;
}

/// True while frames have recently come from the aLogs app.
pub fn in_use() -> bool {
    let s = STATE.lock().unwrap();
    s.last_used != 0 && tick() - s.last_used < 3000
}

/// The aLogs app is running and sharing its capture: pictures then come only from it.
pub fn app_present() -> bool {
    let mut s = STATE.lock().unwrap();
    if !s.enabled || !s.open() {
        return false;
    }
    let beat = s.v().u64(HEARTBEAT) as i64;
    if beat != 0 && now_ms() - beat > HEARTBEAT_STALE_MS {
        s.close(); // the app is gone
        return false;
    }
    true
}

/// The last picture may have our overlays in it (Desktop Duplication): our overlay must be hidden from capture.
pub fn screen_copy() -> bool {
    STATE.lock().unwrap().screen_copy
}

/// How long the app took to read our last picture off the graphics card (ms).
pub fn copy_ms() -> f64 {
    STATE.lock().unwrap().copy_ms
}

/// A proof is being taken (the meter): our overlays stay out of screen captures meanwhile.
pub fn clean_now() -> bool {
    let mut s = STATE.lock().unwrap();
    s.open() && now_ms() < s.v().u64(CLEAN_UNTIL) as i64
}

/// Our overlay windows' spots (screen px), for the meter and the app to cover in their pictures.
pub fn publish_overlays(owner: usize, rects: &[Rect]) {
    let mut s = STATE.lock().unwrap();
    if !s.open() {
        return;
    }
    let v = s.v();
    let at = OVERLAY_BASE + owner * OVERLAY_BLOCK;
    let n = rects.len().min(OVERLAY_MAX);
    for (k, r) in rects.iter().take(n).enumerate() {
        v.set_i32(at + 16 + 16 * k, r.0);
        v.set_i32(at + 20 + 16 * k, r.1);
        v.set_i32(at + 24 + 16 * k, r.0 + r.2);
        v.set_i32(at + 28 + 16 * k, r.1 + r.3);
    }
    v.set_u32(at + 8, pid());
    v.set_u32(at + 12, n as u32);
    v.set_u64(at, now_ms() as u64);
}

/// Every aLogs overlay's spot now (screen px), ours included unless skipped.
pub fn overlays(skip_owner: Option<usize>) -> Vec<Rect> {
    let mut s = STATE.lock().unwrap();
    let mut all = vec![];
    if !s.open() {
        return all;
    }
    let v = s.v();
    let now = now_ms();
    for owner in 0..3 {
        if Some(owner) == skip_owner {
            continue;
        }
        let at = OVERLAY_BASE + owner * OVERLAY_BLOCK;
        if now - v.u64(at) as i64 > OVERLAY_STALE_MS {
            continue;
        }
        let n = (v.u32(at + 12) as usize).min(OVERLAY_MAX);
        for k in 0..n {
            let (x0, y0, x1, y1) = (v.i32(at + 16 + 16 * k), v.i32(at + 20 + 16 * k), v.i32(at + 24 + 16 * k), v.i32(at + 28 + 16 * k));
            if x1 > x0 && y1 > y0 {
                all.push((x0, y0, x1 - x0, y1 - y0));
            }
        }
    }
    all
}

/// That part isn't wanted any more: the box asked for shrinks on the next read.
pub fn forget(cw: i32, ch: i32, sub: Rect) {
    let mut s = STATE.lock().unwrap();
    if !s.enabled || cw <= 0 || ch <= 0 {
        return;
    }
    let sub = intersect(sub, (0, 0, cw, ch));
    if sub.2 <= 0 || sub.3 <= 0 {
        return;
    }
    let area = share(sub, cw, ch);
    s.areas.retain(|a| a.1 != area);
}

/// The part `sub` (client px) of the shared picture as BGR, or None when there is no fresh picture of this window at
/// this size holding that part. `unless_same`: the very picture and part this thread copied last gives None with
/// SAME_AS_LAST set. `still`: a read of the in-game map page, where a picture up to 1.5 s old will do.
pub fn try_grab(hwnd: usize, cw: i32, ch: i32, sub: Rect, unless_same: bool, still: bool) -> Option<Mat> {
    SAME_AS_LAST.with(|c| c.set(false));
    let mut now = 0i64;
    let mut sub = sub;
    let mut mapped: Option<Arc<View>> = None;
    for attempt in 0..4 {
        let (at, sq, flags, src_off, stride);
        {
            let mut s = STATE.lock().unwrap();
            if !s.enabled {
                return None;
            }
            if attempt == 0 {
                let t = tick();
                // ask at the pace we're being called
                if s.last_call != 0 {
                    s.interval = (0.7 * s.interval + 0.3 * (t - s.last_call) as f64).clamp(16.0, 1000.0);
                }
                s.last_call = t;
                if !s.open() {
                    return None;
                }
                now = now_ms();
                if !s.request_slot(now) {
                    return None;
                }
                sub = intersect(sub, (0, 0, cw, ch));
                if sub.2 <= 0 || sub.3 <= 0 {
                    return None;
                }
                s.ask_area(share(sub, cw, ch));
                if s.v().u64(40) != hwnd as u64 {
                    return None;
                }
                mapped = s.view.clone();
            } else if s.slot.is_none() || !s.view.as_ref().zip(mapped.as_ref()).is_some_and(|(a, b)| Arc::ptr_eq(a, b)) {
                return None; // closed or opened again meanwhile
            }
            let v = s.v();
            at = PICTURE_BASE + s.slot.unwrap() * PICTURE;
            sq = seq(v, at + 16);
            if sq & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            if v.u32(at + 12) != s.request {
                return None; // our area isn't in yet
            }
            flags = v.u32(at + 48);
            if flags & 1 == 0 || i32::from(v.u16(at + 56)) != cw || i32::from(v.u16(at + 58)) != ch {
                return None;
            }
            if v.u8(at + 61) > 1 {
                return None; // shrunk: not what we asked for
            }
            let age = (now - v.u64(at + 24) as i64) as f64;
            if age > (if still { STILL_AGE_MS } else { 100.0 }).max(2.0 * s.interval) {
                return None;
            }
            let offset = v.u32(at + 32) as usize;
            stride = v.u32(at + 36) as usize;
            let pic = (i32::from(v.u16(at + 40)), i32::from(v.u16(at + 42)), i32::from(v.u16(at + 44)), i32::from(v.u16(at + 46)));
            if intersect(pic, sub) != sub {
                return None; // a picture of an older, smaller box
            }
            if unless_same && LAST_PICTURE.with(Cell::get) == (at, sq) && LAST_SUB.with(Cell::get) == sub {
                if seq(v, at + 16) != sq {
                    continue;
                }
                LAST_AGE_MS.with(|c| c.set(now - v.u64(at + 24) as i64));
                s.last_used = tick();
                SAME_AS_LAST.with(|c| c.set(true));
                return None;
            }
            src_off = HEADER + offset + (sub.1 - pic.1) as usize * stride + (sub.0 - pic.0) as usize * 4;
        }
        // the conversion outside the lock; the sequence number read again throws away a picture written meanwhile
        let view = mapped.as_ref()?;
        let (w, h) = (sub.2 as usize, sub.3 as usize);
        let mut out = Mat::new(w, h, 3);
        unsafe {
            let base = view.ptr().add(src_off);
            for y in 0..h {
                let row = std::slice::from_raw_parts(base.add(y * stride), w * 4);
                for (d, p) in out.data[y * w * 3..(y + 1) * w * 3].chunks_exact_mut(3).zip(row.chunks_exact(4)) {
                    d.copy_from_slice(&p[..3]);
                }
            }
        }
        let mut s = STATE.lock().unwrap();
        if seq(view, at + 16) == sq {
            LAST_PICTURE.with(|c| c.set((at, sq)));
            LAST_SUB.with(|c| c.set(sub));
            LAST_AGE_MS.with(|c| c.set(now - view.u64(at + 24) as i64));
            s.screen_copy = flags & 2 != 0;
            s.copy_ms = f64::from(view.u32(at + 52)) / 1000.0;
            if !s.announced {
                s.announced = true;
                crate::log::write("using the aLogs app's shared capture");
            }
            s.last_used = tick();
            return Some(out);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_round_outward() {
        assert_eq!(share((0, 0, 1920, 1080), 1920, 1080), (0, 0, 65535, 65535));
        let s = share((100, 50, 400, 200), 1920, 1080);
        assert!(i64::from(s.0) * 1920 / 65535 <= 100 && i64::from(s.0 + s.2) * 1920 / 65535 >= 499);
        assert_eq!(intersect((0, 0, 10, 10), (5, 5, 10, 10)), (5, 5, 5, 5));
        assert_eq!(intersect((0, 0, 10, 10), (20, 5, 10, 10)), (0, 0, 0, 0));
    }
}
