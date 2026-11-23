//! The hourly cleanup: old screenshots go, proof a queued record still needs and character proofs stay; old
//! per-frame fight logs go.

use alogs_meter::housekeeping::{LOG_MIN_AGE_HOURS, clean_logs_over, clean_old_fight_logs, clean_old_screenshots};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn file(p: &Path, hours_old: u64) {
    sized(p, hours_old, 1);
}

fn sized(p: &Path, hours_old: u64, bytes: usize) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, vec![b'x'; bytes]).unwrap();
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(hours_old * 3600)).unwrap();
}

/// A folder made `hours` ago (its creation time: what the clean-up goes by for an empty folder).
fn made_hours_ago(d: &Path, hours: u64) {
    use std::os::windows::fs::{FileTimesExt, OpenOptionsExt};
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000; // needed to open a folder
    let f = std::fs::OpenOptions::new().write(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(d).unwrap();
    let t = SystemTime::now() - Duration::from_secs(hours * 3600);
    f.set_times(std::fs::FileTimes::new().set_created(t).set_modified(t)).unwrap();
}

/// Over the size bound the oldest pictures go first until all are under it; a queued record's files, the character
/// proofs, the test pictures and anything changed within LOG_MIN_AGE_HOURS (the fight now) stay, even if that leaves it
/// over. An empty folder made just now (a fight's snapshots folder at its start) stays; an old empty one goes.
#[test]
fn over_the_size_bound_the_oldest_go_first() {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("housekeeping-size");
    let _ = std::fs::remove_dir_all(&data);
    let logs = data.join("logs");
    sized(&logs.join("screens/a_oldest.jpg"), 40, 100);
    sized(&logs.join("screens/queued.jpg"), 50, 100); // older still, but a record waits for it
    sized(&logs.join("20261101_010101_Boss/f1.png"), 30, 100);
    sized(&logs.join("bar_samples/b.png"), 20, 100);
    sized(&logs.join("end_screens/x/e.jpg"), 10, 100);
    sized(&logs.join("characters/1.png"), 900, 100);
    sized(&logs.join("test_frames/20261101/t.png"), 900, 100);
    sized(&logs.join("20261123_010101_Boss/now.png"), 0, 100); // the fight going on
    sized(&logs.join("scores.csv"), 900, 100); // not a picture
    std::fs::create_dir_all(logs.join("20261123_020202_Boss")).unwrap(); // a fight just begun, nothing in it yet
    made_hours_ago(&logs.join("20261101_010101_Boss"), 30);
    std::fs::create_dir_all(logs.join("submit_queue")).unwrap();
    std::fs::write(logs.join("submit_queue/sim_run__a.json"), r#"{"evidence": "logs/screens/queued.jpg"}"#).unwrap();

    // removable now: a_oldest, f1, b, e, now = 500 bytes; under the bound nothing goes
    assert_eq!(clean_logs_over(&data, 500, LOG_MIN_AGE_HOURS), (0, 0));
    // over: the oldest two go (a_oldest, then f1 with its folder), leaving 300
    assert_eq!(clean_logs_over(&data, 350, LOG_MIN_AGE_HOURS), (2, 200));
    assert!(!logs.join("screens/a_oldest.jpg").exists() && !logs.join("20261101_010101_Boss").exists());
    assert!(logs.join("bar_samples/b.png").exists() && logs.join("end_screens/x/e.jpg").exists());
    assert!(logs.join("screens/queued.jpg").exists(), "a queued record's picture stays");
    assert!(logs.join("20261123_020202_Boss").is_dir(), "the empty folder of a fight just begun stays");
    // a bound nothing can meet: all but the young picture go, and what is kept anyway stays
    assert_eq!(clean_logs_over(&data, 0, LOG_MIN_AGE_HOURS), (2, 200));
    assert!(logs.join("20261123_010101_Boss/now.png").exists(), "the fight going on stays");
    for kept in ["screens/queued.jpg", "characters/1.png", "test_frames/20261101/t.png", "scores.csv"] {
        assert!(logs.join(kept).exists(), "{kept}");
    }
    assert!(logs.join("submit_queue").is_dir());
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
    made_hours_ago(&logs.join("nobar"), 30);
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
