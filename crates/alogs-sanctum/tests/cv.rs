//! cv.rs against OpenCV 4.10 (the OpenCvSharp build Sanctum Helper ships), from tools/cv-goldens: 8-bit results byte
//! for byte, float results within a small tolerance.

use alogs_sanctum::cv::{self, Interp, Mat, MatF, Thresh};
use serde_json::Value as J;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cv")
}

fn load(d: &J) -> Result<Mat, MatF> {
    let bytes = std::fs::read(dir().join(d["file"].as_str().unwrap())).unwrap();
    let (w, h, c) = (d["w"].as_u64().unwrap() as usize, d["h"].as_u64().unwrap() as usize, d["c"].as_u64().unwrap() as usize);
    if d["f32"].as_bool().unwrap() {
        Err(MatF { w, h, c, data: bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect() })
    } else {
        Ok(Mat { w, h, c, data: bytes })
    }
}

fn u8m(d: &J) -> Mat {
    load(d).unwrap_or_else(|_| panic!("expected 8-bit"))
}

fn f32m(d: &J) -> MatF {
    load(d).err().expect("expected float")
}

/// Bytes that differ, and the largest difference.
fn diff(a: &Mat, b: &Mat) -> (usize, i32) {
    assert_eq!((a.w, a.h, a.c), (b.w, b.h, b.c), "sizes");
    let n = a.data.iter().zip(&b.data).filter(|(x, y)| x != y).count();
    let m = a.data.iter().zip(&b.data).map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs()).max().unwrap_or(0);
    (n, m)
}

