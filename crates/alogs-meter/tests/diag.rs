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
