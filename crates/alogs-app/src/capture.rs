//! Shared game capture: one picture of the Aniimo window for every aLogs module.
//!
//! The app takes the game's picture on the graphics card (gpu_capture.rs), the first way that works here:
//! 1. Window capture, only where Windows lets the yellow border be turned off (Windows 11). Only the game
//!    window: our overlays are never in it.
//! 2. Desktop Duplication: the screen the game is on, only while the game is in front. Our overlays can be in
//!    the picture (flag bit 1), so readers hide their overlays from capture then.
//! Window capture is tried again once a minute while on Desktop Duplication.
//! On Windows 10, General Settings › "Capture only the game window" uses window capture anyway, with Windows'
//! yellow border round the game: only the game, never other programs' overlays. The border shows only while
//! something reads pictures; nobody asking closes the capture (and the border with it).
//!
//! Not in front on Windows 10: while no window shows over the game, Desktop Duplication goes on (the screen still
//! shows all of it). With a window over it, the game window alone is taken as on Windows 11, by window capture with
//! Windows 10's yellow border (behind whatever covers the game), closed as soon as the game is uncovered or back in
//! front. Only where that can't start or shows nothing: PrintWindow, at most 4 times a second (each copy makes the
//! game hold a frame about twice as long: measured 15 Nov at 4K).
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
//!   1344 u64 clean until (unix ms): a proof is being taken (the meter); every aLogs program keeps its overlay windows
//!      out of screen captures until then (B9)
//!   1792 u64 game only until (unix ms), a reader's ask on Windows 10: while a result, Party or menu screen is up, window
//!      capture, border and all, so what's read is the game and not what's on top of it. Closed again when it runs out
//!   1352 overlay windows (B9): 3 x 144 { u64 updated (unix ms), u32 pid, u32 count (<= 8), 8 x i32 x0, y0, x1, y1
//!      (screen px) }, owners 0 the meter, 1 Sanctum Helper, 2 the app (the button over the game). Overlays stay in
//!      screenshots and recordings; every reader of a screen copy covers all of them in its own picture. A block not
//!      updated for 3 s is stale (its program closed).
//!   2056 rectangles (B8): SLOTS x 120, for a reader that needs several separate parts of the frame (the meter's
//!      boxes) instead of the one box around them all:
//!      0 u32 count (0: the single area above, as before; at most 7)   4 u32 unused
//!      8 7 x 16 { u16 x0, y0, x1, y1 shares (reader), u16 x, y, w, h client pixels (app, w = 0: off the image) }
//!      The reader writes them before its request id, as with the area. Its picture is then the parts one after
//!      another in its pool part, each w * 4 bytes a row (stride 0 at 36); 40-46 is the box around them; shrink 1.
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
    EnumWindows, GetClassNameW, GetClientRect, GetForegroundWindow, GetTopWindow, GetWindow, GetWindowDisplayAffinity,
    GetWindowLongW, GetWindowRect, GetWindowTextW, IsIconic, IsWindow, IsWindowVisible, GWL_EXSTYLE, GW_HWNDNEXT,
    WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED, WS_EX_TRANSPARENT,
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
const RECT_BASE: usize = 2056;
const RECT_SIZE: usize = 120;
const RECTS: usize = 7;
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
/// A picture the graphics card hasn't finished copying off is looked at again this many ms later (Gpu::ready).
const NOT_READY_MS: u64 = 4;

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

    /// The separate parts slot `slot` wants (client pixels), when it asks for several instead of one area.
    fn rects(&self, slot: usize, width: u32, height: u32) -> Option<Vec<Area>> {
        let at = RECT_BASE + slot * RECT_SIZE;
        let count = (self.get_u32(at) as usize).min(RECTS);
        if count == 0 {
            return None;
        }
        Some((0..count).map(|k| {
            let share = [0, 2, 4, 6].map(|j| self.get_u16(at + 8 + 16 * k + j) as u64);
            let x0 = (share[0] * width as u64 / 65535).min(width as u64 - 1) as u32;
            let y0 = (share[1] * height as u64 / 65535).min(height as u64 - 1) as u32;
            let x1 = ((share[2] * width as u64 + 65534) / 65535).clamp(x0 as u64 + 1, width as u64) as u32;
            let y1 = ((share[3] * height as u64 + 65534) / 65535).clamp(y0 as u64 + 1, height as u64) as u32;
            Area { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
        }).collect())
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
    // The game is a Unity player; this keeps browser tabs and folders named "Aniimo" out. Asked before the title, and
    // that order matters: the title of one of aLogs' own windows is asked of the window's thread (WM_GETTEXT) and
    // waits for its answer. 15 Nov: Sanctum Helper's watcher asked while holding the lock the window's thread was
    // waiting for, and the window froze for good ("Not Responding"). A class name never waits on anyone.
    GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if wide(&buf) != "UnityWndClass" {
        return 1;
    }
    buf = [0u16; 256];
    GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if wide(&buf).to_lowercase() != search.title {
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

/// What the capture did for its readers in the last minute (said in app.log once a minute: where pictures stall).
static STAT_ROUNDS: AtomicU64 = AtomicU64::new(0);
static STAT_NEW_IMAGE: AtomicU64 = AtomicU64::new(0);
static STAT_SERVED: AtomicU64 = AtomicU64::new(0);
static STAT_SAME: AtomicU64 = AtomicU64::new(0);
static STAT_NOT_READY: AtomicU64 = AtomicU64::new(0);
static STAT_BLACK: AtomicU64 = AtomicU64::new(0);
static STAT_BROKEN: AtomicU64 = AtomicU64::new(0);
static STAT_READ_US: AtomicU64 = AtomicU64::new(0);
/// The longest a picture's write mark (its seq odd: readers wait, then give up with "the picture was being written")
/// was held, microseconds.
static STAT_WRITE_MAX_US: AtomicU64 = AtomicU64::new(0);
static STAT_SAID: AtomicU64 = AtomicU64::new(0);

fn stats_line(now: u64, kind: &str) {
    let said = STAT_SAID.load(Ordering::Relaxed);
    if said == 0 {
        STAT_SAID.store(now, Ordering::Relaxed);
        return;
    }
    if now < said + 60_000 {
        return;
    }
    STAT_SAID.store(now, Ordering::Relaxed);
    let t = |a: &AtomicU64| a.swap(0, Ordering::Relaxed);
    let (rounds, new, served, same, not_ready, black, broken, us, write_max) = (
        t(&STAT_ROUNDS), t(&STAT_NEW_IMAGE), t(&STAT_SERVED), t(&STAT_SAME), t(&STAT_NOT_READY), t(&STAT_BLACK), t(&STAT_BROKEN),
        t(&STAT_READ_US), t(&STAT_WRITE_MAX_US),
    );
    crate::app_log(&format!(
        "capture, last minute ({kind}): {rounds} rounds, {new} new game images, {served} pictures written ({:.1} ms each, write mark held at most {:.1} ms), {same} kept as current, {not_ready} not ready on the card yet, {black} black rounds, {broken} broken; {}",
        if served > 0 { us as f64 / served as f64 / 1000.0 } else { 0.0 },
        write_max as f64 / 1000.0,
        crate::gpu_capture::copy_stats_line(),
    ));
}

static GEN_SEEN: AtomicU64 = AtomicU64::new(u64::MAX);

static GAME_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// A reader asked for the game window alone just now (GAME_ONLY_UNTIL).
static BURST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const GAME_ONLY_UNTIL: usize = 1792;

/// General Settings › Capture only the game window, on Windows 10 (Windows 11 always does): window capture with
/// Windows' yellow border instead of Desktop Duplication. Read from settings.json every 2 s by the capture thread.
fn game_window_only() -> bool {
    !borderless_capture() && (GAME_ONLY.load(Ordering::Relaxed) || BURST.load(Ordering::Relaxed))
}

/// The best way to take pictures here: window capture where it can be borderless or the player chose it.
fn best_kind() -> Kind {
    if borderless_capture() || game_window_only() { Kind::Window } else { Kind::Duplication }
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
            // with the border only where it can't be turned off and the player chose the game window only
            Kind::Window => Source::Window(WindowCapture::new(window, game_window_only())?),
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
    /// Whether read() of this area can go without waiting for the graphics card (Gpu::ready), and from which image
    /// (generation): with the card busy, the copy finished can be of an older image than the newest. None: not yet.
    /// The others are always ready, from the newest.
    fn ready(&mut self, _x: u32, _y: u32, _w: u32, _h: u32, _shrink: u32) -> Option<u64> { Some(self.generation()) }
    /// Starts an area's copy ahead of read() (the graphics card: all of a round's copies, then one wait).
    fn queue(&mut self, _x: u32, _y: u32, _w: u32, _h: u32, _shrink: u32) {}
    /// The round is over: copies queued but not read are dropped.
    fn end_round(&mut self) {}
    /// When the newest image was captured, if the source says (window capture does).
    fn captured_at(&self) -> Option<Instant> { None }
    /// The image (generation) the last read() came from and when it was captured: the newest, except on the card.
    fn served(&self) -> (u64, Option<Instant>) { (self.generation(), self.captured_at()) }
}

/// How old an image captured at `captured` is at `now` (ms, for bytes 62-63 of a picture: readers take the picture's
/// time minus this as when the game showed it). 0 when the source doesn't say.
fn age_ms(captured: Option<Instant>, now: Instant) -> u16 {
    captured.map_or(0, |t| now.saturating_duration_since(t).as_millis().min(u16::MAX as u128) as u16)
}

/// Whether a copy of image `copy` is a new picture for reader `r`, asking for `request` at `shrink` (`has`: its
/// picture is of that request now). One no newer than the picture it has is not: with the card busy, the copy finished
/// can be of an older image than the newest, and written again and stamped as new, the reader took an old picture for
/// a new one (and was then kept on it as current until the next image came).
fn is_new(r: &Reader, request: u32, shrink: u32, has: bool, copy: u64) -> bool {
    !(has && r.request == request && r.shrink == shrink && copy <= r.generation)
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
    fn queue(&mut self, x: u32, y: u32, w: u32, h: u32, shrink: u32) {
        let _ = Gpu::queue(self, x, y, w, h, shrink);
    }
    fn ready(&mut self, x: u32, y: u32, w: u32, h: u32, shrink: u32) -> Option<u64> {
        Gpu::ready(self, x, y, w, h, shrink).unwrap_or(Some(self.generation)) // an error: read() says so
    }
    fn end_round(&mut self) {
        Gpu::end_round(self);
    }
    fn captured_at(&self) -> Option<Instant> {
        self.captured_at
    }
    fn served(&self) -> (u64, Option<Instant>) {
        Gpu::served(self)
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

/// A window above the game's, as far as hiding the game on screen goes.
struct Above {
    rect: RECT,     // what of it shows, on screen
    ex_style: u32,
    cloaked: bool,  // there for Windows, drawn nowhere (another desktop, a parked app window)
    excluded: bool, // kept out of captures, like aLogs' own overlays
}

/// Whether that window hides any of the game's picture (`game`: its client area on screen) from Desktop Duplication.
/// Windows that let clicks through (overlays) don't count: they're over the game while it's in front too.
fn covers(game: &RECT, w: &Above) -> bool {
    let overlap = w.rect.left < game.right && w.rect.right > game.left && w.rect.top < game.bottom && w.rect.bottom > game.top;
    let click_through = WS_EX_LAYERED | WS_EX_TRANSPARENT;
    overlap && !w.cloaked && !w.excluded && w.ex_style & click_through != click_through
}

/// No other window shows over the game's picture, so the screen still shows all of it
/// though another window has the focus. Walks the windows from the top one down to the game's; false when unsure.
fn uncovered(hwnd: HWND) -> bool {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
    let Some((width, height, origin)) = client(hwnd) else { return false };
    let game = RECT { left: origin.x, top: origin.y, right: origin.x + width as i32, bottom: origin.y + height as i32 };
    unsafe {
        let mut at = GetTopWindow(null_mut());
        for _ in 0..4000 {
            if at.is_null() {
                return false; // the game's window isn't among them
            }
            if at == hwnd {
                return true;
            }
            if IsWindowVisible(at) != 0 && IsIconic(at) == 0 {
                let window = windows::Win32::Foundation::HWND(at as _);
                // what shows of it: a maximized window's own rectangle reaches 8 px onto the next screen
                let mut rect: RECT = std::mem::zeroed();
                let size = std::mem::size_of::<RECT>() as u32;
                if DwmGetWindowAttribute(window, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut RECT as *mut _, size).is_err() {
                    GetWindowRect(at, &mut rect);
                }
                let mut cloaked = 0u32;
                let _ = DwmGetWindowAttribute(window, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
                let mut affinity = 0u32;
                GetWindowDisplayAffinity(at, &mut affinity);
                let above = Above {
                    rect,
                    ex_style: GetWindowLongW(at, GWL_EXSTYLE) as u32,
                    cloaked: cloaked != 0,
                    excluded: affinity == WDA_EXCLUDEFROMCAPTURE,
                };
                if covers(&game, &above) {
                    return false;
                }
            }
            at = GetWindow(at, GW_HWNDNEXT);
        }
    }
    false
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
    /// The request, image generation and shrink in its picture now: the image its copy came from, which with the card
    /// busy can be older than the newest (Picture::served).
    request: u32,
    generation: u64,
    shrink: u32,
    /// When that image was captured (window capture says; None otherwise).
    captured: Option<Instant>,
    /// Its part of the pool (bytes from the pool's start), kept until its area grows.
    offset: usize,
    room: usize,
}

/// Starts every reader's picture over (a new source, tabbing in or out): their pictures must be written again
/// before readers take them, so the served ids go to 0; their parts of the pool are kept, so a picture is never
/// written over another reader's part while that reader may still be copying it.
fn forget(shared: &Shared, readers: &mut [Reader; SLOTS]) {
    for (i, r) in readers.iter_mut().enumerate() {
        (r.request, r.generation, r.last, r.captured) = (0, 0, 0, None);
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

/// `place`, and when no gap is big enough (the other readers' pictures sit spread over the pool, so a whole 4K frame
/// - the meter reading the result screen - never fits): the others give up their room and this reader starts at the
/// pool's start. Each is first marked not served, under its own sequence number, so its reader waits for its next
/// picture instead of reading one being overwritten; they're placed again on their next turn. Without this the
/// reader was skipped every round ("no room this time") until another reader stopped asking, and the meter had no
/// picture for up to 27 s right after an Interlink result (meter.log 2026-11-06).
fn place_or_make_room(shared: &Shared, readers: &mut [Reader; SLOTS], active: &[bool; SLOTS], slot: usize, bytes: usize) -> Option<usize> {
    if let Some(offset) = place(readers, active, slot, bytes) {
        return Some(offset);
    }
    if bytes > POOL {
        return None;
    }
    for i in 0..SLOTS {
        if i == slot || readers[i].room == 0 {
            continue;
        }
        let at = PICTURE_BASE + i * PICTURE_SIZE;
        shared.write(at + 16, || shared.put_u32(at + 12, 0));
        (readers[i].room, readers[i].request) = (0, 0);
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let said = ROOM_LOG.lock().unwrap().note(now);
    ROOM_UNSAID.store(said.is_none(), Ordering::Relaxed);
    match said {
        Some(0) => crate::app_log(&format!("capture: no room for a {} KB picture (reader {slot}): the other readers were moved (said once a minute from now on)", bytes / 1024)),
        Some(more) => crate::app_log(&format!("capture: no room: the other readers were moved {more} more times, the last for a {} KB picture (reader {slot})", bytes / 1024)),
        None => {}
    }
    Some(0)
}

/// One app.log had 1,476 "no room" lines in 7 hours, up to about 10 a second while several
/// whole-frame readers were asking. The first is said, then a count at most once a minute; a count left when they
/// stop is said once its minute is over (room_log_rest), so the lines add up to how often it happened.
struct RoomLog {
    said_at: Option<u64>, // ms
    since: u32,           // times since it was last said
}
static ROOM_LOG: std::sync::Mutex<RoomLog> = std::sync::Mutex::new(RoomLog { said_at: None, since: 0 });
/// A count is waiting to be said: while none is, each round costs one plain read.
static ROOM_UNSAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Each round: the count the switch held back, when no further time came to say it at.
fn room_log_rest(now: u64) {
    if !ROOM_UNSAID.load(Ordering::Relaxed) {
        return;
    }
    if let Some(more) = ROOM_LOG.lock().unwrap().rest(now) {
        ROOM_UNSAID.store(false, Ordering::Relaxed);
        crate::app_log(&format!("capture: no room: the other readers were moved {more} more times"));
    }
}

impl RoomLog {
    /// Another time at `now` (ms): what to say. Some(0): the first; Some(n): n times since the last line; None: nothing yet.
    fn note(&mut self, now: u64) -> Option<u32> {
        match self.said_at {
            None => {
                self.said_at = Some(now);
                Some(0)
            }
            Some(at) if now.saturating_sub(at) >= 60_000 => {
                let more = self.since + 1;
                (self.said_at, self.since) = (Some(now), 0);
                Some(more)
            }
            Some(_) => {
                self.since += 1;
                None
            }
        }
    }

    /// The times not said yet, once the minute they were kept for is over.
    fn rest(&mut self, now: u64) -> Option<u32> {
        match self.said_at {
            Some(at) if self.since > 0 && now.saturating_sub(at) >= 60_000 => {
                let more = self.since;
                (self.said_at, self.since) = (Some(now), 0);
                Some(more)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod room_log_tests {
    use super::RoomLog;

    #[test]
    fn the_first_then_a_count_each_minute() {
        let mut log = RoomLog { said_at: None, since: 0 };
        assert_eq!(log.note(1_000), Some(0));
        assert_eq!(log.note(1_100), None);
        assert_eq!(log.note(30_000), None);
        assert_eq!(log.note(61_000), Some(3));
        assert_eq!(log.note(61_100), None);
        assert_eq!(log.note(500_000), Some(2));
    }

    #[test]
    fn a_count_left_over_is_said_when_its_minute_is_up() {
        let mut log = RoomLog { said_at: None, since: 0 };
        assert_eq!(log.rest(1_000), None, "nothing happened yet");
        assert_eq!(log.note(1_000), Some(0));
        assert_eq!(log.rest(70_000), None, "the first was said and nothing came after");
        assert_eq!(log.note(70_100), Some(1), "over a minute after the first: said at once");
        assert_eq!(log.note(70_200), None);
        assert_eq!(log.note(80_000), None);
        assert_eq!(log.rest(100_000), None, "its minute isn't over");
        assert_eq!(log.rest(130_100), Some(2));
        assert_eq!(log.rest(200_000), None, "said, so not again");
        // every time is in exactly one line: 1 (first) + 1 + 2 = the 4 notes above
        assert_eq!(log.note(131_000), None, "a new minute starts from the line just written");
        assert_eq!(log.rest(190_100), Some(1));
    }
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
    let client = client(hwnd)?;
    serve_client(shared, gpu, flags, trust_black, hwnd, client, image_origin, readers, only)
}

/// serve, with the game window's client size and its position on screen (`client`) already known.
#[allow(clippy::too_many_arguments)]
fn serve_client(
    shared: &Shared, gpu: &mut dyn Picture, flags: u32, trust_black: bool, hwnd: HWND, client: (u32, u32, POINT),
    image_origin: (i32, i32), readers: &mut [Reader; SLOTS], only: Option<usize>,
) -> Option<bool> {
    let (width, height, origin) = client;
    if !gpu.has_image() {
        return None;
    }
    let now = now_ms();
    room_log_rest(now);
    let (iw, ih) = gpu.size();
    let (cx, cy) = (origin.x - image_origin.0, origin.y - image_origin.1); // the client area inside the image
    let generation = gpu.generation();
    STAT_ROUNDS.fetch_add(1, Ordering::Relaxed);
    if GEN_SEEN.swap(generation, Ordering::Relaxed) != generation {
        STAT_NEW_IMAGE.fetch_add(1, Ordering::Relaxed);
    }
    let active: [bool; SLOTS] = std::array::from_fn(|i| shared.active(i, now));
    // a reader that stopped asking gives up its part of the pool, and its picture is no longer current: when it
    // comes back, its picture is written again first (its old part may hold someone else's by then)
    for i in 0..SLOTS {
        if active[i] || (readers[i].room == 0 && readers[i].request == 0) {
            continue;
        }
        readers[i] = Reader::default();
        shared.write(PICTURE_BASE + i * PICTURE_SIZE + 16, || shared.put_u32(PICTURE_BASE + i * PICTURE_SIZE + 12, 0));
        shared.put_u32(RECT_BASE + i * RECT_SIZE, 0); // the next reader of this slot may not know about parts
    }
    let mut result = None;
    // first every due reader's area is queued on the graphics card, then read one after another below: the round
    // waits for the card once (the first read), not once per reader (B8; each wait was a small stall in the game)
    for slot in (0..SLOTS).filter(|&i| active[i] && only.map_or(true, |o| o == i)) {
        let r = readers[slot];
        if now < r.last + shared.interval(slot) { continue; }
        let (area, request) = shared.wanted(slot, width, height);
        let at = PICTURE_BASE + slot * PICTURE_SIZE;
        let shrink = match unsafe { *shared.base.add(at + 60) } { 2 => 2u32, 4 => 4, _ => 1 };
        if r.request == request && r.generation == generation && r.shrink == shrink && shared.get_u32(at + 12) == request { continue; }
        if let Some(parts) = shared.rects(slot, width, height) {
            for p in parts.iter().filter_map(|p| on_image(*p, cx, cy, iw, ih)) {
                gpu.queue(p.x, p.y, p.w, p.h, 1);
            }
            continue;
        }
        let Some(p) = on_image(area, cx, cy, iw, ih) else { continue };
        let shrink = if p.w / shrink == 0 || p.h / shrink == 0 { 1 } else { shrink };
        gpu.queue(p.x, p.y, p.w, p.h, shrink);
    }
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
        let has = shared.get_u32(at + 12) == request;
        if r.request == request && r.generation == generation && r.shrink == shrink && has {
            STAT_SAME.fetch_add(1, Ordering::Relaxed);
            // same image, same area: still current. Its age moves on with its time, so time minus age still says when
            // its image was captured (with the age left as it was, a picture kept current looked newer each time)
            shared.put_u16(at + 62, age_ms(r.captured, Instant::now()));
            shared.put_u64(at + 24, now);
            readers[slot].last = now;
            continue;
        }
        if let Some(parts) = shared.rects(slot, width, height) {
            match serve_parts(shared, gpu, slot, &parts, (cx, cy, iw, ih), (width, height), flags, trust_black, readers, &active, request, now) {
                Some(Some((shown, from, captured))) => {
                    result = Some(result.unwrap_or(false) || shown);
                    readers[slot] = Reader { last: now, request, generation: from, captured, shrink: 1, ..readers[slot] };
                }
                Some(None) => {}
                None => {
                    gpu.end_round();
                    return None;
                }
            }
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
        // the card hasn't finished copying it off (busy with the game), or only a copy no newer than the picture the
        // reader has: it keeps that one, readable and where it is, and this one is looked at again in a few ms - never
        // waited for with the picture marked "being written"
        let fresh = gpu.ready(x0 as u32, y0 as u32, w, h, shrink).filter(|&copy| is_new(&r, request, shrink, has, copy));
        if fresh.is_none() {
            STAT_NOT_READY.fetch_add(1, Ordering::Relaxed);
            readers[slot].last = now.saturating_sub(shared.interval(slot)).saturating_add(NOT_READY_MS);
            continue;
        }
        if readers[slot].room < bytes || r.request != request {
            match place_or_make_room(shared, readers, &active, slot, bytes) {
                Some(offset) => (readers[slot].offset, readers[slot].room) = (offset, bytes),
                None => continue, // bigger than the whole pool
            }
        }
        let started = Instant::now();
        let out = unsafe { shared.pool().add(readers[slot].offset) };
        let mut from = (generation, None);
        let ok = shared.write(at + 16, || {
            if !gpu.read(x0 as u32, y0 as u32, w, h, out, stride, shrink) {
                return false;
            }
            from = gpu.served();
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
            // how old the image already was (ms, from its capture time; 0 when the source doesn't say): bytes 62-63.
            // The image its copy came from, not the newest one: with the card busy that can be an older one
            shared.put_u16(at + 62, age_ms(from.1, Instant::now()));
            shared.put_u32(at + 12, request);
            result = Some(result.unwrap_or(false) || shown);
            true
        });
        let held = started.elapsed().as_micros() as u64; // the picture's write mark (seq odd) was held this long
        STAT_WRITE_MAX_US.fetch_max(held, Ordering::Relaxed);
        if !ok {
            gpu.end_round(); // copies queued for the rest of the round go with it
            return None;
        }
        STAT_SERVED.fetch_add(1, Ordering::Relaxed);
        STAT_READ_US.fetch_add(held, Ordering::Relaxed);
        readers[slot] = Reader { last: now, request, generation: from.0, captured: from.1, shrink, ..readers[slot] };
    }
    gpu.end_round();
    if let Some(shown) = result {
        shared.header((width, height), (shown || trust_black) as u32 | flags, hwnd, origin);
        shared.put_u32(56, shared.get_u32(56).wrapping_add(1));
    }
    Some(result.unwrap_or(true))
}

/// `area` (client pixels) as image pixels, cut to what's on the image (a window partly off its screen).
fn on_image(area: Area, cx: i32, cy: i32, iw: u32, ih: u32) -> Option<Area> {
    let (x0, y0) = ((cx + area.x as i32).max(0), (cy + area.y as i32).max(0));
    let (x1, y1) = ((cx + (area.x + area.w) as i32).min(iw as i32), (cy + (area.y + area.h) as i32).min(ih as i32));
    (x1 > x0 && y1 > y0).then(|| Area { x: x0 as u32, y: y0 as u32, w: (x1 - x0) as u32, h: (y1 - y0) as u32 })
}

/// Writes a reader's picture made of several parts (see the layout: rectangles), one after another in its part
/// of the pool. Some(Some((shown, image, captured))): written, as old as its oldest part (the image that one came from
/// and when that was captured); Some(None): skipped this time (nothing on the image, not ready, no room);
/// None: the graphics card failed.
#[allow(clippy::too_many_arguments)]
fn serve_parts(
    shared: &Shared, gpu: &mut dyn Picture, slot: usize, parts: &[Area], image: (i32, i32, u32, u32), size: (u32, u32),
    flags: u32, trust_black: bool, readers: &mut [Reader; SLOTS], active: &[bool; SLOTS], request: u32, now: u64,
) -> Option<Option<(bool, u64, Option<Instant>)>> {
    let (cx, cy, iw, ih) = image;
    let on: Vec<Option<Area>> = parts.iter().map(|p| on_image(*p, cx, cy, iw, ih)).collect();
    let bytes: usize = on.iter().flatten().map(|p| p.w as usize * p.h as usize * 4).sum();
    if bytes == 0 {
        return Some(None);
    }
    let at = PICTURE_BASE + slot * PICTURE_SIZE;
    // as in serve: parts the card hasn't finished copying off, or a picture no newer than the one the reader has
    // (from its oldest part), are looked at again shortly, never waited for
    let mut oldest = Some(u64::MAX);
    for p in on.iter().flatten() {
        oldest = oldest.zip(gpu.ready(p.x, p.y, p.w, p.h, 1)).map(|(a, b)| a.min(b));
        if oldest.is_none() {
            break;
        }
    }
    let has = shared.get_u32(at + 12) == request;
    if oldest.filter(|&copy| is_new(&readers[slot], request, 1, has, copy)).is_none() {
        STAT_NOT_READY.fetch_add(1, Ordering::Relaxed);
        readers[slot].last = now.saturating_sub(shared.interval(slot)).saturating_add(NOT_READY_MS);
        return Some(None);
    }
    if readers[slot].room < bytes || readers[slot].request != request {
        match place_or_make_room(shared, readers, active, slot, bytes) {
            Some(offset) => (readers[slot].offset, readers[slot].room) = (offset, bytes),
            None => return Some(None),
        }
    }
    let rects = RECT_BASE + slot * RECT_SIZE;
    let started = Instant::now();
    let mut shown = false;
    let mut from: (u64, Option<Instant>) = (u64::MAX, None);
    let ok = shared.write(at + 16, || {
        let (mut offset, mut bx0, mut by0, mut bx1, mut by1) = (readers[slot].offset, i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for (k, p) in on.iter().enumerate() {
            let Some(p) = p else {
                for j in 0..4 {
                    shared.put_u16(rects + 8 + 16 * k + 8 + 2 * j, 0);
                }
                continue;
            };
            let out = unsafe { shared.pool().add(offset) };
            if !gpu.read(p.x, p.y, p.w, p.h, out, p.w as usize * 4, 1) {
                return false;
            }
            let part = gpu.served();
            if part.0 < from.0 {
                from = part;
            }
            shown |= painted(out, p.w, p.h, p.w as usize * 4);
            let (x, y) = (p.x as i32 - cx, p.y as i32 - cy);
            for (j, v) in [x, y, p.w as i32, p.h as i32].into_iter().enumerate() {
                shared.put_u16(rects + 8 + 16 * k + 8 + 2 * j, v as u16);
            }
            (bx0, by0, bx1, by1) = (bx0.min(x), by0.min(y), bx1.max(x + p.w as i32), by1.max(y + p.h as i32));
            offset += p.w as usize * p.h as usize * 4;
        }
        unsafe { *shared.base.add(at + 61) = 1 };
        shared.put_u64(at + 24, now);
        shared.put_u32(at + 32, readers[slot].offset as u32);
        shared.put_u32(at + 36, 0);
        for (k, v) in [bx0, by0, bx1 - bx0, by1 - by0].into_iter().enumerate() {
            shared.put_u16(at + 40 + 2 * k, v as u16);
        }
        shared.put_u32(at + 48, (shown || trust_black) as u32 | flags);
        shared.put_u32(at + 52, started.elapsed().as_micros().min(u32::MAX as u128) as u32);
        shared.put_u16(at + 56, size.0 as u16);
        shared.put_u16(at + 58, size.1 as u16);
        shared.put_u16(at + 62, age_ms(from.1, Instant::now())); // (serve: the image its oldest part came from)
        shared.put_u32(at + 12, request);
        true
    });
    STAT_WRITE_MAX_US.fetch_max(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    ok.then_some(Some((shown, from.0, from.1)))
}

/// The app's own reader (the capture button over the game, overlay_button.rs): request slot SLOTS - 1, kept under
/// the app's process id, for one small area, read like any other reader's picture.
pub struct AppReader {
    shared: Shared,
    request: u32,
    area: [u16; 4],
}

const CLEAN_UNTIL: usize = 1344;
const OVERLAY_BASE: usize = 1352;
const OVERLAY_BLOCK: usize = 144;
const OVERLAY_MAX: usize = 8;
pub const OWNER_APP: usize = 2;

impl AppReader {
    /// Our overlay windows' spots (screen px: x0, y0, x1, y1), for the other readers to cover (see the layout).
    pub fn publish_overlays(&self, owner: usize, rects: &[(i32, i32, i32, i32)]) {
        let (s, at) = (&self.shared, OVERLAY_BASE + owner * OVERLAY_BLOCK);
        let n = rects.len().min(OVERLAY_MAX);
        for (k, r) in rects.iter().take(n).enumerate() {
            for (j, v) in [r.0, r.1, r.2, r.3].into_iter().enumerate() {
                s.put_u32(at + 16 + 16 * k + 4 * j, v as u32);
            }
        }
        s.put_u32(at + 8, std::process::id());
        s.put_u32(at + 12, n as u32);
        s.put_u64(at, now_ms());
    }

    /// A proof is being taken: overlays out of screen captures now.
    pub fn clean_now(&self) -> bool {
        now_ms() < self.shared.get_u64(CLEAN_UNTIL)
    }

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
                crate::app_log(&format!("shared capture: could not create {NAME}"));
                return;
            };
            GAME_ONLY.store(crate::read_settings().pointer("/capture/game_window_only").and_then(serde_json::Value::as_bool).unwrap_or(false), Ordering::Relaxed);
            let mut best = best_kind();
            let mut kind = best;
            let mut settings_read = 0u64;
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
            let mut away_border = true; // window capture; false once it can't start or shows nothing for this game window
            let mut away_tries = 0u32; // window capture's failed starts and breaks in a row
            let mut away_black_since = 0u64;
            let mut away_last = 0u64;
            let mut away_said = false;
            let mut on_screen_said = false; // said that it stays on the screen though the game isn't in front
            loop {
                let now = now_ms();
                shared.put_u64(HEARTBEAT, now); // alive (readers notice when the app is gone)
                // the player switched "Capture only the game window": start again the chosen way
                if now >= settings_read + 2000 {
                    settings_read = now;
                    let on = crate::read_settings().pointer("/capture/game_window_only").and_then(serde_json::Value::as_bool).unwrap_or(false);
                    if GAME_ONLY.swap(on, Ordering::Relaxed) != on && best_kind() != best {
                        best = best_kind();
                        (kind, source, tries, black_since) = (best, None, 0, 0);
                    }
                }
                // a reader's burst of the game window alone starts or runs out: start again the way that's best now
                let burst = now < shared.get_u64(GAME_ONLY_UNTIL);
                if BURST.swap(burst, Ordering::Relaxed) != burst && best_kind() != best {
                    best = best_kind();
                    (kind, source, tries, black_since) = (best, None, 0, 0);
                }
                // the DPS meter switched off (the overlay button or the app): the flag tells the meter
                let off = !crate::overlay_button::capture_on();
                if shared.get_u32(60) != off as u32 {
                    shared.write(8, || shared.put_u32(60, off as u32));
                }
                // Switched off is the DPS meter's switch: the meter sees the flag and rests, unless it still reads
                // results for the leaderboards (their own switch). Sanctum Helper has its own switch too, so the
                // readers left (the meter reading results, Sanctum Helper, the button's look at its icon row) keep
                // their pictures. Nobody asking: nothing is captured.
                let only: Option<usize> = None;
                // the next reader due (each at its own pace), or nobody: sleep
                let due = (0..SLOTS)
                    .filter(|&i| shared.active(i, now) && only.map_or(true, |o| o == i))
                    .map(|i| readers[i].last + shared.interval(i))
                    .min();
                // Dev switch capture-frame-hold: window capture is told the fastest reader's interval (unused with it off)
                let fastest = (0..SLOTS).filter(|&i| shared.active(i, now) && only.map_or(true, |o| o == i)).map(|i| shared.interval(i)).min();
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
                    (away_copy, away_window, away_border, away_black_since, away_tries) = (None, None, true, 0, 0);
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
                let away = kind.needs_front() && unsafe { GetForegroundWindow() } != hwnd;
                // Another window has the focus but none shows over the game (it's on
                // another screen, or it's a hidden one), so the screen still shows the game: stay on it. The window
                // copy below makes the game hold a frame twice as long, 4 times a second (measured 15 Nov).
                let on_screen = away && uncovered(hwnd);
                if on_screen != on_screen_said {
                    on_screen_said = on_screen;
                    if on_screen {
                        crate::app_log(&format!("shared capture: another window has the focus but none covers the game; staying on the screen"));
                    } else if away {
                        crate::app_log(&format!("shared capture: a window covers the game; copying the game window"));
                    }
                }
                if away && !on_screen {
                    // A window is over the game: Desktop Duplication sees that window. Take the game window alone, as
                    // Windows 11 does: window capture, on the graphics card, at the readers' own pace. Windows 10 draws
                    // its yellow border round the game for it, behind whatever covers it. Only where that can't start
                    // or shows nothing: the window copy (PrintWindow), slowly, since each one makes the game wait.
                    if !away_border && now < away_last + AWAY_MS {
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
                        if let Some(w) = away_window.as_mut() {
                            w.fastest_reader(fastest);
                        }
                        match away_window.as_mut() {
                            Some(w) => match w.update() {
                                Ok(_) => {
                                    let origin = window_frame_origin(windows::Win32::Foundation::HWND(hwnd as _)).unwrap_or((0, 0));
                                    serve(&shared, &mut w.gpu, Kind::Window.flags(), true, hwnd, origin, &mut readers, only)
                                }
                                Err(_) => {
                                    away_window = None;
                                    away_tries += 1;
                                    None
                                }
                            },
                            None => {
                                away_tries += 1;
                                None
                            }
                        }
                    };
                    if away_border && away_tries >= GPU_TRIES {
                        crate::app_log(&format!("shared capture: window capture can't start while a window covers the game; copying the game window"));
                        (away_border, away_window, away_said, away_black_since) = (false, None, false, 0);
                    }
                    match result {
                        Some(true) => {
                            (away_black_since, away_tries) = (0, 0);
                            if !away_said {
                                away_said = true;
                                crate::app_log(&format!("shared capture: a window covers the game, using {}", if away_border { "window capture (with border)" } else { "PrintWindow" }));
                            }
                        }
                        Some(false) if away_border => {
                            away_black_since = if away_black_since == 0 { now } else { away_black_since };
                            if now - away_black_since >= BLACK_MS / 2 {
                                crate::app_log(&format!("shared capture: window capture shows nothing while a window covers the game; copying the game window"));
                                (away_border, away_window, away_said, away_black_since) = (false, None, false, 0);
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
                                crate::app_log(&format!("shared capture: {} can't start ({e}); trying the next way", kind.name()));
                                (kind, tries, black_since) = (kind.next().unwrap_or(kind), 0, 0);
                                retry_better = now + RETRY_BETTER_MS;
                            }
                            wait(&shared, &mut readers);
                            continue;
                        }
                    }
                }
                let s = source.as_mut().unwrap();
                if let Source::Window(w) = s {
                    w.fastest_reader(fastest);
                }
                stats_line(now, kind.name());
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
                        STAT_BROKEN.fetch_add(1, Ordering::Relaxed);
                        source = None;
                        wait(&shared, &mut readers);
                        tries += 1;
                        if tries >= GPU_TRIES {
                            crate::app_log(&format!("shared capture: {} keeps failing; trying the next way", kind.name()));
                            (kind, tries, black_since) = (kind.next().unwrap_or(kind), 0, 0);
                            retry_better = now + RETRY_BETTER_MS;
                        }
                    }
                    Some(true) => {
                        (black_since, tries) = (0, 0);
                        if announced != Some(kind) {
                            announced = Some(kind);
                            crate::app_log(&format!("shared capture: using {}", kind.name()));
                        }
                    }
                    Some(false) => {
                        STAT_BLACK.fetch_add(1, Ordering::Relaxed);
                        black_since = if black_since == 0 { now } else { black_since };
                        if now - black_since >= BLACK_MS {
                            black_since = 0;
                            if let Some(next) = kind.next() {
                                crate::app_log(&format!("shared capture: {} gives no picture; trying {}", kind.name(), next.name()));
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

    /// Looking for the game never waits on one of aLogs' own windows: with a visible window whose thread isn't
    /// answering (as the app's window thread wasn't, 15 Nov), the search still comes back.
    #[test]
    fn finding_the_game_never_waits_on_our_own_windows() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE};
        let (made, is_made) = std::sync::mpsc::channel();
        let (release, hold) = std::sync::mpsc::channel::<()>();
        let owner = std::thread::spawn(move || unsafe {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, class.as_ptr(), class.as_ptr(), WS_POPUP | WS_VISIBLE,
                                       -32000, -32000, 1, 1, null_mut(), null_mut(), null_mut(), null_mut());
            made.send(!hwnd.is_null()).unwrap();
            let _ = hold.recv(); // answers nothing meanwhile
            DestroyWindow(hwnd);
        });
        assert!(is_made.recv().unwrap(), "the test's window");
        let (done, is_done) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            find_game_titled();
            let _ = done.send(());
        });
        let came_back = is_done.recv_timeout(Duration::from_secs(5)).is_ok();
        release.send(()).unwrap();
        owner.join().unwrap();
        assert!(came_back, "the search waited on a window of our own that wasn't answering");
    }

    /// Two small readers spread over the pool leave no gap for a whole 4K frame (the meter on the result screen):
    /// `place` alone finds none, and once the others give up their room it starts at the pool's start.
    #[test]
    fn full_frame_finds_room() {
        const MB: usize = 1 << 20;
        let full = MAX_W * MAX_H * 4;
        let mut readers = [Reader::default(); SLOTS];
        let mut active = [false; SLOTS];
        for (slot, offset) in [(1, 20 * MB), (2, 45 * MB)] {
            (readers[slot].offset, readers[slot].room, active[slot]) = (offset, MB, true);
        }
        active[0] = true;
        assert_eq!(place(&readers, &active, 0, full), None);
        for i in 1..SLOTS {
            readers[i].room = 0;
        }
        assert_eq!(place(&readers, &active, 0, full), Some(0));
    }

    impl Shared {
        /// A mapping of the same size with no name, for tests: never the app's own (Local\aLogs.Capture.v2), which a
        /// running aLogs and its readers are using. (Left open: the test process ends soon.)
        fn private() -> Shared {
            unsafe {
                let handle = CreateFileMappingW(INVALID_HANDLE_VALUE, null(), PAGE_READWRITE, (SIZE >> 32) as u32, SIZE as u32, null());
                assert!(!handle.is_null(), "a private mapping");
                let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, SIZE);
                assert!(!view.Value.is_null(), "its view");
                Shared { base: view.Value as *mut u8 }
            }
        }
    }

    /// A pretend graphics card for serve: `newest` is the newest image, and the copy the card has finished is of image
    /// `back`, which lags behind on a busy card (Gpu::ready offers the newest finished copy, from whichever image).
    /// Image g was captured at `captured[g]`; a picture's pixels are all its image's number.
    struct LateCard {
        newest: u64,
        back: Option<u64>,
        captured: Vec<Instant>,
        last: u64,
    }

    impl Picture for LateCard {
        fn has_image(&self) -> bool {
            self.newest > 0
        }
        fn size(&self) -> (u32, u32) {
            (640, 480)
        }
        fn generation(&self) -> u64 {
            self.newest
        }
        fn ready(&mut self, _x: u32, _y: u32, _w: u32, _h: u32, _shrink: u32) -> Option<u64> {
            self.back
        }
        fn read(&mut self, _x: u32, _y: u32, w: u32, h: u32, out: *mut u8, stride: usize, _shrink: u32) -> bool {
            let Some(back) = self.back else { return false };
            for row in 0..h as usize {
                unsafe { std::ptr::write_bytes(out.add(row * stride), back as u8, w as usize * 4) };
            }
            self.last = back;
            true
        }
        fn captured_at(&self) -> Option<Instant> {
            Some(self.captured[self.newest as usize])
        }
        fn served(&self) -> (u64, Option<Instant>) {
            (self.last, Some(self.captured[self.last as usize]))
        }
    }

    /// Change 4: with the card behind, a reader is given the older image's copy with that image's age, its record keeps
    /// that image, and it isn't handed the same copy again as a new picture. Reader 0 asks for one area, reader 1 for
    /// two parts (serve_parts).
    #[test]
    fn an_older_copy_is_never_passed_on_as_new() {
        let shared = Shared::private();
        for slot in [0, 1] {
            shared.put_u64(SLOT_BASE + slot * 16, now_ms() + 60_000);
            shared.put_u32(SLOT_BASE + slot * 16 + 8, 16);
            shared.put_u32(PICTURE_BASE + slot * PICTURE_SIZE + 8, 1); // request 1 (area 0: the whole frame)
        }
        let parts = RECT_BASE + RECT_SIZE;
        shared.put_u32(parts, 2);
        for (k, share) in [[0u16, 0, 20000, 20000], [30000, 30000, 60000, 60000]].into_iter().enumerate() {
            for (j, v) in share.into_iter().enumerate() {
                shared.put_u16(parts + 8 + 16 * k + 2 * j, v);
            }
        }
        let t = Instant::now();
        // image g captured (1000 - 100 g) ms before the test started
        let captured = (0..6).map(|g| t - Duration::from_millis(1000 - 100 * g)).collect();
        let mut card = LateCard { newest: 1, back: Some(1), captured, last: 0 };
        let mut readers = [Reader::default(); SLOTS];
        let round = |card: &mut LateCard, readers: &mut [Reader; SLOTS]| {
            for r in readers.iter_mut() {
                r.last = 0; // due now
            }
            serve_client(&shared, card, 0, true, null_mut(), (640, 480, POINT { x: 0, y: 0 }), (0, 0), readers, None);
        };
        let seq = |slot: usize| shared.get_u64(PICTURE_BASE + slot * PICTURE_SIZE + 16);
        let age = |slot: usize| shared.get_u16(PICTURE_BASE + slot * PICTURE_SIZE + 62) as u128;
        let time = |slot: usize| shared.get_u64(PICTURE_BASE + slot * PICTURE_SIZE + 24);
        let pixel = |slot: usize| unsafe { *shared.pool().add(shared.get_u32(PICTURE_BASE + slot * PICTURE_SIZE + 32) as usize) } as u64;
        let since = |g: u128| t.elapsed().as_millis() + 1000 - 100 * g; // image g's age now
        let near = |a: u128, b: u128| a.abs_diff(b) <= 3;

        round(&mut card, &mut readers);
        for slot in [0, 1] {
            assert_eq!((shared.get_u32(PICTURE_BASE + slot * PICTURE_SIZE + 12), pixel(slot), readers[slot].generation), (1, 1, 1));
            assert!(near(age(slot), since(1)), "image 1's age: {}", age(slot));
        }

        // a new image; the card hasn't finished copying it
        card.newest = 2;
        let before = [(seq(0), time(0)), (seq(1), time(1))];
        std::thread::sleep(Duration::from_millis(20));
        round(&mut card, &mut readers);
        for slot in [0, 1] {
            assert_eq!((seq(slot), time(slot)), before[slot], "the same copy isn't written again, nor its time moved on as if current");
            assert_eq!(readers[slot].generation, 1);
        }

        // two more images; the card has finished image 2's copy only
        (card.newest, card.back) = (4, Some(2));
        round(&mut card, &mut readers);
        for slot in [0, 1] {
            assert_eq!((pixel(slot), readers[slot].generation), (2, 2));
            assert!(near(age(slot), since(2)), "image 2's age ({}), not the newest one's ({})", age(slot), since(4));
        }
        let before = [seq(0), seq(1)];
        round(&mut card, &mut readers);
        assert_eq!([seq(0), seq(1)], before, "image 2 again: nothing new");

        // the newest is back
        card.back = Some(4);
        round(&mut card, &mut readers);
        let shown = |slot: usize| time(slot) as u128 - age(slot); // when its image was captured, as readers take it
        let first = [shown(0), shown(1)];
        assert_eq!((pixel(0), readers[0].generation, pixel(1), readers[1].generation), (4, 4, 4, 4));
        // kept as current: the time moves on, the age with it
        std::thread::sleep(Duration::from_millis(30));
        let before = time(0);
        round(&mut card, &mut readers);
        assert!(time(0) >= before + 30, "still current: its time moves on");
        for slot in [0, 1] {
            assert!(near(shown(slot), first[slot]), "...and its age with it, so it still says when image 4 was captured");
        }
    }

    #[test]
    fn a_new_picture_is_a_newer_image_or_another_area() {
        let r = Reader { request: 7, generation: 5, shrink: 1, ..Reader::default() };
        assert!(!is_new(&r, 7, 1, true, 5), "the image it has");
        assert!(!is_new(&r, 7, 1, true, 4), "an older one");
        assert!(is_new(&r, 7, 1, true, 6), "a newer one");
        assert!(is_new(&r, 8, 1, true, 4), "another area: whatever is finished");
        assert!(is_new(&r, 7, 2, true, 5), "shrunk otherwise");
        assert!(is_new(&r, 7, 1, false, 5), "its picture was taken away (moved, a new source)");
        let now = Instant::now();
        assert_eq!(age_ms(None, now), 0, "the source doesn't say");
        assert_eq!(age_ms(Some(now - Duration::from_millis(250)), now), 250);
        assert_eq!(age_ms(Some(now - Duration::from_secs(100)), now), u16::MAX);
    }

    /// What counts as a window over the game (a 4K game at 0,0).
    #[test]
    fn what_covers_the_game() {
        let game = RECT { left: 0, top: 0, right: 3840, bottom: 2160 };
        let at = |left, top, right, bottom| Above { rect: RECT { left, top, right, bottom }, ex_style: 0, cloaked: false, excluded: false };
        assert!(covers(&game, &at(100, 100, 900, 700)), "a window over the game");
        assert!(covers(&game, &at(0, 2088, 3840, 2160)), "the taskbar over its bottom edge");
        assert!(covers(&game, &at(-500, -500, 1, 1)), "one pixel of overlap");
        assert!(!covers(&game, &at(3840, 0, 6400, 1440)), "a window filling the next screen");
        assert!(!covers(&game, &at(-32000, -32000, -31999, -31999)), "a window parked off screen");
        assert!(!covers(&game, &at(0, 0, 0, 0)), "an empty window");
        assert!(!covers(&game, &Above { cloaked: true, ..at(0, 0, 3840, 2160) }), "a window Windows draws nowhere");
        assert!(!covers(&game, &Above { excluded: true, ..at(0, 0, 400, 300) }), "an overlay kept out of captures");
        assert!(!covers(&game, &Above { ex_style: WS_EX_LAYERED | WS_EX_TRANSPARENT, ..at(0, 0, 3840, 2160) }), "a click-through overlay");
        assert!(covers(&game, &Above { ex_style: WS_EX_LAYERED, ..at(0, 0, 400, 300) }), "a see-through window that takes clicks");
    }

    /// Window capture of the game as it is now, covered or not, for 3 s (Windows 10 shows its yellow border meanwhile):
    /// `cargo test --bin alogs-app capture::tests::window_capture_now -- --exact --ignored --nocapture`
    #[test]
    #[ignore]
    fn window_capture_now() {
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let hwnd = find_game();
        assert!(!hwnd.is_null(), "the game isn't open");
        let t = Instant::now();
        let mut capture = WindowCapture::new(windows::Win32::Foundation::HWND(hwnd as _), true).expect("window capture");
        let started = t.elapsed();
        let (mut new, mut looks, mut slowest) = (0u32, 0u32, Duration::ZERO);
        while t.elapsed() < Duration::from_secs(3) {
            let one = Instant::now();
            new += capture.update().expect("an update") as u32;
            (looks, slowest) = (looks + 1, slowest.max(one.elapsed()));
            std::thread::sleep(Duration::from_millis(16));
        }
        println!("uncovered: {}, started in {started:?}, {new} new frames in {looks} looks, slowest look {slowest:?}", uncovered(hwnd));
        assert!(new > 0, "no frame came");
    }

    /// A look at this PC as it is now (run it with the game open and another window focused):
    /// `cargo test --bin alogs-app capture::tests::game_uncovered_now -- --exact --ignored --nocapture`
    #[test]
    #[ignore]
    fn game_uncovered_now() {
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let hwnd = find_game();
        assert!(!hwnd.is_null(), "the game isn't open");
        let t = Instant::now();
        let free = uncovered(hwnd);
        println!("game in front: {}, uncovered: {free}, the look took {:?}", unsafe { GetForegroundWindow() } == hwnd, t.elapsed());
    }

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
# several parts (B8): the same pixels as a whole frame there, black elsewhere
from PIL import ImageChops
rects = [(0, 0, 1, 0.42), (0.3, 0.5, 0.4, 0.6), (0.6, 0.7, 0.9, 0.8)]
ok = 0; t = time.time(); ms = 0
while time.time() - t < 1.5:
    t0 = time.perf_counter(); im = wc.shared.read_rects(hwnd, rects); ms += time.perf_counter() - t0
    if im is not None: ok += 1; parts = im
    time.sleep(0.05)
whole = None
for _ in range(20):
    whole = wc.shared.read(hwnd, None)
    if whole is not None: break
    time.sleep(0.05)
W, H = whole.size
def box(r): return (int(r[0] * W), int(r[1] * H), int(r[2] * W), int(r[3] * H))
same = [ImageChops.difference(parts.crop(box(r)), whole.crop(box(r))).getbbox() for r in rects]
black = parts.crop((int(0.05 * W), int(0.9 * H), int(0.25 * W), H)).getbbox() is None
print(f'parts: {ok} frames in 1.5 s, {ms / max(ok, 1) * 1000:.1f} ms per read, size {parts.size}, same as whole {same}, rest black {black}', flush=True)
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
        while started.elapsed().as_millis() < 9000 {
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