fn fdiff(a: &MatF, b: &MatF) -> f32 {
    assert_eq!((a.w, a.h), (b.w, b.h), "sizes");
    a.data.iter().zip(&b.data).filter(|(x, y)| !(x.is_nan() && y.is_nan())).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

fn cases(op: &str) -> Vec<J> {
    let idx: J = serde_json::from_str(&std::fs::read_to_string(dir().join("index.json")).unwrap()).unwrap();
    idx.as_array().unwrap().iter().filter(|c| c["op"] == op).cloned().collect()
}

#[test]
fn resize_matches_opencv() {
    let mut bad = vec![];
    for c in cases("resize") {
        let src = u8m(&c["in"]);
        let want = u8m(&c["out"]);
        let interp = match c["interp"].as_str().unwrap() {
            "area" => Interp::Area,
            "linear" => Interp::Linear,
            "nearest" => Interp::Nearest,
            _ => Interp::Cubic,
        };
        let got = match c["fx"].as_f64() {
            Some(f) => cv::resize(&src, 0, 0, f, f, interp),
            None => cv::resize(&src, c["w"].as_u64().unwrap() as usize, c["h"].as_u64().unwrap() as usize, 0.0, 0.0, interp),
        };
        if (got.w, got.h) != (want.w, want.h) {
            bad.push(format!("{} c{} {:?}: size {}x{} want {}x{}", c["interp"], src.c, c.get("fx"), got.w, got.h, want.w, want.h));
            continue;
        }
        let (n, m) = diff(&got, &want);
        // cubic (only used to enlarge text for OCR): at most 1 off in under 0.5% of bytes
        let cubic_ok = interp == Interp::Cubic && m <= 1 && n * 200 < want.data.len();
        if n > 0 && !cubic_ok {
            bad.push(format!("{} c{} fx {:?} {}x{}: {n} bytes differ (max {m})", c["interp"], src.c, c["fx"].as_f64(), want.w, want.h));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn colour_and_thresholds_match_opencv() {
    for c in cases("cvt") {
        let src = u8m(&c["in"]);
        let got = match c["code"].as_str().unwrap() {
            "BGR2GRAY" | "BGRA2GRAY" => cv::bgr2gray(&src),
            "BGR2HSV" => cv::bgr2hsv(&src),
            _ => cv::bgra2bgr(&src),
        };
        assert_eq!(diff(&got, &u8m(&c["out"])), (0, 0), "{}", c["code"]);
    }
    for c in cases("inrange") {
        let lo: Vec<u8> = c["lo"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap().min(255) as u8).collect();
        let hi: Vec<u8> = c["hi"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap().min(255) as u8).collect();
        assert_eq!(diff(&cv::in_range(&u8m(&c["in"]), &lo, &hi), &u8m(&c["out"])), (0, 0));
    }
    for c in cases("threshold") {
        let ty = c["type"].as_str().unwrap();
        match load(&c["in"]) {
            Ok(src) => {
                let t = if ty.contains("Otsu") {
                    let t = cv::otsu(&src);
                    assert_eq!(t, c["got"].as_f64().unwrap(), "otsu");
                    t
                } else {
                    c["t"].as_f64().unwrap()
                };
                let kind = if ty.contains("BinaryInv") { Thresh::BinaryInv } else { Thresh::Binary };
                assert_eq!(diff(&cv::threshold(&src, t, c["max"].as_f64().unwrap(), kind), &u8m(&c["out"])), (0, 0), "{ty}");
            }
            Err(f) => {
                let got = cv::threshold_f(&f, c["t"].as_f64().unwrap() as f32, c["max"].as_f64().unwrap() as f32, Thresh::TozeroInv);
                assert_eq!(fdiff(&got, &f32m(&c["out"])), 0.0);
            }
        }
    }
}

use alogs_sanctum::cv2::{self, Kernel};

#[test]
fn morphology_and_components_match_opencv() {
    for c in cases("dilate") {
        let k = c["k"].as_u64().unwrap() as usize;
        assert_eq!(diff(&cv2::dilate(&u8m(&c["in"]), &Kernel::rect(k, k)), &u8m(&c["out"])), (0, 0), "dilate {k}");
    }
    let e = Kernel::ellipse(5, 5);
    for c in cases("morph") {
        let src = u8m(&c["in"]);
        let got = if c["type"] == "Open" { cv2::open(&src, &e) } else { cv2::gradient(&src, &e) };
        assert_eq!(diff(&got, &u8m(&c["out"])), (0, 0), "{}", c["type"]);
    }
    for c in cases("components") {
        let (_, comps) = cv2::components(&u8m(&c["in"]));
        assert_eq!(comps.len() as u64, c["n"].as_u64().unwrap());
        for (i, s) in c["stats"].as_array().unwrap().iter().enumerate() {
            let s: Vec<u64> = s.as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
            let g = &comps[i];
            assert_eq!(vec![g.left as u64, g.top as u64, g.width as u64, g.height as u64, g.area as u64], s, "component {i}");
            let cc = &c["centroids"][i];
            assert!((g.cx - cc[0].as_f64().unwrap()).abs() < 1e-9 && (g.cy - cc[1].as_f64().unwrap()).abs() < 1e-9, "centroid {i}");
        }
    }
}

#[test]
fn filters_matching_and_warps_match_opencv() {
    for c in cases("blur") {
        let d = fdiff(&cv2::blur(&f32m(&c["in"]), c["k"].as_u64().unwrap() as usize), &f32m(&c["out"]));
        assert!(d < 1e-5, "blur {}: {d}", c["k"]);
    }
    for c in cases("hanning") {
        let d = fdiff(&cv2::hanning(64, 48), &f32m(&c["out"]));
        assert!(d < 1e-6, "hanning {d}");
    }
    for c in cases("match") {
        let (img, tpl) = (u8m(&c["in"]), u8m(&c["tpl"]));
        let got = if c.get("mask").is_some() { cv2::match_template_masked(&img, &tpl, &u8m(&c["mask"])) } else { cv2::match_template(&img, &tpl) };
        let want = f32m(&c["out"]);
        let d = fdiff(&got, &want);
        assert!(d < 1e-3, "match c{} mask {}: {d}", img.c, c.get("mask").is_some());
        assert_eq!(got.min_max_loc().3, want.min_max_loc().3, "best spot");
    }
    for c in cases("clahe") {
        let (n, m) = diff(&cv2::clahe(&u8m(&c["in"])), &u8m(&c["out"]));
        assert_eq!((n, m), (0, 0), "clahe");
    }
    for c in cases("phase") {
        let (x, y, r) = cv2::phase_correlate(&f32m(&c["a"]), &f32m(&c["b"]), Some(&f32m(&c["win"])));
        let (wx, wy, wr) = (c["x"].as_f64().unwrap(), c["y"].as_f64().unwrap(), c["response"].as_f64().unwrap());
        assert!((x - wx).abs() < 1e-3 && (y - wy).abs() < 1e-3 && (r - wr).abs() < 1e-3, "phase ({x}, {y}, {r}) vs ({wx}, {wy}, {wr})");
    }
    for c in cases("warp") {
        let m: Vec<f64> = c["M"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let src = u8m(&c["in"]);
        let got = cv2::warp_affine(&src, [m[0], m[1], m[2], m[3], m[4], m[5]], src.w, src.h, c["interp"] == "linear");
        assert_eq!(diff(&got, &u8m(&c["out"])), (0, 0), "warp {}", c["interp"]);
    }
}
