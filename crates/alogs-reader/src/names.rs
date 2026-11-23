//! Short in-game names ("0", "Ab") and big numbers read by matching each character against the game's own font.
//! Port of aLogs B16 `bossbar/namereader.py`.

use crate::gamefont::GameFont;
use alogs_pil::ops::{Stat, darker, difference, lighter, logical_and, logical_or, multiply, subtract};
use alogs_pil::{Filter, Image, Mode};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::collections::HashMap;

pub const CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
pub const NAME_PX_4K: f64 = 50.0;
pub const PROFILE_PX_4K: f64 = 61.0;
pub const STATS_PX_4K: f64 = 43.5;
pub const EDGE: u8 = 120;
pub const MIN_SCORE: f64 = 0.6;
pub const MIN_MARGIN: f64 = 0.05;
pub const FITS_KEPT: usize = 1000;
/// A level digit's fit to the game's digit, and its lead over the next best digit (`NameReader::level`): stricter than
/// a name's characters, as the level fixes the boss's HP.
pub const LEVEL_SCORE: f64 = 0.6;
pub const LEVEL_MARGIN: f64 = 0.12;

type BoxF = (f64, f64, f64, f64);
type Rgb = [u8; 3];

fn crop_int(img: &Image, b: BoxF) -> Image {
    img.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64)
}

fn to_one(m: &Image) -> Image {
    let mut o = m.clone();
    o.mode = Mode::One;
    for v in o.data.iter_mut() {
        *v = if *v >= 128 { 255 } else { 0 };
    }
    o
}

fn cut_one(m: &Image) -> Option<Image> {
    let b = m.getbbox()?;
    Some(to_one(&m.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64)))
}

fn bands3(c: &Image) -> Vec<Image> {
    c.split()
}

/// The box's white, colourless pixels (the game's white text), cut to their extent.
pub fn text_mask(img: &Image, b: BoxF, bright: u8, grey: u8) -> Option<Image> {
    let c = crop_int(img, b).convert(Mode::Rgb);
    let s = bands3(&c);
    let lo = darker(&darker(&s[0], &s[1]), &s[2]);
    let hi = lighter(&lighter(&s[0], &s[1]), &s[2]);
    let white = lo.point(|v| if v > bright { 255.0 } else { 0.0 });
    let flat = subtract(&hi, &lo, 1.0, 0).point(|v| if v < grey { 255.0 } else { 0.0 });
    cut_one(&multiply(&white, &flat))
}

fn distance_to(c: &Image, colour: Rgb) -> Image {
    let d: Vec<Image> = bands3(c).iter().zip(colour).map(|(band, v)| difference(band, &Image::new(Mode::L, c.width, c.height, &[v]))).collect();
    lighter(&lighter(&d[0], &d[1]), &d[2])
}

fn median_rgb(im: &Image, mask: Option<&Image>) -> Rgb {
    let m = Stat::new(im, mask).median();
    [m[0] as u8, m[1] as u8, m[2] as u8]
}

/// `(text colour, background colour)` in a box of text on a plain-ish card; `(None, bg)` if nothing stands out.
pub fn ink_colour(img: &Image, b: BoxF, far: u8, dark: bool) -> (Option<Rgb>, Rgb) {
    let c = crop_int(img, b).convert(Mode::Rgb);
    let q = alogs_pil::quant::quantize(&c, 16);
    let i = q.getcolors().into_iter().max().unwrap().1;
    let mut bg = q.palette[i];
    let dist = distance_to(&c, bg);
    let top = dist.getextrema()[0].1;
    if top < far {
        return (None, bg);
    }
    let core = dist.point(|v| if f64::from(v) >= f64::from(top) * 0.85 { 255.0 } else { 0.0 });
    let mut ink = median_rgb(&c, Some(&core));
    let sum = |p: Rgb| p.iter().map(|&v| u32::from(v)).sum::<u32>();
    if dark && sum(ink) > sum(bg) {
        let (w, h) = c.size();
        let mut edge = Image::new(Mode::Rgb, w, 2, &[0, 0, 0]);
        edge.paste(&c.crop_i(0, 0, w as i64, 1), 0, 0);
        edge.paste(&c.crop_i(0, h as i64 - 1, w as i64, h as i64), 0, 1);
        ink = bg;
        bg = median_rgb(&edge, None);
        if sum(bg) <= sum(ink) {
            return (None, bg);
        }
    }
    (Some(ink), bg)
}

