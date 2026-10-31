//! Shared game capture: one picture of the Aniimo window for every aLogs module.
//!
//! The app takes the game's picture on the graphics card (gpu_capture.rs), the first way that works here:
//! 1. Window capture, only where Windows lets the yellow border be turned off (Windows 11). Only the game
//!    window: our overlays are never in it.
//! 2. Desktop Duplication: the screen the game is on, only while the game is in front. Our overlays can be in
//!    the picture (flag bit 1), so readers hide their overlays from capture then.
//! Window capture is tried again once a minute while on Desktop Duplication.
//!
//! Tabbed out on Windows 10 (Desktop Duplication sees the other window then), so the meter can still warn that
//! you're being attacked: the game window is copied with PrintWindow, at most 4 times a second (~50 ms each at 4K,
//! which can make the game stutter, but only while you're not looking at it); where that gives black pictures,
//! window capture with Windows 10's yellow border, closed as soon as the game is back in front.
//!
//! Each reader (the meter, Sanctum Helper, ...) says which area of the game it needs and how often, and gets
//! that area in **its own picture buffer**, written at its own pace: only that area leaves the graphics card,
//! and one reader's frequent small updates never interrupt another reader copying a big picture.
//!
//! Layout of `Local\aLogs.Capture.v2` (little-endian), see `docs/shared-capture.md`:
//!   0  u32 magic "ALCP"   4 u32 version (2)   8 u64 header seq (odd while the header is written)
//!   16 u32 client width   20 u32 client height
//!   28 u32 flags: bit 0 a valid image of the game; bit 1 our overlays may be in it;
//!      bits 4-7 the source (1 PrintWindow, 3 Desktop Duplication, 4 window capture)
//!   32 u64 time of the newest picture (unix ms)   40 u64 game window handle
//!   48 i32 client x   52 i32 client y (screen position of the client area)
//!   56 u32 pictures taken (wraps)   60 u32 1 while the player has switched screen capture off (overlay_button.rs):
//!      then only the app's own icon-row check (slot 15) is served, nothing else
//!   64 request slots: SLOTS x 16 { u64 wanted until (unix ms), u32 interval ms, u32 process id }
//!   320 picture slots: SLOTS x 64, the reader's own picture:
//!      0 u16 x0, y0, x1, y1: the area wanted, as a share of the client size (0-65535; x1 = 0: the whole frame)
//!      8 u32 request id (the reader changes it with the area)   12 u32 served id (the request now in the picture)
//!      16 u64 seq (odd while the picture is written)   24 u64 picture time (unix ms)
//!      32 u32 offset of the pixels from byte 4096   36 u32 stride (bytes per row)
//!      40 u16 x, y, w, h: where the picture is, in client pixels   48 u32 flags (as above)
//!      52 u32 how long reading it off the graphics card took (microseconds)
//!      56 u16 client width, height when it was taken
//!      60 u8 shrink wanted (reader: 1, 2 or 4; the card averages the area down by that)   61 u8 shrink used (app):
//!         the picture is then w / shrink by h / shrink pixels (rounded down), x, y, w, h still in client pixels
//!   2048 u64 heartbeat (unix ms): the capture thread is alive; readers let go of the mapping once it's a few
//!      seconds old (the app was closed)
//!   4096 pixels: the readers' pictures, BGRA, top row first, in a pool of two full frames.
//!
//! A reader claims a request slot (its own pid, or an expired one), keeps "wanted until" a couple of seconds
//! ahead while it needs frames, writes its area and request id, and once the served id matches, copies its
//! picture between two equal even reads of that slot's seq. Nothing is captured while no slot is active.

