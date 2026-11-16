//! The image type and the basic operations, matching Pillow 12.3 (`Image.py` and libImaging) pixel for pixel.

use std::fmt;

/// The modes aLogs uses. `One` keeps 0 or 255 a pixel, like Pillow's mode "1" in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    One,
    L,
    Rgb,
    Rgba,
}

impl Mode {
    pub fn bands(self) -> usize {
        match self {
            Mode::One | Mode::L => 1,
            Mode::Rgb => 3,
            Mode::Rgba => 4,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Mode::One => "1",
            Mode::L => "L",
            Mode::Rgb => "RGB",
            Mode::Rgba => "RGBA",
        }
    }
    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "1" => Mode::One,
            "L" => Mode::L,
            "RGB" => Mode::Rgb,
            "RGBA" => Mode::Rgba,
            _ => return None,
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    pub mode: Mode,
    pub width: usize,
    pub height: usize,
    /// `bands` bytes a pixel, rows top to bottom.
    pub data: Vec<u8>,
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<Image {} {}x{}>", self.mode.name(), self.width, self.height)
    }
}

/// `CLIP8`.
#[inline]
pub(crate) fn clip8(v: i64) -> u8 {
    v.clamp(0, 255) as u8
}

/// `MULDIV255(a, b)`.
#[inline]
pub(crate) fn muldiv255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    ((t >> 8) + t) >> 8
}

/// `DIV255(a)`.
#[inline]
pub(crate) fn div255(a: u32) -> u32 {
    let t = a + 128;
    ((t >> 8) + t) >> 8
}

/// `round()` in Python (half to even), for box coordinates.
pub(crate) fn py_round(v: f64) -> i64 {
    v.round_ties_even() as i64
}

impl Image {
    /// `Image.new(mode, size, color)`: `color` is one value per band (a single value fills every band of L and 1;
    /// for RGB/RGBA give all bands). Missing bands are 0 (alpha 255 is not assumed, as in Pillow for a tuple).
    pub fn new(mode: Mode, width: usize, height: usize, color: &[u8]) -> Image {
        let b = mode.bands();
        let mut px = vec![0u8; b];
        match (mode, color.len()) {
            (_, 0) => {}
            (Mode::One, _) => px[0] = if color[0] != 0 { 255 } else { 0 },
            (_, 1) if b > 1 => {
                // Pillow: an int colour for RGB/RGBA is packed little-endian; aLogs only passes 0
                px[0] = color[0];
                if mode == Mode::Rgba {
                    px[3] = 0;
                }
            }
            _ => px[..color.len().min(b)].copy_from_slice(&color[..color.len().min(b)]),
        }
        let mut data = Vec::with_capacity(width * height * b);
        for _ in 0..width * height {
            data.extend_from_slice(&px);
        }
        Image { mode, width, height, data }
    }

    pub fn from_raw(mode: Mode, width: usize, height: usize, data: Vec<u8>) -> Image {
        assert_eq!(data.len(), width * height * mode.bands(), "frombytes size");
        Image { mode, width, height, data }
    }

    pub fn bands(&self) -> usize {
        self.mode.bands()
    }
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    #[inline]
    pub fn px(&self, x: usize, y: usize) -> &[u8] {
        let b = self.bands();
        let i = (y * self.width + x) * b;
        &self.data[i..i + b]
    }
    #[inline]
    pub fn px_mut(&mut self, x: usize, y: usize) -> &mut [u8] {
        let b = self.bands();
        let i = (y * self.width + x) * b;
        &mut self.data[i..i + b]
    }
    pub fn row(&self, y: usize) -> &[u8] {
        let n = self.width * self.bands();
        &self.data[y * n..(y + 1) * n]
    }

    /// `tobytes()` (mode "1" packs 8 pixels a byte, high bit first).
    pub fn tobytes(&self) -> Vec<u8> {
        if self.mode != Mode::One {
            return self.data.clone();
        }
        let mut out = Vec::with_capacity(self.width.div_ceil(8) * self.height);
        for y in 0..self.height {
            for chunk in self.row(y).chunks(8) {
                let mut byte = 0u8;
                for (i, &v) in chunk.iter().enumerate() {
                    if v != 0 {
                        byte |= 0x80 >> i;
                    }
                }
                out.push(byte);
            }
        }
        out
    }

