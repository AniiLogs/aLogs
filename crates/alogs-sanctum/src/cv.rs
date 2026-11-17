//! The OpenCV operations Sanctum Helper uses, in plain Rust (no OpenCV to build or ship), following OpenCV 4.10's own
//! arithmetic (fixed-point coefficients, its SIMD paths' rounding) so 8-bit results are the same bytes. Checked against
//! goldens from the OpenCvSharp build Sanctum Helper ships (tools/cv-goldens).

/// An 8-bit image, channels interleaved (BGR order where it matters, as OpenCV).
#[derive(Clone, Debug, PartialEq)]
pub struct Mat {
    pub w: usize,
    pub h: usize,
    pub c: usize,
    pub data: Vec<u8>,
}

/// A float image.
#[derive(Clone, Debug, PartialEq)]
pub struct MatF {
    pub w: usize,
    pub h: usize,
    pub c: usize,
    pub data: Vec<f32>,
}

/// cvRound: nearest, ties to even (the FPU's default mode).
#[inline]
pub fn round_i(v: f64) -> i64 {
    v.round_ties_even() as i64
}

#[inline]
fn sat_u8(v: i64) -> u8 {
    v.clamp(0, 255) as u8
}

#[inline]
fn sat_u8f(v: f32) -> u8 {
    sat_u8(v.round_ties_even() as i64)
}

impl Mat {
    pub fn new(w: usize, h: usize, c: usize) -> Mat {
        Mat { w, h, c, data: vec![0; w * h * c] }
    }

    pub fn filled(w: usize, h: usize, c: usize, v: u8) -> Mat {
        Mat { w, h, c, data: vec![v; w * h * c] }
    }

    #[inline]
    pub fn at(&self, x: usize, y: usize, ch: usize) -> u8 {
        self.data[(y * self.w + x) * self.c + ch]
    }

    pub fn row(&self, y: usize) -> &[u8] {
        &self.data[y * self.w * self.c..(y + 1) * self.w * self.c]
    }

    /// A copy of a rectangle (clipped to the image).
    pub fn roi(&self, x: usize, y: usize, w: usize, h: usize) -> Mat {
        let (w, h) = (w.min(self.w.saturating_sub(x)), h.min(self.h.saturating_sub(y)));
        let mut o = Mat::new(w, h, self.c);
        for r in 0..h {
            let s = ((y + r) * self.w + x) * self.c;
            o.data[r * w * self.c..(r + 1) * w * self.c].copy_from_slice(&self.data[s..s + w * self.c]);
        }
        o
    }

    pub fn to_f32(&self, scale: f32) -> MatF {
        MatF { w: self.w, h: self.h, c: self.c, data: self.data.iter().map(|&v| f32::from(v) * scale).collect() }
    }

    pub fn count_nonzero(&self) -> usize {
        self.data.iter().filter(|&&v| v != 0).count()
    }

    /// One channel of it.
    pub fn channel(&self, ch: usize) -> Mat {
        Mat { w: self.w, h: self.h, c: 1, data: self.data.iter().skip(ch).step_by(self.c).copied().collect() }
    }
}

impl MatF {
    pub fn new(w: usize, h: usize, c: usize) -> MatF {
        MatF { w, h, c, data: vec![0.0; w * h * c] }
    }

    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.w + x]
    }

    /// (min, max, min at, max at) of a one-channel image, NaNs skipped (minMaxLoc).
    pub fn min_max_loc(&self) -> (f32, f32, (usize, usize), (usize, usize)) {
        let (mut mn, mut mx, mut lmn, mut lmx) = (f32::MAX, f32::MIN, (0, 0), (0, 0));
        for (i, &v) in self.data.iter().enumerate() {
            if v.is_nan() {
                continue;
            }
            if v < mn {
                mn = v;
                lmn = (i % self.w, i / self.w);
            }
            if v > mx {
                mx = v;
                lmx = (i % self.w, i / self.w);
            }
        }
        (mn, mx, lmn, lmx)
    }
}

