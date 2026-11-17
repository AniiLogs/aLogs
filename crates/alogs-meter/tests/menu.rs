//! Your character's name from the menu (two reads alike with your UID under it) and Link character.

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

fn meter(name: &str) -> Meter {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let env = Env::new(data.join("res"), data);
    env.fixed_now.set(Some(1_800_000_000.0));
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: true,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let mut m = Meter::new(env, args, Box::new(FakeOcr(IndexMap::new())), None);
    m.set_uid("123456789");
    m
}

#[test]
fn the_menu_proves_your_name() {
    let mut m = meter("menu-name");
    m.game_menu("Kyhra", "UID: 12345678", None, 0.0, 0.0, 0.0, "menu", true);
    m.game_menu("Kyhra", "UID: 12345678", None, 0.0, 0.0, 0.0, "menu", true);
    assert_eq!(m.character_name, None, "another UID proves nothing");
    m.game_menu("Kyhra", "UID: 12345678 9", None, 0.0, 0.0, 0.0, "menu", true);
    assert_eq!(m.character_name, None, "one read isn't enough");
    m.game_menu("Kyhra", "UID: 123456789", None, 0.0, 0.0, 0.0, "menu", true);
    assert_eq!(m.character_name.as_deref(), Some("Kyhra"));
    assert!(alogs_meter::team::character_verified(&m.env.data, Some("123456789")));
    // a proven name isn't replaced by another read
    m.game_menu("Kyhro", "UID: 123456789", None, 0.0, 0.0, 0.0, "menu", true);
    m.game_menu("Kyhro", "UID: 123456789", None, 0.0, 0.0, 0.0, "menu", true);
    assert_eq!(m.character_name.as_deref(), Some("Kyhra"));
    // and comes back for the UID next time
    let mut again = Meter::new(Env::new(m.env.res.clone(), m.env.data.clone()), Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0,
        level: None, boss_id: None, replay: true, no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true,
        player_log: PathBuf::new() }, Box::new(FakeOcr(IndexMap::new())), None);
    again.set_uid("123456789");
    assert_eq!(again.character_name.as_deref(), Some("Kyhra"));
}

#[test]
fn link_character_reads_then_waits_for_the_player() {
    let mut m = meter("menu-link");
    let t = 1_800_000_000.0;
    let file = m.env.data.join("character_link.json");
    std::fs::write(&file, format!(r#"{{"request": {t}}}"#)).unwrap();
    m.link_poll(t);
    assert_eq!(m.menu.link.as_ref().unwrap().state, "waiting");
    for _ in 0..3 { m.game_menu("Mira", "UID 123456789", None, 0.0, 0.0, 0.0, "profile", true); }
    assert_eq!(m.menu.link.as_ref().unwrap().state, "found");
    assert_eq!(m.character_name, None, "not before the player confirms");
    std::fs::write(&file, format!(r#"{{"request": {t}, "confirm": {{"uid": "123456789", "name": "Mira"}}}}"#)).unwrap();
    m.link_poll(t + 5.0);
    assert_eq!(m.menu.link.as_ref().unwrap().state, "done");
    assert_eq!(m.character_name.as_deref(), Some("Mira"));
    std::fs::remove_file(&file).unwrap();
    m.link_poll(t + 6.0);
    assert!(m.menu.link.is_none());
}

#[test]
fn the_exp_card_is_noted_once_it_goes() {
    let mut m = meter("menu-exp");
    m.ocr = Box::new(FakeOcr([("n".to_string(), "Level: 64".to_string())].into_iter().collect()));
    let im = Image::new(Mode::Rgb, 1920, 1080, &[40, 40, 40]);
    let t = 1_800_000_000.0;
    let heads = |pe: &str| -> IndexMap<String, String> { [("pl", "Level 64"), ("pe", pe)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() };
    // the card counts up while it slides in: short reads, never too high
    for (i, pe) in ["985/80000", "5085/80000", "5O85/80000"].iter().enumerate() {
        m.profile_notes(&heads(pe), &im, t + i as f64 * 0.5, 1920.0, 0.0, 1080.0);
    }
    let path = m.env.data.join("logs/profile_events.csv");
    assert!(!path.exists(), "not while the card is up");
    m.profile_notes(&IndexMap::new(), &im, t + 4.0, 1920.0, 0.0, 1080.0);
    let csv = std::fs::read_to_string(&path).unwrap();
    assert!(csv.contains(",123456789,exp,64,5085,80000,"), "{csv}");
    // the same numbers again later: behind what's noted, not noted again
    m.profile_notes(&heads("5085/80000"), &im, t + 10.0, 1920.0, 0.0, 1080.0);
    m.profile_notes(&IndexMap::new(), &im, t + 13.0, 1920.0, 0.0, 1080.0);
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
}
