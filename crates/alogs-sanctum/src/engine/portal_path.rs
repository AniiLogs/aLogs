//! Which of the island's five blue doors a Sanctum portal icon on the minimap is, from the paths around it.
//!
//! Near the doors the minimap is mostly fog with a faint path, while the island art has ground texture the minimap
//! doesn't show: compared by colour, the wrong door can fit better than the right one (on 23 Nov the one by Lookout
//! Hill scored 0.23 against 0.44 for the one by the north-east compound, 650 px away). Paths are lighter than the
//! ground around them in both, so both pictures are compared by their fine detail only (each minus its blurred self)
//! on the minimap's open ground: on seven screenshots at that door the right door scored 0.38 to 0.55 and every other
//! door 0.07 or less.

use super::Core;
use crate::cv::{self, Interp, Mat, MatF};
use crate::cv2::{self, Kernel};
use crate::layout_db::Layout;
use crate::vision::P2;

/// A door's path fit must be at least this, and this much better than the next door's, to decide.
const MIN_FIT: f64 = 0.2;
const MIN_LEAD: f64 = 0.1;
/// How far (island-art px) around the place the icon points to the fit looks: the icon's spot is found to a few px.
const SLACK: i64 = 8;
/// Box blur size, applied twice (about a Gaussian of 4 px).
const BLUR: usize = 9;

/// A picture minus its blurred self: its paths and edges, its flat fog gone.
fn detail(gray: &MatF) -> MatF {
    let b = cv2::blur(&cv2::blur(gray, BLUR), BLUR);
    MatF { data: gray.data.iter().zip(&b.data).map(|(g, b)| g - b).collect(), ..b }
}

fn gray_f(m: &Mat) -> MatF {
    cv::bgr2gray(m).to_f32(1.0)
}

