//! The Holo-Battle Sim run's level record (sim_stages.rs), all made-up: levels from two agreeing looks, visits that
//! continue a kept record across a restart, a new run starting a new record, the 14-day limit, the bounds, and the
//! record going out with the win as `extra.stages`.

use alogs_meter::env::Env;
use alogs_meter::meter::{Args, Meter};
use alogs_meter::payloads::sim_payload;
use alogs_meter::sim_stages::{self, MAX_AREAS, MAX_VISITS, SimFight, Stages};
use alogs_pil::Image;
use alogs_pyval::{Value, dict};
use alogs_reader::ocr::Reader;
use indexmap::IndexMap;
use std::path::PathBuf;

struct NoOcr;
impl Reader for NoOcr {
    fn read(&self, images: &IndexMap<String, Image>, _: &IndexMap<String, usize>, _: &IndexMap<String, Image>) -> IndexMap<String, String> {
        images.keys().map(|k| (k.clone(), String::new())).collect()
    }
    fn cached(&self, _: &IndexMap<String, Image>, _: &IndexMap<String, usize>) -> bool { true }
    fn language(&self) -> Option<String> { Some("en-US".into()) }
    fn started(&self) -> bool { true }
}

/// Reads the same text every time and counts its reads.
struct Counted(std::sync::Arc<std::sync::atomic::AtomicUsize>, &'static str);
impl Reader for Counted {
    fn read(&self, images: &IndexMap<String, Image>, _: &IndexMap<String, usize>, _: &IndexMap<String, Image>) -> IndexMap<String, String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        images.keys().map(|k| (k.clone(), self.1.to_string())).collect()
    }
    fn cached(&self, _: &IndexMap<String, Image>, _: &IndexMap<String, usize>) -> bool { true }
    fn language(&self) -> Option<String> { Some("en-US".into()) }
    fn started(&self) -> bool { true }
}

const T0: f64 = 1_800_000_000.0;
const SIM: &str = "1-2-3";

fn dir(name: &str) -> PathBuf {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    data
}

/// A meter on `data` (kept as it is: a restart), at T0 + `at`.
fn meter(data: &std::path::Path, at: f64, uid: Option<&str>) -> Meter {
    let env = Env::new(data.join("res"), data.to_path_buf());
    env.fixed_now.set(Some(T0 + at));
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: true,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let mut m = Meter::new(env, args, Box::new(NoOcr), None);
    m.args.replay = false; // (made as a replay so nothing live starts; the record is off in replays)
    m.uid = uid.map(String::from);
    m.sim_spaces.insert(SIM.into());
    m
}

fn at(m: &Meter, t: f64) {
    m.env.fixed_now.set(Some(T0 + t));
}

/// An area start in the game's log, written `ago` seconds before it was read.
fn enter(m: &mut Meter, space: &str, ago: Option<f64>) {
    let was = m.holosim_space;
    m.holosim_space = m.sim_spaces.contains(space);
    m.stages_enter(space, ago, was);
    m.space = Some(space.into());
}

fn read(m: &mut Meter, t: f64, text: &str) {
    at(m, t);
    m.stages_feed(Some(text), T0 + t);
}

/// The level read on two looks a second apart.
fn level(m: &mut Meter, t: f64, n: i64, of: i64) {
    read(m, t, &format!("{n}/{of}"));
    read(m, t + 1.0, &format!("{n} / {of}"));
}

fn levels(m: &Meter) -> Vec<(i64, i64)> {
    m.stages.rec.as_ref().map_or(vec![], |r| r.visits.iter().flat_map(|v| v.levels.iter().map(|l| (l.n, l.of))).collect())
}

fn kept(data: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(data.join("logs").join(sim_stages::FILE)).unwrap_or("{}".into())).unwrap()
}

