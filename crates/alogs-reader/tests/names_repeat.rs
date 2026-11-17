//! A shape read a second time comes from the fit cache (it panicked on every repeat read of a name: "RefCell already
//! borrowed", seen in the meter on Egg Heist's second end-screen read).

use alogs_pil::{Image, Mode};
use alogs_reader::gamefont::{FONT_NAME, GameFont};
use alogs_reader::names::NameReader;
use std::path::PathBuf;

#[test]
fn the_same_shape_twice() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/font");
    let r = NameReader::new(GameFont::open(&dir, FONT_NAME).expect("the synthetic font"));
    let mut part = Image::new(Mode::L, 12, 18, &[0]);
    for y in 3..15 {
        for x in 5..7 {
            part.data[y * 12 + x] = 255;
        }
    }
    let first = r.shape_fit(&part, '1', 64);
    let again = r.shape_fit(&part, '1', 64);
    assert_eq!(first, again);
}
