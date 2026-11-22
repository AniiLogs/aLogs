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

fn lobby(names: &[&str]) -> Vec<alogs_pyval::Dict> {
    names.iter().enumerate().map(|(i, n)| member(i as i64, n, false)).collect()
}

/// A party's votes are one lobby's: the next lobby, seven minutes on with the same four seats and two new players,
/// reads as its own players at once, on its first look already, whichever sign of the new lobby the meter saw.
#[test]
fn the_next_lobby_is_voted_afresh() {
    let first = ["Kyhra", "Zarvok", "MinMax", "Ashgrove"];
    let second = ["Kyhra", "Pellinore", "MinMax", "Quillon"];
    let t = 1_800_000_000.0;
    // (game log scene, a run between, minutes between the lobbies)
    for (i, (spaces, run, gap)) in [((Some("lobbyA"), Some("lobbyB")), true, 7.0), ((Some("lobbyA"), Some("lobbyB")), false, 0.5),
                                    ((None, None), true, 0.5), ((None, None), false, 7.0)].into_iter().enumerate() {
        let mut m = meter(&format!("party-lobbies-{i}"), &[]);
        m.space = spaces.0.map(String::from);
        for k in 0..60 {
            m.set_party(lobby(&first), t + 2.0 * k as f64);
        }
        assert_eq!(names(&m), first);
        if run {
            m.ps.runs += 1; // a fight started (or a result was recorded)
        }
        m.space = spaces.1.map(String::from);
        let t2 = t + 120.0 + gap * 60.0;
        m.set_party(lobby(&second), t2);
        assert_eq!(names(&m), second, "case {i}: one look of the new lobby");
        for k in 1..6 {
            m.set_party(lobby(&second), t2 + 2.0 * k as f64);
        }
        assert_eq!(names(&m), second, "case {i}: a handful of looks");
        // a look of the new lobby that misses a seat doesn't bring the old lobby's player back
        let mut m = meter(&format!("party-lobbies-kept-{i}"), &[]);
        m.space = spaces.0.map(String::from);
        m.set_party(lobby(&first), t);
        if run {
            m.ps.runs += 1;
        }
        m.space = spaces.1.map(String::from);
        let t2 = t + gap * 60.0;
        m.set_party(lobby(&second[..3]), t2);
        assert_eq!(names(&m), second[..3], "case {i}: no seat kept from the lobby before");
    }
    // within one lobby (same scene, no run, a minute away) the seats' votes carry on as before
    let mut m = meter("party-lobbies-same", &[]);
    m.space = Some("lobbyA".into());
    for k in 0..5 {
        m.set_party(lobby(&first), t + 2.0 * k as f64);
    }
    m.set_party(lobby(&["Kyhra", "Zarv0k", "MinMax", "Ashgrove"]), t + 70.0);
    assert_eq!(names(&m), first, "a misread is outvoted");
}

/// Within one lobby a player can leave and another take the seat: two looks in a row with a new name there, and the
/// new name leads. One odd look between doesn't unseat a settled name, and a misread (a name alike) never counts.
#[test]
fn a_new_player_takes_a_seat_after_two_looks() {
    let t = 1_800_000_000.0;
    let mut m = meter("party-seat-change", &[]);
    for k in 0..10 {
        m.set_party(lobby(&["Kyhra", "Zarvok"]), t + 2.0 * k as f64);
    }
    m.set_party(lobby(&["Kyhra", "Pellinore"]), t + 30.0);
    assert_eq!(names(&m), ["Kyhra", "Zarvok"], "one look: still the settled name");
    m.set_party(lobby(&["Kyhra", "Pellinore"]), t + 32.0);
    assert_eq!(names(&m), ["Kyhra", "Pellinore"], "two looks in a row: the new player");
    m.set_party(lobby(&["Kyhra", "Zarvok"]), t + 34.0);
    assert_eq!(names(&m), ["Kyhra", "Pellinore"], "one look of the old name doesn't bring it back");
    // one odd look (a banner over the name) between settled looks
    let mut m = meter("party-seat-odd", &[]);
    for k in 0..10 {
        m.set_party(lobby(&["Kyhra", "Zarvok"]), t + 2.0 * k as f64);
    }
    for (k, n) in ["Wwmmqq", "Zarvok", "Pellinore", "Zarv0k", "Wwmmqq", "Pellinore"].iter().enumerate() {
        m.set_party(lobby(&["Kyhra", n]), t + 30.0 + 2.0 * k as f64);
        assert_eq!(names(&m), ["Kyhra", "Zarvok"], "look {k} ({n}): odd looks not in a row, or not alike each other");
    }
}

