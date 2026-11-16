//! The Pillow (12.3) operations aLogs' screen readers use, ported from libImaging and Image.py so a reader gives the
//! same pixels, and so the same readings, as the Python meter. tests/pil.rs checks each against Pillow.
//! Known differences: `quant` (see there) and text drawing (not here: FreeType's rasteriser isn't ported).

mod bcn;
mod convert;
pub mod draw;
mod image;
pub mod ops;
pub mod quant;
pub mod resample;

pub use convert::{premultiply, unpremultiply};
pub use image::{Image, Mode, Transpose};
pub use resample::Filter;

impl Image {
    /// `Image.open` for a PNG (L, LA -> L+alpha dropped is not done: LA and palette PNGs become RGBA, as aLogs
    /// converts what it opens), or None.
    pub fn open_png(path: &std::path::Path) -> Option<Image> {
        let file = std::fs::File::open(path).ok()?;
        let mut dec = png::Decoder::new(std::io::BufReader::new(file));
        dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = dec.read_info().ok()?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).ok()?;
        buf.truncate(info.buffer_size());
        let (w, h) = (info.width as usize, info.height as usize);
        Some(match info.color_type {
            png::ColorType::Grayscale => Image::from_raw(Mode::L, w, h, buf),
            png::ColorType::Rgb => Image::from_raw(Mode::Rgb, w, h, buf),
            png::ColorType::Rgba => Image::from_raw(Mode::Rgba, w, h, buf),
            png::ColorType::GrayscaleAlpha => {
                let mut d = Vec::with_capacity(w * h * 4);
                for p in buf.chunks(2) {
                    d.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
                }
                Image::from_raw(Mode::Rgba, w, h, d)
            }
            png::ColorType::Indexed => return None,
        })
    }
}
