//! What the PC is doing, for the meter's once-a-minute "load:" line and a one-time "PC:" line, so a lag report shows
//! where the time went. Windows' own counters and a few kernel calls only (nothing installed, nothing sent). Never
//! other programs' names: only the game, aLogs' own programs, everything else as one share, and the totals.
//! Port of aLogs B16 `bossbar/pcstats.py` and `core.CpuLoad`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const WEBVIEW: &str = "msedgewebview2.exe";
pub const LEFTOVER: [&str; 2] = ["alogs-updating.exe", "alogs-installing.exe"];

/// aLogs' processes by part: meter, app, windows, sanctum, leftover.
pub type Groups = BTreeMap<&'static str, BTreeSet<u32>>;

/// Every process of aLogs', by part (`table`: pid -> (parent, exe lower case); `started`: a process's start time).
pub fn alogs_groups(table: &HashMap<u32, (u32, String)>, app: u32, meter: u32, sanctum: u32, helpers: &[u32], started: &mut dyn FnMut(u32) -> Option<f64>) -> Groups {
    let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&pid, (parent, _)) in table {
        if *parent != 0 && *parent != pid {
            kids.entry(*parent).or_default().push(pid);
        }
    }
    let mut tree = |roots: &[u32]| -> BTreeSet<u32> {
        let mut seen = BTreeSet::new();
        let mut todo: Vec<u32> = roots.iter().copied().filter(|r| table.contains_key(r)).collect();
        while let Some(p) = todo.pop() {
            if !seen.insert(p) {
                continue;
            }
            for &k in kids.get(&p).map_or(&[][..], Vec::as_slice) {
                let (a, b) = (started(k), started(p));
                if a.is_none() || b.is_none() || a >= b {
                    todo.push(k);
                }
            }
        }
        seen
    };
    let mut meter_ = tree(&[meter]);
    meter_.extend(helpers.iter().copied().filter(|h| table.contains_key(h)));
    let sanctum_: BTreeSet<u32> = if sanctum != 0 { tree(&[sanctum]).difference(&meter_).copied().collect() } else { BTreeSet::new() };
    let under_app: BTreeSet<u32> = if app != 0 { tree(&[app]).into_iter().filter(|p| !meter_.contains(p) && !sanctum_.contains(p)).collect() } else { BTreeSet::new() };
    let windows: BTreeSet<u32> = under_app.iter().copied().filter(|p| table[p].1 == WEBVIEW).collect();
    let app_: BTreeSet<u32> = under_app.difference(&windows).copied().collect();
    let ours: BTreeSet<u32> = meter_.iter().chain(&sanctum_).chain(&under_app).copied().collect();
    let roots: Vec<u32> = table.iter().filter(|(p, (_, n))| !ours.contains(p) && (LEFTOVER.contains(&n.as_str()) || n == "alogs-app.exe")).map(|(p, _)| *p).collect();
    let leftover: BTreeSet<u32> = tree(&roots).difference(&ours).copied().collect();
    BTreeMap::from([("meter", meter_), ("app", app_), ("windows", windows), ("sanctum", sanctum_), ("leftover", leftover)])
}

/// The load line's " | aLogs by part: ..." (General Settings › Performance › "What each part of aLogs uses"): the share
/// of the whole PC each part of aLogs used (`used`, from CpuLoad::groups; None the first minute), how many processes a
/// part is when it's more than one, and what the rest of the PC took: the whole PC (`pc`) less the game and aLogs.
pub fn parts_text(groups: &Groups, used: &BTreeMap<&'static str, Option<f64>>, pc: Option<f64>, game: Option<f64>) -> String {
    let pct = |v: Option<f64>| v.map_or("?".to_string(), |v| format!("{v}%"));
    let n = |k: &str| groups.get(k).map_or(0, BTreeSet::len);
    let count = |k: &str| if n(k) > 1 { format!(" ({})", n(k)) } else { String::new() };
    let share = |k: &str| used.get(k).copied().flatten();
    // unknown while any share is; with no game running, the game took nothing
    let other = match pc {
        Some(pc) if used.values().all(Option::is_some) => Some(r1((pc - game.unwrap_or(0.0) - used.values().flatten().sum::<f64>()).max(0.0))),
        _ => None,
    };
    let leftover = match n("leftover") {
        0 => String::new(),
        1 => format!(", left over {} (1)", pct(share("leftover"))),
        _ => format!(", left over {}{}", pct(share("leftover")), count("leftover")),
    };
    format!(
        " | aLogs by part: meter {}{}, app {}, its windows {}{}, Sanctum Helper {}{}{leftover}; other programs {}",
        pct(share("meter")), count("meter"), pct(share("app")), pct(share("windows")), count("windows"), pct(share("sanctum")), count("sanctum"), pct(other)
    )
}