fn payload(m: &mut Meter) -> Value {
    let mut run = dict([("trial", Value::Str("Earth Training".into())), ("difficulty", Value::Str("Hard".into())), ("time_s", Value::Int(325))]);
    if let Some(st) = m.stages_take()
        && let Value::Dict(d) = &mut run
    {
        d.insert("stages".into(), st);
    }
    let clock = m.env.record_clock();
    sim_payload(&run, &Value::Str("1".into()), Some(T0 + 9999.0), &clock)
}

#[test]
fn a_run_in_one_visit_goes_out_with_the_win_and_is_cleared() {
    let data = dir("stages-one");
    let mut m = meter(&data, 0.0, Some("1"));
    enter(&mut m, SIM, Some(0.5));
    for n in 1..=16 {
        let t = n as f64 * 60.0;
        if n > 1 {
            at(&m, t - 5.0);
            enter(&mut m, SIM, Some(0.0));
        }
        level(&mut m, t, n, 16);
        at(&m, t + 50.0);
        m.stages.fight(SimFight { t: T0 + t + 50.0, s: 40.0, damage: 1_000_000 + n, id: "p1".into(), defeated: true, breaks: 1 });
    }
    assert_eq!(levels(&m).len(), 16);
    assert!(kept(&data).get("1").is_some(), "kept on disk while the run goes on");
    let p = payload(&mut m);
    let st = p.get("extra").get("stages");
    assert_eq!(st.get("of").repr(), Value::Int(16).repr());
    let visits = st.get("visits").as_list().unwrap();
    assert_eq!(visits.len(), 1);
    let v = &visits[0];
    assert_eq!(v.get("fromStart").repr(), Value::Bool(true).repr());
    assert_eq!(v.get("enteredAt").as_f64(), Some(T0 - 0.5));
    assert_eq!(v.get("levels").as_list().unwrap().len(), 16);
    assert_eq!(v.get("levels").as_list().unwrap()[4].get("n").repr(), Value::Int(5).repr());
    assert_eq!(v.get("levels").as_list().unwrap()[4].get("t").as_f64(), Some(T0 + 300.0), "the first of the two looks");
    assert_eq!(v.get("areas").as_list().unwrap().len(), 16);
    assert_eq!(v.get("fights").as_list().unwrap().len(), 16);
    assert_eq!(v.get("fights").as_list().unwrap()[0].get("id").repr(), Value::Str("p1".into()).repr());
    assert_eq!(v.get("reads").get("made").repr(), Value::Int(32).repr());
    assert_eq!(v.get("reads").get("counter").repr(), Value::Int(32).repr());
    assert!(v.get("leftAt").is_none());
    assert!(st.get("cut").is_none());
    assert!(m.stages.rec.is_none());
    assert!(kept(&data).get("1").is_none(), "cleared on disk once sent");
    // nothing more seen: the next win has no stages key at all
    let p = payload(&mut m);
    assert!(p.get("extra").as_dict().unwrap().get_str("stages").is_none());
    assert_eq!(p.get("kind").repr(), Value::Str("sim_run".into()).repr());
}

#[test]
fn an_ocr_slip_is_not_a_level() {
    let data = dir("stages-slip");
    let mut m = meter(&data, 0.0, Some("1"));
    enter(&mut m, SIM, None);
    level(&mut m, 10.0, 4, 16);
    read(&mut m, 12.0, "7/16"); // one look only
    read(&mut m, 13.0, "4/16");
    read(&mut m, 14.0, "44/16"); // not plausible: counted as made only
    read(&mut m, 15.0, "");
    read(&mut m, 16.0, "4/18"); // another N within the visit
    read(&mut m, 17.0, "5/16");
    read(&mut m, 18.0, "garbage"); // breaks the two looks
    read(&mut m, 19.0, "5/16");
    assert_eq!(levels(&m), vec![(4, 16)]);
    read(&mut m, 20.0, "5/16");
    assert_eq!(levels(&m), vec![(4, 16), (5, 16)]);
    let v = m.stages.rec.as_ref().unwrap().visits.last().unwrap();
    assert_eq!((v.made, v.counter), (11, 7));
    assert!(!v.from_start, "the meter's look back at its start: in the middle of a visit");
}

