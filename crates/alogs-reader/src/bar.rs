//! Finding and measuring the boss bar. Port of the bar half of aLogs B16 `bossbar/reader.py`.
//!
//! Resolution-independent: the white [ ] brackets at the ends of the boss bar are the ruler. Every position is a
//! fraction of the bracket span, measured once on a 2000x1125 screenshot (span 789 px).

use alogs_pil::ops::{darker, lighter};
use alogs_pil::{Filter, Image, Mode, Transpose};
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

fn line_px(im: &Image, x: i64, y: i64) -> bool {
    let p = px(im, x, y);
    if is_white(p) {
        return true;
    }
    if *p.iter().min().unwrap() < 140 {
        return false;
    }
    for nx in [x - 1, x + 1] {
        if let Some(q) = px_at(im, nx, y)
            && *q.iter().min().unwrap() >= 140
        {
            return true;
        }
    }
    false
}

fn white_run(im: &Image, x: i64, yc: i64, reach: i64, h: i64) -> i64 {
    ((yc - reach).max(0)..(yc + reach).min(h)).filter(|&y| line_px(im, x, y)).count() as i64
}

fn is_bracket(im: &Image, x: i64, yc: i64, thick: i64, w: i64, h: i64) -> bool {
    if (white_run(im, x, yc, 2 * thick, h) as f64) < 1.3 * thick as f64 - 1.5 {
        return false;
    }
    let off = (thick / 3).max(3);
    let mut n = 0;
    for s in [-off, off] {
        for yy in [yc - thick / 3, yc + thick / 3] {
            if is_white(px(im, (x + s).max(0).min(w - 1), yy)) {
                n += 1;
            }
        }
    }
    n <= 2
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

/// `_bright_columns`: per column, how many pixels in the rows around `yc` have their darkest channel at 140 or more.
fn bright_columns(im: &Image, yc: i64, reach: i64, h: i64) -> Option<Vec<u32>> {
    let (y0, y1) = ((yc - reach).max(0), (yc + reach).min(h));
    if y1 <= y0 {
        return None;
    }
    let (r, g, b) = rgb3(&im.crop_i(0, y0, im.width as i64, y1));
    let rows = darker(&darker(&r, &g), &b).point(|v| if v >= 140 { 1.0 } else { 0.0 });
    let t = rows.transpose(Transpose::Transpose);
    let hh = (y1 - y0) as usize;
    Some((0..im.width).map(|x| t.data[x * hh..(x + 1) * hh].iter().filter(|&&v| v == 1).count() as u32).collect())
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
        match check_candidate(im, x0 + pyround(bs as f64 * f), y0 + pyround(by as f64 * f), d, cropped, None, true) {
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
        if spot.frame != im.size() {
            continue;
        }
        let (yc, t) = (spot.yc as i64, spot.thickness as i64);
        let lb = pyround(spot.left_bracket);
        let rb = pyround(spot.left_bracket + spot.span);
        let at = |x0: i64| ((x0 - 2).max(0)..(x0 + 3).min(w)).find(|&x| is_bracket(im, x, yc, t, w, h));
        if !(0 <= yc && yc < h && rb < w) {
            continue;
        }
        let Some(lbx) = at(lb) else { continue };
        let Some(rbx) = at(rb) else { continue };
        for dx in [2, 8, 20] {
            if let Ok(bar) = check_candidate(im, spot.track_left as i64 + dx, spot.yc as i64, d, false, Some((lbx, rbx)), false) {
                return Some(bar);
            }
        }
    }
    None
}

/// Refine one candidate run at full resolution and check it is the boss bar.
fn check_candidate(im: &Image, xg: i64, yg: i64, d: i64, cropped: bool, brackets: Option<(i64, i64)>, fast: bool) -> Result<Bar, String> {
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
    let mut top = yg;
    while top > 0 && colored(px(im, col, top - 1)) {
        top -= 1;
    }
    let mut bot = yg;
    while bot < h - 1 && colored(px(im, col, bot + 1)) {
        bot += 1;
    }
    let yc = (top + bot).div_euclid(2);
    let thick = (bot - top + 1).max(3);
    if !cropped && thick as f64 > h as f64 * 0.03 {
        return Err(format!("too thick ({thick}px)"));
    }
    let (mut lb, mut rb): (Option<f64>, Option<f64>) = (None, None);
    let start;
    let est = SPAN_PER_THICKNESS * thick as f64;
    if let Some((l, r)) = brackets {
        lb = Some(l as f64);
        rb = Some(r as f64);
        start = xg;
    } else {
        let (_, s) = if fast { colored_run(im, yc, (xg - 4 * d).max(0), w) } else { longest_run(im, yc, (xg - 4 * d).max(0), w, colored) };
        start = s;
        let bright = if fast { bright_columns(im, yc, 2 * thick, h) } else { None };
        let need = 1.3 * thick as f64 - 1.5;
        let bracket = |x: i64| {
            let ok = match &bright {
                None => true,
                Some(b) => f64::from(b[x as usize]) >= need,
            };
            ok && is_bracket(im, x, yc, thick, w, h)
        };
        let mut x = start - 1;
        while x > (start - 4 * thick).max(0) - 1 {
            if bracket(x) {
                lb = Some(x as f64);
                break;
            }
            x -= 1;
        }
        if let Some(l) = lb {
            let u = (w as f64).min(h as f64 * 16.0 / 9.0);
            let hi = (w - 1).min((w as f64 - l + 0.06 * u) as i64);
            let lo = ((l + 0.3 * u) as i64).max((w as f64 - l - 0.06 * u) as i64);
            let mut lines = Vec::new();
            let mut x = hi;
            while x > lo {
                if bracket(x) {
                    lines.push(x);
                }
                x -= 1;
            }
            // min() keeps the first of equals
            let mut best: Option<(f64, i64)> = None;
            for x in lines {
                let k = (l + (x as f64 - l) / 2.0 - w as f64 / 2.0).abs();
                if best.is_none_or(|b| k < b.0) {
                    best = Some((k, x));
                }
            }
            rb = best.map(|b| b.1 as f64);
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
        brackets_found: rb.is_some(),
        world,
        right_red,
        track_left: pyround(lbv + TRACK.0 * span),
        track_right: pyround(lbv + right * span),
        thickness: thick,
        frame: im.size(),
    })
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

fn fill_past_cover(med: &mut dyn FnMut(i64) -> Px, left: i64, right: i64, jump: i32) -> (f64, Px, Px) {
    let xs: Vec<i64> = (left..right).filter(|&x| !covered(med(x))).collect();
    if xs.len() < 6 {
        return (1.0, med(left + 2), med(right - 1));
    }
    let fill_color = med(xs[2]);
    let mut edge = right;
    for i in 3..xs.len() - 2 {
        let x = xs[i];
        let a = med(xs[i - 2]);
        if diff(a, med(x)) > jump && diff(a, med(xs[i + 2])) > jump {
            edge = if xs[i] - xs[i - 2] > 2 { x - 1 } else { xs[i - 1] };
            break;
        }
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
        return fill_past_cover(&mut med, left, right, jump);
    }
    let fill_color = med(left + 2);
    let mut edge = right;
    for x in left + 3..right - 2 {
        let a = med(x - 2);
        if diff(a, med(x)) > jump && diff(a, med(x + 2)) > jump {
            edge = x - 1;
            break;
        }
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

/// Is the boss bar still where it was locked? The two white brackets only.
pub fn verify_bar(im: &Image, bar: &Bar) -> bool {
    if bar.track_right >= im.width as i64 || bar.yc >= im.height as i64 {
        return false;
    }
    let (w, h) = (im.width as i64, im.height as i64);
    let t = bar.thickness;
    let lb = pyround(bar.left_bracket);
    let rb = pyround(bar.left_bracket + bar.span);
    let near = |x0: i64| ((x0 - 2).max(0)..(x0 + 3).min(w)).any(|x| is_bracket(im, x, bar.yc, t, w, h));
    near(lb) && near(rb)
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
