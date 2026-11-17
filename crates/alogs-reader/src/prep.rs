//! Text preparation for OCR: game text cut out of the picture as black on white at one size for every resolution.
//! Port of the `prep_*` half of aLogs B16 `bossbar/reader.py`.

use alogs_pil::ops::{autocontrast, chop_invert, darker, expand, invert, lighter, multiply, subtract};
use alogs_pil::{Filter, Image, Mode};

type BoxF = (f64, f64, f64, f64);

fn crop_int(img: &Image, b: BoxF) -> Image {
    img.crop_i(b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64)
}

fn scaled(c: &Image, k: f64, f: Filter) -> Image {
    c.resize(((c.width as f64 * k) as usize).max(1), ((c.height as f64 * k) as usize).max(1), Some(f))
}

fn split3(c: &Image) -> (Image, Image, Image) {
    let mut s = c.split().into_iter();
    (s.next().unwrap(), s.next().unwrap(), s.next().unwrap())
}

fn thr(t: u8) -> impl Fn(u8) -> f64 {
    move |v| if v > t { 255.0 } else { 0.0 }
}

/// White game text -> black on white, padded, at the same text size for any resolution.
pub fn prep_text(img: &Image, b: BoxF, scale: f64, threshold: u8, size: f64, pad: usize) -> Image {
    let c = crop_int(img, b).convert(Mode::L);
    let c = scaled(&c, size / scale, Filter::Bicubic);
    let c = invert(&c.point(thr(threshold)));
    expand(&c, pad, &[255])
}

/// Dark text on a yellow pill (Egg Heist's "+40") as black on white; None with no pill there.
pub fn prep_pill(img: &Image, b: BoxF, scale: f64, size: f64, pad: usize) -> Option<Image> {
    let c = crop_int(img, b).convert(Mode::Rgb);
    let (r, g, bb) = split3(&c);
    let pill = multiply(&multiply(&r.point(thr(220)), &g.point(thr(190))), &bb.point(|v| if v < 150 { 255.0 } else { 0.0 }));
    let at = pill.getbbox()?;
    if ((at.2 - at.0) as f64) < 20.0 * scale || ((at.3 - at.1) as f64) < 8.0 * scale {
        return None;
    }
    let (x, y) = ((12.0 * scale) as i64, (4.0 * scale) as i64);
    let c = c.crop_i(at.0 as i64 + x, at.1 as i64 + y, at.2 as i64 - x, at.3 as i64 - y).convert(Mode::L);
    let c = scaled(&c, size / scale, Filter::Bicubic).point(|v| if v < 120 { 0.0 } else { 255.0 });
    Some(expand(&c, pad, &[255]))
}

/// Red text: red channel over 150 and at least 60 over green and blue.
fn red_mask(c: &Image) -> Image {
    let (r, g, b) = split3(c);
    let redness = subtract(&r, &lighter(&g, &b), 1.0, 0).point(|v| if v >= 60 { 255.0 } else { 0.0 });
    multiply(&r.point(thr(150)), &redness)
}

/// Red text in the left half of the box (at least a level's worth of red pixels).
pub fn has_red(img: &Image, b: BoxF, scale: f64) -> bool {
    let c = img.crop_i(b.0 as i64, b.1 as i64, ((b.0 + b.2) / 2.0) as i64, b.3 as i64).convert(Mode::Rgb);
    red_mask(&c).histogram(None)[255] as f64 >= 300.0 * scale * scale
}

/// `prep_text` with red text kept too (the boss title's red "Lv.65").
pub fn prep_red(img: &Image, b: BoxF, scale: f64, threshold: u8, size: f64, pad: usize) -> Image {
    let c = crop_int(img, b).convert(Mode::Rgb);
    let c = scaled(&c, size / scale, Filter::Bicubic);
    let text = lighter(&c.convert(Mode::L).point(thr(threshold)), &red_mask(&c));
    expand(&invert(&text), pad, &[255])
}

/// Only near-white, unsaturated pixels count as text.
pub fn prep_white(img: &Image, b: BoxF, scale: f64, size: f64) -> Image {
    let c = crop_int(img, b).convert(Mode::Rgb);
    let c = scaled(&c, size / scale, Filter::Bicubic);
    let (r, g, bb) = split3(&c);
    let low = darker(&darker(&r, &g), &bb);
    let high = lighter(&lighter(&r, &g), &bb);
    let text = multiply(&low.point(thr(165)), &subtract(&high, &low, 1.0, 0).point(|v| if v < 50 { 255.0 } else { 0.0 }));
    chop_invert(&text)
}

/// Dark grey text on a light card -> black on white.
pub fn prep_grey(img: &Image, b: BoxF, scale: f64, size: f64, pad: usize) -> Image {
    let c = crop_int(img, b).convert(Mode::Rgb);
    let c = scaled(&c, size / scale, Filter::Bicubic);
    let (r, g, bb) = split3(&c);
    let high = lighter(&lighter(&r, &g), &bb);
    let low = darker(&darker(&r, &g), &bb);
    let text = multiply(
        &high.point(|v| if v < 135 { 255.0 } else { 0.0 }),
        &subtract(&high, &low, 1.0, 0).point(|v| if v < 45 { 255.0 } else { 0.0 }),
    );
    expand(&invert(&text), pad, &[255])
}