// ---------------------------------------------------------------------------------------------- resize

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
    Nearest,
    Linear,
    Cubic,
    Area,
}

const COEF_BITS: i32 = 11;
const COEF_SCALE: i32 = 1 << COEF_BITS;

/// The output size OpenCV gives for scale factors (saturate_cast<int>: rounded).
pub fn scaled_size(w: usize, h: usize, fx: f64, fy: f64) -> (usize, usize) {
    (round_i(w as f64 * fx) as usize, round_i(h as f64 * fy) as usize)
}

/// cv::resize to (dw, dh); with dw = dh = 0, by (fx, fy).
pub fn resize(src: &Mat, dw: usize, dh: usize, fx: f64, fy: f64, interp: Interp) -> Mat {
    let (dw, dh, inv_x, inv_y) = if dw == 0 || dh == 0 {
        let (w, h) = scaled_size(src.w, src.h, fx, fy);
        (w, h, fx, fy)
    } else {
        (dw, dh, dw as f64 / src.w as f64, dh as f64 / src.h as f64)
    };
    if dw == 0 || dh == 0 {
        return Mat::new(dw, dh, src.c);
    }
    if dw == src.w && dh == src.h {
        return src.clone();
    }
    if interp == Interp::Nearest {
        return resize_nn(src, dw, dh, inv_x, inv_y);
    }
    let (scale_x, scale_y) = (1.0 / inv_x, 1.0 / inv_y);
    let (isx, isy) = (round_i(scale_x), round_i(scale_y));
    let area_fast = (scale_x - isx as f64).abs() < f64::EPSILON && (scale_y - isy as f64).abs() < f64::EPSILON;
    let mut interp = interp;
    if interp == Interp::Linear && area_fast && isx == 2 && isy == 2 {
        interp = Interp::Area;
    }
    if interp == Interp::Area && scale_x >= 1.0 && scale_y >= 1.0 {
        return if area_fast { resize_area_fast(src, dw, dh, isx as usize, isy as usize) } else { resize_area(src, dw, dh, scale_x, scale_y) };
    }
    resize_generic(src, dw, dh, scale_x, scale_y, inv_x, inv_y, interp)
}

fn resize_nn(src: &Mat, dw: usize, dh: usize, inv_x: f64, inv_y: f64) -> Mat {
    let (ifx, ify) = (1.0 / inv_x, 1.0 / inv_y);
    let xs: Vec<usize> = (0..dw).map(|x| ((x as f64 * ifx).floor() as usize).min(src.w - 1)).collect();
    let mut o = Mat::new(dw, dh, src.c);
    for y in 0..dh {
        let sy = ((y as f64 * ify).floor() as usize).min(src.h - 1);
        for (x, &sx) in xs.iter().enumerate() {
            for ch in 0..src.c {
                o.data[(y * dw + x) * src.c + ch] = src.data[(sy * src.w + sx) * src.c + ch];
            }
        }
    }
    o
}

fn resize_area_fast(src: &Mat, dw: usize, dh: usize, sx: usize, sy: usize) -> Mat {
    let c = src.c;
    let mut o = Mat::new(dw, dh, c);
    let full_w = src.w / sx;
    let area = (sx * sy) as f32;
    let scale = 1.0f32 / area;
    for dy in 0..dh {
        let y0 = dy * sy;
        if y0 >= src.h {
            continue;
        }
        let full_row = y0 + sy <= src.h;
        for dx in 0..dw {
            let x0 = dx * sx;
            for ch in 0..c {
                let v = if full_row && dx < full_w {
                    let mut sum = 0i64;
                    for yy in 0..sy {
                        for xx in 0..sx {
                            sum += i64::from(src.data[((y0 + yy) * src.w + x0 + xx) * c + ch]);
                        }
                    }
                    if sx == 2 && sy == 2 { ((sum + 2) >> 2) as u8 } else { sat_u8f(sum as f32 * scale) }
                } else {
                    let (mut sum, mut n) = (0i64, 0i64);
                    for yy in 0..sy {
                        if y0 + yy >= src.h {
                            break;
                        }
                        for xx in 0..sx {
                            if x0 + xx >= src.w {
                                break;
                            }
                            sum += i64::from(src.data[((y0 + yy) * src.w + x0 + xx) * c + ch]);
                            n += 1;
                        }
                    }
                    if n == 0 { 0 } else { sat_u8f(sum as f32 / n as f32) }
                };
                o.data[(dy * dw + dx) * c + ch] = v;
            }
        }
    }
    o
}

