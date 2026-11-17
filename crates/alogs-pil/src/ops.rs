//! ImageChops (Chops.c), ImageOps (invert, autocontrast, expand), ImageFilter (rank and Gaussian blur: RankFilter.c,
//! BoxBlur.c, Filter.c's expand) and ImageStat, as Pillow 12.3 computes them.

use crate::image::{Image, Mode};

// ---------------------------------------------------------------- ImageChops

/// Both images' common size, in the first one's mode (`create` in Chops.c).
fn chop(a: &Image, b: &Image, f: impl Fn(u8, u8) -> i32, clip: bool) -> Image {
    assert_eq!(a.bands(), b.bands(), "images do not match");
    let (w, h) = (a.width.min(b.width), a.height.min(b.height));
    let n = a.bands();
    let mut out = Image::new(a.mode, w, h, &[]);
    for y in 0..h {
        for x in 0..w * n {
            let v = f(a.data[y * a.width * n + x], b.data[y * b.width * n + x]);
            out.data[y * w * n + x] = if clip { v.clamp(0, 255) as u8 } else { v as u8 };
        }
    }
    out
}

pub fn lighter(a: &Image, b: &Image) -> Image {
    chop(a, b, |x, y| i32::from(x.max(y)), true)
}
pub fn darker(a: &Image, b: &Image) -> Image {
    chop(a, b, |x, y| i32::from(x.min(y)), true)
}
pub fn difference(a: &Image, b: &Image) -> Image {
    chop(a, b, |x, y| (i32::from(x) - i32::from(y)).abs(), true)
}
pub fn multiply(a: &Image, b: &Image) -> Image {
    chop(a, b, |x, y| i32::from(x) * i32::from(y) / 255, true)
}
pub fn screen(a: &Image, b: &Image) -> Image {
    chop(a, b, |x, y| 255 - (255 - i32::from(x)) * (255 - i32::from(y)) / 255, true)
}
/// `ImageChops.add(a, b, scale, offset)`.
pub fn add(a: &Image, b: &Image, scale: f32, offset: i32) -> Image {
    chop(a, b, |x, y| ((i32::from(x) + i32::from(y)) as f32 / scale + offset as f32) as i32, true)
}
/// `ImageChops.subtract(a, b, scale, offset)`.
pub fn subtract(a: &Image, b: &Image, scale: f32, offset: i32) -> Image {
    chop(a, b, |x, y| ((i32::from(x) - i32::from(y)) as f32 / scale + offset as f32) as i32, true)
}
pub fn logical_and(a: &Image, b: &Image) -> Image {
    assert!(a.mode == Mode::One && b.mode == Mode::One, "image has wrong mode");
    chop(a, b, |x, y| if x != 0 && y != 0 { 255 } else { 0 }, false)
}
pub fn logical_or(a: &Image, b: &Image) -> Image {
    assert!(a.mode == Mode::One && b.mode == Mode::One, "image has wrong mode");
    chop(a, b, |x, y| if x != 0 || y != 0 { 255 } else { 0 }, false)
}
/// `ImageChops.invert`.
pub fn chop_invert(a: &Image) -> Image {
    let mut out = a.clone();
    for v in out.data.iter_mut() {
        *v = 255 - *v;
    }
    out
}

// ---------------------------------------------------------------- ImageOps

/// `ImageOps.invert` (a "1" image goes through `point`, L and RGB through their table).
pub fn invert(im: &Image) -> Image {
    assert!(matches!(im.mode, Mode::One | Mode::L | Mode::Rgb), "not supported for mode {}", im.mode.name());
    im.point(|v| f64::from(255 - v))
}

/// `ImageOps.autocontrast(image, cutoff)` (cutoff in percent at each end; `ignore`/`mask` not used by aLogs).
pub fn autocontrast(im: &Image, cutoff: (f64, f64)) -> Image {
    assert!(matches!(im.mode, Mode::L | Mode::Rgb), "not supported for mode {}", im.mode.name());
    let hist = im.histogram(None);
    let mut lut: Vec<i64> = Vec::with_capacity(hist.len());
    for layer in hist.chunks(256) {
        let mut h: Vec<i64> = layer.iter().map(|&v| v as i64).collect();
        if cutoff != (0.0, 0.0) {
            let n: i64 = h.iter().sum();
            // int(n * cutoff // 100): integer floor division when the cutoff is a whole number
            let cut_of = |c: f64| -> i64 {
                if c.fract() == 0.0 { (n * c as i64).div_euclid(100) } else { ((n as f64 * c) / 100.0).floor() as i64 }
            };
            let mut cut = cut_of(cutoff.0);
            for lo in 0..256 {
                if cut > h[lo] {
                    cut -= h[lo];
                    h[lo] = 0;
                } else {
                    h[lo] -= cut;
                    cut = 0;
                }
                if cut <= 0 {
                    break;
                }
            }
            let mut cut = cut_of(cutoff.1);
            for hi in (0..256).rev() {
                if cut > h[hi] {
                    cut -= h[hi];
                    h[hi] = 0;
                } else {
                    h[hi] -= cut;
                    cut = 0;
                }
                if cut <= 0 {
                    break;
                }
            }
        }
        let lo = (0..256).find(|&i| h[i] != 0).unwrap_or(255);
        let hi = (0..256).rev().find(|&i| h[i] != 0).unwrap_or(0);
        if hi <= lo {
            lut.extend(0..256);
        } else {
            let scale = 255.0 / (hi - lo) as f64;
            let offset = -(lo as f64) * scale;
            for ix in 0..256 {
                lut.push(((ix as f64 * scale + offset) as i64).clamp(0, 255));
            }
        }
    }
    im.point_table(&lut)
}

