//! What the DPS meter shows over the game, and every plug-in's notices: the DPS window (drag to move when unlocked),
//! the HP labels on the boss bar, the notes on end screens, "You're being attacked" with the red edge flash, and the
//! notice cards. Port of aLogs B16 `bossbar/dps_overlay.py` and `notice_window.py`. Runs on the binary's main thread
//! (its windows' messages), the meter on another; the meter hands over its messages (Out) and a View of its state.
//! Without the game's font the text is drawn in Segoe UI (the Python used Tk's text there). The Dev switches
//! overlay-label-cache and overlay-idle-poll are not ported (pictures are redrawn; the loop looks every 15 ms).

/// The overlay's windows are up, so other programs' notices (Sanctum Helper's) come here: status.json says so
/// (`takes_notices` in dps_overlay.py).
pub static TAKES_NOTICES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

use alogs_capture::layered::{self, Layered, Mouse};
use alogs_pil::ops::{gaussian_blur, lighter, multiply};
use alogs_pil::{Filter, Image, Mode};
use alogs_pyval::{Dict, Value};
use alogs_reader::gamefont::{FONT_NAME, GameFont};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

pub const POLL_MS: u64 = 15;
pub const OVERLAY_REDRAW: f64 = 0.25;
pub const BAR_HIDDEN: f64 = 0.5;
/// A bar's own label (no fight yet) goes this long after the meter last told of the bar.
pub const BAR_ONLY_KEEP: f64 = 1.0;
pub const MINIMAP_RIGHT: f64 = 0.195;
pub const BAR_LEFT: f64 = 0.32;
pub const MAP_TOP: f64 = 0.069;
pub const MAP_SCALE: f64 = 0.85;
const ALERT_TEXT: &str = "You're being attacked in Aniimo";
const ALERT_SIZE: f64 = 0.026;
const ALERT_Y: f64 = 0.265;
const ALERT_SHOW: f64 = 2.0;
const ALERT_EVERY: f64 = 5.0;
const HIT_CURVE: [(f64, f64); 4] = [(0.0, 0.0), (0.1, 0.5), (0.233, 0.9), (0.733, 0.0)];
const TOAST_SHOW: f64 = 4.0;
const TOAST_IN: f64 = 0.25;
const TOAST_OUT: f64 = 0.35;
const TOAST_W: f64 = 360.0;
const TOAST_H: f64 = 64.0;
const TOAST_MARGIN: f64 = 18.0;
const HP_GRADIENT: [(f64, [u8; 3]); 7] = [(0.0, [248, 108, 108]), (0.25, [250, 120, 104]), (0.4, [249, 131, 98]), (0.55, [251, 144, 94]),
    (0.7, [254, 157, 91]), (0.85, [255, 167, 88]), (1.0, [255, 176, 85])];

/// Where the overlay's windows are (screen px: x0, y0, x1, y1, until; until 0: there now), and whether they show in
/// captures, for the meter to cover them in its own pictures of the screen (Meter::mask_own).
#[derive(Default)]
pub struct Own {
    pub rects: Vec<(i32, i32, i32, i32, f64)>,
    pub in_capture: bool,
    /// The DPS window's picture and screen spot while it's shown (Meter::proof_with_overlay).
    pub dps_now: Option<(Image, i32, i32)>,
}

/// The meter's state the overlay looks at (published by the meter's loop every step).
#[derive(Clone)]
pub struct View {
    pub fight: bool,
    pub bar_seen: f64,
    pub hit_at: f64,
    pub last_screen: Option<(i32, i32, i32, i32)>,
    pub frame_size: (usize, usize),
    pub origin: (i32, i32),
    pub cfg: Value,
    pub quiet: bool,
    pub capture_off: bool,
    pub bossrush_space: bool,
    pub holosim_space: bool,
    pub hwnd: Option<usize>,
    pub notice_owners: Dict,
    pub running: bool,
}

pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// The HP bar's colour at this share of the track.
pub fn hp_colour(frac: f64) -> [u8; 3] {
    let frac = frac.clamp(0.0, 1.0);
    for w in HP_GRADIENT.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if frac <= b {
            let t = (frac - a) / (b - a);
            return [0, 1, 2].map(|i| (f64::from(ca[i]) + (f64::from(cb[i]) - f64::from(ca[i])) * t).round_ties_even() as u8);
        }
    }
    HP_GRADIENT[6].1
}

/// `fmt`: 1.23M, 45.6K, 1,234.
pub fn fmt(n: f64) -> String {
    if n >= 1e6 {
        format!("{:.2}M", n / 1e6)
    } else if n >= 1e4 {
        format!("{:.1}K", n / 1e3)
    } else {
        let s = format!("{:.0}", n);
        let neg = s.starts_with('-');
        let d = s.trim_start_matches('-');
        let mut o = String::new();
        for (i, c) in d.chars().enumerate() {
            if i > 0 && (d.len() - i) % 3 == 0 {
                o.push(',');
            }
            o.push(c);
        }
        if neg { format!("-{o}") } else { o }
    }
}

