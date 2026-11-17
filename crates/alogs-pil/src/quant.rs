//! `quantize(colors, method=MEDIANCUT)` for an RGB image, as aLogs uses it: the palette (each box's mean colour) and
//! each pixel's box.
//!
//! Known difference: Pillow's median cut (Quant.c) orders equal-valued colours by its hash table's iteration order
//! when it splits a box, so a box's members (and so its mean) can differ by a colour or two from this one. aLogs only
//! takes the commonest box's mean colour as a card's background (namereader.ink_colour).

use crate::image::{Image, Mode};
use std::collections::HashMap;

pub struct Quantized {
    /// RGB palette, one entry a box.
    pub palette: Vec<[u8; 3]>,
    /// Each pixel's palette index.
    pub indexes: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

impl Quantized {
    /// `getcolors()`: `(count, index)` for every index used.
    pub fn getcolors(&self) -> Vec<(u64, usize)> {
        let mut c = vec![0u64; self.palette.len()];
        for &i in &self.indexes {
            c[i as usize] += 1;
        }
        c.iter().enumerate().filter(|(_, n)| **n > 0).map(|(i, &n)| (n, i)).collect()
    }
}

struct BoxN {
    colors: Vec<([u8; 3], u64)>,
}

impl BoxN {
    fn count(&self) -> u64 {
        self.colors.iter().map(|c| c.1).sum()
    }
    fn widest(&self) -> (usize, u8) {
        let mut best = (0, 0u8);
        for ch in 0..3 {
            let lo = self.colors.iter().map(|c| c.0[ch]).min().unwrap_or(0);
            let hi = self.colors.iter().map(|c| c.0[ch]).max().unwrap_or(0);
            if hi - lo > best.1 {
                best = (ch, hi - lo);
            }
        }
        best
    }
}

pub fn quantize(im: &Image, colors: usize) -> Quantized {
    let rgb = if im.mode == Mode::Rgb { im.clone() } else { im.convert(Mode::Rgb) };
    let mut counts: HashMap<[u8; 3], u64> = HashMap::new();
    for p in rgb.data.chunks(3) {
        *counts.entry([p[0], p[1], p[2]]).or_default() += 1;
    }
    let mut all: Vec<([u8; 3], u64)> = counts.into_iter().collect();
    all.sort();
    let mut boxes = vec![BoxN { colors: all }];
    while boxes.len() < colors {
        // split the box with the most pixels that can still be split
        let Some(i) = (0..boxes.len()).filter(|&i| boxes[i].colors.len() > 1).max_by_key(|&i| (boxes[i].count(), std::cmp::Reverse(i)))
        else {
            break;
        };
        let (ch, _) = boxes[i].widest();
        let mut b = std::mem::take(&mut boxes[i].colors);
        b.sort_by_key(|c| (c.0[ch], c.0));
        let total: u64 = b.iter().map(|c| c.1).sum();
        let mut acc = 0;
        let mut cut = 1;
        for (k, c) in b.iter().enumerate() {
            acc += c.1;
            if acc * 2 >= total {
                cut = (k + 1).min(b.len() - 1).max(1);
                break;
            }
        }
        let right = b.split_off(cut);
        boxes[i].colors = b;
        boxes.push(BoxN { colors: right });
    }
    let mut palette = Vec::with_capacity(boxes.len());
    let mut index: HashMap<[u8; 3], u8> = HashMap::new();
    for (k, bx) in boxes.iter().enumerate() {
        let n = bx.count().max(1);
        let mut s = [0u64; 3];
        for (c, cnt) in &bx.colors {
            for ch in 0..3 {
                s[ch] += u64::from(c[ch]) * cnt;
            }
            index.insert(*c, k as u8);
        }
        palette.push([((s[0] + n / 2) / n) as u8, ((s[1] + n / 2) / n) as u8, ((s[2] + n / 2) / n) as u8]);
    }
    let indexes = rgb.data.chunks(3).map(|p| index[&[p[0], p[1], p[2]]]).collect();
    Quantized { palette, indexes, width: rgb.width, height: rgb.height }
}
