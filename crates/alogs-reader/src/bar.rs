//! Finding and measuring the boss bar. Port of the bar half of aLogs B16 `bossbar/reader.py`.
//!
//! Resolution-independent: the white [ ] brackets at the ends of the boss bar are the ruler. Every position is a
//! fraction of the bracket span, measured once on a 2000x1125 screenshot (span 789 px).

use alogs_pil::ops::{darker, lighter};
use alogs_pil::{Filter, Image, Mode};
use regex::bytes::Regex;
use std::sync::Mutex;

pub const REF_SPAN: f64 = 789.0;
pub const TRACK: (f64, f64) = (9.0 / 789.0, 716.0 / 789.0);
pub const WORLD_TRACK_RIGHT: f64 = 784.0 / 789.0;
pub const TITLE_BOX: (f64, f64, f64, f64) = (-30.0 / 789.0, -52.0 / 789.0, 819.0 / 789.0, -8.0 / 789.0);
pub const XN_BOX: (f64, f64, f64, f64) = (722.0 / 789.0, -12.0 / 789.0, 783.0 / 789.0, 14.0 / 789.0);
pub const SPAN_PER_THICKNESS: f64 = 789.0 / 12.0;
pub const COVER: [u8; 3] = [255, 0, 255];

/// What the Dev counters saw (`DEV`): how often the fast fill walk answered, how often the old one did.
pub static DEV: Mutex<(u64, u64)> = Mutex::new((0, 0));
/// Why the last `find_bar` found nothing (`LAST_FAIL`).
pub static LAST_FAIL: Mutex<String> = Mutex::new(String::new());

type Px = [u8; 3];

pub fn is_fill(p: Px) -> bool {
    p[0] >= 220 && (90..=195).contains(&p[1]) && p[2] <= 125
}
pub fn is_ghost(p: Px) -> bool {
    (165..=215).contains(&p[0]) && (55..=105).contains(&p[1]) && (55..=100).contains(&p[2])
}
pub fn is_flash(p: Px) -> bool {
    p.iter().min().copied().unwrap() >= 235
}
pub fn is_white(p: Px) -> bool {
    p.iter().min().copied().unwrap() >= 190
}
pub fn barlike(p: Px) -> bool {
    is_fill(p) || is_ghost(p) || is_flash(p)
}
pub fn colored(p: Px) -> bool {
    is_fill(p) || is_ghost(p)
}
pub fn covered(p: Px) -> bool {
    p[0] >= 250 && p[1] <= 5 && p[2] >= 250
}

/// `px[x, y]` of an RGB picture: negative indexes count from the end (as PixelAccess does); `None` outside.
pub fn px_at(im: &Image, x: i64, y: i64) -> Option<Px> {
    let (w, h) = (im.width as i64, im.height as i64);
    let x = if x < 0 { x + w } else { x };
    let y = if y < 0 { y + h } else { y };
    if x < 0 || y < 0 || x >= w || y >= h {
        return None;
    }
    let p = im.px(x as usize, y as usize);
    Some([p[0], p[1], p[2]])
}
/// `px[x, y]`, a pixel the Python never reads outside the picture (clamped here rather than raising).
pub fn px(im: &Image, x: i64, y: i64) -> Px {
    px_at(im, x, y).unwrap_or_else(|| {
        let p = im.px(x.clamp(0, im.width as i64 - 1) as usize, y.clamp(0, im.height as i64 - 1) as usize);
        [p[0], p[1], p[2]]
    })
}

fn pyround(v: f64) -> i64 {
    v.round_ties_even() as i64
}

/// The boss bar where it was found: geometry in the picture's pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub yc: i64,
    pub left_bracket: f64,
    pub span: f64,
    pub brackets_found: bool,
    pub world: bool,
    pub right_red: f64,
    pub track_left: i64,
    pub track_right: i64,
    pub thickness: i64,
    pub frame: (usize, usize),
}

/// The bracket span the HP labels are placed by (a read much shorter than its left bracket's mirror is that mirror).
pub fn label_span(left: f64, span: f64, width: f64) -> f64 {
    let mirror = width - 2.0 * left;
    if mirror <= 0.0 || span >= mirror * 0.99 || mirror > span * 1.6 {
        return span;
    }
    mirror
}

/// What one column showed of a bracket: the bracket, something else, or too little of it to say (most of it under
/// the COVER of our own windows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Pass,
    Fail,
    Unknown,
}

