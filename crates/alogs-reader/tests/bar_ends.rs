//! The boss bar's ends on drawn bars (1920x1080, the game's proportions): the brackets are told from bright
//! backgrounds, hit flashes, the fill's bright edge, the xN counter and the game's badges; the right end is the left
//! one's mirror; a bracket under our own cover is neither seen nor missing; the lock icon doesn't halve the bar; and a
//! fight's ends are held once seen the same, replaced at most once.

use alogs_pil::{Image, Mode};
use alogs_reader::bar::{self, Bar, COVER, EndsHold, HOLD_LOOKS, HoldNews, Side, TRACK, WORLD_TRACK_RIGHT};

const W: usize = 1920;
const H: usize = 1080;
const YC: i64 = 97;
const T: i64 = 13;
const FILL: [u8; 3] = [250, 107, 105];
const TRACK_DARK: [u8; 3] = [48, 27, 24];
const RIM: [u8; 3] = [15, 15, 18];
const WHITE: [u8; 3] = [245, 245, 245];
const DARK_BG: [u8; 3] = [20, 22, 30];
const SKY: [u8; 3] = [148, 215, 235];

/// The open world's bar at 1080p (half its measured 4K place: brackets at x 613 and 1307), and Interlink's (582, 1338).
const WORLD: (i64, i64) = (613, 1307);
const RUSH: (i64, i64) = (582, 1338);

fn rect(im: &mut Image, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 3]) {
    for y in y0.max(0)..y1.min(im.height as i64) {
        for x in x0.max(0)..x1.min(im.width as i64) {
            let at = (y as usize * im.width + x as usize) * 3;
            im.data[at..at + 3].copy_from_slice(&c);
        }
    }
}

/// The bracket's rows: from the bar's top past the break gauge below it, as in the game.
fn bracket_rows() -> (i64, i64) {
    (YC - T / 2, YC + (0.8 * T as f64).round() as i64 + 1)
}

/// A boss bar with its brackets (2 px wide; `l` and `r` their inner columns), `fill` of its track filled, on `bg`.
/// Interlink's has its xN box with "x1" at the right end.
fn frame(ends: (i64, i64), fill: f64, bg: [u8; 3]) -> Image {
    let (l, r) = ends;
    let world = ends == WORLD;
    let span = (r - l) as f64;
    let mut im = Image::new(Mode::Rgb, W, H, &bg);
    let (top, bot) = (YC - T / 2, YC + T / 2 + 1);
    rect(&mut im, l + 3, top - 2, r - 2, bot + 2, RIM);
    let tl = (l as f64 + TRACK.0 * span).round() as i64;
    let tr = (l as f64 + if world { WORLD_TRACK_RIGHT } else { TRACK.1 } * span).round() as i64;
    rect(&mut im, tl, top, tr, bot, TRACK_DARK);
    rect(&mut im, tl, top, tl + ((tr - tl) as f64 * fill) as i64, bot, FILL);
    if !world {
        // the counter "x1": an x and a 1 in white on the dark box
        let x0 = tr + 14;
        for k in 0..9 {
            rect(&mut im, x0 + k, top + 2 + k, x0 + k + 2, top + 3 + k, WHITE);
            rect(&mut im, x0 + 8 - k, top + 2 + k, x0 + 10 - k, top + 3 + k, WHITE);
        }
        rect(&mut im, x0 + 18, top + 1, x0 + 20, bot - 1, WHITE);
    }
    let (b0, b1) = bracket_rows();
    rect(&mut im, l - 1, b0, l + 1, b1, WHITE);
    rect(&mut im, r, b0, r + 2, b1, WHITE);
    im
}

/// The bar a fight would hold at these ends.
fn bar_at(ends: (i64, i64)) -> Bar {
    let (l, r) = (ends.0 as f64, ends.1 as f64);
    let span = r - l;
    let world = ends == WORLD;
    Bar { yc: YC, left_bracket: l, span, brackets_found: true, world, right_red: 0.0,
          track_left: (l + TRACK.0 * span).round() as i64, track_right: (l + if world { WORLD_TRACK_RIGHT } else { TRACK.1 } * span).round() as i64,
          thickness: T, frame: (W, H) }
}

fn ends_of(b: &Bar) -> (i64, i64) {
    (b.left_bracket.round() as i64, (b.left_bracket + b.span).round() as i64)
}

#[test]
fn true_brackets_are_found_on_both_bar_kinds() {
    for ends in [WORLD, RUSH] {
        let b = bar::find_bar(&frame(ends, 0.7, DARK_BG), false, 8).expect("a bar");
        assert_eq!(ends_of(&b), ends);
        assert!(b.brackets_found, "both brackets seen");
        assert_eq!((b.yc, b.thickness), (YC, T));
        assert_eq!(b.world, ends == WORLD);
        assert!(bar::verify_strict(&frame(ends, 0.7, DARK_BG), &b));
    }
}

