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

/// The ink boxes (left, top, right, bottom) of a name's letters in its mask, left to right: its 8-connected pieces,
/// those on the name's line only (the line: where most of the tall pieces sit), small ones (an i's dot, specks) left
/// out.
pub fn letter_pieces(mask: &Image) -> Vec<(usize, usize, usize, usize)> {
    let (w, h) = (mask.width, mask.height);
    let mut seen = vec![false; w * h];
    let mut pieces: Vec<(usize, usize, usize, usize, usize)> = vec![];
    for start in 0..w * h {
        if seen[start] || mask.data[start] == 0 {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (w, h, 0, 0, 0);
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1, n) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1), n + 1);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if !seen[j] && mask.data[j] != 0 {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        pieces.push((x0, y0, x1, y1, n));
    }
    let tallest = pieces.iter().map(|p| p.3 - p.1).max().unwrap_or(0);
    let mut big: Vec<&(usize, usize, usize, usize, usize)> = pieces.iter().filter(|p| (p.3 - p.1) * 3 >= tallest).collect();
    if big.is_empty() {
        return vec![];
    }
    // the line's baseline: the bottom most of the tall pieces share
    let mut bottoms: Vec<usize> = big.iter().map(|p| p.3).collect();
    bottoms.sort();
    let base = bottoms[bottoms.len() / 2];
    let mut heights: Vec<usize> = big.iter().filter(|p| p.3.abs_diff(base) <= 2).map(|p| p.3 - p.1).collect();
    heights.sort();
    let cap = heights.last().copied().unwrap_or(tallest).max(1);
    big.retain(|p| {
        let ht = p.3 - p.1;
        // on the line (a descender's foot below it), no taller than a capital, more than a speck
        p.3 + cap / 6 >= base && p.3 <= base + cap / 2 && p.1 + cap / 3 <= base && ht * 10 <= cap * 13 && ht * 4 >= cap
    });
    let mut out: Vec<(usize, usize, usize, usize)> = big.iter().map(|p| (p.0, p.1, p.2, p.3)).collect();
    out.sort();
    out
}

/// The characters the text reader mixes up in names: 0 / O / o and 1 / l / I.
pub const LOOK_ALIKES: &str = "0Oo1lI";

