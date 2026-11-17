//! Hosted mode (`--hosted`): Sanctum Helper as a module of the aLogs app, which shows its options and status in its own
//! window. settings.json stays the one settings file (aLogs edits it, this reloads it live); hosted\info.json (once,
//! and when the language changes) lists what the options can be; hosted\status.json says what Sanctum Helper sees
//! (on a change, looked at every second, and at least every 3 s); aLogs drops hosted\cmd-*.json files for actions:
//! new-run, position-map, capture-key, capture-pad, clear-pad, render-layout, open-logs, quit. With `--parent <pid>`
//! it quits when the app does. Port of `Hosted.cs`. Dev switches aren't part of the port: none are listed or on.

use crate::engine::{Engine, Status};
use crate::input::{self, Gamepad, KeyHook};
use crate::lang;
use crate::layout_db::{Layout, LayoutDb};
use crate::overlay::canvas::Canvas;
use crate::overlay::view::{MINI_MAP_STYLES, OverlayView};
use crate::settings::{self, Settings};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub fn dir() -> PathBuf {
    settings::dir().join("hosted")
}

/// What the main loop must do after a hosted tick.
#[derive(Debug, Default, PartialEq)]
pub struct Asks {
    pub quit: bool,
    pub position_map: bool,
    pub language_changed: bool,
    pub settings_changed: bool,
}

pub struct Hosted {
    s: Arc<RwLock<Settings>>,
    db: Arc<LayoutDb>,
    hook: Arc<KeyHook>,
    parent: Option<u32>,
    settings_stamp: Option<std::time::SystemTime>,
    settings_changed_at: Option<Instant>,
    capturing: Option<&'static str>,
    capture_started: Instant,
    keys: Receiver<i32>,
    pad: Option<(Gamepad, i32, Instant)>,
    last_status: Option<String>,
    last_status_at: Option<Instant>,
    ticks: u64,
}

fn write_text(name: &str, text: &str) {
    let path = dir().join(name);
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, text) {
        crate::log::write(&format!("hosted {name}: {e}"));
        return;
    }
    // the app may be reading the file at that moment: try again shortly
    for attempt in 1.. {
        match std::fs::rename(&tmp, &path) {
            Ok(()) => return,
            Err(_) if attempt < 5 => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                crate::log::write(&format!("hosted {name}: {e}"));
                return;
            }
        }
    }
}

fn json_text(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return false;
        }
        let r = WaitForSingleObject(h, 0);
        CloseHandle(h);
        r == WAIT_TIMEOUT
    }
}

#[cfg(not(windows))]
fn alive(_: u32) -> bool {
    true
}

fn settings_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(settings::dir().join("settings.json")).and_then(|m| m.modified()).ok()
}

