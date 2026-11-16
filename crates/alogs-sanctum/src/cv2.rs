//! More of OpenCV in plain Rust: morphology, connected components, box blur, the Hanning window, template matching
//! (CCoeffNormed, with or without a mask), CLAHE, phase correlation and warpAffine. 8-bit results follow OpenCV 4.10's
//! arithmetic; correlations are computed in double, within 1e-4 of OpenCV's floats.

use crate::cv::{Mat, MatF, round_i};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

// ---------------------------------------------------------------------------------------------- morphology

/// A structuring element: (width, height, 0/1 cells).
#[derive(Clone, Debug)]
pub struct Kernel {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<bool>,
}

impl Kernel {
    pub fn rect(w: usize, h: usize) -> Kernel {
        Kernel { w, h, cells: vec![true; w * h] }
    }

    /// getStructuringElement(MORPH_ELLIPSE).
    pub fn ellipse(w: usize, h: usize) -> Kernel {
        let (r, c) = ((h / 2) as i64, (w / 2) as i64);
        let inv_r2 = if r != 0 { 1.0 / (r * r) as f64 } else { 0.0 };
        let mut cells = vec![false; w * h];
        for i in 0..h as i64 {
            let dy = i - r;
            let (j1, j2) = if dy.abs() <= r {
                let dx = round_i(c as f64 * (((r * r - dy * dy) as f64) * inv_r2).sqrt());
                ((c - dx).max(0), (c + dx + 1).min(w as i64))
            } else {
                (0, 0)
            };
            for j in j1..j2 {
                cells[(i as usize) * w + j as usize] = true;
            }
        }
        Kernel { w, h, cells }
    }
}

fn morph(src: &Mat, k: &Kernel, dilate: bool) -> Mat {
    let (ax, ay) = ((k.w / 2) as i64, (k.h / 2) as i64);
    let offs: Vec<(i64, i64)> = (0..k.h).flat_map(|y| (0..k.w).map(move |x| (x, y))).filter(|&(x, y)| k.cells[y * k.w + x]).map(|(x, y)| (x as i64 - ax, y as i64 - ay)).collect();
    let rect = offs.len() == k.w * k.h;
    let c = src.c;
    let pick = |a: u8, b: u8| if dilate { a.max(b) } else { a.min(b) };
    let init = if dilate { 0u8 } else { 255u8 };
    if rect {
        // separable: rows, then columns (outside the image doesn't count)
        let mut tmp = Mat::new(src.w, src.h, c);
        for y in 0..src.h {
            for x in 0..src.w as i64 {
                for ch in 0..c {
                    let mut v = init;
                    for dx in -ax..(k.w as i64 - ax) {
                        let xx = x + dx;
                        if xx >= 0 && xx < src.w as i64 {
                            v = pick(v, src.data[(y * src.w + xx as usize) * c + ch]);
                        }
                    }
                    tmp.data[(y * src.w + x as usize) * c + ch] = v;
                }
            }
        }
        let mut o = Mat::new(src.w, src.h, c);
        for y in 0..src.h as i64 {
            for x in 0..src.w {
                for ch in 0..c {
                    let mut v = init;
                    for dy in -ay..(k.h as i64 - ay) {
                        let yy = y + dy;
                        if yy >= 0 && yy < src.h as i64 {
                            v = pick(v, tmp.data[(yy as usize * src.w + x) * c + ch]);
                        }
                    }
                    o.data[(y as usize * src.w + x) * c + ch] = v;
                }
            }
        }
        return o;
    }
    let mut o = Mat::new(src.w, src.h, c);
    for y in 0..src.h as i64 {
        for x in 0..src.w as i64 {
            for ch in 0..c {
                let mut v = init;
                for &(dx, dy) in &offs {
                    let (xx, yy) = (x + dx, y + dy);
                    if xx >= 0 && yy >= 0 && xx < src.w as i64 && yy < src.h as i64 {
                        v = pick(v, src.data[(yy as usize * src.w + xx as usize) * c + ch]);
                    }
                }
                o.data[(y as usize * src.w + x as usize) * c + ch] = v;
            }
        }
    }
    o
}

