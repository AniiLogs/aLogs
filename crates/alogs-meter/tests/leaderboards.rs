//! The leaderboards' run handling: your bests on a boss this week, and a run decided once and sent once.

use alogs_meter::env::Env;
use alogs_meter::fights::Fight;
use alogs_meter::leaderboards::{LATE_BATTLE_DATA_S, MAX_FOLLOWUPS};
use alogs_meter::meter::{Args, Meter};
use alogs_meter::results_parse::{BATTLE_DATA_SETTLE, BATTLE_DATA_WAIT};
use alogs_meter::submit::Submitter;
use alogs_pyval::{Dict, Value};
use alogs_reader::ocr::NoOcr;
use std::path::PathBuf;

fn meter(name: &str) -> Meter {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(data.join("logs").join("fights")).unwrap();
    let env = Env::new(data.join("res"), data);
    env.fixed_now.set(Some(1_800_000_000.0));
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: true,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let mut m = Meter::new(env, args, Box::new(NoOcr), None);
    m.uid = Some("1".into());
    m
}

fn run(stars: i64, score: i64, time_s: i64, dps: i64) -> Dict {
    let j = format!(r#"{{"puppetId": "2101", "cycle": "c1", "result": {{"rating_stars": {stars}, "score": {score}, "time_s": {time_s},
        "cleared": true, "players": [{{"name": "a", "self": true}}]}},
        "battle_data": {{"players": [{{"name": "a", "avg_dps": {dps}, "break": 0, "taken": 10, "heal": null}}]}}}}"#);
    match Value::json_loads(&j).unwrap() { Value::Dict(d) => d, _ => unreachable!() }
}

#[test]
fn bests_are_kept_per_boss_and_beaten_only_by_more() {
    let m = meter("lb-bests");
    assert_eq!(m.beats_best(&run(3, 1000, 90, 500)), ["score", "clear", "dps", "taken"]);
    assert_eq!(m.beats_best(&run(3, 1000, 90, 500)), Vec::<String>::new());
    assert_eq!(m.beats_best(&run(3, 900, 80, 600)), ["clear", "dps"]);
    assert_eq!(m.beats_best(&run(4, 10, 200, 1)), ["score"]);
    let bests = std::fs::read_to_string(m.env.data.join("logs/bests.json")).unwrap();
    assert!(bests.contains("\"1|c1|2101\""), "{bests}");
}

#[test]
fn a_run_is_decided_once_and_marked_sent() {
    let mut m = meter("lb-decide");
    let path = m.run_path("f1");
    std::fs::write(&path, Value::Dict(run(3, 1000, 90, 500)).json_dumps()).unwrap();
    assert!(m.run_to_send("f1"));
    m.rs.pending_run = Some((path.clone(), 1_799_999_999.0));
    m.check_pending_run(1_800_000_000.0);
    assert!(m.rs.pending_run.is_none());
    let s = std::fs::read_to_string(&path).unwrap();
    assert!(s.contains("\"submit\": true") && s.contains("\"sent\": true") && s.contains("\"new_bests\""), "{s}");
    assert!(m.run_to_send("f1"), "inside its window what follows the run is still sent, as its update");
    m.decide_run(&path); // already decided: nothing changes
    assert_eq!(std::fs::read_to_string(&path).unwrap(), s);
    m.env.fixed_now.set(Some(1_800_000_000.0 + LATE_BATTLE_DATA_S + 1.0));
    assert!(!m.run_to_send("f1"), "after its window nothing more goes");
}

const T: f64 = 1_800_000_000.0;

