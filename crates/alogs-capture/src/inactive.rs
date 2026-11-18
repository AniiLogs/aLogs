//! Keeps the controller away from the game while aLogs has control of an end screen (built-in Windows, no admin, no
//! driver): the game is told it is inactive, with the messages Windows sends a window that stops being the active one,
//! while it really stays the window in front. The game reads no controller while it believes it isn't the active
//! window (17 Nov, on the running game: the controller did nothing for the 20 s it was told so, and worked again after
//! the active set). Nothing on the screen changes: no window of ours and no focus change, so no taskbar over a
//! borderless game and no change of capture method.
//!
//! Our own clicks and scrolls tell the game it is active for their moment (`lend` / `back`). It is told it is active
//! again at `stop`, or after `seconds` whatever happens.
//!
//! Hiding the controller (HidHide) can't do this: the driver only refuses a new open, and a running game has the
//! controller open already (17 Nov: 1228 input changes read through an open handle in 15 s of hiding). The focus guard
//! this replaces (B16 to B21) took the real focus with an invisible window: about 30 focus changes in a 12 s scan,
//! the taskbar showing at each.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};

/// After the game is told it is active: this long (s) before our input, so it takes it.
pub const LEND_SETTLE: f64 = 0.15;
/// After our input: this long (s) before the game is told it is inactive again, so the input is in.
pub const LEND_AFTER: f64 = 0.12;
/// How often (ms) the watch looks: at the time limit, and for a game that has come back to the front.
const WATCH_MS: u64 = 20;

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn sleep(seconds: f64) {
    std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
}

#[derive(Default)]
struct Inner {
    game: AtomicUsize,
    stop_at: AtomicU64,
    /// The game has been told it is inactive, and not yet told otherwise for good.
    on: AtomicBool,
    /// Counts the takes, so the watch of an earlier one knows it is over.
    round: AtomicU64,
    /// How many of our own inputs have the game told it is active right now. Its lock puts every tell in order.
    lent: Mutex<u32>,
    /// Tests: a window that can never be the one in front counts as in front.
    #[cfg(test)]
    as_if_front: AtomicBool,
}

#[derive(Clone, Default)]
pub struct Inactive(Arc<Inner>);

impl Inactive {
    /// Is the game told it is inactive now (our own inputs' moments included)?
    pub fn on(&self) -> bool {
        self.0.on.load(SeqCst)
    }

    /// Only a game that is really in front is told anything: one that isn't has it from Windows already, and Windows
    /// tells it when it is active again.
    fn in_front(&self) -> bool {
        #[cfg(test)]
        if self.0.as_if_front.load(SeqCst) {
            return true;
        }
        let game = self.0.game.load(SeqCst);
        game != 0 && crate::win::imp_foreground() == game
    }

    /// Tell `game` it is inactive for at most `seconds` (again from now when it's on). False when its window didn't
    /// take the message: nothing is changed then, and the controller still reaches the game.
    pub fn take(&self, game: usize, seconds: f64) -> bool {
        let i = &self.0;
        let mut lent = i.lent.lock().unwrap();
        i.stop_at.store(now_ms() + (seconds * 1000.0) as u64, SeqCst);
        if i.on.load(SeqCst) {
            return true;
        }
        i.game.store(game, SeqCst);
        *lent = 0;
        if self.in_front() && !imp::tell(game, false) {
            log::info!(target: "meter", "the game's window didn't take the message (Windows error {}): the controller still reaches the game", imp::last_error());
            i.stop_at.store(0, SeqCst);
            return false;
        }
        i.on.store(true, SeqCst);
        let round = i.round.fetch_add(1, SeqCst) + 1;
        drop(lent);
        let (a, b) = (self.clone(), self.clone());
        let mut l = crate::win::LEND.lock().unwrap();
        l.lend = Some(Arc::new(move || a.lend()));
        l.back = Some(Arc::new(move || b.back()));
        drop(l);
        let me = self.clone();
        std::thread::spawn(move || me.watch(round));
        log::info!(target: "meter", "the game is told it is inactive: it stays in front and reads no controller");
        true
    }

