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

/// The level read by the game's own digits (`title_font`, set on the look the title's text was cropped from) and by
/// the text reader: agreeing, one read settles it; disagreeing, it is unread and read again; the digits alone are
/// voted like the text reader alone.
#[test]
fn the_font_digits_and_the_text_reader() {
    let font = |m: &mut Meter, t: f64, l: Option<i64>| m.title_font = Some((l, t));
    // agreeing: one read starts the fight, with no history
    let mut m = meter("level-font-agree");
    font(&mut m, T, Some(65));
    read(&mut m, T, "Lv. 65 Alpha Stellarys");
    assert_eq!(m.fight.as_ref().map(|f| f.level), Some(65));
    // disagreeing: unread, however often
    let mut m = meter("level-font-disagree");
    for k in 0..4 {
        let t = T + k as f64;
        font(&mut m, t, Some(65));
        read(&mut m, t, "Lv. 6 Alpha Stellarys");
    }
    assert!(m.fight.is_none(), "the readers disagree: unread");
    // the digits alone (the text reader read no level): two agreeing looks
    let mut m = meter("level-font-alone");
    font(&mut m, T, Some(65));
    read(&mut m, T, "Lv.6% Alpha Stellarys");
    assert!(m.fight.is_none());
    font(&mut m, T + 1.0, Some(65));
    read(&mut m, T + 1.0, "Lv.6% Alpha Stellarys");
    assert_eq!(m.fight.as_ref().map(|f| f.level), Some(65));
    // no font on this PC (no digits read): the text reader's vote, as before
    let mut m = meter("level-no-font");
    read(&mut m, T, "Lv. 65 Alpha Stellarys");
    assert!(m.fight.is_none());
    read(&mut m, T + 1.0, "Lv. 65 Alpha Stellarys");
    assert_eq!(m.fight.as_ref().map(|f| f.level), Some(65));
}

/// An Interlink boss of the shipped catalog: its row.
fn rush_row(m: &Meter) -> boss::Row {
    m.catalog.iter().find(|r| r.get("in_boss_rush").is_some_and(|v| v == "yes") && r.get("fixed_hp_rate").is_some_and(|v| !v.is_empty())).unwrap().clone()
}

/// The fight read at bar counter `xn` and `fill` at `t` (an Interlink fight's xN bars).
fn rush_fill(m: &mut Meter, t: f64, xn: i64, fill: f64) {
    let grades = boss::load_grades(&m.env.res, None).unwrap();
    m.env.fixed_now.set(Some(t));
    m.fight.as_mut().unwrap().update(&m.env, Some(fill), RED, EDGE, Some(xn), None, &grades, 180.0, Some(t), true);
}

/// An Interlink fight started on a carried level the title contradicts (60 for a Lv 65 boss): the title read at 65 by
/// both readers puts it right in its first seconds, and it ends with the same damage as a fight started at 65.
#[test]
fn an_interlink_fight_on_a_wrong_carried_level_is_put_right() {
    let looks: Vec<(i64, f64)> = (0..30).map(|k| (10 - k / 6, 1.0 - (k % 6) as f64 * 0.15)).collect();
    let mut right = meter("level-rush-right");
    let row = rush_row(&right);
    let title = row.get("bossTitle_en").cloned().unwrap();
    right.start_fight(&mut Quiet, row.clone(), 65, &title, true, "test");
    let mut wrong = meter("level-rush-wrong");
    wrong.args.boss_id = row.get("puppetId").cloned();
    wrong.start_fight(&mut Quiet, row.clone(), 60, &title, true, "test");
    for (k, (xn, f)) in looks.iter().enumerate() {
        let t = T + 0.5 * k as f64;
        rush_fill(&mut right, t, *xn, *f);
        rush_fill(&mut wrong, t, *xn, *f);
        if k == 4 {
            wrong.title_font = Some((Some(65), t));
            read(&mut wrong, t, &format!("Lv. 65 {title}"));
        }
    }
    let (a, b) = (right.fight.as_ref().unwrap(), wrong.fight.as_ref().unwrap());
    assert_eq!((b.level, b.max_hp), (65, a.max_hp), "put right");
    assert!(a.damage > 0);
    assert_eq!(a.damage, b.damage, "the same damage as started right");
    assert_eq!(wrong.rush_level, Some(65), "the carried level refreshed");
}

/// A two-boss fight corrected after its hand-over: the first boss counts its whole HP at the right level, the second's
/// damage scales with its HP; the same as a fight read right from the start.
#[test]
fn a_two_boss_fight_corrected_after_the_hand_over() {
    let m = meter("level-two-boss");
    let rows: Vec<boss::Row> = m.catalog.iter().filter(|r| r.get("in_boss_rush").is_some_and(|v| v == "yes") && r.get("fixed_hp_rate").is_some_and(|v| !v.is_empty())).take(2).cloned().collect();
    let hp = |r: &boss::Row, l: i64| boss::boss_max_hp(r, l, None).unwrap();
    let grades = boss::load_grades(&m.env.res, None).unwrap();
    let fight = |level: i64| {
        let mut f = alogs_meter::fights::Fight::new(&m.env, "Boss One", level, rows[0].clone(), hp(&rows[0], level), true, false, None, Some(1));
        for k in 0..6 {
            f.update(&m.env, Some(1.0 - k as f64 * 0.1), RED, EDGE, Some(3), None, &grades, 180.0, Some(T + k as f64 * 0.5), true);
        }
        f.next_boss(&m.env, rows[1].clone(), "test");
        for k in 0..6 {
            f.update(&m.env, Some(1.0 - k as f64 * 0.12), RED, EDGE, Some(5), None, &grades, 180.0, Some(T + 5.0 + k as f64 * 0.5), true);
        }
        f
    };
    let right = fight(65);
    let mut wrong = fight(60);
    wrong.correct_level(65, &[hp(&rows[0], 65), hp(&rows[1], 65)], None);
    assert_eq!((wrong.max_hp, wrong.done_hp), (right.max_hp, right.done_hp));
    assert!((wrong.damage - right.damage).abs() <= 1, "{} {}", wrong.damage, right.damage);
}
