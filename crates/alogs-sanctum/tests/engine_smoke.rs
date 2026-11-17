//! The engine starts, ticks and stops cleanly with no game window: settings in a temp folder, a made-up Player.log,
//! a window title nothing has (so no screen of the real game is ever read). Skipped without reference/.

use alogs_sanctum::engine::Engine;
use alogs_sanctum::input::KeyHook;
use alogs_sanctum::layout_db::LayoutDb;
use alogs_sanctum::settings::Settings;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

#[test]
fn engine_runs_without_a_game() {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/SanctumHelper/app/SanctumHelper/Assets");
    if !assets.exists() {
        eprintln!("skipped: reference/ not cloned");
        return;
    }
    let dir = std::env::temp_dir().join(format!("alogs-engine-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: set before any thread of this test binary reads it
    unsafe { std::env::set_var("SANCTUM_DIR", &dir) };
    let log = dir.join("Player.log");
    std::fs::write(&log, "made-up log line\n").unwrap();
    let mut s = Settings::default();
    s.window_title = "no window is called this 9c1e".into();
    let db = Arc::new(LayoutDb::load(&assets).unwrap());
    let hook = Arc::new(KeyHook::new(0x7B));
    let mut e = Engine::new(db, Arc::new(RwLock::new(s)), hook, Some(log));
    let frames = Arc::new(Mutex::new(0));
    let f2 = frames.clone();
    e.on_frame(move |_| *f2.lock().unwrap() += 1);
    e.start();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let st = e.status();
    drop(e);
    let written = std::fs::read_to_string(dir.join("log.txt")).unwrap_or_default();
    std::fs::remove_dir_all(&dir).ok();
    assert!(!st.game_found);
    assert!(!st.in_run);
    assert!(written.contains("perf:") || written.is_empty() || !written.contains("panicked"), "{written}");
}