fn clock(s: f64) -> String {
    let s = s.round_ties_even().max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn rgb(c: &str) -> [u8; 3] {
    let h = |i: usize| u8::from_str_radix(c.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255);
    [h(1), h(3), h(5)]
}

/// Text with a soft dark shadow (blurred, a little down and right); the image and how far the text sits inside it.
pub fn with_shadow(img: &Image, px: f64) -> (Image, i64) {
    let blur = ((px * 0.14).round_ties_even() as i64).max(1);
    let drop = ((px * 0.06).round_ties_even() as i64).max(1);
    let edge = 2 * blur + drop;
    let (w, h) = (img.width + 2 * edge as usize, img.height + 2 * edge as usize);
    let mut out = Image::new(Mode::Rgba, w, h, &[0, 0, 0, 0]);
    let mut mask = Image::new(Mode::L, w, h, &[0]);
    let alpha = img.split().pop().unwrap();
    mask.paste(&alpha, edge + drop, edge + drop);
    let mask = gaussian_blur(&mask, blur as f64).point(|v| f64::from((u32::from(v) * 2).min(255)));
    let mut shade = Image::new(Mode::Rgba, w, h, &[6, 8, 16, 0]);
    shade.putalpha(&mask);
    out.alpha_composite(&shade, 0, 0);
    out.alpha_composite(img, edge, edge);
    (out, edge)
}

/// A filled rounded rectangle (Pillow's, without anti-aliasing), with an optional outline.
fn rounded_rect(im: &mut Image, b: (i64, i64, i64, i64), r: i64, fill: [u8; 4], outline: Option<([u8; 4], i64)>) {
    let inside = |x: i64, y: i64, inset: i64| -> bool {
        let (x0, y0, x1, y1) = (b.0 + inset, b.1 + inset, b.2 - inset, b.3 - inset);
        if x < x0 || x > x1 || y < y0 || y > y1 {
            return false;
        }
        let rr = (r - inset).max(0);
        let cx = if x < x0 + rr { x0 + rr } else if x > x1 - rr { x1 - rr } else { x };
        let cy = if y < y0 + rr { y0 + rr } else if y > y1 - rr { y1 - rr } else { y };
        let (dx, dy) = ((x - cx) as f64, (y - cy) as f64);
        dx * dx + dy * dy <= (rr as f64 + 0.5).powi(2)
    };
    for y in b.1.max(0)..=b.3.min(im.height as i64 - 1) {
        for x in b.0.max(0)..=b.2.min(im.width as i64 - 1) {
            if !inside(x, y, 0) {
                continue;
            }
            let c = match outline {
                Some((oc, w)) if !inside(x, y, w) => oc,
                _ => fill,
            };
            let i = (y as usize * im.width + x as usize) * 4;
            im.data[i..i + 4].copy_from_slice(&c);
        }
    }
}

/// A line of the stats: text (or a (label, value) row), colour, size as a share of the HUD height, style.
#[derive(Clone, PartialEq)]
pub struct Line {
    pub text: (String, Option<String>),
    pub colour: String,
    pub size: f64,
    pub style: &'static str,
}

fn line(t: &str, colour: &str, size: f64, style: &'static str) -> Line {
    Line { text: (t.to_string(), None), colour: colour.into(), size, style }
}

fn row(label: &str, value: &str, size: f64) -> Line {
    Line { text: (label.to_string(), Some(value.to_string())), colour: "#ffffff".into(), size, style: "" }
}

struct Text {
    game: Option<GameFont>,
    plain: Option<alogs_reader::text::Font>,
}

impl Text {
    /// The text as RGBA, and its baseline's y in the picture.
    fn render(&self, text: &str, px: f64, fill: [u8; 3], italic: bool, bold: bool, stroke: bool, lead: Option<(usize, [u8; 3])>) -> (Image, i64) {
        if let Some(f) = &self.game {
            return f.render(text, px, fill, [20, 26, 44], italic, bold, stroke, lead);
        }
        // Segoe UI standing in: white text on a mask, with a thin dark outline
        let Some(f) = &self.plain else { return (Image::new(Mode::Rgba, 1, 1, &[0, 0, 0, 0]), 1) };
        let w = (f.length(text, px as f32) + px as f32 * 0.6).ceil().max(1.0) as usize;
        let h = (px * 1.5).ceil() as usize;
        let mut mask = Image::new(Mode::L, w, h, &[0]);
        f.draw(&mut mask, px as f32 * 0.2, 0.0, text, px as f32, 255);
        let rim = alogs_pil::ops::max_filter(&mask, 3);
        let mut out = Image::new(Mode::Rgba, w, h, &[20, 26, 44, 0]);
        out.putalpha(&rim);
        let mut fg = Image::new(Mode::Rgba, w, h, &[fill[0], fill[1], fill[2], 0]);
        fg.putalpha(&mask);
        out.alpha_composite(&fg, 0, 0);
        let _ = (italic, bold, stroke, lead);
        (out, (px * 1.1) as i64)
    }
}

/// (left, right) of the letters' solid part, leaving out the soft shadow.
fn ink(im: &Image, italic: bool) -> (i64, i64) {
    let a = im.split().pop().unwrap();
    let a = if italic { a.crop_i(0, a.height as i64 / 2, a.width as i64, a.height as i64) } else { a };
    let a = a.point(|v| if v > 140 { 255.0 } else { 0.0 });
    a.getbbox().map_or((0, im.width as i64), |b| (b.0 as i64, b.2 as i64))
}

struct Win {
    w: Layered,
    /// (image, x, y) while placed.
    placed: Option<(Image, i32, i32)>,
}

impl Win {
    fn new() -> Option<Win> {
        Some(Win { w: Layered::new()?, placed: None })
    }
}

struct Toast {
    queue: Vec<(String, String, String, Option<(i32, i32, i32, i32)>, String)>,
    since: Option<f64>,
    img: Option<Image>,
    at: (f64, f64),
    edge: &'static str,
    motion: String,
    screen: (i32, i32, i32, i32),
    alpha: i32,
    shown: Option<(i32, i32)>,
    looked: f64,
    sample: Option<(i32, i32)>,
    drag: Option<(i32, i32)>,
}

pub struct Overlay {
    data: PathBuf,
    view: Arc<Mutex<View>>,
    rx: Receiver<(String, Value)>,
    hide: bool,
    text: Text,
    pos: serde_json::Map<String, serde_json::Value>,
    dw: Win,
    hp: HashMap<&'static str, (Win, Option<(f64, f64)>, f64)>,
    aw: Win,
    ew: Win,
    tw: Win,
    away: bool,
    // the DPS window
    dps_base: Option<(f64, f64)>,
    dps_h: f64,
    dps_key: String,
    dps_shown: bool,
    hide_at: Option<f64>,
    dps_lines: Option<Vec<Line>>,
    dps_img: Option<Image>,
    dps_scale: f64,
    dps_locked: Option<bool>,
    dps_end: bool,
    dps_quiet: bool,
    hp_up: bool,
    /// The HP % stands on a bar whose fight hasn't started: when the meter last told of that bar (show_bar).
    bar_only: Option<f64>,
    /// Where the HP numbers were last drawn: (left edge, top, the bar's x, its row, its span). The HP % stands in
    /// front of them (pct_anchor).
    pct_at: Option<(f64, f64, f64, f64, f64)>,
    /// Room kept right of the HP % for what isn't drawn yet (see show_bar).
    pct_inset: f64,
    bar_left: Option<f64>,
    example: bool,
    example_at: f64,
    dragging: Option<(usize, i32, i32)>,
    pending: Option<Value>,
    drawn_at: f64,
    // the alert
    alert_screen: Option<(i32, i32, i32, i32)>,
    alert_shown: bool,
    alert_since: Option<f64>,
    alert_alpha: i32,
    toast: Toast,
    in_capture: Option<bool>,
    own: Arc<Mutex<Own>>,
    cur: HashMap<usize, (i32, i32, i32, i32)>,
    /// Spots just left: (x0, y0, x1, y1, until, window).
    old: Vec<(i32, i32, i32, i32, f64, usize)>,
    /// Windows Windows keeps out of every capture (the HP labels, once that took): never in a picture of the screen,
    /// so the meter doesn't cover them in its own.
    uncaptured: std::collections::HashSet<usize>,
    published: f64,
    shared: alogs_capture::shared::SharedCapture,
}

/// The HP % stands under the bar in front of the HP numbers ("42.2% \u{b7} HP 99.5K / 235.6K"), not on the bar: there it
/// hid 42 to 49% of the track, and while the boss's HP was under it the meter's reading waited (the owner chose the
/// place from a mockup, 17 Nov). Its right edge is PCT_GAP of the span left of the HP numbers (`pct_at`, set when
/// they are drawn); until they have been drawn for this bar, at PCT_RIGHT of the span.
pub const PCT_RIGHT: f64 = 0.80;
pub const PCT_GAP: f64 = 0.008;

/// Where the HP %'s right edge and top go: `total` is where the HP numbers' left edge and top are, if they have been
/// drawn for this bar (`bar`: its left bracket's x, its centre row, its span); `inset` is the room left for what
/// isn't drawn yet (the dot after the %, before the numbers are there).
pub fn pct_anchor(bar: (f64, f64, f64), total: Option<(f64, f64)>, inset: f64) -> (f64, f64) {
    let (bx, by, span) = bar;
    let (right, top) = total.map_or((bx + PCT_RIGHT * span, by + TOTAL_Y * span), |(left, top)| (left - PCT_GAP * span, top));
    (right - inset, top)
}

/// A label's place moved by hand is kept under this name (overlay_positions.json). The HP %'s has a name of its own
/// since it moved off the bar: a move made from its old place would throw it far from the new one.
fn pos_key(name: &str) -> String {
    if name == "pct" { "hp_pct_line".to_string() } else { format!("hp_{name}") }
}

const TOTAL_Y: f64 = 0.022;
const HP_SPOTS: [(&str, f64, f64, &str, f64); 4] = [
    ("pct", PCT_RIGHT, TOTAL_Y, "ne", 0.022),
    ("bar", 1.018, 0.0, "w", 0.024),
    ("total", 1.0, TOTAL_Y, "ne", 0.022),
    ("break", 1.0, 0.049, "ne", 0.020),
];

impl Overlay {
    pub fn new(data: PathBuf, player_log: Option<PathBuf>, view: Arc<Mutex<View>>, rx: Receiver<(String, Value)>, hide: bool, own: Arc<Mutex<Own>>) -> Option<Overlay> {
        let game = GameFont::load(&data.join("gamefont"), player_log.as_deref(), FONT_NAME).ok();
        log::info!(target: "meter.dps", "overlay font: {}", if game.is_some() { "the game's own (UI_Font_Europe)" } else { "Segoe UI (game font not readable)" });
        let dir = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from).join("Fonts");
        let plain = alogs_reader::text::Font::load(&dir.join("seguisb.ttf")).or_else(|| alogs_reader::text::Font::load(&dir.join("segoeui.ttf")));
        let pos = std::fs::read_to_string(data.join("overlay_positions.json")).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.as_object().cloned()).unwrap_or_default();
        let mut hp = HashMap::new();
        for (name, ..) in HP_SPOTS {
            hp.insert(name, (Win::new()?, None, 1.0));
        }
        let o = Overlay {
            data, view, rx, hide, text: Text { game, plain }, pos, dw: Win::new()?, hp, aw: Win::new()?, ew: Win::new()?, tw: Win::new()?, away: false,
            dps_base: None, dps_h: 1.0, dps_key: String::new(), dps_shown: false, hide_at: None, dps_lines: None, dps_img: None, dps_scale: 1.0,
            dps_locked: None, dps_end: false, dps_quiet: false, hp_up: false, bar_only: None, pct_at: None, pct_inset: 0.0, bar_left: None, example: false, example_at: 0.0, dragging: None,
            pending: None, drawn_at: 0.0, alert_screen: None, alert_shown: false, alert_since: None, alert_alpha: -1,
            toast: Toast { queue: vec![], since: None, img: None, at: (0.0, 0.0), edge: "right", motion: "fade".into(), screen: (0, 0, 1920, 1080),
                alpha: -1, shown: None, looked: 0.0, sample: None, drag: None },
            in_capture: None,
            own,
            cur: HashMap::new(),
            old: vec![],
            uncaptured: Default::default(),
            published: 0.0,
            shared: alogs_capture::shared::SharedCapture::new(3),
        };
        for (_, (w, ..)) in &o.hp {
            w.w.click_through(false);
        }
        o.ew.w.click_through(true);
        o.tw.w.click_through(true);
        Some(o)
    }

    fn wins(&self) -> Vec<&Layered> {
        let mut v = vec![&self.dw.w, &self.aw.w, &self.ew.w, &self.tw.w];
        v.extend(self.hp.values().map(|h| &h.0.w));
        v
    }

    fn save_positions(&self) {
        let _ = std::fs::write(self.data.join("overlay_positions.json"), serde_json::Value::Object(self.pos.clone()).to_string());
    }

    fn present(&mut self, which: &str, image: Image, x: i32, y: i32) {
        let away = self.away;
        let rect = (x, y, x + image.width as i32, y + image.height as i32);
        let win = self.win_mut(which);
        if !away {
            win.w.show(&image, x, y);
        }
        win.placed = Some((image, x, y));
        let h = win.w.hwnd;
        self.own_spot(h, Some(rect));
        if which == "dps" {
            let shown = if away { None } else { self.dw.placed.clone() };
            self.own.lock().unwrap().dps_now = shown;
        }
    }

    fn park(&mut self, which: &str) {
        let win = self.win_mut(which);
        win.placed = None;
        win.w.park();
        let h = win.w.hwnd;
        self.own_spot(h, None);
        if which == "dps" {
            self.own.lock().unwrap().dps_now = None;
        }
    }

    /// Where each overlay window is, for the meter to cover; a spot just left is kept half a second.
    fn own_spot(&mut self, hwnd: usize, rect: Option<(i32, i32, i32, i32)>) {
        let t = now();
        let prev = self.cur.get(&hwnd).copied();
        if prev == rect {
            return;
        }
        if let Some(p) = prev {
            self.old.push((p.0, p.1, p.2, p.3, t + 0.5, hwnd));
        }
        match rect {
            Some(r) => {
                self.cur.insert(hwnd, r);
            }
            None => {
                self.cur.remove(&hwnd);
            }
        }
        self.old.retain(|r| r.4 > t);
        self.publish_own(t);
    }

    /// Our spots for Sanctum Helper and the app to cover in their pictures (and again each second).
    fn publish_own(&mut self, t: f64) {
        self.published = t;
        let mut rects: Vec<(i32, i32, i32, i32, f64, usize)> = self.cur.iter().map(|(h, r)| (r.0, r.1, r.2, r.3, 0.0, *h)).collect();
        rects.extend(self.old.iter().copied());
        let live: Vec<(i32, i32, i32, i32)> = rects.iter().filter(|r| r.4 == 0.0 || r.4 > t).map(|r| (r.0, r.1, r.2, r.3)).collect();
        self.shared.publish_overlays(alogs_capture::shared::OWNER_METER, &live);
        // The meter covers only the windows that can be in its pictures. The HP labels are kept out of captures (in
        // tick): covered all the same, the HP numbers' window (anchored at the right bracket, with its shadow margin)
        // blanked the bracket's lowest rows in 25 of 26 fight frames (22 Nov), and the bracket check failed on the
        // true bar in 4 of them for that alone.
        self.own.lock().unwrap().rects = rects.into_iter().filter(|r| !self.uncaptured.contains(&r.5)).map(|r| (r.0, r.1, r.2, r.3, r.4)).collect();
    }

    fn win_mut(&mut self, which: &str) -> &mut Win {
        match which {
            "dps" => &mut self.dw,
            "alert" => &mut self.aw,
            "edge" => &mut self.ew,
            "toast" => &mut self.tw,
            n => &mut self.hp.get_mut(n).unwrap().0,
        }
    }

    fn cfg_overlay(&self, v: &View, key: &str) -> Value {
        v.cfg.get("overlay").get(key).clone()
    }

    /// The stats as one RGBA image: lines, optional divider, centred (end notes), the dark card's strength in %.
    fn compose(&self, lines: &[Line], h: f64, divider: bool, center: bool, backdrop: f64) -> Image {
        let pad = ((h * if backdrop > 0.0 { 0.011 } else { 0.004 }).round_ties_even() as i64).max(3);
        #[allow(dead_code)]
        enum Part {
            One(Image, i64, bool),
            Row((Image, i64), (Image, i64)),
        }
        let mut parts: Vec<(Part, f64)> = vec![];
        let rows = lines.iter().any(|l| l.text.1.is_some());
        for l in lines {
            let px = (h * l.size).round_ties_even().max(11.0);
            match &l.text.1 {
                Some(v) => {
                    let a = self.text.render(&l.text.0, px, rgb("#aab4c3"), false, false, false, None);
                    let b = self.text.render(v, px, rgb(&l.colour), false, false, false, None);
                    parts.push((Part::Row(a, b), px));
                }
                None => {
                    let it = l.style.contains('i');
                    let (im, base) = self.text.render(&l.text.0, px, rgb(&l.colour), it, l.style.contains('b'), false, None);
                    parts.push((Part::One(im, base, it), px));
                }
            }
        }
        let span = |im: &Image, it: bool| {
            let (a, b) = ink(im, it);
            b - a
        };
        let above = |a: &(Image, i64), b: &(Image, i64)| a.1.max(b.1);
        let tall = |p: &Part| match p {
            Part::One(im, _, _) => im.height as i64,
            Part::Row(a, b) => above(a, b) + (a.0.height as i64 - a.1).max(b.0.height as i64 - b.1),
        };
        let gap = (h * 0.012).round_ties_even() as i64;
        let mut width = if rows {
            let px0 = (h * 0.015).round_ties_even().max(11.0);
            let least = span(&self.text.render("Last 5s", px0, [255; 3], false, false, false, None).0, false) + gap
                + span(&self.text.render("888.8K / 8.88M HP", px0, [255; 3], false, false, false, None).0, false);
            parts.iter().map(|(p, _)| match p {
                Part::One(im, _, it) => span(im, *it),
                Part::Row(a, b) => span(&a.0, false) + gap + span(&b.0, false),
            }).fold(least, i64::max)
        } else {
            parts.iter().map(|(p, _)| match p { Part::One(im, _, _) => im.width as i64, Part::Row(a, b) => (a.0.width + b.0.width) as i64 }).max().unwrap_or(1)
        };
        if divider {
            width = width.max((h * 0.215).round_ties_even() as i64);
        }
        let lw = ((h / 1080.0).round_ties_even() as i64).max(1);
        let top = if divider { (h * 0.01).round_ties_even() as i64 } else { 0 };
        let height = top + parts.iter().map(|(p, px)| tall(p) + (px * 0.04).round_ties_even() as i64).sum::<i64>();
        let (ow, oh) = ((width + 2 * pad) as usize, (height + 2 * pad) as usize);
        let mut out = Image::new(Mode::Rgba, ow, oh, &[0, 0, 0, 0]);
        if backdrop > 0.0 {
            let mut shape = Image::new(Mode::L, ow, oh, &[0]);
            let mut s4 = Image::new(Mode::Rgba, ow, oh, &[0, 0, 0, 0]);
            rounded_rect(&mut s4, (0, 0, ow as i64 - 1, oh as i64 - 1), (h * 0.014).round_ties_even() as i64, [255; 4], None);
            shape.data = s4.data.chunks_exact(4).map(|p| p[3]).collect();
            let strength = 255.0 * backdrop / 100.0;
            let mut fade = Image::new(Mode::L, ow, oh, &[0]);
            for y in 0..oh {
                for x in 0..ow {
                    let v = (x as f64 * 255.0 / ow.max(2) as f64).floor();
                    fade.data[y * ow + x] = (strength * (1.0 - 0.6 * v / 255.0)).round_ties_even() as u8;
                }
            }
            let mut card = Image::new(Mode::Rgba, ow, oh, &[22, 24, 34, 0]);
            card.putalpha(&multiply(&fade, &shape));
            out.alpha_composite(&card, 0, 0);
        }
        if divider {
            let len = (h * 0.215).round_ties_even() as usize;
            for (yy, c) in [(pad + lw, [20u8, 26, 40, 90]), (pad, [232, 236, 242, 190])] {
                let mut l = Image::new(Mode::Rgba, len, lw as usize, &c);
                l.putalpha(&Image::new(Mode::L, len, lw as usize, &[c[3]]));
                out.alpha_composite(&l, pad, yy);
            }
        }
        let mut y = pad + top;
        for (p, px) in &parts {
            match p {
                Part::Row(a, b) => {
                    let base = y + above(a, b);
                    out.alpha_composite(&a.0, pad - ink(&a.0, false).0, base - a.1);
                    out.alpha_composite(&b.0, pad + width - ink(&b.0, false).1, base - b.1);
                }
                Part::One(im, _, it) => {
                    let x = if rows { pad + width - ink(im, *it).1 } else if center { pad + (width - im.width as i64) / 2 } else { pad };
                    out.alpha_composite(im, x, y);
                }
            }
            y += tall(p) + (px * 0.04).round_ties_even() as i64;
        }
        out
    }

    fn dps_locked(&self, v: &View) -> bool {
        let example = self.cfg_overlay(v, "dps_example").truthy();
        !matches!(self.cfg_overlay(v, "dps_locked"), Value::Bool(false)) && !(example && !v.fight)
    }

    fn show_dps(&mut self, v: &View, screen: (i32, i32, i32, i32), bossrush: bool, lines: Option<Vec<Line>>, bar: Option<f64>) {
        let (ox, oy, w, h) = (f64::from(screen.0), f64::from(screen.1), f64::from(screen.2), f64::from(screen.3));
        let u = w.min(h * 16.0 / 9.0);
        let ux = ox + (w - u) / 2.0;
        let hh = u * 9.0 / 16.0;
        let mut scale = 1.0;
        let wp = self.cfg_overlay(v, "world_position").py_str();
        let (base, key) = if bossrush && self.cfg_overlay(v, "rush_position").as_str() == Some("panel") {
            ((ux + u * 0.025, oy + hh * 0.398), "dps_offset2_rush")
        } else if bossrush {
            ((ux + u * 0.068, oy + hh * 0.022), "dps_offset2_rush_top")
        } else if wp == "right" || wp == "quests" {
            ((ux + u * 0.705, oy + hh * 0.10), "dps_offset2_right")
        } else {
            scale = MAP_SCALE;
            let left = ux + u * MINIMAP_RIGHT;
            let right = bar.filter(|b| left < *b && *b < ux + u * 0.5).unwrap_or(ux + u * BAR_LEFT);
            if self.bar_left.is_none_or(|b| (b - right).abs() > hh * 0.01) {
                self.bar_left = Some(right);
            }
            let width = hh * 0.21 * scale;
            (((left + hh * 0.01).max((left + self.bar_left.unwrap()) / 2.0 - width / 2.0), oy + hh * MAP_TOP), "dps_offset2_map")
        };
        self.dps_base = Some(base);
        self.dps_h = hh;
        self.dps_key = key.into();
        let locked = self.dps_locked(v);
        if self.dps_locked != Some(locked) {
            self.dps_locked = Some(locked);
            self.dw.w.click_through(locked);
        }
        let reset = self.cfg_overlay(v, "dps_reset").as_i64().unwrap_or(0);
        if reset != self.pos.get("dps_reset_seen").and_then(serde_json::Value::as_i64).unwrap_or(0) {
            self.pos.retain(|k, _| !k.starts_with("dps_offset"));
            self.pos.insert("dps_reset_seen".into(), reset.into());
            self.save_positions();
        }
        if let Some(l) = lines
            && (Some(&l) != self.dps_lines.as_ref() || self.dps_scale != scale)
        {
            let backdrop = alogs_pyval::py_int(&self.cfg_overlay(v, "backdrop")).unwrap_or(0).clamp(0, 100) as f64;
            self.dps_img = Some(self.compose(&l, hh * scale, false, false, backdrop));
            self.dps_lines = Some(l);
            self.dps_scale = scale;
        }
        let (mut x, mut y) = base;
        let off = self.pos.get(key).and_then(|o| Some((o.get(0)?.as_f64()?, o.get(1)?.as_f64()?)));
        if let Some(o) = off {
            let (nx, ny) = (x + o.0 * hh, y + o.1 * hh);
            if !(ox - hh * 0.2 <= nx && nx <= ox + w && oy - hh * 0.2 <= ny && ny <= oy + h) {
                self.pos.remove(key);
                self.save_positions();
            } else {
                (x, y) = (nx, ny);
            }
        }
        if self.dragging.is_some_and(|d| d.0 == self.dw.w.hwnd)
            && let Some(p) = &self.dw.placed
        {
            (x, y) = (f64::from(p.1), f64::from(p.2));
        }
        if let Some(img) = self.dps_img.clone() {
            self.present("dps", img, x as i32, y as i32);
        }
        self.dps_shown = true;
        self.hide_at = None;
    }

    fn hide_dps(&mut self) {
        self.park("dps");
        self.dps_shown = false;
    }

    fn show_end(&mut self, v: &View, text: &str, top: f64) {
        if !self.cfg_overlay(v, "dps_window").truthy() && !matches!(self.cfg_overlay(v, "dps_window"), Value::None) {
            return;
        }
        let Some((ox, oy, w, h)) = v.last_screen else { return };
        let lines: Vec<Line> = text.split('\n').filter(|t| !t.is_empty()).enumerate()
            .map(|(i, t)| line(t, if i == 0 { "#ffd36a" } else { "#ffffff" }, if i == 0 { 0.02 } else { 0.0165 }, "")).collect();
        let backdrop = alogs_pyval::py_int(&self.cfg_overlay(v, "backdrop")).unwrap_or(0).clamp(0, 100) as f64;
        let img = self.compose(&lines, f64::from(h), false, true, backdrop);
        self.dps_lines = Some(lines);
        let x = (f64::from(ox) + f64::from(w) / 2.0 - img.width as f64 / 2.0) as i32;
        let y = (f64::from(oy) + f64::from(h) * top) as i32;
        self.dps_img = Some(img.clone());
        self.present("dps", img, x, y);
        self.dps_shown = true;
        self.dps_end = true;
    }

    fn draw_hp(&mut self, name: &'static str, text: &str, bar: (f64, f64, f64), colour: &str, hp_rgb: Option<[u8; 3]>) {
        let (_, fx, fy, anchor, fs) = *HP_SPOTS.iter().find(|s| s.0 == name).unwrap();
        let (bx, by, span) = bar;
        let px = (span * fs).round_ties_even().max(11.0);
        let lead = if text.starts_with("HP ") {
            Some((2, hp_rgb.unwrap_or_else(|| hp_colour(1.0))))
        } else if text.starts_with("BREAK") || text.starts_with("BROKEN") {
            Some((text.split(' ').next().unwrap_or("").chars().count(), rgb(colour)))
        } else {
            None
        };
        let (plain, _) = self.text.render(text, px, rgb(colour), false, name == "pct", true, lead);
        let (w, h) = (plain.width as f64, plain.height as f64);
        let (img, edge) = with_shadow(&plain, px);
        let (mut tx, mut ty) = (bx + fx * span, by + fy * span);
        if name == "pct" {
            let total = self.pct_at.filter(|p| (p.2 - bx).abs() < 2.0 && (p.3 - by).abs() < 2.0 && (p.4 - span).abs() < 1.0).map(|p| (p.0, p.1));
            (tx, ty) = pct_anchor(bar, total, self.pct_inset);
        }
        let (mut x, mut y) = match anchor {
            "w" => (tx, ty - h / 2.0),
            "ne" => (tx - w, ty),
            "s" => (tx - w / 2.0, ty - h),
            _ => (tx - w / 2.0, ty - h / 2.0),
        };
        x -= edge as f64;
        y -= edge as f64;
        if let Some(e) = self.hp.get_mut(name) {
            e.1 = Some((x, y));
            e.2 = span;
        }
        if let Some(o) = self.pos.get(&pos_key(name)).and_then(|o| Some((o.get(0)?.as_f64()?, o.get(1)?.as_f64()?))) {
            x += o.0 * span;
            y += o.1 * span;
        }
        let hwnd = self.hp[name].0.w.hwnd;
        if self.dragging.is_some_and(|d| d.0 == hwnd)
            && let Some(p) = &self.hp[name].0.placed
        {
            (x, y) = (f64::from(p.1), f64::from(p.2));
        }
        if name == "total" {
            // where the HP numbers really are (moved by hand too): the % goes in front of them
            self.pct_at = Some((x + edge as f64, y + edge as f64, bx, by, span));
        }
        self.present(name, img, x as i32, y as i32);
    }

    fn hide_hp(&mut self) {
        for (n, ..) in HP_SPOTS {
            self.park(n);
        }
    }

    fn show_hp(&mut self, v: &View, d: &Value) {
        let bar = d.get("bar").as_list().filter(|b| b.len() == 3).map(|b| (b[0].as_f64().unwrap_or(0.0), b[1].as_f64().unwrap_or(0.0), b[2].as_f64().unwrap_or(1.0)));
        let show = !matches!(self.cfg_overlay(v, "hp_labels"), Value::Bool(false));
        let Some(bar) = bar.filter(|_| show) else {
            self.hide_hp();
            return;
        };
        let f = |k: &str| d.get(k).as_f64().unwrap_or(0.0);
        let left = (f("max_hp") - f("damage")).max(0.0);
        let max_hp = if f("boss_hp") != 0.0 { f("boss_hp") } else if f("max_hp") != 0.0 { f("max_hp") } else { 1.0 };
        let seg = d.get("bossrush").truthy() && d.get("xn").truthy() && d.get("seg_size").truthy();
        if seg {
            self.draw_hp("bar", &format!("{} / {}", fmt(f("seg_left")), fmt(f("seg_size"))), bar, "#ffffff", None);
        } else {
            self.park("bar");
        }
        let shown = if seg { f("seg_left") / f("seg_size") } else { left / max_hp };
        // the HP numbers first: the % stands in front of them, wherever they are
        self.draw_hp("total", &format!("HP {} / {}", fmt(left), fmt(max_hp)), bar, "#ffffff", Some(hp_colour(shown)));
        self.pct_inset = 0.0;
        self.draw_hp("pct", &format!("{:.1}% \u{b7}", left / max_hp * 100.0), bar, "#ffffff", None);
        match break_text(d.get("break")) {
            Some((t, c)) => self.draw_hp("break", &t, bar, c, None),
            None => self.park("break"),
        }
    }

    /// A boss bar whose fight hasn't started yet (the meter's "bar"): its HP % from the bar's own fill, at once. The
    /// HP numbers and the break line need the boss, and come with the fight a moment later.
    fn show_bar(&mut self, v: &View, d: &Value) {
        let bar = d.get("bar").as_list().filter(|b| b.len() == 3).map(|b| (b[0].as_f64().unwrap_or(0.0), b[1].as_f64().unwrap_or(0.0), b[2].as_f64().unwrap_or(1.0)));
        let show = !matches!(self.cfg_overlay(v, "hp_labels"), Value::Bool(false));
        let (Some(bar), Some(fill)) = (bar.filter(|_| show), d.get("fill").as_f64()) else { return };
        // alone, with no dot after it yet, and where it will stand once the numbers come: the dot's room is left
        let lone = format!("{:.1}%", fill * 100.0);
        let px = (bar.2 * HP_SPOTS[0].4).round_ties_even().max(11.0);
        let wide = |t: &Text, s: &str| t.render(s, px, [255, 255, 255], false, true, true, None).0.width as f64;
        self.pct_inset = wide(&self.text, &format!("{lone} \u{b7}")) - wide(&self.text, &lone);
        self.draw_hp("pct", &lone, bar, "#ffffff", None);
        self.pct_inset = 0.0;
        for name in ["bar", "total", "break"] {
            self.park(name);
        }
        self.hp_up = true;
        self.bar_only = Some(now());
    }

    fn show_fight(&mut self, v: &View, d: &Value) {
        self.bar_only = None;
        let bossrush = d.get("bossrush").truthy();
        if !matches!(self.cfg_overlay(v, "dps_window"), Value::Bool(false))
            && let Some(screen) = screen_of(d)
        {
            let bar = d.get("bar").as_list().and_then(|b| b.first().and_then(Value::as_f64));
            self.show_dps(v, screen, bossrush, Some(stat_lines(d)), bar);
        }
        if now() - v.bar_seen < BAR_HIDDEN {
            self.show_hp(v, d);
            self.hp_up = true;
        }
    }

    fn draw_alert_text(&mut self, ox: f64, oy: f64, w: f64, hud: f64) {
        let px = (hud * ALERT_SIZE).round_ties_even().max(14.0);
        let (text, _) = self.text.render(ALERT_TEXT, px, [255, 255, 255], false, true, false, None);
        let (dx, dy) = (((px * 0.03).round_ties_even() as i64).max(1), ((px * 0.07).round_ties_even() as i64).max(1));
        let mut img = Image::new(Mode::Rgba, text.width + dx as usize, text.height + dy as usize, &[0, 0, 0, 0]);
        let mut under = Image::new(Mode::Rgba, text.width, text.height, &[0, 0, 0, 0]);
        under.putalpha(&text.split().pop().unwrap().point(|v| f64::from(u32::from(v) * 102 / 255)));
        img.alpha_composite(&under, dx, dy);
        img.alpha_composite(&text, 0, 0);
        let (x, y) = ((ox + w / 2.0 - img.width as f64 / 2.0) as i32, (oy + hud * ALERT_Y - img.height as f64 / 2.0) as i32);
        self.aw.w.show(&img, x, y);
    }

    fn show_alert(&mut self, v: &View) {
        let t0 = now();
        let screen = game_screen(v.last_screen, v.origin, v.frame_size);
        let (ox, oy, w, h) = (f64::from(screen.0), f64::from(screen.1), f64::from(screen.2), f64::from(screen.3));
        let hud = w.min(h * 16.0 / 9.0) * 9.0 / 16.0;
        let since = *self.alert_since.get_or_insert(t0);
        let t = (t0 - since) % ALERT_EVERY;
        if t < ALERT_SHOW {
            if !self.alert_shown || self.alert_screen != Some(screen) {
                self.draw_alert_text(ox, oy, w, hud);
            }
            self.alert_shown = true;
        } else if self.alert_shown {
            self.aw.w.park();
            self.alert_shown = false;
        }
        if self.alert_screen != Some(screen) {
            self.ew.w.show(&edge_image(screen.2.max(1) as usize, screen.3.max(1) as usize), screen.0, screen.1);
            self.ew.w.click_through(true);
            self.alert_alpha = 255;
        }
        self.alert_screen = Some(screen);
        let a = (255.0 * hit_opacity(t)).round_ties_even() as i32;
        if a != self.alert_alpha {
            self.ew.w.set_alpha(a);
            self.alert_alpha = a;
        }
    }

    fn hide_alert(&mut self) {
        if self.alert_since.is_none() {
            return;
        }
        self.aw.w.park();
        self.alert_shown = false;
        self.alert_since = None;
        if self.alert_screen.is_some() {
            self.ew.w.set_alpha(0);
            self.alert_alpha = 0;
        }
    }

    fn game_in_front(&self, v: &View) -> bool {
        let Some(game) = v.hwnd else { return true };
        if self.dragging.is_some() || self.toast.drag.is_some() {
            return true;
        }
        let fg = layered::foreground();
        fg == game || self.wins().iter().any(|w| w.hwnd == fg)
    }

    fn show_example(&mut self, v: &View) {
        let (w, h) = v.frame_size;
        let on = !matches!(self.cfg_overlay(v, "dps_window"), Value::Bool(false)) && self.cfg_overlay(v, "dps_example").truthy() && !v.fight
            && self.hide_at.is_none() && w > 0 && h > 0;
        if on {
            let t = now();
            if t - self.example_at >= OVERLAY_REDRAW {
                self.example_at = t;
                self.example = true;
                let ex = alogs_pyval::dict([("avg", Value::Int(12345)), ("recent", Value::Int(15678)), ("damage", Value::Int(987654)),
                    ("max_hp", Value::Int(2500000)), ("elapsed", Value::Int(80)), ("title", Value::None)]);
                self.show_dps(v, (v.origin.0, v.origin.1, w as i32, h as i32), v.bossrush_space, Some(stat_lines(&ex)), None);
            }
        } else if self.example {
            self.example = false;
            if !v.fight && self.hide_at.is_none() {
                self.hide_dps();
            }
        }
    }

    /// Dragging the DPS window (unlocked), an HP label or the notice sample; double-click resets a label's spot.
    fn mouse(&mut self, v: &View) {
        for (h, kind, x, y) in layered::take_events() {
            let is_dps = h == self.dw.w.hwnd;
            let label = self.hp.iter().find(|(_, e)| e.0.w.hwnd == h).map(|(n, _)| *n);
            let is_toast = h == self.tw.w.hwnd;
            match kind {
                Mouse::Right if is_dps => std::process::exit(0), // right-click closes the meter, as before
                Mouse::Down if is_dps && self.dps_locked(v) => {}
                Mouse::Down if h == self.aw.w.hwnd => {
                    if let Some(g) = v.hwnd {
                        layered::bring_back(g);
                    }
                }
                Mouse::Down if is_toast => {
                    if let Some(s) = self.toast.sample {
                        self.toast.drag = Some((x - s.0, y - s.1));
                    }
                }
                Mouse::Down => {
                    let at = if is_dps { self.dw.placed.as_ref() } else { label.and_then(|n| self.hp[n].0.placed.as_ref()) };
                    if let Some(p) = at {
                        self.dragging = Some((h, x - p.1, y - p.2));
                    }
                }
                Mouse::Move => {
                    if let Some((dh, gx, gy)) = self.dragging
                        && dh == h
                    {
                        let which: &str = if is_dps { "dps" } else { label.unwrap_or("dps") };
                        if let Some(img) = self.win_mut(which).placed.as_ref().map(|p| p.0.clone()) {
                            self.present(which, img, x - gx, y - gy);
                        }
                    } else if is_toast
                        && let (Some(d), Some(_)) = (self.toast.drag, self.toast.sample)
                    {
                        let (nx, ny) = (x - d.0, y - d.1);
                        self.toast.sample = Some((nx, ny));
                        if let Some(img) = self.tw.placed.as_ref().map(|p| p.0.clone()) {
                            self.present("toast", img, nx, ny);
                        }
                    }
                }
                Mouse::Up => {
                    if is_toast && self.toast.drag.is_some() {
                        self.toast.drag = None;
                        if let Some(s) = self.toast.sample.take() {
                            let ((ox, oy, _, hh), _) = toast_screen(v);
                            self.pos.insert("notice".into(), serde_json::json!([(f64::from(s.0) - f64::from(ox)) / f64::from(hh), (f64::from(s.1) - f64::from(oy)) / f64::from(hh)]));
                            self.save_positions();
                        }
                    } else if self.dragging.take().is_some() {
                        if is_dps {
                            if let (Some(b), Some(p)) = (self.dps_base, &self.dw.placed) {
                                let off = serde_json::json!([(f64::from(p.1) - b.0) / self.dps_h, (f64::from(p.2) - b.1) / self.dps_h]);
                                self.pos.insert(self.dps_key.clone(), off);
                                self.save_positions();
                            }
                        } else if let Some(n) = label
                            && let (Some(b), span, Some(p)) = (self.hp[n].1, self.hp[n].2, self.hp[n].0.placed.as_ref())
                        {
                            let off = serde_json::json!([(f64::from(p.1) - b.0) / span, (f64::from(p.2) - b.1) / span]);
                            self.pos.insert(pos_key(n), off);
                            self.save_positions();
                        }
                    }
                }
                Mouse::Double => {
                    if let Some(n) = label {
                        self.pos.remove(&pos_key(n));
                        self.save_positions();
                    } else if is_toast {
                        self.pos.remove("notice");
                        self.save_positions();
                        self.toast.drag = None;
                        self.toast.sample = None;
                    }
                }
                _ => {}
            }
        }
    }

    /// One look of the window loop. False once the meter has stopped.
    pub fn tick(&mut self) -> bool {
        layered::pump();
        let v = self.view.lock().unwrap().clone();
        if !v.running {
            return false;
        }
        self.mouse(&v);
        if now() - self.published > 1.0 {
            self.publish_own(now());
        }
        // in captures unless the player chose otherwise
        let want_in = !self.hide;
        if self.in_capture != Some(want_in) {
            self.in_capture = Some(want_in);
            self.own.lock().unwrap().in_capture = want_in;
            for w in self.wins() {
                w.hide_from_capture(!want_in);
            }
            for e in self.hp.values() {
                // the HP labels sit on the bar the meter reads: kept out of captures (and so not covered, where that
                // took: Windows 10 2004 and later)
                let h = e.0.w.hwnd;
                if e.0.w.hide_from_capture(true) {
                    self.uncaptured.insert(h);
                } else {
                    self.uncaptured.remove(&h);
                }
            }
            self.publish_own(now());
        }
        let front = self.game_in_front(&v);
        if self.away != !front {
            self.away = !front;
            for which in ["dps", "toast", "pct", "bar", "total", "break"] {
                let away = self.away;
                let win = self.win_mut(which);
                if away {
                    win.w.park();
                } else if let Some((img, x, y)) = &win.placed {
                    win.w.show(img, *x, *y);
                }
            }
        }
        let mut msgs = vec![];
        while let Ok(m) = self.rx.try_recv() {
            msgs.push(m);
        }
        if v.quiet || v.capture_off {
            if !self.dps_quiet {
                self.dps_quiet = true;
                self.hide_alert();
                self.hide_hp();
                self.hide_dps();
                self.hide_at = None;
                self.dps_end = false;
                self.pending = None;
            }
            for (k, d) in msgs {
                if k == "toast" {
                    self.queue_toast(&d);
                }
            }
            self.toast_tick(&v);
            return true;
        }
        self.dps_quiet = false;
        if self.away && now() - v.hit_at < 5.0 {
            self.show_alert(&v);
        } else {
            self.hide_alert();
        }
        let mut latest: Option<Value> = None;
        let show_dps_window = !matches!(self.cfg_overlay(&v, "dps_window"), Value::Bool(false));
        for (k, d) in msgs {
            if k == "fight" {
                latest = Some(d);
                continue;
            }
            if k == "toast" {
                self.queue_toast(&d);
                continue;
            }
            if let Some(l) = latest.take() {
                self.show_fight(&v, &l);
                self.pending = None;
                self.drawn_at = now();
            }
            let s = d.as_str().unwrap_or("").to_string();
            match k.as_str() {
                "fight_end" => {
                    self.pending = None;
                    self.hide_hp();
                    let keep = if d.get("bossrush").truthy() { 10 } else if d.get("holosim").truthy() { 0 } else { alogs_pyval::py_int(&self.cfg_overlay(&v, "result_seconds")).unwrap_or(10) };
                    if show_dps_window && self.dps_shown && keep > 0 {
                        if let Some(screen) = screen_of(&d) {
                            let bar = d.get("bar").as_list().and_then(|b| b.first().and_then(Value::as_f64));
                            self.show_dps(&v, screen, d.get("bossrush").truthy(), Some(final_lines(&d)), bar);
                        }
                        self.hide_at = Some(now() + keep as f64);
                    } else if self.dps_shown {
                        self.hide_dps();
                    }
                }
                "status" if s.starts_with("fight over") => {
                    self.hide_hp();
                    if self.dps_shown && self.hide_at.is_none() {
                        self.hide_dps();
                    }
                }
                "status" if s.starts_with("result recorded") => {
                    self.show_end(&v, &end_note_result(&s), 0.665);
                    self.hide_at = Some(now() + 30.0);
                }
                "status" if s.starts_with("Holo-Battle Sim recorded") => {
                    self.show_end(&v, &end_note_sim(&s), 0.86);
                    self.hide_at = Some(now() + 15.0);
                }
                "status" if s.starts_with("Battle Data") => {
                    self.show_end(&v, &end_note_battle_data(&s), 0.765);
                    self.hide_at = Some(now() + 15.0);
                }
                // a bar with no fight yet: its % at once, then at the fights' pace
                "bar" if !v.fight && (self.bar_only.is_none() || now() - self.drawn_at >= OVERLAY_REDRAW) => {
                    self.show_bar(&v, &d);
                    self.drawn_at = now();
                }
                "bar" if !v.fight => self.bar_only = self.bar_only.map(|_| now()),
                "bar_gone" if self.bar_only.is_some() => {
                    self.hide_hp();
                    (self.hp_up, self.bar_only) = (false, None);
                }
                "end_hide" => {
                    if self.dps_end {
                        self.hide_dps();
                        self.dps_end = false;
                    }
                }
                _ => {}
            }
        }
        if latest.is_some() {
            self.pending = latest;
        }
        self.toast_tick(&v);
        if self.pending.is_some() && now() - self.drawn_at >= OVERLAY_REDRAW {
            let p = self.pending.take().unwrap();
            self.show_fight(&v, &p);
            self.drawn_at = now();
        }
        // (should the meter's "bar_gone" never come: a bar's label is not left standing)
        if !v.fight && self.bar_only.is_some_and(|t| now() - t > BAR_ONLY_KEEP) {
            self.hide_hp();
            (self.hp_up, self.bar_only) = (false, None);
        }
        if v.fight && now() - v.bar_seen >= BAR_HIDDEN && self.hp_up {
            self.hide_hp();
            self.hp_up = false;
        }
        if v.fight && v.holosim_space && self.dps_shown && self.hide_at.is_none() && now() - v.bar_seen >= 2.0 {
            self.hide_dps();
        }
        if self.hide_at.is_some_and(|t| now() > t) && !v.fight {
            self.hide_dps();
            self.hide_at = None;
        }
        self.show_example(&v);
        true
    }

    fn queue_toast(&mut self, d: &Value) {
        match d {
            Value::List(l) if l.len() >= 3 => self.toast.queue.push((l[0].py_str(), l[1].py_str(), l[2].py_str(), None, "record".into())),
            Value::Dict(_) => {
                let screen = d.get("screen").as_list().filter(|s| s.len() == 4).map(|s| (s[0].as_i64().unwrap_or(0) as i32, s[1].as_i64().unwrap_or(0) as i32, s[2].as_i64().unwrap_or(0) as i32, s[3].as_i64().unwrap_or(0) as i32));
                self.toast.queue.push((d.get("head").py_str(), if d.get("detail").is_none() { String::new() } else { d.get("detail").py_str() },
                    d.get("accent").as_str().unwrap_or("#5cb8ff").to_string(), screen, d.get("kind").as_str().unwrap_or("notice").to_string()));
            }
            _ => {}
        }
    }

    /// Sanctum Helper's notice files, four times a second: into the queue in the order they came, then deleted.
    fn take_notices(&mut self, t: f64) {
        if t - self.toast.looked < 0.25 {
            return;
        }
        self.toast.looked = t;
        let dir = self.data.join("notices");
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        let mut names: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
        names.sort();
        let hex = regex::Regex::new(r"^#[0-9a-fA-F]{6}$").unwrap();
        for p in names {
            if let Some(d) = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
                let screen = d.get("screen").and_then(|s| s.as_array()).filter(|s| s.len() == 4).and_then(|s| {
                    let v: Vec<i32> = s.iter().filter_map(|x| x.as_f64().map(|f| f as i32)).collect();
                    (v.len() == 4 && v[2] > 0 && v[3] > 0).then(|| (v[0], v[1], v[2], v[3]))
                });
                let accent = d.get("accent").and_then(|a| a.as_str()).filter(|a| hex.is_match(a)).unwrap_or("#5cb8ff").to_string();
                if let Some(head) = d.get("head").and_then(|h| h.as_str()).filter(|h| !h.is_empty()) {
                    let detail = d.get("detail").and_then(|x| x.as_str()).unwrap_or("");
                    self.toast.queue.push((head.chars().take(120).collect(), detail.chars().take(120).collect(), accent, screen,
                        d.get("from").and_then(|f| f.as_str()).unwrap_or("notice").to_string()));
                }
            }
            let _ = std::fs::remove_file(&p);
        }
    }

    fn toast_image(&self, head: &str, detail: &str, accent: &str, s: f64) -> Image {
        let (w, h, r) = ((TOAST_W * s).round_ties_even() as usize, (TOAST_H * s).round_ties_even() as usize, (12.0 * s).round_ties_even() as i64);
        let mut img = Image::new(Mode::Rgba, w, h, &[0, 0, 0, 0]);
        rounded_rect(&mut img, (0, 0, w as i64 - 1, h as i64 - 1), r, [20, 22, 31, 235], Some(([255, 255, 255, 70], (s.round_ties_even() as i64).max(1))));
        let (bx, bw) = ((14.0 * s).round_ties_even() as i64, ((5.0 * s).round_ties_even() as i64).max(3));
        let a = rgb(accent);
        rounded_rect(&mut img, (bx, (10.0 * s).round_ties_even() as i64, bx + bw, h as i64 - (10.0 * s).round_ties_even() as i64), bw / 2 + 1, [a[0], a[1], a[2], 255], None);
        let tx = bx + bw + (12.0 * s).round_ties_even() as i64;
        let room = w as f32 - tx as f32 - (16.0 * s) as f32;
        let Some(font) = &self.text.plain else { return img };
        let fit = |text: &str, px: f32| -> String {
            if font.length(text, px) <= room {
                return text.to_string();
            }
            let mut t: Vec<char> = text.chars().collect();
            while !t.is_empty() && font.length(&(t.iter().collect::<String>() + "…"), px) > room {
                t.pop();
            }
            t.iter().collect::<String>().trim_end_matches([' ', '·']).to_string() + "…"
        };
        let mut put = |text: &str, px: f32, cy: f32, c: [u8; 3]| {
            let mut mask = Image::new(Mode::L, w, h, &[0]);
            font.draw(&mut mask, tx as f32, cy - px * 0.72, text, px, 255);
            let mut layer = Image::new(Mode::Rgba, w, h, &[c[0], c[1], c[2], 0]);
            layer.putalpha(&mask);
            img.alpha_composite(&layer, 0, 0);
        };
        let (big, small) = ((16.0 * s).round_ties_even().max(11.0) as f32, (14.0 * s).round_ties_even().max(10.0) as f32);
        if detail.is_empty() {
            put(&fit(head, big), big, h as f32 / 2.0, [255, 255, 255]);
        } else {
            put(&fit(head, big), big, h as f32 * 0.36, [255, 255, 255]);
            put(&fit(detail, small), small, h as f32 * 0.68, [196, 202, 216]);
        }
        img
    }

    fn toast_spot(&self, place: &str, screen: (i32, i32, i32, i32), w: f64, h: f64, s: f64) -> (f64, f64, &'static str) {
        let (ox, oy, sw, sh) = (f64::from(screen.0), f64::from(screen.1), f64::from(screen.2), f64::from(screen.3));
        let m = (TOAST_MARGIN * s).round_ties_even();
        if place == "custom"
            && let Some(sp) = self.pos.get("notice").and_then(|o| Some((o.get(0)?.as_f64()?, o.get(1)?.as_f64()?)))
        {
            let x = (ox + sp.0 * sh).max(ox).min(ox + sw - w);
            let y = (oy + sp.1 * sh).max(oy).min(oy + sh - h);
            return (x, y, if x + w / 2.0 < ox + sw / 2.0 { "left" } else { "right" });
        }
        match place {
            "top-center" => (ox + (sw - w) / 2.0, oy + (150.0 * s).round_ties_even(), "top"),
            "right" => (ox + sw - w - m, oy + sh * 0.42, "right"),
            "left" => (ox + m, oy + sh * 0.42, "left"),
            _ => (ox + sw - w - m, oy + ((TOAST_MARGIN + 64.0 + 10.0) * s).round_ties_even(), "right"),
        }
    }

    fn toast_tick(&mut self, v: &View) {
        let t = now();
        self.take_notices(t);
        self.toast.queue.retain(|n| !matches!(v.notice_owners.get_str(&n.4), Some(Value::Bool(false))));
        let n = v.cfg.get("notifications");
        let place = n.get("place").as_str().unwrap_or("top-right").to_string();
        let motion = n.get("motion").as_str().unwrap_or("fade").to_string();
        let locked = !matches!(n.get("locked"), Value::Bool(false));
        // custom and unlocked: the sample card to drag
        let want_sample = place == "custom" && !locked && self.toast.since.is_none() && !self.away && !self.hide;
        if !want_sample {
            if self.toast.sample.is_some() && self.toast.drag.is_none() {
                self.park("toast");
                self.toast.sample = None;
            }
        } else if self.toast.sample.is_none() && self.toast.drag.is_none() {
            let (screen, s) = toast_screen(v);
            let img = self.toast_image("Drag me where aLogs notices should show", "Lock it in General Settings \u{203a} Notifications. Double-click: top right", "#c4cad8", s);
            let (x, y, _) = self.toast_spot(if self.pos.contains_key("notice") { "custom" } else { "top-right" }, screen, img.width as f64, img.height as f64, s);
            let (xi, yi) = (x.round_ties_even() as i32, y.round_ties_even() as i32);
            self.toast.sample = Some((xi, yi));
            self.toast.alpha = -1;
            self.present("toast", img, xi, yi);
            self.tw.w.click_through(false);
            self.tw.w.set_alpha(255);
        }
        if self.toast.sample.is_some() || self.toast.drag.is_some() {
            return;
        }
        if self.toast.since.is_none() {
            if self.toast.queue.is_empty() || self.away || self.hide {
                return;
            }
            let first = self.toast.queue[0].clone();
            let records = self.toast.queue.iter().take_while(|q| q.4 == "record").count();
            let (head, detail, accent) = if records >= 3 {
                self.toast.queue.drain(..records);
                (format!("{records} records sent to the leaderboards"), String::new(), "#7fb2ff".to_string())
            } else {
                self.toast.queue.remove(0);
                (first.0.clone(), first.1.clone(), first.2.clone())
            };
            let (screen, s) = match (v.last_screen, first.3) {
                (None, Some(sc)) => (sc, notice_scale(sc)),
                _ => toast_screen(v),
            };
            let img = self.toast_image(&head, &detail, &accent, s);
            let (x, y, edge) = self.toast_spot(&place, screen, img.width as f64, img.height as f64, s);
            self.toast.since = Some(t);
            self.toast.img = Some(img);
            self.toast.at = (x, y);
            self.toast.edge = edge;
            self.toast.motion = motion;
            self.toast.screen = screen;
            self.toast.alpha = -1;
            self.toast.shown = None;
            log::info!(target: "meter", "notice: {head}{}", if detail.is_empty() { String::new() } else { format!(" - {detail}") });
        }
        let el = t - self.toast.since.unwrap();
        if el >= TOAST_SHOW {
            self.park("toast");
            self.toast.since = None;
            return;
        }
        let e = (el / TOAST_IN).min((TOAST_SHOW - el) / TOAST_OUT).min(1.0);
        let e = 1.0 - (1.0 - e).powi(3);
        let (mut x, mut y) = self.toast.at;
        let img = self.toast.img.clone().unwrap();
        let (ox, oy, sw, _) = self.toast.screen;
        let (ox, oy, sw) = (f64::from(ox), f64::from(oy), f64::from(sw));
        if self.toast.motion == "slide" {
            let gap = match self.toast.edge {
                "right" => ox + sw - x,
                "left" => x - ox + img.width as f64,
                _ => y - oy + img.height as f64,
            };
            match self.toast.edge {
                "right" => x += (1.0 - e) * gap,
                "left" => x -= (1.0 - e) * gap,
                _ => y -= (1.0 - e) * gap,
            }
        } else if self.toast.motion == "drop" {
            y -= (1.0 - e) * (y - oy + img.height as f64);
        }
        let spot = (x.round_ties_even() as i32, y.round_ties_even() as i32);
        if Some(spot) != self.toast.shown && !self.away {
            self.present("toast", img, spot.0, spot.1);
            self.tw.w.click_through(true);
            self.toast.shown = Some(spot);
            self.toast.alpha = -1;
        }
        let a = (255.0 * e).round_ties_even() as i32;
        if a != self.toast.alpha && !self.away {
            self.tw.w.set_alpha(a);
            self.toast.alpha = a;
        }
    }

    /// The window loop, until the meter stops.
    pub fn run(mut self) {
        while self.tick() {
            std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
        }
        self.save_positions();
    }
}

