//! Says in app.log when the window stops answering ("Not Responding"), and where it was stuck.
//!
//! A thread asks the window's thread once a second to note the time. When an ask goes unanswered for 5 s, app.log gets
//! what that thread was doing: the commands the page last called (their names only) and the thread's call stack, as
//! `module+offset` with the function's name where Windows knows it (system files' exported functions; aLogs' own are
//! looked up from the build's map file, kept beside each test installer). Again at 30 s if it is still stuck, and a
//! line when it answers again. A panic anywhere in the app gets a line too.
//!
//! Nothing of the player's is in it: no arguments, no memory, only code addresses and names. app.log goes out only with
//! a report the player sends.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::AppHandle;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Diagnostics::Debug::{GetThreadContext, RtlLookupFunctionEntry, RtlVirtualUnwind, CONTEXT};
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW};
use windows_sys::Win32::System::Threading::{
    GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT, THREAD_QUERY_INFORMATION, THREAD_SUSPEND_RESUME,
};

const STUCK_MS: u64 = 5_000;
const STILL_STUCK_MS: u64 = 30_000;
const KEPT: usize = 12;
const FRAMES: usize = 48;

/// When the window's thread last answered (unix ms).
static ANSWERED: AtomicU64 = AtomicU64::new(0);
static WINDOW_THREAD: AtomicU32 = AtomicU32::new(0);
/// The page's last calls: when (unix ms) and which command.
static RECENT: Mutex<VecDeque<(u64, String)>> = Mutex::new(VecDeque::new());

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// A command the page called just now (its name only). Never waits: a call is skipped if the list is being read.
pub fn command(name: &str) {
    if let Ok(mut recent) = RECENT.try_lock() {
        if recent.len() == KEPT {
            recent.pop_front();
        }
        recent.push_back((now_ms(), name.to_string()));
    }
}

/// The recent commands, newest last: "collect_logs 0.4 s ago, build_number 0.1 s ago".
fn recent_commands(now: u64) -> String {
    let recent = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    if recent.is_empty() {
        return "none".into();
    }
    recent.iter().map(|(at, name)| format!("{name} {:.1} s ago", now.saturating_sub(*at) as f64 / 1000.0)).collect::<Vec<_>>().join(", ")
}

/// What an unanswered ask means now: None, or the line to write (the first time past each mark).
/// `said`: 0 nothing yet, 1 the 5 s line, 2 the 30 s line.
fn due(waited: u64, said: u8) -> Option<u8> {
    match said {
        0 if waited >= STUCK_MS => Some(1),
        1 if waited >= STILL_STUCK_MS => Some(2),
        _ => None,
    }
}

/// Call once from the window's own thread (setup).
pub fn start(app: AppHandle) {
    WINDOW_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    ANSWERED.store(now_ms(), Ordering::Relaxed);
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        crate::app_log(&format!("panic on thread {}: {info}", thread.name().unwrap_or("unnamed")));
        before(info);
    }));
    let _ = std::thread::Builder::new().name("watchdog".into()).spawn(move || {
        let (mut asked, mut said, mut last_tick) = (0u64, 0u8, now_ms());
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = now_ms();
            // the PC slept, or this thread itself was held up: that time isn't the window's
            if now.saturating_sub(last_tick) > 3_000 {
                asked += now - last_tick - 1_000;
            }
            last_tick = now;
            if ANSWERED.load(Ordering::Relaxed) >= asked {
                if said > 0 {
                    crate::app_log(&format!("window: answering again after {:.0} s", now.saturating_sub(asked) as f64 / 1000.0));
                    said = 0;
                }
                // asks queue up behind whatever holds the thread, so the next one waits for this one's answer
                asked = now;
                let _ = app.run_on_main_thread(|| ANSWERED.store(now_ms(), Ordering::Relaxed));
                continue;
            }
            let waited = now.saturating_sub(asked);
            if let Some(next) = due(waited, said) {
                said = next;
                let stack = window_stack();
                crate::app_log(&format!(
                    "window: {} for {:.0} s. The page's last calls: {}. The window's thread is at: {}",
                    if next == 1 { "not answering" } else { "still not answering" },
                    waited as f64 / 1000.0,
                    recent_commands(now),
                    if stack.is_empty() { "unknown".into() } else { stack.iter().map(|&a| describe(a)).collect::<Vec<_>>().join(" < ") },
                ));
            }
        }
    });
}

