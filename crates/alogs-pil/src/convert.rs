//! `Image.convert` between "1", "L", "RGB" and "RGBA" (libImaging/Convert.c), with Floyd-Steinberg to "1", and the
//! premultiplied "RGBa" round trip Pillow uses around resizing and transforms.

use crate::image::{Image, Mode, clip8, muldiv255};

#[inline]
fn l24(p: &[u8]) -> u8 {
    ((u32::from(p[0]) * 19595 + u32::from(p[1]) * 38470 + u32::from(p[2]) * 7471 + 0x8000) >> 16) as u8
}
#[inline]
fn l1000(p: &[u8]) -> i32 {
    i32::from(p[0]) * 299 + i32::from(p[1]) * 587 + i32::from(p[2]) * 114
}

/// `tobilevel`: L or RGB to "1" with error diffusion.
fn tobilevel(im: &Image) -> Image {
    let (w, h) = im.size();
    let mut out = Image::new(Mode::One, w, h, &[]);
    let mut errors = vec![0i32; w + 1];
    let b = im.bands();
    for y in 0..h {
        let (mut l, mut l0, mut l1) = (0i32, 0i32, 0i32);
        let row = im.row(y);
        for x in 0..w {
            let v = if b == 1 { i32::from(row[x]) } else { l1000(&row[x * b..x * b + 3]) / 1000 };
            l = i32::from(clip8(i64::from(v + (l + errors[x + 1]) / 16)));
            let o = if l > 128 { 255 } else { 0 };
            out.data[y * w + x] = o as u8;
            l -= o;
            let l2 = l;
            let d2 = l + l;
            l += d2;
            errors[x] = l + l0;
            l += d2;
            l0 = l + l1;
            l1 = l2;
            l += d2;
        }
        errors[w] = l0;
    }
    out
}

pub fn convert(im: &Image, mode: Mode) -> Image {
    use Mode::*;
    if im.mode == mode {
        return im.clone();
    }
    let (w, h) = im.size();
    let n = w * h;
    let b = im.bands();
    let mut data = Vec::with_capacity(n * mode.bands());
    match (im.mode, mode) {
        (L | Rgb, One) => return tobilevel(im),
        (Rgba, One) => return tobilevel(&convert(im, Rgb)),
        (One, L) => data.extend(im.data.iter().map(|&v| if v != 0 { 255 } else { 0 })),
        (One, Rgb) | (One, Rgba) => {
            for &v in &im.data {
                let v = if v != 0 { 255 } else { 0 };
                data.extend_from_slice(&[v, v, v]);
                if mode == Rgba {
                    data.push(255);
                }
            }
        }
        (L, Rgb) | (L, Rgba) => {
            for &v in &im.data {
                data.extend_from_slice(&[v, v, v]);
                if mode == Rgba {
                    data.push(255);
                }
            }
        }
        (Rgb | Rgba, L) => data.extend(im.data.chunks(b).map(l24)),
        (Rgb, Rgba) => {
            for p in im.data.chunks(3) {
                data.extend_from_slice(p);
                data.push(255);
            }
        }
        (Rgba, Rgb) => {
            for p in im.data.chunks(4) {
                data.extend_from_slice(&p[..3]);
            }
        }
        _ => unreachable!(),
    }
    Image::from_raw(mode, w, h, data)
}

/// RGBA -> premultiplied "RGBa" (`rgbA2rgba`).
pub fn premultiply(im: &Image) -> Image {
    let mut out = im.clone();
    for p in out.data.chunks_mut(4) {
        let a = u32::from(p[3]);
        for v in &mut p[..3] {
            *v = muldiv255(u32::from(*v), a) as u8;
        }
    }
    out
}

/// "RGBa" -> RGBA (`rgba2rgbA`).
pub fn unpremultiply(im: &Image) -> Image {
    let mut out = im.clone();
    for p in out.data.chunks_mut(4) {
        let a = i64::from(p[3]);
        if a != 255 && a != 0 {
            for v in &mut p[..3] {
                *v = clip8((255 * i64::from(*v)) / a);
            }
        }
    }
    out
}
