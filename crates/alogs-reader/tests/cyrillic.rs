//! Cyrillic names (cyrillic.rs) on the synthetic font of tests/data/font, which has Latin letters only: here its
//! Cyrillic letters are borrowed. The look-alikes take their Latin twin's shape (Н is H), and Я is a mirrored R, a
//! shape no Latin letter has. The names are made up; no game file is used.

use alogs_pil::{Image, Mode, Transpose};
use alogs_reader::cyrillic::{self, CyrillicNames};
use alogs_reader::gamefont::{FONT_NAME, GameFont};
use alogs_reader::hangul::{self, Letters};
use std::path::PathBuf;

fn synthetic() -> GameFont {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/font");
    GameFont::open(&dir, FONT_NAME).expect("the synthetic font")
}

/// The synthetic font with Cyrillic letters: Я я a mirrored R (drawn into a widened atlas), every other one the shape
/// of a Latin letter (its twin where it has one).
fn font() -> GameFont {
    let mut f = synthetic();
    let pad = f.meta["padding"].as_f64().unwrap();
    let ah = f.meta["atlas_height"].as_f64().unwrap();
    let [gx, gy, gw, gh, bx, by, adv] = f.glyphs[&('R' as u32)];
    let (x0, y0) = ((gx - pad) as i64, (ah - (gy + gh) - pad) as i64);
    let r = f.atlas.crop_i(x0, y0, (gx + gw + pad) as i64, (ah - gy + pad) as i64).transpose(Transpose::FlipLeftRight);
    let old = f.atlas.width;
    let mut atlas = Image::new(Mode::L, old + r.width + 4, f.atlas.height, &[0]);
    atlas.paste(&f.atlas, 0, 0);
    atlas.paste(&r, old as i64 + 2, y0);
    f.atlas = atlas;
    let ya = [old as f64 + 2.0 + pad, gy, gw, gh, adv - bx - gw, by, adv];
    let twin = |c: char| match c {
        'А' => 'A', 'В' => 'B', 'Е' => 'E', 'К' => 'K', 'М' => 'M', 'Н' | 'н' => 'H', 'О' => 'O', 'Р' => 'P', 'С' => 'C',
        'Т' => 'T', 'Х' => 'X', 'а' => 'a', 'е' => 'e', 'о' => 'o', 'р' => 'p', 'с' => 'c', 'у' => 'y', 'х' => 'x',
        _ => 'Z',
    };
    for c in ('А'..='я').chain(['Ё', 'ё']) {
        let g = if c == 'Я' || c == 'я' { ya } else { f.glyphs[&(twin(c) as u32)] };
        f.glyphs.insert(c as u32, g);
    }
    f
}

/// `text` in the font, black on white as a tidied name box, and each letter's spot (the centre of its ink).
fn draw(f: &GameFont, text: &str) -> (Image, Vec<f64>) {
    let (field, _) = f.field(text, 64.0);
    let ink = field.point(|v| if v > 120 { 255.0 } else { 0.0 });
    let b = ink.getbbox().unwrap();
    let ink = ink.crop_i(b.0 as i64 - 12, b.1 as i64 - 12, b.2 as i64 + 12, b.3 as i64 + 12);
    let mut xs = vec![];
    let mut start = None;
    for x in 0..=ink.width {
        let on = x < ink.width && (0..ink.height).any(|y| ink.data[y * ink.width + x] != 0);
        if on && start.is_none() {
            start = Some(x);
        } else if !on && let Some(s) = start {
            xs.push((s + x) as f64 / 2.0);
            start = None;
        }
    }
    (ink.point(|v| if v != 0 { 0.0 } else { 255.0 }), xs)
}

/// The Cyrillic model's read as the helper sends it: per letter its spot and its likeliest characters.
fn letters(xs: &[f64], picks: &[&[(&str, f64)]]) -> Option<Letters> {
    assert_eq!(xs.len(), picks.len());
    Some(xs.iter().zip(picks).map(|(x, p)| (*x, p.iter().map(|(c, q)| (c.to_string(), *q)).collect())).collect())
}

