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
    audio: alogs_capture::shared::SharedAudio,
    hwnd: Option<usize>,
    look_at: f64,
    stats: CapStats,
    said_window: Option<usize>,
    last_size: (usize, usize),
    no_pic_since: f64,
    no_pic_said: f64,
}

/// How the game is captured since the last capture line.
#[derive(Default)]
struct CapStats {
    source: String,
    since: f64,
    n: u64,
    none: u64,
    ms: f64,
    max: f64,
    app_us: u64,
}

impl Frames for Live {
    fn other_overlays(&mut self) -> Vec<(i32, i32, i32, i32)> {
        self.cap.overlays(Some(alogs_capture::shared::OWNER_METER))
    }

    fn audio_want(&mut self, now: f64) {
        self.audio.want(now, 3.0);
    }

    fn audio_cue(&self) -> Option<(u32, f64, f32)> {
        self.audio.cue()
    }

    fn hp_strip(&mut self, hwnd: usize, area: (f64, f64, f64, f64)) -> Option<Image> {
        self.hp_cap.get_or_insert_with(|| SharedCapture::new(2)).read_area(hwnd, area, 1)
    }

    fn grab(&mut self, hwnd: usize, top: Option<f64>) -> Option<Image> {
        if !self.cap.app_present() {
            return None;
        }
        let t0 = std::time::Instant::now();
        let im = self.cap.read(hwnd, top);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let source = format!("shared {}", if self.cap.source.is_empty() { "capture" } else { &self.cap.source });
        if self.stats.source != source {
            log::info!("capture: {source}");
            self.stats = CapStats { source, since: now(), ..Default::default() };
        }
        match &im {
            None => self.stats.none += 1,
            Some(i) => {
                self.stats.n += 1;
                self.stats.ms += ms;
                self.stats.max = self.stats.max.max(ms);
                self.stats.app_us += u64::from(self.cap.copy_us);
                if top.is_none() && i.size() != self.last_size {
                    self.last_size = i.size();
                    log::info!("frame size {}x{} ({})", i.width, i.height, self.cap.source);
                }
            }
        }
        self.picture_gap(im.is_none(), now());
        im
    }
    fn window(&mut self) -> Option<usize> {
        let t = now();
        if self.hwnd.is_none() || t - self.look_at > 2.0 {
            self.look_at = t;
            self.hwnd = win::find_window(&self.title);
            if self.hwnd.is_none() && self.said_window.is_some() {
                log::warn!("game window closed");
                self.said_window = None;
            }
            if let Some(h) = self.hwnd
                && self.said_window != Some(h)
            {
                self.said_window = Some(h);
                log::info!("game window found: hwnd {h}, client {:?}", win::client_rect(h));
                log::info!("{}", alogs_pcstats::system_line(h)); // the PC behind this log, for lag reports
            }
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

impl Live {
    /// Pictures stopping for 2 s or more, logged with the reader's reason, again every 5 s, and when they come back.
    fn picture_gap(&mut self, missing: bool, t: f64) {
        if !missing {
            if self.no_pic_since != 0.0 && t - self.no_pic_since >= 2.0 {
                log::info!("game picture back after {:.1} s", t - self.no_pic_since);
            }
            self.no_pic_since = 0.0;
            return;
        }
        if self.no_pic_since == 0.0 {
            self.no_pic_since = t;
        }
        if t - self.no_pic_since >= 2.0 && t - self.no_pic_said >= 5.0 {
            self.no_pic_said = t;
            log::info!("no game picture for {:.1} s: {} (source {})", t - self.no_pic_since,
                if self.cap.why.is_empty() { "no reason given" } else { &self.cap.why }, if self.cap.source.is_empty() { "none" } else { &self.cap.source });
        }
    }

    /// Once a minute: the capture line, then the load line. True when it was time.
    fn minute(&mut self, t: f64) -> bool {
        let st = &mut self.stats;
        if st.source.is_empty() || t - st.since < 60.0 {
            return false;
        }
        let n = st.n.max(1) as f64;
        log::info!("capture: {}, {:.1} frames/s, {:.1} ms each (max {:.0}){}{}", st.source, st.n as f64 / (t - st.since), st.ms / n, st.max,
            if st.app_us != 0 { format!(", the app's copy {:.1} ms", st.app_us as f64 / n / 1000.0) } else { String::new() },
            if st.none != 0 { format!(", {} without a picture", st.none) } else { String::new() });
        *st = CapStats { source: std::mem::take(&mut st.source), since: t, ..Default::default() };
        true
    }
}

/// Once a minute: what the last minute cost (CPU, GPU, RAM), the frames copied and text read, and where you were.
struct Load {
    cpu: alogs_pcstats::CpuLoad,
    gpu: alogs_pcstats::GpuLoad,
    since: f64,
    discord_said: bool,
}

fn load_line(load: &mut Load, m: &mut Meter, t: f64) {
    let fmt = |v: Option<f64>| v.map_or("?".to_string(), |v| format!("{}%", Value::Float(v).py_str().trim_end_matches(".0")));
    let pid = m.hwnd.map_or(0, win::window_pid);
    let app = std::env::var("ALOGS_PARENT").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let cpu = load.cpu.sample(pid, app);
    let ours: Vec<u32> = [std::process::id(), app, alogs_pcstats::sanctum_pid()].into_iter().filter(|p| *p != 0).collect();
    let gpu = load.gpu.sample();
    let gpu_text = gpu.map_or("GPU ?".to_string(), |g| format!("GPU game {:.0}%, aLogs {:.0}%, whole card {:.0}%",
        g.by_pid.get(&pid).copied().unwrap_or(0.0), ours.iter().map(|q| g.by_pid.get(q).copied().unwrap_or(0.0)).sum::<f64>(), g.total));
    let ours_mb: f64 = ours.iter().filter_map(|q| alogs_pcstats::process_mb(*q)).sum();
    let ram_text = alogs_pcstats::ram().map_or("RAM ?".to_string(), |r| format!("RAM {:.1} GB free of {:.0}, aLogs {ours_mb:.0} MB", r.0, r.1));
    let c = std::mem::take(&mut m.diag.load);
    let span = if load.since == 0.0 { 60.0 } else { t - load.since };
    load.since = t;
    let where_ = if alogs_gamelog::scene_of(m.space.as_deref()) == Some(alogs_meter::meter::HOMELAND_SCENE) { "homeland" }
        else if m.bossrush_space { "Interlink" } else if m.holosim_space { "Holo-Battle Sim" } else if m.space.is_some() { "world" } else { "?" };
    let state = if m.fight.is_some() { "fight" } else if m.idle() { "idle" } else { "busy" };
    let away = if m.hwnd.is_none_or(win::is_foreground) { "" } else { ", tabbed out" };
    let peak = std::mem::take(&mut m.diag.peak_look);
    let (pc, me, game, appc) = cpu.map_or((None, None, None, None), |c| (c.pc, c.meter, c.game, c.app));
    log::info!("load: CPU meter {}, aLogs app {}, game {}, whole PC {} | {:.1} whole frames/s, {:.1} top parts/s, 0.0 box frames/s, {:.1} text reads/s | {where_}, {state}{away} | {gpu_text} | {ram_text} | no PC stalls | longest look {peak:.0} ms",
        fmt(me), fmt(appc), fmt(game), fmt(pc), c.whole as f64 / span, c.part as f64 / span, c.reads as f64 / span);
    // Discord's in-game overlay draws inside the game's picture: said once per start
    if !load.discord_said && let Some(h) = m.hwnd {
        let mods = win::window_modules(h);
        if mods.contains("discordhook64.dll") || mods.contains("discordhook.dll") {
            load.discord_said = true;
            log::info!("Discord's in-game overlay is loaded in the game: it shows in every capture, proof pictures included");
        }
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
        no_overlay: argv.iter().any(|a| a == "--no-overlay") || matches!(cfg.get("overlay").get("show"), Value::Bool(false)),
        no_auto_click: true,
        player_log: player_log.clone(),
        cfg,
    };
    let ocr: Box<dyn Reader> = match Ocr::new(&res.join("ocr-helper.exe")) {
        Ok(mut o) => {
            // Korean player names settled against the game's own fonts (the cache in <data>/gamefont)
            use alogs_reader::gamefont::{FONT_NAME, GameFont, KOREAN_FONT};
            let gf = data.join("gamefont");
            match GameFont::open(&gf, FONT_NAME).map(|l| (l, GameFont::open(&gf, KOREAN_FONT))) {
                Some((latin, Some(korean))) => {
                    o.korean = Some(alogs_reader::hangul::KoreanNames::new(Some(latin), Some(korean)));
                    log::info!("Korean names: on (the game's Korean font)");
                }
                _ => log::info!("Korean names: off (game font not readable: no cache from this game version)"),
            }
            Box::new(o)
        }
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
    alogs_meter::ocrcheck::ocr_language_note(m.ocr.as_ref());
    m.ocr_check.checked = now();
    m.submitter = submitter.clone();
    // housekeeping in the background: old screenshots hourly, the game's data every few hours
    alogs_meter::housekeeping::start_cleanup(data.clone(), stop.clone());
    let keep: Vec<(String, String)> = m.catalog.iter().filter(|r| r.get("monsterType").map(String::as_str) != Some("2"))
        .map(|r| (r["table"].clone(), r["puppetId"].clone())).collect();
    let game_data = alogs_meter::housekeeping::start_refresh(data.clone(), m.env.res.clone(), Some(player_log.clone()), keep, stop.clone());
    if let Some(u) = alogs_gamelog::last_uid(&player_log) {
        m.set_uid(&u);
    }
    if let Some(u) = &m.uid {
        log::info!("player uid {u} (from the game log)");
    }
    let hide = !m.args.cfg.get("overlay").get("in_recordings").truthy() && !matches!(m.args.cfg.get("overlay").get("in_recordings"), Value::None);
    if m.args.no_overlay {
        run_meter(m, title, game_data, submitter, interval, None, None);
        return;
    }
    // the overlay's windows live on this thread (their messages); the meter reads the game on its own
    let view = std::sync::Arc::new(std::sync::Mutex::new(m.overlay_view()));
    let own = std::sync::Arc::new(std::sync::Mutex::new(alogs_meter::overlay::Own::default()));
    m.own = Some(own.clone());
    let (otx, orx) = std::sync::mpsc::channel();
    let v2 = view.clone();
    let meter = std::thread::spawn(move || run_meter(m, title, game_data, submitter, interval, Some(v2), Some(otx)));
    match alogs_meter::overlay::Overlay::new(data.clone(), view.clone(), orx, hide, own) {
        Some(o) => o.run(),
        None => log::warn!("overlay windows could not be made: console output only"),
    }
    let _ = meter.join();
}

/// Both the console and the overlay get the meter's messages.
struct Both {
    console: Console,
    overlay: Option<std::sync::mpsc::Sender<(String, Value)>>,
}

impl Out for Both {
    fn out(&mut self, kind: &str, data: Value) {
        if let Some(t) = &self.overlay {
            let _ = t.send((kind.to_string(), data.clone()));
        }
        self.console.out(kind, data);
    }
}

#[allow(clippy::too_many_arguments)]
fn run_meter(mut m: Meter, title: String, game_data: std::sync::mpsc::Receiver<alogs_meter::housekeeping::GameData>, submitter: Option<std::sync::Arc<Submitter>>,
    interval: f64, view: Option<std::sync::Arc<std::sync::Mutex<alogs_meter::overlay::View>>>, overlay_tx: Option<std::sync::mpsc::Sender<(String, Value)>>) {
    let mut live = Live { title, cap: SharedCapture::new(0), hp_cap: None, audio: alogs_capture::shared::SharedAudio::default(), hwnd: None, look_at: 0.0,
        stats: CapStats::default(), said_window: None, last_size: (0, 0), no_pic_since: 0.0, no_pic_said: 0.0 };
    let mut load = Load { cpu: alogs_pcstats::CpuLoad::default(), gpu: alogs_pcstats::GpuLoad::new(), since: 0.0, discord_said: false };
    let mut console = Both { console: Console::default(), overlay: overlay_tx };
    let mut last_status = 0.0;
    loop {
        let t = now();
        m.take_game_data(&game_data);
        let look = std::time::Instant::now();
        m.step(&mut live, &mut console);
        if let Some(v) = &view {
            *v.lock().unwrap() = m.overlay_view();
        }
        m.diag.peak_look = m.diag.peak_look.max(look.elapsed().as_secs_f64() * 1000.0);
        if live.minute(now()) {
            load_line(&mut load, &mut m, now());
        }
        if t - last_status >= 1.0 {
            last_status = t;
            m.link_poll(t);
            let open = m.hwnd.is_some_and(win::is_window);
            let front = m.hwnd.is_some_and(win::is_foreground);
            m.presence_tick(t, open, front);
            m.ocr_recheck(t);
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
