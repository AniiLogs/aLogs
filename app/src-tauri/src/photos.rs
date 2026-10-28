//! In-game photos for the player's AniiLogs profile.
//!
//! Aniimo saves the pictures you take (when its Settings › Storage is "Local" or "Local and cloud") in its own install
//! folder: `<install>\Pictures\<yy-mm-dd>\<time><id>.png`, 4K PNGs, with `PlayerPhotoMetaInfo.json` beside them filing
//! each one under the UID that took it, with when, where (scene and position) and which Aniimo are in it. The install
//! folder differs per launcher (Steam or not), so it's read from the game's own log, whose first lines name it
//! ("Discovering subsystems at path <install>/Aniimo_Data/..."), with the running game's own path as a fallback.
//!
//! The UI shows the photos straight from that folder (the asset protocol, allowed for that one folder at run time),
//! makes a WebP of the ones you pick and uploads them (app.js); nothing here sends anything.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

fn player_log_dirs(settings: &Value) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(custom) = settings.pointer("/game/player_log").and_then(Value::as_str).filter(|p| !p.trim().is_empty()) {
        if let Some(dir) = Path::new(custom.trim()).parent() {
            out.push(dir.to_path_buf());
        }
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        out.push(PathBuf::from(home).join("AppData").join("LocalLow").join("Aniimo").join("Aniimo"));
    }
    out
}

/// The install folder named in the game's log ("... at path C:/.../Aniimo/Aniimo_Data/UnitySubsystems").
fn install_from_logs(settings: &Value) -> Option<PathBuf> {
    for dir in player_log_dirs(settings) {
        for name in ["Player.log", "Player-prev.log"] {
            // only the start of the log (it can be hundreds of MB), read leniently (it isn't always clean UTF-8)
            let Ok(file) = fs::File::open(dir.join(name)) else { continue };
            let mut head = Vec::new();
            use std::io::Read;
            if file.take(256 * 1024).read_to_end(&mut head).is_err() {
                continue;
            }
            let text = String::from_utf8_lossy(&head);
            for line in text.lines().take(200) {
                if let Some(at) = line.find("Aniimo_Data") {
                    let start = line[..at].rfind(|c: char| c == ' ' || c == '[' || c == '\'').map_or(0, |i| i + 1);
                    let root = PathBuf::from(line[start..at].trim_end_matches(['/', '\\']));
                    if root.join("Aniimo.exe").exists() || root.join("Pictures").exists() {
                        return Some(root);
                    }
                }
            }
        }
    }
    None
}

/// The running game's own folder (its window's process), when the log doesn't say.
fn install_from_process() -> Option<PathBuf> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let hwnd = crate::capture::find_game();
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(handle);
        ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])).parent().map(Path::to_path_buf)).flatten()
    }
}

/// `<yy-mm-dd>\<yyyymmddHHMMSS><id>.png` -> the photo's id and when it was taken (local time, roughly: no zone).
fn from_file_name(path: &Path) -> Option<(String, Option<i64>)> {
    let stem = path.file_stem()?.to_str()?;
    if stem.len() < 22 || !stem[..14].bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let id = stem[14..].to_string();
    let n = |a: usize, b: usize| stem[a..b].parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (n(0, 4)?, n(4, 6)?, n(6, 8)?, n(8, 10)?, n(10, 12)?, n(12, 14)?);
    // days from 1970-01-01 (civil calendar), good enough to order photos without their record
    let (yy, mm) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5 + d - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    Some((id, Some(days * 86400 + h * 3600 + mi * 60 + s)))
}

/// The game's photos, newest first: {folder, photos: [{id, file, gameUid, takenAt, sceneId, pos, aniimo}]}.
// (async): on a worker thread, not the window's (it showed "Not responding" while this ran)
#[tauri::command(async)]
pub fn game_photos(app: AppHandle) -> Value {
    let (folder, found) = scan(&crate::read_settings());
    if let Some(folder) = folder.filter(|f| f.is_dir()) {
        let _ = app.asset_protocol_scope().allow_directory(&folder, true);    // the UI may show this folder's pictures
    }
    found
}

fn scan(settings: &Value) -> (Option<PathBuf>, Value) {
    let Some(install) = install_from_logs(settings).or_else(install_from_process) else {
        return (None, json!({ "folder": null, "photos": [] }));
    };
    let folder = install.join("Pictures");
    if !folder.is_dir() {
        return (None, json!({ "folder": folder.to_string_lossy(), "photos": [] }));
    }

    // every PNG in the dated folders
    let mut photos: HashMap<String, Value> = HashMap::new();
    for day in fs::read_dir(&folder).into_iter().flatten().flatten() {
        if !day.path().is_dir() {
            continue;
        }
        for file in fs::read_dir(day.path()).into_iter().flatten().flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() != Some("png") {
                continue;
            }
            let Some((id, taken)) = from_file_name(&path) else { continue };
            // localTime: the time is the file name's, the game's local time read as if it were UTC (the page
            // corrects it); the game's own record below gives the real time and clears it
            photos.insert(id.clone(), json!({ "id": id, "file": path.to_string_lossy(), "gameUid": null, "takenAt": taken,
                "localTime": true, "sceneId": null, "pos": null, "aniimo": [] }));
        }
    }
    // what the game recorded for them, by UID; a photo is listed under each Aniimo in it, so entries repeat
    let meta: Value = fs::read_to_string(folder.join("PlayerPhotoMetaInfo.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    for (uid, player) in meta.as_object().into_iter().flatten() {
        for entries in player.pointer("/playerLocalPhotos").and_then(Value::as_object).into_iter().flat_map(|m| m.values()) {
            for entry in entries.as_array().into_iter().flatten() {
                let Some(id) = entry["uuid"].as_str() else { continue };            // built-in placeholders have none
                let Some(photo) = photos.get_mut(id) else { continue };
                photo["gameUid"] = json!(uid);
                if let Some(ts) = entry["ts"].as_i64() {
                    photo["takenAt"] = json!(ts);
                    photo["localTime"] = json!(false);
                }
                photo["sceneId"] = entry["sceneId"].clone();
                photo["pos"] = entry["pos"].clone();
                let mut ids: Vec<i64> = entry["templateIds"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect();
                ids.sort_unstable();
                ids.dedup();
                photo["aniimo"] = json!(ids);
            }
        }
    }
    let mut list: Vec<Value> = photos.into_values().collect();
    list.sort_by_key(|p| std::cmp::Reverse(p["takenAt"].as_i64().unwrap_or(0)));
    (Some(folder.clone()), json!({ "folder": folder.to_string_lossy(), "photos": list }))
}

