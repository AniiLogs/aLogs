//! The screen-capture switch on the game: a small button (the aLogs icon, or the player's own picture) that turns
//! screen capture on and off with one click. Bright while capture is on, dim while it's off.
//!
//! - Left click: on / off. Drag it anywhere (unless locked).
//! - Right click: a menu to switch capture, lock the button in place, use your own picture (any image, shrunk to
//!   fit the button) or the aLogs icon again, or hide the button (aLogs › General shows it again).
//! - It shows only while the game is in front, and never takes the focus from the game when clicked.
//! - By default it sits in the game's own icon row at the bottom left (after Chat, Photo and Emoticon), sized like
//!   those icons, with its hotkey under it in the game's key label style (no label without a hotkey), and it hides
//!   whenever the game hides those icons (menus, the map, cutscenes). Dragged elsewhere, it stays where it was put.
//! - Hotkey (Insert by default, a key the game doesn't use; changeable in General): only while the game is in front.
//!
//! Everything in the icon row is measured on the game's 4K layout and scaled with its UI area (a 16:9 box: on
//! ultrawide screens centred, see Sanctum Helper's Regions.Hud), anchored to the bottom left like the game's own.
//!
//! A native layered window rather than a web view: a click on it must not activate it (the game would lose the
//! focus, and Desktop Duplication only captures while the game is in front).
//! Its state lives in `<data>/overlay_button.json`: { capture_on, show, x, y, locked, icon, hotkey_vk, hotkey_label }.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter};
use windows::core::{w, Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GdiFlush, GetDC,
    GetTextExtentPoint32W, MonitorFromPoint, ReleaseDC, SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, MONITOR_DEFAULTTONULL, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmapSource, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeHighQualityCubic, WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OPENFILENAMEW};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, RegisterHotKey, ReleaseCapture, SetCapture, UnregisterHotKey, MOD_NOREPEAT};
use windows::Win32::UI::WindowsAndMessaging::*;

const BASE_SIZE: i32 = 44; // the button at 100% display scale (px), when dragged off the icon row
const HOTKEY_ID: i32 = 1;
const VK_INSERT: u32 = 0x2D;

// The game's icon row at 4K (3840x2160 UI area), measured on its HUD: Chat, Photo and Emoticon at x 378 / 505 / 632,
// 127 px apart, icons ~84 px, centred 133 px above the bottom; their key labels are 40 px high pills centred 59 px
// above the bottom, light grey (227) with dark text (37, 44, 53), radius ~8, at least 42 px wide.
const ROW_X: f64 = 632.0 + 127.0; // our slot: right after the Emoticon icon
const ROW_ICON: f64 = 84.0;
const ROW_ICON_UP: f64 = 133.0;
const ROW_PILL_UP: f64 = 59.0;
const ROW_PILL_H: f64 = 40.0;
const ROW_PILL_MIN_W: f64 = 42.0;
const ROW_FONT: f64 = 34.0;
const PILL_BG: [u8; 3] = [227, 227, 227];
const PILL_TEXT: [u8; 3] = [37, 44, 53];
const WM_REFRESH: u32 = WM_APP + 1;
const TIMER_FOLLOW: usize = 1;
const DRAG_PX: i32 = 4; // movement before a press counts as a drag, not a click

const MENU_CAPTURE: usize = 1;
const MENU_LOCK: usize = 2;
const MENU_ICON: usize = 3;
const MENU_DEFAULT_ICON: usize = 4;
const MENU_HIDE: usize = 5;
const MENU_ROW: usize = 6;

/// The default picture: the app's own icon.
const APP_ICON: &[u8] = include_bytes!("../icons/128x128@2x.png");

static CAPTURE_ON: AtomicBool = AtomicBool::new(true);
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static APP: OnceLock<AppHandle> = OnceLock::new();
static STATE: Mutex<Option<State>> = Mutex::new(None);

#[derive(Clone)]
struct State {
    show: bool,
    x: Option<i32>,
    y: Option<i32>,
    locked: bool,
    icon: Option<PathBuf>,
    /// Virtual-key code of the hotkey (0: none) and how its label reads ("Ins").
    hotkey_vk: u32,
    hotkey_label: String,
}

