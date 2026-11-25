//! Egg Heist's end screen and boss rush's Challenge Stats with a stand-in OCR: a match is noted once two reads agree.

use alogs_meter::env::Env;
use alogs_meter::meter::{Args, Frames, Meter, Out};
use alogs_pil::{Image, Mode};
use alogs_pyval::Value;
use alogs_reader::ocr::Reader;
use indexmap::IndexMap;
use std::path::PathBuf;

struct FakeOcr(IndexMap<String, String>);
impl Reader for FakeOcr {
    fn read(&self, images: &IndexMap<String, Image>, _: &IndexMap<String, usize>, _: &IndexMap<String, Image>) -> IndexMap<String, String> {
        images.keys().map(|k| (k.clone(), self.0.get(k).cloned().unwrap_or_default())).collect()
    }
    fn cached(&self, _: &IndexMap<String, Image>, _: &IndexMap<String, usize>) -> bool { true }
    fn language(&self) -> Option<String> { Some("en-US".into()) }
    fn started(&self) -> bool { true }
}

struct NoFrames;
impl Frames for NoFrames {
    fn grab(&mut self, _: usize, _: Option<f64>) -> Option<Image> { None }
    fn window(&mut self) -> Option<usize> { None }
}

#[derive(Default)]
struct Log(Vec<(String, Value)>);
impl Out for Log {
    fn out(&mut self, kind: &str, data: Value) { self.0.push((kind.to_string(), data)); }
}