use std::ptr::{null, null_mut};
use std::sync::atomic::{fence, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetDC, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::System::Memory::{CreateFileMappingW, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetClientRect, GetForegroundWindow, GetWindowTextW, IsIconic, IsWindow, IsWindowVisible,
};

use crate::gpu_capture::{window_frame_origin, Duplication, Gpu, WindowCapture};

pub const NAME: &str = "Local\\aLogs.Capture.v2";
const MAGIC: u32 = u32::from_le_bytes(*b"ALCP");
const VERSION: u32 = 2;
const HEADER: usize = 4096;
const SLOTS: usize = 16;
const SLOT_BASE: usize = 64;
const PICTURE_BASE: usize = 320;
const PICTURE_SIZE: usize = 64;
const HEARTBEAT: usize = 2048;
const MAX_W: usize = 3840;
const MAX_H: usize = 2160;
/// Room for the readers' pictures: two whole frames (say a full frame for the meter plus everyone's small areas).
const POOL: usize = 2 * MAX_W * MAX_H * 4;
const SIZE: usize = HEADER + POOL;
/// Only black pictures for this long before trying the next way (fades and loading screens are shorter; it was
/// 8 pictures, which with a 60-a-second reader was ~130 ms and dropped window capture over a fade).
const BLACK_MS: u64 = 5_000;
/// A way that keeps failing to start or breaking is given up after this many tries.
const GPU_TRIES: u32 = 3;
/// While on Desktop Duplication, how often window capture is tried again.
const RETRY_BETTER_MS: u64 = 60_000;
/// Tabbed out on Windows 10: at most one copy of the game window this often.
const AWAY_MS: u64 = 250;
const PW_CLIENTONLY: u32 = 1;
const PW_RENDERFULLCONTENT: u32 = 2;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

struct Shared {
    base: *mut u8,
}

impl Shared {
    fn create() -> Option<Shared> {
        let name: Vec<u16> = NAME.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let handle = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                null(),
                PAGE_READWRITE,
                (SIZE >> 32) as u32,
                SIZE as u32,
                name.as_ptr(),
            );
            if handle.is_null() {
                return None;
            }
            let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, SIZE);
            if view.Value.is_null() {
                CloseHandle(handle);
                return None;
            }
            // The mapping lives as long as the app: the handle is never closed on purpose.
            let shared = Shared { base: view.Value as *mut u8 };
            shared.put_u32(0, MAGIC);
            shared.put_u32(4, VERSION);
            Some(shared)
        }
    }

    fn atomic(&self, at: usize) -> &AtomicU64 {
        unsafe { &*(self.base.add(at) as *const AtomicU64) }
    }
    fn put_u16(&self, at: usize, value: u16) {
        unsafe { (self.base.add(at) as *mut u16).write_unaligned(value) }
    }
    fn put_u32(&self, at: usize, value: u32) {
        unsafe { (self.base.add(at) as *mut u32).write_unaligned(value) }
    }
    fn put_u64(&self, at: usize, value: u64) {
        unsafe { (self.base.add(at) as *mut u64).write_unaligned(value) }
    }
    fn get_u16(&self, at: usize) -> u16 {
        unsafe { (self.base.add(at) as *const u16).read_volatile() }
    }
    fn get_u32(&self, at: usize) -> u32 {
        unsafe { (self.base.add(at) as *const u32).read_volatile() }
    }
    fn get_u64(&self, at: usize) -> u64 {
        unsafe { (self.base.add(at) as *const u64).read_volatile() }
    }

    fn active(&self, slot: usize, now: u64) -> bool {
        self.get_u64(SLOT_BASE + slot * 16) > now
    }
    fn interval(&self, slot: usize) -> u64 {
        self.get_u32(SLOT_BASE + slot * 16 + 8).clamp(16, 2000) as u64
    }

    /// The area slot `slot` wants, in client pixels, and its request id.
    fn wanted(&self, slot: usize, width: u32, height: u32) -> (Area, u32) {
        let at = PICTURE_BASE + slot * PICTURE_SIZE;
        // the id first: readers write the area, then the id, so an area read after the id is at least that new
        let request = self.get_u32(at + 8);
        fence(Ordering::Acquire);
        let share = [0, 2, 4, 6].map(|k| self.get_u16(at + k) as u64);
        let area = if share[2] == 0 || share[3] == 0 {
            Area { x: 0, y: 0, w: width, h: height } // nothing said: the whole frame
        } else {
            let x0 = (share[0] * width as u64 / 65535).min(width as u64 - 1) as u32;
            let y0 = (share[1] * height as u64 / 65535).min(height as u64 - 1) as u32;
            let x1 = ((share[2] * width as u64 + 65534) / 65535).clamp(x0 as u64 + 1, width as u64) as u32;
            let y1 = ((share[3] * height as u64 + 65534) / 65535).clamp(y0 as u64 + 1, height as u64) as u32;
            Area { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
        };
        (area, request)
    }

    fn pool(&self) -> *mut u8 {
        unsafe { self.base.add(HEADER) }
    }

    /// Writes between an odd and an even value of the seq at `at`, so readers retry instead of taking a torn copy.
    fn write<T>(&self, at: usize, body: impl FnOnce() -> T) -> T {
        let seq = self.atomic(at);
        let start = seq.load(Ordering::Relaxed) | 1;
        seq.store(start, Ordering::Relaxed);
        fence(Ordering::Release);
        let result = body();
        fence(Ordering::Release);
        seq.store(start + 1, Ordering::Release);
        result
    }

    /// The general header: which window, its size and where, the source and the newest picture's time.
    fn header(&self, size: (u32, u32), flags: u32, hwnd: HWND, origin: POINT) {
        self.write(8, || {
            self.put_u32(16, size.0);
            self.put_u32(20, size.1);
            self.put_u32(24, size.0 * 4);
            self.put_u32(28, flags);
            self.put_u64(32, now_ms());
            self.put_u64(40, hwnd as u64);
            self.put_u32(48, origin.x as u32);
            self.put_u32(52, origin.y as u32);
        });
    }

    fn mark_invalid(&self) {
        self.write(8, || {
            self.put_u32(28, 0);
            self.put_u64(32, now_ms());
        });
    }
}

// The view is only touched from the capture thread.
unsafe impl Send for Shared {}

#[derive(Clone, Copy, PartialEq, Default)]
struct Area {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

fn wide(buf: &[u16]) -> String {
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())])
}

struct Search {
    title: String,
    found: HWND,
    area: i64,
}

unsafe extern "system" fn each_window(hwnd: HWND, lparam: LPARAM) -> i32 {
    let search = &mut *(lparam as *mut Search);
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let mut buf = [0u16; 256];
    GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if wide(&buf).to_lowercase() != search.title {
        return 1;
    }
    // The game is a Unity player; this keeps browser tabs and folders named "Aniimo" out.
    GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if wide(&buf) != "UnityWndClass" {
        return 1;
    }
    // the largest of them, like the meter's find_window (bossbar/wincapture.py), so both take the same window
    let mut rect: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    let area = (rect.right as i64) * (rect.bottom as i64);
    if search.found.is_null() || area > search.area {
        (search.found, search.area) = (hwnd, area);
    }
    1
}

