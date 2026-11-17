//! The overlay key: a global low-level keyboard (and, while needed, mouse) hook that only watches one key and never
//! swallows input, so the game still gets it; and controller buttons through XInput as a second key. Port of
//! `KeyHook.cs` and `Gamepad.cs`. The hooks run on a thread of their own with the message loop they need (in C# it
//! was the UI thread).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub const VK_MBUTTON: i32 = 0x04;
pub const VK_XBUTTON1: i32 = 0x05;
pub const VK_XBUTTON2: i32 = 0x06;

type StateFn = Box<dyn Fn(bool) + Send>;
type KeyFn = Box<dyn Fn(i32) + Send>;

#[derive(Default)]
struct Shared {
    key: AtomicI32,
    capturing: AtomicBool,
    down: AtomicBool,
    thread: AtomicU32,
    on_state: Mutex<Vec<StateFn>>,
    on_any: Mutex<Vec<KeyFn>>,
}

static HOOK: OnceLock<Arc<Shared>> = OnceLock::new();

fn shared() -> &'static Arc<Shared> {
    HOOK.get_or_init(|| Arc::new(Shared::default()))
}

fn is_mouse(vk: i32) -> bool {
    matches!(vk, VK_MBUTTON | VK_XBUTTON1 | VK_XBUTTON2)
}

/// Left/right variants arrive as their own codes; the generic key accepts them.
pub fn matches(key: i32, vk: i32) -> bool {
    vk == key || (key == 0x10 && matches!(vk, 0xA0 | 0xA1)) || (key == 0x11 && matches!(vk, 0xA2 | 0xA3)) || (key == 0x12 && matches!(vk, 0xA4 | 0xA5))
}

fn set(s: &Shared, value: bool) {
    if s.down.swap(value, Ordering::SeqCst) == value {
        return;
    }
    for f in s.on_state.lock().unwrap().iter() {
        f(value);
    }
}

fn any_pressed(s: &Shared, vk: i32) {
    for f in s.on_any.lock().unwrap().iter() {
        f(vk);
    }
}

/// The overlay key's hook (one per process).
pub struct KeyHook {
    s: Arc<Shared>,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    pub const WM_UPDATE: u32 = WM_APP + 1;

    unsafe extern "system" fn kb_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code >= 0 {
            let k = unsafe { &*(lp as *const KBDLLHOOKSTRUCT) };
            let msg = wp as u32;
            let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
            let vk = k.vkCode as i32;
            let s = shared();
            if is_down {
                any_pressed(s, vk);
            }
            if matches(s.key.load(Ordering::Relaxed), vk) {
                if is_down {
                    set(s, true);
                } else if is_up {
                    set(s, false);
                }
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wp, lp) }
    }

    unsafe extern "system" fn ms_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code >= 0 {
            let m = unsafe { &*(lp as *const MSLLHOOKSTRUCT) };
            let x = if (m.mouseData >> 16) == 1 { VK_XBUTTON1 } else { VK_XBUTTON2 };
            let (vk, is_down) = match wp as u32 {
                WM_MBUTTONDOWN => (VK_MBUTTON, true),
                WM_MBUTTONUP => (VK_MBUTTON, false),
                WM_XBUTTONDOWN => (x, true),
                WM_XBUTTONUP => (x, false),
                _ => (0, false),
            };
            if vk != 0 {
                let s = shared();
                if is_down {
                    any_pressed(s, vk);
                }
                if vk == s.key.load(Ordering::Relaxed) {
                    set(s, is_down);
                }
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wp, lp) }
    }

    /// The hook thread: the keyboard hook always, the mouse hook while a mouse button is the key or a key is being
    /// picked (every mouse move of the PC went through the process all day otherwise).
    pub fn run(s: Arc<Shared>) {
        unsafe {
            s.thread.store(GetCurrentThreadId(), Ordering::SeqCst);
            let module = GetModuleHandleW(std::ptr::null());
            let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), module, 0);
            if kb.is_null() {
                crate::log::write(&format!("keyboard hook failed: {}", std::io::Error::last_os_error()));
            }
            let mut ms: HHOOK = std::ptr::null_mut();
            let update = |ms: &mut HHOOK| {
                let want = is_mouse(s.key.load(Ordering::Relaxed)) || s.capturing.load(Ordering::Relaxed);
                if want && ms.is_null() {
                    *ms = SetWindowsHookExW(WH_MOUSE_LL, Some(ms_proc), module, 0);
                    if ms.is_null() {
                        crate::log::write(&format!("mouse hook failed: {}", std::io::Error::last_os_error()));
                    }
                } else if !want && !ms.is_null() {
                    UnhookWindowsHookEx(*ms);
                    *ms = std::ptr::null_mut();
                }
            };
            update(&mut ms);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message == WM_UPDATE && msg.hwnd.is_null() {
                    update(&mut ms);
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if !ms.is_null() {
                UnhookWindowsHookEx(ms);
            }
            if !kb.is_null() {
                UnhookWindowsHookEx(kb);
            }
        }
    }

    pub fn post(s: &Shared, msg: u32) {
        let t = s.thread.load(Ordering::SeqCst);
        if t != 0 {
            unsafe { PostThreadMessageW(t, msg, 0, 0) };
        }
    }

    pub fn key_held(vk: i32) -> bool {
        (unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk) } as u16 & 0x8000) != 0
    }

    pub const QUIT: u32 = WM_QUIT;
}

