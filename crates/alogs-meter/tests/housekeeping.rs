//! The hourly cleanup: old screenshots go, proof a queued record still needs and character proofs stay; old
//! per-frame fight logs go.

use alogs_meter::housekeeping::{clean_old_fight_logs, clean_old_screenshots};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn file(p: &Path, hours_old: u64) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(hours_old * 3600)).unwrap();
}

#[test]
fn old_pictures_go_needed_ones_stay() {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("housekeeping");
    let _ = std::fs::remove_dir_all(&data);
    let logs = data.join("logs");
    file(&logs.join("screens/old.jpg"), 30);
    file(&logs.join("screens/new.jpg"), 1);
    file(&logs.join("screens/queued.jpg"), 30);
    file(&logs.join("characters/1.png"), 900);
    file(&logs.join("battle_data/20260101.txt"), 30);
    file(&logs.join("fights/x.json"), 900);
    file(&logs.join("nobar/a.jpg"), 30);
    std::fs::create_dir_all(logs.join("submit_queue")).unwrap();
    std::fs::write(logs.join("submit_queue/1.json"), r#"{"evidence": "logs/screens/queued.jpg"}"#).unwrap();
    let (files, _) = clean_old_screenshots(&data, Some(24));
    assert_eq!(files, 3);
    assert!(!logs.join("screens/old.jpg").exists() && !logs.join("battle_data/20260101.txt").exists());
    assert!(logs.join("screens/new.jpg").exists() && logs.join("screens/queued.jpg").exists());
    assert!(logs.join("characters/1.png").exists() && logs.join("fights/x.json").exists());
    assert!(!logs.join("nobar").exists(), "a folder left empty goes");
    assert_eq!(clean_old_screenshots(&data, None), (0, 0), "keep everything");
    file(&logs.join("20260101_010101_Boss.csv"), 24 * 40);
    file(&logs.join("scores.csv"), 24 * 40);
    assert_eq!(clean_old_fight_logs(&data, 30.0).0, 1);
    assert!(logs.join("scores.csv").exists());
}

/// A record waiting in the queue for hours keeps every picture it names: its Party screen and its gizmo screens too.
/// The same pictures with no record waiting go.
#[test]
fn a_queued_records_party_and_gizmo_pictures_stay() {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("housekeeping-queued");
    let _ = std::fs::remove_dir_all(&data);
    let logs = data.join("logs");
    let names = ["screens/party_a.jpg", "screens/gizmo_a_1.jpg", "screens/gizmo_a_2.jpg", "screens/result_a.jpg"];
    for n in names {
        file(&logs.join(n), 30);
    }
    file(&logs.join("screens/other.jpg"), 30);
    std::fs::create_dir_all(logs.join("submit_queue")).unwrap();
    std::fs::write(logs.join("submit_queue/sim_run__a.json"), r#"{"evidence": "logs/screens/result_a.jpg", "battle": null,
        "party": "logs/screens/party_a.jpg", "gizmos": ["logs/screens/gizmo_a_1.jpg", "logs/screens/gizmo_a_2.jpg"]}"#).unwrap();
    let (files, _) = clean_old_screenshots(&data, Some(0));
    assert_eq!(files, 1, "only the picture no record names");
    assert!(!logs.join("screens/other.jpg").exists());
    for n in names {
        assert!(logs.join(n).exists(), "{n} kept while its record waits");
    }
    // the record sent: the same pictures go
    std::fs::remove_file(logs.join("submit_queue/sim_run__a.json")).unwrap();
    assert_eq!(clean_old_screenshots(&data, Some(0)).0, 4);
    for n in names {
        assert!(!logs.join(n).exists(), "{n}");
    }
}