/// The bracket's own rows: from the bar's top row down to this share of its thickness below its centre row. In the
/// game the bracket runs from the bar's top past the break gauge below it (43 rows at 4K for a bar 25 to 28 thick,
/// rows 181 to 223 for a bar centred on row 194, measured 22 Nov); on the drawn test frames it reaches 0.7 to 0.8 of
/// the thickness above and below the centre. These rows are bracket in both.
const BRACKET_REACH: f64 = 0.7;
/// A bracket pixel differs this much (the channels' differences added up) from some pixel within a few px on that
/// side: a bright background (a daytime sky is (148, 215, 235)) is as bright beside a column as on it, and a pale sky
/// beside the white bracket still differs by 59 (195, 242, 251 against 245, 245, 245).
const BRACKET_CONTRAST: i32 = 45;
/// ...and is brighter than it by this much (the channels added up): the soft edge between a pale sky and the bar's
/// dark rim differs from both, but is darker than the sky.
const BRACKET_BRIGHTER: i32 = 30;
/// A right bracket is taken this close to the left one's mirror about the frame's centre line, px. Both bar kinds are
/// centred: at 4K the true ends are 0 to 1 px from it, on the drawn test frames of every size 0 to 2 px.
pub const MIRROR_PX: f64 = 3.0;
/// The bar is centred on the frame: its ends' midpoint is this close to the centre line, px.
pub const CENTRED_PX: f64 = 2.0;

/// Is column x the bar's bracket? Only the bracket's own rows count; a pixel counts when it is bright (its darkest
/// channel 140 or more) and stands out from some pixel within `off` px on its left and on its right (the line is a
/// few px wide): differs from it and is brighter. On both sides: strong; on one: where the bracket's tabs reach in
/// towards the bar. Most rows must count and half must be strong. Covered pixels say nothing either way.
fn bracket_side(im: &Image, x: i64, yc: i64, thick: i64, h: i64) -> Side {
    let reach = ((BRACKET_REACH * thick as f64).round() as i64).max(3);
    let off = (thick / 5).max(3);
    let (mut known, mut cover, mut line, mut strong) = (0, 0, 0, 0);
    // (true too when there's nothing to compare with on that side: every pixel covered or outside the picture)
    let side = |p: Px, dir: i64, y: i64| {
        let mut any = false;
        for k in 1..=off {
            if let Some(q) = px_at(im, x + dir * k, y).filter(|q| !covered(*q)) {
                if sum3(p) as i32 - sum3(q) as i32 >= BRACKET_BRIGHTER && diff(p, q) >= BRACKET_CONTRAST {
                    return true;
                }
                any = true;
            }
        }
        !any
    };
    for y in (yc - thick / 2).max(0)..=(yc + reach).min(h - 1) {
        let p = px(im, x, y);
        if covered(p) {
            cover += 1;
            continue;
        }
        known += 1;
        if *p.iter().min().unwrap() < 140 {
            continue;
        }
        let (l, r) = (side(p, -1, y), side(p, 1, y));
        if l || r {
            line += 1;
        }
        if l && r {
            strong += 1;
        }
    }
    if known * 5 < (known + cover) * 2 {
        return Side::Unknown;
    }
    if line * 5 >= known * 4 && strong * 2 >= known { Side::Pass } else { Side::Fail }
}

fn is_bracket(im: &Image, x: i64, yc: i64, thick: i64, h: i64) -> bool {
    bracket_side(im, x, yc, thick, h) == Side::Pass
}

/// How the bracket near x0 (within 2 px) looks: Pass if any column passes, else Unknown if any is covered, else Fail.
fn side_near(im: &Image, x0: i64, yc: i64, thick: i64) -> Side {
    let (w, h) = (im.width as i64, im.height as i64);
    let mut any_unknown = false;
    for x in (x0 - 2).max(0)..(x0 + 3).min(w) {
        match bracket_side(im, x, yc, thick, h) {
            Side::Pass => return Side::Pass,
            Side::Unknown => any_unknown = true,
            Side::Fail => {}
        }
    }
    if any_unknown { Side::Unknown } else { Side::Fail }
}

fn longest_run(im: &Image, y: i64, x0: i64, x1: i64, test: fn(Px) -> bool) -> (i64, i64) {
    let (mut best, mut run, mut start) = ((0, 0), 0, 0);
    for x in x0..x1 {
        if test(px(im, x, y)) {
            if run == 0 {
                start = x;
            }
            run += 1;
            if run > best.0 {
                best = (run, start);
            }
        } else {
            run = 0;
        }
    }
    best
}

fn band(lo: u8, hi: u8) -> impl Fn(u8) -> f64 {
    move |v| if lo <= v && v <= hi { 255.0 } else { 0.0 }
}

fn rgb3(im: &Image) -> (Image, Image, Image) {
    let s = im.convert(Mode::Rgb).split();
    let mut it = s.into_iter();
    (it.next().unwrap(), it.next().unwrap(), it.next().unwrap())
}

/// The fill and ghost class maps of a picture (255 where the class holds).
fn fill_ghost(im: &Image) -> (Image, Image, (Image, Image, Image)) {
    let (r, g, b) = rgb3(im);
    let fill = darker(&darker(&r.point(band(220, 255)), &g.point(band(90, 195))), &b.point(band(0, 125)));
    let ghost = darker(&darker(&r.point(band(165, 215)), &g.point(band(55, 105))), &b.point(band(55, 100)));
    (fill, ghost, (r, g, b))
}