/// computeResizeAreaTab: (destination index, source index, weight).
fn area_tab(ssize: usize, dsize: usize, scale: f64) -> Vec<(usize, usize, f32)> {
    let mut tab = vec![];
    for dx in 0..dsize {
        let fsx1 = dx as f64 * scale;
        let fsx2 = fsx1 + scale;
        let cell = scale.min(ssize as f64 - fsx1);
        let mut sx1 = fsx1.ceil() as i64;
        let mut sx2 = fsx2.floor() as i64;
        sx2 = sx2.min(ssize as i64 - 1);
        sx1 = sx1.min(sx2);
        if sx1 as f64 - fsx1 > 1e-3 {
            tab.push((dx, (sx1 - 1) as usize, ((sx1 as f64 - fsx1) / cell) as f32));
        }
        for sx in sx1..sx2 {
            tab.push((dx, sx as usize, (1.0 / cell) as f32));
        }
        if fsx2 - sx2 as f64 > 1e-3 {
            tab.push((dx, sx2 as usize, ((fsx2 - sx2 as f64).min(1.0).min(cell) / cell) as f32));
        }
    }
    tab
}

fn resize_area(src: &Mat, dw: usize, dh: usize, scale_x: f64, scale_y: f64) -> Mat {
    let c = src.c;
    let xtab = area_tab(src.w, dw, scale_x);
    let ytab = area_tab(src.h, dh, scale_y);
    let width = dw * c;
    let mut o = Mat::new(dw, dh, c);
    let mut buf = vec![0f32; width];
    let mut sum = vec![0f32; width];
    let mut prev_dy = ytab.first().map_or(0, |t| t.0);
    for &(dy, sy, beta) in &ytab {
        let s = src.row(sy);
        buf.iter_mut().for_each(|v| *v = 0.0);
        for &(dx, sx, alpha) in &xtab {
            for ch in 0..c {
                buf[dx * c + ch] += f32::from(s[sx * c + ch]) * alpha;
            }
        }
        if dy != prev_dy {
            for i in 0..width {
                o.data[prev_dy * width + i] = sat_u8f(sum[i]);
                sum[i] = beta * buf[i];
            }
            prev_dy = dy;
        } else {
            for i in 0..width {
                sum[i] += beta * buf[i];
            }
        }
    }
    for i in 0..width {
        o.data[prev_dy * width + i] = sat_u8f(sum[i]);
    }
    o
}

fn cubic_coeffs(x: f32) -> [f32; 4] {
    const A: f32 = -0.75;
    let c0 = ((A * (x + 1.0) - 5.0 * A) * (x + 1.0) + 8.0 * A) * (x + 1.0) - 4.0 * A;
    let c1 = ((A + 2.0) * x - (A + 3.0)) * x * x + 1.0;
    let c2 = ((A + 2.0) * (1.0 - x) - (A + 3.0)) * (1.0 - x) * (1.0 - x) + 1.0;
    [c0, c1, c2, 1.0 - c0 - c1 - c2]
}

