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

#[test]
fn challenge_stats_go_to_scores() {
    let mut m = meter("egg-challenge", &[("score", "1,234,5O7"), ("stars", "27")]);
    let im = Image::new(Mode::Rgb, 1920, 1080, &[40, 40, 40]);
    let mut log = Log::default();
    m.challenge_stats(&mut NoFrames, &mut log, &im, 1_800_000_000.0, 1920.0, 0.0, 1080.0);
    let csv = std::fs::read_to_string(m.env.data.join("logs/scores.csv")).unwrap();
    assert!(csv.contains("challenge_stats,all (weekly best),1234507,27"), "{csv}");
}
