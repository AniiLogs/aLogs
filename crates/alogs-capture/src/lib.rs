//! The game picture from the aLogs app's shared capture, and the game window and our inputs to it.
//! Port of aLogs B16 `bossbar/wincapture.py`.

pub mod shared;
pub mod win;

pub use shared::{SharedAudio, SharedCapture};
