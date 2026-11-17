//! Reader for the aLogs app's shared game picture (`Local\aLogs.Capture.v2`; layout in the app's capture.rs and
//! docs/shared-capture.md) and its audio cues (`LocalLogs.Audio.v1`). Port of `SharedCapture` and `SharedAudio` in
//! aLogs B16 `bossbar/wincapture.py`.

use alogs_pil::{Image, Mode};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const NAME: &str = "Local\\aLogs.Capture.v2";
pub const MAGIC: u32 = 0x50434C41;
pub const HEADER: usize = 4096;
pub const SLOTS: usize = 16;
pub const SLOT_BASE: usize = 64;
pub const PICTURE_BASE: usize = 320;
pub const PICTURE: usize = 64;
pub const RECT_BASE: usize = 2056;
pub const RECT_SIZE: usize = 120;
pub const RECTS: usize = 7;
pub const HEARTBEAT: usize = 2048;
pub const HEARTBEAT_STALE: u64 = 5000;
pub const CLEAN_UNTIL: usize = 1344;
pub const OVERLAY_BASE: usize = 1352;
pub const OVERLAY_BLOCK: usize = 144;
pub const OVERLAY_MAX: usize = 8;
pub const OVERLAY_STALE: u64 = 3000;
pub const GAME_ONLY_UNTIL: usize = 1792;
/// "Game fast until" (unix ms): the burst of the game window alone is a script's, which wants its pictures quickly.
pub const GAME_FAST_UNTIL: usize = 1800;
pub const OWNER_METER: usize = 0;
pub const OWNER_SANCTUM: usize = 1;
pub const OWNER_APP: usize = 2;
pub const WHOLE_FRESH_MS: f64 = 1500.0;
pub const AREA_KEEP: f64 = 2.0;
pub const AREA_SHORT: f64 = 0.3;
pub const WAIT: f64 = 0.12;

fn now_s() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

pub fn source_name(flags: u32) -> &'static str {
    match flags >> 4 & 15 {
        1 => "window copy (tabbed out)",
        3 => "Desktop Duplication",
        4 => "window capture",
        _ => "?",
    }
}

/// A mapped view of a named shared memory block.
pub struct View {
    base: *mut u8,
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

unsafe impl Send for View {}
// Reads and writes go through the shared mapping, which other processes write too: no Rust-side aliasing.
unsafe impl Sync for View {}

impl View {
    #[cfg(windows)]
    pub fn open(name: &str, size: usize) -> Option<View> {
        use windows_sys::Win32::System::Memory::{FILE_MAP_ALL_ACCESS, MapViewOfFile, OpenFileMappingW};
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let h = OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, wide.as_ptr());
            if h.is_null() {
                return None;
            }
            let v = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, size);
            if v.Value.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(h);
                return None;
            }
            Some(View { base: v.Value as *mut u8, handle: h })
        }
    }
    #[cfg(not(windows))]
    pub fn open(_: &str, _: usize) -> Option<View> {
        None
    }

    /// The start of the mapping (pixels are read straight from it).
    pub fn ptr(&self) -> *const u8 {
        self.base
    }

    pub fn u8(&self, at: usize) -> u8 {
        unsafe { std::ptr::read_volatile(self.base.add(at)) }
    }
    pub fn u16(&self, at: usize) -> u16 {
        unsafe { std::ptr::read_volatile(self.base.add(at) as *const u16) }
    }
    pub fn u32(&self, at: usize) -> u32 {
        unsafe { std::ptr::read_volatile(self.base.add(at) as *const u32) }
    }
    pub fn u64(&self, at: usize) -> u64 {
        unsafe { std::ptr::read_volatile(self.base.add(at) as *const u64) }
    }
    pub fn i32(&self, at: usize) -> i32 {
        unsafe { std::ptr::read_volatile(self.base.add(at) as *const i32) }
    }
    pub fn f32(&self, at: usize) -> f32 {
        unsafe { std::ptr::read_volatile(self.base.add(at) as *const f32) }
    }
    pub fn set_u8(&self, at: usize, v: u8) {
        unsafe { std::ptr::write_volatile(self.base.add(at), v) }
    }
    pub fn set_u16(&self, at: usize, v: u16) {
        unsafe { std::ptr::write_volatile(self.base.add(at) as *mut u16, v) }
    }
    pub fn set_u32(&self, at: usize, v: u32) {
        unsafe { std::ptr::write_volatile(self.base.add(at) as *mut u32, v) }
    }
    pub fn set_u64(&self, at: usize, v: u64) {
        unsafe { std::ptr::write_volatile(self.base.add(at) as *mut u64, v) }
    }
    pub fn set_i32(&self, at: usize, v: i32) {
        unsafe { std::ptr::write_volatile(self.base.add(at) as *mut i32, v) }
    }
    /// BGRX rows at `at` (stride bytes apart) as an RGB image.
    pub fn bgrx(&self, at: usize, w: usize, h: usize, stride: usize) -> Image {
        bgrx_rgb(&self.rows(at, w, h, stride), w, h)
    }

    /// `h` rows of `w` BGRX pixels at `at`, `stride` bytes apart, copied out as they are (w * 4 bytes a row). The
    /// readers copy a picture this way while they check its seq and turn it into an image after (bgrx_rgb): at 4K the
    /// turning took 9-12 ms more, and a picture the app rewrote meanwhile had to be read again ("the picture was
    /// rewritten while it was read", 17 Nov).
    pub fn rows(&self, at: usize, w: usize, h: usize, stride: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            out.extend_from_slice(unsafe { std::slice::from_raw_parts(self.base.add(at + y * stride), w * 4) });
        }
        out
    }
}

