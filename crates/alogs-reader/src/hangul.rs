//! Korean player names (Hangul), settled against the game's own fonts. Port of aLogs B16 `bossbar/hangulnames.py`.

use crate::gamefont::GameFont;
use alogs_gamelog::difflib::ratio;
use alogs_pil::ops::{logical_and, logical_or, max_filter, min_filter, multiply};
use alogs_pil::{Filter, Image, Mode};
use serde_json::Value as J;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

pub const INK: u8 = 80;
pub const MIN_P: f64 = 1e-4;
pub const GLYPH_PX: f64 = 64.0;
pub const MIN_FIT: f64 = 0.55;
pub const NOT_A_LETTER: f64 = 0.3;
pub const LATIN_SAME: f64 = 0.8;
pub const SPECK: usize = 5;
pub const SPACE: f64 = 0.6;
pub const LONE_P: f64 = 0.9;
pub const SMALL_CAPS: &str = "cosuvwxz";
pub const LOOKALIKES: [&str; 3] = ["0Oo", "9q", "1lI"];

fn twin(c: &str) -> Option<&'static str> {
    Some(match c {
        "7" => "ㄱ",
        "L" => "ㄴ",
        "2" | "Z" => "ㄹ",
        "0" | "O" | "o" => "ㅇ",
        "T" => "ㅜ",
        "E" => "ㅌ",
        "C" => "ㄷ",
        "l" | "I" | "1" => "ㅣ",
        "-" => "ㅡ",
        "H" => "ㅐ",
        "A" | "V" => "ㅅ",
        "D" => "ㅁ",
        "S" => "ㅈ",
        _ => return None,
    })
}

pub fn is_hangul(c: char) -> bool {
    ('ᄀ'..='ᇿ').contains(&c) || ('㄰'..='㆏').contains(&c) || ('가'..='힣').contains(&c)
}
pub fn has_hangul(t: &str) -> bool {
    t.chars().any(is_hangul)
}
/// `is_hangul(c)` on a string, as Python compares strings (a one-character string in practice).
fn is_hangul_s(s: &str) -> bool {
    let r = |lo: &str, hi: &str| lo <= s && s <= hi;
    r("ᄀ", "ᇿ") || r("㄰", "㆏") || r("가", "힣")
}
fn syllable_s(s: &str) -> bool {
    "가" <= s && s <= "힣"
}

/// The Korean model's letters: `(x, [(character, likelihood), ...])` each.
pub type Letters = Vec<(f64, Vec<(String, f64)>)>;

fn parse_letters(v: &J) -> Option<Letters> {
    v.as_array()?
        .iter()
        .map(|l| {
            let x = l.get(0)?.as_f64()?;
            let picks = l.get(1)?.as_array()?.iter().map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1)?.as_f64()?))).collect::<Option<Vec<_>>>()?;
            Some((x, picks))
        })
        .collect()
}

/// The helper's answer to a name box: (Windows OCR's text, the Korean model's letters, the Latin model's text).
pub fn split_reply(line: &str) -> (String, Option<Letters>, String) {
    let mut f: Vec<&str> = line.split('\t').collect();
    f.resize(f.len().max(3), "");
    let letters = if f[1].is_empty() { None } else { serde_json::from_str::<J>(f[1]).ok().and_then(|v| parse_letters(&v)) };
    (f[0].trim().to_string(), letters, f[2].trim().to_string())
}

/// The Cyrillic model's letters in a helper's answer (its fourth field, there when the box was sent with flag 8).
pub fn cyrillic_letters(line: &str) -> Option<Letters> {
    let f = line.split('\t').nth(3).filter(|f| !f.is_empty())?;
    serde_json::from_str::<J>(f).ok().and_then(|v| parse_letters(&v))
}

pub fn hangul_first(letters: &Option<Letters>) -> bool {
    letters.as_ref().is_some_and(|l| !l.is_empty() && has_hangul(&l.iter().filter_map(|(_, p)| p.first().map(|x| x.0.as_str())).collect::<String>()))
}

