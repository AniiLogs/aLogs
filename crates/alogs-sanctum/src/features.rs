//! Feature matching for the full map when shapes alone can't place it (the island, and a Sanctum map with one
//! candidate and no doors): ORB key points (FAST corners ranked by Harris, oriented, rotated BRIEF), Hamming
//! nearest neighbours with a ratio test, and a similarity transform fitted by RANSAC. Port of `ArtFeatureCache`,
//! `Vision.Fit` and `MapMatcher` from `Vision.cs`, with OpenCV's parameters (pyramid 1.2 x 8, edge 15, patch 31,
//! FAST 12, Harris). The BRIEF test pattern is our own (both sides are computed here), so descriptors are not
//! OpenCV's bit for bit; matching results are equivalent.

use crate::cv::{self, Interp, Mat};
use crate::cv2;
use crate::layout_db::Layout;
use crate::vision::{MapArtCache, MapPose};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

const PATCH: i64 = 31;
const HALF: i64 = PATCH / 2;
const BORDER: i64 = 16;
const FAST_T: i16 = 12;
const HARRIS_BLOCK: i64 = 7;

pub type Desc = [u64; 4];

#[derive(Clone, Copy, Debug)]
pub struct KeyPoint {
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub response: f32,
    pub level: usize,
}

/// 256 point pairs within the patch (radius 13, so every rotation stays inside it).
fn pattern() -> &'static Vec<[(i32, i32); 2]> {
    static P: OnceLock<Vec<[(i32, i32); 2]>> = OnceLock::new();
    P.get_or_init(|| {
        let mut s = 0x9E37_79B9_7F4A_7C15u64;
        let mut uni = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut gauss = move || {
            loop {
                // Box-Muller, sigma = patch / 5, kept inside radius 13
                let (u, v) = (uni().max(1e-12), uni());
                let r = (-2.0 * u.ln()).sqrt() * (PATCH as f64 / 5.0);
                let (x, y) = ((r * (std::f64::consts::TAU * v).cos()).round() as i32, (r * (std::f64::consts::TAU * v).sin()).round() as i32);
                if x * x + y * y <= 13 * 13 {
                    return (x, y);
                }
            }
        };
        let mut out = Vec::with_capacity(256);
        while out.len() < 256 {
            let (a, b) = (gauss(), gauss());
            if a != b {
                out.push([a, b]);
            }
        }
        out
    })
}

const CIRCLE: [(i64, i64); 16] = [(0, -3), (1, -3), (2, -2), (3, -1), (3, 0), (3, 1), (2, 2), (1, 3), (0, 3), (-1, 3), (-2, 2), (-3, 1), (-3, 0), (-3, -1), (-2, -2), (-1, -3)];

/// FAST-9 score (largest threshold at which the pixel is still a corner, minus one), or 0 when it isn't one.
fn fast_score(img: &Mat, x: i64, y: i64) -> i16 {
    let w = img.w as i64;
    let p = i16::from(img.data[(y * w + x) as usize]);
    let mut d = [0i16; 16];
    for (i, (dx, dy)) in CIRCLE.iter().enumerate() {
        d[i] = i16::from(img.data[((y + dy) * w + x + dx) as usize]) - p;
    }
    // quick reject: of the four compass points at least two must pass
    let pass = |v: i16| v > FAST_T || v < -FAST_T;
    if [d[0], d[4], d[8], d[12]].iter().filter(|&&v| pass(v)).count() < 2 {
        return 0;
    }
    let mut best = 0i16;
    for k in 0..16 {
        let (mut lo, mut hi) = (i16::MAX, i16::MAX);
        for j in 0..9 {
            let v = d[(k + j) % 16];
            lo = lo.min(v);
            hi = hi.min(-v);
        }
        best = best.max(lo).max(hi);
    }
    if best > FAST_T { best - 1 } else { 0 }
}

