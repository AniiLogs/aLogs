//! Windows' built-in OCR (Windows.Media.Ocr, en-US) for the boss meter, without PowerShell or image files.
//!
//! Prints `READY` at start, then answers requests on stdin until it closes:
//!   request: b"OCR1", u32 LE image count, then per image u32 LE width, u32 LE height, width*height grey bytes
//!   answer:  one UTF-8 line per image, in order: the recognised lines joined with " / "
#![windows_subsystem = "windows"]   // no console window

use std::io::{self, BufWriter, Read, Write};
use windows::core::{Result, HSTRING};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

/// The text of one grey image, lines joined with " / "; empty when it can't be read.
fn recognise(engine: &OcrEngine, max_dim: u32, w: u32, h: u32, grey: &[u8]) -> String {
    if w == 0 || h == 0 || w > max_dim || h > max_dim { return String::new(); }
    let run = || -> Result<String> {
        // Gray8, as the PNG decoder hands the PowerShell helper the same picture
        let writer = DataWriter::new()?;
        writer.WriteBytes(grey)?;
        let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
            &writer.DetachBuffer()?, BitmapPixelFormat::Gray8, w as i32, h as i32, BitmapAlphaMode::Ignore)?;
        let result = engine.RecognizeAsync(&bitmap)?.join()?;
        let mut lines = Vec::new();
        for line in result.Lines()? { lines.push(line.Text()?.to_string()); }
        Ok(lines.join(" / "))
    };
    run().unwrap_or_default().replace(['\t', '\r', '\n'], " ")
}

fn main() -> Result<()> {
    let engine = OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&HSTRING::from("en-US"))?)?;
    let max_dim = OcrEngine::MaxImageDimension()?;
    let mut input = io::stdin().lock();
    let mut out = BufWriter::new(io::stdout().lock());
    if writeln!(out, "READY").and_then(|_| out.flush()).is_err() { return Ok(()); }
    let mut grey = Vec::new();
    loop {
        // any read error (stdin closed, or a broken request) ends the helper
        let mut magic = [0u8; 4];
        if input.read_exact(&mut magic).is_err() || &magic != b"OCR1" { return Ok(()); }
        let Ok(count) = read_u32(&mut input) else { return Ok(()) };
        for _ in 0..count {
            let (Ok(w), Ok(h)) = (read_u32(&mut input), read_u32(&mut input)) else { return Ok(()) };
            grey.resize(w as usize * h as usize, 0);
            if input.read_exact(&mut grey).is_err() { return Ok(()); }
            let text = recognise(&engine, max_dim, w, h, &grey);
            if writeln!(out, "{text}").is_err() { return Ok(()); }
        }
        if out.flush().is_err() { return Ok(()); }
    }
}