pub fn dilate(src: &Mat, k: &Kernel) -> Mat {
    morph(src, k, true)
}

pub fn erode(src: &Mat, k: &Kernel) -> Mat {
    morph(src, k, false)
}

pub fn open(src: &Mat, k: &Kernel) -> Mat {
    dilate(&erode(src, k), k)
}

pub fn gradient(src: &Mat, k: &Kernel) -> Mat {
    let (d, e) = (dilate(src, k), erode(src, k));
    Mat { data: d.data.iter().zip(&e.data).map(|(a, b)| a.saturating_sub(*b)).collect(), ..d }
}

// ---------------------------------------------------------------------------------------------- components

/// One connected region: (left, top, width, height, area) and its centroid.
#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
    pub area: usize,
    pub cx: f64,
    pub cy: f64,
}

/// connectedComponentsWithStats (8-connected): label 0 the background, the rest numbered in raster order.
pub fn components(src: &Mat) -> (Vec<u32>, Vec<Component>) {
    let (w, h) = (src.w, src.h);
    let mut parent: Vec<u32> = vec![0];
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    }
    let mut lab = vec![0u32; w * h];
    for y in 0..h {
        for x in 0..w {
            if src.data[y * w + x] == 0 {
                continue;
            }
            let mut ns = [0u32; 4];
            let mut n = 0;
            for (dx, dy) in [(-1i64, 0i64), (-1, -1), (0, -1), (1, -1)] {
                let (xx, yy) = (x as i64 + dx, y as i64 + dy);
                if xx >= 0 && yy >= 0 && xx < w as i64 {
                    let l = lab[yy as usize * w + xx as usize];
                    if l != 0 {
                        ns[n] = l;
                        n += 1;
                    }
                }
            }
            if n == 0 {
                let l = parent.len() as u32;
                parent.push(l);
                lab[y * w + x] = l;
            } else {
                let mut m = find(&mut parent, ns[0]);
                for &l in &ns[1..n] {
                    let r = find(&mut parent, l);
                    if r != m {
                        let (a, b) = (m.min(r), m.max(r));
                        parent[b as usize] = a;
                        m = a;
                    }
                }
                lab[y * w + x] = m;
            }
        }
    }
    // final labels in the order OpenCV's block scan meets each region: by its first 2 x 2 block (rows in pairs)
    let mut first: Vec<(usize, usize)> = vec![(usize::MAX, usize::MAX); parent.len()];
    for y in 0..h {
        for x in 0..w {
            let v = lab[y * w + x];
            if v != 0 {
                let r = find(&mut parent, v) as usize;
                first[r] = first[r].min((y / 2, x / 2));
            }
        }
    }
    let mut roots: Vec<usize> = (1..parent.len()).filter(|&i| find(&mut parent, i as u32) as usize == i && first[i].0 != usize::MAX).collect();
    roots.sort_by_key(|&r| first[r]);
    let mut map = vec![0u32; parent.len()];
    for (i, &r) in roots.iter().enumerate() {
        map[r] = i as u32 + 1;
    }
    let next = roots.len() as u32 + 1;
    for v in lab.iter_mut() {
        if *v != 0 {
            *v = map[find(&mut parent, *v) as usize];
        }
    }
    let n = next as usize;
    let mut st = vec![(usize::MAX, usize::MAX, 0usize, 0usize, 0usize, 0f64, 0f64); n];
    for y in 0..h {
        for x in 0..w {
            let s = &mut st[lab[y * w + x] as usize];
            s.0 = s.0.min(x);
            s.1 = s.1.min(y);
            s.2 = s.2.max(x);
            s.3 = s.3.max(y);
            s.4 += 1;
            s.5 += x as f64;
            s.6 += y as f64;
        }
    }
    let comps = st.into_iter().map(|s| if s.4 == 0 {
        Component { left: 0, top: 0, width: 0, height: 0, area: 0, cx: f64::NAN, cy: f64::NAN }
    } else {
        Component { left: s.0, top: s.1, width: s.2 - s.0 + 1, height: s.3 - s.1 + 1, area: s.4, cx: s.5 / s.4 as f64, cy: s.6 / s.4 as f64 }
    }).collect();
    (lab, comps)
}

