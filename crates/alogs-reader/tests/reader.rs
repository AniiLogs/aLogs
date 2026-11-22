//! Parity with aLogs B16 `reader.py` and `xn_digits.py` on synthetic boss-bar frames (tools/gen_reader_goldens.py) and
//! made-up OCR text. The xN templates are read from reference/ (skipped when it isn't cloned).

use alogs_pil::{Image, Mode};
use alogs_reader::bar::{self, Bar, Spot};
use alogs_reader::{boss, parse, prep, xn};
use serde_json::{Value as J, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}
fn res() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/aLogs/bossbar")
}
fn goldens() -> J {
    serde_json::from_str(&std::fs::read_to_string(data().join("reader_goldens.json")).unwrap()).unwrap()
}

fn digest(im: &Image) -> J {
    let mut h = Sha256::new();
    h.update(format!("{} {} {} ", im.mode.name(), im.width, im.height).as_bytes());
    h.update(im.tobytes());
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    json!([im.mode.name(), im.width, im.height, &hex[..20]])
}
fn hash(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|b| format!("{b:02x}")).collect::<String>()[..20].to_string()
}

/// A float the Python wrote as an int (a whole number) compares equal.
fn num(v: f64) -> J {
    if v.fract() == 0.0 && v.abs() < 1e15 { json!(v as i64) } else { json!(v) }
}

fn bar_json(b: &Bar) -> J {
    json!({"yc": b.yc, "left_bracket": num(b.left_bracket), "span": num(b.span), "brackets_found": b.brackets_found,
        "world": b.world, "right_red": num(b.right_red), "track_left": b.track_left, "track_right": b.track_right,
        "thickness": b.thickness, "frame": [b.frame.0, b.frame.1]})
}

fn same_json(a: &J, b: &J) -> bool {
    match (a, b) {
        (J::Number(x), J::Number(y)) => x.as_f64() == y.as_f64(),
        (J::Array(x), J::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_json(p, q)),
        (J::Object(x), J::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same_json(v, w))),
        _ => a == b,
    }
}

fn load(name: &str) -> Image {
    let im = Image::open_png(&data().join("frames").join(name)).unwrap();
    if im.mode == Mode::Rgb { im } else { im.convert(Mode::Rgb) }
}

