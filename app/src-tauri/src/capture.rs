//! Shared game capture: one copy of the Aniimo window for every aLogs module.
//!
//! The app copies the game's client area with PrintWindow (the compositor's own image of the
//! window, so overlays on top of it are never in the frame and there is no capture border) into a
//! named shared-memory block. The meter, Sanctum Helper and anything later read the latest frame
//! from there instead of each copying the window themselves.
//!
//! Layout of `Local\aLogs.Capture.v1` (little-endian), see `docs/shared-capture.md`:
//!   0  u32 magic "ALCP"      4  u32 version (1)
//!   8  u64 seq: odd while a frame is being written, even when it is complete
//!   16 u32 width   20 u32 height   24 u32 stride (bytes per row, width * 4)
//!   28 u32 flags (bit 0: the frame is a valid image of the game)
//!   32 u64 frame time (unix ms)   40 u64 game window handle
//!   48 i32 client x   52 i32 client y (screen position of the client area)
//!   56 u32 capture count (wraps)   60 u32 reserved
//!   64 request slots: SLOTS x { u64 wanted until (unix ms), u32 interval ms, u32 process id }
//!   4096 pixels: BGRA, top row first, up to MAX_W x MAX_H
//!
//! A reader claims a slot (its own pid, or an expired one), keeps "wanted until" a couple of
//! seconds ahead while it needs frames, and copies rows between two equal even `seq` reads.
//! Nothing is captured while no slot is active.

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{fence, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::System::Memory::{CreateFileMappingW, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetClientRect, GetWindowTextW, IsIconic, IsWindow, IsWindowVisible,
};

pub const NAME: &str = "Local\\aLogs.Capture.v1";
const MAGIC: u32 = u32::from_le_bytes(*b"ALCP");
const VERSION: u32 = 1;
const HEADER: usize = 4096;
const SLOTS: usize = 16;
const SLOT_BASE: usize = 64;
const MAX_W: usize = 3840;
const MAX_H: usize = 2160;
const SIZE: usize = HEADER + MAX_W * MAX_H * 4;
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

    fn seq(&self) -> &AtomicU64 {
        unsafe { &*(self.base.add(8) as *const AtomicU64) }
    }
    fn put_u32(&self, at: usize, value: u32) {
        unsafe { (self.base.add(at) as *mut u32).write_unaligned(value) }
    }
    fn put_u64(&self, at: usize, value: u64) {
        unsafe { (self.base.add(at) as *mut u64).write_unaligned(value) }
    }
    fn get_u32(&self, at: usize) -> u32 {
        unsafe { (self.base.add(at) as *const u32).read_volatile() }
    }
    fn get_u64(&self, at: usize) -> u64 {
        unsafe { (self.base.add(at) as *const u64).read_volatile() }
    }

    /// The shortest interval any reader currently wants, or None when nobody needs frames.
    fn wanted_interval(&self, now: u64) -> Option<u64> {
        (0..SLOTS)
            .filter_map(|i| {
                let at = SLOT_BASE + i * 16;
                (self.get_u64(at) > now).then(|| self.get_u32(at + 8).clamp(16, 2000) as u64)
            })
            .min()
    }

    fn pixels(&self) -> *mut c_void {
        unsafe { self.base.add(HEADER) as *mut c_void }
    }
}

// The view is only touched from the capture thread.
unsafe impl Send for Shared {}

fn wide(buf: &[u16]) -> String {
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())])
}

struct Search {
    title: String,
    found: HWND,
}

unsafe extern "system" fn each_window(hwnd: HWND, lparam: LPARAM) -> i32 {
    let search = &mut *(lparam as *mut Search);
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let mut buf = [0u16; 256];
    GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if !wide(&buf).to_lowercase().contains(&search.title) {
        return 1;
    }
    // The game is a Unity player; this keeps browser tabs and folders named "Aniimo" out.
    GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if wide(&buf) != "UnityWndClass" {
        return 1;
    }
    search.found = hwnd;
    0
}

pub(crate) fn find_game(title: &str) -> HWND {
    let mut search = Search { title: title.to_lowercase(), found: null_mut() };
    unsafe { EnumWindows(Some(each_window), &mut search as *mut Search as LPARAM) };
    search.found
}

/// A memory DC and bitmap kept between frames, rebuilt when the game's size changes.
struct Canvas {
    dc: HDC,
    bitmap: HBITMAP,
    old: *mut c_void,
    width: i32,
    height: i32,
}

