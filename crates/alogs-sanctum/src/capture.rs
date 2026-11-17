//! The game's window and copies of its pixels without a capture border: the aLogs app's shared picture, the window's
//! own image (PrintWindow: windows on top, our overlay, are never in it), the screen (BitBlt), and the screen without
//! some of our own windows (the Magnification API's filter list). Port of `Capture.cs`.

use crate::cv::Mat;
use crate::shared_capture;

pub type Rect = (i32, i32, i32, i32);

fn intersect(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.0.max(b.0), a.1.max(b.1));
    let (x1, y1) = ((a.0 + a.2).min(b.0 + b.2), (a.1 + a.3).min(b.1 + b.3));
    if x1 <= x0 || y1 <= y0 { (0, 0, 0, 0) } else { (x0, y0, x1 - x0, y1 - y0) }
}

/// The game's window: its handle and client area in physical screen pixels.
#[derive(Default)]
pub struct GameWindow {
    pub handle: usize,
    pub client: Rect,
}

#[cfg(windows)]
mod native {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    pub fn h(x: usize) -> HWND {
        x as HWND
    }

    pub fn text(hwnd: usize) -> String {
        let mut buf = [0u16; 512];
        let n = unsafe { GetWindowTextW(h(hwnd), buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    pub fn class(hwnd: usize) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetClassNameW(h(hwnd), buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    pub fn is_window(hwnd: usize) -> bool {
        unsafe { IsWindow(h(hwnd)) != 0 }
    }

    pub fn is_iconic(hwnd: usize) -> bool {
        unsafe { IsIconic(h(hwnd)) != 0 }
    }

    pub fn foreground() -> usize {
        unsafe { GetForegroundWindow() as usize }
    }

    pub fn client(hwnd: usize) -> Option<(i32, i32, i32, i32)> {
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if unsafe { GetClientRect(h(hwnd), &mut r) } == 0 {
            return None;
        }
        let mut p = POINT { x: 0, y: 0 };
        unsafe { ClientToScreen(h(hwnd), &mut p) };
        Some((p.x, p.y, r.right - r.left, r.bottom - r.top))
    }

    struct Search {
        title: String,
        found: usize,
    }

    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> i32 {
        let s = unsafe { &mut *(lp as *mut Search) };
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }
        if !text(hwnd as usize).to_lowercase().contains(&s.title) {
            return 1;
        }
        // the game is a Unity player; this keeps browser tabs titled "Aniimo ..." out
        if class(hwnd as usize) != "UnityWndClass" {
            return 1;
        }
        s.found = hwnd as usize;
        0
    }

    pub fn find(title: &str) -> usize {
        let mut s = Search { title: title.to_lowercase(), found: 0 };
        unsafe { EnumWindows(Some(each), &mut s as *mut Search as LPARAM) };
        s.found
    }
}

#[cfg(windows)]
impl GameWindow {
    pub fn is_valid(&self) -> bool {
        self.handle != 0 && native::is_window(self.handle) && !native::is_iconic(self.handle)
    }

    /// In front (not while minimized: nothing should be read or shown then).
    pub fn is_foreground(&self) -> bool {
        self.handle != 0 && native::foreground() == self.handle && !native::is_iconic(self.handle)
    }

    pub fn is_minimized(&self) -> bool {
        self.handle != 0 && native::is_iconic(self.handle) && native::is_window(self.handle)
    }

    /// Finds the game again if needed and reads its client area; false when there's no usable window. Minimized, it
    /// is still the game, kept at the size it had.
    pub fn refresh(&mut self, title: &str) -> bool {
        let t = title.to_lowercase();
        if self.is_minimized() && native::text(self.handle).to_lowercase().contains(&t) {
            return self.client.2 > 100 && self.client.3 > 100;
        }
        if !self.is_valid() || !native::text(self.handle).to_lowercase().contains(&t) {
            self.handle = native::find(title);
        }
        if self.handle == 0 {
            return false;
        }
        let Some(c) = native::client(self.handle) else { return false };
        self.client = c;
        c.2 > 100 && c.3 > 100
    }
}

#[cfg(not(windows))]
impl GameWindow {
    pub fn is_valid(&self) -> bool {
        false
    }
    pub fn is_foreground(&self) -> bool {
        false
    }
    pub fn is_minimized(&self) -> bool {
        false
    }
    pub fn refresh(&mut self, _: &str) -> bool {
        false
    }
}

/// A top-down 32-bit DIB section in a memory DC.
#[cfg(windows)]
struct Dib {
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    bmp: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    old: windows_sys::Win32::Graphics::Gdi::HGDIOBJ,
    bits: *mut u8,
    w: usize,
    h: usize,
}

#[cfg(windows)]
impl Dib {
    fn new(w: usize, h: usize) -> Option<Dib> {
        use windows_sys::Win32::Graphics::Gdi::*;
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
        };
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            let mut bits = std::ptr::null_mut();
            let bmp = CreateDIBSection(dc, &bi, 0, &mut bits, std::ptr::null_mut(), 0);
            if bmp.is_null() || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            let old = SelectObject(dc, bmp);
            Some(Dib { dc, bmp, old, bits: bits as *mut u8, w, h })
        }
    }

    /// Part (x, y, w, h) as BGR.
    fn bgr(&self, r: Rect) -> Mat {
        let (w, h) = (r.2 as usize, r.3 as usize);
        let mut out = Mat::new(w, h, 3);
        let px = unsafe { std::slice::from_raw_parts(self.bits, self.w * self.h * 4) };
        for y in 0..h {
            let row = &px[((r.1 as usize + y) * self.w + r.0 as usize) * 4..][..w * 4];
            for (d, p) in out.data[y * w * 3..(y + 1) * w * 3].chunks_exact_mut(3).zip(row.chunks_exact(4)) {
                d.copy_from_slice(&p[..3]);
            }
        }
        out
    }
}

#[cfg(windows)]
impl Drop for Dib {
    fn drop(&mut self) {
        use windows_sys::Win32::Graphics::Gdi::*;
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bmp);
            DeleteDC(self.dc);
        }
    }
}

