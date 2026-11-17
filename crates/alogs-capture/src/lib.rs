//! The game picture from the aLogs app's shared capture, and the game window and our inputs to it.
//! Port of aLogs B16 `bossbar/wincapture.py`.

pub mod focusguard;
pub mod input;
pub mod layered;
pub mod rewasd;
pub mod presence;
pub mod shared;
pub mod win;

pub use shared::{SharedAudio, SharedCapture};
