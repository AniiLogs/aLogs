//! Pillow's "bcn" decoder (libImaging/BcnDecode.c) for the block formats Unity textures use: BC1 (DXT1), BC3 (DXT5),
//! BC4 and BC7, the way `Image.frombytes(mode, size, data, "bcn", n)` decodes them. UnityPy decodes these formats with
//! it, so a texture decoded here has the same pixels as the Python meter's.

use crate::{Image, Mode};

#[derive(Clone, Copy, Default)]
struct Rgba {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

fn decode_565(x: u16) -> Rgba {
    let mut r = ((x & 0xf800) >> 8) as u32;
    r |= r >> 5;
    let mut g = ((x & 0x7e0) >> 3) as u32;
    g |= g >> 6;
    let mut b = ((x & 0x1f) << 3) as u32;
    b |= b >> 5;
    Rgba { r: r as u8, g: g as u8, b: b as u8, a: 0xff }
}

fn decode_bc1_color(dst: &mut [Rgba; 16], src: &[u8], separate_alpha: bool) {
    let c0 = u16::from_le_bytes([src[0], src[1]]);
    let c1 = u16::from_le_bytes([src[2], src[3]]);
    let lut = u32::from_le_bytes([src[4], src[5], src[6], src[7]]);
    let mut p = [decode_565(c0), decode_565(c1), Rgba::default(), Rgba::default()];
    let (r0, g0, b0) = (p[0].r as u32, p[0].g as u32, p[0].b as u32);
    let (r1, g1, b1) = (p[1].r as u32, p[1].g as u32, p[1].b as u32);
    // BC2 and BC3 reuse BC1 colour blocks but always act like c0 > c1
    if c0 > c1 || separate_alpha {
        p[2] = Rgba { r: ((2 * r0 + r1) / 3) as u8, g: ((2 * g0 + g1) / 3) as u8, b: ((2 * b0 + b1) / 3) as u8, a: 0xff };
        p[3] = Rgba { r: ((r0 + 2 * r1) / 3) as u8, g: ((g0 + 2 * g1) / 3) as u8, b: ((b0 + 2 * b1) / 3) as u8, a: 0xff };
    } else {
        p[2] = Rgba { r: ((r0 + r1) / 2) as u8, g: ((g0 + g1) / 2) as u8, b: ((b0 + b1) / 2) as u8, a: 0xff };
        p[3] = Rgba { r: 0, g: 0, b: 0, a: 0 };
    }
    for (n, d) in dst.iter_mut().enumerate() {
        *d = p[(3 & (lut >> (2 * n))) as usize];
    }
}

/// The BC3 alpha / BC4 block: 16 values.
fn decode_bc3_alpha(src: &[u8]) -> [u8; 16] {
    let (a0, a1) = (src[0] as u32, src[1] as u32);
    let mut a = [0u8; 8];
    a[0] = a0 as u8;
    a[1] = a1 as u8;
    if a0 > a1 {
        for i in 0..6u32 {
            a[2 + i as usize] = (((6 - i) * a0 + (1 + i) * a1) / 7) as u8;
        }
    } else {
        for i in 0..4u32 {
            a[2 + i as usize] = (((4 - i) * a0 + (1 + i) * a1) / 5) as u8;
        }
        a[6] = 0;
        a[7] = 0xff;
    }
    let lut1 = src[2] as u32 | (src[3] as u32) << 8 | (src[4] as u32) << 16;
    let lut2 = src[5] as u32 | (src[6] as u32) << 8 | (src[7] as u32) << 16;
    let mut out = [0u8; 16];
    for n in 0..8 {
        out[n] = a[(7 & (lut1 >> (3 * n))) as usize];
        out[8 + n] = a[(7 & (lut2 >> (3 * n))) as usize];
    }
    out
}

/// (subsets, partition bits, rotation bits, index selection bit, colour bits, alpha bits, endpoint p-bits, shared
/// p-bits, index bits, secondary index bits)
const BC7_MODES: [[u8; 10]; 8] = [
    [3, 4, 0, 0, 4, 0, 1, 0, 3, 0],
    [2, 6, 0, 0, 6, 0, 0, 1, 3, 0],
    [3, 6, 0, 0, 5, 0, 0, 0, 2, 0],
    [2, 6, 0, 0, 7, 0, 1, 0, 2, 0],
    [1, 0, 2, 1, 5, 6, 0, 0, 2, 3],
    [1, 0, 2, 0, 7, 8, 0, 0, 2, 2],
    [1, 0, 0, 0, 7, 7, 1, 0, 4, 0],
    [2, 6, 0, 0, 5, 5, 1, 0, 2, 0],
];

const BC7_SI2: [u16; 64] = [
    0xcccc, 0x8888, 0xeeee, 0xecc8, 0xc880, 0xfeec, 0xfec8, 0xec80, 0xc800, 0xffec, 0xfe80, 0xe800, 0xffe8, 0xff00, 0xfff0,
    0xf000, 0xf710, 0x008e, 0x7100, 0x08ce, 0x008c, 0x7310, 0x3100, 0x8cce, 0x088c, 0x3110, 0x6666, 0x366c, 0x17e8, 0x0ff0,
    0x718e, 0x399c, 0xaaaa, 0xf0f0, 0x5a5a, 0x33cc, 0x3c3c, 0x55aa, 0x9696, 0xa55a, 0x73ce, 0x13c8, 0x324c, 0x3bdc, 0x6996,
    0xc33c, 0x9966, 0x0660, 0x0272, 0x04e4, 0x4e40, 0x2720, 0xc936, 0x936c, 0x39c6, 0x639c, 0x9336, 0x9cc6, 0x817e, 0xe718,
    0xccf0, 0x0fcc, 0x7744, 0xee22,
];

const BC7_SI3: [u32; 64] = [
    0xaa685050, 0x6a5a5040, 0x5a5a4200, 0x5450a0a8, 0xa5a50000, 0xa0a05050, 0x5555a0a0, 0x5a5a5050, 0xaa550000, 0xaa555500,
    0xaaaa5500, 0x90909090, 0x94949494, 0xa4a4a4a4, 0xa9a59450, 0x2a0a4250, 0xa5945040, 0x0a425054, 0xa5a5a500, 0x55a0a0a0,
    0xa8a85454, 0x6a6a4040, 0xa4a45000, 0x1a1a0500, 0x0050a4a4, 0xaaa59090, 0x14696914, 0x69691400, 0xa08585a0, 0xaa821414,
    0x50a4a450, 0x6a5a0200, 0xa9a58000, 0x5090a0a8, 0xa8a09050, 0x24242424, 0x00aa5500, 0x24924924, 0x24499224, 0x50a50a50,
    0x500aa550, 0xaaaa4444, 0x66660000, 0xa5a0a5a0, 0x50a050a0, 0x69286928, 0x44aaaa44, 0x66666600, 0xaa444444, 0x54a854a8,
    0x95809580, 0x96969600, 0xa85454a8, 0x80959580, 0xaa141414, 0x96960000, 0xaaaa1414, 0xa05050a0, 0xa0a5a5a0, 0x96000000,
    0x40804080, 0xa9a8a9a8, 0xaaaaaa44, 0x2a4a5254,
];

const BC7_AI0: [u8; 64] = [
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 2, 8, 2, 2, 8, 8, 15, 2, 8, 2, 2, 8, 8, 2, 2, 15, 15, 6,
    8, 2, 8, 15, 15, 2, 8, 2, 2, 2, 15, 15, 6, 6, 2, 6, 8, 15, 15, 2, 2, 15, 15, 15, 15, 15, 2, 2, 15,
];

const BC7_AI1: [u8; 64] = [
    3, 3, 15, 15, 8, 3, 15, 15, 8, 8, 6, 6, 6, 5, 3, 3, 3, 3, 8, 15, 3, 3, 6, 10, 5, 8, 8, 6, 8, 5, 15, 15, 8, 15, 3, 5, 6, 10,
    8, 15, 15, 3, 15, 5, 15, 15, 15, 15, 3, 15, 5, 5, 5, 8, 5, 10, 5, 10, 8, 13, 15, 12, 3, 3,
];

const BC7_AI2: [u8; 64] = [
    15, 8, 8, 3, 15, 15, 3, 8, 15, 15, 15, 15, 15, 15, 15, 8, 15, 8, 15, 3, 15, 8, 15, 8, 3, 15, 6, 10, 15, 15, 10, 8, 15, 3,
    15, 10, 10, 8, 9, 10, 6, 15, 8, 15, 3, 6, 6, 8, 15, 3, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 3, 15, 15, 8,
];

const W2: [u8; 4] = [0, 21, 43, 64];
const W3: [u8; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const W4: [u8; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

fn weights(n: u8) -> &'static [u8] {
    match n {
        2 => &W2,
        3 => &W3,
        _ => &W4,
    }
}

fn subset(ns: u8, partition: u8, n: usize) -> usize {
    match ns {
        2 => (1 & (BC7_SI2[partition as usize] >> n)) as usize,
        3 => (3 & (BC7_SI3[partition as usize] >> (2 * n))) as usize,
        _ => 0,
    }
}

fn get_bits(src: u128, bit: usize, count: u8) -> u8 {
    if count == 0 || bit >= 128 {
        return 0;
    }
    ((src >> bit) as u32 & ((1u32 << count) - 1)) as u8
}

fn expand(v: u8, bits: u8) -> u8 {
    let v = (((v as u32) << (8 - bits as u32)) as u8) as u32;
    (v | (v >> bits)) as u8
}

fn lerp(e: &[Rgba], s0: u8, s1: u8) -> Rgba {
    let (s0, s1) = (s0 as u32, s1 as u32);
    let (t0, t1) = (64 - s0, 64 - s1);
    let m = |a: u8, b: u8, t: u32, s: u32| ((t * a as u32 + s * b as u32 + 32) >> 6) as u8;
    Rgba { r: m(e[0].r, e[1].r, t0, s0), g: m(e[0].g, e[1].g, t0, s0), b: m(e[0].b, e[1].b, t0, s0), a: m(e[0].a, e[1].a, t1, s1) }
}

fn decode_bc7_block(col: &mut [Rgba; 16], src: &[u8]) {
    let first = src[0];
    let src = u128::from_le_bytes(src[..16].try_into().unwrap());
    if first == 0 {
        // degenerate case when no bits set
        for c in col.iter_mut() {
            *c = Rgba { r: 0, g: 0, b: 0, a: 255 };
        }
        return;
    }
    let mode = first.trailing_zeros() as usize;
    let mut bit = mode + 1;
    let [ns, pb, rb, isb, mut cb, mut ab, epb, spb, ib0, ib2_0] = BC7_MODES[mode];
    let cw = weights(ib0);
    let aw = weights(if ab != 0 && ib2_0 != 0 { ib2_0 } else { ib0 });
    let mut load = |bits: u8| {
        let v = get_bits(src, bit, bits);
        bit += bits as usize;
        v
    };
    let partition = load(pb);
    let rotation = load(rb);
    let index_sel = load(isb);
    let numep = (ns as usize) << 1;
    let mut ep = [Rgba::default(); 6];
    for e in ep.iter_mut().take(numep) {
        e.r = load(cb);
    }
    for e in ep.iter_mut().take(numep) {
        e.g = load(cb);
    }
    for e in ep.iter_mut().take(numep) {
        e.b = load(cb);
    }
    for e in ep.iter_mut().take(numep) {
        e.a = if ab != 0 { load(ab) } else { 255 };
    }
    let p = |x: &mut u8, v: u8| *x = (*x << 1) | v;
    if epb != 0 {
        cb += 1;
        if ab != 0 {
            ab += 1;
        }
        for e in ep.iter_mut().take(numep) {
            let v = load(1);
            p(&mut e.r, v);
            p(&mut e.g, v);
            p(&mut e.b, v);
            if ab != 0 {
                p(&mut e.a, v);
            }
        }
    }
    if spb != 0 {
        cb += 1;
        if ab != 0 {
            ab += 1;
        }
        for i in (0..numep).step_by(2) {
            let v = load(1);
            for e in &mut ep[i..i + 2] {
                p(&mut e.r, v);
                p(&mut e.g, v);
                p(&mut e.b, v);
                if ab != 0 {
                    p(&mut e.a, v);
                }
            }
        }
    }
    for e in ep.iter_mut().take(numep) {
        e.r = expand(e.r, cb);
        e.g = expand(e.g, cb);
        e.b = expand(e.b, cb);
        if ab != 0 {
            e.a = expand(e.a, ab);
        }
    }
    let mut cibit = bit;
    let mut aibit = cibit + 16 * ib0 as usize - ns as usize;
    for (i, c) in col.iter_mut().enumerate() {
        let s = subset(ns, partition, i) << 1;
        let mut ib = ib0;
        if i == 0 {
            ib -= 1;
        } else if ns == 2 {
            if i == BC7_AI0[partition as usize] as usize {
                ib -= 1;
            }
        } else if ns == 3 && (i == BC7_AI1[partition as usize] as usize || i == BC7_AI2[partition as usize] as usize) {
            ib -= 1;
        }
        let i0 = get_bits(src, cibit, ib) as usize;
        cibit += ib as usize;
        *c = if ab != 0 && ib2_0 != 0 {
            let ib2 = if i == 0 { ib2_0 - 1 } else { ib2_0 };
            let i1 = get_bits(src, aibit, ib2) as usize;
            aibit += ib2 as usize;
            if index_sel != 0 { lerp(&ep[s..], aw[i1], cw[i0]) } else { lerp(&ep[s..], cw[i0], aw[i1]) }
        } else {
            lerp(&ep[s..], cw[i0], cw[i0])
        };
        match rotation {
            1 => std::mem::swap(&mut c.r, &mut c.a),
            2 => std::mem::swap(&mut c.g, &mut c.a),
            3 => std::mem::swap(&mut c.b, &mut c.a),
            _ => {}
        }
    }
}

impl Image {
    /// `Image.frombytes(mode, (w, h), data, "bcn", n)` for n = 1 (BC1, RGBA), 3 (BC3, RGBA), 4 (BC4, L) and 7 (BC7,
    /// RGBA). Blocks run left to right, top to bottom; a short buffer leaves the rest black (Pillow raises "not enough
    /// image data" instead: None here).
    pub fn from_bcn(n: u8, width: usize, height: usize, data: &[u8]) -> Option<Image> {
        let (mode, block) = match n {
            1 => (Mode::Rgba, 8),
            3 | 7 => (Mode::Rgba, 16),
            4 => (Mode::L, 8),
            _ => return None,
        };
        let (bw, bh) = (width.div_ceil(4), height.div_ceil(4));
        if data.len() < bw * bh * block {
            return None;
        }
        let bands = mode.bands();
        let mut out = Image::new(mode, width, height, &[]);
        let mut col = [Rgba::default(); 16];
        for by in 0..bh {
            for bx in 0..bw {
                let src = &data[(by * bw + bx) * block..][..block];
                let lum = match n {
                    1 => {
                        decode_bc1_color(&mut col, src, false);
                        None
                    }
                    3 => {
                        decode_bc1_color(&mut col, &src[8..], true);
                        let a = decode_bc3_alpha(src);
                        for (c, a) in col.iter_mut().zip(a) {
                            c.a = a;
                        }
                        None
                    }
                    4 => Some(decode_bc3_alpha(src)),
                    _ => {
                        decode_bc7_block(&mut col, src);
                        None
                    }
                };
                for j in 0..16 {
                    let (x, y) = (bx * 4 + j % 4, by * 4 + j / 4);
                    if x >= width || y >= height {
                        continue;
                    }
                    let o = (y * width + x) * bands;
                    match lum {
                        Some(l) => out.data[o] = l[j],
                        None => out.data[o..o + 4].copy_from_slice(&[col[j].r, col[j].g, col[j].b, col[j].a]),
                    }
                }
            }
        }
        Some(out)
    }

    /// Saves the picture as a PNG (L, RGB or RGBA; mode 1 as L).
    pub fn save_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut enc = png::Encoder::new(file, self.width as u32, self.height as u32);
        enc.set_color(match self.mode {
            Mode::One | Mode::L => png::ColorType::Grayscale,
            Mode::Rgb => png::ColorType::Rgb,
            Mode::Rgba => png::ColorType::Rgba,
        });
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(std::io::Error::other)?;
        w.write_image_data(&self.data).map_err(std::io::Error::other)?;
        w.finish().map_err(std::io::Error::other)
    }
}