fn state_path() -> PathBuf {
    crate::data_dir().join("overlay_button.json")
}

fn load() -> State {
    let v: Value = std::fs::read_to_string(state_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
    CAPTURE_ON.store(v["capture_on"].as_bool().unwrap_or(true), Ordering::SeqCst);
    State {
        show: v["show"].as_bool().unwrap_or(true),
        x: v["x"].as_i64().map(|n| n as i32),
        y: v["y"].as_i64().map(|n| n as i32),
        locked: v["locked"].as_bool().unwrap_or(false),
        icon: v["icon"].as_str().map(PathBuf::from).filter(|p| p.is_file()),
        hotkey_vk: v["hotkey_vk"].as_u64().map_or(VK_INSERT, |n| n as u32),
        hotkey_label: v["hotkey_label"].as_str().unwrap_or("Ins").to_string(),
    }
}

fn save(s: &State) {
    let v = json!({
        "capture_on": CAPTURE_ON.load(Ordering::SeqCst), "show": s.show, "x": s.x, "y": s.y, "locked": s.locked,
        "icon": s.icon.as_ref().map(|p| p.to_string_lossy().to_string()),
        "hotkey_vk": s.hotkey_vk, "hotkey_label": s.hotkey_label,
    });
    let path = state_path();
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string_pretty(&v).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE.lock().unwrap();
    let state = guard.get_or_insert_with(load);
    let out = f(state);
    save(state);
    out
}

/// Screen capture is on (the capture thread and Sanctum Helper follow this).
pub fn capture_on() -> bool {
    STATE.lock().unwrap().get_or_insert_with(load);
    CAPTURE_ON.load(Ordering::SeqCst)
}

/// What the app's settings page shows.
pub fn info() -> Value {
    let s = STATE.lock().unwrap().get_or_insert_with(load).clone();
    json!({ "capture_on": capture_on(), "show": s.show, "locked": s.locked, "custom_icon": s.icon.is_some(),
            "in_row": s.x.is_none(), "hotkey_vk": s.hotkey_vk, "hotkey_label": if s.hotkey_vk == 0 { "" } else { &s.hotkey_label } })
}

fn changed() {
    let hwnd = WINDOW.load(Ordering::SeqCst);
    if hwnd != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as _)), WM_REFRESH, WPARAM(0), LPARAM(0));
        }
    }
    if let Some(app) = APP.get() {
        let _ = app.emit("capture-switch", info());
    }
}

pub fn set_capture(on: bool) {
    with_state(|_| CAPTURE_ON.store(on, Ordering::SeqCst));
    eprintln!("screen capture switched {}", if on { "on" } else { "off" });
    changed();
}

pub fn set_shown(show: bool) {
    with_state(|s| s.show = show);
    changed();
}

pub fn set_locked(locked: bool) {
    with_state(|s| s.locked = locked);
    changed();
}

/// The hotkey (0: none) and its label under the button.
pub fn set_hotkey(vk: u32, label: &str) {
    with_state(|s| (s.hotkey_vk, s.hotkey_label) = (vk, label.chars().take(8).collect()));
    changed();
}

/// Back into the game's icon row (also for a button lost off screen).
pub fn reset_position() {
    with_state(|s| (s.x, s.y) = (None, None));
    changed();
}

pub fn use_default_icon() {
    with_state(|s| s.icon = None);
    changed();
}

/// Copies the player's picture into the data folder (so it stays if the original moves) and uses it.
fn use_icon(file: &std::path::Path) {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    let dest = crate::data_dir().join(format!("overlay_button_icon.{ext}"));
    if std::fs::copy(file, &dest).is_ok() && decode(&std::fs::read(&dest).unwrap_or_default(), 16).is_some() {
        with_state(|s| s.icon = Some(dest));
        changed();
    }
}

/// Starts the button's thread.
pub fn start(app: AppHandle, game_title: String) {
    let _ = APP.set(app);
    std::thread::Builder::new()
        .name("overlay-button".into())
        .spawn(move || unsafe { run(game_title) })
        .ok();
}

