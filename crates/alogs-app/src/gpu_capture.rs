//! GPU capture for the shared game frame (capture.rs). Both ways keep the picture on the graphics card
//! and only copy the parts the readers asked for into memory, instead of a whole 4K frame each time:
//!
//! - **Window capture** (Windows Graphics Capture, as OBS uses): only the game window, so our overlays
//!   are never in it. Used only where Windows lets the yellow capture border be turned off (Windows 11).
//! - **Desktop Duplication**: the screen the game is on, where Windows hands over only what changed.
//!   No border on any Windows 10/11; like the old screen copy, overlays on top are in the picture.
//!
//! Each keeps the newest image in its own texture (`Gpu::cache`), so a reader asking for a new area
//! gets it from the last image even when nothing on screen has changed since.

use windows::core::{Interface, Result, HSTRING};
use windows::Foundation::Metadata::ApiInformation;
use windows::Foundation::TimeSpan;
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind, GraphicsCaptureItem,
    GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Security::Authorization::AppCapabilityAccess::AppCapabilityAccessStatus;
use windows::Win32::Foundation::{E_FAIL, HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Query, ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_RESOURCE_MISC_GENERATE_MIPS,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIDevice, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
    DXGI_ERROR_WAIT_TIMEOUT, DXGI_ERROR_WAS_STILL_DRAWING, DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Gdi::{MonitorFromWindow, HMONITOR, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

use std::time::{Duration, Instant};

/// How long a copy the card hasn't finished stays queued for its reader (Gpu::ready) before it's started again.
const KEEP_QUEUED: std::time::Duration = std::time::Duration::from_secs(2);

/// At most this many combined copies on their way off the card at once (may_start).
const IN_FLIGHT: usize = 2;

/// Staging textures kept between frames (one per area size the readers ask for), the least recently used going first.
const STAGING_KEPT: usize = 24;
/// ...and at most this much memory for each kind (a 4K frame is 33 MB: eight of those held 265 MB).
const STAGING_BYTES: usize = 128 << 20;

/// Dev switch capture-hold-frame: the screen's image is held only while new ones keep coming this close together...
const FLOWING: std::time::Duration = std::time::Duration::from_millis(100);
/// ...and a reader left without its new area this long on a still screen has Desktop Duplication started again.
const STARVED: std::time::Duration = std::time::Duration::from_millis(300);

/// Room for a `width` x `height` texture in `kept` (sizes from `size`): drops the least recently used (the front).
fn make_room<T>(kept: &mut Vec<T>, size: impl Fn(&T) -> (u32, u32), width: u32, height: u32) {
    let bytes = |(w, h): (u32, u32)| w as usize * h as usize * 4;
    while !kept.is_empty()
        && (kept.len() >= STAGING_KEPT || kept.iter().map(|t| bytes(size(t))).sum::<usize>() + bytes((width, height)) > STAGING_BYTES)
    {
        kept.remove(0);
    }
}

/// The graphics adapter showing `hwnd`'s monitor, and its name. On a PC with more than one (integrated graphics beside
/// a graphics card, or a virtual display driver) the default adapter can be another one: window capture's pictures then
/// cross from one to the other, and under load the copies barely finished (16 Nov: a new 1440p screen, 25,000 copies a
/// minute not ready, the meter without a picture for up to 22 s).
fn window_adapter(hwnd: HWND) -> Option<(IDXGIAdapter, String)> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut a = 0;
        while let Ok(adapter) = factory.EnumAdapters1(a) {
            a += 1;
            let mut o = 0;
            while let Ok(output) = adapter.EnumOutputs(o) {
                o += 1;
                if output.GetDesc().ok().is_some_and(|d| d.Monitor == monitor) {
                    let name = adapter.GetDesc1().map(|d| String::from_utf16_lossy(&d.Description).trim_end_matches('\0').to_string()).unwrap_or_default();
                    return Some((adapter.cast::<IDXGIAdapter>().ok()?, name));
                }
            }
        }
        None
    }
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn D3DKMTSetProcessSchedulingPriorityClass(process: *mut core::ffi::c_void, class: i32) -> i32;
}

/// The app's GPU scheduling class above normal (once), as screen recorders do: with a game keeping the card full, the
/// capture's copies otherwise wait behind every frame it draws. HIGH where Windows allows it, else ABOVE_NORMAL.
pub fn raise_gpu_class() {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        let me = unsafe { windows::Win32::System::Threading::GetCurrentProcess() };
        for (class, name) in [(4, "high"), (3, "above normal")] {
            let status = unsafe { D3DKMTSetProcessSchedulingPriorityClass(me.0, class) };
            if status == 0 {
                crate::app_log(&format!("capture: GPU scheduling class {name}"));
                return;
            }
        }
        crate::app_log("capture: GPU scheduling class not raised (Windows refused)");
    });
}

fn device(adapter: Option<&IDXGIAdapter>) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    raise_gpu_class();
    let (mut device, mut context) = (None, None);
    let driver = if adapter.is_some() { D3D_DRIVER_TYPE_UNKNOWN } else { D3D_DRIVER_TYPE_HARDWARE };
    unsafe {
        D3D11CreateDevice(
            adapter,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?
    };
    match (device, context) {
        (Some(d), Some(c)) => Ok((d, c)),
        _ => Err(E_FAIL.into()),
    }
}

fn texture_desc(width: u32, height: u32, staging: bool) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: if staging { D3D11_USAGE_STAGING } else { D3D11_USAGE_DEFAULT },
        BindFlags: 0,
        CPUAccessFlags: if staging { D3D11_CPU_ACCESS_READ.0 as u32 } else { 0 },
        MiscFlags: 0,
    }
}

/// General Settings › Performance › Screen capture on the graphics card: Lowest (capture.gpu_priority "lowest", which the
/// page writes only while Performance toggles is on). Set at start and on every settings save (apply_settings);
/// it was the Dev switch capture-low-priority.
pub static LOW_PRIORITY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn apply_settings(settings: &serde_json::Value) {
    let low = settings.pointer("/capture/gpu_priority").and_then(serde_json::Value::as_str) == Some("lowest");
    LOW_PRIORITY.store(low, std::sync::atomic::Ordering::Relaxed);
}

/// The device, the newest image (on the card) and the small textures areas are read through.
pub struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    cache: Option<(ID3D11Texture2D, u32, u32)>,
    staging: Vec<(u32, u32, ID3D11Texture2D)>,
    /// Textures that shrink an area on the card (its mip levels: 1/2, 1/4), by size and number of levels.
    shrinkers: Vec<(u32, u32, u32, ID3D11Texture2D, ID3D11ShaderResourceView)>,
    /// Counts new images, so the caller can tell an unchanged one from a new one.
    pub generation: u64,
    /// Areas whose copy off the card is on its way (queue), shrunk ones: a copy the card hasn't finished (ready) stays
    /// queued into later rounds, so it isn't started again over and over; one not read within KEEP_QUEUED is dropped.
    queued: Vec<Queued>,
    /// The image the last read() came from (generation) and when Windows captured it: with the card busy, an older
    /// copy than the newest image can be the one finished (Copies::serving).
    last_read: (u64, Option<Instant>),
    /// When Windows captured the newest image (window capture's own frame time), if known.
    pub captured_at: Option<std::time::Instant>,
    /// Dev switch capture-hold-frame: `cache` is the screen's own texture, held from Desktop Duplication, not a copy
    /// of it (hold); never written to.
    borrowed: bool,
    /// ...which is ours until the round's areas are copied out of it (end_round), then given back to this.
    held: Option<IDXGIOutputDuplication>,
    /// ...and once it's given back there's nothing to copy a new area from until the next image (queue, ready).
    /// Areas already on their way are unaffected.
    lost: bool,
    /// Copies were queued since the card was last sent its commands. Direct3D 11 keeps them in its own buffer until
    /// something flushes it, and a look that doesn't wait (ready) doesn't: they could sit there unsent, round after round.
    unflushed: bool,
    /// The full-size areas asked for this round (queue), copied off the card together as one box (union).
    round_want: Vec<Rect>,
    /// Combined copies on their way or done, and what decides when one may start, which serves a reader and when a
    /// held frame can go back (Copies).
    copies: Copies<ID3D11Texture2D>,
    /// Staging textures for combined copies, kept for reuse by size.
    union_staging: Vec<(u32, u32, ID3D11Texture2D)>,
    /// Since when a reader has been asking for an area that can't be copied (`lost`).
    starved: Option<std::time::Instant>,
    /// General Settings › Performance › Screen capture on the graphics card = Lowest: this device's work on the card
    /// goes behind the game's (LOW_PRIORITY).
    low: bool,
    /// The priority was set once on this device (low_priority).
    prio_set: bool,
    /// Dev switch capture-frame-hold (window capture; 14 Nov, the owner's call to try it): the frame Windows handed
    /// over, kept open as the image (`cache` is its own texture, `borrowed`), so the whole frame isn't first copied on
    /// the card. None with the switch off, and on Desktop Duplication.
    frame: Option<Direct3D11CaptureFrame>,
    /// ...and the frames it replaced that a copy may still be reading, each closed (back to Windows, which draws the
    /// next frame into it) only once the card has done every copy queued from it: an event query issued after the
    /// last of them reports done. Closed any sooner, an area still being copied could come out half old frame, half
    /// new. A replaced frame nothing was copied from goes back at once (retire_frame).
    retiring: Vec<(Direct3D11CaptureFrame, ID3D11Query)>,
    /// For the frame-hold line in app.log once a minute: replaced frames that went straight back, and the most that
    /// waited in `retiring` at once.
    frames_back: u32,
    most_waiting: usize,
}