/// Correlation of a with the window of b at (x0, y0), over the pixels the mask keeps.
fn fit(a: &MatF, b: &MatF, x0: usize, y0: usize, keep: &[usize]) -> f64 {
    let n = keep.len() as f64;
    if n < 50.0 {
        return -1.0;
    }
    let at = |i: usize| b.data[(y0 + i / a.w) * b.w + x0 + i % a.w];
    let (ma, mb) = keep.iter().fold((0f64, 0f64), |(sa, sb), &i| (sa + f64::from(a.data[i]), sb + f64::from(at(i))));
    let (ma, mb) = (ma / n, mb / n);
    let (mut ab, mut aa, mut bb) = (0f64, 0f64, 0f64);
    for &i in keep {
        let (x, y) = (f64::from(a.data[i]) - ma, f64::from(at(i)) - mb);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    ab / ((aa * bb).sqrt() + 1e-6)
}

impl Core {
    /// The player's island position when the blue portal icon sits `icon` (minimap px at 1080p, from the middle) away,
    /// decided by the paths: Some((door, player, its fit, the next door's fit)) only when one door fits clearly best.
    /// `small`: the minimap at 1080p size.
    pub(super) fn portal_by_paths(&mut self, small: &Mat, icon: P2, isl: &Layout) -> Option<(P2, P2, f64, f64)> {
        let full = self.arts.get(isl, 0)?.bgr();
        let doors: Vec<P2> = isl.markers.iter().filter(|m| m.kind == "entrance_side").map(|m| (m.img[0], m.img[1])).collect();
        pick_door(small, icon, &full, &doors)
    }
}

/// As Core::portal_by_paths, with the island art (BGR) and its blue doors given.
pub fn pick_door(small: &Mat, icon: P2, full: &Mat, doors: &[P2]) -> Option<(P2, P2, f64, f64)> {
    // the minimap at island-art scale (1.2 minimap px per art px)
    let sm = cv::resize(small, 0, 0, 1.0 / 1.2, 1.0 / 1.2, Interp::Area);
    let (w, h) = (sm.w, sm.h);
    if w < 40 || h < 40 {
        return None;
    }
    let hsv = cv::bgr2hsv(&sm);
    let icons = cv2::dilate(&cv::in_range(&hsv, &[0, 121, 91], &[180, 255, 255]), &Kernel::rect(9, 9));
    let white = cv2::dilate(&cv::in_range(&hsv, &[0, 0, 201], &[180, 44, 255]), &Kernel::rect(7, 7));
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let r = w.min(h) as f64 * 0.42;
    let (ix, iy) = (cx + icon.0 / 1.2, cy + icon.1 / 1.2);
    let keep: Vec<usize> = (0..w * h)
        .filter(|&i| {
            let (x, y) = ((i % w) as f64 + 0.5, (i / w) as f64 + 0.5);
            let d = (x - cx).hypot(y - cy);
            d < r && d > (r / 4.0).max(5.0) && icons.data[i] == 0 && white.data[i] == 0 && (x - ix).hypot(y - iy) > 14.0
        })
        .collect();
    let mine = detail(&gray_f(&sm));
    let mut fits: Vec<(P2, P2, f64)> = vec![];
    for &door in doors {
        let (px, py) = (door.0 - icon.0 / 1.2, door.1 - icon.1 / 1.2);
        // the art around this door's spot, with room for the slack and the blur's border
        let pad = SLACK + BLUR as i64;
        let (x0, y0) = (px as i64 - w as i64 / 2 - pad, py as i64 - h as i64 / 2 - pad);
        let (rw, rh) = (w as i64 + 2 * pad, h as i64 + 2 * pad);
        if x0 < 0 || y0 < 0 || x0 + rw > full.w as i64 || y0 + rh > full.h as i64 {
            continue;
        }
        let theirs = detail(&gray_f(&full.roi(x0 as usize, y0 as usize, rw as usize, rh as usize)));
        let mut best = (-1.0, (px, py));
        for oy in (-SLACK..=SLACK).step_by(2) {
            for ox in (-SLACK..=SLACK).step_by(2) {
                let f = fit(&mine, &theirs, (BLUR as i64 + SLACK + ox) as usize, (BLUR as i64 + SLACK + oy) as usize, &keep);
                if f > best.0 {
                    best = (f, (px + ox as f64, py + oy as f64));
                }
            }
        }
        fits.push((door, best.1, best.0));
    }
    fits.sort_by(|a, b| b.2.total_cmp(&a.2));
    let (top, next) = (fits.first()?, fits.get(1).map_or(0.0, |f| f.2));
    (top.2 >= MIN_FIT && top.2 - next >= MIN_LEAD).then_some((top.0, top.1, top.2, next))
}

#[cfg(test)]
mod tests {
    /// Local check against real screenshots (never in the repository: game pictures and map art). ALOGS_PORTAL_SHOTS: a
    /// folder of 2560x1440 game screenshots taken standing by the blue door at ALOGS_PORTAL_DOOR ("x,y"), with
    /// ALOGS_SANCTUM_ASSETS the Assets folder. Run with --ignored.
    #[test]
    #[ignore]
    fn screenshots_pick_their_door() {
        let (Some(dir), Some(assets), Some(want)) = (std::env::var_os("ALOGS_PORTAL_SHOTS"), std::env::var_os("ALOGS_SANCTUM_ASSETS"), std::env::var("ALOGS_PORTAL_DOOR").ok()) else {
            return;
        };
        let want: Vec<f64> = want.split(',').map(|v| v.trim().parse().unwrap()).collect();
        let assets = std::path::PathBuf::from(assets);
        let art = crate::vision::imread(&assets.join("maps").join("3005.png")).unwrap();
        let art = if art.c == 4 { crate::cv::bgra2bgr(&art) } else { art };
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(assets.join("layouts.json")).unwrap()).unwrap();
        let isl = json["layouts"].as_array().unwrap().iter().find(|l| l["layout"] == 3005).unwrap();
        let doors: Vec<(f64, f64)> = isl["markers"].as_array().unwrap().iter().filter(|m| m["kind"] == "entrance_side").map(|m| (m["img"][0].as_f64().unwrap(), m["img"][1].as_f64().unwrap())).collect();
        let icon = crate::vision::imread(&assets.join("icons").join("mm_portal_blue.png")).unwrap();
        let icon = if icon.c == 4 { crate::cv::bgra2bgr(&icon) } else { icon };
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let shot = crate::vision::imread(&e.path()).unwrap();
            let shot = if shot.c == 4 { crate::cv::bgra2bgr(&shot) } else { shot };
            let k = shot.h as f64 / 1080.0;
            let mm = shot.roi((75.0 * k) as usize, (32.0 * k) as usize, (210.0 * k) as usize, (210.0 * k) as usize);
            let small = crate::cv::resize(&mm, 210, 210, 0.0, 0.0, crate::cv::Interp::Area);
            let res = crate::cv2::match_template(&small, &icon);
            let (_, _, _, loc) = res.min_max_loc();
            let icon_at = (loc.0 as f64 + icon.w as f64 / 2.0 - 105.0, loc.1 as f64 + icon.h as f64 / 2.0 - 105.0);
            let got = super::pick_door(&small, icon_at, &art, &doors);
            eprintln!("{}: icon {icon_at:?} -> {got:?}", e.path().display());
            let (door, ..) = got.expect("no clear door");
            assert!((door.0 - want[0]).abs() < 1.0 && (door.1 - want[1]).abs() < 1.0, "picked {door:?}");
        }
    }
}