/// `[(left, right, height)]` of each shape between empty columns.
pub fn characters_at(mask: &Image) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    let col = |x: usize| (0..mask.height).any(|y| mask.data[y * mask.width + x] != 0);
    for x in 0..=mask.width {
        let ink = x < mask.width && col(x);
        if ink && start.is_none() {
            start = Some(x);
        } else if !ink && let Some(s) = start {
            let b = mask.crop_i(s as i64, 0, x as i64, mask.height as i64).getbbox().unwrap();
            out.push((s, x, b.3 - b.1));
            start = None;
        }
    }
    out
}

/// The mask without the small round icon some name cards show in front of the name.
pub fn drop_icon(mask: &Image) -> Image {
    let spans = characters_at(mask);
    if spans.len() < 2 {
        return mask.clone();
    }
    let ((x0, x1, h), (nx0, _, _)) = (spans[0], spans[1]);
    let w = x1 - x0;
    let r = w as f64 / h.max(1) as f64;
    if (0.8..=1.25).contains(&r) && (nx0 - x1) as f64 >= 0.35 * mask.height as f64 {
        let rest = mask.crop_i(nx0 as i64, 0, mask.width as i64, mask.height as i64);
        if let Some(b) = rest.getbbox() {
            return rest.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64);
        }
    }
    mask.clone()
}

/// "female", "male" or None: the Profile name card's mark after the name (told by the hole's place in its ring).
pub fn gender_mark(img: &Image, b: BoxF, far: u8) -> Option<&'static str> {
    let (ink, bg) = ink_colour(img, b, 80, false);
    let ink = ink?;
    let c = crop_int(img, b).convert(Mode::Rgb);
    let away = |col: Rgb| distance_to(&c, col).point(move |v| if v > far { 255.0 } else { 0.0 });
    let mark = multiply(&away(bg), &away(ink));
    let mut runs: Vec<[usize; 2]> = Vec::new();
    let (mut start, mut last): (Option<usize>, usize) = (None, 0);
    for x in 0..mark.width {
        if (0..mark.height).any(|y| mark.data[y * mark.width + x] != 0) {
            if start.is_none() || x - last > 2 {
                runs.push([x, x]);
                start = Some(x);
            }
            runs.last_mut().unwrap()[1] = x;
            last = x;
        }
    }
    let min_w = 4f64.max(mark.height as f64 * 0.2);
    let runs: Vec<[usize; 2]> = runs.into_iter().filter(|r| (r[1] - r[0] + 1) as f64 >= min_w).collect();
    let r = runs.last()?;
    let part = mark.crop_i(r[0] as i64, 0, r[1] as i64 + 1, mark.height as i64);
    let bb = part.getbbox()?;
    let part = part.crop_i(bb.0 as i64, bb.1 as i64, bb.2 as i64, bb.3 as i64);
    let (w, h) = part.size();
    if h < 8 || !(0.5..=1.6).contains(&(w as f64 / h as f64)) {
        return None;
    }
    let mut room = Image::new(Mode::L, w + 2, h + 2, &[0]);
    room.paste(&part, 1, 1);
    alogs_pil::draw::floodfill(&mut room, (0, 0), &[128]);
    let hole = room.crop_i(1, 1, w as i64 + 1, h as i64 + 1).point(|v| if v == 0 { 255.0 } else { 0.0 });
    let (mut n, mut sx, mut sy) = (0usize, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            if hole.data[y * w + x] != 0 {
                n += 1;
                sx += x;
                sy += y;
            }
        }
    }
    if (n as f64) < (w * h) as f64 * 0.02 {
        return None;
    }
    let (dx, dy) = (sx as f64 / n as f64 - w as f64 / 2.0, sy as f64 / n as f64 - h as f64 / 2.0);
    if dx > 0.0 && dy < 0.0 {
        return Some("female");
    }
    if dx < 0.0 && dy > 0.0 {
        return Some("male");
    }
    None
}

