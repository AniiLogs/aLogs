//! Meter::step on synthetic boss-bar frames with a stand-in OCR: a world boss is named from its title, its fight
//! follows the bar, and it ends when the bar is gone. Needs reference/ (the shipped boss list); skipped without it.

use alogs_meter::env::Env;
use alogs_meter::meter::{Args, Frames, Meter, Out};
use alogs_pil::{Image, Mode};
use alogs_pyval::Value;
use alogs_reader::ocr::Reader;
use indexmap::IndexMap;
use std::path::PathBuf;

struct FakeOcr(String, std::sync::Mutex<IndexMap<String, String>>);
impl Reader for FakeOcr {
    fn read(&self, images: &IndexMap<String, Image>, _: &IndexMap<String, usize>, _: &IndexMap<String, Image>) -> IndexMap<String, String> {
        let screen = self.1.lock().unwrap();
        images.keys().map(|k| (k.clone(), if k == "title" { self.0.clone() } else { screen.get(k).cloned().unwrap_or_default() })).collect()
    }
    fn cached(&self, _: &IndexMap<String, Image>, _: &IndexMap<String, usize>) -> bool { true }
    fn language(&self) -> Option<String> { Some("en-US".into()) }
    fn started(&self) -> bool { true }
}

struct Script { frames: Vec<Image>, i: usize }
impl Frames for Script {
    fn grab(&mut self, _: usize, _: Option<f64>) -> Option<Image> {
        self.frames.get(self.i.min(self.frames.len() - 1)).cloned()
    }
    fn window(&mut self) -> Option<usize> { Some(1) }
}

#[derive(Default)]
struct Log(Vec<(String, Value)>);
impl Out for Log {
    fn out(&mut self, kind: &str, data: Value) { self.0.push((kind.to_string(), data)); }
}

#[test]
fn a_world_fight_starts_follows_and_ends() {
    let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/aLogs/bossbar");
    if !res.exists() { eprintln!("skipped: reference/ not cloned"); return; }
    let frames_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../alogs-reader/tests/data/frames");
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("meter-step");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let mut env = Env::new(res.clone(), data);
    env.write_files = false;
    env.fixed_now.set(Some(1_800_000_000.0));
    // a world boss from the shipped list, by its bar title
    let catalog = alogs_reader::boss::load_catalog(&res, None).unwrap();
    let row = catalog.iter().find(|r| r["table"] == "puppet_data" && r["in_boss_rush"] != "yes" && !r["bossTitle_en"].is_empty()
        && r["fixed_hp_rate"].contains("3101") && r["puppetId"].starts_with("21")).expect("a wild boss row").clone();
    let title = format!("Lv.55 {}", row["bossTitle_en"]);
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: true,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let mut m = Meter::new(env, args, Box::new(FakeOcr(title, Default::default())), None);
    let full = Image::open_png(&frames_dir.join("bar14_1920x1080.png")).unwrap().convert(Mode::Rgb);   // a world bar, 83% full
    let blank = Image::new(Mode::Rgb, 1920, 1080, &[20, 20, 20]);
    let mut script = Script { frames: vec![full, blank], i: 0 };
    let mut log = Log::default();
    let mut t = 1_800_000_000.0;
    for _ in 0..30 {
        m.env.fixed_now.set(Some(t));
        m.step(&mut script, &mut log);
        t += 0.1;
    }
    let f = m.fight.as_ref().expect("a fight started from the title");
    assert_eq!(f.title, row["bossTitle_en"]);
    assert!(!f.bossrush);
    assert!(log.0.iter().any(|(k, _)| k == "fight"), "the overlay got the fight's numbers");
    // the bar goes: the fight ends after the grace
    script.i = 1;
    for _ in 0..30 {
        t += 1.0;
        m.env.fixed_now.set(Some(t));
        m.step(&mut script, &mut log);
    }
    assert!(m.fight.is_none(), "the fight ended when its bar was gone");
    let (lf, _) = m.last_fight.as_ref().unwrap();
    assert_eq!(lf.title, row["bossTitle_en"]);
    assert!(log.0.iter().any(|(k, v)| k == "status" && v.as_str().is_some_and(|s| s.starts_with("fight over (boss bar gone)"))), "{:?}", log.0.iter().rev().take(3).collect::<Vec<_>>());
}

