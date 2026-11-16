//! eggheist.rs against the Python (tools/gen_egg_goldens.py).

use alogs_meter::eggheist as e;
use alogs_pyval::Value;
use serde_json::{Value as J, json};

fn j(v: &Value) -> J {
    serde_json::from_str(&v.json_dumps()).unwrap()
}

#[test]
fn eggheist_matches_python() {
    let g: J = serde_json::from_str(include_str!("data/egg_goldens.json")).unwrap();
    let s = |v: &J| v.as_str().map(String::from);
    let i = |v: &J| v.as_i64();
    let lad: Vec<J> = e::LADDER.iter().map(|r| json!([r.0, r.1, r.2, r.3])).collect();
    assert_eq!(J::Array(lad), g["ladder"]);
    for c in g["tier_of"].as_array().unwrap() {
        assert_eq!(json!(e::tier_of(c[0].as_str())), c[1], "tier_of {}", c[0]);
    }
    for c in g["standing"].as_array().unwrap() {
        let got = e::standing(c[0].as_str(), i(&c[1]), i(&c[2])).map_or(J::Null, |v| j(&v));
        assert_eq!(got, c[3], "standing {c:?}");
    }
    for c in g["gain_possible"].as_array().unwrap() {
        assert_eq!(json!(e::gain_possible(i(&c[0]), i(&c[1]))), c[2], "gain_possible {c:?}");
    }
    for c in g["parse_points"].as_array().unwrap() {
        let (a, b) = e::parse_points(c[0].as_str());
        assert_eq!(json!([a, b]), c[1], "parse_points {}", c[0]);
    }
    for c in g["parse_gain"].as_array().unwrap() {
        assert_eq!(json!(e::parse_gain(c[0].as_str())), c[1], "parse_gain {}", c[0]);
    }
    for c in g["parse_season"].as_array().unwrap() {
        assert_eq!(json!(e::parse_season(c[0].as_str(), &s(&c[1]).unwrap())), c[2], "parse_season {c:?}");
    }
    for c in g["current_season"].as_array().unwrap() {
        assert_eq!(json!(e::current_season(c[0].as_str().unwrap())), c[1], "current_season {}", c[0]);
    }
}