/// BGRX pixels, `w` x `h` with w * 4 bytes a row (View::rows), as an RGB image.
pub fn bgrx_rgb(raw: &[u8], w: usize, h: usize) -> Image {
    let mut data = Vec::with_capacity(w * h * 3);
    for p in raw[..w * h * 4].chunks_exact(4) {
        data.extend_from_slice(&[p[2], p[1], p[0]]);
    }
    Image::from_raw(Mode::Rgb, w, h, data)
}

impl Drop for View {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Memory::{MEMORY_MAPPED_VIEW_ADDRESS, UnmapViewOfFile};
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.base as _ });
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

type Box4 = (u16, u16, u16, u16);

/// The shared capture's reader: one request slot and its own picture buffer.
pub struct SharedCapture {
    pub owner: u32,
    view: Option<View>,
    slot: Option<usize>,
    next_open: f64,
    last_call: f64,
    pub interval: f64,
    pub screen_copy: bool,
    pub copy_us: u32,
    /// Why the last read gave no picture.
    pub why: String,
    /// Where the last picture came from.
    pub source: String,
    areas: Vec<(f64, Box4)>,
    box_: Option<Vec<Box4>>,
    request: u32,
    boxes: VecDeque<(u32, Vec<Box4>)>,
    /// Area requests made, and how many asked for more rows.
    pub asked: u64,
    pub grown: u64,
    pub picture_time: f64,
    pub frame_time: f64,
    /// The Dev switch area-keep-short.
    pub area_short: bool,
}

impl SharedCapture {
    /// `tag` > 0: a second reader in this process with a slot of its own.
    pub fn new(tag: u32) -> SharedCapture {
        SharedCapture {
            owner: std::process::id() | (tag << 28),
            view: None,
            slot: None,
            next_open: 0.0,
            last_call: 0.0,
            interval: 250.0,
            screen_copy: false,
            copy_us: 0,
            why: String::new(),
            source: String::new(),
            areas: vec![],
            box_: None,
            request: 0,
            boxes: VecDeque::new(),
            asked: 0,
            grown: 0,
            picture_time: 0.0,
            frame_time: 0.0,
            area_short: false,
        }
    }

    fn close(&mut self) {
        self.view = None;
        self.slot = None;
    }

