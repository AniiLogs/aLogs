//! The overlay draws each style of map for a real layout (from reference/, skipped without it). Set
//! OVERLAY_SHOTS=<folder> to keep the pictures for a look (never in the repo: they hold the game's map art).

use alogs_sanctum::engine::OverlayFrame;
use alogs_sanctum::layout_db::LayoutDb;
use alogs_sanctum::overlay::canvas::Canvas;
use alogs_sanctum::overlay::view::OverlayView;
use alogs_sanctum::settings::Settings;
use alogs_sanctum::vision::MapPose;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn every_style_draws() {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/SanctumHelper/app/SanctumHelper/Assets");
    if !assets.exists() {
        eprintln!("skipped: reference/ not cloned");
        return;
    }
    let db = Arc::new(LayoutDb::load(&assets).unwrap());
    let l = db.by_number(53).unwrap().clone();
    let me = l.orange().map(|o| (o.img[0] + 60.0, o.img[1] + 40.0)).unwrap();
    let keep = std::env::var_os("OVERLAY_SHOTS").map(PathBuf::from);
    let mut view = OverlayView::new(db.clone());
    let client = (0, 0, 1920, 1080);
    let mut cases: Vec<(String, Settings, OverlayFrame)> = vec![];
    for style in ["Corner", "Disc", "Radar", "Compass", "Overlay"] {
        let mut s = Settings::default();
        s.mini_map_mode = style.into();
        let f = OverlayFrame { client, visible: true, layout: Some(53), player: Some(me), player_fresh: true, mini: true, heading: Some(40.0), camera: Some(60.0), difficulty: Some("Hard".into()), ..Default::default() };
        cases.push((style.into(), s, f));
    }
    let mut s = Settings::default();
    s.show_labels = true;
    let big = OverlayFrame { client, visible: true, layout: Some(53), player: Some(me), mini: true, big: true, heading: Some(200.0), ..Default::default() };
    cases.push(("Big".into(), s.clone(), big));
    let full = OverlayFrame { client, visible: true, layout: Some(53), player: Some(me), pose: Some(MapPose { layout: 53, scale: 0.6, tx: 300.0, ty: 60.0, inliers: 100, rotation: 0.0 }), heading: Some(90.0), difficulty: Some("Chaos".into()), ..Default::default() };
    cases.push(("Full".into(), s.clone(), full));
    let unaligned = OverlayFrame { client, visible: true, layout: Some(53), status: "x".into(), ..Default::default() };
    cases.push(("Unaligned".into(), s, unaligned));
    // the corner map at its window size (the panel plus room for labels), art already decoded: one display frame
    {
        let (s, f) = (&cases[0].1, &cases[0].2);
        let mut cv = Canvas::new(1920, 1080).unwrap();
        view.render(&mut cv, s, f, None, None, None);
        view.offset = (1300.0, 30.0);
        let t = std::time::Instant::now();
        for _ in 0..20 {
            let mut cv = Canvas::new(450, 450).unwrap();
            view.render(&mut cv, s, f, None, None, None);
            let _ = cv.finish();
        }
        eprintln!("corner map frame: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
        if std::env::var_os("OVERLAY_PROFILE").is_some() {
            let mut bare = s.clone();
            for k in ["doors", "key", "challenge", "pile", "pot", "mound", "simple", "exquisite", "luxurious", "legendary", "nest"] {
                bare.layers.insert(k.into(), false);
            }
            let t = std::time::Instant::now();
            for _ in 0..20 {
                let mut cv = Canvas::new(450, 450).unwrap();
                view.render(&mut cv, &bare, f, None, None, None);
                let _ = cv.finish();
            }
            eprintln!("  without markers: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
            let t = std::time::Instant::now();
            for _ in 0..20 {
                let mut cv = Canvas::new(450, 450).unwrap();
                cv.push_opacity(0.8);
                cv.pop_layer();
                let _ = cv.finish();
            }
            eprintln!("  empty canvas + one layer: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
        }
        view.offset = (0.0, 0.0);
    }
    for (name, s, f) in cases {
        let mut cv = Canvas::new(1920, 1080).unwrap();
        view.render(&mut cv, &s, &f, None, None, None);
        let pix = cv.finish();
        let drawn = pix.data().chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(drawn > 2000, "{name}: only {drawn} pixels drawn");
        if let Some(dir) = &keep {
            std::fs::create_dir_all(dir).unwrap();
            pix.save_png(dir.join(format!("{name}.png"))).unwrap();
        }
    }
}
