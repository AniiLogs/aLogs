//! The boss's level read from the bar's title against the font's own digits (`NameReader::level`). The game's font
//! can't be in the repository, so the shapes here are drawn by hand (digits that share no ink but their corner marks,
//! a plain L, v and dot) and given to the reader as its font's (`set_glyph`); the titles are drawn from the same shapes at a
//! 4K title box's size, in the colours a level is shown in.

use alogs_pil::{Image, Mode};
use alogs_reader::gamefont::{FONT_NAME, GameFont};
use alogs_reader::names::NameReader;
use std::path::PathBuf;

/// The capitals' height in a 4K title box 77 px tall (0.47 of it, as the game's), the strokes' width.
const CAP: usize = 36;
const STROKE: usize = 6;

fn rect(im: &mut Image, x0: usize, y0: usize, x1: usize, y1: usize) {
    for y in y0..y1.min(im.height) {
        for x in x0..x1.min(im.width) {
            im.data[y * im.width + x] = 255;
        }
    }
}

/// One character's ink (Mode L, 255 = ink), `CAP` tall (the v and the dot lower, on the same baseline).
fn shape(ch: char) -> Image {
    let (w, h) = (if ch == '.' { STROKE + 2 } else { 22 }, CAP);
    let mut g = Image::new(Mode::L, w, h, &[0]);
    let s = STROKE;
        // a digit: a corner mark and a thin foot (so each is the same size and one piece) and one block in a cell of its
    // own (two across, five down): no two digits share ink beyond the mark and the foot
    if let Some(d) = ch.to_digit(10) {
        let (d, cw, chh) = (d as usize, w / 2, h / 5);
        rect(&mut g, 0, 0, s, s);
        rect(&mut g, 0, h - 3, w, h);
        let (cx, cy) = (d % 2 * cw, d / 2 * chh);
        rect(&mut g, cx, cy, cx + cw, cy + chh);
    }
    match ch {
        'L' => {
            rect(&mut g, 0, 0, s, h);
            rect(&mut g, 0, h - s, w, h);
        }
        'v' => {
            // two strokes meeting at the foot, x-height (0.7 of the capitals)
            let top = h - h * 7 / 10;
            for y in top..h {
                let k = (y - top) * (w / 2 - s / 2) / (h - top).max(1);
                rect(&mut g, k, y, k + s, y + 1);
                rect(&mut g, w - k - s, y, w - k, y + 1);
            }
        }
        '.' => rect(&mut g, 0, h - s, s, h),
        '%' => {
            rect(&mut g, 0, 0, 8, 8);
            rect(&mut g, w - 8, h - 8, w, h);
            for y in 0..h {
                let x = (h - 1 - y) * (w - 4) / h;
                rect(&mut g, x, y, x + 4, y + 1);
            }
        }
        _ => {}
    }
    g
}

fn reader() -> NameReader {
    let r = NameReader::new(GameFont::open(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/font"), FONT_NAME).expect("the synthetic font"));
    for ch in "Lv.0123456789".chars() {
        r.set_glyph(ch, 64, &shape(ch));
    }
    r
}

const WHITE: [u8; 3] = [240, 240, 240];
const GREEN: [u8; 3] = [175, 240, 120];
const ORANGE: [u8; 3] = [240, 160, 80];

/// A 4K title box (1440 x 77) on a dark ground with `text` drawn from the shapes in `colour` from x 300: a space is a
/// word gap; `gap` px between characters (negative: touching, overlapping).
fn title(text: &str, colour: [u8; 3], gap: i64) -> Image {
    let mut im = Image::new(Mode::Rgb, 1440, 77, &[40, 32, 30]);
    let base = 77 / 2 + CAP / 2;
    let mut x = 300i64;
    for ch in text.chars() {
        if ch == ' ' {
            x += 24;
            continue;
        }
        let g = shape(ch);
        for y in 0..g.height {
            for gx in 0..g.width {
                if g.data[y * g.width + gx] != 0 {
                    im.putpixel((x + gx as i64) as usize, base - g.height + y, &colour);
                }
            }
        }
        x += g.width as i64 + gap;
    }
    // the separator and the name after it, in white
    let sep = (x + 30) as usize;
    for y in base - CAP..base {
        im.putpixel(sep, y, &[150, 150, 150]);
    }
    for k in 0..5 {
        for y in base - CAP..base {
            for dx in 0..STROKE {
                im.putpixel(sep + 40 + k * 30 + dx, y, &WHITE);
            }
        }
    }
    im
}

fn read(r: &NameReader, im: &Image) -> Option<i64> {
    r.level(im, (0.0, 0.0, im.width as f64, im.height as f64))
}

#[test]
fn the_level_after_lv_in_each_colour() {
    let r = reader();
    assert_eq!(read(&r, &title("Lv. 65", GREEN, 4)), Some(65));
    assert_eq!(read(&r, &title("Lv. 73", ORANGE, 4)), Some(73));
    assert_eq!(read(&r, &title("Lv. 72", WHITE, 4)), Some(72));
    assert_eq!(read(&r, &title("Lv. 6", GREEN, 4)), Some(6), "a low-level boss: one digit");
    assert_eq!(read(&r, &title("Lv. 15", GREEN, 4)), Some(15));
}

#[test]
fn three_digits_a_leading_zero_or_a_stray_symbol_are_unread() {
    let r = reader();
    assert_eq!(read(&r, &title("Lv. 650", GREEN, 4)), None, "three digits");
    assert_eq!(read(&r, &title("Lv. 06", GREEN, 4)), None, "a leading zero");
    assert_eq!(read(&r, &title("Lv. 6%", GREEN, 4)), None, "a symbol after the digit");
    assert_eq!(read(&r, &title("65", GREEN, 4)), None, "no Lv before it");
}

#[test]
fn a_digit_touching_the_dot() {
    let r = reader();
    // "Lv." then the 6 drawn against the dot (no gap)
    let mut im = title("Lv.", GREEN, 4);
    let x = 300 + 22 + 4 + 22 + 4 + STROKE + 2; // past L, v and the dot's box
    let base = 77 / 2 + CAP / 2;
    for (k, ch) in "65".chars().enumerate() {
        let g = shape(ch);
        let gx0 = x - 2 + k * 26; // the 6 starts right at the dot's ink
        for y in 0..g.height {
            for gx in 0..g.width {
                if g.data[y * g.width + gx] != 0 {
                    im.putpixel(gx0 + gx, base - g.height + y, &GREEN);
                }
            }
        }
    }
    assert_eq!(read(&r, &im), Some(65));
}