    fn open(&mut self, now: f64) -> bool {
        if self.view.is_some() {
            return true;
        }
        if now < self.next_open {
            return false;
        }
        self.next_open = now + 2.0;
        match View::open(NAME, 0) {
            Some(v) if v.u32(0) == MAGIC => {
                self.view = Some(v);
                true
            }
            _ => false,
        }
    }

    fn none(&mut self, why: &str) -> Option<Image> {
        self.why = why.to_string();
        None
    }

    fn pace(&mut self, now: f64, floor: f64) {
        if self.last_call != 0.0 {
            self.interval = (0.7 * self.interval + 0.3 * (now - self.last_call) * 1000.0).clamp(floor, 1000.0);
        }
        self.last_call = now;
    }

    fn request_slot(&mut self, now_ms: u64) -> bool {
        let v = self.view.as_ref().unwrap();
        let pid = self.owner;
        if self.slot.is_none_or(|s| v.u32(SLOT_BASE + s * 16 + 12) != pid) {
            let free = (0..SLOTS).find(|&i| v.u64(SLOT_BASE + i * 16) < now_ms || v.u32(SLOT_BASE + i * 16 + 12) == pid);
            let Some(f) = free else { return false };
            self.slot = Some(f);
            self.box_ = None;
            self.boxes.clear();
            v.set_u8(PICTURE_BASE + f * PICTURE + 60, 0);
        }
        let at = SLOT_BASE + self.slot.unwrap() * 16;
        v.set_u32(at + 12, pid);
        v.set_u32(at + 8, self.interval as u32);
        v.set_u64(at, now_ms + 2500);
        true
    }

    fn remember(&mut self, req: u32, b: Vec<Box4>) {
        while self.boxes.len() > 3 {
            self.boxes.pop_front();
        }
        self.boxes.push_back((req, b));
    }

    fn ask_area(&mut self, now: f64, area: Box4) {
        let keep = if self.area_short { AREA_SHORT } else { AREA_KEEP };
        self.areas.retain(|(t, a)| now - t < keep && *a != area);
        self.areas.push((now, area));
        let b = (
            self.areas.iter().map(|a| a.1.0).min().unwrap(),
            self.areas.iter().map(|a| a.1.1).min().unwrap(),
            self.areas.iter().map(|a| a.1.2).max().unwrap(),
            self.areas.iter().map(|a| a.1.3).max().unwrap(),
        );
        let slot = self.slot.unwrap();
        let v = self.view.as_ref().unwrap();
        let at = PICTURE_BASE + slot * PICTURE;
        if self.box_.as_deref() != Some(&[b][..]) || v.u32(at + 8) != self.request {
            self.asked += 1;
            if let Some(old) = self.box_.as_ref().filter(|o| o.len() == 1)
                && b.3 > old[0].3
            {
                self.grown += 1;
            }
            self.box_ = Some(vec![b]);
            self.request = (self.request % 0xFFFF_FFFF) + 1;
            v.set_u32(RECT_BASE + slot * RECT_SIZE, 0);
            for (k, x) in [b.0, b.1, b.2, b.3].into_iter().enumerate() {
                v.set_u16(at + 2 * k, x);
            }
            v.set_u32(at + 8, self.request);
            let r = self.request;
            self.remember(r, vec![b]);
        }
    }

    fn times(&mut self, at: usize) {
        let v = self.view.as_ref().unwrap();
        self.picture_time = v.u64(at + 24) as f64 / 1000.0;
        self.frame_time = self.picture_time - f64::from(v.u16(at + 62)) / 1000.0;
    }