/// `ImageOps.expand(image, border, fill)`.
pub fn expand(im: &Image, border: usize, fill: &[u8]) -> Image {
    let mut out = Image::new(im.mode, im.width + 2 * border, im.height + 2 * border, fill);
    out.paste(im, border as i64, border as i64);
    out
}

// ---------------------------------------------------------------- ImageFilter

/// `ImagingExpand`: the edges repeated `margin` pixels out.
fn expand_edges(im: &Image, margin: usize) -> Image {
    let (w, h) = (im.width + 2 * margin, im.height + 2 * margin);
    let b = im.bands();
    let mut out = Image::new(im.mode, w, h, &[]);
    for y in 0..h {
        let sy = y.saturating_sub(margin).min(im.height - 1);
        for x in 0..w {
            let sx = x.saturating_sub(margin).min(im.width - 1);
            let s = (sy * im.width + sx) * b;
            out.data[(y * w + x) * b..(y * w + x + 1) * b].copy_from_slice(&im.data[s..s + b]);
        }
    }
    out
}

/// `RankFilter(size, rank)` on each band (`MaxFilter`: rank size*size-1, `MinFilter`: rank 0).
pub fn rank_filter(im: &Image, size: usize, rank: usize) -> Image {
    assert!(size % 2 == 1, "bad filter size");
    let bands = im.split();
    let done: Vec<Image> = bands
        .iter()
        .map(|band| {
            let e = expand_edges(band, size / 2);
            let mut out = Image::new(band.mode, band.width, band.height, &[]);
            let mut win = Vec::with_capacity(size * size);
            for y in 0..band.height {
                for x in 0..band.width {
                    win.clear();
                    for yy in 0..size {
                        win.extend_from_slice(&e.data[(y + yy) * e.width + x..(y + yy) * e.width + x + size]);
                    }
                    win.select_nth_unstable(rank);
                    out.data[y * band.width + x] = win[rank];
                }
            }
            out
        })
        .collect();
    if done.len() == 1 { done.into_iter().next().unwrap() } else { Image::merge(im.mode, &done) }
}

pub fn max_filter(im: &Image, size: usize) -> Image {
    rank_filter(im, size, size * size - 1)
}
pub fn min_filter(im: &Image, size: usize) -> Image {
    rank_filter(im, size, 0)
}

fn gaussian_blur_radius(radius: f32, passes: i32) -> f32 {
    let sigma2: f32 = radius * radius / passes as f32;
    let big_l: f32 = (12.0 * f64::from(sigma2) + 1.0).sqrt() as f32;
    let l: f32 = ((f64::from(big_l) - 1.0) / 2.0).floor() as f32;
    let mut a: f32 = (2.0 * l + 1.0) * (l * (l + 1.0) - 3.0 * sigma2);
    a /= 6.0 * (sigma2 - (l + 1.0) * (l + 1.0));
    l + a
}

/// One row of `ImagingLineBoxBlur8` (`step` apart within `data`, `n` long).
#[allow(clippy::too_many_arguments)]
fn line_box_blur(input: &[u8], out: &mut [u8], lastx: usize, radius: usize, edge_a: usize, edge_b: usize, ww: u32, fw: u32) {
    let at = |i: usize| u32::from(input[i]);
    let mut acc: u32 = at(0).wrapping_mul(radius as u32 + 1);
    for x in 0..edge_a.saturating_sub(1) {
        acc = acc.wrapping_add(at(x));
    }
    acc = acc.wrapping_add(at(lastx).wrapping_mul((radius as i64 - edge_a as i64 + 1) as u32));
    let mut step = |acc: &mut u32, x: usize, sub: usize, add: usize, left: usize, right: usize| {
        *acc = acc.wrapping_add(at(add)).wrapping_sub(at(sub));
        let bulk = acc.wrapping_mul(ww).wrapping_add((at(left) + at(right)).wrapping_mul(fw));
        out[x] = (bulk.wrapping_add(1 << 23) >> 24) as u8;
    };
    if edge_a <= edge_b {
        for x in 0..edge_a {
            step(&mut acc, x, 0, x + radius, 0, x + radius + 1);
        }
        for x in edge_a..edge_b {
            step(&mut acc, x, x - radius - 1, x + radius, x - radius - 1, x + radius + 1);
        }
        for x in edge_b..=lastx {
            step(&mut acc, x, x - radius - 1, lastx, x - radius - 1, lastx);
        }
    } else {
        for x in 0..edge_b {
            step(&mut acc, x, 0, x + radius, 0, x + radius + 1);
        }
        for x in edge_b..edge_a {
            step(&mut acc, x, 0, lastx, 0, lastx);
        }
        for x in edge_a..=lastx {
            step(&mut acc, x, x - radius - 1, lastx, x - radius - 1, lastx);
        }
    }
}

