// Desktop app for the aLogs suite. It owns the settings window, starts and stops the boss DPS meter (a packaged
// helper process), shows the meter's live status, and handles start-with-Windows, the tray and HidHide.
// The meter and this app share one settings.json in the meter's data folder. The app also runs the shared game
// capture (capture.rs) that the meter and Sanctum Helper read frames from.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::{json, Value};
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};
mod audio;
mod auth;
mod capture;
mod gamepad;
mod gpu_capture;
mod photos;
mod hidhide;
mod overlay_button;
mod sanctum;
mod site;
mod update;

const APP_DIR: &str = "aLogs"; // the suite's short name: data folder, tray; must match bossbar/settings.py
const FULL_NAME: &str = "aLogs: Aniiloger's Companion App";
const NO_WINDOW: u32 = 0x0800_0000;
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// The meter process. It always runs while the app is open; a watchdog restarts it if it ever stops.
struct Meter {
    child: Mutex<Option<Child>>,
    quitting: AtomicBool,
}

/// "Stop now" for the meter: a named Windows event it checks on every look (ALOGS_STOP_EVENT), so it saves an open
/// fight and exits in a fraction of a second. The meter also stops by itself when this app's process is gone
/// (ALOGS_PARENT), so quitting never has to wait for it.
fn stop_event_name() -> String {
    format!(r"Local\aLogs.MeterStop.{}", std::process::id())
}

fn stop_event() -> isize {
    use windows_sys::Win32::System::Threading::CreateEventW;
    static EVENT: std::sync::OnceLock<isize> = std::sync::OnceLock::new();
    *EVENT.get_or_init(|| {
        let name: Vec<u16> = stop_event_name().encode_utf16().chain(Some(0)).collect();
        unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) as isize }   // manual reset, not set
    })
}

fn signal_meter_stop(on: bool) {
    use windows_sys::Win32::System::Threading::{ResetEvent, SetEvent};
    let event = stop_event();
    if event != 0 {
        unsafe {
            if on { SetEvent(event as _); } else { ResetEvent(event as _); }
        }
    }
}

pub(crate) fn data_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(base).join(APP_DIR).join("meter");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

