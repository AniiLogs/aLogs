//! A fight's boss bar ends through the meter's own look, on drawn 1920x1080 bars with a fake clock: held once seen the
//! same, read on at when a bracket is hidden (no whole frame for it), a search result elsewhere not taken mid-fight, a
//! wrong first lock replaced by the true ends, the held place kept for the next fights, an Interlink fight never on
//! the open world's track, and the first reading at changed ends not counted as damage.

use alogs_meter::env::Env;
use alogs_meter::fights::{CONFIRM_GAP, Fight};
use alogs_meter::meter::{Args, ENDS_REPLACE_S, Frames, Meter, Out};
use alogs_pil::{Image, Mode};
use alogs_pyval::Value;
use alogs_reader::bar::{self, Bar, Spot, TRACK, WORLD_TRACK_RIGHT};
use alogs_reader::boss::{self, Row};
use alogs_reader::ocr::NoOcr;
use std::path::PathBuf;

const T0: f64 = 1_800_000_000.0;
const W: usize = 1920;
const H: usize = 1080;
const YC: i64 = 97;
const TH: i64 = 13;
const FILL: [u8; 3] = [250, 107, 105];
const EDGE: [u8; 3] = [48, 27, 24];
const WORLD: (i64, i64) = (613, 1307);
const RUSH: (i64, i64) = (582, 1338);

fn env(name: &str) -> Env {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let mut env = Env::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("res"), data);
    env.write_files = false;
    env.fixed_now.set(Some(T0));
    env
}

fn meter(name: &str) -> Meter {
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: false,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let mut m = Meter::new(env(name), args, Box::new(NoOcr), None);
    m.hwnd = Some(1);
    m
}

fn row(rush: bool) -> Row {
    let mut r = Row::new();
    for (k, v) in [("puppetId", if rush { "80000001" } else { "2100000001" }), ("table", "puppet_data"), ("bossTitle_en", "Test Boss"),
                   ("name_en", "Test Boss"), ("in_boss_rush", if rush { "yes" } else { "no" })] {
        r.insert(k.into(), v.into());
    }
    r
}

fn rect(im: &mut Image, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 3]) {
    for y in y0.max(0)..y1.min(im.height as i64) {
        for x in x0.max(0)..x1.min(im.width as i64) {
            let at = (y as usize * im.width + x as usize) * 3;
            im.data[at..at + 3].copy_from_slice(&c);
        }
    }
}

/// A boss bar as the game draws it (see alogs-reader's tests/bar_ends.rs), `fill` of its track filled.
fn frame(ends: (i64, i64), fill: f64) -> Image {
    let (l, r) = ends;
    let world = ends != RUSH;
    let span = (r - l) as f64;
    let mut im = Image::new(Mode::Rgb, W, H, &[20, 22, 30]);
    let (top, bot) = (YC - TH / 2, YC + TH / 2 + 1);
    rect(&mut im, l + 3, top - 2, r - 2, bot + 2, [15, 15, 18]);
    let tl = (l as f64 + TRACK.0 * span).round() as i64;
    let tr = (l as f64 + if world { WORLD_TRACK_RIGHT } else { TRACK.1 } * span).round() as i64;
    rect(&mut im, tl, top, tr, bot, EDGE);
    rect(&mut im, tl, top, tl + ((tr - tl) as f64 * fill) as i64, bot, FILL);
    if !world {
        let x0 = tr + 14;
        for k in 0..9 {
            rect(&mut im, x0 + k, top + 2 + k, x0 + k + 2, top + 3 + k, [245, 245, 245]);
            rect(&mut im, x0 + 8 - k, top + 2 + k, x0 + 10 - k, top + 3 + k, [245, 245, 245]);
        }
        rect(&mut im, x0 + 18, top + 1, x0 + 20, bot - 1, [245, 245, 245]);
    }
    let (b0, b1) = (YC - TH / 2, YC + 11);
    rect(&mut im, l - 1, b0, l + 1, b1, [245, 245, 245]);
    rect(&mut im, r, b0, r + 2, b1, [245, 245, 245]);
    im
}

/// The game's orange "250%" badge over the right bracket.
fn badge(mut im: Image, ends: (i64, i64)) -> Image {
    rect(&mut im, ends.1 - 20, YC - TH, ends.1 + 25, YC + TH, [255, 170, 60]);
    im
}

fn ends_of(b: &Bar) -> (i64, i64) {
    (b.left_bracket.round() as i64, (b.left_bracket + b.span).round() as i64)
}

/// One picture, and what each grab asked for (None: the whole frame).
struct Script {
    frame: Image,
    asked: Vec<Option<f64>>,
}
impl Frames for Script {
    fn grab(&mut self, _: usize, top: Option<f64>) -> Option<Image> {
        self.asked.push(top);
        Some(self.frame.clone())
    }
    fn window(&mut self) -> Option<usize> {
        Some(1)
    }
}