/// The game's window: visible, a Unity player, titled as General Settings › Game window title says (read each time, so
/// a change applies at once; trimmed, any case, "Aniimo" when empty, like the meter's find_window), the largest.
pub(crate) fn find_game() -> HWND {
    find_game_titled()
}

/// Whether this PC's Windows can capture the game window without the yellow border (Windows 11 and later). Without
/// it (Windows 10) aLogs uses Desktop Duplication and keeps its overlays out of screen captures: About says so.
pub(crate) fn borderless_capture() -> bool {
    static KNOWN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *KNOWN.get_or_init(WindowCapture::borderless_supported)
}

fn find_game_titled() -> HWND {
    let mut search = Search { title: crate::game_title().to_lowercase(), found: null_mut(), area: 0 };
    unsafe { EnumWindows(Some(each_window), &mut search as *mut Search as LPARAM) };
    search.found
}

/// Where pictures come from, best first (see the top).
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
enum Kind {
    Window,
    Duplication,
}

impl Kind {
    fn next(self) -> Option<Kind> {
        match self {
            Kind::Window => Some(Kind::Duplication),
            Kind::Duplication => None,
        }
    }
    /// Flag bits 1 and 4-7 for pictures from this source.
    fn flags(self) -> u32 {
        match self {
            Kind::Duplication => 2 | 3 << 4,
            Kind::Window => 4 << 4,
        }
    }
    /// The screen shows whatever is in front: only counts while the game is in front.
    fn needs_front(self) -> bool {
        self == Kind::Duplication
    }
    /// Flags for the tabbed-out window copy: only the game window, no overlays in it.
    const PRINT_FLAGS: u32 = 1 << 4;

    fn name(self) -> &'static str {
        match self {
            Kind::Window => "window capture (GPU)",
            Kind::Duplication => "Desktop Duplication (GPU)",
        }
    }
}

enum Source {
    Window(WindowCapture),
    Duplication(Duplication),
}

impl Source {
    fn open(kind: Kind, hwnd: HWND) -> windows::core::Result<Source> {
        let window = windows::Win32::Foundation::HWND(hwnd as _);
        Ok(match kind {
            Kind::Window => Source::Window(WindowCapture::new(window, false)?),
            Kind::Duplication => Source::Duplication(Duplication::new(window)?),
        })
    }
    /// Takes the newest image on the card. Err: broken, start it again.
    fn update(&mut self) -> windows::core::Result<bool> {
        match self {
            Source::Window(w) => w.update(),
            Source::Duplication(d) => d.update(),
        }
    }
    fn gpu(&mut self) -> &mut dyn Picture {
        match self {
            Source::Window(w) => &mut w.gpu,
            Source::Duplication(d) => &mut d.gpu,
        }
    }
    /// The game window is now on a different screen than the one being duplicated.
    fn moved(&self, hwnd: HWND) -> bool {
        match self {
            Source::Duplication(d) => unsafe {
                windows::Win32::Graphics::Gdi::MonitorFromWindow(
                    windows::Win32::Foundation::HWND(hwnd as _),
                    windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST,
                ) != d.monitor
            },
            Source::Window(_) => false,
        }
    }
    /// Where the image starts on screen (the client area's offset in it follows from that).
    fn origin(&self, hwnd: HWND) -> (i32, i32) {
        match self {
            Source::Window(_) => window_frame_origin(windows::Win32::Foundation::HWND(hwnd as _)).unwrap_or((0, 0)),
            Source::Duplication(d) => (d.desktop.left, d.desktop.top),
        }
    }
}

/// A picture the readers' areas are copied out of: on the graphics card, or a window copy in memory.
trait Picture {
    fn has_image(&self) -> bool;
    fn size(&self) -> (u32, u32);
    fn generation(&self) -> u64;
    fn read(&mut self, x: u32, y: u32, w: u32, h: u32, out: *mut u8, stride: usize, shrink: u32) -> bool;
}

impl Picture for Gpu {
    fn has_image(&self) -> bool {
        Gpu::has_image(self)
    }
    fn size(&self) -> (u32, u32) {
        Gpu::size(self)
    }
    fn generation(&self) -> u64 {
        self.generation
    }
    fn read(&mut self, x: u32, y: u32, w: u32, h: u32, out: *mut u8, stride: usize, shrink: u32) -> bool {
        Gpu::read(self, x, y, w, h, out, stride, shrink).is_ok()
    }
}

/// The game window drawn by PrintWindow into memory (tabbed out on Windows 10 only).
struct WindowCopy {
    dc: HDC,
    bitmap: HBITMAP,
    old: *mut std::ffi::c_void,
    bits: *mut u8,
    width: u32,
    height: u32,
    generation: u64,
}

impl WindowCopy {
    fn new(width: u32, height: u32) -> Option<WindowCopy> {
        unsafe {
            let screen = GetDC(null_mut());
            let dc = CreateCompatibleDC(screen);
            ReleaseDC(null_mut(), screen);
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // top row first
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if dc.is_null() || bitmap.is_null() || bits.is_null() {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                if !dc.is_null() {
                    DeleteDC(dc);
                }
                return None;
            }
            let old = SelectObject(dc, bitmap);
            Some(WindowCopy { dc, bitmap, old, bits: bits as *mut u8, width, height, generation: 0 })
        }
    }