/// Per output position: the source position and the fixed-point coefficients; and the [min, max) range where every
/// tap is inside the source.
fn generic_tab(ssize: usize, dsize: usize, scale: f64, inv: f64, interp: Interp, clamp: bool) -> (Vec<i64>, Vec<[i32; 4]>, usize, usize) {
    let area = interp == Interp::Area;
    let ksize = if interp == Interp::Cubic { 4 } else { 2 };
    let k2 = ksize / 2;
    let (mut xmin, mut xmax) = (0usize, dsize);
    let mut ofs = vec![];
    let mut coef = vec![];
    for dx in 0..dsize {
        let (mut sx, mut fx);
        if !area {
            fx = ((dx as f64 + 0.5) * scale - 0.5) as f32;
            sx = fx.floor() as i64;
            fx -= sx as f32;
        } else {
            sx = (dx as f64 * scale).floor() as i64;
            fx = ((dx + 1) as f64 - (sx + 1) as f64 * inv) as f32;
            fx = if fx <= 0.0 { 0.0 } else { fx - fx.floor() };
        }
        if clamp && sx < k2 as i64 - 1 {
            xmin = dx + 1;
            if sx < 0 && interp != Interp::Cubic {
                fx = 0.0;
                sx = 0;
            }
        }
        if clamp && sx + k2 as i64 >= ssize as i64 {
            xmax = xmax.min(dx);
            if sx >= ssize as i64 - 1 && interp != Interp::Cubic {
                fx = 0.0;
                sx = ssize as i64 - 1;
            }
        }
        ofs.push(sx);
        let cb = if interp == Interp::Cubic { cubic_coeffs(fx) } else { [1.0 - fx, fx, 0.0, 0.0] };
        let mut q = cb.map(|v| (v * COEF_SCALE as f32).round_ties_even() as i32);
        if interp == Interp::Cubic {
            // taps summing to 2048 (the largest takes the rest): closest to OpenCV measured, under 0.5% of bytes 1 off
            let fix = COEF_SCALE - q.iter().sum::<i32>();
            let i = (0..4).max_by_key(|&i| q[i]).unwrap();
            q[i] += fix;
        }
        coef.push(q);
    }
    (ofs, coef, xmin, xmax)
}

#[allow(clippy::too_many_arguments)]
fn resize_generic(src: &Mat, dw: usize, dh: usize, scale_x: f64, scale_y: f64, inv_x: f64, inv_y: f64, interp: Interp) -> Mat {
    let c = src.c;
    let (xofs, alpha, xmin, xmax) = generic_tab(src.w, dw, scale_x, inv_x, interp, true);
    let (yofs, beta, _, _) = generic_tab(src.h, dh, scale_y, inv_y, interp, false);
    let cubic = interp == Interp::Cubic;
    let width = dw * c;
    let swidth = (src.w * c) as i64;
    // the horizontal pass of one source row
    let hresize = |row: &[u8]| -> Vec<i32> {
        let mut d = vec![0i32; width];
        for dx in 0..dw {
            let a = alpha[dx];
            for ch in 0..c {
                let sx = xofs[dx] * c as i64 + ch as i64;
                let v = if cubic {
                    let inside = dx >= xmin && dx < xmax;
                    let mut v = 0i32;
                    for (j, aj) in a.iter().enumerate() {
                        let mut sxj = sx - c as i64 + j as i64 * c as i64;
                        if !inside && (sxj < 0 || sxj >= swidth) {
                            while sxj < 0 {
                                sxj += c as i64;
                            }
                            while sxj >= swidth {
                                sxj -= c as i64;
                            }
                        }
                        v += i32::from(row[sxj as usize]) * aj;
                    }
                    v
                } else if dx < xmax {
                    i32::from(row[sx as usize]) * a[0] + i32::from(row[(sx + c as i64) as usize]) * a[1]
                } else {
                    i32::from(row[sx as usize]) * COEF_SCALE
                };
                d[dx * c + ch] = v;
            }
        }
        d
    };
    let mut o = Mat::new(dw, dh, c);
    for dy in 0..dh {
        let sy0 = yofs[dy];
        let b = beta[dy];
        let rows: Vec<Vec<i32>> = if cubic {
            (0..4).map(|k| hresize(src.row((sy0 - 1 + k).clamp(0, src.h as i64 - 1) as usize))).collect()
        } else {
            (0..2).map(|k| hresize(src.row((sy0 + k).clamp(0, src.h as i64 - 1) as usize))).collect()
        };
        let d = &mut o.data[dy * width..(dy + 1) * width];
        let mut x = 0usize;
        if cubic {
            // VResizeCubicVec_32s8u: floats, 16 at a time, (((S3*b3) + S2*b2) + S1*b1) + S0*b0 rounded
            let s = 1.0f32 / (COEF_SCALE as f32 * COEF_SCALE as f32);
            let bf: Vec<f32> = b.iter().map(|&v| v as f32 * s).collect();
            // VResizeCubicVec_32s8u's float sum on every pixel (as measured)
            while x < width {
                for i in x..width {
                    let v = rows[3][i] as f32 * bf[3];
                    let v = rows[2][i] as f32 * bf[2] + v;
                    let v = rows[1][i] as f32 * bf[1] + v;
                    let v = rows[0][i] as f32 * bf[0] + v;
                    d[i] = sat_u8(v.round_ties_even() as i64);
                }
                x = width;
            }
            for i in x..width {
                let v: i64 = (0..4).map(|k| i64::from(rows[k][i]) * i64::from(b[k])).sum();
                d[i] = sat_u8((v + (1 << 21)) >> 22);
            }
        } else {
            // VResizeLinearVec_32s8u: ((S0>>4)*b0>>16) + ((S1>>4)*b1>>16), then (+2)>>2 (every pixel, as measured)
            let simd = |i: usize| -> u8 {
                let p = |v: i32| (v >> 4).clamp(i16::MIN as i32, i16::MAX as i32);
                let t = ((p(rows[0][i]) * b[0]) >> 16) + ((p(rows[1][i]) * b[1]) >> 16);
                sat_u8(i64::from((t + 2) >> 2))
            };
            while x + 16 <= width {
                for i in x..x + 16 {
                    d[i] = simd(i);
                }
                x += 16;
            }
            for i in x..width {
                d[i] = simd(i);
            }
        }
    }
    o
}

