//! Windows' built-in OCR (Windows.Media.Ocr, en-US) for the boss meter, without PowerShell or image files, plus two
//! text models for player names: a Latin one (names read far better than Windows OCR does) and a Korean one (Hangul,
//! which the English OCR can't see).
//!
//! A PC without en-US OCR uses another English it has, else its user's own languages; with no OCR language at all
//! every Windows OCR line is empty, and the name models still read.
//!
//! Prints `READY` at start, then answers requests on stdin until it closes:
//!   request: b"OCR1", u32 LE image count, then per image u32 LE width, u32 LE height, width*height grey bytes
//!   answer:  one UTF-8 line per image, in order: the recognised lines joined with " / "
//!   request: b"OCR2", u32 LE image count, then per image u8 flags, u32 LE width, u32 LE height, grey bytes
//!   answer:  per image, the English OCR's line (empty with flag 2), then with flag 1 or 4 two tab-separated fields:
//!            the Korean model's read as JSON with flag 1 - [[x, [[character, probability], ...]], ...]: each letter's
//!            spot (its centre, in the image's px) and its likeliest characters, best first, which the meter checks
//!            against the game's own fonts - and the Latin model's text with flag 4.
//!
//! Both models are PaddleOCR's (Apache-2.0, model/LICENSE-model.txt): korean_PP-OCRv5_mobile_rec, converted to ONNX,
//! and PP-OCRv6_small_rec, as PaddlePaddle publishes it in ONNX. On 346 player names from real result, Battle Data
//! and Party screens the Latin one read 90% exactly, Windows OCR 42% (model/README.md). They run with tract, built
//! into this program: nothing to install or download on the player's PC.
#![windows_subsystem = "windows"]   // no console window

use std::io::{self, BufWriter, Read, Write};
use tract_onnx::prelude::*;
use windows::core::{Result, HSTRING};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

const KOREAN_MODEL: &[u8] = include_bytes!("../model/korean_rec.onnx");
const KOREAN_DICT: &str = include_str!("../model/korean_dict.txt");
const LATIN_MODEL: &[u8] = include_bytes!("../model/latin_rec.onnx");
const LATIN_DICT: &str = include_str!("../model/latin_dict.txt");
const MODEL_HEIGHT: usize = 48;
/// The input widths the models run at: a box is padded to the first one that fits (past the last, to steps of 32).
/// Each width is an optimised plan of its own (15-22 MB, ~150 ms to build), and the Korean model's crops vary with
/// the name's length: on the 419 labelled names they needed 10 widths of 96-384 px in steps of 32, now 4.
const WIDTHS: [usize; 10] = [128, 192, 256, 384, 512, 768, 1024, 1536, 2048, 3072];
const PLANS: usize = 4;               // plans kept per model, the most recently used (19 widths took 740 MB, now ~200)
const CANDIDATES: usize = 4;          // the likeliest characters sent per letter (plus the best Hangul and Latin ones)

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