/// An area of the image: x, y, w, h (pixels).
type Rect = (u32, u32, u32, u32);

/// Whether `outer` holds all of `inner`.
fn holds(outer: Rect, inner: Rect) -> bool {
    let ((ux, uy, uw, uh), (x, y, w, h)) = (outer, inner);
    x >= ux && y >= uy && x + w <= ux + uw && y + h <= uy + uh
}

/// One combined copy of a round's areas (Copies::unions); `S` is its staging texture (a number in the tests).
struct Union<S> {
    rect: Rect,
    staging: S,
    /// The image it was copied from, and when Windows captured that image (window capture says; None otherwise).
    generation: u64,
    captured: Option<Instant>,
    /// When it was queued.
    at: Instant,
    /// Its memory while mapped this round for a reader (data, row pitch).
    mapped: Option<(*const u8, usize)>,
    /// The card has finished it (seen once, whoever looked: Copies::poll, Copies::serving).
    done: bool,
}

/// What the box of a round's areas needs (Copies::plan).
#[derive(Debug, PartialEq)]
enum Plan {
    /// Nothing asked for this round, or none of it on the image.
    Nothing,
    /// A copy of this image already holds it all, finished or on its way.
    Held,
    /// It waits: a copy on its way holds an area a reader is asking for (it serves them once it's back), or another
    /// may not start beside the ones on their way (may_start). It starts once one is back.
    Wait,
    /// A copy of this box starts now.
    Start(Rect),
}

/// The copies off the card without the card: when a combined copy may start, which copy serves a reader, and when a
/// held frame can go back to Windows. Gpu does the card's side around it; the tests drive it with a pretend slow card.
struct Copies<S> {
    /// Combined copies on their way or done, newest last.
    unions: Vec<Union<S>>,
    /// frame-hold (window capture): the held frame's image (generation) and how many copies started from it the
    /// card hasn't been seen to finish. None while no frame is held.
    frame: Option<(u64, u32)>,
    /// For app.log (COPY_STATS, merged each round).
    stats: CopyStats,
}

impl<S> Copies<S> {
    fn new() -> Copies<S> {
        Copies { unions: Vec::new(), frame: None, stats: CopyStats::new() }
    }

    /// Looks at every combined copy still on its way, without waiting (`card`: whether the card has finished this
    /// one), whoever asked for it. Seen only when a reader asked for an area inside its box, a copy nobody wanted any
    /// more (the meter gives up a look after 120 ms and asks for another box) was never seen finished, and nothing new
    /// started behind it until KEEP_QUEUED ran out: gaps of 2.1 s at 120 fps (17 Nov).
    fn poll(&mut self, now: Instant, card: &mut dyn FnMut(&S) -> bool) {
        for i in 0..self.unions.len() {
            if !self.unions[i].done && card(&self.unions[i].staging) {
                self.finished(i, now);
            }
        }
    }

    /// The card has finished copy `i` (seen at `now`).
    fn finished(&mut self, i: usize, now: Instant) {
        let u = &mut self.unions[i];
        if u.done {
            return;
        }
        u.done = true;
        let (generation, took) = (u.generation, now.saturating_duration_since(u.at));
        self.stats.came_back(took);
        self.copy_done(generation);
    }

    /// What `want`, the full-size areas asked for this round, need: the box around them on the image (`size`), and
    /// whether a copy of it starts now. The copies on their way are looked at first (poll).
    fn plan(&mut self, want: &[Rect], generation: u64, size: (u32, u32), now: Instant, card: &mut dyn FnMut(&S) -> bool) -> Plan {
        if want.is_empty() {
            return Plan::Nothing;
        }
        let x0 = want.iter().map(|a| a.0).min().unwrap();
        let y0 = want.iter().map(|a| a.1).min().unwrap();
        let x1 = want.iter().map(|a| a.0 + a.2).max().unwrap().min(size.0);
        let y1 = want.iter().map(|a| a.1 + a.3).max().unwrap().min(size.1);
        if x1 <= x0 || y1 <= y0 {
            return Plan::Nothing;
        }
        let rect = (x0, y0, x1 - x0, y1 - y0);
        if self.unions.iter().any(|u| u.generation == generation && holds(u.rect, rect)) {
            return Plan::Held;
        }
        self.poll(now, card);
        if may_start(&self.unions, want, generation, self.frame.is_some()) { Plan::Start(rect) } else { Plan::Wait }
    }

    /// A copy (Plan::Start) is on its way.
    fn started(&mut self, union: Union<S>) {
        if self.unions.iter().any(|u| !u.done) {
            self.stats.beside += 1;
        }
        self.copy_from(union.generation);
        self.unions.push(union);
    }

    /// The copy that serves a reader's `area` now: the newest finished one holding it, from whichever image (those
    /// holding it still on their way looked at first, without waiting). None: the area waits for a later round.
    fn serving(&mut self, area: Rect, now: Instant, card: &mut dyn FnMut(&S) -> bool) -> Option<usize> {
        for i in (0..self.unions.len()).rev() {
            if !self.unions[i].done && holds(self.unions[i].rect, area) && card(&self.unions[i].staging) {
                self.finished(i, now);
            }
        }
        (0..self.unions.len()).rev().find(|&i| self.unions[i].done && holds(self.unions[i].rect, area))
    }

    /// The round is over: what was mapped is let go (`unmap`); copies older than the newest one read are gone, and one
    /// still on its way stays for the next rounds, up to KEEP_QUEUED (counted as dropped unfinished if the card, asked
    /// once more, hasn't finished it: `card`).
    fn end_round(&mut self, now: Instant, unmap: &mut dyn FnMut(&S), card: &mut dyn FnMut(&S) -> bool) {
        let newest_read = self.unions.iter().rposition(|u| u.mapped.is_some());
        for u in self.unions.iter_mut() {
            if u.mapped.take().is_some() {
                unmap(&u.staging);
            }
        }
        if let Some(n) = newest_read {
            self.unions.drain(..n);
        }
        let dropped = &mut self.stats.dropped;
        self.unions.retain(|u| {
            let keep = now.saturating_duration_since(u.at) < KEEP_QUEUED;
            *dropped += (!keep && !u.done && !card(&u.staging)) as u32;
            keep
        });
    }

    /// frame-hold: the frame of image `generation` is held now (None: no frame is), and nothing was copied from it yet.
    fn frame_held(&mut self, generation: Option<u64>) {
        self.frame = generation.map(|g| (g, 0));
    }

    /// A copy (combined or shrunk) was started from image `generation`.
    fn copy_from(&mut self, generation: u64) {
        if let Some((g, n)) = self.frame.as_mut() {
            *n += (*g == generation) as u32;
        }
    }

    /// The card was seen to finish a copy from image `generation`.
    fn copy_done(&mut self, generation: u64) {
        if let Some((g, n)) = self.frame.as_mut() {
            if *g == generation {
                *n = n.saturating_sub(1);
            }
        }
    }

    /// How many copies started from the held frame may still be reading it. 0: when it's replaced it goes straight back
    /// to Windows, no event query to wait for behind the card's other work (retire_frame). A copy dropped before it
    /// was seen finished still counts: the card may not be done with it.
    fn frame_reads(&self) -> u32 {
        self.frame.map_or(0, |f| f.1)
    }
}

/// Whether a copy of a new box may start beside the combined copies on their way (`unions`), for the round's areas
/// `want` from image `generation`; `holding_frame`: frame-hold, the copies read straight from Windows' frames.
fn may_start<S>(unions: &[Union<S>], want: &[Rect], generation: u64, holding_frame: bool) -> bool {
    let on_way: Vec<&Union<S>> = unions.iter().filter(|u| !u.done).collect();
    if on_way.is_empty() {
        return true;
    }
    if on_way.len() >= IN_FLIGHT {
        return false;
    }
    // One holding an area a reader is asking for serves it once it's back, so the next box waits for it: with the
    // card full (120 Hz, 16 Nov) a copy a frame queued up behind the game, each held its frame open, and Windows ran
    // out of buffers to send new ones (2 a second).
    if on_way.iter().any(|u| want.iter().any(|&a| holds(u.rect, a))) {
        return false;
    }
    // One nobody is asking for: the new box starts beside it, unless that keeps a second frame from Windows (frame-hold,
    // the one on its way reads a frame already replaced: with that one waiting too, the pool's 3 buffers are all
    // taken). The card does one device's copies in order, so the new one wouldn't be back sooner there; it starts the
    // round the old one is seen finished (poll).
    !holding_frame || on_way.iter().all(|u| u.generation == generation)
}

/// A shrunk area's copy off the card (Gpu::queued).
struct Queued {
    /// (x, y, w, h, shrink) as asked for.
    key: (u32, u32, u32, u32, u32),
    staging: ID3D11Texture2D,
    /// The picture's size (shrunk).
    width: u32,
    height: u32,
    /// When it was queued, the image it was copied from and when Windows captured that image.
    at: Instant,
    generation: u64,
    captured: Option<Instant>,
    /// The card was seen to finish it.
    done: bool,
}

/// What the copies off the card did since app.log last said (the capture line once a minute, capture.rs).
pub struct CopyStats {
    /// Copies seen finished, how long each took from queued to seen finished (ms; up to TOOK_KEPT of them a minute)
    /// and the longest.
    back: u32,
    took_ms: Vec<u32>,
    longest_ms: u32,
    /// Dropped unfinished after KEEP_QUEUED.
    dropped: u32,
    /// Combined copies started beside one on its way that nobody was asking for (may_start).
    beside: u32,
}

const TOOK_KEPT: usize = 50_000;

