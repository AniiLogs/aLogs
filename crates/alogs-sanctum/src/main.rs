//! SanctumHelper.exe: the Lost Sanctum map over the game. Run by the aLogs app with `--hosted --parent <pid>` (it is
//! the drop-in for the C# build); standalone it runs the same way without the app's window (the C# tray icon and
//! settings window aren't part of the port: the aLogs app is the settings UI). Port of `App.xaml.cs`.
//!
//! Other arguments: `--assets <dir>` (Assets beside the exe otherwise), `--player-log <file>` (the game's own
//! otherwise), `--place-test` (placement of the on-screen map without the engine), `--challenge-test <png> <out>`
//! (does a saved screen show a challenge's "Victory Conditions"?), `--render-layout <n> <out.png>` (a layout as the
//! layout browser draws it).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use alogs_sanctum::cv::{self, Interp};
use alogs_sanctum::engine::{Engine, OverlayFrame};
use alogs_sanctum::hosted::{self, Hosted};
use alogs_sanctum::input::KeyHook;
use alogs_sanctum::layout_db::LayoutDb;
use alogs_sanctum::notices::Toasts;
use alogs_sanctum::ocr::OcrService;
use alogs_sanctum::overlay::canvas::Rgba;
use alogs_sanctum::overlay::view::difficulty_color;
use alogs_sanctum::overlay::window::OverlayWindow;
use alogs_sanctum::settings::{self, Settings};
use alogs_sanctum::{engine, lang, log, vision};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

fn arg_after(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// One Sanctum Helper at a time (`Local\SanctumHelper.Single`, as the C# build): false when another runs.
#[cfg(windows)]
fn single_instance() -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "Local\\SanctumHelper.Single".encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
        // (the handle is kept for the life of the process)
        !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

#[cfg(not(windows))]
fn single_instance() -> bool {
    true
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = hosted::assets_dir(&args);
    if args.first().map(String::as_str) == Some("--challenge-test") && args.len() >= 3 {
        let text = vision::imread(&PathBuf::from(&args[1])).map_or("(no picture)".to_string(), |shot| {
            let shot = if shot.c == 4 { cv::bgra2bgr(&shot) } else { shot };
            let f = 1080.0 / shot.h as f64;
            let small = cv::resize(&shot, 0, 0, f, f, Interp::Area);
            engine::run::challenge_text(&OcrService::new(None), &small).unwrap_or_else(|| "(none)".into())
        });
        let _ = std::fs::write(&args[2], text);
        return;
    }
    let settings = Settings::load();
    let db = match LayoutDb::load(&assets) {
        Ok(d) => Arc::new(d),
        Err(e) => {
            log::write(&format!("could not load the layout data from {}: {e}", assets.display()));
            std::process::exit(2);
        }
    };
    lang::init(&assets, &settings.language, &settings.game_language);
    if args.first().map(String::as_str) == Some("--render-layout") && args.len() >= 3 {
        if let Some(layout) = args[1].parse::<i64>().ok().and_then(|n| db.by_number(n))
            && let Some(p) = hosted::render_preview(&db, &settings, layout, 900, 600)
        {
            let _ = p.save_png(&args[2]);
        }
        return;
    }
    let settings = Arc::new(RwLock::new(settings));
    if args.iter().any(|a| a == "--place-test") {
        // placement of the on-screen map without the engine (works beside a running copy)
        let Some(mut ov) = OverlayWindow::new(db, settings) else { return };
        ov.begin_placement();
        while ov.is_placing() {
            ov.tick(Default::default());
            std::thread::sleep(Duration::from_millis(16));
        }
        return;
    }
    let hosted_mode = args.iter().any(|a| a == "--hosted");
    if !single_instance() {
        log::write(if hosted_mode { "hosted: another Sanctum Helper is already running" } else { "another Sanctum Helper is already running" });
        std::process::exit(if hosted_mode { 3 } else { 0 });
    }
    {
        let s = settings.read().unwrap();
        log::write(&format!(
            "start v{} (Rust), {} layouts, key {}, difficulty {}, language {} (game {})",
            env!("CARGO_PKG_VERSION"),
            db.layouts.len(),
            alogs_sanctum::input::key_name(s.hold_key as i32),
            s.difficulty,
            lang::code(),
            lang::game_code()
        ));
    }
    let first_run = !settings::dir().join("settings.json").exists();
    if first_run {
        settings.read().unwrap().save();
    }
    let hook = Arc::new(KeyHook::new(settings.read().unwrap().hold_key as i32));
    let Some(mut overlay) = OverlayWindow::new(db.clone(), settings.clone()) else {
        log::write("could not make the overlay window");
        std::process::exit(2);
    };
    let mut toasts = Toasts::new(settings.clone());
    let player_log = arg_after(&args, "--player-log").map(PathBuf::from);
    let mut eng = Engine::new(db.clone(), settings.clone(), hook.clone(), player_log);
    let (ftx, frames) = channel::<OverlayFrame>();
    eng.on_frame(move |f| {
        let _ = ftx.send(f);
    });
    let (ntx, notes) = channel::<(String, Option<String>)>();
    eng.on_notify(move |t, d| {
        let _ = ntx.send((t.to_string(), d.map(str::to_string)));
    });
    let (ctx, fallbacks) = channel::<()>();
    eng.on_capture_fallback(move || {
        let _ = ctx.send(());
    });
    let (ltx, languages) = channel::<()>();
    eng.on_language_detected(move || {
        let _ = ltx.send(());
    });
    eng.start();
    let mut host = if hosted_mode {
        let parent = arg_after(&args, "--parent").and_then(|p| p.parse::<u32>().ok());
        log::write("hosted by the aLogs app");
        Some(Hosted::new(settings.clone(), db.clone(), hook.clone(), parent))
    } else {
        None
    };
    let mut next_host = Instant::now();
    loop {
        let t0 = Instant::now();
        for f in frames.try_iter() {
            overlay.apply(f);
        }
        for (text, diff) in notes.try_iter() {
            let accent = diff.as_deref().map_or(Rgba::rgb(0x5C, 0xB8, 0xFF), |d| difficulty_color(Some(d)));
            toasts.notify(&text, accent, eng.status().client);
        }
        if fallbacks.try_iter().count() > 0 {
            overlay.apply_capture_mode(true);
        }
        let mut language = languages.try_iter().count() > 0;
        if let Some(h) = host.as_mut()
            && Instant::now() >= next_host
        {
            next_host = Instant::now() + Duration::from_millis(250);
            let asks = h.tick(&eng);
            if asks.quit {
                break;
            }
            if asks.position_map {
                overlay.begin_placement();
            }
            if asks.settings_changed {
                overlay.apply_capture_mode(false);
            }
            language |= asks.language_changed;
        }
        if language {
            let (al, gl) = {
                let s = settings.read().unwrap();
                (s.language.clone(), s.game_language.clone())
            };
            lang::apply(&al, &gl);
            eng.game_language_changed();
            if let Some(h) = &host {
                h.write_info();
            }
        }
        overlay.tick(eng.heading());
        toasts.tick();
        let spent = t0.elapsed();
        if spent < Duration::from_millis(16) {
            std::thread::sleep(Duration::from_millis(16) - spent);
        }
    }
    log::write("closing");
    drop(eng);
    settings.read().unwrap().save();
}
