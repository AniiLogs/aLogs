//! The Pillow (12.3) operations aLogs' screen readers use, ported from libImaging and Image.py so a reader gives the
//! same pixels, and so the same readings, as the Python meter. tests/pil.rs checks each against Pillow.

mod convert;
mod image;
pub mod resample;

pub use convert::{premultiply, unpremultiply};
pub use image::{Image, Mode, Transpose};
pub use resample::Filter;