/// `_colored_run`: the first longest run of fill/ghost pixels on row y between x0 and x1.
fn colored_run(im: &Image, y: i64, x0: i64, x1: i64) -> (i64, i64) {
    if x1 <= x0 {
        return (0, 0);
    }
    let (fill, ghost, _) = fill_ghost(&im.crop_i(x0, y, x1, y + 1));
    let both = lighter(&fill, &ghost);
    let mut best = (0, 0);
    let mut i = 0;
    let d = &both.data;
    while i < d.len() {
        if d[i] == 255 {
            let s = i;
            while i < d.len() && d[i] == 255 {
                i += 1;
            }
            if (i - s) as i64 > best.0 {
                best = ((i - s) as i64, x0 + s as i64);
            }
        } else {
            i += 1;
        }
    }
    best
}

/// `_band_runs`: `(n, y, start)` of every row whose longest fill/ghost run (flash gaps under `gap`) is 4 px or more.
fn band_runs(bandim: &Image, gap: i64) -> Vec<(i64, i64, i64)> {
    let (fill, ghost, (r, g, b)) = fill_ghost(bandim);
    let flash = darker(&darker(&r.point(band(235, 255)), &g.point(band(235, 255))), &b.point(band(235, 255)));
    let colour = lighter(&fill, &ghost);
    if colour.getbbox().is_none() {
        return vec![];
    }
    let mut classes = Image::new(Mode::L, bandim.width, bandim.height, &[b'.']);
    classes.paste_mask(&Image::new(Mode::L, bandim.width, bandim.height, &[b'f']), 0, 0, Some(&flash));
    classes.paste_mask(&Image::new(Mode::L, bandim.width, bandim.height, &[b'c']), 0, 0, Some(&colour));
    let w = bandim.width;
    let run = Regex::new(&format!(r"c(?:f{{0,{}}}c)*", (gap - 1).max(0))).unwrap();
    let mut out = Vec::new();
    for y in 0..bandim.height {
        let row = &classes.data[y * w..(y + 1) * w];
        if row.iter().filter(|&&c| c == b'c').count() < 2 {
            continue;
        }
        let mut best = (0i64, 0i64);
        for m in run.find_iter(row) {
            if (m.end() - m.start()) as i64 > best.0 {
                best = ((m.end() - m.start()) as i64, m.start() as i64);
            }
        }
        if best.0 >= 4 {
            out.push((best.0, y as i64, best.1));
        }
    }
    out
}

/// Locate the boss bar: the best few red/orange rows, each checked for the bar's two white brackets.
pub fn find_bar(im: &Image, cropped: bool, tries: usize) -> Option<Bar> {
    let (w, h) = (im.width as i64, im.height as i64);
    let (y0, y1) = if cropped { (0, h) } else { (0, (h as f64 * 0.22) as i64) };
    let (x0, x1) = if cropped { (0, w) } else { ((w as f64 * 0.08) as i64, (w as f64 * 0.92) as i64) };
    let f = (w as f64 / 960.0).max(1.0);
    let d = pyround(f).max(1);
    let bw = pyround((x1 - x0) as f64 / f).max(1) as usize;
    let bh = pyround((y1 - y0) as f64 / f).max(1) as usize;
    let bandim = im.crop_i(x0, y0, x1, y1).resize(bw, bh, Some(Filter::Nearest));
    let mut runs = band_runs(&bandim, pyround(24.0 / f).max(2));
    if runs.is_empty() {
        *LAST_FAIL.lock().unwrap() = "no red/orange run near the top".into();
        return None;
    }
    runs.sort_by(|a, b| b.cmp(a));
    let (mut tried, mut reasons): (Vec<i64>, Vec<String>) = (vec![], vec![]);
    for (_, by, bs) in runs {
        if tried.len() >= tries {
            break;
        }
        if tried.iter().any(|t| (by - t).abs() <= 2) {
            continue;
        }
        tried.push(by);
        match check_candidate(im, x0 + pyround(bs as f64 * f), y0 + pyround(by as f64 * f), d, cropped, None, true, true) {
            Ok(bar) => {
                LAST_FAIL.lock().unwrap().clear();
                return Some(bar);
            }
            Err(r) => reasons.push(r),
        }
    }
    let mut seen = Vec::new();
    for r in reasons {
        if !seen.contains(&r) {
            seen.push(r);
        }
    }
    *LAST_FAIL.lock().unwrap() = if seen.is_empty() { "no candidate".into() } else { seen.join("; ") };
    None
}

/// A remembered bar spot (`bar_spots` in the meter): where a bar was for one window size.
#[derive(Clone, Debug)]
pub struct Spot {
    pub frame: (usize, usize),
    pub yc: f64,
    pub thickness: f64,
    pub left_bracket: f64,
    pub span: f64,
    pub track_left: f64,
}

