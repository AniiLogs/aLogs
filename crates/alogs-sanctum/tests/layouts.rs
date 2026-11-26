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

/// Boss rooms (Level_Room_30_BOSS, tagged "boss" by Sanctum Helper's builder) are framed like key rooms: their square
/// comes from the side the prefab names, as a key room's imgRect does, and their Legendary chest is certain. Layout 38's
/// own numbers (game build 4047363).
#[test]
fn boss_rooms_are_framed_and_their_legendary_is_sure() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("boss-layouts");
    std::fs::create_dir_all(&dir).unwrap();
    let json = r#"{"layouts": [{"scene": 20038, "layout": 38, "difficulties": ["Nightmare", "Chaos"],
        "worldToImage": {"ox": -1222.779848101266, "oy": -834.2470092226613, "sx": 6.572151898734178, "sz": 6.566534914361},
        "rooms": [
            {"id": 47, "type": 3, "kind": "boss", "prefab": "Level_Room_30_BOSS", "world": [429, -387], "img": [1596.7, 1707.0]},
            {"id": 62, "type": 5, "kind": "key", "prefab": "Level_Room_40_004_Key", "world": [400, -174], "img": [1406.1, 308.3],
             "size": 40, "imgRect": [1274.6, 177.0, 1537.5, 439.7]},
            {"id": 5, "type": 1, "kind": "room", "prefab": "Level_Room_30_007", "world": [0, 0], "img": [0, 0]}
        ],
        "markers": [{"kind": "legendary", "room": 47, "group": "BOSS", "world": [429.03, -387.28], "img": [1596.9, 1707.0]}]}]}"#;
    std::fs::write(dir.join("layouts.json"), json).unwrap();
    let db = LayoutDb::load(&dir).unwrap();
    let l = db.by_number(38).unwrap();
    let boss: Vec<_> = l.boss_rooms().collect();
    assert_eq!(boss.len(), 1);
    let r = l.room_rect(boss[0]).expect("a boss room's square from its prefab's 30 m");
    for (got, want) in r.iter().zip([1498.09, 1608.50, 1695.26, 1805.50]) {
        assert!((got - want).abs() < 0.05, "{r:?}");
    }
    // the side from the prefab gives what the builder wrote for a key room's imgRect
    let mut key = l.rooms[1].clone();
    key.img_rect = None;
    key.size = None;
    for (got, want) in l.room_rect(&key).unwrap().iter().zip([1274.6, 177.0, 1537.5, 439.7]) {
        assert!((got - want).abs() < 0.05);
    }
    assert!(l.sure_room(47) && l.sure_room(62), "the boss room's and the key room's Legendary are always there");
    assert!(!l.sure_room(5), "elsewhere only a chance");
    assert_eq!(grade("BOSS"), 0, "the boss room's chests are in no graded set: never ruled out");
}

/// Every boss room in Sanctum Helper's shipped layouts.json gets a frame (with its assets, from reference/ or a
/// SanctumHelper/ checkout; skipped without them).
#[test]
fn every_shipped_boss_room_has_a_frame() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(assets) = ["reference/SanctumHelper/app/SanctumHelper/Assets", "SanctumHelper/app/SanctumHelper/Assets"].iter().map(|p| root.join(p)).find(|p| p.join("layouts.json").exists()) else {
        eprintln!("skipped: no Sanctum Helper assets");
        return;
    };
    let db = LayoutDb::load(&assets).unwrap();
    let mut n = 0;
    for l in &db.layouts {
        for r in l.boss_rooms() {
            let q = l.room_rect(r).unwrap_or_else(|| panic!("{} room {}", l.title(), r.id));
            assert!(q[0] < r.img[0] && r.img[0] < q[2] && q[1] < r.img[1] && r.img[1] < q[3], "{} room {}: {q:?} around {:?}", l.title(), r.id, r.img);
            n += 1;
        }
    }
    eprintln!("{n} boss rooms");
}