// ---------------------------------------------------------------------------------------------- colour

/// BGR (or BGRA) to gray: OpenCV's fixed-point weights.
pub fn bgr2gray(src: &Mat) -> Mat {
    let mut o = Mat::new(src.w, src.h, 1);
    for (i, p) in src.data.chunks_exact(src.c).enumerate() {
        let v = i32::from(p[0]) * 3735 + i32::from(p[1]) * 19235 + i32::from(p[2]) * 9798;
        o.data[i] = ((v + (1 << 14)) >> 15) as u8;
    }
    o
}

pub fn bgra2bgr(src: &Mat) -> Mat {
    Mat { w: src.w, h: src.h, c: 3, data: src.data.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect() }
}

pub fn gray2bgr(src: &Mat) -> Mat {
    Mat { w: src.w, h: src.h, c: 3, data: src.data.iter().flat_map(|&v| [v, v, v]).collect() }
}

/// BGR to HSV (H 0-180): OpenCV's division tables, 12-bit fixed point.
pub fn bgr2hsv(src: &Mat) -> Mat {
    const SHIFT: i32 = 12;
    let sdiv: Vec<i32> = (0..256).map(|i| if i == 0 { 0 } else { round_i((255 << SHIFT) as f64 / i as f64) as i32 }).collect();
    let hdiv: Vec<i32> = (0..256).map(|i| if i == 0 { 0 } else { round_i((180 << SHIFT) as f64 / (6.0 * i as f64)) as i32 }).collect();
    let mut o = Mat::new(src.w, src.h, 3);
    for (i, p) in src.data.chunks_exact(src.c).enumerate() {
        let (b, g, r) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
        let v = b.max(g).max(r);
        let vmin = b.min(g).min(r);
        let diff = v - vmin;
        let vr = if v == r { -1 } else { 0 };
        let vg = if v == g { -1 } else { 0 };
        let s = (diff * sdiv[v as usize] + (1 << (SHIFT - 1))) >> SHIFT;
        let mut h = (vr & (g - b)) + (!vr & ((vg & (b - r + 2 * diff)) + (!vg & (r - g + 4 * diff))));
        h = (h * hdiv[diff as usize] + (1 << (SHIFT - 1))) >> SHIFT;
        if h < 0 {
            h += 180;
        }
        o.data[i * 3] = h as u8;
        o.data[i * 3 + 1] = s as u8;
        o.data[i * 3 + 2] = v as u8;
    }
    o
}

