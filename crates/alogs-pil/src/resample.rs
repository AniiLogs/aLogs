//! `Image.resize`, `reduce` and `thumbnail` (libImaging/Resample.c, Reduce.c; Image.py), and the affine transforms
//! and rotations (Geometry.c).

use crate::convert::{premultiply, unpremultiply};
use crate::image::{Image, Mode, Transpose};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Box,
    Bilinear,
    Hamming,
    Bicubic,
    Lanczos,
}

impl Filter {
    fn support(self) -> f64 {
        match self {
            Filter::Nearest => 0.0,
            Filter::Box => 0.5,
            Filter::Bilinear | Filter::Hamming => 1.0,
            Filter::Bicubic => 2.0,
            Filter::Lanczos => 3.0,
        }
    }
    fn eval(self, x: f64) -> f64 {
        match self {
            Filter::Box => {
                if x > -0.5 && x <= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Filter::Bilinear => {
                let x = x.abs();
                if x < 1.0 { 1.0 - x } else { 0.0 }
            }
            Filter::Hamming => {
                let x = x.abs();
                if x == 0.0 {
                    1.0
                } else if x >= 1.0 {
                    0.0
                } else {
                    let x = x * std::f64::consts::PI;
                    x.sin() / x * (f64::from(0.54f32) + f64::from(0.46f32) * x.cos())
                }
            }
            Filter::Bicubic => {
                let a = -0.5;
                let x = x.abs();
                if x < 1.0 {
                    ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
                } else if x < 2.0 {
                    (((x - 5.0) * x + 8.0) * x - 4.0) * a
                } else {
                    0.0
                }
            }
            Filter::Lanczos => {
                if (-3.0..3.0).contains(&x) {
                    sinc(x) * sinc(x / 3.0)
                } else {
                    0.0
                }
            }
            Filter::Nearest => 0.0,
        }
    }
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let x = x * std::f64::consts::PI;
    x.sin() / x
}

const PRECISION_BITS: u32 = 32 - 8 - 2;

/// `precompute_coeffs` + `normalize_coeffs_8bpc`: (ksize, bounds, fixed-point coefficients).
fn coeffs(in_size: usize, in0: f32, in1: f32, out_size: usize, f: Filter) -> (usize, Vec<(usize, usize)>, Vec<i32>) {
    let scale = f64::from(in1 - in0) / out_size as f64;
    let filterscale = scale.max(1.0);
    let support = f.support() * filterscale;
    let ksize = support.ceil() as usize * 2 + 1;
    let mut kk = vec![0f64; out_size * ksize];
    let mut bounds = Vec::with_capacity(out_size);
    let inv = 1.0 / filterscale;
    for xx in 0..out_size {
        let center = f64::from(in0) + (xx as f64 + 0.5) * scale;
        let mut ww = 0.0;
        let xmin = ((center - support + 0.5) as i64).max(0);
        let xmax = ((center + support + 0.5) as i64).min(in_size as i64) - xmin;
        let k = &mut kk[xx * ksize..(xx + 1) * ksize];
        for x in 0..xmax.max(0) as usize {
            let w = f.eval((x as f64 + xmin as f64 - center + 0.5) * inv);
            k[x] = w;
            ww += w;
        }
        if ww != 0.0 {
            for v in k.iter_mut().take(xmax.max(0) as usize) {
                *v /= ww;
            }
        }
        bounds.push((xmin as usize, xmax.max(0) as usize));
    }
    let fixed = kk
        .iter()
        .map(|&v| {
            if v < 0.0 {
                (-0.5 + v * f64::from(1u32 << PRECISION_BITS)) as i32
            } else {
                (0.5 + v * f64::from(1u32 << PRECISION_BITS)) as i32
            }
        })
        .collect();
    (ksize, bounds, fixed)
}

#[inline]
fn clip_fixed(v: i32) -> u8 {
    (v >> PRECISION_BITS).clamp(0, 255) as u8
}

fn resample_h(im: &Image, y0: usize, rows: usize, w: usize, ksize: usize, bounds: &[(usize, usize)], kk: &[i32]) -> Image {
    let b = im.bands();
    let mut out = Image::new(im.mode, w, rows, &[]);
    for yy in 0..rows {
        let src = im.row(yy + y0);
        for xx in 0..w {
            let (xmin, xmax) = bounds[xx];
            let k = &kk[xx * ksize..];
            for c in 0..b {
                let mut ss: i32 = 1 << (PRECISION_BITS - 1);
                for x in 0..xmax {
                    ss = ss.wrapping_add(i32::from(src[(x + xmin) * b + c]).wrapping_mul(k[x]));
                }
                out.data[(yy * w + xx) * b + c] = clip_fixed(ss);
            }
        }
    }
    out
}

fn resample_v(im: &Image, h: usize, ksize: usize, bounds: &[(usize, usize)], kk: &[i32]) -> Image {
    let b = im.bands();
    let w = im.width;
    let mut out = Image::new(im.mode, w, h, &[]);
    for yy in 0..h {
        let (ymin, ymax) = bounds[yy];
        let k = &kk[yy * ksize..];
        for xx in 0..w {
            for c in 0..b {
                let mut ss: i32 = 1 << (PRECISION_BITS - 1);
                for y in 0..ymax {
                    ss = ss.wrapping_add(i32::from(im.data[((y + ymin) * w + xx) * b + c]).wrapping_mul(k[y]));
                }
                out.data[(yy * w + xx) * b + c] = clip_fixed(ss);
            }
        }
    }
    out
}

/// `ImagingResample` (two passes, horizontal first).
fn resample(im: &Image, w: usize, h: usize, f: Filter, bx: [f32; 4]) -> Image {
    let need_h = w != im.width || bx[0] != 0.0 || bx[2] != w as f32;
    let need_v = h != im.height || bx[1] != 0.0 || bx[3] != h as f32;
    let (ks_v, mut bounds_v, kk_v) = coeffs(im.height, bx[1], bx[3], h, f);
    let ybox_first = bounds_v[0].0;
    let ybox_last = bounds_v[h - 1].0 + bounds_v[h - 1].1;
    let mut cur = None;
    if need_h {
        let (ks_h, bounds_h, kk_h) = coeffs(im.width, bx[0], bx[2], w, f);
        for b in bounds_v.iter_mut() {
            b.0 -= ybox_first;
        }
        cur = Some(resample_h(im, ybox_first, ybox_last - ybox_first, w, ks_h, &bounds_h, &kk_h));
    }
    if need_v {
        let src = cur.as_ref().unwrap_or(im);
        return resample_v(src, h, ks_v, &bounds_v, &kk_v);
    }
    cur.unwrap_or_else(|| im.clone())
}

/// `ImagingScaleAffine`: nearest-neighbour scaling (the NEAREST resize path).
fn scale_affine(im: &Image, w: usize, h: usize, a: [f64; 6], fill: &mut Image) {
    let b = im.bands();
    let mut xintab = vec![0usize; w];
    let mut xo = a[2] + a[0] * 0.5;
    let mut yo = a[5] + a[4] * 0.5;
    let (mut xmin, mut xmax) = (w, 0);
    for (x, slot) in xintab.iter_mut().enumerate() {
        let xin = coord(xo);
        if xin >= 0 && (xin as usize) < im.width {
            xmax = x + 1;
            xmin = xmin.min(x);
            *slot = xin as usize;
        }
        xo += a[0];
    }
    for y in 0..h {
        let yi = coord(yo);
        if yi >= 0 && (yi as usize) < im.height {
            for x in xmin..xmax {
                let s = (yi as usize * im.width + xintab[x]) * b;
                let d = (y * w + x) * b;
                fill.data[d..d + b].copy_from_slice(&im.data[s..s + b]);
            }
        }
        yo += a[4];
    }
}

#[inline]
fn coord(v: f64) -> i64 {
    if v < 0.0 { -1 } else { v as i64 }
}
#[inline]
fn floor_i(v: f64) -> i64 {
    if v < 0.0 { v.floor() as i64 } else { v as i64 }
}

/// The C `resize(size, filter, box)` (`self.im.resize`).
fn core_resize(im: &Image, w: usize, h: usize, f: Filter, bx: [f32; 4]) -> Image {
    if f == Filter::Nearest {
        let a = [
            f64::from(bx[2] - bx[0]) / w as f64,
            0.0,
            f64::from(bx[0]),
            0.0,
            f64::from(bx[3] - bx[1]) / h as f64,
            f64::from(bx[1]),
        ];
        let mut out = Image::new(im.mode, w, h, &[]);
        scale_affine(im, w, h, a, &mut out);
        return out;
    }
    resample(im, w, h, f, bx)
}

impl Image {
    /// `resize(size, resample, box=None, reducing_gap=None)`. `resample` None is Pillow's default (BICUBIC).
    pub fn resize(&self, w: usize, h: usize, resample: Option<Filter>) -> Image {
        self.resize_full(w, h, resample, None, None)
    }

