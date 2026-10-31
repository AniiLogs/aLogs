//! Self-update. A minute after start and every 6 hours the app reads its update feed: GitHub's latest release
//! (releases/latest/download/latest.json) or the beta feed (the update-feed release's beta.json, which a release also
//! points at, so testers move on to it) when General › "Get beta updates" is on or this build is a beta itself (ahead
//! of the latest release). Checking always runs, so you're
//! always told about an update. With General › "Download updates automatically" on (the default) a newer build is
//! then downloaded in the background; off, it downloads when you choose to install it. The window shows the
//! download's progress.
//!
//! Nothing installs without a click: "Install now" closes aLogs (the meter saves first) and runs the installer with
//! only its progress bar (no pages, no questions), which opens aLogs again when done; "Next time aLogs opens"
//! installs it the same way at the next start, before anything else runs.
//!
//! Storage: the downloaded installer is one fixed file, %LOCALAPPDATA%\aLogs\update\aLogs-update.exe, with
//! update.json (its version and signature) next to it. A newer download overwrites it, and it's deleted once the
//! installed app is that version or newer, so updates can't pile up. Its signature is checked against the public key
//! in tauri.conf.json when it's downloaded and again right before it runs (the private key never leaves the PC that
//! publishes).
//!
//! Builds of one version are told apart by their RC number: each build carries it in build.json, and the feeds
//! give their version as "0.1.28+rc.9", so a beta updated in place (same version, newer RC) is still an update.
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

const RELEASE_FEED: &str = "https://github.com/AniiLogs/aniilogs.github.io/releases/latest/download/latest.json";
const BETA_FEED: &str = "https://github.com/AniiLogs/aniilogs.github.io/releases/download/update-feed/beta.json";
const BUILD: &str = include_str!("../build.json");

/// The newer build the last check found (the download itself lives on disk, see update_dir).
#[derive(Default)]
pub struct Pending(Mutex<Option<tauri_plugin_updater::Update>>);

/// This build's RC number (0 for a build made without tauri-build.mjs).
pub fn build_rc() -> u64 {
    serde_json::from_str::<Value>(BUILD).ok().and_then(|v| v["rc"].as_u64()).unwrap_or(0)
}

/// This build's public beta number (B2: 2), 0 for a build from before betas were numbered.
pub fn build_beta() -> u64 {
    serde_json::from_str::<Value>(BUILD).ok().and_then(|v| v["beta"].as_u64()).unwrap_or(0)
}

/// What players call this build: "B2", or "RC53" for one from before betas were numbered.
pub fn build_name() -> String {
    match (build_beta(), build_rc()) {
        (0, 0) => String::new(),
        (0, rc) => format!("RC{rc}"),
        (beta, _) => format!("B{beta}"),
    }
}

fn feed_rc(v: &semver::Version) -> u64 {
    v.build.as_str().strip_prefix("rc.").and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// (major, minor, patch, rc) of a feed version ("0.1.28+rc.9").
fn rank(version: &str) -> Option<(u64, u64, u64, u64)> {
    let v = semver::Version::parse(version).ok()?;
    Some((v.major, v.minor, v.patch, feed_rc(&v)))
}

fn this_build(app: &AppHandle) -> (u64, u64, u64, u64) {
    let v = &app.package_info().version;
    (v.major, v.minor, v.patch, build_rc())
}

fn update_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join(super::APP_DIR).join("update")
}

fn saved() -> Option<Value> {
    let info: Value = serde_json::from_str(&std::fs::read_to_string(update_dir().join("update.json")).ok()?).ok()?;
    update_dir().join("aLogs-update.exe").exists().then_some(info)
}

fn save_info(info: &Value) {
    let _ = std::fs::write(update_dir().join("update.json"), serde_json::to_string_pretty(info).unwrap_or_default());
}

fn auto_download() -> bool {
    super::read_settings().pointer("/app/auto_download").and_then(Value::as_bool).unwrap_or(true)
}

/// The installer on disk, checked against the public key in tauri.conf.json (minisign, like the updater itself).
fn verified(app: &AppHandle, info: &Value) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let b64 = |s: &str| -> Result<String, String> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(s).map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|e| e.to_string())
    };
    let pubkey = app.config().plugins.0.get("updater").and_then(|u| u["pubkey"].as_str()).ok_or("no update key in this build")?;
    let key = minisign_verify::PublicKey::decode(&b64(pubkey)?).map_err(|e| e.to_string())?;
    let sig = minisign_verify::Signature::decode(&b64(info["signature"].as_str().unwrap_or(""))?).map_err(|e| e.to_string())?;
    let data = std::fs::read(update_dir().join("aLogs-update.exe")).map_err(|e| e.to_string())?;
    key.verify(&data, &sig, true).map_err(|_| "the downloaded update doesn't match its signature".to_string())?;
    Ok(data)
}

