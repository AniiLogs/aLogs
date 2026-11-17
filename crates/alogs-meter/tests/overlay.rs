//! The overlay's text: numbers, the break line, the end-screen notes and the stats (as the Python's docstrings show).

use alogs_meter::overlay::{break_text, end_note_battle_data, end_note_result, end_note_sim, fmt, hp_colour, stat_lines};
use alogs_pyval::{Value, dict};

#[test]
fn numbers_and_notes_read_as_before() {
    assert_eq!(fmt(1_234_567.0), "1.23M");
    assert_eq!(fmt(45_678.0), "45.7K");
    assert_eq!(fmt(9_999.0), "9,999");
    assert_eq!(hp_colour(1.0), [255, 176, 85]);
    assert_eq!(hp_colour(0.0), [248, 108, 108]);
    assert_eq!(end_note_result("result recorded: Omega Sherro - score 11232, 11 stars, 180s"), "Result saved\nOmega Sherro · 11,232 · 11 stars · 3:00");
    assert_eq!(end_note_result("something else"), "Result saved");
    assert_eq!(end_note_sim("Holo-Battle Sim recorded: Earth Training - Trial in 644 s (43 gizmos)"), "Holo-Battle Sim saved\nEarth Training - Trial · 10:44 · 43 gizmos");
    assert_eq!(end_note_battle_data("Battle Data screen saved: x - game team DPS 12,345 vs meter 12,000"), "Battle Data saved\nTeam DPS 12,345 · meter 12,000");
    let b = dict([("left", Value::Float(0.5)), ("rate", Value::Float(2.0)), ("breaks", Value::Int(1)), ("phase", Value::Str("drain".into()))]);
    assert_eq!(break_text(&b).unwrap().0, "BREAK 50% left · 2.0%/s · 1 break");
    let b = dict([("left", Value::Float(0.25)), ("size", Value::Float(4000.0)), ("rate", Value::Float(1.0)), ("kind", Value::Str("orange".into()))]);
    assert_eq!(break_text(&b).unwrap(), ("BREAK 1,000 / 4,000 · 40/s".to_string(), "#ffc58a"));
    assert!(break_text(&dict([("left", Value::None)])).is_none());
    let d = dict([("damage", Value::Int(0)), ("avg", Value::Int(0)), ("max_hp", Value::Int(2_500_000)), ("elapsed", Value::Int(80))]);
    let lines = stat_lines(&d);
    assert_eq!(lines[0].text.0, "— DPS");
    assert_eq!(lines[1].text.1.as_deref(), Some("waiting..."));
    assert_eq!(lines[3].text.1.as_deref(), Some("1:20"));
}
