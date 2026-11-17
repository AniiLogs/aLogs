//! Parity with aLogs B16 `namereader.py` and `icons.py`'s font-free parts (tools/gen_names_goldens.py).
//! `ink` comes from quantize, a documented difference: it's compared, but a mismatch there only skips ink_mask.

use alogs_pil::{Image, Mode};
use alogs_reader::{icons, names};
use serde_json::{Value as J, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn digest(im: Option<&Image>) -> J {
    let Some(im) = im else { return J::Null };
    let mut h = Sha256::new();
    h.update(format!("{} {} {} ", im.mode.name(), im.width, im.height).as_bytes());
    h.update(im.tobytes());
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    json!([im.mode.name(), im.width, im.height, &hex[..20]])
}

#[test]
fn matchers_match_reference() {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let g: Vec<J> = serde_json::from_str(&std::fs::read_to_string(d.join("names_goldens.json")).unwrap()).unwrap();
    let (mut ink_same, mut ink_total) = (0, 0);
    for c in &g {
        let f = c["file"].as_str().unwrap();
        let im = Image::open_png(&d.join("frames").join(f)).unwrap().convert(Mode::Rgb);
        let b: Vec<f64> = c["box"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let bx = (b[0], b[1], b[2], b[3]);
        let m = names::text_mask(&im, bx, 190, 50);
        assert_eq!(digest(m.as_ref()), c["text_mask"], "text_mask {f} {bx:?}");
        if let Some(m) = &m {
            let ca: Vec<J> = names::characters_at(m).iter().map(|x| json!([x.0, x.1, x.2])).collect();
            assert_eq!(J::Array(ca), c["characters_at"], "characters_at {f}");
            let parts = names::characters(m);
            assert_eq!(J::Array(parts.iter().map(|p| digest(Some(p))).collect()), c["characters"], "characters {f}");
            assert_eq!(digest(Some(&names::drop_icon(m))), c["drop_icon"], "drop_icon {f}");
            if parts.len() >= 2 {
                let o = [names::overlap(&parts[0], &parts[1], 2), names::overlap(&parts[0], &parts[0], 1)];
                assert_eq!(json!(o), c["overlap"], "overlap {f}");
            }
        }
        let a = im.crop(bx.0, bx.1, bx.2, bx.3);
        let bb = im.crop(bx.0 + 3.0, bx.1 + 1.0, bx.2 + 3.0, bx.3 + 1.0);
        let mask = a.convert(Mode::L).point(|v| if v > 60 { 255.0 } else { 0.0 });
        let n = [icons::ncc(&a, &bb, &mask), icons::ncc_pooled(&a, &bb, &mask)];
        let want: Vec<f64> = c["ncc"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        for (x, y) in n.iter().zip(&want) {
            assert!((x - y).abs() <= 1e-12 * y.abs().max(1.0), "ncc {f}: {n:?} vs {want:?}");
        }
        ink_total += 1;
        let (ink, bg) = names::ink_colour(&im, bx, 80, false);
        let _ = bg;
        if ink.map(|i| json!(i)).unwrap_or(J::Null) == c["ink"] {
            ink_same += 1;
        }
    }
    eprintln!("ink_colour (quantize, a documented difference): {ink_same} of {ink_total} the same");
}