pub fn lone_latin(letters: &Option<Letters>) -> String {
    let Some(l) = letters else { return String::new() };
    if l.len() != 1 || l[0].1.is_empty() {
        return String::new();
    }
    let (c, p) = &l[0].1[0];
    let mut it = c.chars();
    match (it.next(), it.next()) {
        (Some(ch), None) if ch.is_ascii_alphanumeric() && *p >= LONE_P => c.clone(),
        _ => String::new(),
    }
}

pub fn looks_hangul(letters: &Option<Letters>) -> bool {
    let Some(l) = letters.as_ref().filter(|l| !l.is_empty()) else { return false };
    let best: Vec<&str> = l.iter().filter_map(|(_, p)| p.first().map(|x| x.0.as_str())).collect();
    let hangul: Vec<&str> = best.iter().copied().filter(|c| is_hangul_s(c)).collect();
    !hangul.is_empty() && 2 * hangul.len() >= best.len() && (hangul.len() >= 2 || hangul.iter().any(|c| syllable_s(c)))
}

fn ink_mask(img: &Image) -> Image {
    img.convert(Mode::L).point(|v| if v < INK { 255.0 } else { 0.0 })
}

/// Each model letter's ink in a tidied box: {letter's index: (its 1-bit shape, (left, top, right, bottom))}, the runs
/// of inked columns going to the nearest letter's spot within reach. None when the box holds no ink.
pub(crate) fn letter_parts(img: &Image, letters: &Letters) -> Option<std::collections::BTreeMap<usize, (Image, (usize, usize, usize, usize))>> {
    let mask = ink_mask(img);
    let rs = runs(&mask);
    if rs.is_empty() || letters.is_empty() {
        return None;
    }
    let xs: Vec<f64> = letters.iter().map(|(x, _)| *x).collect();
    let reach: Vec<f64> = (0..xs.len())
        .map(|i| {
            let near: Vec<f64> = [i.wrapping_sub(1), i + 1].iter().filter(|&&j| j < xs.len()).map(|&j| (xs[j] - xs[i]).abs()).collect();
            0.6 * near.iter().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v)))).unwrap_or(img.height as f64)
        })
        .collect();
    let mut owner: std::collections::BTreeMap<usize, Vec<(usize, usize, usize, usize)>> = Default::default();
    for r in &rs {
        let c = (r.0 + r.1) as f64 / 2.0;
        let mut i = 0;
        for j in 1..xs.len() {
            if (xs[j] - c).abs() < (xs[i] - c).abs() {
                i = j;
            }
        }
        if (xs[i] - c).abs() <= reach[i] {
            owner.entry(i).or_default().push(*r);
        }
    }
    Some(
        owner
            .into_iter()
            .map(|(i, mine)| {
                let b = (
                    mine.iter().map(|r| r.0).min().unwrap(),
                    mine.iter().map(|r| r.2).min().unwrap(),
                    mine.iter().map(|r| r.1).max().unwrap(),
                    mine.iter().map(|r| r.3).max().unwrap(),
                );
                (i, (to_one(&mask.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64)), b))
            })
            .collect(),
    )
}

/// `[(left, right, top, bottom)]` of each run of inked columns.
fn runs(mask: &Image) -> Vec<(usize, usize, usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let col_ink = |x: usize| (0..mask.height).any(|y| mask.data[y * mask.width + x] != 0);
    for x in 0..=mask.width {
        let ink = x < mask.width && col_ink(x);
        if ink && start.is_none() {
            start = Some(x);
        } else if !ink && let Some(s) = start {
            let b = mask.crop_i(s as i64, 0, x as i64, mask.height as i64).getbbox().unwrap();
            out.push((s, x, b.1, b.3));
            start = None;
        }
    }
    out
}

/// How many separate letter-sized pieces of ink a tidied name box holds.
pub fn ink_pieces(img: Option<&Image>) -> usize {
    let Some(img) = img else { return 0 };
    let r = runs(&ink_mask(img));
    let Some(tall) = r.iter().map(|r| r.3 - r.2).max() else { return 0 };
    r.iter().filter(|r| (r.3 - r.2) as f64 >= tall as f64 / 2.0 && r.1 - r.0 >= 2).count()
}