/// The box's pixels nearer the text colour than the background, as `text_mask` gives them.
pub fn ink_mask(img: &Image, b: BoxF, ink: Rgb, bg: Rgb) -> Option<Image> {
    let c = crop_int(img, b).convert(Mode::Rgb);
    let tol = ink.iter().zip(bg).map(|(a, b)| (i32::from(*a) - i32::from(b)).abs()).max().unwrap() / 2;
    cut_one(&distance_to(&c, ink).point(move |v| if i32::from(v) < tol { 255.0 } else { 0.0 }))
}

/// The mask split at its empty columns, specks left out.
pub fn characters(mask: &Image) -> Vec<Image> {
    let mut out = Vec::new();
    let mut start = None;
    let col = |x: usize| (0..mask.height).any(|y| mask.data[y * mask.width + x] != 0);
    for x in 0..=mask.width {
        let ink = x < mask.width && col(x);
        if ink && start.is_none() {
            start = Some(x);
        } else if !ink && let Some(s) = start {
            let part = mask.crop_i(s as i64, 0, x as i64, mask.height as i64);
            if let Some(b) = part.getbbox() {
                let small = mask.height as f64 * 0.3;
                if (b.2 - b.0) as f64 >= small || (b.3 - b.1) as f64 >= small {
                    out.push(part.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64));
                }
            }
            start = None;
        }
    }
    out
}

fn count(im: &Image) -> u64 {
    im.convert(Mode::L).histogram(None)[255]
}

/// Intersection over union of two 1-bit shapes at the best small shift.
pub fn overlap(a: &Image, b: &Image, slack: i64) -> f64 {
    let (w, h) = (a.width.max(b.width) + 2 * slack as usize, a.height.max(b.height) + 2 * slack as usize);
    let mut aa = Image::new(Mode::One, w, h, &[0]);
    aa.paste(a, slack, slack);
    let mut best = 0.0f64;
    for dy in -slack..=slack {
        for dx in -slack..=slack {
            let mut bb = Image::new(Mode::One, w, h, &[0]);
            bb.paste(b, slack + dx, slack + dy);
            let u = count(&logical_or(&aa, &bb));
            if u > 0 {
                best = best.max(count(&logical_and(&aa, &bb)) as f64 / u as f64);
            }
        }
    }
    best
}

/// The colours a boss's level is drawn in, by how strong the boss is (green, orange, white, red), with some room for
/// the light behind it.
fn level_green(p: Rgb) -> bool {
    p[1] >= 200 && p[0] <= 215 && p[2] <= 185 && p[1] >= p[0] + 15
}
fn level_orange(p: Rgb) -> bool {
    p[0] >= 215 && (120..=200).contains(&p[1]) && p[2] <= 130
}
fn level_white(p: Rgb) -> bool {
    p.iter().min().copied().unwrap_or(0) >= 200 && p.iter().max().copied().unwrap_or(0) - p.iter().min().copied().unwrap_or(0) < 40
}
fn level_red(p: Rgb) -> bool {
    p[0] >= 200 && p[1] <= 110 && p[2] <= 110
}

fn desc(mut v: Vec<(f64, char)>) -> Vec<(f64, char)> {
    // sorted(reverse=True) on (score, character)
    v.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    v
}

type ShapeKey = (u32, (usize, usize), Vec<u8>);

pub struct NameReader {
    pub font: GameFont,
    cache: RefCell<HashMap<(char, i64), Option<Image>>>,
    fits: RefCell<IndexMap<ShapeKey, HashMap<char, f64>>>,
}

impl NameReader {
    pub fn new(font: GameFont) -> NameReader {
        NameReader { font, cache: RefCell::new(HashMap::new()), fits: RefCell::new(IndexMap::new()) }
    }