    /// The newest picture of this window (its top `top_fraction`, or all of it), or None (`why` says why).
    pub fn read(&mut self, hwnd: usize, top_fraction: Option<f64>) -> Option<Image> {
        let now = now_s();
        self.pace(now, 25.0);
        if !self.open(now) {
            return self.none("the app shares no picture (not running, or its capture is off)");
        }
        let now_ms = (now * 1000.0) as u64;
        if !self.request_slot(now_ms) {
            return self.none("no free reader slot");
        }
        let top: u16 = match top_fraction {
            None => 65535,
            Some(f) if f == 0.0 => 65535,
            Some(f) => 65535u32.min((65535.0 * f) as u32 + 1) as u16,
        };
        self.ask_area(now, (0, 0, 65535, top));
        let slot = self.slot.unwrap();
        let at = PICTURE_BASE + slot * PICTURE;
        if self.view.as_ref().unwrap().u64(40) != hwnd as u64 {
            return self.none("the app is capturing another window");
        }
        let start = Instant::now();
        let until = Duration::from_secs_f64(WAIT);
        let area_until = if self.source == source_name(1 << 4) { Duration::ZERO } else { until };
        loop {
            let v = self.view.as_ref().unwrap();
            let seq = v.u64(at + 16);
            if seq & 1 == 1 {
                if start.elapsed() >= until {
                    return self.none("the picture was being written");
                }
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            let served = v.u32(at + 12);
            let ok_box = self.boxes.iter().find(|b| b.0 == served).is_some_and(|b| b.1.len() == 1 && b.1[0].0 == 0 && b.1[0].1 == 0 && b.1[0].2 == 65535 && b.1[0].3 >= top);
            if !ok_box {
                if start.elapsed() >= area_until {
                    return self.none("our area is not in the picture yet");
                }
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            let flags = v.u32(at + 48);
            let mut fresh = if flags >> 4 & 15 == 1 { 600.0 } else { (2.0 * self.interval).max(150.0) };
            if top_fraction.is_none_or(|f| f == 0.0) {
                fresh = fresh.max(WHOLE_FRESH_MS);
            }
            if flags & 1 == 0 || now_ms as f64 - v.u64(at + 24) as f64 > fresh {
                return self.none("no fresh picture from the app (no valid frame, or an old one)");
            }
            if v.u8(at + 61) > 1 {
                return self.none("the picture came shrunk");
            }
            let (offset, stride) = (v.u32(at + 32) as usize, v.u32(at + 36) as usize);
            let (x, y, w, h) = (v.u16(at + 40) as usize, v.u16(at + 42) as usize, v.u16(at + 44) as usize, v.u16(at + 46) as usize);
            let (cw, ch) = (v.u16(at + 56) as usize, v.u16(at + 58) as usize);
            let rows = match top_fraction {
                Some(f) if f != 0.0 => ch.min((ch as f64 * f) as usize),
                _ => ch,
            };
            if x + w > cw || y + h > ch || w == 0 || h == 0 {
                return self.none("the picture does not fit the window");
            }
            let got = h.min(rows.saturating_sub(y));
            let raw = (got > 0).then(|| v.rows(HEADER + offset, w, got, stride));
            if v.u64(at + 16) == seq {
                self.screen_copy = flags & 2 != 0;
                self.copy_us = v.u32(at + 52);
                self.source = source_name(flags).to_string();
                self.times(at);
                // turned into an image only now, after the check (View::rows)
                let mut im = raw.map(|raw| bgrx_rgb(&raw, w, got));
                if (x, y, w, got) != (0, 0, cw, rows) {
                    let mut full = Image::new(Mode::Rgb, cw, rows, &[0, 0, 0]);
                    if let Some(i) = &im {
                        full.paste(i, x as i64, y as i64);
                    }
                    im = Some(full);
                }
                return im;
            }
            if start.elapsed() >= until {
                return self.none("the picture was rewritten while it was read");
            }
        }
    }

    /// `rects` joined down to `most` by merging the pair whose joined box adds the least area.
    pub fn merge_rects(rects: &[Box4], most: usize) -> Vec<Box4> {
        let mut rs: Vec<Box4> = rects.to_vec();
        let area = |r: Box4| i64::from(r.2 - r.0) * i64::from(r.3 - r.1);
        let join = |a: Box4, b: Box4| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3));
        while rs.len() > most {
            let mut best: Option<(i64, usize, usize)> = None;
            for i in 0..rs.len() {
                for j in i + 1..rs.len() {
                    let k = area(join(rs[i], rs[j])) - area(rs[i]) - area(rs[j]);
                    if best.is_none_or(|b| k < b.0) {
                        best = Some((k, i, j));
                    }
                }
            }
            let (_, i, j) = best.unwrap();
            rs[i] = join(rs[i], rs[j]);
            rs.remove(j);
        }
        rs
    }

