//! Parity with aLogs B16 `results.py`'s text parsers and `core.py`'s Teammates, party_pill and screen signatures
//! (tools/gen_results_goldens.py).

use alogs_meter::results_parse as rp;
use alogs_meter::team::{self, Teammates};
use alogs_pil::{Image, Mode};
use alogs_pyval::{Dict, Value};
use indexmap::IndexMap;
use serde_json::{Value as J, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}
fn g() -> J {
    serde_json::from_str(&std::fs::read_to_string(dir().join("results_goldens.json")).unwrap()).unwrap()
}
fn pv(j: &J) -> Value {
    Value::from_json(j)
}
/// The golden's JSON as Python's repr gives it, to compare with a Value's repr.
fn repr(j: &J) -> String {
    pv(j).repr()
}

#[test]
fn text_parsers_match_reference() {
    let g = g();
    for c in g["canon"].as_array().unwrap() {
        assert_eq!(rp::canon(c[0].as_str().unwrap(), &rp::RESULT_STATS, 0.6), c[1].as_str().unwrap(), "canon {}", c[0]);
    }
    for c in g["first_num"].as_array().unwrap() {
        assert_eq!(json!(rp::first_num(c[0].as_str())), c[1], "first_num {}", c[0]);
    }
    for c in g["parse_sim"].as_array().unwrap() {
        assert_eq!(rp::parse_sim(c[0].as_str()).map_or("None".into(), |v| v.repr()), repr(&c[1]), "parse_sim {}", c[0]);
    }
    let names: Vec<String> = ["Irisalis", "Ignitis", "Leafy", "Sparkelf"].iter().map(|s| s.to_string()).collect();
    for c in g["sim_team"].as_array().unwrap() {
        let t: IndexMap<String, String> = c[0].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
        let species: Vec<Option<String>> = c[1].as_array().unwrap().iter().map(|v| v.as_str().map(String::from)).collect();
        assert_eq!(rp::parse_sim_team(&t, &names, &species).repr(), repr(&c[2]), "parse_sim_team {}", c[0]);
    }
    for c in g["battle_data"].as_array().unwrap() {
        assert_eq!(Value::List(rp::parse_battle_data(c[0].as_str())).repr(), repr(&c[1]), "parse_battle_data {}", c[0]);
    }
}

#[test]
fn votes_match_reference() {
    let g = g();
    for seq in g["votes"].as_array().unwrap() {
        let mut votes = rp::Votes::new();
        for s in seq.as_array().unwrap() {
            let k = s[0].as_str().unwrap();
            let got = rp::vote(&mut votes, k, pv(&s[1]));
            assert_eq!(got.repr(), repr(&s[2]), "vote {s}");
            assert_eq!(rp::confirmed(&votes, "a").repr(), repr(&s[3]), "confirmed a {s}");
            assert_eq!(rp::confirmed(&votes, "b").repr(), repr(&s[4]), "confirmed b {s}");
            let mut slotted = rp::Votes::new();
            for i in 0..4 {
                if let Some(c) = votes.get(k) {
                    slotted.insert(format!("n{i}"), c.clone());
                }
            }
            let agreed: Vec<i64> = rp::agreed_slots(&slotted, |i| format!("n{i}")).into_iter().collect();
            assert_eq!(json!(agreed), s[5], "agreed_slots {s}");
        }
    }
}

fn dicts(j: &J) -> Vec<Dict> {
    j.as_array().unwrap().iter().map(|p| pv(p).as_dict().unwrap().clone()).collect()
}

#[test]
fn teammates_and_slots_match_reference() {
    let g = g();
    let tm = &g["teammates"];
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("teammates.json");
    std::fs::write(&path, tm["seen"].to_string()).unwrap();
    let team = Teammates::load(path);
    for c in tm["fold"].as_array().unwrap() {
        assert_eq!(Teammates::fold(c[0].as_str().unwrap()), c[1].as_str().unwrap(), "fold {}", c[0]);
    }
    for c in tm["steady"].as_array().unwrap() {
        assert_eq!(team.steady(c[0].as_str().unwrap()), c[1].as_str().unwrap(), "steady {}", c[0]);
    }
    for c in g["pills"].as_array().unwrap() {
        let got = team::party_pill(c[0].as_str().unwrap(), c[1].as_i64().unwrap()).map(|(s, n)| json!([s, n])).unwrap_or(J::Null);
        assert_eq!(got, c[2], "party_pill {}", c[0]);
    }
    for c in g["slots"].as_array().unwrap() {
        let mut players = dicts(&c["players"]);
        let party: Vec<Value> = c["party"].as_array().unwrap().iter().map(pv).collect();
        let sure: BTreeSet<i64> = c["sure"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
        let agreed: BTreeSet<i64> = c["agreed"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
        let you = c["you"].as_str();
        rp::party_slots(&team, &mut players, Some(&party), &sure, you, &agreed);
        let after = Value::List(players.iter().cloned().map(Value::Dict).collect());
        assert_eq!(after.repr(), repr(&c["after"]), "party_slots {c}");
        let names: Vec<String> = c["players"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(rp::own_name(you, &names, Some(&party)).map_or(J::Null, |s| json!(s)), c["own"], "own_name {c}");
    }
}

#[test]
fn screen_signatures_match_reference() {
    let g = g();
    let fr = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../alogs-reader/tests/data/frames");
    for c in g["sigs"].as_array().unwrap() {
        let im = Image::open_png(&fr.join(c["file"].as_str().unwrap())).unwrap().convert(Mode::Rgb);
        let other = Image::open_png(&fr.join(c["other"].as_str().unwrap())).unwrap().convert(Mode::Rgb).resize(im.width, im.height, None);
        let a = team::screen_sig(&im);
        let b = team::screen_sig(&other);
        assert_eq!(json!(a), c["sig"], "screen_sig {}", c["file"]);
        assert_eq!(json!(team::top_sig_direct(&im, im.height as f64 * 0.42)), c["top"], "top_sig_direct {}", c["file"]);
        assert_eq!(json!(team::screen_changed(&a, &b, 24.0)), c["changed"]);
        let (w, o25, o40) = rp::screen_difference(&im, &other);
        assert_eq!(json!({"worst": w, "over25": o25, "over40": o40}), c["diff"], "screen_difference {}", c["file"]);
    }
}
