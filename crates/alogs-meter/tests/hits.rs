//! "You're being attacked" with a stand-in HP reader: a drop read three times in a row raises it; a number flipping
//! between two readings (a misread) and a small drop on its own don't.

use alogs_meter::env::Env;
use alogs_meter::meter::{Args, Frames, Meter};
use alogs_pil::{Image, Mode};
use alogs_reader::ocr::{NoOcr, Reader};
use indexmap::IndexMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct HpOcr(Arc<Mutex<String>>);
impl Reader for HpOcr {
    fn read(&self, images: &IndexMap<String, Image>, _: &IndexMap<String, usize>, _: &IndexMap<String, Image>) -> IndexMap<String, String> {
        images.keys().map(|k| (k.clone(), self.0.lock().unwrap().clone())).collect()
    }
    fn cached(&self, _: &IndexMap<String, Image>, _: &IndexMap<String, usize>) -> bool { true }
    fn language(&self) -> Option<String> { Some("en-US".into()) }
    fn started(&self) -> bool { true }
}

/// Tabbed out; each strip differs so every read goes to the OCR.
struct Away(u8);
impl Frames for Away {
    fn grab(&mut self, _: usize, _: Option<f64>) -> Option<Image> { None }
    fn window(&mut self) -> Option<usize> { Some(1) }
    fn is_foreground(&self, _: usize) -> bool { false }
    fn hp_strip(&mut self, _: usize, _: (f64, f64, f64, f64)) -> Option<Image> {
        self.0 = self.0.wrapping_add(1);
        Some(Image::new(Mode::Rgb, 80, 16, &[self.0, 0, 0]))
    }
}

fn meter(name: &str) -> (Meter, Arc<Mutex<String>>) {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let env = Env::new(data.join("res"), data);
    env.fixed_now.set(Some(1_800_000_000.0));
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: false,
                      no_submit: true, no_debug_frames: true, no_overlay: false, no_auto_click: true, player_log: PathBuf::new() };
    let mut m = Meter::new(env, args, Box::new(NoOcr), None);
    let text = Arc::new(Mutex::new(String::new()));
    m.hp_ocr = Some(Box::new(HpOcr(text.clone())));
    m.hwnd = Some(1);
    m.frame_size = (1920, 1080);
    (m, text)
}

fn feed(m: &mut Meter, f: &mut Away, text: &Arc<Mutex<String>>, reads: &[&str], t: &mut f64) {
    for r in reads {
        *text.lock().unwrap() = r.to_string();
        m.hit_watch(f, *t);
        *t += 0.3;
    }
}

#[test]
fn a_big_drop_raises_the_alert() {
    let (mut m, text) = meter("hits-big");
    let mut f = Away(0);
    let mut t = 1_800_000_000.0;
    feed(&mut m, &mut f, &text, &["12694/12694", "12694/12694"], &mut t);
    feed(&mut m, &mut f, &text, &["12000/12694", "12000/12694"], &mut t);
    assert_eq!(m.hit_at, 0.0, "two reads aren't enough");
    feed(&mut m, &mut f, &text, &["12000/12694"], &mut t);
    assert!(m.hit_at > 0.0);
}

#[test]
fn a_misread_flipping_back_and_forth_is_not_a_hit() {
    let (mut m, text) = meter("hits-flip");
    let mut f = Away(0);
    let mut t = 1_800_000_000.0;
    feed(&mut m, &mut f, &text, &["11390/12694", "11390/12694"], &mut t);
    feed(&mut m, &mut f, &text, &["11356/12694"; 3], &mut t);      // small, on its own: not taken
    feed(&mut m, &mut f, &text, &["11390/12694"; 2], &mut t);      // back up
    feed(&mut m, &mut f, &text, &["11356/12694"; 3], &mut t);      // the same two again: a flip
    assert_eq!(m.hit_at, 0.0);
}