    fn glyph(&self, ch: char, px: i64) -> Option<Image> {
        if let Some(g) = self.cache.borrow().get(&(ch, px)) {
            return g.clone();
        }
        let (field, _) = self.font.field(&ch.to_string(), px as f64);
        let g = cut_one(&field.point(|v| if v > EDGE { 255.0 } else { 0.0 }));
        self.cache.borrow_mut().insert((ch, px), g.clone());
        g
    }

    /// Use `g` (its ink, cut to it) as the shape of `ch` at `px`: a glyph drawn by hand, for a font that lacks one, and
    /// for tests (the game's font can't be in the repository).
    pub fn set_glyph(&self, ch: char, px: i64, g: &Image) {
        self.cache.borrow_mut().insert((ch, px), cut_one(g));
        self.fits.borrow_mut().clear();
    }

    /// `[(score, character)]` best first, for one character's shape.
    pub fn best(&self, part: &Image, px: i64, chars: &str) -> Vec<(f64, char)> {
        let mut scores = Vec::new();
        for ch in chars.chars() {
            let Some(g) = self.glyph(ch, px) else { continue };
            if (g.height as i64 - part.height as i64).abs() > 3.max(part.height as i64 / 3) {
                continue;
            }
            if (g.width as i64 - part.width as i64).abs() > 3.max(part.width as i64 / 3) {
                continue;
            }
            scores.push((overlap(part, &g, 2), ch));
        }
        desc(scores)
    }

    /// How well one character's ink is `ch` whatever its size (kept per exact shape).
    pub fn shape_fit(&self, part: &Image, ch: char, px: i64) -> f64 {
        let key: ShapeKey = (px as u32, part.size(), part.tobytes());
        // (the cached value taken in a statement of its own: an `if let` on the borrow kept it borrowed through the
        // block, and the second borrow below panicked on every repeat read of a name)
        let cached = self.fits.borrow().get(&key).and_then(|m| m.get(&ch).copied());
        if let Some(v) = cached {
            let mut f = self.fits.borrow_mut();
            let idx = f.get_index_of(&key).unwrap();
            let last = f.len() - 1;
            f.move_index(idx, last);
            return v;
        }
        let v = self.shape_fit_raw(part, ch, px);
        let mut f = self.fits.borrow_mut();
        if let Some(idx) = f.get_index_of(&key) {
            let last = f.len() - 1;
            f.move_index(idx, last);
        } else {
            f.insert(key.clone(), HashMap::new());
            if f.len() > FITS_KEPT {
                f.shift_remove_index(0);
            }
        }
        if let Some(m) = f.get_mut(&key) {
            m.insert(ch, v);
        }
        v
    }

    fn shape_fit_raw(&self, part: &Image, ch: char, px: i64) -> f64 {
        let Some(g) = self.glyph(ch, px) else { return 0.0 };
        let (a, b) = (part.width as f64 / part.height as f64, g.width as f64 / g.height as f64);
        overlap(part, &g.resize(part.width, part.height, Some(Filter::Nearest)), 1) * a.min(b) / a.max(b)
    }

    /// The boss's level in the bar's title box ("Lv. 65 Alpha Stellarys"), read against the game's own font. The level
    /// is drawn in one colour (green, orange, white or red, by how strong the boss is): for each, the box's pixels of
    /// that colour are cut into characters, "L" and "v" are looked for by their shapes, and the characters after them
    /// (the dot passed over, or cut off where it touches the first digit) are matched one by one against the game's
    /// digits. Some(level) only when every digit is clear and there are one or two of them, not starting with 0; else
    /// None (unread).
    pub fn level(&self, img: &Image, b: BoxF) -> Option<i64> {
        let c = crop_int(img, b).convert(Mode::Rgb);
        let (w, h) = c.size();
        if w < 20 || h < 8 {
            return None;
        }
        let colours: [fn(Rgb) -> bool; 4] = [level_green, level_orange, level_white, level_red];
        for colour in colours {
            let mut mask = Image::new(Mode::L, w, h, &[0]);
            for y in 0..h {
                for x in 0..w {
                    let p = c.px(x, y);
                    if colour([p[0], p[1], p[2]]) {
                        mask.data[y * w + x] = 255;
                    }
                }
            }
            if let Some(found) = self.level_in(&mask) {
                return found;
            }
        }
        None
    }