/// The boss bar where it was last seen: the same checks as `find_bar` on that one spot.
pub fn bar_at(im: &Image, spots: &[Spot]) -> Option<Bar> {
    let (w, h) = (im.width as i64, im.height as i64);
    let d = pyround(w as f64 / 960.0).max(1);
    for spot in spots {
        if spot.frame != im.size() || !spot_centred(spot) {
            // (a place that isn't centred is a wrong find: the bar always is)
            continue;
        }
        let (yc, t) = (spot.yc as i64, spot.thickness as i64);
        let lb = pyround(spot.left_bracket);
        let rb = pyround(spot.left_bracket + spot.span);
        let at = |x0: i64| ((x0 - 2).max(0)..(x0 + 3).min(w)).find(|&x| is_bracket(im, x, yc, t, h));
        if !(0 <= yc && yc < h && rb < w) {
            continue;
        }
        let Some(lbx) = at(lb) else { continue };
        let Some(rbx) = at(rb) else { continue };
        for dx in [2, 8, 20] {
            if let Ok(bar) = check_candidate(im, spot.track_left as i64 + dx, spot.yc as i64, d, false, Some((lbx, rbx)), false, true) {
                return Some(bar);
            }
        }
    }
    None
}

/// Is a remembered place centred on its frame (as the bar always is)?
pub fn spot_centred(s: &Spot) -> bool {
    (s.left_bracket + s.span / 2.0 - s.frame.0 as f64 / 2.0).abs() <= CENTRED_PX
}

/// Is this bar centred on its frame (as the true bar always is)?
pub fn centred(b: &Bar) -> bool {
    (b.left_bracket + b.span / 2.0 - b.frame.0 as f64 / 2.0).abs() <= CENTRED_PX
}

/// The bar's top and bottom rows in column `col` around row `yg` (the fill or ghost colour, unbroken).
fn bar_rows(im: &Image, col: i64, yg: i64, h: i64) -> (i64, i64) {
    let mut top = yg;
    while top > 0 && colored(px(im, col, top - 1)) {
        top -= 1;
    }
    let mut bot = yg;
    while bot < h - 1 && colored(px(im, col, bot + 1)) {
        bot += 1;
    }
    (top, bot)
}

/// Refine one candidate run at full resolution and check it is the boss bar. `thick_check`: its thickness is checked
/// against the picture's height (not in a picture of the top part only).
#[allow(clippy::too_many_arguments)]
fn check_candidate(im: &Image, xg: i64, yg: i64, d: i64, cropped: bool, brackets: Option<(i64, i64)>, fast: bool, thick_check: bool) -> Result<Bar, String> {
    let (w, h) = (im.width as i64, im.height as i64);
    let col = (w - 1).min(xg + 4 * d);
    let mut yg = yg;
    if !colored(px(im, col, yg)) {
        let near: Vec<i64> = ((yg - d).max(0)..(h).min(yg + d + 1)).filter(|&y| colored(px(im, col, y))).collect();
        if near.is_empty() {
            return Err("run vanished at full resolution".into());
        }
        yg = near[0];
    }
    let (mut top, mut bot) = bar_rows(im, col, yg, h);
    // The thickness is the most of a few columns along the fill: the break state's lock icon covers the lower half
    // of the bar's left end, and from one column there the bar read 13 to 16 px thick at 4K instead of 25 to 28
    // and its row 4 to 6 px high (92 of 526 finds in fights, 17-22 Nov).
    for k in 1..=6 {
        let c = col + k * 6 * d;
        if c >= w || !colored(px(im, c, yg)) {
            break;
        }
        let (t2, b2) = bar_rows(im, c, yg, h);
        if b2 - t2 > bot - top {
            (top, bot) = (t2, b2);
        }
    }
    let yc = (top + bot).div_euclid(2);
    let thick = (bot - top + 1).max(3);
    if !cropped && thick_check && thick as f64 > h as f64 * 0.03 {
        return Err(format!("too thick ({thick}px)"));
    }
    let (mut lb, mut rb): (Option<f64>, Option<f64>) = (None, None);
    let mut mirrored = false;
    let start;
    let est = SPAN_PER_THICKNESS * thick as f64;
    if let Some((l, r)) = brackets {
        lb = Some(l as f64);
        rb = Some(r as f64);
        start = xg;
    } else {
        let (_, s) = if fast { colored_run(im, yc, (xg - 4 * d).max(0), w) } else { longest_run(im, yc, (xg - 4 * d).max(0), w, colored) };
        start = s;
        let bracket = |x: i64| is_bracket(im, x, yc, thick, h);
        let mut x = start - 1;
        while x > (start - 4 * thick).max(0) - 1 {
            if bracket(x) {
                lb = Some(x as f64);
                break;
            }
            x -= 1;
        }
        if let Some(l) = lb {
            if cropped {
                // (a crop isn't centred on the screen: the right bracket nearest the mirror, from further off)
                let u = (w as f64).min(h as f64 * 16.0 / 9.0);
                let hi = (w - 1).min((w as f64 - l + 0.06 * u) as i64);
                let lo = ((l + 0.3 * u) as i64).max((w as f64 - l - 0.06 * u) as i64);
                rb = nearest_line(&bracket, hi, lo, l, w);
            } else {
                // The bar is centred: its right bracket is the left one's mirror. Only a bracket within MIRROR_PX of
                // it is taken; anything further off (the fill's bright edge, a hit's pale flash, the "1" of the
                // counter "x1", the counter box's edge, icons, sky: all taken within 230 px before 22 Nov) is not.
                // When none shows there (the game's "250%" badge, an icon, a hit over it) the mirror itself is the end.
                let m = w as f64 - l;
                let (hi, lo) = ((w - 1).min((m + MIRROR_PX) as i64), (m - MIRROR_PX).ceil() as i64 - 1);
                rb = nearest_line(&bracket, hi, lo, l, w);
                if rb.is_none() && m < w as f64 {
                    rb = Some(m);
                    mirrored = true;
                }
            }
        }
    }
    let span;
    let lbv;
    match (lb, rb) {
        (Some(l), Some(r)) => {
            span = r - l;
            lbv = l;
        }
        _ if !cropped => return Err(if lb.is_none() { "no left bracket".into() } else { "no right bracket".into() }),
        _ => {
            span = est;
            lbv = start as f64 - TRACK.0 * span;
        }
    }
    let mut world = false;
    let right_red;
    if !cropped {
        if (lbv + span / 2.0 - w as f64 / 2.0).abs() > 0.03 * w as f64 {
            return Err("not centred on screen".into());
        }
        let (x_from, x_to) = ((lbv + TRACK.1 * span + 0.01 * span) as i64, (lbv + 0.99 * span) as i64);
        if x_to <= x_from {
            return Err("no room for an xN box".into());
        }
        let rows: Vec<i64> = (yc - thick.div_euclid(2)..yc + thick.div_euclid(2) + 1).filter(|&y| 0 <= y && y < h).collect();
        let n = (x_to - x_from) as f64;
        let mut digits = 0;
        for x in x_from..x_to {
            for &y in &rows {
                if is_white(px(im, x, y)) {
                    digits += 1;
                }
            }
        }
        let dark = (x_from..x_to)
            .filter(|&x| {
                let p = px(im, x, yc);
                *p.iter().max().unwrap() < 120 || is_white(p)
            })
            .count() as f64;
        let red = (x_from..x_to).filter(|&x| colored(px(im, x, yc))).count() as f64;
        if f64::from(digits) >= 0.03 * n * rows.len() as f64 && dark >= 0.6 * n {
            world = false;
        } else if dark + red >= 0.8 * n {
            world = true;
        } else {
            return Err("no xN box or open track at the right end".into());
        }
        right_red = red / n;
    } else {
        right_red = 0.0;
    }
    let right = if world { WORLD_TRACK_RIGHT } else { TRACK.1 };
    Ok(Bar {
        yc,
        left_bracket: lbv,
        span,
        brackets_found: rb.is_some() && !mirrored,
        world,
        right_red,
        track_left: pyround(lbv + TRACK.0 * span),
        track_right: pyround(lbv + right * span),
        thickness: thick,
        frame: im.size(),
    })
}

