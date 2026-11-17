//! Online / AFK for the player's AniiLogs profile, while the game runs and aLogs is on. You're AFK when either has
//! gone on for 20 minutes: Aniimo isn't the window in front, or nobody has touched the PC (no keyboard or mouse
//! input, no controller input). Nothing here sends anything. Port of aLogs B16 `bossbar/presence.py`.

pub const AFK_AFTER: f64 = 20.0 * 60.0;

/// Seconds since the last keyboard or mouse input anywhere on this PC.
pub fn keyboard_mouse_idle() -> f64 {
    imp::keyboard_mouse_idle()
}

pub struct Presence {
    pads: [Option<u32>; 4],
    pad_input: f64,
    in_front: f64,
    pad_check: f64,
}

impl Presence {
    pub fn new(now: f64) -> Presence {
        Presence { pads: [None; 4], pad_input: now, in_front: now, pad_check: 0.0 }
    }

    /// Seconds since a controller's state last changed.
    pub fn controller_idle(&mut self, now: f64) -> f64 {
        if now - self.pad_check >= 1.0 {
            self.pad_check = now;
            for i in 0..4 {
                match imp::pad_packet(i as u32) {
                    None => self.pads[i] = None,
                    Some(p) => {
                        if self.pads[i].is_some_and(|old| old != p) {
                            self.pad_input = now;
                        }
                        self.pads[i] = Some(p);
                    }
                }
            }
        }
        now - self.pad_input
    }

    /// "online" or "afk".
    pub fn state(&mut self, now: f64, game_in_front: bool) -> &'static str {
        if game_in_front {
            self.in_front = now;
        }
        let idle = keyboard_mouse_idle().min(self.controller_idle(now));
        if now - self.in_front >= AFK_AFTER || idle >= AFK_AFTER { "afk" } else { "online" }
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    use windows_sys::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};

    pub fn keyboard_mouse_idle() -> f64 {
        let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        if unsafe { GetLastInputInfo(&mut li) } == 0 {
            return 0.0;
        }
        f64::from(unsafe { GetTickCount() }.wrapping_sub(li.dwTime)) / 1000.0
    }

    pub fn pad_packet(i: u32) -> Option<u32> {
        let mut st: XINPUT_STATE = unsafe { std::mem::zeroed() };
        (unsafe { XInputGetState(i, &mut st) } == 0).then_some(st.dwPacketNumber)
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn keyboard_mouse_idle() -> f64 {
        0.0
    }
    pub fn pad_packet(_: u32) -> Option<u32> {
        None
    }
}
