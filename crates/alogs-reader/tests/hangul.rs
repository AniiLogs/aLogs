//! Parity with aLogs B16 `hangulnames.py`'s font-free helpers (tools/gen_hangul_goldens.py).

use alogs_pil::Image;
use alogs_reader::hangul;
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

fn letters_json(l: &Option<hangul::Letters>) -> J {
    match l {
        None => J::Null,
        Some(l) => J::Array(l.iter().map(|(x, p)| json!([x, p.iter().map(|(c, q)| json!([c, q])).collect::<Vec<_>>()])).collect()),
    }
}

#[test]
fn helpers_match_reference() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let g: J = serde_json::from_str(&std::fs::read_to_string(dir.join("hangul_goldens.json")).unwrap()).unwrap();
    for b in g["boxes"].as_array().unwrap() {
        let im = Image::open_png(&dir.join("names").join(b["file"].as_str().unwrap())).unwrap();
        let t = hangul::tidy(&im, b["pad"].as_u64().unwrap() as usize);
        assert_eq!(digest(t.as_ref()), b["tidy"], "tidy {}", b["file"]);
        assert_eq!(json!(hangul::ink_pieces(t.as_ref())), b["pieces"], "ink_pieces {}", b["file"]);
        assert_eq!(json!(hangul::ink_pieces(Some(&im))), b["pieces_raw"], "ink_pieces raw {}", b["file"]);
    }
    for r in g["replies"].as_array().unwrap() {
        let (t, l, latin) = hangul::split_reply(r[0].as_str().unwrap());
        let lj = letters_json(&l);
        let same = |a: &J, b: &J| a.to_string().replace(".0,", ",").replace(".0]", "]") == b.to_string().replace(".0,", ",").replace(".0]", "]");
        assert!(same(&json!([t, lj, latin]), &r[1]), "split_reply {}: {} vs {}", r[0], json!([t, lj, latin]), r[1]);
        assert_eq!(json!(hangul::hangul_first(&l)), r[2], "hangul_first {}", r[0]);
        assert_eq!(json!(hangul::lone_latin(&l)), r[3], "lone_latin {}", r[0]);
        assert_eq!(json!(hangul::looks_hangul(&l)), r[4], "looks_hangul {}", r[0]);
    }
    for c in g["clean"].as_array().unwrap() {
        assert_eq!(json!(hangul::clean_latin(c[0].as_str().unwrap())), c[1], "clean_latin {}", c[0]);
    }
}
