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
