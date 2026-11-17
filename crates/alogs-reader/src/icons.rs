//! The game's own pictures behind the Party and Victory screens' icons (head portraits, card art, buff icons, Holo-
//! Battle Sim pictures) and the matchers that use them. Port of aLogs B16 `bossbar/icons.py`: the cache
//! (icons.json + PNGs in the data folder) is read here, and written from the game's Unity bundles by
//! `icons_extract::extract` when it's missing or from another game build (`Icons::load`).

use alogs_pil::ops::{darker, difference, lighter, multiply, subtract};
use alogs_pil::{Filter, Image, Mode};
use indexmap::IndexMap;
use serde_json::Value as J;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

pub const CACHE_VERSION: i64 = 9;
pub const HEAD_SIZE: usize = 96;
pub const BUFF_SIZE: usize = 64;
pub const SIM_SIZE: usize = 96;

type BoxF = (f64, f64, f64, f64);

fn r(v: f64) -> i64 {
    v.round_ties_even() as i64
}

/// `count`, `mean` and `var` per band, as ImageStat (and icons.Stat) give them.
struct St {
    count: Vec<u64>,
    mean: Vec<f64>,
    var: Vec<f64>,
}

fn stat(img: &Image, mask: Option<&Image>) -> St {
    let h = img.histogram(mask);
    let (mut count, mut mean, mut var) = (vec![], vec![], vec![]);
    for band in h.chunks(256) {
        let n: u64 = band.iter().sum();
        let s = band.iter().enumerate().map(|(j, &c)| j as u64 * c).sum::<u64>() as f64;
        let s2 = band.iter().enumerate().map(|(j, &c)| (j * j) as u64 * c).sum::<u64>() as f64;
        count.push(n);
        mean.push(if n > 0 { s / n as f64 } else { 0.0 });
        var.push(if n > 0 { (s2 - s.powf(2.0) / n as f64) / n as f64 } else { 0.0 });
    }
    St { count, mean, var }
}

/// Masked normalized correlation of two same-size RGB images, averaged over the channels (-1..1).
pub fn ncc(a: &Image, b: &Image, mask: &Image) -> f64 {
    let (sa, sb) = (stat(a, Some(mask)), stat(b, Some(mask)));
    if sa.count[0] == 0 {
        return -1.0;
    }
    let cross = stat(&multiply(a, b), Some(mask)).mean;
    let mut out = 0.0;
    for k in 0..3 {
        let cov = cross[k] * 255.0 - sa.mean[k] * sb.mean[k];
        out += cov / ((sa.var[k] * sb.var[k]).powf(0.5) + 1e-6);
    }
    out / 3.0
}

/// Masked normalized correlation over all three channels together (-1..1).
pub fn ncc_pooled(a: &Image, b: &Image, mask: &Image) -> f64 {
    let (sa, sb) = (stat(a, Some(mask)), stat(b, Some(mask)));
    if sa.count[0] == 0 {
        return -1.0;
    }
    let cross = stat(&multiply(a, b), Some(mask)).mean;
    let ma = sa.mean.iter().sum::<f64>() / 3.0;
    let mb = sb.mean.iter().sum::<f64>() / 3.0;
    let cov = (0..3).map(|k| cross[k] * 255.0).sum::<f64>() / 3.0 - ma * mb;
    let va = (0..3).map(|k| sa.var[k] + sa.mean[k].powi(2)).sum::<f64>() / 3.0 - ma * ma;
    let vb = (0..3).map(|k| sb.var[k] + sb.mean[k].powi(2)).sum::<f64>() / 3.0 - mb * mb;
    cov / ((va * vb).powf(0.5) + 1e-6)
}

fn alpha(im: &Image) -> Image {
    im.split().pop().unwrap()
}
fn above(t: u8) -> impl Fn(u8) -> f64 {
    move |v| if v > t { 255.0 } else { 0.0 }
}
fn crop_int(im: &Image, b: BoxF) -> Image {
    im.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64)
}
fn rgb3(c: &Image) -> (Image, Image, Image) {
    let mut s = c.split().into_iter();
    (s.next().unwrap(), s.next().unwrap(), s.next().unwrap())
}

