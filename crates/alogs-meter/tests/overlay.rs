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

/// A notice is sized and placed by the game's place on screen. Only a fight records that place, so before the first
/// fight since the meter started it comes from the game window's place and its last whole picture, not from a 1080p
/// screen at the top left (19 Nov: half-size notices on a 3840x2160 screen, from the battle prep screen).
#[test]
fn a_notice_before_the_first_fight_is_sized_by_the_game_window() {
    use alogs_meter::overlay::{game_screen, notice_scale};
    // no fight yet, the game fills a 3840x2160 screen: its own size, twice the 1080p notice
    let s = game_screen(None, (0, 0), (3840, 2160));
    assert_eq!(s, (0, 0, 3840, 2160));
    assert_eq!(notice_scale(s), 2.0);
    // a window on a second screen: its place too
    assert_eq!(game_screen(None, (3840, 120), (2560, 1440)), (3840, 120, 2560, 1440));
    // a fight's place wins while there is one
    assert_eq!(game_screen(Some((10, 20, 1280, 720)), (0, 0), (3840, 2160)), (10, 20, 1280, 720));
    // nothing seen of the game yet: the 1080p screen, as before
    let s = game_screen(None, (0, 0), (0, 0));
    assert_eq!(s, (0, 0, 1920, 1080));
    assert_eq!(notice_scale(s), 1.0);
    // wider than 16:9: by the 16:9 part
    assert_eq!(notice_scale((0, 0, 3440, 1440)), 1440.0 / 1080.0);
}

/// The HP % stands in front of the HP numbers, on their line: a gap left of where they start, wherever they are.
/// Before they have been drawn for a bar it stands where they usually start, and leaves room for what comes later.
#[test]
fn the_hp_percent_stands_in_front_of_the_hp_numbers() {
    use alogs_meter::overlay::{PCT_GAP, PCT_RIGHT, pct_anchor};
    let bar = (1226.0, 194.0, 1388.0);
    // the numbers start at x 2300 on the row at y 224.5: the % ends a gap before that, on that row
    assert_eq!(pct_anchor(bar, Some((2300.0, 224.5)), 0.0), (2300.0 - PCT_GAP * 1388.0, 224.5));
    // the numbers moved by hand: the % goes with them
    assert_eq!(pct_anchor(bar, Some((1800.0, 500.0)), 0.0), (1800.0 - PCT_GAP * 1388.0, 500.0));
    // not drawn yet for this bar: where they usually start, under the bar
    let (right, top) = pct_anchor(bar, None, 0.0);
    assert_eq!(right, 1226.0 + PCT_RIGHT * 1388.0);
    assert!(top > 194.0 + 13.0, "under the bar, not on it: {top}");
    // room kept for the dot that comes with the numbers
    assert_eq!(pct_anchor(bar, None, 18.0).0, right - 18.0);
}

/// By hand: the HP line as the overlay draws it (the game's own font, the labels' shadow, their places), put on a real
/// picture of the bar with no labels in it, to look at:
/// `ALOGS_GAMEFONT=<aLogs data>\gamefont ALOGS_BAR_SHOT=<a 16:9 frame, png> ALOGS_OUT=<png to write> cargo test -p alogs-meter --test overlay -- --ignored`
#[test]
#[ignore = "needs the game's font and a real picture"]
fn the_hp_line_on_a_real_bar() {
    use alogs_meter::overlay::{pct_anchor, with_shadow};
    use alogs_pil::{Image, Mode};
    use alogs_reader::gamefont::{FONT_NAME, GameFont};
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
    let font = GameFont::load(std::path::Path::new(&var("ALOGS_GAMEFONT")), None, FONT_NAME).expect("the game's font");
    let mut im = Image::open_png(std::path::Path::new(&var("ALOGS_BAR_SHOT"))).expect("the picture").convert(Mode::Rgba);
    let k = im.width as f64 / 3840.0;
    let bar = (1226.0 * k, 194.0 * k, 1388.0 * k);
    let (bx, by, span) = bar;
    let outline = [20, 26, 44];
    let put = |im: &mut Image, text: &str, fs: f64, fill: [u8; 3], bold: bool, lead: Option<(usize, [u8; 3])>, right: f64, top: f64| -> f64 {
        let px = (span * fs).round_ties_even().max(11.0);
        let (plain, _) = font.render(text, px, fill, outline, false, bold, true, lead);
        let (img, edge) = with_shadow(&plain, px);
        let left = right - plain.width as f64;
        im.alpha_composite(&img, (left - edge as f64) as i64, (top - edge as f64) as i64);
        left
    };
    // the HP numbers and the break line where they have always been, the % in front of the numbers
    let total_top = by + 0.022 * span;
    let total_left = put(&mut im, "HP 281.9K / 397.0K", 0.022, [255, 255, 255], false, Some((2, hp_colour(0.71))), bx + span, total_top);
    put(&mut im, "BREAK 10.5K / 43.1K \u{b7} 4,916/s", 0.020, [255, 197, 138], false, Some((5, [255, 197, 138])), bx + span, by + 0.049 * span);
    let (right, top) = pct_anchor(bar, Some((total_left, total_top)), 0.0);
    put(&mut im, "71.0% \u{b7}", 0.022, [255, 255, 255], true, None, right, top);
    let crop = im.crop_i((1060.0 * k) as i64, (70.0 * k) as i64, (2780.0 * k) as i64, (300.0 * k) as i64);
    crop.save_png(std::path::Path::new(&var("ALOGS_OUT"))).expect("the picture written");
}
