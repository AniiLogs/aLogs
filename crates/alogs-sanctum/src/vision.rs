//! Reading the game's maps: the layout art prepared for matching, where the player is on the minimap (Sanctum and
//! island), which layout the open map shows and where (from its door icons), the map's exact placement (from its
//! explored floor), the game's own icons, and which way the player faces. Port of `Vision.cs` with every Dev switch
//! off (Sanctum Helper's default). The ORB matcher (MapMatcher) is in `features.rs`.

use crate::cv::{self, Interp, Mat, MatF};
use crate::cv2::{self, Kernel, Masked};
use crate::layout_db::{Layout, LayoutDb};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub type P2 = (f64, f64);

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// Where a layout's map image sits on screen: screen = image * scale + (tx, ty), in client pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct MapPose {
    pub layout: i64,
    pub scale: f64,
    pub tx: f64,
    pub ty: f64,
    pub inliers: i64,
    pub rotation: f64,
}

impl MapPose {
    pub fn to_screen(&self, ix: f64, iy: f64) -> P2 {
        (ix * self.scale + self.tx, iy * self.scale + self.ty)
    }
}

/// The game's UI area in a client: a 16:9 box, as tall as the window and centred on wider screens, as wide and kept
/// to the top on narrower ones. (x, y, w, h).
pub fn hud(w: usize, h: usize) -> (usize, usize, usize, usize) {
    let u = (w as f64).min(h as f64 * 16.0 / 9.0);
    (((w as f64 - u) / 2.0).round() as usize, 0, u.round() as usize, h.min((u * 9.0 / 16.0).round() as usize))
}

pub fn ui_scale(w: usize, h: usize) -> f64 {
    hud(w, h).3 as f64 / 1080.0
}

/// A PNG as OpenCV's IMREAD_UNCHANGED gives it: BGR or BGRA (8 bits).
pub fn imread(path: &Path) -> Option<Mat> {
    let mut d = png::Decoder::new(std::fs::File::open(path).ok()?);
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let data = &buf[..info.buffer_size()];
    let (c, out): (usize, Vec<u8>) = match info.color_type {
        png::ColorType::Rgba => (4, data.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect()),
        png::ColorType::Rgb => (3, data.chunks_exact(3).flat_map(|p| [p[2], p[1], p[0]]).collect()),
        png::ColorType::GrayscaleAlpha => (4, data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()),
        _ => (1, data.to_vec()),
    };
    Some(Mat { w, h, c, data: out })
}

/// The map art prepared for matching: on the dark panel the game draws it on (gray and color), and its alpha.
pub struct MapArt {
    pub gray: Mat,
    pub alpha: Mat,
    path: PathBuf,
    pad: (usize, usize),
    bgr: Mutex<Option<Arc<Mat>>>,
}

/// out = (channel - 20) * alpha / 255 + 20, rounded.
fn on_background(ch: &[u8], alpha: &[u8]) -> Vec<u8> {
    let s = (1.0 / 255.0) as f32;
    ch.iter().zip(alpha).map(|(&c, &a)| ((f32::from(c) - 20.0) * (f32::from(a) * s) + 20.0).round_ties_even().clamp(0.0, 255.0) as u8).collect()
}

impl MapArt {
    fn read_padded(path: &Path, pad: (usize, usize)) -> Option<Mat> {
        let raw = imread(path)?;
        let raw = if raw.c == 4 { raw } else {
            let bgr = if raw.c == 1 { cv::gray2bgr(&raw) } else { raw };
            Mat { w: bgr.w, h: bgr.h, c: 4, data: bgr.data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() }
        };
        if raw.w >= pad.0 && raw.h >= pad.1 {
            return Some(raw);
        }
        // upper floors can be cropped at the bottom / right: padded to the ground floor's size
        let (w, h) = (raw.w.max(pad.0), raw.h.max(pad.1));
        let mut o = Mat::new(w, h, 4);
        for y in 0..raw.h {
            o.data[y * w * 4..(y * w + raw.w) * 4].copy_from_slice(raw.row(y));
        }
        Some(o)
    }

    pub fn load(path: &Path, pad: (usize, usize)) -> Option<MapArt> {
        let rgba = Self::read_padded(path, pad)?;
        let alpha = rgba.channel(3);
        let g = cv::bgr2gray(&rgba);
        let gray = Mat { data: on_background(&g.data, &alpha.data), ..g };
        Some(MapArt { gray, alpha, path: path.to_path_buf(), pad, bgr: Mutex::new(None) })
    }

    /// 1 where the art has walkable floor (gray over 120 and alpha over 128), else 0.
    pub fn floor_binary(&self) -> Mat {
        Mat { data: self.gray.data.iter().zip(&self.alpha.data).map(|(&g, &a)| u8::from(g > 120 && a > 128)).collect(), ..self.gray }
    }

    /// The color art on the map's dark background (made on first use).
    pub fn bgr(&self) -> Arc<Mat> {
        let mut g = self.bgr.lock().unwrap();
        if let Some(b) = g.as_ref() {
            return b.clone();
        }
        let rgba = Self::read_padded(&self.path, self.pad).unwrap_or_else(|| Mat::new(1, 1, 4));
        let a = rgba.channel(3);
        let chans: Vec<Vec<u8>> = (0..3).map(|c| on_background(&rgba.channel(c).data, &a.data)).collect();
        let data = (0..rgba.w * rgba.h).flat_map(|i| [chans[0][i], chans[1][i], chans[2][i]]).collect();
        let m = Arc::new(Mat { w: rgba.w, h: rgba.h, c: 3, data });
        *g = Some(m.clone());
        m
    }
}

/// The map art of each layout and floor, loaded on first use.
pub struct MapArtCache {
    db: Arc<LayoutDb>,
    art: Mutex<HashMap<(i64, usize), Arc<MapArt>>>,
}

impl MapArtCache {
    pub fn new(db: Arc<LayoutDb>) -> MapArtCache {
        MapArtCache { db, art: Mutex::new(HashMap::new()) }
    }

    pub fn get(&self, l: &Layout, floor: usize) -> Option<Arc<MapArt>> {
        if let Some(a) = self.art.lock().unwrap().get(&(l.number, floor)) {
            return Some(a.clone());
        }
        let pad = (l.image_size.first().copied().unwrap_or(0) as usize, l.image_size.get(1).copied().unwrap_or(0) as usize);
        let a = Arc::new(MapArt::load(&self.db.floor_path(l, floor), pad)?);
        self.art.lock().unwrap().insert((l.number, floor), a.clone());
        Some(a)
    }

    /// Lets go of every layout's art but the ones kept.
    pub fn trim(&self, keep: impl Fn(i64) -> bool) {
        self.art.lock().unwrap().retain(|k, _| keep(k.0));
    }

    pub fn loaded(&self) -> usize {
        self.art.lock().unwrap().len()
    }
}

fn planes_u8(m: &Mat) -> Vec<Vec<f64>> {
    (0..m.c).map(|ch| m.data.iter().skip(ch).step_by(m.c).map(|&v| f64::from(v) / 255.0).collect()).collect()
}

/// The best score and its spot, and a subpixel offset from the neighbours (parabola), in a cleaned score map.
fn peak(res: &MatF) -> (f32, (usize, usize), f64, f64) {
    let (_, max, _, loc) = res.min_max_loc();
    let (x, y) = loc;
    let at = |xx: usize, yy: usize| f64::from(res.data[yy * res.w + xx]);
    let (mut sx, mut sy) = (0.0, 0.0);
    if x > 0 && x + 1 < res.w {
        let d = at(x - 1, y) - 2.0 * at(x, y) + at(x + 1, y);
        if d < 0.0 {
            sx = (0.5 * (at(x - 1, y) - at(x + 1, y)) / d).clamp(-0.5, 0.5);
        }
    }
    if y > 0 && y + 1 < res.h {
        let d = at(x, y - 1) - 2.0 * at(x, y) + at(x, y + 1);
        if d < 0.0 {
            sy = (0.5 * (at(x, y - 1) - at(x, y + 1)) / d).clamp(-0.5, 0.5);
        }
    }
    (max, loc, sx, sy)
}