/// Any bright text on a darker background: brightness, stretched, as dark text on white.
pub fn prep_bright(img: &Image, b: BoxF, scale: f64, size: f64) -> Image {
    let c = crop_int(img, b).convert(Mode::L);
    let c = scaled(&c, size / scale, Filter::Lanczos);
    expand(&invert(&autocontrast(&c, (2.0, 2.0))), 20, &[255])
}

/// A lone number printed after a drawn label, so it is read as part of a line ("Defeats: 1").
pub fn prep_labelled(img: &Image, b: BoxF, scale: f64, label: &str, size: f64) -> Image {
    let c = crop_int(img, b).convert(Mode::L);
    let c = scaled(&c, size / scale, Filter::Lanczos);
    let c = invert(&autocontrast(&c, (2.0, 2.0)));
    with_label(&c, label)
}

/// `prep_labelled` for dark text on a light background: not inverted.
pub fn prep_labelled_dark(img: &Image, b: BoxF, scale: f64, label: &str, size: f64) -> Image {
    let c = crop_int(img, b).convert(Mode::L);
    let c = autocontrast(&scaled(&c, size / scale, Filter::Lanczos), (2.0, 2.0));
    with_label(&c, label)
}

fn with_label(c: &Image, label: &str) -> Image {
    let px = ((c.height as f64 * 0.6) as usize).max(12) as f32;
    let font = crate::text::Font::arial_bold();
    let w = font.as_ref().map_or(label.len() as f32 * px * 0.55, |f| f.length(label, px)) as usize + 60;
    let mut out = Image::new(Mode::L, c.width + w + 20, c.height + 40, &[255]);
    if let Some(f) = &font {
        f.draw(&mut out, 20.0, 20.0 + c.height as f32 * 0.2, label, px, 0);
    }
    out.paste(c, w as i64, 20);
    out
}

/// Very short text (e.g. "x16"): repeated side by side, which Windows OCR reads where it skips one copy.
pub fn prep_short(img: &Image, b: BoxF, scale: f64, copies: usize) -> Image {
    let c = prep_white(img, b, scale, 4.0);
    let gap = 20;
    let mut out = Image::new(Mode::L, c.width * copies + gap * (copies + 1), c.height + 2 * gap, &[255]);
    for i in 0..copies {
        out.paste(&c, (gap + i * (c.width + gap)) as i64, gap as i64);
    }
    out
}

/// A name box as it is on screen, in grey, for the Latin text model (shrunk smoothly to 48 px when taller).
pub fn grey_crop(img: &Image, b: BoxF, height: usize) -> Image {
    let c = crop_int(img, b).convert(Mode::L);
    if c.height > height {
        let w = ((c.width as f64 * height as f64 / c.height as f64).round_ties_even() as usize).max(1);
        c.resize(w, height, Some(Filter::Lanczos))
    } else {
        c
    }
}

/// What `prep_labelled` read after its drawn label: "Player name: 0" -> "0".
pub fn after_label(text: Option<&str>, label: &str) -> String {
    let mut t = text.unwrap_or("").trim().to_string();
    let low = t.to_lowercase();
    let key = label.to_lowercase();
    let key = key.trim_end_matches(':');
    if let Some(i) = low.find(key) {
        // positions in the lower-cased text map back one to one for the ASCII labels aLogs uses
        t = t.chars().skip(low[..i].chars().count() + key.chars().count()).collect();
    }
    t.trim_matches(|c| " /:;|.".contains(c)).trim().to_string()
}

/// Width / height of the bright text in a box.
pub fn ink_aspect(img: &Image, b: BoxF, threshold: u8) -> Option<f64> {
    let bb = crop_int(img, b).convert(Mode::L).point(thr(threshold)).getbbox()?;
    Some((bb.2 - bb.0) as f64 / (bb.3 - bb.1) as f64)
}

/// A name box as a small coarse picture (the same name on the same screen gives the same one).
pub fn name_signature(img: &Image) -> ((usize, usize), Vec<u8>) {
    let g = img.convert(Mode::L);
    (g.size(), g.resize(64, 16, Some(Filter::Bilinear)).point(|v| f64::from(v / 64)).tobytes())
}

/// The battle prep screen's big slanted label at the bottom left, levelled for OCR.
pub fn prep_label(img: &Image, u: f64, ux: f64, h: f64) -> Image {
    let c = img.crop_i((ux + u * 0.17) as i64, (h * 0.75) as i64, (ux + u * 0.46) as i64, (h * 0.97) as i64).convert(Mode::L);
    let c = c.rotate(-11.0, Filter::Bicubic, true, &[0]);
    let c = scaled(&c, 2000.0 / u, Filter::Bicubic);
    expand(&invert(&c.point(thr(200))), 20, &[255])
}
