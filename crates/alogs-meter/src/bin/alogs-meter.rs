//! The boss DPS meter, as a program: a drop-in for the Python meter (bossbar.exe) the aLogs app starts. Port of
//! `main`, `Meter.run`, `write_status`, `plugin_status`, `setup_log` and `console_out` in aLogs B16 `bossbar/live.py`
//! and `core.py`. Console output only for now (the overlay windows aren't ported yet: TODO(port) dps_overlay.py).
//!
//!   alogs-meter [--interval S] [--title T] [--duration N] [--level N] [--boss-id ID] [--player-log PATH]
//!               [--no-overlay] [--no-debug-frames] [--no-submit] [--res DIR]

use alogs_capture::win;
use alogs_capture::SharedCapture;
use alogs_meter::env::Env;
use alogs_meter::meter::{Args, Frames, Meter, Out};
use alogs_meter::submit::Submitter;
use alogs_pil::Image;
use alogs_protocol::settings;
use alogs_pyval::{Value, dict};
use alogs_reader::ocr::{NoOcr, Ocr, Reader};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const AWAY_INTERVAL: f64 = 0.25;
const IDLE_INTERVAL: f64 = 0.25;

/// logs/meter.log, rotated at 5 MB (4 files), lines as the Python meter writes them.
struct FileLog {
    path: PathBuf,
    file: Mutex<std::fs::File>,
}

impl log::Log for FileLog {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, r: &log::Record) {
        let now = chrono::Local::now();
        let level = match r.level() {
            log::Level::Error => "ERROR",
            log::Level::Warn => "WARNING",
            log::Level::Info => "INFO",
            _ => "DEBUG",
        };
        let line = format!("{}.{:03} {:<5} {}\n", now.format("%Y-%m-%d %H:%M:%S"), now.timestamp_subsec_millis(), level, r.args());
        let mut f = self.file.lock().unwrap();
        let _ = f.write_all(line.as_bytes());
        if f.metadata().is_ok_and(|m| m.len() > 5 << 20) {
            for i in (1..3).rev() {
                let _ = std::fs::rename(self.path.with_extension(format!("log.{i}")), self.path.with_extension(format!("log.{}", i + 1)));
            }
            let _ = std::fs::rename(&self.path, self.path.with_extension("log.1"));
            if let Ok(n) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
                *f = n;
            }
        }
    }
    fn flush(&self) {}
}

fn setup_log(data: &std::path::Path) {
    let dir = data.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("meter.log");
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = log::set_boxed_logger(Box::new(FileLog { path, file: Mutex::new(f) }));
        log::set_max_level(log::LevelFilter::Debug);
    }
    log::info!("{} meter started {}", "=".repeat(30), "=".repeat(30));
    log::info!("alogs-meter (Rust) {} | args {}", env!("CARGO_PKG_VERSION"), std::env::args().skip(1).collect::<Vec<_>>().join(" "));
}