impl CopyStats {
    const fn new() -> CopyStats {
        CopyStats { back: 0, took_ms: Vec::new(), longest_ms: 0, dropped: 0, beside: 0 }
    }

    fn came_back(&mut self, took: Duration) {
        let ms = took.as_millis().min(u32::MAX as u128) as u32;
        self.back += 1;
        if self.took_ms.len() < TOOK_KEPT {
            self.took_ms.push(ms);
        }
        self.longest_ms = self.longest_ms.max(ms);
    }

    fn is_empty(&self) -> bool {
        self.back == 0 && self.dropped == 0 && self.beside == 0
    }

    fn add(&mut self, other: CopyStats) {
        self.back += other.back;
        let room = TOOK_KEPT.saturating_sub(self.took_ms.len());
        self.took_ms.extend(other.took_ms.into_iter().take(room));
        self.longest_ms = self.longest_ms.max(other.longest_ms);
        self.dropped += other.dropped;
        self.beside += other.beside;
    }

    /// The numbers for app.log, and they start over.
    fn line(&mut self) -> String {
        let mut took = std::mem::take(&mut self.took_ms);
        took.sort_unstable();
        let typical = took.get(took.len() / 2).copied().unwrap_or(0);
        let line = format!(
            "copies off the card: {} back (typical {typical} ms, longest {} ms), {} dropped unfinished after {} s, {} started beside one nobody wanted",
            self.back,
            self.longest_ms,
            self.dropped,
            KEEP_QUEUED.as_secs(),
            self.beside
        );
        *self = CopyStats::new();
        line
    }
}

/// Every Gpu's copies (window capture, Desktop Duplication, the tabbed-out window capture), merged at each round's end.
static COPY_STATS: std::sync::Mutex<CopyStats> = std::sync::Mutex::new(CopyStats::new());

/// The copies' numbers since the last time (for the capture line in app.log once a minute), and they start over.
pub fn copy_stats_line() -> String {
    COPY_STATS.lock().unwrap_or_else(|e| e.into_inner()).line()
}

/// Whether the card has finished the copy into `staging`, without waiting (mapped and let go again at once).
fn finished_on_card(context: &ID3D11DeviceContext, staging: &ID3D11Texture2D) -> bool {
    let mut m = D3D11_MAPPED_SUBRESOURCE::default();
    let got = unsafe { context.Map(staging, 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32, Some(&mut m)) };
    if got.is_ok() {
        unsafe { context.Unmap(staging, 0) };
    }
    got.is_ok()
}

impl Gpu {
    fn new(device: ID3D11Device, context: ID3D11DeviceContext) -> Gpu {
        Gpu {
            unflushed: false,
            round_want: Vec::new(),
            copies: Copies::new(),
            last_read: (0, None),
            frames_back: 0,
            most_waiting: 0,
            union_staging: Vec::new(),
            prio_set: false,
            device,
            context,
            cache: None,
            staging: Vec::new(),
            shrinkers: Vec::new(),
            generation: 0,
            queued: Vec::new(),
            captured_at: None,
            borrowed: false,
            held: None,
            lost: false,
            starved: None,
            low: false,
            frame: None,
            retiring: Vec::new(),
        }
    }