// ---------------------------------------------------------------------------------------------- filters

/// reflect-101 index.
fn reflect(i: i64, n: i64) -> usize {
    if n == 1 {
        return 0;
    }
    let mut i = i;
    while i < 0 || i >= n {
        i = if i < 0 { -i } else { 2 * n - 2 - i };
    }
    i as usize
}

/// blur (normalized box filter, reflect-101 border) on a one-channel float image.
pub fn blur(src: &MatF, k: usize) -> MatF {
    let a = (k / 2) as i64;
    let (w, h) = (src.w as i64, src.h as i64);
    let mut tmp = vec![0f64; src.data.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0f64;
            for d in -a..(k as i64 - a) {
                s += f64::from(src.data[(y * w) as usize + reflect(x + d, w)]);
            }
            tmp[(y * w + x) as usize] = s;
        }
    }
    let mut o = MatF::new(src.w, src.h, 1);
    let scale = 1.0 / (k * k) as f64;
    for y in 0..h {
        for x in 0..w {
            let mut s = 0f64;
            for d in -a..(k as i64 - a) {
                s += tmp[reflect(y + d, h) * w as usize + x as usize];
            }
            o.data[(y * w + x) as usize] = (s * scale) as f32;
        }
    }
    o
}

/// createHanningWindow (CV_32F).
pub fn hanning(w: usize, h: usize) -> MatF {
    let c0 = 2.0 * std::f64::consts::PI / (w as f64 - 1.0);
    let c1 = 2.0 * std::f64::consts::PI / (h as f64 - 1.0);
    let wc: Vec<f64> = (0..w).map(|j| 0.5 * (1.0 - (c0 * j as f64).cos())).collect();
    let mut o = MatF::new(w, h, 1);
    for i in 0..h {
        let wr = 0.5 * (1.0 - (c1 * i as f64).cos());
        for j in 0..w {
            o.data[i * w + j] = (wr * wc[j]).sqrt() as f32;
        }
    }
    o
}

// ---------------------------------------------------------------------------------------------- FFT helpers

fn fft2(data: &mut [Complex<f64>], w: usize, h: usize, inverse: bool) {
    let mut p = FftPlanner::new();
    let fr = if inverse { p.plan_fft_inverse(w) } else { p.plan_fft_forward(w) };
    for r in data.chunks_exact_mut(w) {
        fr.process(r);
    }
    let fc = if inverse { p.plan_fft_inverse(h) } else { p.plan_fft_forward(h) };
    let mut col = vec![Complex::new(0.0, 0.0); h];
    for x in 0..w {
        for y in 0..h {
            col[y] = data[y * w + x];
        }
        fc.process(&mut col);
        for y in 0..h {
            data[y * w + x] = col[y];
        }
    }
}

/// Valid cross-correlations of `tpls` (each tw x th) over `img` (iw x ih), all through one forward transform of
/// the image: out[k][(y, x)] = sum I[y+i][x+j] * T_k[i][j].
fn xcorr(img: &[f64], iw: usize, ih: usize, tpls: &[&[f64]], tw: usize, th: usize) -> Vec<Vec<f64>> {
    let mut fi: Vec<Complex<f64>> = img.iter().map(|&v| Complex::new(v, 0.0)).collect();
    fft2(&mut fi, iw, ih, false);
    let (rw, rh) = (iw - tw + 1, ih - th + 1);
    let n = (iw * ih) as f64;
    tpls.iter().map(|t| {
        let mut ft = vec![Complex::new(0.0, 0.0); iw * ih];
        for y in 0..th {
            for x in 0..tw {
                ft[y * iw + x] = Complex::new(t[y * tw + x], 0.0);
            }
        }
        fft2(&mut ft, iw, ih, false);
        let mut p: Vec<Complex<f64>> = fi.iter().zip(&ft).map(|(a, b)| a * b.conj()).collect();
        fft2(&mut p, iw, ih, true);
        let mut out = vec![0f64; rw * rh];
        for y in 0..rh {
            for x in 0..rw {
                out[y * rw + x] = p[y * iw + x].re / n;
            }
        }
        out
    }).collect()
}