    /// Only the parts `rects` (shares 0-1) of the newest picture, pasted in place on a black frame, or None.
    pub fn read_rects(&mut self, hwnd: usize, rects: &[(f64, f64, f64, f64)]) -> Option<Image> {
        let now = now_s();
        self.pace(now, 25.0);
        if !self.open(now) || !self.request_slot((now * 1000.0) as u64) {
            return self.none("the app shares no picture (not running, or no free reader slot)");
        }
        let share = |v: f64, k: usize| (65535i64.min(0i64.max((v * 65535.0) as i64 + i64::from(k >= 2)))) as u16;
        let mut want: Vec<Box4> = Self::merge_rects(&rects.iter().map(|r| (share(r.0, 0), share(r.1, 1), share(r.2, 2), share(r.3, 3))).collect::<Vec<_>>(), RECTS);
        want.sort();
        let slot = self.slot.unwrap();
        let (at, ra) = (PICTURE_BASE + slot * PICTURE, RECT_BASE + slot * RECT_SIZE);
        {
            let v = self.view.as_ref().unwrap();
            if self.box_.as_ref() != Some(&want) || v.u32(at + 8) != self.request || v.u32(ra) as usize != want.len() {
                self.request = (self.request % 0xFFFF_FFFF) + 1;
                v.set_u8(at + 60, 0);
                for (k, r) in want.iter().enumerate() {
                    for (j, x) in [r.0, r.1, r.2, r.3].into_iter().enumerate() {
                        v.set_u16(ra + 8 + 16 * k + 2 * j, x);
                    }
                }
                v.set_u32(ra, want.len() as u32);
                let whole = (
                    want.iter().map(|r| r.0).min().unwrap(),
                    want.iter().map(|r| r.1).min().unwrap(),
                    want.iter().map(|r| r.2).max().unwrap(),
                    want.iter().map(|r| r.3).max().unwrap(),
                );
                for (k, x) in [whole.0, whole.1, whole.2, whole.3].into_iter().enumerate() {
                    v.set_u16(at + 2 * k, x);
                }
                v.set_u32(at + 8, self.request);
            }
        }
        if self.box_.as_ref() != Some(&want) {
            self.box_ = Some(want.clone());
            let r = self.request;
            self.remember(r, want.clone());
        }
        if self.view.as_ref().unwrap().u64(40) != hwnd as u64 {
            return self.none("the app is capturing another window");
        }
        let start = Instant::now();
        let until = Duration::from_secs_f64(WAIT);
        loop {
            let v = self.view.as_ref().unwrap();
            let seq = v.u64(at + 16);
            if seq & 1 == 1 || v.u32(at + 12) != self.request {
                if start.elapsed() >= until {
                    // two things: the app writing the picture, or not having written our parts yet (a copy off the
                    // graphics card not back). Both were "being written" before 17 Nov.
                    return self.none(if seq & 1 == 1 { "the picture was being written" } else { "our parts are not in the picture yet" });
                }
                std::thread::sleep(Duration::from_millis(if seq & 1 == 1 { 1 } else { 2 }));
                continue;
            }
            let flags = v.u32(at + 48);
            let fresh = if flags >> 4 & 15 == 1 { 600.0 } else { (2.0 * self.interval).max(150.0) };
            if flags & 1 == 0 || now_s() * 1000.0 - v.u64(at + 24) as f64 > fresh {
                return self.none("no fresh picture from the app (no valid frame, or an old one)");
            }
            if v.u8(at + 61) > 1 {
                return self.none("the picture came shrunk");
            }
            let (mut offset, stride) = (v.u32(at + 32) as usize, v.u32(at + 36) as usize);
            let (cw, ch) = (v.u16(at + 56) as usize, v.u16(at + 58) as usize);
            if cw == 0 || ch == 0 {
                return self.none("the picture does not fit the window");
            }
            // the parts' pixels copied out as they are (x, y, w, h, BGRX), turned into the image after the check
            let mut got: Vec<(usize, usize, usize, usize, Vec<u8>)> = Vec::new();
            if stride != 0 {
                let (x, y, w, h) = (v.u16(at + 40) as usize, v.u16(at + 42) as usize, v.u16(at + 44) as usize, v.u16(at + 46) as usize);
                if w > 0 && h > 0 && x + w <= cw && y + h <= ch {
                    got.push((x, y, w, h, v.rows(HEADER + offset, w, h, stride)));
                }
            } else {
                for k in 0..want.len() {
                    let g = |j: usize| v.u16(ra + 8 + 16 * k + 8 + 2 * j) as usize;
                    let (x, y, w, h) = (g(0), g(1), g(2), g(3));
                    if w == 0 || h == 0 {
                        continue;
                    }
                    if x + w > cw || y + h > ch {
                        return self.none("the picture does not fit the window");
                    }
                    got.push((x, y, w, h, v.rows(HEADER + offset, w, h, w * 4)));
                    offset += w * 4 * h;
                }
            }
            if v.u64(at + 16) == seq {
                self.screen_copy = flags & 2 != 0;
                self.copy_us = v.u32(at + 52);
                self.source = source_name(flags).to_string();
                self.times(at);
                let mut full = Image::new(Mode::Rgb, cw, ch, &[0, 0, 0]);
                for (x, y, w, h, raw) in got {
                    full.paste(&bgrx_rgb(&raw, w, h), x as i64, y as i64);
                }
                return Some(full);
            }
            if start.elapsed() >= until {
                return self.none("the picture was rewritten while it was read");
            }
        }
    }