    /// The level in one colour's mask: None when no "Lv" is there, Some(None) when it is but its digits aren't clear.
    fn level_in(&self, mask: &Image) -> Option<Option<i64>> {
        let (w, h) = mask.size();
        let spans = characters_at(mask);
        // a span's ink box: (left, top, right, bottom) in the mask
        let ink = |s: (usize, usize, usize), skip: usize| -> Option<(Image, (usize, usize, usize, usize))> {
            let piece = mask.crop_i((s.0 + skip) as i64, 0, s.1 as i64, h as i64);
            let bb = piece.getbbox()?;
            Some((to_one(&piece.crop_i(bb.0 as i64, bb.1 as i64, bb.2 as i64, bb.3 as i64)), (s.0 + skip + bb.0, bb.1, s.0 + skip + bb.2, bb.3)))
        };
        let best = |im: &Image, set: &str| desc(set.chars().map(|ch| (self.shape_fit(im, ch, 64), ch)).collect());
        let is = |f: &[(f64, char)], ch: char| f.len() > 1 && f[0].1 == ch && f[0].0 >= MIN_SCORE && f[0].0 - f[1].0 >= MIN_MARGIN;
        const SHAPES: &str = "Lv0123456789AEFHIJKT";
        for i in 0..spans.len().saturating_sub(2) {
            let (Some((l, lb)), Some((v, vb))) = (ink(spans[i], 0), ink(spans[i + 1], 0)) else { continue };
            let tall = lb.3 - lb.1;
            // the L: a capital's height; the v: lower, its foot on the L's baseline, close after it
            // (the title's capitals are about 0.47 of its box's height: anything else is other lettering)
            if tall < 6 || tall * 100 < h * 38 || tall * 100 > h * 58 || vb.3.abs_diff(lb.3) > tall / 6 + 1 || (vb.3 - vb.1) * 10 > tall * 9 || vb.0 > lb.2 + tall {
                continue;
            }
            if !(is(&best(&l, SHAPES), 'L') && is(&best(&v, "Lv0123456789xyuwnr"), 'v')) {
                continue;
            }
            let mut digits = String::new();
            let mut prev_end = vb.2;
            for &s in &spans[i + 2..] {
                let Some((_, sb)) = ink(s, 0) else { continue };
                if sb.0 > prev_end + tall {
                    break; // (the gap before the separator and the name)
                }
                // the dot: small, on the baseline, before the digits
                if digits.is_empty() && (sb.3 - sb.1) * 3 < tall && (sb.2 - sb.0) * 2 < tall {
                    prev_end = sb.2;
                    continue;
                }
                if (sb.3 - sb.1) * 100 < tall * 85 || sb.3.abs_diff(lb.3) > tall / 6 + 1 {
                    // not a digit's height on the L's baseline: the separator after a space ends the level; anything
                    // close after a digit (a symbol, a digit cut by something over it) leaves it unread
                    if sb.0 * 2 < prev_end * 2 + tall {
                        return Some(None);
                    }
                    break;
                }
                // the dot touching the first digit: its own columns hold ink only near the baseline
                let low = |x: usize| (0..h).all(|y| mask.data[y * w + x] == 0 || y * 4 >= sb.3 * 4 - tall);
                let skip = if digits.is_empty() { (s.0..s.1).take_while(|&x| low(x)).count() } else { 0 };
                let Some((d, db)) = ink(s, skip) else { return Some(None) };
                // one digit's width at most (two touching digits are no clear read)
                if (db.2 - db.0) * 10 > tall * 9 {
                    return Some(None);
                }
                let f = best(&d, "0123456789");
                if !(f.len() > 1 && f[0].0 >= LEVEL_SCORE && f[0].0 - f[1].0 >= LEVEL_MARGIN) {
                    return Some(None);
                }
                digits.push(f[0].1);
                prev_end = sb.2;
            }
            if digits.is_empty() || digits.len() > 2 || digits.starts_with('0') {
                return Some(None);
            }
            return Some(digits.parse().ok());
        }
        None
    }

