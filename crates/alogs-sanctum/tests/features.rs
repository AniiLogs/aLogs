//! ORB matching on a made-up map: the screen shows the art zoomed and shifted; the fit must find that.

use alogs_sanctum::cv::{self, Interp, Mat};
use alogs_sanctum::features::{self, Features};

/// Random overlapping boxes of many grey levels: lots of corners.
fn art(w: usize, h: usize) -> Mat {
    let mut m = Mat::new(w, h, 1);
    m.data.fill(30);
    let mut s = 777u64;
    let mut rnd = |n: usize| {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((s >> 33) as usize) % n
    };
    for _ in 0..400 {
        let (bw, bh) = (8 + rnd(60), 8 + rnd(60));
        let (x, y, v) = (rnd(w - bw), rnd(h - bh), 40 + rnd(210) as u8);
        for yy in y..y + bh {
            m.data[yy * w + x..yy * w + x + bw].fill(v);
        }
    }
    m
}

#[test]
fn orb_fit_recovers_zoom_and_shift() {
    let a = art(800, 600);
    let mut desc = vec![];
    let mut full_pts = vec![];
    for s in [0.5, 1.0] {
        let small = if s == 1.0 { a.clone() } else { cv::resize(&a, 0, 0, s, s, Interp::Area) };
        let (k, d) = features::orb(&small, 3000);
        full_pts.extend(k.iter().map(|p| (f64::from(p.x) / s, f64::from(p.y) / s)));
        desc.extend(d);
    }
    let feats = Features { desc, full_pts };
    // screen = art * 0.8 + (40, 25), cut to 600 x 450
    let zoomed = cv::resize(&a, 0, 0, 0.8, 0.8, Interp::Area);
    let mut screen = Mat::new(600, 450, 1);
    screen.data.fill(30);
    for y in 25..450 {
        for x in 40..600 {
            let (sx, sy) = (x - 40, y - 25);
            if sx < zoomed.w && sy < zoomed.h {
                screen.data[y * 600 + x] = zoomed.data[sy * zoomed.w + sx];
            }
        }
    }
    let (k, d) = features::orb(&screen, 6000);
    assert!(k.len() > 300, "{} key points", k.len());
    let ([a_, b_, tx, ty], inl, matches) = features::fit(&k, &d, 1.0, &feats, 0.78, 4.0).expect("no fit");
    let kk = (a_ * a_ + b_ * b_).sqrt();
    let scale = 1.0 / kk;
    assert!(inl >= 40, "{inl}/{matches} inliers");
    assert!((scale - 0.8).abs() < 0.01, "scale {scale}");
    assert!(b_.atan2(a_).to_degrees().abs() < 0.5);
    let (sx, sy) = (-tx * scale, -ty * scale);
    assert!((sx - 40.0).abs() < 2.0 && (sy - 25.0).abs() < 2.0, "shift ({sx}, {sy})");
}

#[test]
fn ransac_ignores_outliers() {
    let src: Vec<(f64, f64)> = (0..60).map(|i| ((i * 37 % 500) as f64, (i * 91 % 400) as f64)).collect();
    let mut dst: Vec<(f64, f64)> = src.iter().map(|&(x, y)| (1.5 * x + 10.0, 1.5 * y - 4.0)).collect();
    for (i, d) in dst.iter_mut().enumerate().take(20) {
        *d = ((i * 13) as f64, (i * 7) as f64);
    }
    let (m, n) = features::estimate_similarity(&src, &dst, 3.0, 3000, 0.995, 10).unwrap();
    assert_eq!(n, 40);
    assert!((m[0] - 1.5).abs() < 1e-6 && m[1].abs() < 1e-6 && (m[2] - 10.0).abs() < 1e-4 && (m[3] + 4.0).abs() < 1e-4, "{m:?}");
}
