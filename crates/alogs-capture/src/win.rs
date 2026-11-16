//! The game window and our own inputs to it. Port of the window and input half of aLogs B16 `bossbar/wincapture.py`
//! (find_window, client_rect, window_modules, window_copy, hide_from_capture, is_foreground, move/click/wiggle/key/
//! scroll). Windows only; elsewhere these find nothing and do nothing.

use alogs_pil::Image;
use std::sync::Mutex;

/// While aLogs has control of an end screen its focus guard holds the focus in the game's place: that window counts
/// as the game, and our inputs lend the game the focus for a moment (`lend` / `back`).
pub struct Standin {
    pub hwnd: Option<usize>,
    pub lend: Option<Box<dyn Fn() + Send>>,
    pub back: Option<Box<dyn Fn() + Send>>,
}

pub static STANDIN: Mutex<Standin> = Mutex::new(Standin { hwnd: None, lend: None, back: None });

fn game_moment<T>(f: impl FnOnce() -> T) -> T {
    if let Some(l) = STANDIN.lock().unwrap().lend.as_ref() {
        l();
    }
    let r = f();
    if let Some(b) = STANDIN.lock().unwrap().back.as_ref() {
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
                let n = GetWindowTextLengthW(h);
                if n > 0 && IsWindowVisible(h) != 0 {
                    let mut b = vec![0u16; n as usize + 1];
                    GetWindowTextW(h, b.as_mut_ptr(), n + 1);
                    let mut c = [0u16; 64];
                    GetClassNameW(h, c.as_mut_ptr(), 64);
                    let title = String::from_utf16_lossy(&b[..n as usize]);
                    let class = String::from_utf16_lossy(&c[..c.iter().position(|&x| x == 0).unwrap_or(64)]);
                    if title.trim().to_lowercase() == ctx.want && class == "UnityWndClass" {
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

const MOVE: u32 = 0x1;
const LEFTDOWN: u32 = 0x2;
const LEFTUP: u32 = 0x4;
const WHEEL: u32 = 0x0800;
pub const VK_ESCAPE: u16 = 0x1B;

fn sleep(ms: u64) {
    std::thread::sleep(std::time::Duration::from_millis(ms));
}

/// The game is in front (or our focus guard stands in for it).
pub fn is_foreground(hwnd: usize) -> bool {
    let fg = imp::foreground();
    fg == hwnd || (hwnd != 0 && STANDIN.lock().unwrap().hwnd.is_some_and(|s| s == fg))
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

/// Turn the mouse wheel `notches` at a point (negative: down), the cursor put back after (unless `keep`).
pub fn scroll_at(x: i32, y: i32, notches: i32, keep: bool) {
    game_moment(|| {
        let (px, py) = imp::cursor();
        move_to(x, y);
        imp::mouse(MOVE, 6, 0, 0);
        sleep(20);
        imp::mouse(MOVE, -6, 0, 0);
        sleep(30);
        for _ in 0..notches.unsigned_abs() {
            imp::mouse(WHEEL, 0, 0, if notches > 0 { 120 } else { -120 });
            sleep(30);
        }
        if !keep {
            imp::set_cursor(px, py);
        }
    });
}