    /// The number in `box` read digit by digit against the game's own digits, or None unless every digit is clear.
    pub fn number(&self, img: &Image, b: BoxF, margin: f64, dark: bool, right: bool, unit: bool) -> Option<i64> {
        let mask = if dark {
            let (ink, bg) = ink_colour(img, b, 80, true);
            ink.and_then(|i| ink_mask(img, b, i, bg))
        } else {
            text_mask(img, b, 190, 50)
        }?;
        let spans = characters_at(&mask);
        let tall = spans.iter().map(|s| s.2).max().unwrap_or(0);
        let mut spans: Vec<(usize, usize, usize)> = spans.into_iter().filter(|s| s.2 as f64 >= 0.7 * tall as f64).collect();
        if unit && spans.len() > 1 && (spans[spans.len() - 1].2 as f64) < 0.85 * tall as f64 {
            spans.pop();
        }
        if right && !spans.is_empty() {
            let mut run = vec![spans[spans.len() - 1]];
            for s in spans[..spans.len() - 1].iter().rev() {
                if (run[0].0 as f64 - s.1 as f64) > 0.45 * tall as f64 {
                    break;
                }
                run.insert(0, *s);
            }
            spans = run;
        }
        let parts: Vec<Image> = spans
            .iter()
            .filter_map(|&(x0, x1, _)| {
                let piece = mask.crop_i(x0 as i64, 0, x1 as i64, mask.height as i64);
                piece.getbbox().map(|bb| piece.crop_i(bb.0 as i64, bb.1 as i64, bb.2 as i64, bb.3 as i64))
            })
            .collect();
        if parts.is_empty() || parts.len() > 7 {
            return None;
        }
        let mut out = String::new();
        for part in &parts {
            let fits = desc("0123456789".chars().map(|d| (self.shape_fit(part, d, 64), d)).collect());
            if fits[0].0 < MIN_SCORE || fits[0].0 - fits[1].0 < margin {
                return None;
            }
            out.push(fits[0].1);
        }
        out.parse().ok()
    }

    /// The name in `box` (`u`: the width of the screen's 16:9 area), or None unless every character is clear.
    pub fn read(&self, img: &Image, b: BoxF, u: f64, max_chars: usize, px_4k: f64, mask: Option<Image>, chars: &str) -> Option<String> {
        let mask = match mask {
            Some(m) => m,
            None => text_mask(img, b, 190, 50)?,
        };
        let parts = characters(&mask);
        if parts.is_empty() || parts.len() > max_chars {
            return None;
        }
        let base = px_4k * u / 3840.0;
        let rb = base.round_ties_even() as i64;
        let mut guess: Option<(f64, f64, String)> = None;
        let mut sizes = vec![rb - 1, rb, rb + 1];
        sizes.dedup();
        for px in sizes {
            if px < 8 {
                continue;
            }
            let mut picks = Vec::new();
            for part in &parts {
                let s = self.best(part, px, chars);
                if s.is_empty() {
                    break;
                }
                picks.push((s[0].0, s[0].0 - s.get(1).map_or(0.0, |x| x.0), s[0].1));
            }
            if picks.len() != parts.len() {
                continue;
            }
            let worst = picks.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            if guess.as_ref().is_none_or(|g| worst > g.0) {
                guess = Some((worst, picks.iter().map(|p| p.1).fold(f64::INFINITY, f64::min), picks.iter().map(|p| p.2).collect()));
            }
        }
        let g = guess?;
        if g.0 < MIN_SCORE || g.1 < MIN_MARGIN || g.2.contains('I') || g.2.contains('l') {
            return None;
        }
        Some(g.2)
    }
}