#[derive(Default)]
struct Quiet;
impl Out for Quiet {
    fn out(&mut self, _: &str, _: Value) {}
}

/// A fight on the bar in `im` (its lock as the finder gives it), looked at `looks` times 0.1 s apart from `t`.
fn fight_on(m: &mut Meter, frames: &mut Script, rush: bool, t: &mut f64, looks: usize) {
    m.frame_size = (W, H);
    m.bar = bar::find_bar(&frames.frame, false, 8);
    m.bar_seen = *t;
    let mut f = Fight::new(&m.env, "Test Boss", 50, row(rush), 1_000_000, rush, false, None, None);
    f.hit = true;
    m.fight = Some(f);
    run(m, frames, t, looks);
}

fn run(m: &mut Meter, frames: &mut Script, t: &mut f64, looks: usize) {
    for _ in 0..looks {
        *t += 0.1;
        m.env.fixed_now.set(Some(*t));
        m.step(frames, &mut Quiet);
    }
}

/// The ends are held after a few looks and their place is kept (replacing a wrong one there); then the game's badge
/// hides the right bracket: the fight reads on at the held ends from the top part alone, with no whole frame.
#[test]
fn held_ends_read_on_without_a_whole_frame() {
    let mut m = meter("ends-held");
    // a place kept by an older build at this bar, 20 px short at the right end
    m.bar_spots = vec![Spot { frame: (W, H), yc: YC as f64, thickness: TH as f64, left_bracket: 613.0, span: 674.0, track_left: 621.0 }];
    let mut frames = Script { frame: frame(WORLD, 0.9), asked: vec![] };
    let mut t = T0;
    fight_on(&mut m, &mut frames, false, &mut t, 5);
    let held = m.ends.held.clone().expect("held");
    assert_eq!(ends_of(&held), WORLD);
    assert!(m.ends.seen >= bar::HOLD_LOOKS);
    assert_eq!(m.bar_spots.len(), 1, "the wrong place at this bar is replaced: {:?}", m.bar_spots);
    assert_eq!(m.bar_spots[0].span, held.span);
    let saved = std::fs::read_to_string(m.env.data.join("bar_spot.json")).unwrap();
    assert!(saved.contains("\"span\": 694") && !saved.contains("674"), "{saved}");
    // the badge comes over the right bracket, and the bar goes on falling
    frames.frame = badge(frame(WORLD, 0.8), WORLD);
    frames.asked.clear();
    run(&mut m, &mut frames, &mut t, 10);
    assert!(frames.asked.iter().all(Option::is_some), "no whole frame for a hidden bracket: {:?}", frames.asked);
    assert!(bar::same_ends(m.bar.as_ref().unwrap(), &held));
    assert!(m.ends.held_reads >= 9, "read at the held ends: {}", m.ends.held_reads);
    let f = m.fight.as_ref().unwrap();
    assert!((f.seg_fill - 0.8).abs() < 0.01, "the fall was read: {}", f.seg_fill);
}

/// Mid-fight, the held bar is gone from its place and a search finds another bar 30 px in at each end (60 px at 4K):
/// it isn't taken, however often it is seen; the fight goes on at the held ends until the bar-gone grace.
#[test]
fn a_search_result_elsewhere_mid_fight_is_not_taken() {
    let mut m = meter("ends-mid");
    let mut frames = Script { frame: frame(WORLD, 0.9), asked: vec![] };
    let mut t = T0;
    fight_on(&mut m, &mut frames, false, &mut t, 5);
    let held = m.ends.held.clone().expect("held");
    // past the early part of the fight, the bar read all along
    for _ in 0..(ENDS_REPLACE_S as usize + 5) {
        run(&mut m, &mut frames, &mut t, 10);
    }
    frames.frame = frame((WORLD.0 + 30, WORLD.1 - 30), 0.9);
    run(&mut m, &mut frames, &mut t, 8);
    assert!(m.fight.is_some());
    assert!(bar::same_ends(m.bar.as_ref().unwrap(), &held), "still the held ends: {:?}", m.bar);
    assert!(m.ends.not_taken >= 5);
    assert!(!m.ends.replaced);
}