/// The return addresses on the window thread's stack, innermost first. The thread is stopped for the walk, and nothing
/// between stopping it and letting it go may allocate or take a lock: it may be holding the heap's.
fn window_stack() -> Vec<u64> {
    let mut frames = [0u64; FRAMES];
    let mut count = 0;
    unsafe {
        let thread = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, 0, WINDOW_THREAD.load(Ordering::Relaxed));
        if thread.is_null() {
            return Vec::new();
        }
        let mut context: CONTEXT = std::mem::zeroed();
        context.ContextFlags = 0x0010_000B; // CONTEXT_AMD64 | CONTROL | INTEGER | FLOATING_POINT
        if SuspendThread(thread) != u32::MAX {
            if GetThreadContext(thread, &mut context) != 0 {
                while count < FRAMES && context.Rip != 0 {
                    frames[count] = context.Rip;
                    count += 1;
                    let mut base = 0u64;
                    let function = RtlLookupFunctionEntry(context.Rip, &mut base, std::ptr::null_mut());
                    if function.is_null() {
                        // a leaf function: its return address is on top of the stack
                        if context.Rsp == 0 {
                            break;
                        }
                        context.Rip = *(context.Rsp as *const u64);
                        context.Rsp += 8;
                    } else {
                        let (mut data, mut frame) = (std::ptr::null_mut(), 0u64);
                        RtlVirtualUnwind(0, base, context.Rip, function, &mut context, &mut data, &mut frame, std::ptr::null_mut());
                    }
                }
            }
            ResumeThread(thread);
        }
        CloseHandle(thread);
    }
    frames[..count].to_vec()
}

/// "user32.dll+0x2a4f1 (SendMessageW)": the file the address is in (its name, never its folder), how far in, and the
/// function's name where Windows has one.
fn describe(address: u64) -> String {
    let mut module = std::ptr::null_mut();
    // FROM_ADDRESS | UNCHANGED_REFCOUNT
    let found = unsafe { GetModuleHandleExW(0x4 | 0x2, address as *const u16, &mut module) } != 0 && !module.is_null();
    if !found {
        return format!("{address:#x}");
    }
    let mut path = [0u16; 520];
    let length = unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) } as usize;
    let path = String::from_utf16_lossy(&path[..length.min(path.len())]);
    let file = path.rsplit(['\\', '/']).next().unwrap_or("?");
    let mut name = None;
    backtrace::resolve(address as *mut std::ffi::c_void, |symbol| {
        if name.is_none() {
            name = symbol.name().map(|n| n.to_string());
        }
    });
    let offset = address - module as u64;
    match name {
        Some(name) => format!("{file}+{offset:#x} ({name})"),
        None => format!("{file}+{offset:#x}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_at_5_and_30_seconds_once_each() {
        assert_eq!(due(4_999, 0), None);
        assert_eq!(due(5_000, 0), Some(1));
        assert_eq!(due(12_000, 1), None, "said once: quiet until 30 s");
        assert_eq!(due(30_000, 1), Some(2));
        assert_eq!(due(300_000, 2), None, "and then no more");
    }

    #[test]
    fn recent_commands_keep_the_newest() {
        RECENT.lock().unwrap().clear();
        assert_eq!(recent_commands(now_ms()), "none");
        for i in 0..KEPT + 3 {
            command(&format!("call_{i}"));
        }
        let text = recent_commands(now_ms());
        assert!(!text.contains("call_2 "), "the oldest went: {text}");
        assert!(text.starts_with("call_3 ") && text.contains(&format!("call_{} ", KEPT + 2)), "{text}");
    }

    /// A thread held in a wait is walked from outside, and the walk names where it waits.
    #[test]
    fn a_waiting_thread_is_walked() {
        let (tell, told) = std::sync::mpsc::channel();
        let (release, hold) = std::sync::mpsc::channel::<()>();
        let held = std::thread::spawn(move || {
            tell.send(unsafe { GetCurrentThreadId() }).unwrap();
            let _ = hold.recv();
        });
        WINDOW_THREAD.store(told.recv().unwrap(), Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(200));
        let stack = window_stack();
        let text = stack.iter().map(|&a| describe(a)).collect::<Vec<_>>().join(" < ");
        release.send(()).unwrap();
        held.join().unwrap();
        println!("{text}");
        assert!(stack.len() >= 3, "{text}");
        assert!(text.starts_with("ntdll.dll+"), "it waits in Windows: {text}");
        assert!(!text.contains(":\\"), "no folders: {text}");
    }
}