/// The Latin model's read of a name box without a trailing icon read as a circled number or a mark.
pub fn clean_latin(text: &str) -> String {
    let marks = |c: char| ('①'..='⓿').contains(&c) || ('❶'..='➓').contains(&c) || "。｡.,·•…".contains(c);
    let t = text.trim().trim_end_matches(marks).trim();
    let re = regex::Regex::new(r"\s*_\s*").unwrap();
    re.replace_all(t, "_").into_owned()
}

/// A prepared name box as the Korean model and the font check see it: edge-touching dark regions cleared, specks
/// gone, the line nearest the middle cut out on white. None when no ink is left.
pub fn tidy(img: &Image, pad: usize) -> Option<Image> {
    tidy_with(img, pad, SPECK, false)
}

/// `tidy` with specks up to `speck` px gone (thinner strokes go with them); `top_open`: ink touching the box's top
/// edge stays (a box that starts inside a name tab, under the top of its tallest letters).
pub fn tidy_with(img: &Image, pad: usize, speck: usize, top_open: bool) -> Option<Image> {
    let grey = img.convert(Mode::L);
    if img.width <= 2 * pad || img.height <= 2 * pad {
        return None;
    }
    let mask = grey.crop_i(pad as i64, pad as i64, (img.width - pad) as i64, (img.height - pad) as i64).point(|v| if v < INK { 255.0 } else { 0.0 });
    let mut ring = Image::new(Mode::L, mask.width + 2, mask.height + 2, &[255]);
    ring.paste(&mask, 1, 1);
    let corner = if top_open {
        ring.paste(&Image::new(Mode::L, ring.width, 1, &[0]), 0, 0);
        (0, ring.height - 1)
    } else {
        (0, 0)
    };
    alogs_pil::draw::floodfill(&mut ring, corner, &[128]);
    let ink = ring.crop_i(1, 1, mask.width as i64 + 1, mask.height as i64 + 1).point(|v| if v == 255 { 255.0 } else { 0.0 });
    let ink = if speck > 1 { max_filter(&min_filter(&ink, speck), speck) } else { ink };
    let rows: Vec<bool> = (0..ink.height).map(|y| ink.row(y).iter().any(|&v| v != 0)).collect();
    let mut bands: Vec<[usize; 2]> = Vec::new();
    let mut start = None;
    for (y, on) in rows.iter().copied().chain(std::iter::once(false)).enumerate() {
        if on && start.is_none() {
            start = Some(y);
        } else if !on && let Some(s) = start {
            bands.push([s, y]);
            start = None;
        }
    }
    let mut merged: Vec<[usize; 2]> = Vec::new();
    for b in bands {
        if let Some(last) = merged.last_mut()
            && ((b[0] - last[1]) as f64) < ((b[1] - b[0]).max(last[1] - last[0]) as f64) / 3.0
        {
            last[1] = b[1];
            continue;
        }
        merged.push(b);
    }
    let mid = ink.height as f64 / 2.0;
    let mut best: Option<(f64, [usize; 2])> = None;
    for b in &merged {
        let k = ((b[0] + b[1]) as f64 / 2.0 - mid).abs();
        if best.is_none_or(|x| k < x.0) {
            best = Some((k, *b));
        }
    }
    let [top, bottom] = best?.1;
    let mut line = Image::new(Mode::L, ink.width, ink.height, &[0]);
    alogs_pil::draw::rectangle(&mut line, (0.0, top as f64, ink.width as f64 - 1.0, bottom as f64 - 1.0), &[255]);
    let ink = multiply(&ink, &line);
    let b = ink.getbbox()?;
    let p = (((0.35 * (b.3 - b.1) as f64).round_ties_even()) as usize).max(4);
    let mut out = Image::new(Mode::L, b.2 - b.0 + 2 * p, b.3 - b.1 + 2 * p, &[255]);
    let part = ink.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64);
    out.paste_mask(&Image::new(Mode::L, part.width, part.height, &[0]), p as i64, p as i64, Some(&part));
    Some(out)
}

