//! Sanctum Helper as an aLogs module. The app runs it with `--hosted --parent <pid>`: no tray icon or settings
//! window of its own. Its options stay in its own settings.json (%LOCALAPPDATA%\SanctumHelper), which the
//! Sanctum Helper page edits; it reloads them live. hosted\info.json lists the choices, hosted\status.json what
//! it sees, and hosted\cmd-*.json files ask it to do things (see Hosted.cs in the Sanctum Helper source).

use serde_json::{json, Map, Value};
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};

const NO_WINDOW: u32 = 0x0800_0000;
/// Exit code Sanctum Helper uses when a standalone copy is already running.
const EXIT_STANDALONE_RUNNING: i32 = 3;

pub struct Sanctum {
    child: Mutex<Option<Child>>,
    /// A standalone Sanctum Helper holds its single-instance lock; retried every so often.
    blocked_until: AtomicU64,
    blocked: AtomicBool,
}

impl Sanctum {
    pub fn new() -> Sanctum {
        Sanctum { child: Mutex::new(None), blocked_until: AtomicU64::new(0), blocked: AtomicBool::new(false) }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub(crate) fn sh_dir() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())).join("SanctumHelper")
}

fn hosted_dir() -> PathBuf {
    sh_dir().join("hosted")
}

fn read_json(path: &PathBuf) -> Value {
    fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

/// The bundled Sanctum Helper (resources/sanctumhelper), or a local build while developing.
fn exe(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(res) = app.path().resource_dir() {
        let bundled = res.join("resources").join("sanctumhelper").join("SanctumHelper.exe");
        if bundled.exists() {
            return Some(bundled);
        }
    }
    let dev = std::env::current_dir().ok()?.join("..").join("..").join("SanctumHelper").join("build").join("publish").join("SanctumHelper.exe");
    dev.exists().then_some(dev)
}

pub fn enabled(settings: &Value) -> bool {
    settings.pointer("/sanctum/enabled").and_then(Value::as_bool).unwrap_or(true) && crate::overlay_button::capture_on()
}

/// Sanctum Helper runs on the PC's .NET Desktop Runtime, version 9 or newer (Program Files\dotnet\shared).
pub fn dotnet_desktop_installed() -> bool {
    let base = std::env::var("ProgramW6432").or_else(|_| std::env::var("ProgramFiles")).unwrap_or_else(|_| r"C:\Program Files".into());
    let dir = Path::new(&base).join("dotnet").join("shared").join("Microsoft.WindowsDesktop.App");
    fs::read_dir(dir).map_or(false, |entries| entries.flatten().any(|e| {
        e.file_name().to_string_lossy().split('.').next().and_then(|major| major.parse::<u32>().ok()).map_or(false, |major| major >= 9)
    }))
}

/// Downloads Microsoft's .NET 9 Desktop Runtime installer and runs it (Windows asks for admin approval).
// (async): on a worker thread, not the window's (it showed "Not responding" while this ran)
#[tauri::command(async)]
pub fn install_dotnet_desktop() -> Result<(), String> {
    let script = "$f = Join-Path $env:TEMP 'windowsdesktop-runtime-9-win-x64.exe'; \
        Invoke-WebRequest -UseBasicParsing 'https://aka.ms/dotnet/9.0/windowsdesktop-runtime-win-x64.exe' -OutFile $f; \
        Start-Process $f -ArgumentList '/install','/quiet','/norestart' -Verb RunAs -Wait";
    let status = Command::new("powershell").args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(NO_WINDOW).status().map_err(|e| e.to_string())?;
    if !status.success() || !dotnet_desktop_installed() {
        return Err("The .NET Desktop Runtime wasn't installed (the download failed or the admin prompt was declined).".into());
    }
    Ok(())
}

/// Starts Sanctum Helper if it is enabled and not running; stops it when it was turned off.
/// General › Priority for a running Sanctum Helper.
pub fn set_priority(app: &AppHandle, settings: &Value) {
    if let Some(child) = app.state::<Sanctum>().child.lock().unwrap().as_ref() {
        crate::set_priority(Some(child), settings);
    }
}

pub fn ensure(app: &AppHandle, settings: &Value) {
    let state = app.state::<Sanctum>();
    let mut guard = state.child.lock().unwrap();
    if let Some(child) = guard.as_mut() {
        match child.try_wait() {
            Ok(None) => {
                if !enabled(settings) {
                    stop_child(child);
                    *guard = None;
                }
                return;
            }
            Ok(Some(status)) => {
                if status.code() == Some(EXIT_STANDALONE_RUNNING) {
                    state.blocked.store(true, Ordering::SeqCst);
                    state.blocked_until.store(now_ms() + 15_000, Ordering::SeqCst);
                }
                *guard = None;
            }
            Err(_) => *guard = None,
        }
    }
    if !enabled(settings) || now_ms() < state.blocked_until.load(Ordering::SeqCst) {
        return;
    }
    let Some(path) = exe(app) else { return };
    if !dotnet_desktop_installed() { return; }                       // the page offers to install the runtime
    let mut cmd = Command::new(&path);
    cmd.args(["--hosted", "--parent", &std::process::id().to_string()]).creation_flags(NO_WINDOW);
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    match cmd.spawn() {
        Ok(child) => {
            state.blocked.store(false, Ordering::SeqCst);
            crate::set_priority(Some(&child), settings);
            *guard = Some(child);
        }
        Err(e) => eprintln!("could not start Sanctum Helper: {e}"),
    }
}

fn stop_child(child: &mut Child) {
    let pid = child.id().to_string();
    // asked to close (it saves first); a windowless hosted process ignores a plain taskkill, so that was always forced
    let _ = write_command(Map::from_iter([("command".to_string(), json!("quit"))]));
    for _ in 0..30 {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = Command::new("taskkill").args(["/PID", &pid, "/T", "/F"]).creation_flags(NO_WINDOW).status();
    let _ = child.wait();
}

/// Everything the Sanctum Helper page shows: its settings, the choices (info) and what it sees (status).
#[tauri::command]
pub fn sanctum_state(app: AppHandle, state: State<Sanctum>) -> Value {
    let running = state.child.lock().unwrap().as_mut().map_or(false, |c| matches!(c.try_wait(), Ok(None)));
    let mut status = read_json(&hosted_dir().join("status.json"));
    // A status file left by an earlier run doesn't count.
    let fresh = status.get("time").and_then(Value::as_u64).map_or(false, |t| now_ms().saturating_sub(t) < 5000);
    if !fresh {
        status = Value::Null;
    }
    json!({
        "installed": exe(&app).is_some(),
        "runtimeMissing": !dotnet_desktop_installed(),
        "running": running && fresh,
        "starting": running && !fresh,
        "standaloneRunning": state.blocked.load(Ordering::SeqCst),
        "settings": read_json(&sh_dir().join("settings.json")),
        "info": read_json(&hosted_dir().join("info.json")),
        "status": status,
        "folder": sh_dir().to_string_lossy(),
    })
}

/// The settings aLogs and Sanctum Helper share, set once in General Settings: the game window's title and whether
/// overlays show in recordings and screenshots. Copied into Sanctum Helper's settings.json when they differ (it
/// picks up its file's changes), at start and whenever aLogs' settings are saved.
pub fn share_settings(settings: &Value) {
    let title = settings.pointer("/game/window_title").and_then(Value::as_str).filter(|t| !t.trim().is_empty()).unwrap_or("Aniimo");
    // overlay.in_recordings replaced the meter's own overlay.hide_from_capture (the opposite)
    let in_recordings = settings.pointer("/overlay/in_recordings").and_then(Value::as_bool)
        .unwrap_or_else(|| !settings.pointer("/overlay/hide_from_capture").and_then(Value::as_bool).unwrap_or(false));
    let current = read_json(&sh_dir().join("settings.json"));
    if current.get("WindowTitle").and_then(Value::as_str) != Some(title)
        || current.get("RecordableOverlay").and_then(Value::as_bool) != Some(in_recordings)
    {
        let _ = sanctum_save(serde_json::json!({ "WindowTitle": title, "RecordableOverlay": in_recordings }));
    }
}

/// Changes some of Sanctum Helper's settings (a partial object, merged into its settings.json).
#[tauri::command]
pub fn sanctum_save(changes: Value) -> Result<Value, String> {
    let Value::Object(changes) = changes else { return Err("Expected an object.".into()) };
    let path = sh_dir().join("settings.json");
    let mut settings = match read_json(&path) {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    for (key, value) in changes {
        // Layers is a dictionary: merge it so one switch doesn't drop the others.
        if let (Some(Value::Object(old)), Value::Object(new)) = (settings.get_mut(&key), &value) {
            for (k, v) in new {
                old.insert(k.clone(), v.clone());
            }
        } else {
            settings.insert(key, value);
        }
    }
    let _ = fs::create_dir_all(sh_dir());
    let text = serde_json::to_string_pretty(&Value::Object(settings.clone())).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(Value::Object(settings))
}

/// Asks the running Sanctum Helper to do something: new-run, position-map, capture-key, capture-pad,
/// clear-pad, render-layout { layout, width, height }, open-logs.
#[tauri::command]
pub fn sanctum_command(command: String, args: Option<Value>) -> Result<(), String> {
    const ALLOWED: [&str; 7] = ["new-run", "position-map", "capture-key", "capture-pad", "clear-pad", "render-layout", "open-logs"];
    if !ALLOWED.contains(&command.as_str()) {
        return Err("Unknown command.".into());
    }
    let mut body = match args {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    body.insert("command".into(), json!(command));
    write_command(body)
}

/// A command for the running Sanctum Helper: a small file in its hosted folder, which it picks up and deletes.
fn write_command(body: Map<String, Value>) -> Result<(), String> {
    let dir = hosted_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!("cmd-{}-{}", now_ms(), std::process::id());
    let tmp = dir.join(format!("{name}.tmp"));
    fs::write(&tmp, Value::Object(body).to_string()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, dir.join(format!("{name}.json"))).map_err(|e| e.to_string())?;
    Ok(())
}

fn data_url(path: &PathBuf) -> Option<String> {
    use base64::Engine as _;
    let bytes = fs::read(path).ok()?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// A picture rendered by Sanctum Helper for the layout browser (after a render-layout command), if it is there.
#[tauri::command]
pub fn sanctum_layout_image(layout: u32) -> Option<String> {
    data_url(&hosted_dir().join(format!("layout-{layout}.png")))
}

/// One of Sanctum Helper's marker icons (Assets/icons/<name>.png).
#[tauri::command]
pub fn sanctum_icon(app: AppHandle, name: String) -> Option<String> {
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let dir = exe(&app)?.parent()?.join("Assets").join("icons");
    data_url(&dir.join(format!("{name}.png")))
}