/// The bracket column nearest the mirror of `l` between `hi` and `lo` (scanned down from `hi`: the first of equals).
fn nearest_line(bracket: &dyn Fn(i64) -> bool, hi: i64, lo: i64, l: f64, w: i64) -> Option<f64> {
    let mut best: Option<(f64, i64)> = None;
    let mut x = hi;
    while x > lo {
        if bracket(x) {
            let k = (l + (x as f64 - l) / 2.0 - w as f64 / 2.0).abs();
            if best.is_none_or(|b| k < b.0) {
                best = Some((k, x));
            }
        }
        x -= 1;
    }
    best.map(|b| b.1 as f64)
}

/// The bar's ends looked for again where a bar is locked (`near`), in this picture (a whole frame or its top part):
/// the same checks as `find_bar` from that bar's track. The frame size is the locked bar's.
pub fn find_at(im: &Image, near: &Bar) -> Option<Bar> {
    if near.yc >= im.height as i64 || near.track_left >= im.width as i64 {
        return None;
    }
    let d = pyround(near.frame.0 as f64 / 960.0).max(1);
    let b = check_candidate(im, near.track_left + 2, near.yc, d, false, None, true, false).ok()?;
    Some(Bar { frame: near.frame, ..b })
}

/// In an Interlink fight the bar always has the Interlink track (its xN box at the right end), whatever the right end
/// showed: the game's orange "250%" badge sits where a world bar's track goes on, and a fight read on the world track
/// read its fill 6 to 7 points low (21 Nov).
pub fn interlink_track(bar: &Bar) -> Bar {
    Bar { world: false, track_right: pyround(bar.left_bracket + TRACK.1 * bar.span), ..bar.clone() }
}

/// In an Interlink fight a bar found without its xN box still has the Interlink track.
pub fn as_bossrush_bar(bar: &Bar) -> Bar {
    if !bar.world || bar.right_red > 0.2 {
        return bar.clone();
    }
    Bar { world: false, track_right: pyround(bar.left_bracket + TRACK.1 * bar.span), ..bar.clone() }
}

/// Out in the open world a bar is never Interlink's: the track runs on to the right bracket.
pub fn as_world_bar(bar: &Bar) -> Bar {
    if bar.world {
        return bar.clone();
    }
    Bar { world: true, track_right: pyround(bar.left_bracket + WORLD_TRACK_RIGHT * bar.span), ..bar.clone() }
}

