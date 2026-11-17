//! Parity with aLogs B16 `bossbar/hpcalc.py`.
//! - tests/data/hpcalc_goldens.json: written by tools/gen_hpcalc_goldens.py from the reference; every value must match
//!   bit for bit.
//! - reference/aLogs/bossbar/boss_catalog.csv: the reference's shipped max HP at ten levels for every boss, read in
//!   place (never copied here). Skipped when reference/ isn't cloned.

use alogs_gamedata::hpcalc::{break_points, max_hp, rate, total};
use serde_json::Value;
use std::path::PathBuf;

fn goldens() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/hpcalc_goldens.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn cases<'a>(g: &'a Value, key: &str) -> &'a Vec<Value> {
    g[key].as_array().unwrap()
}

/// Same f64 bits (both None, or both the same number).
fn same(got: Option<f64>, want: &Value, what: &str) {
    let want = want.as_f64();
    match (got, want) {
        (None, None) => {}
        (Some(a), Some(b)) => assert!(a.to_bits() == b.to_bits(), "{what}: got {a:?}, reference {b:?}"),
        _ => panic!("{what}: got {got:?}, reference {want:?}"),
    }
}

#[test]
fn rate_matches_reference() {
    let g = goldens();
    let mut n = 0;
    for c in cases(&g, "rate") {
        for v in c["values"].as_array().unwrap() {
            let lv = v[0].as_f64().unwrap();
            same(rate(&c["entry"], lv), &v[1], &format!("rate({}, {lv})", c["entry"]));
            n += 1;
        }
    }
    assert!(n > 2000, "only {n} rate goldens");
}

#[test]
fn total_matches_reference() {
    let g = goldens();
    for c in cases(&g, "total") {
        let (species, ind) = (c["species"].as_f64().unwrap(), c["ind"].as_f64().unwrap());
        for v in c["values"].as_array().unwrap() {
            let lv = v[0].as_i64().unwrap();
            assert_eq!(total(species, lv, ind), v[1].as_i64().unwrap(), "total({species}, {lv}, {ind})");
        }
    }
}

#[test]
fn max_hp_matches_reference() {
    let g = goldens();
    for c in cases(&g, "max_hp") {
        let species = c["species"].as_f64().unwrap();
        for v in c["values"].as_array().unwrap() {
            let lv = v[0].as_i64().unwrap();
            assert_eq!(max_hp(species, &c["entry"], lv, 0.0), v[1].as_i64(), "max_hp({species}, {}, {lv})", c["entry"]);
        }
    }
}

#[test]
fn break_points_match_reference() {
    let g = goldens();
    for c in cases(&g, "break_points") {
        let f = c["formula"].as_i64().unwrap();
        let args: Vec<f64> = c["args"].as_array().unwrap().iter().map(|a| a.as_f64().unwrap()).collect();
        for v in c["values"].as_array().unwrap() {
            let lv = v[0].as_f64().unwrap();
            same(break_points(f, &args, lv), &v[1], &format!("break_points({f}, {args:?}, {lv})"));
        }
    }
}

#[test]
fn python_rounding_is_half_to_even() {
    // species -19.7 at level 1: floor(0.3 / 120 * 41 * 20) = 2 base HP; Python's round(2.5) == 2, round(3.5) == 4
    assert_eq!(total(-19.7, 1, 0.0), 2);
    assert_eq!(max_hp(-19.7, &serde_json::json!({"1": 3, "2": 1.25}), 1, 0.0), Some(2));
    assert_eq!(max_hp(-19.7, &serde_json::json!({"1": 3, "2": 1.75}), 1, 0.0), Some(4));
}

#[test]
fn shipped_boss_catalog_matches() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/aLogs/bossbar/boss_catalog.csv");
    if !p.exists() {
        eprintln!("skipped: {} not cloned (see docs/rewrite-plan.md)", p.display());
        return;
    }
    let mut rd = csv::Reader::from_path(&p).unwrap();
    let headers = rd.headers().unwrap().clone();
    let col = |name: &str| headers.iter().position(|h| h == name).unwrap();
    let (sp, adj, fr) = (col("species_hp"), col("prop_species_adj"), col("fixed_hp_rate"));
    let levels: Vec<(usize, i64)> = headers
        .iter()
        .enumerate()
        .filter_map(|(i, h)| h.strip_prefix("maxHP_lv").map(|l| (i, l.parse().unwrap())))
        .collect();
    assert_eq!(levels.len(), 10);
    let (mut rows, mut checked) = (0, 0);
    for rec in rd.records() {
        let rec = rec.unwrap();
        rows += 1;
        let f = |i: usize| rec[i].parse::<f64>().unwrap_or(0.0);
        let species = f(sp) + f(adj);
        let entry: Value = serde_json::from_str(&rec[fr]).unwrap_or(Value::Null);
        let has_rate = entry.as_object().is_some_and(|o| !o.is_empty());
        for &(i, lv) in &levels {
            let got = if has_rate { max_hp(species, &entry, lv, 0.0) } else { None };
            let got = got.map(|h| h.to_string()).unwrap_or_default();
            assert_eq!(got, &rec[i], "row {rows} ({}) level {lv}", &rec[0]);
            checked += 1;
        }
    }
    assert!(rows > 300, "only {rows} catalog rows");
    eprintln!("boss_catalog.csv: {rows} rows, {checked} max HP values match");
}
