//! A fight counts from its bar's first sight (what the bar read before its boss was named is put through the fight),
//! and a reading far below the last must stand before it is taken.

use alogs_meter::env::Env;
use alogs_meter::fights::{CONFIRM_GAP, EARLY_GAP_S, Fight, LOW_CONFIRM_S};
use alogs_meter::meter::{Args, Meter};
use alogs_reader::bar::Bar;
use alogs_reader::boss::{self, Row};
use alogs_reader::ocr::NoOcr;
use std::path::PathBuf;

const RED: [u8; 3] = [250, 107, 105];
const EDGE: [u8; 3] = [48, 27, 24];
const T: f64 = 1_800_000_000.0;

fn env(name: &str) -> Env {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let mut env = Env::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("res"), data);
    env.write_files = false;
    env.fixed_now.set(Some(T));
    env
}

/// A world boss (not Interlink) with 100,000 HP for the tests' sums.
fn row() -> Row {
    let mut r = Row::new();
    for (k, v) in [("puppetId", "2100000001"), ("table", "puppet_data"), ("bossTitle_en", "Test Boss"), ("name_en", "Test Boss"), ("in_boss_rush", "no")] {
        r.insert(k.into(), v.into());
    }
    r
}

fn fight(env: &Env) -> Fight {
    Fight::new(env, "Test Boss", 50, row(), 100_000, false, false, None, None)
}

fn bar(span: f64, yc: i64) -> Bar {
    Bar { yc, left_bracket: 1226.0, span, brackets_found: true, world: true, right_red: 0.0, track_left: 1242, track_right: 2605, thickness: 26, frame: (3840, 2160) }
}

#[test]
fn a_much_lower_reading_must_stand_before_it_is_taken() {
    let env = env("early-low");
    let grades = boss::load_grades(&env.res, None).unwrap();
    let read = |f: &mut Fight, at: f64, fill: f64| f.update(&env, Some(fill), RED, EDGE, None, None, &grades, 180.0, Some(at), true);
    // as in the Python: a second reading 0.09 s after the first is enough
    let mut f = fight(&env);
    assert_eq!(f.low_confirm, CONFIRM_GAP);
    read(&mut f, T, 1.0);
    read(&mut f, T + 0.1, 0.40);
    assert_eq!(f.seg_fill, 1.0, "one low reading waits");
    read(&mut f, T + 0.2, 0.40);
    assert_eq!(f.seg_fill, 0.40, "the second, 0.1 s later, is taken");
    // the live meter: it must stand LOW_CONFIRM_S, every reading in between agreeing
    let mut f = fight(&env);
    f.low_confirm = LOW_CONFIRM_S;
    read(&mut f, T, 1.0);
    read(&mut f, T + 0.1, 0.40);
    read(&mut f, T + 0.2, 0.40);
    assert_eq!(f.seg_fill, 1.0, "two low readings 0.1 s apart still wait");
    read(&mut f, T + 0.25, 0.99);
    assert_eq!(f.seg_fill, 0.99, "the bar reads full again: the low reading was wrong, and is gone");
    assert_eq!(f.damage, 1_000);
    // a real big hit stands, and is taken 0.3 s after it was first read
    read(&mut f, T + 0.30, 0.40);
    read(&mut f, T + 0.40, 0.40);
    read(&mut f, T + 0.50, 0.39);
    assert_eq!(f.seg_fill, 0.99, "0.2 s in: still waiting");
    read(&mut f, T + 0.61, 0.39);
    assert_eq!(f.seg_fill, 0.39, "it stood 0.3 s: taken");
    assert_eq!(f.damage, 61_000);
}

#[test]
fn a_fight_counts_from_its_bars_first_sight() {
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: false,
                      no_submit: true, no_debug_frames: true, no_overlay: false, no_auto_click: true, player_log: PathBuf::new() };
    let mut m = Meter::new(env("early-replay"), args, Box::new(NoOcr), None);
    let b = bar(1388.0, 194);
    // the bar is up for a second before its boss's name is read, and a hit lands in that second
    m.keep_early(&b, T - 1.0, 1.0, RED, EDGE);
    m.keep_early(&b, T - 0.8, 1.0, RED, EDGE);
    m.keep_early(&b, T - 0.6, 0.90, RED, EDGE);
    m.keep_early(&b, T - 0.4, 0.90, RED, EDGE);
    assert_eq!(m.early.len(), 4);
    let mut f = fight(&m.env);
    m.replay_early(&mut f);
    assert!(m.early.is_empty(), "the readings are the fight's now");
    assert_eq!(f.created, T - 1.0, "the fight began when its bar was first seen");
    assert!(f.hit, "the hit before the name was read is the fight's first hit");
    assert!((f.t0 - (T - 0.7)).abs() < 1e-6, "its clock starts at that hit: {}", f.t0 - T);
    assert_eq!(f.damage, 10_000);
    // without them the fight begins now, as before
    let f = fight(&m.env);
    assert_eq!(f.created, T);
    assert!(!f.hit);
    // another bar's readings are not this one's: a different size or place starts the keeping over, and so does a gap
    m.keep_early(&b, T, 1.0, RED, EDGE);
    m.keep_early(&bar(1255.0, 194), T + 0.1, 0.5, RED, EDGE);
    assert_eq!(m.early.len(), 1, "a bar of another size");
    m.keep_early(&bar(1255.0, 240), T + 0.2, 0.5, RED, EDGE);
    assert_eq!(m.early.len(), 1, "a bar at another height");
    m.keep_early(&bar(1255.0, 240), T + 0.3, 0.5, RED, EDGE);
    assert_eq!(m.early.len(), 2, "the same bar again");
    m.keep_early(&bar(1255.0, 240), T + 0.3 + EARLY_GAP_S + 0.1, 0.5, RED, EDGE);
    assert_eq!(m.early.len(), 1, "after a gap it is a new sight of the bar");
    // an Interlink fight starts with its bar and takes its time from the game's timer: nothing is put through it
    let mut rush = Fight::new(&m.env, "Test Boss", 50, row(), 100_000, true, false, None, None);
    m.replay_early(&mut rush);
    assert_eq!(rush.created, T);
    assert!(m.early.is_empty());
}

