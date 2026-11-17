//! Sanctum Helper's own log (log.txt in its settings folder, started afresh past 2 MB). Port of `Log` in Settings.cs.
//! Never write a player's sign-in or other private data here.

use std::io::Write;
use std::sync::Mutex;

static GATE: Mutex<()> = Mutex::new(());

pub fn path() -> std::path::PathBuf {
    crate::settings::dir().join("log.txt")
}

pub fn write(msg: &str) {
    let line = format!("{} {msg}\n", chrono::Local::now().format("%H:%M:%S%.3f"));
    let _g = GATE.lock().unwrap_or_else(|e| e.into_inner());
    let p = path();
    let _ = std::fs::create_dir_all(crate::settings::dir());
    if std::fs::metadata(&p).is_ok_and(|m| m.len() > 2_000_000) {
        let _ = std::fs::remove_file(&p);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = f.write_all(line.as_bytes());
    }
}