fn left_at_4(name: &str) -> PathBuf {
    let data = dir(name);
    let mut m = meter(&data, 0.0, Some("1"));
    enter(&mut m, SIM, Some(0.0));
    level(&mut m, 30.0, 1, 16);
    level(&mut m, 90.0, 2, 16);
    level(&mut m, 150.0, 3, 16);
    level(&mut m, 210.0, 4, 16);
    at(&m, 250.0);
    enter(&mut m, "9-9-9", Some(0.0)); // left the Sim
    assert!(!m.stages.open);
    drop(m); // aLogs closed
    data
}

#[test]
fn a_visit_left_at_4_and_continued_at_4_carries_on_across_a_restart() {
    let data = left_at_4("stages-continue-4");
    let mut m = meter(&data, 3600.0, Some("1"));
    enter(&mut m, SIM, Some(0.0));
    level(&mut m, 3610.0, 4, 16);
    level(&mut m, 3680.0, 5, 16);
    let rec = m.stages.rec.as_ref().unwrap();
    assert_eq!(rec.visits.len(), 2);
    assert_eq!(rec.visits[0].left_at, Some(T0 + 250.0));
    assert_eq!(levels(&m), vec![(1, 16), (2, 16), (3, 16), (4, 16), (4, 16), (5, 16)]);
}

#[test]
fn a_visit_left_at_4_and_continued_at_5_carries_on_across_a_restart() {
    let data = left_at_4("stages-continue-5");
    // the UID comes after the log's look back, as at a real start
    let mut m = meter(&data, 7200.0, None);
    enter(&mut m, SIM, None);
    level(&mut m, 7210.0, 5, 16);
    m.uid = Some("1".into());
    at(&m, 7215.0);
    m.stages_feed(Some(""), T0 + 7215.0);
    let rec = m.stages.rec.as_ref().unwrap();
    assert_eq!(rec.visits.len(), 2);
    assert!(rec.visits[0].from_start && !rec.visits[1].from_start);
    assert_eq!(levels(&m), vec![(1, 16), (2, 16), (3, 16), (4, 16), (5, 16)]);
    let p = payload(&mut m);
    assert_eq!(p.get("extra").get("stages").get("visits").as_list().unwrap().len(), 2);
}

#[test]
fn a_first_read_of_1_after_a_kept_9_starts_a_new_record() {
    let data = dir("stages-new");
    let mut m = meter(&data, 0.0, Some("1"));
    enter(&mut m, SIM, Some(0.0));
    level(&mut m, 10.0, 9, 16);
    enter(&mut m, "9-9-9", Some(0.0));
    drop(m);
    let mut m = meter(&data, 600.0, Some("1"));
    enter(&mut m, SIM, Some(0.0));
    assert_eq!(m.stages.rec.as_ref().unwrap().visits.len(), 2, "undecided until the counter is read");
    level(&mut m, 610.0, 1, 16);
    assert_eq!(m.stages.rec.as_ref().unwrap().visits.len(), 1);
    assert_eq!(levels(&m), vec![(1, 16)]);
    assert_eq!(kept(&data)["1"]["visits"].as_array().unwrap().len(), 1);
    // and another N starts one too
    enter(&mut m, "9-9-9", Some(0.0));
    enter(&mut m, SIM, Some(0.0));
    level(&mut m, 700.0, 3, 12);
    assert_eq!(levels(&m), vec![(3, 12)]);
}