/// Sliding window sums of a one-channel plane (summed-area table).
fn window_sums(v: &[f64], w: usize, h: usize, tw: usize, th: usize) -> Vec<f64> {
    let mut sat = vec![0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0f64;
        for x in 0..w {
            row += v[y * w + x];
            sat[(y + 1) * (w + 1) + x + 1] = sat[y * (w + 1) + x + 1] + row;
        }
    }
    let (rw, rh) = (w - tw + 1, h - th + 1);
    let mut o = vec![0f64; rw * rh];
    for y in 0..rh {
        for x in 0..rw {
            let s = |yy: usize, xx: usize| sat[yy * (w + 1) + xx];
            o[y * rw + x] = s(y + th, x + tw) - s(y, x + tw) - s(y + th, x) + s(y, x);
        }
    }
    o
}

fn planes(m: &Mat, scale: f64) -> Vec<Vec<f64>> {
    (0..m.c).map(|ch| m.data.iter().skip(ch).step_by(m.c).map(|&v| f64::from(v) * scale).collect()).collect()
}

/// matchTemplate(TM_CCOEFF_NORMED), without a mask: OpenCV's normalization (channels summed).
pub fn match_template(img: &Mat, tpl: &Mat) -> MatF {
    let (rw, rh) = (img.w + 1 - tpl.w, img.h + 1 - tpl.h);
    let area = (tpl.w * tpl.h) as f64;
    let ip = planes(img, 1.0);
    let tp = planes(tpl, 1.0);
    let means: Vec<f64> = tp.iter().map(|t| t.iter().sum::<f64>() / area).collect();
    let var: f64 = tp.iter().zip(&means).map(|(t, m)| t.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / area).sum();
    if var < f64::EPSILON {
        return MatF { w: rw, h: rh, c: 1, data: vec![1.0; rw * rh] };
    }
    let tnorm = var.sqrt() / (1.0 / area).sqrt();
    let mut num = vec![0f64; rw * rh];
    let mut wsum2 = vec![0f64; rw * rh];
    let mut wmean2 = vec![0f64; rw * rh];
    for ch in 0..img.c {
        let corr = &xcorr(&ip[ch], img.w, img.h, &[&tp[ch]], tpl.w, tpl.h)[0];
        let s = window_sums(&ip[ch], img.w, img.h, tpl.w, tpl.h);
        let sq: Vec<f64> = ip[ch].iter().map(|v| v * v).collect();
        let s2 = window_sums(&sq, img.w, img.h, tpl.w, tpl.h);
        for i in 0..rw * rh {
            num[i] += corr[i] - s[i] * means[ch];
            wsum2[i] += s2[i];
            wmean2[i] += s[i] * s[i] / area;
        }
    }
    let data = (0..rw * rh).map(|i| {
        let t = (wsum2[i] - wmean2[i]).max(0.0).sqrt() * tnorm;
        let n = num[i];
        (if n.abs() < t { n / t } else if n.abs() < t * 1.125 { n.signum() } else { 0.0 }) as f32
    }).collect();
    MatF { w: rw, h: rh, c: 1, data }
}