    pub fn resize_full(&self, w: usize, h: usize, resample: Option<Filter>, bx: Option<[f64; 4]>, reducing_gap: Option<f64>) -> Image {
        self.try_resize_full(w, h, resample, bx, reducing_gap).expect("resize")
    }

    /// `resize`, with the ValueErrors Pillow's C code raises for a bad size or box.
    pub fn try_resize_full(
        &self,
        w: usize,
        h: usize,
        resample: Option<Filter>,
        bx: Option<[f64; 4]>,
        reducing_gap: Option<f64>,
    ) -> Result<Image, &'static str> {
        if w < 1 || h < 1 {
            return Err("height and width must be > 0");
        }
        if let Some(b) = bx {
            let b32 = b.map(|v| v as f32);
            if b32[0] < 0.0 || b32[1] < 0.0 {
                return Err("box offset can't be negative");
            }
            if b32[2] > self.width as f32 || b32[3] > self.height as f32 {
                return Err("box can't exceed original image size");
            }
            if b32[2] - b32[0] < 0.0 || b32[3] - b32[1] < 0.0 {
                return Err("box can't be empty");
            }
        }
        Ok(self.resize_checked(w, h, resample, bx, reducing_gap))
    }

    fn resize_checked(&self, w: usize, h: usize, resample: Option<Filter>, bx: Option<[f64; 4]>, reducing_gap: Option<f64>) -> Image {
        let mut f = resample.unwrap_or(Filter::Bicubic);
        let bx = bx.unwrap_or([0.0, 0.0, self.width as f64, self.height as f64]);
        if (w, h) == self.size() && bx == [0.0, 0.0, self.width as f64, self.height as f64] {
            return self.clone();
        }
        if self.mode == Mode::One {
            f = Filter::Nearest;
        }
        if self.mode == Mode::Rgba && f != Filter::Nearest {
            let pm = premultiply(self);
            return unpremultiply(&pm.resize_rgba_inner(w, h, f, bx));
        }
        let mut src = self.clone();
        let mut bx = bx;
        if let Some(gap) = reducing_gap
            && f != Filter::Nearest
        {
            let fx = (((bx[2] - bx[0]) / w as f64 / gap) as i64).max(1);
            let fy = (((bx[3] - bx[1]) / h as f64 / gap) as i64).max(1);
            if fx > 1 || fy > 1 {
                let rb = self.safe_box(w, h, f, bx);
                src = self.reduce_box(fx as usize, fy as usize, Some(rb));
                bx = [
                    (bx[0] - rb.0 as f64) / fx as f64,
                    (bx[1] - rb.1 as f64) / fy as f64,
                    (bx[2] - rb.0 as f64) / fx as f64,
                    (bx[3] - rb.1 as f64) / fy as f64,
                ];
            }
        }
        let fb = |b: [f64; 4]| [b[0] as f32, b[1] as f32, b[2] as f32, b[3] as f32];
        if src.height > src.width * 100 && h < src.height {
            let first = core_resize(&src, src.width, h, f, fb([0.0, bx[1], src.width as f64, bx[3]]));
            return core_resize(&first, w, h, f, fb([bx[0], 0.0, bx[2], h as f64]));
        }
        core_resize(&src, w, h, f, fb(bx))
    }

