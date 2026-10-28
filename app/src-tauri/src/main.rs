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
mod hidhide;
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

fn data_dir() -> PathBuf {
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

fn read_settings() -> Value {
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
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = Command::new("taskkill").args(["/PID", &pid, "/T", "/F"]).creation_flags(NO_WINDOW).status();
        let _ = child.wait();
    }
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
        if let Some(child) = guard.as_mut() {
            let _ = child.kill();
        }
        *guard = None;
        return Ok(());
    }
    if let Some(child) = guard.as_mut() {
        if let Ok(None) = child.try_wait() {
            return Ok(());
        }
    }
    let mut cmd = meter_command(app).ok_or("The meter program was not found in this installation.")?;
    signal_meter_stop(false);
    cmd.env("ALOGS_DATA", data_dir()).env("ALOGS_STOP_EVENT", stop_event_name())
        .env("ALOGS_PARENT", std::process::id().to_string())
        .env("ALOGS_VERSION", format!("aLogs-{}+rc.{}", app.package_info().version, update::build_rc()))
        .creation_flags(NO_WINDOW);
    *guard = Some(cmd.spawn().map_err(|e| format!("Could not start the meter: {e}"))?);
    Ok(())
}

/// Quits at once: the window goes away, the meter is told to stop, and the app exits without waiting. The meter
/// (stop event, parent gone) and Sanctum Helper (--parent) each notice within a fraction of a second, save and exit.
fn quit_app(app: &AppHandle) {
    app.state::<Meter>().quitting.store(true, Ordering::SeqCst);
    for (_, window) in app.webview_windows() {
        let _ = window.hide();
    }
    signal_meter_stop(true);
    app.exit(0);
}

#[tauri::command]
fn get_settings() -> Value {
    read_settings()
}

/// Saves the settings. The meter picks most of them up by itself within a second; only the overlay's and the game
/// window's are set when it starts, so it restarts just for those (a restart ends a fight in progress).
#[tauri::command]
fn save_settings(app: AppHandle, meter: State<Meter>, settings: Value) -> Result<bool, String> {
    let old = read_settings();
    let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    let path = settings_path();
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    let autostart = settings.pointer("/app/start_with_windows").and_then(Value::as_bool).unwrap_or(false);
    set_autostart(autostart)?;
    if ["overlay", "game"].iter().any(|k| old.get(*k) != settings.get(*k)) {
        stop_meter_inner(&meter);
    }
    ensure_meter(&app)?;
    sanctum::share_settings(&settings);
    sanctum::ensure(&app, &settings);
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
        "meter_log" => data.join("logs").join("meter.log"),
        url if url.starts_with("https://") => {
            Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).creation_flags(NO_WINDOW).spawn().map_err(|e| e.to_string())?;
            return Ok(());
        }
        _ => return Err("Unknown target.".into()),
    };
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
                }
            }
        }
        Scrubber {
            rules: vec![
                rule(r#"(?i)([A-Z]:[\\/]+Users[\\/]+)[^\\/\s"']+"#, "${1}<user>"),
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
#[tauri::command]
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

    // the zip's size so far, kept by its buffer (the writer doesn't tell)
    struct Sized(std::io::Cursor<Vec<u8>>, std::rc::Rc<std::cell::Cell<u64>>);
    impl Write for Sized {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let n = self.0.write(buf)?;
            self.1.set(self.0.get_ref().len() as u64);
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }
    impl std::io::Seek for Sized {
        fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
            self.0.seek(to)
        }
    }
    let scrub = Scrubber::new();
    let size = std::rc::Rc::new(std::cell::Cell::new(0u64));
    let mut zip = zip::ZipWriter::new(Sized(std::io::Cursor::new(Vec::new()), size.clone()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in files {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(scrub.clean(&String::from_utf8_lossy(&bytes)).as_bytes()).map_err(|e| e.to_string())?;
        zip.flush().map_err(|e| e.to_string())?;
        if size.get() > LIMIT {
            zip.abort_file().map_err(|e| e.to_string())?;
        }
    }
    let bytes = zip.finish().map_err(|e| e.to_string())?.0.into_inner();
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Another account proved this player's UID first (the meter saved what a dispute would send in
/// character_dispute.json): queue the dispute, with the player's note, for the meter to send with the game menu proof.
#[tauri::command]
fn send_character_dispute(note: String) -> Result<(), String> {
    let data = data_dir();
    let text = fs::read_to_string(data.join("character_dispute.json")).map_err(|_| "Nothing to dispute.".to_string())?;
    let dispute: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let uid = dispute["gameUid"].as_str().unwrap_or("").chars().filter(|c| c.is_ascii_digit()).collect::<String>();
    if uid.is_empty() {
        return Err("Nothing to dispute.".into());
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
            get_settings, save_settings, get_status, open_target,
            auth::auth_listen_start, auth::auth_listen_wait, setup_controller_blocking, controller_status, test_controller_blocking, set_controller_blocked, send_character_dispute, collect_logs, site::site_view,
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
            let title = read_settings().pointer("/game/window_title").and_then(Value::as_str).unwrap_or("Aniimo").to_string();
            audio::start(title.clone());     // listens only while the meter asks (Boss Kill audio cue)
            capture::start(title);
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