/// matchTemplate(TM_CCOEFF_NORMED, mask) with a binary mask (Sanctum Helper's MaskedTemplate arithmetic: 8-bit as
/// 0-1, channels summed). Flat windows give NaN.
pub fn match_template_masked(img: &Mat, tpl: &Mat, mask: &Mat) -> MatF {
    let (rw, rh) = (img.w + 1 - tpl.w, img.h + 1 - tpl.h);
    let m: Vec<f64> = mask.data.iter().map(|&v| if v > 0 { 1.0 } else { 0.0 }).collect();
    let msum = m.iter().sum::<f64>().max(1e-9);
    let ip = planes(img, 1.0 / 255.0);
    let tp = planes(tpl, 1.0 / 255.0);
    let mut tnorm = 0f64;
    let tz: Vec<Vec<f64>> = tp.iter().map(|t| {
        let mean = t.iter().zip(&m).map(|(a, b)| a * b).sum::<f64>() / msum;
        let z: Vec<f64> = t.iter().zip(&m).map(|(a, b)| (a - mean) * b).collect();
        tnorm += z.iter().map(|v| v * v).sum::<f64>();
        z
    }).collect();
    let mut num = vec![0f64; rw * rh];
    let mut sum2 = vec![0f64; rw * rh];
    let mut sqi = vec![0f64; img.w * img.h];
    for ch in 0..img.c {
        let r = xcorr(&ip[ch], img.w, img.h, &[&tz[ch], &m], tpl.w, tpl.h);
        for i in 0..rw * rh {
            num[i] += r[0][i];
            sum2[i] += r[1][i] * r[1][i];
        }
        for (s, v) in sqi.iter_mut().zip(&ip[ch]) {
            *s += v * v;
        }
    }
    let sq = &xcorr(&sqi, img.w, img.h, &[&m], tpl.w, tpl.h)[0];
    let data = (0..rw * rh).map(|i| {
        let d = (sq[i] * tnorm - sum2[i] * tnorm / msum).max(0.0).sqrt();
        if d <= 0.0 { f32::NAN } else { (num[i] / d) as f32 }
    }).collect();
    MatF { w: rw, h: rh, c: 1, data }
}

// ---------------------------------------------------------------------------------------------- CLAHE

/// CLAHE (clip limit 2, 8 x 8 tiles) on an 8-bit gray image, OpenCV's arithmetic.
pub fn clahe(src: &Mat) -> Mat {
    const TILES: usize = 8;
    const HIST: usize = 256;
    let ext = if src.w % TILES == 0 && src.h % TILES == 0 {
        src.clone()
    } else {
        let (ew, eh) = (src.w + TILES - src.w % TILES, src.h + TILES - src.h % TILES);
        let mut e = Mat::new(ew, eh, 1);
        for y in 0..eh {
            for x in 0..ew {
                e.data[y * ew + x] = src.data[reflect(y as i64, src.h as i64) * src.w + reflect(x as i64, src.w as i64)];
            }
        }
        e
    };
    let (tw, th) = (ext.w / TILES, ext.h / TILES);
    let total = tw * th;
    let clip = ((2.0 * total as f64 / HIST as f64) as i64).max(1);
    let lut_scale = (HIST - 1) as f32 / total as f32;
    let mut luts = vec![[0u8; HIST]; TILES * TILES];
    for ty in 0..TILES {
        for tx in 0..TILES {
            let mut hist = [0i64; HIST];
            for y in ty * th..(ty + 1) * th {
                for x in tx * tw..(tx + 1) * tw {
                    hist[ext.data[y * ext.w + x] as usize] += 1;
                }
            }
            let mut clipped = 0i64;
            for v in hist.iter_mut() {
                if *v > clip {
                    clipped += *v - clip;
                    *v = clip;
                }
            }
            let batch = clipped / HIST as i64;
            let mut residual = clipped - batch * HIST as i64;
            for v in hist.iter_mut() {
                *v += batch;
            }
            if residual != 0 {
                let step = ((HIST as i64) / residual).max(1) as usize;
                let mut i = 0usize;
                while i < HIST && residual > 0 {
                    hist[i] += 1;
                    i += step;
                    residual -= 1;
                }
            }
            let mut sum = 0i64;
            for (i, v) in hist.iter().enumerate() {
                sum += v;
                luts[ty * TILES + tx][i] = (sum as f32 * lut_scale).round_ties_even().clamp(0.0, 255.0) as u8;
            }
        }
    }
    let (inv_tw, inv_th) = (1.0f32 / tw as f32, 1.0f32 / th as f32);
    let xs: Vec<(usize, usize, f32, f32)> = (0..src.w).map(|x| {
        let txf = x as f32 * inv_tw - 0.5;
        let tx1 = txf.floor() as i64;
        let xa = txf - tx1 as f32;
        ((tx1.max(0)) as usize, ((tx1 + 1).min(TILES as i64 - 1)) as usize, xa, 1.0 - xa)
    }).collect();
    let mut o = Mat::new(src.w, src.h, 1);
    for y in 0..src.h {
        let tyf = y as f32 * inv_th - 0.5;
        let ty1 = tyf.floor() as i64;
        let ya = tyf - ty1 as f32;
        let ya1 = 1.0 - ya;
        let (t1, t2) = (ty1.max(0) as usize, (ty1 + 1).min(TILES as i64 - 1) as usize);
        for x in 0..src.w {
            let (x1, x2, xa, xa1) = xs[x];
            let v = src.data[y * src.w + x] as usize;
            let l = |ty: usize, tx: usize| f32::from(luts[ty * TILES + tx][v]);
            let r = (l(t1, x1) * xa1 + l(t1, x2) * xa) * ya1 + (l(t2, x1) * xa1 + l(t2, x2) * xa) * ya;
            o.data[y * src.w + x] = r.round_ties_even().clamp(0.0, 255.0) as u8;
        }
    }
    o
}