/// The console output (and the status line logged once a minute per message).
#[derive(Default)]
struct Console {
    last: String,
    logged: HashMap<String, f64>,
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn fmt(n: f64) -> String {
    if n >= 1e6 { format!("{:.2}M", n / 1e6) } else if n >= 1e3 { format!("{:.1}k", n / 1e3) } else { format!("{n:.0}") }
}

impl Out for Console {
    fn out(&mut self, kind: &str, data: Value) {
        match kind {
            "status" => {
                let s = data.py_str();
                let t = now();
                if s != self.last && t - self.logged.get(&s).copied().unwrap_or(0.0) >= 60.0 {
                    println!("\n{} {s}", chrono::Local::now().format("%H:%M:%S"));
                    log::info!("{s}");
                    self.logged.insert(s.clone(), t);
                }
                self.last = s;
            }
            "fight" => {
                let xn = data.get("xn").as_i64().map_or(String::new(), |x| format!("x{x}"));
                print!(
                    "\r{} Lv{} {xn} fill {:5.1}% | dmg {} | avg {}/s | 5s {}/s | {:.0}s   ",
                    data.get("title").py_str(),
                    data.get("level").py_str(),
                    data.get("fill").as_f64().unwrap_or(0.0) * 100.0,
                    fmt(data.get("damage").as_f64().unwrap_or(0.0)),
                    fmt(data.get("avg").as_f64().unwrap_or(0.0)),
                    fmt(data.get("recent").as_f64().unwrap_or(0.0)),
                    data.get("elapsed").as_f64().unwrap_or(0.0)
                );
                let _ = std::io::stdout().flush();
            }
            _ => {}
        }
    }
}

/// Frames from the app's shared capture.
struct Live {
    title: String,
    cap: SharedCapture,
    hp_cap: Option<SharedCapture>,
    hwnd: Option<usize>,
    look_at: f64,
}

impl Frames for Live {
    fn hp_strip(&mut self, hwnd: usize, area: (f64, f64, f64, f64)) -> Option<Image> {
        self.hp_cap.get_or_insert_with(|| SharedCapture::new(2)).read_area(hwnd, area, 1)
    }