fn meter(name: &str, ocr: &[(&str, &str)]) -> Meter {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let env = Env::new(data.join("res"), data);
    env.fixed_now.set(Some(1_800_000_000.0));
    let mut cfg = alogs_protocol::settings::defaults();
    if let Value::Dict(d) = &mut cfg { d.insert("capture".into(), alogs_pyval::dict([("keep_hours", Value::Int(0))])); }
    let args = Args { cfg, duration: 180.0, level: None, boss_id: None, replay: true,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let ocr: IndexMap<String, String> = ocr.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let mut m = Meter::new(env, args, Box::new(FakeOcr(ocr)), None);
    m.uid = Some("1".into());
    // the stand-in OCR reads the numbers too: no game font, which on a PC with the game installed would be extracted
    // from it and read the blank picture instead (no loot, no defeats: the reads never agree)
    m.font_tried = true;
    m
}

#[test]
fn an_egg_heist_match_is_noted_once_two_reads_agree() {
    let mut m = meter("egg-match", &[("difficulty", "Hard"), ("mode", "Mode: Egg Heist"), ("time", "11:50"), ("goal", ""),
                                     ("loot", "40,939"), ("defeats", "Defeats: l2"), ("chests", "2/3")]);
    let im = Image::new(Mode::Rgb, 1920, 1080, &[40, 40, 40]);
    let mut log = Log::default();
    let t = 1_800_000_000.0;
    let path = m.env.data.join("logs/egg_matches.csv");
    m.egg_result(&mut NoFrames, &mut log, &im, t, 1920.0, 0.0, 1080.0, "Adventure Successful");
    assert!(!path.exists(), "one read isn't enough");
    m.egg_result(&mut NoFrames, &mut log, &im, t + 0.5, 1920.0, 0.0, 1080.0, "Adventure Successful");
    let csv = std::fs::read_to_string(&path).unwrap();
    assert_eq!(csv.lines().count(), 2, "{csv}");
    assert!(csv.contains(",1,success,Hard,Egg Heist,710,,40939,12,2/3,"), "{csv}");
    assert!(log.0.iter().any(|(k, v)| k == "status" && v.py_str() == "Egg Heist match recorded: escaped, 40,939 loot (Hard)"), "{:?}", log.0);
    // still up: the same match, not noted again
    m.egg_result(&mut NoFrames, &mut log, &im, t + 5.0, 1920.0, 0.0, 1080.0, "Adventure Successful");
    m.egg_result(&mut NoFrames, &mut log, &im, t + 5.5, 1920.0, 0.0, 1080.0, "Adventure Successful");
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
}

const T: f64 = 1_800_000_000.0;
const END: [(&str, &str); 7] = [("difficulty", "Hard"), ("mode", "Mode: Egg Heist"), ("time", "11:50"), ("goal", ""), ("loot", "40,939"),
                                ("defeats", "Defeats: l2"), ("chests", "2/3")];

/// A live meter (no game window: nothing is clicked, nothing is told) with Automation and submissions on, the hold dry,
/// and a sender that isn't running: what is queued stays in the queue.
fn holding(name: &str, ocr: &[(&str, &str)], results: &[(&str, Value)]) -> Meter {
    let mut m = meter(name, ocr);
    let d = m.env.data.clone();
    std::fs::write(d.join("settings.json"),
        r#"{"leaderboard": {"submit": true, "token": "alb_test", "api_url": "http://127.0.0.1:9", "include_screenshots": false}}"#).unwrap();
    m.submitter = Some(alogs_meter::submit::Submitter::new(d));
    m.args.replay = false;
    m.hold = alogs_capture::input::InputBlock::dry();
    if let Value::Dict(c) = &mut m.args.cfg {
        c.insert("leaderboard".into(), alogs_pyval::dict([("submit", Value::Bool(true)), ("automation", Value::Bool(true))]));
        let mut r = alogs_pyval::dict([("hold_inputs", Value::Bool(true)), ("egg_auto_next", Value::Bool(true))]);
        if let Value::Dict(rd) = &mut r {
            for (k, v) in results {
                rd.insert((*k).into(), v.clone());
            }
        }
        c.insert("results".into(), r);
    }
    m
}

fn heads(k: &str, v: &str) -> IndexMap<String, String> {
    [(k.to_string(), v.to_string())].into_iter().collect()
}

/// The sender got this copy up (the reply the site gave).
fn arrived(m: &mut Meter, out: &mut Log, copy: Value, reply: &str) {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send((copy, Value::json_loads(reply).unwrap())).unwrap();
    m.take_sent(out, &rx);
}

fn toasts(out: &Log) -> Vec<String> {
    out.0.iter().filter(|(k, _)| k == "toast").map(|(_, v)| v.as_list().unwrap()[0].py_str()).collect()
}

/// The standard: held from the moment the end screen is spotted (before it is read), not let go when its record is
/// queued but when it is sent; with Next pressed by aLogs, on through the loading to the rank screen; the rank sent the
/// same held way; let go once that is sent. One notice for the match and its rank.
#[test]
fn an_egg_heist_match_and_its_rank_are_held_until_sent_with_one_notice() {
    let mut ocr: Vec<(&str, &str)> = END.to_vec();
    ocr.extend([("high", "1337/1350"), ("title", "Pro Egg Snatcher")]);
    let mut m = holding("egg-hold-flow", &ocr, &[("hold_seconds", Value::Int(30))]);
    let im = Image::new(Mode::Rgb, 1920, 1080, &[40, 40, 40]);
    let mut out = Log::default();

    m.egg_screen_hold(Some("Egg Heist"), &heads("eh", "Adventure Successful"), T, true);
    let h = m.egg.hold.clone().expect("held from the end screen's first sight");
    assert_eq!((h.screen, h.until, h.on_to_rank), ("end screen", T + 30.0, false), "results.hold_seconds is the time limit");
    assert!(m.hold.active());
    m.egg_result(&mut NoFrames, &mut out, &im, T, 1920.0, 0.0, 1080.0, "Adventure Successful");
    m.egg_result(&mut NoFrames, &mut out, &im, T + 0.5, 1920.0, 0.0, 1080.0, "Adventure Successful");
    let first = m.send_waiting_copy.clone().expect("the match is queued, and the hold waits for it");
    assert_eq!(first.get("kind").py_str(), "eggheist_match");
    assert!(m.egg.hold.is_some() && m.hold.active(), "queued is not sent: still held");

    m.egg_hold_tick(T + 10.0, true);
    assert!(m.egg.hold.is_some(), "a picture coming back doesn't end it; only the send or the time limit does");
    m.env.fixed_now.set(Some(T + 2.0));
    arrived(&mut m, &mut out, first, r#"{"submissionId": "s1"}"#);
    let h = m.egg.hold.clone().expect("aLogs presses Next: held on to the rank screen");
    assert_eq!((h.screen, h.on_to_rank, h.until), ("end screen", true, T + 32.0), "a new time limit from the send");
    assert_eq!(toasts(&out), ["Egg Heist match submitted"]);

    // the loading, then the rank screen ("Tap anywhere to continue")
    m.egg_screen_hold(Some("Egg Heist"), &heads("et", "Tap anywhere to continue"), T + 5.0, true);
    assert_eq!(m.egg.hold.as_ref().map(|h| h.screen), Some("rank screen"));
    m.egg_rank_gain(&mut NoFrames, &mut out, &im, T + 5.0, 1920.0, 0.0, 1080.0, false);
    m.egg_rank_gain(&mut NoFrames, &mut out, &im, T + 6.0, 1920.0, 0.0, 1080.0, false);
    let rank = m.send_waiting_copy.clone().expect("the rank goes the held way too");
    assert_eq!(rank.get("kind").py_str(), "eggheist_season");
    assert!(m.egg.hold.is_some(), "still held until the rank is sent");
    arrived(&mut m, &mut out, rank, r#"{"submissionId": "s2"}"#);
    assert!(m.egg.hold.is_none() && !m.hold.active(), "let go at once once sent");
    assert_eq!(toasts(&out), ["Egg Heist match submitted"], "one notice per match: its rank goes silently");
}

/// Without Next pressed by aLogs the end screen's hold lets go once its record is sent; and nothing but the time limit
/// (results.hold_seconds) ends a hold whose record isn't sent.
#[test]
fn an_egg_heist_hold_ends_when_sent_or_at_its_time_limit() {
    let mut m = holding("egg-hold-limit", &END, &[("egg_auto_next", Value::Bool(false)), ("hold_seconds", Value::Int(20))]);
    let im = Image::new(Mode::Rgb, 1920, 1080, &[40, 40, 40]);
    let mut out = Log::default();
    m.egg_screen_hold(Some("Egg Heist"), &heads("eh", "Adventure Failed"), T, true);
    m.egg_result(&mut NoFrames, &mut out, &im, T, 1920.0, 0.0, 1080.0, "Adventure Failed");
    m.egg_result(&mut NoFrames, &mut out, &im, T + 0.5, 1920.0, 0.0, 1080.0, "Adventure Failed");
    let first = m.send_waiting_copy.clone().unwrap();
    arrived(&mut m, &mut out, first, r#"{"submissionId": "s1"}"#);
    assert!(m.egg.hold.is_none() && !m.hold.active(), "Next is the player's: let go once sent");

    // a match whose send never gets through: held to the time limit, not past it
    m.egg.sent_at = 0.0;
    m.send_waiting = None;
    m.egg_screen_hold(Some("Egg Heist"), &heads("eh", "Adventure Successful"), T + 100.0, true);
    m.egg_hold_tick(T + 119.0, true);
    assert!(m.egg.hold.is_some());
    m.egg_hold_tick(T + 120.5, true);
    assert!(m.egg.hold.is_none() && !m.hold.active());
    // the game in the background lets go at once; nothing is held with Automation off
    m.egg_screen_hold(Some("Egg Heist"), &heads("eh", "Adventure Successful"), T + 130.0, true);
    m.egg_hold_tick(T + 131.0, false);
    assert!(m.egg.hold.is_none());
    if let Value::Dict(c) = &mut m.args.cfg {
        c.insert("leaderboard".into(), alogs_pyval::dict([("submit", Value::Bool(true)), ("automation", Value::Bool(false))]));
    }
    m.egg_screen_hold(Some("Egg Heist"), &heads("eh", "Adventure Successful"), T + 140.0, true);
    assert!(m.egg.hold.is_none());
}

/// Rank Info, a menu screen the player opened: its record goes the leaderboards' way without holding anything, and a
/// changed read of the same look shows no second notice.
#[test]
fn rank_info_reads_show_one_notice() {
    let mut m = holding("egg-rank-info", &[("title", "Pro Egg Snatcher"), ("points", "Rank Points 1337/1350")], &[]);
    let im = Image::new(Mode::Rgb, 1920, 1080, &[40, 40, 40]);
    let mut out = Log::default();
    m.egg_rank_info(&mut NoFrames, &mut out, &im, T, 1920.0, 0.0, 1080.0);
    assert!(m.send_waiting.is_none() && !m.hold.active(), "nothing held for a menu screen");
    let q = m.env.data.join("logs").join("submit_queue");
    let copy = |q: &std::path::Path| {
        let f = std::fs::read_dir(q).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with("eggheist_season")).unwrap().path();
        Value::json_loads(&std::fs::read_to_string(f).unwrap()).unwrap().get("payload").clone()
    };
    let one = copy(&q);
    let one_dump = one.json_dumps();
    arrived(&mut m, &mut out, one, r#"{"submissionId": "s1"}"#);
    m.ocr = Box::new(FakeOcr([("title", "Pro Egg Snatcher"), ("points", "Rank Points 1338/1350")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()));
    m.egg_rank_info(&mut NoFrames, &mut out, &im, T + 6.0, 1920.0, 0.0, 1080.0);
    let two = copy(&q);
    assert_ne!(one_dump, two.json_dumps(), "the changed read is sent");
    arrived(&mut m, &mut out, two, r#"{"submissionId": "s1", "updated": true}"#);
    assert_eq!(toasts(&out), ["Egg Heist rank submitted"]);
}

#[test]
fn challenge_stats_go_to_scores() {
    let mut m = meter("egg-challenge", &[("score", "1,234,5O7"), ("stars", "27")]);
    let im = Image::new(Mode::Rgb, 1920, 1080, &[40, 40, 40]);
    let mut log = Log::default();
    m.challenge_stats(&mut NoFrames, &mut log, &im, 1_800_000_000.0, 1920.0, 0.0, 1080.0);
    let csv = std::fs::read_to_string(m.env.data.join("logs/scores.csv")).unwrap();
    assert!(csv.contains("challenge_stats,all (weekly best),1234507,27"), "{csv}");
}

#[test]
fn a_rank_screen_is_due_after_a_match_so_the_homeland_does_not_rest() {
    // a match queued from a homeland comes back there, and its rank screen shows there: the meter must read on
    let mut m = meter("egg-rank-due", &[]);
    let t = 1_800_000_000.0;
    assert!(!m.egg_rank_due(t), "no match yet: nothing due");
    m.egg.sent_at = t;
    assert!(m.egg_rank_due(t + 15.0), "the match was just sent: its rank screen may still come");
    assert!(!m.egg_rank_due(t + 91.0), "past the rank screen's wait");
    m.egg.rank_read_at = t + 20.0;
    assert!(!m.egg_rank_due(t + 21.0), "its rank screen is read: the homeland may rest");
    m.egg.hold = Some(alogs_meter::egg::EggHold { until: t + 200.0, screen: "end screen", on_to_rank: true, held: true });
    assert!(m.egg_rank_due(t + 150.0), "a hold that is on always reads on");
}