impl KeyHook {
    pub fn new(key: i32) -> KeyHook {
        let s = shared().clone();
        s.key.store(key, Ordering::SeqCst);
        #[cfg(windows)]
        {
            let t = s.clone();
            let _ = std::thread::Builder::new().name("SanctumHelper key hook".into()).spawn(move || imp::run(t));
        }
        KeyHook { s }
    }

    pub fn key(&self) -> i32 {
        self.s.key.load(Ordering::SeqCst)
    }

    pub fn set_key(&self, key: i32) {
        self.s.key.store(key, Ordering::SeqCst);
        #[cfg(windows)]
        imp::post(&self.s, imp::WM_UPDATE);
    }

    /// A new key is being picked (a mouse button can be one): the mouse is watched meanwhile.
    pub fn set_capturing(&self, on: bool) {
        self.s.capturing.store(on, Ordering::SeqCst);
        #[cfg(windows)]
        imp::post(&self.s, imp::WM_UPDATE);
    }

    pub fn is_down(&self) -> bool {
        self.s.down.load(Ordering::SeqCst)
    }

    /// Called (on the hook thread) whenever the key goes down or up.
    pub fn on_state_changed(&self, f: impl Fn(bool) + Send + 'static) {
        self.s.on_state.lock().unwrap().push(Box::new(f));
    }

    /// Called (on the hook thread) for any key press: picking a new binding.
    pub fn on_any_key(&self, f: impl Fn(i32) + Send + 'static) {
        self.s.on_any.lock().unwrap().push(Box::new(f));
    }

    /// Safety net for a missed key-up (focus changes while held).
    pub fn poll(&self) {
        #[cfg(windows)]
        if self.is_down() && !imp::key_held(self.key()) {
            set(&self.s, false);
        }
    }
}

impl Drop for KeyHook {
    fn drop(&mut self) {
        #[cfg(windows)]
        imp::post(&self.s, imp::QUIT);
    }
}

/// The key is held down right now (polled; for keys read without a hook, as Enter / Esc while placing).
pub fn key_held(vk: i32) -> bool {
    #[cfg(windows)]
    return imp::key_held(vk);
    #[cfg(not(windows))]
    {
        let _ = vk;
        false
    }
}