/// `ImagingHorizontalBoxBlur` on every band.
fn horizontal_box_blur(im: &mut Image, radius_f: f32) {
    let radius = radius_f as usize;
    let ww = (16_777_216f32 / (radius_f * 2.0 + 1.0)) as u32;
    let fw = (16_777_216u32.wrapping_sub((radius as u32 * 2 + 1).wrapping_mul(ww))) / 2;
    let w = im.width;
    let edge_a = (radius + 1).min(w);
    let edge_b = (w as i64 - radius as i64 - 1).max(0) as usize;
    let b = im.bands();
    let mut line = vec![0u8; w];
    let mut out = vec![0u8; w];
    for y in 0..im.height {
        for c in 0..b {
            for x in 0..w {
                line[x] = im.data[(y * w + x) * b + c];
            }
            line_box_blur(&line, &mut out, w - 1, radius, edge_a, edge_b, ww, fw);
            for x in 0..w {
                im.data[(y * w + x) * b + c] = out[x];
            }
        }
    }
}

/// `ImageFilter.GaussianBlur(radius)` (three box blurs).
pub fn gaussian_blur(im: &Image, radius: f64) -> Image {
    assert!(matches!(im.mode, Mode::L | Mode::Rgb | Mode::Rgba), "image has wrong mode");
    if radius == 0.0 || im.width == 0 || im.height == 0 {
        return im.clone();
    }
    let r = gaussian_blur_radius(radius as f32, 3);
    let mut out = im.clone();
    if r != 0.0 {
        for _ in 0..3 {
            horizontal_box_blur(&mut out, r);
        }
        let mut t = out.transpose(crate::Transpose::Transpose);
        for _ in 0..3 {
            horizontal_box_blur(&mut t, r);
        }
        out = t.transpose(crate::Transpose::Transpose);
    }
    out
}

// ---------------------------------------------------------------- ImageStat

/// `ImageStat.Stat(image, mask)`: the per-band statistics, from the histogram.
pub struct Stat {
    pub h: Vec<u64>,
}

impl Stat {
    pub fn new(im: &Image, mask: Option<&Image>) -> Stat {
        Stat { h: im.histogram(mask) }
    }
    fn layers(&self) -> impl Iterator<Item = &[u64]> {
        self.h.chunks(256)
    }
    pub fn count(&self) -> Vec<u64> {
        self.layers().map(|l| l.iter().sum()).collect()
    }
    pub fn sum(&self) -> Vec<f64> {
        self.layers().map(|l| l.iter().enumerate().fold(0.0, |s, (j, &n)| s + (j as u64 * n) as f64)).collect()
    }
    pub fn sum2(&self) -> Vec<f64> {
        self.layers().map(|l| l.iter().enumerate().fold(0.0, |s, (j, &n)| s + (j * j) as f64 * n as f64)).collect()
    }
    pub fn mean(&self) -> Vec<f64> {
        let (s, c) = (self.sum(), self.count());
        s.iter().zip(&c).map(|(s, &c)| if c > 0 { s / c as f64 } else { 0.0 }).collect()
    }
    pub fn median(&self) -> Vec<usize> {
        self.layers()
            .map(|l| {
                let half = l.iter().sum::<u64>() / 2;
                let mut s = 0;
                for (j, &n) in l.iter().enumerate() {
                    s += n;
                    if s > half {
                        return j;
                    }
                }
                255
            })
            .collect()
    }
    pub fn rms(&self) -> Vec<f64> {
        let (s2, c) = (self.sum2(), self.count());
        s2.iter().zip(&c).map(|(s, &c)| if c > 0 { (s / c as f64).sqrt() } else { 0.0 }).collect()
    }
    pub fn var(&self) -> Vec<f64> {
        let (s, s2, c) = (self.sum(), self.sum2(), self.count());
        (0..c.len()).map(|i| if c[i] > 0 { (s2[i] - s[i].powf(2.0) / c[i] as f64) / c[i] as f64 } else { 0.0 }).collect()
    }
    pub fn stddev(&self) -> Vec<f64> {
        self.var().iter().map(|v| v.sqrt()).collect()
    }
    pub fn extrema(&self) -> Vec<(usize, usize)> {
        self.layers()
            .map(|l| {
                let lo = (0..256).find(|&i| l[i] != 0).unwrap_or(255);
                let hi = (0..256).rev().find(|&i| l[i] != 0).unwrap_or(0);
                (lo, hi)
            })
            .collect()
    }
}