/// Where the game is on screen, for a notice or the alert: where its last fight was drawn, else the game window's
/// place with the size of its last whole picture (the meter has both from its first look), else a 1080p screen.
/// (Only a fight sets `last`. Until 19 Nov a notice that came before the first fight since the meter started was
/// drawn for 1920x1080 at the top left: on a 3840x2160 screen two "Interlink best submitted" notices from the battle prep
/// screen came out 360x64 px, half size, and slid in from the middle of the top edge.)
pub fn game_screen(last: Option<(i32, i32, i32, i32)>, origin: (i32, i32), frame: (usize, usize)) -> (i32, i32, i32, i32) {
    last.or_else(|| (frame.0 > 0 && frame.1 > 0).then(|| (origin.0, origin.1, frame.0 as i32, frame.1 as i32))).unwrap_or((0, 0, 1920, 1080))
}

/// How much bigger than at 1080p a notice is drawn on this screen (by the 16:9 part of it).
pub fn notice_scale(screen: (i32, i32, i32, i32)) -> f64 {
    f64::from(screen.2).min(f64::from(screen.3) * 16.0 / 9.0) * 9.0 / 16.0 / 1080.0
}

fn toast_screen(v: &View) -> ((i32, i32, i32, i32), f64) {
    let screen = game_screen(v.last_screen, v.origin, v.frame_size);
    (screen, notice_scale(screen))
}