    fn resize_rgba_inner(&self, w: usize, h: usize, f: Filter, bx: [f64; 4]) -> Image {
        // the premultiplied image resized as a 4-band image (box only, no reducing gap: Pillow passes none here)
        let fb = [bx[0] as f32, bx[1] as f32, bx[2] as f32, bx[3] as f32];
        core_resize(self, w, h, f, fb)
    }

    fn safe_box(&self, w: usize, h: usize, f: Filter, bx: [f64; 4]) -> (usize, usize, usize, usize) {
        let support = f.support() - 0.5;
        let sx = support * ((bx[2] - bx[0]) / w as f64);
        let sy = support * ((bx[3] - bx[1]) / h as f64);
        (
            ((bx[0] - sx) as i64).max(0) as usize,
            ((bx[1] - sy) as i64).max(0) as usize,
            ((bx[2] + sx).ceil() as i64).min(self.width as i64) as usize,
            ((bx[3] + sy).ceil() as i64).min(self.height as i64) as usize,
        )
    }

    /// `reduce(factor)`.
    pub fn reduce(&self, factor: usize) -> Image {
        self.reduce_box(factor, factor, None)
    }

    /// `reduce((fx, fy), box)`.
    pub fn reduce_box(&self, fx: usize, fy: usize, bx: Option<(usize, usize, usize, usize)>) -> Image {
        let (x0, y0, x1, y1) = bx.unwrap_or((0, 0, self.width, self.height));
        if fx == 1 && fy == 1 {
            return self.crop_i(x0 as i64, y0 as i64, x1 as i64, y1 as i64);
        }
        if self.mode == Mode::Rgba {
            return unpremultiply(&premultiply(self).reduce_raw(fx, fy, (x0, y0, x1 - x0, y1 - y0)));
        }
        self.reduce_raw(fx, fy, (x0, y0, x1 - x0, y1 - y0))
    }

