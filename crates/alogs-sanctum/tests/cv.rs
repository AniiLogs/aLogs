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