/// An image file's pixels (premultiplied BGRA), shrunk to fit `size` x `size` with its shape kept, centred.
fn decode(bytes: &[u8], size: u32) -> Option<Vec<u8>> {
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = factory.CreateStream().ok()?;
        stream.InitializeFromMemory(bytes).ok()?;
        let decoder = factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        let scale = size as f64 / w.max(h) as f64;
        let (sw, sh) = (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1));
        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler.Initialize(&frame, sw, sh, WICBitmapInterpolationModeHighQualityCubic).ok()?;
        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)
            .ok()?;
        let source: IWICBitmapSource = converter.cast().ok()?;
        let mut small = vec![0u8; (sw * sh * 4) as usize];
        source.CopyPixels(std::ptr::null(), sw * 4, &mut small).ok()?;
        let mut out = vec![0u8; (size * size * 4) as usize];
        let (ox, oy) = ((size - sw) / 2, (size - sh) / 2);
        for row in 0..sh {
            let from = (row * sw * 4) as usize;
            let to = (((oy + row) * size + ox) * 4) as usize;
            out[to..to + (sw * 4) as usize].copy_from_slice(&small[from..from + (sw * 4) as usize]);
        }
        Some(out)
    }
}

/// The button's picture at `size` px: bright while capture is on; grey and faint while it's off.
fn picture(size: u32) -> Vec<u8> {
    let icon = STATE.lock().unwrap().as_ref().and_then(|s| s.icon.clone());
    let own = icon.and_then(|p| std::fs::read(p).ok()).and_then(|b| decode(&b, size));
    let mut px = match own {
        Some(px) => px, // the player's own picture keeps its colours
        // the aLogs icon drawn like the game's own row icons: white, with the dark parts in the game's dark grey
        None => {
            let mut px = decode(APP_ICON, size).unwrap_or_else(|| vec![0u8; (size * size * 4) as usize]);
            for p in px.chunks_exact_mut(4) {
                let a = p[3] as u32;
                if a == 0 {
                    continue;
                }
                let un = |c: u8| (c as u32 * 255 / a).min(255);
                let lum = (un(p[2]) * 77 + un(p[1]) * 150 + un(p[0]) * 29) >> 8;
                let rgb = if lum >= 90 { [255u32, 255, 255] } else { [53, 44, 37] }; // B, G, R
                for c in 0..3 {
                    p[c] = (rgb[c] * a / 255) as u8;
                }
            }
            px
        }
    };
    if !CAPTURE_ON.load(Ordering::SeqCst) {
        for p in px.chunks_exact_mut(4) {
            // premultiplied: grey from the colours, then everything at 40%
            let grey = (p[0] as u32 * 29 + p[1] as u32 * 150 + p[2] as u32 * 77) >> 8;
            let g = (grey * 40 / 100) as u8;
            p[0] = g;
            p[1] = g;
            p[2] = g;
            p[3] = (p[3] as u32 * 40 / 100) as u8;
        }
    }
    px
}

/// Where the button goes and how big: its window rect on screen, the icon's size in it and the key pill (if any).
#[derive(Clone, Copy, PartialEq)]
struct Layout {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    icon: i32,
    pill: Option<(i32, i32, i32, i32)>, // x, y, w, h inside the window
    font: i32,
}

/// The game's UI area (a 16:9 box, centred on wider screens) of a client `w` x `h`, and its scale against 4K.
fn ui_box(w: i32, h: i32) -> (f64, f64) {
    let u = (w as f64).min(h as f64 * 16.0 / 9.0);
    ((w as f64 - u) / 2.0, u * 9.0 / 16.0 / 2160.0)
}

fn label() -> Option<String> {
    STATE.lock().unwrap().as_ref().filter(|s| s.hotkey_vk != 0 && !s.hotkey_label.is_empty()).map(|s| s.hotkey_label.clone())
}