#[test]
fn a_record_older_than_14_days_is_dropped() {
    let data = dir("stages-old");
    let mut m = meter(&data, 0.0, Some("1"));
    enter(&mut m, SIM, Some(0.0));
    level(&mut m, 10.0, 6, 16);
    drop(m);
    let mut m = meter(&data, 13.0 * 86400.0, Some("1"));
    enter(&mut m, SIM, Some(0.0));
    assert_eq!(m.stages.rec.as_ref().unwrap().visits.len(), 2, "13 days: still kept");
    drop(m);
    let mut m = meter(&data, 13.0 * 86400.0 + 15.0 * 86400.0, Some("1"));
    enter(&mut m, SIM, Some(0.0));
    assert_eq!(m.stages.rec.as_ref().unwrap().visits.len(), 1);
    assert!(levels(&m).is_empty());
}

#[test]
fn the_record_is_bounded_and_says_cut() {
    let mut s = Stages::default();
    for i in 0..MAX_VISITS + 3 {
        s.visit_start(T0 + i as f64 * 100.0, true);
        s.visit_end(T0 + i as f64 * 100.0 + 50.0);
    }
    let rec = s.rec.as_ref().unwrap();
    assert_eq!(rec.visits.len(), MAX_VISITS);
    assert_eq!(rec.visits[0].entered_at, T0 + 300.0, "the newest kept");
    assert!(rec.cut);
    let mut s = Stages::default();
    s.visit_start(T0, true);
    for i in 0..MAX_AREAS + 5 {
        s.area(T0 + i as f64);
    }
    let (j, _) = s.take().unwrap();
    assert_eq!(j["visits"][0]["areas"].as_array().unwrap().len(), MAX_AREAS);
    assert_eq!(j["cut"], serde_json::json!(true));
    assert!(s.take().is_none());
}

#[test]
fn nothing_seen_means_no_stages_key() {
    let mut s = Stages::default();
    assert!(s.take().is_none());
    s.visit_start(T0, true);
    assert!(s.take().is_none(), "a visit with nothing in it is nothing seen");
    let data = dir("stages-none");
    let mut m = meter(&data, 0.0, Some("1"));
    let p = payload(&mut m);
    assert!(p.get("extra").as_dict().unwrap().get_str("stages").is_none());
}

#[test]
fn the_kept_file_reads_back_the_same() {
    let mut s = Stages::default();
    s.visit_start(T0, false);
    s.area(T0 + 1.25);
    s.read(Some("2/16"), T0 + 2.0);
    s.read(Some("2/16"), T0 + 3.0);
    s.fight(SimFight { t: T0 + 9.0, s: 5.5, damage: 12, id: "p".into(), defeated: false, breaks: 0 });
    s.visit_end(T0 + 20.0);
    let rec = s.rec.clone().unwrap();
    let back = sim_stages::Record::from_json(&rec.to_json()).unwrap();
    assert_eq!(back.to_json(), rec.to_json());
    assert_eq!(back.visits[0].areas, vec![T0 + 1.3]);
}

#[test]
fn the_counter_is_read_once_a_second_and_only_in_a_visit() {
    let data = dir("stages-look");
    let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut m = meter(&data, 0.0, Some("1"));
    m.ocr = Box::new(Counted(n.clone(), "3/16"));
    let im = Image::new(alogs_pil::Mode::Rgb, 3840, 2160, &[40, 40, 40]);
    m.holosim_space = true;
    m.stages_look(&im, T0, true, false);
    assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 0, "no visit open: no read");
    m.holosim_space = false;
    enter(&mut m, SIM, Some(0.0));
    for t in [0.0, 0.2, 0.5, 0.99, 1.0, 1.1, 1.9, 2.0] {
        m.stages_look(&im, T0 + t, true, false);
    }
    assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 3, "at 0, 1 and 2 s");
    assert_eq!(levels(&m), vec![(3, 16)]);
    // a copy of the screen with the game behind another window: not read
    m.stages_look(&im, T0 + 5.0, false, true);
    assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 3);
    // what a read costs here besides the OCR itself (the box's crop and its prep, at 4K)
    let t = std::time::Instant::now();
    for i in 0..20 {
        m.stages_look(&im, T0 + 10.0 + i as f64, true, false);
    }
    eprintln!("level read at 4K without the OCR: {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
}
