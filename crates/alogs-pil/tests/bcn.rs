//! The bcn decoder against Pillow's (tools/gen_bcn_goldens.py) on made-up blocks.

use alogs_pil::Image;
use serde_json::Value as J;

fn stream(seed: u32) -> impl FnMut() -> u8 {
    let mut x = seed;
    move || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x as u8
    }
}

fn blocks(n: u8, seed: u32, w: usize, h: usize) -> Vec<u8> {
    let size = if n == 1 || n == 4 { 8 } else { 16 };
    let mut g = stream(seed);
    let mut data: Vec<u8> = (0..(w / 4) * (h / 4) * size).map(|_| g()).collect();
    if n == 7 {
        for (k, b) in data.chunks_mut(size).enumerate() {
            b[0] = if k % 9 == 8 { 0 } else { (b[0] | 1) << (k % 9) };
        }
    }
    data
}

#[test]
fn bcn_matches_pillow() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bcn_goldens.json");
    let cases: J = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut bad = vec![];
    for c in cases.as_array().unwrap() {
        let (n, seed) = (c["n"].as_u64().unwrap() as u8, c["seed"].as_u64().unwrap() as u32);
        let (w, h) = (c["size"][0].as_u64().unwrap() as usize, c["size"][1].as_u64().unwrap() as usize);
        let im = Image::from_bcn(n, w, h, &blocks(n, seed, w, h)).unwrap();
        let got = format!("{:x}", md5::compute(&im.data));
        if got != c["md5"] {
            bad.push(format!("bc{n} seed {seed} {w}x{h}"));
        }
    }
    assert!(bad.is_empty(), "differ from Pillow: {bad:?}");
}

#[test]
fn polygon_matches_pillow() {
    use alogs_pil::{Mode, draw};
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/polygon_goldens.json");
    let cases: J = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut bad = vec![];
    for (i, c) in cases.as_array().unwrap().iter().enumerate() {
        let (w, h) = (c["size"][0].as_u64().unwrap() as usize, c["size"][1].as_u64().unwrap() as usize);
        let mut im = Image::new(Mode::One, w, h, &[0]);
        for t in c["tris"].as_array().unwrap() {
            let pts: Vec<(f64, f64)> = t.as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
            draw::polygon(&mut im, &pts, &[1]);
        }
        if format!("{:x}", md5::compute(&im.convert(Mode::L).data)) != c["md5"] {
            bad.push(i);
        }
    }
    assert!(bad.is_empty(), "{} of {} differ from Pillow: {bad:?}", bad.len(), cases.as_array().unwrap().len());
}
