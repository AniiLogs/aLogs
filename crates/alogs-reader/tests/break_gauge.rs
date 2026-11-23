//! The break gauge as `bar::measure_break` reads it, on bars drawn here at 3840x2160 with the open world bar's
//! geometry and colours (as the game shows them): the gauge read right of the lock icon, unknown under our own cover,
//! empty only with the track's start seen empty, the broken look and the lock icon told.

use alogs_pil::{Image, Mode};
use alogs_reader::bar::{self, Bar, BreakRead};

const YC: i64 = 194;
const SPAN: f64 = 1367.0;
const TL: i64 = 1243;
const TR: i64 = 2585;
const BG: [u8; 3] = [42, 30, 24];
const HP: [u8; 3] = [248, 105, 105];
const WHITE: [u8; 3] = [218, 218, 218];
const ORANGE: [u8; 3] = [248, 204, 151];
const TRACK: [u8; 3] = [56, 47, 39];
const GLOW: [u8; 3] = [106, 99, 63];
const YELLOW: [u8; 3] = [255, 230, 100];
const LOCK_WHITE: [u8; 3] = [253, 253, 253];
const LOCK_PINK: [u8; 3] = [226, 180, 177];
const COVER: [u8; 3] = [255, 0, 255];
/// The gauge line's rows at 4K (yc + 18 to yc + 25, as in the game's frame).
const GAUGE: (i64, i64) = (212, 219);

fn the_bar() -> Bar {
    Bar { yc: YC, left_bracket: 1227.0, span: SPAN, brackets_found: true, world: true, right_red: 0.0, track_left: TL, track_right: TR, thickness: 26, frame: (3840, 2160) }
}

fn rect(im: &mut Image, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 3]) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            im.putpixel(x as usize, y as usize, &c);
        }
    }
}

/// The bar with its HP fill and the gauge `left` full (`line`: its colour), on an empty track (`track`).
fn drawn(left: f64, line: [u8; 3], track: [u8; 3]) -> Image {
    let mut im = Image::new(Mode::Rgb, 3840, 400, &BG);
    rect(&mut im, TL, YC - 13, TR, YC + 12, HP);
    rect(&mut im, TL, GAUGE.0, TR, GAUGE.1, track);
    let end = TL + ((TR - TL) as f64 * left) as i64 - 1;
    if left > 0.0 {
        rect(&mut im, TL, GAUGE.0, end, GAUGE.1, line);
    }
    im
}

/// The lock icon over the track's start: its white lock body over and under the gauge's row, its glow after it.
fn lock(im: &mut Image) {
    rect(im, TL, GAUGE.0 - 2, TL + 16, GAUGE.1 + 9, LOCK_WHITE);
    rect(im, TL + 17, GAUGE.0, TL + 22, GAUGE.1, BG);
    rect(im, TL + 23, GAUGE.0, TL + 34, GAUGE.1, LOCK_PINK);
}

/// The broken look: the HP row's recent-damage piece in yellow (4% of the track).
fn broken_look(im: &mut Image) {
    let x = TL + (TR - TL) * 40 / 100;
    rect(im, x, YC - 13, x + (TR - TL) * 4 / 100, YC + 12, YELLOW);
}

fn read(im: &Image) -> BreakRead {
    bar::measure_break(im, &the_bar())
}

fn near(got: Option<f64>, want: f64) -> bool {
    got.is_some_and(|g| (g - want).abs() < 0.01)
}

#[test]
fn a_gauge_is_read_as_it_shows() {
    for left in [0.92, 0.42, 0.10] {
        let r = read(&drawn(left, WHITE, TRACK));
        assert!(near(r.gauge, left) && r.kind == Some("white") && !r.broken && !r.locked, "{left}: {r:?}");
    }
    // low, still draining: the line turns orange
    for left in [0.27, 0.04] {
        let r = read(&drawn(left, ORANGE, TRACK));
        assert!(near(r.gauge, left) && r.kind == Some("orange") && !r.broken, "{left}: {r:?}");
    }
}

/// The lock icon over the track's start: the gauge is read right of it, at its true value, and the icon is told.
#[test]
fn the_gauge_is_read_beside_the_lock_icon() {
    for (left, line) in [(0.35, WHITE), (0.27, ORANGE), (0.65, WHITE)] {
        let mut im = drawn(left, line, TRACK);
        lock(&mut im);
        let r = read(&im);
        assert!(near(r.gauge, left) && r.locked, "{left}: {r:?}");
    }
    // with the gauge really empty under the icon: the start is hidden, so it isn't "empty"; the icon still shows
    let mut im = drawn(0.0, WHITE, TRACK);
    lock(&mut im);
    let r = read(&im);
    assert!(r.gauge.is_none() && r.locked, "{r:?}");
    // no icon: none told
    assert!(!read(&drawn(0.35, WHITE, TRACK)).locked);
}

/// Our own cover over the gauge's right third: a gauge reaching under it is unknown; one ending before it reads true.
#[test]
fn under_our_cover_the_gauge_is_unknown() {
    let edge = TL + (TR - TL) * 67 / 100;
    for (left, want) in [(0.92, None), (0.70, None), (0.50, Some(0.50)), (0.10, Some(0.10))] {
        let mut im = drawn(left, WHITE, TRACK);
        rect(&mut im, edge, GAUGE.0 + 1, TR + 40, GAUGE.1 + 40, COVER);
        let r = read(&im);
        match want {
            None => assert!(r.gauge.is_none(), "{left}: {r:?}"),
            Some(w) => assert!(near(r.gauge, w), "{left}: {r:?}"),
        }
    }
}

/// Empty (no line) needs the broken look and the track's start seen empty: dark, or the broken state's glow; a flash
/// over it says nothing, and an empty track without the broken look is no reading (a bar with no gauge looks so).
#[test]
fn empty_needs_the_start_seen_empty() {
    let r = read(&drawn(0.0, WHITE, TRACK));
    assert_eq!((r.gauge, r.broken), (None, false), "{r:?}");
    let mut im = drawn(0.0, WHITE, TRACK);
    broken_look(&mut im);
    assert_eq!(read(&im).gauge, Some(0.0));
    let mut im = drawn(0.0, WHITE, GLOW);
    broken_look(&mut im);
    let r = read(&im);
    assert_eq!((r.gauge, r.broken, r.locked), (Some(0.0), true, false), "{r:?}");
    // a bright yellow flash where the gauge is: no reading
    let mut im = drawn(0.0, WHITE, [250, 235, 160]);
    broken_look(&mut im);
    let r = read(&im);
    assert!(r.gauge.is_none(), "{r:?}");
    // a nearly empty gauge: a short piece with the empty track after it
    let r = read(&drawn(0.012, ORANGE, TRACK));
    assert!(near(r.gauge, 0.012), "{r:?}");
}

/// The refill: a white line growing from the left while the broken look is still up.
#[test]
fn the_refill_line_with_the_broken_look() {
    let mut im = drawn(0.06, WHITE, GLOW);
    broken_look(&mut im);
    let r = read(&im);
    assert!(near(r.gauge, 0.06) && r.kind == Some("white") && r.broken, "{r:?}");
}