    /// `ImagingReduce` with box = (x, y, width, height).
    fn reduce_raw(&self, xs: usize, ys: usize, bx: (usize, usize, usize, usize)) -> Image {
        let (bx0, by0, bw, bh) = bx;
        let (ow, oh) = (bw.div_ceil(xs), bh.div_ceil(ys));
        let b = self.bands();
        let mut out = Image::new(self.mode, ow, oh, &[]);
        let mult = |n: usize| (4_294_967_296.0f32 / ((1u32 << 8) * n as u32) as f32) as u32;
        let cell = |out: &mut Image, x: usize, y: usize, xa: usize, xb: usize, ya: usize, yb: usize| {
            let n = (xb - xa) * (yb - ya);
            let (m, amend) = (mult(n), (n / 2) as u32);
            for c in 0..b {
                let mut ss = amend;
                for yy in ya..yb {
                    for xx in xa..xb {
                        ss = ss.wrapping_add(u32::from(self.data[(yy * self.width + xx) * b + c]));
                    }
                }
                out.data[(y * ow + x) * b + c] = (ss.wrapping_mul(m) >> 24) as u8;
            }
        };
        for y in 0..oh {
            let ya = by0 + y * ys;
            let yb = (ya + ys).min(by0 + bh);
            for x in 0..ow {
                let xa = bx0 + x * xs;
                let xb = (xa + xs).min(bx0 + bw);
                cell(&mut out, x, y, xa, xb, ya, yb);
            }
        }
        out
    }

    /// `thumbnail(size)` (BICUBIC, reducing_gap 2.0), returning the new image (Pillow changes the image in place).
    pub fn thumbnail(&self, w: usize, h: usize) -> Image {
        self.thumbnail_filter(w, h, Filter::Bicubic)
    }

    /// `thumbnail(size, resample)` (reducing_gap 2.0).
    pub fn thumbnail_filter(&self, w: usize, h: usize, resample: Filter) -> Image {
        let (x, y) = (w as i64, h as i64);
        if x >= self.width as i64 && y >= self.height as i64 {
            return self.clone();
        }
        let aspect = self.width as f64 / self.height as f64;
        let round_aspect = |n: f64, key: &dyn Fn(f64) -> f64| -> i64 {
            let (fl, ce) = (n.floor(), n.ceil());
            let pick = if key(ce) < key(fl) { ce } else { fl }; // min() keeps the first of equals: floor
            (pick as i64).max(1)
        };
        let (nx, ny) = if x as f64 / y as f64 >= aspect {
            let yy = y as f64;
            (round_aspect(yy * aspect, &|n| (aspect - n / yy).abs()), y)
        } else {
            let xx = x as f64;
            (x, round_aspect(xx / aspect, &|n| if n == 0.0 { 0.0 } else { (aspect - xx / n).abs() }))
        };
        if (nx as usize, ny as usize) == self.size() {
            return self.clone();
        }
        self.resize_full(nx as usize, ny as usize, Some(resample), None, Some(2.0))
    }

    /// `transform(size, AFFINE, data, resample, fillcolor)`.
    pub fn transform_affine(&self, w: usize, h: usize, a: [f64; 6], resample: Filter, fill: &[u8]) -> Image {
        let resample = if self.mode == Mode::One { Filter::Nearest } else { resample };
        if self.mode == Mode::Rgba && resample != Filter::Nearest {
            return unpremultiply(&premultiply(self).transform_affine_raw(w, h, a, resample, fill));
        }
        self.transform_affine_raw(w, h, a, resample, fill)
    }