/// `(key, score, margin over the next name, its own key)` of a match.
pub type Match = (String, f64, f64, String);

fn ranked(best: IndexMap<String, (f64, String)>) -> Option<Match> {
    let mut v: Vec<(String, (f64, String))> = best.into_iter().collect();
    v.sort_by(|a, b| b.1.0.total_cmp(&a.1.0)); // stable: ties keep their order
    let (name, (s, key)) = v.first()?.clone();
    Some((name, s, s - v.get(1).map_or(-1.0, |x| x.1.0), key))
}

/// sorted(((score, key)...), reverse=True)
fn desc_keys(mut v: Vec<(f64, String)>) -> Vec<(f64, String)> {
    v.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    v
}

pub struct Icons {
    pub build: J,
    pub heads: IndexMap<String, (String, Image)>,
    pub cards: IndexMap<String, (String, Image)>,
    pub buffs: IndexMap<String, J>,
    buff_art: HashMap<String, Image>,
    pub sim: J,
    pub sim_gizmos: J,
    sim_art: IndexMap<(String, String), Image>,
    fades: HashMap<String, Image>,
    sized: RefCell<HashMap<(String, usize), Image>>,
    faded: RefCell<HashMap<(usize, String), Image>>,
    cuts: RefCell<HashMap<(String, usize, (i64, i64, i64, i64), String), Option<(Image, Option<Image>, Image)>>>,
    card_sized: RefCell<HashMap<(String, usize, usize), Image>>,
    digits: RefCell<Option<IndexMap<char, Vec<Image>>>>,
}

/// What shows in one card's window, made small, and where the whole art lies over it at that size.
struct CardLook {
    crop: Image,
    w0: f64,
    h0: f64,
    ox0: f64,
    oy0: f64,
}

impl CardLook {
    fn new(im: &Image, window: BoxF, art: BoxF, n: usize) -> CardLook {
        let (x0, y0, x1, y1) = window;
        let h = r(n as f64 * (y1 - y0) / (x1 - x0)).max(4) as usize;
        let k = n as f64 / (x1 - x0);
        let crop = crop_int(im, window).convert(Mode::Rgb).resize(n, h, Some(Filter::Bilinear));
        CardLook { crop, w0: art.2 * k, h0: art.3 * k, ox0: (art.0 - x0) * k, oy0: (art.1 - y0) * k }
    }
}

fn fade_of(kind: &str) -> (&'static str, f64) {
    if kind == "lead" { ("ui_img_grabeggs_centeritem_mask1", 1.0) } else { ("ui_img_grabeggs_centeritem_mask2", 208.0 / 204.0) }
}

impl Icons {
    /// Is this fade ('team', 'lead') in the cache?
    pub fn has_fade(&self, kind: &str) -> bool {
        self.fades.contains_key(kind)
    }

    /// Are the Holo-Battle Sim pictures cached?
    pub fn has_sim_art(&self) -> bool {
        !self.sim_art.is_empty()
    }

    /// The cached icons, read from the game again when it was updated since (icons.py `Icons.load`); the error says
    /// why when the game can't be read.
    pub fn load(cache_dir: &Path, player_log: Option<&Path>) -> Result<Icons, String> {
        if let Some(icons) = Icons::open(cache_dir) {
            let Some((_, game)) = crate::bundles::game(player_log) else { return Ok(icons) };
            match crate::bundles::build(&game) {
                None => return Ok(icons),
                Some(b) if icons.build.as_str() == Some(b.as_str()) => return Ok(icons),
                _ => {}
            }
        }
        match crate::icons_extract::extract(cache_dir, player_log) {
            Ok(true) => Icons::open(cache_dir).ok_or_else(|| "the extracted icons can't be read".to_string()),
            Ok(false) => Err("the game install was not found".into()),
            Err(e) => Err(e),
        }
    }