/// What the window shows about the update found, if any.
fn describe(app: &AppHandle) -> Option<Value> {
    // the feed also says which beta it is (B3), for the sidebar; older feeds don't
    let found_beta = app.state::<Pending>().0.lock().unwrap().as_ref().and_then(|u| u.raw_json["beta"].as_u64());
    let found = app.state::<Pending>().0.lock().unwrap().as_ref().map(|u| u.version.clone());
    let disk = saved();
    let version = found.or_else(|| disk.as_ref().and_then(|d| d["version"].as_str().map(String::from)))?;
    let r = rank(&version)?;
    let on_disk = disk.as_ref().is_some_and(|d| d["version"].as_str() == Some(version.as_str()));
    Some(json!({
        "version": format!("{}.{}.{}", r.0, r.1, r.2), "rc": r.3, "beta": found_beta, "downloaded": on_disk,
        "next_start": on_disk && disk.as_ref().is_some_and(|d| d["next_start"].as_bool() == Some(true)),
    }))
}

/// Downloads the update found into the one fixed file (replacing an older one), with progress for the window.
async fn download(app: &AppHandle) -> Result<(), String> {
    let pending = app.state::<Pending>().0.lock().unwrap().clone();
    let Some(update) = pending else {
        // nothing found this session (offline, or the first check hasn't run): a newer one already downloaded is fine
        let newer_on_disk = saved().and_then(|d| d["version"].as_str().and_then(rank)).is_some_and(|r| r > this_build(app));
        return if newer_on_disk { Ok(()) } else { Err("No update was found.".into()) };
    };
    if saved().is_some_and(|d| d["version"].as_str() == Some(update.version.as_str())) {
        return Ok(());
    }
    let (mut done, mut shown) = (0u64, -1i64);
    let progress = app.clone();
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk as u64;
                let pct = total.map_or(-1, |t| (done * 100 / t.max(1)) as i64);
                if pct != shown {
                    shown = pct;
                    let _ = progress.emit("update-progress", json!({ "done": done, "total": total, "percent": pct }));
                }
            },
            || {},
        )
        .await
        .map_err(|e| format!("The download failed: {e}"))?;   // the updater checked its signature here
    std::fs::create_dir_all(update_dir()).map_err(|e| e.to_string())?;
    let part = update_dir().join("aLogs-update.exe.part");
    std::fs::write(&part, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&part, update_dir().join("aLogs-update.exe")).map_err(|e| e.to_string())?;
    save_info(&json!({ "version": update.version, "signature": update.signature, "next_start": false }));
    Ok(())
}

/// One feed: the build it offers if that's newer than this one (None when it isn't, or when the feed isn't published
/// yet), and the version the feed gives either way.
async fn read_feed(app: &AppHandle, url: &str) -> Result<(Option<tauri_plugin_updater::Update>, Option<(u64, u64, u64, u64)>), String> {
    let have = this_build(app);
    let offered = std::sync::Arc::new(Mutex::new(None));
    let seen = offered.clone();
    let updater = app
        .updater_builder()
        .endpoints(vec![url.parse().map_err(|e| format!("{e}"))?])
        .map_err(|e| e.to_string())?
        .version_comparator(move |_, remote| {
            let r = rank(&remote.version.to_string());
            *seen.lock().unwrap() = r;
            r.is_some_and(|r| r > have)
        })
        .build()
        .map_err(|e| e.to_string())?;
    let found = match updater.check().await {
        Ok(found) => found,
        Err(tauri_plugin_updater::Error::ReleaseNotFound) => None,   // not published yet: nothing to update to
        Err(e) => return Err(e.to_string()),
    };
    let version = *offered.lock().unwrap();
    Ok((found, version))
}

/// Looks for a newer build, and downloads it when that's allowed. Returns what the window shows, or None when this
/// is the newest. Betas come with General › "Get beta updates", and also whenever this build is ahead of the latest
/// release (a beta installed by hand, or before any release has self-update): a beta tester keeps getting betas
/// until the release catches up, then follows releases.
pub async fn check(app: &AppHandle) -> Result<Option<Value>, String> {
    let opted_in = super::read_settings().pointer("/app/beta_updates").and_then(Value::as_bool).unwrap_or(false);
    let (from_release, release) = read_feed(app, RELEASE_FEED).await?;
    let ahead = release.map_or(true, |r| this_build(app) > r);
    let found = if opted_in || ahead {
        let (from_beta, beta) = read_feed(app, BETA_FEED).await?;
        if beta.is_some() && beta >= release { from_beta } else { from_release.or(from_beta) }
    } else {
        from_release
    };
    *app.state::<Pending>().0.lock().unwrap() = found.clone();
    if found.is_none() {
        return Ok(describe(app));                        // a download kept for "next time" still counts
    }
    if auto_download() {
        download(app).await?;
    }
    let info = describe(app);
    let _ = app.emit("update-ready", info.clone());
    Ok(info)
}

