//! Parity with aLogs B16 `core.py` (LogWatcher, last_uid, scene_of, interlink_score, same_name) and Python's difflib,
//! from goldens written by tools/gen_gamelog_goldens.py on a made-up log.

use alogs_gamelog::{Event, FixedClock, LogWatcher, interlink_score, last_uid, same_name, scene_of};
use serde_json::{Value as J, json};
use std::path::PathBuf;

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}
fn goldens() -> J {
    serde_json::from_str(&std::fs::read_to_string(data().join("gamelog_goldens.json")).unwrap()).unwrap()
}

fn ev(e: &Event) -> J {
    let opt = |s: &Option<String>| s.as_ref().map_or(J::Null, |s| json!(s));
    let d = |d: &Option<f64>| d.map_or(J::Null, |d| json!(d));
    match e {
        Event::Uid(u) => json!(["uid", u, null]),
        Event::Enter(s, x) => json!(["enter", s, d(x)]),
        Event::Leave(s, x) => json!(["leave", s, d(x)]),
        Event::BossRush(s, x) => json!(["bossrush", opt(s), d(x)]),
        Event::HoloSim(s, x) => json!(["holosim", opt(s), d(x)]),
    }
}

#[test]
fn feeds_match_reference() {
    let g = goldens();
    let now = g["now"].as_f64().unwrap();
    let log = std::fs::read(data().join("made_up.log")).unwrap();
    for case in g["feeds"].as_array().unwrap() {
        let label = format!("{} prefill {}", case["cut"], case["prefill"]);
        let mut w = LogWatcher::new(data().join("made_up.log"), Box::new(FixedClock(now)));
        for _ in 0..case["prefill"].as_u64().unwrap() {
            w.delays.push_back(0.0);
        }
        let mut at = 0;
        for n in case["sizes"].as_array().unwrap() {
            let n = n.as_u64().unwrap() as usize;
            w.feed(&log[at..at + n]);
            at += n;
        }
        let want = &case["result"];
        let got: Vec<J> = w.events.iter().map(ev).collect();
        let want_ev = want["events"].as_array().unwrap();
        assert_eq!(got.len(), want_ev.len(), "{label}: event count");
        for (i, (a, b)) in got.iter().zip(want_ev).enumerate() {
            assert_eq!(a, b, "{label}: event {i}");
        }
        assert_eq!(w.uid.as_deref(), want["uid"].as_str(), "{label}: uid");
        assert_eq!(w.current.as_deref(), want["current"].as_str(), "{label}: current");
        let delays: Vec<f64> = want["delays"].as_array().unwrap().iter().map(|d| d.as_f64().unwrap()).collect();
        assert_eq!(w.delays.iter().copied().collect::<Vec<_>>(), delays, "{label}: delays");
        let buf: Vec<u8> = want["buf"].as_str().unwrap().chars().map(|c| c as u32 as u8).collect();
        assert_eq!(w.buf, buf, "{label}: unfinished line");
    }
}

#[test]
fn look_back_and_uids_match_reference() {
    let g = goldens();
    for case in g["look_back"].as_array().unwrap() {
        let mut w = LogWatcher::new(data().join("made_up.log"), Box::new(FixedClock(0.0)));
        w.look_back(case["size"].as_u64().unwrap(), case["span"].as_u64().unwrap());
        let got: Vec<J> = w.events.iter().map(ev).collect();
        assert_eq!(J::Array(got), case["events"], "look_back {case}");
        assert_eq!(w.current.as_deref(), case["current"].as_str());
    }
    assert_eq!(last_uid(&data().join("made_up.log")).as_deref(), g["last_uid"].as_str());
    assert_eq!(last_uid(&data().join("spaces.log")).as_deref(), g["last_uid_spaces"].as_str());
    for c in g["scene_of"].as_array().unwrap() {
        assert_eq!(scene_of(c[0].as_str()), c[1].as_i64(), "scene_of {}", c[0]);
    }
}

#[test]
fn interlink_score_matches_reference() {
    let g = goldens();
    for c in g["interlink_score"].as_array().unwrap() {
        let got = interlink_score(c[0].as_i64(), c[1].as_i64(), c[2].as_i64(), c[3].as_bool().unwrap());
        assert_eq!(got, c[4].as_i64(), "interlink_score{c}");
    }
}

#[test]
fn same_name_and_difflib_match_reference() {
    let g = goldens();
    for c in g["same_name"].as_array().unwrap() {
        assert_eq!(same_name(c[0].as_str(), c[1].as_str()), c[2].as_bool().unwrap(), "same_name {} {}", c[0], c[1]);
    }
    for c in g["ratio"].as_array().unwrap() {
        let got = alogs_gamelog::difflib::ratio(c[0].as_str().unwrap(), c[1].as_str().unwrap());
        assert_eq!(got.to_bits(), c[2].as_f64().unwrap().to_bits(), "ratio {} / {}", c[0], c[1]);
    }
}

#[test]
fn poll_follows_a_growing_log() {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("player.log");
    std::fs::write(&path, b"old createVoxelScene new>>>id:1-100000000001-550\nfb_bossrush\n").unwrap();
    let mut w = LogWatcher::new(path.clone(), Box::new(FixedClock(0.0)));
    let pos = w.poll(None);
    assert_eq!(
        w.take_events(),
        vec![
            Event::Enter("1-100000000001-550".into(), None),
            Event::BossRush(Some("1-100000000001-550".into()), None)
        ]
    );
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    f.write_all(b"net callerId=1234567 x\nclearSpace is_destroy:true id:9-9-9\nhalf a li").unwrap();
    let pos = w.poll(pos);
    assert_eq!(w.take_events(), vec![Event::Uid("1234567".into()), Event::Leave("9-9-9".into(), None)]);
    std::fs::write(&path, b"new\n").unwrap(); // the game started again: a new, shorter log
    let pos2 = w.poll(pos);
    assert_eq!(pos2, Some(4));
    assert!(w.buf.is_empty());
}
