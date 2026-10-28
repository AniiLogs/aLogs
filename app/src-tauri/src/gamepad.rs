//! Which controller buttons are held right now (XInput: Xbox controllers, and others through Steam Input), for the
//! screen-capture switch's controller hotkey. The Xbox button comes from XInput's undocumented XInputGetStateEx
//! (xinput1_4.dll ordinal 100); without it, every other button still works.

use std::sync::OnceLock;
use windows::core::{s, PCSTR};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

/// The bits: XInput's buttons (Up 0x1, Down 0x2, Left 0x4, Right 0x8, Menu 0x10, View 0x20, LS 0x40, RS 0x80,
/// LB 0x100, RB 0x200, Xbox 0x400, A 0x1000, B 0x2000, X 0x4000, Y 0x8000), plus the triggers pulled past half way.
pub const LT: u32 = 0x1_0000;
pub const RT: u32 = 0x2_0000;

#[repr(C)]
#[derive(Default)]
struct Gamepad {
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    thumbs: [i16; 4],
}

#[repr(C)]
#[derive(Default)]
struct XState {
    packet: u32,
    pad: Gamepad,
}

type GetState = unsafe extern "system" fn(u32, *mut XState) -> u32;

fn get_state() -> Option<GetState> {
    static FOUND: OnceLock<Option<GetState>> = OnceLock::new();
    *FOUND.get_or_init(|| unsafe {
        for dll in [s!("xinput1_4.dll"), s!("xinput1_3.dll"), s!("xinput9_1_0.dll")] {
            let Ok(module) = LoadLibraryA(dll) else { continue };
            // ordinal 100: XInputGetStateEx, which also reports the Xbox button
            let f = GetProcAddress(module, PCSTR(100 as *const u8)).or_else(|| GetProcAddress(module, s!("XInputGetState")));
            if let Some(f) = f {
                return Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, GetState>(f));
            }
        }
        None
    })
}

/// The buttons held on any connected controller (0: none, or no XInput).
pub fn held() -> u32 {
    let Some(get) = get_state() else { return 0 };
    let mut all = 0;
    for user in 0..4 {
        let mut state = XState::default();
        if unsafe { get(user, &mut state) } == 0 {
            all |= state.pad.buttons as u32;
            all |= if state.pad.left_trigger > 128 { LT } else { 0 } | if state.pad.right_trigger > 128 { RT } else { 0 };
        }
    }
    all
}

#[cfg(test)]
mod tests {
    /// XInput loads, and what's held now: `cargo test --bin alogs-app gamepad -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reads_controllers() {
        assert!(super::get_state().is_some(), "no XInput");
        for _ in 0..20 {
            println!("held {:#x}", super::held());
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
}