/// The part `sub` (client px) of the window's client area: the app's shared picture when it's running, else
/// PrintWindow. None when the window gave no image (then fall back to the screen).
pub fn grab_window(hwnd: usize, cw: i32, ch: i32, sub: Rect) -> Option<Mat> {
    if let Some(m) = shared_capture::try_grab(hwnd, cw, ch, sub, false, false) {
        return Some(m);
    }
    let sub = intersect(sub, (0, 0, cw, ch));
    if sub.2 <= 0 || sub.3 <= 0 {
        return None;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Gdi::GdiFlush;
        use windows_sys::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
        const PW_CLIENTONLY: PRINT_WINDOW_FLAGS = 1;
        const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = 2;
        let dib = Dib::new(cw as usize, ch as usize)?;
        if unsafe { PrintWindow(hwnd as _, dib.dc, PW_CLIENTONLY | PW_RENDERFULLCONTENT) } == 0 {
            return None;
        }
        unsafe { GdiFlush() };
        let m = dib.bgr(sub);
        // a window that can't be printed comes back black
        if m.data.iter().all(|&v| v == 0) {
            return None;
        }
        Some(m)
    }
    #[cfg(not(windows))]
    None
}

/// A copy of part of the screen (screen px).
pub fn grab_screen(r: Rect) -> Mat {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Gdi::*;
        let Some(dib) = Dib::new(r.2.max(1) as usize, r.3.max(1) as usize) else { return Mat::new(r.2.max(0) as usize, r.3.max(0) as usize, 3) };
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            BitBlt(dib.dc, 0, 0, r.2, r.3, screen, r.0, r.1, SRCCOPY);
            ReleaseDC(std::ptr::null_mut(), screen);
            GdiFlush();
        }
        dib.bgr((0, 0, r.2, r.3))
    }
    #[cfg(not(windows))]
    Mat::new(r.2.max(0) as usize, r.3.max(0) as usize, 3)
}

/// A picture of part of the screen without some of our own windows in it, through a hidden magnifier per thread.
pub mod magnifier {
    use super::Rect;
    use crate::cv::Mat;
    use std::sync::atomic::{AtomicBool, Ordering};

    static FAILED: AtomicBool = AtomicBool::new(false);

    /// True once the magnifier gave no picture here: the map then stays out of captures instead.
    pub fn failed() -> bool {
        FAILED.load(Ordering::Relaxed)
    }

    fn fail(why: &str) {
        if !FAILED.swap(true, Ordering::Relaxed) {
            crate::log::write(&format!("magnifier {why}: maps over the game stay out of screen captures where the picture is a copy of the screen"));
        }
    }

    #[cfg(windows)]
    mod imp {
        use super::*;
        use std::cell::RefCell;
        use std::sync::Mutex;
        use windows_sys::Win32::Foundation::{HWND, RECT};
        use windows_sys::Win32::Graphics::Gdi::{HRGN, InvalidateRect, UpdateWindow};
        use windows_sys::Win32::UI::Magnification::*;
        use windows_sys::Win32::UI::WindowsAndMessaging::*;

        static INIT: Mutex<bool> = Mutex::new(false);

        struct Instance {
            host: HWND,
            mag: HWND,
            size: (u32, u32),
        }

        thread_local! {
            static MINE: RefCell<Option<Instance>> = const { RefCell::new(None) };
            static GOT: RefCell<(Option<Mat>, (u32, u32))> = const { RefCell::new((None, (0, 0))) };
        }

