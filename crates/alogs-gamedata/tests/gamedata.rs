//! Parity with aLogs B16 `bossbar/gamedata.py` on a synthetic game data folder (tests/data/game, made by
//! tools/gen_gamedata_goldens.py: no real game data) and 150 byte-flipped copies of its pmdata.bin. Every result must
//! have the reference's `repr()`, and a copy the reference fails on must fail here too.

use alogs_gamedata::gamedata as g;
use alogs_gamedata::pmdata::{PmData, Result};
use alogs_pyval::Value;
use serde_json::Value as J;
use std::path::{Path, PathBuf};

const TABLES: &[&str] = &[
    "bossrush_cycle_data",
    "bossrush_guanka_data",
    "property_data",
    "puppet_data",
    "puppet_data1",
    "puppet_data2",
    "puppet_data3",
    "sys_config_data",
    "bossrush_grade_data",
    "nope",
];

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}
fn pm_path() -> PathBuf {
    data().join("game/lua/LuaScripts/Common/Data/pmdata.bin")
}

fn check(want: &J, got: Result<String>, what: &str) {
    match (want.get("ok").and_then(J::as_str), got) {
        (Some(w), Ok(g)) => assert_eq!(g, w, "{what}"),
        (None, Err(_)) => {}
        (Some(w), Err(e)) => panic!("{what}: failed ({e}), reference gave {}", &w[..w.len().min(200)]),
        (None, Ok(g)) => panic!("{what}: gave {}, reference failed ({})", &g[..g.len().min(200)], want["err"]),
    }
}

fn report(bytes: &[u8], path: &Path, want: &J, label: &str) {
    for t in TABLES {
        let got = PmData::from_bytes(bytes.to_vec()).table(t).map(|v| v.repr());
        check(&want[*t], got, &format!("{label} table {t}"));
    }
    check(&want["read_cycles"], g::read_cycles(&mut PmData::from_bytes(bytes.to_vec())).map(|v| v.repr()), &format!("{label} read_cycles"));
    check(&want["read_grades"], g::read_grades(&mut PmData::from_bytes(bytes.to_vec())).map(|v| v.repr()), &format!("{label} read_grades"));
    check(&want["read_rush_rules"], g::read_rush_rules(path).map(|v| v.repr()), &format!("{label} read_rush_rules"));
    let ids = g::boss_rush_ids(&mut PmData::from_bytes(bytes.to_vec()))
        .map(|s| Value::List(s.into_iter().map(Value::Str).collect()).repr());
    check(&want["boss_rush_ids"], ids, &format!("{label} boss_rush_ids"));
}

fn goldens() -> J {
    serde_json::from_str(&std::fs::read_to_string(data().join("gamedata_goldens.json")).unwrap()).unwrap()
}

#[test]
fn synthetic_game_data_matches_reference() {
    let gl = goldens();
    let base = &gl["base"];
    let bytes = std::fs::read(pm_path()).unwrap();
    report(&bytes, &pm_path(), base, "base");
    let keep = vec![("puppet_data1".to_string(), "600101".to_string())];
    let catalog = data().join("game/boss_catalog.csv");
    let grades = data().join("game/boss_rush_grades.csv");
    let lines = |v: Vec<String>| Value::List(v.into_iter().map(Value::Str).collect()).repr();
    check(&base["read_bosses"], g::read_bosses(&pm_path(), &keep).map(|v| v.repr()), "read_bosses");
    check(&base["catalog_rows"], g::catalog_rows(&pm_path(), &keep).map(|v| v.repr()), "catalog_rows");
    check(&base["catalog_drift"], g::catalog_drift(Some(&pm_path()), &catalog, None).map(lines), "catalog_drift");
    check(&base["catalog_drift_none"], g::catalog_drift(None, &catalog, None).map(lines), "catalog_drift without game");
    let mut pm = PmData::from_bytes(bytes.clone());
    check(&base["grades_drift"], g::grades_drift(Some(&mut pm), &grades).map(lines), "grades_drift");
    check(&base["shipped_grades"], g::shipped_grades(&grades).map(|v| v.repr()), "shipped_grades");
    let cycles = g::read_cycles(&mut PmData::from_bytes(bytes)).unwrap();
    let covers: Vec<bool> = [1759999999.0, 1760000000.0, 1760300000.0, 1763628799.0, 1763628800.0]
        .iter()
        .map(|&now| g::covers_now(&cycles, Some(now)).unwrap())
        .collect();
    let want: Vec<bool> = base["covers"].as_array().unwrap().iter().map(|b| b.as_bool().unwrap()).collect();
    assert_eq!(covers, want, "covers_now");
}

