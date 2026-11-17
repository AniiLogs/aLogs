//! The reader against a stand-in for the app's side of `Local\aLogs.Capture.v2`: a frame written the way capture.rs
//! writes it is read back whole, as its top part, and as separate parts.
#![cfg(windows)]

use alogs_capture::SharedCapture;
use alogs_capture::shared::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Memory::{CreateFileMappingW, FILE_MAP_ALL_ACCESS, MapViewOfFile, PAGE_READWRITE};

const W: usize = 64;
const H: usize = 36;

struct Map(*mut u8);
unsafe impl Send for Map {}
unsafe impl Sync for Map {}
impl Map {
    fn u16(&self, at: usize) -> u16 { unsafe { std::ptr::read_volatile(self.0.add(at) as *const u16) } }
    fn u32(&self, at: usize) -> u32 { unsafe { std::ptr::read_volatile(self.0.add(at) as *const u32) } }
    fn u64(&self, at: usize) -> u64 { unsafe { std::ptr::read_volatile(self.0.add(at) as *const u64) } }
    fn w16(&self, at: usize, v: u16) { unsafe { std::ptr::write_volatile(self.0.add(at) as *mut u16, v) } }
    fn w32(&self, at: usize, v: u32) { unsafe { std::ptr::write_volatile(self.0.add(at) as *mut u32, v) } }
    fn w64(&self, at: usize, v: u64) { unsafe { std::ptr::write_volatile(self.0.add(at) as *mut u64, v) } }
    fn w8(&self, at: usize, v: u8) { unsafe { std::ptr::write_volatile(self.0.add(at), v) } }
}

fn px(x: usize, y: usize) -> [u8; 3] {
    [(x * 4) as u8, (y * 7) as u8, ((x + y) * 3) as u8]
}

#[test]
fn reads_what_the_app_serves() {
    let name: Vec<u16> = NAME.encode_utf16().chain(std::iter::once(0)).collect();
    let size = HEADER + 2 * W * H * 4 + 4096;
    let map = unsafe {
        let h = CreateFileMappingW(INVALID_HANDLE_VALUE, std::ptr::null(), PAGE_READWRITE, 0, size as u32, name.as_ptr());
        assert!(!h.is_null());
        // a running aLogs app owns this mapping: never write a made-up picture into it
        if windows_sys::Win32::Foundation::GetLastError() == windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS {
            windows_sys::Win32::Foundation::CloseHandle(h);
            eprintln!("skipped: the aLogs app is running on this PC (its shared capture is live)");
            return;
        }
        Arc::new(Map(MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, 0).Value as *mut u8))
    };
    let hwnd = 0x1234usize;
    map.w32(0, MAGIC);
    map.w32(4, 2);
    map.w64(40, hwnd as u64);
    map.w32(16, W as u32);
    map.w32(20, H as u32);
    let stop = Arc::new(AtomicBool::new(false));
    let (m2, s2) = (map.clone(), stop.clone());
    let app = std::thread::spawn(move || {
        let now_ms = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        while !s2.load(Ordering::Relaxed) {
            m2.w64(HEARTBEAT, now_ms());
            for i in 0..SLOTS {
                if m2.u64(SLOT_BASE + i * 16) < now_ms() { continue; }
                let at = PICTURE_BASE + i * PICTURE;
                let req = m2.u32(at + 8);
                if req == 0 || m2.u32(at + 12) == req { continue; }
                let ra = RECT_BASE + i * RECT_SIZE;
                let parts = m2.u32(ra) as usize;
                let seq = m2.u64(at + 16);
                m2.w64(at + 16, seq + 1);
                let off = i * W * H * 4 / 8;
                let mut write_part = |x0: usize, y0: usize, w: usize, h: usize, mut pos: usize| {
                    for y in 0..h { for x in 0..w {
                        let p = px(x0 + x, y0 + y);
                        for (k, v) in [p[2], p[1], p[0], 255].into_iter().enumerate() { m2.w8(HEADER + pos + k, v); }
                        pos += 4;
                    } }
                    pos
                };
                if parts == 0 {
                    let y1 = (H * m2.u16(at + 6) as usize).div_ceil(65535).min(H);
                    write_part(0, 0, W, y1, off);
                    for (k, v) in [0u16, 0, W as u16, y1 as u16].into_iter().enumerate() { m2.w16(at + 40 + 2 * k, v); }
                    m2.w32(at + 36, (W * 4) as u32);
                } else {
                    let mut pos = off;
                    for k in 0..parts {
                        let s = |j: usize| m2.u16(ra + 8 + 16 * k + 2 * j) as usize;
                        let (x0, y0) = (W * s(0) / 65535, H * s(1) / 65535);
                        let (x1, y1) = ((W * s(2)).div_ceil(65535).min(W), (H * s(3)).div_ceil(65535).min(H));
                        for (j, v) in [x0, y0, x1 - x0, y1 - y0].into_iter().enumerate() { m2.w16(ra + 8 + 16 * k + 8 + 2 * j, v as u16); }
                        pos = write_part(x0, y0, x1 - x0, y1 - y0, pos);
                    }
                    m2.w32(at + 36, 0);
                }
                m2.w32(at + 32, off as u32);
                m2.w16(at + 56, W as u16);
                m2.w16(at + 58, H as u16);
                m2.w32(at + 48, 1 | (4 << 4));
                m2.w64(at + 24, now_ms());
                m2.w32(at + 12, req);
                m2.w64(at + 16, seq + 2);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });
    let mut r = SharedCapture::new(0);
    let mut got = None;
    for _ in 0..20 {
        got = r.read(hwnd, None);
        if got.is_some() { break; }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let im = got.unwrap_or_else(|| panic!("no picture: {}", r.why));
    assert_eq!(im.size(), (W, H));
    assert_eq!(im.px(10, 20), &px(10, 20)[..]);
    assert_eq!(r.source, "window capture");
    let mut top = None;
    for _ in 0..20 {
        top = r.read(hwnd, Some(0.5));
        if top.is_some() { break; }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let top = top.unwrap_or_else(|| panic!("no top part: {}", r.why));
    assert_eq!(top.size(), (W, H / 2));
    let mut parts = None;
    for _ in 0..20 {
        parts = r.read_rects(hwnd, &[(0.0, 0.0, 0.25, 0.25), (0.5, 0.5, 1.0, 1.0)]);
        if parts.is_some() { break; }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let parts = parts.unwrap_or_else(|| panic!("no parts: {}", r.why));
    assert_eq!(parts.px(5, 5), &px(5, 5)[..], "inside the first part");
    assert_eq!(parts.px(60, 30), &px(60, 30)[..], "inside the second part");
    assert_eq!(parts.px(40, 5), &[0, 0, 0][..], "between the parts: black");
    r.publish_overlays(OWNER_METER, &[(1, 2, 3, 4)]);
    assert_eq!(r.overlays(None), vec![(1, 2, 3, 4)]);
    assert!(r.overlays(Some(OWNER_METER)).is_empty());
    assert!(r.app_present());
    stop.store(true, Ordering::Relaxed);
    app.join().unwrap();
}
