//! A team card named from its art: every Aniimo's art tried (`Icons::card`), or only the art of the Aniimo the card's
//! own line names (`Icons::card_of`, a few ms, used before aLogs takes control of the Sim's Victory screen).

use alogs_pil::{Filter, Image, Mode};
use alogs_reader::icons::{CACHE_VERSION, Icons};
use std::path::PathBuf;

/// A card art nothing like the others: big blocks of its own colours (it must still tell them apart made small).
fn art(seed: u32) -> Image {
    let (w, h) = (128usize, 210usize);
    let mut data = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let block = (x / 16) as u32 * 31 + (y / 15) as u32 * 17 + seed * 101;
            let v = block.wrapping_mul(2_654_435_761);
            data.extend_from_slice(&[(v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8, 255]);
        }
    }
    Image::from_raw(Mode::Rgba, w, h, data)
}

/// A cache holding four card arts, two of them under one name (an Aniimo with two looks).
fn cache() -> (PathBuf, Icons) {
    let dir = std::env::temp_dir().join(format!("alogs-cards-test-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(dir.join("cards")).unwrap();
    let cards = [("a", "Alpha", 1), ("b", "Beta", 2), ("b2", "Beta", 3), ("c", "Gamma", 4)];
    for (key, _, seed) in cards {
        art(seed).save_png(&dir.join("cards").join(format!("{key}.png"))).unwrap();
    }
    let names: serde_json::Map<String, serde_json::Value> = cards.iter().map(|(k, n, _)| (k.to_string(), serde_json::Value::from(*n))).collect();
    let meta = serde_json::json!({ "version": CACHE_VERSION, "heads": {}, "buffs": {}, "cards": names });
    std::fs::write(dir.join("icons.json"), meta.to_string()).unwrap();
    let icons = Icons::open(&dir).expect("the test's icon cache");
    (dir, icons)
}

/// The screen with card art `seed` drawn at `at` (x, y, w, h), and the window through which part of it shows.
fn screen(seed: u32, at: (f64, f64, f64, f64)) -> (Image, (f64, f64, f64, f64)) {
    let mut im = Image::new(Mode::Rgba, 640, 480, &[20, 20, 20, 255]);
    im.paste(&art(seed).resize(at.2 as usize, at.3 as usize, Some(Filter::Bilinear)), at.0 as i64, at.1 as i64);
    let window = (at.0 + at.2 * 0.2, at.1 + at.3 * 0.2, at.0 + at.2 * 0.8, at.1 + at.3 * 0.6);
    (im, window)
}

#[test]
fn the_named_art_scores_as_the_full_search_does() {
    let (dir, icons) = cache();
    let at = (200.0, 100.0, 160.0, 262.0);
    let (im, window) = screen(3, at); // the second look of "Beta"
    let all = icons.card(&im, window, at, 36).expect("a card");
    assert_eq!(all.0, "Beta");
    assert!(all.1 > 0.9, "the art on the card fits it: {}", all.1);
    // one Aniimo's art tried: the same score, from either of its looks
    let named = icons.card_of(&im, window, at, 36, "Beta").expect("Beta has card art");
    assert!((named - all.1).abs() < 1e-12, "named {named}, searched {}", all.1);
    // another Aniimo's art doesn't fit, and a name the game doesn't have has no art
    let other = icons.card_of(&im, window, at, 36, "Alpha").expect("Alpha has card art");
    assert!(other < 0.6, "another Aniimo's art on this card: {other}");
    assert_eq!(icons.card_of(&im, window, at, 36, "Nobody"), None);
    let _ = std::fs::remove_dir_all(dir);
}

/// Every art's part for a card slot can be made ahead of the match, a few a call: the match then finds them ready and
/// gives the same answer.
#[test]
fn the_arts_for_a_slot_are_made_ready_a_few_at_a_time() {
    let (dir, icons) = cache();
    let at = (200.0, 100.0, 160.0, 262.0);
    let (im, window) = screen(1, at);
    let mut budget = 3;
    assert_eq!(icons.warm_cards(window, at, 36, &mut budget), 1, "four arts, three made");
    assert_eq!(budget, 0);
    let mut budget = 10;
    assert_eq!(icons.warm_cards(window, at, 36, &mut budget), 0, "the last one made");
    assert_eq!(budget, 9, "only the one still missing was made");
    assert_eq!(icons.warm_cards(window, at, 36, &mut budget), 0);
    assert_eq!(budget, 9, "nothing made twice");
    let warm = icons.card(&im, window, at, 36).expect("a card");
    let (dir2, fresh) = cache();
    let cold = fresh.card(&im, window, at, 36).expect("a card");
    assert_eq!((warm.0.as_str(), warm.1, warm.2), (cold.0.as_str(), cold.1, cold.2), "ready or not, the same match");
    assert_eq!(warm.0, "Alpha");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(dir2);
}
