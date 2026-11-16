//! Texture2D pictures (UnityPy's Texture2DConverter) for the formats the game's UI uses. Rows come out as stored,
//! bottom row first (`flip=False`); [`Env::texture`] flips them the way `Texture2D.image` does.

use crate::{Env, Obj, Result, V};
use alogs_pil::{Image, Mode};

pub const ALPHA8: i64 = 1;
pub const RGB24: i64 = 3;
pub const RGBA32: i64 = 4;
pub const ARGB32: i64 = 5;
pub const DXT1: i64 = 10;
pub const DXT5: i64 = 12;
pub const RGBA_HALF: i64 = 17;
pub const BC7: i64 = 25;
pub const BC4: i64 = 26;

fn half_to_f64(h: u16) -> f64 {
    let (sign, exp, man) = ((h >> 15) & 1, ((h >> 10) & 0x1f) as i32, (h & 0x3ff) as f64);
    let v = match exp {
        0 => man * 2f64.powi(-24),
        31 => {
            if man == 0.0 {
                f64::INFINITY
            } else {
                f64::NAN
            }
        }
        e => (1.0 + man / 1024.0) * 2f64.powi(e - 15),
    };
    if sign == 1 { -v } else { v }
}

fn need(data: &[u8], n: usize) -> Result<&[u8]> {
    data.get(..n).ok_or_else(|| "not enough image data".to_string())
}

/// The texture's pixels as UnityPy's `parse_image_data(..., flip=False)` makes them.
pub fn decode(format: i64, width: usize, height: usize, data: &[u8]) -> Result<Image> {
    if width == 0 || height == 0 {
        return Ok(Image::new(Mode::Rgba, 0, 0, &[]));
    }
    if data.is_empty() {
        return Err("Texture2D has no image data".into());
    }
    let n = width * height;
    let (pw, ph) = (width.div_ceil(4) * 4, height.div_ceil(4) * 4);
    let bcn = |k: u8| -> Result<Image> {
        let im = Image::from_bcn(k, pw, ph, data).ok_or("not enough image data")?;
        Ok(if (pw, ph) != (width, height) { im.crop_i(0, 0, width as i64, height as i64) } else { im })
    };
    Ok(match format {
        ALPHA8 => {
            let a = need(data, n)?;
            let mut d = vec![0u8; n * 4];
            for (px, &v) in d.chunks_mut(4).zip(a) {
                px[3] = v;
            }
            Image::from_raw(Mode::Rgba, width, height, d)
        }
        RGB24 => Image::from_raw(Mode::Rgb, width, height, need(data, n * 3)?.to_vec()),
        RGBA32 => Image::from_raw(Mode::Rgba, width, height, need(data, n * 4)?.to_vec()),
        ARGB32 => {
            let d = need(data, n * 4)?.chunks(4).flat_map(|p| [p[1], p[2], p[3], p[0]]).collect();
            Image::from_raw(Mode::Rgba, width, height, d)
        }
        DXT1 => bcn(1)?,
        DXT5 => bcn(3)?,
        BC4 => bcn(4)?,
        BC7 => bcn(7)?,
        RGBA_HALF => {
            // UnityPy's half(): three halves a pixel (its "RGB" codec), each int(x * 256), which must fit a byte
            let mut d = Vec::with_capacity(n * 3);
            for h in need(data, n * 6)?.chunks(2) {
                let v = (half_to_f64(u16::from_le_bytes([h[0], h[1]])) * 256.0).trunc();
                if !(0.0..=255.0).contains(&v) {
                    return Err("bytes must be in range(0, 256)".into());
                }
                d.push(v as u8);
            }
            Image::from_raw(Mode::Rgb, width, height, d)
        }
        f => return Err(format!("Not implemented texture format: {f}")),
    })
}

impl Env {
    /// A Texture2D's stored pixels (bottom row first): its image data, or its stream in a `.resS`.
    pub fn texture_raw(&self, tex: &V) -> Result<Image> {
        let (w, h) = (tex.get("m_Width").as_i64().unwrap_or(0) as usize, tex.get("m_Height").as_i64().unwrap_or(0) as usize);
        let format = tex.get("m_TextureFormat").as_i64().ok_or("no texture format")?;
        let inline = tex.get("image data").as_bytes();
        let stream = tex.get("m_StreamData");
        let data = if !inline.is_empty() {
            inline.to_vec()
        } else if let Some(path) = stream.get("path").as_str().filter(|p| !p.is_empty()) {
            let (off, size) = (stream.get("offset").as_i64().unwrap_or(0) as u64, stream.get("size").as_i64().unwrap_or(0) as usize);
            self.resource(path, off, size)?
        } else {
            Vec::new()
        };
        decode(format, w, h, &data)
    }

    /// `Texture2D.image`: the picture the right way up.
    pub fn texture(&self, o: Obj) -> Result<Image> {
        Ok(crate::sprite::flip(self.texture_raw(&self.read(o)?)?))
    }
}
