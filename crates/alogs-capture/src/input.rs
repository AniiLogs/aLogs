//! Holding the player's inputs for a moment, and the controller: port of aLogs B16 `bossbar/inputblock.py` and
//! `controller.py`.
//!
//! - InputBlock: briefly holds back keyboard keys and mouse buttons (Windows' low-level hooks, no admin, nothing
//!   installed). Mouse movement still works except around one of our clicks (freeze_moves); our own clicks (marked
//!   injected) pass; it removes itself after `seconds` whatever happens; Ctrl+Shift+F12 or Ctrl+Alt+F12 releases it.
//! - hide_controllers: HidHide's driver hides the gamepads on its block list (installed by the desktop app).
//! - pad_buttons / PressWatch: reading presses (XInput, and the keyboard's F), nothing blocked.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

// The hooks' state: one hold at a time (the hook procedures get no user data).
static STOP_AT: AtomicU64 = AtomicU64::new(0);
static MOVES_UNTIL: AtomicU64 = AtomicU64::new(0);
static SWALLOWED: AtomicU64 = AtomicU64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
pub struct InputBlock;

impl InputBlock {
    pub fn active(&self) -> bool {
        RUNNING.load(Ordering::SeqCst)
    }

    /// Hold for `seconds` from now (already holding: for this long from now).
    pub fn start(&self, seconds: f64) {
        STOP_AT.store(now_ms() + (seconds * 1000.0) as u64, Ordering::SeqCst);
        if RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        SWALLOWED.store(0, Ordering::SeqCst);
        std::thread::spawn(|| {
            imp::run();
            RUNNING.store(false, Ordering::SeqCst);
        });
    }

    pub fn stop(&self) {
        STOP_AT.store(0, Ordering::SeqCst);
    }

    /// Hold back the player's mouse movement too for `seconds` (while the hold is on): around our own click.
    pub fn freeze_moves(&self, seconds: f64) {
        MOVES_UNTIL.store(now_ms() + (seconds * 1000.0) as u64, Ordering::SeqCst);
    }
}

/// Is HidHide's driver there?
pub fn hidhide_available() -> bool {
    imp::hidhide(None)
}

static HIDDEN: AtomicBool = AtomicBool::new(false);

/// Are the controllers hidden now (by us)?
pub fn controllers_hidden() -> bool {
    HIDDEN.load(Ordering::SeqCst)
}

/// Turn HidHide's hiding on (the block list's controllers disappear for apps) or off.
pub fn hide_controllers(on: bool) -> bool {
    let ok = imp::hidhide(Some(on));
    HIDDEN.store(on && ok, Ordering::SeqCst);
    log::info!(target: "meter", "controller {}{}", if on { "hidden" } else { "visible again" }, if ok { String::new() } else { format!(" - HidHide error {}", std::io::Error::last_os_error().raw_os_error().unwrap_or(0)) });
    ok
}

pub const XINPUT_Y: u16 = 0x8000;

/// The buttons held on any connected controller (XInput bits), 0 without one.
pub fn pad_buttons() -> u16 {
    imp::pad_buttons()
}

/// Watches, 100 times a second, for presses of the controller's Y or the keyboard's F while on: `last` is the newest
/// (what, when). Reads only; the press still goes to the game.
#[derive(Clone, Default)]
pub struct PressWatch {
    on: Arc<AtomicBool>,
    last: Arc<std::sync::Mutex<Option<(&'static str, f64)>>>,
}

impl PressWatch {
    pub fn last(&self) -> Option<(&'static str, f64)> {
        *self.last.lock().unwrap()
    }