/// Windows OCR in English, the game's language: en-US, else another English the PC has, else one of the user's own
/// languages. None when the PC has none of them (TryCreate* fails: Windows gives null).
fn windows_ocr() -> Option<OcrEngine> {
    let create = |tag: &HSTRING| Language::CreateLanguage(tag).and_then(|l| OcrEngine::TryCreateFromLanguage(&l)).ok();
    create(&HSTRING::from("en-US"))
        .or_else(|| OcrEngine::AvailableRecognizerLanguages().ok()?.into_iter()
            .filter_map(|l| l.LanguageTag().ok())
            .find(|t| t.to_string().to_ascii_lowercase().starts_with("en-"))
            .and_then(|t| create(&t)))
        .or_else(|| OcrEngine::TryCreateFromUserProfileLanguages().ok())
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

/// A box taller than the model's 48 px, shrunk to nw x 48 (row by row) as PIL's bilinear resize does it: each pixel
/// a weighted average of all those it covers. Prepared name boxes are about 3x too tall, and sampling only the 2x2
/// nearest pixels dropped detail: of the 419 labelled names, 357 read exactly through the meter that way, 365 shrunk
/// like this (366 with WIDTHS; grey crops to the Latin model 384 either way, 386 with WIDTHS), no kind of box worse.
fn shrink(w: usize, h: usize, grey: &[u8], nw: usize) -> Vec<f32> {
    let (across, down) = (weights(w, nw), weights(h, MODEL_HEIGHT));
    let mut rows = vec![0f32; h * nw];
    for y in 0..h {
        for (x, (lo, k)) in across.iter().enumerate() {
            rows[y * nw + x] = k.iter().enumerate().map(|(i, v)| v * grey[y * w + lo + i] as f32).sum();
        }
    }
    let mut out = vec![0f32; MODEL_HEIGHT * nw];
    for (y, (lo, k)) in down.iter().enumerate() {
        for x in 0..nw { out[y * nw + x] = k.iter().enumerate().map(|(i, v)| v * rows[(lo + i) * nw + x]).sum(); }
    }
    out
}

/// Scaling `from` px to `to`: per new pixel, the first old one it takes and their weights (a triangle as wide as
/// the shrink, as PIL's bilinear filter, summing to 1).
fn weights(from: usize, to: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = from as f32 / to as f32;
    let support = scale.max(1.0);
    (0..to).map(|i| {
        let centre = (i as f32 + 0.5) * scale;
        let lo = ((centre - support + 0.5) as isize).max(0) as usize;
        let hi = ((centre + support + 0.5) as isize).min(from as isize) as usize;
        let mut k: Vec<f32> = (lo..hi).map(|x| (1.0 - ((x as f32 - centre + 0.5) / support).abs()).max(0.0)).collect();
        let sum: f32 = k.iter().sum();
        if sum > 0.0 { for v in &mut k { *v /= sum; } }
        (lo, k)
    }).collect()
}

/// A text-line model (CTC), loaded the first time it's needed. The image is scaled to the model's 48 px height and
/// padded to one of a few widths (WIDTHS), each an optimised plan; only the last few used are kept, so its memory
/// stays bounded however many sizes come in.
struct Recognizer {
    bytes: &'static [u8],
    model: Option<InferenceModel>,
    plans: Vec<(usize, TypedRunnableModel<TypedModel>)>,   // (width, plan), the most recently used last
    chars: Vec<String>,   // index 0 is CTC's blank, the last a space
}

/// One letter as read: its centre x in the image and [(character, probability)], best first.
type Letter = (f32, Vec<(String, f32)>);

impl Recognizer {
    fn new(bytes: &'static [u8], dict: &str) -> Recognizer {
        let mut chars = vec![String::new()];
        chars.extend(dict.lines().map(str::to_string));
        chars.push(" ".to_string());
        Recognizer { bytes, model: None, plans: Vec::new(), chars }
    }

    fn plan(&mut self, width: usize) -> TractResult<&TypedRunnableModel<TypedModel>> {
        if let Some(i) = self.plans.iter().position(|(w, _)| *w == width) {
            let used = self.plans.remove(i);
            self.plans.push(used);
        } else {
            if self.model.is_none() { self.model = Some(tract_onnx::onnx().model_for_read(&mut &self.bytes[..])?); }
            let plan = self.model.clone().unwrap()
                .with_input_fact(0, f32::fact([1, 3, MODEL_HEIGHT, width]).into())?
                .into_optimized()?.into_runnable()?;
            if self.plans.len() >= PLANS { self.plans.remove(0); }    // the least recently used goes
            self.plans.push((width, plan));
        }
        Ok(&self.plans.last().unwrap().1)
    }

    /// Each letter the model finds, with `extra` likeliest characters beyond the best (0: the best alone).
    fn letters(&mut self, w: usize, h: usize, grey: &[u8], extra: usize) -> TractResult<Vec<Letter>> {
        if w == 0 || h == 0 { return Ok(Vec::new()); }
        let nw = ((w as f32 * MODEL_HEIGHT as f32 / h as f32).round() as usize).clamp(16, 3200);
        let room = nw + 32;                                         // a little room after the name
        let width = WIDTHS.iter().copied().find(|&x| x >= room).unwrap_or(room.div_ceil(32) * 32);
        // scaled to nw x 48 (shrunk smoothly when taller, else bilinear), grey in all three channels,
        // (v / 255 - 0.5) / 0.5; the padding stays 0
        let mut input = tract_ndarray::Array4::<f32>::zeros((1, 3, MODEL_HEIGHT, width));
        let (sx, sy) = (w as f32 / nw as f32, h as f32 / MODEL_HEIGHT as f32);
        let shrunk = (h > MODEL_HEIGHT).then(|| shrink(w, h, grey, nw));
        for y in 0..MODEL_HEIGHT {
            let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (h - 1) as f32);
            let (y0, ty) = (fy.floor() as usize, fy.fract());
            let y1 = (y0 + 1).min(h - 1);
            for x in 0..nw {
                let v = match &shrunk {
                    Some(small) => small[y * nw + x],
                    None => {
                        let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (w - 1) as f32);
                        let (x0, tx) = (fx.floor() as usize, fx.fract());
                        let x1 = (x0 + 1).min(w - 1);
                        let p = |xx: usize, yy: usize| grey[yy * w + xx] as f32;
                        (p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx) * (1.0 - ty) + (p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx) * ty
                    }
                };
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
                let pick: Vec<usize> = if extra == 0 { vec![best] } else {
                    let mut order: Vec<usize> = (1..classes).collect();
                    order.sort_by(|&a, &b| row[b].total_cmp(&row[a]));
                    let mut pick: Vec<usize> = order.iter().copied().take(extra).collect();
                    // the best Hangul and the best Latin letter or digit too: the game's font tells such twins apart (7 and ㄱ)
                    let first = |f: &dyn Fn(char) -> bool| order.iter().copied().find(|&k| self.chars[k].chars().next().is_some_and(|c| f(c)));
                    for k in [first(&is_hangul), first(&|c: char| c.is_ascii_alphanumeric())].into_iter().flatten() {
                        if !pick.contains(&k) { pick.push(k); }
                    }
                    pick
                };
                let x = (t as f32 + 0.5) * step;
                letters.push((x, pick.into_iter().map(|k| (self.chars[k].clone(), row[k])).collect()));
            }
            prev = best;
        }
        Ok(letters)
    }

    /// The line as the model reads it (each letter's best character).
    fn text(&mut self, w: usize, h: usize, grey: &[u8]) -> String {
        self.letters(w, h, grey, 0).unwrap_or_default().into_iter()
            .filter_map(|(_, picks)| picks.into_iter().next().map(|(c, _)| c)).collect::<String>()
            .replace(['\t', '\r', '\n'], " ").trim().to_string()
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

fn letters_json(letters: &[Letter]) -> String {
    let parts: Vec<String> = letters.iter().map(|(x, picks)| {
        let p: Vec<String> = picks.iter().map(|(c, v)| format!("[{},{:.4}]", json_str(c), v)).collect();
        format!("[{:.1},[{}]]", x, p.join(","))
    }).collect();
    format!("[{}]", parts.join(","))
}

fn main() -> Result<()> {
    let engine = windows_ocr();
    let max_dim = OcrEngine::MaxImageDimension().unwrap_or(0);
    let mut korean = Recognizer::new(KOREAN_MODEL, KOREAN_DICT);
    let mut latin = Recognizer::new(LATIN_MODEL, LATIN_DICT);
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
            let f = flags[0];
            // flag 2: no English OCR (the meter sends it the box itself, the models a copy of their own)
            let mut line = match &engine {
                Some(engine) if f & 2 == 0 => recognise(engine, max_dim, w, h, &grey),
                _ => String::new(),
            };
            if f & 5 != 0 {
                let k = if f & 1 != 0 { letters_json(&korean.letters(w as usize, h as usize, &grey, CANDIDATES).unwrap_or_default()) } else { String::new() };
                let l = if f & 4 != 0 { latin.text(w as usize, h as usize, &grey) } else { String::new() };
                line = format!("{line}\t{k}\t{l}");
            }
            if writeln!(out, "{line}").is_err() { return Ok(()); }
        }
        if out.flush().is_err() { return Ok(()); }
    }
}