/// A daytime sky behind the bar is bright on every column: once, every sky column passed as a bracket.
#[test]
fn a_bright_sky_is_no_bracket() {
    let im = frame(WORLD, 0.7, SKY);
    let b = bar::find_bar(&im, false, 8).expect("a bar");
    assert_eq!(ends_of(&b), WORLD, "the brackets, not the sky beside them");
    assert!(bar::verify_strict(&im, &b));
    // the same bar shifted into the sky does not verify
    let mut off = bar_at(WORLD);
    off.left_bracket -= 12.0;
    off.span += 24.0;
    assert_eq!(bar::bracket_sides(&im, &off), (Side::Fail, Side::Fail));
    assert!(!bar::verify_bar(&im, &off));
}

/// A hit's pale flash and the fill's bright edge stand near the right end; neither is it.
#[test]
fn a_hit_flash_and_the_fills_edge_are_not_the_right_end() {
    let mut im = frame(WORLD, 0.8, DARK_BG);
    let b = bar_at(WORLD);
    let edge = b.track_left + ((b.track_right - b.track_left) as f64 * 0.8) as i64;
    let (b0, b1) = bracket_rows();
    rect(&mut im, edge - 2, b0, edge, b1, WHITE); // the fill's bright cap, as tall as a bracket
    rect(&mut im, edge, YC - T / 2, edge + 60, YC + T / 2 + 1, [240, 240, 235]); // the pale flash block
    rect(&mut im, edge + 58, b0, edge + 60, b1, WHITE); // its bright right edge
    let found = bar::find_bar(&im, false, 8).expect("a bar");
    assert_eq!(ends_of(&found), WORLD);
    assert!(found.brackets_found);
}

/// Interlink's counter "x1" sits inside the bar near its right end: its "1" is no right bracket.
#[test]
fn the_counter_is_not_the_right_end() {
    let mut im = frame(RUSH, 0.5, DARK_BG);
    let (b0, b1) = bracket_rows();
    let one = RUSH.1 - 30; // a "1" drawn as tall as a bracket, 30 px in (60 px at 4K)
    rect(&mut im, one, b0, one + 2, b1, WHITE);
    let found = bar::find_bar(&im, false, 8).expect("a bar");
    assert_eq!(ends_of(&found), RUSH);
    assert!(!found.world);
}

/// The game's "250%" badge covers the right bracket: the right end is the left one's mirror, marked as not seen; the
/// locked bar's check fails on that side (the meter reads on at held ends, see the meter's tests).
#[test]
fn a_badge_over_the_right_bracket_leaves_the_mirror() {
    let mut im = frame(WORLD, 0.6, DARK_BG);
    rect(&mut im, WORLD.1 - 20, YC - T, WORLD.1 + 25, YC + T, [255, 170, 60]);
    let found = bar::find_bar(&im, false, 8).expect("a bar");
    assert_eq!(ends_of(&found), WORLD, "the mirror of the left bracket");
    assert!(!found.brackets_found, "the right bracket wasn't seen");
    assert!(!bar::centred(&found) || !found.brackets_found);
    assert_eq!(bar::bracket_sides(&im, &bar_at(WORLD)), (Side::Pass, Side::Fail));
    assert!(!bar::verify_bar(&im, &bar_at(WORLD)));
    // a held bar's sight needs both brackets: the badge frame is not one
    let mut hold = EndsHold::default();
    assert_eq!(hold.look(Some(&found), true), HoldNews::Nothing);
}

/// Our own label's cover (magenta) over the right bracket's lower rows: what shows still passes; all covered, the
/// side is unknown, not failed, and the bar still verifies on its other bracket (but is no sight of both).
#[test]
fn our_cover_is_neither_seen_nor_missing() {
    let b = bar_at(WORLD);
    let mut im = frame(WORLD, 0.7, DARK_BG);
    rect(&mut im, WORLD.1 - 200, YC + 3, WORLD.1 + 9, YC + 20, COVER);
    assert_eq!(bar::bracket_sides(&im, &b).1, Side::Pass, "the bracket's upper rows show");
    assert!(bar::verify_strict(&im, &b));
    rect(&mut im, WORLD.1 - 200, YC - 20, WORLD.1 + 9, YC + 20, COVER);
    assert_eq!(bar::bracket_sides(&im, &b), (Side::Pass, Side::Unknown));
    assert!(bar::verify_bar(&im, &b), "a covered bracket is no failed one");
    assert!(!bar::verify_strict(&im, &b));
}

/// The break state's lock icon covers the lower half of the bar's left end: the bar is still read its full
/// thickness, at its own row.
#[test]
fn the_lock_icon_does_not_halve_the_bar() {
    let mut im = frame(WORLD, 0.7, DARK_BG);
    let tl = bar_at(WORLD).track_left;
    rect(&mut im, tl - 2, YC, tl + 40, YC + T, [92, 92, 104]);
    let found = bar::find_bar(&im, false, 8).expect("a bar");
    assert_eq!((found.yc, found.thickness), (YC, T));
    assert_eq!(ends_of(&found), WORLD);
}