    /// A new copy of the window's client area; false when PrintWindow failed.
    fn take(&mut self, hwnd: HWND) -> bool {
        let ok = unsafe { PrintWindow(hwnd, self.dc, PW_CLIENTONLY | PW_RENDERFULLCONTENT) != 0 };
        unsafe { GdiFlush() };
        self.generation += 1;
        ok
    }
}

impl Drop for WindowCopy {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

impl Picture for WindowCopy {
    fn has_image(&self) -> bool {
        self.generation > 0
    }
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    fn generation(&self) -> u64 {
        self.generation
    }
    fn read(&mut self, x: u32, y: u32, w: u32, h: u32, out: *mut u8, stride: usize, shrink: u32) -> bool {
        let at = |xx: u32, yy: u32| unsafe { self.bits.add(((yy as usize) * self.width as usize + xx as usize) * 4) };
        if shrink <= 1 {
            for row in 0..h {
                unsafe { std::ptr::copy_nonoverlapping(at(x, y + row), out.add(row as usize * stride), w as usize * 4) };
            }
            return true;
        }
        // tabbed out only, 4 a second: averaged here like the card's smaller pictures
        let n = shrink * shrink;
        for row in 0..h / shrink {
            for col in 0..w / shrink {
                let mut sum = [0u32; 4];
                for dy in 0..shrink {
                    for dx in 0..shrink {
                        let p = at(x + col * shrink + dx, y + row * shrink + dy);
                        for c in 0..4 {
                            sum[c] += unsafe { *p.add(c) } as u32;
                        }
                    }
                }
                for c in 0..4 {
                    unsafe { *out.add(row as usize * stride + col as usize * 4 + c) = (sum[c] / n) as u8 };
                }
            }
        }
        true
    }
}

fn client(hwnd: HWND) -> Option<(u32, u32, POINT)> {
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        if width < 100 || height < 100 || width as usize > MAX_W || height as usize > MAX_H {
            return None;
        }
        let mut origin = POINT { x: 0, y: 0 };
        ClientToScreen(hwnd, &mut origin);
        Some((width as u32, height as u32, origin))
    }
}

/// True if a spread of the pixels at `pixels` (w x h, `stride` bytes per row) has anything not black.
fn painted(pixels: *const u8, w: u32, h: u32, stride: usize) -> bool {
    let count = (w * h) as usize;
    (0..count).step_by(97.max(count / 4000)).any(|i| {
        let (x, y) = (i % w as usize, i / w as usize);
        unsafe { (pixels.add(y * stride + x * 4) as *const u32).read_unaligned() & 0x00ff_ffff != 0 }
    })
}

/// What the capture thread remembers per reader.
#[derive(Clone, Copy, Default)]
struct Reader {
    /// When its picture was last written or confirmed current.
    last: u64,
    /// The request, image generation and shrink in its picture now.
    request: u32,
    generation: u64,
    shrink: u32,
    /// Its part of the pool (bytes from the pool's start), kept until its area grows.
    offset: usize,
    room: usize,
}

/// Starts every reader's picture over (a new source, tabbing in or out): their pictures must be written again
/// before readers take them, so the served ids go to 0; their parts of the pool are kept, so a picture is never
/// written over another reader's part while that reader may still be copying it.
fn forget(shared: &Shared, readers: &mut [Reader; SLOTS]) {
    for (i, r) in readers.iter_mut().enumerate() {
        (r.request, r.generation, r.last) = (0, 0, 0);
        shared.write(PICTURE_BASE + i * PICTURE_SIZE + 16, || shared.put_u32(PICTURE_BASE + i * PICTURE_SIZE + 12, 0));
    }
}

/// Finds room for `bytes` in the pool next to the other active readers' pictures (first fit).
fn place(readers: &[Reader; SLOTS], active: &[bool; SLOTS], slot: usize, bytes: usize) -> Option<usize> {
    let mut taken: Vec<(usize, usize)> =
        (0..SLOTS).filter(|&i| i != slot && active[i] && readers[i].room > 0).map(|i| (readers[i].offset, readers[i].room)).collect();
    taken.sort();
    let mut at = 0;
    for (offset, room) in taken {
        if offset >= at + bytes {
            break;
        }
        at = at.max(offset + room);
    }
    (at + bytes <= POOL).then_some(at)
}