fn fast(img: &Mat) -> Vec<(i64, i64, i16)> {
    let (w, h) = (img.w as i64, img.h as i64);
    if w <= 2 * BORDER || h <= 2 * BORDER {
        return vec![];
    }
    let mut score = vec![0i16; (w * h) as usize];
    for y in BORDER..h - BORDER {
        for x in BORDER..w - BORDER {
            score[(y * w + x) as usize] = fast_score(img, x, y);
        }
    }
    let mut out = vec![];
    for y in BORDER..h - BORDER {
        for x in BORDER..w - BORDER {
            let s = score[(y * w + x) as usize];
            if s == 0 {
                continue;
            }
            let mut max = true;
            'n: for dy in -1..=1 {
                for dx in -1..=1 {
                    if (dx, dy) != (0, 0) && score[((y + dy) * w + x + dx) as usize] >= s && (dy < 0 || (dy == 0 && dx < 0) || score[((y + dy) * w + x + dx) as usize] > s) {
                        max = false;
                        break 'n;
                    }
                }
            }
            if max {
                out.push((x, y, s));
            }
        }
    }
    out
}

/// Harris corner response over a 7 x 7 block (Sobel gradients), as ORB ranks its corners.
fn harris(img: &Mat, x: i64, y: i64) -> f32 {
    let w = img.w as i64;
    let at = |xx: i64, yy: i64| f64::from(img.data[(yy * w + xx) as usize]);
    let r = HARRIS_BLOCK / 2;
    let (mut a, mut b, mut c) = (0.0, 0.0, 0.0);
    for yy in y - r..=y + r {
        for xx in x - r..=x + r {
            let dx = (at(xx + 1, yy - 1) + 2.0 * at(xx + 1, yy) + at(xx + 1, yy + 1)) - (at(xx - 1, yy - 1) + 2.0 * at(xx - 1, yy) + at(xx - 1, yy + 1));
            let dy = (at(xx - 1, yy + 1) + 2.0 * at(xx, yy + 1) + at(xx + 1, yy + 1)) - (at(xx - 1, yy - 1) + 2.0 * at(xx, yy - 1) + at(xx + 1, yy - 1));
            a += dx * dx;
            b += dy * dy;
            c += dx * dy;
        }
    }
    let s = 1.0 / (4.0 * HARRIS_BLOCK as f64 * 255.0);
    let (a, b, c) = (a * s * s, b * s * s, c * s * s);
    (a * b - c * c - 0.04 * (a + b) * (a + b)) as f32
}

/// Orientation by intensity centroid over the patch's circle, in degrees.
fn ic_angle(img: &Mat, x: i64, y: i64) -> f32 {
    let w = img.w as i64;
    let (mut m01, mut m10) = (0i64, 0i64);
    for dy in -HALF..=HALF {
        let span = ((HALF * HALF - dy * dy) as f64).sqrt() as i64;
        for dx in -span..=span {
            let v = i64::from(img.data[((y + dy) * w + x + dx) as usize]);
            m10 += dx * v;
            m01 += dy * v;
        }
    }
    let a = (m01 as f32).atan2(m10 as f32).to_degrees();
    if a < 0.0 { a + 360.0 } else { a }
}