        unsafe extern "system" fn on_image(_: HWND, src: *mut core::ffi::c_void, sh: MAGIMAGEHEADER, _: *mut core::ffi::c_void, _: MAGIMAGEHEADER, _: RECT, _: RECT, _: HRGN) -> i32 {
            GOT.with(|g| {
                let mut g = g.borrow_mut();
                if (sh.width, sh.height) != g.1 {
                    return;
                }
                let (w, h, stride) = (sh.width as usize, sh.height as usize, sh.stride as usize);
                let mut out = Mat::new(w, h, 3);
                let base = unsafe { (src as *const u8).add(sh.offset as usize) };
                for y in 0..h {
                    let row = unsafe { std::slice::from_raw_parts(base.add(y * stride), w * 4) };
                    for (d, p) in out.data[y * w * 3..(y + 1) * w * 3].chunks_exact_mut(3).zip(row.chunks_exact(4)) {
                        d.copy_from_slice(&p[..3]);
                    }
                }
                g.0 = Some(out);
            });
            1
        }

        fn wide(s: &str) -> Vec<u16> {
            s.encode_utf16().chain(std::iter::once(0)).collect()
        }

        fn create() -> Option<Instance> {
            unsafe {
                let (stat, title, magcls, empty) = (wide("Static"), wide("SanctumHelper magnifier"), wide("Magnifier"), wide(""));
                let host = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, stat.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, 1, 1,
                    std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
                if host.is_null() {
                    return None;
                }
                SetLayeredWindowAttributes(host, 0, 255, LWA_ALPHA);
                let mag = CreateWindowExW(0, magcls.as_ptr(), empty.as_ptr(), WS_CHILD | WS_VISIBLE, 0, 0, 1, 1, host, std::ptr::null_mut(),
                    std::ptr::null_mut(), std::ptr::null());
                if mag.is_null() || MagSetImageScalingCallback(mag, Some(on_image)) == 0 {
                    return None;
                }
                Some(Instance { host, mag, size: (0, 0) })
            }
        }

        pub fn grab(r: Rect, without: &[usize]) -> Result<Option<Mat>, &'static str> {
            {
                let mut i = INIT.lock().unwrap();
                if !*i {
                    if unsafe { MagInitialize() } == 0 {
                        return Err("not available");
                    }
                    *i = true;
                }
            }
            MINE.with(|m| {
                let mut m = m.borrow_mut();
                if m.is_none() {
                    *m = Some(create().ok_or("no magnifier window")?);
                }
                let inst = m.as_mut().unwrap();
                let size = (r.2 as u32, r.3 as u32);
                GOT.with(|g| *g.borrow_mut() = (None, size));
                unsafe {
                    if inst.size != size {
                        inst.size = size;
                        SetWindowPos(inst.host, std::ptr::null_mut(), 0, 0, r.2, r.3, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
                        SetWindowPos(inst.mag, std::ptr::null_mut(), 0, 0, r.2, r.3, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                    let mut list: Vec<HWND> = without.iter().filter(|&&h| h != 0).map(|&h| h as HWND).collect();
                    MagSetWindowFilterList(inst.mag, MW_FILTERMODE_EXCLUDE, list.len() as i32, list.as_mut_ptr());
                    if MagSetWindowSource(inst.mag, RECT { left: r.0, top: r.1, right: r.0 + r.2, bottom: r.1 + r.3 }) == 0 {
                        return Ok(None);
                    }
                    InvalidateRect(inst.mag, std::ptr::null(), 1);
                    UpdateWindow(inst.mag);
                    // this thread has no message loop: drop what the hidden windows were sent meanwhile
                    let mut msg: MSG = std::mem::zeroed();
                    while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                Ok(GOT.with(|g| g.borrow_mut().0.take()))
            })
        }
    }

    pub fn grab(r: Rect, without: &[usize]) -> Option<Mat> {
        if failed() || r.2 <= 0 || r.3 <= 0 {
            return None;
        }
        #[cfg(windows)]
        {
            match imp::grab(r, without) {
                Ok(Some(m)) => Some(m),
                Ok(None) => {
                    fail("gave no picture");
                    None
                }
                Err(e) => {
                    fail(e);
                    None
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = without;
            fail("not available");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_copy_and_window_search() {
        let m = grab_screen((0, 0, 16, 12));
        assert_eq!((m.w, m.h, m.c), (16, 12, 3));
        let mut g = GameWindow::default();
        assert!(!g.refresh("no window is called this 7f3a"));
        assert_eq!(g.handle, 0);
        assert_eq!(intersect((0, 0, 10, 10), (-5, -5, 10, 10)), (0, 0, 5, 5));
    }
}