/// Installs the downloaded update the way Discord does: aLogs closes into a small "Updating aLogs" window (a copy of
/// this program, outside the install folder so it can be replaced, named so the installer's "is aLogs running?"
/// check doesn't stop it), which runs the installer silently (/S, no Windows installer pages), as an update
/// (/UPDATE) that opens aLogs again when it's done (/R). The meter saves first. Without the copy, the installer's
/// own progress window as before (/P).
fn run_installer(app: &AppHandle) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let info = saved().ok_or("The update isn't downloaded.")?;
    verified(app, &info)?;
    app.state::<super::Meter>().quitting.store(true, std::sync::atomic::Ordering::SeqCst);
    for (_, window) in app.webview_windows() {
        let _ = window.hide();
    }
    super::signal_meter_stop(true);
    let installer = update_dir().join("aLogs-update.exe");
    let label = match info["beta"].as_u64() {
        Some(b) => format!("{} B{b}", info["version"].as_str().unwrap_or("")),
        None => info["version"].as_str().unwrap_or("").split('+').next().unwrap_or("").to_string(),
    };
    let splash = splash_copy();
    let started = match &splash {
        Some(exe) => std::process::Command::new(exe)
            .arg("--update-splash").arg(&installer).arg(&label).args(std::env::current_exe().ok())
            .creation_flags(0x0000_0008)                 // DETACHED_PROCESS: it outlives aLogs
            .spawn(),
        None => std::process::Command::new(&installer)
            .args(["/P", "/UPDATE", "/R"])
            .creation_flags(0x0000_0008)
            .spawn(),
    };
    started.map_err(|e| format!("Couldn't start the update: {e}"))?;
    std::process::exit(0);
}

/// This program copied to %TEMP%\aLogs-updating\aLogs-updating.exe for the updating window (None if it can't be).
fn splash_copy() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("aLogs-updating");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let exe = dir.join("aLogs-updating.exe");
    std::fs::copy(std::env::current_exe().ok()?, &exe).ok()?;
    Some(exe)
}

/// The updating window (main.rs --update-splash <installer> <label> <installed aLogs>): shown at once, the installer runs silently
/// behind it, then it says "Starting aLogs…" while the installer opens the new aLogs, and closes. It stays up at least
/// 2.5 s so it never just flickers. If the installer fails, it says so and opens the installed aLogs as it was.
pub fn splash(context: tauri::Context<tauri::Wry>, args: Vec<String>) {
    let installer = args.first().cloned().unwrap_or_default();
    let label = args.get(1).cloned().unwrap_or_default();
    let installed = args.get(2).map(PathBuf::from);
    let result = tauri::Builder::default()
        .setup(move |app| {
            let to: String = label.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == ' ').collect();
            let url = format!("updating.html?to={}", to.replace(' ', "%20"));
            tauri::WebviewWindowBuilder::new(app, "updating", tauri::WebviewUrl::App(url.into()))
                .title("Updating aLogs")
                .inner_size(340.0, 400.0)
                .resizable(false)
                .decorations(false)
                .transparent(false)
                .center()
                .always_on_top(true)
                .build()?;
            let handle = app.handle().clone();
            let installed = installed.clone();
            let installer = installer.clone();
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                std::thread::sleep(std::time::Duration::from_millis(400));     // the window draws first
                let _ = handle.emit("update-step", "installing");
                let ok = std::process::Command::new(&installer)
                    .args(["/S", "/UPDATE", "/R"])
                    .status()
                    .is_ok_and(|s| s.success());
                let _ = handle.emit("update-step", if ok { "starting" } else { "failed" });
                let shown = started.elapsed().as_millis() as u64;
                let wait = if ok { 2500u64.saturating_sub(shown).max(1200) } else { 6000 };
                std::thread::sleep(std::time::Duration::from_millis(wait));
                if !ok {
                    if let Some(exe) = &installed {
                        let _ = std::process::Command::new(exe).spawn();       // aLogs as it was
                    }
                }
                handle.exit(0);
            });
            Ok(())
        })
        .run(context);
    if let Err(e) = result {
        eprintln!("updating window: {e}");
        let _ = std::process::Command::new(&args.first().cloned().unwrap_or_default()).args(["/P", "/UPDATE", "/R"]).spawn();
    }
}