#[test]
fn frames_match_reference() {
    let g = goldens();
    let tpl = xn::load_templates(&res());
    let mut fails = Vec::new();
    for c in g["frames"].as_array().unwrap() {
        let name = c["file"].as_str().unwrap();
        let im = load(name);
        let mut check = |what: &str, got: J, want: &J| {
            if !same_json(&got, want) {
                fails.push(format!("{name} {what}: got {got}, reference {want}"));
            }
        };
        let found = bar::find_bar(&im, false, 8);
        check("find_bar", found.as_ref().map_or(J::Null, bar_json), &c["find_bar"]);
        let fail = bar::LAST_FAIL.lock().unwrap().clone();
        // the rules from before 22 Nov (a Holo-Battle Sim room) give the reference's answers too
        let old = bar::find_bar_old(&im, false, 8);
        check("find_bar_old", old.as_ref().map_or(J::Null, bar_json), &c["find_bar"]);
        if let Some(b) = &old {
            let spot = Spot { frame: b.frame, yc: b.yc as f64, thickness: b.thickness as f64, left_bracket: b.left_bracket, span: b.span, track_left: b.track_left as f64 };
            check("bar_at_old", bar::bar_at_old(&im, &[spot]).as_ref().map_or(J::Null, bar_json), &c["bar_at"]);
            check("verify_old", json!(bar::verify_bar_old(&im, b)), &c["verify"]);
        }
        let Some(b) = found else {
            check("last_fail", json!(fail), &c["last_fail"]);
            continue;
        };
        let spot = Spot { frame: b.frame, yc: b.yc as f64, thickness: b.thickness as f64, left_bracket: b.left_bracket, span: b.span, track_left: b.track_left as f64 };
        check("bar_at", bar::bar_at(&im, &[spot]).as_ref().map_or(J::Null, bar_json), &c["bar_at"]);
        let (f, fl, gh) = bar::measure_fill(&im, &b);
        check("fill", json!([num(f), fl, gh]), &c["fill"]);
        let (fa, col, edge) = bar::measure_fill_any(&im, &b, 70);
        check("fill_any", json!([num(fa), col, edge]), &c["fill_any"]);
        let (bf, kind, broken) = bar::measure_break(&im, &b);
        check("break", json!([bf.map(num), kind, broken]), &c["break"]);
        check("verify", json!(bar::verify_bar(&im, &b)), &c["verify"]);
        check("bossrush_bar", bar_json(&bar::as_bossrush_bar(&b)), &c["bossrush_bar"]);
        check("world_bar", bar_json(&bar::as_world_bar(&b)), &c["world_bar"]);
        let scale = b.span / bar::REF_SPAN;
        let tb = bar::box_at(&b, bar::TITLE_BOX);
        let xb = bar::box_at(&b, bar::XN_BOX);
        let (w, h) = (im.width as f64, im.height as f64);
        let bx = |t: (f64, f64, f64, f64)| json!([num(t.0), num(t.1), num(t.2), num(t.3)]);
        check("boxes", json!([bx(tb), bx(xb), bx(bar::timer_box(w, h, false)), bx(bar::timer_box(w, h, true))]), &c["boxes"]);
        check("prep_text", digest(&prep::prep_text(&im, tb, scale, 190, 3.0, 24)), &c["prep_text"]);
        check("has_red", json!(prep::has_red(&im, tb, scale)), &c["has_red"]);
        check("prep_red", digest(&prep::prep_red(&im, tb, scale, 190, 3.0, 24)), &c["prep_red"]);
        check("prep_short", digest(&prep::prep_short(&im, xb, scale, 3)), &c["prep_short"]);
        check("prep_white", digest(&prep::prep_white(&im, xb, scale, 4.0)), &c["prep_white"]);
        check("prep_grey", digest(&prep::prep_grey(&im, tb, scale, 3.0, 24)), &c["prep_grey"]);
        check("prep_bright", digest(&prep::prep_bright(&im, tb, scale, 2.0)), &c["prep_bright"]);
        check("grey_crop", digest(&prep::grey_crop(&im, tb, 48)), &c["grey_crop"]);
        check("ink_aspect", prep::ink_aspect(&im, xb, 200).map_or(J::Null, num), &c["ink_aspect"]);
        check("prep_pill", prep::prep_pill(&im, tb, scale, 3.0, 24).map_or(J::Null, |i| digest(&i)), &c["prep_pill"]);
        let sig = prep::name_signature(&im.crop_i(tb.0 as i64, tb.1 as i64, tb.2 as i64, tb.3 as i64));
        check("name_signature", json!([[sig.0.0, sig.0.1], hash(&sig.1)]), &c["name_signature"]);
        let crop = im.crop_i(xb.0 as i64, xb.1 as i64, xb.2 as i64, xb.3 as i64);
        check("xn_mask", json!(hash(&xn::mask_bytes(&crop).2)), &c["xn_mask"]);
        let m = xn::white_mask(&crop);
        let gl: Vec<J> = xn::split_touching(&m, &xn::glyphs(&m)).iter().map(|g| json!([g.0, g.1, g.2, g.3])).collect();
        check("xn_glyphs", J::Array(gl), &c["xn_glyphs"]);
        if !tpl.is_empty() {
            check("xn_read", json!(xn::read_counter(&crop, &tpl, 0.22, 21)), &c["xn_read"]);
        }
    }
    assert!(fails.is_empty(), "{} differences:\n{}", fails.len(), fails[..fails.len().min(15)].join("\n"));
}

#[test]
fn parsing_matches_reference() {
    let g = goldens();
    let p = &g["parse"];
    for c in p["title"].as_array().unwrap() {
        let (l, t) = parse::parse_title(c[0].as_str());
        assert_eq!(json!([l, t]), c[1], "parse_title {}", c[0]);
    }
    for c in p["pair"].as_array().unwrap() {
        let (l, t) = parse::parse_title_pair(c[0].as_str(), c[1].as_str());
        assert_eq!(json!([l, t]), c[2], "parse_title_pair {c}");
    }
    for c in p["xn"].as_array().unwrap() {
        assert_eq!(json!(parse::parse_xn(c[0].as_str(), c[1].as_i64().unwrap())), c[2], "parse_xn {} top {}", c[0], c[1]);
    }
    for c in p["timer"].as_array().unwrap() {
        assert_eq!(json!(parse::parse_timer(c[0].as_str())), c[1], "parse_timer {}", c[0]);
    }
    for c in p["norm"].as_array().unwrap() {
        assert_eq!(json!(parse::norm(c[0].as_str())), c[1], "norm {}", c[0]);
    }
    for c in p["after_label"].as_array().unwrap() {
        assert_eq!(json!(prep::after_label(c[0].as_str(), c[1].as_str().unwrap())), c[2], "after_label {c}");
    }
}

#[test]
fn damage_matches_reference() {
    let g = goldens();
    let cum = [0.0, 0.1, 0.3, 0.6, 1.0];
    let cases = g["parse"]["damage"].as_array().unwrap();
    let mut i = 0;
    for m in [1000.0, 333333.0] {
        for f in [0.0, 0.42, 1.0] {
            for x in [None, Some(1), Some(2), Some(4), Some(5)] {
                for br in [true, false] {
                    let (d, info) = boss::damage_dealt(m, f, x, &cum, br);
                    let got = json!([d, info.map(|g| json!({"grade": g.grade, "size_hp": g.size_hp, "left_hp": g.left_hp}))]);
                    assert_eq!(got, cases[i][3], "damage_dealt({m}, {f}, {x:?}, {br})");
                    i += 1;
                }
            }
        }
    }
}