    /// Dev switch capture-frame-hold: window capture's `frame` (its texture `source`, the window's `width` x `height`
    /// of it) is the newest image as it is, with no copy of the whole frame on the card; areas are copied straight out
    /// of it. The frame it replaces is retired (closed at once, or once its copies are done).
    fn hold_frame(&mut self, frame: Direct3D11CaptureFrame, source: &ID3D11Texture2D, width: u32, height: u32) -> Result<()> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { source.GetDesc(&mut desc) };
        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(E_FAIL.into()); // (as store: not readable as is)
        }
        // the old frame retired first (nothing is asked of the card between that and the image moving on, so its
        // query comes after every copy out of it); if it can't be, the old frame stays the image, open
        self.retire_frame()?;
        self.cache = Some((source.clone(), width.min(desc.Width), height.min(desc.Height)));
        (self.borrowed, self.lost, self.starved) = (true, false, None);
        self.frame = Some(frame);
        self.generation += 1;
        self.copies.frame_held(Some(self.generation));
        Ok(())
    }

    /// The held frame (if any) is no longer the image. Nothing copied from it still on the card (Copies::frame_reads):
    /// it goes straight back to Windows. Otherwise an event query after everything asked of the card so far (so after
    /// every copy out of it), and it waits in `retiring` until that's done. Every replaced frame waited for one before:
    /// each query waited behind whatever the card hadn't done for us yet, and with two waiting the pool (3 buffers)
    /// had none left for the next frame (17 Nov at 120 fps: 6,585 updates in a minute found one still waiting, and
    /// Windows handed over 2 frames a second).
    fn retire_frame(&mut self) -> Result<()> {
        let Some(frame) = self.frame.take() else { return Ok(()) };
        if self.copies.frame_reads() == 0 {
            let _ = frame.Close();
            self.copies.frame_held(None);
            self.frames_back += 1;
            return Ok(());
        }
        let mut query = None;
        let made = unsafe { self.device.CreateQuery(&D3D11_QUERY_DESC { Query: D3D11_QUERY_EVENT, MiscFlags: 0 }, Some(&mut query)) };
        match (made, query) {
            (Ok(()), Some(query)) => {
                unsafe {
                    self.context.End(&query);
                    self.context.Flush();
                }
                self.retiring.push((frame, query));
                self.copies.frame_held(None);
                self.most_waiting = self.most_waiting.max(self.retiring.len());
                Ok(())
            }
            (made, _) => {
                // no way to know when its copies are done: kept (never closed early) until the capture starts again
                self.frame = Some(frame);
                Err(made.err().unwrap_or_else(|| E_FAIL.into()))
            }
        }
    }

    /// Closes the retired frames whose copies the card has done, without waiting. Returns how many are still busy.
    fn retire_done(&mut self) -> usize {
        let context = &self.context;
        self.retiring.retain(|(frame, query)| {
            let mut done = 0i32; // the event query's answer, a BOOL: written only once it's done
            let asked = unsafe { context.GetData(query, Some(&mut done as *mut i32 as *mut _), std::mem::size_of::<i32>() as u32, 0) };
            // an error is a lost device: nothing it queued will be done, and the capture starts again
            let finished = asked.is_err() || done != 0;
            if finished {
                let _ = frame.Close();
            }
            !finished
        });
        self.retiring.len()
    }

    /// Every frame still open goes back to Windows (the window capture closing).
    fn close_frames(&mut self) {
        if let Some(frame) = self.frame.take() {
            let _ = frame.Close();
        }
        self.copies.frame_held(None);
        for (frame, _) in self.retiring.drain(..) {
            let _ = frame.Close();
        }
    }

    /// Lowest (LOW_PRIORITY) or not: the copies this device asks of the graphics card are done after
    /// the game's own work (the lowest of the priorities a program may give itself), or as before.
    fn low_priority(&mut self, low: bool) {
        // (applied the first time too: the default is no longer the card's normal priority)
        if low == self.low && self.prio_set {
            return;
        }
        (self.low, self.prio_set) = (low, true);
        // (Normal otherwise. Tried the highest, +7, on 16 Nov with the game keeping the card at ~90% at 1440p: fewer
        // new images came and the readers waited longer, so not that.)
        let want = if low { -7 } else { 0 };
        let set = self.device.cast::<IDXGIDevice>().and_then(|d| unsafe { d.SetGPUThreadPriority(want) });
        match set {
            Ok(()) => crate::app_log(&format!("capture: graphics card priority {}", if low { "lowest (Performance)" } else { "normal" })),
            Err(e) => crate::app_log(&format!("capture: graphics card priority {want} not set ({e})")),
        }
    }

    /// Dev switch capture-hold-frame: the screen's own texture, held from Desktop Duplication, is the newest image as
    /// it is, with no copy of the whole screen on the card. Areas are copied straight out of it until it's given
    /// back to `from`, when the round ends. `new`: a new image, not the one given back taken again.
    fn hold(&mut self, source: &ID3D11Texture2D, new: bool, from: &IDXGIOutputDuplication) -> Result<()> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { source.GetDesc(&mut desc) };
        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(E_FAIL.into()); // an HDR desktop (16-bit colour): not readable as is
        }
        self.cache = Some((source.clone(), desc.Width, desc.Height));
        (self.borrowed, self.lost, self.starved) = (true, false, None);
        self.held = Some(from.clone());
        if new {
            self.generation += 1;
        }
        Ok(())
    }

    /// The held texture goes back to Windows, the copies started out of it sent to the card first. Right after the
    /// round, not just before the next image is asked for: asked for straight after one went back, Windows had no
    /// new image half the time, which halved the pictures (measured: 145 new images in 300 rounds instead of 288).
    fn give_back(&mut self) {
        if let Some(duplication) = self.held.take() {
            unsafe {
                self.context.Flush();
                let _ = duplication.ReleaseFrame();
            }
            self.lost = true;
        }
    }

    /// Keeps the top-left `width` x `height` of `source` as the newest image (a copy on the card).
    fn store(&mut self, source: &ID3D11Texture2D, width: u32, height: u32) -> Result<()> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { source.GetDesc(&mut desc) };
        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(E_FAIL.into()); // an HDR desktop (16-bit colour): not readable as is
        }
        let (width, height) = (width.min(desc.Width), height.min(desc.Height));
        if self.borrowed {
            (self.cache, self.borrowed) = (None, false); // the screen's own texture (hold): the copy gets its own again
        }
        (self.lost, self.starved) = (false, None);
        if self.cache.as_ref().map_or(true, |c| c.1 != width || c.2 != height) {
            let mut texture = None;
            unsafe { self.device.CreateTexture2D(&texture_desc(width, height, false), None, Some(&mut texture))? };
            self.cache = Some((texture.ok_or(E_FAIL)?, width, height));
        }
        let cache = &self.cache.as_ref().unwrap().0;
        let area = D3D11_BOX { left: 0, top: 0, front: 0, right: width, bottom: height, back: 1 };
        unsafe { self.context.CopySubresourceRegion(cache, 0, 0, 0, 0, source, 0, Some(&area)) };
        self.generation += 1;
        Ok(())
    }

    /// One copy off the card of the box around every full-size area asked for this round, unless a copy of this
    /// image already holds it all. Readers' areas are cut out of it on the CPU: one job for the card a round instead of
    /// one per reader (16 Nov: at 1440p with the game keeping the card at ~90%, separate copies waited up to a second).
    fn union(&mut self) -> Result<()> {
        if self.round_want.is_empty() {
            return Ok(());
        }
        let want = std::mem::take(&mut self.round_want);
        if self.lost {
            self.starved.get_or_insert_with(std::time::Instant::now);
            return Ok(());
        }
        let Some((cache, cw, ch)) = self.cache.as_ref() else { return Err(E_FAIL.into()) };
        let (cache, size) = (cache.clone(), (*cw, *ch));
        // Readers keep the last finished picture while a copy is on its way; may_start says when the next may start
        // beside it, and one that hasn't come back in KEEP_QUEUED is dropped (end_round).
        let context = &self.context;
        let rect = match self.copies.plan(&want, self.generation, size, Instant::now(), &mut |s| finished_on_card(context, s)) {
            Plan::Start(rect) => rect,
            Plan::Nothing | Plan::Held | Plan::Wait => return Ok(()),
        };
        let (x0, y0, w, h) = rect;
        let unions = &self.copies.unions;
        let free = self.union_staging.iter().position(|s| s.0 == w && s.1 == h && !unions.iter().any(|u| u.staging == s.2));
        let staging = match free {
            Some(i) => self.union_staging[i].2.clone(),
            None => {
                let mut texture = None;
                unsafe { self.device.CreateTexture2D(&texture_desc(w, h, true), None, Some(&mut texture))? };
                let texture = texture.ok_or(E_FAIL)?;
                let busy: Vec<ID3D11Texture2D> = unions.iter().map(|u| u.staging.clone()).collect();
                self.union_staging.retain(|s| busy.contains(&s.2) || (s.0, s.1) == (w, h));
                if self.union_staging.len() >= 8 {
                    self.union_staging.retain(|s| busy.contains(&s.2));
                }
                self.union_staging.push((w, h, texture.clone()));
                texture
            }
        };
        let area = D3D11_BOX { left: x0, top: y0, front: 0, right: x0 + w, bottom: y0 + h, back: 1 };
        unsafe { self.context.CopySubresourceRegion(&staging, 0, 0, 0, 0, &cache, 0, Some(&area)) };
        self.unflushed = true;
        let (generation, captured) = (self.generation, self.captured_at);
        self.copies.started(Union { rect, staging, generation, captured, at: Instant::now(), mapped: None, done: false });
        Ok(())
    }

    /// The newest combined copy holding this area that the card has finished (mapped for the rest of the round), or
    /// None: then the area waits for the next round, reading the copy a round or a frame later instead of waiting.
    /// It can be a copy of an older image than the newest: its generation says which (Copies::serving).
    fn union_ready(&mut self, x: u32, y: u32, w: u32, h: u32) -> Option<usize> {
        if self.unflushed {
            self.unflushed = false;
            unsafe { self.context.Flush() };
        }
        let context = &self.context;
        let i = self.copies.serving((x, y, w, h), Instant::now(), &mut |s| finished_on_card(context, s))?;
        let u = &mut self.copies.unions[i];
        if u.mapped.is_none() {
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe { context.Map(&u.staging, 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32, Some(&mut m)) }.ok()?;
            u.mapped = Some((m.pData as *const u8, m.RowPitch as usize));
        }
        Some(i)
    }

    pub fn has_image(&self) -> bool {
        self.cache.is_some()
    }

    /// Size of the newest image.
    pub fn size(&self) -> (u32, u32) {
        self.cache.as_ref().map_or((0, 0), |c| (c.1, c.2))
    }

    /// Copies the area `x, y, width, height` of the newest image into memory at `out` (rows `stride`
    /// bytes apart, BGRA). Only this area leaves the graphics card. With `shrink` 2 or 4 the card first averages it
    /// down to half or a quarter (its mip levels), and only that smaller picture leaves it: width / shrink by
    /// height / shrink (rounded down). The image it came from: `served`.
    pub fn read(&mut self, x: u32, y: u32, width: u32, height: u32, out: *mut u8, stride: usize, shrink: u32) -> Result<()> {
        self.last_read = (self.generation, self.captured_at);
        if shrink <= 1 {
            self.union()?;
            let Some(i) = self.union_ready(x, y, width, height) else {
                // callers ask ready() first; asked anyway: nothing finished holds it yet
                return Err(E_FAIL.into());
            };
            let u = &self.copies.unions[i];
            let (ux, uy, _, _) = u.rect;
            let (data, pitch) = u.mapped.unwrap();
            self.last_read = (u.generation, u.captured);
            unsafe {
                for row in 0..height as usize {
                    std::ptr::copy_nonoverlapping(
                        data.add((y - uy) as usize * pitch + row * pitch + (x - ux) as usize * 4),
                        out.add(row * stride),
                        width as usize * 4,
                    );
                }
            }
            return Ok(());
        }
        let key = (x, y, width, height, shrink);
        if !self.queued.iter().any(|q| q.key == key) {
            self.queue(x, y, width, height, shrink)?;
        }
        let Some(i) = self.queued.iter().position(|q| q.key == key) else {
            // an empty area; or nothing to copy it from (callers ask ready() first, which says so)
            return if self.lost { Err(E_FAIL.into()) } else { Ok(()) };
        };
        let q = self.queued.remove(i);
        self.last_read = (q.generation, q.captured);
        let (staging, width, height) = (q.staging, q.width, q.height);
        unsafe {
            // the first map of a round waits for the card once; the copies queued with it are done by then
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            let waited = std::time::Instant::now();
            self.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            // a wait this long leaves every reader without a picture (its slot stays "being written"): on 6 Nov the
            // meter had none for 18 and 28 s right after an Interlink result screen. Said in app.log, to find out why.
            let ms = waited.elapsed().as_millis();
            if ms >= 1000 {
                crate::app_log(&format!("capture: the graphics card took {ms} ms to hand over a picture ({width}x{height})"));
            }
            if !q.done {
                self.copies.stats.came_back(q.at.elapsed());
                self.copies.copy_done(q.generation);
            }
            let source = mapped.pData as *const u8;
            for row in 0..height as usize {
                std::ptr::copy_nonoverlapping(
                    source.add(row * mapped.RowPitch as usize),
                    out.add(row * stride),
                    width as usize * 4,
                );
            }
            self.context.Unmap(&staging, 0);
        }
        Ok(())
    }

    /// Whether the card has finished copying this area off (queued now if it isn't yet), without waiting. With the
    /// game keeping the card busy, a whole 4K picture took it 500-850 ms on an Interlink result screen (7 Nov); waited
    /// for, that left the reader's picture "being written" the whole time and the meter without one for 5-13 s. The
    /// caller asks again next round and keeps the finished picture it has readable meanwhile.
    ///
    /// Some(generation): ready, from that image, which with the card busy can be older than the newest (a reader that
    /// already has that image's picture has nothing new: capture.rs `is_new`). None: not yet.
    pub fn ready(&mut self, x: u32, y: u32, width: u32, height: u32, shrink: u32) -> Result<Option<u64>> {
        let now = (!self.lost).then_some(self.generation);
        if shrink <= 1 {
            if width == 0 || height == 0 {
                return Ok(now);
            }
            if !self.copies.unions.iter().any(|u| u.generation == self.generation && holds(u.rect, (x, y, width, height))) {
                self.queue(x, y, width, height, 1)?;
            }
            self.union()?;
            return Ok(self.union_ready(x, y, width, height).map(|i| self.copies.unions[i].generation));
        }
        let key = (x, y, width, height, shrink);
        if !self.queued.iter().any(|q| q.key == key) {
            self.queue(x, y, width, height, shrink)?;
        }
        // an empty area; or the held image was given back and no newer one came: asked again next round
        let Some(i) = self.queued.iter().position(|q| q.key == key) else { return Ok(now) };
        // the copies queued are sent to the card now (once), or a look that doesn't wait could find them never started:
        // at 1440p with the game keeping the card busy, 22,000 looks a minute found copies unfinished (16 Nov)
        if self.unflushed {
            self.unflushed = false;
            unsafe { self.context.Flush() };
        }
        let q = &mut self.queued[i];
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        match unsafe { self.context.Map(&q.staging, 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32, Some(&mut mapped)) } {
            Ok(()) => {
                unsafe { self.context.Unmap(&q.staging, 0) };
                if !q.done {
                    q.done = true;
                    let (took, generation) = (q.at.elapsed(), q.generation);
                    self.copies.stats.came_back(took);
                    self.copies.copy_done(generation);
                }
                Ok(Some(self.queued[i].generation))
            }
            Err(e) if e.code() == DXGI_ERROR_WAS_STILL_DRAWING => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The round is over: copies read are gone; one still on its way stays for the next rounds (ready), up to
    /// KEEP_QUEUED, then it's dropped and started again.
    pub fn end_round(&mut self) {
        self.give_back();
        // capture-frame-hold: a replaced frame back to Windows as soon as its copies are done (the round's reads
        // waited for the card), so the pool has its buffer for the next frame before the next round (nothing to do
        // with the switch off)
        if !self.retiring.is_empty() {
            self.retire_done();
        }
        let now = Instant::now();
        let (context, mut dropped) = (&self.context, 0);
        // (a shrunk area's copy is looked at only by its reader: one left behind by a new area may be long finished)
        self.queued.retain(|q| {
            let keep = now.saturating_duration_since(q.at) < KEEP_QUEUED;
            dropped += (!keep && !q.done && !finished_on_card(context, &q.staging)) as u32;
            keep
        });
        self.copies.stats.dropped += dropped;
        self.round_want.clear();
        self.copies.end_round(now, &mut |s| unsafe { context.Unmap(s, 0) }, &mut |s| finished_on_card(context, s));
        if !self.copies.stats.is_empty() {
            let stats = std::mem::replace(&mut self.copies.stats, CopyStats::new());
            COPY_STATS.lock().unwrap_or_else(|e| e.into_inner()).add(stats);
        }
    }

    /// The image (generation) the last read() came from, and when Windows captured it (None: the source doesn't say).
    pub fn served(&self) -> (u64, Option<Instant>) {
        self.last_read
    }

    /// Starts copying the area `x, y, width, height` (shrunk by `shrink`) off the card into a staging texture of its
    /// own, without waiting: every reader's area is queued first, then read() takes them one after another, so a
    /// round waits for the card once instead of once per reader (each wait was a small stall; B8).
    pub fn queue(&mut self, x: u32, y: u32, width: u32, height: u32, shrink: u32) -> Result<()> {
        if shrink <= 1 {
            if width > 0 && height > 0 {
                self.round_want.push((x, y, width, height));
            }
            return Ok(());
        }
        let key = (x, y, width, height, shrink);
        if self.queued.iter().any(|q| q.key == key) {
            return Ok(()); // already on its way (from an earlier round, not finished yet)
        }
        if self.lost {
            self.starved.get_or_insert_with(std::time::Instant::now);
            return Ok(());
        }
        let Some((cache, cw, ch)) = self.cache.as_ref() else { return Err(E_FAIL.into()) };
        let (width, height) = (width.min(cw.saturating_sub(x)), height.min(ch.saturating_sub(y)));
        if width == 0 || height == 0 {
            return Ok(());
        }
        let cache = cache.clone();
        let level = match shrink {
            4 => 2,
            2 => 1,
            _ => 0,
        };
        let area = D3D11_BOX { left: x, top: y, front: 0, right: x + width, bottom: y + height, back: 1 };
        let (source, sub, width, height) = if level == 0 || width >> level == 0 || height >> level == 0 {
            (cache, 0, width, height)
        } else {
            // the area into a texture with mip levels, which the card fills with the averaged smaller pictures
            let levels = level + 1;
            let index = match self.shrinkers.iter().position(|s| s.0 == width && s.1 == height && s.2 == levels) {
                Some(i) => {
                    let used = self.shrinkers.remove(i); // most recently used to the back
                    self.shrinkers.push(used);
                    self.shrinkers.len() - 1
                }
                None => {
                    let desc = D3D11_TEXTURE2D_DESC {
                        MipLevels: levels,
                        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                        MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
                        ..texture_desc(width, height, false)
                    };
                    let (mut texture, mut view) = (None, None);
                    unsafe {
                        self.device.CreateTexture2D(&desc, None, Some(&mut texture))?;
                        let texture = texture.as_ref().ok_or(E_FAIL)?;
                        self.device.CreateShaderResourceView(texture, None, Some(&mut view))?;
                    }
                    make_room(&mut self.shrinkers, |s| (s.0, s.1), width, height);
                    self.shrinkers.push((width, height, levels, texture.ok_or(E_FAIL)?, view.ok_or(E_FAIL)?));
                    self.shrinkers.len() - 1
                }
            };
            let (_, _, _, texture, view) = &self.shrinkers[index];
            unsafe {
                self.context.CopySubresourceRegion(texture, 0, 0, 0, 0, &cache, 0, Some(&area));
                self.context.GenerateMips(view);
            }
            (texture.clone(), level, width >> level, height >> level)
        };
        // a staging texture of this size that no other queued area is using
        let index = match self.staging.iter().position(|s| s.0 == width && s.1 == height && !self.queued.iter().any(|q| q.staging == s.2)) {
            Some(i) => {
                let used = self.staging.remove(i);
                self.staging.push(used);
                self.staging.len() - 1
            }
            None => {
                let mut texture = None;
                unsafe { self.device.CreateTexture2D(&texture_desc(width, height, true), None, Some(&mut texture))? };
                make_room(&mut self.staging, |s| (s.0, s.1), width, height);
                self.staging.push((width, height, texture.ok_or(E_FAIL)?));
                self.staging.len() - 1
            }
        };
        let staging = self.staging[index].2.clone();
        unsafe {
            if sub == 0 {
                self.context.CopySubresourceRegion(&staging, 0, 0, 0, 0, &source, 0, Some(&area));
            } else {
                self.context.CopySubresourceRegion(&staging, 0, 0, 0, 0, &source, sub, None);
            }
        }
        let (generation, captured) = (self.generation, self.captured_at);
        self.queued.push(Queued { key, staging, width, height, at: Instant::now(), generation, captured, done: false });
        self.copies.copy_from(generation);
        self.unflushed = true;
        Ok(())
    }
}

/// Desktop Duplication of the screen the game window is on.
pub struct Duplication {
    pub gpu: Gpu,
    duplication: IDXGIOutputDuplication,
    /// That screen's position on the desktop, and the screen itself (to notice the game moving to another).
    pub desktop: RECT,
    pub monitor: HMONITOR,
    /// When the last new image came: one is held only while they keep coming (FLOWING).
    last_new: Option<std::time::Instant>,
    /// Images held, images copied and rounds without a new one, for a line in app.log once a minute while holding.
    counts: [u32; 3],
    counted_since: std::time::Instant,
}

impl Duplication {
    pub fn new(hwnd: HWND) -> Result<Duplication> {
        unsafe {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
            let mut a = 0;
            while let Ok(adapter) = factory.EnumAdapters1(a) {
                a += 1;
                let mut o = 0;
                while let Ok(output) = adapter.EnumOutputs(o) {
                    o += 1;
                    let desc = output.GetDesc()?;
                    if desc.Monitor != monitor {
                        continue;
                    }
                    let (device, context) = device(Some(&adapter.cast::<IDXGIAdapter>()?))?;
                    let duplication = output.cast::<IDXGIOutput1>()?.DuplicateOutput(&device)?;
                    return Ok(Duplication {
                        gpu: Gpu::new(device, context),
                        duplication,
                        desktop: desc.DesktopCoordinates,
                        monitor,
                        last_new: None,
                        counts: [0; 3],
                        counted_since: std::time::Instant::now(),
                    });
                }
            }
        }
        Err(E_FAIL.into())
    }

    /// Takes the newest screen image if it changed. Ok(false): nothing new (or no image yet). An error means
    /// the duplication is lost (resolution or fullscreen change, a UAC prompt): make a new one.
    pub fn update(&mut self) -> Result<bool> {
        self.gpu.low_priority(LOW_PRIORITY.load(std::sync::atomic::Ordering::Relaxed));
        self.update_as(crate::dev::on("capture-hold-frame"))
    }

    /// `hold` (dev switch capture-hold-frame): instead of copying the whole screen into a texture of our own on every
    /// new image and giving the frame straight back, the frame is kept for the round and the readers' areas are
    /// copied out of the screen's own texture; it goes back when the round ends (Gpu::end_round). Off: exactly as
    /// before.
    fn update_as(&mut self, hold: bool) -> Result<bool> {
        self.gpu.give_back(); // a round that ended without its end_round
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        match unsafe { self.duplication.AcquireNextFrame(0, &mut info, &mut resource) } {
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                self.count(2);
                // a still screen right after a held image went back: a reader with a new area has nothing to get it
                // from. Started again, Desktop Duplication hands over the whole screen at once, and that one is copied.
                if self.gpu.starved.is_some_and(|t| t.elapsed() >= STARVED) {
                    crate::app_log("capture: no new screen image while a reader waits for a new area; starting Desktop Duplication again");
                    return Err(E_FAIL.into());
                }
                return Ok(false);
            }
            Err(e) => return Err(e),
            Ok(()) => {}
        }
        // (whether the image is new, whether the frame is kept)
        let result: Result<(bool, bool)> = (|| {
            // only the mouse pointer moved: the picture is the same
            let same = info.LastPresentTime == 0 && self.gpu.has_image();
            if same && !self.gpu.lost {
                return Ok((false, false));
            }
            let texture: ID3D11Texture2D = resource.ok_or(E_FAIL)?.cast()?;
            if same {
                self.gpu.hold(&texture, false, &self.duplication)?; // the image that was given back, unchanged: ours again
                return Ok((false, true));
            }
            // held only while images keep coming: on a still screen the copy stays readable for any new area
            let keep = hold && self.last_new.is_some_and(|t| t.elapsed() < FLOWING);
            self.last_new = Some(std::time::Instant::now());
            if keep {
                self.gpu.hold(&texture, true, &self.duplication)?;
            } else {
                self.gpu.store(&texture, u32::MAX, u32::MAX)?; // the whole screen, at its real pixel size
            }
            Ok((true, keep))
        })();
        match result {
            Ok((new, keep)) => {
                if !keep {
                    let _ = unsafe { self.duplication.ReleaseFrame() };
                }
                if new {
                    self.count(if keep { 0 } else { 1 });
                }
                Ok(new)
            }
            Err(e) => {
                self.gpu.held = None;
                let _ = unsafe { self.duplication.ReleaseFrame() };
                Err(e)
            }
        }
    }

    /// One more image held (0), copied (1) or round without a new one (2); once a minute, if any was held, app.log
    /// says how many of each, so a test can tell what the switch did.
    fn count(&mut self, what: usize) {
        self.counts[what] += 1;
        if self.counted_since.elapsed().as_secs() >= 60 {
            if self.counts[0] > 0 {
                let [held, copied, none] = self.counts;
                crate::app_log(&format!("capture: hold-frame, last minute: {held} images held, {copied} copied, {none} rounds without a new one"));
            }
            (self.counts, self.counted_since) = ([0; 3], std::time::Instant::now());
        }
    }
}

