//! Per-pixel alpha overlay windows (Windows layered windows, UpdateLayeredWindow): the whole window is one
//! premultiplied RGBA bitmap, so soft shadows blend into the game; fully transparent pixels let clicks through.
//! Port of aLogs B16 `bossbar/layered.py`, with the windows themselves (Tk made them there) and their mouse events.

use alogs_pil::{Image, Mode};
use std::cell::RefCell;

/// A mouse event on one of our windows: (window, kind, screen x, screen y).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mouse {
    Down,
    Move,
    Up,
    Double,
    Right,
}

thread_local! {
    static EVENTS: RefCell<Vec<(usize, Mouse, i32, i32)>> = const { RefCell::new(Vec::new()) };
}

/// The mouse events since the last call (the overlay's thread pumps messages in `pump`).
pub fn take_events() -> Vec<(usize, Mouse, i32, i32)> {
    EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

pub struct Layered {
    pub hwnd: usize,
}

impl Layered {
    /// A new top-most, tool (no taskbar) overlay window, parked off screen.
    pub fn new() -> Option<Layered> {
        imp::create().map(|hwnd| Layered { hwnd })
    }

    /// Show an RGBA image as the whole window at screen (x, y).
    pub fn show(&self, image: &Image, x: i32, y: i32) -> bool {
        let rgba = image.convert(Mode::Rgba);
        let mut bgra = Vec::with_capacity(rgba.data.len());
        for p in rgba.data.chunks_exact(4) {
            let a = u32::from(p[3]);
            // ImageChops.multiply(c, a): c * a / 255 rounded down
            let m = |c: u8| ((u32::from(c) * a) / 255) as u8;
            bgra.extend_from_slice(&[m(p[2]), m(p[1]), m(p[0]), p[3]]);
        }
        imp::show(self.hwnd, &bgra, rgba.width as i32, rgba.height as i32, x, y)
    }

    /// Fade the whole window (0-255) without sending its bitmap again.
    pub fn set_alpha(&self, alpha: i32) -> bool {
        imp::set_alpha(self.hwnd, alpha.clamp(0, 255) as u8)
    }

    /// Off screen, through Windows itself.
    pub fn park(&self) -> bool {
        imp::park(self.hwnd)
    }

    /// Clicks go to whatever is under the window (on), or the window takes them (off: to drag it).
    pub fn click_through(&self, on: bool) {
        imp::click_through(self.hwnd, on);
    }

    /// Out of (or back in) screen captures.
    pub fn hide_from_capture(&self, on: bool) -> bool {
        crate::win::hide_from_capture(self.hwnd, on)
    }
}

impl Drop for Layered {
    fn drop(&mut self) {
        imp::destroy(self.hwnd);
    }
}

/// Handle the windows' messages for a moment (the overlay's loop calls it every look).
pub fn pump() {
    imp::pump();
}

/// The window in front now.
pub fn foreground() -> usize {
    crate::win::imp_foreground()
}

/// The game in front again (a click on the alert): restored if minimized.
pub fn bring_back(hwnd: usize) {
    imp::bring_back(hwnd);
}

#[cfg(windows)]
mod imp {
    use super::{EVENTS, Mouse};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const CLASS: &str = "aLogsOverlay";

    unsafe extern "system" fn proc_(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        let kind = match m {
            WM_LBUTTONDOWN => Some(Mouse::Down),
            WM_MOUSEMOVE => Some(Mouse::Move),
            WM_LBUTTONUP => Some(Mouse::Up),
            WM_LBUTTONDBLCLK => Some(Mouse::Double),
            WM_RBUTTONUP => Some(Mouse::Right),
            _ => None,
        };
        if let Some(k) = kind {
            let mut p = POINT { x: 0, y: 0 };
            unsafe { GetCursorPos(&mut p) };
            EVENTS.with(|e| e.borrow_mut().push((h as usize, k, p.x, p.y)));
            if k == Mouse::Down {
                unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::SetCapture(h) };
            } else if k == Mouse::Up {
                unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture() };
            }
            return 0;
        }
        if m == WM_MOUSEACTIVATE {
            return MA_NOACTIVATE as LRESULT;
        }
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    pub fn create() -> Option<usize> {
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let class = wide(CLASS);
            let wc = WNDCLASSW { style: CS_DBLCLKS, lpfnWndProc: Some(proc_), cbClsExtra: 0, cbWndExtra: 0, hInstance: module, hIcon: std::ptr::null_mut(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW), hbrBackground: std::ptr::null_mut(), lpszMenuName: std::ptr::null(), lpszClassName: class.as_ptr() };
            RegisterClassW(&wc); // (already registered: fine)
            let title = wide("aLogs");
            let h = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, class.as_ptr(), title.as_ptr(), WS_POPUP,
                -32000, -32000, 1, 1, std::ptr::null_mut(), std::ptr::null_mut(), module, std::ptr::null());
            if h.is_null() {
                return None;
            }
            ShowWindow(h, SW_SHOWNOACTIVATE);
            Some(h as usize)
        }
    }

    pub fn show(hwnd: usize, bgra: &[u8], w: i32, h: i32, x: i32, y: i32) -> bool {
        if w <= 0 || h <= 0 {
            return false;
        }
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            let mem = CreateCompatibleDC(screen);
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32,
                biCompression: 0, biSizeImage: 0, biXPelsPerMeter: 0, biYPelsPerMeter: 0, biClrUsed: 0, biClrImportant: 0 };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            let mut ok = false;
            if !bmp.is_null() && !bits.is_null() {
                std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len().min((w * h * 4) as usize));
                let old = SelectObject(mem, bmp);
                let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: 1 };
                let pt = POINT { x, y };
                let sz = SIZE { cx: w, cy: h };
                let src = POINT { x: 0, y: 0 };
                ok = UpdateLayeredWindow(hwnd as HWND, screen, &pt, &sz, mem, &src, 0, &blend, ULW_ALPHA) != 0;
                SelectObject(mem, old);
                DeleteObject(bmp);
            }
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
            ok
        }
    }

    pub fn set_alpha(hwnd: usize, a: u8) -> bool {
        let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: a, AlphaFormat: 1 };
        unsafe { UpdateLayeredWindow(hwnd as HWND, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null(), 0, &blend, ULW_ALPHA) != 0 }
    }

    pub fn park(hwnd: usize) -> bool {
        unsafe { SetWindowPos(hwnd as HWND, std::ptr::null_mut(), -32000, -32000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) != 0 }
    }

    pub fn click_through(hwnd: usize, on: bool) {
        unsafe {
            let style = GetWindowLongW(hwnd as HWND, GWL_EXSTYLE) as u32;
            let style = if on { style | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE } else { style & !WS_EX_TRANSPARENT };
            SetWindowLongW(hwnd as HWND, GWL_EXSTYLE, style as i32);
        }
    }

    pub fn destroy(hwnd: usize) {
        unsafe { DestroyWindow(hwnd as HWND) };
    }

    pub fn pump() {
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    pub fn bring_back(hwnd: usize) {
        unsafe {
            if IsIconic(hwnd as HWND) != 0 {
                ShowWindow(hwnd as HWND, SW_RESTORE);
            }
            SetForegroundWindow(hwnd as HWND);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn create() -> Option<usize> {
        None
    }
    pub fn show(_: usize, _: &[u8], _: i32, _: i32, _: i32, _: i32) -> bool {
        false
    }
    pub fn set_alpha(_: usize, _: u8) -> bool {
        false
    }
    pub fn park(_: usize) -> bool {
        false
    }
    pub fn click_through(_: usize, _: bool) {}
    pub fn destroy(_: usize) {}
    pub fn pump() {}
    pub fn bring_back(_: usize) {}
}
