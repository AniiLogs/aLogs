//! The game window and our own inputs to it. Port of the window and input half of aLogs B16 `bossbar/wincapture.py`
//! (find_window, client_rect, window_modules, window_copy, hide_from_capture, is_foreground, move/click/wiggle/key/
//! scroll). Windows only; elsewhere these find nothing and do nothing.

use alogs_pil::Image;
use std::sync::{Arc, Mutex};

/// While aLogs has control of an end screen the game is told it is inactive (`inactive.rs`): our own inputs tell it
/// it is active for their moment (`lend` before, `back` after).
pub struct Lend {
    pub lend: Option<Arc<dyn Fn() + Send + Sync>>,
    pub back: Option<Arc<dyn Fn() + Send + Sync>>,
}

pub static LEND: Mutex<Lend> = Mutex::new(Lend { lend: None, back: None });

fn game_moment<T>(f: impl FnOnce() -> T) -> T {
    // each is taken out of the lock before it's called: it waits, and the hold may end meanwhile
    let lend = LEND.lock().unwrap().lend.clone();
    if let Some(l) = lend {
        l();
    }
    let r = f();
    let back = LEND.lock().unwrap().back.clone();
    if let Some(b) = back {
        b();
    }
    r
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::Storage::Xps::PrintWindow;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    pub fn find_window(title: &str) -> Option<usize> {
        let want = {
            let t = title.trim().to_lowercase();
            if t.is_empty() { "aniimo".to_string() } else { t }
        };
        struct Ctx {
            want: String,
            best: (i64, usize),
        }
        unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> i32 {
            unsafe {
                let ctx = &mut *(l as *mut Ctx);
                // The class first: it is read without asking the window. A title is asked of the window's own thread
                // when the window is this program's, and waits for it: the meter's overlay windows answered once a
                // loop of theirs, 16 asks a search. Only a visible game-engine window is asked for its title.
                let mut c = [0u16; 64];
                if IsWindowVisible(h) == 0 || GetClassNameW(h, c.as_mut_ptr(), 64) == 0 {
                    return 1;
                }
                let class = String::from_utf16_lossy(&c[..c.iter().position(|&x| x == 0).unwrap_or(64)]);
                if class != "UnityWndClass" {
                    return 1;
                }
                let n = GetWindowTextLengthW(h);
                if n > 0 {
                    let mut b = vec![0u16; n as usize + 1];
                    GetWindowTextW(h, b.as_mut_ptr(), n + 1);
                    let title = String::from_utf16_lossy(&b[..n as usize]);
                    if title.trim().to_lowercase() == ctx.want {
                        let mut r: RECT = std::mem::zeroed();
                        GetClientRect(h, &mut r);
                        let area = i64::from(r.right) * i64::from(r.bottom);
                        if area > ctx.best.0 {
                            ctx.best = (area, h as usize);
                        }
                    }
                }
                1
            }
        }
        let mut ctx = Ctx { want, best: (0, 0) };
        unsafe { EnumWindows(Some(cb), &mut ctx as *mut Ctx as LPARAM) };
        (ctx.best.1 != 0).then_some(ctx.best.1)
    }

    pub fn client_rect(hwnd: usize) -> (i32, i32, i32, i32) {
        unsafe {
            let mut r: RECT = std::mem::zeroed();
            GetClientRect(hwnd as HWND, &mut r);
            let mut p = POINT { x: 0, y: 0 };
            ClientToScreen(hwnd as HWND, &mut p);
            (p.x, p.y, r.right, r.bottom)
        }
    }

    /// Is this still a window?
    pub fn is_window(hwnd: usize) -> bool {
        unsafe { IsWindow(hwnd as HWND) != 0 }
    }

    /// The process id of the window's program, or 0.
    pub fn window_pid(hwnd: usize) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd as HWND, &mut pid) };
        pid
    }

    pub fn window_modules(hwnd: usize) -> std::collections::BTreeSet<String> {
        use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
        let mut out = std::collections::BTreeSet::new();
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd as HWND, &mut pid);
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
            if snap.is_null() || snap == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                return out;
            }
            let mut e: MODULEENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
            let mut ok = Module32FirstW(snap, &mut e);
            while ok != 0 {
                let n = e.szModule.iter().position(|&c| c == 0).unwrap_or(e.szModule.len());
                out.insert(String::from_utf16_lossy(&e.szModule[..n]).to_lowercase());
                ok = Module32NextW(snap, &mut e);
            }
            windows_sys::Win32::Foundation::CloseHandle(snap);
        }
        out
    }

    pub fn window_copy(hwnd: usize) -> Option<Image> {
        unsafe {
            let mut r: RECT = std::mem::zeroed();
            if GetClientRect(hwnd as HWND, &mut r) == 0 || r.right < 16 || r.bottom < 16 {
                return None;
            }
            let (w, h) = (r.right, r.bottom);
            let screen = GetDC(std::ptr::null_mut());
            let mem = CreateCompatibleDC(screen);
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp as _);
            let mut out = None;
            if PrintWindow(hwnd as HWND, mem, 3) != 0 {
                let mut buf = vec![0u8; (w * h * 4) as usize];
                GetBitmapBits(bmp, buf.len() as i32, buf.as_mut_ptr() as _);
                let mut data = Vec::with_capacity((w * h * 3) as usize);
                for p in buf.chunks_exact(4) {
                    data.extend_from_slice(&[p[2], p[1], p[0]]);
                }
                out = Some(Image::from_raw(alogs_pil::Mode::Rgb, w as usize, h as usize, data));
            }
            SelectObject(mem, old);
            DeleteObject(bmp as _);
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
            out
        }
    }

    pub fn hide_from_capture(hwnd: usize, on: bool) -> bool {
        unsafe { SetWindowDisplayAffinity(hwnd as HWND, if on { 0x11 } else { 0 }) != 0 }
    }

    pub fn foreground() -> usize {
        unsafe { GetForegroundWindow() as usize }
    }

    pub fn move_to(x: i32, y: i32) {
        unsafe {
            let (vx, vy, vw, vh) = (GetSystemMetrics(76), GetSystemMetrics(77), GetSystemMetrics(78), GetSystemMetrics(79));
            let nx = ((f64::from(x - vx) * 65535.0 / f64::from((vw - 1).max(1))).round_ties_even()) as i32;
            let ny = ((f64::from(y - vy) * 65535.0 / f64::from((vh - 1).max(1))).round_ties_even()) as i32;
            mouse_event(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, nx, ny, 0, 0);
            SetCursorPos(x, y);
        }
    }

    pub fn cursor() -> (i32, i32) {
        unsafe {
            let mut p = POINT { x: 0, y: 0 };
            GetCursorPos(&mut p);
            (p.x, p.y)
        }
    }
    pub fn set_cursor(x: i32, y: i32) {
        unsafe { SetCursorPos(x, y) };
    }
    pub fn mouse(flags: u32, dx: i32, dy: i32, data: i32) {
        unsafe { mouse_event(flags, dx, dy, data, 0) };
    }
    pub fn key(vk: u16) {
        unsafe {
            let scan = MapVirtualKeyW(u32::from(vk), 0) as u8;
            keybd_event(vk as u8, scan, 0, 0);
            std::thread::sleep(std::time::Duration::from_millis(60));
            keybd_event(vk as u8, scan, KEYEVENTF_KEYUP, 0);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn find_window(_: &str) -> Option<usize> {
        None
    }
    pub fn client_rect(_: usize) -> (i32, i32, i32, i32) {
        (0, 0, 0, 0)
    }
    pub fn window_pid(_: usize) -> u32 {
        0
    }
    pub fn is_window(_: usize) -> bool {
        false
    }
    pub fn window_modules(_: usize) -> std::collections::BTreeSet<String> {
        Default::default()
    }
    pub fn window_copy(_: usize) -> Option<Image> {
        None
    }
    pub fn hide_from_capture(_: usize, _: bool) -> bool {
        false
    }
    pub fn foreground() -> usize {
        0
    }
    pub fn move_to(_: i32, _: i32) {}
    pub fn cursor() -> (i32, i32) {
        (0, 0)
    }
    pub fn set_cursor(_: i32, _: i32) {}
    pub fn mouse(_: u32, _: i32, _: i32, _: i32) {}
    pub fn key(_: u16) {}
}

pub use imp::{client_rect, find_window, hide_from_capture, move_to, window_copy, window_modules, window_pid, is_window};

/// The window in front now.
pub fn imp_foreground() -> usize {
    imp::foreground()
}

const MOVE: u32 = 0x1;
const LEFTDOWN: u32 = 0x2;
const LEFTUP: u32 = 0x4;
const WHEEL: u32 = 0x0800;
pub const VK_ESCAPE: u16 = 0x1B;

fn sleep(ms: u64) {
    std::thread::sleep(std::time::Duration::from_millis(ms));
}

/// The game is the window in front.
pub fn is_foreground(hwnd: usize) -> bool {
    imp::foreground() == hwnd
}

/// One left click at a screen point, the cursor put back after (unless `keep`).
pub fn click_at(x: i32, y: i32, keep: bool) {
    game_moment(|| {
        let (px, py) = imp::cursor();
        move_to(x, y);
        sleep(120);
        move_to(x, y);
        imp::mouse(LEFTDOWN, 0, 0, 0);
        sleep(50);
        move_to(x, y);
        imp::mouse(LEFTUP, 0, 0, 0);
        sleep(30);
        if !keep {
            imp::set_cursor(px, py);
        }
    });
}

/// A little real mouse movement around a point (switches a game in controller mode to mouse mode).
pub fn wiggle_at(x: i32, y: i32) {
    game_moment(|| {
        move_to(x, y);
        sleep(30);
        for dx in [2, -2] {
            imp::mouse(MOVE, dx, 0, 0);
            sleep(30);
        }
        move_to(x, y);
    });
}

/// One key press (with its scan code) to the window in front.
pub fn press_key(vk: u16) {
    game_moment(|| imp::key(vk));
}

/// Several of our own inputs in one moment: the game is told it is active once for all of them, not once each (each
/// time costs LEND_SETTLE + LEND_AFTER, 0.27 s, while the game is told it is inactive).
pub fn one_moment<T>(f: impl FnOnce() -> T) -> T {
    game_moment(f)
}

/// Where the pointer is now, in screen px.
pub fn pointer() -> (i32, i32) {
    imp::cursor()
}

/// The pointer is this far (px) from a point at most: it counts as on it.
const ON_POINT: i32 = 8;
/// A pointer that had to travel is given this long (ms) on its point before a press or the wheel, as a click's is.
const ARRIVE_MS: u64 = 120;

/// Is the pointer on (x, y) already?
fn pointer_on(x: i32, y: i32) -> bool {
    let (px, py) = imp::cursor();
    (px - x).abs() <= ON_POINT && (py - y).abs() <= ON_POINT
}

/// Turn the mouse wheel `notches` at a point (negative: down), the cursor put back after (unless `keep`).
///
/// The wheel turns what the game's pointer is over, and the game takes the pointer's place from mouse movement it has
/// had time to see: a pointer that comes from elsewhere is given ARRIVE_MS there first, as a click's is, and is put on
/// the point again before each notch. (17 Nov: with the pointer still on "All" from the click before, the Gizmo
/// screen's wheel turned the filter column 50 ms after the move, the list never moved, and the run went out with 7 of
/// its 11 gizmos.)
pub fn scroll_at(x: i32, y: i32, notches: i32, keep: bool) {
    game_moment(|| {
        let (px, py) = imp::cursor();
        let there = pointer_on(x, y);
        move_to(x, y);
        imp::mouse(MOVE, 6, 0, 0);
        sleep(20);
        imp::mouse(MOVE, -6, 0, 0);
        sleep(30);
        if !there {
            move_to(x, y);
            sleep(ARRIVE_MS);
        }
        for _ in 0..notches.unsigned_abs() {
            move_to(x, y);
            imp::mouse(WHEEL, 0, 0, if notches > 0 { 120 } else { -120 });
            sleep(30);
        }
        if !keep {
            imp::set_cursor(px, py);
        }
    });
}

/// Handles to wait on for "stop": the app's stop event (ALOGS_STOP_EVENT) and the app's own process (ALOGS_PARENT).
pub struct StopHandles(Vec<(&'static str, usize)>);

impl StopHandles {
    pub fn open() -> StopHandles {
        StopHandles(stop_imp::open())
    }

    /// "event" (the app asked) or "parent" (the app is gone), once signalled.
    pub fn signalled(&self) -> Option<&'static str> {
        self.0.iter().find(|(_, h)| stop_imp::signalled(*h)).map(|(k, _)| *k)
    }
}

#[cfg(windows)]
mod stop_imp {
    use windows_sys::Win32::System::Threading::{OpenEventW, OpenProcess, WaitForSingleObject};
    const SYNCHRONIZE: u32 = 0x0010_0000;

    pub fn open() -> Vec<(&'static str, usize)> {
        let mut out = vec![];
        if let Ok(name) = std::env::var("ALOGS_STOP_EVENT")
            && !name.is_empty()
        {
            let w: Vec<u16> = name.encode_utf16().chain([0]).collect();
            let h = unsafe { OpenEventW(SYNCHRONIZE, 0, w.as_ptr()) };
            if !h.is_null() {
                out.push(("event", h as usize));
            }
        }
        if let Some(parent) = std::env::var("ALOGS_PARENT").ok().and_then(|v| v.parse::<u32>().ok()).filter(|p| *p != 0) {
            let h = unsafe { OpenProcess(SYNCHRONIZE, 0, parent) };
            if !h.is_null() {
                out.push(("parent", h as usize));
            }
        }
        out
    }

    pub fn signalled(h: usize) -> bool {
        unsafe { WaitForSingleObject(h as windows_sys::Win32::Foundation::HANDLE, 0) == 0 }
    }
}

#[cfg(not(windows))]
mod stop_imp {
    pub fn open() -> Vec<(&'static str, usize)> {
        vec![]
    }
    pub fn signalled(_: usize) -> bool {
        false
    }
}

#[cfg(windows)]
#[link(name = "shcore")]
unsafe extern "system" {
    fn SetProcessDpiAwareness(value: i32) -> i32;
}

/// Real pixels on scaled displays, for the whole program: call first thing in `main`, before any window exists.
/// B16's `wincapture.py` did this on import (`SetProcessDpiAwareness(2)`). Without it Windows treats the program as
/// unaware of display scaling: on a 4K screen at 150% it reports a 2560x1440 desktop and stretches every window we
/// draw by 1.5, while the pictures of the game we size them from are in real pixels, so overlays came out too big
/// and off the screen.
pub fn real_pixels() {
    #[cfg(windows)]
    unsafe {
        SetProcessDpiAwareness(2); // per monitor; fails harmlessly if already set
    }
}
