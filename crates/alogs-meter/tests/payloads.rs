//! Parity with aLogs B16 `submit.py`'s payload builders on made-up run summaries (tools/gen_payload_goldens.py).

use alogs_gamelog::FixedClock;
use alogs_meter::payloads as pl;
use alogs_pyval::Value;
use alogs_reader::boss::Schedule;
use serde_json::Value as J;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}
fn pv(j: &J) -> Value {
    Value::from_json(j)
}
fn notice_repr(n: Option<(String, String, String)>) -> String {
    match n {
        None => "None".into(),
        Some((a, b, c)) => Value::List(vec![Value::Str(a), Value::Str(b), Value::Str(c)]).repr(),
    }
}

#[test]
fn payloads_match_reference() {
    let g: J = serde_json::from_str(&std::fs::read_to_string(dir().join("payload_goldens.json")).unwrap()).unwrap();
    let clock = FixedClock(g["now"].as_f64().unwrap());
    let sched = Schedule::new(dir().join("cycles.json"));
    let species: BTreeSet<String> = ["irisalis", "leafy", "sparkelf"].iter().map(|s| s.to_string()).collect();
    for (i, c) in g["runs"].as_array().unwrap().iter().enumerate() {
        let p = pl::run_payload(&pv(&c["summary"]), &Value::Str("100000000009".into()), &sched, &clock);
        assert_eq!(p.repr(), pv(&c["payload"]).repr(), "run_payload {i}: {}", c["summary"]);
        assert_eq!(pl::send_check(&p, Some(&species)).repr(), pv(&c["checked"]).repr(), "send_check {i}");
        assert_eq!(pl::send_check(&p, None).repr(), pv(&c["checked_none"]).repr(), "send_check (no species) {i}");
    }
    let s = |v: &J| pv(v);
    for c in g["scores"].as_array().unwrap() {
        let a = &c[0];
        let boss_id = a.get(6).map_or(Value::None, s);
        let got = pl::score_payload(a[0].as_str().unwrap(), a[1].as_str().unwrap(), &s(&a[2]), &s(&a[3]), &s(&a[4]), &s(&a[5]), &boss_id, &sched, &clock);
        assert_eq!(got.repr(), pv(&c[1]).repr(), "score_payload {a}");
    }
    for c in g["sims"].as_array().unwrap() {
        let a = &c[0];
        let got = pl::sim_payload(&s(&a[0]), &s(&a[1]), a[2].as_f64(), &clock);
        assert_eq!(got.repr(), pv(&c[1]).repr(), "sim_payload {a}");
    }
    for c in g["eggs"].as_array().unwrap() {
        let a = &c[0];
        let got = pl::egg_match_payload(&s(&a[0]), &s(&a[1]), &s(&a[2]), a.get(3).and_then(J::as_f64), &clock);
        assert_eq!(got.repr(), pv(&c[1]).repr(), "egg_match_payload {a}");
    }
    for c in g["seasons"].as_array().unwrap() {
        let a = &c[0];
        let prof = a.get(3).map(s);
        let got = pl::eggheist_payload(&s(&a[0]), &s(&a[1]), &s(&a[2]), prof.as_ref(), &clock);
        assert_eq!(got.repr(), pv(&c[1]).repr(), "eggheist_payload {a}");
    }
    for c in g["notices"].as_array().unwrap() {
        let want = if c[1].is_null() { "None".to_string() } else { pv(&c[1]).repr() };
        assert_eq!(notice_repr(pl::sent_notice(&pv(&c[0]), None)), want, "sent_notice {}", c[0]);
    }
}

/// The meter's log lines, kept for the tests to look at.
struct Lines;
static LINES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
impl log::Log for Lines {
    fn enabled(&self, _: &log::Metadata) -> bool { true }
    fn log(&self, r: &log::Record) { LINES.lock().unwrap().push(r.args().to_string()); }
    fn flush(&self) {}
}
fn logged(part: &str) -> bool {
    let _ = log::set_logger(&Lines);
    log::set_max_level(log::LevelFilter::Info);
    LINES.lock().unwrap().iter().any(|l| l.contains(part))
}