/// Fraction of the track still filled with live HP, and the flash and ghost pixel counts.
pub fn measure_fill(im: &Image, bar: &Bar) -> (f64, i64, i64) {
    let w = im.width as i64;
    let yc = bar.yc;
    let q = (bar.thickness / 4).max(1);
    let rows = [yc - q, yc, yc + q];
    let (left, right) = (bar.track_left, bar.track_right.min(w));
    let (mut last, mut flash, mut ghost) = (left - 1, 0, 0);
    for x in left..right {
        let v: Vec<Px> = rows.iter().map(|&y| px(im, x, y)).collect();
        let count = |f: fn(Px) -> bool| v.iter().filter(|p| f(**p)).count();
        if count(is_fill) >= 2 {
            last = x;
        } else if count(is_flash) >= 2 {
            flash += 1;
        } else if count(is_ghost) >= 2 {
            ghost += 1;
        }
    }
    (((last - left + 1) as f64 / (right - left) as f64).clamp(0.0, 1.0), flash, ghost)
}

fn sum3(p: Px) -> u32 {
    u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])
}

/// The median of three pixels by their sum (ties in row order: Python's stable sort).
fn median3(a: Px, b: Px, c: Px) -> Px {
    let mut v = [a, b, c];
    v.sort_by_key(|p| sum3(*p));
    v[1]
}

fn diff(a: Px, b: Px) -> i32 {
    (0..3).map(|i| (i32::from(a[i]) - i32::from(b[i])).abs()).sum()
}

/// A mark drawn on the bar is at most this many bar thicknesses wide: the bar's colour must go on within that.
const MARK_REACH: i64 = 2;
/// ...for this many pixels in a row.
const MARK_RUN: usize = 6;

/// Is `p` the fill's colour `a` again (the bar's colour at its left start: a mark's edges are soft, so the pixels
/// just before one are already a mix)? Well inside the jump that ends a fill:
/// the track, the lighter trail of a hit and a hit's white flash are all further from the fill than that.
fn same_fill(a: Px, p: Px, jump: i32) -> bool {
    diff(a, p) <= jump / 2
}

/// Past a sharp change of colour at `from`: where the fill's own colour `a` goes on (MARK_RUN pixels of it in a
/// row, starting within `reach` px), or None when it doesn't. Where it does, the change was a mark drawn on the
/// bar and the fill goes on behind it. The game draws small orange marks on the track at about 3% and 8% of it:
/// the walk stopped at one, a fight read 6% for 10.7 s while its bar showed 55%, and its DPS came out 21,532
/// (17 Nov). The first read of 98 of 428 fights on the Python days was that 8 to 9% too.
fn fill_resumes(med: &mut dyn FnMut(i64) -> Px, a: Px, from: i64, right: i64, reach: i64, jump: i32) -> Option<i64> {
    (from..(right - MARK_RUN as i64).min(from + reach)).find(|&k| (0..MARK_RUN as i64).all(|j| same_fill(a, med(k + j), jump)))
}

fn fill_past_cover(med: &mut dyn FnMut(i64) -> Px, left: i64, right: i64, jump: i32, reach: i64) -> (f64, Px, Px) {
    let xs: Vec<i64> = (left..right).filter(|&x| !covered(med(x))).collect();
    if xs.len() < 6 {
        return (1.0, med(left + 2), med(right - 1));
    }
    let fill_color = med(xs[2]);
    let mut edge = right;
    let mut i = 3;
    while i < xs.len() - 2 {
        let x = xs[i];
        let a = med(xs[i - 2]);
        if diff(a, med(x)) > jump && diff(a, med(xs[i + 2])) > jump {
            // a mark on the bar, not the fill's end: the bar's colour goes on behind it (see fill_resumes)
            let on = (i..xs.len().saturating_sub(MARK_RUN)).take_while(|&k| xs[k] - x < reach).find(|&k| (0..MARK_RUN).all(|j| same_fill(fill_color, med(xs[k + j]), jump)));
            if let Some(k) = on {
                i = k + 3;
                continue;
            }
            edge = if xs[i] - xs[i - 2] > 2 { x - 1 } else { xs[i - 1] };
            break;
        }
        i += 1;
    }
    let frac = ((edge - left + 1) as f64 / (right - left) as f64).clamp(0.0, 1.0);
    (frac, fill_color, med((right - 1).min(edge + 3)))
}