#[test]
fn a_letter_no_latin_one_looks_like_makes_it_cyrillic() {
    let f = font();
    let names = CyrillicNames::new(font()).expect("the font draws Cyrillic");
    let (img, xs) = draw(&f, "Яна");
    let read = names.read(&img, &letters(&xs, &[&[("Я", 0.9), ("R", 0.05)], &[("н", 0.8), ("H", 0.15)], &[("а", 0.9), ("a", 0.09)]]));
    assert_eq!(read.as_deref(), Some("Яна"));
    // the model sure of the Latin twins of the look-alikes: in a Cyrillic word they are Cyrillic all the same
    let read = names.read(&img, &letters(&xs, &[&[("Я", 0.9)], &[("H", 0.8), ("н", 0.15)], &[("a", 0.9), ("а", 0.09)]])).unwrap();
    assert!(read.starts_with('Я') && read.chars().count() == 3 && read.chars().all(cyrillic::is_cyrillic), "{read:?}");
}

#[test]
fn latin_ink_stays_latin_whatever_the_model_says() {
    let f = font();
    let names = CyrillicNames::new(font()).unwrap();
    // an R drawn, the model taking it for Я: the font says R, whether the model offers R or not
    let (img, xs) = draw(&f, "Ran");
    assert_eq!(names.read(&img, &letters(&xs, &[&[("Я", 0.9), ("R", 0.05)], &[("а", 0.9)], &[("п", 0.6), ("n", 0.3)]])), None);
    assert_eq!(names.read(&img, &letters(&xs, &[&[("Я", 0.99)], &[("а", 0.9)], &[("п", 0.9)]])), None);
}

#[test]
fn look_alikes_alone_stay_latin() {
    let f = font();
    let names = CyrillicNames::new(font()).unwrap();
    let (img, xs) = draw(&f, "Hoc");
    assert_eq!(names.read(&img, &letters(&xs, &[&[("Н", 0.9)], &[("о", 0.9)], &[("с", 0.9)]])), None);
}

#[test]
fn the_party_badge_stays_latin() {
    let f = font();
    let names = CyrillicNames::new(font()).unwrap();
    let (img, xs) = draw(&f, "2P  Яна");
    let read = names.read(&img, &letters(&xs, &[&[("2", 0.9)], &[("Р", 0.7), ("P", 0.3)], &[("Я", 0.9)], &[("н", 0.9)], &[("а", 0.9)]]));
    assert_eq!(read.as_deref(), Some("2P Яна"));
}

#[test]
fn a_font_without_cyrillic_turns_it_off() {
    assert!(CyrillicNames::new(synthetic()).is_none());
}

#[test]
fn which_letters_tell() {
    // the letters the game draws like Latin ones never tell a Cyrillic name
    for c in "АВЕКМНОРСТХаеорсух".chars() {
        assert!(!cyrillic::telling(c), "{c}");
    }
    for c in "ДЖЗИЙЛПУФЦЧШЩЪЫЬЭЮЯбгджзийлтфцчшщъыьэюя".chars() {
        assert!(cyrillic::telling(c), "{c}");
    }
    assert!(!cyrillic::telling('R') && !cyrillic::telling('7'));
    assert!(cyrillic::has_cyrillic("Ab Дc") && !cyrillic::has_cyrillic("Abc 한"));
}

#[test]
fn the_helpers_fourth_field() {
    let line = "\t[[3.0,[[\"ㄱ\",0.9]]]]\tAb\t[[3.0,[[\"Д\",0.8],[\"A\",0.1]]],[9.5,[[\"а\",0.7]]]]";
    let l = hangul::cyrillic_letters(line).unwrap();
    assert_eq!(l.len(), 2);
    assert_eq!(l[0].1[0], ("Д".to_string(), 0.8));
    assert_eq!(l[1].0, 9.5);
    // the first three fields read as before
    let (text, korean, latin) = hangul::split_reply(line);
    assert_eq!((text.as_str(), korean.map(|k| k.len()), latin.as_str()), ("", Some(1), "Ab"));
    // an answer without the Cyrillic model's field
    assert!(hangul::cyrillic_letters("\t\tAb").is_none());
    assert!(hangul::cyrillic_letters("\t\tAb\t").is_none());
}