fn to_one(im: &Image) -> Image {
    let mut o = im.clone();
    o.mode = Mode::One;
    for v in o.data.iter_mut() {
        *v = if *v >= 128 { 255 } else { 0 };
    }
    o
}

/// Intersection over union of two same-size 1-bit shapes, at the best small shift.
fn iou(a: &Image, b: &Image, slack: i64) -> f64 {
    let (w, h) = (a.width + 2 * slack as usize, a.height + 2 * slack as usize);
    let mut aa = Image::new(Mode::One, w, h, &[0]);
    aa.paste(a, slack, slack);
    let mut best = 0.0f64;
    for dy in -slack..=slack {
        for dx in -slack..=slack {
            let mut bb = Image::new(Mode::One, w, h, &[0]);
            bb.paste(b, slack + dx, slack + dy);
            let union = logical_or(&aa, &bb).convert(Mode::L).histogram(None)[255];
            if union > 0 {
                best = best.max(logical_and(&aa, &bb).convert(Mode::L).histogram(None)[255] as f64 / union as f64);
            }
        }
    }
    best
}

pub struct KoreanNames {
    pub latin: Option<GameFont>,
    pub korean: Option<GameFont>,
    cache: RefCell<HashMap<String, Option<Image>>>,
    /// The last read's letters and fits, for the log.
    pub last: RefCell<Vec<(String, f64)>>,
}

impl KoreanNames {
    pub fn new(latin: Option<GameFont>, korean: Option<GameFont>) -> KoreanNames {
        KoreanNames { latin, korean, cache: RefCell::new(HashMap::new()), last: RefCell::new(vec![]) }
    }