    fn transform_affine_raw(&self, w: usize, h: usize, a: [f64; 6], f: Filter, fill: &[u8]) -> Image {
        let mut out = Image::new(self.mode, w, h, fill);
        if f == Filter::Nearest {
            if a[1] == 0.0 && a[3] == 0.0 {
                scale_affine(self, w, h, a, &mut out);
                return out;
            }
            let check = |x: f64, y: f64| (x * a[0] + y * a[1] + a[2]).abs() < 32768.0 && (x * a[3] + y * a[4] + a[5]).abs() < 32768.0;
            let (fw, fh) = (w as f64, h as f64);
            if check(0.0, 0.0) && check(fw, fh) && check(0.0, fh) && check(fw, 0.0) {
                self.affine_fixed(&mut out, a);
                return out;
            }
            self.affine_float(&mut out, a);
            return out;
        }
        let b = self.bands();
        let mut px = vec![0u8; b];
        for y in 0..h {
            for x in 0..w {
                let (xin, yin) = (x as f64 + 0.5, y as f64 + 0.5);
                let xx = a[0] * xin + a[1] * yin + a[2];
                let yy = a[3] * xin + a[4] * yin + a[5];
                let ok = match f {
                    Filter::Bilinear => self.bilinear(&mut px, xx, yy),
                    Filter::Bicubic => self.bicubic(&mut px, xx, yy),
                    _ => panic!("{f:?} can't be used in a transform"),
                };
                if ok {
                    out.px_mut(x, y).copy_from_slice(&px);
                }
            }
        }
        out
    }

    fn affine_fixed(&self, out: &mut Image, a: [f64; 6]) {
        let fix = |v: f64| floor_i(v * 65536.0 + 0.5) as i32;
        let (a0, a1, a3, a4) = (fix(a[0]), fix(a[1]), fix(a[3]), fix(a[4]));
        let mut a2 = fix(a[2] + a[0] * 0.5 + a[1] * 0.5);
        let mut a5 = fix(a[5] + a[3] * 0.5 + a[4] * 0.5);
        let b = self.bands();
        for y in 0..out.height {
            let (mut xx, mut yy) = (a2, a5);
            for x in 0..out.width {
                let xin = xx >> 16;
                if xin >= 0 && (xin as usize) < self.width {
                    let yin = yy >> 16;
                    if yin >= 0 && (yin as usize) < self.height {
                        let s = (yin as usize * self.width + xin as usize) * b;
                        let d = (y * out.width + x) * b;
                        out.data[d..d + b].copy_from_slice(&self.data[s..s + b]);
                    }
                }
                xx = xx.wrapping_add(a0);
                yy = yy.wrapping_add(a3);
            }
            a2 = a2.wrapping_add(a1);
            a5 = a5.wrapping_add(a4);
        }
    }

    fn affine_float(&self, out: &mut Image, a: [f64; 6]) {
        let b = self.bands();
        let mut xo = a[2] + a[1] * 0.5 + a[0] * 0.5;
        let mut yo = a[5] + a[4] * 0.5 + a[3] * 0.5;
        for y in 0..out.height {
            let (mut xx, mut yy) = (xo, yo);
            for x in 0..out.width {
                let xin = coord(xx);
                if xin >= 0 && (xin as usize) < self.width {
                    let yin = coord(yy);
                    if yin >= 0 && (yin as usize) < self.height {
                        let s = (yin as usize * self.width + xin as usize) * b;
                        let d = (y * out.width + x) * b;
                        out.data[d..d + b].copy_from_slice(&self.data[s..s + b]);
                    }
                }
                xx += a[0];
                yy += a[3];
            }
            xo += a[1];
            yo += a[4];
        }
    }

    fn xclip(&self, x: i64) -> usize {
        x.clamp(0, self.width as i64 - 1) as usize
    }
    fn yclip(&self, y: i64) -> usize {
        y.clamp(0, self.height as i64 - 1) as usize
    }

    fn bilinear(&self, out: &mut [u8], xin: f64, yin: f64) -> bool {
        if xin < 0.0 || xin >= self.width as f64 || yin < 0.0 || yin >= self.height as f64 {
            return false;
        }
        let (xin, yin) = (xin - 0.5, yin - 0.5);
        let (x, y) = (floor_i(xin), floor_i(yin));
        let (dx, dy) = (xin - x as f64, yin - y as f64);
        let b = self.bands();
        let (x0, x1) = (self.xclip(x), self.xclip(x + 1));
        for c in 0..b {
            let at = |yy: usize, xx: usize| f64::from(self.data[(yy * self.width + xx) * b + c]);
            let lerp = |a: f64, bb: f64, d: f64| a + (bb - a) * d;
            let yr = self.yclip(y);
            let v1 = lerp(at(yr, x0), at(yr, x1), dx);
            let v2 = if y + 1 >= 0 && y + 1 < self.height as i64 {
                lerp(at((y + 1) as usize, x0), at((y + 1) as usize, x1), dx)
            } else {
                v1
            };
            let v = lerp(v1, v2, dy);
            out[c] = v as u8;
        }
        true
    }