// ---------------------------------------------------------------------------------------------- phase correlation

/// phaseCorrelate(a, b, window) as Sanctum Helper gets it: the shift of b against a, and the peak's response. The
/// window is ignored, as OpenCvSharp 4.10's PhaseCorrelate does (measured: its results are the unwindowed ones).
pub fn phase_correlate(a: &MatF, b: &MatF, _win: Option<&MatF>) -> (f64, f64, f64) {
    let (w, h) = (a.w, a.h);
    let wv = |_: usize| 1.0;
    let mut fa: Vec<Complex<f64>> = (0..w * h).map(|i| Complex::new(f64::from(a.data[i]) * wv(i), 0.0)).collect();
    let mut fb: Vec<Complex<f64>> = (0..w * h).map(|i| Complex::new(f64::from(b.data[i]) * wv(i), 0.0)).collect();
    fft2(&mut fa, w, h, false);
    fft2(&mut fb, w, h, false);
    let mut c: Vec<Complex<f64>> = fa.iter().zip(&fb).map(|(x, y)| {
        let p = x * y.conj();
        let m = p.norm();
        if m > 0.0 { p / m } else { Complex::new(0.0, 0.0) }
    }).collect();
    fft2(&mut c, w, h, true);
    // fftshift, then the peak and its 5 x 5 weighted centroid
    let mut s = vec![0f64; w * h];
    for y in 0..h {
        for x in 0..w {
            s[((y + h / 2) % h) * w + (x + w / 2) % w] = c[y * w + x].re / (w * h) as f64;
        }
    }
    let (mut best, mut bx, mut by) = (f64::MIN, 0usize, 0usize);
    for (i, &v) in s.iter().enumerate() {
        if v > best {
            best = v;
            bx = i % w;
            by = i / w;
        }
    }
    let (mut sum, mut sx, mut sy) = (0f64, 0f64, 0f64);
    for y in by.saturating_sub(2)..(by + 3).min(h) {
        for x in bx.saturating_sub(2)..(bx + 3).min(w) {
            let v = s[y * w + x];
            sum += v;
            sx += v * x as f64;
            sy += v * y as f64;
        }
    }
    let (cx, cy) = (sx / sum, sy / sum);
    ((w / 2) as f64 - cx, (h / 2) as f64 - cy, sum)
}

// ---------------------------------------------------------------------------------------------- warpAffine