fn screen_of(d: &Value) -> Option<(i32, i32, i32, i32)> {
    let s = d.get("screen").as_list()?;
    (s.len() == 4).then(|| (s[0].as_i64().unwrap_or(0) as i32, s[1].as_i64().unwrap_or(0) as i32, s[2].as_i64().unwrap_or(0) as i32, s[3].as_i64().unwrap_or(0) as i32))
}

fn hit_opacity(t: f64) -> f64 {
    for w in HIT_CURVE.windows(2) {
        let ((t0, v0), (t1, v1)) = (w[0], w[1]);
        if t0 <= t && t <= t1 {
            return v0 + (v1 - v0) * (t - t0) / (t1 - t0);
        }
    }
    0.0
}

/// Red around the screen's edges, fading to nothing towards the middle (drawn small, then enlarged).
fn edge_image(sw: usize, sh: usize) -> Image {
    let (w, h) = ((sw / 8).max(16), (sh / 8).max(9));
    let depth = 0.1 * w.min(h) as f64;
    let ramp = |n: usize| -> Vec<u8> { (0..n).map(|i| (220.0 * (1.0 - i as f64 / depth).max(0.0).powf(2.2)).round_ties_even() as u8).collect() };
    let (rh, rw) = (ramp(h), ramp(w));
    let mut col = Image::new(Mode::L, w, h, &[0]);
    let mut rowi = Image::new(Mode::L, w, h, &[0]);
    for y in 0..h {
        for x in 0..w {
            col.data[y * w + x] = rh[y].max(rh[h - 1 - y]);
            rowi.data[y * w + x] = rw[x].max(rw[w - 1 - x]);
        }
    }
    let mask = lighter(&col, &rowi).resize(sw, sh, Some(Filter::Bilinear));
    let mut img = Image::new(Mode::Rgba, sw, sh, &[235, 30, 30, 0]);
    img.putalpha(&mask);
    img
}

