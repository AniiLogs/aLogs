//! "I was in this run" (seat_note.rs): an Interlink fight followed to its end with no result screen read sends a note
//! so the site can link the player's seat; never when a result was read, for a fight left early, without its
//! instance, or twice. Made-up values only.

use alogs_meter::env::Env;
use alogs_meter::fights::{Fight, Timer};
use alogs_meter::meter::{Args, Meter, Out};
use alogs_meter::seat_note::SEAT_NOTE_WAIT_S;
use alogs_pyval::Value;
use alogs_reader::ocr::NoOcr;
use std::path::PathBuf;

const T: f64 = 1_800_000_000.0;
const INSTANCE: &str = "inst-made-up-1";

#[derive(Default)]
struct Log(Vec<(String, Value)>);
impl Out for Log {
    fn out(&mut self, kind: &str, data: Value) {
        self.0.push((kind.to_string(), data));
    }
}

/// A live meter with submissions on (the sender isn't running: what is queued stays to look at), in an instance.
fn meter(name: &str) -> Meter {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(data.join("logs").join("fights")).unwrap();
    let mut env = Env::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("res"), data.clone());
    env.write_files = false;
    env.fixed_now.set(Some(T));
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: false,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let mut m = Meter::new(env, args, Box::new(NoOcr), None);
    m.uid = Some("100000001".into());
    m.character_name = Some("Quillon".into());
    std::fs::write(data.join("settings.json"), r#"{"leaderboard": {"submit": true, "token": "alb_test", "api_url": "http://127.0.0.1:9"}}"#).unwrap();
    m.submitter = Some(alogs_meter::submit::Submitter::new(data));
    m.space = Some(INSTANCE.into());
    m
}

/// An Interlink fight started now and `into` s along its 180 s clock (read just now), on bar x5 at 60%.
fn fight(m: &mut Meter, csv: &str, into: f64) {
    let mut row = alogs_reader::boss::Row::new();
    for (k, v) in [("puppetId", "2101"), ("table", "puppet_data"), ("bossTitle_en", "Made-up Boss")] {
        row.insert(k.into(), v.into());
    }
    let mut f = Fight::new(&m.env, "Made-up Boss", 60, row, 1_000_000, true, false, None, None);
    f.csv_name = csv.into();
    let mut t = Timer::new(180.0);
    t.val = Some(180.0 - into);
    t.at = Some(m.env.now());
    f.timer = Some(t);
    f.hit = true;
    f.seg = Some(5);
    f.seg_fill = 0.6;
    f.started_at_top = true;
    m.fight = Some(f);
    m.seat_note_start(true, true);
    m.bar_seen = m.env.now();
}

fn at(m: &mut Meter, t: f64) {
    m.env.fixed_now.set(Some(t));
    m.seat_note_check(t);
}

fn note(m: &Meter, csv: &str) -> Option<Value> {
    let p = m.env.data.join("logs").join("submit_queue").join(format!("interlink_presence__{csv}.json"));
    std::fs::read_to_string(p).ok().and_then(|t| Value::json_loads(&t))
}

