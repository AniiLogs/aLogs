//! Looking for the game window never asks one of this program's own windows for its title. A title is asked of the
//! window's own thread when the window is this program's, and waits for it: the meter's 8 overlay windows answered once
//! a loop of theirs, and the search, made every 2 s, took 0.34 to 0.48 s each time (17 Nov).

#![cfg(windows)]

use std::ptr::null_mut;
use std::time::Duration;
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE};

#[test]
fn the_search_never_waits_on_our_own_windows() {
    // a visible window of ours, titled, whose thread answers nothing (as a busy overlay thread doesn't)
    let (made, is_made) = std::sync::mpsc::channel();
    let (release, hold) = std::sync::mpsc::channel::<()>();
    let owner = std::thread::spawn(move || unsafe {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let title: Vec<u16> = "aLogs\0".encode_utf16().collect();
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, class.as_ptr(), title.as_ptr(), WS_POPUP | WS_VISIBLE, -32000, -32000, 1, 1, null_mut(), null_mut(), null_mut(), null_mut());
        made.send(!hwnd.is_null()).unwrap();
        let _ = hold.recv();
        DestroyWindow(hwnd);
    });
    assert!(is_made.recv().unwrap(), "the test's window");
    let (done, is_done) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // (a title no window has: the search goes through every window)
        let found = alogs_capture::win::find_window("no game is called this");
        let _ = done.send(found);
    });
    let came_back = is_done.recv_timeout(Duration::from_secs(5));
    release.send(()).unwrap();
    owner.join().unwrap();
    assert_eq!(came_back, Ok(None), "the search waited on a window of our own that wasn't answering");
}
