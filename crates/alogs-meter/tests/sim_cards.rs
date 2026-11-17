//! The Sim's team cards: the Aniimo a card's own line names (`sim_card_names`), which is checked against that one
//! Aniimo's art before aLogs takes control; the search over every Aniimo's art is left for the cards without one.

use alogs_meter::results_parse::{SIM_CARD_MARGIN, SIM_CARD_SCORE, sim_card_boxes, sim_card_names};
use alogs_meter::sim::SIM_CARD_NAMED;
use indexmap::IndexMap;

fn names() -> Vec<String> {
    ["Hexxin", "Irisalis", "Leafy", "Somniwing", "Turbo"].map(String::from).to_vec()
}

#[test]
fn a_cards_line_names_its_aniimo() {
    let t: IndexMap<String, String> = [
        ("c0w", "Lv.70 Irisalis"),  // read cleanly
        ("c0b", ""),
        ("c1w", "x"),               // only the second read has the line
        ("c1b", "Lv 60 Leafy"),
        ("c2w", "Lv.70 Hexxln"),    // a letter misread: still Hexxin
        ("c3w", "Lv.60 Mr Fluff"),  // a nickname: no Aniimo of the game's
        ("c4w", "Lv.0 Turbo"),      // not a level
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let got = sim_card_names(&t, &names());
    assert_eq!(got, vec![Some("Irisalis".to_string()), Some("Leafy".to_string()), Some("Hexxin".to_string()), None, None]);
    // nothing read at all: no card is named
    assert_eq!(sim_card_names(&IndexMap::new(), &names()), vec![None; 5]);
}

/// By hand, on a real Victory screen (a 16:9 PNG) and the game's icon cache (<aLogs data>\icons):
/// `ALOGS_ICONS=... ALOGS_SIM_SHOT=... cargo test -p alogs-meter --release --test sim_cards -- --ignored --nocapture`
/// Every card the full search is sure of must get the same score from its own Aniimo's art alone, above
/// SIM_CARD_NAMED; the times of both ways are printed.
#[test]
#[ignore = "needs a real Victory screen and the game's icons"]
fn the_named_art_on_a_real_victory_screen() {
    let (Ok(icons), Ok(shot)) = (std::env::var("ALOGS_ICONS"), std::env::var("ALOGS_SIM_SHOT")) else { panic!("set ALOGS_ICONS and ALOGS_SIM_SHOT") };
    let ic = alogs_reader::icons::Icons::open(std::path::Path::new(&icons)).expect("the icon cache");
    let im = alogs_pil::Image::open_png(std::path::Path::new(&shot)).expect("the picture");
    let (w, h) = (im.width as f64, im.height as f64);
    let u = w.min(h * 16.0 / 9.0);
    let ux = (w - u) / 2.0;
    let boxes = sim_card_boxes(u, ux, h);
    for round in ["first", "again"] {
        let t0 = std::time::Instant::now();
        let all: Vec<_> = boxes.iter().map(|(window, art)| ic.card(&im, *window, *art, 36)).collect();
        let searched = t0.elapsed();
        let t1 = std::time::Instant::now();
        let named: Vec<_> = boxes.iter().zip(&all).map(|((window, art), r)| r.as_ref().and_then(|r| ic.card_of(&im, *window, *art, 36, &r.0))).collect();
        println!("{round}: every Aniimo's art {:.0} ms, the named Aniimo's art {:.1} ms, {} card arts", searched.as_secs_f64() * 1000.0, t1.elapsed().as_secs_f64() * 1000.0, ic.cards.len());
        for (i, (r, n)) in all.iter().zip(&named).enumerate() {
            let Some(r) = r else { continue };
            println!("  card {i}: score {:.2}, margin {:.2}, by its name's art {:.2}", r.1, r.2, n.unwrap_or(-1.0));
            if r.1 >= SIM_CARD_SCORE && r.2 >= SIM_CARD_MARGIN {
                let n = n.expect("the winner has art");
                assert!((n - r.1).abs() < 1e-9, "card {i}: {n} by name, {} searched", r.1);
                assert!(n >= SIM_CARD_NAMED, "card {i}: {n} is under SIM_CARD_NAMED");
            }
        }
    }
}
