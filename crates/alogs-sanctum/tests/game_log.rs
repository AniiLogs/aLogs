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
    add("createVoxelScene new>>>id:islandA");
    g.poll();
    assert_eq!(g.place, Place::Island);
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
