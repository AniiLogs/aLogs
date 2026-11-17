//! vision.rs on made-up pictures: the HUD box, PNG loading as OpenCV loads it, and the view-cone heading.

use alogs_sanctum::cv::Mat;
use alogs_sanctum::vision;

#[test]
fn hud_box_is_16_by_9() {
    assert_eq!(vision::hud(1920, 1080), (0, 0, 1920, 1080));
    assert_eq!(vision::hud(2560, 1080), (320, 0, 1920, 1080));
    assert_eq!(vision::hud(1280, 1024), (0, 0, 1280, 720));
    assert!((vision::ui_scale(3840, 2160) - 2.0).abs() < 1e-9);
}

#[test]
fn png_loads_as_bgra() {
    let dir = std::env::temp_dir().join(format!("alogs-vision-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("t.png");
    {
        let f = std::fs::File::create(&p).unwrap();
        let mut e = png::Encoder::new(f, 2, 1);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&[10, 20, 30, 40, 50, 60, 70, 80]).unwrap();
    }
    let m = vision::imread(&p).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!((m.w, m.h, m.c), (2, 1, 4));
    assert_eq!(m.data, vec![30, 20, 10, 40, 70, 60, 50, 80]);
}

/// A dark minimap with a white 70-degree wedge centred on `deg` (clockwise from north).
fn cone(deg: f64) -> Mat {
    let n = 210;
    let mut m = Mat::new(n, n, 3);
    m.data.fill(40);
    let c = n as f64 / 2.0;
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f64 + 0.5 - c, y as f64 + 0.5 - c);
            let r = (dx * dx + dy * dy).sqrt();
            let a = (dx.atan2(-dy).to_degrees() + 360.0) % 360.0;
            let d = ((a - deg + 540.0) % 360.0 - 180.0).abs();
            if r < 80.0 && d < 35.0 {
                let i = (y * n + x) * 3;
                m.data[i..i + 3].fill(240);
            }
        }
    }
    m
}

#[test]
fn view_cone_gives_the_heading() {
    for deg in [0.0, 45.0, 137.0, 270.0, 350.0] {
        let (h, conf) = vision::from_view_cone(&cone(deg), 1.0);
        let h = h.unwrap_or_else(|| panic!("no heading for {deg}"));
        let err = ((h - deg + 540.0) % 360.0 - 180.0).abs();
        assert!(err < 3.0, "{deg}: got {h}");
        assert!(conf > 0.5);
        let (c, _) = vision::from_view_cone_centroid(&cone(deg), 1.0);
        let err = ((c.unwrap() - deg + 540.0) % 360.0 - 180.0).abs();
        assert!(err < 5.0, "centroid {deg}: got {c:?}");
    }
    let mut dark = Mat::new(210, 210, 3);
    dark.data.fill(40);
    assert!(vision::from_view_cone(&dark, 1.0).0.is_none());
}

use alogs_sanctum::layout_db::LayoutDb;
use alogs_sanctum::vision::{MapArtCache, MinimapTracker};
use std::sync::Arc;

/// A made-up floor plan: random rooms joined by corridors, grey floor on a transparent sheet.
fn floor_plan(w: usize, h: usize) -> Vec<u8> {
    let mut seed = 12345u64;
    let mut rnd = |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as usize) % n
    };
    let mut a = vec![0u8; w * h * 4];
    let fill = |a: &mut Vec<u8>, x0: usize, y0: usize, x1: usize, y1: usize| {
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                a[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[200, 200, 200, 255]);
            }
        }
    };
    let mut prev: Option<(usize, usize)> = None;
    for _ in 0..40 {
        let (rw, rh) = (30 + rnd(90), 30 + rnd(90));
        let (x, y) = (rnd(w - rw), rnd(h - rh));
        fill(&mut a, x, y, x + rw, y + rh);
        let c = (x + rw / 2, y + rh / 2);
        if let Some(p) = prev {
            fill(&mut a, p.0.min(c.0), p.1, p.0.max(c.0) + 12, p.1 + 12);
            fill(&mut a, c.0, p.1.min(c.1), c.0 + 12, p.1.max(c.1) + 12);
        }
        prev = Some(c);
    }
    a
}

/// The minimap the game would draw around art spot p at `s` screen px per art px (dark ground, grey floor).
fn minimap(art: &[u8], w: usize, h: usize, p: (f64, f64), s: f64, n: usize) -> Mat {
    let mut m = Mat::new(n, n, 3);
    m.data.fill(22);
    for y in 0..n {
        for x in 0..n {
            let ax = (p.0 + (x as f64 + 0.5 - n as f64 / 2.0) / s) as i64;
            let ay = (p.1 + (y as f64 + 0.5 - n as f64 / 2.0) / s) as i64;
            if ax >= 0 && ay >= 0 && (ax as usize) < w && (ay as usize) < h && art[(ay as usize * w + ax as usize) * 4 + 3] > 0 {
                m.data[(y * n + x) * 3..(y * n + x) * 3 + 3].fill(200);
            }
        }
    }
    m
}

#[test]
fn minimap_tracker_finds_and_follows_the_player() {
    let (w, h) = (900, 700);
    let dir = std::env::temp_dir().join(format!("alogs-tracker-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let art = floor_plan(w, h);
    {
        let mut e = png::Encoder::new(std::fs::File::create(dir.join("floor.png")).unwrap(), w as u32, h as u32);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&art).unwrap();
    }
    let json = format!(r#"{{"layouts":[{{"layout":7,"kind":"sanctum","image":"floor.png","imageSize":[{w},{h}]}}]}}"#);
    std::fs::write(dir.join("layouts.json"), json).unwrap();
    let db = Arc::new(LayoutDb::load(&dir).unwrap());
    let layout = db.layouts[0].clone();
    let mut t = MinimapTracker::new(Arc::new(MapArtCache::new(db)));
    // a spot on the floor, away from the edges
    let on_floor = |x: usize, y: usize| art[(y * w + x) * 4 + 3] > 0;
    let p = (300..600).flat_map(|y| (300..600).map(move |x| (x, y))).find(|&(x, y)| on_floor(x, y)).unwrap();
    let p = (p.0 as f64, p.1 as f64);
    let got = t.locate(&minimap(&art, w, h, p, 0.6, 240), &layout, 1.0);
    let got = got.unwrap_or_else(|| panic!("not found: {}", t.last_info));
    assert!((got.0 - p.0).abs() + (got.1 - p.1).abs() < 6.0, "found {got:?}, was at {p:?}: {}", t.last_info);
    // walk a little: tracked at full detail near the last fix
    let q = (p.0 + 8.0, p.1 - 5.0);
    let got = t.locate(&minimap(&art, w, h, q, 0.6, 240), &layout, 1.0).unwrap_or_else(|| panic!("lost: {}", t.last_info));
    std::fs::remove_dir_all(&dir).ok();
    assert!((got.0 - q.0).abs() + (got.1 - q.1).abs() < 8.0, "followed to {got:?}, was at {q:?}: {}", t.last_info);
}
