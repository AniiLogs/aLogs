//! The layout the game's log names. On a made-up layout list: a scene finds its Sanctum layout, never the island and
//! nothing for a scene no layout has. With Sanctum Helper's own assets (from reference/ or a SanctumHelper/ checkout,
//! skipped without them): the engine, with no game window and so no picture at all, takes the layout the log names
//! as the Sanctum loads, and never starts working it out from the minimap.

use alogs_sanctum::engine::Engine;
use alogs_sanctum::game_log::Place;
use alogs_sanctum::input::KeyHook;
use alogs_sanctum::layout_db::LayoutDb;
use alogs_sanctum::settings::Settings;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

#[test]
fn a_scene_finds_its_layout() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("named-layouts");
    std::fs::create_dir_all(&dir).unwrap();
    let json = r#"{"layouts": [
        {"scene": 3005, "layout": 3005, "kind": "island"},
        {"scene": 20032, "layout": 32, "kind": "sanctum", "difficulties": ["Normal"]},
        {"scene": 20054, "layout": 54, "kind": "sanctum", "difficulties": ["Chaos"]}
    ]}"#;
    std::fs::write(dir.join("layouts.json"), json).unwrap();
    let db = LayoutDb::load(&dir).unwrap();
    assert_eq!(db.by_scene(20054).map(|l| l.number), Some(54));
    assert_eq!(db.by_scene(20032).map(|l| l.number), Some(32));
    assert!(db.by_scene(3005).is_none(), "the island is no Sanctum");
    assert!(db.by_scene(20099).is_none(), "a scene no layout has");
}

fn assets() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    ["reference/SanctumHelper/app/SanctumHelper/Assets", "SanctumHelper/app/SanctumHelper/Assets"].iter().map(|p| root.join(p)).find(|p| p.join("layouts.json").exists())
}

fn wait_until(secs: f64, mut ok: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed().as_secs_f64() < secs {
        if ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    ok()
}

#[test]
fn the_engine_takes_the_layout_the_log_names() {
    let Some(assets) = assets() else {
        eprintln!("skipped: no Sanctum Helper assets (reference/ or SanctumHelper/ not cloned)");
        return;
    };
    let dir = std::env::temp_dir().join(format!("alogs-engine-named-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: set before any thread of this test binary reads it (the other test here reads no settings)
    unsafe { std::env::set_var("SANCTUM_DIR", &dir) };
    let log = dir.join("Player.log");
    std::fs::write(&log, "made-up log line\n").unwrap();
    let add = |s: &str| {
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(f, "{s}").unwrap();
    };
    let s = Settings { window_title: "no window is called this 9c1e".into(), ..Default::default() };
    let db = Arc::new(LayoutDb::load(&assets).unwrap());
    let told = db.candidates(None).last().map(|l| (l.number, l.scene)).unwrap();
    let e = {
        let mut e = Engine::new(db.clone(), Arc::new(RwLock::new(s)), Arc::new(KeyHook::new(0x7B)), Some(log.clone()));
        e.start();
        e
    };
    // a run begins: the first dungeon area is the island (after the 3 s an Interlink room would be named in)
    add("connect to worldx-eu-prod-dungeon-07");
    add("createVoxelScene new>>>id:islandA");
    assert!(wait_until(8.0, || e.status().game_log_place == Some(Place::Island)), "{:?}", e.status().game_log_place);
    assert_eq!(e.status().layout, None);
    // into the Sanctum: the game names its scene as it loads
    let named_at = Instant::now();
    add("createVoxelScene new>>>id:sanctumB");
    add(&format!("[warn] scene='Level_FB_Random_Map_{}' light probes missing", told.1));
    let locked = wait_until(4.0, || e.status().layout == Some(told.0));
    let took = named_at.elapsed().as_secs_f64();
    let st = e.status();
    // past the wait for the name: no candidate's map was loaded for this Sanctum
    std::thread::sleep(Duration::from_millis(1800));
    let read = || std::fs::read_to_string(dir.join("log.txt")).unwrap_or_default();
    let first = read();
    // the next run: its Sanctum is never named (a patch took the line away). The layout stays open for the minimap,
    // and the candidates' maps are loaded once the wait for a name is over.
    add("connect to worldx-eu-prod-line-01");
    add("createVoxelScene new>>>id:lobby1");
    add("connect to worldx-eu-prod-dungeon-02");
    add("createVoxelScene new>>>id:islandC");
    let new_run = wait_until(8.0, || e.status().game_log_place == Some(Place::Island) && e.status().layout.is_none());
    add("createVoxelScene new>>>id:sanctumD");
    let unnamed = wait_until(5.0, || read().contains("no name for this Sanctum"));
    let st2 = e.status();
    drop(e);
    let written = read();
    std::fs::remove_dir_all(&dir).ok();
    assert!(locked, "layout {:?} via {:?}, wanted {}\n{written}", st.layout, st.layout_source, told.0);
    assert_eq!(st.layout_source, "game log");
    assert!(took < 3.0, "known {took:.2} s after the log named it");
    assert!(first.contains(&format!("layout locked: {} via game log", told.0)), "{first}");
    assert!(!first.contains("no name for this Sanctum"), "{first}");
    assert!(!written.contains("identifying the layout from the minimap"), "no picture, so nothing to identify from: {written}");
    assert!(!st.game_found, "no game window, no picture: the log alone said it");
    assert!(new_run, "the next run forgets the layout: {:?} {:?}\n{written}", st2.game_log_place, st2.layout);
    assert!(unnamed, "{written}");
    assert_eq!(st2.layout, None, "no name, no layout from the log");
    eprintln!("the layout was known {took:.2} s after the log named the Sanctum");
}
