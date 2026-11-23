//! Parity with aLogs B16 `fights.py` (Fight, Timer, BreakTracker) on scripted fights (tools/gen_fight_goldens.py).
//! Needs reference/ (the shipped grade table and boss_break.json); skipped without it.

use alogs_meter::env::Env;
use alogs_meter::fights::{BREAK_CONFIRM_S, BreakTracker, Fight, Timer};
use alogs_reader::bar::BreakRead;
use alogs_reader::boss::{self, Row, Schedule};
use serde_json::{Value as J, json};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}
fn res() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/aLogs/bossbar")
}

fn row(pid: &str, title: &str, prop: &str) -> Row {
    let mut r = Row::new();
    for (k, v) in [
        ("puppetId", pid),
        ("table", "puppet_data"),
        ("bossTitle_en", title),
        ("name_en", title),
        ("species_hp", "95"),
        ("prop_species_adj", "0"),
        ("propId", prop),
        ("fixed_hp_rate", r#"{"1": 3101, "2": 8.8, "3": 1.176, "4": 1.2, "5": 0}"#),
        ("in_boss_rush", "yes"),
    ] {
        r.insert(k.into(), v.into());
    }
    r
}

fn num(v: f64) -> J {
    json!(v)
}

fn same(a: &J, b: &J) -> bool {
    match (a, b) {
        (J::Number(x), J::Number(y)) => x.as_f64() == y.as_f64(),
        (J::Array(x), J::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        _ => a == b,
    }
}

fn state(f: &Fight) -> J {
    let opt_f = |v: Option<f64>| v.map_or(J::Null, num);
    json!([
        f.seg, f.seg_fill, f.damage, f.elapsed, f.avg, f.recent, f.limit, f.hit, f.t0, f.refill_pending, f.phases.len(),
        f.max_hp, f.boss_hp, f.done_hp, f.started_at_top, f.low_pending.map(|l| json!([l.0, l.1])), f.first_damage,
        f.seg_info.as_ref().map(|s| json!([s.grade, s.size_hp, s.left_hp])), f.order, f.column,
        f.phases.iter().map(|p| json!([p.damage, p.defeated, p.start_s, opt_f(p.end_s), opt_f(p.hit_s)])).collect::<Vec<_>>()
    ])
}

#[test]
fn scripted_fights_match_reference() {
    if !res().exists() {
        eprintln!("skipped: reference/ not cloned");
        return;
    }
    let g: J = serde_json::from_str(&std::fs::read_to_string(dir().join("fight_goldens.json")).unwrap()).unwrap();
    let prop = g["prop"].as_str().unwrap();
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("meter-data");
    let _ = std::fs::create_dir_all(&data);
    let mut env = Env::new(res(), data);
    env.schedule = Schedule::new(dir().join("cycles.json"));
    env.write_files = false;
    let grades = boss::load_grades(&env.res, None).unwrap();
    for c in g["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let a = &c["args"];
        env.fixed_now.set(Some(1_800_000_000.0));
        let r = |v: &J| row(v["puppetId"].as_str().unwrap(), v["bossTitle_en"].as_str().unwrap(), prop);
        let mut f = Fight::new(
            &env,
            a[0].as_str().unwrap(),
            a[1].as_i64().unwrap(),
            r(&a[2]),
            a[3].as_i64().unwrap(),
            a[4].as_bool().unwrap(),
            false,
            if a[5].is_null() { None } else { Some(r(&a[5])) },
            a[6].as_i64(),
        );
        let slide = c["slide"].as_bool().unwrap();
        for (i, (fr, want)) in c["frames"].as_array().unwrap().iter().zip(c["states"].as_array().unwrap()).enumerate() {
            let at = fr["at"].as_f64().unwrap();
            env.fixed_now.set(Some(at));
            let rgb: Vec<u8> = fr["rgb"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
            f.update(&env, fr["fill"].as_f64(), [rgb[0], rgb[1], rgb[2]], [40, 30, 30], fr["xn"].as_i64(), fr["timer"].as_i64(), &grades, 180.0, Some(at), slide);
            if fr.get("defeat").and_then(J::as_bool) == Some(true) {
                f.mark_defeated(&env, "test");
            }
            let got = state(&f);
            assert!(same(&got, want), "{name} frame {i} {fr}:\n got {got}\nwant {want}");
        }
        f.close(&env);
    }
}

#[test]
fn timers_and_break_gauges_match_reference() {
    let g: J = serde_json::from_str(&std::fs::read_to_string(dir().join("fight_goldens.json")).unwrap()).unwrap();
    for seq in g["timers"].as_array().unwrap() {
        let mut tm = Timer::new(180.0);
        for s in seq.as_array().unwrap() {
            let t = s[0].as_f64().unwrap();
            let res = tm.feed(s[1].as_f64(), t);
            let got = json!([t, s[1], res, tm.val, tm.at, tm.predict(t)]);
            assert!(same(&got, s), "timer: got {got}, want {s}");
        }
    }
    // (the reference counts a break on its first empty look and a refill on the first jump up; here a break needs a
    // look BREAK_CONFIRM_S on that is still empty, and a jump up a second reading holding it: that look is fed after
    // the reference's, the break dated from the first empty look, and every row is compared after it)
    for seq in g["breaks"].as_array().unwrap() {
        let mut b = BreakTracker::new(Some(1234.5));
        for s in seq.as_array().unwrap() {
            let t = s[0].as_f64().unwrap();
            let kind = None;
            let (before, phase) = (b.breaks, b.phase);
            b.feed(s[1].as_f64(), kind, t);
            if s[6].as_i64() == Some(before + 1) {
                assert_eq!((b.breaks, b.phase), (before, "drain"), "not counted on its first empty look at {t}");
                b.feed(s[1].as_f64(), kind, t + BREAK_CONFIRM_S);
                assert_eq!((b.breaks, b.phase), (before + 1, "broken"), "counted on the look that confirms it");
                continue;
            }
            if phase == "drain" && s[3] == "refill" {
                assert_eq!(b.phase, "drain", "a jump up waits for the next reading at {t}");
                b.feed(s[1].as_f64(), kind, t);
            }
            let i = b.info(t);
            let got = json!([t, s[1], i.left, i.phase, i.rate, i.rate_now, i.breaks, i.last_after]);
            assert!(same(&got, s), "break: got {got}, want {s}");
        }
    }
}

/// The bar's red, and grey where a skill animation hid the HUD.
const RED: [u8; 3] = [240, 110, 100];
const GREY: [u8; 3] = [120, 120, 120];
const GOLD: [u8; 3] = [250, 210, 90];

/// One look: the gauge, its line's colour, the broken look, the lock icon.
fn look(gauge: Option<f64>, kind: Option<&'static str>, broken: bool, locked: bool) -> BreakRead {
    BreakRead { gauge, kind, broken, badge: broken, locked: Some(locked), unknown: None }
}
fn white(g: f64) -> BreakRead {
    look(Some(g), Some("white"), false, false)
}
/// Fed as the open world's or Holo-Battle Sim's bar (`by_look`), the bar read.
fn see(b: &mut BreakTracker, r: BreakRead, t: f64) {
    b.feed_look(&r, RED, true, t);
}

/// A Holo-Battle Sim fight: a skill animation hides the HUD (no bar read), a golden flash where the bar was, the bar
/// comes back half full: no break. Later the real one: the drain to a sliver (no broken look: just a low reading),
/// then the broken look: one break, at that look.
#[test]
fn a_hidden_bar_and_a_flash_are_no_break() {
    let mut b = BreakTracker::new(Some(1000.0));
    see(&mut b, white(0.384), 16.88);
    for t in [17.16, 17.54, 18.14] {
        b.feed_look(&white(0.384), GREY, true, t);
    }
    for t in [18.32, 19.42] {
        b.feed_look(&look(Some(0.0), None, true, false), GOLD, true, t);
    }
    assert_eq!((b.breaks, b.phase), (0, "drain"));
    see(&mut b, white(0.353), 19.84);
    let mut t = 19.84;
    let mut g: f64 = 0.353;
    while g > 0.05 {
        t += 0.1;
        g -= 0.0072;
        see(&mut b, white(g.max(0.046)), t);
    }
    assert_eq!(b.breaks, 0, "no break while it drains");
    for (t, g) in [(24.49, 0.046), (24.59, 0.009), (24.7, 0.009), (24.9, 0.005)] {
        see(&mut b, look(Some(g), Some("orange"), false, false), t);
    }
    assert_eq!((b.breaks, b.phase), (0, "drain"), "no broken look: not broken");
    see(&mut b, look(Some(0.0), None, true, false), 25.1);
    assert_eq!((b.breaks, b.phase), (1, "broken"), "the broken look at 25.1");
    assert_eq!(b.last_after, None);
    for i in 0..200 {
        see(&mut b, look(Some(0.0), None, true, false), 25.2 + i as f64 * 0.1);
    }
    assert_eq!(b.breaks, 1, "once");
}

/// The fight of 22 Nov, 21:07: the gauge a sliver of 0.5 to 0.8% for 3 s with no broken look (the boss not broken
/// yet): a low reading, no break. Then the broken look: one break, dated at that look. A sliver that never gets the
/// broken look is never a break.
#[test]
fn a_sliver_without_the_broken_look_is_no_break() {
    let mut b = BreakTracker::new(Some(1000.0));
    see(&mut b, look(Some(0.03), Some("orange"), false, false), 43.5);
    let mut t = 43.92;
    for g in [0.008, 0.007, 0.006, 0.005].iter().cycle().take(32) {
        see(&mut b, look(Some(*g), Some("orange"), false, false), t);
        assert_eq!((b.breaks, b.phase), (0, "drain"), "at {t}");
        t += 0.1;
    }
    assert_eq!(b.left, Some(0.005), "shown as it is");
    see(&mut b, look(Some(0.0), None, true, false), 47.3);
    assert_eq!((b.breaks, b.phase), (1, "broken"));
    assert!(b.last_after.is_none_or(|a| a > 47.0 - 43.5), "dated at the broken look");
    let mut c = BreakTracker::new(Some(1000.0));
    see(&mut c, look(Some(0.03), Some("orange"), false, false), 0.0);
    for k in 0..200 {
        see(&mut c, look(Some(0.006), Some("orange"), false, false), 0.1 + k as f64 * 0.1);
    }
    assert_eq!((c.breaks, c.phase), (0, "drain"));
}

/// After the refill, "unbroken again" at 99.6%; one look then reads 93.7% with the broken look: no second refill
/// (the refill comes only from a break), and each line is logged once.
#[test]
fn no_refill_straight_after_the_reset() {
    let mut b = BreakTracker::new(Some(1000.0));
    see(&mut b, look(Some(0.1), Some("white"), false, false), 0.0);
    see(&mut b, look(Some(0.0), None, true, false), 1.0);
    see(&mut b, look(Some(0.034), Some("white"), true, false), 5.0);
    see(&mut b, look(Some(0.971), Some("white"), true, false), 9.0);
    see(&mut b, look(Some(0.996), Some("white"), false, false), 62.46);
    assert_eq!(b.phase, "drain");
    see(&mut b, look(Some(0.937), Some("white"), true, false), 62.67);
    see(&mut b, look(Some(0.996), Some("white"), false, false), 62.79);
    assert_eq!((b.phase, b.breaks), ("drain", 1), "no second refill");
}

/// One look that reads empty with the bar on screen, the gauge back 0.13 s later: no break, the drain carries on.
#[test]
fn a_one_look_misread_is_no_break() {
    for by_look in [true, false] {
        let mut b = BreakTracker::new(Some(1000.0));
        let mut t = 0.0;
        for i in 0..40 {
            t = i as f64 * 0.1;
            b.feed_look(&white(0.705 - 0.01 * i as f64), RED, by_look, t);
        }
        let mut clean = b.clone(); // the same fight without the misread
        b.feed_look(&look(Some(0.0), None, false, false), RED, by_look, t + 0.1);
        b.feed_look(&white(0.358), RED, by_look, t + 0.23);
        clean.feed_look(&white(0.358), RED, by_look, t + 0.23);
        assert_eq!((b.breaks, b.phase), (0, "drain"));
        assert_eq!(b.left, Some(0.358));
        assert_eq!(b.info(t + 0.5), clean.info(t + 0.5), "as if the misread never was");
    }
}

/// A real break by a burst: 0.40, then empty on every look for 9 s: one break, dated from the first empty look.
#[test]
fn a_burst_break_counts_from_its_first_empty_look() {
    let mut b = BreakTracker::new(Some(1000.0));
    b.feed(Some(0.99), None, 0.0);
    b.feed(Some(0.40), None, 10.0);
    for i in 0..90 {
        b.feed(Some(0.0), None, 10.1 + i as f64 * 0.1);
    }
    assert_eq!((b.breaks, b.phase), (1, "broken"));
    assert!((b.last_after.unwrap() - 10.1).abs() < 1e-9, "{:?}", b.last_after);
}

/// Empty for 0.3 s, then the fight ends: never counted.
#[test]
fn an_empty_spell_cut_by_the_fights_end_is_not_counted() {
    let mut b = BreakTracker::new(Some(1000.0));
    b.feed(Some(0.30), None, 0.0);
    for t in [0.1, 0.2, 0.3, 0.4] {
        b.feed(Some(0.0), None, t);
    }
    assert_eq!((b.breaks, b.phase), (0, "drain"));
    assert_eq!(b.info(0.4).breaks, 0);
}

/// The lock icon: the boss can't be broken. With it up the gauge is shown as it reads (35%), and an empty gauge for 3 s
/// is no break; once it is gone, an empty gauge with the broken look is one break, then.
#[test]
fn no_break_while_the_lock_icon_is_up() {
    let mut b = BreakTracker::new(Some(1000.0));
    see(&mut b, white(0.42), 0.0);
    see(&mut b, look(Some(0.35), Some("white"), false, true), 1.0);
    let i = b.info(1.0);
    assert_eq!((i.left, i.locked, i.breaks), (Some(0.35), true, 0));
    for k in 0..30 {
        see(&mut b, look(Some(0.0), None, k % 2 == 0, true), 2.0 + k as f64 * 0.1);
    }
    assert_eq!((b.breaks, b.phase), (0, "drain"), "never with the icon up");
    see(&mut b, look(Some(0.0), None, false, false), 5.1);
    assert_eq!(b.breaks, 0, "the icon gone, no broken look: not yet");
    see(&mut b, look(Some(0.0), None, true, false), 5.2);
    assert_eq!((b.breaks, b.phase, b.locked), (1, "broken", false), "the broken look with the icon gone: one break");
}

/// The open world's look: an empty gauge with the broken look is a break at once; a white line growing from the left
/// with the broken look is the refill; the bright flashes before it are neither; a line without the broken look is the
/// drain again.
#[test]
fn the_broken_look_decides_at_once() {
    let mut b = BreakTracker::new(Some(1000.0));
    see(&mut b, look(Some(0.04), Some("orange"), false, false), 0.0);
    see(&mut b, look(Some(0.0), None, true, false), 0.1);
    assert_eq!((b.breaks, b.phase), (1, "broken"), "at once");
    // the bright flashes at the end of the broken state: no line, no new break, no refill
    for (k, r) in [look(Some(0.9), Some("orange"), true, false), look(Some(0.62), Some("white"), true, false), look(None, None, true, false)].into_iter().enumerate() {
        see(&mut b, r, 1.0 + k as f64 * 0.1);
        assert_eq!((b.breaks, b.phase), (1, "broken"), "{k}");
    }
    see(&mut b, look(Some(0.069), Some("white"), true, false), 8.0);
    assert_eq!(b.phase, "refill", "the white line growing from the left");
    see(&mut b, look(Some(0.5), Some("white"), true, false), 10.0);
    assert_eq!(b.phase, "refill");
    see(&mut b, look(Some(0.99), Some("white"), true, false), 13.0);
    assert_eq!(b.phase, "refill", "full, but the badge still up");
    see(&mut b, look(Some(0.995), Some("white"), false, false), 13.5);
    assert_eq!(b.phase, "drain", "unbroken again: the line, the broken look gone");
    assert_eq!(b.breaks, 1);
}

/// One broken spell is one break, however often the HUD goes: two cutscenes (the bar away 2.5 s each) with the broken
/// look back after each count nothing more; a gap during the drain changes nothing; a second break only after a
/// sighting of the gauge unbroken again.
#[test]
fn one_break_per_broken_spell() {
    let mut b = BreakTracker::new(Some(1000.0));
    let gap = |b: &mut BreakTracker, from: f64| {
        let mut t = from;
        while t < from + 2.5 {
            b.feed_look(&look(None, None, false, false), GREY, true, t); // (the bar not read)
            t += 0.1;
        }
    };
    see(&mut b, white(0.40), 0.0);
    gap(&mut b, 0.1);
    assert_eq!((b.phase, b.left), ("drain", Some(0.40)), "a gap in the drain changes nothing");
    see(&mut b, white(0.10), 3.0);
    see(&mut b, look(Some(0.0), None, true, false), 3.5);
    assert_eq!((b.breaks, b.phase), (1, "broken"));
    for k in 0..2 {
        let t = 4.0 + k as f64 * 4.0;
        gap(&mut b, t);
        see(&mut b, look(Some(0.0), None, true, false), t + 2.6);
        see(&mut b, look(None, None, true, false), t + 2.7); // (a flash)
        assert_eq!((b.breaks, b.phase), (1, "broken"), "after cutscene {k}");
    }
    see(&mut b, look(Some(0.08), Some("white"), true, false), 13.0);
    gap(&mut b, 13.1);
    see(&mut b, look(Some(0.0), None, true, false), 15.7); // (the line unread after the gap)
    see(&mut b, look(Some(0.4), Some("white"), true, false), 16.0);
    assert_eq!((b.breaks, b.phase), (1, "refill"));
    see(&mut b, look(Some(0.99), Some("white"), false, false), 18.0);
    assert_eq!(b.phase, "drain", "the reset: a line, the broken look gone");
    see(&mut b, white(0.30), 30.0);
    see(&mut b, look(Some(0.0), None, true, false), 31.0);
    assert_eq!((b.breaks, b.phase), (2, "broken"), "a second real break after the reset");
}

/// A break whose drain to empty wasn't seen (the gauge unread for a while, last read at 52%) is still counted on the
/// first look that shows it broken: no line, the track's start empty, the broken look.
#[test]
fn a_break_seen_late_still_counts() {
    let mut b = BreakTracker::new(Some(1000.0));
    see(&mut b, white(0.521), 24.6);
    for k in 0..300 {
        b.feed_look(&look(None, None, false, false), RED, true, 24.7 + k as f64 * 0.1); // (unknown)
    }
    see(&mut b, look(Some(0.0), None, true, false), 60.4);
    assert_eq!((b.breaks, b.phase), (1, "broken"));
    assert!(b.unknown_summary().contains("no line found 300"), "{}", b.unknown_summary());
}

/// The fight of 22 Nov (open world) as the gauge truly went, each look as it reads now: a one-look low misread in the
/// drain, the lock icon at 35% and 27% (orange), the drain to 4%, empty at 34.9, the broken look from 36.5 with the
/// glow (no line), flashes, the refill's white line from 43.6, up under where our label used to be (unknown), the
/// broken look gone at 50.7: exactly one break and one refill.
#[test]
fn the_fight_of_22_november() {
    let mut b = BreakTracker::new(Some(1000.0));
    let mut phases = vec![];
    let mut feed = |b: &mut BreakTracker, r: BreakRead, t: f64| {
        see(b, r, t);
        if phases.last() != Some(&b.phase) {
            phases.push(b.phase);
        }
    };
    let mut t = 0.1;
    while t < 8.8 {
        feed(&mut b, white(0.426), t);
        t += 0.1;
    }
    feed(&mut b, white(0.264), 8.84); // (a misread)
    feed(&mut b, white(0.426), 8.95);
    let mut g: f64 = 0.426;
    while t < 26.5 {
        g -= 0.0004;
        feed(&mut b, white(g), t);
        t += 0.1;
    }
    for (t, g, kind, locked) in [(26.51, 0.371, "white", true), (27.54, 0.314, "white", true), (28.55, 0.275, "orange", true), (29.58, 0.256, "orange", true),
                                 (30.09, 0.227, "orange", false), (32.35, 0.155, "orange", false), (33.9, 0.067, "orange", false), (34.4, 0.031, "orange", false)] {
        feed(&mut b, look(Some(g), Some(kind), false, locked), t);
    }
    feed(&mut b, look(Some(0.010), Some("orange"), false, false), 34.92);
    assert_eq!(b.breaks, 0, "empty, not yet the broken look");
    let mut t = 36.54;
    while t < 43.5 {
        let r = if (t * 10.0) as i64 % 3 == 0 { look(None, None, true, false) } else { look(Some(0.0), None, true, false) };
        feed(&mut b, r, t);
        t += 0.25;
    }
    feed(&mut b, look(Some(0.9), Some("orange"), true, false), 42.0); // (a flash)
    let mut g = 0.069;
    let mut t = 43.62;
    while g < 0.66 {
        feed(&mut b, look(Some(g), Some("white"), true, false), t);
        g += 0.014;
        t += 0.1;
    }
    while t < 50.7 {
        feed(&mut b, look(None, Some("white"), true, false), t); // (under the old cover)
        t += 0.1;
    }
    feed(&mut b, look(None, Some("white"), false, false), 51.2);
    feed(&mut b, white(0.98), 51.7);
    assert_eq!(b.breaks, 1, "one break");
    assert_eq!(phases, vec!["drain", "broken", "refill", "drain"], "one refill");
}

/// An Interlink bar (no broken look to go by): a jump up is a refill only when the next reading holds it.
#[test]
fn an_interlink_jump_up_needs_a_second_reading() {
    let mut b = BreakTracker::new(Some(1000.0));
    for (t, g) in [(0.0, 0.42), (0.1, 0.27), (0.2, 0.42), (0.3, 0.41)] {
        b.feed_look(&white(g), RED, false, t);
    }
    assert_eq!(b.phase, "drain", "a misread low and back");
    for (t, g) in [(1.0, 0.40), (1.1, 0.70), (1.2, 0.71)] {
        b.feed_look(&white(g), RED, false, t);
    }
    assert_eq!(b.phase, "refill", "held");
}

/// The Holo-Battle Sim fight of 22 Nov, drawn at 3840x2160 and read through `bar::measure_break` with the bar found 6 px
/// too high (as the meter had it, y 188 for 194): the gauge is still read on every look; the lock icon up while the
/// gauge stands at 52%; the drain to 10% (orange); the break; broken with flashes; the refill past 67%; the badge gone
/// at the full gauge. Exactly one break, one refill, and the gauge shown never more than a second behind the picture.
#[test]
fn the_sim_fight_of_22_november_read_from_pictures() {
    use alogs_pil::{Image, Mode};
    use alogs_reader::bar::{self, Bar};
    const YC: i64 = 194;
    const TL: i64 = 1243;
    const TR: i64 = 2584;
    let span = 1388.0;
    let lb = 1226.0;
    let rect = |im: &mut Image, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 3]| {
        for y in y0..=y1 {
            for x in x0..=x1 {
                im.putpixel(x as usize, y as usize, &c);
            }
        }
    };
    // what a look shows: the gauge (None: none, a flash over it), its colour, the broken look, the lock icon
    let draw = |gauge: Option<f64>, line: [u8; 3], broken: bool, locked: bool, flash: bool| {
        let mut im = Image::new(Mode::Rgb, 3840, 400, &[42, 30, 24]);
        rect(&mut im, TL, YC - 13, TR, YC + 12, [248, 105, 105]);
        rect(&mut im, TL, 212, TR, 219, if broken { [108, 101, 63] } else { [56, 47, 39] });
        if let Some(g) = gauge {
            rect(&mut im, TL, 212, TL + ((TR - TL) as f64 * g) as i64 - 1, 219, line);
        }
        if flash {
            rect(&mut im, TL, 205, TR, 225, [255, 240, 170]);
        }
        if broken {
            let x = TL + (TR - TL) * 40 / 100;
            rect(&mut im, x, YC - 13, x + (TR - TL) * 4 / 100, YC + 12, [255, 230, 100]);
            // the "250%" badge: four pale glyphs and the orange arrow over the bar's right end
            let x = |f: f64| (lb + f * span) as i64;
            for k in 0..4 {
                let x0 = x(0.895) + k * 22;
                rect(&mut im, x0, YC - 20, x0 + 15, YC + 4, [255, 236, 150]);
            }
            rect(&mut im, x(0.975), YC - 18, x(0.995), YC + 4, [250, 150, 60]);
        }
        if locked {
            rect(&mut im, TL, 210, TL + 16, 228, [253, 253, 253]);
            rect(&mut im, TL + 17, 212, TL + 22, 219, [42, 30, 24]);
            rect(&mut im, TL + 23, 212, TL + 34, 219, [226, 180, 177]);
        }
        im
    };
    let found = Bar { yc: 188, left_bracket: lb, span, brackets_found: true, world: true, right_red: 0.0, track_left: TL, track_right: TR, thickness: 26, frame: (3840, 2160) };
    const WHITE: [u8; 3] = [218, 218, 218];
    const ORANGE: [u8; 3] = [248, 204, 151];
    // (time, gauge shown, line colour, broken look, lock icon, flash)
    let mut looks: Vec<(f64, Option<f64>, [u8; 3], bool, bool, bool)> = vec![];
    let mut t = 0.0;
    while t < 23.5 {
        looks.push((t, Some(1.0 - 0.43 * t / 23.5), WHITE, false, false, false));
        t += 0.5;
    }
    looks.push((23.7, Some(0.535), WHITE, false, true, false));
    looks.push((24.2, Some(0.53), WHITE, false, true, false));
    looks.push((24.7, Some(0.521), WHITE, false, true, false));
    let mut t = 25.2;
    while t < 45.0 {
        looks.push((t, Some(0.521), WHITE, false, (35.8..36.5).contains(&t), false));
        t += 0.5;
    }
    while t < 59.0 {
        let g = 0.521 - (0.521 - 0.097) * (t - 45.0) / 14.0;
        looks.push((t, Some(g), if g < 0.3 { ORANGE } else { WHITE }, false, false, false));
        t += 0.5;
    }
    looks.push((59.4, Some(0.045), ORANGE, false, false, false));
    looks.push((59.9, None, WHITE, true, false, true));
    let mut t = 60.4;
    while t < 69.6 {
        looks.push((t, None, WHITE, true, false, (t * 2.0) as i64 % 5 == 0));
        t += 0.5;
    }
    let mut t = 70.06;
    let mut g = 0.046;
    while g < 0.99 {
        looks.push((t, Some(g), WHITE, true, false, false));
        g += 0.0735;
        t += 0.5;
    }
    looks.push((t, Some(0.996), WHITE, false, false, false));
    let mut b = BreakTracker::new(Some(1000.0));
    let mut phases = vec![];
    let mut last_true = 0.0;
    let mut flashes = 0;
    for (t, gauge, line, broken, locked, flash) in looks {
        flashes += u32::from(flash);
        let im = draw(gauge, line, broken, locked, flash);
        let r = bar::measure_break(&im, &found);
        b.feed_look(&r, [248, 105, 105], true, t);
        if phases.last() != Some(&b.phase) {
            phases.push(b.phase);
        }
        assert_eq!(b.locked, locked, "the lock icon at {t}");
        if let (Some(g), false) = (gauge, broken) {
            // the drain: the gauge shown as the picture shows it, never more than a second behind
            if b.left.is_some_and(|l| (l - g).abs() < 0.015) {
                last_true = t;
            }
            assert!(t - last_true <= 1.0, "the gauge stood at {:?} at {t} for {g:.3}", b.left);
        }
    }
    assert_eq!(b.breaks, 1, "one break");
    assert_eq!(phases, vec!["drain", "broken", "refill", "drain"], "one refill");
    assert_eq!(b.unknown_summary(), format!("{flashes} looks (no line found {flashes})"), "every look read but the flashes");
}