/// 7 x 7 Gaussian (sigma 2) with reflect-101 borders, before the BRIEF tests.
fn gauss7(img: &Mat) -> Mat {
    let k: Vec<f32> = {
        let v: Vec<f32> = (-3..=3).map(|i: i32| (-(i * i) as f32 / 8.0).exp()).collect();
        let s: f32 = v.iter().sum();
        v.into_iter().map(|x| x / s).collect()
    };
    let (w, h) = (img.w as i64, img.h as i64);
    let refl = |i: i64, n: i64| -> i64 {
        if n == 1 { 0 } else if i < 0 { (-i).min(n - 1) } else if i >= n { (2 * n - 2 - i).max(0) } else { i }
    };
    let mut tmp = vec![0f32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            tmp[(y * w + x) as usize] = (0..7).map(|j| k[j] * f32::from(img.data[(y * w + refl(x + j as i64 - 3, w)) as usize])).sum();
        }
    }
    let mut out = Mat::new(img.w, img.h, 1);
    for y in 0..h {
        for x in 0..w {
            let v: f32 = (0..7).map(|j| k[j] * tmp[(refl(y + j as i64 - 3, h) * w + x) as usize]).sum();
            out.data[(y * w + x) as usize] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn brief(img: &Mat, x: i64, y: i64, angle: f32) -> Desc {
    let (s, c) = angle.to_radians().sin_cos();
    let w = img.w as i64;
    let at = |(px, py): (i32, i32)| {
        let (px, py) = (px as f32, py as f32);
        let xx = (px * c - py * s).round() as i64;
        let yy = (px * s + py * c).round() as i64;
        img.data[((y + yy) * w + x + xx) as usize]
    };
    let mut d = [0u64; 4];
    for (i, p) in pattern().iter().enumerate() {
        if at(p[0]) < at(p[1]) {
            d[i / 64] |= 1 << (i % 64);
        }
    }
    d
}

/// ORB on an 8-bit gray image: up to `n` key points (in that image's pixels) and their descriptors.
pub fn orb(gray: &Mat, n: usize) -> (Vec<KeyPoint>, Vec<Desc>) {
    const LEVELS: usize = 8;
    const SCALE: f64 = 1.2;
    let f = 1.0 / SCALE;
    let mut per = vec![0usize; LEVELS];
    let mut want = n as f64 * (1.0 - f) / (1.0 - f.powi(LEVELS as i32));
    let mut sum = 0;
    for p in per.iter_mut().take(LEVELS - 1) {
        *p = want.round() as usize;
        sum += *p;
        want *= f;
    }
    per[LEVELS - 1] = n.saturating_sub(sum);
    let (mut kps, mut descs) = (vec![], vec![]);
    for (lvl, &quota) in per.iter().enumerate() {
        let sc = SCALE.powi(lvl as i32);
        let (lw, lh) = ((gray.w as f64 / sc).round() as usize, (gray.h as f64 / sc).round() as usize);
        if lw <= 2 * BORDER as usize || lh <= 2 * BORDER as usize || quota == 0 {
            break;
        }
        let img = if lvl == 0 { gray.clone() } else { cv::resize(gray, lw, lh, 0.0, 0.0, Interp::Linear) };
        let mut cands = fast(&img);
        // keep the strongest FAST corners, then rank those by Harris
        cands.sort_by(|a, b| b.2.cmp(&a.2));
        cands.truncate(quota * 2);
        let mut ranked: Vec<(i64, i64, f32)> = cands.iter().map(|&(x, y, _)| (x, y, harris(&img, x, y))).collect();
        ranked.sort_by(|a, b| b.2.total_cmp(&a.2));
        ranked.truncate(quota);
        let blurred = gauss7(&img);
        for (x, y, r) in ranked {
            let angle = ic_angle(&img, x, y);
            descs.push(brief(&blurred, x, y, angle));
            kps.push(KeyPoint { x: (x as f64 * sc) as f32, y: (y as f64 * sc) as f32, angle, response: r, level: lvl });
        }
    }
    (kps, descs)
}

fn hamming(a: &Desc, b: &Desc) -> u32 {
    (a[0] ^ b[0]).count_ones() + (a[1] ^ b[1]).count_ones() + (a[2] ^ b[2]).count_ones() + (a[3] ^ b[3]).count_ones()
}

/// The two nearest train descriptors of each query one: (query, best, best distance, second distance).
pub fn knn2(query: &[Desc], train: &[Desc]) -> Vec<(usize, usize, u32, u32)> {
    query.iter().enumerate().filter_map(|(qi, q)| {
        let (mut b1, mut d1, mut d2) = (usize::MAX, u32::MAX, u32::MAX);
        for (ti, t) in train.iter().enumerate() {
            let d = hamming(q, t);
            if d < d1 {
                d2 = d1;
                d1 = d;
                b1 = ti;
            } else if d < d2 {
                d2 = d;
            }
        }
        (d2 != u32::MAX).then_some((qi, b1, d1, d2))
    }).collect()
}

/// Similarity (rotation, uniform scale, shift) dst = [a -b; b a] src + t by least squares.
fn fit_similarity(src: &[(f64, f64)], dst: &[(f64, f64)], idx: &[usize]) -> Option<[f64; 4]> {
    let n = idx.len() as f64;
    if idx.len() < 2 {
        return None;
    }
    let (mut sx, mut sy, mut dx, mut dy) = (0.0, 0.0, 0.0, 0.0);
    for &i in idx {
        sx += src[i].0;
        sy += src[i].1;
        dx += dst[i].0;
        dy += dst[i].1;
    }
    let (sx, sy, dx, dy) = (sx / n, sy / n, dx / n, dy / n);
    let (mut num_a, mut num_b, mut den) = (0.0, 0.0, 0.0);
    for &i in idx {
        let (px, py) = (src[i].0 - sx, src[i].1 - sy);
        let (qx, qy) = (dst[i].0 - dx, dst[i].1 - dy);
        num_a += px * qx + py * qy;
        num_b += px * qy - py * qx;
        den += px * px + py * py;
    }
    if den < 1e-9 {
        return None;
    }
    let (a, b) = (num_a / den, num_b / den);
    Some([a, b, dx - (a * sx - b * sy), dy - (b * sx + a * sy)])
}

fn inliers(m: &[f64; 4], src: &[(f64, f64)], dst: &[(f64, f64)], thr: f64) -> Vec<usize> {
    let t2 = thr * thr;
    (0..src.len()).filter(|&i| {
        let (x, y) = src[i];
        let (ex, ey) = (m[0] * x - m[1] * y + m[2] - dst[i].0, m[1] * x + m[0] * y + m[3] - dst[i].1);
        ex * ex + ey * ey <= t2
    }).collect()
}

/// RANSAC over two-point samples (up to `iters`, stopping early at the confidence), then refined on the inliers:
/// the model [a, b, tx, ty] and its inlier count. As EstimateAffinePartial2D(RANSAC, thr, 3000, 0.995, 10).
pub fn estimate_similarity(src: &[(f64, f64)], dst: &[(f64, f64)], thr: f64, iters: usize, conf: f64, refine: usize) -> Option<([f64; 4], usize)> {
    let n = src.len();
    if n < 2 {
        return None;
    }
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut rnd = move |k: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % k as u64) as usize
    };
    let (mut best, mut best_in): (Option<[f64; 4]>, Vec<usize>) = (None, vec![]);
    let mut max_iters = iters;
    let mut it = 0;
    while it < max_iters {
        it += 1;
        let i = rnd(n);
        let j = rnd(n);
        if i == j {
            continue;
        }
        let Some(m) = fit_similarity(src, dst, &[i, j]) else { continue };
        let inl = inliers(&m, src, dst, thr);
        if inl.len() > best_in.len() {
            best = Some(m);
            best_in = inl;
            let ep = 1.0 - best_in.len() as f64 / n as f64;
            let denom = (1.0 - (1.0 - ep).powi(2)).ln();
            if denom < 0.0 {
                let need = ((1.0 - conf).ln() / denom).ceil();
                if need.is_finite() && need >= 0.0 {
                    max_iters = max_iters.min(need as usize);
                }
            } else {
                break;
            }
        }
    }
    let mut m = best?;
    for _ in 0..refine {
        let Some(r) = fit_similarity(src, dst, &best_in) else { break };
        let inl = inliers(&r, src, dst, thr);
        if inl.len() < best_in.len() {
            break;
        }
        let same = inl == best_in;
        m = r;
        best_in = inl;
        if same {
            break;
        }
    }
    Some((m, best_in.len()))
}