    /// The part `area` (shares 0-1) of the newest picture; `shrink` 2 or 4 asks the card to average it down first.
    pub fn read_area(&mut self, hwnd: usize, area: (f64, f64, f64, f64), shrink: u8) -> Option<Image> {
        let now = now_s();
        self.pace(now, 16.0);
        if !self.open(now) || !self.request_slot((now * 1000.0) as u64) {
            return self.none("the app shares no picture (not running, or no free reader slot)");
        }
        let s = |v: f64| (65535i64.min(0i64.max((v * 65535.0) as i64))) as u16;
        let want = (s(area.0), s(area.1), s(area.2), s(area.3));
        let slot = self.slot.unwrap();
        let at = PICTURE_BASE + slot * PICTURE;
        {
            let v = self.view.as_ref().unwrap();
            if v.u8(at + 60) != shrink {
                v.set_u8(at + 60, shrink);
                self.box_ = None;
            }
        }
        self.ask_area(now, want);
        let v = self.view.as_ref().unwrap();
        if v.u64(40) != hwnd as u64 {
            return None;
        }
        for _ in 0..4 {
            let seq = v.u64(at + 16);
            if seq & 1 == 1 {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            if v.u32(at + 12) != self.request {
                return None;
            }
            let flags = v.u32(at + 48);
            if flags & 1 == 0 || now * 1000.0 - v.u64(at + 24) as f64 > (2.0 * self.interval).max(100.0) {
                return None;
            }
            let (offset, stride) = (v.u32(at + 32) as usize, v.u32(at + 36) as usize);
            let used = v.u8(at + 61).max(1) as usize;
            let (w, h) = (v.u16(at + 44) as usize / used, v.u16(at + 46) as usize / used);
            let raw = v.rows(HEADER + offset, w, h, stride);
            if v.u64(at + 16) == seq {
                self.picture_time = v.u64(at + 24) as f64 / 1000.0;
                self.frame_time = self.picture_time - f64::from(v.u16(at + 62)) / 1000.0;
                return Some(bgrx_rgb(&raw, w, h)); // (after the check, as in read)
            }
        }
        None
    }

    /// Our overlay windows' spots (screen px) for the other readers to cover.
    pub fn publish_overlays(&mut self, owner: usize, rects: &[(i32, i32, i32, i32)]) {
        if !self.open(now_s()) {
            return;
        }
        let v = self.view.as_ref().unwrap();
        let at = OVERLAY_BASE + owner * OVERLAY_BLOCK;
        let rects = &rects[..rects.len().min(OVERLAY_MAX)];
        for (k, r) in rects.iter().enumerate() {
            for (j, x) in [r.0, r.1, r.2, r.3].into_iter().enumerate() {
                v.set_i32(at + 16 + 16 * k + 4 * j, x);
            }
        }
        v.set_u32(at + 8, std::process::id());
        v.set_u32(at + 12, rects.len() as u32);
        v.set_u64(at, (now_s() * 1000.0) as u64);
    }

    /// Every other aLogs program's overlay spots now (screen px).
    pub fn overlays(&mut self, skip_owner: Option<usize>) -> Vec<(i32, i32, i32, i32)> {
        if !self.open(now_s()) {
            return vec![];
        }
        let v = self.view.as_ref().unwrap();
        let now = (now_s() * 1000.0) as u64;
        let mut out = Vec::new();
        for owner in 0..3 {
            if Some(owner) == skip_owner {
                continue;
            }
            let at = OVERLAY_BASE + owner * OVERLAY_BLOCK;
            if now.saturating_sub(v.u64(at)) > OVERLAY_STALE {
                continue;
            }
            for k in 0..(v.u32(at + 12) as usize).min(OVERLAY_MAX) {
                let g = |j: usize| v.i32(at + 16 + 16 * k + 4 * j);
                out.push((g(0), g(1), g(2), g(3)));
            }
        }
        out
    }

    pub fn set_game_only_until(&mut self, ms: u64) {
        if self.open(now_s()) {
            self.view.as_ref().unwrap().set_u64(GAME_ONLY_UNTIL, ms);
        }
    }
    /// A script has the screen until then (unix ms): its pictures of the game window alone come the quick way
    /// (on Windows 10 window capture, with its border), not by the slower window copy.
    pub fn set_game_fast_until(&mut self, ms: u64) {
        if self.open(now_s()) {
            self.view.as_ref().unwrap().set_u64(GAME_FAST_UNTIL, ms);
        }
    }
    pub fn set_clean_until(&mut self, ms: u64) {
        if self.open(now_s()) {
            self.view.as_ref().unwrap().set_u64(CLEAN_UNTIL, ms);
        }
    }
    pub fn clean_now(&mut self) -> bool {
        self.open(now_s()) && now_s() * 1000.0 < self.view.as_ref().unwrap().u64(CLEAN_UNTIL) as f64
    }
    /// The player turned screen capture off.
    pub fn switched_off(&mut self) -> bool {
        self.open(now_s()) && self.view.as_ref().unwrap().u32(60) == 1
    }
    /// The aLogs app is running and sharing its capture (a stopped heartbeat lets the mapping go).
    pub fn app_present(&mut self) -> bool {
        let now = now_s();
        if self.open(now) {
            let beat = self.view.as_ref().unwrap().u64(HEARTBEAT);
            if beat != 0 && now * 1000.0 - beat as f64 > HEARTBEAT_STALE as f64 {
                self.close();
                return false;
            }
            return true;
        }
        false
    }
}

/// Reader for the app's audio cues: `want` keeps it listening; `cue` is the newest boss kill sound.
pub struct SharedAudio {
    view: Option<View>,
    next_open: f64,
}

impl Default for SharedAudio {
    fn default() -> Self {
        SharedAudio { view: None, next_open: 0.0 }
    }
}

impl SharedAudio {
    /// The app's name for it (alogs-app audio.rs NAME; a test below holds the two together). It read
    /// "LocalLogs.Audio.v1" until 17 Nov 2026: the Python had a BEL byte where the name's backslash and "a" were
    /// meant, the port dropped the byte, and under neither name was there a mapping to open. So no boss kill sound
    /// was ever heard, in any build.
    pub const NAME: &'static str = "Local\\aLogs.Audio.v1";
    pub const MAGIC: u32 = 0x55414C41;

