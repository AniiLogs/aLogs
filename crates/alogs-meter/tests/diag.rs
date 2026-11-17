//! Test pictures only while Save test pictures is on, spaced per kind; samples keep the newest few.

use alogs_meter::env::Env;
use alogs_meter::meter::{Args, Meter};
use alogs_pil::{Image, Mode};
use alogs_pyval::Value;
use alogs_reader::ocr::NoOcr;
use std::path::PathBuf;

fn meter(name: &str, test_frames: bool) -> Meter {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let env = Env::new(data.join("res"), data);
    env.fixed_now.set(Some(1_800_000_000.0));
    let mut cfg = alogs_protocol::settings::defaults();
    if let Value::Dict(d) = &mut cfg { d.insert("capture".into(), alogs_pyval::dict([("test_frames", Value::Bool(test_frames))])); }
    let args = Args { cfg, duration: 180.0, level: None, boss_id: None, replay: false,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    Meter::new(env, args, Box::new(NoOcr), None)
}

fn count(d: &std::path::Path) -> usize {
    std::fs::read_dir(d).map_or(0, |r| r.count())
}

#[test]
fn test_pictures_follow_the_switch_and_their_gaps() {
    let im = Image::new(Mode::Rgb, 64, 36, &[10, 20, 30]);
    let t = 1_800_000_000.0;
    let mut off = meter("diag-off", false);
    off.test_frame(&im, "result", t);
    off.keep_sample(&im, "party", None, None);
    assert!(!off.env.data.join("logs").exists());
    let mut m = meter("diag-on", true);
    for i in 0..4 {
        m.test_frame(&im, "result", t + i as f64 * 3.0);   // every 8 s at most: t and t+9
    }
    for _ in 0..3 {
        m.keep_sample(&im, "party", Some(0.0), Some(2));
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    let day = alogs_meter::results::local(t, "%Y%m%d");
    assert_eq!(count(&m.env.data.join("logs/test_frames").join(day).join("result")), 2);
    assert!(count(&m.env.data.join("logs/samples")) >= 1);
}

/// "Boss bar snapshots during fights": the bar with its name above and its counter below, no wider than 1000 px,
/// named by the seconds into the fight; none without a fight.
#[test]
fn a_boss_bar_snapshot_is_the_bar_with_its_text_and_no_wider_than_1000() {
    use alogs_meter::diag::{snapshot_box, snapshot_name, snapshot_picture};
    use alogs_reader::bar::Bar;
    let bar = |left: f64, span: f64, yc: i64, frame: (usize, usize)| Bar {
        yc, left_bracket: left, span, brackets_found: true, world: false, right_red: 0.0, track_left: left as i64 + 8,
        track_right: (left + span) as i64 - 8, thickness: 12, frame,
    };
    // 1920x1080: a 760 px bar at x 580, y 77
    let b = bar(580.0, 760.0, 77, (1920, 1080));
    assert_eq!(snapshot_box(&b), (564, 16, 1362, 99));
    let im = Image::new(Mode::Rgb, 1920, 1080, &[10, 20, 30]);
    let shot = snapshot_picture(&im, &b);
    assert_eq!((shot.width, shot.height), (798, 83), "under 1000 px: kept as it is");
    // 3840x2160: twice the size is made 1000 px wide, its height in step
    let b4k = bar(1160.0, 1520.0, 154, (3840, 2160));
    assert_eq!(snapshot_box(&b4k), (1129, 32, 2725, 199));
    let shot = snapshot_picture(&Image::new(Mode::Rgb, 3840, 2160, &[10, 20, 30]), &b4k);
    assert_eq!((shot.width, shot.height), (1000, 167 * 1000 / 1596));
    assert_eq!(snapshot_name(12.5), "0012.50.png");
    assert_eq!(snapshot_name(0.04), "0000.04.png");
    assert_eq!(snapshot_name(125.0), "0125.00.png");

    // no fight: nothing is saved
    let mut m = meter("diag-snapshot", true);
    m.snapshot(&im, &b, 1_800_000_000.0);
    assert!(!m.env.data.join("logs").join("fights").exists());
}