/// Key points of a layout's art (full-size art pixels) and their descriptors.
pub struct Features {
    pub desc: Vec<Desc>,
    pub full_pts: Vec<(f64, f64)>,
}

/// Each layout's art features, made on first use (art at 1/2 and 1/4 size, CLAHE, ORB 3000).
pub struct ArtFeatureCache {
    arts: Arc<MapArtCache>,
    cache: Mutex<HashMap<i64, Arc<Features>>>,
}

impl ArtFeatureCache {
    pub fn new(arts: Arc<MapArtCache>) -> ArtFeatureCache {
        ArtFeatureCache { arts, cache: Mutex::new(HashMap::new()) }
    }

    pub fn get(&self, l: &Layout) -> Option<Arc<Features>> {
        if let Some(f) = self.cache.lock().unwrap().get(&l.number) {
            return Some(f.clone());
        }
        let gray = self.arts.get(l, 0)?.gray.clone();
        let (mut desc, mut full_pts) = (vec![], vec![]);
        for s in [0.5, 0.25] {
            let small = cv::resize(&gray, 0, 0, s, s, Interp::Area);
            let eq = cv2::clahe(&small);
            let (k, d) = orb(&eq, 3000);
            full_pts.extend(k.iter().map(|p| (f64::from(p.x) / s, f64::from(p.y) / s)));
            desc.extend(d);
        }
        let f = Arc::new(Features { desc, full_pts });
        self.cache.lock().unwrap().insert(l.number, f.clone());
        Some(f)
    }