    /// The cached pictures in `cache_dir`, or None when the cache is missing or from another version.
    pub fn open(cache_dir: &Path) -> Option<Icons> {
        let meta: J = serde_json::from_str(&std::fs::read_to_string(cache_dir.join("icons.json")).ok()?).ok()?;
        if meta.get("version").and_then(J::as_i64) != Some(CACHE_VERSION) {
            return None;
        }
        let png = |p: std::path::PathBuf| Image::open_png(&p).map(|i| i.convert(Mode::Rgba));
        let mut heads = IndexMap::new();
        for (k, n) in meta["heads"].as_object()? {
            heads.insert(k.clone(), (n.as_str().unwrap_or("").to_string(), png(cache_dir.join("heads").join(format!("{k}.png")))?));
        }
        let mut cards = IndexMap::new();
        if let Some(c) = meta.get("cards").and_then(J::as_object) {
            for (k, n) in c {
                cards.insert(k.clone(), (n.as_str().unwrap_or("").to_string(), png(cache_dir.join("cards").join(format!("{k}.png")))?));
            }
        }
        let buffs: IndexMap<String, J> = meta["buffs"].as_object()?.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let mut buff_art = HashMap::new();
        for b in buffs.values() {
            let icon = b["icon"].as_str()?.to_string();
            if !buff_art.contains_key(&icon) {
                buff_art.insert(icon.clone(), alpha(&png(cache_dir.join("buffs").join(format!("{icon}.png")))?));
            }
        }
        let sim = meta.get("sim").cloned().unwrap_or(J::Object(Default::default()));
        let mut sim_art = IndexMap::new();
        if let Some(s) = sim.as_object() {
            for (sid, v) in s {
                for part in ["series", "core", "transform"] {
                    if v.get(format!("{part}_art")).is_some_and(|a| !a.is_null() && a != &J::Bool(false) && a != "") {
                        sim_art.insert((part.to_string(), sid.clone()), png(cache_dir.join("sim").join(format!("{part}_{sid}.png")))?);
                    }
                }
            }
        }
        let mut fades = HashMap::new();
        for k in ["team", "lead"] {
            if meta.get(format!("{k}_fade")).is_some_and(|v| v.as_bool() != Some(false) && !v.is_null()) {
                fades.insert(k.to_string(), Image::open_png(&cache_dir.join(format!("{k}_fade.png")))?.convert(Mode::L));
            }
        }
        Some(Icons {
            build: meta.get("build").cloned().unwrap_or(J::Null),
            heads,
            cards,
            buffs,
            buff_art,
            sim_gizmos: meta.get("sim_gizmos").cloned().unwrap_or(J::Object(Default::default())),
            sim,
            sim_art,
            fades,
            sized: RefCell::new(HashMap::new()),
            faded: RefCell::new(HashMap::new()),
            cuts: RefCell::new(HashMap::new()),
            card_sized: RefCell::new(HashMap::new()),
            digits: RefCell::new(None),
        })
    }

    fn head_sized(&self, key: &str, side: usize) -> Image {
        let k = (key.to_string(), side);
        if let Some(i) = self.sized.borrow().get(&k) {
            return i.clone();
        }
        let i = self.heads[key].1.resize(side, side, Some(Filter::Bilinear));
        self.sized.borrow_mut().insert(k, i.clone());
        i
    }

    fn fade(&self, side: usize, kind: &str) -> Image {
        let k = (side, kind.to_string());
        if let Some(i) = self.faded.borrow().get(&k) {
            return i.clone();
        }
        let big = r(side as f64 * fade_of(kind).1) as usize;
        let o = ((big as i64 - side as i64).div_euclid(2)) as i64;
        let i = self.fades[kind].resize(big, big, Some(Filter::Bilinear)).crop_i(o, o, o + side as i64, o + side as i64);
        self.faded.borrow_mut().insert(k, i.clone());
        i
    }

    fn cut(&self, key: &str, side: usize, bx: (i64, i64, i64, i64), fade: &str) -> Option<(Image, Option<Image>, Image)> {
        let k = (key.to_string(), side, bx, fade.to_string());
        if let Some(c) = self.cuts.borrow().get(&k) {
            return c.clone();
        }
        let part = self.head_sized(key, side).crop_i(bx.0, bx.1, bx.2, bx.3);
        let (shown, mask, least) = if !fade.is_empty() {
            let shown = multiply(&alpha(&part), &self.fade(side, fade).crop_i(bx.0, bx.1, bx.2, bx.3));
            let mask = shown.point(above(64));
            (Some(shown), mask, 0.15)
        } else {
            (None, alpha(&part).point(above(200)), 0.3)
        };
        let frac = 255.0 * mask.histogram(None)[255] as f64 / (mask.width * mask.height) as f64;
        let cut = if frac < least * 255.0 { None } else { Some((part.convert(Mode::Rgb), shown, mask)) };
        let mut cuts = self.cuts.borrow_mut();
        if cuts.len() >= 6000 {
            cuts.clear();
        }
        cuts.insert(k, cut.clone());
        cut
    }