impl Canvas {
    fn new(width: i32, height: i32) -> Option<Canvas> {
        unsafe {
            let screen = GetDC(null_mut());
            let dc = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            ReleaseDC(null_mut(), screen);
            if dc.is_null() || bitmap.is_null() {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                if !dc.is_null() {
                    DeleteDC(dc);
                }
                return None;
            }
            let old = SelectObject(dc, bitmap);
            Some(Canvas { dc, bitmap, old, width, height })
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

/// One frame into shared memory. Returns false when the window gave no usable image.
fn capture(shared: &Shared, canvas: &mut Option<Canvas>, hwnd: HWND) -> bool {
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut rect) == 0 {
            return false;
        }
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        if width < 100 || height < 100 || width as usize > MAX_W || height as usize > MAX_H {
            return false;
        }
        if canvas.as_ref().map_or(true, |c| c.width != width || c.height != height) {
            *canvas = Canvas::new(width, height);
        }
        let Some(canvas) = canvas.as_ref() else { return false };
        if PrintWindow(hwnd, canvas.dc, PW_CLIENTONLY | PW_RENDERFULLCONTENT) == 0 {
            return false;
        }
        let mut origin = POINT { x: 0, y: 0 };
        ClientToScreen(hwnd, &mut origin);

        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // top row first
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };

        // Seqlock: odd while writing, so readers retry instead of taking a torn frame.
        let seq = shared.seq();
        let start = seq.load(Ordering::Relaxed) | 1;
        seq.store(start, Ordering::Relaxed);
        fence(Ordering::Release);
        // The bitmap must not be selected into a DC for GetDIBits.
        SelectObject(canvas.dc, canvas.old);
        let lines = GetDIBits(canvas.dc, canvas.bitmap, 0, height as u32, shared.pixels(), &mut info, DIB_RGB_COLORS);
        SelectObject(canvas.dc, canvas.bitmap);
        // A window that can't be printed comes back black; check a spread of pixels.
        let words = shared.pixels() as *const u32;
        let count = (width * height) as usize;
        let painted = lines == height && (0..count).step_by(997).any(|i| words.add(i).read() & 0x00ff_ffff != 0);
        shared.put_u32(16, width as u32);
        shared.put_u32(20, height as u32);
        shared.put_u32(24, width as u32 * 4);
        shared.put_u32(28, painted as u32);
        shared.put_u64(32, now_ms());
        shared.put_u64(40, hwnd as u64);
        shared.put_u32(48, origin.x as u32);
        shared.put_u32(52, origin.y as u32);
        shared.put_u32(56, shared.get_u32(56).wrapping_add(1));
        fence(Ordering::Release);
        seq.store(start + 1, Ordering::Release);
        painted
    }
}

fn mark_invalid(shared: &Shared) {
    let seq = shared.seq();
    let start = seq.load(Ordering::Relaxed) | 1;
    seq.store(start, Ordering::Relaxed);
    shared.put_u32(28, 0);
    shared.put_u64(32, now_ms());
    seq.store(start + 1, Ordering::Release);
}

/// Starts the capture thread. `title` is the game window's title (matched case-insensitively).
pub fn start(title: String) {
    std::thread::Builder::new()
        .name("shared-capture".into())
        .spawn(move || {
            // Real pixels on scaled displays (150% etc.), the same frame size the game renders.
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
            let Some(shared) = Shared::create() else {
                eprintln!("shared capture: could not create {NAME}");
                return;
            };
            let mut canvas: Option<Canvas> = None;
            let mut hwnd: HWND = null_mut();
            let mut last_find = 0u64;
            let mut last_frame = 0u64;
            loop {
                let now = now_ms();
                let Some(interval) = shared.wanted_interval(now) else {
                    canvas = None; // free the bitmap while idle
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                };
                if now < last_frame + interval {
                    std::thread::sleep(Duration::from_millis((last_frame + interval - now).min(10)));
                    continue;
                }
                last_frame = now;
                let alive = unsafe { !hwnd.is_null() && IsWindow(hwnd) != 0 };
                if !alive && now > last_find + 1000 {
                    hwnd = find_game(&title);
                    last_find = now;
                }
                let usable = unsafe { !hwnd.is_null() && IsWindow(hwnd) != 0 && IsIconic(hwnd) == 0 };
                if !usable || !capture(&shared, &mut canvas, hwnd) {
                    mark_invalid(&shared);
                    if !usable {
                        hwnd = null_mut();
                    }
                }
            }
        })
        .ok();
}
