//! The Party screen and the party's voting with a stand-in OCR: names voted over reads, AI only on two reads alike,
//! a missed player kept for one read.

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
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: true,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let ocr: IndexMap<String, String> = ocr.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Meter::new(env, args, Box::new(FakeOcr(ocr)), None)
}

fn member(slot: i64, name: &str, ai: bool) -> alogs_pyval::Dict {
    alogs_pyval::dict([("slot", Value::Int(slot)), ("name", Value::Str(name.into())), ("ai", Value::Bool(ai)), ("self", Value::Bool(false))])
        .as_dict().cloned().unwrap()
}

fn names(m: &Meter) -> Vec<String> {
    m.party.as_ref().unwrap().get("members").as_list().unwrap().iter()
        .map(|p| format!("{}{}", p.get("name").py_str(), if p.get("ai").truthy() { " (AI)" } else { "" })).collect()
}

#[test]
fn the_party_is_voted_over_reads() {
    let mut m = meter("party-votes", &[]);
    let t = 1_800_000_000.0;
    m.set_party(vec![member(0, "Kyhra", false), member(1, "Zarvok", true)], t);
    assert_eq!(names(&m), ["Kyhra", "Zarvok"], "one read saying AI isn't enough");
    m.set_party(vec![member(0, "Kyhra", false), member(1, "Zarvok", true)], t + 2.0);
    assert_eq!(names(&m), ["Kyhra", "Zarvok (AI)"]);
    m.set_party(vec![member(0, "Kyhro", false), member(1, "Zarvok", true)], t + 4.0);
    assert_eq!(names(&m), ["Kyhra", "Zarvok (AI)"], "a misread is outvoted");
    m.set_party(vec![member(0, "Kyhra", false)], t + 6.0);
    assert_eq!(names(&m), ["Kyhra", "Zarvok (AI)"], "a player one read missed is kept");
    m.set_party(vec![member(0, "Kyhra", false)], t + 8.0);
    assert_eq!(names(&m), ["Kyhra"], "two reads missing them: gone");
}

#[test]
fn the_party_screen_reads_players_and_ai() {
    let mut m = meter("party-screen", &[("n0", "1P Kyhra"), ("n1", "2P @ Mira Vosk O"), ("n2", "3P Flora"),
                                        ("a0", "Irisalis / Lv.60 CP:3578"), ("a1", "Spark / Lv.52 CP 2901"), ("a2", "Mossy / Lv.40")]);
    m.character_name = Some("Kyhra".into());
    let im = Image::new(Mode::Rgb, 1920, 1080, &[40, 40, 40]);
    let t = 1_800_000_000.0;
    m.party_screen(&im, t, 1920.0, 0.0, 1080.0);
    m.party_screen(&im, t + 2.0, 1920.0, 0.0, 1080.0);
    assert_eq!(names(&m), ["Kyhra", "MiraVosk", "Flora (AI)"]);
    let p = m.party.as_ref().unwrap().get("members").as_list().unwrap().clone();
    assert!(p[0].get("self").truthy());
    assert_eq!(p[0].get("cp").as_i64(), Some(3578));
    assert_eq!(p[1].get("aniimo_level").as_i64(), Some(52));
    assert_eq!(p[1].get("nickname").py_str(), "Spark");
}

#[test]
fn a_portrait_counts_only_clearly_ahead() {
    use alogs_meter::party_icons::tile_sure;
    let m = |s: f64, a: f64| Some(("Leafy".to_string(), s, a, String::new()));
    assert!(tile_sure(&m(0.55, 0.15)) && tile_sure(&m(0.68, 0.10)));
    assert!(!tile_sure(&m(0.60, 0.12)) && !tile_sure(&m(0.9, 0.09)) && !tile_sure(&None));
}
