//! Parity with aLogs B16 `fights.py` (Fight, Timer, BreakTracker) on scripted fights (tools/gen_fight_goldens.py).
//! Needs reference/ (the shipped grade table and boss_break.json); skipped without it.

use alogs_meter::env::Env;
use alogs_meter::fights::{BreakTracker, Fight, Timer};
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
    for seq in g["breaks"].as_array().unwrap() {
        let mut b = BreakTracker::new(Some(1234.5));
        for s in seq.as_array().unwrap() {
            let t = s[0].as_f64().unwrap();
            let kind = None;
            b.feed(s[1].as_f64(), kind, t);
            let i = b.info(t);
            let got = json!([t, s[1], i.left, i.phase, i.rate, i.rate_now, i.breaks, i.last_after]);
            assert!(same(&got, s), "break: got {got}, want {s}");
        }
    }
}
