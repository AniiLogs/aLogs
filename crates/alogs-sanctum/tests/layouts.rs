//! The layout database and chest sets on Sanctum Helper's own layouts.json (from reference/, skipped without it).

use alogs_sanctum::layout_db::{LayoutDb, Run, allowed, grade};
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn layouts_load_and_chest_sets_narrow() {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/SanctumHelper/app/SanctumHelper/Assets");
    if !assets.exists() { eprintln!("skipped: reference/ not cloned"); return; }
    let db = LayoutDb::load(&assets).unwrap();
    assert_eq!(db.layouts.len(), 33);
    assert!(db.island().is_some());
    assert!(!db.candidates(None).is_empty());
    assert_eq!(grade("G4宝箱"), 4);
    assert_eq!(grade("key"), 0);
    let mut g: HashMap<String, HashMap<String, Vec<i64>>> = HashMap::new();
    g.insert("Hard".into(), [("room".to_string(), vec![4, 5]), ("hallway".to_string(), vec![2])].into_iter().collect());
    assert_eq!(allowed(&g, [4, 5, 6], Some("Hard")), vec![4, 5]);
    assert_eq!(allowed(&g, [1, 2, 3], Some("Hard")), vec![2]);
    assert_eq!(allowed(&g, [4, 6], Some("Unknown")), vec![4, 6], "a name the table lacks rules nothing out");
    // a chest seen where only one set of its room has one: that room's set is known
    let far = |a: &[f64], b: &[f64]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt() > 1.5;
    let mut found = None;
    'outer: for l in &db.layouts {
        for (room, by) in alogs_sanctum::layout_db::sets_of(l) {
            if by.len() < 2 { continue; }
            for (g, cs) in &by {
                for c in cs {
                    // unique in its room, and no other room's chest near it either
                    let alone = l.markers.iter().filter(|m| alogs_sanctum::layout_db::is_chest(m) && alogs_sanctum::layout_db::grade(&m.group) != 0)
                        .all(|m| (m.room == room && alogs_sanctum::layout_db::grade(&m.group) == *g) || far(&m.world, &c.world));
                    if alone { found = Some((l, room, *g, c.clone())); break 'outer; }
                }
            }
        }
    }
    let (l, room, g1, c) = found.expect("a chest of one set only");
    let mut run = Run::new(&db.grades, l, None);
    assert!(run.saw(c.world[0], c.world[1], None));
    assert_eq!(run.possible(room), vec![g1]);
    assert!(run.confirmed(room));
}

#[test]
fn settings_keep_what_alogs_changed_meanwhile() {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sanctum-settings");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // SAFETY: this test alone sets it, before anything reads it
    unsafe { std::env::set_var("SANCTUM_DIR", &d) };
    std::fs::write(d.join("settings.json"), r#"{"HoldKey": 20, "Difficulty": "Beginner", "WindowTitle": "Aniimo"}"#).unwrap();
    let mut s = alogs_sanctum::settings::Settings::load();
    assert_eq!(s.hold_key, 0xA2, "v1's Caps Lock moves to Left Ctrl");
    assert_eq!(s.difficulty, "Normal");
    assert_eq!(s.version, 5);
    // aLogs changes the window title in the file; this process changes the opacity: both survive the save
    let mut on_disk: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(d.join("settings.json")).unwrap()).unwrap();
    on_disk["WindowTitle"] = "Aniimo2".into();
    std::fs::write(d.join("settings.json"), on_disk.to_string()).unwrap();
    s.map_opacity = 0.3;
    s.save();
    let back: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(d.join("settings.json")).unwrap()).unwrap();
    assert_eq!(back["WindowTitle"], "Aniimo2");
    assert_eq!(back["MapOpacity"], 0.3);
}