#[derive(Default)]
struct Sent(Vec<(String, alogs_pyval::Value)>);
impl alogs_meter::meter::Out for Sent {
    fn out(&mut self, kind: &str, data: alogs_pyval::Value) {
        self.0.push((kind.to_string(), data));
    }
}

fn live_meter(name: &str) -> Meter {
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: false,
                      no_submit: true, no_debug_frames: true, no_overlay: false, no_auto_click: true, player_log: PathBuf::new() };
    Meter::new(env(name), args, Box::new(NoOcr), None)
}

fn spot_of(b: &Bar) -> alogs_reader::bar::Spot {
    alogs_reader::bar::Spot { frame: b.frame, yc: b.yc as f64, thickness: b.thickness as f64, left_bracket: b.left_bracket, span: b.span, track_left: b.track_left as f64 }
}

/// The overlay is told of a boss bar before its fight starts, so the HP % shows from the bar's first look: only at a
/// place where a fight started before (most of what the finder takes for a bar is no bar), never for Interlink (its %
/// is of all the bar's layers), and it is told once when that bar goes again with no fight.
#[test]
fn the_overlay_hears_of_a_bar_before_its_fight() {
    let mut m = live_meter("early-bar");
    let mut out = Sent::default();
    let b = bar(1388.0, 194);
    m.frame_size = b.frame;
    m.emit_bar(&mut out, &b, 0.97, RED);
    assert!(out.0.is_empty(), "no fight ever started on a bar here: it may be no bar");
    m.bar_spots.push(spot_of(&b));
    m.emit_bar(&mut out, &b, 0.97, [120, 200, 240]);
    assert!(out.0.is_empty(), "not the bar's own red: nothing to show");
    m.bossrush_space = true;
    m.emit_bar(&mut out, &b, 0.97, RED);
    assert!(out.0.is_empty(), "an Interlink room");
    m.bossrush_space = false;
    m.emit_bar(&mut out, &bar(1388.0, 400), 0.97, RED);
    assert!(out.0.is_empty(), "a bar somewhere else on the screen");
    m.emit_bar(&mut out, &b, 0.97, RED);
    assert_eq!(out.0.len(), 1);
    let (kind, d) = &out.0[0];
    assert_eq!(kind, "bar");
    assert_eq!(d.get("fill").as_f64(), Some(0.97));
    assert_eq!(d.get("bar").as_list().map(Vec::len), Some(3), "where the bar is, for the label's place");
    // the bar is still up: nothing more is said
    m.bar = Some(b.clone());
    m.bar_gone_check(&mut out);
    assert_eq!(out.0.len(), 1);
    // it goes without a fight: the label is taken away, once
    m.bar = None;
    m.bar_gone_check(&mut out);
    m.bar_gone_check(&mut out);
    assert_eq!(out.0.iter().filter(|(k, _)| k == "bar_gone").count(), 1);
    // a fight came of it: the fight's own labels take over, nothing is taken away
    m.emit_bar(&mut out, &b, 0.97, RED);
    m.bar = Some(b);
    m.fight = Some(fight(&m.env));
    m.bar_gone_check(&mut out);
    assert_eq!(out.0.iter().filter(|(k, _)| k == "bar_gone").count(), 1, "no second one");
}

/// bar_spot.json holds only places where a fight started: places kept by older builds (every find, most of them no
/// bar) are not taken, since a label now shows at a kept place at once.
#[test]
fn only_a_fights_place_is_kept_for_the_labels() {
    let e = env("early-spots");
    let old = r#"[{"yc": 40, "track_left": 900, "left_bracket": 880, "span": 1711, "thickness": 30, "frame": [3840, 2160]},
                 {"yc": 194, "track_left": 1242, "left_bracket": 1226, "span": 1388, "thickness": 26, "frame": [3840, 2160], "fight": true}]"#;
    std::fs::write(e.data.join("bar_spot.json"), old).unwrap();
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: false,
                      no_submit: true, no_debug_frames: true, no_overlay: false, no_auto_click: true, player_log: PathBuf::new() };
    let m = Meter::new(e, args, Box::new(NoOcr), None);
    assert_eq!(m.bar_spots.len(), 1, "the place with no fight to its name is dropped");
    assert_eq!(m.bar_spots[0].yc, 194.0);
    assert!(m.known_spot(&bar(1388.0, 194)));
    assert!(!m.known_spot(&bar(1388.0, 40)));
}
