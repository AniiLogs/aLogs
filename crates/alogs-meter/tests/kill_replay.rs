//! A recorded Interlink kill replayed look by look (tests/data/kill_bar_burst.csv: the bar's fill, colours, counter
//! and timer as one run's fight csv logged them, 19 Nov, 115 s, numbers only). Four bars went in the counter's first
//! seconds: the meter, allowed at most 3 bars a step, stayed three bars up, refused the refills after for the stale
//! counter, and saved the kill with 2.4 million of the boss's 38.8 million HP.

use alogs_meter::env::Env;
use alogs_meter::fights::Fight;
use alogs_reader::boss::{self, Row};
use std::path::PathBuf;

const MAX_HP: i64 = 38_849_885;

fn hex(s: &str) -> [u8; 3] {
    let b = |i: usize| s.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok()).unwrap_or(0);
    [b(0), b(2), b(4)]
}

fn env(name: &str) -> Env {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::create_dir_all(&data);
    let mut env = Env::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("res"), data);
    env.write_files = false;
    env
}

fn fight(env: &Env) -> Fight {
    let mut row = Row::new();
    row.insert("puppetId".into(), "1".into());
    Fight::new(env, "Made-up Boss", 60, row, MAX_HP, true, false, None, None)
}

#[test]
fn a_kill_after_a_burst_of_bars_counts_the_boss_s_hp() {
    let env = env("kill-replay");
    let grades = boss::load_grades(&env.res, None).unwrap();
    let t0 = 1_800_000_000.0;
    env.fixed_now.set(Some(t0));
    let mut f = fight(&env);
    let mut rd = csv::Reader::from_path(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/kill_bar_burst.csv")).unwrap();
    let mut seg_at_20s = None;
    for r in rd.records() {
        let r = r.unwrap();
        let at = t0 + r[0].parse::<f64>().unwrap();
        env.fixed_now.set(Some(at));
        let fill = r[3].parse::<f64>().ok().map(|v| v / 100.0);
        f.update(&env, fill, hex(&r[4]), hex(&r[5]), r[2].parse().ok(), r[1].parse().ok(), &grades, 180.0, Some(at), true);
        if seg_at_20s.is_none() && at - t0 >= 20.5 {
            seg_at_20s = f.seg;
        }
    }
    // the counter went x15 -> x11 in 2.3 s; read x10 for a second, the meter takes it (it stood at x14 to the end)
    assert!(seg_at_20s.is_some_and(|s| s <= 9), "the meter follows the counter's burst, x{seg_at_20s:?} at 20 s");
    assert_eq!(f.seg, Some(1), "the last bar when it went");
    let left = (f.max_hp - f.damage) as f64 / f.boss_hp as f64;
    assert!(left < 0.02, "a sliver left when the bar went: {left:.3} of the boss");
    // what end_fight does with a bar gone with under 15% left: the kill counts the boss's whole HP
    f.mark_defeated(&env, "boss bar gone");
    assert_eq!(f.damage, MAX_HP);
}

/// A drop of more than 3 bars is taken only when the counter shows it plainly: three of the last four reads, every
/// read under the meter's bar for a second, and not "x1" or a number made of the bar's own digits (a lost digit).
#[test]
fn a_counter_with_a_lost_digit_is_not_a_burst() {
    let env = env("kill-replay-digit");
    let grades = boss::load_grades(&env.res, None).unwrap();
    let t0 = 1_800_000_000.0;
    let red = [250, 110, 100];
    let edge = [255, 255, 255];
    let run = |reads: &[(i64, f64)], from: i64| {
        env.fixed_now.set(Some(t0));
        let mut f = fight(&env);
        let mut t = t0;
        // the counter's top bar first, then down a bar at a time to `from`
        let top = grades["bossHpLossPercent"].len() as i64 - 1;
        for x in (from..=top).rev() {
            for _ in 0..4 {
                t += 0.12;
                env.fixed_now.set(Some(t));
                f.update(&env, Some(0.5), red, edge, Some(x), None, &grades, 180.0, Some(t), true);
            }
        }
        assert_eq!(f.seg, Some(from));
        for &(x, fill) in reads {
            t += 0.12;
            env.fixed_now.set(Some(t));
            f.update(&env, Some(fill), red, edge, Some(x), None, &grades, 180.0, Some(t), true);
        }
        f.seg
    };
    let steady = |x: i64| vec![(x, 0.5); 15]; // 1.8 s of one read
    assert_eq!(run(&steady(9), 14), Some(9), "x14 -> x9, read for 1.8 s: a burst of bars");
    assert_eq!(run(&steady(9)[..6], 14), Some(14), "read for 0.7 s only: not yet");
    assert_eq!(run(&steady(1), 8), Some(8), "never straight to x1");
    assert_eq!(run(&steady(3), 13), Some(13), "x3 from x13: a digit lost");
    let mixed: Vec<(i64, f64)> = (0..15).map(|i| (if i % 3 == 0 { 14 } else { 9 }, 0.5)).collect();
    assert_eq!(run(&mixed, 14), Some(14), "the bar's own number still read in between");
}
