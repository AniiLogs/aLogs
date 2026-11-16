//! Reading the boss bar's segment counter (x21 ... x1) by matching digit shapes instead of OCR.
//! Port of aLogs B16 `bossbar/xn_digits.py`.

use alogs_pil::ops::{darker, lighter, subtract};
use alogs_pil::{Filter, Image, Mode};
use indexmap::IndexMap;

pub const GW: usize = 12;
pub const GH: usize = 18;
pub const SMALL_H: usize = 30;
pub const READ_H: usize = 30;

/// Digit -> template vector (xn_templates.json), in the file's order.
pub type Templates = IndexMap<String, Vec<f64>>;

pub fn load_templates(res: &std::path::Path) -> Templates {
    std::fs::read_to_string(res.join("xn_templates.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// `mask_bytes`: (width, height, one 0/1 byte a pixel) of the near-white, unsaturated text.
pub fn mask_bytes(img: &Image) -> (usize, usize, Vec<u8>) {
    let mut s = img.convert(Mode::Rgb).split().into_iter();
    let (r, g, b) = (s.next().unwrap(), s.next().unwrap(), s.next().unwrap());
    let lo = darker(&darker(&r, &g), &b);
    let hi = lighter(&lighter(&r, &g), &b);
    let m = darker(
        &lo.point(|v| if v > 165 { 1.0 } else { 0.0 }),
        &subtract(&hi, &lo, 1.0, 0).point(|v| if v < 60 { 1.0 } else { 0.0 }),
    );
    (img.width, img.height, m.tobytes())
}

type Mask = Vec<Vec<u8>>;

pub fn white_mask(img: &Image) -> Mask {
    let (w, h, d) = mask_bytes(img);
    (0..h).map(|y| d[y * w..(y + 1) * w].to_vec()).collect()
}

/// Glyphs split by empty columns: `(x0, x1, y0, y1)`, specks dropped.
pub fn glyphs(mask: &Mask) -> Vec<(usize, usize, usize, usize)> {
    let h = mask.len();
    let w = if h > 0 { mask[0].len() } else { 0 };
    let cols: Vec<u32> = (0..w).map(|x| (0..h).map(|y| u32::from(mask[y][x])).sum()).collect();
    let mut out = Vec::new();
    let mut x = 0;
    while x < w {
        if cols[x] != 0 {
            let s = x;
            while x < w && cols[x] != 0 {
                x += 1;
            }
            let ys: Vec<usize> = (0..h).filter(|&y| (s..x).any(|c| mask[y][c] != 0)).collect();
            out.push((s, x, ys[0], ys[ys.len() - 1] + 1));
        }
        x += 1;
    }
    let tall = out.iter().map(|g| g.3 - g.2).max().unwrap_or(0);
    out.into_iter().filter(|g| (g.3 - g.2) as f64 >= 0.35 * tall as f64 && g.1 - g.0 >= 2).collect()
}

pub fn normalize(mask: &Mask, g: (usize, usize, usize, usize)) -> Vec<f64> {
    let (x0, x1, y0, y1) = g;
    let mut data = Vec::with_capacity((x1 - x0) * (y1 - y0));
    for row in &mask[y0..y1] {
        data.extend(row[x0..x1].iter().map(|&v| 255 * v));
    }
    let im = Image::from_raw(Mode::L, x1 - x0, y1 - y0, data).resize(GW, GH, Some(Filter::Bilinear));
    im.data.iter().map(|&v| f64::from(v) / 255.0).collect()
}

/// Glyphs that touch once scaled (one shape wider than tall) cut at their thinnest middle column.
pub fn split_touching(mask: &Mask, gs: &[(usize, usize, usize, usize)]) -> Vec<(usize, usize, usize, usize)> {
    let mut out = Vec::new();
    for &g in gs {
        let mut parts = vec![g];
        while let Some(&last) = parts.last() {
            if !((last.1 - last.0) as f64 > 1.05 * (last.3 - last.2) as f64 && parts.len() < 4) {
                break;
            }
            let (x0, x1, y0, y1) = parts.pop().unwrap();
            let w = x1 - x0;
            let lo = x0 + ((w as f64 * 0.3) as usize).max(2);
            let hi = x1 as i64 - ((w as f64 * 0.3) as i64).max(2);
            if hi <= lo as i64 {
                parts.push((x0, x1, y0, y1));
                break;
            }
            // min() keeps the first of equals
            let col = |x: usize| (y0..y1).map(|y| u32::from(mask[y][x])).sum::<u32>();
            let cut = (lo..hi as usize).min_by_key(|&x| col(x)).unwrap();
            for (a, b) in [(x0, cut), (cut + 1, x1)] {
                let ys: Vec<usize> = (y0..y1).filter(|&y| (a..b).any(|c| mask[y][c] != 0)).collect();
                if !ys.is_empty() && b >= a + 2 {
                    parts.push((a, b, ys[0], ys[ys.len() - 1] + 1));
                }
            }
        }
        out.extend(parts);
    }
    out
}

/// Glyph vectors of the digits after the "x".
pub fn split_counter(img: &Image) -> Vec<Vec<f64>> {
    let m = white_mask(img);
    let gs = split_touching(&m, &glyphs(&m));
    if gs.len() < 2 {
        return vec![];
    }
    let tall = gs.iter().map(|g| g.3 - g.2).max().unwrap();
    gs[1..]
        .iter()
        .filter(|g| (g.3 - g.2) as f64 >= 0.8 * tall as f64)
        .map(|&g| normalize(&m, g))
        .take(2)
        .collect()
}

/// `(best digit, its distance, the second best's distance)`.
pub fn best_match(vec: &[f64], tpl: &Templates) -> (Option<String>, f64, f64) {
    let (mut best, mut bd, mut sd) = (None, 1e9, 1e9);
    for (d, t) in tpl {
        let mut s = 0.0;
        for (a, b) in vec.iter().zip(t) {
            s += (a - b).abs();
        }
        let dist = s / vec.len() as f64;
        if dist < bd {
            sd = bd;
            best = Some(d.clone());
            bd = dist;
        } else if dist < sd {
            sd = dist;
        }
    }
    (best, bd, sd)
}

/// The counter value (1 up to `top`) or None when unsure.
pub fn read_counter(img: &Image, tpl: &Templates, max_dist: f64, top: i64) -> Option<i64> {
    if tpl.is_empty() {
        return None;
    }
    let img = if img.height < SMALL_H {
        img.resize(((img.width as f64 * READ_H as f64 / img.height as f64).round_ties_even() as usize).max(1), READ_H, Some(Filter::Bicubic))
    } else {
        img.clone()
    };
    let ds = split_counter(&img);
    if ds.is_empty() {
        return None;
    }
    let mut s = String::new();
    for v in &ds {
        let (d, bd, sd) = best_match(v, tpl);
        let d = d?;
        if bd > max_dist || sd - bd < 0.03 {
            return None;
        }
        s += &d;
    }
    let n: i64 = s.parse().ok()?;
    (1..=top).contains(&n).then_some(n)
}

/// `read_counter`, worked out again only when the counter's white text changed (the last mask and answer kept).
#[derive(Default)]
pub struct CounterCache {
    pub reads: u64,
    pub same: u64,
    last: Option<((usize, usize, Vec<u8>), i64, u64, Option<i64>)>,
}

impl CounterCache {
    pub fn read(&mut self, img: &Image, tpl: &Templates, top: i64, max_dist: f64) -> Option<i64> {
        if tpl.is_empty() {
            return read_counter(img, tpl, max_dist, top);
        }
        let img = if img.height < SMALL_H {
            img.resize(((img.width as f64 * READ_H as f64 / img.height as f64).round_ties_even() as usize).max(1), READ_H, Some(Filter::Bicubic))
        } else {
            img.clone()
        };
        let key = (mask_bytes(&img), top, max_dist.to_bits());
        self.reads += 1;
        if let Some(l) = &self.last
            && l.0 == key.0
            && l.1 == key.1
            && l.2 == key.2
        {
            self.same += 1;
            return l.3;
        }
        let value = read_counter(&img, tpl, max_dist, top);
        self.last = Some((key.0, key.1, key.2, value));
        value
    }
}
