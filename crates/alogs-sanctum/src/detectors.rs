//! Placing the game's 1080p UI regions on a client of any size, and reading the party screen's goal line for the
//! difficulty. Port of `Detectors.cs` (Regions, DifficultyDetector).

use crate::cv::{self, Mat, Thresh};
use crate::lang;
use crate::ocr::OcrService;
pub use crate::settings::RegionF;
use crate::vision::hud;
use regex::Regex;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopCenter,
    TopRight,
    TopLeft,
}

/// A 1080p region on a client of any size: the game's UI scales with its UI area and stays anchored to its edges.
/// (x, y, w, h), cut to the client.
pub fn to_client(r: RegionF, a: Anchor, w: usize, h: usize) -> (usize, usize, usize, usize) {
    let (hx, hy, hw, hh) = hud(w, h);
    let s = hh as f64 / 1080.0;
    let x = match a {
        Anchor::TopCenter => hx as f64 + hw as f64 / 2.0 + (r.x - 960.0) * s,
        Anchor::TopRight => (hx + hw) as f64 - (1920.0 - r.x) * s,
        Anchor::TopLeft => hx as f64 + r.x * s,
    };
    let rd = |v: f64| v.round() as i64;
    let (x, y, rw, rh) = (rd(x), rd(hy as f64 + r.y * s), rd(r.w * s), rd(r.h * s));
    let (x0, y0) = (x.max(0), y.max(0));
    let (x1, y1) = ((x + rw).min(w as i64), (y + rh).min(h as i64));
    if x1 <= x0 || y1 <= y0 { (0, 0, 0, 0) } else { (x0 as usize, y0 as usize, (x1 - x0) as usize, (y1 - y0) as usize) }
}

/// Reads the party screen's goal line ("Operation: Egg Heist Team Mode Nightmare").
#[derive(Default)]
pub struct DifficultyDetector {
    pub last_text: String,
    last_gray: Option<Mat>,
    last_result: Option<String>,
}

impl DifficultyDetector {
    /// A difficulty when the frame shows the Egg Heist team-mode party screen. The frame may be a band from the top
    /// of the window: the region is placed by the client's size, not the frame's.
    pub fn detect(&mut self, ocr: &OcrService, frame: &Mat, region: RegionF, client_w: usize, client_h: usize) -> Option<String> {
        let cw = if client_w > 0 { client_w } else { frame.w };
        let ch = if client_h > 0 { client_h } else { frame.h };
        let (x, y, w, h) = to_client(region, Anchor::TopCenter, cw, ch);
        let (w, h) = (w.min(frame.w.saturating_sub(x)), h.min(frame.h.saturating_sub(y)));
        if w < 20 || h < 10 {
            return None;
        }
        self.detect_goal(ocr, &frame.roi(x, y, w, h))
    }

    /// The goal line's own crop. Light text on a dark banner: only bright pixels are read. The same picture as last
    /// time gives the same answer without OCR.
    pub fn detect_goal(&mut self, ocr: &OcrService, crop: &Mat) -> Option<String> {
        if crop.w < 20 || crop.h < 10 {
            return None;
        }
        let gray = cv::bgr2gray(crop);
        if let Some(lg) = &self.last_gray
            && (lg.w, lg.h) == (gray.w, gray.h)
        {
            let sum: u64 = gray.data.iter().zip(&lg.data).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
            if (sum as f64) / (gray.data.len() as f64) < 1.0 {
                return self.last_result.clone();
            }
        }
        self.last_gray = Some(gray.clone());
        self.last_result = None;
        let bin = cv::threshold(&gray, 150.0, 255.0, Thresh::BinaryInv);
        let mut text = ocr.read(&cv::gray2bgr(&bin));
        if text.trim().is_empty() {
            text = ocr.read(crop);
        }
        self.last_result = parse_lang(&text, &lang::game_code());
        self.last_text = text;
        self.last_result.clone()
    }
}

fn is_punct(c: char) -> bool {
    (c.is_ascii_punctuation() && !"$+<=>^`|~".contains(c))
        || matches!(c, '\u{2010}'..='\u{2027}' | '\u{3001}'..='\u{3003}' | '\u{3008}'..='\u{3011}' | '\u{FF01}'..='\u{FF0F}'
            | '\u{FF1A}' | '\u{FF1B}' | '\u{FF1F}' | '\u{00A1}' | '\u{00BF}' | '\u{00AB}' | '\u{00BB}' | '\u{30FB}')
}

/// Accent-, case-, space- and punctuation-free form for comparing OCR text with the game's wording.
pub fn norm(s: &str) -> String {
    s.nfd()
        .filter(|&c| !unicode_normalization::char::is_combining_mark(c) && !c.is_whitespace() && !is_punct(c) && c != '\u{200b}')
        .flat_map(char::to_lowercase)
        .collect()
}

/// The party line in the game's language: its Egg Heist team-mode title and a difficulty. English (and the game
/// still showing English) keeps the OCR-tolerant patterns.
pub fn parse_lang(text: &str, game_lang: &str) -> Option<String> {
    if game_lang == "en" {
        return parse(text);
    }
    let t = norm(text);
    let mut titles = lang::variants(game_lang, "EggHeistTeamMode");
    titles.extend(lang::variants(game_lang, "EggHeist"));
    if !titles.iter().map(|v| norm(v)).filter(|v| v.chars().count() >= 2).any(|v| t.contains(&v)) {
        return parse(text);
    }
    for d in ["Chaos", "Nightmare", "Hard", "Normal"] {
        if lang::variants(game_lang, d).iter().map(|v| norm(v)).filter(|v| !v.is_empty()).any(|v| t.contains(&v)) {
            return Some(d.into());
        }
    }
    parse(text)
}

type Patterns = (Regex, Regex, Vec<(Regex, &'static str)>);

pub fn parse(text: &str) -> Option<String> {
    static RE: OnceLock<Patterns> = OnceLock::new();
    let (egg, team, words) = RE.get_or_init(|| {
        (
            Regex::new(r"(?i)egg\s*he[il1]st").unwrap(),
            Regex::new(r"(?i)team").unwrap(),
            vec![
                (Regex::new(r"(?i)chaos|cha0s").unwrap(), "Chaos"),
                (Regex::new(r"(?i)nightm|n[il1]ghtmare").unwrap(), "Nightmare"),
                (Regex::new(r"(?i)\bhard\b").unwrap(), "Hard"),
                (Regex::new(r"(?i)\bnormal\b|beginner").unwrap(), "Normal"),
            ],
        )
    });
    let t = text.replace('\n', " ");
    if !egg.is_match(&t) || !team.is_match(&t) {
        return None;
    }
    words.iter().find(|(re, _)| re.is_match(&t)).map(|(_, n)| n.to_string())
}