/// A meter with submissions on (the sender isn't running: what is queued stays in the queue to look at).
fn sending(name: &str, screenshots: bool) -> Meter {
    let mut m = meter(name);
    let d = m.env.data.clone();
    std::fs::write(d.join("settings.json"),
        format!(r#"{{"leaderboard": {{"submit": true, "token": "alb_test", "api_url": "http://127.0.0.1:9", "include_screenshots": {screenshots}}}}}"#)).unwrap();
    m.submitter = Some(Submitter::new(d));
    m
}

fn pic(m: &Meter, rel: &str) {
    let p = m.env.data.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"made-up picture").unwrap();
}

fn on_disk(m: &Meter, rel: &str) -> bool {
    m.env.data.join(rel).is_file()
}

const RESULT_PIC: &str = "logs/screens/result_20270115_080000.jpg";
const PARTY_PIC: &str = "logs/screens/party_20270115_080000.jpg";

/// A finished Interlink run's summary, its result screen read, no Battle Data yet.
fn summary(m: &Meter, csv: &str, pictures: bool) -> PathBuf {
    let shot = if pictures { format!(r#""{RESULT_PIC}""#) } else { "null".into() };
    let party = if pictures { format!(r#", "party_screenshot": "{PARTY_PIC}""#) } else { String::new() };
    let j = format!(r#"{{"boss": "Made-up Boss", "puppetId": "2101", "level": 50, "uid": "1", "cycle": "c1", "started": "2027-01-15 08:00:00",
        "bossrush": true, "interlink_scene": true, "meter": {{"csv": "{csv}.csv", "damage": 1000, "seconds": 90.0, "dps": 11}},
        "result": {{"rating_stars": 3, "score": 1000, "time_s": 90, "cleared": true, "screenshot": {shot},
                    "players": [{{"slot": 0, "name": "Made-up One", "self": true, "stat": "DMG Dealt", "value": 1000}}]}}{party}}}"#);
    if pictures {
        pic(m, RESULT_PIC);
        pic(m, PARTY_PIC);
    }
    let path = m.run_path(csv);
    std::fs::write(&path, j).unwrap();
    path
}

fn edit(path: &PathBuf, f: impl FnOnce(&mut Dict)) {
    let Value::Dict(mut s) = Value::json_loads(&std::fs::read_to_string(path).unwrap()).unwrap() else { unreachable!() };
    f(&mut s);
    std::fs::write(path, Value::Dict(s).json_dumps()).unwrap();
}

fn read(path: &PathBuf) -> Value {
    Value::json_loads(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Battle Data's numbers for the run (and its picture when given).
fn battle_data(m: &Meter, path: &PathBuf, dps: i64, shot: Option<&str>) {
    if let Some(s) = shot {
        pic(m, s);
    }
    let bd = Value::json_loads(&format!(r#"{{"players": [{{"slot": 0, "name": "Made-up One", "avg_dps": {dps}, "break": 1, "taken": 2, "heal": 3}}],
        "team_avg_dps": {dps}, "screenshot": {}}}"#, shot.map_or("null".into(), |s| format!("\"{s}\"")))).unwrap();
    edit(path, |s| s.insert("battle_data".into(), bd));
}

fn queue_file(m: &Meter, csv: &str) -> PathBuf {
    m.env.data.join("logs").join("submit_queue").join(format!("interlink_run__{csv}.json"))
}

fn queued(m: &Meter, csv: &str) -> Option<Value> {
    std::fs::read_to_string(queue_file(m, csv)).ok().and_then(|t| Value::json_loads(&t))
}

/// The sender got the copy up: it leaves the queue and its pictures are gone (Submitter::drop_proof).
fn uploaded(m: &Meter, csv: &str) -> Value {
    let item = queued(m, csv).expect("a copy was queued");
    std::fs::remove_file(queue_file(m, csv)).unwrap();
    for k in ["evidence", "battle", "party"] {
        if item.get(k).truthy() {
            let _ = std::fs::remove_file(m.env.data.join(item.get(k).py_str()));
        }
    }
    item
}

fn avg_dps(item: &Value) -> Value {
    item.get("payload").get("players").as_list().unwrap()[0].get("avgDps").clone()
}

fn interlink_fight(m: &Meter, csv: &str) -> Fight {
    let mut row = alogs_reader::boss::Row::new();
    for (k, v) in [("puppetId", "2101"), ("table", "puppet_data"), ("bossTitle_en", "Made-up Boss")] {
        row.insert(k.into(), v.into());
    }
    let mut f = Fight::new(&m.env, "Made-up Boss", 50, row, 100_000, true, false, None, None);
    f.scene_ok = true;
    f.csv_name = csv.into();
    f
}

fn result_read() -> Value {
    Value::json_loads(r#"{"rating_stars": 3, "score": 1000, "time_s": 90, "cleared": true, "screenshot": null,
        "players": [{"slot": 0, "name": "Made-up One", "self": true, "stat": "DMG Dealt", "value": 1000}]}"#).unwrap()
}

/// A result screen read whole is sent at once; one not read whole still waits BATTLE_DATA_WAIT for Battle Data.
#[test]
fn a_result_read_whole_is_sent_at_once() {
    let mut m = sending("lb-at-once", false);
    let f = interlink_fight(&m, "fight_a");
    m.rs.result_seen = T;
    m.rs.result_ready = T;
    m.fight_summary(&f, vec![("result", result_read())]);
    m.send_when_read(&f);
    assert!(m.rs.pending_run.is_none());
    let s = read(&m.run_path("fight_a"));
    assert!(s.get("submit").truthy() && s.get("sent").truthy(), "{}", s.json_dumps());
    assert!(queued(&m, "fight_a").is_some(), "the run is queued now");

    let f = interlink_fight(&m, "fight_b");
    m.rs.result_seen = T + 1.0;
    m.fight_summary(&f, vec![("result", result_read())]);
    m.send_when_read(&f);
    assert_eq!(m.rs.pending_run, Some((m.run_path("fight_b"), T + BATTLE_DATA_WAIT)), "not read whole: it waits");
    assert!(!read(&m.run_path("fight_b")).get("sent").truthy());
    assert!(queued(&m, "fight_b").is_none());
}

/// A later read of the result screen that corrects a name, after the first look at the update (6 s after the send):
/// the update goes BATTLE_DATA_SETTLE after that read. A later read that changes nothing (its read count aside) sends
/// nothing.
#[test]
fn a_later_result_read_that_corrects_a_name_goes_as_an_update() {
    let mut m = sending("lb-later-read", false);
    let f = interlink_fight(&m, "fight_a");
    m.rs.result_seen = T;
    m.rs.result_ready = T;
    let read_as = |name: &str, reads: i64| {
        Value::json_loads(&format!(r#"{{"rating_stars": 3, "score": 1000, "time_s": 90, "cleared": true, "screenshot": null, "reads": {reads},
            "players": [{{"slot": 0, "name": "{name}", "self": true, "stat": "DMG Dealt", "value": 1000}}]}}"#)).unwrap()
    };
    m.fight_summary(&f, vec![("result", read_as("Made-up Won", 1))]);
    m.send_when_read(&f);
    uploaded(&m, "fight_a");
    m.env.fixed_now.set(Some(T + 7.0));
    m.check_followup(T + 7.0);
    assert!(queued(&m, "fight_a").is_none(), "the first look: nothing changed yet");

    m.env.fixed_now.set(Some(T + 10.0));
    m.result_read_again(&f, read_as("Made-up One", 2), T + 10.0);
    m.env.fixed_now.set(Some(T + 15.0));
    m.check_followup(T + 15.0);
    assert!(queued(&m, "fight_a").is_none(), "not before BATTLE_DATA_SETTLE after the read");
    m.env.fixed_now.set(Some(T + 16.5));
    m.check_followup(T + 16.5);
    let upd = queued(&m, "fight_a").expect("the corrected name goes as an update");
    assert_eq!(upd.get("payload").get("players").as_list().unwrap()[0].get("name").py_str(), "Made-up One");
    uploaded(&m, "fight_a");

    m.env.fixed_now.set(Some(T + 20.0));
    m.result_read_again(&f, read_as("Made-up One", 3), T + 20.0);
    m.env.fixed_now.set(Some(T + 26.5));
    m.check_followup(T + 26.5);
    assert!(queued(&m, "fight_a").is_none(), "an unchanged read sends nothing");
    assert_eq!(read(&m.run_path("fight_a")).get("followups").as_i64(), Some(1));
}

/// Another run waiting to be sent isn't dropped when the next one is scheduled: it goes first, on what it has.
#[test]
fn a_waiting_run_is_sent_before_the_next_is_scheduled() {
    let mut m = sending("lb-two-waiting", false);
    let a = summary(&m, "fight_a", false);
    m.schedule_run(a.clone(), false, T);
    let b = summary(&m, "fight_b", false);
    m.schedule_run(b.clone(), false, T + 30.0);
    assert!(read(&a).get("sent").truthy(), "the first run went");
    assert_eq!(m.rs.pending_run, Some((b, T + 30.0 + BATTLE_DATA_WAIT)));
}

/// Battle Data read complete after the first send: one update with its cards and its picture (the result screen as
/// its evidence: a copy of the one kept, since the first copy's picture went with it); nothing again when nothing
/// changed; MAX_FOLLOWUPS at most.
#[test]
fn battle_data_after_the_send_goes_as_an_update() {
    let mut m = sending("lb-update", true);
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path);
    let first = uploaded(&m, "fight_a");
    assert_eq!(first.get("evidence").py_str(), RESULT_PIC);
    assert!(avg_dps(&first).is_none(), "no Battle Data in the first copy");
    let late = read(&path).get("late_result").py_str();
    assert!(on_disk(&m, &late), "a copy of the result screen is kept for the update");

    battle_data(&m, &path, 500, Some("logs/battle_data/20270115_080010.jpg"));
    m.send_late_battle_data("fight_a");
    let upd = queued(&m, "fight_a").expect("the update is queued");
    assert_eq!(avg_dps(&upd).as_i64(), Some(500));
    assert_eq!(upd.get("payload").get("clientRunId").py_str(), "fight_a", "the same record");
    assert_eq!(upd.get("battle").py_str(), "logs/battle_data/20270115_080010.jpg");
    let ev = upd.get("evidence").py_str();
    assert!(ev.starts_with("logs/screens/resend_") && on_disk(&m, &ev), "{ev}");
    assert!(on_disk(&m, &late), "the kept copy stays for a later update");
    uploaded(&m, "fight_a");

    m.send_late_battle_data("fight_a");
    m.check_followup(T + 60.0);
    assert!(queued(&m, "fight_a").is_none(), "nothing changed: nothing sent");

    for dps in [501, 502, 503] {
        battle_data(&m, &path, dps, None);
        m.send_late_battle_data("fight_a");
    }
    assert_eq!(read(&path).get("followups").as_i64(), Some(MAX_FOLLOWUPS));
    let last = queued(&m, "fight_a").unwrap();
    assert_eq!(avg_dps(&last).as_i64(), Some(502), "the fourth update didn't go");
    assert!(!last.get("evidence").truthy() && !last.get("battle").truthy(), "no new picture: none sent");
}

/// With "Attach the screenshots as proof" off there is no picture to keep, and Battle Data's numbers still follow.
#[test]
fn without_screenshots_the_numbers_still_follow() {
    let mut m = sending("lb-numbers-only", false);
    let path = summary(&m, "fight_a", false);
    m.decide_run(&path);
    uploaded(&m, "fight_a");
    assert!(!read(&path).get("late_result").truthy());
    battle_data(&m, &path, 500, None);
    m.send_late_battle_data("fight_a");
    let upd = queued(&m, "fight_a").expect("the update is queued");
    assert_eq!(avg_dps(&upd).as_i64(), Some(500));
    assert!(!upd.get("evidence").truthy() && !upd.get("battle").truthy());
}

/// After the window (LATE_BATTLE_DATA_S from the first send) nothing follows the run, and the result screen kept for
/// its update is gone; the meter starting keeps none either.
#[test]
fn after_the_window_nothing_follows_and_no_copy_is_kept() {
    let mut m = sending("lb-window", true);
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path);
    uploaded(&m, "fight_a");
    let late = read(&path).get("late_result").py_str();
    assert!(on_disk(&m, &late));
    m.check_followup(T + BATTLE_DATA_SETTLE + 1.0);
    assert!(on_disk(&m, &late), "the window is still open");
    assert!(m.rs.followup.is_some());

    let after = T + LATE_BATTLE_DATA_S + 1.0;
    m.env.fixed_now.set(Some(after));
    battle_data(&m, &path, 500, Some("logs/battle_data/20270115_080310.jpg"));
    m.send_late_battle_data("fight_a");
    m.check_followup(after);
    assert!(queued(&m, "fight_a").is_none(), "nothing after the window");
    assert!(m.rs.followup.is_none());
    assert!(!on_disk(&m, &late), "the kept result screen is gone with its window");
    assert!(!read(&path).get("late_result").truthy());

    pic(&m, "logs/screens/late_result_20270101_000000.jpg");
    m.forget_late_results();
    assert!(!on_disk(&m, "logs/screens/late_result_20270101_000000.jpg"), "the meter starting keeps no copy");
}