/// The pill's width for `text` at `font` px (the game's pills fit their text with ~12 px each side at 4K).
unsafe fn pill_width(text: &str, font: i32, k: f64) -> i32 {
    let dc = CreateCompatibleDC(None);
    let f = CreateFontW(-font, 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"));
    let old = SelectObject(dc, f.into());
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
    SelectObject(dc, old);
    let _ = DeleteObject(f.into());
    let _ = DeleteDC(dc);
    (size.cx + (24.0 * k) as i32).max((ROW_PILL_MIN_W * k).round() as i32)
}

/// In the game's icon row (no saved spot), or where the player dragged it.
unsafe fn layout(hwnd: HWND, game: HWND) -> Option<Layout> {
    // a saved spot that's no longer on any screen (a monitor unplugged): back in the icon row
    let saved = STATE.lock().unwrap().as_ref().and_then(|s| s.x.zip(s.y)).filter(|&(x, y)| {
        !MonitorFromPoint(POINT { x: x + BASE_SIZE / 2, y: y + BASE_SIZE / 2 }, MONITOR_DEFAULTTONULL).is_invalid()
    });
    let text = label();
    let (k, icon, x0, y_icon) = match saved {
        None => {
            let mut rect = RECT::default();
            if game.is_invalid() || GetClientRect(game, &mut rect).is_err() || rect.bottom < 100 {
                return None;
            }
            let mut p = POINT::default();
            let _ = ClientToScreen(game, &mut p);
            let (bx, k) = ui_box(rect.right, rect.bottom);
            let icon = (ROW_ICON * k).round() as i32;
            let cx = p.x as f64 + bx + ROW_X * k;
            let cy = p.y as f64 + rect.bottom as f64 - ROW_ICON_UP * k;
            (k, icon, cx, cy)
        }
        Some((x, y)) => {
            let dpi = GetDpiForWindow(hwnd).max(96) as f64;
            let icon = (BASE_SIZE as f64 * dpi / 96.0).round() as i32;
            let k = icon as f64 / ROW_ICON;
            (k, icon, x as f64 + icon as f64 / 2.0, y as f64 + icon as f64 / 2.0)
        }
    };
    let font = (ROW_FONT * k).round().max(9.0) as i32;
    let pill_h = (ROW_PILL_H * k).round() as i32;
    let pill_w = text.as_deref().map(|t| pill_width(t, font, k));
    let w = icon.max(pill_w.unwrap_or(0));
    let gap = ((ROW_ICON_UP - ROW_PILL_UP) * k - (ROW_ICON + ROW_PILL_H) * k / 2.0).round() as i32;
    let h = icon + pill_w.map_or(0, |_| gap + pill_h);
    let pill = pill_w.map(|pw| ((w - pw) / 2, icon + gap, pw, pill_h));
    Some(Layout { x: (x0 - w as f64 / 2.0).round() as i32, y: (y_icon - icon as f64 / 2.0).round() as i32, w, h, icon, pill, font })
}

/// The key pill: a light grey rounded box with the hotkey in dark text, like the game's own labels.
unsafe fn draw_pill(dc: windows::Win32::Graphics::Gdi::HDC, bits: *mut u8, width: i32, l: &Layout, text: &str) {
    let Some((px, py, pw, ph)) = l.pill else { return };
    let r = (8.0 * ph as f64 / ROW_PILL_H).max(2.0);
    let inside = |x: i32, y: i32| -> f64 {
        // coverage of a pixel by the rounded rectangle (soft edge one pixel wide)
        let (fx, fy) = (x as f64 + 0.5 - px as f64, y as f64 + 0.5 - py as f64);
        let (cx, cy) = (fx.clamp(r, pw as f64 - r), fy.clamp(r, ph as f64 - r));
        let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
        (r + 0.5 - d).clamp(0.0, 1.0)
    };
    for y in py..py + ph {
        for x in px..px + pw {
            let a = inside(x, y);
            let at = ((y * width + x) * 4) as usize;
            for c in 0..3 {
                *bits.add(at + c) = (PILL_BG[2 - c] as f64 * a) as u8;
            }
            *bits.add(at + 3) = (255.0 * a) as u8;
        }
    }
    let _ = GdiFlush();
    let f = CreateFontW(-l.font, 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"));
    let old = SelectObject(dc, f.into());
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, COLORREF(PILL_TEXT[0] as u32 | (PILL_TEXT[1] as u32) << 8 | (PILL_TEXT[2] as u32) << 16));
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut rect = RECT { left: px, top: py, right: px + pw, bottom: py + ph };
    DrawTextW(dc, &mut wide, &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    let _ = GdiFlush();
    SelectObject(dc, old);
    let _ = DeleteObject(f.into());
    // GDI text leaves alpha at 0: the pill is solid, so give every pixel inside it its coverage back
    for y in py..py + ph {
        for x in px..px + pw {
            let at = ((y * width + x) * 4) as usize;
            *bits.add(at + 3) = (255.0 * inside(x, y)) as u8;
        }
    }
}

/// Draws the button (icon, and the key pill under it) at its spot (layered window, per-pixel alpha).
static LAST_LAYOUT: Mutex<Option<Layout>> = Mutex::new(None);

unsafe fn paint(hwnd: HWND, l: &Layout) {
    *LAST_LAYOUT.lock().unwrap() = Some(*l);
    let (w, h) = (l.w.max(1), l.h.max(1));
    let icon = picture(l.icon as u32);
    let screen = GetDC(None);
    let dc = CreateCompatibleDC(Some(screen));
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    if let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
        let out = bits as *mut u8;
        std::ptr::write_bytes(out, 0, (w * h * 4) as usize);
        let ox = (w - l.icon) / 2;
        for row in 0..l.icon {
            std::ptr::copy_nonoverlapping(icon.as_ptr().add((row * l.icon * 4) as usize), out.add(((row * w + ox) * 4) as usize), (l.icon * 4) as usize);
        }
        let old = SelectObject(dc, bitmap.into());
        if let Some(text) = label() {
            draw_pill(dc, out, w, l, &text);
            if !CAPTURE_ON.load(Ordering::SeqCst) {
                if let Some((px, py, pw, ph)) = l.pill {
                    for y in py..py + ph {
                        for x in px..px + pw {
                            let at = ((y * w + x) * 4) as usize;
                            for c in 0..4 {
                                *out.add(at + c) = (*out.add(at + c) as u32 * 55 / 100) as u8;
                            }
                        }
                    }
                }
            }
        }
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let at = POINT { x: l.x, y: l.y };
        let extent = SIZE { cx: w, cy: h };
        let zero = POINT::default();
        let _ = UpdateLayeredWindow(hwnd, Some(screen), Some(&at), Some(&extent), Some(dc), Some(&zero), COLORREF(0), Some(&blend), ULW_ALPHA);
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
    }
    let _ = DeleteDC(dc);
    ReleaseDC(None, screen);
}

/// The game's own icon row is showing: its Chat and Emoticon key pills (light grey with a dark key name) are where
/// they belong. Read from the app's shared capture, a small strip at the bottom left.
fn row_visible(reader: &mut crate::capture::AppReader, game: HWND) -> Option<bool> {
    let mut rect = RECT::default();
    if unsafe { GetClientRect(game, &mut rect) }.is_err() || rect.bottom < 100 {
        return None;
    }
    let (cw, ch) = (rect.right as f64, rect.bottom as f64);
    let (bx, k) = ui_box(rect.right, rect.bottom);
    // the strip from the Chat pill to the Emoticon pill, at 4K: x 340-660, y 2082-2120
    let (x0, x1) = (bx + 340.0 * k, bx + 660.0 * k);
    let (y0, y1) = (ch - (2160.0 - 2082.0) * k, ch - (2160.0 - 2120.0) * k);
    let share = |v: f64, size: f64| ((v / size).clamp(0.0, 1.0) * 65535.0) as u16;
    let area = [share(x0, cw), share(y0, ch), share(x1, cw), share(y1, ch)];
    let (w, h, px) = reader.read(game.0 as isize, area, 250)?;
    // inside each pill (4K x 352-400 for Enter, 620-646 for O): mostly light grey, some dark text
    let looks_like_pill = |from: f64, to: f64| {
        let (a, b) = ((((bx + from * k) - x0) * w as f64 / (x1 - x0)) as u32, (((bx + to * k) - x0) * w as f64 / (x1 - x0)) as u32);
        let (mut light, mut dark, mut n) = (0u32, 0u32, 0u32);
        for y in h / 6..h * 5 / 6 {
            for x in a.min(w)..b.min(w) {
                let p = &px[((y * w + x) * 4) as usize..][..3];
                let (lo, hi) = (*p.iter().min().unwrap(), *p.iter().max().unwrap());
                n += 1;
                light += (lo >= 185 && hi - lo < 30) as u32;
                dark += (hi < 110) as u32;
            }
        }
        n > 0 && light * 2 >= n && dark * 25 >= n && dark * 5 <= n * 2
    };
    Some(looks_like_pill(352.0, 400.0) && looks_like_pill(620.0, 646.0))
}

struct Press {
    start: POINT,
    window: POINT,
    dragging: bool,
}

static PRESS: Mutex<Option<(i32, i32, i32, i32, bool)>> = Mutex::new(None);

fn press() -> Option<Press> {
    PRESS.lock().unwrap().map(|(sx, sy, wx, wy, d)| Press { start: POINT { x: sx, y: sy }, window: POINT { x: wx, y: wy }, dragging: d })
}

fn set_press(p: Option<Press>) {
    *PRESS.lock().unwrap() = p.map(|p| (p.start.x, p.start.y, p.window.x, p.window.y, p.dragging));
}

unsafe fn menu(hwnd: HWND) {
    let s = STATE.lock().unwrap().clone().unwrap_or_else(load);
    let Ok(m) = CreatePopupMenu() else { return };
    let check = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
    let _ = AppendMenuW(m, MF_STRING | check(CAPTURE_ON.load(Ordering::SeqCst)), MENU_CAPTURE, w!("Screen capture"));
    let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(m, MF_STRING | check(s.locked), MENU_LOCK, w!("Lock in place"));
    let _ = AppendMenuW(m, MF_STRING, MENU_ICON, w!("Use my own picture…"));
    let row_flags = if s.x.is_some() { MF_STRING } else { MF_STRING | MF_GRAYED };
    let _ = AppendMenuW(m, row_flags, MENU_ROW, w!("Back to the game's icon row"));
    let default_flags = if s.icon.is_some() { MF_STRING } else { MF_STRING | MF_GRAYED };
    let _ = AppendMenuW(m, default_flags, MENU_DEFAULT_ICON, w!("Use the aLogs icon"));
    let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(m, MF_STRING, MENU_HIDE, w!("Hide this button (aLogs › General shows it again)"));
    let mut at = POINT::default();
    let _ = GetCursorPos(&mut at);
    // a popup menu only closes properly when its window is in front; give the game the focus back afterwards
    let previous = GetForegroundWindow();
    let _ = SetForegroundWindow(hwnd);
    let picked = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, at.x, at.y, None, hwnd, None);
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(m);
    if !previous.is_invalid() {
        let _ = SetForegroundWindow(previous);
    }
    match picked.0 as usize {
        MENU_CAPTURE => set_capture(!CAPTURE_ON.load(Ordering::SeqCst)),
        MENU_LOCK => set_locked(!s.locked),
        MENU_ICON => {
            if let Some(file) = pick_image(hwnd) {
                use_icon(&file);
            }
            if !previous.is_invalid() {
                let _ = SetForegroundWindow(previous);
            }
        }
        MENU_DEFAULT_ICON => use_default_icon(),
        MENU_HIDE => set_shown(false),
        MENU_ROW => reset_position(),
        _ => {}
    }
}

