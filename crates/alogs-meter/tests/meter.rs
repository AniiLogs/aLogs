//! Meter::step on synthetic boss-bar frames with a stand-in OCR: a world boss is named from its title, its fight
//! follows the bar, and it ends when the bar is gone. Needs reference/ (the shipped boss list); skipped without it.

use alogs_meter::env::Env;
use alogs_meter::meter::{Args, Frames, Meter, Out};
use alogs_pil::{Image, Mode};
use alogs_pyval::Value;
use alogs_reader::ocr::Reader;
use indexmap::IndexMap;
use std::path::PathBuf;

struct FakeOcr(String);
impl Reader for FakeOcr {
    fn read(&self, images: &IndexMap<String, Image>, _: &IndexMap<String, usize>, _: &IndexMap<String, Image>) -> IndexMap<String, String> {
        images.keys().map(|k| (k.clone(), if k == "title" { self.0.clone() } else { String::new() })).collect()
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
                      no_submit: true, no_debug_frames: true, no_overlay: true, player_log: PathBuf::new() };
    let mut m = Meter::new(env, args, Box::new(FakeOcr(title)), None);
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
