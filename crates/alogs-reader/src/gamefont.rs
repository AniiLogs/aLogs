//! The game's own UI font (TextMeshPro signed-distance-field atlases). Port of `GameFont` in aLogs B16
//! `bossbar/gamefont.py`: the cached atlas (gamefont*.png + .json in the data folder) and the distance field of a
//! text. Reading the atlas out of the game's Unity bundles (`extract`, UnityPy) is not ported yet: the cache the Python
//! meter wrote is used. `render` (the overlay's text style) lives with the overlay.

use alogs_pil::ops::lighter;
use alogs_pil::{Filter, Image, Mode};
use serde_json::Value as J;
use std::collections::HashMap;
use std::path::Path;

pub const FONT_NAME: &str = "UI_Font_Europe";
pub const KOREAN_FONT: &str = "UI_Font_Korea";
pub const CACHE_VERSION: i64 = 1;

fn stem(font_name: &str) -> String {
    if font_name == FONT_NAME { "gamefont".into() } else { format!("gamefont_{font_name}") }
}

pub struct GameFont {
    pub meta: J,
    pub atlas: Image,
    /// code point -> [x, y, w, h, bearing x, bearing y, advance]
    pub glyphs: HashMap<u32, [f64; 7]>,
}

impl GameFont {
    pub fn open(cache_dir: &Path, font_name: &str) -> Option<GameFont> {
        let meta: J = serde_json::from_str(&std::fs::read_to_string(cache_dir.join(format!("{}.json", stem(font_name)))).ok()?).ok()?;
        if meta.get("version").and_then(J::as_i64) != Some(CACHE_VERSION) {
            return None;
        }
        let atlas = Image::open_png(&cache_dir.join(format!("{}.png", stem(font_name))))?.convert(Mode::L);
        let mut glyphs = HashMap::new();
        for (k, v) in meta["glyphs"].as_object()? {
            let a = v.as_array()?;
            let mut g = [0.0; 7];
            for (i, x) in a.iter().take(7).enumerate() {
                g[i] = x.as_f64()?;
            }
            glyphs.insert(k.parse().ok()?, g);
        }
        Some(GameFont { meta, atlas, glyphs })
    }

    fn glyph(&self, c: char) -> Option<&[f64; 7]> {
        self.glyphs.get(&(c as u32)).or_else(|| self.glyphs.get(&63))
    }

    /// The text's distance field (0..255, 128 = the glyph edge) at `px` font size, and the baseline y.
    pub fn field(&self, text: &str, px: f64) -> (Image, f64) {
        let m = &self.meta;
        let s = px / m["point_size"].as_f64().unwrap_or(1.0);
        let pad = m["padding"].as_f64().unwrap_or(2.0);
        let spread = 10.0;
        let width: f64 = text.chars().map(|c| self.glyph(c).map_or(0.0, |g| g[6])).sum::<f64>() * s;
        let asc = m["ascent"].as_f64().unwrap_or(0.0) * s;
        let desc = -m["descent"].as_f64().unwrap_or(0.0) * s;
        let w = (width + 2.0 * spread * s + 4.0) as usize;
        let h = (asc + desc + 2.0 * spread * s + 4.0) as usize;
        let mut field = Image::new(Mode::L, w, h, &[0]);
        let mut x = spread * s;
        let base = spread * s + asc;
        let ah = m["atlas_height"].as_f64().unwrap_or(self.atlas.height as f64);
        let r = |v: f64| v.round_ties_even() as i64;
        for c in text.chars() {
            let Some(&[gx, gy, gw, gh, bx, by, adv]) = self.glyph(c) else { continue };
            if gw != 0.0 && gh != 0.0 {
                let crop = self.atlas.crop(gx - pad, ah - (gy + gh) - pad, gx + gw + pad, ah - gy + pad);
                let cw = (r(crop.width as f64 * s)).max(1) as usize;
                let ch = (r(crop.height as f64 * s)).max(1) as usize;
                let crop = crop.resize(cw, ch, Some(Filter::Bicubic));
                let (px0, py0) = (r(x + (bx - pad) * s), r(base - (by + pad) * s));
                let here = field.crop_i(px0, py0, px0 + cw as i64, py0 + ch as i64);
                field.paste(&lighter(&here, &crop), px0, py0);
            }
            x += adv * s;
        }
        (field, base)
    }
}
