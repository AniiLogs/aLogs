//! Drawing a short label in a TrueType font (the label `prep_labelled` prints before a lone number for OCR).
//!
//! Known difference: Pillow draws with FreeType; this uses ab_glyph, so the label's pixels differ slightly. Only
//! Windows OCR reads them (the label is dropped from the read, `after_label`).

use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont};
use alogs_pil::Image;

pub struct Font {
    font: FontVec,
}

impl Font {
    pub fn load(path: &std::path::Path) -> Option<Font> {
        let data = std::fs::read(path).ok()?;
        Some(Font { font: FontVec::try_from_vec(data).ok()? })
    }

    /// `ImageFont.truetype('arialbd.ttf')`: Windows' Arial Bold.
    pub fn arial_bold() -> Option<Font> {
        let dir = std::env::var_os("WINDIR").map_or_else(|| std::path::PathBuf::from(r"C:\Windows"), std::path::PathBuf::from);
        Font::load(&dir.join("Fonts").join("arialbd.ttf"))
    }

    /// `font.getlength(text)` at `px` pixels.
    pub fn length(&self, text: &str, px: f32) -> f32 {
        let f = self.font.as_scaled(PxScale::from(px));
        let mut w = 0.0;
        let mut prev = None;
        for c in text.chars() {
            let id = f.glyph_id(c);
            if let Some(p) = prev {
                w += f.kern(p, id);
            }
            w += f.h_advance(id);
            prev = Some(id);
        }
        w
    }

    /// `ImageDraw.text((x, y), text, fill=ink)` on an L image: (x, y) is the top left of the text's line box.
    pub fn draw(&self, im: &mut Image, x: f32, y: f32, text: &str, px: f32, ink: u8) {
        let f = self.font.as_scaled(PxScale::from(px));
        let mut pen = x;
        let base = y + f.ascent();
        let mut prev = None;
        for c in text.chars() {
            let id = f.glyph_id(c);
            if let Some(p) = prev {
                pen += f.kern(p, id);
            }
            let g = id.with_scale_and_position(PxScale::from(px), ab_glyph::point(pen, base));
            if let Some(o) = self.font.outline_glyph(g) {
                let bb = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    let (xx, yy) = (bb.min.x as i64 + i64::from(gx), bb.min.y as i64 + i64::from(gy));
                    if xx >= 0 && yy >= 0 && (xx as usize) < im.width && (yy as usize) < im.height && cov > 0.0 {
                        let p = im.px_mut(xx as usize, yy as usize);
                        let a = cov.min(1.0);
                        p[0] = (f32::from(p[0]) * (1.0 - a) + f32::from(ink) * a).round() as u8;
                    }
                });
            }
            pen += f.h_advance(id);
            prev = Some(id);
        }
    }
}