    /// The Aniimo whose portrait shows through `window`; `square` = (x, y, side) of its whole square. `fade`: "",
    /// "team" or "lead".
    pub fn head(&self, im: &Image, window: BoxF, square: (f64, f64, f64), n: usize, fade: &str) -> Option<Match> {
        let (x0, y0, x1, y1) = window;
        let h = r(n as f64 * (y1 - y0) / (x1 - x0)).max(4) as usize;
        let k = n as f64 / (x1 - x0);
        let crop = crop_int(im, window).convert(Mode::Rgb).resize(n, h, Some(Filter::Bilinear));
        let fade = if self.fades.contains_key(fade) { fade } else { "" };
        let card = if fade.is_empty() {
            None
        } else {
            let m = alogs_pil::ops::Stat::new(&crop, None).mean();
            Some(Image::new(Mode::Rgb, n, h, &[m[0] as u8, m[1] as u8, m[2] as u8]))
        };
        let side0 = square.2 * k;
        let (ox0, oy0) = ((square.0 - x0) * k, (square.1 - y0) * k);
        let score = |key: &str, scale: f64, dx: i64, dy: i64| -> f64 {
            let side = (side0 * scale) as i64;
            let ox = r(ox0 + (side0 - side as f64) / 2.0) + dx;
            let oy = r(oy0 + (side0 - side as f64) / 2.0) + dy;
            if ox > 0 || oy > 0 || ox + side < n as i64 || oy + side < h as i64 {
                return -1.0;
            }
            let bx = (-ox, -oy, -ox + n as i64, -oy + h as i64);
            let Some((rgb, shown, mask)) = self.cut(key, side as usize, bx, fade) else { return -1.0 };
            match (&card, shown) {
                (Some(c), Some(s)) => ncc_pooled(&crop, &Image::composite(&rgb, c, &s), &mask),
                _ => ncc_pooled(&crop, &rgb, &mask),
            }
        };
        let first = desc_keys(self.heads.keys().map(|key| (score(key, 1.0, 0, 0), key.clone())).collect());
        let mut best: IndexMap<String, (f64, String)> = IndexMap::new();
        for (_, key) in first.into_iter().take(12) {
            let mut s = f64::NEG_INFINITY;
            for sc in [0.95, 1.0, 1.05] {
                for dx in -2..=2 {
                    for dy in -2..=2 {
                        s = s.max(score(&key, sc, dx, dy));
                    }
                }
            }
            let name = self.heads[&key].0.clone();
            if s > best.get(&name).map_or(-2.0, |b| b.0) {
                best.insert(name, (s, key));
            }
        }
        ranked(best)
    }

    fn card_sized(&self, key: &str, w: usize, h: usize) -> Image {
        let k = (key.to_string(), w, h);
        if let Some(i) = self.card_sized.borrow().get(&k) {
            return i.clone();
        }
        let mut c = self.card_sized.borrow_mut();
        if c.len() > 4000 {
            c.clear();
        }
        let i = self.cards[key].1.resize(w, h, Some(Filter::Bilinear));
        c.insert(k, i.clone());
        i
    }