/// Writes the readers that are due: each gets the area it asked for, in its own picture. A reader whose area
/// and image are unchanged only gets its picture time moved on (nothing leaves the graphics card).
/// Returns whether any picture written had something on it (None: nothing was written).
///
/// `trust_black`: the source shows the game as it is (the GPU ways), so a black picture is a real one (a fade, a
/// loading screen) and is handed to readers as valid; the window copy's black pictures mean it failed.
fn serve(
    shared: &Shared, gpu: &mut dyn Picture, flags: u32, trust_black: bool, hwnd: HWND, image_origin: (i32, i32),
    readers: &mut [Reader; SLOTS], only: Option<usize>,
) -> Option<bool> {
    let (width, height, origin) = client(hwnd)?;
    if !gpu.has_image() {
        return None;
    }
    let now = now_ms();
    let (iw, ih) = gpu.size();
    let (cx, cy) = (origin.x - image_origin.0, origin.y - image_origin.1); // the client area inside the image
    let generation = gpu.generation();
    let active: [bool; SLOTS] = std::array::from_fn(|i| shared.active(i, now));
    // a reader that stopped asking gives up its part of the pool, and its picture is no longer current: when it
    // comes back, its picture is written again first (its old part may hold someone else's by then)
    for i in 0..SLOTS {
        if active[i] || (readers[i].room == 0 && readers[i].request == 0) {
            continue;
        }
        readers[i] = Reader::default();
        shared.write(PICTURE_BASE + i * PICTURE_SIZE + 16, || shared.put_u32(PICTURE_BASE + i * PICTURE_SIZE + 12, 0));
    }
    let mut result = None;
    for slot in (0..SLOTS).filter(|&i| active[i] && only.map_or(true, |o| o == i)) {
        let r = readers[slot];
        if now < r.last + shared.interval(slot) {
            continue; // not due yet: it asks at its own pace
        }
        let (area, request) = shared.wanted(slot, width, height);
        let at = PICTURE_BASE + slot * PICTURE_SIZE;
        let shrink = match unsafe { *shared.base.add(at + 60) } {
            2 => 2u32,
            4 => 4,
            _ => 1,
        };
        if r.request == request && r.generation == generation && r.shrink == shrink && shared.get_u32(at + 12) == request {
            shared.put_u64(at + 24, now); // same image, same area: still current
            readers[slot].last = now;
            continue;
        }
        // the part of the area that's on the image (a window partly off its screen is cut to what's on it)
        let (x0, y0) = ((cx + area.x as i32).max(0), (cy + area.y as i32).max(0));
        let (x1, y1) = ((cx + (area.x + area.w) as i32).min(iw as i32), (cy + (area.y + area.h) as i32).min(ih as i32));
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
        let shrink = if w / shrink == 0 || h / shrink == 0 { 1 } else { shrink };
        let (pw, ph) = (w / shrink, h / shrink); // the picture's own size
        let (stride, bytes) = (pw as usize * 4, pw as usize * 4 * ph as usize);
        if readers[slot].room < bytes || r.request != request {
            match place(readers, &active, slot, bytes) {
                Some(offset) => (readers[slot].offset, readers[slot].room) = (offset, bytes),
                None => continue, // no room this time
            }
        }
        let started = Instant::now();
        let out = unsafe { shared.pool().add(readers[slot].offset) };
        let ok = shared.write(at + 16, || {
            if !gpu.read(x0 as u32, y0 as u32, w, h, out, stride, shrink) {
                return false;
            }
            let shown = painted(out, pw, ph, stride);
            unsafe { *shared.base.add(at + 61) = shrink as u8 };
            shared.put_u64(at + 24, now);
            shared.put_u32(at + 32, readers[slot].offset as u32);
            shared.put_u32(at + 36, stride as u32);
            for (k, v) in [x0 - cx, y0 - cy, w as i32, h as i32].into_iter().enumerate() {
                shared.put_u16(at + 40 + 2 * k, v as u16);
            }
            shared.put_u32(at + 48, (shown || trust_black) as u32 | flags);
            shared.put_u32(at + 52, started.elapsed().as_micros().min(u32::MAX as u128) as u32);
            shared.put_u16(at + 56, width as u16);
            shared.put_u16(at + 58, height as u16);
            shared.put_u32(at + 12, request);
            result = Some(result.unwrap_or(false) || shown);
            true
        });
        if !ok {
            return None;
        }
        readers[slot] = Reader { last: now, request, generation, shrink, ..readers[slot] };
    }
    if let Some(shown) = result {
        shared.header((width, height), (shown || trust_black) as u32 | flags, hwnd, origin);
        shared.put_u32(56, shared.get_u32(56).wrapping_add(1));
    }
    Some(result.unwrap_or(true))
}

/// The app's own reader (the capture button over the game, overlay_button.rs): request slot SLOTS - 1, kept under
/// the app's process id, for one small area, read like any other reader's picture.
pub struct AppReader {
    shared: Shared,
    request: u32,
    area: [u16; 4],
}

impl AppReader {
    pub fn new() -> Option<AppReader> {
        Shared::create().map(|shared| AppReader { shared, request: 0, area: [0; 4] })
    }

