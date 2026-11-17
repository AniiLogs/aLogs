//! The leaderboards' run handling: your bests on a boss this week, and a run decided once and sent once.

use alogs_meter::env::Env;
use alogs_meter::meter::{Args, Meter};
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
    assert!(!m.run_to_send("f1"));
    m.decide_run(&path); // already decided: nothing changes
    assert_eq!(std::fs::read_to_string(&path).unwrap(), s);
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