impl Hosted {
    pub fn new(s: Arc<RwLock<Settings>>, db: Arc<LayoutDb>, hook: Arc<KeyHook>, parent: Option<u32>) -> Hosted {
        let _ = std::fs::create_dir_all(dir());
        if let Ok(rd) = std::fs::read_dir(dir()) {
            for e in rd.filter_map(Result::ok) {
                let n = e.file_name().to_string_lossy().to_string();
                if n.starts_with("cmd-") && n.ends_with(".json") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let parent = parent.filter(|p| alive(*p));
        let (tx, keys) = channel();
        hook.on_any_key(move |vk| {
            let _ = tx.send(vk);
        });
        let h = Hosted {
            s,
            db,
            hook,
            parent,
            settings_stamp: settings_mtime(),
            settings_changed_at: None,
            capturing: None,
            capture_started: Instant::now(),
            keys,
            pad: None,
            last_status: None,
            last_status_at: None,
            ticks: 0,
        };
        h.write_info();
        h
    }

    /// Every 250 ms: the parent, settings changed by aLogs, commands, a key being picked, the status every second.
    pub fn tick(&mut self, engine: &Engine) -> Asks {
        let mut asks = Asks::default();
        if self.parent.is_some_and(|p| !alive(p)) {
            asks.quit = true;
            return asks;
        }
        // settings changed on disk (by aLogs): wait for the write to settle, then apply
        let stamp = settings_mtime();
        if stamp != self.settings_stamp {
            self.settings_stamp = stamp;
            self.settings_changed_at = Some(Instant::now());
        }
        if self.settings_changed_at.is_some_and(|t| t.elapsed() > Duration::from_millis(200)) {
            self.settings_changed_at = None;
            self.reload(engine, &mut asks);
        }
        self.run_commands(engine, &mut asks);
        self.keys_picked();
        self.pad_picking();
        if self.capturing == Some("key") && self.capture_started.elapsed() > Duration::from_secs(15) {
            self.capturing = None;
            self.hook.set_capturing(false);
        }
        self.ticks += 1;
        if self.ticks % 4 == 0 {
            self.write_status(engine);
        }
        asks
    }

    fn reload(&mut self, engine: &Engine, asks: &mut Asks) {
        let Ok(text) = std::fs::read_to_string(settings::dir().join("settings.json")) else { return };
        if Some(&text) == settings::last_json().as_ref() {
            return; // our own save
        }
        let fresh = Settings::load();
        let (old_lang, old_game, old_diff, old_layout);
        {
            let mut s = self.s.write().unwrap();
            (old_lang, old_game, old_diff, old_layout) = (s.language.clone(), s.game_language.clone(), s.difficulty.clone(), s.forced_layout);
            *s = fresh;
        }
        let s = self.s.read().unwrap().clone();
        self.hook.set_key(s.hold_key as i32);
        if s.language != old_lang || s.game_language != old_game {
            asks.language_changed = true;
        }
        if s.difficulty != old_diff || s.forced_layout != old_layout {
            engine.reset_run();
        }
        asks.settings_changed = true;
        crate::log::write("hosted: settings reloaded");
        self.write_info(); // names follow the app language
        self.write_status(engine);
    }

    fn run_commands(&mut self, engine: &Engine, asks: &mut Asks) {
        let Ok(rd) = std::fs::read_dir(dir()) else { return };
        let mut files: Vec<PathBuf> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("cmd-")) && p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for f in files {
            let text = match std::fs::read_to_string(&f) {
                Ok(t) => t,
                Err(_) => continue, // still being written
            };
            let _ = std::fs::remove_file(&f);
            let Ok(cmd) = serde_json::from_str::<Value>(&text) else { continue };
            self.run(&cmd, engine, asks);
        }
    }

    fn run(&mut self, cmd: &Value, engine: &Engine, asks: &mut Asks) {
        match cmd["command"].as_str().unwrap_or("") {
            "new-run" => engine.reset_run(),
            "quit" => asks.quit = true,
            "position-map" => asks.position_map = true,
            // the mouse is watched while a key is picked: a mouse button can be one
            "capture-key" => {
                self.capturing = Some("key");
                self.capture_started = Instant::now();
                self.hook.set_capturing(true);
                while self.keys.try_recv().is_ok() {}
            }
            "capture-pad" => self.capture_pad(),
            "clear-pad" => {
                let mut s = self.s.write().unwrap();
                s.hold_button = 0;
                s.save();
            }
            "open-logs" => {
                let d = settings::dir();
                let _ = std::fs::create_dir_all(&d);
                let _ = std::process::Command::new("explorer").arg(&d).spawn();
            }
            "render-layout" => {
                let w = cmd["width"].as_i64().unwrap_or(900);
                let h = cmd["height"].as_i64().unwrap_or(600);
                if let Some(n) = cmd["layout"].as_i64() {
                    self.render_layout(n, w, h);
                }
            }
            _ => {}
        }
        self.write_status(engine);
    }

    fn keys_picked(&mut self) {
        while let Ok(vk) = self.keys.try_recv() {
            if self.capturing != Some("key") {
                continue;
            }
            self.capturing = None;
            self.hook.set_capturing(false);
            if vk == 0x1B {
                continue; // Esc cancels
            }
            self.hook.set_key(vk);
            let mut s = self.s.write().unwrap();
            s.hold_key = i64::from(vk);
            s.save();
        }
    }

    /// Waits for a newly pressed controller button (Esc cancels, 15 s timeout).
    fn capture_pad(&mut self) {
        if self.pad.is_some() {
            return;
        }
        let mut pad = Gamepad::default();
        pad.poll();
        let before = pad.buttons;
        self.pad = Some((pad, before, Instant::now()));
        self.capturing = Some("pad");
    }

    fn pad_picking(&mut self) {
        let Some((pad, before, started)) = self.pad.as_mut() else { return };
        pad.poll();
        let fresh = pad.buttons & !*before;
        *before &= pad.buttons;
        let esc = input::key_held(0x1B);
        if fresh == 0 && !esc && started.elapsed() < Duration::from_secs(15) {
            return;
        }
        if fresh != 0 {
            let mut bit = fresh & fresh.wrapping_neg();
            if fresh & input::LEFT_TRIGGER != 0 {
                bit = input::LEFT_TRIGGER;
            }
            if fresh & input::RIGHT_TRIGGER != 0 {
                bit = input::RIGHT_TRIGGER;
            }
            let mut s = self.s.write().unwrap();
            s.hold_button = i64::from(bit);
            s.save();
        }
        self.pad = None;
        self.capturing = None;
    }

    /// The layout browser's picture of one layout, drawn as the settings preview draws it: hosted\layout-<n>.png.
    pub fn render_layout(&self, number: i64, width: i64, height: i64) {
        let Some(l) = self.db.by_number(number) else { return };
        let (w, h) = (width.clamp(200, 2000) as i32, height.clamp(150, 1500) as i32);
        let s = self.s.read().unwrap().clone();
        if let Some(p) = render_preview(&self.db, &s, l, w, h) {
            let path = dir().join(format!("layout-{number}.png"));
            let tmp = dir().join(format!("layout-{number}.png.tmp"));
            if p.save_png(&tmp).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    pub fn write_info(&self) {
        let db = &self.db;
        let mut layouts: Vec<&Layout> = db.layouts.iter().filter(|l| !l.difficulties.is_empty()).collect();
        layouts.sort_by_key(|l| l.number);
        let info = json!({
            "version": env!("CARGO_PKG_VERSION"),
            "gameDataVersion": db.game_data_version,
            "dev": [],
            "difficulties": db.difficulties.iter().map(|d| json!({"value": d, "name": lang::difficulty(Some(d))})).collect::<Vec<_>>(),
            "styles": MINI_MAP_STYLES.iter().map(|m| json!({"value": m.0, "name": lang::t(m.1), "description": lang::t(m.2)})).collect::<Vec<_>>(),
            "languages": lang::LANGUAGES.iter().map(|l| json!({"code": l.0, "name": l.1})).collect::<Vec<_>>(),
            "gameLanguage": lang::name_of(&lang::game_code()),
            "layouts": layouts.iter().map(|l| json!({
                "number": l.number,
                "title": l.display_title(),
                "compass": lang::t(l.compass()),
                "difficulties": l.difficulties.iter().map(|d| lang::difficulty(Some(d))).collect::<Vec<_>>(),
                "keyRooms": l.key_rooms().count(),
                "nests": l.nest_count(),
            })).collect::<Vec<_>>(),
        });
        write_text("info.json", &json_text(&info));
    }

    fn status(&self, st: &Status, time: i64) -> Value {
        let s = self.s.read().unwrap();
        let effective = if s.difficulty == "Auto" { st.detected_difficulty.clone() } else { Some(s.difficulty.clone()) };
        let layout = st.layout.and_then(|n| self.db.by_number(n)).map(|l| {
            json!({"number": l.number, "title": l.display_title(), "compass": lang::t(l.compass()), "source": lang::t(&st.layout_source)})
        });
        json!({
            "time": time,
            "pid": std::process::id(),
            "devOn": [],
            "devPersistent": [],
            "devChanged": [],
            "gameFound": st.game_found,
            "inRun": st.game_found && st.in_run,
            "difficulty": effective.map(|d| lang::difficulty(Some(&d))),
            "difficultySource": if s.difficulty == "Auto" { if st.detected_difficulty.is_some() { "party screen" } else { "open the party screen once" } } else { "set by hand" },
            "layout": layout,
            "keyName": input::key_name(s.hold_key as i32),
            "padName": lang::t(&input::button_name(s.hold_button as i32)),
            "capturing": self.capturing,
            "ocrLanguage": st.ocr_language,
            "ocrMissingLanguage": st.ocr_missing_language,
            "windowCapture": st.using_window_capture,
            "sharedCapture": crate::shared_capture::in_use(),
            "lastMatch": st.last_match_info,
        })
    }

    /// Written when something in it changed, and at least every 3 s so aLogs sees it's alive.
    pub fn write_status(&mut self, engine: &Engine) {
        let st = engine.status();
        let same = json_text(&self.status(&st, 0));
        if self.last_status.as_ref() == Some(&same) && self.last_status_at.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
            return;
        }
        self.last_status = Some(same);
        self.last_status_at = Some(Instant::now());
        write_text("status.json", &json_text(&self.status(&st, chrono::Utc::now().timestamp_millis())));
    }
}

/// A layout drawn as the settings window's preview draws it (no "not lined up" hint).
pub fn render_preview(db: &Arc<LayoutDb>, s: &Settings, l: &Layout, w: i32, h: i32) -> Option<tiny_skia::Pixmap> {
    let mut view = OverlayView::new(db.clone());
    view.preview = true;
    let f = crate::engine::OverlayFrame { client: (0, 0, w, h), visible: true, layout: Some(l.number), difficulty: Some(l.difficulties.join(" / ")), ..Default::default() };
    let mut cv = Canvas::new(w as u32, h as u32)?;
    view.render(&mut cv, s, &f, None, None, None);
    Some(cv.finish())
}

/// Where the assets are: `--assets <dir>`, else Assets beside the exe.
pub fn assets_dir(args: &[String]) -> PathBuf {
    if let Some(i) = args.iter().position(|a| a == "--assets")
        && let Some(d) = args.get(i + 1)
    {
        return PathBuf::from(d);
    }
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default().join("Assets")
}
