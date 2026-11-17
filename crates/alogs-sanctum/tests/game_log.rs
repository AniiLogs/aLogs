//! Where the game log says you are, on a made-up log: an Egg Heist run's island, its Sanctum and the scene it names;
//! an Interlink room in between is elsewhere; a homeland is a homeland.

use alogs_sanctum::game_log::{GameLog, Place};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[test]
fn a_run_is_followed_through_the_log() {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sanctum-player.log");
    std::fs::write(&p, "start\n").unwrap();
    let t = Arc::new(Mutex::new(100.0));
    let t2 = t.clone();
    let mut g = GameLog::new(Some(p.clone())).with_clock(Box::new(move || *t2.lock().unwrap()));
    g.poll(); // caught up
    let add = |s: &str| { let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap(); writeln!(f, "{s}").unwrap(); };
    add("connect to worldx-eu-prod-line-01");
    add("createVoxelScene new>>>id:lobby1");
    g.poll();
    assert_eq!(g.place, Place::Elsewhere);
    add("connect to worldx-eu-prod-dungeon-07");
    add("createVoxelScene new>>>id:islandA");
    g.poll();
    assert_eq!(g.place, Place::Elsewhere, "waiting for an Interlink scene that doesn't come");
    *t.lock().unwrap() += 4.0;
    g.poll();
    assert_eq!(g.place, Place::Island);
    assert_eq!(g.run_id(), Some("islandA"));
    add("createVoxelScene new>>>id:sanctumB");
    add("[warn] scene='Level_FB_Random_Map_20032' light probes missing");
    g.poll();
    assert_eq!(g.place, Place::Sanctum);
    assert_eq!(g.sanctum_scene, Some(20032));
    assert_eq!(g.named_sanctum(), Some((20032, 0.0)), "named just now");
    *t.lock().unwrap() += 20.0;
    assert_eq!(g.named_sanctum(), Some((20032, 20.0)));
    add("createVoxelScene new>>>id:islandA");
    g.poll();
    assert_eq!(g.place, Place::Island);
    assert_eq!(g.named_sanctum(), None, "out on the island no Sanctum is named");
    add("connect to worldx-eu-prod-dungeon-03");
    add("createVoxelScene new>>>id:roomZ");
    add("loading fb_bossrush_room_02");
    g.poll();
    assert_eq!(g.place, Place::Elsewhere);
    add("connect to worldx-eu-prod-singleworld-02");
    add("createVoxelScene new>>>id:100-1234-550");
    g.poll();
    assert_eq!(g.place, Place::Homeland);
    assert!(g.changes.contains(&(Place::Island, true)), "{:?}", g.changes);
}

/// The game's light-probe loader names the Sanctum's scene; its wording changed between game builds. The first
/// wording read, and the newer one as seen for the lobby (the scene is an id there, the level is in Config= and in
/// the data's folder), all give the scene. A line that only mentions a Sanctum scene some other way never does: it is
/// kept as words for the helper's log.
#[test]
fn the_scene_is_read_in_each_wording_of_the_loader() {
    let enter = ["connect to worldx-eu-prod-line-01", "createVoxelScene new>>>id:lobby1", "connect to worldx-eu-prod-dungeon-07", "createVoxelScene new>>>id:islandA", "createVoxelScene new>>>id:sanctumB"];
    let named = |name: &str, tag: &str| {
        let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("sanctum-player-wording-{tag}.log"));
        std::fs::write(&p, enter.join("\n") + "\n" + name + "\n").unwrap();
        let mut g = GameLog::new(Some(p));
        g.poll();
        assert_eq!(g.place, Place::Sanctum, "{tag}");
        (g.sanctum_scene, g.sanctum_mention)
    };
    assert_eq!(named("[warn] scene='Level_FB_Random_Map_20054' light probes missing", "first").0, Some(20054));
    let newer = "[98.7158][Error]|[XGIProbe.Load] GI Probe load failed. Scene='0f3a9c', Config='Level_FB_Random_Map_20054_BakingConfig', Platform='PC', BrickData='', Reason=none";
    assert_eq!(named(newer, "config").0, Some(20054));
    let folder = "[98.7158][Error]|[XGIProbe.Load] GI Probe load failed. Scene='0f3a9c', Config='', BrickData='C:/Games/Aniimo/AssetsExtra/raw/xrenderPC/Level_FB_Random_Map_20054/XGIProbe/CellBricksData.bytes'";
    assert_eq!(named(folder, "folder").0, Some(20054));
    let runtime = folder.replace("xrenderPC/Level_FB", "xrenderPC/RuntimeLevel_FB");
    assert_eq!(named(&runtime, "runtime").0, Some(20054));
    // some other line about a Sanctum scene: not the name, kept as words (and of a path, nothing above its folder)
    let (scene, words) = named("Unloading C:/Users/someone/cache/Level_FB_Random_Map_20032_lod.uab (3 objects)", "other");
    assert_eq!(scene, None);
    assert_eq!(words.as_deref(), Some("/Level_FB_Random_Map_20032_lod.uab (3 objects)"));
    // out on the island nothing is read at all
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sanctum-player-wording-island.log");
    std::fs::write(&p, enter[..4].join("\n") + "\n" + newer + "\n").unwrap();
    let mut g = GameLog::new(Some(p));
    g.poll();
    assert_eq!((g.sanctum_scene, g.sanctum_mention.clone()), (None, None));
}

/// Sanctum Helper started in the middle of a run: the log already holds the Sanctum and its name. The name still
/// says which layout it is, but not that the player stands at the door: it counts as named long ago.
#[test]
fn a_name_read_while_catching_up_is_old() {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sanctum-player-midrun.log");
    let lines = [
        "connect to worldx-eu-prod-line-01",
        "createVoxelScene new>>>id:lobby1",
        "connect to worldx-eu-prod-dungeon-07",
        "createVoxelScene new>>>id:islandA",
        "createVoxelScene new>>>id:sanctumB",
        "[warn] scene='Level_FB_Random_Map_20054' light probes missing",
    ];
    std::fs::write(&p, lines.join("\n") + "\n").unwrap();
    let t = Arc::new(Mutex::new(5000.0));
    let t2 = t.clone();
    let mut g = GameLog::new(Some(p.clone())).with_clock(Box::new(move || *t2.lock().unwrap()));
    g.poll();
    assert_eq!(g.place, Place::Sanctum);
    let (scene, age) = g.named_sanctum().unwrap();
    assert_eq!(scene, 20054);
    assert!(age > 1000.0, "{age}");
    assert!(g.changes.is_empty(), "what the log already held is the starting point, not news: {:?}", g.changes);
    // the same Sanctum named again, live (gone out and back in): fresh
    let add = |s: &str| { let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap(); writeln!(f, "{s}").unwrap(); };
    add("createVoxelScene new>>>id:islandA");
    add("createVoxelScene new>>>id:sanctumB");
    add("[warn] scene='Level_FB_Random_Map_20054' light probes missing");
    g.poll();
    assert_eq!(g.named_sanctum(), Some((20054, 0.0)));
}
