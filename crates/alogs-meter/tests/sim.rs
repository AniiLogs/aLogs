//! Holo-Battle Sim's Victory screen with a stand-in OCR: a win is noted once in sim_runs.csv, the same win still up
//! isn't noted again, and a defeat isn't noted.

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

fn meter(name: &str, title: &str) -> Meter {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let env = Env::new(data.join("res"), data);
    env.fixed_now.set(Some(1_800_000_000.0));
    let mut cfg = alogs_protocol::settings::defaults();
    if let Value::Dict(d) = &mut cfg { d.insert("capture".into(), alogs_pyval::dict([("keep_hours", Value::Int(0))])); }
    let args = Args { cfg, duration: 180.0, level: None, boss_id: None, replay: true,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let ocr: IndexMap<String, String> = [("w", "Battle time 00:05:25 Earth Training - Hard"), ("title", title)]
        .into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let mut m = Meter::new(env, args, Box::new(FakeOcr(ocr)), None);
    m.uid = Some("1".into());
    m
}

#[test]
fn a_victory_is_noted_once() {
    let mut m = meter("sim-win", "Victory");
    let im = Image::new(Mode::Rgb, 1920, 1080, &[40, 40, 40]);
    let mut log = Log::default();
    let t = 1_800_000_000.0;
    // the coins and shares haven't faded in (none read): it waits up to 1.5 s for them
    m.sim_result(&mut NoFrames, &mut log, &im, t, 1920.0, 0.0, 1080.0);
    assert!(!m.env.data.join("logs/sim_runs.csv").exists());
    m.sim_result(&mut NoFrames, &mut log, &im, t + 2.0, 1920.0, 0.0, 1080.0);
    let csv = std::fs::read_to_string(m.env.data.join("logs/sim_runs.csv")).unwrap();
    assert_eq!(csv.lines().count(), 2, "{csv}");
    assert!(csv.contains(",1,Earth Training,Hard,325,"), "{csv}");
    assert!(log.0.iter().any(|(k, v)| k == "status" && v.py_str() == "Holo-Battle Sim recorded: Earth Training - Hard in 325 s"), "{:?}", log.0);
    // the same win still up 40 s later: read again, not noted again
    m.sim_result(&mut NoFrames, &mut log, &im, t + 40.0, 1920.0, 0.0, 1080.0);
    assert_eq!(std::fs::read_to_string(m.env.data.join("logs/sim_runs.csv")).unwrap().lines().count(), 2);
}

#[test]
fn a_defeat_is_not_noted() {
    let mut m = meter("sim-loss", "Defeat");
    let im = Image::new(Mode::Rgb, 1920, 1080, &[40, 40, 40]);
    m.sim_result(&mut NoFrames, &mut Log::default(), &im, 1_800_000_000.0, 1920.0, 0.0, 1080.0);
    assert!(!m.env.data.join("logs/sim_runs.csv").exists());
}