#[test]
fn a_result_screen_lands_in_the_runs_summary() {
    let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/aLogs/bossbar");
    if !res.exists() { eprintln!("skipped: reference/ not cloned"); return; }
    let frames_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../alogs-reader/tests/data/frames");
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("meter-result");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let mut env = Env::new(res.clone(), data.clone());
    env.write_files = false;
    let catalog = alogs_reader::boss::load_catalog(&res, None).unwrap();
    let row = catalog.iter().find(|r| r["table"] == "puppet_data" && r["in_boss_rush"] != "yes" && r["puppetId"].starts_with("21")
        && r["fixed_hp_rate"].contains("3101") && !r["bossTitle_en"].is_empty()).unwrap().clone();
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: true,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let ocr = Box::new(FakeOcr(format!("Lv.55 {}", row["bossTitle_en"]), Default::default()));
    let screen = ocr.1.lock().unwrap().clone();
    drop(screen);
    let mut m = Meter::new(env, args, ocr, None);
    let full = Image::open_png(&frames_dir.join("bar14_1920x1080.png")).unwrap().convert(Mode::Rgb);
    let mut card = Image::new(Mode::Rgb, 1920, 1080, &[20, 20, 20]);
    alogs_pil::draw::rectangle(&mut card, (200.0, 300.0, 1700.0, 900.0), &[250, 250, 250]);   // ink so text boxes count
    let mut script = Script { frames: vec![full, card], i: 0 };
    let mut log = Log::default();
    let mut t = 1_800_000_000.0;
    for _ in 0..20 { m.env.fixed_now.set(Some(t)); m.step(&mut script, &mut log); t += 0.1; }
    assert!(m.fight.is_some());
    script.i = 1;
    for _ in 0..25 { t += 1.0; m.env.fixed_now.set(Some(t)); m.step(&mut script, &mut log); }
    assert!(m.fight.is_none());
    // the result screen comes up: the stand-in OCR answers its boxes
    {
        let mut answers = IndexMap::new();
        for (k, v) in [("lbl", "Challenge Score Completion Time"), ("cs", "Challenge Complete"), ("rating", "17 17 17"),
                       ("score", "17,164"), ("time", "95s 95s 95s"), ("name0", "Kyhra"), ("stat0", "Total Damage Dealt / 123456")] {
            answers.insert(k.to_string(), v.to_string());
        }
        // FakeOcr is behind the Meter's Box: reach it through a fresh one with the answers
        m.ocr = Box::new(FakeOcr(String::new(), std::sync::Mutex::new(answers)));
    }
    for _ in 0..3 { t += 1.0; m.env.fixed_now.set(Some(t)); m.step(&mut script, &mut log); }
    let csv = m.last_fight.as_ref().unwrap().0.csv_name.clone();
    let s: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(data.join("logs/fights").join(format!("{csv}.json"))).unwrap()).unwrap();
    assert_eq!(s["boss"], row["bossTitle_en"].as_str());
    assert_eq!(s["result"]["rating_stars"], 17);
    assert_eq!(s["result"]["score"], 17164);
    assert_eq!(s["result"]["time_s"], 95);
    assert_eq!(s["result"]["players"][0]["name"], "Kyhra");
    assert_eq!(s["result"]["players"][0]["value"], 123456);
    assert_eq!(s["result"]["players"][0]["stat"], "Total Damage Dealt");
    assert!(log.0.iter().any(|(k, v)| k == "status" && v.as_str().is_some_and(|x| x.starts_with("result recorded"))));
}