fn boss_row(d: &Value) -> Vec<Line> {
    match d.get("boss").as_list() {
        Some(b) if b.len() == 2 => vec![row("Boss", &format!("{}/{}", b[0].py_str(), b[1].py_str()), 0.015)],
        _ => vec![],
    }
}

/// DPS, then rows for the last 5 s, team damage and the fight's time (dashes until the first hit).
pub fn stat_lines(d: &Value) -> Vec<Line> {
    let f = |k: &str| d.get(k).as_f64().unwrap_or(0.0);
    let hit = f("damage") > 0.0;
    let mut v = vec![
        line(&if hit { format!("{} DPS", fmt(f("avg"))) } else { "— DPS".into() }, "#ffd36a", 0.0235, "ib"),
        row("Last 5s", &if hit { format!("{} DPS", fmt(f("recent"))) } else { "waiting...".into() }, 0.015),
        row("Team", &format!("{} / {} HP", fmt(f("damage")), fmt(f("max_hp"))), 0.015),
        row("Time", &clock(f("elapsed")), 0.015),
    ];
    v.extend(boss_row(d));
    v
}

/// The fight's result, left in the stats spot for a few seconds.
pub fn final_lines(d: &Value) -> Vec<Line> {
    let f = |k: &str| d.get(k).as_f64().unwrap_or(0.0);
    let result = if d.get("defeated").truthy() && d.get("title").truthy() { format!("Defeated {}", d.get("title").py_str()) } else if d.get("defeated").truthy() { "Defeated".into() } else { "Fight over".into() };
    let mut v = vec![
        line(&format!("{} DPS", fmt(f("avg"))), "#ffd36a", 0.0235, "ib"),
        row("Result", &result, 0.015),
        row("Team", &format!("{} / {} HP", fmt(f("damage")), fmt(f("max_hp"))), 0.015),
        row("Time", &clock(f("elapsed")), 0.015),
    ];
    v.extend(boss_row(d));
    v
}

