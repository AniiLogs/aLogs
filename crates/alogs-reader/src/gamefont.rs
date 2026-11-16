//! The game's own UI font (TextMeshPro signed-distance-field atlases). Port of `GameFont` in aLogs B16
//! `bossbar/gamefont.py`: the cached atlas (gamefont*.png + .json in the data folder) and the distance field of a
//! text, and `extract`, which reads the atlas and glyph table out of the game's Unity bundles (alogs-unity in place of
//! UnityPy) into that cache. `render` draws a text in the game's HUD style.

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

/// Writes `<cache_dir>/gamefont.png` and `gamefont.json` (or the other font's own pair) from the game install, as
/// gamefont.py `extract` does. Ok(false) when the install or the font isn't found.
///
/// Differences from the Python, both so it works on today's installs: the game is found as icons.py finds it
/// (gamedata's `find_game_data`, which also knows installs outside steamapps), and the font bundles are also looked
/// for in the game's download cache, by their names in its manifests (an install may keep them only there).
pub fn extract(cache_dir: &Path, player_log: Option<&Path>, font_name: &str) -> Result<bool, String> {
    use alogs_unity::{CLASS_MONOBEHAVIOUR, CLASS_TEXTURE2D, Env, V};
    let Some((_, game)) = crate::bundles::game(player_log) else { return Ok(false) };
    let mut paths: Vec<std::path::PathBuf> =
        crate::bundles::shipped(&game).into_iter().filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().contains("xgui_font"))).collect();
    let mut cached: Vec<(String, std::path::PathBuf)> = crate::bundles::cached_by_name(&game).into_iter().filter(|(n, _)| n.contains("xgui_font")).collect();
    cached.sort();
    let shipped_names: Vec<String> = paths.iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
    paths.extend(cached.into_iter().filter(|(n, _)| !shipped_names.contains(n)).map(|(_, p)| p));
    if paths.is_empty() {
        return Ok(false);
    }
    let env = Env::load(&paths)?;
    let num = |v: &V| match *v {
        V::Int(i) => J::from(i),
        V::Float(f) => J::from(f),
        V::Bool(b) => J::from(b),
        _ => J::Null,
    };
    for o in env.objects().collect::<Vec<_>>() {
        if env.class_id(o) != CLASS_MONOBEHAVIOUR {
            continue;
        }
        let Ok(tree) = env.read(o) else { continue };
        if tree.get("m_Name").as_str() != Some(font_name) {
            continue;
        }
        let atlas = tree.get("m_AtlasTextures").as_list().first().cloned().unwrap_or_default();
        let Some(tex) = atlas.get("m_PathID").as_i64().and_then(|pid| env.find(pid, CLASS_TEXTURE2D)) else { continue };
        let img = env.texture(tex)?.convert(Mode::Rgba).split().swap_remove(3); // the distance field is in alpha
        std::fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
        img.save_png(&cache_dir.join(format!("{}.png", stem(font_name)))).map_err(|e| e.to_string())?;
        let face = tree.get("m_FaceInfo");
        let mut glyphs: HashMap<i64, &V> = HashMap::new();
        for g in tree.get("m_GlyphTable").as_list() {
            glyphs.insert(g.get("m_Index").as_i64().unwrap_or(-1), g);
        }
        let mut table = serde_json::Map::new();
        for ch in tree.get("m_CharacterTable").as_list() {
            let Some(g) = ch.get("m_GlyphIndex").as_i64().and_then(|i| glyphs.get(&i)) else { continue };
            let (r, m) = (g.get("m_GlyphRect"), g.get("m_Metrics"));
            let row: Vec<J> = [r.get("m_X"), r.get("m_Y"), r.get("m_Width"), r.get("m_Height"), m.get("m_HorizontalBearingX"), m.get("m_HorizontalBearingY"), m.get("m_HorizontalAdvance")]
                .into_iter()
                .map(num)
                .collect();
            table.insert(num(ch.get("m_Unicode")).to_string(), J::from(row));
        }
        let padding = if tree.has("m_AtlasPadding") { num(tree.get("m_AtlasPadding")) } else { J::from(2) };
        let meta = serde_json::json!({
            "version": CACHE_VERSION, "point_size": num(face.get("m_PointSize")), "ascent": num(face.get("m_AscentLine")),
            "descent": num(face.get("m_DescentLine")), "line_height": num(face.get("m_LineHeight")), "padding": padding,
            "atlas_height": img.height, "glyphs": table,
        });
        std::fs::write(cache_dir.join(format!("{}.json", stem(font_name))), meta.to_string()).map_err(|e| e.to_string())?;
        return Ok(true);
    }
    Ok(false)
}