    fn bicubic(&self, out: &mut [u8], xin: f64, yin: f64) -> bool {
        if xin < 0.0 || xin >= self.width as f64 || yin < 0.0 || yin >= self.height as f64 {
            return false;
        }
        let (xin, yin) = (xin - 0.5, yin - 0.5);
        let (mut x, mut y) = (floor_i(xin), floor_i(yin));
        let (dx, dy) = (xin - x as f64, yin - y as f64);
        x -= 1;
        y -= 1;
        let b = self.bands();
        let cubic = |v1: f64, v2: f64, v3: f64, v4: f64, d: f64| {
            let p1 = v2;
            let p2 = -v1 + v3;
            let p3 = 2.0 * (v1 - v2) + v3 - v4;
            let p4 = -v1 + v2 - v3 + v4;
            p1 + d * (p2 + d * (p3 + d * p4))
        };
        let xs = [self.xclip(x), self.xclip(x + 1), self.xclip(x + 2), self.xclip(x + 3)];
        for c in 0..b {
            let row = |yy: usize| {
                let at = |xx: usize| f64::from(self.data[(yy * self.width + xx) * b + c]);
                cubic(at(xs[0]), at(xs[1]), at(xs[2]), at(xs[3]), dx)
            };
            let v1 = row(self.yclip(y));
            let inside = |k: i64| y + k >= 0 && y + k < self.height as i64;
            let v2 = if inside(1) { row((y + 1) as usize) } else { v1 };
            let v3 = if inside(2) { row((y + 2) as usize) } else { v2 };
            let v4 = if inside(3) { row((y + 3) as usize) } else { v3 };
            let v = cubic(v1, v2, v3, v4, dy);
            out[c] = if v <= 0.0 {
                0
            } else if v >= 255.0 {
                255
            } else {
                v as u8
            };
        }
        true
    }

    /// `rotate(angle, resample, expand, fillcolor)` about the centre.
    pub fn rotate(&self, angle: f64, resample: Filter, expand: bool, fill: &[u8]) -> Image {
        let angle = angle.rem_euclid(360.0);
        if angle == 0.0 {
            return self.clone();
        }
        if angle == 180.0 {
            return self.transpose(Transpose::Rotate180);
        }
        if (angle == 90.0 || angle == 270.0) && (expand || self.width == self.height) {
            return self.transpose(if angle == 90.0 { Transpose::Rotate90 } else { Transpose::Rotate270 });
        }
        let (mut w, mut h) = (self.width as f64, self.height as f64);
        let (cx, cy) = (w / 2.0, h / 2.0);
        let rad = -angle.to_radians();
        let r15 = |v: f64| round_digits(v, 15);
        let mut m = [r15(rad.cos()), r15(rad.sin()), 0.0, r15(-rad.sin()), r15(rad.cos()), 0.0];
        let tr = |x: f64, y: f64, m: &[f64; 6]| (m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5]);
        let (t2, t5) = tr(-cx, -cy, &m);
        m[2] = t2 + cx;
        m[5] = t5 + cy;
        if expand {
            let pts = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].map(|(x, y)| tr(x, y, &m));
            let maxx = pts.iter().map(|p| p.0).fold(f64::MIN, f64::max);
            let minx = pts.iter().map(|p| p.0).fold(f64::MAX, f64::min);
            let maxy = pts.iter().map(|p| p.1).fold(f64::MIN, f64::max);
            let miny = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min);
            let nw = maxx.ceil() - minx.floor();
            let nh = maxy.ceil() - miny.floor();
            let (t2, t5) = tr(-(nw - w) / 2.0, -(nh - h) / 2.0, &m);
            m[2] = t2;
            m[5] = t5;
            w = nw;
            h = nh;
        }
        self.transform_affine(w as usize, h as usize, m, resample, fill)
    }
}

/// Python's `round(v, n)` for floats (correctly rounded through the decimal string, as CPython does).
pub fn round_digits(v: f64, n: usize) -> f64 {
    format!("{v:.n$}").parse().unwrap()
}