    /// How well one card art fits what shows in a card's window, the art a little bigger or smaller and a pixel or two
    /// off its place.
    fn card_score(&self, look: &CardLook, key: &str, scale: f64, dx: i64, dy: i64) -> f64 {
        let (n, h) = (look.crop.width, look.crop.height);
        let (w, hh) = (((look.w0 * scale) as i64).max(1), ((look.h0 * scale) as i64).max(1));
        let ox = r(look.ox0 + (look.w0 - w as f64) / 2.0) + dx;
        let oy = r(look.oy0 + (look.h0 - hh as f64) / 2.0) + dy;
        if ox > 0 || oy > 0 || ox + w < n as i64 || oy + hh < h as i64 {
            return -1.0;
        }
        let part = self.card_sized(key, w as usize, hh as usize).crop_i(-ox, -oy, -ox + n as i64, -oy + h as i64);
        let mask = alpha(&part).point(above(200));
        if (mask.histogram(None)[255] as f64) < 0.3 * (n * h) as f64 {
            return -1.0;
        }
        ncc_pooled(&look.crop, &part.convert(Mode::Rgb), &mask)
    }

    /// One card art's best fit over the sizes and places tried.
    fn card_fit(&self, look: &CardLook, key: &str) -> f64 {
        let mut s = f64::NEG_INFINITY;
        for sc in [0.96, 1.0, 1.04] {
            for dx in -2..=2 {
                for dy in -2..=2 {
                    s = s.max(self.card_score(look, key, sc, dx, dy));
                }
            }
        }
        s
    }

    /// The Aniimo whose card art shows in `window`; `art` = (x, y, w, h) of the whole art.
    pub fn card(&self, im: &Image, window: BoxF, art: BoxF, n: usize) -> Option<Match> {
        if self.cards.is_empty() {
            return None;
        }
        let look = CardLook::new(im, window, art, n);
        let first = desc_keys(self.cards.keys().map(|key| (self.card_score(&look, key, 1.0, 0, 0), key.clone())).collect());
        let mut best: IndexMap<String, (f64, String)> = IndexMap::new();
        for (_, key) in first.into_iter().take(12) {
            let s = self.card_fit(&look, &key);
            let name = self.cards[&key].0.clone();
            if s > best.get(&name).map_or(-2.0, |b| b.0) {
                best.insert(name, (s, key));
            }
        }
        ranked(best)
    }

    /// Every Aniimo's card art made the size it shows at in one card slot (`window`, `art` and `n` as `card` takes
    /// them), ahead of a match: what every card has in common is its slot, the same window onto the same place of
    /// whichever art is in it, so each art's part for a slot can be made before any card is on screen. `card` spent
    /// most of its time making them (0.53 of 0.65 s for the five slots, 17 Nov); found ready, it tries all 313 arts
    /// for a card in about 23 ms. At most `budget` arts are made a call (a third of a millisecond each), counted
    /// down; the number still to make is returned.
    pub fn warm_cards(&self, window: BoxF, art: BoxF, n: usize, budget: &mut usize) -> usize {
        let k = n as f64 / (window.2 - window.0);
        let (w, h) = (((art.2 * k) as i64).max(1) as usize, ((art.3 * k) as i64).max(1) as usize);
        let mut left = 0;
        for key in self.cards.keys() {
            if self.card_sized.borrow().contains_key(&(key.clone(), w, h)) {
                continue;
            }
            if *budget == 0 {
                left += 1;
                continue;
            }
            *budget -= 1;
            self.card_sized(key, w, h);
        }
        left
    }

    /// How well the card art of the Aniimo called `name` fits what shows in `window` (the same score `card` gives its
    /// winner), or None when the game has no card art under that name. One Aniimo's art is tried, not all of them: for
    /// a card whose name is already read, this says in a few ms whether the art agrees.
    pub fn card_of(&self, im: &Image, window: BoxF, art: BoxF, n: usize, name: &str) -> Option<f64> {
        let keys: Vec<&String> = self.cards.iter().filter(|(_, (called, _))| called == name).map(|(key, _)| key).collect();
        if keys.is_empty() {
            return None;
        }
        let look = CardLook::new(im, window, art, n);
        Some(keys.into_iter().map(|key| self.card_fit(&look, key)).fold(f64::NEG_INFINITY, f64::max))
    }