/// Both bar kinds are centred: the right bracket is taken only within MIRROR_PX of the left one's mirror.
#[test]
fn a_right_bracket_off_the_mirror_is_not_taken() {
    // a bar whose right bracket stands 6 px further out than the mirror (as no game bar does)
    let mut im = frame(WORLD, 0.7, DARK_BG);
    let (b0, b1) = bracket_rows();
    rect(&mut im, WORLD.1, b0, WORLD.1 + 2, b1, RIM);
    rect(&mut im, WORLD.1 + 6, b0, WORLD.1 + 8, b1, WHITE);
    let found = bar::find_bar(&im, false, 8).expect("a bar");
    assert_eq!(ends_of(&found), WORLD, "the mirror, not the line 6 px off it");
    assert!(!found.brackets_found);
}

/// The held ends: seen the same (both brackets, centred) on HOLD_LOOKS looks; replaced once, early, by a pair seen as
/// often on looks where the held one wasn't seen; never again.
#[test]
fn held_ends_are_replaced_once() {
    let truth = bar_at(WORLD);
    // a first find too short at the right end (and still centred: a pair the brackets "passed" for)
    let mut short = truth.clone();
    short.left_bracket += 10.0;
    short.span -= 20.0;
    let mut far = truth.clone();
    far.left_bracket -= 30.0;
    far.span += 60.0;
    let mut h = EndsHold::default();
    for i in 1..HOLD_LOOKS {
        assert_eq!(h.look(Some(&short), true), HoldNews::Nothing, "look {i}");
    }
    assert_eq!(h.look(Some(&short), true), HoldNews::Held);
    assert!(bar::same_ends(h.held.as_ref().unwrap(), &short));
    // the true ends, on looks where the short ones aren't seen: replaced after as many looks
    assert_eq!(h.look(Some(&truth), true), HoldNews::Nothing);
    assert_eq!(h.look(None, true), HoldNews::Nothing, "a look without a sight doesn't count either way");
    assert_eq!(h.look(Some(&truth), true), HoldNews::Nothing);
    assert_eq!(h.look(Some(&truth), true), HoldNews::Replaced);
    assert!(bar::same_ends(h.held.as_ref().unwrap(), &truth));
    assert!(h.replaced);
    // the held ends are seen again: counted
    let seen = h.seen;
    assert_eq!(h.look(Some(&truth), true), HoldNews::Nothing);
    assert_eq!(h.seen, seen + 1);
    // another pair, seen any number of times, replaces nothing now
    for _ in 0..10 {
        assert_eq!(h.look(Some(&far), true), HoldNews::Nothing);
    }
    assert!(bar::same_ends(h.held.as_ref().unwrap(), &truth));
}

/// A rival pair must be seen HOLD_LOOKS times in a row of looks that don't see the held ends; after the early part of
/// the fight it isn't counted at all; a pair whose right bracket wasn't seen, or that isn't centred, never counts.
#[test]
fn held_ends_stand_against_a_few_other_sights() {
    let truth = bar_at(WORLD);
    let mut off = truth.clone();
    off.left_bracket += 30.0;
    off.span -= 60.0;
    let mut h = EndsHold::default();
    for _ in 0..HOLD_LOOKS {
        h.look(Some(&truth), true);
    }
    assert!(h.held.is_some());
    for _ in 0..5 {
        h.look(Some(&off), true);
        h.look(Some(&off), true);
        h.look(Some(&truth), true); // the held ends seen in between: the count starts over
    }
    assert!(!h.replaced);
    for _ in 0..10 {
        assert_eq!(h.look(Some(&off), false), HoldNews::Nothing, "mid-fight");
    }
    assert!(bar::same_ends(h.held.as_ref().unwrap(), &truth));
    let mut h = EndsHold::default();
    let mirrored = Bar { brackets_found: false, ..truth.clone() };
    let mut skew = truth.clone();
    skew.span -= 20.0;
    for _ in 0..10 {
        h.look(Some(&mirrored), true);
        h.look(Some(&skew), true);
    }
    assert!(h.held.is_none(), "neither a mirrored end nor an uncentred pair holds");
}

/// Looked for again from a locked bar's track, in a picture of the top part only (no whole frame).
#[test]
fn ends_are_found_again_in_the_top_part() {
    let im = frame(RUSH, 0.4, SKY);
    let top = im.crop_i(0, 0, W as i64, 160);
    let near = bar_at(RUSH);
    let again = bar::find_at(&top, &near).expect("found at the locked place");
    assert_eq!(ends_of(&again), RUSH);
    assert_eq!(again.frame, (W, H), "the frame size is the locked bar's");
    assert!(again.brackets_found);
}

/// In an Interlink fight the track is always Interlink's, whatever the right end showed (the badge left a bar "world").
#[test]
fn the_interlink_track_whatever_the_right_end_showed() {
    let mut b = bar_at(RUSH);
    b.world = true;
    b.right_red = 0.3;
    b.track_right = (b.left_bracket + WORLD_TRACK_RIGHT * b.span).round() as i64;
    assert!(bar::as_bossrush_bar(&b).world, "outside a fight the right end's colour still decides");
    let r = bar::interlink_track(&b);
    assert!(!r.world);
    assert_eq!(r.track_right, bar_at(RUSH).track_right);
}