/// warpAffine with a forward 2 x 3 matrix, constant 0 border, nearest or bilinear (OpenCV's fixed point).
pub fn warp_affine(src: &Mat, m: [f64; 6], dw: usize, dh: usize, linear: bool) -> Mat {
    let d = m[0] * m[4] - m[1] * m[3];
    let d = if d != 0.0 { 1.0 / d } else { 0.0 };
    let (a11, a22) = (m[4] * d, m[0] * d);
    let (a12, a21) = (m[1] * -d, m[3] * -d);
    let b1 = -a11 * m[2] - a12 * m[5];
    let b2 = -a21 * m[2] - a22 * m[5];
    let inv = [a11, a12, b1, a21, a22, b2];
    const AB_BITS: i32 = 10;
    const AB_SCALE: f64 = 1024.0;
    const INTER_BITS: i32 = 5;
    const TAB: i32 = 32;
    let round_delta: i64 = if linear { (AB_SCALE as i64) / TAB as i64 / 2 } else { (AB_SCALE as i64) / 2 };
    let c = src.c;
    // bilinear weights: 2 x 2 per 1/32 position, in 1/32768, summing to 32768
    let wtab: Vec<[i32; 4]> = (0..TAB * TAB).map(|k| {
        let (ay, ax) = ((k / TAB) as f32 / TAB as f32, (k % TAB) as f32 / TAB as f32);
        let vy = [1.0 - ay, ay];
        let vx = [1.0 - ax, ax];
        let mut t = [0i32; 4];
        for i in 0..2 {
            for j in 0..2 {
                t[i * 2 + j] = (vy[i] * vx[j] * 32768.0).round_ties_even() as i32;
            }
        }
        let sum: i32 = t.iter().sum();
        if sum != 32768 {
            let diff = sum - 32768;
            let (mut mi, mut ma) = (0usize, 0usize);
            for i in 0..4 {
                if t[i] < t[mi] {
                    mi = i;
                } else if t[i] > t[ma] {
                    ma = i;
                }
            }
            if diff < 0 { t[ma] -= diff } else { t[mi] -= diff }
        }
        t
    }).collect();
    let mut o = Mat::new(dw, dh, c);
    let px = |x: i64, y: i64, ch: usize| -> i32 {
        if x < 0 || y < 0 || x >= src.w as i64 || y >= src.h as i64 { 0 } else { i32::from(src.data[(y as usize * src.w + x as usize) * c + ch]) }
    };
    for y in 0..dh {
        let x0 = round_i((inv[1] * y as f64 + inv[2]) * AB_SCALE) + round_delta;
        let y0 = round_i((inv[4] * y as f64 + inv[5]) * AB_SCALE) + round_delta;
        for x in 0..dw {
            let adx = round_i(inv[0] * x as f64 * AB_SCALE);
            let bdx = round_i(inv[3] * x as f64 * AB_SCALE);
            for ch in 0..c {
                let v = if linear {
                    let xx = (x0 + adx) >> (AB_BITS - INTER_BITS);
                    let yy = (y0 + bdx) >> (AB_BITS - INTER_BITS);
                    let (sx, sy) = (xx >> INTER_BITS, yy >> INTER_BITS);
                    let w = wtab[(((yy & (TAB as i64 - 1)) * TAB as i64) + (xx & (TAB as i64 - 1))) as usize];
                    if sx >= src.w as i64 || sx + 1 < 0 || sy >= src.h as i64 || sy + 1 < 0 {
                        0
                    } else {
                        let s = px(sx, sy, ch) * w[0] + px(sx + 1, sy, ch) * w[1] + px(sx, sy + 1, ch) * w[2] + px(sx + 1, sy + 1, ch) * w[3];
                        ((s + (1 << 14)) >> 15).clamp(0, 255) as u8
                    }
                } else {
                    let sx = (x0 + adx) >> AB_BITS;
                    let sy = (y0 + bdx) >> AB_BITS;
                    px(sx, sy, ch) as u8
                };
                o.data[(y * dw + x) * c + ch] = v;
            }
        }
    }
    o
}
