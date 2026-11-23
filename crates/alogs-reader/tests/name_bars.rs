//! A name's 1, l and I told apart by their own ink (`NameReader::settle_look_alikes`): the 1 by its flag and its
//! width (twice a bar's), a bar as l when it stands as tall as the name's tall small letters and as I at its capitals'
//! height, only when the name has both to measure against. The game's font can't be in the repository, so the shapes
//! are drawn by hand and given to the reader as its font's (its l a little taller than its I, as the game's is); the
//! names are drawn from them in white, as the game shows names, on a dark ground.

use alogs_pil::{Image, Mode};
use alogs_reader::gamefont::{FONT_NAME, GameFont};
use alogs_reader::names::NameReader;
use std::path::PathBuf;

const CAP: usize = 36;
const TALL: usize = 39;
const SMALL: usize = 24;
const S: usize = 5;

fn rect(g: &mut Image, x0: usize, y0: usize, x1: usize, y1: usize) {
    for y in y0..y1.min(g.height) {
        for x in x0..x1.min(g.width) {
            g.data[y * g.width + x] = 255;
        }
    }
}

/// The hand-drawn shapes: H (a capital), k (a tall small letter), x (a small letter), the bars I and l, the 1 (a
/// flag at its top left, then its stem), and a round icon-like block (`#`) for the pills' marks.
fn shape(ch: char) -> Image {
    match ch {
        'H' => {
            let mut g = Image::new(Mode::L, 26, CAP, &[0]);
            rect(&mut g, 0, 0, S, CAP);
            rect(&mut g, 21, 0, 26, CAP);
            rect(&mut g, 0, CAP / 2 - 2, 26, CAP / 2 + 3);
            g
        }
        'k' => {
            let mut g = Image::new(Mode::L, 20, TALL, &[0]);
            rect(&mut g, 0, 0, S, TALL);
            rect(&mut g, 0, TALL - SMALL / 2 - 2, 20, TALL - SMALL / 2 + 3);
            rect(&mut g, 15, TALL - SMALL, 20, TALL);
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
        'I' => {
            let mut g = Image::new(Mode::L, S, CAP, &[0]);
            rect(&mut g, 0, 0, S, CAP);
            g
        }
        'l' => {
            let mut g = Image::new(Mode::L, S, TALL, &[0]);
            rect(&mut g, 0, 0, S, TALL);
            g
        }
        '1' => {
            let mut g = Image::new(Mode::L, 14, TALL, &[0]);
            rect(&mut g, 0, 0, 14, 7);
            rect(&mut g, 9, 0, 14, TALL);
            g
        }
        _ => {
            let mut g = Image::new(Mode::L, 28, 28, &[0]);
            rect(&mut g, 0, 0, 28, 28);
            g
        }
    }
}

fn reader() -> NameReader {
    let r = NameReader::new(GameFont::open(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/font"), FONT_NAME).expect("the synthetic font"));
    for ch in "Hkx1Il".chars() {
        r.set_glyph(ch, 64, &shape(ch));
    }
    r
}

/// The text drawn from the shapes, white on dark, 6 px apart (a space: 34 px), on one baseline.
fn drawn(text: &str) -> Image {
    let mut im = Image::new(Mode::Rgb, 500, 60, &[45, 40, 50]);
    let base = 50;
    let mut x = 20;
    for ch in text.chars() {
        if ch == ' ' {
            x += 28;
            continue;
        }
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
    r.settle_look_alikes(&drawn(picture), (0.0, 0.0, 500.0, 60.0), read)
}

#[test]
fn the_one_by_its_flag_and_width() {
    let r = reader();
    assert_eq!(settle(&r, "Hk1x", "Hklx"), ("Hk1x".into(), vec![]));
    assert_eq!(settle(&r, "Hk1x", "HkIx"), ("Hk1x".into(), vec![]));
    assert_eq!(settle(&r, "Hk1x", "Hk1x"), ("Hk1x".into(), vec![]));
    // no capital or tall letter needed to tell a 1 from a bar
    assert_eq!(settle(&r, "x1x", "xlx"), ("x1x".into(), vec![]));
}

#[test]
fn a_bar_by_its_height_when_both_are_measured() {
    let r = reader();
    assert_eq!(settle(&r, "HkIx", "Hklx"), ("HkIx".into(), vec![]));
    assert_eq!(settle(&r, "HkIx", "Hk1x"), ("HkIx".into(), vec![]));
    assert_eq!(settle(&r, "Hklx", "HkIx"), ("Hklx".into(), vec![]));
    assert_eq!(settle(&r, "Hklx", "Hk1x"), ("Hklx".into(), vec![]));
}

#[test]
fn a_bar_without_both_measures_is_left_as_read() {
    let r = reader();
    // no tall small letter: an I's height and an l's can't be told apart against one measure
    assert_eq!(settle(&r, "Hlx", "HIx"), ("HIx".into(), vec![1]));
    // no capital
    assert_eq!(settle(&r, "kIx", "klx"), ("klx".into(), vec![1]));
    // a bar read as 1: known not to be a 1, but neither l nor I is clear: left as read
    assert_eq!(settle(&r, "Hlx", "H1x"), ("H1x".into(), vec![1]));
}

#[test]
fn the_name_among_a_pills_marks() {
    let r = reader();
    let pill = drawn("# Hk1x #");
    let b = (0.0, 0.0, 500.0, 60.0);
    assert_eq!(r.settle_look_alikes_in_line(&pill, b, "HkIx"), ("Hk1x".into(), vec![]));
    // the plain settle sees the marks as letters too: nothing settled
    assert_eq!(r.settle_look_alikes(&pill, b, "HkIx"), ("HkIx".into(), vec![2]));
    // no group of the name's length: nothing settled
    assert_eq!(r.settle_look_alikes_in_line(&pill, b, "HkIxx"), ("HkIxx".into(), vec![2]));
}
