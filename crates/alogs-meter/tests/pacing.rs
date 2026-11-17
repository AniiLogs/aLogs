//! How much of the picture a look copies between fights (full_frame_due, screen_parts) and the wait for window
//! capture on a spotted screen (game_only), on a live meter.

use alogs_meter::env::Env;
use alogs_meter::meter::{Args, Frames, Meter, TOP_PART};
use alogs_meter::pacing::Due;
use alogs_pil::Image;
use alogs_reader::ocr::NoOcr;
use std::path::PathBuf;

struct ScreenCopy {
    asked: u64,
}
impl Frames for ScreenCopy {
    fn grab(&mut self, _: usize, _: Option<f64>) -> Option<Image> { None }
    fn window(&mut self) -> Option<usize> { Some(1) }
    fn screen_copy(&self) -> bool { true }
    fn set_game_only_until(&mut self, ms: u64) { self.asked = ms; }
}

fn meter(name: &str) -> Meter {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let env = Env::new(data.join("res"), data);
    env.fixed_now.set(Some(1_800_000_000.0));
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: false,
                      no_submit: true, no_debug_frames: true, no_overlay: false, no_auto_click: true, player_log: PathBuf::new() };
    let mut m = Meter::new(env, args, Box::new(NoOcr), None);
    m.hwnd = Some(1);
    m.frame_size = (3840, 2160);
    m
}

#[test]
fn between_fights_only_the_top_part_unless_something_needs_more() {
    let mut m = meter("pacing-due");
    let now = 1_800_000_000.0;
    // a screen read is due (none yet): just its boxes
    assert_eq!(m.full_frame_due(now), Due::Screen);
    // read a moment ago, and the top part hasn't changed: nothing more than the top part
    m.rs.last_results = now - 0.5;
    m.top_sig = Some(vec![1, 2, 3]);
    m.ss.screen_read_sig = Some(vec![1, 2, 3]);
    assert_eq!(m.full_frame_due(now), Due::No);
    // a screen is up: whole frames until it's gone
    m.rs.last_results = now - 2.0;
    m.ss.full_until = now + 1.0;
    assert_eq!(m.full_frame_due(now), Due::Whole);
    // just after a match's end screen: whole frames on every look (the rank screen can come any moment)
    m.ss.full_until = 0.0;
    m.rs.last_results = now - 0.5;
    m.egg.sent_at = now - 10.0;
    assert_eq!(m.full_frame_due(now), Due::Whole);
    // the boxes: the top part first, the rest below it, all within the frame
    let parts = m.screen_parts(now).unwrap();
    assert_eq!(parts[0], (0.0, 0.0, 1.0, TOP_PART));
    assert!(parts.len() > 3);
    assert!(parts.iter().all(|p| p.0 >= 0.0 && p.1 >= 0.0 && p.2 <= 1.0 && p.3 <= 1.0 && p.0 < p.2 && p.1 < p.3));
}

#[test]
fn a_spotted_screen_waits_for_window_capture_then_gives_up() {
    let mut m = meter("pacing-game-only");
    let mut f = ScreenCopy { asked: 0 };
    let now = 1_800_000_000.0;
    assert!(m.game_only(&mut f, "result", now), "waits");
    assert!(f.asked > 0, "asks the app for window capture");
    assert!(m.game_only(&mut f, "result", now + 1.0));
    assert!(!m.game_only(&mut f, "result", now + 1.6), "gives up after 1.5 s");
    assert!(!m.game_only(&mut f, "EXP card", now + 1.7), "not a screen that needs it");
}

/// "How often the meter looks for a boss bar": a change to the setting is the pace from the next look, without a
/// restart; the pace the meter started with (here --interval's) holds until the setting changes.
#[test]
fn the_pace_between_looks_follows_its_setting() {
    use alogs_meter::pacing::look_interval;
    use alogs_pyval::{Value, dict};
    let cfg = |v: Value| dict([("capture", dict([("look_interval", v)]))]);
    let mut seen = Some(0.05);
    // started with --interval 0.2 while the setting says 0.05: nothing changed, so 0.2 stays
    assert_eq!(look_interval(0.2, &mut seen, &cfg(Value::Float(0.05))), 0.2);
    // the setting changes: it is the pace now, and stays
    assert_eq!(look_interval(0.2, &mut seen, &cfg(Value::Float(0.1))), 0.1);
    assert_eq!(seen, Some(0.1));
    assert_eq!(look_interval(0.1, &mut seen, &cfg(Value::Float(0.1))), 0.1);
    // a whole number of seconds is read too
    assert_eq!(look_interval(0.1, &mut seen, &cfg(Value::Int(1))), 1.0);
    // nonsense (nothing, zero, below zero) never becomes the pace
    for bad in [Value::None, Value::Float(0.0), Value::Float(-1.0)] {
        assert_eq!(look_interval(1.0, &mut seen, &cfg(bad)), 1.0);
    }
    // ...and a proper value afterwards is taken again
    assert_eq!(look_interval(1.0, &mut seen, &cfg(Value::Float(0.05))), 0.05);
}