    pub fn prepare<'a>(&self, layouts: impl IntoIterator<Item = &'a Layout>) {
        for l in layouts {
            self.get(l);
        }
    }

    pub fn trim(&self, keep: impl Fn(i64) -> bool) {
        self.cache.lock().unwrap().retain(|k, _| keep(*k));
    }
}

/// Screen key points -> art: similarity [a, b, tx, ty] (art = [a -b; b a] q + t), inliers and matches.
pub fn fit(kps: &[KeyPoint], desc: &[Desc], query_scale: f64, art: &Features, ratio: f64, reproj: f64) -> Option<([f64; 4], usize, usize)> {
    if art.desc.is_empty() || desc.is_empty() {
        return None;
    }
    let (mut src, mut dst) = (vec![], vec![]);
    for (qi, ti, d1, d2) in knn2(desc, &art.desc) {
        if f64::from(d1) > ratio * f64::from(d2) {
            continue;
        }
        src.push((f64::from(kps[qi].x) / query_scale, f64::from(kps[qi].y) / query_scale));
        dst.push(art.full_pts[ti]);
    }
    if src.len() < 6 {
        return None;
    }
    let (m, n) = estimate_similarity(&src, &dst, reproj, 3000, 0.995, 10)?;
    Some((m, n, src.len()))
}

/// Places the full map by its features: the candidate layout with the most consistent matches.
pub struct MapMatcher {
    features: Arc<ArtFeatureCache>,
    pub last_info: String,
}

impl MapMatcher {
    pub fn new(features: Arc<ArtFeatureCache>) -> MapMatcher {
        MapMatcher { features, last_info: String::new() }
    }

    pub fn match_map(&mut self, frame: &Mat, candidates: &[&Layout], previous: Option<&MapPose>) -> Option<MapPose> {
        if candidates.is_empty() {
            return None;
        }
        let f = (1080.0 / frame.h as f64).min(1.0);
        let gray = if frame.c == 1 { frame.clone() } else { cv::bgr2gray(frame) };
        let small = cv::resize(&gray, 0, 0, f, f, Interp::Area);
        let eq = cv2::clahe(&small);
        let (kps, desc) = orb(&eq, 6000);
        if desc.is_empty() || kps.len() < 30 {
            self.last_info = "too few features on screen".into();
            return None;
        }
        let mut order: Vec<&Layout> = candidates.to_vec();
        if let Some(p) = previous {
            order.sort_by_key(|c| if c.number == p.layout { 0 } else { 1 });
        }
        let (mut best, mut second): (Option<MapPose>, usize) = (None, 0);
        let mut info = vec![];
        for l in order {
            let Some(art) = self.features.get(l) else {
                info.push(format!("{}:-", l.number));
                continue;
            };
            let Some(([a, b, tx, ty], inl, matches)) = fit(&kps, &desc, f, &art, 0.78, 4.0) else {
                info.push(format!("{}:-", l.number));
                continue;
            };
            let k = (a * a + b * b).sqrt();
            let rot = b.atan2(a).to_degrees();
            info.push(format!("{}:{inl}/{matches} s{:.2} r{rot:.1}", l.number, 1.0 / k));
            // the game never rotates its full map, and its zoom keeps the art between about 0.15x and 6x
            if rot.abs() > 3.0 || k < 1.0 / 6.0 || k > 1.0 / 0.15 {
                continue;
            }
            let scale = 1.0 / k;
            if best.as_ref().is_none_or(|bp| inl as i64 > bp.inliers) {
                second = best.as_ref().map_or(second, |bp| bp.inliers as usize);
                best = Some(MapPose { layout: l.number, scale, tx: -tx * scale, ty: -ty * scale, inliers: inl as i64, rotation: rot });
            } else {
                second = second.max(inl);
            }
            if previous.is_some_and(|p| p.layout == l.number) && inl >= 40 {
                break;
            }
        }
        self.last_info = info.join("  ");
        let b = best?;
        if b.inliers < 14 || (b.inliers as f64) < second as f64 * 1.5 {
            return None;
        }
        Some(b)
    }
}