    /// The area `area` (shares of the client size, 0-65535: x0, y0, x1, y1) of the newest picture of `hwnd`, as
    /// (width, height, BGRA rows), once a picture holds it; None until then, when capture is off or stale.
    pub fn read(&mut self, hwnd: isize, area: [u16; 4], interval_ms: u32) -> Option<(u32, u32, Vec<u8>)> {
        let (s, slot, now) = (&self.shared, SLOTS - 1, now_ms());
        let at = SLOT_BASE + slot * 16;
        s.put_u64(at, now + 2500);
        s.put_u32(at + 8, interval_ms);
        s.put_u32(at + 12, std::process::id());
        let pa = PICTURE_BASE + slot * PICTURE_SIZE;
        unsafe { *s.base.add(pa + 60) = 0 }; // whole size (a reader before us may have asked for less)
        if area != self.area || s.get_u32(pa + 8) != self.request {
            self.area = area;
            self.request = self.request.wrapping_add(1).max(1);
            for (k, v) in area.into_iter().enumerate() {
                s.put_u16(pa + 2 * k, v);
            }
            s.put_u32(pa + 8, self.request);
        }
        if s.get_u64(40) != hwnd as u64 {   // (served while switched off too: the one allowed area)
            return None;
        }
        for _ in 0..4 {
            let seq = s.atomic(pa + 16).load(Ordering::Acquire);
            if seq & 1 == 1 {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            if s.get_u32(pa + 12) != self.request || s.get_u32(pa + 48) & 1 == 0 || now.saturating_sub(s.get_u64(pa + 24)) > 1000
                || unsafe { *s.base.add(pa + 61) } > 1
            {
                return None;
            }
            let (offset, stride) = (s.get_u32(pa + 32) as usize, s.get_u32(pa + 36) as usize);
            let (w, h) = (s.get_u16(pa + 44) as u32, s.get_u16(pa + 46) as u32);
            let mut out = vec![0u8; (w * h * 4) as usize];
            for row in 0..h as usize {
                unsafe {
                    std::ptr::copy_nonoverlapping(s.pool().add(offset + row * stride), out.as_mut_ptr().add(row * w as usize * 4), w as usize * 4);
                }
            }
            fence(Ordering::Acquire);
            if s.atomic(pa + 16).load(Ordering::Acquire) == seq {
                return Some((w, h, out));
            }
        }
        None
    }
}

/// Starts the capture thread. `title` is the game window's title (matched case-insensitively).
pub fn start() {
    std::thread::Builder::new()
        .name("shared-capture".into())
        .spawn(move || {
            // Real pixels on scaled displays (150% etc.), the same frame size the game renders.
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
            let _ = unsafe { windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED) };
            let Some(shared) = Shared::create() else {
                eprintln!("shared capture: could not create {NAME}");
                return;
            };
            let best = if WindowCapture::borderless_supported() { Kind::Window } else { Kind::Duplication };
            let mut kind = best;
            let mut source: Option<Source> = None;
            let mut tries = 0u32; // failed starts or breaks of the current way
            let mut black_since = 0u64; // when the pictures went black (0: they aren't)
            let mut retry_better = 0u64;
            let mut readers = [Reader::default(); SLOTS];
            let mut hwnd: HWND = null_mut();
            let mut last_find = 0u64;
            let mut announced: Option<Kind> = None;
            // tabbed out on Windows 10: the window copy, or (where it's black) window capture with its border
            let mut away_copy: Option<WindowCopy> = None;
            let mut away_window: Option<WindowCapture> = None;
            let mut away_border = false; // the window copy gave only black pictures for this game window
            let mut away_black_since = 0u64;
            let mut away_last = 0u64;
            let mut away_said = false;
            loop {
                let now = now_ms();
                shared.put_u64(HEARTBEAT, now); // alive (readers notice when the app is gone)
                // switched off by the player (the overlay button or the app): nothing is captured at all
                let off = !crate::overlay_button::capture_on();
                if shared.get_u32(60) != off as u32 {
                    shared.write(8, || shared.put_u32(60, off as u32));
                }
                // Switched off is the DPS meter's switch: the meter sees the flag and stops asking (it rests), and
                // Sanctum Helper has its own switch (it isn't running when off), so the readers left (Sanctum Helper
                // when on, the button's look at its icon row) keep their pictures. Nobody asking: nothing is captured.
                let only: Option<usize> = None;
                // the next reader due (each at its own pace), or nobody: sleep
                let due = (0..SLOTS)
                    .filter(|&i| shared.active(i, now) && only.map_or(true, |o| o == i))
                    .map(|i| readers[i].last + shared.interval(i))
                    .min();
                let Some(due) = due else {
                    source = None; // free the GPU textures while idle
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                };
                if now < due {
                    std::thread::sleep(Duration::from_millis((due - now).min(10)));
                    continue;
                }
                let alive = unsafe { !hwnd.is_null() && IsWindow(hwnd) != 0 };
                if !alive && now > last_find + 1000 {
                    hwnd = find_game();
                    last_find = now;
                    (kind, source, tries, black_since) = (best, None, 0, 0); // a new game window: the best way first again
                    (away_copy, away_window, away_border, away_black_since) = (None, None, false, 0);
                }
                let usable = unsafe { !hwnd.is_null() && IsWindow(hwnd) != 0 && IsIconic(hwnd) == 0 };
                let wait = |shared: &Shared, readers: &mut [Reader; SLOTS]| {
                    shared.mark_invalid();
                    for r in readers.iter_mut() {
                        r.last = now; // try again at each reader's next turn, not in a busy loop
                    }
                };
                if !usable {
                    wait(&shared, &mut readers);
                    hwnd = null_mut();
                    continue;
                }
                // On Desktop Duplication where window capture exists: try it again now and then.
                if kind > best && now >= retry_better {
                    retry_better = now + RETRY_BETTER_MS;
                    (kind, source, tries, black_since) = (best, None, 0, 0);
                }
                // Desktop Duplication shows one screen: the game moved to another, so start it on that one
                if source.as_ref().is_some_and(|s| s.moved(hwnd)) {
                    source = None;
                }
                if kind.needs_front() && unsafe { GetForegroundWindow() } != hwnd {
                    // Tabbed out: Desktop Duplication sees the other window. Copy the game window itself, slowly.
                    if now < away_last + AWAY_MS {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    away_last = now;
                    let Some((width, height, _)) = client(hwnd) else {
                        wait(&shared, &mut readers);
                        continue;
                    };
                    let result = if !away_border {
                        if away_copy.as_ref().map_or(true, |c| (c.width, c.height) != (width, height)) {
                            away_copy = WindowCopy::new(width, height);
                        }
                        match away_copy.as_mut() {
                            Some(c) => {
                                if c.take(hwnd) {
                                    // the copy is the client area itself: it starts where the client area does
                                    let origin = client(hwnd).map_or((0, 0), |(_, _, o)| (o.x, o.y));
                                    serve(&shared, c, Kind::PRINT_FLAGS, false, hwnd, origin, &mut readers, only)
                                } else {
                                    Some(false)
                                }
                            }
                            None => Some(false),
                        }
                    } else {
                        if away_window.is_none() {
                            away_window = WindowCapture::new(windows::Win32::Foundation::HWND(hwnd as _), true).ok();
                            if away_window.is_some() {
                                forget(&shared, &mut readers);
                            }
                        }
                        match away_window.as_mut() {
                            Some(w) => match w.update() {
                                Ok(_) => {
                                    let origin = window_frame_origin(windows::Win32::Foundation::HWND(hwnd as _)).unwrap_or((0, 0));
                                    serve(&shared, &mut w.gpu, Kind::Window.flags(), true, hwnd, origin, &mut readers, only)
                                }
                                Err(_) => {
                                    away_window = None;
                                    None
                                }
                            },
                            None => None,
                        }
                    };
                    match result {
                        Some(true) => {
                            away_black_since = 0;
                            if !away_said {
                                away_said = true;
                                eprintln!("shared capture: tabbed out, using {}", if away_border { "window capture (with border)" } else { "PrintWindow" });
                            }
                        }
                        Some(false) if !away_border => {
                            away_black_since = if away_black_since == 0 { now } else { away_black_since };
                            if now - away_black_since >= BLACK_MS / 2 {
                                eprintln!("shared capture: PrintWindow gives no picture; window capture while tabbed out");
                                (away_border, away_copy, away_said) = (true, None, false);
                            }
                        }
                        _ => wait(&shared, &mut readers),
                    }
                    continue;
                }
                // back in front: the tabbed-out ways are closed (window capture's border goes with it)
                if away_copy.is_some() || away_window.is_some() {
                    (away_copy, away_window, away_said) = (None, None, false);
                    forget(&shared, &mut readers);
                }
                if source.is_none() {
                    match Source::open(kind, hwnd) {
                        Ok(s) => {
                            source = Some(s);
                            forget(&shared, &mut readers);
                        }
                        Err(e) => {
                            tries += 1;
                            if tries >= GPU_TRIES {
                                eprintln!("shared capture: {} can't start ({e}); trying the next way", kind.name());
                                (kind, tries, black_since) = (kind.next().unwrap_or(kind), 0, 0);
                                retry_better = now + RETRY_BETTER_MS;
                            }
                            wait(&shared, &mut readers);
                            continue;
                        }
                    }
                }
                let s = source.as_mut().unwrap();
                let result = match s.update() {
                    Err(_) => None,
                    Ok(_) => {
                        let image_origin = s.origin(hwnd);
                        serve(&shared, s.gpu(), kind.flags(), true, hwnd, image_origin, &mut readers, only).or(Some(false))
                    }
                };
                match result {
                    // broken (resolution change, fullscreen switch, lost device): start it again, a few times
                    None => {
                        source = None;
                        wait(&shared, &mut readers);
                        tries += 1;
                        if tries >= GPU_TRIES {
                            eprintln!("shared capture: {} keeps failing; trying the next way", kind.name());
                            (kind, tries, black_since) = (kind.next().unwrap_or(kind), 0, 0);
                            retry_better = now + RETRY_BETTER_MS;
                        }
                    }
                    Some(true) => {
                        (black_since, tries) = (0, 0);
                        if announced != Some(kind) {
                            announced = Some(kind);
                            eprintln!("shared capture: using {}", kind.name());
                        }
                    }
                    Some(false) => {
                        black_since = if black_since == 0 { now } else { black_since };
                        if now - black_since >= BLACK_MS {
                            black_since = 0;
                            if let Some(next) = kind.next() {
                                eprintln!("shared capture: {} gives no picture; trying {}", kind.name(), next.name());
                                (kind, source, tries) = (next, None, 0);
                                retry_better = now + RETRY_BETTER_MS;
                            }
                        }
                    }
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The meter's reader (bossbar/wincapture.py) against pictures served here from Desktop Duplication of the
    /// window in front, alongside a fast small-area reader like Sanctum Helper's:
    /// `cargo test --bin alogs-app capture::tests::meter_reads_asked_area -- --exact --ignored --nocapture`
    #[test]
    #[ignore]
    fn meter_reads_asked_area() {
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let hwnd = unsafe { GetForegroundWindow() };
        let mut source = Source::open(Kind::Duplication, hwnd).expect("Desktop Duplication");
        meter_reader_beside_corner(hwnd, |shared, readers| {
            if source.update().is_ok() {
                let origin = source.origin(hwnd);
                serve(shared, source.gpu(), Kind::Duplication.flags(), true, hwnd, origin, readers, None);
            }
        });
    }

    /// The damage-number recorder (bossbar/dmgnumbers.py) on Desktop Duplication of the window in front, for 4 s:
    /// `cargo test --bin alogs-app capture::tests::damage_number_recorder -- --exact --ignored --nocapture`
    #[test]
    #[ignore]
    fn damage_number_recorder() {
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let hwnd = unsafe { GetForegroundWindow() };
        let mut source = Source::open(Kind::Duplication, hwnd).expect("Desktop Duplication");
        let script = r#"
import sys, time, os, tempfile, logging, glob
sys.path.insert(0, sys.argv[2])   # bossbar, next to the app (given by the test)
logging.basicConfig(level=logging.INFO, format='%(message)s', stream=sys.stdout)
import dmgnumbers as N
here = tempfile.mkdtemp(); N.BURST_S = 4
r = N.Recorder(here, logging.getLogger('t'))
r.fight_started(int(sys.argv[1]), 'Test Boss')
r.thread.join(); t = time.time()
folder = glob.glob(os.path.join(here, 'logs', 'test_frames', '*', 'numbers', '*'))[0]
while glob.glob(os.path.join(folder, '*.rgb')) and time.time() - t < 120: time.sleep(0.5)
files = sorted(glob.glob(os.path.join(folder, '*.rgb.z')))
mb = sum(os.path.getsize(f) for f in files) / 1e6
print(len(files), 'files compressed in', round(time.time() - t, 1), 's,', round(mb), 'MB, size', N.load_frame(files[0]).size if files else None, flush=True)
"#;
        python_beside_corner(hwnd, script, |shared, readers| {
            if source.update().is_ok() {
                let origin = source.origin(hwnd);
                serve(shared, source.gpu(), Kind::Duplication.flags(), true, hwnd, origin, readers, None);
            }
        });
    }

    /// The same with the tabbed-out window copy (PrintWindow, 4 a second):
    /// `cargo test --bin alogs-app capture::tests::meter_reads_window_copy -- --exact --ignored --nocapture`
    #[test]
    #[ignore]
    fn meter_reads_window_copy() {
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let hwnd = unsafe { GetForegroundWindow() };
        let (w, h, _) = client(hwnd).expect("a window");
        let mut copy = WindowCopy::new(w, h).expect("window copy");
        let mut last = 0u64;
        meter_reader_beside_corner(hwnd, |shared, readers| {
            if now_ms() >= last + AWAY_MS {
                last = now_ms();
                let t = Instant::now();
                assert!(copy.take(hwnd), "PrintWindow");
                let took = t.elapsed();
                let origin = client(hwnd).map_or((0, 0), |(_, _, o)| (o.x, o.y));
                let shown = serve(shared, &mut copy, Kind::PRINT_FLAGS, false, hwnd, origin, readers, None);
                println!("window copy {took:?}, painted {shown:?}");
            }
        });
    }

    const READER: &str = r#"
import sys, time
sys.path.insert(0, sys.argv[2])   # bossbar, next to the app (given by the test)
import wincapture as wc
hwnd = int(sys.argv[1])
for part in (0.42, None, 0.42):
    ok = 0; t = time.time(); ms = 0
    while time.time() - t < 1.5:
        t0 = time.perf_counter(); im = wc.shared.read(hwnd, part); ms += time.perf_counter() - t0
        if im is not None: ok += 1; size = im.size
        time.sleep(0.05)
    print(f'part {part}: {ok} frames in 1.5 s, last {size if ok else None}, {ms / max(ok, 1) * 1000:.1f} ms per read, source {wc.shared.source!r}, overlays {wc.shared.screen_copy}', flush=True)
"#;

    fn meter_reader_beside_corner(hwnd: HWND, next: impl FnMut(&Shared, &mut [Reader; SLOTS])) {
        python_beside_corner(hwnd, READER, next)
    }

    fn python_beside_corner(hwnd: HWND, script: &str, mut next: impl FnMut(&Shared, &mut [Reader; SLOTS])) {
        let shared = Shared::create().expect("shared memory");
        // slot 15: a reader like Sanctum Helper's corner, every 16 ms
        let at = PICTURE_BASE + 15 * PICTURE_SIZE;
        for (k, v) in [0u16, 0, 10000, 12000].into_iter().enumerate() {
            shared.put_u16(at + 2 * k, v);
        }
        shared.put_u32(at + 8, 1);
        let child = std::process::Command::new("python")
            .args(["-c", script, &(hwnd as usize).to_string(), concat!(env!("CARGO_MANIFEST_DIR"), "/../../bossbar")])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("python");
        let mut readers = [Reader::default(); SLOTS];
        let started = Instant::now();
        let (mut corner, mut copy_us, mut copies) = (0, 0u64, 0u64);
        while started.elapsed().as_millis() < 5000 {
            shared.put_u64(SLOT_BASE + 15 * 16, now_ms() + 2000);
            shared.put_u32(SLOT_BASE + 15 * 16 + 8, 16);
            let before = readers;
            next(&shared, &mut readers);
            if readers[15].last != before[15].last {
                corner += 1;
            }
            for i in 0..SLOTS {
                if readers[i].generation != before[i].generation || readers[i].request != before[i].request {
                    copy_us += shared.get_u32(PICTURE_BASE + i * PICTURE_SIZE + 52) as u64;
                    copies += 1;
                }
            }
            std::thread::sleep(Duration::from_millis(4));
        }
        let out = child.wait_with_output().unwrap();
        print!("{}", String::from_utf8_lossy(&out.stdout));
        println!("fast corner reader served {corner} times in 5 s; {copies} pictures read off the card, {:.2} ms each", copy_us as f64 / copies.max(1) as f64 / 1000.0);
    }
}