/// The fight started on a wrong lock (its right end 20 px inside the bar): the true ends are found and held, and the
/// held place is the true one.
#[test]
fn a_wrong_first_lock_gives_way_to_the_true_ends() {
    let mut m = meter("ends-first");
    let mut frames = Script { frame: frame(RUSH, 0.95), asked: vec![] };
    let mut t = T0;
    m.frame_size = (W, H);
    let truth = bar::find_bar(&frames.frame, false, 8).unwrap();
    let mut wrong = truth.clone();
    wrong.span -= 20.0;
    wrong.track_right -= 20;
    m.bar = Some(wrong);
    m.bar_seen = t;
    let mut f = Fight::new(&m.env, "Test Boss", 50, row(true), 1_000_000, true, false, None, None);
    f.hit = true;
    f.bar_geom = Some((truth.span - 20.0, YC as f64));
    m.fight = Some(f);
    run(&mut m, &mut frames, &mut t, 6);
    assert_eq!(ends_of(m.ends.held.as_ref().expect("held")), RUSH);
    assert_eq!(ends_of(m.bar.as_ref().unwrap()), RUSH);
    assert_eq!(m.fight.as_ref().unwrap().bar_geom.map(|g| g.0), Some(truth.span), "the fight's geometry is the held one");
    assert_eq!(m.bar_spots[0].span, truth.span);
}

/// An Interlink fight is never read on the open world's track: not from a lock found before it, nor from a search.
#[test]
fn an_interlink_fight_is_never_on_the_world_track() {
    let mut m = meter("ends-rush");
    let mut frames = Script { frame: frame(RUSH, 0.7), asked: vec![] };
    let mut t = T0;
    m.frame_size = (W, H);
    let found = bar::find_bar(&frames.frame, false, 8).unwrap();
    // a lock that the badge left a world bar (its right end more than 20% orange)
    let world = Bar { world: true, right_red: 0.3, track_right: (found.left_bracket + WORLD_TRACK_RIGHT * found.span).round() as i64, ..found.clone() };
    assert!(bar::as_bossrush_bar(&world).world);
    m.bar = Some(world.clone());
    m.bar_seen = t;
    m.fight = Some(Fight::new(&m.env, "Test Boss", 60, row(true), 1_000_000, true, false, None, None));
    run(&mut m, &mut frames, &mut t, 3);
    let b = m.bar.as_ref().unwrap();
    assert!(!b.world);
    assert_eq!(b.track_right, found.track_right);
    assert!(!m.bar_kind(world.clone()).world, "a search result too");
    // outside an Interlink fight the right end's colour decides as before
    m.fight = None;
    m.bossrush_space = true;
    assert!(m.bar_kind(world).world);
}

/// The first reading after the ends changed waits for one more look before it counts as damage.
#[test]
fn the_first_reading_at_changed_ends_waits_a_look() {
    let e = env("ends-moved");
    let grades = boss::load_grades(&e.res, None).unwrap();
    let mut f = Fight::new(&e, "Test Boss", 50, row(false), 100_000, false, false, None, None);
    let read = |f: &mut Fight, at: f64, fill: f64| f.update(&e, Some(fill), FILL, EDGE, None, None, &grades, 180.0, Some(at), true);
    read(&mut f, T0, 0.90);
    read(&mut f, T0 + 0.1, 0.89);
    assert_eq!(f.seg_fill, 0.89);
    // a look at other ends reads 0.80: not damage yet
    f.ends_moved = true;
    read(&mut f, T0 + 0.2, 0.80);
    assert_eq!(f.seg_fill, 0.89, "waits");
    assert!(!f.ends_moved, "the mark is for one look");
    // the next look at those ends reads it again: taken
    read(&mut f, T0 + 0.2 + CONFIRM_GAP, 0.80);
    assert_eq!(f.seg_fill, 0.80);
    // a fuller reading at changed ends is no damage either way
    f.ends_moved = true;
    read(&mut f, T0 + 0.5, 0.95);
    assert_eq!(f.seg_fill, 0.80);
    // and through the meter: a change of ends is seen once
    let mut m = meter("ends-moved-meter");
    let b = bar::find_bar(&frame(WORLD, 0.9), false, 8).unwrap();
    assert!(!m.ends_moved(&b), "the fight's first reading");
    assert!(!m.ends_moved(&b));
    let other = Bar { span: b.span - 20.0, ..b.clone() };
    assert!(m.ends_moved(&other));
    assert!(!m.ends_moved(&other));
}

/// A place kept that isn't centred on its frame (a wrong find: the bar always is) isn't loaded.
#[test]
fn an_uncentred_place_is_not_loaded() {
    let e = env("ends-spots");
    let kept = r#"[{"yc": 194, "track_left": 1243, "left_bracket": 1227, "span": 1367, "thickness": 25, "fight": true, "frame": [3840, 2160]},
                  {"yc": 194, "track_left": 1181, "left_bracket": 1164, "span": 1512, "thickness": 27, "fight": true, "frame": [3840, 2160]}]"#;
    std::fs::write(e.data.join("bar_spot.json"), kept).unwrap();
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: false,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let m = Meter::new(e, args, Box::new(NoOcr), None);
    assert_eq!(m.bar_spots.len(), 1);
    assert_eq!(m.bar_spots[0].span, 1512.0);
}