    /// `getpixel((x, y))` as a list of band values.
    pub fn getpixel(&self, x: usize, y: usize) -> Vec<u8> {
        self.px(x, y).to_vec()
    }
    pub fn putpixel(&mut self, x: usize, y: usize, v: &[u8]) {
        let one = self.mode == Mode::One;
        let p = self.px_mut(x, y);
        for (i, d) in p.iter_mut().enumerate() {
            let s = *v.get(i).unwrap_or(&0);
            *d = if one { if s != 0 { 255 } else { 0 } } else { s };
        }
    }

    /// `convert(mode)` with Pillow's default Floyd-Steinberg dithering to "1".
    pub fn convert(&self, mode: Mode) -> Image {
        crate::convert::convert(self, mode)
    }

    /// `crop(box)`: coordinates rounded as Pillow does; outside the image is 0.
    pub fn crop(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> Image {
        let (x0, y0, x1, y1) = (py_round(x0), py_round(y0), py_round(x1), py_round(y1));
        let (w, h) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
        let b = self.bands();
        let mut out = Image::new(self.mode, w, h, &[]);
        for y in 0..h as i64 {
            let sy = y0 + y;
            if sy < 0 || sy >= self.height as i64 {
                continue;
            }
            let xs = x0.max(0);
            let xe = x1.min(self.width as i64);
            if xs >= xe {
                continue;
            }
            let src = &self.data[(sy as usize * self.width + xs as usize) * b..(sy as usize * self.width + xe as usize) * b];
            let d0 = (y as usize * w + (xs - x0) as usize) * b;
            out.data[d0..d0 + src.len()].copy_from_slice(src);
        }
        out
    }
    /// `crop` with integer box.
    pub fn crop_i(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> Image {
        self.crop(x0 as f64, y0 as f64, x1 as f64, y1 as f64)
    }

    /// `point(lut)` with a table per band (`lut.len() == 256 * bands`, values clipped to 0..255 as Pillow does).
    pub fn point_table(&self, lut: &[i64]) -> Image {
        let b = self.bands();
        assert_eq!(lut.len(), 256 * b);
        let table: Vec<u8> = lut.iter().map(|&v| clip8(v)).collect();
        let mut out = self.clone();
        for (i, v) in out.data.iter_mut().enumerate() {
            *v = table[(i % b) * 256 + *v as usize];
        }
        // the mode stays (a "1" image keeps the table's raw values, as Pillow's does; nonzero counts as white)
        out
    }

    /// `point(function)`: the function on 0..255, the same for every band (results rounded half to even, as
    /// `Image.point` rounds a table).
    pub fn point(&self, f: impl Fn(u8) -> f64) -> Image {
        let one: Vec<i64> = (0..=255u8).map(|v| py_round(f(v))).collect();
        let mut lut = Vec::with_capacity(256 * self.bands());
        for _ in 0..self.bands() {
            lut.extend_from_slice(&one);
        }
        self.point_table(&lut)
    }

    /// `point(lut, "1")` on an L image: the table's raw results as a "1" image (nonzero counts as white).
    pub fn point_to_1(&self, f: impl Fn(u8) -> f64) -> Image {
        let mut out = self.point(f);
        out.mode = Mode::One;
        out
    }

    /// `split()`: one L image a band ("1" stays "1").
    pub fn split(&self) -> Vec<Image> {
        let b = self.bands();
        let m = if self.mode == Mode::One { Mode::One } else { Mode::L };
        (0..b)
            .map(|k| Image::from_raw(m, self.width, self.height, self.data.iter().skip(k).step_by(b).copied().collect()))
            .collect()
    }

    /// `Image.merge(mode, bands)`.
    pub fn merge(mode: Mode, bands: &[Image]) -> Image {
        let (w, h) = bands[0].size();
        let b = mode.bands();
        assert_eq!(bands.len(), b);
        let mut data = vec![0u8; w * h * b];
        for (k, band) in bands.iter().enumerate() {
            for (i, &v) in band.data.iter().enumerate() {
                data[i * b + k] = v;
            }
        }
        Image::from_raw(mode, w, h, data)
    }

    /// `getbbox()` (for RGBA, the alpha band only): `(x0, y0, x1, y1)` of what isn't 0, or None.
    pub fn getbbox(&self) -> Option<(usize, usize, usize, usize)> {
        let b = self.bands();
        let nonzero = |p: &[u8]| if self.mode == Mode::Rgba { p[3] != 0 } else { p.iter().any(|&v| v != 0) };
        let (mut x0, mut y0, mut x1, mut y1) = (self.width, self.height, 0, 0);
        for y in 0..self.height {
            for x in 0..self.width {
                let i = (y * self.width + x) * b;
                if nonzero(&self.data[i..i + b]) {
                    x0 = x0.min(x);
                    x1 = x1.max(x + 1);
                    y0 = y0.min(y);
                    y1 = y1.max(y + 1);
                }
            }
        }
        if x1 == 0 { None } else { Some((x0, y0, x1, y1)) }
    }

    /// `histogram(mask=None)`: 256 counts a band.
    pub fn histogram(&self, mask: Option<&Image>) -> Vec<u64> {
        let b = self.bands();
        let mut h = vec![0u64; 256 * b];
        for i in 0..self.width * self.height {
            if let Some(m) = mask
                && m.data[i * m.bands()] == 0
            {
                continue;
            }
            for k in 0..b {
                h[k * 256 + self.data[i * b + k] as usize] += 1;
            }
        }
        h
    }

    /// `getextrema()` per band.
    pub fn getextrema(&self) -> Vec<(u8, u8)> {
        let b = self.bands();
        (0..b)
            .map(|k| {
                let it = self.data.iter().skip(k).step_by(b);
                (*it.clone().min().unwrap_or(&0), *it.max().unwrap_or(&0))
            })
            .collect()
    }

    /// `paste(im, (x, y))` without a mask (the other image converted to this mode first, as Pillow does).
    pub fn paste(&mut self, im: &Image, x: i64, y: i64) {
        self.paste_mask(im, x, y, None);
    }

    /// `paste(im, (x, y), mask)`: with an L/1 mask the pasted pixels blend by it; an RGBA mask uses its alpha.
    pub fn paste_mask(&mut self, im: &Image, x: i64, y: i64, mask: Option<&Image>) {
        let src = if im.mode == self.mode { im.clone() } else { im.convert(self.mode) };
        let b = self.bands();
        for sy in 0..src.height as i64 {
            let dy = y + sy;
            if dy < 0 || dy >= self.height as i64 {
                continue;
            }
            for sx in 0..src.width as i64 {
                let dx = x + sx;
                if dx < 0 || dx >= self.width as i64 {
                    continue;
                }
                let s = src.px(sx as usize, sy as usize).to_vec();
                let m: u32 = match mask {
                    None => 255,
                    Some(mk) => {
                        if sx as usize >= mk.width || sy as usize >= mk.height {
                            continue;
                        }
                        let p = mk.px(sx as usize, sy as usize);
                        match mk.mode {
                            Mode::Rgba => u32::from(p[3]),
                            Mode::One => if p[0] != 0 { 255 } else { 0 }, // paste_mask_1: copy where set
                            _ => u32::from(p[0]),
                        }
                    }
                };
                let d = self.px_mut(dx as usize, dy as usize);
                for k in 0..b {
                    d[k] = match m {
                        255 => s[k],
                        0 => d[k],
                        m => div255(u32::from(d[k]) * (255 - m) + u32::from(s[k]) * m) as u8,
                    };
                }
            }
        }
    }

    /// `Image.composite(a, b, mask)`: `a` where the mask is white, `b` where it's black, blended between.
    pub fn composite(a: &Image, b: &Image, mask: &Image) -> Image {
        let mut out = b.clone();
        out.paste_mask(a, 0, 0, Some(mask));
        out
    }

    /// `transpose(method)`.
    pub fn transpose(&self, t: Transpose) -> Image {
        let (w, h) = (self.width, self.height);
        let (ow, oh) = match t {
            Transpose::FlipLeftRight | Transpose::FlipTopBottom | Transpose::Rotate180 => (w, h),
            _ => (h, w),
        };
        let mut out = Image::new(self.mode, ow, oh, &[]);
        for y in 0..h {
            for x in 0..w {
                let (nx, ny) = match t {
                    Transpose::FlipLeftRight => (w - 1 - x, y),
                    Transpose::FlipTopBottom => (x, h - 1 - y),
                    Transpose::Rotate90 => (y, w - 1 - x),
                    Transpose::Rotate180 => (w - 1 - x, h - 1 - y),
                    Transpose::Rotate270 => (h - 1 - y, x),
                    Transpose::Transpose => (y, x),
                    Transpose::Transverse => (h - 1 - y, w - 1 - x),
                };
                let p = self.px(x, y).to_vec();
                out.px_mut(nx, ny).copy_from_slice(&p);
            }
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transpose {
    FlipLeftRight,
    FlipTopBottom,
    Rotate90,
    Rotate180,
    Rotate270,
    Transpose,
    Transverse,
}
