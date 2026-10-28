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
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIDevice, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
    DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Gdi::{MonitorFromWindow, HMONITOR, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// Staging textures kept between frames (one per area size the readers ask for), the least recently used going first.
const STAGING_KEPT: usize = 8;
/// ...and at most this much memory for each kind (a 4K frame is 33 MB: eight of those held 265 MB).
const STAGING_BYTES: usize = 128 << 20;

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
}

impl Gpu {
    fn new(device: ID3D11Device, context: ID3D11DeviceContext) -> Gpu {
        Gpu { device, context, cache: None, staging: Vec::new(), shrinkers: Vec::new(), generation: 0 }
    }

    /// Keeps the top-left `width` x `height` of `source` as the newest image (a copy on the card).
    fn store(&mut self, source: &ID3D11Texture2D, width: u32, height: u32) -> Result<()> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { source.GetDesc(&mut desc) };
        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(E_FAIL.into()); // an HDR desktop (16-bit colour): not readable as is
        }
        let (width, height) = (width.min(desc.Width), height.min(desc.Height));
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
        let index = match self.staging.iter().position(|s| s.0 == width && s.1 == height) {
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
        let staging = &self.staging[index].2;
        unsafe {
            if sub == 0 {
                self.context.CopySubresourceRegion(staging, 0, 0, 0, 0, &source, 0, Some(&area));
            } else {
                self.context.CopySubresourceRegion(staging, 0, 0, 0, 0, &source, sub, None);
            }
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let source = mapped.pData as *const u8;
            for row in 0..height as usize {
                std::ptr::copy_nonoverlapping(
                    source.add(row * mapped.RowPitch as usize),
                    out.add(row * stride),
                    width as usize * 4,
                );
            }
            self.context.Unmap(staging, 0);
        }
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
                    return Ok(Duplication { gpu: Gpu::new(device, context), duplication, desktop: desc.DesktopCoordinates, monitor });
                }
            }
        }
        Err(E_FAIL.into())
    }

    /// Takes the newest screen image if it changed. Ok(false): nothing new (or no image yet). An error means
    /// the duplication is lost (resolution or fullscreen change, a UAC prompt): make a new one.
    pub fn update(&mut self) -> Result<bool> {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        match unsafe { self.duplication.AcquireNextFrame(0, &mut info, &mut resource) } {
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(false),
            Err(e) => return Err(e),
            Ok(()) => {}
        }
        let result = (|| {
            // only the mouse pointer moved: the picture is the same
            if info.LastPresentTime == 0 && self.gpu.has_image() {
                return Ok(false);
            }
            let texture: ID3D11Texture2D = resource.ok_or(E_FAIL)?.cast()?;
            self.gpu.store(&texture, u32::MAX, u32::MAX)?; // the whole screen, at its real pixel size
            Ok(true)
        })();
        let _ = unsafe { self.duplication.ReleaseFrame() };
        result
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

    #[test]
    #[ignore]
    fn window_capture_available() {
        println!("borderless window capture supported: {}", WindowCapture::borderless_supported());
    }
}