unsafe fn pick_image(owner: HWND) -> Option<PathBuf> {
    let mut file = [0u16; 1024];
    let filter: Vec<u16> = "Pictures (png, jpg, gif, bmp, ico, webp)\0*.png;*.jpg;*.jpeg;*.gif;*.bmp;*.ico;*.webp\0\0".encode_utf16().collect();
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        lpstrTitle: w!("A picture for the aLogs capture button"),
        Flags: OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !GetOpenFileNameW(&mut ofn).as_bool() {
        return None;
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Some(PathBuf::from(String::from_utf16_lossy(&file[..len])))
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize), // clicks never take the focus from the game
        WM_SETCURSOR => {
            let locked = STATE.lock().unwrap().as_ref().map_or(false, |s| s.locked);
            let _ = SetCursor(LoadCursorW(None, if locked { IDC_HAND } else { IDC_SIZEALL }).ok());
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            let mut at = POINT::default();
            let _ = GetCursorPos(&mut at);
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            set_press(Some(Press { start: at, window: POINT { x: rect.left, y: rect.top }, dragging: false }));
            SetCapture(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if let Some(mut p) = press().filter(|_| GetCapture() == hwnd) {
                let locked = STATE.lock().unwrap().as_ref().map_or(false, |s| s.locked);
                let mut at = POINT::default();
                let _ = GetCursorPos(&mut at);
                let (dx, dy) = (at.x - p.start.x, at.y - p.start.y);
                if !locked && (p.dragging || dx.abs() > DRAG_PX || dy.abs() > DRAG_PX) {
                    p.dragging = true;
                    let _ = SetWindowPos(hwnd, None, p.window.x + dx, p.window.y + dy, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    set_press(Some(p));
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let p = press();
            set_press(None);
            let _ = ReleaseCapture();
            match p {
                Some(p) if p.dragging => {
                    // the icon's centre where it was let go, as the corner of the icon size used off the row
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    let l = LAST_LAYOUT.lock().unwrap().unwrap_or(Layout { x: 0, y: 0, w: rect.right - rect.left, h: 0, icon: BASE_SIZE, pill: None, font: 0 });
                    let base = (BASE_SIZE as f64 * GetDpiForWindow(hwnd).max(96) as f64 / 96.0).round() as i32;
                    let (cx, cy) = (rect.left + l.w / 2, rect.top + l.icon / 2);
                    with_state(|s| (s.x, s.y) = (Some(cx - base / 2), Some(cy - base / 2)));
                }
                Some(_) => set_capture(!CAPTURE_ON.load(Ordering::SeqCst)),
                None => {}
            }
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            set_press(None); // the press is over even without a button-up (it kept the button showing everywhere)
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            menu(hwnd);
            LRESULT(0)
        }
        WM_HOTKEY if wparam.0 as i32 == HOTKEY_ID => {
            set_capture(!CAPTURE_ON.load(Ordering::SeqCst));
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn run(game_title: String) {
    let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    {
        let mut guard = STATE.lock().unwrap();
        guard.get_or_insert_with(load);
    }
    let Ok(module) = GetModuleHandleW(None) else { return };
    let class = w!("aLogsCaptureButton");
    let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: module.into(), lpszClassName: class, ..Default::default() };
    RegisterClassW(&wc);
    let Ok(hwnd) = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        class,
        w!("aLogs screen capture"),
        WS_POPUP,
        0,
        0,
        BASE_SIZE,
        BASE_SIZE,
        None,
        None,
        Some(module.into()),
        None,
    ) else {
        return;
    };
    WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);
    let _ = SetTimer(Some(hwnd), TIMER_FOLLOW, 250, None);
    let mut shown = false;
    let mut drawn: Option<(Layout, bool, Option<String>, Option<PathBuf>)> = None;
    let mut hotkey: Option<u32> = None; // registered while the game is in front
    let mut reader = crate::capture::AppReader::new();
    let mut row_seen = 0u64; // when the game's icon row was last seen (hidden after half a second without it)
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        if (msg.message == WM_TIMER && msg.hwnd == hwnd) || (msg.message == WM_REFRESH && msg.hwnd == hwnd) {
            let game = HWND(crate::capture::find_game(&game_title) as _);
            let front = GetForegroundWindow();
            let in_front = !game.is_invalid() && (front == game || front == hwnd || press().is_some());
            let s = STATE.lock().unwrap().clone().unwrap_or_else(load);
            // the hotkey works only while the game is in front, so the key keeps its normal use elsewhere
            let want_key = (in_front && s.hotkey_vk != 0).then_some(s.hotkey_vk);
            if want_key != hotkey {
                let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
                hotkey = want_key.filter(|&vk| RegisterHotKey(Some(hwnd), HOTKEY_ID, MOD_NOREPEAT, vk).is_ok());
            }
            // in the icon row: only while the game shows it (capture off: we can't see it, so while in front)
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
            let row = if s.x.is_some() || !CAPTURE_ON.load(Ordering::SeqCst) {
                true
            } else if !(s.show && in_front) {
                false // no need to look (and no capture asked for) while it can't show anyway
            } else {
                if let Some(r) = reader.as_mut() {
                    if row_visible(r, game) == Some(true) {
                        row_seen = now;
                    }
                }
                now.saturating_sub(row_seen) < 500 || press().is_some()
            };
            let wanted = s.show && in_front && row;
            if wanted {
                if let Some(l) = layout(hwnd, game) {
                    let look = (l, CAPTURE_ON.load(Ordering::SeqCst), label(), s.icon.clone());
                    if drawn.as_ref() != Some(&look) && press().map_or(true, |p| !p.dragging) {
                        paint(hwnd, &l);
                        drawn = Some(look);
                    }
                }
            }
            if wanted != shown {
                shown = wanted;
                let _ = ShowWindow(hwnd, if wanted { SW_SHOWNOACTIVATE } else { SW_HIDE });
                if wanted {
                    let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
            }
            continue;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_fits_and_dims() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let px = decode(APP_ICON, 44).expect("the app icon decodes");
        assert_eq!(px.len(), 44 * 44 * 4);
        assert!(px.chunks_exact(4).any(|p| p[3] == 255), "solid pixels");
        assert!(px.chunks_exact(4).any(|p| p[3] == 0), "transparent corners");
        // a wide picture keeps its shape: centred, with empty rows above and below
        let wide = {
            let factory: IWICImagingFactory = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).unwrap() };
            let _ = factory;
            decode(APP_ICON, 20).unwrap()
        };
        assert_eq!(wide.len(), 20 * 20 * 4);
        CAPTURE_ON.store(false, Ordering::SeqCst);
        let dim = picture(44);
        CAPTURE_ON.store(true, Ordering::SeqCst);
        let bright = picture(44);
        let sum = |v: &[u8]| v.chunks_exact(4).map(|p| p[3] as u64).sum::<u64>();
        assert!(sum(&dim) * 2 < sum(&bright), "off is much fainter");
        assert!(dim.chunks_exact(4).all(|p| p[0] == p[1] && p[1] == p[2]), "off is grey");
    }
}

#[cfg(test)]
mod look {
    use super::*;

    /// Draws the button as it sits in the icon row at 4K (on and off) into `<temp>/alogs_button_<on|off>.bgra`
    /// (width, height in the first 8 bytes), to compare with the game's own icons:
    /// `cargo test --bin alogs-app overlay_button::look -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn render_row_button() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            *STATE.lock().unwrap() = Some(State { show: true, x: None, y: None, locked: false, icon: None, hotkey_vk: VK_INSERT, hotkey_label: "Ins".into() });
            let k = 1.0;
            let icon = (ROW_ICON * k).round() as i32;
            let font = (ROW_FONT * k).round() as i32;
            let pill_h = (ROW_PILL_H * k).round() as i32;
            let pw = pill_width("Ins", font, k);
            let w = icon.max(pw);
            let gap = ((ROW_ICON_UP - ROW_PILL_UP) * k - (ROW_ICON + ROW_PILL_H) * k / 2.0).round() as i32;
            let l = Layout { x: 0, y: 0, w, h: icon + gap + pill_h, icon, pill: Some(((w - pw) / 2, icon + gap, pw, pill_h)), font };
            for on in [true, false] {
                CAPTURE_ON.store(on, Ordering::SeqCst);
                let pixels = picture(l.icon as u32);
                let dc = CreateCompatibleDC(None);
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: l.w, biHeight: -l.h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
                    ..Default::default()
                };
                let mut bits = std::ptr::null_mut();
                let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
                let out = bits as *mut u8;
                std::ptr::write_bytes(out, 0, (l.w * l.h * 4) as usize);
                let ox = (l.w - l.icon) / 2;
                for row in 0..l.icon {
                    std::ptr::copy_nonoverlapping(pixels.as_ptr().add((row * l.icon * 4) as usize), out.add(((row * l.w + ox) * 4) as usize), (l.icon * 4) as usize);
                }
                let old = SelectObject(dc, bitmap.into());
                draw_pill(dc, out, l.w, &l, "Ins");
                let mut file = (l.w as u32).to_le_bytes().to_vec();
                file.extend((l.h as u32).to_le_bytes());
                file.extend(std::slice::from_raw_parts(out, (l.w * l.h * 4) as usize));
                std::fs::write(std::env::temp_dir().join(format!("alogs_button_{}.bgra", if on { "on" } else { "off" })), file).unwrap();
                SelectObject(dc, old);
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(dc);
            }
        }
    }
}
