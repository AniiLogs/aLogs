//! Windows' built-in OCR (Windows.Media.Ocr, en-US) for the boss meter, without PowerShell or image files, plus a
//! Korean text model for player names (Hangul names, which the English OCR can't see).
//!
//! Prints `READY` at start, then answers requests on stdin until it closes:
//!   request: b"OCR1", u32 LE image count, then per image u32 LE width, u32 LE height, width*height grey bytes
//!   answer:  one UTF-8 line per image, in order: the recognised lines joined with " / "
//!   request: b"OCR2", u32 LE image count, then per image u8 flags, u32 LE width, u32 LE height, grey bytes
//!   answer:  the same line per image; flag 1 (a player name) adds a tab and the Korean model's read as JSON
//!            (flag 2: the Korean model's read alone, after an empty English one):
//!            [[x, [[character, probability], ...]], ...] - each letter's spot (its centre, in the image's px) and its
//!            likeliest characters, best first. The meter checks those against the game's own fonts.
//!
//! The Korean model is PaddleOCR's korean_PP-OCRv5_mobile_rec (Apache-2.0, model/LICENSE-model.txt), converted to
//! ONNX and run with tract, built into this program: nothing to install or download on the player's PC.
#![windows_subsystem = "windows"]   // no console window

use std::collections::HashMap;
use std::io::{self, BufWriter, Read, Write};
use tract_onnx::prelude::*;
use windows::core::{Result, HSTRING};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

const KOREAN_MODEL: &[u8] = include_bytes!("../model/korean_rec.onnx");
const KOREAN_DICT: &str = include_str!("../model/korean_dict.txt");
const MODEL_HEIGHT: usize = 48;
const CANDIDATES: usize = 4;          // the likeliest characters sent per letter (plus the best Hangul and Latin ones)

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

fn is_hangul(c: char) -> bool {
    matches!(c, '\u{1100}'..='\u{11ff}' | '\u{3130}'..='\u{318f}' | '\u{ac00}'..='\u{d7a3}')
}

/// The Korean model, loaded the first time a name is read. The image is scaled to the model's 48 px height and padded
/// to a width in steps of 32, so a few optimised plans (one per width) cover every name box.
struct Korean {
    model: Option<InferenceModel>,
    plans: HashMap<usize, TypedRunnableModel<TypedModel>>,
    chars: Vec<String>,   // index 0 is CTC's blank, the last a space
}

impl Korean {
    fn new() -> Korean {
        let mut chars = vec![String::new()];
        chars.extend(KOREAN_DICT.lines().map(str::to_string));
        chars.push(" ".to_string());
        Korean { model: None, plans: HashMap::new(), chars }
    }

    fn plan(&mut self, width: usize) -> TractResult<&TypedRunnableModel<TypedModel>> {
        if !self.plans.contains_key(&width) {
            if self.model.is_none() { self.model = Some(tract_onnx::onnx().model_for_read(&mut &KOREAN_MODEL[..])?); }
            let plan = self.model.clone().unwrap()
                .with_input_fact(0, f32::fact([1, 3, MODEL_HEIGHT, width]).into())?
                .into_optimized()?.into_runnable()?;
            self.plans.insert(width, plan);
        }
        Ok(&self.plans[&width])
    }

