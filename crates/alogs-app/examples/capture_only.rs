// Runs only the shared capture (for testing readers without the whole app):
//   cargo run --example capture_only [window title] [seconds]
// The app's own helpers the capture calls are stood in for here: its log goes to the console, the game's title comes
// from the command line, settings are the defaults, no Dev switch is on and the overlay button never turns it off.
// (the app code it pulls in has much the example never uses)
#![allow(dead_code)]

#[path = "../src/capture.rs"]
mod capture;
#[path = "../src/gpu_capture.rs"]
mod gpu_capture;

use std::sync::OnceLock;

static TITLE: OnceLock<String> = OnceLock::new();

pub(crate) fn app_log(line: &str) {
    println!("{line}");
}

pub(crate) fn game_title() -> String {
    TITLE.get().cloned().unwrap_or_else(|| "Aniimo".into())
}

pub(crate) fn read_settings() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

mod dev {
    pub fn on(_: &str) -> bool {
        false
    }
}

mod overlay_button {
    pub fn capture_on() -> bool {
        true
    }
}

fn main() {
    let _ = TITLE.set(std::env::args().nth(1).unwrap_or_else(|| "Aniimo".into()));
    capture::start();
    std::thread::sleep(std::time::Duration::from_secs(std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(60)));
}
