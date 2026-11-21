//! How much of the picture a look copies between fights (full_frame_due, screen_parts) and the wait for window
//! capture on a spotted screen (game_only), on a live meter.

use alogs_meter::env::Env;
use alogs_meter::meter::{Args, Frames, Meter, TOP_PART};
use alogs_meter::pacing::Due;
use alogs_pil::Image;
use alogs_reader::ocr::NoOcr;
use std::path::PathBuf;

#[derive(Default)]
struct ScreenCopy {
    asked: u64,
    fast: u64,
}
impl Frames for ScreenCopy {
    fn grab(&mut self, _: usize, _: Option<f64>) -> Option<Image> { None }
    fn window(&mut self) -> Option<usize> { Some(1) }
    fn screen_copy(&self) -> bool { true }
    fn set_game_only_until(&mut self, ms: u64) { self.asked = ms; }
    fn set_game_fast_until(&mut self, ms: u64) { self.fast = ms; }
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
    let mut f = ScreenCopy::default();
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

struct Placed;
impl Frames for Placed {
    fn grab(&mut self, _: usize, _: Option<f64>) -> Option<Image> { None }
    fn window(&mut self) -> Option<usize> { Some(1) }
    fn origin(&mut self, _: usize) -> Option<(i32, i32)> { Some((2560, -8)) }
}

struct Quiet;
impl alogs_meter::meter::Out for Quiet {
    fn out(&mut self, _: &str, _: alogs_pyval::Value) {}
}

/// The meter knows where the game's picture sits on the screen (a second monitor, a window that isn't at the top left):
/// its clicks, scrolls and overlay places are counted from there.
#[test]
fn the_games_place_on_screen_is_read_with_every_look() {
    let mut m = meter("pacing-origin");
    m.args.replay = false;
    assert_eq!(m.origin, (0, 0));
    m.step(&mut Placed, &mut Quiet);
    assert_eq!(m.origin, (2560, -8));
}

/// The meter isn't idle while a screen a script starts from is expected: idle, it looks 4 times a second and reads
/// the screen's text once a second at most, and the script starts that much later.
#[test]
fn not_idle_while_a_scripts_screen_is_expected() {
    let mut m = meter("pacing-expected");
    m.args.replay = false;
    assert!(m.idle(), "nothing going on");
    // a Holo-Battle Sim room: idle until its last level, where the Victory screen is next
    m.holosim_space = true;
    assert!(m.idle(), "a Sim room before the last level");
    m.sim.gate.last = true;
    assert!(!m.idle(), "the last level: the Victory screen is expected");
    m.holosim_space = false;
    assert!(m.idle(), "the gate's state of a room already left doesn't count");
    // Egg Heist's rank screen being read
    m.egg.points = Some("12".into());
    assert!(!m.idle(), "Egg Heist's points are being read");
    m.egg.points = None;
    assert!(m.idle());
}

/// A screen's read made from a frame holding only the boxes is made again from a whole frame, whatever this look's
/// own frame is: handled as it was, the screen's reader and its proof ran on a mostly black picture.
#[test]
fn a_read_made_from_a_boxes_only_frame_is_made_again_whole() {
    let mut m = meter("pacing-read-parts");
    m.args.replay = false;
    let t = 1_800_000_000.0;
    let im = Image::new(alogs_pil::Mode::Rgb, 1920, 1080, &[0, 0, 0]);
    let heads: alogs_meter::screens::Heads = [("cs".to_string(), "challenge".to_string())].into_iter().collect();
    // this look's frame is whole, the read's was not: nothing is handled, the read is asked for again at once
    (m.parts_frame, m.ss.read_parts, m.rs.last_results) = (false, true, 123.0);
    m.read_screen(&mut Placed, &mut Quiet, &im, t, &heads);
    assert_eq!(m.rs.last_results, 0.0, "not asked for again");
    assert!(m.ss.full_until > t, "whole frames from here on");
    // the other way round: the read's frame was whole, so it is handled
    (m.parts_frame, m.ss.read_parts, m.rs.last_results) = (true, false, 123.0);
    m.read_screen(&mut Placed, &mut Quiet, &im, t, &heads);
    assert_eq!(m.rs.last_results, 123.0, "a read from a whole frame was thrown away");
}

/// While a script has the screen the game window alone is asked for on every look's turn (again each 0.5 s), and
/// before the Sim's Victory screen is spotted: asked only from a spotted screen's read, window capture came after the
/// first read (which was then made twice) and ran out during the Gizmo tour, which reads no screens (Windows 10).
#[test]
fn the_game_window_alone_is_kept_through_a_script() {
    use alogs_meter::pacing::{GAME_ONLY_HOLD_S, SIM_VICTORY_SOON_S};
    let mut m = meter("pacing-keep");
    m.args.replay = false;
    let t = 1_800_000_000.0;
    let until = |at: f64| ((at + GAME_ONLY_HOLD_S) * 1000.0) as u64;
    let mut f = ScreenCopy::default();
    m.game_only_keep(&mut f, t);
    assert_eq!(f.asked, 0, "nothing on: not asked");
    // the Gizmo tour
    m.sim.tour = Some(Default::default());
    m.game_only_keep(&mut f, t);
    assert_eq!(f.asked, until(t), "asked at the tour's first look");
    assert_eq!(f.fast, ((t + alogs_meter::pacing::GAME_FAST_HOLD_S) * 1000.0) as u64, "and quick pictures with it: a script has the screen");
    m.game_only_keep(&mut f, t + 0.3);
    assert_eq!(f.asked, until(t), "not again within 0.5 s");
    m.game_only_keep(&mut f, t + 0.6);
    assert_eq!(f.asked, until(t + 0.6), "kept going: it never runs out while the tour runs");
    m.sim.tour = None;
    f.asked = 0;
    m.game_only_keep(&mut f, t + 5.0);
    assert_eq!(f.asked, 0, "the tour is over: no more");
    // the Sim's last fight has just ended: asked before the Victory screen is there to spot
    (m.holosim_space, m.sim.gate.last, m.fight_ended_at) = (true, true, t + 9.0);
    m.game_only_keep(&mut f, t + 10.0);
    assert_eq!(f.asked, until(t + 10.0), "the Victory screen is expected");
    assert!(f.fast < ((t + 5.0) * 1000.0) as u64, "no script yet: no window capture, so no border in play");
    f.asked = 0;
    m.game_only_keep(&mut f, t + 9.0 + SIM_VICTORY_SOON_S + 1.0);
    assert_eq!(f.asked, 0, "no Victory screen came: not held open");
    // another mode's fight, and a replay, ask for nothing
    m.holosim_space = false;
    m.fight_ended_at = t + 20.0;
    m.game_only_keep(&mut f, t + 21.0);
    assert_eq!(f.asked, 0, "not a Sim room");
    (m.holosim_space, m.args.replay) = (true, true);
    m.game_only_keep(&mut f, t + 22.0);
    assert_eq!(f.asked, 0, "a replay has no capture to ask");
}

/// The capture showing the game window alone (Windows 11, or the window copy after a switch).
struct Window;
impl Frames for Window {
    fn grab(&mut self, _: usize, _: Option<f64>) -> Option<Image> { None }
    fn window(&mut self) -> Option<usize> { Some(1) }
    fn screen_copy(&self) -> bool { false }
}

/// A menu spotted in a copy of the screen, at `t` (the game window alone is asked for and waited on).
fn spotted_in_the_screen_copy(name: &str, t: f64) -> Meter {
    let mut m = meter(name);
    assert!(m.game_only(&mut ScreenCopy::default(), "menu", t), "waits for the game window");
    m.game_only_gone(true, Some("menu"), t);
    m
}

/// "A screen read from the screen copy is not in the game window": said only from reads of the game window's own
/// pictures, not the first one after the switch, and only after two of them in a row miss it. Before, it was said on
/// the first read handled after the switch, even one made from the screen copy (33 times since the Rust build, every
/// one false).
#[test]
fn a_screen_gone_from_the_game_window_is_said_after_two_window_reads() {
    let t = 1_800_000_000.0;
    let mut m = spotted_in_the_screen_copy("pacing-gone", t);
    // a read of the screen copy, handled after the switch: not judged
    m.game_only_gone(true, None, t + 0.2);
    assert!(m.pacing.game_only_spot.is_some());
    // the first read of the game window's pictures: not judged either, but the next read comes at once
    m.rs.last_results = 123.0;
    m.game_only_gone(false, None, t + 0.4);
    assert_eq!((m.pacing.gone_misses, m.pacing.gone_said), (0, 0));
    assert_eq!(m.rs.last_results, 0.0, "read again at once");
    // one miss: not yet
    m.game_only_gone(false, None, t + 0.6);
    assert_eq!((m.pacing.gone_misses, m.pacing.gone_said), (1, 0));
    // two in a row: said once, and the spot is done with
    m.game_only_gone(false, None, t + 0.8);
    assert_eq!(m.pacing.gone_said, 1);
    assert!(m.pacing.game_only_spot.is_none());
    m.game_only_gone(false, None, t + 1.0);
    m.game_only_gone(false, None, t + 1.2);
    assert_eq!(m.pacing.gone_said, 1, "not said again");
}

#[test]
fn a_screen_still_in_the_game_window_is_not_said_gone() {
    let t = 1_800_000_000.0;
    // the game window's picture shows it between two misses (the Party screen, still up): the count starts again
    let mut m = spotted_in_the_screen_copy("pacing-gone-still", t);
    m.game_only_gone(false, None, t + 0.4); // first after the switch
    m.game_only_gone(false, None, t + 0.6); // a miss
    m.game_only_gone(false, Some("menu"), t + 0.8); // there it is
    m.game_only_gone(false, None, t + 1.0);
    assert_eq!((m.pacing.gone_misses, m.pacing.gone_said), (1, 0));
    // back on the screen copy in between: its reads say nothing about the game window, and the count starts again
    let mut m = spotted_in_the_screen_copy("pacing-gone-copy", t);
    m.game_only_gone(false, None, t + 0.4);
    m.game_only_gone(false, None, t + 0.6);
    m.game_only_gone(true, None, t + 0.8);
    m.game_only_gone(false, None, t + 1.0); // first after a switch again
    m.game_only_gone(false, None, t + 1.2);
    assert_eq!((m.pacing.gone_misses, m.pacing.gone_said), (1, 0));
    // too long after the spot to say anything about it
    let mut m = spotted_in_the_screen_copy("pacing-gone-late", t);
    m.game_only_gone(false, None, t + 0.4);
    m.game_only_gone(false, None, t + 0.6);
    m.game_only_gone(false, None, t + alogs_meter::pacing::GAME_ONLY_GONE_S + 1.0);
    assert_eq!(m.pacing.gone_said, 0);
    assert!(m.pacing.game_only_spot.is_none(), "forgotten");
}

/// The read's own frame decides, not what the capture does when the read is handled: a read of a screen copy handled
/// once the capture shows the game window is still a screen-copy read.
#[test]
fn a_read_is_judged_by_its_own_frame() {
    let t = 1_800_000_000.0;
    let im = Image::new(alogs_pil::Mode::Rgb, 1920, 1080, &[0, 0, 0]);
    let none = alogs_meter::screens::Heads::new();
    let mut m = spotted_in_the_screen_copy("pacing-gone-own-frame", t);
    m.args.replay = false;
    m.ss.read_copy = true;
    for i in 1..=3 {
        m.read_screen(&mut Window, &mut Quiet, &im, t + 0.2 * f64::from(i), &none);
    }
    assert_eq!(m.pacing.gone_said, 0, "three reads of the screen copy, handled after the switch");
    assert!(m.pacing.game_only_spot.is_some());
    // the frame's source travels with the read from the look that made it
    m.frame_full = true;
    m.rs.last_results = 0.0;
    m.check_battle_data(&mut ScreenCopy::default(), &mut Quiet, &im, t + 1.0);
    assert!(m.ss.read_copy, "made from a screen copy");
    m.rs.last_results = 0.0;
    m.check_battle_data(&mut Window, &mut Quiet, &im, t + 1.2);
    assert!(!m.ss.read_copy, "made from the game window's picture");
}

/// One setting changed in a copy of the settings.
fn set(cfg: &alogs_pyval::Value, section: &str, key: &str, on: bool) -> alogs_pyval::Value {
    use alogs_pyval::Value;
    let mut top = cfg.as_dict().cloned().unwrap();
    let mut part = top.at(section).as_dict().cloned().unwrap_or_default();
    part.insert(Value::Str(key.into()), Value::Bool(on));
    top.insert(Value::Str(section.into()), Value::Dict(part));
    Value::Dict(top)
}

/// The game's icons with one Holo-gizmo known (the Gizmo tour starts only with the gizmo names loaded).
fn icons_with_a_gizmo(name: &str) -> alogs_reader::icons::Icons {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let meta = serde_json::json!({ "version": alogs_reader::icons::CACHE_VERSION, "heads": {}, "buffs": {}, "sim_gizmos": { "Energizing Bracer": ["0", "Core"] } });
    std::fs::write(dir.join("icons.json"), meta.to_string()).unwrap();
    alogs_reader::icons::Icons::open(&dir).expect("the test's icon cache")
}

/// aLogs takes control at the first sight of the Victory screen's Battle time line, the one thing every Victory
/// screen has, before the title, the team's cards or the coins are read: taken after the whole read, control came
/// 0.2 to 0.35 s later (and up to 1.5 s more while the right side slides in), time in which a press skips the screen.
#[test]
fn control_at_the_first_sight_of_the_victory_screen() {
    use alogs_meter::sim_tour::SIM_SPOT_WAIT;
    let mut m = meter("pacing-spot");
    m.args.replay = false;
    let t = 1_800_000_000.0;
    // the automation on; nothing is really held (no input hooks in a test)
    let on = set(&set(&m.args.cfg, "leaderboard", "submit", true), "leaderboard", "automation", true);
    m.args.cfg = set(&on, "results", "hold_inputs", false);
    m.icons = Some(icons_with_a_gizmo("pacing-spot-icons"));
    (m.holosim_space, m.fight_ended_at) = (true, t - 1.0);
    m.sim_spot_control(t, false);
    assert!(m.sim.ctl.is_none(), "the game isn't in front");
    m.sim.seen = t - 20.0;
    m.sim_spot_control(t, true);
    assert!(m.sim.ctl.is_none(), "a Victory screen left up, handled 20 s ago");
    (m.sim.seen, m.fight_ended_at) = (0.0, t - 60.0);
    m.sim_spot_control(t, true);
    assert!(m.sim.ctl.is_none(), "no fight just before it: not a new Victory screen");
    (m.fight_ended_at, m.holosim_space) = (t - 1.0, false);
    m.sim_spot_control(t, true);
    assert!(m.sim.ctl.is_none(), "not a Sim room");
    m.holosim_space = true;
    let saved = m.args.cfg.clone();
    m.args.cfg = set(&saved, "results", "sim_gizmos", false);
    m.sim_spot_control(t, true);
    assert!(m.sim.ctl.is_none(), "the Gizmo tour is switched off: nothing to take control for");
    m.args.cfg = saved;
    // a new Victory screen: control at once
    m.sim_spot_control(t, true);
    assert_eq!(m.sim.ctl.as_ref().map(|c| c.stage), Some(Some("spot")), "control from the Battle time line");
    m.sim_check(t + 1.0);
    assert!(m.sim.ctl.is_some(), "kept while the screen is being read");
    m.sim_check(t + SIM_SPOT_WAIT + 0.1);
    assert!(m.sim.ctl.is_none(), "no Victory read from it: control given back");
}