impl Drop for Duplication {
    fn drop(&mut self) {
        self.gpu.give_back();
    }
}

/// Windows Graphics Capture of the game window, without the yellow border.
pub struct WindowCapture {
    pub gpu: Gpu,
    device: IDirect3DDevice,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    size: SizeInt32,
    /// Dev switch capture-frame-hold: the fastest due reader's interval (ms, from capture.rs), and the session's
    /// MinUpdateInterval as set from it (None: never set, Windows' own), or whether setting it failed.
    fastest: Option<u64>,
    interval_set: Option<u64>,
    interval_failed: bool,
    /// Frames held, frames copied, and updates that found a replaced frame still being copied from; app.log once a
    /// minute while holding.
    counts: [u32; 3],
    counted_since: std::time::Instant,
}

/// capture-frame-hold: whether this Windows has GraphicsCaptureSession.MinUpdateInterval (Windows 11 24H2 and later).
fn min_interval_supported() -> bool {
    static KNOWN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *KNOWN.get_or_init(|| {
        ApiInformation::IsPropertyPresent(&HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession"), &HSTRING::from("MinUpdateInterval"))
            .unwrap_or(false)
    })
}

impl WindowCapture {
    /// True where Windows allows window capture without its yellow border (Windows 11).
    pub fn borderless_supported() -> bool {
        let class = HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession");
        GraphicsCaptureSession::IsSupported().unwrap_or(false)
            && ApiInformation::IsPropertyPresent(&class, &HSTRING::from("IsBorderRequired")).unwrap_or(false)
    }

    /// Window capture without the yellow border; with `border_ok`, also where Windows draws it (Windows 10: only
    /// used while the player is tabbed out and the window can't be copied otherwise).
    pub fn new(hwnd: HWND, border_ok: bool) -> Result<WindowCapture> {
        if !border_ok && !Self::borderless_supported() {
            return Err(E_FAIL.into());
        }
        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            return Err(E_FAIL.into());
        }
        // Borderless capture needs the user's (or for desktop apps, Windows') consent; never capture with a border
        // unless that's allowed.
        if !border_ok && ApiInformation::IsTypePresent(&HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureAccess")).unwrap_or(false) {
            let status = GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)?.join()?;
            if status != AppCapabilityAccessStatus::Allowed {
                return Err(E_FAIL.into());
            }
        }
        // on the adapter that shows the game's monitor (the default one only when that can't be found or used)
        raise_gpu_class();
        let adapter = window_adapter(hwnd);
        let name = adapter.as_ref().map_or_else(|| "the default adapter".to_string(), |a| a.1.clone());
        let (d3d, context) = match adapter.as_ref().and_then(|a| device(Some(&a.0)).ok()) {
            Some(d) => d,
            None => device(None)?,
        };
        crate::app_log(&format!("capture: window capture on {name}"));
        let device: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&d3d.cast::<IDXGIDevice>()?)? }.cast()?;
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd)? };
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 3, size)?;
        let session = pool.CreateCaptureSession(&item)?;
        if let Err(e) = session.SetIsBorderRequired(false) {
            if !border_ok {
                return Err(e);
            }
        }
        let _ = session.SetIsCursorCaptureEnabled(false);
        session.StartCapture()?;
        Ok(WindowCapture {
            gpu: Gpu::new(d3d, context),
            device,
            pool,
            session,
            size,
            fastest: None,
            interval_set: None,
            interval_failed: false,
            counts: [0; 3],
            counted_since: std::time::Instant::now(),
        })
    }

    /// The fastest reader's interval now (ms; capture.rs, every round): with capture-frame-hold on, the session's
    /// MinUpdateInterval follows it (update). Unused with the switch off.
    pub fn fastest_reader(&mut self, ms: Option<u64>) {
        self.fastest = ms;
    }

    /// capture-frame-hold: Windows hands over a new frame no more often than the fastest reader asks for one, where it
    /// can be told (min_interval_supported); set only when that interval changes.
    fn follow_fastest(&mut self) {
        let Some(ms) = self.fastest else { return };
        if self.interval_set == Some(ms) || self.interval_failed || !min_interval_supported() {
            return;
        }
        match self.session.SetMinUpdateInterval(TimeSpan { Duration: ms as i64 * 10_000 }) {
            Ok(()) => {
                if self.interval_set.is_none() {
                    crate::app_log(&format!("capture: frame-hold: window capture's update interval follows the fastest reader ({ms} ms now)"));
                }
                self.interval_set = Some(ms);
            }
            Err(e) => {
                self.interval_failed = true;
                crate::app_log(&format!("capture: frame-hold: window capture's update interval not set ({e})"));
            }
        }
    }

    /// One more frame held (0), copied (1) or update with a replaced frame still being copied from (2); once a minute,
    /// if any was held, app.log says how many of each, so a test can tell what the switch did.
    fn count(&mut self, what: usize) {
        self.counts[what] += 1;
        if self.counted_since.elapsed().as_secs() >= 60 {
            if self.counts[0] > 0 {
                let [held, copied, busy] = self.counts;
                let (back, most) = (self.gpu.frames_back, self.gpu.most_waiting);
                let interval = self.interval_set.map_or("Windows' own".to_string(), |ms| format!("{ms} ms"));
                crate::app_log(&format!(
                    "capture: frame-hold, last minute: {held} frames held, {copied} copied, {busy} updates with a replaced frame still being copied from, {back} replaced frames given straight back, at most {most} waiting to go back at once; update interval {interval}"
                ));
            }
            (self.counts, self.counted_since) = ([0; 3], std::time::Instant::now());
            (self.gpu.frames_back, self.gpu.most_waiting) = (0, self.gpu.retiring.len());
        }
    }

    /// Takes the newest window image if there is one. Ok(false): nothing new since the last.
    ///
    /// Dev switch capture-frame-hold (14 Nov): instead of copying every new frame whole into a texture of our own and
    /// closing it at once, the newest frame stays open as the image and the readers' areas are copied out of it; it's
    /// closed only once a newer frame has come and the card has done every copy queued from it (Gpu::retire_frame),
    /// while the pool's other buffer takes the next frame. Off: exactly as before.
    pub fn update(&mut self) -> Result<bool> {
        // On by default since 16 Nov (at 1440p the copy of every whole frame was the biggest job asked of a busy card)
        self.update_as(true)
    }

    /// `hold`: the Dev switch capture-frame-hold (update).
    fn update_as(&mut self, hold: bool) -> Result<bool> {
        if !hold && self.interval_set.is_some() {
            // turned off after it set the session's update interval: a new session, as Windows starts one
            crate::app_log("capture: frame-hold off: window capture starts again at Windows' own update interval");
            return Err(E_FAIL.into());
        }
        if !self.gpu.retiring.is_empty() && self.gpu.retire_done() > 0 {
            self.count(2);
        }
        if hold {
            self.follow_fastest();
        }
        // the pool hands frames out oldest first: take all that are waiting and keep the newest (one at a time,
        // a picture was up to two reader turns old while stamped as new)
        let mut newest = None;
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            if let Some(older) = newest.replace(frame) {
                let _ = older.Close();
            }
        }
        let Some(frame) = newest else { return Ok(false) };
        self.gpu.low_priority(LOW_PRIORITY.load(std::sync::atomic::Ordering::Relaxed));
        // when Windows captured it (its QPC time, 100 ns units), as a moment on this clock: readers then time a fight
        // by when the game showed a frame, not by when they got around to reading it (B8)
        self.gpu.captured_at = frame.SystemRelativeTime().ok().and_then(|t| {
            let (mut count, mut freq) = (0i64, 0i64);
            unsafe {
                windows::Win32::System::Performance::QueryPerformanceCounter(&mut count).ok()?;
                windows::Win32::System::Performance::QueryPerformanceFrequency(&mut freq).ok()?;
            }
            let now_100ns = (count as i128 * 10_000_000 / freq.max(1) as i128) as i64;
            let age = std::time::Duration::from_nanos(((now_100ns - t.Duration).max(0) as u64).saturating_mul(100));
            std::time::Instant::now().checked_sub(age.min(std::time::Duration::from_secs(60)))
        });
        let mut kept = false; // capture-frame-hold: the frame is the image now, closed later (retire_done)
        let result = (|| {
            let content = frame.ContentSize()?;
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
            let resized = content.Width != self.size.Width || content.Height != self.size.Height;
            // (a resized window's frame is copied, as before: the pool is made again at the new size just below)
            if hold && !resized {
                self.gpu.hold_frame(frame.clone(), &texture, content.Width.max(1) as u32, content.Height.max(1) as u32)?;
                kept = true;
            } else {
                self.gpu.store(&texture, content.Width.max(1) as u32, content.Height.max(1) as u32)?;
                self.gpu.retire_frame()?; // a frame held before (the switch just turned off, or a resize): retired
            }
            if resized {
                self.size = content; // the window was resized: buffers at the new size from now on
                self.pool.Recreate(&self.device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 3, content)?;
            }
            Ok(true)
        })();
        if !kept {
            let _ = frame.Close();
        }
        if hold && result.is_ok() {
            self.count(if kept { 0 } else { 1 });
        }
        result
    }
}