/// A result column read alike again and again keeps its own name over a wholly different Party name, and says so
/// (the lineup yields to it at send); a first look, before any repeat, still takes the Party screen's name.
#[test]
fn a_settled_result_name_is_marked() {
    use alogs_meter::results_parse::party_slots;
    use alogs_meter::team::Teammates;
    use std::collections::BTreeSet;
    let team = Teammates::load(PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("settled-teammates.json"));
    let party: Vec<Value> = lobby(&["Kyhra", "Zarvok", "MinMax"]).into_iter().map(Value::Dict).collect();
    let read = || vec![member(0, "Kyhra", false), member(1, "Pellinore", false), member(2, "MinMax", false)];
    let mut players = read();
    let settled = party_slots(&team, &mut players, Some(&party), &BTreeSet::new(), None, &[0, 1, 2].into());
    assert_eq!(players[1].at("name").py_str(), "Pellinore");
    assert_eq!(settled, [1].into());
    let mut players = read();
    let settled = party_slots(&team, &mut players, Some(&party), &BTreeSet::new(), None, &BTreeSet::new());
    assert_eq!(players[1].at("name").py_str(), "Zarvok", "a first look: the Party screen's name, a first guess");
    assert!(settled.is_empty());
    // an alike misread of the Party name is not a settled other name
    let mut players = vec![member(0, "Kyhra", false), member(1, "Zarv0kk", false)];
    assert!(party_slots(&team, &mut players, Some(&party), &BTreeSet::new(), None, &[0, 1].into()).is_empty());
}

/// A name of one or two characters ("7", "ab", "42") is all the pill holds beside a clean badge: kept. A crumb beside
/// a real name is still dropped, and a crumb with no badge, or beside a misread one, is still nothing.
#[test]
fn a_short_name_beside_a_clean_badge_is_kept() {
    use alogs_meter::team::party_pill;
    let p = |s: &str, i: i64| party_pill(s, i);
    assert_eq!(p("1P 7", 0), Some((0, "7".into())), "a one-character name");
    assert_eq!(p("2P ab", 1), Some((1, "ab".into())), "two lowercase letters");
    assert_eq!(p("3P 42", 2), Some((2, "42".into())), "two digits");
    assert_eq!(p("4P q", 3), Some((3, "q".into())));
    assert_eq!(p("2P7", 1), Some((1, "7".into())), "glued to its badge");
    assert_eq!(p("3P Mira Vosk x", 2), Some((2, "MiraVosk".into())), "a crumb beside a real name is dropped");
    assert_eq!(p("2P @ Flora O", 0), Some((1, "Flora".into())));
    assert_eq!(p("x", 0), None, "no badge, only a crumb");
    assert_eq!(p("@ iA", 0), None);
    assert_eq!(p("lP x", 0), None, "a misread badge doesn't vouch for a crumb");
    assert_eq!(p(".4pR", 0), None);
    assert_eq!(p("2P @ O", 1), None, "two crumbs: no name");
    assert_eq!(p("2P @", 1), None, "a sign alone is no name");
    assert_eq!(p("2P", 1), None, "the badge alone");
}

/// The Party screen with a one-character player: the seat is kept, and the run's lineup carries it.
#[test]
fn a_one_character_player_keeps_their_seat_in_the_lineup() {
    let mut m = meter("party-short-name", &[("n0", "1P Zarvok"), ("n1", "2P 7"), ("n2", "3P ab"),
                                            ("a0", "Irisalis / Lv.60 CP:3578"), ("a1", "Spark / Lv.52 CP 2901"), ("a2", "Mossy / Lv.40 CP 1200")]);
    m.character_name = Some("7".into());
    let im = Image::new(Mode::Rgb, 1920, 1080, &[40, 40, 40]);
    let t = 1_800_000_000.0;
    m.party_screen(&im, t, 1920.0, 0.0, 1080.0);
    m.party_screen(&im, t + 2.0, 1920.0, 0.0, 1080.0);
    assert_eq!(names(&m), ["Zarvok", "7", "ab"]);
    let party = m.party.as_ref().unwrap().get("members").as_list().unwrap().clone();
    assert!(party[1].get("self").truthy(), "your own seat, by your name");
    assert_eq!(party[1].get("cp").as_i64(), Some(2901));
    let summary = alogs_pyval::dict([("party", Value::List(party)), ("result", alogs_pyval::dict([("players", Value::List(vec![]))]))]);
    let lineup = alogs_meter::payloads::lineup(&summary);
    let seats: Vec<(i64, String)> = lineup.get("lineup").as_list().unwrap().iter().map(|e| (e.get("slot").as_i64().unwrap(), e.get("name").py_str())).collect();
    assert_eq!(seats, [(0, "Zarvok".to_string()), (1, "7".to_string()), (2, "ab".to_string())]);
}