    /// Each letter the model finds: (its centre x in the image, [(character, probability)] best first).
    fn read(&mut self, w: usize, h: usize, grey: &[u8]) -> TractResult<Vec<(f32, Vec<(String, f32)>)>> {
        if w == 0 || h == 0 { return Ok(Vec::new()); }
        let nw = ((w as f32 * MODEL_HEIGHT as f32 / h as f32).round() as usize).clamp(16, 3200);
        let width = (nw + 32).max(96).div_ceil(32) * 32;          // a little room after the name, in steps of 32
        // bilinear scaling to nw x 48, grey in all three channels, (v / 255 - 0.5) / 0.5; the padding stays 0
        let mut input = tract_ndarray::Array4::<f32>::zeros((1, 3, MODEL_HEIGHT, width));
        let (sx, sy) = (w as f32 / nw as f32, h as f32 / MODEL_HEIGHT as f32);
        for y in 0..MODEL_HEIGHT {
            let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (h - 1) as f32);
            let (y0, ty) = (fy.floor() as usize, fy.fract());
            let y1 = (y0 + 1).min(h - 1);
            for x in 0..nw {
                let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (w - 1) as f32);
                let (x0, tx) = (fx.floor() as usize, fx.fract());
                let x1 = (x0 + 1).min(w - 1);
                let p = |xx: usize, yy: usize| grey[yy * w + xx] as f32;
                let v = (p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx) * (1.0 - ty) + (p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx) * ty;
                let n = (v / 255.0 - 0.5) / 0.5;
                for c in 0..3 { input[[0, c, y, x]] = n; }
            }
        }
        let out = self.plan(width)?.run(tvec!(Tensor::from(input).into()))?;
        let probs = out[0].to_array_view::<f32>()?;
        let (frames, classes) = (probs.shape()[1], probs.shape()[2].min(self.chars.len()));
        let step = width as f32 / frames as f32 * sx;            // one frame's width in the image's own px
        let mut letters = Vec::new();
        let mut prev = 0usize;
        for t in 0..frames {
            let row: Vec<f32> = (0..classes).map(|k| probs[[0, t, k]]).collect();
            let best = (0..classes).max_by(|&a, &b| row[a].total_cmp(&row[b])).unwrap_or(0);
            if best != 0 && best != prev {
                let mut order: Vec<usize> = (1..classes).collect();
                order.sort_by(|&a, &b| row[b].total_cmp(&row[a]));
                let mut pick: Vec<usize> = order.iter().copied().take(CANDIDATES).collect();
                // the best Hangul and the best Latin letter or digit too: the game's font tells such twins apart (7 and ㄱ)
                let first = |f: &dyn Fn(char) -> bool| order.iter().copied().find(|&k| self.chars[k].chars().next().is_some_and(|c| f(c)));
                for k in [first(&is_hangul), first(&|c: char| c.is_ascii_alphanumeric())].into_iter().flatten() {
                    if !pick.contains(&k) { pick.push(k); }
                }
                let x = (t as f32 + 0.5) * step;
                letters.push((x, pick.into_iter().map(|k| (self.chars[k].clone(), row[k])).collect()));
            }
            prev = best;
        }
        Ok(letters)
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn letters_json(letters: &[(f32, Vec<(String, f32)>)]) -> String {
    let parts: Vec<String> = letters.iter().map(|(x, picks)| {
        let p: Vec<String> = picks.iter().map(|(c, v)| format!("[{},{:.4}]", json_str(c), v)).collect();
        format!("[{:.1},[{}]]", x, p.join(","))
    }).collect();
    format!("[{}]", parts.join(","))
}

fn main() -> Result<()> {
    let engine = OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&HSTRING::from("en-US"))?)?;
    let max_dim = OcrEngine::MaxImageDimension()?;
    let mut korean = Korean::new();
    let mut input = io::stdin().lock();
    let mut out = BufWriter::new(io::stdout().lock());
    if writeln!(out, "READY").and_then(|_| out.flush()).is_err() { return Ok(()); }
    let mut grey = Vec::new();
    loop {
        // any read error (stdin closed, or a broken request) ends the helper
        let mut magic = [0u8; 4];
        if input.read_exact(&mut magic).is_err() { return Ok(()); }
        let flagged = match &magic { b"OCR1" => false, b"OCR2" => true, _ => return Ok(()) };
        let Ok(count) = read_u32(&mut input) else { return Ok(()) };
        for _ in 0..count {
            let mut flags = [0u8; 1];
            if flagged && input.read_exact(&mut flags).is_err() { return Ok(()); }
            let (Ok(w), Ok(h)) = (read_u32(&mut input), read_u32(&mut input)) else { return Ok(()) };
            grey.resize(w as usize * h as usize, 0);
            if input.read_exact(&mut grey).is_err() { return Ok(()); }
            // flag 2: the Korean model only (the meter sends the English OCR the box itself, the model a tidied copy)
            let mut line = if flags[0] & 2 != 0 { String::new() } else { recognise(&engine, max_dim, w, h, &grey) };
            if flags[0] & 3 != 0 {
                let letters = korean.read(w as usize, h as usize, &grey).unwrap_or_default();
                line = format!("{line}\t{}", letters_json(&letters));
            }
            if writeln!(out, "{line}").is_err() { return Ok(()); }
        }
        if out.flush().is_err() { return Ok(()); }
    }
}