    fn badge_look(rad: f64) -> (Image, Image, i64) {
        let k = (rad * 2.0).round_ties_even() / 2.0;
        let n = ((k * 1.2) as usize) * 2 + 1;
        let c = (n / 2) as f64;
        let mut img = Image::new(Mode::Rgb, n, n, &[128, 128, 128]);
        let mut mask = Image::new(Mode::L, n, n, &[0]);
        for (t, col) in [(1.07, [220u8, 220, 220]), (0.97, [60, 60, 75])] {
            let e = (c - k * t, c - k * t, c + k * t, c + k * t);
            alogs_pil::draw::ellipse(&mut img, e, Some(&col), None, 1);
            alogs_pil::draw::ellipse(&mut mask, e, Some(&[255]), None, 1);
        }
        let e = (c - k * 0.7, c - k * 0.7, c + k * 0.7, c + k * 0.7);
        alogs_pil::draw::ellipse(&mut img, e, Some(&[128, 128, 128]), None, 1);
        alogs_pil::draw::ellipse(&mut mask, e, Some(&[0]), None, 1);
        (img, mask, c as i64)
    }

    /// The centre of the skill badge nearest (x, y): (score, x, y).
    pub fn badge(&self, im: &Image, x: f64, y: f64, rad: f64, reach: i64, reach_y: Option<i64>, step: usize) -> (f64, f64, f64) {
        let (img, mask, c) = Self::badge_look(rad);
        let n = img.width as i64;
        let ry = reach_y.unwrap_or(reach);
        let at = |px: f64, py: f64| {
            let (x0, y0) = (r(px) - c, r(py) - c);
            ncc(&im.crop_i(x0, y0, x0 + n, y0 + n).convert(Mode::Rgb), &img, &mask)
        };
        // max() over (score, x, y): ties go to the larger x, then y
        let better = |a: (f64, f64, f64), b: (f64, f64, f64)| (a.0, a.1, a.2).partial_cmp(&(b.0, b.1, b.2)) == Some(std::cmp::Ordering::Greater);
        let mut best: Option<(f64, f64, f64)> = None;
        let mut dx = -reach;
        while dx <= reach {
            let mut dy = -ry;
            while dy <= ry {
                let cand = (at(x + dx as f64, y + dy as f64), x + dx as f64, y + dy as f64);
                if best.is_none_or(|b| better(cand, b)) {
                    best = Some(cand);
                }
                dy += step as i64;
            }
            dx += step as i64;
        }
        let mut best = best.unwrap();
        for _ in 0..(if step > 2 { 2 } else { 1 }) {
            let mut nb: Option<(f64, f64, f64)> = None;
            for dx in [-1.0, 0.0, 1.0] {
                for dy in [-1.0, 0.0, 1.0] {
                    let cand = (at(best.1 + dx, best.2 + dy), best.1 + dx, best.2 + dy);
                    if nb.is_none_or(|b| better(cand, b)) {
                        nb = Some(cand);
                    }
                }
            }
            best = nb.unwrap();
        }
        best
    }

    /// The series whose white glyph shows in `box`: (series id, overlap, margin), or None for an empty slot.
    pub fn sim_glyph(&self, im: &Image, b: BoxF) -> Option<(String, f64, f64)> {
        let crop = crop_int(im, b).convert(Mode::Rgb);
        let (rr, g, bb) = rgb3(&crop);
        let low = darker(&darker(&rr, &g), &bb);
        let high = lighter(&lighter(&rr, &g), &bb);
        let white = multiply(&low.point(above(200)), &subtract(&high, &low, 1.0, 0).point(|v| if v < 40 { 255.0 } else { 0.0 }));
        let outline = |m: &Image| -> Option<Image> {
            let m = m.point(above(128));
            let bx = m.getbbox()?;
            Some(m.crop_i(bx.0 as i64, bx.1 as i64, bx.2 as i64, bx.3 as i64).resize(48, 48, None).point(above(128)))
        };
        let shape = outline(&white)?;
        if shape.histogram(None)[255] < 200 {
            return None;
        }
        let mut res = Vec::new();
        for ((part, sid), art) in &self.sim_art {
            if part != "series" {
                continue;
            }
            let Some(rf) = outline(&alpha(art)) else { continue };
            let both = darker(&shape, &rf).histogram(None)[255];
            let either = lighter(&shape, &rf).histogram(None)[255];
            res.push((if either > 0 { both as f64 / either as f64 } else { 0.0 }, sid.clone()));
        }
        let res = desc_keys(res);
        let (s, sid) = res.first()?.clone();
        Some((sid, s, s - res.get(1).map_or(0.0, |x| x.0)))
    }