/// Fill fraction without assuming a colour: follow the bar from its left end until the colour jumps sharply.
/// `(fraction, fill colour, edge colour)`.
pub fn measure_fill_any(im: &Image, bar: &Bar, jump: i32) -> (f64, Px, Px) {
    let rgb = if im.mode == Mode::Rgb { None } else { Some(im.convert(Mode::Rgb)) };
    let fast_ok = im.mode == Mode::Rgb;
    let im = rgb.as_ref().unwrap_or(im);
    let yc = bar.yc;
    let q = (bar.thickness / 4).max(1);
    let rows = [yc - q, yc, yc + q];
    let right_fast = bar.track_right.min(im.width as i64 - 3);
    let left = bar.track_left;
    let fast = fast_ok && left >= 0 && right_fast - left >= 6 && yc - q >= 0 && yc + q < im.height as i64;
    {
        let mut dev = DEV.lock().unwrap();
        if fast {
            dev.0 += 1;
        } else {
            dev.1 += 1;
        }
    }
    let right = right_fast;
    let reach = MARK_REACH * bar.thickness.max(1);
    let mut seen: std::collections::HashMap<i64, Px> = std::collections::HashMap::new();
    let mut med = |x: i64| *seen.entry(x).or_insert_with(|| median3(px(im, x, rows[0]), px(im, x, rows[1]), px(im, x, rows[2])));
    let any_cover = if fast {
        // every third pixel of the centre row, as the slow walk looks
        let picks: Vec<Px> = (left..right).step_by(3).map(|x| px(im, x, yc)).collect();
        picks.iter().map(|p| p[0]).max().unwrap_or(0) >= 250
            && picks.iter().map(|p| p[1]).min().unwrap_or(255) <= 5
            && picks.iter().map(|p| p[2]).max().unwrap_or(0) >= 250
            && picks.iter().any(|p| covered(*p))
    } else {
        (left..right).step_by(3).any(|x| covered(px(im, x, yc)))
    };
    if any_cover {
        return fill_past_cover(&mut med, left, right, jump, reach);
    }
    let fill_color = med(left + 2);
    let mut edge = right;
    let mut x = left + 3;
    while x < right - 2 {
        let a = med(x - 2);
        if diff(a, med(x)) > jump && diff(a, med(x + 2)) > jump {
            if let Some(on) = fill_resumes(&mut med, fill_color, x, right, reach, jump) {
                x = on + 3; // a mark on the bar: the fill goes on behind it
                continue;
            }
            edge = x - 1;
            break;
        }
        x += 1;
    }
    let frac = ((edge - left + 1) as f64 / (right - left) as f64).clamp(0.0, 1.0);
    (frac, fill_color, med((right - 1).min(edge + 3)))
}

pub const BREAK_BELOW: (f64, f64) = (0.010, 0.019);
fn gauge_white(p: Px) -> bool {
    *p.iter().min().unwrap() >= 195 && p.iter().max().unwrap() - p.iter().min().unwrap() <= 14
}
fn gauge_orange(p: Px) -> bool {
    p[0] >= 235 && (185..=220).contains(&p[1]) && (125..=170).contains(&p[2])
}
fn break_yellow(p: Px) -> bool {
    p[0] >= 235 && p[1] >= 200 && p[2] <= 150
}
fn dark(p: Px) -> bool {
    *p.iter().max().unwrap() < 135
}

/// The break gauge: (fraction left or None, "white"/"orange"/None, broken).
pub fn measure_break(im: &Image, bar: &Bar) -> (Option<f64>, Option<&'static str>, bool) {
    let (w, h) = (im.width as i64, im.height as i64);
    let s = bar.span;
    let yc = bar.yc;
    let (left, right) = (bar.track_left, bar.track_right.min(w - 1));
    if right - left < 50 {
        return (None, None, false);
    }
    let yellow = (left..right).step_by(4).filter(|&x| break_yellow(px(im, x, yc))).count() as f64 / ((right - left) as f64 / 4.0);
    let broken = yellow > 0.05;
    let gap = pyround(0.004 * s).max(2);
    let (y0, y1) = ((yc as f64 + BREAK_BELOW.0 * s) as i64, (h - gap - 1).min((yc as f64 + BREAK_BELOW.1 * s) as i64 + 1));
    let x0 = left + 3;
    let tests: [(fn(Px) -> bool, &str); 2] = [(gauge_white, "white"), (gauge_orange, "orange")];
    for y in y0..y1 {
        for (test, kind) in tests {
            if !(test(px(im, x0, y)) && test(px(im, x0 + 3, y))) {
                continue;
            }
            if !(1..gap + 2).any(|d| dark(px(im, x0, y - d))) || !(1..gap + 2).any(|d| dark(px(im, x0, y + d))) {
                continue;
            }
            let (mut x, mut miss, mut last) = (x0, 0, x0);
            while x < right && (test(px(im, x, y)) || covered(px(im, x, y)) || miss < 2) {
                if test(px(im, x, y)) {
                    miss = 0;
                    last = x;
                } else if !covered(px(im, x, y)) {
                    miss += 1;
                }
                x += 1;
            }
            return (Some(((last + 1 - left) as f64 / (right - left) as f64).clamp(0.0, 1.0)), Some(kind), broken);
        }
    }
    (if broken { Some(0.0) } else { None }, None, broken)
}

/// The two brackets of a locked bar as this picture shows them (each within 2 px of its place).
pub fn bracket_sides(im: &Image, bar: &Bar) -> (Side, Side) {
    if bar.track_right >= im.width as i64 || bar.yc >= im.height as i64 {
        return (Side::Fail, Side::Fail);
    }
    let lb = pyround(bar.left_bracket);
    let rb = pyround(bar.left_bracket + bar.span);
    (side_near(im, lb, bar.yc, bar.thickness), side_near(im, rb, bar.yc, bar.thickness))
}

