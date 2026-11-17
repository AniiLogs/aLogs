//! Stops a build when the shipped boss list or Interlink HP-bar table isn't the installed game's (catalog_drift,
//! grades_drift), as the B16 build's `bossbar/build_meter.py` check_data did. A stale boss list (no in_boss_rush on
//! the second bosses of weeks 4003-4005's middle fights) matched bosses to the wrong ones; a stale grade table (16 bars)
//! put every Interlink fight's damage on the wrong bar.
//!
//!     cargo run --release -p alogs-gamedata --example check_data -- <folder with boss_catalog.csv>
//!
//! (aLogs' `npm run meter:build` runs it on `crates/alogs-meter/res`.) Without a game install only what can be checked
//! without it is: every boss in the shipped week schedule has a boss row. Exit code 1 when anything differs.
//!
//! Nothing in the workspace rewrites the two files yet: the B16 build's `gamedata.py catalog` / `grades` does (in a
//! checkout of the old commit, e.g. reference/aLogs/bossbar; it writes next to itself), then copy the file over.
use alogs_gamedata::gamedata as g;
use alogs_gamedata::pmdata::PmData;
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let game = g::find_game_data(None);
    let place = match &game {
        Some(p) => p.display().to_string(),
        None => "what can be checked without the game (no game install found)".into(),
    };
    let catalog = g::catalog_drift(game.as_deref(), &dir.join("boss_catalog.csv"), None);
    let grades = match &game {
        Some(p) => match PmData::open(p) {
            Ok(mut pm) => g::grades_drift(Some(&mut pm), &dir.join("boss_rush_grades.csv")),
            Err(e) => Ok(vec![format!("the game data can't be read: {e}")]),
        },
        None => g::grades_drift(None, &dir.join("boss_rush_grades.csv")),
    };
    let mut stop = false;
    for (name, fix, drift) in [("boss_catalog.csv", "catalog", catalog), ("boss_rush_grades.csv", "grades", grades)] {
        let drift = drift.unwrap_or_else(|e| vec![format!("can't be checked: {e}")]);
        if drift.is_empty() {
            println!("{name} checked against {place}");
            continue;
        }
        println!("{name} differs from {place}:");
        for line in drift.iter().take(40) {
            println!("  {line}");
        }
        println!("  fix: `python gamedata.py {fix}` in the B16 meter's folder (a checkout of the old commit, e.g. reference/aLogs/bossbar), copy {name} into crates/alogs-meter/res and read the diff; or update the game first");
        stop = true;
    }
    if stop {
        eprintln!("build stopped: the shipped game data is out of date");
        std::process::exit(1);
    }
}