/// (text, colour) for the break gauge line under the boss bar, or None while it can't be read.
pub fn break_text(b: &Value) -> Option<(String, &'static str)> {
    if b.get("left").is_none() && !b.get("breaks").truthy() {
        return None;
    }
    let rate = if b.get("rate_now").is_none() { b.get("rate").as_f64() } else { b.get("rate_now").as_f64() };
    let n = b.get("breaks").as_i64().unwrap_or(0);
    let count = if n != 0 { format!(" · {n} break{}", if n != 1 { "s" } else { "" }) } else { String::new() };
    let left = b.get("left").as_f64().unwrap_or(0.0);
    Some(match b.get("phase").as_str() {
        Some("broken") => (format!("BROKEN{count}"), "#ffe066"),
        Some("refill") => (format!("BREAK gauge refilling{count}"), "#c8d0dc"),
        _ => {
            let text = match b.get("size").as_f64().filter(|s| *s != 0.0) {
                Some(size) => format!("BREAK {} / {}{}{count}", fmt(left * size), fmt(size), rate.map_or(String::new(), |r| format!(" · {}/s", fmt(r * size / 100.0)))),
                None => format!("BREAK {:.0}% left{}{count}", left * 100.0, rate.map_or(String::new(), |r| format!(" · {r:.1}%/s"))),
            };
            (text, if b.get("kind").as_str() == Some("orange") { "#ffc58a" } else { "#dfe8f5" })
        }
    })
}