/// Is the boss bar still where it was locked? The two white brackets only: neither may show something else, and at
/// least one must show (a bracket under our own windows' cover is neither).
pub fn verify_bar(im: &Image, bar: &Bar) -> bool {
    let (l, r) = bracket_sides(im, bar);
    l != Side::Fail && r != Side::Fail && (l == Side::Pass || r == Side::Pass)
}

/// Both brackets seen where the bar is locked: a look that confirms its ends.
pub fn verify_strict(im: &Image, bar: &Bar) -> bool {
    bracket_sides(im, bar) == (Side::Pass, Side::Pass)
}

/// Looks on which a fight's bar ends must be seen the same (both brackets, centred) before they are its ends.
pub const HOLD_LOOKS: u32 = 3;
/// Two sights of the ends agree when each end is within this many px (and the row within 3).
pub const AGREE_PX: f64 = 2.0;

/// Do two bars have the same ends?
pub fn same_ends(a: &Bar, b: &Bar) -> bool {
    a.frame.0 == b.frame.0
        && (a.left_bracket - b.left_bracket).abs() <= AGREE_PX
        && (a.left_bracket + a.span - b.left_bracket - b.span).abs() <= AGREE_PX
        && (a.yc - b.yc).abs() <= 3
}

/// What one look did to the held ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldNews {
    Nothing,
    /// The fight's ends are held from now on.
    Held,
    /// The held ends were replaced (once a fight, early in it).
    Replaced,
}

/// A fight's bar ends, held once seen. The bar never moves during a fight, so every other span the finder returns is
/// a measuring error: once the ends have been seen the same on HOLD_LOOKS looks (both brackets, centred on the frame)
/// they are the fight's, and the bar is read there whatever a later search says. The ends a fight merely started on
/// are not trusted (first finds measured spans of 1153 and 1367 px, the bar's being 1387): early in the
/// fight the held ends are replaced once by a pair that is better, i.e. seen HOLD_LOOKS times on looks where the
/// held ones were not seen at all (any look that sees the held ends starts that count over).
#[derive(Clone, Debug, Default)]
pub struct EndsHold {
    pub held: Option<Bar>,
    /// Looks the held ends were seen on.
    pub seen: u32,
    pub replaced: bool,
    cand: Option<(Bar, u32)>,
    /// The fight the ends are of (its log's name), the frame size, when it was first looked at.
    pub fight: Option<String>,
    pub frame: (usize, usize),
    pub since: f64,
    /// The ends the last reading was taken at (left bracket, span, row).
    pub last_read: Option<(f64, f64, i64)>,
    /// Looks read at the held ends although their bracket check failed; search results not taken.
    pub held_reads: u32,
    pub not_taken: u32,
}

impl EndsHold {
    /// Nothing held yet, for this fight (its id) at this frame size, first looked at since.
    pub fn new(fight: Option<String>, frame: (usize, usize), since: f64) -> EndsHold {
        EndsHold { fight, frame, since, ..Default::default() }
    }

    /// One look's sight of the ends: a bar whose both brackets were seen (None: not seen on this look). `early`: the
    /// held ends may still be replaced.
    pub fn look(&mut self, seen: Option<&Bar>, early: bool) -> HoldNews {
        let Some(s) = seen.filter(|s| s.brackets_found && centred(s)) else { return HoldNews::Nothing };
        if let Some(h) = &self.held {
            if same_ends(s, h) {
                self.seen += 1;
                self.cand = None;
                return HoldNews::Nothing;
            }
            if self.replaced || !early {
                return HoldNews::Nothing;
            }
        }
        self.cand = match self.cand.take() {
            Some((c, n)) if same_ends(&c, s) => Some((c, n + 1)),
            _ => Some((s.clone(), 1)),
        };
        if self.cand.as_ref().is_some_and(|c| c.1 < HOLD_LOOKS) {
            return HoldNews::Nothing;
        }
        let (c, n) = self.cand.take().unwrap();
        let news = if self.held.is_some() { HoldNews::Replaced } else { HoldNews::Held };
        self.replaced |= news == HoldNews::Replaced;
        self.held = Some(c);
        self.seen = n;
        news
    }
}

pub fn box_at(bar: &Bar, rel: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    let (lb, s, yc) = (bar.left_bracket, bar.span, bar.yc as f64);
    (lb + rel.0 * s, yc + rel.1 * s, lb + rel.2 * s, yc + rel.3 * s)
}

/// Boss rush countdown, pinned to the top-left of the 16:9 UI area.
pub fn timer_box(w: f64, h: f64, wide: bool) -> (f64, f64, f64, f64) {
    let u = w.min(h * 16.0 / 9.0);
    if wide { (u * 0.02, h * 0.26, u * 0.10, h * 0.33) } else { (u * 0.03, h * 0.27, u * 0.09, h * 0.32) }
}