    /// The series whose `part` picture ("core" / "transform") shows in `box`: (series id, score, margin).
    pub fn sim_picture(&self, im: &Image, b: BoxF, part: &str) -> Option<(String, f64, f64)> {
        const N: usize = 80;
        let crop = crop_int(im, b).convert(Mode::Rgb).resize(N, N, Some(Filter::Bilinear));
        let mut res = Vec::new();
        for ((p, sid), art) in &self.sim_art {
            if p != part {
                continue;
            }
            let mut best = -1.0f64;
            if part == "transform" {
                for inset in [0usize, 4, 8] {
                    let side = N - 2 * inset;
                    let mut mask = Image::new(Mode::L, N, N, &[0]);
                    let e = ((inset + 4) as f64, (inset + 4) as f64, (N - inset - 4) as f64, (N - inset - 4) as f64);
                    alogs_pil::draw::ellipse(&mut mask, e, Some(&[255]), None, 1);
                    let mut bg = Image::new(Mode::Rgb, art.width, art.height, &[128, 128, 128]);
                    bg.paste_mask(art, 0, 0, Some(art));
                    let mut rf = Image::new(Mode::Rgb, N, N, &[128, 128, 128]);
                    rf.paste(&bg.resize(side, side, Some(Filter::Bilinear)), inset as i64, inset as i64);
                    best = best.max(ncc(&crop, &rf, &mask));
                }
                res.push((best, sid.clone()));
                continue;
            }
            for side in [52usize, 58, 64, 70] {
                let g = art.thumbnail_filter(side, side, Filter::Bilinear);
                let mask = alpha(&g).point(above(160));
                let rgb = g.convert(Mode::Rgb);
                for dx in (-8..=8).step_by(4) {
                    for dy in (-8..=8).step_by(4) {
                        let x = (N as i64 - g.width as i64).div_euclid(2) + dx;
                        let y = (N as i64 - g.height as i64).div_euclid(2) + dy;
                        if x < 0 || y < 0 || x + g.width as i64 > N as i64 || y + g.height as i64 > N as i64 {
                            continue;
                        }
                        best = best.max(ncc(&crop.crop_i(x, y, x + g.width as i64, y + g.height as i64), &rgb, &mask));
                    }
                }
            }
            res.push((best, sid.clone()));
        }
        let res = desc_keys(res);
        let (s, sid) = res.first()?.clone();
        Some((sid, s, s - res.get(1).map_or(-1.0, |x| x.0)))
    }

    /// The buff of `kind` ("season" / "offering") whose white glyph shows in `box`: (icon, score, margin).
    pub fn buff(&self, im: &Image, b: BoxF, kind: &str, text_below: bool) -> Option<(String, f64, f64)> {
        let n = BUFF_SIZE;
        let crop = crop_int(im, b).convert(Mode::Rgb).resize(n, n, Some(Filter::Bilinear));
        let (rr, g, bb) = rgb3(&crop);
        let white = darker(&darker(&rr, &g), &bb);
        let white = Image::merge(Mode::Rgb, &[white.clone(), white.clone(), white]);
        let mut mask = Image::new(Mode::L, n, n, &[0]);
        let nf = n as f64;
        if text_below {
            alogs_pil::draw::rectangle(&mut mask, (0.0, 0.0, nf, (nf * 0.62).trunc()), &[255]);
        } else {
            alogs_pil::draw::ellipse(&mut mask, (nf * 0.1, nf * 0.1, nf * 0.9, nf * 0.9), Some(&[255]), None, 1);
        }
        let (sides, dxs, dys): ((f64, f64), Vec<i64>, Vec<i64>) = if text_below {
            ((0.98, 1.08), (-2..3).step_by(2).collect(), (-4..7).step_by(2).collect())
        } else {
            ((0.8, 0.96), (-2..5).step_by(2).collect(), (-4..9).step_by(2).collect())
        };
        // a set of icons: Python's iteration order is its hash order; sorted here (only ties could differ)
        let icons: std::collections::BTreeSet<String> =
            self.buffs.values().filter(|b| b["kind"].as_str() == Some(kind)).filter_map(|b| b["icon"].as_str().map(String::from)).collect();
        let mut res = Vec::new();
        for icon in icons {
            let mut best = -1.0f64;
            let mut side = (nf * sides.0) as usize;
            while side <= (nf * sides.1) as usize {
                let g = self.buff_art[&icon].resize(side, side, Some(Filter::Bilinear));
                let x0 = (n as i64 - side as i64).div_euclid(2);
                for &dx in &dxs {
                    for &dy in &dys {
                        let mut rf = Image::new(Mode::L, n, n, &[0]);
                        rf.paste(&g, x0 + dx, x0 + dy);
                        best = best.max(ncc(&white, &Image::merge(Mode::Rgb, &[rf.clone(), rf.clone(), rf]), &mask));
                    }
                }
                side += 2;
            }
            res.push((best, icon));
        }
        let res = desc_keys(res);
        let (s, icon) = res.first()?.clone();
        Some((icon, s, s - res.get(1).map_or(-1.0, |x| x.0)))
    }