fn read_json(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

/// General Settings › Game window title, trimmed; "Aniimo" when it's empty (an empty one found no window at all).
pub(crate) fn game_title() -> String {
    let title = read_settings().pointer("/game/window_title").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if title.is_empty() { "Aniimo".into() } else { title }
}

pub(crate) fn read_settings() -> Value {
    match read_json(&settings_path()) {
        Value::Object(map) => Value::Object(map),
        _ => json!({}),
    }
}

/// The packaged meter (resources/meter/meter.exe); while developing, the Python source next to the app.
fn meter_command(app: &AppHandle) -> Option<Command> {
    if let Ok(res) = app.path().resource_dir() {
        let exe = res.join("resources").join("meter").join("meter.exe");
        if exe.exists() {
            return Some(Command::new(exe));
        }
    }
    let source = std::env::current_dir().ok()?.join("..").join("..").join("bossbar").join("live.py");
    let source = if source.exists() { source } else { PathBuf::from(r"..\bossbar\live.py") };
    if !source.exists() {
        return None;
    }
    let mut cmd = Command::new("python");
    cmd.arg(source);
    Some(cmd)
}

fn meter_running(meter: &State<Meter>) -> bool {
    let mut guard = meter.child.lock().unwrap();
    match guard.as_mut() {
        Some(child) => match child.try_wait() {
            Ok(None) => true,
            _ => {
                *guard = None;
                false
            }
        },
        None => false,
    }
}

fn stop_meter_inner(meter: &State<Meter>) {
    if let Some(mut child) = meter.child.lock().unwrap().take() {
        // asked to stop, it saves and exits within a fraction of a second; forced only if it hangs
        signal_meter_stop(true);
        let pid = child.id().to_string();
        for _ in 0..30 {
            if let Ok(Some(_)) = child.try_wait() {
                release_controllers();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = Command::new("taskkill").args(["/PID", &pid, "/T", "/F"]).creation_flags(NO_WINDOW).status();
        let _ = child.wait();
    }
    release_controllers();
}

/// Starts the meter unless it is already running. Check and start happen under one lock, so the watchdog and a
/// settings restart can never start two.
fn ensure_meter(app: &AppHandle) -> Result<(), String> {
    let meter = app.state::<Meter>();
    if meter.quitting.load(Ordering::SeqCst) {
        return Ok(());
    }
    let mut guard = meter.child.lock().unwrap();
    // DPS Meter > Meter & overlay > "Run the DPS meter": off means no meter at all (no overlay, records or submissions)
    if !read_settings().pointer("/meter/enabled").and_then(Value::as_bool).unwrap_or(true) {
        if let Some(mut child) = guard.take() {
            // asked to stop like on quit (it saves an open fight), forced only if it hangs (was killed outright)
            signal_meter_stop(true);
            for _ in 0..30 {
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            if let Ok(None) = child.try_wait() {
                let _ = child.kill();
                let _ = child.wait();
            }
            release_controllers();
        }
        return Ok(());
    }
    if let Some(child) = guard.as_mut() {
        if let Ok(None) = child.try_wait() {
            return Ok(());
        }
        release_controllers(); // it stopped on its own (a crash): it may have been holding the controller hidden
    }
    let mut cmd = meter_command(app).ok_or("The meter program was not found in this installation.")?;
    signal_meter_stop(false);
    cmd.env("ALOGS_DATA", data_dir()).env("ALOGS_STOP_EVENT", stop_event_name())
        .env("ALOGS_PARENT", std::process::id().to_string())
        .env("ALOGS_VERSION", format!("aLogs-{}+rc.{}", app.package_info().version, update::build_rc()))
        .creation_flags(NO_WINDOW);
    let child = cmd.spawn().map_err(|e| format!("Could not start the meter: {e}"))?;
    set_priority(Some(&child), &read_settings());
    *guard = Some(child);
    Ok(())
}

/// Quits at once: the window goes away, the meter is told to stop, and the app exits without waiting. The meter
/// (stop event, parent gone) and Sanctum Helper (--parent) each notice within a fraction of a second, save and exit.
/// Controllers visible to the game again: called whenever the meter stops, in case it stopped during an input hold
/// with the controller hidden (HidHide stays switched on otherwise until the next start).
fn release_controllers() {
    if hidhide::installed() {
        let _ = hidhide::set_active(false);
    }
}

fn quit_app(app: &AppHandle) {
    app.state::<Meter>().quitting.store(true, Ordering::SeqCst);
    release_controllers();
    for (_, window) in app.webview_windows() {
        let _ = window.hide();
    }
    signal_meter_stop(true);
    app.exit(0);
}

/// How much of the PC aLogs, the meter and Sanctum Helper may take when it's busy (General › Priority):
/// "low" (below normal: the game and OBS come first), "normal" or "high" (above normal); not set (the default):
/// left as Windows starts it (normal), so None.
fn priority_class(settings: &Value) -> Option<u32> {
    use windows_sys::Win32::System::Threading::{ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS};
    match settings.pointer("/app/priority").and_then(Value::as_str) {
        Some("low") => Some(BELOW_NORMAL_PRIORITY_CLASS),
        Some("normal") => Some(NORMAL_PRIORITY_CLASS),
        Some("high") => Some(ABOVE_NORMAL_PRIORITY_CLASS),
        _ => None,
    }
}

/// Sets the priority of `child` (or, with None, of aLogs itself).
pub(crate) fn set_priority(child: Option<&Child>, settings: &Value) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetPriorityClass};
    let handle = match child {
        Some(c) => c.as_raw_handle() as _,
        None => unsafe { GetCurrentProcess() },
    };
    // not set: back to normal (it may have been set earlier this session)
    let class = priority_class(settings).unwrap_or(windows_sys::Win32::System::Threading::NORMAL_PRIORITY_CLASS);
    unsafe { SetPriorityClass(handle, class) };
}

fn apply_priority(app: &AppHandle, settings: &Value) {
    set_priority(None, settings);
    if let Some(child) = app.state::<Meter>().child.lock().unwrap().as_ref() {
        set_priority(Some(child), settings);
    }
    sanctum::set_priority(app, settings);
}

#[tauri::command]
fn capture_switch() -> Value {
    overlay_button::info()
}

/// General Settings › Screen capture › Use your own picture (it was in the button's right-click menu, which can't be
/// used in game: the mouse only shows while Alt is held).
#[tauri::command(async)]
fn capture_choose_icon() -> Value {
    overlay_button::choose_icon();
    overlay_button::info()
}

/// The controller buttons held right now (General's controller picker asks while it waits).
#[tauri::command]
fn capture_pad_held() -> u32 {
    overlay_button::pad_held()
}

/// The screen-capture switch and its overlay button, from the app's settings page.
#[tauri::command]
fn set_capture_switch(app: AppHandle, capture_on: Option<bool>, show: Option<bool>, locked: Option<bool>, reset_position: Option<bool>, default_icon: Option<bool>, hotkey: Option<Value>, pad: Option<Value>) -> Value {
    if let Some(on) = capture_on {
        overlay_button::set_capture(on);
        sanctum::ensure(&app, &read_settings());
    }
    if let Some(show) = show {
        overlay_button::set_shown(show);
    }
    if let Some(locked) = locked {
        overlay_button::set_locked(locked);
    }
    if reset_position == Some(true) {
        overlay_button::reset_position();
    }
    if default_icon == Some(true) {
        overlay_button::use_default_icon();
    }
    // {vk, mods, label} from General's key picker; vk 0 (or null) clears it: no hotkey, no label under the button
    if let Some(h) = hotkey {
        let vk = h.get("vk").and_then(Value::as_u64).unwrap_or(0) as u32;
        let mods = h.get("mods").and_then(Value::as_u64).unwrap_or(0) as u32;
        overlay_button::set_hotkey(vk, mods, h.get("label").and_then(Value::as_str).unwrap_or(""));
    }
    // {buttons, label} from the controller picker; 0 clears it
    if let Some(p) = pad {
        let buttons = p.get("buttons").and_then(Value::as_u64).unwrap_or(0) as u32;
        overlay_button::set_pad(buttons, p.get("label").and_then(Value::as_str).unwrap_or(""));
    }
    overlay_button::info()
}

#[tauri::command]
fn get_settings() -> Value {
    read_settings()
}

/// Saves the settings. The meter picks most of them up by itself within a second; only the overlay's and the game
/// window's are set when it starts, so it restarts just for those (a restart ends a fight in progress).
// (async): on a worker thread, not the window's (it showed "Not responding" while this ran)
#[tauri::command(async)]
fn save_settings(app: AppHandle, meter: State<'_, Meter>, settings: Value) -> Result<bool, String> {
    let old = read_settings();
    let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    let path = settings_path();
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    // the meter may be reading settings.json at that moment (Windows then refuses the replace): try again shortly
    let mut tries = 0;
    while let Err(e) = fs::rename(&tmp, &path) {
        tries += 1;
        if tries >= 10 {
            return Err(e.to_string());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let autostart = settings.pointer("/app/start_with_windows").and_then(Value::as_bool).unwrap_or(false);
    set_autostart(autostart)?;
    if ["overlay", "game"].iter().any(|k| old.get(*k) != settings.get(*k)) {
        stop_meter_inner(&meter);
    }
    ensure_meter(&app)?;
    sanctum::share_settings(&settings);
    sanctum::ensure(&app, &settings);
    apply_priority(&app, &settings);
    Ok(true)
}

#[tauri::command]
fn get_status(meter: State<Meter>) -> Value {
    let running = meter_running(&meter);
    let mut status = read_json(&data_dir().join("status.json"));
    if !status.is_object() {
        status = json!({});
    }
    status["running"] = json!(running);
    status["data_dir"] = json!(data_dir().to_string_lossy());
    status["hidhide_installed"] = json!(hidhide::installed());
    status["borderless_capture"] = json!(capture::borderless_capture());
    status
}

fn set_autostart(on: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = Command::new("reg");
    if on {
        cmd.args(["add", RUN_KEY, "/v", APP_DIR, "/t", "REG_SZ", "/d"])
            .arg(format!("\"{}\" --minimized", exe.display()))
            .arg("/f");
    } else {
        cmd.args(["delete", RUN_KEY, "/v", APP_DIR, "/f"]);
    }
    let _ = cmd.creation_flags(NO_WINDOW).status();
    Ok(())
}

/// Opens one of the meter's folders/files, or a web page (https only) in the default browser.
#[tauri::command]
fn open_target(target: String) -> Result<(), String> {
    let data = data_dir();
    let path = match target.as_str() {
        "data" => data,
        "logs" => data.join("logs"),
        "test_frames" => data.join("logs").join("test_frames"),
        "meter_log" => data.join("logs").join("meter.log"),
        url if url.starts_with("https://") => {
            Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).creation_flags(NO_WINDOW).spawn().map_err(|e| e.to_string())?;
            return Ok(());
        }
        _ => return Err("Unknown target.".into()),
    };
    if target == "test_frames" {
        let _ = fs::create_dir_all(&path); // empty until the first test picture: open it anyway
    }
    if !path.exists() {
        return Err(format!("{} does not exist yet.", path.display()));
    }
    let program = if path.is_dir() { "explorer" } else { "notepad" };
    Command::new(program).arg(path).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Keeps private data out of a "Send logs" report: every file is text, and before it's zipped the Windows user and PC
/// names (in paths like C:\Users\<name>\AppData) become <user>/<pc>, and anything shaped like a sign-in token, a
/// Bearer header, a Discord ID or an email address is masked. The game UID (12 digits) and in-game name stay: they're
/// what a report is about.
struct Scrubber {
    rules: Vec<(regex::Regex, &'static str)>,
    names: Vec<(regex::Regex, &'static str)>,
}

impl Scrubber {
    fn new() -> Scrubber {
        let rule = |pattern: &str, with: &'static str| (regex::Regex::new(pattern).expect("scrub pattern"), with);
        let mut names = Vec::new();
        for (var, with) in [("USERNAME", "<user>"), ("COMPUTERNAME", "<pc>")] {
            if let Ok(name) = std::env::var(var) {
                if name.chars().count() >= 3 {
                    names.push(rule(&format!(r"(?i)\b{}\b", regex::escape(&name)), with));
                    // and as JSON escapes it: letters beyond ASCII as \uXXXX (a name with an accent, say)
                    let escaped: String =
                        name.chars().map(|c| if c.is_ascii() { c.to_string() } else { format!(r"\u{:04x}", c as u32) }).collect();
                    if escaped != name {
                        names.push(rule(&format!("(?i){}", regex::escape(&escaped)), with));
                    }
                }
            }
        }
        Scrubber {
            rules: vec![
                // the user folder's name, also where JSON wrote its letters as \uXXXX (those stopped the old rule)
                rule(r#"(?i)([A-Z]:[\\/]+Users[\\/]+)(?:\\u[0-9a-f]{4}|[^\\/\s"'])+"#, "${1}<user>"),
                rule(r"alb_[A-Za-z0-9_*-]+", "alb_***"),
                rule(r"(?i)\bBearer\s+\S+", "Bearer ***"),
                rule(r"[A-Za-z0-9_-]{32,}", "***"),
                rule(r"\b\d{17,20}\b", "***"),
                rule(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}", "***@***"),
            ],
            names,
        }
    }

    fn clean(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (pattern, with) in self.rules.iter().chain(&self.names) {
            out = pattern.replace_all(&out, *with).into_owned();
        }
        out
    }
}

/// The logs for a "Send logs" report, as a zip (base64), one for the whole app: which build, the meter (meter.log, the
/// newest fight logs, profile notes, status, settings without the leaderboard token) and Sanctum Helper (its log,
/// settings and state). Most useful first; whatever would take it past ~7.5 MB is left out.
// (async): on a worker thread, not the window's (it showed "Not responding" while this ran)
#[tauri::command(async)]
fn collect_logs(app: AppHandle) -> Result<String, String> {
    collect_logs_inner(&app.package_info().version.to_string(), meter_running(&app.state::<Meter>()))
}

fn collect_logs_inner(version: &str, meter_on: bool) -> Result<String, String> {
    use std::io::Write;
    use base64::Engine;
    const LIMIT: u64 = 7_500_000;
    let data = data_dir();
    let logs = data.join("logs");
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    fn add(files: &mut Vec<(String, Vec<u8>)>, name: &str, path: PathBuf) {
        if let Ok(bytes) = fs::read(&path) {
            files.push((name.to_string(), bytes));
        }
    }
    let info = format!(
        "aLogs {version} (RC {})\nmeter running: {meter_on}\nsent: {}\n",
        update::build_number(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
    );
    files.push(("app.txt".into(), info.into_bytes()));
    add(&mut files, "meter.log", logs.join("meter.log"));
    let mut fights: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(&logs)
        .map(|dir| {
            dir.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    name.starts_with('2') && name.ends_with(".csv")     // one per fight: 20261027_185320_AlphaIrisal.csv
                })
                .filter_map(|p| Some((fs::metadata(&p).ok()?.modified().ok()?, p)))
                .collect()
        })
        .unwrap_or_default();
    fights.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in fights.into_iter().take(20) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("fight.csv").to_string();
        add(&mut files, &format!("fights/{name}"), path);
    }
    for name in ["profile_events.csv", "time_check.csv", "egg_matches.csv"] {
        add(&mut files, name, logs.join(name));
    }
    add(&mut files, "status.json", data.join("status.json"));
    let mut settings = read_settings();
    if let Some(board) = settings.get_mut("leaderboard").and_then(Value::as_object_mut) {
        board.remove("token");                               // the Discord sign-in
    }
    if let Some(game) = settings.get_mut("game").and_then(Value::as_object_mut) {
        game.remove("player_log");                           // a path on this PC
    }
    files.push(("settings.json".into(), serde_json::to_vec_pretty(&settings).unwrap_or_default()));
    let sh = sanctum::sh_dir();
    add(&mut files, "sanctum/log.txt", sh.join("log.txt"));
    add(&mut files, "sanctum/settings.json", sh.join("settings.json"));
    add(&mut files, "sanctum/status.json", sh.join("hosted").join("status.json"));
    add(&mut files, "sanctum/info.json", sh.join("hosted").join("info.json"));
    add(&mut files, "meter.log.1", logs.join("meter.log.1"));

    // Each file is compressed on its own first to see what it adds, and only added if the report stays under the
    // limit (writing it and then aborting it left stray bytes that could make the whole zip unreadable).
    let scrub = Scrubber::new();
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let measure = |name: &str, text: &[u8]| -> Result<u64, String> {
        let mut probe = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        probe.start_file(name, options).map_err(|e| e.to_string())?;
        probe.write_all(text).map_err(|e| e.to_string())?;
        Ok(probe.finish().map_err(|e| e.to_string())?.into_inner().len() as u64)
    };
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut total = 0u64;
    for (name, bytes) in files {
        let text = scrub.clean(&String::from_utf8_lossy(&bytes));
        let adds = measure(&name, text.as_bytes())?;
        if total + adds > LIMIT {
            continue; // too big for what's left: the rest (smaller) may still fit
        }
        total += adds;
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    }
    let bytes = zip.finish().map_err(|e| e.to_string())?.into_inner();
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Another account proved this player's UID first (the meter saved what a dispute would send in
/// character_dispute.json): queue the dispute, with the player's note, for the meter to send with the game menu proof.
#[tauri::command]
fn send_character_dispute(note: String) -> Result<(), String> {
    let data = data_dir();
    let text = fs::read_to_string(data.join("character_dispute.json")).map_err(|_| "There's no dispute to send.".to_string())?;
    let dispute: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let uid = dispute["gameUid"].as_str().unwrap_or("").chars().filter(|c| c.is_ascii_digit()).collect::<String>();
    if uid.is_empty() {
        return Err("There's no dispute to send.".into());
    }
    let item = serde_json::json!({
        "endpoint": "disputes",
        "payload": { "gameUid": uid, "name": dispute["name"], "note": note.chars().take(500).collect::<String>() },
        "proof": dispute["proof"],
        "queued_at": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0),
    });
    let queue = data.join("logs").join("submit_queue");
    fs::create_dir_all(&queue).map_err(|e| e.to_string())?;
    fs::write(queue.join(format!("dispute__{uid}.json")), item.to_string()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Link character (Dashboard): asks the meter to read your name and UID from the game's Profile screen
/// (character_link.json; the meter shows what it read in status.json's character.link), then sends it once you confirm.
/// A proven name only changes this way: the meter never replaces it on its own.
#[tauri::command]
fn character_link(action: String, uid: Option<String>, name: Option<String>) -> Result<(), String> {
    let path = data_dir().join("character_link.json");
    match action.as_str() {
        "request" => {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            fs::write(&path, serde_json::json!({ "request": now }).to_string()).map_err(|e| e.to_string())
        }
        "confirm" => {
            let text = fs::read_to_string(&path).map_err(|_| "The link request expired. Press Link character again.".to_string())?;
            let mut req: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            req["confirm"] = serde_json::json!({ "uid": uid.unwrap_or_default(), "name": name.unwrap_or_default() });
            fs::write(&path, req.to_string()).map_err(|e| e.to_string())
        }
        _ => {
            let _ = fs::remove_file(&path);
            Ok(())
        }
    }
}

/// Controller blocking: installs the HidHide driver if needed and attaches it to the controller classes (Windows asks
/// for admin; also repairs an install that never attached), then puts every connected game controller on its block
/// list. Returns how many controllers were found.
#[tauri::command]
async fn setup_controller_blocking(app: AppHandle) -> Result<usize, String> {
    if !hidhide::installed() || !hidhide::filters_registered() {
        let dir = app.path().resource_dir().map_err(|e| e.to_string())?.join("resources").join("vendor").join("hidhide");
        if !dir.join("HidHide.inf").exists() {
            return Err("The HidHide driver isn't included in this build.".into());
        }
        hidhide::run_elevated(&format!("--hidhide install \"{}\"", dir.display()))?;
        if !hidhide::installed() {
            return Err("The HidHide driver was installed but isn't running yet. Restart Windows, then try again.".into());
        }
    }
    hidhide::block_connected_controllers()
}

/// The Controller section: driver installed, hiding on right now, and each connected controller by name.
#[tauri::command]
async fn controller_status() -> Value {
    hidhide::status()
}

/// Block one controller, or stop blocking it.
#[tauri::command]
async fn set_controller_blocked(id: String, on: bool) -> Result<(), String> {
    hidhide::set_blocked(&id, on)
}

/// Hides the blocked controllers for a moment and checks they really disappear for apps.
#[tauri::command]
async fn test_controller_blocking() -> Result<Vec<String>, String> {
    hidhide::test_blocking()
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn main() {
    // the one admin step of controller blocking (the app runs itself elevated for it; so does the uninstaller)
    let args: Vec<String> = std::env::args().collect();
    if let Some(at) = args.iter().position(|a| a == "--hidhide") {
        std::process::exit(hidhide::admin_task(&args[at + 1..]));
    }
    // never leave controllers hidden after a crash
    if hidhide::installed() {
        let _ = hidhide::set_active(false);
    }
    let minimized = std::env::args().any(|a| a == "--minimized");
    tauri::Builder::default()
        .manage(Meter { child: Mutex::new(None), quitting: AtomicBool::new(false) })
        .manage(sanctum::Sanctum::new())
        .manage(update::Pending::default())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_settings, save_settings, get_status, open_target, capture_switch, set_capture_switch, capture_pad_held, capture_choose_icon,
            auth::auth_listen_start, auth::auth_listen_wait, auth::auth_listen_cancel, auth::auth_window_open, auth::auth_window_close, setup_controller_blocking, controller_status, test_controller_blocking, set_controller_blocked, send_character_dispute, character_link, collect_logs, photos::game_photos, photos::photos_stamp, site::site_view,
            sanctum::install_dotnet_desktop,
            sanctum::sanctum_state, sanctum::sanctum_save, sanctum::sanctum_command,
            sanctum::sanctum_layout_image, sanctum::sanctum_icon,
            update::update_state, update::check_update, update::install_update, update::install_update_later,
            update::build_number
        ])
        .setup(move |app| {
            let show = MenuItem::with_id(app, "show", "Open", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit (stops the meter)", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip(FULL_NAME)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit" => quit_app(app),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;
            audio::start();     // listens only while the meter asks (Boss Kill audio cue)
            set_priority(None, &read_settings());
            overlay_button::start(app.handle().clone());   // the on/off button over the game
            capture::start();
            // The meter runs whenever the app is open; the watchdog brings it back if it ever stops.
            // Sanctum Helper (when enabled) likewise.
            update::at_start(app.handle());      // an update set for "next time aLogs opens" installs before the meter starts
            let _ = ensure_meter(app.handle());
            sanctum::share_settings(&read_settings());
            sanctum::ensure(app.handle(), &read_settings());
            update::start(app.handle());
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                if handle.state::<Meter>().quitting.load(Ordering::SeqCst) {
                    break;
                }
                let _ = ensure_meter(&handle);
                sanctum::ensure(&handle, &read_settings());
            });
            if minimized {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let to_tray = read_settings().pointer("/app/close_to_tray").and_then(Value::as_bool).unwrap_or(true);
                if to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    api.prevent_close();
                    quit_app(window.app_handle());
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running the app");
}


#[cfg(test)]
mod scrub_tests {
    use super::Scrubber;

    #[test]
    fn user_folder_with_escaped_letters_is_scrubbed() {
        let s = Scrubber::new();
        let line = r#"{"player_log": "C:\Users\Jo\u00e3o\AppData\LocalLow\Aniimo\Player.log"}"#;
        let out = s.clean(line);
        assert!(!out.contains("Jo") && !out.contains("00e3"), "{out}");
        assert!(out.contains("<user>"), "{out}");
        assert_eq!(s.clean(r"C:\Users\Ana\x"), r"C:\Users\<user>\x");
    }
}