/// A key's name as the settings show it.
pub fn key_name(vk: i32) -> String {
    match vk {
        0x14 => "Caps Lock".into(),
        0x09 => "Tab".into(),
        0x20 => "Space".into(),
        0x10 => "Shift".into(),
        0x11 => "Ctrl".into(),
        0x12 => "Alt".into(),
        0xA0 => "Left Shift".into(),
        0xA1 => "Right Shift".into(),
        0xA2 => "Left Ctrl".into(),
        0xA3 => "Right Ctrl".into(),
        0xA4 => "Left Alt".into(),
        0xA5 => "Right Alt".into(),
        0xC0 => "` (backtick)".into(),
        0x04 => "Middle mouse".into(),
        0x05 => "Mouse back (X1)".into(),
        0x06 => "Mouse forward (X2)".into(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x60..=0x69 => format!("Numpad {}", vk - 0x60),
        _ => format!("Key 0x{vk:02X}"),
    }
}

pub const LEFT_TRIGGER: i32 = 0x10000;
pub const RIGHT_TRIGGER: i32 = 0x20000;

const PAD_NAMES: [(i32, &str); 16] = [
    (0x1000, "A"), (0x2000, "B"), (0x4000, "X"), (0x8000, "Y"), (0x0100, "LB"), (0x0200, "RB"), (LEFT_TRIGGER, "LT"),
    (RIGHT_TRIGGER, "RT"), (0x0040, "Left stick click"), (0x0080, "Right stick click"), (0x0020, "View / Back"),
    (0x0010, "Menu / Start"), (0x0001, "D-pad up"), (0x0002, "D-pad down"), (0x0004, "D-pad left"), (0x0008, "D-pad right"),
];

pub fn button_name(button: i32) -> String {
    if button == 0 {
        return "None".into();
    }
    PAD_NAMES.iter().find(|n| n.0 == button).map_or_else(|| format!("Button 0x{button:X}"), |n| n.1.to_string())
}

/// Controller buttons of every connected pad (XInput: Xbox and compatible pads; nothing of the game is touched).
#[derive(Default)]
pub struct Gamepad {
    connected: [bool; 4],
    next_scan: Option<std::time::Instant>,
    /// All buttons held right now on any connected pad (0 when none).
    pub buttons: i32,
}

impl Gamepad {
    /// Reads the pads; empty slots are only re-checked twice a second (asking an empty slot is slow).
    pub fn poll(&mut self) {
        let now = std::time::Instant::now();
        let scan = self.next_scan.is_none_or(|t| now >= t);
        if scan {
            self.next_scan = Some(now + std::time::Duration::from_millis(500));
        }
        let mut all = 0;
        for i in 0..4 {
            if !self.connected[i] && !scan {
                continue;
            }
            match get_state(i as u32) {
                Some((b, lt, rt)) => {
                    self.connected[i] = true;
                    all |= i32::from(b);
                    if lt > 60 {
                        all |= LEFT_TRIGGER;
                    }
                    if rt > 60 {
                        all |= RIGHT_TRIGGER;
                    }
                }
                None => self.connected[i] = false,
            }
        }
        self.buttons = all;
    }

    pub fn is_down(&self, button: i32) -> bool {
        button != 0 && (self.buttons & button) == button
    }
}

#[cfg(windows)]
fn get_state(user: u32) -> Option<(u16, u8, u8)> {
    use windows_sys::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};
    let mut st: XINPUT_STATE = unsafe { std::mem::zeroed() };
    (unsafe { XInputGetState(user, &mut st) } == 0).then_some((st.Gamepad.wButtons, st.Gamepad.bLeftTrigger, st.Gamepad.bRightTrigger))
}

#[cfg(not(windows))]
fn get_state(_: u32) -> Option<(u16, u8, u8)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_generic_keys() {
        assert!(matches(0x10, 0xA1) && matches(0x12, 0xA4) && !matches(0x10, 0xA2) && matches(0x14, 0x14));
        assert_eq!(key_name(0x71), "F2");
        assert_eq!(key_name(0x47), "G");
        assert_eq!(key_name(0x63), "Numpad 3");
        assert_eq!(key_name(0xFF), "Key 0xFF");
        assert_eq!(button_name(LEFT_TRIGGER), "LT");
        assert_eq!(button_name(0x40000), "Button 0x40000");
        let mut p = Gamepad::default();
        p.poll();
        assert!(!p.is_down(0));
    }
}