    /// The buff ids for an icon.
    pub fn buffs_named(&self, icon: &str, kind: &str, level: Option<i64>) -> Vec<String> {
        self.buffs
            .iter()
            .filter(|(_, b)| b["icon"].as_str() == Some(icon) && b["kind"].as_str() == Some(kind) && level.is_none_or(|l| b.get("level").and_then(J::as_i64) == Some(l)))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// A lone white digit in `box` (a Season Buff's "Lv. N"): (digit, distance, margin) or None.
    pub fn digit(&self, im: &Image, b: BoxF) -> Option<(char, f64, f64)> {
        if self.digits.borrow().is_none() {
            let mut d: IndexMap<char, Vec<Image>> = IndexMap::new();
            let dir = std::env::var_os("WINDIR").map_or_else(|| std::path::PathBuf::from(r"C:\Windows"), std::path::PathBuf::from).join("Fonts");
            for fname in ["arialbd.ttf", "seguisb.ttf", "segoeui.ttf", "arial.ttf"] {
                let Some(font) = crate::text::Font::load(&dir.join(fname)) else { continue };
                for ch in "123456789".chars() {
                    let mut g = Image::new(Mode::L, 60, 80, &[0]);
                    font.draw(&mut g, 8.0, 0.0, &ch.to_string(), 60.0, 255);
                    if let Some(s) = shape(&g.point(above(128))) {
                        d.entry(ch).or_default().push(s);
                    }
                }
            }
            *self.digits.borrow_mut() = Some(d);
        }
        let crop = crop_int(im, b).convert(Mode::Rgb);
        let (rr, g, bb) = rgb3(&crop);
        let low = darker(&darker(&rr, &g), &bb);
        let high = lighter(&lighter(&rr, &g), &bb);
        let grey = subtract(&high, &low, 1.0, 0).point(|v| if v < 45 { 255.0 } else { 0.0 });
        let mut mask = multiply(&low.point(above(200)), &grey);
        let top = (mask.height as f64 * 0.18) as i64;
        let mw = mask.width as f64;
        alogs_pil::draw::rectangle(&mut mask, (0.0, 0.0, mw, top as f64), &[0]);
        let s = shape(&mask)?;
        let digits = self.digits.borrow();
        let mut res: Vec<(f64, char)> = digits
            .as_ref()?
            .iter()
            .map(|(d, ts)| {
                let m = ts.iter().map(|t| alogs_pil::ops::Stat::new(&difference(&s, t), None).mean()[0] / 255.0).fold(f64::INFINITY, f64::min);
                (m, *d)
            })
            .collect();
        res.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        if res.len() < 2 {
            return None;
        }
        Some((res[0].1, res[0].0, res[1].0 - res[0].0))
    }
}

/// A glyph's white pixels, cropped to them and set to 14x20 (None when there's hardly anything).
fn shape(mask: &Image) -> Option<Image> {
    let b = mask.getbbox()?;
    if (b.2 - b.0) * (b.3 - b.1) < 8 {
        return None;
    }
    Some(mask.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64).resize(14, 20, Some(Filter::Bilinear)))
}