#[test]
fn byte_flipped_copies_match_reference() {
    let gl = goldens();
    let base = std::fs::read(pm_path()).unwrap();
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fuzz-pmdata.bin");
    let cases = gl["fuzz"].as_array().unwrap();
    assert!(cases.len() >= 100);
    for (i, case) in cases.iter().enumerate() {
        let mut b = base.clone();
        for f in case["flips"].as_array().unwrap() {
            b[f[0].as_u64().unwrap() as usize] = f[1].as_u64().unwrap() as u8;
        }
        std::fs::write(&tmp, &b).unwrap();
        report(&b, &tmp, &case["report"], &format!("fuzz {i} {}", case["flips"]));
    }
}

#[test]
fn rush_rules_and_grade_ladders() {
    use alogs_pyval::dict;
    let cfg = dict([
        ("BossRushMaxStarSideBoss", Value::Bool(true)),
        ("BossRushStarMultiplierMidBoss", Value::Int(3)),
        ("BossRushPlayerBotList", dict([("a", Value::Str("x".into())), ("b", Value::Int(1))])),
    ]);
    assert!(g::rush_rules_from(&cfg).is_err(), "sorted() of a str and an int raises");
    let cfg = dict([("BossRushMaxStarSideBoss", Value::Bool(true)), ("BossRushStarMultiplierMidBoss", Value::Int(3))]);
    assert_eq!(g::rush_rules_from(&cfg).unwrap().repr(), "{'starSide': True, 'starCenter': 3}");
    // a ladder that goes down is not usable
    let rows = |vals: &[f64]| {
        Value::Dict(
            vals.iter()
                .enumerate()
                .map(|(i, v)| (Value::Int(i as i64), dict([("bossHpLossPercent", Value::Float(*v))])))
                .collect(),
        )
    };
    assert_eq!(g::grade_rows(&rows(&[0.0, 0.6, 0.5, 1.0])).unwrap().repr(), "[]");
    assert_eq!(g::grade_rows(&rows(&[0.0, 0.5, 1.0])).unwrap().as_list().unwrap().len(), 3);
}

/// The shipped files written from a game's data check against that data with no drift, and writing them again over
/// themselves changes no byte.
#[test]
fn written_game_data_checks_clean() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("written-game-data");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (catalog, grades) = (dir.join("boss_catalog.csv"), dir.join("boss_rush_grades.csv"));
    let (n, _) = g::write_catalog(&pm_path(), &catalog).unwrap();
    assert!(n > 0);
    let mut pm = PmData::from_bytes(std::fs::read(pm_path()).unwrap());
    assert!(g::write_grades(&mut pm, &grades).unwrap() >= 3);
    // (the made-up data names stage bosses it has no puppet_data row for: no writer can add those)
    let drift: Vec<String> = g::catalog_drift(Some(&pm_path()), &catalog, None).unwrap().into_iter().filter(|l| !l.ends_with("has no puppet_data row")).collect();
    assert_eq!(drift, Vec::<String>::new());
    let mut pm = PmData::from_bytes(std::fs::read(pm_path()).unwrap());
    assert_eq!(g::grades_drift(Some(&mut pm), &grades).unwrap(), Vec::<String>::new());
    let (a, b) = (std::fs::read(&catalog).unwrap(), std::fs::read(&grades).unwrap());
    assert_eq!(g::write_catalog(&pm_path(), &catalog).unwrap(), (n, 0), "nothing changed the second time");
    let mut pm = PmData::from_bytes(std::fs::read(pm_path()).unwrap());
    g::write_grades(&mut pm, &grades).unwrap();
    assert!(std::fs::read(&catalog).unwrap() == a && std::fs::read(&grades).unwrap() == b, "the same bytes");
    assert!(!a.contains(&b'\r'), "LF line ends");
}