fn card(slot: i64, name: &str, me: bool, aniimo: &str, cp: i64) -> J {
    serde_json::json!({"slot": slot, "name": name, "ai": false, "self": me, "aniimo": aniimo, "aniimo_level": 60, "cp": cp})
}
fn row(slot: i64, name: &str, settled: bool) -> J {
    let mut r = serde_json::json!({"slot": slot, "name": name, "ai": false, "self": slot == 0, "stat": "Damage", "value": 1000});
    if settled {
        r["settled"] = J::Bool(true);
    }
    r
}
fn sent(party: Vec<J>, players: Vec<J>) -> Vec<J> {
    let _ = logged("");
    let summary = pv(&serde_json::json!({"party": party, "result": {"players": players}}));
    let l = pl::lineup(&summary);
    serde_json::from_str::<J>(&l.json_dumps()).unwrap()["lineup"].as_array().unwrap().clone()
}

/// At send, a seat's name the result screen settled by its own repeats wins a wholly different lineup name, when the
/// two screens' other seats line up; the seat's Aniimo and CP stay the Party screen's.
#[test]
fn the_result_screen_wins_a_real_disagreement() {
    let party = || vec![card(0, "Kyhra", true, "Irisalis", 3578), card(1, "Zarvok", false, "Leafy", 3001), card(2, "MinMax", false, "Sparkelf", 2900), card(3, "Ashgrove", false, "Leafy", 3100)];
    let l = sent(party(), vec![row(0, "Kyhra", false), row(1, "Pellinore", true), row(2, "MinMax", false), row(3, "Ashgrove", false)]);
    assert_eq!(l[1]["name"], "Pellinore");
    assert_eq!((l[1]["aniimo"].as_str(), l[1]["cp"].as_i64(), l[1]["aniimoLevel"].as_i64()), (Some("Leafy"), Some(3001), Some(60)), "the Party screen's lead");
    assert!(logged("lineup: slot 2's \"Zarvok\" sent as the result screen's \"Pellinore\" (a different name)"));
    // two seats wholly different, two lining up (yours counted)
    let l = sent(party(), vec![row(0, "Kyhra", false), row(1, "Pellinore", true), row(2, "MinMax", false), row(3, "Quillon", true)]);
    assert_eq!((l[1]["name"].as_str(), l[3]["name"].as_str()), (Some("Pellinore"), Some("Quillon")));
    // fewer than two other seats line up: the screens' seats may not be the same players' at all
    let l = sent(party(), vec![row(0, "Kyhra", false), row(1, "Pellinore", true), row(2, "Tobiah", false), row(3, "Quillon", false)]);
    assert_eq!(l[1]["name"], "Zarvok");
    assert!(logged("lineup: slot 2's \"Zarvok\" and the result screen's \"Pellinore\" disagree (a different name), sent as it is: only 1 other seat(s) line up"));
    // a name the result screen didn't settle by itself is no reason
    let l = sent(party(), vec![row(0, "Kyhra", false), row(1, "Pellinore", false), row(2, "MinMax", false), row(3, "Ashgrove", false)]);
    assert_eq!(l[1]["name"], "Zarvok");
    // an alike misread is handled as before
    let l = sent(party(), vec![row(0, "Kyhra", false), row(1, "Zarv0k", false), row(2, "MinMax", false), row(3, "Ashgrove", false)]);
    assert_eq!(l[1]["name"], "Zarv0k");
    assert!(logged("lineup: slot 2's \"Zarvok\" sent as the result screen's \"Zarv0k\""));
    // never one player in two seats
    let l = sent(party(), vec![row(0, "Kyhra", false), row(1, "MinMax", true), row(2, "MinMax", false), row(3, "Ashgrove", false)]);
    assert_eq!((l[1]["name"].as_str(), l[2]["name"].as_str()), (Some("Zarvok"), Some("MinMax")));
    assert!(logged("sent as it is: the result's name is another seat's"));
}
