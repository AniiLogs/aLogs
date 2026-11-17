//! Sprite pictures as UnityPy's SpriteHelper draws them: the texture's rect, turned back if it was packed rotated,
//! and for a tight sprite masked to its mesh's triangles; then flipped the right way up.
//!
//! Not ported (an error instead): sprites in a SpriteAtlas, and tight sprites whose mesh has texture coordinates
//! (UnityPy copies their triangles one by one). None of the game's UI sprites are either.

use crate::{CLASS_SPRITE_ATLAS, CLASS_TEXTURE2D, Env, Obj, Result, V};
use alogs_pil::{Image, Mode, Transpose, draw};

/// Bytes a component of a vertex format takes (2019+ formats).
fn component_size(format: i64) -> Result<usize> {
    Ok(match format {
        0 | 10 | 11 => 4,
        1 | 4 | 5 | 8 | 9 => 2,
        2 | 3 | 6 | 7 => 1,
        f => return Err(format!("unknown vertex format {f}")),
    })
}

pub struct Mesh {
    pub vertices: Vec<[f64; 3]>,
    /// some texture coordinate isn't 0
    pub has_uv: bool,
    pub triangles: Vec<[usize; 3]>,
}

/// The sprite's mesh (UnityPy's MeshHandler on the SpriteRenderData): vertex positions, whether it has UVs, and
/// its triangles.
pub fn mesh(rd: &V) -> Result<Mesh> {
    let vd = rd.get("m_VertexData");
    let count = vd.get("m_VertexCount").as_i64().unwrap_or(0) as usize;
    let data = vd.get("m_DataSize").as_bytes();
    let chans: Vec<[i64; 4]> = vd
        .get("m_Channels")
        .as_list()
        .iter()
        .map(|c| ["stream", "offset", "format", "dimension"].map(|k| c.get(k).as_i64().unwrap_or(0)))
        .collect();
    let mut vertices = Vec::new();
    let mut has_uv = false;
    if count > 0 && !data.is_empty() && !chans.is_empty() {
        let streams = 1 + chans.iter().map(|c| c[0]).max().unwrap_or(0) as usize;
        let mut info = Vec::new(); // (offset, stride)
        let mut offset = 0usize;
        for s in 0..streams {
            let mut stride = 0;
            for c in chans.iter().filter(|c| c[0] as usize == s && c[3] > 0) {
                stride += (c[3] as usize & 0xf) * component_size(c[2])?;
            }
            info.push((offset, stride));
            offset += count * stride;
            offset = (offset + 15) & !15;
        }
        let read = |ch: usize, comps: usize| -> Result<Vec<Vec<f64>>> {
            let c = chans[ch];
            if c[2] != 0 {
                return Err(format!("vertex channel {ch} isn't float"));
            }
            let (so, stride) = info[c[0] as usize];
            let mut out = Vec::with_capacity(count);
            for v in 0..count {
                let at = so + v * stride + c[1] as usize;
                let mut p = Vec::with_capacity(comps);
                for k in 0..comps.min(c[3] as usize & 0xf) {
                    let b = data.get(at + 4 * k..at + 4 * k + 4).ok_or("vertex data cut short")?;
                    p.push(f32::from_le_bytes(b.try_into().unwrap()) as f64);
                }
                out.push(p);
            }
            Ok(out)
        };
        if chans[0][3] > 0 {
            vertices = read(0, 3)?.into_iter().map(|p| [p.first().copied().unwrap_or(0.0), p.get(1).copied().unwrap_or(0.0), p.get(2).copied().unwrap_or(0.0)]).collect();
        }
        if chans.len() > 4 && chans[4][3] > 0 {
            has_uv = read(4, 2)?.iter().any(|p| p.iter().any(|&x| x != 0.0));
        }
    }
    let ib = rd.get("m_IndexBuffer").as_bytes();
    let idx: Vec<usize> = ib.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b) as usize).collect();
    let mut triangles = Vec::new();
    for sm in rd.get("m_SubMeshes").as_list() {
        let first = sm.get("firstByte").as_i64().unwrap_or(0) as usize / 2;
        let n = sm.get("indexCount").as_i64().unwrap_or(0) as usize;
        if sm.get("topology").as_i64().unwrap_or(0) != 0 {
            return Err("only triangle meshes are read".into());
        }
        for i in (first..first + n).step_by(3) {
            let t = idx.get(i..i + 3).ok_or("index buffer cut short")?;
            triangles.push([t[0], t[1], t[2]]);
        }
    }
    Ok(Mesh { vertices, has_uv, triangles })
}