/// 'result recorded: Omega Sherro - score 11232, 11 stars, 180s' -> 'Result saved\nOmega Sherro · 11,232 · 11 stars · 3:00'.
pub fn end_note_result(d: &str) -> String {
    let re = regex::Regex::new(r"^result recorded: (.*) - score (\w+), (\w+) stars, (\w+)s$").unwrap();
    let Some(m) = re.captures(d) else { return "Result saved".into() };
    let num = |s: &str| s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty();
    let mut parts = vec![m[1].to_string()];
    if num(&m[2]) {
        let n = m[2].to_string();
        let mut o = String::new();
        for (i, c) in n.chars().enumerate() {
            if i > 0 && (n.len() - i) % 3 == 0 {
                o.push(',');
            }
            o.push(c);
        }
        parts.push(o);
    }
    if num(&m[3]) {
        parts.push(format!("{} stars", &m[3]));
    }
    if num(&m[4]) {
        let s: i64 = m[4].parse().unwrap_or(0);
        parts.push(format!("{}:{:02}", s / 60, s % 60));
    }
    format!("Result saved\n{}", parts.join(" · "))
}

pub fn end_note_sim(d: &str) -> String {
    let re = regex::Regex::new(r"^Holo-Battle Sim recorded: (.*) in (\d+) s(?: \((.*)\))?$").unwrap();
    let Some(m) = re.captures(d) else { return "Holo-Battle Sim saved".into() };
    let s: i64 = m[2].parse().unwrap_or(0);
    let mut parts = vec![m[1].to_string(), format!("{}:{:02}", s / 60, s % 60)];
    if let Some(e) = m.get(3) {
        parts.push(e.as_str().to_string());
    }
    format!("Holo-Battle Sim saved\n{}", parts.join(" · "))
}

