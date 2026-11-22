//! Parity with aLogs B16 `fights.py` (Fight, Timer, BreakTracker) on scripted fights (tools/gen_fight_goldens.py).
//! Needs reference/ (the shipped grade table and boss_break.json); skipped without it.

use alogs_meter::env::Env;
use alogs_meter::fights::{BREAK_CONFIRM_S, BreakTracker, Fight, Timer};
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
    // (the reference counts a break on its first empty look; here it needs a look BREAK_CONFIRM_S on that is still
    // empty: that look is fed after the empty one, the empty look's own row isn't compared, and every later row is the
    // reference's, the break dated from the first empty look)
    for seq in g["breaks"].as_array().unwrap() {
        let mut b = BreakTracker::new(Some(1234.5));
        for s in seq.as_array().unwrap() {
            let t = s[0].as_f64().unwrap();
            let kind = None;
            let before = b.breaks;
            b.feed(s[1].as_f64(), kind, t);
            if s[6].as_i64() == Some(before + 1) {
                assert_eq!((b.breaks, b.phase), (before, "drain"), "not counted on its first empty look at {t}");
                b.feed(s[1].as_f64(), kind, t + BREAK_CONFIRM_S);
                assert_eq!((b.breaks, b.phase), (before + 1, "broken"), "counted on the look that confirms it");
                continue;
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

/// A Holo-Battle Sim fight: a skill animation hides the HUD (no bar read), a golden flash where the bar was reads
/// "broken", the bar comes back half full: no break. Later the real one: a drain to empty that stays, one break dated
/// at its first empty look and counted half a second on.
#[test]
fn a_hidden_bar_and_a_flash_are_no_break() {
    let mut b = BreakTracker::new(Some(1000.0));
    b.feed_look(Some(0.384), None, RED, 16.88);
    for t in [17.16, 17.54, 18.14] {
        b.feed_look(Some(0.384), None, GREY, t);
    }
    for t in [18.32, 19.42] {
        b.feed_look(Some(0.0), None, GOLD, t);
    }
    assert_eq!((b.breaks, b.phase), (0, "drain"));
    b.feed_look(Some(0.353), None, RED, 19.84);
    let mut t = 19.84;
    let mut g: f64 = 0.353;
    while g > 0.05 {
        t += 0.1;
        g -= 0.0072;
        b.feed_look(Some(g.max(0.046)), None, RED, t);
    }
    assert_eq!(b.breaks, 0, "no break while it drains");
    for (t, g) in [(24.49, 0.046), (24.59, 0.009), (24.7, 0.009), (24.9, 0.005)] {
        b.feed_look(Some(g), None, RED, t);
    }
    assert_eq!((b.breaks, b.phase), (0, "drain"), "not yet: empty for under half a second");
    b.feed_look(Some(0.009), None, RED, 25.1);
    assert_eq!((b.breaks, b.phase), (1, "broken"), "counted by 25.1");
    for i in 0..200 {
        b.feed_look(Some(if i % 2 == 0 { 0.009 } else { 0.005 }), None, RED, 25.2 + i as f64 * 0.1);
    }
    assert_eq!(b.breaks, 1, "once");
}

/// One look that reads empty with the bar on screen, the gauge back 0.13 s later: no break, the drain carries on.
#[test]
fn a_one_look_misread_is_no_break() {
    let mut b = BreakTracker::new(Some(1000.0));
    let mut t = 0.0;
    for i in 0..40 {
        t = i as f64 * 0.1;
        b.feed_look(Some(0.705 - 0.01 * i as f64), None, RED, t);
    }
    let mut clean = b.clone(); // the same fight without the misread
    b.feed_look(Some(0.0), None, RED, t + 0.1);
    b.feed_look(Some(0.358), None, RED, t + 0.23);
    clean.feed_look(Some(0.358), None, RED, t + 0.23);
    assert_eq!((b.breaks, b.phase), (0, "drain"));
    assert_eq!(b.left, Some(0.358));
    assert!(b.rate(t + 0.23).0.is_some(), "the drain rate carries on");
    assert_eq!(b.info(t + 0.5), clean.info(t + 0.5), "as if the misread never was");
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
