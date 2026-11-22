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

/// `game` from a run's record goes into its extra as it is (the goldens' records have none, so carry none); a record
/// without a build gets no `game` key.
#[test]
fn runs_carry_the_game_build_when_known() {
    use alogs_gamelog::GameBuild;
    let full = pl::game_value(&GameBuild { build: 3900001, patch: Some(3900777), patch_action: Some("TipPatch".into()) });
    assert_eq!(full.json_dumps(), r#"{"build": 3900001, "patch": 3900777, "patchAction": "TipPatch"}"#);
    let only = pl::game_value(&GameBuild { build: 3900001, patch: None, patch_action: None });
    assert_eq!(only.json_dumps(), r#"{"build": 3900001}"#);

    let g: J = serde_json::from_str(&std::fs::read_to_string(dir().join("payload_goldens.json")).unwrap()).unwrap();
    let clock = FixedClock(g["now"].as_f64().unwrap());
    let sched = Schedule::new(dir().join("cycles.json"));
    let with = |rec: &J, game: &Value| {
        let mut v = pv(rec);
        if let Value::Dict(d) = &mut v {
            d.insert("game".into(), game.clone());
        }
        v
    };
    // the golden payload with extra.game added
    let plus = |want: &J, game: &Value| {
        let mut w = pv(want);
        if let Value::Dict(d) = &mut w {
            let mut e = d.at("extra").as_dict().cloned().unwrap();
            e.insert("game".into(), game.clone());
            d.insert("extra".into(), Value::Dict(e));
        }
        w.repr()
    };
    let no_game = |p: &Value| p.get("extra").as_dict().is_some_and(|e| e.get_str("game").is_none());
    let bad = [Value::None, Value::Int(3900001), Value::json_loads(r#"{"patch": 5}"#).unwrap(), Value::json_loads(r#"{"build": "3900001"}"#).unwrap()];
    for game in [&full, &only] {
        for c in g["runs"].as_array().unwrap() {
            assert!(c["summary"].get("game").is_none());
            let p = pl::run_payload(&with(&c["summary"], game), &Value::Str("100000000009".into()), &sched, &clock);
            assert_eq!(p.repr(), plus(&c["payload"], game), "run_payload");
            assert_eq!(pl::send_check(&p, None).get("extra").get("game").repr(), game.repr(), "kept by send_check");
        }
        for c in g["sims"].as_array().unwrap() {
            let a = &c[0];
            let got = pl::sim_payload(&with(&a[0], game), &pv(&a[1]), a[2].as_f64(), &clock);
            assert_eq!(got.repr(), plus(&c[1], game), "sim_payload {a}");
        }
        for c in g["eggs"].as_array().unwrap() {
            let a = &c[0];
            let got = pl::egg_match_payload(&with(&a[0], game), &pv(&a[1]), &pv(&a[2]), a.get(3).and_then(J::as_f64), &clock);
            assert_eq!(got.repr(), plus(&c[1], game), "egg_match_payload {a}");
        }
    }
    for game in &bad {
        let c = &g["runs"][0];
        assert!(no_game(&pl::run_payload(&with(&c["summary"], game), &Value::Str("1".into()), &sched, &clock)), "{}", game.repr());
        let a = &g["sims"][0][0];
        assert!(no_game(&pl::sim_payload(&with(&a[0], game), &pv(&a[1]), a[2].as_f64(), &clock)));
        let a = &g["eggs"][0][0];
        assert!(no_game(&pl::egg_match_payload(&with(&a[0], game), &pv(&a[1]), &pv(&a[2]), a.get(3).and_then(J::as_f64), &clock)));
    }
}

/// The meter reads the build from its Player.log and logs it when first known or changed (numbers only).
#[test]
fn the_meter_reads_and_logs_the_game_build() {
    use alogs_meter::{env::Env, meter::{Args, Meter}};
    let _ = logged("");
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("payload-game-build");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let log = data.join("Player.log");
    let env = Env::new(data.join("res"), data.clone());
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: true,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: log.clone() };
    let mut m = Meter::new(env, args, Box::new(alogs_reader::ocr::NoOcr), None);
    assert!(m.game_build_now().is_none(), "no log yet");
    std::fs::write(&log, "[17.8][Log]|PrintGameVersionInfo: deploy[xwinpublish], scm[3800005], res[1]\n").unwrap();
    assert_eq!(m.game_build_now().unwrap().json_dumps(), r#"{"build": 3800005}"#);
    assert!(logged("game build 3800005"));
    let count = || LINES.lock().unwrap().iter().filter(|l| l.starts_with("game build ")).count();
    let before = count();
    m.game_build_now();
    assert_eq!(count(), before, "logged once per change");
    std::fs::write(&log, "[17.8][Log]|PrintGameVersionInfo: deploy[xwinpublish], scm[3800005], res[1]\n\
        [48.1][Log]|MatchLuaVerList: Matched cliVer[1.1.3800005], act[TipPatch], res[3800009], url[]\n").unwrap();
    assert_eq!(m.game_build_now().unwrap().json_dumps(), r#"{"build": 3800005, "patch": 3800009, "patchAction": "TipPatch"}"#);
    assert!(logged("game build 3800005 (offered 3800009, TipPatch)"));
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

fn notice(payload: &str, reply: &str, before: Option<&pl::Announced>) -> pl::Notice {
    pl::sent_notice_for(&Value::json_loads(payload).unwrap(), Some(&Value::json_loads(reply).unwrap()), before)
}
fn shown(n: &pl::Notice) -> (String, String) {
    let (a, b, _) = n.show.clone().expect("a notice");
    (a, b)
}
const RUN: &str = r#"{"kind": "interlink_run", "clientRunId": "r1", "boss": {"name": "Made-up Boss"}, "stars": 30, "score": 12345, "extra": {"newBests": []}}"#;
const RUN_BEST: &str = r#"{"kind": "interlink_run", "clientRunId": "r1", "boss": {"name": "Made-up Boss"}, "stars": 30, "score": 12345, "extra": {"newBests": ["score"]}}"#;
const DOT: &str = " \u{b7} ";

/// A first send that beat nothing says the run was submitted, with today's details.
#[test]
fn a_first_send_not_improved_is_submitted() {
    let n = notice(RUN, r#"{"submissionId": "s1"}"#, None);
    assert_eq!(shown(&n), ("Interlink run submitted".into(), format!("Made-up Boss{DOT}30 stars{DOT}12,345")));
    assert_eq!(n.announce, Some(pl::Announced { improved: false, figure: Some((30, 12345)) }));
    let sim = notice(r#"{"kind": "sim_run", "clientRunId": "s", "trial": "T", "difficulty": "Hard"}"#, "{}", None);
    assert_eq!(shown(&sim), ("Holo-Battle Sim run submitted".into(), format!("T{DOT}Hard")));
    let egg = notice(r#"{"kind": "eggheist_match", "clientRunId": "e", "difficulty": "Hard", "score": 40}"#, "{}", None);
    assert_eq!(shown(&egg), ("Egg Heist match submitted".into(), format!("Hard{DOT}40")));
}

/// Without the site's word, this PC's bests (newBests has "score") make an Interlink run's ranking updated.
#[test]
fn a_first_send_improved_by_this_pcs_bests() {
    let n = notice(RUN_BEST, "{}", None);
    assert_eq!(shown(&n).0, "Interlink ranking updated");
    assert_eq!(shown(&n).1, format!("Made-up Boss{DOT}30 stars{DOT}12,345"));
    assert_eq!(n.why, "this PC's bests");
    // only Interlink has bests on this PC
    let egg = notice(r#"{"kind": "eggheist_match", "clientRunId": "e", "score": 40, "extra": {"newBests": ["score"]}}"#, "{}", None);
    assert_eq!(shown(&egg).0, "Egg Heist match submitted");
}

/// The site's standing wins over this PC's bests, and its placement goes last.
#[test]
fn a_first_send_with_the_sites_standing() {
    let n = notice(RUN, r#"{"standing": {"best": true, "position": 2, "of": 1041}}"#, None);
    assert_eq!(shown(&n), ("Interlink ranking updated".into(), format!("Made-up Boss{DOT}30 stars{DOT}12,345{DOT}#2 of 1,041")));
    assert_eq!(n.why, "the site's standing: #2 of 1,041");
    let n = notice(RUN, r#"{"standing": {"best": true, "position": 1234}}"#, None);
    assert_eq!(shown(&n).1, format!("Made-up Boss{DOT}30 stars{DOT}12,345{DOT}#1,234"));
    let n = notice(RUN, r#"{"standing": {"best": true}}"#, None);
    assert_eq!(shown(&n), ("Interlink ranking updated".into(), format!("Made-up Boss{DOT}30 stars{DOT}12,345")));
    let n = notice(RUN_BEST, r#"{"standing": {"best": false, "position": 3, "of": 9}}"#, None);
    assert_eq!(shown(&n), ("Interlink run submitted".into(), format!("Made-up Boss{DOT}30 stars{DOT}12,345")));
    assert_eq!(n.announce.unwrap().improved, false);
    let sim = notice(r#"{"kind": "sim_run", "clientRunId": "s", "trial": "T", "difficulty": "Hard"}"#, r#"{"standing": {"best": true, "position": 5, "of": 20}}"#, None);
    assert_eq!(shown(&sim), ("Holo-Battle Sim ranking updated".into(), format!("T{DOT}Hard{DOT}#5 of 20")));
    let egg = notice(r#"{"kind": "eggheist_match", "clientRunId": "e", "difficulty": "Hard", "score": 40}"#, r#"{"standing": {"best": true}}"#, None);
    assert_eq!(shown(&egg).0, "Egg Heist ranking updated");
}

/// A standing of the wrong shape is read as not there, piece by piece.
#[test]
fn a_malformed_standing_is_not_there() {
    for reply in [r#"{"standing": "best"}"#, r#"{"standing": [true]}"#, r#"{"standing": {"best": 1}}"#, r#"{"standing": {"best": "yes"}}"#] {
        assert_eq!(shown(&notice(RUN, reply, None)).0, "Interlink run submitted", "{reply}");
        assert_eq!(shown(&notice(RUN_BEST, reply, None)).0, "Interlink ranking updated", "{reply}");
    }
    let bad_place = [
        r#"{"standing": {"best": true, "position": 0, "of": 5}}"#,
        r#"{"standing": {"best": true, "position": "2", "of": 5}}"#,
        r#"{"standing": {"best": true, "position": true}}"#,
        r#"{"standing": {"best": true, "position": 2.0}}"#,
    ];
    for reply in bad_place {
        assert_eq!(shown(&notice(RUN, reply, None)).1, format!("Made-up Boss{DOT}30 stars{DOT}12,345"), "{reply}");
    }
    let bad_of = notice(RUN, r#"{"standing": {"best": true, "position": 7, "of": 3}}"#, None);
    assert_eq!(shown(&bad_of).1, format!("Made-up Boss{DOT}30 stars{DOT}12,345{DOT}#7"));
}

/// A run's update gets a notice only when its standing got better than its notice said.
#[test]
fn an_update_shows_only_a_better_standing() {
    let was = |improved, figure| pl::Announced { improved, figure };
    let up = r#"{"updated": true}"#;
    // nothing changed: silent
    let n = notice(RUN, up, Some(&was(false, Some((30, 12345)))));
    assert_eq!(n.show, None);
    assert_eq!(n.why, "no notice for this update of the run (its notice is up already)");
    let n = notice(RUN_BEST, up, Some(&was(true, Some((30, 12345)))));
    assert_eq!(n.show, None);
    // it improved now (announced as not)
    let n = notice(RUN_BEST, up, Some(&was(false, Some((30, 12345)))));
    assert_eq!(shown(&n).0, "Interlink ranking updated");
    assert_eq!(n.why, "this PC's bests; an update, better than its notice");
    // a higher figure than the improved notice said
    let n = notice(RUN, r#"{"updated": true, "standing": {"best": true, "position": 1, "of": 41}}"#, Some(&was(true, Some((30, 12000)))));
    assert_eq!(shown(&n), ("Interlink ranking updated".into(), format!("Made-up Boss{DOT}30 stars{DOT}12,345{DOT}#1 of 41")));
    // higher but not improved: silent
    assert_eq!(notice(RUN, up, Some(&was(false, Some((29, 99999))))).show, None);
    // a run the meter has no memory of
    let n = notice(RUN_BEST, up, None);
    assert_eq!(n.show, None);
    assert_eq!(n.why, "no notice for this update of the run (the meter has no notice of it)");
}

/// The Egg Heist rank and the Interlink best keep "submitted" on a first send and "updated" on an update.
#[test]
fn season_and_best_records_say_submitted_then_updated() {
    let season = r#"{"kind": "eggheist_season", "clientRunId": "eggheist-S2", "rank": {"name": "Gold"}, "score": 1319}"#;
    assert_eq!(shown(&notice(season, "{}", None)), ("Egg Heist rank submitted".into(), format!("Gold{DOT}1,319")));
    assert_eq!(shown(&notice(season, r#"{"updated": true}"#, None)).0, "Egg Heist rank updated");
    let best = r#"{"kind": "interlink_best", "boss": {"name": "Made-up Boss"}, "stars": 3, "score": 9}"#;
    assert_eq!(shown(&notice(best, "{}", None)).0, "Interlink best submitted");
    assert_eq!(shown(&notice(best, r#"{"updated": true, "standing": {"best": true, "position": 1}}"#, None)).0, "Interlink best updated");
    assert_eq!(notice(r#"{"kind": "interlink_presence", "clientRunId": "r1"}"#, "{}", None).show, None);
}