pub fn end_note_battle_data(d: &str) -> String {
    let game = regex::Regex::new(r"game team DPS ([\d,]+)").unwrap().captures(d).map(|m| m[1].to_string());
    let meter = regex::Regex::new(r"vs meter ([\d,]+)").unwrap().captures(d).map(|m| m[1].to_string());
    match game {
        None => "Battle Data saved".into(),
        Some(g) => format!("Battle Data saved\nTeam DPS {g}{}", meter.map_or(String::new(), |m| format!(" · meter {m}"))),
    }
}

impl crate::meter::Meter {
    /// What the overlay looks at, as of now.
    pub fn overlay_view(&self) -> View {
        View {
            fight: self.fight.is_some(),
            bar_seen: self.bar_seen,
            hit_at: self.hit_at,
            last_screen: self.last_screen.map(|(x, y, w, h)| (x, y, w as i32, h as i32)),
            frame_size: self.frame_size,
            origin: self.origin,
            cfg: self.args.cfg.clone(),
            quiet: self.quiet,
            capture_off: self.capture_off,
            bossrush_space: self.bossrush_space,
            holosim_space: self.holosim_space,
            hwnd: self.hwnd,
            notice_owners: self.manifest.notice_owners(&self.args.cfg, !self.capture_off, &self.health),
            running: true,
        }
    }
}
