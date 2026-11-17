//! Each operation against Pillow 12.3: tools/gen_pil_goldens.py ran Pillow on images made from a seed (make(), the
//! same here) and kept a hash of each result's mode, size and bytes.

use alogs_pil::{Filter, Image, Mode, Transpose};
use serde_json::{Value as J, json};
use sha2::{Digest, Sha256};

const M64: u128 = 1 << 64;

fn make(spec: &J) -> Image {
    let mode = Mode::parse(spec[0].as_str().unwrap()).unwrap();
    let (w, h) = (spec[1].as_u64().unwrap() as usize, spec[2].as_u64().unwrap() as usize);
    let (seed, kind) = (spec[3].as_u64().unwrap() as u128, spec[4].as_u64().unwrap());
    let mut s: u128 = (seed * 2654435761 + 1) % M64;
    let mut nxt = || {
        s = (s * 6364136223846793005 + 1442695040888963407) % M64;
        (s >> 56) as u32
    };
    let bands = mode.bands();
    let mut out = Vec::with_capacity(w * h * bands);
    for y in 0..h {
        for x in 0..w {
            for b in 0..bands {
                let r = nxt();
                let v = match kind {
                    0 => r,
                    1 => (x as u32 * 7 + y as u32 * 13 + b as u32 * 40 + (r >> 4)) & 255,
                    2 => {
                        if r < 40 {
                            nxt()
                        } else {
                            0
                        }
                    }
                    _ => {
                        if (x / 3 + y / 2 + b) % 3 == 0 || r > 230 {
                            255
                        } else {
                            0
                        }
                    }
                };
                out.push(v as u8);
            }
        }
    }
    if mode == Mode::One {
        for v in out.iter_mut() {
            *v = if *v >= 128 { 255 } else { 0 };
        }
    }
    Image::from_raw(mode, w, h, out)
}

fn digest(im: &Image) -> J {
    let mut hsh = Sha256::new();
    hsh.update(format!("{} {} {} ", im.mode.name(), im.width, im.height).as_bytes());
    hsh.update(im.tobytes());
    let hex: String = hsh.finalize().iter().map(|b| format!("{b:02x}")).collect();
    json!([im.mode.name(), im.width, im.height, &hex[..20]])
}

fn filter(name: &str) -> Filter {
    match name {
        "nearest" => Filter::Nearest,
        "box" => Filter::Box,
        "bilinear" => Filter::Bilinear,
        "hamming" => Filter::Hamming,
        "bicubic" => Filter::Bicubic,
        "lanczos" => Filter::Lanczos,
        _ => panic!("{name}"),
    }
}

fn f(a: &J, k: &str) -> f64 {
    a[k].as_f64().unwrap()
}
fn u(a: &J, k: &str) -> usize {
    a[k].as_u64().unwrap() as usize
}

enum Out {
    Image(Image),
    Value(J),
    Error,
}