/// (x, y, w, h) intersection, as OpenCV's Rect.Intersect (empty when they don't meet).
fn intersect(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> (i64, i64, i64, i64) {
    let (x0, y0) = (a.0.max(b.0), a.1.max(b.1));
    let (x1, y1) = ((a.0 + a.2).min(b.0 + b.2), (a.1 + a.3).min(b.1 + b.3));
    if x1 <= x0 || y1 <= y0 { (0, 0, 0, 0) } else { (x0, y0, x1 - x0, y1 - y0) }
}

/// The colored parts of map icons (blue/cyan doors and eggs, the orange door), dilated.
pub fn icon_mask(hsv: &Mat, margin: usize) -> Mat {
    let m = cv::bitwise_or(&cv::in_range(hsv, &[85, 56, 91], &[130, 255, 255]), &cv::in_range(hsv, &[0, 121, 91], &[22, 255, 255]));
    cv2::dilate(&m, &Kernel::rect(margin, margin))
}

/// Floor shape pixels of the minimap (the Sanctum tracker's reading of it): floor, compare mask, and how much floor.
fn minimap_floor(small: &Mat, work: f64) -> (Mat, Mat, usize) {
    let hsv = cv::bgr2hsv(small);
    let mut disc = Mat::new(small.w, small.h, 1);
    let (cx, cy) = ((small.w / 2) as i64, (small.h / 2) as i64);
    let r = (small.w.min(small.h) as f64 * 0.44) as i64;
    cv2::circle(&mut disc, cx, cy, r, 255);
    cv2::circle(&mut disc, cx, cy, (r / 6).max(4), 0);
    let grey = cv::bitwise_and(&cv::in_range(&hsv, &[0, 0, 0], &[180, 40, 255]), &disc);
    let v = hsv.channel(2);
    let mut cut = 131.0;
    if grey.count_nonzero() > 50 {
        let sel = Mat { w: grey.count_nonzero(), h: 1, c: 1, data: v.data.iter().zip(&grey.data).filter(|(_, g)| **g != 0).map(|(x, _)| *x).collect() };
        cut = cv::otsu(&sel).max(70.0);
    }
    let lo = |c: f64| (c + 1.0).min(255.0) as u8;
    let mut floor = cv::in_range(&hsv, &[0, 0, lo(cut)], &[180, 40, 255]);
    let mut tan = cv::in_range(&hsv, &[5, 0, lo(cut.max(110.0))], &[30, 95, 255]);
    {
        let vf = v.to_f32(1.0);
        let fw = ((8.0 * work).round() as usize).max(3);
        let mean = cv2::blur(&vf, fw);
        let sqv = MatF { data: vf.data.iter().map(|x| x * x).collect(), ..vf.clone() };
        let sq = cv2::blur(&sqv, fw);
        for i in 0..tan.data.len() {
            let var = sq.data[i] - mean.data[i] * mean.data[i];
            if var > 100.0 {
                tan.data[i] = 0;
            }
        }
    }
    floor = cv::bitwise_and(&cv::bitwise_or(&floor, &tan), &disc);
    let icons = icon_mask(&hsv, ((15.0 * work) as usize).max(5));
    for i in 0..floor.data.len() {
        if icons.data[i] != 0 {
            floor.data[i] = 0;
            disc.data[i] = 0;
        }
    }
    let n = floor.count_nonzero();
    let band = (((10.0 * work).round() as usize) | 1).max(1);
    let mask = cv::bitwise_and(&cv2::dilate(&floor, &Kernel::rect(band, band)), &disc);
    (floor, mask, n)
}

/// Finds the player from the minimap once the layout is known: the explored floor's shape against the layout's.
pub struct MinimapTracker {
    arts: Arc<MapArtCache>,
    art_floor: HashMap<(i64, usize, u64, u64), MatF>,
    pub floor: usize,
    pending_floor: usize,
    floor_votes: i64,
    scale: Option<f64>,
    last: Option<P2>,
    jump_candidate: Option<P2>,
    last_ok: f64,
    seeded_at: f64,
    other_floors_at: f64,
    floor_vote_since: f64,
    jump_since: f64,
    pub last_score: f64,
    pub last_info: String,
    vel: P2,
    filter_time: f64,
}

const SEARCH_WORK: f64 = 0.5;
const TRACK_WORK: f64 = 1.0;
const SEARCH_SCALES: [f64; 5] = [0.56, 0.58, 0.6, 0.62, 0.64];

impl MinimapTracker {
    pub fn new(arts: Arc<MapArtCache>) -> MinimapTracker {
        MinimapTracker {
            arts, art_floor: HashMap::new(), floor: 0, pending_floor: 0, floor_votes: 0, scale: None, last: None, jump_candidate: None,
            last_ok: 0.0, seeded_at: 0.0, other_floors_at: 0.0, floor_vote_since: 0.0, jump_since: 0.0, last_score: 0.0, last_info: String::new(),
            vel: (0.0, 0.0), filter_time: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.scale = None;
        self.last = None;
        self.last_score = 0.0;
        self.vel = (0.0, 0.0);
        self.floor = 0;
    }

    pub fn release(&mut self, keep: impl Fn(i64) -> bool) {
        self.art_floor.retain(|k, _| keep(k.0));
    }

    pub fn last_position(&self) -> Option<P2> {
        self.last
    }

    pub fn velocity(&self) -> P2 {
        self.vel
    }

    /// Start tracking from a known spot (the arrow on the full map).
    pub fn seed(&mut self, p: P2) {
        self.last = Some(p);
        self.vel = (0.0, 0.0);
        self.seeded_at = now();
        self.filter_time = self.seeded_at;
    }

    /// The map screen is up: the player can't move, so the last fix stays good.
    pub fn hold_still(&mut self) {
        if self.last.is_none() {
            return;
        }
        self.vel = (0.0, 0.0);
        self.last_ok = now();
        self.filter_time = self.last_ok;
    }

    fn art_floor(&mut self, l: &Layout, fl: usize, s: f64, work: f64) -> Option<MatF> {
        let key = (l.number, fl, s.to_bits(), work.to_bits());
        if let Some(m) = self.art_floor.get(&key) {
            return Some(m.clone());
        }
        let a = self.arts.get(l, fl)?;
        let f = a.floor_binary().to_f32(1.0);
        let small = cv2::resize_f(&f, 0, 0, s * work, s * work, false);
        self.art_floor.insert(key, small.clone());
        Some(small)
    }

    /// The player's position in map-art pixels from the square minimap crop (centred on the arrow); ui = client
    /// height / 1080.
    pub fn locate(&mut self, minimap: &Mat, layout: &Layout, ui: f64) -> Option<P2> {
        if minimap.w < 40 {
            return None;
        }
        let tracking = self.last.is_some();
        let work = if tracking && self.scale.is_some() { TRACK_WORK } else { SEARCH_WORK };
        let small = cv::resize(minimap, 0, 0, work / ui, work / ui, Interp::Area);
        let (floor8, mask, floor_px) = minimap_floor(&small, work);
        if floor_px < small.w * small.h / 40 {
            self.last_info = format!("minimap: too little explored floor ({floor_px})");
            return None;
        }
        let tpl = floor8.to_f32(1.0 / 255.0);
        let matcher = Masked::from_f32(&tpl, &mask);
        let scales: Vec<f64> = match (self.scale, tracking) {
            (Some(k), true) => vec![k],
            _ => SEARCH_SCALES.to_vec(),
        };
        let t = now();
        let floors = LayoutDb::floor_count(layout);
        let mut best: Option<(P2, f64, f64, f64, usize)> = None;
        let mut per_floor: HashMap<usize, (P2, f64, f64, f64, usize)> = HashMap::new();
        let mut all_floors = true;
        let others_due = t - self.other_floors_at >= 0.045;
        let mut order: Vec<usize> = (0..floors).collect();
        order.sort_by_key(|&f| if f == self.floor { 0 } else { 1 });
        for &fl in &order {
            for &s in &scales {
                if fl != self.floor && tracking && !others_due && best.is_some_and(|b| b.4 == self.floor && b.1 >= 0.6) {
                    all_floors = false;
                    continue;
                }
                let Some(map) = self.art_floor(layout, fl, s, work) else { continue };
                let kk = s * work;
                let mut roi = (0i64, 0i64, map.w as i64, map.h as i64);
                if tracking {
                    let lost = (t - self.last_ok.max(self.seeded_at)).min(8.0);
                    let half = ((120.0 + 60.0 * lost) * kk) as i64;
                    let l = self.last.unwrap();
                    roi = intersect(((l.0 * kk) as i64 - half - tpl.w as i64 / 2, (l.1 * kk) as i64 - half - tpl.h as i64 / 2, 2 * half + tpl.w as i64, 2 * half + tpl.h as i64), roi);
                }
                if roi.2 <= tpl.w as i64 || roi.3 <= tpl.h as i64 {
                    continue;
                }
                let part = cv2::roi_f(&map, roi.0 as usize, roi.1 as usize, roi.2 as usize, roi.3 as usize);
                let mut res = matcher.run_f32(&part);
                cv2::clean_scores(&mut res);
                let (max, loc, sx, sy) = peak(&res);
                cv2::circle_f(&mut res, loc.0 as i64, loc.1 as i64, (50.0 * kk) as i64, -1.0);
                let max2 = f64::from(res.min_max_loc().1);
                let p = ((roi.0 as f64 + loc.0 as f64 + sx + tpl.w as f64 / 2.0) / kk, (roi.1 as f64 + loc.1 as f64 + sy + tpl.h as f64 / 2.0) / kk);
                let cand = (p, f64::from(max), max2, s, fl);
                if per_floor.get(&fl).is_none_or(|pf| cand.1 > pf.1) {
                    per_floor.insert(fl, cand);
                }
                let bonus = |f: usize| if f == self.floor { 0.03 } else { 0.0 };
                if best.is_none_or(|b| cand.1 + bonus(fl) > b.1 + bonus(b.4)) {
                    best = Some(cand);
                }
            }
        }
        if all_floors {
            self.other_floors_at = t;
        }
        let mut b = best?;
        if b.4 != self.floor && per_floor.contains_key(&self.floor) {
            if self.pending_floor != b.4 || self.floor_votes == 0 {
                self.floor_vote_since = t;
            }
            self.floor_votes = if self.pending_floor == b.4 { self.floor_votes + 1 } else { 1 };
            self.pending_floor = b.4;
            if self.floor_votes < 3 || t - self.floor_vote_since < 0.09 {
                b = per_floor[&self.floor];
            }
        } else if all_floors {
            self.floor_votes = 0;
        }
        self.last_score = b.1;
        self.last_info = format!("minimap {:.2} (2nd {:.2}) at ({:.0},{:.0}) s{:.2} level {} floor {floor_px}", b.1, b.2, b.0.0, b.0.1, b.3, b.4);
        let dt = (t - self.filter_time).clamp(0.005, 0.5);
        let recent = t - self.last_ok < 1.0;
        let pred = self.last.map_or(b.0, |l| (l.0 + self.vel.0 * dt, l.1 + self.vel.1 * dt));
        let near_pred = tracking && recent && (b.0.0 - pred.0).abs() + (b.0.1 - pred.1).abs() < 30.0;
        let lost_for = (t - self.last_ok.max(self.seeded_at)).min(10.0);
        let near_last = tracking && !recent && self.last.is_some_and(|l| (b.0.0 - l.0).abs() + (b.0.1 - l.1).abs() < 120.0 + 40.0 * lost_for) && b.2 < b.1 * 0.75;
        let ok = if tracking { b.1 >= if near_pred { 0.35 } else if recent { 0.45 } else if near_last { 0.38 } else { 0.55 } } else { b.1 >= 0.6 && b.2 < b.1 * 0.85 };
        if !ok {
            self.filter_time = t;
            if tracking && recent && t - self.last_ok < 0.6 {
                self.vel = (self.vel.0 * 0.7, self.vel.1 * 0.7);
                self.last = Some(pred);
                return Some(pred);
            }
            self.vel = (0.0, 0.0);
            if tracking && t - self.seeded_at > 60.0 && t - self.last_ok > 60.0 {
                self.last = None;
            }
            return None;
        }
        let miss = (b.0.0 - pred.0).abs() + (b.0.1 - pred.1).abs();
        if tracking && recent && miss > 60.0 {
            let agrees = self.jump_candidate.is_some_and(|j| (j.0 - b.0.0).abs() + (j.1 - b.0.1).abs() < 40.0);
            if !agrees {
                self.jump_since = t;
            }
            self.jump_candidate = Some(b.0);
            if !agrees || t - self.jump_since < 0.03 {
                self.last_info += " (jump, waiting for confirmation)";
                return Some(pred);
            }
            self.vel = (0.0, 0.0);
            self.last = Some(b.0);
        } else if !tracking || !recent {
            self.vel = (0.0, 0.0);
            self.last = Some(b.0);
        } else {
            let (rx, ry) = (b.0.0 - pred.0, b.0.1 - pred.1);
            self.last = Some((pred.0 + 0.55 * rx, pred.1 + 0.55 * ry));
            self.vel = (self.vel.0 + 0.12 * rx / dt, self.vel.1 + 0.12 * ry / dt);
            let sp = (self.vel.0 * self.vel.0 + self.vel.1 * self.vel.1).sqrt();
            if sp > 150.0 {
                self.vel = (self.vel.0 * 150.0 / sp, self.vel.1 * 150.0 / sp);
            }
        }
        self.jump_candidate = None;
        self.last_ok = t;
        self.filter_time = t;
        self.scale = Some(b.3);
        self.floor = b.4;
        self.last
    }
}

/// The biggest blob of a color (dilated 7 x 7): centre and area, between min and max area.
fn blob(hsv: &Mat, lo: [u8; 3], hi: [u8; 3], min_area: usize, max_area: usize, exclude: Option<(usize, usize)>) -> Option<(P2, usize)> {
    let mut m = cv::in_range(hsv, &lo, &hi);
    if let Some((ew, eh)) = exclude {
        for y in 0..eh.min(m.h) {
            for x in 0..ew.min(m.w) {
                m.data[y * m.w + x] = 0;
            }
        }
    }
    let m = cv2::dilate(&m, &Kernel::rect(7, 7));
    let (_, comps) = cv2::components(&m);
    comps.iter().skip(1).filter(|c| c.area >= min_area && c.area <= max_area).max_by_key(|c| c.area).map(|c| ((c.cx, c.cy), c.area))
}

/// Aligns the full map from its two exit icons (orange and blue doors), which the game draws even under fog.
pub struct DoorLocator {
    arts: Arc<MapArtCache>,
    header: Option<Mat>,
    half_orange: Option<Mat>,
    half_blue: Option<Mat>,
    pub last_info: String,
    pub last_orange: Option<P2>,
    pub last_blue: Option<P2>,
    pub last_map_open: bool,
}

const DEFAULT_SCALE: f64 = 0.632;

impl DoorLocator {
    pub fn new(arts: Arc<MapArtCache>, header: Option<Mat>, orange: Option<Mat>, blue: Option<Mat>) -> DoorLocator {
        let half = |m: Option<Mat>| m.filter(|m| m.w > 1).map(|m| cv::resize(&m, 0, 0, 0.5, 0.5, Interp::Area));
        DoorLocator { arts, header, half_orange: half(orange), half_blue: half(blue), last_info: String::new(), last_orange: None, last_blue: None, last_map_open: false }
    }

    pub fn mark_closed(&mut self) {
        self.last_orange = None;
        self.last_blue = None;
        self.last_map_open = false;
        self.last_info = "map not open".into();
    }

    /// The full map screen's title icon (top left).
    pub fn is_map_open(&self, small1080: &Mat, hx: usize) -> bool {
        let Some(h) = &self.header else { return true };
        let (aw, ah) = ((small1080.w - hx.min(small1080.w)).min(400), small1080.h.min(200));
        if aw < h.w || ah < h.h {
            return false;
        }
        let roi = small1080.roi(hx, 0, aw, ah);
        cv2::match_template(&roi, h).min_max_loc().1 >= 0.8
    }

    fn find_icon(half: &Mat, tpl: Option<&Mat>, hx: usize) -> (Option<P2>, f64) {
        const H: f64 = 0.5;
        let Some(t) = tpl else { return (None, 0.0) };
        if half.w < t.w || half.h < t.h {
            return (None, 0.0);
        }
        let mut res = cv2::match_template(half, t);
        let (ew, eh) = ((((hx + 400) as f64) * H) as usize, (120.0 * H) as usize);
        let lw = (((hx + 150) as f64) * H) as usize;
        for y in 0..res.h {
            for x in 0..res.w {
                if (x < ew && y < eh) || x < lw {
                    res.data[y * res.w + x] = -1.0;
                }
            }
        }
        let (_, max, _, loc) = res.min_max_loc();
        let p = if max >= 0.75 { Some(((loc.0 as f64 + t.w as f64 / 2.0) / H, (loc.1 as f64 + t.h as f64 / 2.0) / H)) } else { None };
        (p, f64::from(max))
    }

    /// Which layout the open map shows and where: the door icons' direction and distance pick it, revealed floor
    /// separates look-alikes. `arrow` gets the player arrow's spot (client px).
    pub fn locate(&mut self, frame: &Mat, candidates: &[&Layout], arrow: &mut Option<P2>, preferred: Option<i64>, favoured: Option<&[i64]>) -> Option<MapPose> {
        *arrow = None;
        let (ux, _, _, uh) = hud(frame.w, frame.h);
        let f = 1080.0 / uh as f64;
        let hx = (ux as f64 * f).round() as usize;
        let small = cv::resize(frame, 0, 0, f, f, if f < 1.0 { Interp::Area } else { Interp::Linear });
        self.last_orange = None;
        self.last_blue = None;
        self.last_map_open = self.is_map_open(&small, hx);
        if !self.last_map_open {
            self.last_info = "map not open".into();
            return None;
        }
        let hsv = cv::bgr2hsv(&small);
        let hud_ex = ((hx + 320).min(small.w), 300.min(small.h));
        let (mut o, mut b);
        let scores;
        if self.half_orange.is_some() && self.half_blue.is_some() {
            let half = cv::resize(&small, 0, 0, 0.5, 0.5, Interp::Area);
            let (oc, os) = Self::find_icon(&half, self.half_orange.as_ref(), hx);
            let (bc, bs) = Self::find_icon(&half, self.half_blue.as_ref(), hx);
            o = oc.map(|c| (c, 1));
            b = bc.map(|c| (c, 1));
            scores = format!("icons o{os:.2} b{bs:.2}");
        } else {
            o = blob(&hsv, [5, 75, 90], [19, 255, 255], 30, 2500, Some(hud_ex));
            b = blob(&hsv, [100, 100, 100], [125, 255, 255], 30, 2500, Some(hud_ex));
            scores = String::new();
        }
        const ANCHOR_Y: f64 = 11.0;
        o = o.map(|(c, a)| ((c.0, c.1 + ANCHOR_Y), a));
        b = b.map(|(c, a)| ((c.0, c.1 + ANCHOR_Y), a));
        self.last_orange = o.map(|(c, _)| (c.0 / f, c.1 / f));
        self.last_blue = b.map(|(c, _)| (c.0 / f, c.1 / f));
        if let Some((c, _)) = blob(&hsv, [20, 70, 100], [35, 255, 255], 150, 4000, Some(hud_ex)) {
            *arrow = Some((c.0 / f, c.1 / f));
        }
        let (Some((oo, _)), Some((bb, _))) = (o, b) else {
            self.last_info = format!("doors: orange {}, blue {} ({scores})", if o.is_none() { "missing" } else { "ok" }, if b.is_none() { "missing" } else { "ok" });
            return None;
        };
        let (wx, wy) = (bb.0 - oo.0, bb.1 - oo.1);
        let dist = (wx * wx + wy * wy).sqrt();
        if dist < 25.0 {
            self.last_info = "doors overlap".into();
            return None;
        }
        let ang = wx.atan2(-wy).to_degrees();
        let mut lit = cv::in_range(&hsv, &[0, 0, 141], &[180, 40, 255]);
        for y in 0..hud_ex.1 {
            for x in 0..hud_ex.0 {
                lit.data[y * lit.w + x] = 0;
            }
        }
        let lit_count = lit.count_nonzero();
        let mut scored: Vec<(&Layout, f64, f64, f64)> = vec![];
        for &l in candidates {
            let (Some(or), Some(bl)) = (l.orange(), l.blue()) else { continue };
            let (oi, bi) = (&or.img, &bl.img);
            let (vx, vy) = (bi[0] - oi[0], bi[1] - oi[1]);
            let vlen = (vx * vx + vy * vy).sqrt();
            let a = vx.atan2(-vy).to_degrees();
            let err = (((a - ang) % 360.0 + 540.0) % 360.0 - 180.0).abs();
            let s = dist / vlen;
            if err > 8.0 || !(0.1..=6.0).contains(&s) {
                continue;
            }
            let mut cover = -1.0;
            if lit_count > 5000
                && let Some(drawn) = self.arts.get(l, 0)
            {
                let m = [s, 0.0, oo.0 - s * oi[0], 0.0, s, oo.1 - s * oi[1]];
                let warped = cv2::warp_affine(&drawn.alpha, m, small.w, small.h, false);
                let both = warped.data.iter().zip(&lit.data).filter(|(w, l)| **w > 128 && **l != 0).count();
                cover = both as f64 / lit_count as f64;
            }
            scored.push((l, err, s, cover));
        }
        self.last_info = format!("doors {ang:.1}° {:.0}px lit {lit_count}: {}", dist / f,
            scored.iter().map(|x| format!("{}:{:.1}° c{:.2} s{:.3}", x.0.number, x.1, x.3, x.2)).collect::<Vec<_>>().join("  "));
        if scored.is_empty() {
            return None;
        }
        let key = |x: &(&Layout, f64, f64, f64)| if x.3 >= 0.0 { x.3 - x.1 * 0.01 } else { -x.1 };
        let mut ranked = scored.clone();
        ranked.sort_by(|a, b| key(b).total_cmp(&key(a)));
        let mut best = ranked[0];
        if best.3 >= 0.0 && best.3 < 0.6 {
            return None;
        }
        if ranked.len() > 1 {
            let next = ranked[1];
            let clear = if best.3 >= 0.0 { best.3 - next.3 > 0.15 } else { next.1 - best.1 > 2.5 };
            if !clear {
                let off = |sc: f64| (sc / DEFAULT_SCALE).ln().abs();
                let mut close: Vec<(&Layout, f64, f64, f64)> = ranked.iter().copied().filter(|x| x.3 < 0.0 || x.3 >= best.3 - 0.15).collect();
                close.sort_by(|a, b| off(a.2).total_cmp(&off(b.2)));
                let pi = preferred.and_then(|p| close.iter().position(|x| x.0.number == p));
                let fav: Vec<_> = favoured.map_or(vec![], |fv| close.iter().copied().filter(|x| fv.contains(&x.0.number)).collect());
                if let Some(i) = pi {
                    best = close[i];
                    self.last_info += &format!("  (tie -> minimap's layout {})", best.0.number);
                } else if fav.len() == 1 {
                    best = fav[0];
                    self.last_info += &format!("  (tie -> difficulty's layout {})", best.0.number);
                } else if off(close[0].2) < 1.05f64.ln() && (close.len() == 1 || off(close[1].2) > 1.12f64.ln()) {
                    best = close[0];
                    self.last_info += &format!("  (tie -> default zoom picks {})", best.0.number);
                } else {
                    self.last_info += "  (ambiguous)";
                    return None;
                }
            }
        }
        let sc = best.2 / f;
        let bo = &best.0.orange().unwrap().img;
        Some(MapPose { layout: best.0.number, scale: sc, tx: oo.0 / f - sc * bo[0], ty: oo.1 / f - sc * bo[1], inliers: 100, rotation: 0.0 })
    }
}

/// Explored floor on the full map (light grey or tan), without the title, zoom bar and icons.
fn screen_floor(frame1080: &Mat, hx: usize) -> Mat {
    let hsv = cv::bgr2hsv(frame1080);
    let mut floor = cv::bitwise_or(&cv::in_range(&hsv, &[0, 0, 141], &[180, 40, 255]), &cv::in_range(&hsv, &[5, 0, 141], &[30, 95, 255]));
    let (w, h) = (floor.w, floor.h);
    for y in 0..h {
        for x in 0..w {
            if (x < (hx + 320).min(w) && y < 300.min(h)) || x < (hx + 140).min(w) {
                floor.data[y * w + x] = 0;
            }
        }
    }
    let icons = cv2::dilate(&cv::in_range(&hsv, &[0, 101, 91], &[180, 255, 255]), &Kernel::rect(25, 25));
    let icons2 = icon_mask(&hsv, 25);
    for i in 0..floor.data.len() {
        if icons.data[i] != 0 || icons2.data[i] != 0 {
            floor.data[i] = 0;
        }
    }
    floor
}

/// Exact placement of the full map: the explored floor on screen against the layout's floor shape, searching zoom
/// and position around a first guess.
pub struct FloorRefiner {
    arts: Arc<MapArtCache>,
    art_floor: HashMap<(i64, usize), MatF>,
    pub last_info: String,
    pub last_score: f64,
    pub floor: usize,
}

const K: f64 = 0.5;

struct Prepared {
    f: f64,
    lit: usize,
    bx: (usize, usize, usize, usize),
    matcher: Option<Masked>,
}

impl FloorRefiner {
    pub fn new(arts: Arc<MapArtCache>) -> FloorRefiner {
        FloorRefiner { arts, art_floor: HashMap::new(), last_info: String::new(), last_score: 0.0, floor: 0 }
    }

    pub fn trim(&mut self, keep: impl Fn(i64) -> bool) {
        self.art_floor.retain(|k, _| keep(k.0));
    }

    fn art(&mut self, l: &Layout, level: usize) -> Option<MatF> {
        if let Some(a) = self.art_floor.get(&(l.number, level)) {
            return Some(a.clone());
        }
        let a = self.arts.get(l, level)?.floor_binary().to_f32(1.0);
        self.art_floor.insert((l.number, level), a.clone());
        Some(a)
    }

    fn prepare(frame: &Mat) -> Prepared {
        let (ux, _, _, uh) = hud(frame.w, frame.h);
        let f = 1080.0 / uh as f64;
        let f1080 = cv::resize(frame, 0, 0, f, f, Interp::Area);
        let fl = screen_floor(&f1080, (ux as f64 * f).round() as usize);
        let lit = fl.count_nonzero();
        let mut p = Prepared { f, lit, bx: (0, 0, 0, 0), matcher: None };
        if lit < 2000 {
            return p;
        }
        let small = cv2::resize_f(&fl.to_f32(1.0 / 255.0), 0, 0, K, K, false);
        let dil = cv2::dilate(&fl, &Kernel::rect(15, 15));
        let mask = cv::resize(&dil, small.w, small.h, 0.0, 0.0, Interp::Nearest);
        p.bx = cv2::bounding_rect(&mask);
        if p.bx.2 >= 20 && p.bx.3 >= 20 {
            let t = cv2::roi_f(&small, p.bx.0, p.bx.1, p.bx.2, p.bx.3);
            let m = mask.roi(p.bx.0, p.bx.1, p.bx.2, p.bx.3);
            p.matcher = Some(Masked::from_f32(&t, &m));
        }
        p
    }

    /// Best pose near the guess, or None: spread = relative zoom range, window = position range (1080p px).
    pub fn refine(&mut self, frame: &Mat, guess: &MapPose, layout: &Layout, spread: f64, window: i64) -> Option<MapPose> {
        let prep = Self::prepare(frame);
        let floors = LayoutDb::floor_count(layout);
        let (mut best_pose, mut best_score, mut best_info, mut best_floor) = (None, -1.0, String::new(), 0);
        let mut order: Vec<usize> = (0..floors).collect();
        order.sort_by_key(|&f| if f == self.floor { 0 } else { 1 });
        for fl in order {
            let p = self.refine_on(&prep, guess, layout, spread, window, fl);
            if self.last_score > best_score {
                best_score = self.last_score;
                best_pose = p;
                best_info = self.last_info.clone();
                best_floor = fl;
            }
            if best_score >= 0.6 {
                break;
            }
        }
        self.last_score = best_score.max(0.0);
        self.last_info = best_info + &if floors > 1 { format!(" level {best_floor}") } else { String::new() };
        if best_pose.is_some() {
            self.floor = best_floor;
        }
        best_pose
    }

    fn refine_on(&mut self, p: &Prepared, guess: &MapPose, layout: &Layout, spread: f64, window: i64, fl: usize) -> Option<MapPose> {
        self.last_score = 0.0;
        if p.lit < 2000 {
            self.last_info = format!("refine: little floor ({})", p.lit);
            return None;
        }
        let tpl = p.matcher.as_ref()?;
        let f = p.f;
        let (s, tx, ty) = (guess.scale * f, guess.tx * f, guess.ty * f);
        let (x0, y0) = (p.bx.0 as f64 / K, p.bx.1 as f64 / K);
        let art = self.art(layout, fl)?;
        let mut best: Option<(f64, f64, f64, f64)> = None;
        let search = |base: f64, bx: f64, by: f64, spr: f64, steps: usize, win: i64, best: &mut Option<(f64, f64, f64, f64)>| {
            for i in 0..steps {
                let sc = base * if steps == 1 { 1.0 } else { ((1.0 - spr).ln() + ((1.0 + spr).ln() - (1.0 - spr).ln()) * i as f64 / (steps - 1) as f64).exp() };
                let a = cv2::resize_f(&art, 0, 0, sc * K, sc * K, false);
                let ex = (x0 - bx) / base * sc * K;
                let ey = (y0 - by) / base * sc * K;
                let w = ((win as f64 * K) as i64).max(2);
                let pp = tpl.w.max(tpl.h) as i64 + w;
                let roi = intersect((ex as i64 + pp - w, ey as i64 + pp - w, tpl.w as i64 + 2 * w, tpl.h as i64 + 2 * w), (0, 0, a.w as i64 + 2 * pp, a.h as i64 + 2 * pp));
                if roi.2 < tpl.w as i64 || roi.3 < tpl.h as i64 {
                    continue;
                }
                let mut part = MatF::new(roi.2 as usize, roi.3 as usize, 1);
                let from = intersect((roi.0 - pp, roi.1 - pp, roi.2, roi.3), (0, 0, a.w as i64, a.h as i64));
                for yy in 0..from.3 {
                    for xx in 0..from.2 {
                        let (sx, sy) = (from.0 + xx, from.1 + yy);
                        let (dx, dy) = (sx - (roi.0 - pp), sy - (roi.1 - pp));
                        part.data[dy as usize * part.w + dx as usize] = a.data[sy as usize * a.w + sx as usize];
                    }
                }
                let mut r = tpl.run_f32(&part);
                cv2::clean_scores(&mut r);
                let (_, mx, _, loc) = r.min_max_loc();
                let mx = f64::from(mx);
                if best.is_none_or(|b| mx > b.0) {
                    let ax = (roi.0 as f64 + loc.0 as f64 - pp as f64) / (sc * K);
                    let ay = (roi.1 as f64 + loc.1 as f64 - pp as f64) / (sc * K);
                    *best = Some((mx, sc, x0 - sc * ax, y0 - sc * ay));
                }
            }
        };
        search(s, tx, ty, spread, if spread > 0.1 { 15 } else { 7 }, window, &mut best);
        let c = best?;
        search(c.1, c.2, c.3, 0.02, 5, 12, &mut best);
        let b = best?;
        self.last_score = b.0;
        self.last_info = format!("refine {:.2} s{:.3} (guess s{:.3}) lit {}", b.0, b.1 / f, guess.scale, p.lit);
        if b.0 < 0.35 {
            return None;
        }
        Some(MapPose { layout: layout.number, scale: b.1 / f, tx: b.2 / f, ty: b.3 / f, inliers: guess.inliers.max(60), rotation: 0.0 })
    }
}

/// Finds the player on the island from the minimap: the island art drawn north-up at 1.2x, matched in color.
pub struct IslandTracker {
    arts: Arc<MapArtCache>,
    art: HashMap<(u64, u64), Arc<Mat>>,
    scale: Option<f64>,
    last_full_best: Option<P2>,
    last: Option<P2>,
    jump_candidate: Option<P2>,
    vel: P2,
    last_ok: f64,
    filter_time: f64,
    jump_since: f64,
    pub last_score: f64,
    pub last_info: String,
}

const I_SEARCH_WORK: f64 = 0.35;
const I_TRACK_WORK: f64 = 0.7;

impl IslandTracker {
    pub fn new(arts: Arc<MapArtCache>) -> IslandTracker {
        IslandTracker { arts, art: HashMap::new(), scale: None, last_full_best: None, last: None, jump_candidate: None, vel: (0.0, 0.0), last_ok: 0.0,
            filter_time: 0.0, jump_since: 0.0, last_score: 0.0, last_info: String::new() }
    }

    pub fn tracking(&self) -> bool {
        self.last.is_some() && now() - self.last_ok < 20.0
    }

    pub fn velocity(&self) -> P2 {
        self.vel
    }

    pub fn reset(&mut self) {
        self.scale = None;
        self.last = None;
        self.vel = (0.0, 0.0);
    }

    pub fn release(&mut self) {
        self.art.clear();
    }

    pub fn seed(&mut self, p: P2) {
        self.last = Some(p);
        self.vel = (0.0, 0.0);
        self.last_ok = now();
        self.filter_time = self.last_ok;
    }

    fn art(&mut self, island: &Layout, s: f64, work: f64) -> Option<Arc<Mat>> {
        let key = (s.to_bits(), work.to_bits());
        if let Some(a) = self.art.get(&key) {
            return Some(a.clone());
        }
        let m = Arc::new(cv::resize(&self.arts.get(island, 0)?.bgr(), 0, 0, s * work, s * work, Interp::Area));
        self.art.insert(key, m.clone());
        Some(m)
    }

    /// The player's position in island-art pixels, or None. A whole-island search is slow; the caller says when.
    pub fn locate(&mut self, minimap: &Mat, island: &Layout, ui: f64, allow_full: bool) -> Option<P2> {
        let tracking = self.tracking();
        if !tracking && !allow_full {
            return None;
        }
        let work = if tracking && self.scale.is_some() { I_TRACK_WORK } else { I_SEARCH_WORK };
        let small = cv::resize(minimap, 0, 0, work / ui, work / ui, Interp::Area);
        let hsv = cv::bgr2hsv(&small);
        let mut mask = Mat::new(small.w, small.h, 1);
        let (cx, cy) = ((small.w / 2) as i64, (small.h / 2) as i64);
        let r = (small.w.min(small.h) as f64 * 0.42) as i64;
        cv2::circle(&mut mask, cx, cy, r, 255);
        cv2::circle(&mut mask, cx, cy, (r / 4).max(5), 0);
        {
            let k5 = ((9.0 * work) as usize).max(3);
            let icons = cv2::dilate(&cv::in_range(&hsv, &[0, 121, 91], &[180, 255, 255]), &Kernel::rect(k5, k5));
            let k7 = ((7.0 * work) as usize).max(3);
            let white = cv2::dilate(&cv::in_range(&hsv, &[0, 0, 201], &[180, 44, 255]), &Kernel::rect(k7, k7));
            for i in 0..mask.data.len() {
                if icons.data[i] != 0 || white.data[i] != 0 {
                    mask.data[i] = 0;
                }
            }
        }
        let matcher = Masked::from_u8(&small, &mask);
        let scales: Vec<f64> = match (tracking, self.scale) {
            (true, Some(k)) => vec![k],
            _ => vec![1.2],
        };
        let t = now();
        let mut shortlist: Vec<(P2, f64, f64)> = vec![];
        let mut best: Option<(P2, f64, f64, f64)> = None;
        for &s in &scales {
            let kk = s * work;
            let (mut res, roi_x, roi_y);
            if tracking {
                let full = self.arts.get(island, 0)?.bgr();
                let lost = (t - self.last_ok).min(8.0);
                let reach = (100.0 + 60.0 * lost) as i64;
                let half = (reach as f64 + small.w as f64 / 2.0 / kk) as i64;
                let here = self.last.unwrap();
                let from = intersect((here.0 as i64 - half, here.1 as i64 - half, 2 * half, 2 * half), (0, 0, full.w as i64, full.h as i64));
                if from.2 <= 0 || from.3 <= 0 {
                    continue;
                }
                let part = full.roi(from.0 as usize, from.1 as usize, from.2 as usize, from.3 as usize);
                let map = cv::resize(&part, 0, 0, kk, kk, Interp::Area);
                if map.w <= small.w || map.h <= small.h {
                    continue;
                }
                res = matcher.run(&planes_u8(&map), map.w, map.h);
                roi_x = from.0 as f64 * kk;
                roi_y = from.1 as f64 * kk;
            } else {
                let Some(map) = self.art(island, s, work) else { continue };
                if map.w <= small.w || map.h <= small.h {
                    continue;
                }
                res = matcher.run(&planes_u8(&map), map.w, map.h);
                roi_x = 0.0;
                roi_y = 0.0;
            }
            cv2::clean_scores(&mut res);
            let (_, max, _, loc) = res.min_max_loc();
            if !tracking {
                let mut rc = res.clone();
                for _ in 0..5 {
                    let (_, pv, _, pl) = rc.min_max_loc();
                    if pv < 0.35 {
                        break;
                    }
                    shortlist.push((((roi_x + pl.0 as f64 + small.w as f64 / 2.0) / kk, (roi_y + pl.1 as f64 + small.h as f64 / 2.0) / kk), s, f64::from(pv)));
                    cv2::circle_f(&mut rc, pl.0 as i64, pl.1 as i64, (60.0 * kk) as i64, -1.0);
                }
            }
            cv2::circle_f(&mut res, loc.0 as i64, loc.1 as i64, (60.0 * kk) as i64, -1.0);
            let max2 = f64::from(res.min_max_loc().1);
            if best.is_none_or(|b| f64::from(max) > b.1) {
                best = Some((((roi_x + loc.0 as f64 + small.w as f64 / 2.0) / kk, (roi_y + loc.1 as f64 + small.h as f64 / 2.0) / kk), f64::from(max), max2, s));
            }
        }
        if !tracking && !shortlist.is_empty() {
            let fine_small = cv::resize(minimap, 0, 0, I_TRACK_WORK / ui, I_TRACK_WORK / ui, Interp::Area);
            let fine_mask = cv::resize(&mask, fine_small.w, fine_small.h, 0.0, 0.0, Interp::Nearest);
            let fm = Masked::from_u8(&fine_small, &fine_mask);
            let full = self.arts.get(island, 0)?.bgr();
            shortlist.sort_by(|a, b| b.2.total_cmp(&a.2));
            let mut fine: Vec<(P2, f64, f64)> = vec![];
            for &(p, s0, _) in shortlist.iter().take(6) {
                let kk = s0 * I_TRACK_WORK;
                let half = (40.0 + fine_small.w as f64 / 2.0 / kk) as i64;
                let src = intersect((p.0 as i64 - half, p.1 as i64 - half, 2 * half, 2 * half), (0, 0, full.w as i64, full.h as i64));
                if src.2 <= 0 || src.3 <= 0 {
                    continue;
                }
                let crop = cv::resize(&full.roi(src.0 as usize, src.1 as usize, src.2 as usize, src.3 as usize), 0, 0, kk, kk, Interp::Area);
                if crop.w <= fine_small.w || crop.h <= fine_small.h {
                    continue;
                }
                let mut r2 = fm.run(&planes_u8(&crop), crop.w, crop.h);
                cv2::clean_scores(&mut r2);
                let (_, fv, _, fl) = r2.min_max_loc();
                fine.push(((src.0 as f64 + (fl.0 as f64 + fine_small.w as f64 / 2.0) / kk, src.1 as f64 + (fl.1 as f64 + fine_small.h as f64 / 2.0) / kk), f64::from(fv), s0));
            }
            fine.sort_by(|a, b| b.1.total_cmp(&a.1));
            if let Some(top) = fine.first() {
                let second = fine.iter().skip(1).filter(|x| (x.0.0 - top.0.0).abs() + (x.0.1 - top.0.1).abs() > 60.0).map(|x| x.1).fold(0.0, f64::max);
                best = Some((top.0, top.1, second, top.2));
            }
        }
        let b = best?;
        self.last_score = b.1;
        self.last_info = format!("island {:.2} (2nd {:.2}) at ({:.0},{:.0}) s{:.2}", b.1, b.2, b.0.0, b.0.1, b.3);
        let dt = (t - self.filter_time).clamp(0.005, 0.5);
        let near_expected = tracking && t - self.last_ok < 1.5 && self.last.is_some_and(|l| (b.0.0 - (l.0 + self.vel.0 * dt)).abs() + (b.0.1 - (l.1 + self.vel.1 * dt)).abs() < 40.0);
        let mut ok = if tracking { b.1 >= if near_expected { 0.30 } else { 0.42 } } else { b.1 >= 0.6 && b.2 < b.1 * 0.97 };
        if !tracking {
            let repeat = self.last_full_best.is_some_and(|l| (l.0 - b.0.0).abs() + (l.1 - b.0.1).abs() < 80.0 && b.1 >= 0.5);
            self.last_full_best = Some(b.0);
            ok |= repeat;
        }
        if !ok {
            self.filter_time = t;
            return None;
        }
        let pred = match (self.last, tracking) {
            (Some(l), true) => (l.0 + self.vel.0 * dt, l.1 + self.vel.1 * dt),
            _ => b.0,
        };
        if tracking && (b.0.0 - pred.0).abs() + (b.0.1 - pred.1).abs() > 80.0 {
            let agrees = self.jump_candidate.is_some_and(|j| (j.0 - b.0.0).abs() + (j.1 - b.0.1).abs() < 40.0);
            if !agrees {
                self.jump_since = t;
            }
            self.jump_candidate = Some(b.0);
            if !agrees || t - self.jump_since < 0.03 {
                return Some(pred);
            }
            self.last = Some(b.0);
            self.vel = (0.0, 0.0);
        } else if !tracking {
            self.last = Some(b.0);
            self.vel = (0.0, 0.0);
        } else {
            let (rx, ry) = (b.0.0 - pred.0, b.0.1 - pred.1);
            self.last = Some((pred.0 + 0.55 * rx, pred.1 + 0.55 * ry));
            self.vel = (self.vel.0 + 0.12 * rx / dt, self.vel.1 + 0.12 * ry / dt);
            let sp = (self.vel.0 * self.vel.0 + self.vel.1 * self.vel.1).sqrt();
            if sp > 120.0 {
                self.vel = (self.vel.0 * 120.0 / sp, self.vel.1 * 120.0 / sp);
            }
        }
        self.jump_candidate = None;
        self.last_ok = t;
        self.filter_time = t;
        self.scale = Some(b.3);
        self.last
    }
}

/// The game's own map icons on a picture (sprites with their transparency as the mask).
pub struct MapIcons {
    cache: Mutex<HashMap<(PathBuf, usize), Arc<(Mat, Mat, Masked)>>>,
}

impl Default for MapIcons {
    fn default() -> Self {
        MapIcons { cache: Mutex::new(HashMap::new()) }
    }
}

impl MapIcons {
    fn sprite(&self, path: &Path, size: usize) -> Option<Arc<(Mat, Mat, Masked)>> {
        if let Some(s) = self.cache.lock().unwrap().get(&(path.to_path_buf(), size)) {
            return Some(s.clone());
        }
        let rgba = imread(path)?;
        if rgba.c != 4 {
            return None;
        }
        let small = cv::resize(&rgba, size, size, 0.0, 0.0, Interp::Area);
        let bgr = cv::bgra2bgr(&small);
        let mask = cv::threshold(&small.channel(3), 128.0, 255.0, cv::Thresh::Binary);
        let m = Masked::from_u8(&bgr, &mask);
        let s = Arc::new((bgr, mask, m));
        self.cache.lock().unwrap().insert((path.to_path_buf(), size), s.clone());
        Some(s)
    }

    /// Matches of an icon in a 1080p-scaled picture, strongest first, at least `apart` px from each other.
    pub fn find(&self, bgr: &Mat, icon: &Path, sizes: &[usize], min_score: f64, max: usize, apart: i64) -> Vec<(P2, f64, usize)> {
        let mut all = vec![];
        let planes = planes_u8(bgr);
        for &size in sizes {
            let Some(s) = self.sprite(icon, size) else { continue };
            if bgr.w <= size || bgr.h <= size {
                continue;
            }
            let mut res = s.2.run(&planes, bgr.w, bgr.h);
            for v in res.data.iter_mut() {
                if v.is_nan() {
                    *v = -1.0;
                }
                if *v > 1.0 {
                    *v = 0.0;
                }
            }
            for _ in 0..max {
                let (_, v, _, loc) = res.min_max_loc();
                if f64::from(v) < min_score {
                    break;
                }
                all.push(((loc.0 as f64 + size as f64 / 2.0, loc.1 as f64 + size as f64 / 2.0), f64::from(v), size));
                cv2::circle_f(&mut res, loc.0 as i64, loc.1 as i64, apart, -1.0);
            }
        }
        all.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut kept: Vec<(P2, f64, usize)> = vec![];
        for c in all {
            if kept.len() < max && kept.iter().all(|k| (k.0.0 - c.0.0).abs() + (k.0.1 - c.0.1).abs() > apart as f64) {
                kept.push(c);
            }
        }
        kept
    }
}

/// Which way the player faces: the arrow sprite rotated in 2.5 degree steps and matched at the minimap centre; or
/// from the white view cone.
pub struct HeadingDetector {
    bank: Vec<(f64, usize, usize, Masked)>,
    pub last_score: f64,
    eggs: Vec<(usize, usize, Masked)>,
}

impl HeadingDetector {
    pub fn new(sprite: &Path, icons_dir: &Path) -> HeadingDetector {
        let mut bank = vec![];
        if let Some(rgba) = imread(sprite).filter(|m| m.c == 4) {
            let small = cv::resize(&rgba, 0, 0, 0.40, 0.40, Interp::Area);
            let mut a = 0.0;
            while a < 360.0 {
                let m = cv2::rotation_matrix(small.w as f64 / 2.0, small.h as f64 / 2.0, -a, 1.0);
                let r = cv2::warp_affine(&small, m, small.w, small.h, true);
                let bgr = cv::bgra2bgr(&r);
                let mask = cv::threshold(&r.channel(3), 150.0, 255.0, cv::Thresh::Binary);
                bank.push((a, bgr.w, bgr.h, Masked::from_u8(&bgr, &mask)));
                a += 2.5;
            }
        }
        let eggs = ["hud_egg_marker.png", "hud_egg_marker2.png"].iter().filter_map(|n| {
            let t = imread(&icons_dir.join(n))?;
            let t = if t.c == 4 { cv::bgra2bgr(&t) } else { t };
            let mut m = Mat::new(t.w, t.h, 1);
            cv2::circle(&mut m, (t.w / 2) as i64, (t.h / 2) as i64, (t.w / 2) as i64 - 2, 255);
            cv2::circle(&mut m, t.w as i64 - 3, t.h as i64 - 3, (t.w / 3) as i64, 0);
            Some((t.w, t.h, Masked::from_u8(&t, &m)))
        }).collect();
        HeadingDetector { bank, last_score: 0.0, eggs }
    }

    /// How much the minimap's centre looks like the egg marker (0-1).
    pub fn egg_score(&self, minimap: &Mat, ui: f64) -> f64 {
        if self.eggs.is_empty() {
            return 0.0;
        }
        let half = ((15.0 + 12.0) * ui) as i64;
        let (cx, cy) = ((minimap.w / 2) as i64, (minimap.h / 2) as i64);
        let roi = intersect((cx - half, cy - half, 2 * half, 2 * half), (0, 0, minimap.w as i64, minimap.h as i64));
        let crop = cv::resize(&minimap.roi(roi.0 as usize, roi.1 as usize, roi.2 as usize, roi.3 as usize), 0, 0, 1.0 / ui, 1.0 / ui, Interp::Area);
        let planes = planes_u8(&crop);
        let mut best = 0f64;
        for (w, h, m) in &self.eggs {
            if crop.w <= *w || crop.h <= *h {
                continue;
            }
            let mut res = m.run(&planes, crop.w, crop.h);
            for v in res.data.iter_mut() {
                if v.is_nan() {
                    *v = -1.0;
                }
                if *v > 1.0 {
                    *v = 0.0;
                }
            }
            best = best.max(f64::from(res.min_max_loc().1));
        }
        best
    }

    /// Degrees clockwise from north, or None; `hint` = the heading a moment ago (searched around first).
    pub fn detect(&mut self, minimap: &Mat, ui: f64, hint: Option<f64>) -> Option<f64> {
        if self.bank.is_empty() {
            return None;
        }
        let half = (24.0 * ui) as i64;
        let (cx, cy) = ((minimap.w / 2) as i64, (minimap.h / 2) as i64);
        let roi = intersect((cx - half, cy - half, half * 2, half * 2), (0, 0, minimap.w as i64, minimap.h as i64));
        let crop = cv::resize(&minimap.roi(roi.0 as usize, roi.1 as usize, roi.2 as usize, roi.3 as usize), 48, 48, 0.0, 0.0, Interp::Area);
        let planes = planes_u8(&crop);
        let score = |i: usize| -> f64 {
            let (_, w, h, m) = &self.bank[i];
            if crop.w < *w || crop.h < *h {
                return -1.0;
            }
            let mut res = m.run(&planes, crop.w, crop.h);
            for v in res.data.iter_mut() {
                if v.is_nan() {
                    *v = -1.0;
                }
                if *v > 1.0 {
                    *v = 0.0;
                }
            }
            f64::from(res.min_max_loc().1)
        };
        let n = self.bank.len() as i64;
        let (mut best, mut best_score) = (0i64, -1.0);
        if let Some(h0) = hint {
            let c0 = ((h0 / 2.5).round() as i64).rem_euclid(n);
            for d in (-9..=9).step_by(3) {
                let i = (c0 + d).rem_euclid(n);
                let sc = score(i as usize);
                if sc > best_score {
                    best_score = sc;
                    best = i;
                }
            }
            let mid = best;
            for d in [-2, -1, 1, 2] {
                let i = (mid + d).rem_euclid(n);
                let sc = score(i as usize);
                if sc > best_score {
                    best_score = sc;
                    best = i;
                }
            }
            if best_score >= 0.65 {
                self.last_score = best_score;
                return Some(self.bank[best as usize].0);
            }
            best_score = -1.0;
            best = 0;
        }
        let step = 6;
        for i in (0..n).step_by(step as usize) {
            let sc = score(i as usize);
            if sc > best_score {
                best_score = sc;
                best = i;
            }
        }
        let coarse = best;
        for d in -step + 1..step {
            if d == 0 {
                continue;
            }
            let i = (coarse + d).rem_euclid(n);
            let sc = score(i as usize);
            if sc > best_score {
                best_score = sc;
                best = i;
            }
        }
        self.last_score = best_score;
        (best_score >= 0.65).then(|| self.bank[best as usize].0)
    }
}

/// Per-slice mean near-white level of the ring around the player.
fn cone_profile(small: &Mat, bins: usize) -> Vec<f64> {
    let (w, h) = (small.w, small.h);
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let r_big = cx.min(cy) * 0.84;
    let (r0, r1) = (r_big * 0.14, r_big * 0.55);
    let bx0 = ((cx - r1 - 1.0).floor().max(0.0)) as usize;
    let by0 = ((cy - r1 - 1.0).floor().max(0.0)) as usize;
    let bx1 = ((cx + r1 + 1.0).ceil() as usize).min(w);
    let by1 = ((cy + r1 + 1.0).ceil() as usize).min(h);
    let part = small.roi(bx0, by0, bx1 - bx0, by1 - by0);
    let hsv = cv::bgr2hsv(&part);
    let mut sum = vec![0f64; bins];
    let mut n = vec![0i64; bins];
    for y in by0..by1 {
        for x in bx0..bx1 {
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            let r = (dx * dx + dy * dy).sqrt();
            if r < r0 || r > r1 {
                continue;
            }
            let ang = (dx.atan2(-dy).to_degrees() + 360.0) % 360.0;
            let b = ((ang / (360.0 / bins as f64)) as usize) % bins;
            let i = ((y - by0) * part.w + (x - bx0)) * 3;
            let (sat, val) = (hsv.data[i + 1], hsv.data[i + 2]);
            if sat < 50 && val > 205 {
                sum[b] += (f64::from(val - 205) / 35.0).min(1.0);
            }
            n[b] += 1;
        }
    }
    (0..bins).map(|i| if n[i] > 0 { sum[i] / n[i] as f64 } else { 0.0 }).collect()
}

/// The best arc of `win` slices (first slice, sum); ties keep the first.
fn best_arc(prof: &[f64], win: usize) -> (usize, f64) {
    let bins = prof.len();
    if prof.iter().all(|&v| v == 0.0) {
        return (0, 0.0);
    }
    let (mut best, mut best_sum) = (0usize, -1.0);
    for i in 0..bins {
        let t: f64 = (0..win).map(|j| prof[(i + j) % bins]).sum();
        if t > best_sum {
            best_sum = t;
            best = i;
        }
    }
    (best, best_sum)
}

fn outside(prof: &[f64], best: usize, win: usize) -> f64 {
    let bins = prof.len();
    let mut rest: Vec<f64> = (0..bins).filter(|&i| (i as i64 - best as i64).rem_euclid(bins as i64) as usize >= win).map(|i| prof[i]).collect();
    rest.sort_by(f64::total_cmp);
    rest[rest.len() / 2]
}

/// Which way the camera looks, from the white view cone the minimap draws: degrees and confidence.
pub fn from_view_cone(minimap: &Mat, ui: f64) -> (Option<f64>, f64) {
    let k = 210.0 / minimap.w.min(minimap.h) as f64;
    let small = cv::resize(minimap, 0, 0, k, k, Interp::Area);
    const BINS: usize = 360;
    let raw = cone_profile(&small, BINS);
    let prof: Vec<f64> = (0..BINS).map(|i| (-2i64..=2).map(|j| raw[(i as i64 + j).rem_euclid(BINS as i64) as usize]).sum::<f64>() / 5.0).collect();
    const WIN: usize = 72;
    let (best, best_sum) = best_arc(&prof, WIN);
    let inside = best_sum / WIN as f64;
    let out = outside(&prof, best, WIN);
    let conf = (inside - out).clamp(0.0, 1.0);
    if inside < 0.25 || inside < out * 2.0 + 0.1 {
        return (None, conf);
    }
    let mut arc: Vec<f64> = (0..WIN).map(|j| prof[(best + j) % BINS]).collect();
    arc.sort_by(f64::total_cmp);
    let plateau = arc[(arc.len() as f64 * 0.75) as usize];
    let level = out + 0.5 * (plateau - out);
    let mid = ((best + WIN / 2) % BINS) as i64;
    let edge = |step: i64| -> Option<f64> {
        for d in 0..100i64 {
            let a = (mid + step * d).rem_euclid(BINS as i64) as usize;
            let b = (mid + step * (d + 1)).rem_euclid(BINS as i64) as usize;
            if prof[a] >= level && prof[b] < level {
                return Some(mid as f64 + step as f64 * (d as f64 + (prof[a] - level) / (prof[a] - prof[b]).max(1e-9)));
            }
        }
        None
    };
    if let (Some(l), Some(r)) = (edge(-1), edge(1))
        && (35.0..=140.0).contains(&(r - l))
    {
        return (Some((((l + r) / 2.0 + 0.5) % 360.0 + 360.0) % 360.0), conf);
    }
    from_view_cone_centroid(minimap, ui)
}

/// The fallback reader: the brightness-weighted centre of the brightest 72-degree arc, at half resolution.
pub fn from_view_cone_centroid(minimap: &Mat, _ui: f64) -> (Option<f64>, f64) {
    let k = 105.0 / minimap.w.min(minimap.h) as f64;
    let small = cv::resize(minimap, 0, 0, k, k, Interp::Area);
    const BINS: usize = 90;
    let prof = cone_profile(&small, BINS);
    const WIN: usize = 18;
    let (best, best_sum) = best_arc(&prof, WIN);
    let inside = best_sum / WIN as f64;
    let out = outside(&prof, best, WIN);
    let conf = (inside - out).clamp(0.0, 1.0);
    if inside < 0.25 || inside < out * 2.0 + 0.1 {
        return (None, conf);
    }
    let (mut sx, mut sy) = (0.0, 0.0);
    for j in 0..WIN {
        let i = (best + j) % BINS;
        let a = ((i * 4 + 2) as f64).to_radians();
        sx += prof[i] * a.sin();
        sy += prof[i] * a.cos();
    }
    (Some((sx.atan2(sy).to_degrees() + 360.0) % 360.0), conf)
}
