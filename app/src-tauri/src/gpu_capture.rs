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
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind, GraphicsCaptureItem,
    GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Security::Authorization::AppCapabilityAccess::AppCapabilityAccessStatus;
use windows::Win32::Foundation::{E_FAIL, HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_RESOURCE_MISC_GENERATE_MIPS,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
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

/// How long a copy the card hasn't finished stays queued for its reader (Gpu::ready) before it's started again.
const KEEP_QUEUED: std::time::Duration = std::time::Duration::from_secs(2);

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

fn device(adapter: Option<&IDXGIAdapter>) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
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
    /// Areas whose copy off the card is on its way (queue): (x, y, w, h, shrink), its staging texture, the picture's
    /// size and when it was queued. A copy the card hasn't finished (ready) stays queued into later rounds, so it
    /// isn't started again over and over; one not read within KEEP_QUEUED is dropped.
    queued: Vec<((u32, u32, u32, u32, u32), ID3D11Texture2D, u32, u32, std::time::Instant)>,
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
    /// Since when a reader has been asking for an area that can't be copied (`lost`).
    starved: Option<std::time::Instant>,
    /// Dev switch capture-low-priority: this device's work on the card goes behind the game's.
    low: bool,
}

impl Gpu {
    fn new(device: ID3D11Device, context: ID3D11DeviceContext) -> Gpu {
        Gpu {
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
        }
    }

    /// Dev switch capture-low-priority, on or off: the copies this device asks of the graphics card are done after
    /// the game's own work (the lowest of the priorities a program may give itself), or as before.
    fn low_priority(&mut self, low: bool) {
        if low == self.low {
            return;
        }
        self.low = low;
        let set = self.device.cast::<IDXGIDevice>().and_then(|d| unsafe { d.SetGPUThreadPriority(if low { -7 } else { 0 }) });
        match set {
            Ok(()) => crate::app_log(&format!("capture: graphics card priority {}", if low { "lowest (capture-low-priority)" } else { "normal again" })),
            Err(e) => crate::app_log(&format!("capture: graphics card priority not changed ({e})")),
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
    /// height / shrink (rounded down).
    pub fn read(&mut self, x: u32, y: u32, width: u32, height: u32, out: *mut u8, stride: usize, shrink: u32) -> Result<()> {
        let key = (x, y, width, height, shrink);
        if !self.queued.iter().any(|q| q.0 == key) {
            self.queue(x, y, width, height, shrink)?;
        }
        let Some(i) = self.queued.iter().position(|q| q.0 == key) else {
            // an empty area; or nothing to copy it from (callers ask ready() first, which says so)
            return if self.lost { Err(E_FAIL.into()) } else { Ok(()) };
        };
        let (_, staging, width, height, _) = self.queued.remove(i);
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
    pub fn ready(&mut self, x: u32, y: u32, width: u32, height: u32, shrink: u32) -> Result<bool> {
        let key = (x, y, width, height, shrink);
        if !self.queued.iter().any(|q| q.0 == key) {
            self.queue(x, y, width, height, shrink)?;
        }
        // an empty area; or the held image was given back and no newer one came: asked again next round
        let Some(q) = self.queued.iter().find(|q| q.0 == key) else { return Ok(!self.lost) };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        match unsafe { self.context.Map(&q.1, 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32, Some(&mut mapped)) } {
            Ok(()) => {
                unsafe { self.context.Unmap(&q.1, 0) };
                Ok(true)
            }
            Err(e) if e.code() == DXGI_ERROR_WAS_STILL_DRAWING => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// The round is over: copies read are gone; one still on its way stays for the next rounds (ready), up to
    /// KEEP_QUEUED, then it's dropped and started again.
    pub fn end_round(&mut self) {
        self.give_back();
        self.queued.retain(|q| q.4.elapsed() < KEEP_QUEUED);
    }

    /// Starts copying the area `x, y, width, height` (shrunk by `shrink`) off the card into a staging texture of its
    /// own, without waiting: every reader's area is queued first, then read() takes them one after another, so a
    /// round waits for the card once instead of once per reader (each wait was a small stall; B8).
    pub fn queue(&mut self, x: u32, y: u32, width: u32, height: u32, shrink: u32) -> Result<()> {
        let key = (x, y, width, height, shrink);
        if self.queued.iter().any(|q| q.0 == key) {
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
        let index = match self.staging.iter().position(|s| s.0 == width && s.1 == height && !self.queued.iter().any(|q| q.1 == s.2)) {
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
        self.queued.push((key, staging, width, height, std::time::Instant::now()));
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
        self.gpu.low_priority(crate::dev::on("capture-low-priority"));
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
        let (d3d, context) = device(None)?;
        let device: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&d3d.cast::<IDXGIDevice>()?)? }.cast()?;
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd)? };
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
        let session = pool.CreateCaptureSession(&item)?;
        if let Err(e) = session.SetIsBorderRequired(false) {
            if !border_ok {
                return Err(e);
            }
        }
        let _ = session.SetIsCursorCaptureEnabled(false);
        session.StartCapture()?;
        Ok(WindowCapture { gpu: Gpu::new(d3d, context), device, pool, session, size })
    }

    /// Takes the newest window image if there is one. Ok(false): nothing new since the last.
    pub fn update(&mut self) -> Result<bool> {
        // the pool hands frames out oldest first: take all that are waiting and keep the newest (one at a time,
        // a picture was up to two reader turns old while stamped as new)
        let mut newest = None;
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            if let Some(older) = newest.replace(frame) {
                let _ = older.Close();
            }
        }
        let Some(frame) = newest else { return Ok(false) };
        self.gpu.low_priority(crate::dev::on("capture-low-priority"));
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
        let result = (|| {
            let content = frame.ContentSize()?;
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
            self.gpu.store(&texture, content.Width.max(1) as u32, content.Height.max(1) as u32)?;
            if content.Width != self.size.Width || content.Height != self.size.Height {
                self.size = content; // the window was resized: buffers at the new size from now on
                self.pool.Recreate(&self.device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, content)?;
            }
            Ok(true)
        })();
        let _ = frame.Close();
        result
    }
}

impl Drop for WindowCapture {
    fn drop(&mut self) {
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
                if d.gpu.ready(x, 0, 600, 400, 1).expect("ready") {
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

    #[test]
    #[ignore]
    fn window_capture_available() {
        println!("borderless window capture supported: {}", WindowCapture::borderless_supported());
    }
}