impl GameFont {
    /// The cached font, extracting it from the game first when there's no cache or it's from another version
    /// (gamefont.py `GameFont.load`); the error says why when the game files can't be read.
    pub fn load(cache_dir: &Path, player_log: Option<&Path>, font_name: &str) -> Result<GameFont, String> {
        if let Some(f) = GameFont::open(cache_dir, font_name) {
            return Ok(f);
        }
        match extract(cache_dir, player_log, font_name) {
            Ok(true) => GameFont::open(cache_dir, font_name).ok_or_else(|| "the extracted font can't be read".to_string()),
            Ok(false) => Err("the game install was not found".into()),
            Err(e) => Err(e),
        }
    }

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

    /// RGBA image of the text in the game's style, and its baseline's y in the picture (gamefont.render). `bold`
    /// thickens the strokes; `stroke`: a solid black outline instead of the faint one; `lead`: the first n characters
    /// in their own colour, with a thicker outline ('HP' in red before white numbers).
    #[allow(clippy::too_many_arguments)]
    pub fn render(&self, text: &str, px: f64, fill: [u8; 3], outline: [u8; 3], italic: bool, bold: bool, stroke: bool, lead: Option<(usize, [u8; 3])>) -> (Image, i64) {
        use alogs_pil::ops::{gaussian_blur, max_filter, multiply};
        let (field, base) = self.field(text, px);
        let soft = (64.0 / px.max(1.0) * 2.0).max(1.0);
        let band = |edge: f64| field.point(|v| ((f64::from(v) - edge) * 255.0 / (2.0 * soft * 8.0) + 128.0).clamp(0.0, 255.0).trunc());
        let edge = 128.0 - if bold { 14.0 } else { 0.0 };
        let body = band(edge);
        let rim = band(edge - 22.0);
        let shadow = gaussian_blur(&rim, (px / 10.0).max(1.5));
        let (w, h) = (field.width, field.height);
        let dy = ((px / 20.0).round_ties_even() as usize).max(1);
        let mut out = Image::new(Mode::Rgba, w, h + dy, &[0, 0, 0, 0]);
        let mut sh = Image::new(Mode::Rgba, w, h, &[outline[0], outline[1], outline[2], 0]);
        sh.putalpha(&shadow.point(|v| f64::from(u32::from(v) * 120 / 255)));
        out.alpha_composite(&sh, 0, dy as i64);
        let mut upto = 0usize;
        if let Some((n, _)) = lead {
            let ps = self.meta["point_size"].as_f64().unwrap_or(1.0);
            let s = px / ps;
            let adv: f64 = text.chars().take(n).map(|c| self.glyph(c).or_else(|| self.glyph('?')).map_or(0.0, |g| g[6])).sum();
            upto = w.min((10.0 * s + 2.0 + adv * s).round_ties_even() as usize);
        }
        let oc = if stroke { [0, 0, 0] } else { outline };
        let mut ol = Image::new(Mode::Rgba, w, h, &[oc[0], oc[1], oc[2], 0]);
        if stroke {
            let size = ((px / 10.0).round_ties_even() as usize).max(3) | 1;
            let mut alpha = lighter(&rim, &max_filter(&body, size));
            if lead.is_some() {
                let grow = ((px / 4.0).round_ties_even() as usize).max(5) | 1;
                let thick = max_filter(&body, grow);
                let mut keep = Image::new(Mode::L, w, h, &[0]);
                keep.paste(&Image::new(Mode::L, w.min(upto + grow / 2), h, &[255]), 0, 0);
                alpha = lighter(&alpha, &multiply(&thick, &keep));
            }
            ol.putalpha(&alpha);
        } else {
            ol.putalpha(&rim.point(|v| f64::from(u32::from(v) * 55 / 255)));
        }
        out.alpha_composite(&ol, 0, 0);
        let mut fg = Image::new(Mode::Rgba, w, h, &[fill[0], fill[1], fill[2], 0]);
        if let Some((_, c)) = lead
            && upto > 0
        {
            fg.paste(&Image::new(Mode::Rgba, upto, h, &[c[0], c[1], c[2], 0]), 0, 0);
        }
        fg.putalpha(&body);
        out.alpha_composite(&fg, 0, 0);
        if italic {
            let k = 0.21;
            let nw = out.width + (out.height as f64 * k) as usize;
            out = out.transform_affine(nw, out.height, [1.0, k, -(out.height as f64) * k, 0.0, 1.0, 0.0], Filter::Bicubic, &[0, 0, 0, 0]);
        }
        let bb = out.getbbox().unwrap_or((0, 0, 1, 1));
        let out = out.crop_i(bb.0 as i64, bb.1 as i64, bb.2 as i64, bb.3 as i64);
        (out, base.round_ties_even() as i64 - bb.1 as i64)
    }
}