fn run(op: &str, a: &J, ims: &[Image]) -> Out {
    let im = ims.first();
    let img = |i: Image| Out::Image(i);
    match op {
        "convert" => img(im.unwrap().convert(Mode::parse(a["mode"].as_str().unwrap()).unwrap())),
        "point_threshold" => {
            let t = a["t"].as_u64().unwrap() as u8;
            img(im.unwrap().point(|v| if v > t { 255.0 } else { 0.0 }))
        }
        "point_scale" => img(im.unwrap().point(|v| f64::from(v) * 1.7 - 20.0)),
        "point_to_1" => {
            let t = a["t"].as_u64().unwrap() as u8;
            img(im.unwrap().point_to_1(|v| if v > t { 255.0 } else { 0.0 }))
        }
        "crop" => {
            let b: Vec<f64> = a["box"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            let r = |v: f64| v.round_ties_even();
            if r(b[2]) < r(b[0]) || r(b[3]) < r(b[1]) {
                return Out::Error;
            }
            img(im.unwrap().crop(b[0], b[1], b[2], b[3]))
        }
        "resize" => img(im.unwrap().resize(u(a, "w"), u(a, "h"), Some(filter(a["filter"].as_str().unwrap())))),
        "resize_default" => img(im.unwrap().resize(u(a, "w"), u(a, "h"), None)),
        "resize_box" => {
            let b: Vec<f64> = a["box"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            match im.unwrap().try_resize_full(u(a, "w"), u(a, "h"), Some(filter(a["filter"].as_str().unwrap())), Some([b[0], b[1], b[2], b[3]]), None) {
                Ok(i) => img(i),
                Err(_) => Out::Error,
            }
        }
        "resize_gap" => img(im.unwrap().resize_full(u(a, "w"), u(a, "h"), Some(filter(a["filter"].as_str().unwrap())), None, Some(f(a, "gap")))),
        "reduce" => img(im.unwrap().reduce(u(a, "f"))),
        "reduce_xy" => img(im.unwrap().reduce_box(u(a, "fx"), u(a, "fy"), Some((3, 2, 31, 27)))),
        "thumbnail" => img(im.unwrap().thumbnail(u(a, "w"), u(a, "h"))),
        "affine" => {
            let v: Vec<f64> = a["a"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            img(im.unwrap().transform_affine(u(a, "w"), u(a, "h"), [v[0], v[1], v[2], v[3], v[4], v[5]], filter(a["filter"].as_str().unwrap()), &[]))
        }
        "rotate" => img(im.unwrap().rotate(f(a, "angle"), filter(a["filter"].as_str().unwrap()), a["expand"].as_bool().unwrap(), &[0])),
        "transpose" => {
            let t = match a["t"].as_str().unwrap() {
                "FLIP_LEFT_RIGHT" => Transpose::FlipLeftRight,
                "FLIP_TOP_BOTTOM" => Transpose::FlipTopBottom,
                "ROTATE_90" => Transpose::Rotate90,
                "ROTATE_180" => Transpose::Rotate180,
                "ROTATE_270" => Transpose::Rotate270,
                "TRANSPOSE" => Transpose::Transpose,
                _ => Transpose::Transverse,
            };
            img(im.unwrap().transpose(t))
        }
        "getbbox" => Out::Value(im.unwrap().getbbox().map_or(J::Null, |b| json!([b.0, b.1, b.2, b.3]))),
        "histogram" => Out::Value(json!(im.unwrap().histogram(None))),
        "extrema" => {
            let e = im.unwrap().getextrema();
            Out::Value(if e.len() == 1 { json!([e[0].0, e[0].1]) } else { json!(e.iter().map(|x| [x.0, x.1]).collect::<Vec<_>>()) })
        }
        "split_merge" => {
            let i = im.unwrap();
            let mut s = i.split();
            if i.mode == Mode::Rgb {
                s.reverse();
            }
            img(Image::merge(i.mode, &s))
        }
        "paste" => {
            let mut base = ims[0].clone();
            base.paste_mask(&ims[1], a["x"].as_i64().unwrap(), a["y"].as_i64().unwrap(), ims.get(2));
            img(base)
        }
        "composite" => img(Image::composite(&ims[0], &ims[1], &ims[2])),
        "new" => img(Image::new(Mode::Rgba, 5, 3, &[1, 2, 3, 4])),
        "chop" => {
            use alogs_pil::ops::*;
            let fun: fn(&Image, &Image) -> Image = match a["name"].as_str().unwrap() {
                "lighter" => lighter,
                "darker" => darker,
                "difference" => difference,
                "multiply" => multiply,
                _ => screen,
            };
            img(fun(&ims[0], &ims[1]))
        }
        "chop_sub" => img(alogs_pil::ops::subtract(&ims[0], &ims[1], f(a, "scale") as f32, a["offset"].as_i64().unwrap() as i32)),
        "chop_add" => img(alogs_pil::ops::add(&ims[0], &ims[1], f(a, "scale") as f32, a["offset"].as_i64().unwrap() as i32)),
        "chop_and" => img(alogs_pil::ops::logical_and(&ims[0], &ims[1])),
        "chop_or" => img(alogs_pil::ops::logical_or(&ims[0], &ims[1])),
        "chop_invert" => img(alogs_pil::ops::chop_invert(&ims[0])),
        "ops_invert" => img(alogs_pil::ops::invert(&ims[0])),
        "autocontrast" => {
            let co = match &a["cutoff"] {
                J::Array(v) => (v[0].as_f64().unwrap(), v[1].as_f64().unwrap()),
                v => (v.as_f64().unwrap(), v.as_f64().unwrap()),
            };
            img(alogs_pil::ops::autocontrast(&ims[0], co))
        }
        "expand" => img(alogs_pil::ops::expand(&ims[0], u(a, "border"), &[a["fill"].as_u64().unwrap() as u8])),
        "maxfilter" => img(alogs_pil::ops::max_filter(&ims[0], u(a, "size"))),
        "minfilter" => img(alogs_pil::ops::min_filter(&ims[0], u(a, "size"))),
        "gaussian" => img(alogs_pil::ops::gaussian_blur(&ims[0], f(a, "radius"))),
        "stat" => {
            let st = alogs_pil::ops::Stat::new(&ims[0], ims.get(1));
            Out::Value(json!({"mean": st.mean(), "median": st.median(), "stddev": st.stddev(),
                "extrema": st.extrema().iter().map(|e| [e.0, e.1]).collect::<Vec<_>>(), "count": st.count()}))
        }
        "ellipse" | "rectangle" => {
            let xy: Vec<f64> = a["xy"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            let ink = [a["ink"].as_u64().unwrap() as u8];
            let mut im = ims[0].clone();
            let xy = (xy[0], xy[1], xy[2], xy[3]);
            if op == "rectangle" {
                alogs_pil::draw::rectangle(&mut im, xy, &ink);
            } else if a["width"].as_i64().unwrap() > 0 {
                alogs_pil::draw::ellipse(&mut im, xy, None, Some(&ink), a["width"].as_i64().unwrap() as i32);
            } else {
                alogs_pil::draw::ellipse(&mut im, xy, Some(&ink), None, 1);
            }
            img(im)
        }
        "floodfill" => {
            let mut im = ims[0].clone();
            alogs_pil::draw::floodfill(&mut im, (1, 1), &[a["ink"].as_u64().unwrap() as u8]);
            img(im)
        }

        _ => panic!("unknown op {op}"),
    }
}

#[test]
fn operations_match_pillow() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/pil_goldens.json");
    let cases: Vec<J> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut fails = Vec::new();
    let mut by_op = std::collections::BTreeMap::<String, (usize, usize)>::new();
    for c in &cases {
        let op = c["op"].as_str().unwrap();
        let ims: Vec<Image> = c["inputs"].as_array().unwrap().iter().map(make).collect();
        let got = run(op, &c["args"], &ims);
        let ok = match (&got, c.get("image"), c.get("value"), c.get("error")) {
            (Out::Image(i), Some(want), _, _) => digest(i) == *want,
            (Out::Value(v), _, Some(want), _) => v == want,
            (Out::Error, _, _, Some(_)) => true,
            _ => false,
        };
        let e = by_op.entry(op.to_string()).or_default();
        e.0 += 1;
        if !ok {
            e.1 += 1;
            if fails.len() < 12 {
                let g = match &got {
                    Out::Image(i) => digest(i),
                    Out::Value(v) => v.clone(),
                    Out::Error => json!("error"),
                };
                fails.push(format!("{op} {} {}: got {g}, Pillow {}", c["args"], c["inputs"], c.get("image").or(c.get("value")).or(c.get("error")).unwrap()));
            }
        }
    }
    let summary: Vec<String> = by_op.iter().filter(|(_, v)| v.1 > 0).map(|(k, v)| format!("{k}: {}/{} wrong", v.1, v.0)).collect();
    assert!(fails.is_empty(), "{}\n{}", summary.join("\n"), fails.join("\n"));
}
