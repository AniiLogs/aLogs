//! Keeps the controller away from the game while aLogs has control of an end screen (built-in Windows, no admin):
//! an invisible window of aLogs' own takes the focus from the game, which reads no controller while it isn't the
//! active window. Our own clicks lend the game the focus for that moment (`lend` / `back`); the window gives the
//! focus back and closes itself after `seconds`, whatever happens. Port of aLogs B16 `bossbar/focusguard.py`.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub const LEND_SETTLE: f64 = 0.15;
pub const LEND_AFTER: f64 = 0.12;

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[derive(Default)]
struct Inner {
    hwnd: AtomicUsize,
    game: AtomicUsize,
    stop_at: AtomicU64,
    lent: Mutex<u32>,
    running: std::sync::atomic::AtomicBool,
}

#[derive(Clone, Default)]
pub struct FocusGuard(Arc<Inner>);

impl FocusGuard {
    pub fn active(&self) -> bool {
        self.0.running.load(Ordering::SeqCst) && self.0.hwnd.load(Ordering::SeqCst) != 0
    }

    /// Take the focus from `game` for at most `seconds` (again from now when it's on). False when Windows wouldn't
    /// give it (then the game keeps the focus).
    pub fn take(&self, game: usize, seconds: f64) -> bool {
        let i = &self.0;
        i.stop_at.store(now_ms() + (seconds * 1000.0) as u64, Ordering::SeqCst);
        if self.active() {
            return true;
        }
        i.game.store(game, Ordering::SeqCst);
        i.hwnd.store(0, Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::channel();
        let me = self.clone();
        i.running.store(true, Ordering::SeqCst);
        std::thread::spawn(move || {
            imp::run(&me, tx);
            me.0.running.store(false, Ordering::SeqCst);
        });
        if rx.recv_timeout(std::time::Duration::from_secs(2)).is_err() || i.hwnd.load(Ordering::SeqCst) == 0 {
            log::info!(target: "meter", "focus guard: its window could not be made");
            i.stop_at.store(0, Ordering::SeqCst);
            return false;
        }
        let h = i.hwnd.load(Ordering::SeqCst);
        let ok = {
            let _l = i.lent.lock().unwrap();
            imp::bring_to_front(h, 3)
        };
        if !ok {
            log::info!(target: "meter", "focus guard: Windows did not give it the focus; the controller still reaches the game");
            i.stop_at.store(0, Ordering::SeqCst);
            return false;
        }
        let (a, b) = (self.clone(), self.clone());
        let mut s = crate::win::STANDIN.lock().unwrap();
        s.hwnd = Some(h);
        s.lend = Some(Box::new(move || a.lend()));
        s.back = Some(Box::new(move || b.back()));
        drop(s);
        log::info!(target: "meter", "focus guard on: the game is in the background, so it reads no controller");
        true
    }

    pub fn stop(&self) {
        self.0.stop_at.store(0, Ordering::SeqCst);
    }

    /// The game in front for one of our own clicks; `back` takes the focus again.
    pub fn lend(&self) {
        let mut lent = self.0.lent.lock().unwrap();
        if !self.active() {
            return;
        }
        *lent += 1;
        if *lent == 1 && imp::bring_to_front(self.0.game.load(Ordering::SeqCst), 3) {
            std::thread::sleep(std::time::Duration::from_secs_f64(LEND_SETTLE));
        }
    }

    pub fn back(&self) {
        let mut lent = self.0.lent.lock().unwrap();
        if *lent == 0 {
            return;
        }
        *lent -= 1;
        if *lent == 0 && self.active() && now_ms() < self.0.stop_at.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_secs_f64(LEND_AFTER));
            imp::bring_to_front(self.0.hwnd.load(Ordering::SeqCst), 3);
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::{FocusGuard, now_ms};
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    unsafe extern "system" fn proc_(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    /// Make `hwnd` the active window, joining the input of the window in front for the moment of the call.
    pub fn bring_to_front(hwnd: usize, tries: u32) -> bool {
        let h = hwnd as HWND;
        for _ in 0..tries {
            unsafe {
                let fg = GetForegroundWindow();
                if fg == h {
                    return true;
                }
                let me = GetCurrentThreadId();
                let other = if fg.is_null() { 0 } else { GetWindowThreadProcessId(fg, std::ptr::null_mut()) };
                let joined = other != 0 && other != me && AttachThreadInput(me, other, 1) != 0;
                BringWindowToTop(h);
                SetForegroundWindow(h);
                if joined {
                    AttachThreadInput(me, other, 0);
                }
                if GetForegroundWindow() == h {
                    return true;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        unsafe { GetForegroundWindow() == h }
    }

    use windows_sys::Win32::System::Threading::AttachThreadInput;

    pub fn run(g: &FocusGuard, ready: std::sync::mpsc::Sender<()>) {
        let i = &g.0;
        let class: Vec<u16> = "aLogsFocusGuard".encode_utf16().chain([0]).collect();
        let title: Vec<u16> = "aLogs".encode_utf16().chain([0]).collect();
        let h = unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW { style: 0, lpfnWndProc: Some(proc_), cbClsExtra: 0, cbWndExtra: 0, hInstance: module, hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(), hbrBackground: std::ptr::null_mut(), lpszMenuName: std::ptr::null(), lpszClassName: class.as_ptr() };
            RegisterClassW(&wc); // (already registered: fine)
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(i.game.load(Ordering::SeqCst) as HWND, &mut r);
            let h = CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST, class.as_ptr(), title.as_ptr(), WS_POPUP,
                r.left, r.top, 1, 1, std::ptr::null_mut(), std::ptr::null_mut(), module, std::ptr::null());
            if !h.is_null() {
                SetLayeredWindowAttributes(h, 0, 0, LWA_ALPHA);
                crate::win::hide_from_capture(h as usize, true);
                ShowWindow(h, SW_SHOW);
            }
            h
        };
        i.hwnd.store(h as usize, Ordering::SeqCst);
        let _ = ready.send(());
        if h.is_null() {
            return;
        }
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while now_ms() < i.stop_at.load(Ordering::SeqCst) {
                MsgWaitForMultipleObjects(0, std::ptr::null(), 0, 20, QS_ALLINPUT);
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        let was_front = {
            let _l = i.lent.lock().unwrap();
            let mut s = crate::win::STANDIN.lock().unwrap();
            s.hwnd = None;
            s.lend = None;
            s.back = None;
            drop(s);
            let front = unsafe { GetForegroundWindow() } == h;
            if front {
                bring_to_front(i.game.load(Ordering::SeqCst), 3);
            }
            unsafe { DestroyWindow(h) };
            i.hwnd.store(0, Ordering::SeqCst);
            front
        };
        *i.lent.lock().unwrap() = 0;
        log::info!(target: "meter", "{}", if was_front { "focus guard off: the game has the focus again" } else { "focus guard off" });
    }
}

#[cfg(not(windows))]
mod imp {
    use super::FocusGuard;
    pub fn bring_to_front(_: usize, _: u32) -> bool {
        false
    }
    pub fn run(_: &FocusGuard, ready: std::sync::mpsc::Sender<()>) {
        let _ = ready.send(());
    }
}