/// Sanctum Helper's process id from its status file (hosted by the aLogs app), or 0.
pub fn sanctum_pid() -> u32 {
    let p = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SanctumHelper").join("hosted").join("status.json");
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).and_then(|v| v.get("pid").and_then(serde_json::Value::as_u64)).unwrap_or(0) as u32
}

/// CPU used since the last sample, as % of the whole PC (all cores = 100).
#[derive(Default)]
pub struct CpuLoad {
    last: Option<(f64, f64, Option<f64>, Option<f64>, u32, Option<f64>)>,
    span: Option<f64>,
    pid_last: Option<HashMap<u32, (f64, f64)>>,
    pid_wall: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct CpuSample {
    pub pc: Option<f64>,
    pub meter: Option<f64>,
    pub game: Option<f64>,
    pub app: Option<f64>,
}

fn r1(v: f64) -> f64 {
    (v * 10.0).round_ties_even() / 10.0
}

impl CpuLoad {
    /// {pc, meter, game, app} since the last call, or None (the first call, or Windows didn't answer).
    pub fn sample(&mut self, game_pid: u32, app_pid: u32) -> Option<CpuSample> {
        let (total, busy) = win::system_times()?;
        let me = win::own_cpu();
        let (game, app) = (win::process_times(game_pid).map(|t| t.1), win::process_times(app_pid).map(|t| t.1));
        let now = (total, busy, me, game, game_pid, app);
        let last = self.last.replace(now)?;
        if now.0 <= last.0 {
            return None;
        }
        let span = now.0 - last.0;
        self.span = Some(span);
        let pct = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => Some(r1(100.0 * (a - b) / span)),
            _ => None,
        };
        Some(CpuSample { pc: pct(Some(now.1), Some(last.1)), meter: pct(now.2, last.2), game: if now.4 == last.4 { pct(now.3, last.3) } else { None }, app: pct(now.5, last.5) })
    }

    /// (start, CPU seconds) of a process, or None.
    pub fn times(&self, pid: u32) -> Option<(f64, f64)> {
        win::process_times(pid)
    }

    /// Starts afresh (the per-part shares were switched off).
    pub fn forget_parts(&mut self) {
        self.pid_last = None;
    }

    /// % of the whole PC each group used since the last call (right after sample()); None on the first call.
    pub fn groups(&mut self, groups: &Groups, times: &HashMap<u32, (f64, f64)>) -> BTreeMap<&'static str, Option<f64>> {
        let wall = win::wall_time();
        let (last, last_wall, span) = (self.pid_last.take(), self.pid_wall, self.span);
        let mut seen = HashMap::new();
        let mut out = BTreeMap::new();
        for (name, pids) in groups {
            let mut used = 0.0;
            for &p in pids {
                let Some(t) = times.get(&p).copied().or_else(|| self.times(p)) else { continue };
                seen.insert(p, t);
                match last.as_ref().and_then(|l| l.get(&p)) {
                    Some(b) if b.0 == t.0 => used += (t.1 - b.1).max(0.0),
                    _ => {
                        if last_wall.is_some_and(|w| t.0 >= w) {
                            used += t.1;
                        }
                    }
                }
            }
            out.insert(*name, used);
        }
        self.pid_last = Some(seen);
        self.pid_wall = Some(wall);
        match (last, span) {
            (Some(_), Some(s)) if s != 0.0 => out.into_iter().map(|(k, u)| (k, Some(r1(100.0 * u / s)))).collect(),
            _ => out.into_keys().map(|k| (k, None)).collect(),
        }
    }
}

/// Graphics card use per process (its busiest engine, like Task Manager), from the GPU Engine counters.
pub struct GpuLoad {
    q: win::Pdh,
}

pub struct GpuSample {
    pub by_pid: HashMap<u32, f64>,
    pub total: f64,
}

impl Default for GpuLoad {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuLoad {
    pub fn new() -> GpuLoad {
        GpuLoad { q: win::Pdh::open("\\GPU Engine(*)\\Utilization Percentage") }
    }