/// An update that replaces a first copy still in the queue names the original result screen and keeps the first
/// copy's pictures (none is left behind); a later one with no new picture carries them too.
#[test]
fn an_update_replacing_an_unsent_copy_keeps_its_pictures() {
    let mut m = sending("lb-replace", true);
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path);
    battle_data(&m, &path, 500, Some("logs/battle_data/20270115_080010.jpg"));
    m.send_late_battle_data("fight_a");
    let upd = queued(&m, "fight_a").unwrap();
    assert_eq!(upd.get("evidence").py_str(), RESULT_PIC, "the original result screen");
    assert_eq!(upd.get("battle").py_str(), "logs/battle_data/20270115_080010.jpg");
    assert_eq!(upd.get("party").py_str(), PARTY_PIC);
    let screens: Vec<String> = std::fs::read_dir(m.env.data.join("logs/screens")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(!screens.iter().any(|n| n.starts_with("resend_")), "no copy made: {screens:?}");

    // the result screen's later reads corrected the score: no new picture, the queued copy's still go
    edit(&path, |s| {
        let Value::Dict(mut r) = s.at("result").clone() else { unreachable!() };
        r.insert("score".into(), Value::Int(1001));
        s.insert("result".into(), Value::Dict(r));
    });
    m.check_followup(T + BATTLE_DATA_SETTLE + 1.0);
    let upd = queued(&m, "fight_a").unwrap();
    assert_eq!(upd.get("payload").get("score").as_i64(), Some(1001));
    for (k, v) in [("evidence", RESULT_PIC), ("battle", "logs/battle_data/20270115_080010.jpg"), ("party", PARTY_PIC)] {
        assert_eq!(upd.get(k).py_str(), v);
        assert!(on_disk(&m, v));
    }
}

