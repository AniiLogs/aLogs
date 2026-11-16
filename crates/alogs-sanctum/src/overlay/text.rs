//! Text as WPF's FormattedText set it for the overlay: Segoe UI SemiBold (read from Windows' own fonts at run time,
//! never shipped), measured, and turned into a path so labels can carry a dark outline.

use ab_glyph::{Font, FontArc, OutlineCurve, ScaleFont};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tiny_skia::{Path, PathBuilder};

fn font() -> Option<&'static FontArc> {
    static F: OnceLock<Option<FontArc>> = OnceLock::new();
    F.get_or_init(|| {
        let dir = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
        ["seguisb.ttf", "segoeui.ttf", "arial.ttf"].iter().find_map(|n| FontArc::try_from_vec(std::fs::read(dir.join(n)).ok()?).ok())
    })
    .as_ref()
}

/// Width and height (line height) of a text at an em size in px.
pub fn measure(text: &str, size: f64) -> (f64, f64) {
    let Some(f) = font() else { return (text.chars().count() as f64 * size * 0.55, size * 1.33) };
    let s = f.as_scaled(f.pt_to_px_scale(size as f32 * 0.75).unwrap_or(ab_glyph::PxScale::from(size as f32)));
    let mut w = 0.0f32;
    let mut prev = None;
    for c in text.chars() {
        let id = s.glyph_id(c);
        if let Some(p) = prev {
            w += s.kern(p, id);
        }
        w += s.h_advance(id);
        prev = Some(id);
    }
    (f64::from(w), f64::from(s.ascent() - s.descent() + s.line_gap()))
}

type Key = (String, u64, bool);

/// The text's glyphs as one path, its top-left at (x, y) when `above` is false; centred on x either way, its bottom
/// at y when `above` (as the overlay's labels were placed). Cached: labels rarely change.
pub fn label_path(text: &str, size: f64, above: bool) -> Option<(Path, f64, f64)> {
    static CACHE: OnceLock<Mutex<HashMap<Key, Option<(Path, f64, f64)>>>> = OnceLock::new();
    let key = (text.to_string(), size.to_bits(), above);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(p) = cache.lock().unwrap().get(&key) {
        return p.clone();
    }
    let (w, h) = measure(text, size);
    let made = path_at(text, size, -w / 2.0, if above { -h } else { 0.0 }).map(|p| (p, w, h));
    let mut c = cache.lock().unwrap();
    if c.len() >= 256 {
        c.clear(); // (radar distances keep changing)
    }
    c.insert(key, made.clone());
    made
}

/// The glyphs of a text with its line box's top-left at (x, y).
pub fn path_at(text: &str, size: f64, x: f64, y: f64) -> Option<Path> {
    let f = font()?;
    let s = f.as_scaled(f.pt_to_px_scale(size as f32 * 0.75)?);
    let k = s.h_scale_factor();
    let kv = s.v_scale_factor();
    let base = y as f32 + s.ascent();
    let mut pen = x as f32;
    let mut pb = PathBuilder::new();
    let mut prev = None;
    for c in text.chars() {
        let id = s.glyph_id(c);
        if let Some(p) = prev {
            pen += s.kern(p, id);
        }
        if let Some(o) = f.outline(id) {
            let mut last: Option<ab_glyph::Point> = None;
            let tr = |p: ab_glyph::Point| (pen + p.x * k, base - p.y * kv);
            for curve in &o.curves {
                let (start, end) = match curve {
                    OutlineCurve::Line(a, b) => (*a, *b),
                    OutlineCurve::Quad(a, _, b) => (*a, *b),
                    OutlineCurve::Cubic(a, _, _, b) => (*a, *b),
                };
                if last.is_none_or(|l| (l.x - start.x).abs() > 1e-3 || (l.y - start.y).abs() > 1e-3) {
                    if last.is_some() {
                        pb.close();
                    }
                    let (sx, sy) = tr(start);
                    pb.move_to(sx, sy);
                }
                match curve {
                    OutlineCurve::Line(_, b) => {
                        let (bx, by) = tr(*b);
                        pb.line_to(bx, by);
                    }
                    OutlineCurve::Quad(_, c1, b) => {
                        let ((cx, cy), (bx, by)) = (tr(*c1), tr(*b));
                        pb.quad_to(cx, cy, bx, by);
                    }
                    OutlineCurve::Cubic(_, c1, c2, b) => {
                        let ((ax, ay), (cx, cy), (bx, by)) = (tr(*c1), tr(*c2), tr(*b));
                        pb.cubic_to(ax, ay, cx, cy, bx, by);
                    }
                }
                last = Some(end);
            }
            if last.is_some() {
                pb.close();
            }
        }
        pen += s.h_advance(id);
        prev = Some(id);
    }
    pb.finish()
}