    fn open(&mut self, now: f64) -> bool {
        if self.view.is_some() {
            return true;
        }
        if now < self.next_open {
            return false;
        }
        self.next_open = now + 2.0;
        match View::open(Self::NAME, 4096) {
            Some(v) if v.u32(0) == Self::MAGIC => {
                self.view = Some(v);
                true
            }
            _ => false,
        }
    }

    pub fn want(&mut self, now: f64, seconds: f64) -> bool {
        if self.open(now) {
            self.view.as_ref().unwrap().set_u64(8, ((now + seconds) * 1000.0) as u64);
            true
        } else {
            false
        }
    }
    /// 0 not listening, 1 listening, 2 can't listen; None without the app.
    pub fn state(&self) -> Option<u32> {
        self.view.as_ref().map(|v| v.u32(32))
    }
    /// (count, unix time the sound started, match score) of the newest boss kill sound.
    pub fn cue(&self) -> Option<(u32, f64, f32)> {
        let v = self.view.as_ref()?;
        let n = v.u32(28);
        if n == 0 {
            return None;
        }
        Some((n, v.u64(16) as f64 / 1000.0, v.f32(24)))
    }
    /// (state, last heard, best recent match, when it started) for the Dev switch kill-sound-log.
    pub fn listening(&self) -> Option<(u32, f64, f32, f64)> {
        let v = self.view.as_ref()?;
        Some((v.u32(32), v.u64(40) as f64 / 1000.0, v.f32(56), v.u64(48) as f64 / 1000.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app creates the mappings these readers open, so the names must be the same on both sides (alogs-app
    /// doesn't use this crate: its names are read from its source).
    #[test]
    fn the_readers_open_the_names_the_app_creates() {
        let app = |file: &str| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../alogs-app/src").join(file);
            let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let line = src.lines().find(|l| l.starts_with("pub const NAME: &str = ")).unwrap_or_else(|| panic!("no NAME in {file}"));
            line.split('"').nth(1).unwrap().replace("\\\\", "\\")
        };
        assert_eq!(SharedAudio::NAME, app("audio.rs"));
        assert_eq!(NAME, app("capture.rs"));
        assert_eq!(SharedAudio::NAME, r"Local\aLogs.Audio.v1");
    }

    /// Pixels copied out as they are and turned into an image after the seq check come out as the image made straight
    /// from the mapping did: BGRX to RGB, the stride's padding left out.
    #[test]
    fn rows_then_rgb_is_the_picture() {
        let (w, h, stride) = (5usize, 3usize, 24usize); // 20 bytes of pixels a row, 4 of padding
        let mut mapped = vec![0xEEu8; stride * h];
        for y in 0..h {
            for x in 0..w {
                mapped[y * stride + x * 4..][..4].copy_from_slice(&[x as u8, y as u8, 100 + (x + y) as u8, 255]);
            }
        }
        let view = std::mem::ManuallyDrop::new(View {
            base: mapped.as_mut_ptr(),
            #[cfg(windows)]
            handle: std::ptr::null_mut(),
        }); // (not a real mapping: nothing to let go)
        let raw = view.rows(0, w, h, stride);
        assert_eq!(raw.len(), w * h * 4);
        let want: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| [100 + (x + y) as u8, y as u8, x as u8])).collect();
        assert!(bgrx_rgb(&raw, w, h) == Image::from_raw(Mode::Rgb, w, h, want), "the same pixels");
    }
}