    /// The game is told it is active again, before this returns. Nothing when it's off.
    pub fn stop(&self) {
        self.end(None);
    }

    /// Until the time limit: a game that comes back to the front has just been told by Windows that it is active, so
    /// it is told otherwise again.
    fn watch(&self, round: u64) {
        let i = &self.0;
        let mut was_front = self.in_front();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(WATCH_MS));
            if i.round.load(SeqCst) != round || !i.on.load(SeqCst) {
                return;
            }
            if now_ms() >= i.stop_at.load(SeqCst) {
                self.end(Some(round));
                continue; // (given more time since that look: it goes on; ended: the next look returns)
            }
            let front = self.in_front();
            if front && !was_front {
                let lent = i.lent.lock().unwrap();
                if *lent == 0 && i.on.load(SeqCst) && i.round.load(SeqCst) == round {
                    imp::tell(i.game.load(SeqCst), false);
                }
            }
            was_front = front;
        }
    }

    /// `timed_out`: the watch of this round found the time up (checked again here, in order with a take).
    fn end(&self, timed_out: Option<u64>) {
        let i = &self.0;
        let mut lent = i.lent.lock().unwrap();
        if let Some(round) = timed_out
            && (i.round.load(SeqCst) != round || now_ms() < i.stop_at.load(SeqCst))
        {
            return;
        }
        if !i.on.swap(false, SeqCst) {
            return;
        }
        i.stop_at.store(0, SeqCst);
        *lent = 0;
        let mut l = crate::win::LEND.lock().unwrap();
        l.lend = None;
        l.back = None;
        drop(l);
        let told = self.in_front() && imp::tell(i.game.load(SeqCst), true);
        log::info!(target: "meter", "{}", if told { "the game is told it is active again" } else { "the game is no longer told it is inactive (it isn't in front: Windows tells it when it is)" });
    }

    /// The game is told it is active for one of our own inputs; `back` tells it otherwise again.
    pub fn lend(&self) {
        let i = &self.0;
        let mut lent = i.lent.lock().unwrap();
        if !i.on.load(SeqCst) {
            return;
        }
        *lent += 1;
        if *lent == 1 && self.in_front() && imp::tell(i.game.load(SeqCst), true) {
            sleep(LEND_SETTLE);
        }
    }

    pub fn back(&self) {
        let i = &self.0;
        let mut lent = i.lent.lock().unwrap();
        if *lent == 0 {
            return;
        }
        *lent -= 1;
        if *lent == 0 && i.on.load(SeqCst) && now_ms() < i.stop_at.load(SeqCst) {
            sleep(LEND_AFTER);
            if self.in_front() {
                imp::tell(i.game.load(SeqCst), false);
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{GetLastError, HWND};
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_ACTIVATE, WM_ACTIVATEAPP, WM_KILLFOCUS, WM_NCACTIVATE, WM_SETFOCUS};

    /// The messages Windows sends a window that stops being the active one (`active` false) or becomes it, in Windows'
    /// order: the set tried on the running game. Posted, so a busy game never holds us up. False when the window
    /// didn't take the first of them (it is gone, or runs as administrator while we don't).
    pub fn tell(game: usize, active: bool) -> bool {
        if game == 0 {
            return false; // (no window: a post to "no window" would land in our own thread's queue)
        }
        let on = usize::from(active);
        let set = [(WM_NCACTIVATE, on), (WM_ACTIVATE, on), (WM_ACTIVATEAPP, on), (if active { WM_SETFOCUS } else { WM_KILLFOCUS }, 0)];
        set.iter().all(|&(message, w)| unsafe { PostMessageW(game as HWND, message, w, 0) } != 0)
    }

    pub fn last_error() -> u32 {
        unsafe { GetLastError() }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn tell(_: usize, _: bool) -> bool {
        false
    }
    pub fn last_error() -> u32 {
        0
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    thread_local! {
        static GOT: RefCell<Vec<(u32, usize)>> = const { RefCell::new(Vec::new()) };
    }

    const INACTIVE: [(u32, usize); 4] = [(WM_NCACTIVATE, 0), (WM_ACTIVATE, 0), (WM_ACTIVATEAPP, 0), (WM_KILLFOCUS, 0)];
    const ACTIVE: [(u32, usize); 4] = [(WM_NCACTIVATE, 1), (WM_ACTIVATE, 1), (WM_ACTIVATEAPP, 1), (WM_SETFOCUS, 0)];

    unsafe extern "system" fn proc_(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if matches!(m, WM_NCACTIVATE | WM_ACTIVATE | WM_ACTIVATEAPP | WM_KILLFOCUS | WM_SETFOCUS) {
            GOT.with(|g| g.borrow_mut().push((m, w)));
            return 0;
        }
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    /// A message-only window of this thread, in the game's place: it notes what it is told.
    fn window() -> usize {
        let class: Vec<u16> = "aLogsInactiveTest".encode_utf16().chain([0]).collect();
        let h = unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW { style: 0, lpfnWndProc: Some(proc_), cbClsExtra: 0, cbWndExtra: 0, hInstance: module, hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(), hbrBackground: std::ptr::null_mut(), lpszMenuName: std::ptr::null(), lpszClassName: class.as_ptr() };
            RegisterClassW(&wc);
            CreateWindowExW(0, class.as_ptr(), class.as_ptr(), 0, 0, 0, 0, 0, HWND_MESSAGE, std::ptr::null_mut(), module, std::ptr::null())
        };
        assert!(!h.is_null());
        h as usize
    }

    /// What the window has been told since the last ask, in order.
    fn got() -> Vec<(u32, usize)> {
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                DispatchMessageW(&msg);
            }
        }
        GOT.with(|g| std::mem::take(&mut *g.borrow_mut()))
    }

    fn wait(ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    // One test: the hand-over to our own inputs is one for the whole program (win::LEND).
    #[test]
    fn the_game_is_told_inactive_for_the_hold_and_active_for_our_own_inputs() {
        let h = window();
        got();
        assert!(imp::tell(h, false));
        assert_eq!(got(), INACTIVE);
        assert!(imp::tell(h, true));
        assert_eq!(got(), ACTIVE);
        assert!(!imp::tell(0, false), "no window: nothing told");

        let g = Inactive::default();
        g.0.as_if_front.store(true, SeqCst);
        assert!(g.take(h, 30.0));
        assert!(g.on());
        assert_eq!(got(), INACTIVE);
        assert!(g.take(h, 30.0), "taken again: more time, nothing told twice");
        assert!(got().is_empty());

        // one of our own inputs: active for its moment, inactive after
        assert_eq!(crate::win::one_moment(got), ACTIVE);
        assert_eq!(got(), INACTIVE);
        // several in one moment: told once
        let inside = crate::win::one_moment(|| {
            crate::win::one_moment(|| ());
            got()
        });
        assert_eq!(inside, ACTIVE);
        assert_eq!(got(), INACTIVE);

        g.stop();
        assert!(!g.on());
        assert_eq!(got(), ACTIVE, "told active before stop returns");
        g.stop();
        crate::win::one_moment(|| ());
        assert!(got().is_empty(), "off: our inputs tell the game nothing");

        // the time limit ends it, whatever happens
        assert!(g.take(h, 0.05));
        assert_eq!(got(), INACTIVE);
        wait(300);
        assert_eq!(got(), ACTIVE);
        assert!(!g.on());

        // a game that isn't in front is told nothing; when it comes to the front it is told again
        g.0.as_if_front.store(false, SeqCst);
        assert!(g.take(h, 30.0));
        wait(100);
        assert!(got().is_empty());
        crate::win::one_moment(|| ());
        assert!(got().is_empty());
        g.0.as_if_front.store(true, SeqCst);
        wait(200);
        assert_eq!(got(), INACTIVE);
        g.0.as_if_front.store(false, SeqCst);
        g.stop();
        assert!(got().is_empty(), "not in front at the end: Windows tells it when it is");

        // a window that is gone: nothing changed
        g.0.as_if_front.store(true, SeqCst);
        unsafe { DestroyWindow(h as HWND) };
        assert!(!g.take(h, 30.0));
        assert!(!g.on());
    }
}
