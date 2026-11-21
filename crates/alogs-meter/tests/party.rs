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
    m.set_party(vec![member(0, "Kyhra", false), member(1, "Tammy", true)], t);
    assert_eq!(names(&m), ["Kyhra", "Tammy (AI)"], "a bot's name with a bot's card");
    m.set_party(vec![member(0, "Kyhra", false), member(1, "Tammy", true)], t + 2.0);
    assert_eq!(names(&m), ["Kyhra", "Tammy (AI)"]);
    m.set_party(vec![member(0, "Kyhro", false), member(1, "Tammy", true)], t + 4.0);
    assert_eq!(names(&m), ["Kyhra", "Tammy (AI)"], "a misread is outvoted");
    m.set_party(vec![member(0, "Kyhra", false)], t + 6.0);
    assert_eq!(names(&m), ["Kyhra", "Tammy (AI)"], "a player one read missed is kept");
    m.set_party(vec![member(0, "Kyhra", false)], t + 8.0);
    assert_eq!(names(&m), ["Kyhra"], "two reads missing them: gone");
}

/// A member with a sign of its own (one look of the Party screen: bot_sign), voted over the screen's looks.
fn signed(slot: i64, name: &str, sign: Option<bool>, flag: bool) -> alogs_pyval::Dict {
    let mut m = member(slot, name, flag);
    m.insert("ai_sign".into(), sign.map_or(Value::None, Value::Bool));
    m
}

/// A bot is a bot's name with no CP on its card. A name that isn't a bot's is a player whatever the looks say, and a
/// card with a CP outvotes the looks without one only by having more of them.
#[test]
fn a_bot_is_a_bot_name_without_cp_voted_over_looks() {
    let mut m = meter("party-bot-votes", &[]);
    let t = 1_800_000_000.0;
    // a player's slot that a look flagged (the old white-mark reading): never a bot
    m.set_party(vec![member(0, "Kyhra", false), member(1, "Zarvok", true)], t);
    m.set_party(vec![member(0, "Kyhra", false), member(1, "Zarvok", true)], t + 2.0);
    assert_eq!(names(&m), ["Kyhra", "Zarvok"]);
    // a bot's name: two looks without CP, then one with a CP read: still a bot (2 to 1)
    m.set_party(vec![signed(2, "Leonard", Some(true), true)], t + 4.0);
    m.set_party(vec![signed(2, "Leonard", Some(true), true)], t + 6.0);
    m.set_party(vec![signed(2, "Leonard", Some(false), false)], t + 8.0);
    assert!(names(&m).contains(&"Leonard (AI)".to_string()), "{:?}", names(&m));
    // two more with a CP: a real player who happens to be called that
    m.set_party(vec![signed(2, "Leonard", Some(false), false)], t + 10.0);
    m.set_party(vec![signed(2, "Leonard", Some(false), false)], t + 12.0);
    assert!(names(&m).contains(&"Leonard".to_string()), "{:?}", names(&m));
    // looks that couldn't tell (a card that didn't read) only break a tie, with their own guess
    let mut m = meter("party-bot-tie", &[]);
    m.set_party(vec![signed(3, "Flora", None, true)], t);
    assert!(names(&m).contains(&"Flora (AI)".to_string()));
    m.set_party(vec![signed(3, "Flora", Some(false), false)], t + 2.0);
    m.set_party(vec![signed(3, "Flora", None, true)], t + 4.0);
    assert!(names(&m).contains(&"Flora".to_string()), "a CP read beats looks that couldn't tell: {:?}", names(&m));
}

/// Result-screen and Battle Data slots follow their Party entry: a bot's name is a bot only when the Party entry is
/// one; a name that isn't a bot's is never one.
#[test]
fn result_slots_follow_the_party_entry() {
    use alogs_meter::results_parse::settle_bots;
    let bots: Vec<String> = ["Flora", "Mario", "Leonard", "Tammy"].iter().map(|s| s.to_string()).collect();
    let entry = |slot: i64, name: &str, ai: bool| Value::Dict(member(slot, name, ai));
    let party = vec![entry(0, "Kyhra", false), entry(1, "Mario", true), entry(2, "Tammy", false), entry(3, "Zarvok", true)];
    let row = |slot: i64, name: &str, ai: bool| member(slot, name, ai);
    let mut players = vec![row(0, "Kyhra", false), row(1, "Mario", false), row(2, "Tammy", true), row(3, "Zarvok", true)];
    settle_bots(&mut players, Some(&party), &bots);
    let ai: Vec<bool> = players.iter().map(|p| p.at("ai").truthy()).collect();
    assert_eq!(ai, [false, true, false, false]);
    // no Party read: a bot's name is a bot, any other name a player
    let mut players = vec![row(0, "Flora", false), row(1, "Zarvok", true), row(2, "Leonard", true)];
    settle_bots(&mut players, None, &bots);
    assert_eq!(players.iter().map(|p| p.at("ai").truthy()).collect::<Vec<_>>(), [true, false, true]);
}

#[test]
fn a_bot_sign_needs_a_bot_name_and_no_cp() {
    use alogs_meter::results_parse::{bot_sign, is_bot_name};
    let bots: Vec<String> = ["Mario", "Flora", "Leonard", "Tammy"].iter().map(|s| s.to_string()).collect();
    assert!(is_bot_name(&bots, "flora") && is_bot_name(&bots, " Leonard ") && !is_bot_name(&bots, "Florance") && !is_bot_name(&bots, ""));
    assert_eq!(bot_sign(&bots, "Mario", true, None), Some(true));
    assert_eq!(bot_sign(&bots, "Mario", true, Some(3010)), Some(false), "a CP: a player");
    assert_eq!(bot_sign(&bots, "Mario", false, None), None, "the card didn't read");
    assert_eq!(bot_sign(&bots, "Zarvok", true, None), Some(false), "not a bot's name");
    assert_eq!(bot_sign(&bots, "Zarvok", false, None), Some(false));
}

#[test]
fn the_bots_names_come_from_the_game_data() {
    use alogs_meter::results_parse::load_bot_names;
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bot-names");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(dir.join("game_cycles.json"));
    assert_eq!(load_bot_names(&dir), ["flora", "mario", "leonard", "tammy"], "none in the data: the four");
    std::fs::write(dir.join("game_cycles.json"), r#"{"stamp": "v4:1:2", "cycles": [], "bot_names": ["Quill", "Brannoc"]}"#).unwrap();
    assert_eq!(load_bot_names(&dir), ["quill", "brannoc"]);
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