    fn grab(&mut self, hwnd: usize, top: Option<f64>) -> Option<Image> {
        if !self.cap.app_present() {
            return None;
        }
        self.cap.read(hwnd, top)
    }
    fn window(&mut self) -> Option<usize> {
        let t = now();
        if self.hwnd.is_none() || t - self.look_at > 2.0 {
            self.look_at = t;
            self.hwnd = win::find_window(&self.title);
        }
        self.hwnd
    }
    fn frame_time(&self) -> Option<f64> {
        let t = self.cap.frame_time;
        (t != 0.0 && (0.0..1.0).contains(&(now() - t))).then_some(t)
    }
    fn switched_off(&mut self) -> bool {
        self.cap.switched_off()
    }
    fn is_foreground(&self, hwnd: usize) -> bool {
        win::is_foreground(hwnd)
    }
    fn screen_copy(&self) -> bool {
        self.cap.screen_copy
    }
    fn set_clean_until(&mut self, ms: u64) {
        self.cap.set_clean_until(ms);
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn plugin_status(m: &Meter, meter_on: bool) -> Value {
    let cfg = &m.args.cfg;
    let mut out = alogs_pyval::Dict::new();
    for (pid, p) in &m.manifest.plugins {
        if !p.get("needs").as_list().is_some_and(|n| n.iter().any(|x| x.as_str() == Some("reader"))) {
            continue;
        }
        let health = m.health.status(pid);
        let on = m.manifest.switched_on(cfg, pid, meter_on);
        let (state, why) = if health.get("failed").truthy() {
            ("failed", "stopped after repeated errors: turn it off and on to try again")
        } else if !on {
            ("off", "")
        } else if m.homeland_rest {
            ("resting", "in a homeland: nothing is read there")
        } else if m.hwnd.is_none() {
            ("waiting", "waiting for the game")
        } else if pid == "leaderboards" && !cfg.get("leaderboard").get("token").py_str().starts_with("alb_") {
            ("attention", "not signed in: sign in on its Account tab")
        } else {
            ("on", "")
        };
        out.insert(
            Value::Str(pid.clone()),
            dict([("on", Value::Bool(on)), ("state", Value::Str(state.into())), ("why", Value::Str(why.into())), ("last_error", health.get("last_error").clone())]),
        );
    }
    Value::Dict(out)
}

fn write_status(m: &Meter, sub: Option<&Submitter>, live: &Live, t: f64) {
    let f = m.fight.as_ref();
    let fight = f.map_or(Value::None, |f| {
        dict([
            ("boss", Value::Str(f.title.clone())),
            ("level", Value::Int(f.level)),
            ("segment", f.seg.map_or(Value::None, Value::Int)),
            ("damage", Value::Int(f.damage)),
            ("max_hp", Value::Int(f.max_hp)),
            ("seconds", Value::Int(f.elapsed.round_ties_even() as i64)),
            ("avg_dps", Value::Int(f.avg.round_ties_even() as i64)),
            ("recent_dps", Value::Int(f.recent.round_ties_even() as i64)),
            ("boss_of", f.boss_of().map_or(Value::None, |(a, b)| Value::List(vec![Value::Int(a), Value::Int(b)]))),
        ])
    });
    let lang = m.ocr.language();
    let status = dict([
        ("time", Value::Float(t)),
        ("pid", Value::Int(i64::from(std::process::id()))),
        ("status", Value::Str(m.status_text.clone())),
        ("game_window", Value::Bool(m.hwnd.is_some())),
        ("capture", if live.cap.source.is_empty() { Value::None } else { Value::Str(live.cap.source.clone()) }),
        ("homeland", Value::Bool(m.homeland_rest)),
        ("notices", Value::Bool(false)), // TODO(port): notice_window (Sanctum Helper's notices handed over)
        ("plugins", plugin_status(m, !m.capture_off)),
        ("frame", if m.frame_size.0 > 0 { Value::List(vec![Value::Int(m.frame_size.0 as i64), Value::Int(m.frame_size.1 as i64)]) } else { Value::None }),
        ("boss_bar", Value::Bool(m.bar.is_some())),
        ("fight", fight),
        (
            "character",
            dict([
                ("uid", m.uid.clone().map_or(Value::None, Value::Str)),
                ("name", m.character_name.clone().map_or(Value::None, Value::Str)),
                ("verified", Value::Bool(alogs_meter::team::character_verified(&m.env.data, m.uid.as_deref()))),
                ("dispute", sub.and_then(|s| s.dispute.lock().unwrap().clone()).unwrap_or(Value::None)),
                ("link", m.menu.link.as_ref().map_or(Value::None, |l| dict([
                    ("request", Value::Float(l.request)),
                    ("state", Value::Str(l.state.into())),
                    ("name", l.name.clone().map_or(Value::None, Value::Str)),
                    ("uid", l.uid.clone().map_or(Value::None, Value::Str)),
                ]))),
            ]),
        ),
        ("auto_battle_data", Value::Bool(m.auto.auto_until != 0.0)),
        (
            "leaderboard",
            dict([
                ("on", Value::Bool(sub.is_some_and(|s| s.config.lock().unwrap().is_some()))),
                ("pending", Value::Int(sub.map_or(0, |s| s.pending() as i64))),
                ("last", sub.and_then(|s| s.last_result.lock().unwrap().clone()).map_or(Value::None, |(t, s)| Value::List(vec![Value::Float(t), Value::Str(s)]))),
            ]),
        ),
        (
            "ocr",
            dict([
                ("language", lang.clone().filter(|l| !l.is_empty()).map_or(Value::None, Value::Str)),
                ("english", alogs_reader::ocr::ocr_english(lang.as_deref()).map_or(Value::None, Value::Bool)),
                ("started", Value::Bool(m.ocr.started())),
                ("switching", Value::Bool(false)),
            ]),
        ),
    ]);
    settings::write_status(&m.env.data, &status);
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_default();
    let res = arg(&argv, "--res").map(PathBuf::from).unwrap_or_else(|| exe_dir.clone());
    let data = settings::data_dir(true, &exe_dir);
    let _ = std::fs::create_dir_all(&data);
    let cfg = settings::load(&data);
    setup_log(&data);
    let shown = {
        let mut c = cfg.clone();
        if let Value::Dict(d) = &mut c
            && let Some(Value::Dict(lb)) = d.get_str("leaderboard").cloned()
        {
            let lb: alogs_pyval::Dict = lb.iter().filter(|(k, _)| k.as_str() != Some("token")).map(|(k, v)| (k.clone(), v.clone())).collect();
            d.insert("leaderboard".into(), Value::Dict(lb));
        }
        c
    };
    log::info!("data folder {}; settings {}", data.display(), shown.json_dumps()); // the Discord sign-in (token) is never logged
    let interval = arg(&argv, "--interval").and_then(|v| v.parse().ok()).unwrap_or_else(|| cfg.get("capture").get("look_interval").as_f64().unwrap_or(0.05));
    let title = arg(&argv, "--title").unwrap_or_else(|| {
        let t = cfg.get("game").get("window_title").py_str();
        if t.trim().is_empty() { "Aniimo".into() } else { t.trim().into() }
    });
    let default_log = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(r"AppData\LocalLow\Aniimo\Aniimo\Player.log");
    let player_log = arg(&argv, "--player-log").map(PathBuf::from).unwrap_or_else(|| {
        let p = cfg.get("game").get("player_log").py_str();
        if p.is_empty() { default_log } else { PathBuf::from(p) }
    });
    let args = Args {
        duration: arg(&argv, "--duration").and_then(|v| v.parse().ok()).unwrap_or(180.0),
        level: arg(&argv, "--level").and_then(|v| v.parse().ok()),
        boss_id: arg(&argv, "--boss-id"),
        replay: false,
        no_submit: argv.iter().any(|a| a == "--no-submit"),
        no_debug_frames: argv.iter().any(|a| a == "--no-debug-frames") || !cfg.get("capture").get("debug_frames").truthy(),
        no_overlay: true,
        no_auto_click: true,
        player_log: player_log.clone(),
        cfg,
    };
    let ocr: Box<dyn Reader> = match Ocr::new(&res.join("ocr-helper.exe")) {
        Ok(o) => Box::new(o),
        Err(e) => {
            log::warn!("OCR helper did not start ({e}): text is not read");
            Box::new(NoOcr)
        }
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let w = alogs_gamelog::LogWatcher::new(player_log.clone(), Box::new(alogs_gamelog::SystemClock));
        let st = stop.clone();
        std::thread::spawn(move || w.run(tx, st));
    }
    let submitter = if args.no_submit {
        None
    } else {
        let s = Submitter::new(data.clone());
        let r = s.clone();
        std::thread::spawn(move || r.run());
        Some(s)
    };
    let env = Env::new(res, data.clone());
    let mut m = Meter::new(env, args, ocr, Some(rx));
    if !m.args.no_overlay {
        // "You're being attacked" reads your HP with a helper of its own (no overlay: the alert can't show, so none)
        m.hp_ocr = Ocr::new(&m.env.res.join("ocr-helper.exe")).ok().map(|o| Box::new(o) as Box<dyn Reader>);
    }
    m.submitter = submitter.clone();
    if let Some(u) = alogs_gamelog::last_uid(&player_log) {
        m.set_uid(&u);
    }
    if let Some(u) = &m.uid {
        log::info!("player uid {u} (from the game log)");
    }
    let mut live = Live { title, cap: SharedCapture::new(0), hp_cap: None, hwnd: None, look_at: 0.0 };
    let mut console = Console::default();
    let mut last_status = 0.0;
    loop {
        let t = now();
        m.step(&mut live, &mut console);
        if t - last_status >= 1.0 {
            last_status = t;
            m.link_poll(t);
            write_status(&m, submitter.as_deref(), &live, t);
        }
        let mut iv = if m.fight.is_some() { m.fight_interval() } else { interval };
        if m.fight.is_some() && now() - m.bar_seen > 1.0 {
            iv = iv.max(0.1);
        }
        if let Some(h) = m.hwnd
            && !win::is_foreground(h)
        {
            iv = iv.max(if m.fight.is_some() { 0.5 } else { AWAY_INTERVAL });
        } else if m.idle() {
            iv = iv.max(IDLE_INTERVAL);
        }
        if m.capture_off {
            iv = iv.max(AWAY_INTERVAL);
        }
        if m.homeland_rest {
            iv = iv.max(1.0);
        }
        let nap = (iv - (now() - t)).max(0.0);
        std::thread::sleep(std::time::Duration::from_secs_f64(nap));
    }
}