/// At start, before the meter or the window: removes an update that's installed now, and installs one that was set
/// for "next time aLogs opens". Also clears the updater's own temporary copies.
pub fn at_start(app: &AppHandle) {
    std::thread::spawn(clean_installer_copies);
    let Some(info) = saved() else {
        let _ = std::fs::remove_dir_all(update_dir());   // half a download, or nothing
        return;
    };
    let newer = info["version"].as_str().and_then(rank).is_some_and(|r| r > this_build(app));
    if !newer {
        let _ = std::fs::remove_dir_all(update_dir());   // installed (or older): the copy isn't needed any more
    } else if info["next_start"].as_bool() == Some(true) {
        let mut once = info.clone();
        once["next_start"] = json!(false);               // tried once: if the installer fails, aLogs still opens next time
        save_info(&once);
        if let Err(e) = run_installer(app) {
            eprintln!("update at start: {e}");
        }
    }
}

/// Installer copies the updater plugin itself may leave in %TEMP% (aLogs-<version>-updater-*), and the updating
/// window's copy of aLogs (aLogs-updating): gone after 10 minutes.
fn clean_installer_copies() {
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let old = entry.metadata().and_then(|m| m.modified()).map_or(false, |t| t < cutoff);
        if (name == "aLogs-updating" || name.starts_with("aLogs-") && name.contains("-updater-")) && entry.path().is_dir() && old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Starts the checks: a minute after start (the meter and window come first), then every 30 minutes (was 6 hours:
/// aLogs left open all day didn't notice a beta published that afternoon).
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(60));
        loop {
            if let Err(e) = tauri::async_runtime::block_on(check(&app)) {
                eprintln!("update check: {e}");
            }
            std::thread::sleep(std::time::Duration::from_secs(30 * 60));
        }
    });
}

/// This build's RC number (log reports and test results: every build, counting up).
#[tauri::command]
pub fn build_number() -> u64 {
    build_rc()
}

/// This build's name for About ("0.1.28 · B2"), so everyone can tell exactly which build they're on.
#[tauri::command]
pub fn build_label() -> String {
    build_name()
}

#[tauri::command]
pub fn update_state(app: AppHandle) -> Option<Value> {
    describe(&app)
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<Value>, String> {
    check(&app).await
}

/// "Install now": downloads it if needed (with progress), then closes aLogs and installs; aLogs opens again after.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    download(&app).await?;
    run_installer(&app)
}

/// "Next time aLogs opens": downloads it if needed, then leaves it to install at the next start.
#[tauri::command]
pub async fn install_update_later(app: AppHandle) -> Result<Option<Value>, String> {
    download(&app).await?;
    let mut info = saved().ok_or("The update isn't downloaded.")?;
    info["next_start"] = json!(true);
    save_info(&info);
    Ok(describe(&app))
}

#[cfg(test)]
mod tests {
    use super::feed_rc;

    fn newer(remote: &str, local: &str, local_rc: u64) -> bool {
        let (r, l) = (semver::Version::parse(remote).unwrap(), semver::Version::parse(local).unwrap());
        (r.major, r.minor, r.patch, feed_rc(&r)) > (l.major, l.minor, l.patch, local_rc)
    }

    #[test]
    fn builds_of_one_version_are_told_apart() {
        assert!(newer("0.1.28+rc.9", "0.1.28", 8));        // a beta updated in place: same version, newer build
        assert!(!newer("0.1.28+rc.8", "0.1.28", 8));       // the same build: nothing to do
        assert!(!newer("0.1.28+rc.7", "0.1.28", 8));       // an older build is never offered
        assert!(newer("0.1.29+rc.1", "0.1.28", 12));       // a new version wins whatever the build numbers
        assert!(!newer("0.1.27+rc.30", "0.1.28", 1));
        assert!(newer("0.1.28", "0.1.27", 5));             // a feed without a build number still works by version
    }
}

#[cfg(test)]
mod signature_check {
    /// Run by hand before publishing: SIGCHECK_EXE=<installer> cargo test --release signature_check -- --ignored
    #[test]
    #[ignore]
    fn the_build_verifies_with_the_app_key() {
        use base64::Engine;
        let b64 = |s: &str| String::from_utf8(base64::engine::general_purpose::STANDARD.decode(s.trim()).unwrap()).unwrap();
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let key = minisign_verify::PublicKey::decode(&b64(conf["plugins"]["updater"]["pubkey"].as_str().unwrap())).unwrap();
        let exe = std::env::var("SIGCHECK_EXE").unwrap();
        let sig = minisign_verify::Signature::decode(&b64(&std::fs::read_to_string(format!("{exe}.sig")).unwrap())).unwrap();
        key.verify(&std::fs::read(&exe).unwrap(), &sig, true).expect("signature doesn't verify");
    }
}