/// UnityPy's mask_sprite: the triangles (positions from the lowest vertex, times pixels per unit) filled in a mode
/// "1" mask; the picture kept inside it.
fn mask(sprite: &V, m: &Mesh, img: Image) -> Result<Image> {
    if m.vertices.is_empty() {
        return Err("No vertices found in sprite mesh!".into());
    }
    let min_x = m.vertices.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min);
    let min_y = m.vertices.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min);
    let factor = sprite.get("m_PixelsToUnits").as_f64().unwrap_or(100.0);
    let pts: Vec<(f64, f64)> = m.vertices.iter().map(|v| ((v[0] - min_x) * factor, (v[1] - min_y) * factor)).collect();
    let mut mk = Image::new(Mode::One, img.width, img.height, &[0]);
    for t in &m.triangles {
        let tri = [*pts.get(t[0]).ok_or("bad index")?, *pts.get(t[1]).ok_or("bad index")?, *pts.get(t[2]).ok_or("bad index")?];
        draw::polygon(&mut mk, &tri, &[1]);
    }
    Ok(match img.mode {
        // composite(img, empty, mask) with a 0/255 mask: the picture where it's set, nothing elsewhere
        Mode::Rgba => {
            let mut out = img;
            for (px, &m) in out.data.as_chunks_mut::<4>().0.iter_mut().zip(&mk.data) {
                if m == 0 {
                    px.copy_from_slice(&[0, 0, 0, 0]);
                }
            }
            out
        }
        // putalpha(mask): RGB becomes RGBA; L becomes LA, which converts to RGBA as (l, l, l, a)
        _ => {
            let mut out = img.convert(Mode::Rgba);
            out.putalpha(&mk.convert(Mode::L));
            out
        }
    })
}

impl Env {
    /// `Sprite.image`.
    pub fn sprite(&self, o: Obj) -> Result<Image> {
        let s = self.read(o)?;
        self.sprite_from(o, &s)
    }

    /// `Sprite.image` for an already read sprite.
    pub fn sprite_from(&self, o: Obj, s: &V) -> Result<Image> {
        if s.get("m_SpriteAtlas").get("m_PathID").as_i64().unwrap_or(0) != 0 {
            return Err("sprites in an atlas aren't read".into());
        }
        if let Some(tag) = s.get("m_AtlasTags").as_list().first().and_then(V::as_str) {
            let f = &self.files[o.file].2;
            let atlas = (0..f.objects.len()).map(|index| Obj { file: o.file, index }).any(|a| self.class_id(a) == CLASS_SPRITE_ATLAS && self.peek_name(a).as_deref() == Some(tag));
            if atlas {
                return Err("sprites in an atlas aren't read".into());
            }
        }
        let rd = s.get("m_RD");
        let tex = self.deref(o, rd.get("texture"), CLASS_TEXTURE2D).ok_or("the sprite's texture isn't loaded")?;
        let mut original = self.texture_raw(&self.read(tex)?)?;
        if let Some(alpha) = self.deref(o, rd.get("alphaTexture"), CLASS_TEXTURE2D) {
            let a = self.texture_raw(&self.read(alpha)?)?;
            let a = a.split().swap_remove(0);
            let mut rgba = original.convert(Mode::Rgba);
            rgba.putalpha(&a);
            original = rgba;
        }
        let r = rd.get("textureRect");
        let f = |k: &str| r.get(k).as_f64().unwrap_or(0.0);
        let (x, y, w, h) = (f("x"), f("y"), f("width"), f("height"));
        let mut img = original.crop(x, y, x + w, y + h);
        let settings = rd.get("settingsRaw").as_i64().unwrap_or(0);
        if settings & 1 == 1 {
            img = match (settings >> 2) & 0xf {
                1 => img.transpose(Transpose::FlipLeftRight),
                2 => img.transpose(Transpose::FlipTopBottom),
                3 => img.transpose(Transpose::Rotate180),
                4 => img.transpose(Transpose::Rotate270),
                _ => img,
            };
        }
        if (settings >> 1) & 1 == 0 {
            // tight packing
            let m = mesh(rd)?;
            if m.has_uv {
                return Err("tight sprites with texture coordinates aren't read".into());
            }
            img = mask(s, &m, img)?;
        }
        Ok(flip(img))
    }
}

/// `transpose(FLIP_TOP_BOTTOM)`, a row at a time.
pub fn flip(img: Image) -> Image {
    let row = img.width * img.mode.bands();
    if row == 0 {
        return img;
    }
    let data = img.data.chunks_exact(row).rev().flatten().copied().collect();
    Image::from_raw(img.mode, img.width, img.height, data)
}
