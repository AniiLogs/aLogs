//! gamefont field and render against the Python (tools/gen_font_goldens.py) on a synthetic font cache.

use alogs_reader::gamefont::{FONT_NAME, GameFont};
use serde_json::Value as J;
use std::path::PathBuf;

fn md5(data: &[u8]) -> String {
    format!("{:x}", md5::compute(data))
}

#[test]
fn field_and_render_match_python() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/font");
    let f = GameFont::open(&dir, FONT_NAME).expect("the synthetic font");
    let cases: J = serde_json::from_str(&std::fs::read_to_string(dir.join("font_goldens.json")).unwrap()).unwrap();
    let (mut ok, mut bad) = (0, vec![]);
    for c in cases.as_array().unwrap() {
        let (t, px) = (c["text"].as_str().unwrap(), c["px"].as_f64().unwrap());
        let (im, base) = if c["kind"] == "field" {
            let (im, b) = f.field(t, px);
            (im, J::from(b))
        } else {
            let lead = c["lead"].as_array().map(|l| (l[0].as_u64().unwrap() as usize, [0, 1, 2].map(|i| l[1][i].as_u64().unwrap() as u8)));
            let (im, b) = f.render(t, px, [255, 230, 120], [20, 26, 44], c["italic"].as_bool().unwrap(), c["bold"].as_bool().unwrap(), c["stroke"].as_bool().unwrap(), lead);
            (im, J::from(b))
        };
        let same = [im.width as u64, im.height as u64] == [c["size"][0].as_u64().unwrap(), c["size"][1].as_u64().unwrap()] && md5(&im.data) == c["md5"] && base == c["base"];
        if same { ok += 1 } else { bad.push(format!("{} {t:?} {px} {:?}", c["kind"], (im.width, im.height, base, &c["size"], &c["base"]))) }
    }
    assert!(bad.is_empty(), "{ok} ok, {} differ: {:#?}", bad.len(), &bad[..bad.len().min(12)]);
}
