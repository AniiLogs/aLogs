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
/// 14 Nov, the owner's call: Sanctum Helper runs with the game, not whenever aLogs does. It starts once the game's
/// window is there (the watchdog's ensure, every 3 s) and stops this long after it's gone: room for an alt-tab, a
/// crash or a restart of the game (a minimized game still has its window, so that never counts as gone).
const GAME_GONE_GRACE_MS: u64 = 60_000;

pub struct Sanctum {
    child: Mutex<Option<Child>>,
    /// A standalone Sanctum Helper holds its single-instance lock; retried every so often.
    blocked_until: AtomicU64,
    blocked: AtomicBool,
    /// When the game's window was first seen missing (ms since 1970), 0 while it's there (GAME_GONE_GRACE_MS).
    game_gone_at: AtomicU64,
}

impl Sanctum {
    pub fn new() -> Sanctum {
        Sanctum { child: Mutex::new(None), blocked_until: AtomicU64::new(0), blocked: AtomicBool::new(false), game_gone_at: AtomicU64::new(0) }
    }
}

/// Whether the game's window is there (visible or minimized), found as the shared capture finds it.
fn game_open() -> bool {
    !crate::capture::find_game().is_null()
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

/// Sanctum Helper's Dev switches that are on right now, each with its revision and title, and the ids of those made
/// persistent: nothing unless it's running (a status file left by an earlier run doesn't count). For the Dev tab's count
/// of how long each was on, and the feedback sent from there.
pub(crate) fn dev_switches() -> (Vec<(String, u64, String)>, Vec<String>) {
    let status = read_json(&hosted_dir().join("status.json"));
    if !status.get("time").and_then(Value::as_u64).is_some_and(|t| now_ms().saturating_sub(t) < 5000) {
        return (Vec::new(), Vec::new());
    }
    let ids = |list: &Value| -> Vec<String> { list.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect() };
    let on = ids(&status["devOn"]);
    if on.is_empty() {
        return (Vec::new(), ids(&status["devPersistent"]));
    }
    let info = read_json(&hosted_dir().join("info.json"));
    let option = |id: &str| info["dev"].as_array().and_then(|all| all.iter().find(|o| o["id"] == id));
    let on = on
        .into_iter()
        .map(|id| {
            let found = option(&id);
            let title = found.and_then(|o| o["title"].as_str()).unwrap_or(&id).to_string();
            (id.clone(), found.and_then(|o| o["revision"].as_u64()).unwrap_or(0), title)
        })
        .collect();
    (on, ids(&status["devPersistent"]))
}

/// The bundled Sanctum Helper (resources/sanctumhelper, Assets beside it), or, while developing, the workspace's own
/// Rust build next to this exe (crates/alogs-sanctum), with the reference's Assets (the map art is the game's: never
/// in the repo). The second value is the Assets folder to pass, when it isn't beside the exe.
fn exe(app: &AppHandle) -> Option<(PathBuf, Option<PathBuf>)> {
    if let Ok(res) = app.path().resource_dir() {
        let bundled = res.join("resources").join("sanctumhelper").join("SanctumHelper.exe");
        if bundled.exists() {
            return Some((bundled, None));
        }
    }
    let me = std::env::current_exe().ok()?;
    let dev = me.with_file_name("SanctumHelper.exe");
    if !dev.exists() {
        return None;
    }
    // target/<profile>/alogs-app.exe -> the workspace
    let assets = me.parent().and_then(|p| p.parent()).and_then(|p| p.parent())
        .map(|root| root.join("reference").join("SanctumHelper").join("app").join("SanctumHelper").join("Assets"))
        .filter(|a| a.exists());
    Some((dev, assets))
}

/// Sanctum Helper's own switch alone: it doesn't depend on the DPS meter or the leaderboards (the button over the game
/// turns it off and back on through this same setting).
pub fn enabled(settings: &Value) -> bool {
    settings.pointer("/sanctum/enabled").and_then(Value::as_bool).unwrap_or(true)
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
        try { Start-Process $f -ArgumentList '/install','/quiet','/norestart' -Verb RunAs -Wait } \
        finally { Remove-Item $f -ErrorAction SilentlyContinue }";
    let status = Command::new("powershell").args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(NO_WINDOW).status().map_err(|e| e.to_string())?;
    if !status.success() || !dotnet_desktop_installed() {
        return Err("The .NET Desktop Runtime wasn't installed (the download failed or the admin prompt was declined).".into());
    }
    Ok(())
}

/// General › Priority for a running Sanctum Helper.
pub fn set_priority(app: &AppHandle, settings: &Value) {
    if let Some(child) = app.state::<Sanctum>().child.lock().unwrap().as_ref() {
        crate::set_priority(Some(child), settings);
    }
}

/// How long the game's window has been gone (ms), 0 while it's there; the first time it's missed counts from now.
fn game_gone_for(state: &Sanctum, now: u64) -> u64 {
    gone_for(game_open(), &state.game_gone_at, now)
}

fn gone_for(open: bool, gone_at: &AtomicU64, now: u64) -> u64 {
    if open {
        gone_at.store(0, Ordering::SeqCst);
        return 0;
    }
    match gone_at.load(Ordering::SeqCst) {
        0 => {
            gone_at.store(now, Ordering::SeqCst);
            1
        }
        at => now.saturating_sub(at).max(1),
    }
}

/// Starts Sanctum Helper if it is enabled, the game's window is there and it isn't running; stops it when it was
/// turned off, or GAME_GONE_GRACE_MS after the game's window went (14 Nov, the owner's call: it used to run whenever
/// aLogs did). app.log says why each time it starts or stops.
pub fn ensure(app: &AppHandle, settings: &Value) {
    let state = app.state::<Sanctum>();
    // looked for before the lock is taken: nothing that may wait on another thread happens while it's held (the
    // window's thread takes it too, for the page's sanctum_state)
    let gone_for = game_gone_for(&state, now_ms());
    let mut guard = state.child.lock().unwrap();
    if let Some(child) = guard.as_mut() {
        match child.try_wait() {
            Ok(None) => {
                let why = if !enabled(settings) {
                    Some("it was turned off".to_string())
                } else if gone_for >= GAME_GONE_GRACE_MS {
                    Some(format!("the game's window has been gone for {} s", gone_for / 1000))
                } else {
                    None
                };
                if let Some(why) = why {
                    stop_child(child);
                    *guard = None;
                    crate::app_log(&format!("Sanctum Helper: stopped ({why})"));
                }
                return;
            }
            Ok(Some(status)) => {
                if status.code() == Some(EXIT_STANDALONE_RUNNING) {
                    state.blocked_until.store(now_ms() + 15_000, Ordering::SeqCst);
                    if !state.blocked.swap(true, Ordering::SeqCst) {
                        crate::app_log("Sanctum Helper: another copy of it is running on its own; trying again every 15 s");
                    }
                } else {
                    crate::app_log(&format!("Sanctum Helper: it ended by itself (exit code {:?})", status.code()));
                }
                *guard = None;
            }
            Err(_) => *guard = None,
        }
    }
    // not until the game's window is there: within the watchdog's 3 s of it appearing
    if !enabled(settings) || gone_for > 0 || now_ms() < state.blocked_until.load(Ordering::SeqCst) {
        return;
    }
    // (the Rust build needs no .NET runtime: nothing to install first)
    let Some((path, assets)) = exe(app) else { return };
    let mut cmd = Command::new(&path);
    cmd.args(["--hosted", "--parent", &std::process::id().to_string()]).creation_flags(NO_WINDOW);
    if let Some(a) = assets {
        cmd.arg("--assets").arg(a);
    }
    if crate::update::build_beta() > 0 {
        cmd.arg("--dev");   // the Dev tab's persistent switches are only ever loaded on a beta
    }
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    match cmd.spawn() {
        Ok(child) => {
            state.blocked.store(false, Ordering::SeqCst);
            crate::set_priority(Some(&child), settings);
            crate::app_log("Sanctum Helper: started (it's on and the game's window is open)");
            *guard = Some(child);
        }
        Err(e) => crate::app_log(&format!("Sanctum Helper: could not start ({e})")),
    }
}

/// Restarts Sanctum Helper when it's running (asked to quit, so it saves first), so it starts afresh: after the app
/// installs English OCR (install_english_ocr). It reads the Party screen with Windows OCR and builds its engine once,
/// as it starts (Engine.cs `OcrService ocr = new(...)`; again only when the game language changes), so until then it
/// kept reading with no OCR language, or the user's own. Not running (off, or no .NET): nothing to do.
pub fn restart(app: &AppHandle, settings: &Value) {
    {
        let state = app.state::<Sanctum>();
        let mut guard = state.child.lock().unwrap();
        let Some(child) = guard.as_mut() else { return };
        if !matches!(child.try_wait(), Ok(None)) {
            return;                                                  // it stopped: the watchdog's ensure starts it
        }
        stop_child(child);
        *guard = None;
        crate::app_log("Sanctum Helper: stopped (restarting it, for the OCR language)");
    }
    ensure(app, settings);
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
    let game = game_open();
    let mut status = read_json(&hosted_dir().join("status.json"));
    // A status file left by an earlier run doesn't count.
    let fresh = status.get("time").and_then(Value::as_u64).map_or(false, |t| now_ms().saturating_sub(t) < 5000);
    if !fresh {
        status = Value::Null;
    }
    json!({
        "installed": exe(&app).is_some(),
        "runtimeMissing": false,   // the Rust build needs no .NET runtime
        "running": running && fresh,
        "starting": running && !fresh,
        "standaloneRunning": state.blocked.load(Ordering::SeqCst),
        // 14 Nov: not running because the game isn't open: it starts with the game (the page says so, not "Starting…")
        "gameOpen": game,
        "waitingForGame": !running && !game,
        "settings": read_json(&sh_dir().join("settings.json")),
        "info": read_json(&hosted_dir().join("info.json")),
        "status": status,
        "folder": sh_dir().to_string_lossy(),
        "beta": crate::update::build_beta() > 0,
    })
}

/// The settings aLogs and Sanctum Helper share, set once in General Settings: the game window's title, whether
/// overlays show in recordings and screenshots, and Performance's Corner map redraws. Copied into Sanctum Helper's settings.json when they differ (it
/// picks up its file's changes), at start and whenever aLogs' settings are saved.
pub fn share_settings(settings: &Value) {
    let title = settings.pointer("/game/window_title").and_then(Value::as_str).filter(|t| !t.trim().is_empty()).unwrap_or("Aniimo");
    // overlay.in_recordings replaced the meter's own overlay.hide_from_capture (the opposite)
    let in_recordings = settings.pointer("/overlay/in_recordings").and_then(Value::as_bool)
        .unwrap_or_else(|| !settings.pointer("/overlay/hide_from_capture").and_then(Value::as_bool).unwrap_or(false));
    // General Settings › Performance › Corner map redraws (the page writes "capped" only while Performance toggles is on)
    let redraw_cap = settings.pointer("/sanctum/map_redraw").and_then(Value::as_str) == Some("capped");
    let current = read_json(&sh_dir().join("settings.json"));
    if current.get("WindowTitle").and_then(Value::as_str) != Some(title)
        || current.get("RecordableOverlay").and_then(Value::as_bool) != Some(in_recordings)
        || current.get("RedrawCap").and_then(Value::as_bool).unwrap_or(false) != redraw_cap
    {
        let _ = sanctum_save(serde_json::json!({ "WindowTitle": title, "RecordableOverlay": in_recordings, "RedrawCap": redraw_cap }));
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
/// 14 Nov: it runs only with the game, so while it's stopped a command would wait for nobody (it clears old ones as it
/// starts): open-logs opens its folder from here, anything else says it starts with the game.
#[tauri::command]
pub fn sanctum_command(state: State<Sanctum>, command: String, args: Option<Value>) -> Result<(), String> {
    const ALLOWED: [&str; 9] = ["dev-set", "dev-persistent", "new-run", "position-map", "capture-key", "capture-pad", "clear-pad", "render-layout", "open-logs"];
    if !ALLOWED.contains(&command.as_str()) {
        return Err("Unknown command.".into());
    }
    let running = state.child.lock().unwrap().as_mut().map_or(false, |c| matches!(c.try_wait(), Ok(None)));
    if !running {
        if command == "open-logs" {
            let _ = fs::create_dir_all(sh_dir());
            Command::new("explorer").arg(sh_dir()).spawn().map_err(|e| e.to_string())?;
            return Ok(());
        }
        return Err("Sanctum Helper isn't running: it starts with the game.".into());
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
    let (path, assets) = exe(&app)?;
    let dir = assets.unwrap_or_else(|| path.with_file_name("Assets")).join("icons");
    data_url(&dir.join(format!("{name}.png")))
}

/// 14 Nov: Sanctum Helper with the game: never started while the game's window is missing, stopped only once it has
/// been gone for GAME_GONE_GRACE_MS, and a window back in time starts the count again.
#[cfg(test)]
mod game_tests {
    use super::{gone_for, GAME_GONE_GRACE_MS};
    use std::sync::atomic::AtomicU64;

    #[test]
    fn the_grace_counts_from_the_first_miss() {
        let at = AtomicU64::new(0);
        assert_eq!(gone_for(true, &at, 1_000), 0, "open: not gone");
        assert!(gone_for(false, &at, 2_000) > 0, "missed once: gone (nothing starts)");
        assert!(gone_for(false, &at, 2_000 + GAME_GONE_GRACE_MS - 1) < GAME_GONE_GRACE_MS, "an alt-tab or restart: kept");
        assert!(gone_for(false, &at, 2_000 + GAME_GONE_GRACE_MS) >= GAME_GONE_GRACE_MS, "a minute: stopped");
        assert_eq!(gone_for(true, &at, 70_000), 0, "back: not gone");
        assert!(gone_for(false, &at, 80_000) < GAME_GONE_GRACE_MS, "gone again: counted afresh");
    }
}
