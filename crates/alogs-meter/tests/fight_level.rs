//! A fight's level from the bar's title: a level with a symbol glued to its digits is unread, a fight starts on
//! agreeing reads (one when it is the level this boss had here a moment ago, three when it differs from it), and a
//! level read wrong is put right in the fight by agreeing reads, its damage following. Made-up reads of a boss from the
//! shipped catalog, a fake clock.

use alogs_meter::env::Env;
use alogs_meter::fights::LEVEL_FIX_READS;
use alogs_meter::meter::{Args, Meter, Out};
use alogs_pyval::Value;
use alogs_reader::boss;
use alogs_reader::ocr::NoOcr;
use alogs_reader::parse::parse_title;
use std::path::PathBuf;

const T: f64 = 1_800_000_000.0;
const RED: [u8; 3] = [248, 105, 105];
const EDGE: [u8; 3] = [60, 40, 40];

struct Quiet;
impl Out for Quiet {
    fn out(&mut self, _: &str, _: Value) {}
}

fn meter(name: &str) -> Meter {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let mut env = Env::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("res"), data);
    env.write_files = false;
    env.fixed_now.set(Some(T));
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: false,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    Meter::new(env, args, Box::new(NoOcr), None)
}

/// The HP of the fight's boss at `level`.
fn hp(m: &Meter, level: i64) -> i64 {
    boss::boss_max_hp(&m.fight.as_ref().unwrap().row, level, None).unwrap()
}

/// A title read at `t`.
fn read(m: &mut Meter, t: f64, text: &str) {
    m.env.fixed_now.set(Some(t));
    m.identify(&mut Quiet, Some(text), None, None);
}

/// The fight read at `fill` at `t`.
fn fill(m: &mut Meter, t: f64, fill: f64) {
    let grades = boss::load_grades(&m.env.res, None).unwrap();
    m.env.fixed_now.set(Some(t));
    m.fight.as_mut().unwrap().update(&m.env, Some(fill), RED, EDGE, None, None, &grades, 180.0, Some(t), true);
}

#[test]
fn a_symbol_glued_to_the_level_is_unread() {
    for t in ["Lv.6% Alpha Stellarys", "Lv.6$ Alpha", "Lv.65% Alpha"] {
        assert_eq!(parse_title(Some(t)).0, None, "{t}");
    }
    assert_eq!(parse_title(Some("Lv.65 Alpha Stellarys")).0, Some(65));
    assert_eq!(parse_title(Some("Lv. 6 Alpha Stellarys")).0, Some(6));
    assert_eq!(parse_title(Some("Lv.6S Alpha")).0, Some(65), "an S read for a 5, as before");
}

/// Three fights of one boss in one place: two at Lv 65, then the third's first title reads "Lv.6% ..." (unread), a
/// single "Lv. 6" (another level than the boss had here: not believed alone), then "Lv. 65": the fight is Lv 65.
#[test]
fn the_third_fight_of_three_is_not_lv_6() {
    let mut m = meter("level-three-fights");
    let mut t = T;
    for _ in 0..2 {
        read(&mut m, t, "Lv. 65 Alpha Stellarys");
        read(&mut m, t + 1.0, "Lv. 65 Alpha Stellarys");
        assert_eq!(m.fight.as_ref().map(|f| f.level), Some(65));
        m.end_fight(&mut Quiet, "boss bar gone");
        t += 120.0;
    }
    read(&mut m, t, "Lv.6% Alpha Stellarys *");
    assert!(m.fight.is_none(), "a glued symbol: unread");
    read(&mut m, t + 0.3, "Lv. 6 Alpha Stellarys");
    assert!(m.fight.is_none(), "one read of another level than this boss had here: not believed");
    read(&mut m, t + 1.0, "Lv. 65 Alpha Stellarys");
    let f = m.fight.as_ref().expect("started at the level it had here");
    assert_eq!((f.level, f.max_hp), (65, hp(&m, 65)));
}

/// A boss that really is Lv 6, read so again and again: Lv 6 it stays.
#[test]
fn a_genuine_low_level_stays() {
    let mut m = meter("level-low");
    read(&mut m, T, "Lv. 6 Alpha Stellarys");
    assert!(m.fight.is_none(), "one read: not yet");
    read(&mut m, T + 1.0, "Lv. 6 Alpha Stellarys");
    assert_eq!(m.fight.as_ref().map(|f| f.level), Some(6));
    for k in 0..10 {
        read(&mut m, T + 2.0 + k as f64, "Lv. 6 Alpha Stellarys");
    }
    let f = m.fight.as_ref().unwrap();
    assert_eq!((f.level, f.max_hp), (6, hp(&m, 6)));
}

/// A fight started on a wrong level is put right by LEVEL_FIX_READS agreeing reads of the right one: its HP is the
/// right level's and its damage follows the fill, the same as a fight read right from the start.
#[test]
fn a_level_corrected_mid_fight_gives_the_same_damage() {
    let fills = [1.0, 0.97, 0.93, 0.90, 0.86, 0.81, 0.77, 0.74, 0.70, 0.66];
    // read right from the start
    let mut right = meter("level-right");
    read(&mut right, T, "Lv. 65 Alpha Stellarys");
    read(&mut right, T + 1.0, "Lv. 65 Alpha Stellarys");
    // read wrong at the start (two agreeing misreads), then put right
    let mut wrong = meter("level-wrong");
    read(&mut wrong, T, "Lv. 6 Alpha Stellarys");
    read(&mut wrong, T + 1.0, "Lv. 6 Alpha Stellarys");
    assert_eq!(wrong.fight.as_ref().unwrap().level, 6);
    for (i, f) in fills.iter().enumerate() {
        let t = T + 2.0 + i as f64;
        fill(&mut right, t, *f);
        fill(&mut wrong, t, *f);
        if (3..3 + LEVEL_FIX_READS).contains(&i) {
            read(&mut wrong, t + 0.5, "Lv. 65 Alpha Stellarys");
        }
    }
    let (a, b) = (right.fight.as_ref().unwrap(), wrong.fight.as_ref().unwrap());
    assert_eq!((b.level, b.max_hp), (65, hp(&wrong, 65)), "corrected");
    assert_eq!(a.damage, b.damage, "the same damage as read right from the start");
    assert!((a.avg - b.avg).abs() < 1.0, "{} {}", a.avg, b.avg);
}