    /// The time since the last sample, or None (no counters, or made just now).
    pub fn sample(&mut self) -> Option<GpuSample> {
        let items = self.q.sample()?;
        let re = regex::Regex::new(r"^pid_(\d+)_.*_engtype_(\w+)").unwrap();
        let mut per: HashMap<(u32, String), f64> = HashMap::new();
        let mut engines: HashMap<String, f64> = HashMap::new();
        for (name, value) in items {
            let Some(m) = re.captures(&name) else { continue };
            if value <= 0.0 {
                continue;
            }
            let (pid, eng) = (m[1].parse::<u32>().unwrap_or(0), m[2].to_string());
            *per.entry((pid, eng.clone())).or_insert(0.0) += value;
            *engines.entry(eng).or_insert(0.0) += value;
        }
        let mut by_pid: HashMap<u32, f64> = HashMap::new();
        for ((pid, _), v) in per {
            let e = by_pid.entry(pid).or_insert(0.0);
            *e = e.max(v);
        }
        Some(GpuSample { by_pid, total: engines.values().copied().fold(0.0, f64::max).min(100.0) })
    }
}

/// (free GB, total GB) or None.
pub fn ram() -> Option<(f64, f64)> {
    win::ram()
}

/// A process's working set in MB, or None.
pub fn process_mb(pid: u32) -> Option<f64> {
    if pid == 0 { None } else { win::process_mb(pid) }
}

/// pid -> (parent pid, program file name in lower case) of every running process. The names are only compared with
/// aLogs' own programs'; none is ever written down.
pub fn process_table() -> HashMap<u32, (u32, String)> {
    win::process_table()
}

/// One line on the PC: graphics card(s), CPU and threads, RAM, the main monitor's mode, the game window.
pub fn system_line(hwnd: usize) -> String {
    let Some(mem) = ram() else { return "PC: ?".into() };
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    let cards = win::graphics_cards();
    format!("PC: graphics {} | CPU {}, {threads} threads | RAM {:.0} GB | monitor {} | game window {}",
        if cards.is_empty() { "?".into() } else { cards.join(", ") }, win::cpu_name(), mem.1, win::monitor_mode(), win::window_mode(hwnd))
}

#[cfg(windows)]
mod win {
    use std::collections::HashMap;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE, RECT};
    use windows_sys::Win32::Graphics::Gdi::{DEVMODEW, DISPLAY_DEVICEW, ENUM_CURRENT_SETTINGS, EnumDisplayDevicesW, EnumDisplaySettingsW, GetMonitorInfoW, MONITORINFO, MonitorFromWindow};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
    use windows_sys::Win32::System::Performance::*;
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::SystemInformation::{GetSystemTimeAsFileTime, GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes, GetSystemTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GWL_STYLE, GetWindowLongW, GetWindowRect, IsIconic};

    fn ft(f: &FILETIME) -> f64 {
        ((u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)) as f64 / 1e7
    }