    fn glyph(&self, ch: &str) -> Option<Image> {
        if let Some(g) = self.cache.borrow().get(ch) {
            return g.clone();
        }
        let mut it = ch.chars();
        let one = match (it.next(), it.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        };
        let font = if one.is_some_and(is_hangul) { &self.korean } else { &self.latin };
        let shape = (|| {
            let c = one?;
            let f = font.as_ref()?;
            if !f.glyphs.contains_key(&(c as u32)) {
                return None;
            }
            let (field, _) = f.field(ch, GLYPH_PX);
            let m = field.point(|v| if v > 120 { 255.0 } else { 0.0 });
            let b = m.getbbox()?;
            Some(to_one(&m.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64)))
        })();
        self.cache.borrow_mut().insert(ch.to_string(), shape.clone());
        shape
    }

    /// How well a letter's ink is this character (0..1).
    pub fn fit(&self, part: &Image, ch: &str) -> f64 {
        let Some(g) = self.glyph(ch) else { return 0.0 };
        let (a, b) = (part.width as f64 / part.height as f64, g.width as f64 / g.height as f64);
        let aspect = a.min(b) / a.max(b);
        iou(part, &g.resize(part.width, part.height, Some(Filter::Nearest)), 1) * aspect
    }

    /// `lone_latin`'s character settled against the game's font among its look-alikes; "" when none fits.
    pub fn lone(&self, img: &Image, letters: &Option<Letters>) -> String {
        let c = lone_latin(letters);
        if c.is_empty() || ink_pieces(Some(img)) != 1 {
            return String::new();
        }
        let mask = ink_mask(img);
        let Some(b) = mask.getbbox() else { return String::new() };
        let part = to_one(&mask.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64));
        let mut cands: BTreeSet<String> = BTreeSet::from([c.clone()]);
        if SMALL_CAPS.contains(&c.to_lowercase()) {
            let sw: String = c.chars().map(|ch| if ch.is_uppercase() { ch.to_ascii_lowercase() } else { ch.to_ascii_uppercase() }).collect();
            cands.insert(sw);
        }
        for g in LOOKALIKES {
            if g.contains(&c) {
                cands.extend(g.chars().map(String::from));
            }
        }
        let mut best: Option<(f64, String)> = None;
        for x in &cands {
            let f = self.fit(&part, x);
            if best.as_ref().is_none_or(|b| f > b.0) {
                best = Some((f, x.clone()));
            }
        }
        let (f, x) = best.unwrap();
        if f >= NOT_A_LETTER { x } else { String::new() }
    }

    /// The name in Hangul when the game's Korean letters fit it, else None.
    pub fn read(&self, img: &Image, text: &str, letters: &Option<Letters>) -> Option<String> {
        self.last.borrow_mut().clear();
        let letters = letters.as_ref().filter(|l| !l.is_empty())?;
        self.korean.as_ref()?;
        let mut cands: Vec<Vec<(String, f64)>> = letters
            .iter()
            .map(|(_, picks)| {
                let v: Vec<(String, f64)> = picks.iter().filter(|p| p.1 >= MIN_P).cloned().collect();
                if v.is_empty() { picks.iter().take(1).cloned().collect() } else { v }
            })
            .collect();
        for picks in cands.iter_mut() {
            if let Some(first) = picks.first()
                && let Some(t) = twin(&first.0)
                && !picks.iter().any(|p| p.0 == t)
            {
                picks.push((t.to_string(), 0.0));
            }
        }
        if !cands.iter().flatten().any(|(c, _)| is_hangul_s(c)) {
            return None;
        }
        let top: String = cands.iter().filter_map(|p| p.first().map(|x| x.0.clone())).collect();
        let plain = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
        if !has_hangul(&top) && !plain(text).is_empty() && ratio(&plain(text), &plain(&top)) >= LATIN_SAME {
            return None;
        }
        let parts = letter_parts(img, letters)?;
        let mut chosen: Vec<(String, f64, (usize, usize, usize, usize))> = Vec::new();
        for (i, picks) in cands.iter().enumerate() {
            let Some((part, b)) = parts.get(&i) else { continue };
            let (part, b) = (part, *b);
            let mut best: Option<(f64, f64, String)> = None;
            for (c, p) in picks {
                let f = self.fit(&part, c);
                if best.as_ref().is_none_or(|x| f + 0.15 * p > x.0 + 0.15 * x.1) {
                    best = Some((f, *p, c.clone()));
                }
            }
            let (fit, _, c) = best.unwrap();
            if fit >= NOT_A_LETTER {
                chosen.push((c, fit, b));
            }
        }
        *self.last.borrow_mut() = chosen.iter().map(|(c, f, _)| (c.clone(), (f * 100.0).round_ties_even() / 100.0)).collect();
        if chosen.is_empty() {
            return None;
        }
        let mut heights: Vec<usize> = chosen.iter().map(|(_, _, b)| b.3 - b.1).collect();
        heights.sort_unstable();
        let wide = SPACE * heights[heights.len() / 2] as f64;
        let mut words: Vec<Vec<&(String, f64, (usize, usize, usize, usize))>> = vec![vec![&chosen[0]]];
        for w in chosen.windows(2) {
            if (w[1].2.0 as f64 - w[0].2.2 as f64) > wide {
                words.push(vec![]);
            }
            words.last_mut().unwrap().push(&w[1]);
        }
        let korean = |word: &Vec<&(String, f64, (usize, usize, usize, usize))>| {
            let hangul: Vec<(&String, f64)> = word.iter().filter(|(c, _, _)| is_hangul_s(c)).map(|(c, f, _)| (c, *f)).collect();
            !hangul.is_empty()
                && hangul.iter().all(|(_, f)| *f >= MIN_FIT)
                && (hangul.len() >= 2 || syllable_s(hangul[0].0))
        };
        let text_of = |word: &Vec<&(String, f64, (usize, usize, usize, usize))>| word.iter().map(|(c, _, _)| c.as_str()).collect::<String>();
        if !words.iter().any(korean) {
            return None;
        }
        let badge = regex::Regex::new(r"^[1-4][Pp]$").unwrap();
        let keep: Vec<String> = words.iter().filter(|w| korean(w) || badge.is_match(&text_of(w))).map(text_of).collect();
        Some(keep.join(" "))
    }
}