// ---------------------------------------------------------------------------------------------- thresholds

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thresh {
    Binary,
    BinaryInv,
    TozeroInv,
}

/// threshold on 8-bit: v > t (OpenCV compares against floor(t)).
pub fn threshold(src: &Mat, t: f64, max: f64, kind: Thresh) -> Mat {
    let ti = t.floor() as i32;
    let m = sat_u8(round_i(max));
    let data = src.data.iter().map(|&v| {
        let over = i32::from(v) > ti;
        match kind {
            Thresh::Binary => if over { m } else { 0 },
            Thresh::BinaryInv => if over { 0 } else { m },
            Thresh::TozeroInv => if over { 0 } else { v },
        }
    }).collect();
    Mat { data, ..*src }
}

pub fn threshold_f(src: &MatF, t: f32, max: f32, kind: Thresh) -> MatF {
    let data = src.data.iter().map(|&v| {
        let over = v > t;
        match kind {
            Thresh::Binary => if over { max } else { 0.0 },
            Thresh::BinaryInv => if over { 0.0 } else { max },
            Thresh::TozeroInv => if over { 0.0 } else { v },
        }
    }).collect();
    MatF { data, ..*src }
}

/// Otsu's threshold of an 8-bit image (Vision.Otsu's arithmetic, which is OpenCV's).
pub fn otsu(src: &Mat) -> f64 {
    let mut h = [0f64; 256];
    for &v in &src.data {
        h[v as usize] += 1.0;
    }
    let total: f64 = h.iter().sum();
    if total <= 0.0 {
        return 0.0;
    }
    const EPS: f64 = 1.192092896e-07;
    let scale = 1.0 / total;
    let mu: f64 = h.iter().enumerate().map(|(i, v)| i as f64 * v).sum::<f64>() * scale;
    let (mut mu1, mut q1, mut max_sigma, mut max_val) = (0.0, 0.0, 0.0, 0.0);
    for (i, &hv) in h.iter().enumerate() {
        let pi = hv * scale;
        mu1 *= q1;
        q1 += pi;
        let q2 = 1.0 - q1;
        if q1.min(q2) < EPS || q1.max(q2) > 1.0 - EPS {
            continue;
        }
        mu1 = (mu1 + i as f64 * pi) / q1;
        let mu2 = (mu - q1 * mu1) / q2;
        let sigma = q1 * q2 * (mu1 - mu2) * (mu1 - mu2);
        if sigma > max_sigma {
            max_sigma = sigma;
            max_val = i as f64;
        }
    }
    max_val
}

/// inRange: 255 where every channel is within [lo, hi].
pub fn in_range(src: &Mat, lo: &[u8], hi: &[u8]) -> Mat {
    let data = src.data.chunks_exact(src.c).map(|p| if p.iter().zip(lo).zip(hi).all(|((v, l), h)| v >= l && v <= h) { 255 } else { 0 }).collect();
    Mat { w: src.w, h: src.h, c: 1, data }
}

pub fn bitwise_and(a: &Mat, b: &Mat) -> Mat {
    Mat { data: a.data.iter().zip(&b.data).map(|(x, y)| x & y).collect(), ..*a }
}

pub fn bitwise_or(a: &Mat, b: &Mat) -> Mat {
    Mat { data: a.data.iter().zip(&b.data).map(|(x, y)| x | y).collect(), ..*a }
}

pub fn bitwise_not(a: &Mat) -> Mat {
    Mat { data: a.data.iter().map(|x| !x).collect(), ..*a }
}
