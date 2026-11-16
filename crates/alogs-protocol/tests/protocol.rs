//! Parity with aLogs B16 `settings.py`, `plugins.py` and `notice_window.py`'s `take_notices`, from goldens written by
//! tools/gen_protocol_goldens.py.

use alogs_protocol::notices;
use alogs_protocol::plugins::{Health, Manifest, SwitchWatch};
use alogs_protocol::settings;
use alogs_pyval::Value;
use serde_json::Value as J;

fn goldens() -> J {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/protocol_goldens.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn pv(j: &J) -> Value {
    Value::from_json(j)
}

#[test]
fn settings_load_and_save_match_reference() {
    let g = goldens();
    for case in g["settings"].as_array().unwrap() {
        let cfg = settings::load_from(case["stored"].as_str());
        assert_eq!(cfg.repr(), case["load"].as_str().unwrap(), "load {}", case["stored"]);
        assert_eq!(settings::save_text(&cfg), case["save"].as_str().unwrap(), "save {}", case["stored"]);
    }
    for case in g["automation"].as_array().unwrap() {
        let got = settings::automation(&pv(&case["cfg"]), case["what"].as_str());
        assert_eq!(got, case["on"].as_bool().unwrap(), "automation {}", case["what"]);
    }
}

#[test]
fn plugins_match_reference() {
    let g = goldens();
    let m = Manifest::bundled();
    for case in g["plugins"].as_array().unwrap() {
        let cfg = pv(&case["cfg"]);
        let meter_on = case["meter_on"].as_bool().unwrap();
        let mut h = Health::new();
        for pid in case["failed"].as_array().unwrap() {
            for t in 0..3 {
                h.error(pid.as_str().unwrap(), "x", "x", "x", f64::from(t));
            }
        }
        for (pid, want) in case["switched_on"].as_object().unwrap() {
            assert_eq!(m.switched_on(&cfg, pid, meter_on), want.as_bool().unwrap(), "{pid} {}", case["cfg"]);
        }
        assert_eq!(Value::Dict(m.notice_owners(&cfg, meter_on, &h)).repr(), case["notice_owners"].as_str().unwrap());
        assert_eq!(m.meter_mode(!meter_on, &cfg, &h), case["meter_mode"].as_str().unwrap(), "{}", case["cfg"]);
    }
}

#[test]
fn health_matches_reference() {
    let g = goldens();
    for case in g["health"].as_array().unwrap() {
        let mut h = Health::new();
        let steps = case["steps"].as_array().unwrap();
        let times = case["times"].as_array().unwrap();
        for (t, want) in times.iter().zip(steps) {
            h.error("leaderboards", "leaderboards", r#"ValueError("bad 'x'")"#, "send", t.as_f64().unwrap());
            assert_eq!(h.failed("leaderboards"), want["failed"].as_bool().unwrap(), "{times:?} at {t}");
            assert_eq!(h.status("leaderboards").repr(), want["status"].as_str().unwrap());
        }
        h.reset("leaderboards");
        let last = steps.last().unwrap();
        assert_eq!(h.failed("leaderboards"), last["failed"].as_bool().unwrap());
        assert_eq!(h.status("leaderboards").repr(), last["status"].as_str().unwrap());
    }
}

#[test]
fn guarded_and_switch_watch() {
    let m = Manifest::bundled();
    let mut h = Health::new();
    for t in 0..3 {
        let v = h.guarded(&m, "leaderboards", "send", f64::from(t), -1, || Err::<i32, _>("boom".to_string()));
        assert_eq!(v, -1);
    }
    assert!(h.failed("leaderboards"));
    assert_eq!(h.guarded(&m, "leaderboards", "send", 4.0, 0, || Ok(7)), 0, "a stopped plug-in doesn't run");
    let mut w = SwitchWatch::default();
    let on = Value::json_loads(r#"{"leaderboard": {"submit": true}}"#).unwrap();
    let off = Value::json_loads(r#"{"leaderboard": {"submit": false}}"#).unwrap();
    w.tick(&m, &on, true, &mut h);
    assert!(h.failed("leaderboards"));
    w.tick(&m, &off, true, &mut h);
    w.tick(&m, &on, true, &mut h);
    assert!(!h.failed("leaderboards"), "off and on again: another go");
    assert_eq!(h.guarded(&m, "dps", "draw", 9.0, 0, || -> Result<i32, String> { panic!("drawn wrong") }), 0);
}

#[test]
fn notices_match_reference() {
    let g = goldens();
    for case in g["notices"].as_array().unwrap() {
        let text = case["text"].as_str().unwrap();
        let got = notices::parse(text);
        if case.get("raised").is_some() {
            // The Python's loop raises on a file that is JSON but not an object (and leaves it there, so every look
            // raises again). Here it's dropped and deleted like any other unreadable notice.
            assert!(got.is_err(), "{text}");
            continue;
        }
        let want = case["queue"].as_array().unwrap();
        match (got, want.first()) {
            (Ok(Some(n)), Some(w)) => {
                assert_eq!(n.head, w["head"].as_str().unwrap(), "{text}");
                assert_eq!(n.detail, w["detail"].as_str().unwrap(), "{text}");
                assert_eq!(n.accent, w["accent"].as_str().unwrap(), "{text}");
                assert_eq!(n.kind, w["kind"].as_str().unwrap(), "{text}");
                let ws = w["screen"].as_array().map(|s| s.iter().map(|v| v.as_i64().unwrap()).collect::<Vec<_>>());
                assert_eq!(n.screen.map(|s| s.to_vec()), ws, "{text}");
            }
            (Ok(None) | Err(()), None) => {}
            (got, w) => panic!("{text}: got {got:?}, reference {w:?}"),
        }
    }
}

#[test]
fn notice_files_round_trip() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("notices-test");
    let _ = std::fs::remove_dir_all(&dir);
    let n = notices::Notice {
        head: "Key room".into(),
        detail: "North".into(),
        accent: "#ffaa00".into(),
        screen: Some([0, 0, 1280, 720]),
        kind: "sanctum".into(),
    };
    let p1 = notices::write(&dir, &n).unwrap();
    std::fs::write(dir.join("junk.json"), "[1]").unwrap();
    let name = p1.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(name.split('-').next().unwrap().len(), 20, "20-digit ticks");
    let got = notices::take(&dir);
    assert_eq!(got, vec![n]);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "all taken, junk too");
}
