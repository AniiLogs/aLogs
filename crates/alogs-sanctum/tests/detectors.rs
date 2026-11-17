//! The party screen reader's text rules and region placement.

use alogs_sanctum::detectors::{self, Anchor, RegionF};
use alogs_sanctum::lang;
use std::collections::HashMap;

#[test]
fn english_goal_line() {
    assert_eq!(detectors::parse("Operation: Egg Heist Team Mode Nightmare").as_deref(), Some("Nightmare"));
    assert_eq!(detectors::parse("Operation: Egg He1st Team Mode\nDifficulty: Cha0s").as_deref(), Some("Chaos"));
    assert_eq!(detectors::parse("Egg Heist Team Mode Normal").as_deref(), Some("Normal"));
    assert_eq!(detectors::parse("Egg Heist Solo Hard"), None);
    assert_eq!(detectors::parse("Team Mode Hard"), None);
    assert_eq!(detectors::parse("Egg Heist Team Mode Hardcore"), None);
}

#[test]
fn other_languages_use_the_game_terms() {
    let mut de = HashMap::new();
    de.insert("EggHeistTeamMode".to_string(), vec!["Eierraub Teammodus".to_string()]);
    de.insert("Hard".to_string(), vec!["Schwer".to_string()]);
    de.insert("Chaos".to_string(), vec!["Chaos".to_string()]);
    let mut terms = HashMap::new();
    terms.insert("de_DE".to_string(), de);
    lang::set_terms(terms);
    assert_eq!(detectors::parse_lang("Operation: EIERRAUB  Team-modus  Schwer", "de_DE").as_deref(), Some("Hard"));
    assert_eq!(detectors::parse_lang("Operation: Egg Heist Team Mode Chaos", "de_DE").as_deref(), Some("Chaos"));
    assert_eq!(detectors::parse_lang("Irgendwas", "de_DE"), None);
    assert_eq!(detectors::norm("Tiếng Việt, ok!"), "tiengvietok");
}

#[test]
fn regions_follow_the_ui_box() {
    let r = RegionF { x: 740.0, y: 6.0, w: 400.0, h: 54.0 };
    assert_eq!(detectors::to_client(r, Anchor::TopCenter, 1920, 1080), (740, 6, 400, 54));
    assert_eq!(detectors::to_client(r, Anchor::TopCenter, 3840, 2160), (1480, 12, 800, 108));
    // 21:9: the UI box starts 440 px in at 3440 x 1440 (scale 4/3)
    let m = RegionF { x: 75.0, y: 32.0, w: 210.0, h: 210.0 };
    assert_eq!(detectors::to_client(m, Anchor::TopLeft, 3440, 1440), (540, 43, 280, 280));
    assert_eq!(detectors::to_client(RegionF { x: 1800.0, y: 0.0, w: 200.0, h: 50.0 }, Anchor::TopRight, 1920, 1080), (1800, 0, 120, 50));
}

#[test]
fn windows_ocr_reads_a_blank_picture() {
    let ocr = alogs_sanctum::ocr::OcrService::new(Some("en-US"));
    let mut m = alogs_sanctum::cv::Mat::new(120, 40, 3);
    m.data.fill(255);
    // no text (and no engine on machines without one): nothing read, no failure
    assert_eq!(ocr.read(&m).trim(), "");
    if ocr.available() {
        assert_ne!(ocr.language, "none");
    }
}
