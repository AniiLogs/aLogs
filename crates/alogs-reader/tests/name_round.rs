//! A name's 0, O and o told apart by their own ink (`NameReader::settle_round`): a capital's height or a small
//! letter's, and the width for the height (0 narrow, O round). The game's font can't be in the repository, so the
//! shapes are drawn by hand and given to the reader as its font's; the names are drawn from them in white, as the game
//! shows names, on a dark ground.

use alogs_pil::{Image, Mode};
use alogs_reader::gamefont::{FONT_NAME, GameFont};
use alogs_reader::names::NameReader;
use std::path::PathBuf;

const CAP: usize = 36;
const SMALL: usize = 24;
const S: usize = 5;

fn rect(g: &mut Image, x0: usize, y0: usize, x1: usize, y1: usize) {
    for y in y0..y1.min(g.height) {
        for x in x0..x1.min(g.width) {
            g.data[y * g.width + x] = 255;
        }
    }
}

/// A ring w wide, h tall.
fn ring(w: usize, h: usize) -> Image {
    let mut g = Image::new(Mode::L, w, h, &[0]);
    rect(&mut g, 0, 0, w, S);
    rect(&mut g, 0, h - S, w, h);
    rect(&mut g, 0, 0, S, h);
    rect(&mut g, w - S, 0, w, h);
    g
}

/// The hand-drawn shapes: H and x as the capitals' and small letters' references, 0 narrow, O round, o small.
fn shape(ch: char) -> Image {
    match ch {
        'H' => {
            let mut g = Image::new(Mode::L, 26, CAP, &[0]);
            rect(&mut g, 0, 0, S, CAP);
            rect(&mut g, 21, 0, 26, CAP);
            rect(&mut g, 0, CAP / 2 - 2, 26, CAP / 2 + 3);
            g
        }
        'x' => {
            let mut g = Image::new(Mode::L, 22, SMALL, &[0]);
            for y in 0..SMALL {
                let k = y * (22 - S) / SMALL;
                rect(&mut g, k, y, k + S, y + 1);
                rect(&mut g, 22 - S - k, y, 22 - k, y + 1);
            }
            g
        }
        '0' => ring(20, CAP),
        'O' => ring(34, CAP),
        'o' => ring(24, SMALL),
        _ => ring(27, CAP), // (an in-between shape, for the unclear case)
    }
}

fn reader() -> NameReader {
    let r = NameReader::new(GameFont::open(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/font"), FONT_NAME).expect("the synthetic font"));
    for ch in "Hx0Oo".chars() {
        r.set_glyph(ch, 64, &shape(ch));
    }
    r
}

/// The name drawn from the shapes (`?` draws the in-between ring), white on dark, 6 px apart, on one baseline.
fn drawn(text: &str) -> Image {
    let mut im = Image::new(Mode::Rgb, 400, 60, &[45, 40, 50]);
    let base = 48;
    let mut x = 20;
    for ch in text.chars() {
        let g = shape(ch);
        for y in 0..g.height {
            for gx in 0..g.width {
                if g.data[y * g.width + gx] != 0 {
                    im.putpixel(x + gx, base - g.height + y, &[245, 245, 245]);
                }
            }
        }
        x += g.width + 6;
    }
    im
}

fn settle(r: &NameReader, picture: &str, read: &str) -> (String, Vec<usize>) {
    r.settle_round(&drawn(picture), (0.0, 0.0, 400.0, 60.0), read)
}

#[test]
fn each_decided_by_its_size() {
    let r = reader();
    // the zero read as a capital O (the owner's case), and every other mix-up
    assert_eq!(settle(&r, "Hx0x", "HxOx"), ("Hx0x".into(), vec![]));
    assert_eq!(settle(&r, "Hx0x", "Hxox"), ("Hx0x".into(), vec![]));
    assert_eq!(settle(&r, "HxOx", "Hx0x"), ("HxOx".into(), vec![]));
    assert_eq!(settle(&r, "HxOx", "Hxox"), ("HxOx".into(), vec![]));
    assert_eq!(settle(&r, "Hxox", "HxOx"), ("Hxox".into(), vec![]));
    assert_eq!(settle(&r, "Hxox", "Hx0x"), ("Hxox".into(), vec![]));
    // read right already: kept
    assert_eq!(settle(&r, "H0Ox", "H0Ox"), ("H0Ox".into(), vec![]));
}

#[test]
fn an_unclear_one_is_left_as_read() {
    let r = reader();
    // a ring between the 0's width and the O's: neither clearly
    assert_eq!(settle(&r, "Hx?x", "HxOx"), ("HxOx".into(), vec![2]));
    // more letters read than the ink has: nothing settled
    assert_eq!(settle(&r, "Hx0x", "HxOxx"), ("HxOxx".into(), vec![2]));
    // no capital to measure against: left as read
    assert_eq!(settle(&r, "x0x", "xOx"), ("xOx".into(), vec![1]));
}