    pub fn start(&self) {
        if self.on.swap(true, Ordering::SeqCst) {
            return;
        }
        let (on, last) = (self.on.clone(), self.last.clone());
        std::thread::spawn(move || {
            let mut was_y = pad_buttons() & XINPUT_Y != 0;
            let mut was_f = imp::key_down(0x46);
            while on.load(Ordering::SeqCst) {
                let y = pad_buttons() & XINPUT_Y != 0;
                let f = imp::key_down(0x46);
                let t = now_ms() as f64 / 1000.0;
                if y && !was_y {
                    *last.lock().unwrap() = Some(("Y", t));
                } else if f && !was_f {
                    *last.lock().unwrap() = Some(("F", t));
                }
                (was_y, was_f) = (y, f);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
    }

    pub fn stop(&self) {
        self.on.store(false, Ordering::SeqCst);
    }
}

#[cfg(windows)]
mod imp {
    use super::{MOVES_UNTIL, STOP_AT, SWALLOWED, now_ms};
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, INVALID_HANDLE_VALUE, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows_sys::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const VK_SHIFT: u32 = 0x10;
    const VK_CONTROL: u32 = 0x11;
    const VK_F12: u32 = 0x7B;
    const KEY_UPS: [usize; 2] = [0x101, 0x105];
    const BUTTON_UPS: [usize; 4] = [0x202, 0x205, 0x208, 0x20C];
    const IOCTL_SET_ACTIVE: u32 = (32769 << 16) | (1 << 14) | (2053 << 2);

    pub fn key_down(vk: i32) -> bool {
        unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
    }

    unsafe extern "system" fn kb(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code == 0 && !KEY_UPS.contains(&wp) {
            let k = unsafe { &*(lp as *const KBDLLHOOKSTRUCT) };
            if k.vkCode == VK_F12 && key_down(VK_CONTROL as i32) && (key_down(VK_SHIFT as i32) || k.flags & LLKHF_ALTDOWN != 0) {
                STOP_AT.store(0, Ordering::SeqCst);
                return 1;
            }
            if k.flags & LLKHF_INJECTED == 0 && ![VK_SHIFT, VK_CONTROL, 0xA0, 0xA1, 0xA2, 0xA3].contains(&k.vkCode) {
                SWALLOWED.fetch_add(1, Ordering::SeqCst);
                return 1;
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wp, lp) }
    }

    unsafe extern "system" fn ms(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code == 0 && !BUTTON_UPS.contains(&wp) && (wp != WM_MOUSEMOVE as usize || now_ms() < MOVES_UNTIL.load(Ordering::SeqCst)) {
            let m = unsafe { &*(lp as *const MSLLHOOKSTRUCT) };
            if m.flags & LLMHF_INJECTED == 0 {
                if wp != WM_MOUSEMOVE as usize {
                    SWALLOWED.fetch_add(1, Ordering::SeqCst);
                }
                return 1;
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wp, lp) }
    }

    pub fn run() {
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let hk = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb), module, 0);
            let hm = SetWindowsHookExW(WH_MOUSE_LL, Some(ms), module, 0);
            let err = if hk.is_null() || hm.is_null() { std::io::Error::last_os_error().raw_os_error().unwrap_or(0) } else { 0 };
            log::info!(target: "meter", "input hold on (keyboard hook {}, mouse hook {}{})", if hk.is_null() { "FAILED" } else { "ok" },
                if hm.is_null() { "FAILED" } else { "ok" }, if err != 0 { format!(", error {err}") } else { String::new() });
            let mut msg: MSG = std::mem::zeroed();
            while now_ms() < STOP_AT.load(Ordering::SeqCst) {
                MsgWaitForMultipleObjects(0, std::ptr::null(), 0, 20, QS_ALLINPUT);
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {}
            }
            if !hk.is_null() {
                UnhookWindowsHookEx(hk);
            }
            if !hm.is_null() {
                UnhookWindowsHookEx(hm);
            }
        }
        log::info!(target: "meter", "input hold off ({} presses held back)", SWALLOWED.load(Ordering::SeqCst));
    }

    /// None: is the driver there; Some(on): set hiding.
    pub fn hidhide(set: Option<bool>) -> bool {
        let name: Vec<u16> = r"\\.\HidHide".encode_utf16().chain([0]).collect();
        let h = unsafe { CreateFileW(name.as_ptr(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut()) };
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            return false;
        }
        let ok = match set {
            None => true,
            Some(on) => {
                let flag: u8 = u8::from(on);
                let mut got = 0u32;
                unsafe { DeviceIoControl(h, IOCTL_SET_ACTIVE, (&raw const flag).cast(), 1, std::ptr::null_mut(), 0, &mut got, std::ptr::null_mut()) != 0 }
            }
        };
        unsafe { CloseHandle(h) };
        ok
    }

    pub fn pad_buttons() -> u16 {
        let mut held = 0u16;
        for i in 0..4 {
            let mut st: XINPUT_STATE = unsafe { std::mem::zeroed() };
            if unsafe { XInputGetState(i, &mut st) } == 0 {
                held |= st.Gamepad.wButtons as u16;
            }
        }
        held
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn key_down(_: i32) -> bool {
        false
    }
    pub fn run() {}
    pub fn hidhide(_: Option<bool>) -> bool {
        false
    }
    pub fn pad_buttons() -> u16 {
        0
    }
}