/// Holding inputs: never while the automatic Battle Data session runs (its own hold stays), nothing with Automation
/// off; and a run's first copy reaching the site doesn't end the hold waiting for its update.
#[test]
fn the_hold_waits_for_the_copy_it_was_started_for() {
    let mut m = sending("lb-hold-copy", true);
    m.args.replay = false; // (only the question is asked here: no hold is started)
    assert!(!m.holding_for_send(), "Automation off: nothing held");
    if let Value::Dict(d) = &mut m.args.cfg {
        d.insert("leaderboard".into(), alogs_pyval::dict([("submit", Value::Bool(true)), ("automation", Value::Bool(true))]));
        d.insert("results".into(), alogs_pyval::dict([("hold_inputs", Value::Bool(true))]));
    }
    assert!(m.holding_for_send());
    m.auto.auto_until = T + 60.0;
    assert!(!m.holding_for_send(), "the automatic session's hold isn't touched by a send");
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path); // sent while the session runs: no hold of its own
    assert_eq!(m.send_waiting, None);
    m.auto.auto_until = 0.0;
    m.args.replay = true;

    let first = queued(&m, "fight_a").unwrap().get("payload").clone();
    battle_data(&m, &path, 500, Some("logs/battle_data/20270115_080010.jpg"));
    m.send_late_battle_data("fight_a");
    let update = queued(&m, "fight_a").unwrap().get("payload").clone();
    // what send_result_held keeps when it holds for the update
    m.send_waiting = Some("fight_a".into());
    m.send_waiting_copy = Some(update.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    let reply = Value::json_loads(r#"{"submissionId": "s1"}"#).unwrap();
    let mut out = Said::default();
    tx.send((first, reply.clone())).unwrap();
    m.take_sent(&mut out, &rx);
    assert_eq!(m.send_waiting.as_deref(), Some("fight_a"), "the first copy's arrival ended the update's hold");
    tx.send((update, reply)).unwrap();
    m.take_sent(&mut out, &rx);
    assert_eq!(m.send_waiting, None);
    assert!(m.send_waiting_copy.is_none());
}

#[derive(Default)]
struct Said(Vec<(String, Value)>);
impl alogs_meter::meter::Out for Said {
    fn out(&mut self, kind: &str, data: Value) {
        self.0.push((kind.to_string(), data));
    }
}

/// "Tell me when a record is sent to the leaderboards": what the sender got through comes back to the meter's loop
/// (Submitter::on_sent) and gets its notice over the game; with the setting off, none.
#[test]
fn a_sent_record_gets_its_notice_unless_turned_off() {
    let mut m = meter("lb-sent-notice");
    let (tx, rx) = std::sync::mpsc::channel();
    let payload = Value::json_loads(r#"{"kind": "interlink_run", "boss": {"name": "Made-up Boss"}, "stars": 3, "score": 12345}"#).unwrap();
    let reply = Value::json_loads(r#"{"submissionId": "s1"}"#).unwrap();
    tx.send((payload.clone(), reply.clone())).unwrap();
    let mut out = Said::default();
    m.take_sent(&mut out, &rx);
    assert_eq!(out.0.len(), 1, "{:?}", out.0.iter().map(|o| o.1.json_dumps()).collect::<Vec<_>>());
    assert_eq!(out.0[0].0, "toast");
    let said = out.0[0].1.json_dumps();
    assert!(said.contains("Interlink run sent") && said.contains("Made-up Boss") && said.contains("12,345"), "{said}");
    m.take_sent(&mut out, &rx); // nothing new: nothing said again
    assert_eq!(out.0.len(), 1);

    if let Value::Dict(d) = &mut m.args.cfg {
        d.insert("leaderboard".into(), alogs_pyval::dict([("notify_sent", Value::Bool(false))]));
    }
    tx.send((payload, reply)).unwrap();
    m.take_sent(&mut out, &rx);
    assert_eq!(out.0.len(), 1, "a notice with the setting off");
    assert!(rx.try_recv().is_err(), "the record was taken in all the same");
}

/// Inputs held for a result come back when that result is sent, whatever else is still waiting to go.
#[test]
fn the_hold_ends_when_its_own_record_is_sent() {
    let mut m = meter("lb-hold-own");
    let (tx, rx) = std::sync::mpsc::channel();
    let record = |run: &str| Value::json_loads(&format!(r#"{{"kind": "sim_run", "clientRunId": "{run}", "trial": "Made-up Trial"}}"#)).unwrap();
    let reply = Value::json_loads(r#"{"submissionId": "s1"}"#).unwrap();
    let mut out = Said::default();
    m.send_waiting = Some("run-2".into());
    tx.send((record("run-1"), reply.clone())).unwrap();
    m.take_sent(&mut out, &rx);
    assert_eq!(m.send_waiting.as_deref(), Some("run-2"), "another record's arrival ended the hold");
    tx.send((record("run-2"), reply)).unwrap();
    m.take_sent(&mut out, &rx);
    assert_eq!(m.send_waiting, None);
    assert_eq!(out.0.len(), 2, "both records got their notice");
}

/// The stand-in OCR for a result screen: each box answers what the test set.
struct Answers(indexmap::IndexMap<String, String>);
impl alogs_reader::ocr::Reader for Answers {
    fn read(&self, images: &indexmap::IndexMap<String, alogs_pil::Image>, _: &indexmap::IndexMap<String, usize>, _: &indexmap::IndexMap<String, alogs_pil::Image>) -> indexmap::IndexMap<String, String> {
        images.keys().map(|k| (k.clone(), self.0.get(k).cloned().unwrap_or_default())).collect()
    }
    fn cached(&self, _: &indexmap::IndexMap<String, alogs_pil::Image>, _: &indexmap::IndexMap<String, usize>) -> bool { true }
    fn language(&self) -> Option<String> { Some("en-US".into()) }
    fn started(&self) -> bool { true }
}

struct NoFrames;
impl alogs_meter::meter::Frames for NoFrames {
    fn grab(&mut self, _: usize, _: Option<f64>) -> Option<alogs_pil::Image> { None }
    fn window(&mut self) -> Option<usize> { None }
}

fn result_answers(name: &str) -> Box<Answers> {
    answers_for(&[(name, "Total Damage Dealt / 123456")])
}

/// A result screen's boxes: rating, score, time, and each slot's (name, highlight).
fn answers_for(slots: &[(&str, &str)]) -> Box<Answers> {
    let mut a = indexmap::IndexMap::new();
    for (k, v) in [("rating", "17"), ("score", "17,164"), ("time", "95s")] {
        a.insert(k.to_string(), v.to_string());
    }
    for (i, (n, s)) in slots.iter().enumerate() {
        a.insert(format!("name{i}"), n.to_string());
        a.insert(format!("stat{i}"), s.to_string());
    }
    Box::new(Answers(a))
}

/// "Dealt the Last Strike" is shown without a number: a slot with it counts as read without one (two agreeing looks
/// still needed), and its value stays empty. Any other highlight without its number still keeps the screen unread.
#[test]
fn the_last_strike_needs_no_number() {
    let im = alogs_pil::Image::new(alogs_pil::Mode::Rgb, 1920, 1080, &[20, 20, 20]);
    let mut out = Said::default();
    let run = |name: &str, second: &str, out: &mut Said| -> Meter {
        let mut m = sending(name, false);
        m.args.replay = false; // (no game window: the click is tried, seen by its time, nothing clicked on the PC)
        m.last_fight = Some((interlink_fight(&m, "fight_a"), T));
        m.auto.auto_until = T + 60.0;
        m.auto.hold_started = T;
        m.auto.last_click = T + alogs_meter::leaderboards::RESULT_PAUSE - 1.0;
        for at in [T, T + 1.6] {
            m.env.fixed_now.set(Some(at));
            m.ocr = answers_for(&[("Made-up One", "Total Damage Dealt / 123456"), ("Made-up Two", second)]);
            m.result_screen(&mut NoFrames, out, &im, at, 1920.0, 0.0, 1080.0, "Challenge Complete", None);
            if at == T {
                assert!(m.rs.result_ready < m.rs.result_seen, "one look is not enough");
                assert!(queued(&m, "fight_a").is_none());
            }
        }
        m
    };
    let m = run("lb-last-strike", "Dealt the Last Strike", &mut out);
    assert_eq!(m.rs.result_ready, T + 1.6, "read on the second, agreeing look");
    assert_eq!(m.auto.last_click, T + 1.6, "the click came with it");
    let q = queued(&m, "fight_a").expect("the run is sent then");
    let p = q.get("payload").get("players").as_list().unwrap()[1].clone();
    assert_eq!(p.get("stat").py_str(), "Dealt the Last Strike");
    assert!(p.get("value").is_none(), "no value for it: {}", p.json_dumps());

    let m = run("lb-other-no-number", "Most Healing", &mut out);
    assert!(m.rs.result_ready < m.rs.result_seen, "another highlight without its number: not read");
    assert!(queued(&m, "fight_a").is_none());
}

/// A result screen counts as read only when two looks in a row agree: a first look read whole neither clicks View
/// Battle Data nor sends; a second that reads one name differently doesn't either; a third that agrees with the
/// second does both, with its names, and the run's proof is that look's picture (the first one's file is gone). A
/// result that never reads agreeing still waits BATTLE_DATA_WAIT, as before; looks come quickly until it is read.
#[test]
fn a_result_screen_is_read_when_two_looks_agree() {
    let mut m = sending("lb-two-looks", true);
    m.args.replay = false; // (no game window: the click is tried, seen by its time, and nothing is clicked on the PC)
    m.last_fight = Some((interlink_fight(&m, "fight_a"), T));
    m.auto.auto_until = T + 60.0; // the automatic session runs
    m.auto.hold_started = T;
    let before = T + alogs_meter::leaderboards::RESULT_PAUSE - 1.0;
    m.auto.last_click = before;
    let im = alogs_pil::Image::new(alogs_pil::Mode::Rgb, 1920, 1080, &[20, 20, 20]);
    let mut out = Said::default();
    let mut look = |m: &mut Meter, at: f64, name: &str| {
        m.env.fixed_now.set(Some(at));
        m.ocr = result_answers(name);
        m.result_screen(&mut NoFrames, &mut out, &im, at, 1920.0, 0.0, 1080.0, "Challenge Complete", None);
    };
    let path = m.run_path("fight_a");
    let shot = |m: &Meter| read(&m.run_path("fight_a")).get("result").get("screenshot").py_str();

    look(&mut m, T, "Made-up Ome");
    assert!(m.rs.result_ready < m.rs.result_seen, "one look is not enough");
    assert_eq!(m.auto.last_click, before, "no click on the first look");
    assert!(queued(&m, "fight_a").is_none() && !read(&path).get("sent").truthy(), "nothing sent on the first look");
    assert_eq!(m.rs.pending_run, Some((path.clone(), T + BATTLE_DATA_WAIT)), "the fallback wait is as before");
    let first = shot(&m);
    assert!(on_disk(&m, &first), "{first}");

    // the quick next look, with or without Automation
    m.auto.auto_until = 0.0;
    m.ss.screen_read_sig = Some(vec![1, 2, 3]);
    m.top_sig = Some(vec![1, 2, 3]); // the picture hasn't changed
    m.rs.last_results = T;
    assert!(m.screen_read_due(T + 0.3, false), "a result screen not read yet is looked at again soon");
    m.auto.auto_until = T + 60.0;

    look(&mut m, T + 1.6, "Made-up One");
    assert!(m.rs.result_ready < m.rs.result_seen, "the looks disagree on a name");
    assert_eq!(m.auto.last_click, before);
    assert!(queued(&m, "fight_a").is_none());

    look(&mut m, T + 3.2, "Made-up One");
    assert_eq!(m.rs.result_ready, T + 3.2, "two looks in a row agree");
    assert_eq!(m.auto.last_click, T + 3.2, "the click came with that look");
    let q = queued(&m, "fight_a").expect("the run is sent with that look");
    assert_eq!(q.get("payload").get("players").as_list().unwrap()[0].get("name").py_str(), "Made-up One");
    let kept = shot(&m);
    assert_ne!(kept, first, "the proof is the later look's picture");
    assert!(on_disk(&m, &kept) && !on_disk(&m, &first), "{kept} kept, {first} gone");
    assert_eq!(q.get("evidence").py_str(), kept);

    // read: the looks come at the usual pace again
    m.auto.auto_until = 0.0;
    m.rs.last_results = T + 3.2;
    assert!(!m.screen_read_due(T + 3.5, false), "once read, a result screen left up costs nothing more");
}