/// The positions (0-based, among the text's characters) of its look-alikes.
pub fn look_alike_positions(text: &str) -> Vec<usize> {
    text.chars().enumerate().filter(|(_, c)| LOOK_ALIKES.contains(*c)).map(|(i, _)| i).collect()
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

    /// A name's look-alikes told apart by their own ink: 0 / O / o and 1 / l / I (the Windows reader mixes them up: a
    /// zero in a name was sent as a capital O). `text` is the name as read; `b` its box on the screen (white lettering,
    /// the box holding the name alone). See `settle_mask`.
    pub fn settle_look_alikes(&self, img: &Image, b: BoxF, text: &str) -> (String, Vec<usize>) {
        let at = look_alike_positions(text);
        if at.is_empty() {
            return (text.to_string(), at);
        }
        match text_mask(img, b, 190, 50) {
            Some(mask) => self.settle_mask(&mask, text),
            None => (text.to_string(), at),
        }
    }

    /// As `settle_look_alikes`, for a box where the name shares its line with other white marks (the Party screen's
    /// pills: a badge before the name, icons around it). The line's ink is cut into groups at wide gaps (wider than
    /// half the letters' height); the name is the one group with as many letters as the name has, else nothing is
    /// settled.
    pub fn settle_look_alikes_in_line(&self, img: &Image, b: BoxF, text: &str) -> (String, Vec<usize>) {
        let at = look_alike_positions(text);
        if at.is_empty() {
            return (text.to_string(), at);
        }
        match text_mask(img, b, 190, 50) {
            Some(mask) => self.settle_mask_in_line(&mask, text),
            None => (text.to_string(), at),
        }
    }

    /// `settle_look_alikes_in_line` on a mask already made (the Profile's name card: dark lettering on a light card,
    /// its ink found by colour).
    pub fn settle_mask_in_line(&self, mask: &Image, text: &str) -> (String, Vec<usize>) {
        let at = look_alike_positions(text);
        if at.is_empty() {
            return (text.to_string(), at);
        }
        let letters = text.chars().filter(|c| !c.is_whitespace()).count();
        let pieces = letter_pieces(mask);
        if pieces.is_empty() {
            return (text.to_string(), at);
        }
        let mut hs: Vec<usize> = pieces.iter().map(|p| p.3 - p.1).collect();
        hs.sort();
        let gap = (hs[hs.len() / 2] / 2).max(2);
        let mut groups: Vec<(usize, usize, usize)> = vec![]; // (left, right, pieces)
        for p in &pieces {
            match groups.last_mut() {
                Some(g) if p.0 <= g.1 + gap => {
                    g.1 = g.1.max(p.2);
                    g.2 += 1;
                }
                _ => groups.push((p.0, p.2, 1)),
            }
        }
        let fit: Vec<&(usize, usize, usize)> = groups.iter().filter(|g| g.2 == letters).collect();
        if fit.len() != 1 {
            return (text.to_string(), at);
        }
        let g = fit[0];
        let part = mask.crop_i(g.0 as i64, 0, g.1 as i64, mask.height as i64);
        self.settle_mask(&part, text)
    }

    /// The look-alikes of `text` settled by the ink of `mask` (the name's lettering, white on black, the name alone on
    /// its line). The ink is cut into letters (connected pieces on the name's line, an i's dot and specks left out),
    /// one per letter of the name, else nothing is settled. Each is measured against the name's own letters that
    /// aren't look-alikes, and against the game font's proportions:
    /// - 0 / O / o: its height against the capitals' and small letters' says o or not; its width for its height says
    ///   0 (narrow) or O (round); the glyph fit must not disagree.
    /// - 1 / l / I: its width for its height and the flag at its top left say 1 (a flag, twice a bar's width) or a
    ///   plain bar; a bar is l when it stands as tall as the name's tall small letters (b d h k), above the capitals,
    ///   and I at the capitals' height, each only with that letter's own measure in the name and a clear lead (the
    ///   game font has no serifs and its l is barely taller than its I: without both measures it stays as read).
    ///
    /// Each decision needs a clear lead, else the character stays as read. Returns the name with the settled
    /// characters and the positions (0-based, among the name's characters) left as read.
    pub fn settle_mask(&self, mask: &Image, text: &str) -> (String, Vec<usize>) {
        let chars: Vec<char> = text.chars().collect();
        let at = look_alike_positions(text);
        if at.is_empty() {
            return (text.to_string(), vec![]);
        }
        let letters: Vec<usize> = (0..chars.len()).filter(|&i| !chars[i].is_whitespace()).collect();
        let boxes = letter_pieces(mask);
        if boxes.len() != letters.len() {
            return (text.to_string(), at);
        }
        let median = |mut v: Vec<f64>| -> Option<f64> {
            if v.is_empty() {
                return None;
            }
            v.sort_by(f64::total_cmp);
            Some(v[v.len() / 2])
        };
        // the heights of the name's letters in a set, the look-alikes left out (they are what is being decided)
        let height_of = |set: &dyn Fn(char) -> bool| {
            median(letters.iter().enumerate().filter(|(_, i)| !at.contains(i) && set(chars[**i])).map(|(k, _)| (boxes[k].3 - boxes[k].1) as f64).collect())
        };
        let capital = |c: char| c.is_ascii_uppercase() && !"OQJI".contains(c);
        // the name's capitals (and digits, and the tall small letters), its capitals alone, its tall small letters
        // alone, and its small letters
        let cap = height_of(&|c: char| capital(c) || ('2'..='9').contains(&c) || "bdhk".contains(c));
        let caps_only = height_of(&capital);
        let tall_small = height_of(&|c: char| "bdhk".contains(c));
        let small = height_of(&|c: char| "acemnrsuvwxz".contains(c));
        let g = |ch: char| self.glyph(ch, 64).map(|g| (g.width as f64, g.height as f64));
        let mut out = chars.clone();
        let mut left = vec![];
        for &i in &at {
            let k = letters.iter().position(|&j| j == i).unwrap();
            let (x0, y0, x1, y1) = boxes[k];
            let part = to_one(&mask.crop_i(x0 as i64, y0 as i64, x1 as i64, y1 as i64));
            let pick = if matches!(chars[i], '0' | 'O' | 'o') {
                cap.and_then(|cap| self.settle_round_one(&part, cap, small, &g))
            } else {
                self.settle_bar_one(&part, caps_only, tall_small, &g)
            };
            match pick {
                Some(p) => out[i] = p,
                None => left.push(i),
            }
        }
        (out.into_iter().collect(), left)
    }

    /// One 0, O or o: see `settle_mask`.
    fn settle_round_one(&self, part: &Image, cap: f64, small: Option<f64>, g: &dyn Fn(char) -> Option<(f64, f64)>) -> Option<char> {
        // the game font's proportions: o's height for the capitals', and width for height of 0, O, o
        let (h, z, big, o) = (g('H')?, g('0')?, g('O')?, g('o')?);
        let small = small.unwrap_or(cap * o.1 / h.1);
        let (rz, rbig) = (z.0 / z.1, big.0 / big.1);
        let (w, hh) = (part.width as f64, part.height as f64);
        // o: a small letter's height; 0 and O: a capital's (a clear lead either way, else left as read)
        let tall = (hh - small).abs() > 2.0 * (hh - cap).abs() && hh > (small + cap) / 2.0;
        let low = (hh - cap).abs() > 2.0 * (hh - small).abs() && hh < (small + cap) / 2.0;
        let pick = if low {
            'o'
        } else if tall {
            // 0 or O: its width for its height, nearer the font's 0 or O by a clear lead
            let r = w / hh.max(1.0);
            let (dz, db) = ((r - rz).abs(), (r - rbig).abs());
            if dz * 2.0 < db {
                '0'
            } else if db * 2.0 < dz {
                'O'
            } else {
                return None;
            }
        } else {
            return None;
        };
        // the glyph fit as the second witness, where shapes differ (the fit is taken at the ink's own size, so O and
        // o are one shape to it): a 0 must fit its narrow shape, an O or o the round one, no worse than the other
        let fit = |c: char| self.shape_fit(part, c, 64);
        let ok = if pick == '0' { fit('0') + 0.02 >= fit('O') } else { fit(pick) + 0.02 >= fit('0') };
        ok.then_some(pick)
    }

    /// One 1, l or I: see `settle_mask`.
    fn settle_bar_one(&self, part: &Image, caps: Option<f64>, tall_small: Option<f64>, g: &dyn Fn(char) -> Option<(f64, f64)>) -> Option<char> {
        let (one, bar) = (g('1')?, g('I')?);
        let (w, hh) = (part.width as f64, part.height as f64);
        if hh < 8.0 {
            return None;
        }
        // its width for its height: the 1's (flag and stem) or a bar's, by a clear lead
        let r = w / hh;
        let (d1, db) = ((r - one.0 / one.1).abs(), (r - bar.0 / bar.1).abs());
        // the flag: in its top fifth, ink further left than anywhere in its bottom half (by a quarter of its width
        // for a 1; a bar's sides are straight)
        let left_of = |y0: usize, y1: usize| (y0..y1).filter_map(|y| (0..part.width).find(|&x| part.data[y * part.width + x] != 0)).min();
        let hu = part.height;
        let (top, low) = (left_of(0, (hu / 5).max(1)), left_of(hu / 2, hu));
        let flag = match (top, low) {
            (Some(t), Some(l)) => l.saturating_sub(t) as f64 / w.max(1.0),
            _ => return None,
        };
        let fit = |c: char| self.shape_fit(part, c, 64);
        if d1 * 2.0 < db && flag >= 0.25 {
            return (fit('1') + 0.02 >= fit('I')).then_some('1');
        }
        if !(db * 2.0 < d1 && flag < 0.15 && fit('I') + 0.02 >= fit('1')) {
            return None;
        }
        // a bar: l as tall as the tall small letters, I as the capitals, both measured in this name, apart by at least
        // two pixels, and the bar nearer one by a clear lead
        let (cap, asc) = (caps?, tall_small?);
        if asc - cap < 2.0 {
            return None;
        }
        let (dl, di) = ((hh - asc).abs(), (hh - cap).abs());
        if dl * 2.0 < di && dl <= 1.0 {
            Some('l')
        } else if di * 2.0 < dl && di <= 1.0 {
            Some('I')
        } else {
            None
        }
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
