//! Text recognition with the OCR engine built into Windows 10/11. Port of `Ocr.cs` (OcrService).

use crate::cv::Mat;

pub struct OcrService {
    #[cfg(windows)]
    engine: Option<windows::Media::Ocr::OcrEngine>,
    pub language: String,
    /// The language wanted but not installed.
    pub missing_language: Option<String>,
}

impl OcrService {
    /// OCR for a language (BCP-47 tag) when Windows has it, else English, else the user's own languages.
    #[cfg(windows)]
    pub fn new(tag: Option<&str>) -> OcrService {
        use windows::Globalization::Language;
        use windows::Media::Ocr::OcrEngine;
        use windows::core::HSTRING;
        let create = |t: &str| -> Option<OcrEngine> {
            let l = Language::CreateLanguage(&HSTRING::from(t)).ok()?;
            if OcrEngine::IsLanguageSupported(&l).ok()? { OcrEngine::TryCreateFromLanguage(&l).ok() } else { None }
        };
        let mut engine = None;
        let mut missing_language = None;
        if let Some(t) = tag.filter(|t| *t != "en-US") {
            engine = create(t);
            if engine.is_none() {
                missing_language = Some(t.to_string());
            }
        }
        engine = engine.or_else(|| create("en-US")).or_else(|| OcrEngine::TryCreateFromUserProfileLanguages().ok());
        let language = engine
            .as_ref()
            .and_then(|e| e.RecognizerLanguage().ok())
            .and_then(|l| l.LanguageTag().ok())
            .map_or("none".to_string(), |s| s.to_string());
        OcrService { engine, language, missing_language }
    }

    #[cfg(not(windows))]
    pub fn new(tag: Option<&str>) -> OcrService {
        OcrService { language: "none".into(), missing_language: tag.map(str::to_string) }
    }

    pub fn available(&self) -> bool {
        #[cfg(windows)]
        return self.engine.is_some();
        #[cfg(not(windows))]
        false
    }

    /// Reads text from a BGR image; small crops are enlarged first (the engine is far better on text ~20 px tall).
    #[cfg(windows)]
    pub fn read(&self, bgr: &Mat) -> String {
        use crate::cv::{self, Interp};
        let Some(engine) = &self.engine else { return String::new() };
        if bgr.w == 0 || bgr.h == 0 {
            return String::new();
        }
        let f = if bgr.h < 80 { 2.5 } else { 1.0 };
        let big = cv::resize(bgr, 0, 0, f, f, Interp::Cubic);
        let big = if big.c == 1 { cv::gray2bgr(&big) } else { big };
        let bytes: Vec<u8> = big.data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
        read_bgra(engine, &bytes, big.w, big.h).unwrap_or_default()
    }

    #[cfg(not(windows))]
    pub fn read(&self, _bgr: &Mat) -> String {
        String::new()
    }
}

#[cfg(windows)]
fn read_bgra(engine: &windows::Media::Ocr::OcrEngine, bytes: &[u8], w: usize, h: usize) -> windows::core::Result<String> {
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
    use windows::Storage::Streams::DataWriter;
    let dw = DataWriter::new()?;
    dw.WriteBytes(bytes)?;
    let buf = dw.DetachBuffer()?;
    let sb = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(&buf, BitmapPixelFormat::Bgra8, w as i32, h as i32, BitmapAlphaMode::Premultiplied)?;
    let result = engine.RecognizeAsync(&sb)?.join()?;
    let lines: Vec<String> = result.Lines()?.into_iter().filter_map(|l| l.Text().ok()).map(|s| s.to_string()).collect();
    Ok(lines.join(" "))
}