    fn zero() -> FILETIME {
        FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 }
    }

    pub fn system_times() -> Option<(f64, f64)> {
        let (mut idle, mut kern, mut user) = (zero(), zero(), zero());
        if unsafe { GetSystemTimes(&mut idle, &mut kern, &mut user) } == 0 {
            return None;
        }
        let total = ft(&kern) + ft(&user);
        Some((total, total - ft(&idle)))
    }

    fn times_of(h: HANDLE) -> Option<(f64, f64)> {
        let (mut c, mut e, mut k, mut u) = (zero(), zero(), zero(), zero());
        if unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) } == 0 {
            return None;
        }
        Some((ft(&c), ft(&k) + ft(&u)))
    }

    pub fn own_cpu() -> Option<f64> {
        times_of(unsafe { GetCurrentProcess() }).map(|t| t.1)
    }

    pub fn process_times(pid: u32) -> Option<(f64, f64)> {
        if pid == 0 {
            return None;
        }
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return None;
        }
        let t = times_of(h);
        unsafe { CloseHandle(h) };
        t
    }

    pub fn wall_time() -> f64 {
        let mut f = zero();
        unsafe { GetSystemTimeAsFileTime(&mut f) };
        ft(&f)
    }

    pub fn ram() -> Option<(f64, f64)> {
        let mut s: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        s.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut s) } == 0 {
            return None;
        }
        let gb = (1u64 << 30) as f64;
        Some((s.ullAvailPhys as f64 / gb, s.ullTotalPhys as f64 / gb))
    }

    pub fn process_mb(pid: u32) -> Option<f64> {
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
        if h.is_null() {
            return None;
        }
        let mut m: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        m.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = unsafe { GetProcessMemoryInfo(h, &mut m, m.cb) };
        unsafe { CloseHandle(h) };
        (ok != 0).then(|| m.WorkingSetSize as f64 / (1u64 << 20) as f64)
    }

    pub fn process_table() -> HashMap<u32, (u32, String)> {
        let mut table = HashMap::new();
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap.is_null() || snap == INVALID_HANDLE_VALUE {
            return table;
        }
        let mut e: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = unsafe { Process32FirstW(snap, &mut e) };
        while ok != 0 {
            let n = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
            table.insert(e.th32ProcessID, (e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..n]).to_lowercase()));
            ok = unsafe { Process32NextW(snap, &mut e) };
        }
        unsafe { CloseHandle(snap) };
        table
    }

    type H = *mut core::ffi::c_void;
    const PDH_FMT_NOCAP100: u32 = 0x8000;

    pub struct Pdh {
        query: H,
        counter: H,
        fresh: bool,
    }

    // SAFETY: the query handle is used from one thread at a time (the meter's loop).
    unsafe impl Send for Pdh {}

    impl Pdh {
        pub fn open(path: &str) -> Pdh {
            let mut q: H = std::ptr::null_mut();
            let mut c: H = std::ptr::null_mut();
            let w: Vec<u16> = path.encode_utf16().chain([0]).collect();
            unsafe {
                if PdhOpenQueryW(std::ptr::null(), 0, &mut q) != 0 {
                    return Pdh { query: std::ptr::null_mut(), counter: std::ptr::null_mut(), fresh: true };
                }
                if PdhAddEnglishCounterW(q, w.as_ptr(), 0, &mut c) != 0 {
                    PdhCloseQuery(q);
                    return Pdh { query: std::ptr::null_mut(), counter: std::ptr::null_mut(), fresh: true };
                }
                PdhCollectQueryData(q);
            }
            Pdh { query: q, counter: c, fresh: true }
        }

        pub fn sample(&mut self) -> Option<Vec<(String, f64)>> {
            if self.query.is_null() || unsafe { PdhCollectQueryData(self.query) } != 0 {
                return None;
            }
            if self.fresh {
                self.fresh = false;
                return None;
            }
            let flags = PDH_FMT_DOUBLE | PDH_FMT_NOCAP100;
            let (mut size, mut count) = (0u32, 0u32);
            let r = unsafe { PdhGetFormattedCounterArrayW(self.counter, flags, &mut size, &mut count, std::ptr::null_mut()) };
            if r as u32 != PDH_MORE_DATA || size == 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if unsafe { PdhGetFormattedCounterArrayW(self.counter, flags, &mut size, &mut count, items) } != 0 {
                return None;
            }
            let mut out = vec![];
            for i in 0..count as usize {
                let it = unsafe { &*items.add(i) };
                let name = if it.szName.is_null() {
                    String::new()
                } else {
                    let mut n = 0;
                    while unsafe { *it.szName.add(n) } != 0 {
                        n += 1;
                    }
                    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(it.szName, n) })
                };
                out.push((name, unsafe { it.FmtValue.Anonymous.doubleValue }));
            }
            Some(out)
        }
    }

    impl Drop for Pdh {
        fn drop(&mut self) {
            if !self.query.is_null() {
                unsafe { PdhCloseQuery(self.query) };
            }
        }
    }

    pub fn graphics_cards() -> Vec<String> {
        let mut names: Vec<String> = vec![];
        let mut i = 0;
        loop {
            let mut d: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
            d.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
            if unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut d, 0) } == 0 {
                break;
            }
            let n = d.DeviceString.iter().position(|c| *c == 0).unwrap_or(d.DeviceString.len());
            let s = String::from_utf16_lossy(&d.DeviceString[..n]);
            if !s.is_empty() && !names.contains(&s) && d.StateFlags & 0x1 != 0 {
                names.push(s);
            }
            i += 1;
        }
        names
    }

    pub fn cpu_name() -> String {
        winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
            .open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")
            .and_then(|k| k.get_value::<String, _>("ProcessorNameString"))
            .map_or_else(|_| "?".into(), |s| s.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    pub fn monitor_mode() -> String {
        let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if unsafe { EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut dm) } == 0 {
            return "?".into();
        }
        format!("{}x{} at {} Hz", dm.dmPelsWidth, dm.dmPelsHeight, dm.dmDisplayFrequency)
    }

    pub fn window_mode(hwnd: usize) -> String {
        if hwnd == 0 {
            return "?".into();
        }
        let h = hwnd as windows_sys::Win32::Foundation::HWND;
        if unsafe { IsIconic(h) } != 0 {
            return "minimized".into();
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        unsafe { GetWindowRect(h, &mut r) };
        let mon = unsafe { MonitorFromWindow(h, 2) };
        let mut mi: MONITORINFO = unsafe { std::mem::zeroed() };
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        unsafe { GetMonitorInfoW(mon, &mut mi) };
        let m = mi.rcMonitor;
        let covers = (r.left, r.top, r.right, r.bottom) == (m.left, m.top, m.right, m.bottom);
        let style = unsafe { GetWindowLongW(h, GWL_STYLE) } as u32;
        let framed = style & 0x00C0_0000 != 0;
        let mode = if framed || !covers { "windowed" } else { "fullscreen or borderless" };
        format!("{mode} {}x{}", r.right - r.left, r.bottom - r.top)
    }
}

#[cfg(not(windows))]
mod win {
    use std::collections::HashMap;
    pub fn system_times() -> Option<(f64, f64)> { None }
    pub fn own_cpu() -> Option<f64> { None }
    pub fn process_times(_: u32) -> Option<(f64, f64)> { None }
    pub fn wall_time() -> f64 { 0.0 }
    pub fn ram() -> Option<(f64, f64)> { None }
    pub fn process_mb(_: u32) -> Option<f64> { None }
    pub fn process_table() -> HashMap<u32, (u32, String)> { HashMap::new() }
    pub struct Pdh;
    impl Pdh {
        pub fn open(_: &str) -> Pdh { Pdh }
        pub fn sample(&mut self) -> Option<Vec<(String, f64)>> { None }
    }
    pub fn graphics_cards() -> Vec<String> { vec![] }
    pub fn cpu_name() -> String { "?".into() }
    pub fn monitor_mode() -> String { "?".into() }
    pub fn window_mode(_: usize) -> String { "?".into() }
}

#[cfg(test)]
mod parts_tests {
    use super::*;

    fn groups(meter: &[u32], windows: &[u32], leftover: &[u32]) -> Groups {
        BTreeMap::from([
            ("meter", meter.iter().copied().collect()),
            ("app", BTreeSet::from([10])),
            ("windows", windows.iter().copied().collect()),
            ("sanctum", BTreeSet::from([30])),
            ("leftover", leftover.iter().copied().collect()),
        ])
    }

    /// "What each part of aLogs uses": each part's share, a count where a part is several processes, and the rest of the
    /// PC as what's left of the whole after the game and aLogs.
    #[test]
    fn the_parts_line_names_each_part_and_the_rest() {
        let used = BTreeMap::from([("meter", Some(1.4)), ("app", Some(0.1)), ("windows", Some(2.0)), ("sanctum", Some(0.5)), ("leftover", Some(0.0))]);
        assert_eq!(
            parts_text(&groups(&[1, 2, 3, 4], &[11, 12], &[]), &used, Some(34.6), Some(18.3)),
            " | aLogs by part: meter 1.4% (4), app 0.1%, its windows 2% (2), Sanctum Helper 0.5%; other programs 12.3%"
        );
        // a leftover updating window is said, with how many
        assert_eq!(
            parts_text(&groups(&[1], &[11], &[40]), &used, Some(34.6), None),
            " | aLogs by part: meter 1.4%, app 0.1%, its windows 2%, Sanctum Helper 0.5%, left over 0% (1); other programs 30.6%"
        );
        // the first minute nothing is known yet: no made-up rest
        let unknown: BTreeMap<&'static str, Option<f64>> = used.keys().map(|k| (*k, None)).collect();
        assert_eq!(
            parts_text(&groups(&[1], &[11], &[]), &unknown, Some(34.6), Some(18.3)),
            " | aLogs by part: meter ?, app ?, its windows ?, Sanctum Helper ?; other programs ?"
        );
    }
}