/// Ran out of time (the clock at zero), no result screen read: the note goes SEAT_NOTE_WAIT_S on, once, with the
/// run's own fields and nothing else.
#[test]
fn a_fight_run_to_its_end_without_a_result_sends_a_note() {
    let mut m = meter("seat-note-sent");
    fight(&mut m, "fight_a", 178.5);
    m.end_fight(&mut Log::default(), "scene changed (game log)");
    at(&mut m, T + SEAT_NOTE_WAIT_S - 1.0);
    assert!(note(&m, "fight_a").is_none(), "not before its wait");
    at(&mut m, T + SEAT_NOTE_WAIT_S + 0.5);
    let item = note(&m, "fight_a").expect("the note is queued");
    assert!(!item.get("evidence").truthy() && !item.get("party").truthy(), "no pictures");
    let mut p = item.get("payload").clone();
    if let Value::Dict(d) = &mut p {
        d.pop("server"); // (this PC's game server from its registry, or null on a PC without the game)
        assert!(matches!(d.pop("playedAt"), Some(Value::Int(_))), "played at: the fight's start, as a run's");
        d.pop("cycle"); // (whichever week the shipped schedule says)
    }
    assert_eq!(p.json_dumps(), format!(r#"{{"kind": "interlink_presence", "clientRunId": "fight_a", "gameUid": "100000001", "boss": {{"id": "2101", "name": "Made-up Boss", "level": 60}}, "extra": {{"instanceId": "{INSTANCE}", "name": "Quillon"}}, "clientVersion": "meter-0.1"}}"#));
    // never twice
    let q = m.env.data.join("logs").join("submit_queue").join("interlink_presence__fight_a.json");
    std::fs::remove_file(&q).unwrap();
    at(&mut m, T + 2.0 * SEAT_NOTE_WAIT_S);
    let f = m.last_fight.take().unwrap().0;
    m.seat_note.instance = Some(INSTANCE.into());
    m.seat_note_end(&f, true, T + 61.0);
    at(&mut m, T + 4.0 * SEAT_NOTE_WAIT_S);
    assert!(note(&m, "fight_a").is_none(), "once only");
}

/// Its own seat from this run's Party read, when the read has an entry marked as you.
#[test]
fn the_note_carries_your_party_seat() {
    let mut m = meter("seat-note-slot");
    let member = |slot: i64, name: &str, me: bool| alogs_pyval::dict([("slot", Value::Int(slot)), ("name", Value::Str(name.into())), ("ai", Value::Bool(false)), ("self", Value::Bool(me))]).as_dict().cloned().unwrap();
    m.set_party(vec![member(0, "Zarvok", false), member(2, "Quillon", true)], T - 60.0);
    fight(&mut m, "fight_s", 177.0);
    m.end_fight(&mut Log::default(), "boss bar gone");
    at(&mut m, T + SEAT_NOTE_WAIT_S + 0.5);
    let extra = note(&m, "fight_s").expect("queued").get("payload").get("extra").clone();
    assert_eq!(extra.get("slot").as_i64(), Some(2));
    assert_eq!(extra.get("instanceId").py_str(), INSTANCE);
}

/// A result recorded for the fight: the run itself is sent, no note.
#[test]
fn no_note_when_a_result_was_read() {
    let mut m = meter("seat-note-result");
    fight(&mut m, "fight_r", 178.5);
    m.end_fight(&mut Log::default(), "boss bar gone");
    m.ps.result_at = T + 3.0; // (the result screen's first read records it)
    at(&mut m, T + SEAT_NOTE_WAIT_S + 0.5);
    assert!(note(&m, "fight_r").is_none());
    assert!(m.seat_note.pending.is_none());
}

/// The fight's run file, as fight_summary writes it.
fn run_file(m: &Meter, csv: &str, json: &str) {
    std::fs::write(m.run_path(csv), json).unwrap();
}

/// The real order: the result screen's first read ends the fight (stamped with the clock of that moment) and then
/// records its result with the look's time, a little before that end. The run file holds the result: no note.
#[test]
fn no_note_when_the_result_screen_ended_the_fight() {
    let mut m = meter("seat-note-result-ended");
    fight(&mut m, "fight_q", 178.5);
    m.env.fixed_now.set(Some(T + 3.0));
    m.end_fight(&mut Log::default(), "result screen");
    m.ps.result_at = T + 2.9; // (the look's time, before the end's stamp)
    run_file(&m, "fight_q", r#"{"result": {"score": 1234, "players": []}}"#);
    at(&mut m, T + 3.0 + SEAT_NOTE_WAIT_S + 0.5);
    assert!(note(&m, "fight_q").is_none());
    assert!(m.seat_note.pending.is_none());
}

/// A run file marked sent: no note either.
#[test]
fn no_note_when_the_run_was_sent() {
    let mut m = meter("seat-note-run-sent");
    fight(&mut m, "fight_t", 178.5);
    m.end_fight(&mut Log::default(), "boss bar gone");
    run_file(&m, "fight_t", r#"{"sent": true}"#);
    at(&mut m, T + SEAT_NOTE_WAIT_S + 0.5);
    assert!(note(&m, "fight_t").is_none());
}

/// A fight left long before its end (abandoned), or one with no instance known: no note.
#[test]
fn no_note_for_a_fight_left_early_or_without_its_instance() {
    let mut m = meter("seat-note-early");
    fight(&mut m, "fight_e", 60.0);
    m.end_fight(&mut Log::default(), "scene changed (game log)");
    at(&mut m, T + SEAT_NOTE_WAIT_S + 0.5);
    assert!(note(&m, "fight_e").is_none(), "left two minutes before the end");

    let mut m = meter("seat-note-no-instance");
    m.space = Some("100-100000001-3000".into()); // the open world, a scene: no instance
    fight(&mut m, "fight_i", 178.5);
    m.end_fight(&mut Log::default(), "boss bar gone");
    at(&mut m, T + SEAT_NOTE_WAIT_S + 0.5);
    assert!(note(&m, "fight_i").is_none());
}

/// A kill: the boss's last bar nearly empty at the bar's last sight counts as the fight's end.
#[test]
fn a_kill_counts_as_the_end() {
    let mut m = meter("seat-note-kill");
    fight(&mut m, "fight_k", 90.0);
    if let Some(f) = m.fight.as_mut() {
        f.seg = Some(1);
        f.seg_fill = 0.02;
    }
    m.env.fixed_now.set(Some(T + 4.0)); // ended a few seconds after the bar's last sight
    m.end_fight(&mut Log::default(), "boss bar gone");
    at(&mut m, T + 4.0 + SEAT_NOTE_WAIT_S + 0.5);
    assert!(note(&m, "fight_k").is_some());
}

/// The site doesn't know the kind (an older site answers 400): the note leaves the queue quietly, nothing set aside
/// and no "rejected" line for the app.
#[test]
fn an_older_site_turning_it_down_is_quiet() {
    use std::io::{BufRead, BufReader, Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let api = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let mut c = conn.unwrap();
            let mut r = BufReader::new(c.try_clone().unwrap());
            let mut len = 0;
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h == "\r\n" || h.is_empty() {
                    break;
                }
                if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let reply = r#"{"error": "Unknown kind."}"#;
            let _ = write!(c, "HTTP/1.1 400 X\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{reply}", reply.len());
        }
    });
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("seat-note-old-site");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("settings.json"), format!(r#"{{"leaderboard": {{"submit": true, "token": "alb_test", "api_url": "{api}"}}}}"#)).unwrap();
    let s = alogs_meter::submit::Submitter::new(d.clone());
    let runner = s.clone();
    std::thread::spawn(move || runner.run());
    let p = alogs_pyval::dict([("kind", Value::Str("interlink_presence".into())), ("clientRunId", Value::Str("fight_o".into())), ("server", Value::Str("x".into()))]);
    s.enqueue(&p, None, None, None, None);
    for _ in 0..100 {
        if s.pending() == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(s.pending(), 0, "gone from the queue");
    let names: Vec<String> = std::fs::read_dir(d.join("logs/submit_queue")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(names.is_empty(), "nothing set aside: {names:?}");
    assert!(s.last_result.lock().unwrap().is_none(), "no line for the app");
    s.stop.store(true, std::sync::atomic::Ordering::Relaxed);
}