impl Drop for WindowCapture {
    fn drop(&mut self) {
        self.gpu.close_frames(); // capture-frame-hold: nothing reads from them any more
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

/// Where the window's picture starts on screen: window capture images begin at the window's visible frame
/// (without the invisible resize borders).
pub fn window_frame_origin(hwnd: HWND) -> Option<(i32, i32)> {
    let mut rect = RECT::default();
    unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32)
            .ok()?;
    }
    Some((rect.left, rect.top))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Times Desktop Duplication on this PC: `cargo test gpu_capture -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn duplication_speed() {
        unsafe {
            windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        let mut d = Duplication::new(hwnd).expect("Desktop Duplication");
        let started = std::time::Instant::now();
        while !d.gpu.has_image() && started.elapsed().as_secs() < 3 {
            d.update().expect("update");
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        let (w, h) = d.gpu.size();
        println!("screen {w}x{h}, first image after {:?}", started.elapsed());
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for (name, aw, ah) in [("whole", w, h), ("top 42%", w, h * 42 / 100), ("corner 600x400", 600, 400)] {
            let t = std::time::Instant::now();
            for _ in 0..20 {
                d.gpu.read(0, 0, aw, ah, buf.as_mut_ptr(), w as usize * 4, 1).unwrap();
            }
            println!("{name}: {:.2} ms per read", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
        }
        let t = std::time::Instant::now();
        let mut new = 0;
        for _ in 0..60 {
            if d.update().unwrap() { new += 1; }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        println!("update: {new} new images in {:?}", t.elapsed());
    }

    /// Dev switch capture-hold-frame against the copy, on this PC's screen as it is now:
    /// `cargo test --release --bins hold_frame -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn hold_frame_speed() {
        unsafe {
            windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        for hold in [false, true, false, true] {
            let mut d = Duplication::new(hwnd).expect("Desktop Duplication");
            let started = std::time::Instant::now();
            while !d.gpu.has_image() && started.elapsed().as_secs() < 3 {
                d.update_as(hold).expect("update");
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
            let (w, h) = d.gpu.size();
            let mut buf = vec![0u8; 600 * 400 * 4];
            let (mut new, mut held, mut none, mut read, mut waiting, mut x) = (0, 0, 0, 0, 0, 0u32);
            let (mut in_update, mut in_read) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
            for round in 0..300u32 {
                let t = std::time::Instant::now();
                match d.update_as(hold) {
                    Ok(true) => new += 1,
                    Ok(false) => none += 1,
                    Err(_) => {
                        // a still screen with a reader waiting: started again, as the capture thread does
                        d = Duplication::new(hwnd).expect("Desktop Duplication again");
                        continue;
                    }
                }
                in_update += t.elapsed();
                held += d.gpu.held.is_some() as u32;
                // an area asked for until it's read (as serve does), then a new one: the hard case while nothing is held
                let t = std::time::Instant::now();
                if d.gpu.ready(x, 0, 600, 400, 1).expect("ready").is_some() {
                    d.gpu.read(x, 0, 600, 400, buf.as_mut_ptr(), 600 * 4, 1).expect("read");
                    assert!(buf.chunks(4).step_by(97).any(|p| p[..3] != [0, 0, 0]), "a black picture");
                    read += 1;
                    x = (round * 7) % (w - 600);
                } else {
                    waiting += 1;
                }
                in_read += t.elapsed();
                d.gpu.end_round();
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
            println!(
                "hold {hold}: screen {w}x{h}, 300 rounds: {new} new images ({held} rounds holding), {none} without; {read} areas read, {waiting} not yet; update {:.3} ms, read {:.3} ms a round",
                in_update.as_secs_f64() * 1000.0 / 300.0,
                in_read.as_secs_f64() * 1000.0 / 300.0,
            );
        }
    }

    /// Dev switch capture-frame-hold against the copy, on window capture of the window in front (Windows 11; with
    /// the yellow border elsewhere): `cargo test --release --bins frame_hold -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn frame_hold_speed() {
        unsafe {
            windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        for hold in [false, true, false, true] {
            let mut w = WindowCapture::new(hwnd, true).expect("window capture");
            w.fastest_reader(Some(16));
            let started = std::time::Instant::now();
            while !w.gpu.has_image() && started.elapsed().as_secs() < 3 {
                w.update_as(hold).expect("update");
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
            let (iw, ih) = w.gpu.size();
            let (aw, ah) = (iw.min(600), ih.min(400));
            let mut buf = vec![0u8; (aw * ah * 4) as usize];
            let (mut new, mut none, mut read, mut waiting, mut busy) = (0, 0, 0, 0, 0);
            let (mut in_update, mut in_read) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
            for round in 0..300u32 {
                let t = std::time::Instant::now();
                if w.update_as(hold).expect("update") { new += 1 } else { none += 1 }
                in_update += t.elapsed();
                busy += w.gpu.retiring.len();
                let x = (round * 7) % (iw - aw).max(1);
                let t = std::time::Instant::now();
                if w.gpu.ready(x, 0, aw, ah, 1).expect("ready").is_some() {
                    w.gpu.read(x, 0, aw, ah, buf.as_mut_ptr(), aw as usize * 4, 1).expect("read");
                    read += 1;
                } else {
                    waiting += 1;
                }
                in_read += t.elapsed();
                w.gpu.end_round();
                assert!(w.gpu.retiring.len() <= 1, "frames pile up");
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
            println!(
                "hold {hold}: window {iw}x{ih}, 300 rounds: {new} new frames, {none} without; {read} areas read, {waiting} not yet; {busy} rounds with a frame still retiring; update {:.3} ms, read {:.3} ms a round",
                in_update.as_secs_f64() * 1000.0 / 300.0,
                in_read.as_secs_f64() * 1000.0 / 300.0,
            );
        }
    }

    #[test]
    #[ignore]
    fn window_capture_available() {
        println!("borderless window capture supported: {}", WindowCapture::borderless_supported());
    }

    // A slow graphics card without a game: copies that come back only some rounds after they were queued.

    const TOP: Rect = (0, 0, 1920, 450);
    const WHOLE: Rect = (0, 0, 1920, 1080);
    const ICON: Rect = (1800, 1000, 100, 60);

    fn union(rect: Rect, staging: usize, generation: u64, at: Instant) -> Union<usize> {
        Union { rect, staging, generation, captured: None, at, mapped: None, done: false }
    }

    /// A pretend graphics card for Copies: copy `n` (its staging "texture" is its number) is finished `late` rounds
    /// after it was queued, and never before the ones queued earlier (one device's copies are done in order).
    struct SlowCard {
        late: u32,
        round: u32,
        ready_at: Vec<u32>,
    }

    impl SlowCard {
        fn new(late: u32) -> SlowCard {
            SlowCard { late, round: 0, ready_at: Vec::new() }
        }
        fn copy(&mut self) -> usize {
            let after = self.ready_at.last().copied().unwrap_or(0);
            self.ready_at.push((self.round + self.late).max(after));
            self.ready_at.len() - 1
        }
        fn finished(&self, n: usize) -> bool {
            self.round >= self.ready_at[n]
        }
        /// When an event query issued now is answered: once every copy queued so far is done.
        fn query(&self) -> u32 {
            self.ready_at.last().copied().unwrap_or(0).max(self.round)
        }
    }

    /// One round as serve and Gpu do it: the due readers' `areas` asked for, the box planned (and started on the card),
    /// then each area looked for. The image each one's copy came from, if one is back.
    fn round(copies: &mut Copies<usize>, card: &mut SlowCard, now: Instant, generation: u64, areas: &[Rect]) -> Vec<Option<u64>> {
        let plan = copies.plan(areas, generation, (1920, 1080), now, &mut |s| card.finished(*s));
        if let Plan::Start(rect) = plan {
            let n = card.copy();
            copies.started(union(rect, n, generation, now));
        }
        let got = areas
            .iter()
            .map(|&a| {
                let i = copies.serving(a, now, &mut |s| card.finished(*s))?;
                copies.unions[i].mapped = Some((std::ptr::null(), 0)); // read this round (end_round)
                Some(copies.unions[i].generation)
            })
            .collect();
        copies.end_round(now, &mut |_| {}, &mut |s| card.finished(*s));
        got
    }

    #[test]
    fn when_a_new_box_may_start() {
        let t = Instant::now();
        let on_way = |rect, generation| union(rect, 0, generation, t);
        let back = |rect, generation| Union { done: true, ..union(rect, 0, generation, t) };
        assert!(may_start::<usize>(&[], &[TOP], 5, true), "nothing on its way");
        assert!(may_start(&[back(TOP, 5)], &[WHOLE], 5, true), "the one before is back");
        assert!(!may_start(&[on_way(TOP, 5)], &[(10, 10, 100, 100)], 6, true), "on its way with an area a reader asks for: it serves that one");
        assert!(may_start(&[on_way(TOP, 5)], &[ICON], 5, true), "nobody asks for anything in it, same frame: beside it");
        assert!(!may_start(&[on_way(TOP, 4)], &[ICON], 5, true), "nobody's, but it reads a replaced frame: that would hold a second one");
        assert!(may_start(&[on_way(TOP, 4)], &[ICON], 5, false), "...which only matters with frame-hold");
        assert!(!may_start(&[on_way(TOP, 5), on_way(ICON, 5)], &[(0, 600, 100, 100)], 5, false), "two on their way already");
    }

    /// Change 2: a copy on its way that holds nothing a due reader asks for doesn't hold up a new box.
    #[test]
    fn a_copy_nobody_wants_doesnt_hold_up_a_new_box() {
        let t = Instant::now();
        let mut card = SlowCard::new(100);
        let mut copies: Copies<usize> = Copies::new();
        copies.frame_held(Some(1));
        round(&mut copies, &mut card, t, 1, &[TOP]);
        card.round = 1;
        assert_eq!(copies.plan(&[TOP, ICON], 1, (1920, 1080), t, &mut |s| card.finished(*s)), Plan::Wait, "its reader still asks: it waits");
        assert_eq!(round(&mut copies, &mut card, t, 1, &[ICON]), vec![None], "nobody asks for the top part: the icon row starts beside it");
        assert_eq!((copies.unions.len(), copies.stats.beside), (2, 1));
        assert_eq!(copies.frame_reads(), 2, "both read the held frame: it waits for them once it's replaced");
        card.round = 2;
        assert_eq!(copies.plan(&[(0, 600, 10, 10)], 1, (1920, 1080), t, &mut |s| card.finished(*s)), Plan::Wait, "two on their way");
        // Desktop Duplication (no frame held): beside one of an older image too
        let mut copies: Copies<usize> = Copies::new();
        round(&mut copies, &mut card, t, 1, &[TOP]);
        assert_eq!(copies.plan(&[ICON], 2, (1920, 1080), t, &mut |s| card.finished(*s)), Plan::Start(ICON));
    }

    /// Change 1: the copy on its way is seen finished whoever asked for it, so the next box starts behind it.
    #[test]
    fn a_copy_nobody_reads_is_seen_finished() {
        let t = Instant::now();
        let mut card = SlowCard::new(10);
        let mut copies: Copies<usize> = Copies::new();
        copies.frame_held(Some(1));
        assert_eq!(round(&mut copies, &mut card, t, 1, &[TOP]), vec![None]);
        // the meter gave up on the top part and asks for the whole frame; the frame was replaced meanwhile
        copies.frame_held(Some(2));
        for r in 1..10 {
            card.round = r;
            assert_eq!(copies.plan(&[WHOLE], 2, (1920, 1080), t, &mut |s| card.finished(*s)), Plan::Wait, "round {r}");
        }
        card.round = 10;
        assert_eq!(copies.plan(&[WHOLE], 2, (1920, 1080), t, &mut |s| card.finished(*s)), Plan::Start(WHOLE), "the round it's back");
        assert!(copies.unions[0].done, "seen finished with nobody asking for anything in it");
    }

    #[test]
    fn the_newest_finished_copy_serves_and_says_its_image() {
        let t = Instant::now();
        let mut card = SlowCard::new(5);
        let mut copies: Copies<usize> = Copies::new();
        copies.started(union(WHOLE, card.copy(), 3, t));
        card.round = 5;
        copies.started(union(TOP, card.copy(), 7, t));
        assert_eq!(copies.serving(ICON, t, &mut |s| card.finished(*s)), Some(0), "only the whole frame holds it");
        let i = copies.serving((0, 0, 100, 100), t, &mut |s| card.finished(*s)).unwrap();
        assert_eq!(copies.unions[i].generation, 3, "the top part of image 7 isn't back: the older copy, saying so");
        card.round = 10;
        let i = copies.serving((0, 0, 100, 100), t, &mut |s| card.finished(*s)).unwrap();
        assert_eq!(copies.unions[i].generation, 7, "back now: the newest");
        assert_eq!(copies.serving((0, 1070, 10, 10), t, &mut |s| card.finished(*s)), Some(0));
    }

    /// Change 3: a replaced frame waits for the card only while a copy started from it may still be reading it.
    #[test]
    fn a_replaced_frame_goes_back_at_once_unless_a_copy_reads_it() {
        let t = Instant::now();
        let mut copies: Copies<usize> = Copies::new();
        copies.frame_held(Some(1));
        assert_eq!(copies.frame_reads(), 0, "nothing copied from it: straight back");
        copies.frame_held(Some(2));
        copies.started(union(TOP, 0, 2, t));
        assert_eq!(copies.frame_reads(), 1, "a copy on its way reads it: it waits for its event query");
        copies.poll(t, &mut |_| true);
        assert_eq!(copies.frame_reads(), 0, "seen finished: straight back");
        copies.frame_held(Some(3));
        copies.started(union(TOP, 1, 3, t));
        copies.end_round(t + KEEP_QUEUED, &mut |_| {}, &mut |_| false);
        assert!(copies.unions.is_empty(), "dropped after KEEP_QUEUED");
        assert_eq!(copies.stats.dropped, 1);
        assert_eq!(copies.frame_reads(), 1, "dropped before it was seen finished: the card may still be reading the frame");
        // shrunk areas' copies count too (Gpu::queue, Gpu::ready); a copy of another image doesn't
        copies.frame_held(Some(4));
        copies.copy_from(4);
        copies.copy_from(3);
        assert_eq!(copies.frame_reads(), 1);
        copies.copy_done(3);
        assert_eq!(copies.frame_reads(), 1);
        copies.copy_done(4);
        assert_eq!(copies.frame_reads(), 0);
    }

    /// Changes 1-3 together on a card that hands every copy back 160 ms late (over the 120 ms a meter look waits),
    /// a new frame every 8 ms (120 fps), frame-hold's pool of 3 buffers, rounds 4 ms apart: the meter reading the top
    /// part and, once a look gives up, the whole frame until it gets one; and a slower reader low on the screen.
    /// Before, the whole frame waited for the top part's copy, which nobody read any more, to be dropped (2 s), and
    /// every replaced frame waited behind the card.
    #[test]
    fn a_slow_card_never_strands_readers() {
        const LATE: u32 = 40; // rounds (4 ms each)
        const GIVE_UP: u32 = 30; // the meter's 120 ms
        const ICON_EVERY: u32 = 25;
        let t = Instant::now();
        let at = |r: u32| t + Duration::from_millis(4 * r as u64);
        let mut card = SlowCard::new(LATE);
        let mut copies: Copies<usize> = Copies::new();
        let (mut generation, mut retiring) = (1u64, Vec::<u32>::new());
        copies.frame_held(Some(generation));
        let (mut frames, mut dry, mut most) = (0, 0, 0);
        // per reader: the image of its picture, the round it last got a new one, the longest wait
        let mut meter = (0u64, 0u32, 0u32);
        let mut icon = (0u64, 0u32, 0u32);
        let mut box_ = TOP;
        for r in 0..3000u32 {
            card.round = r;
            retiring.retain(|&answered| r < answered); // retire_done
            if r % 2 == 0 {
                // Windows hands over a frame only into a buffer of the pool's 3 that isn't held
                if 1 + retiring.len() < 3 {
                    if copies.frame_reads() > 0 {
                        retiring.push(card.query()); // retire_frame: waits for its copies
                    }
                    generation += 1;
                    copies.frame_held(Some(generation));
                    frames += 1;
                } else {
                    dry += 1;
                }
            }
            most = most.max(retiring.len());
            if r - meter.1 >= GIVE_UP {
                box_ = WHOLE; // a look gave up: the whole frame next (and until one comes)
            }
            let icon_due = r % ICON_EVERY == 0 || r - icon.1 > ICON_EVERY;
            let areas: Vec<Rect> = if icon_due { vec![box_, ICON] } else { vec![box_] };
            let got = round(&mut copies, &mut card, at(r), generation, &areas);
            for (reader, image) in [(&mut meter, got[0]), (&mut icon, got.get(1).copied().flatten())] {
                if let Some(image) = image.filter(|&g| g > reader.0) {
                    reader.2 = reader.2.max(r - reader.1);
                    (reader.0, reader.1) = (image, r);
                }
            }
            if meter.1 == r {
                box_ = TOP; // a picture came: back to the top part
            }
        }
        println!("meter waited at most {} rounds, the icon reader {}; {frames} frames, {dry} times no buffer, at most {most} waiting; {}", meter.2, icon.2, copies.stats.line());
        assert!(meter.2 <= 3 * LATE, "the meter waited {} rounds for a picture", meter.2);
        assert!(icon.2 <= 3 * LATE + ICON_EVERY, "the icon reader waited {} rounds", icon.2);
        assert_eq!(copies.stats.dropped, 0, "no copy left to run out");
        assert_eq!(dry, 0, "Windows always had a buffer for the next frame");
        assert!(most <= 1, "at most one frame waiting to go back");
        assert!(generation - meter.0 <= 2 * LATE as u64, "the meter's picture is recent (image {} of {generation})", meter.0);
    }

    #[test]
    fn copy_numbers_for_app_log() {
        let mut stats = CopyStats::new();
        for ms in [12, 30, 5, 400, 18] {
            stats.came_back(Duration::from_millis(ms));
        }
        stats.dropped = 2;
        stats.beside = 1;
        let mut all = CopyStats::new();
        all.add(stats);
        assert_eq!(
            all.line(),
            "copies off the card: 5 back (typical 18 ms, longest 400 ms), 2 dropped unfinished after 2 s, 1 started beside one nobody wanted"
        );
        assert_eq!(all.line(), "copies off the card: 0 back (typical 0 ms, longest 0 ms), 0 dropped unfinished after 2 s, 0 started beside one nobody wanted");
    }
}
