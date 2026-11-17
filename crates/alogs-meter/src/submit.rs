//! Sending the meter's records to the AniiLogs leaderboards, only when the player opts in. Port of the sender half of
//! aLogs B16 `bossbar/submit.py`: every record goes to logs/submit_queue/ first and a background thread sends it from
//! there, so nothing is lost while offline.
//!
//! Known difference: proof pictures are JPEG-encoded by `jpeg-encoder`, not libjpeg, so their bytes differ from the
//! Python meter's (the same pictures, sizes and qualities; the site decodes either).

use crate::payloads::{safe, send_check, species_names, standing_key};
use alogs_pil::{Filter, Image, Mode};
use alogs_protocol::devflags::DevFlags;
use alogs_protocol::settings;
use alogs_pyval::{Dict, Value, dict};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const PROOF_SIZE: (usize, usize) = (1280, 720);
pub const FULL_WIDTH: usize = 1920;
pub const PARTY_MAX: usize = 1_500_000;
pub const SET_ASIDE_DAYS: f64 = 7.0;
pub const HOLD_MAX_S: f64 = 300.0;
pub const HOLD_LOOK_S: f64 = 2.0;
pub const FIGHT_STALE_S: f64 = 5.0;

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// The leaderboard account while signed in (an alb_ token).
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub api_url: String,
    pub token: String,
    pub screenshots: bool,
    pub link_character: bool,
    pub submit: bool,
}

pub fn load_account(data: &Path) -> Option<Account> {
    let cfg = settings::load(data);
    let c = cfg.get("leaderboard");
    let token = c.get("token").py_str();
    if !token.starts_with("alb_") {
        return None;
    }
    let api = c.get("api_url").py_str();
    let api = if api.trim().is_empty() { settings::defaults().get("leaderboard").get("api_url").py_str() } else { api.trim().to_string() };
    let flag = |k: &str, d: bool| match c.as_dict().and_then(|x| x.get_str(k)) {
        Some(v) => v.truthy(),
        None => d,
    };
    Some(Account {
        api_url: api.trim_end_matches('/').to_string(),
        token,
        screenshots: flag("include_screenshots", true),
        link_character: flag("link_character", true),
        submit: c.get("submit").truthy(),
    })
}

pub fn load_config(data: &Path) -> Option<Account> {
    load_account(data).filter(|a| a.submit)
}

/// Written whole, then renamed into place.
pub fn write_json(path: &Path, v: &Value, indent: Option<usize>) -> std::io::Result<()> {
    let text = match indent {
        Some(n) => v.json_dumps_indent(n),
        None => v.json_dumps(),
    };
    settings::write_whole(path, &text)
}

pub(crate) fn jpeg(im: &Image, quality: u8) -> Vec<u8> {
    let rgb = if im.mode == Mode::Rgb { im.clone() } else { im.convert(Mode::Rgb) };
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, quality);
    let _ = enc.encode(&rgb.data, rgb.width as u16, rgb.height as u16, jpeg_encoder::ColorType::Rgb);
    out
}

fn open_rgb(path: &Path) -> Option<Image> {
    let im = Image::open_png(path).or_else(|| {
        let bytes = std::fs::read(path).ok()?;
        let mut d = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
        let px = d.decode().ok()?;
        let info = d.info()?;
        Some(Image::from_raw(Mode::Rgb, info.width as usize, info.height as usize, px))
    })?;
    Some(im.convert(Mode::Rgb))
}

/// `ImageOps.pad(im, size, LANCZOS, black)`: fitted inside `size`, centred on black.
pub fn pad(im: &Image, w: usize, h: usize) -> Image {
    let (ir, dr) = (im.width as f64 / im.height as f64, w as f64 / h as f64);
    if ir == dr {
        return im.resize(w, h, Some(Filter::Lanczos));
    }
    let mut out = Image::new(im.mode, w, h, &[0, 0, 0]);
    if ir > dr {
        let nh = (im.height as f64 / im.width as f64 * w as f64).round_ties_even() as usize;
        let img = if nh != h { im.resize(w, nh, Some(Filter::Lanczos)) } else { im.clone() };
        let y = ((h as f64 - nh as f64) * 0.5).round_ties_even() as i64;
        out.paste(&img, 0, y);
    } else {
        let nw = (im.width as f64 / im.height as f64 * h as f64).round_ties_even() as usize;
        let img = if nw != w { im.resize(nw, h, Some(Filter::Lanczos)) } else { im.clone() };
        let x = ((w as f64 - nw as f64) * 0.5).round_ties_even() as i64;
        out.paste(&img, x, 0);
    }
    out
}

fn evidence_bytes(path: &Path) -> Option<Vec<u8>> {
    let im = open_rgb(path)?;
    let im = if im.size() != PROOF_SIZE { pad(&im, PROOF_SIZE.0, PROOF_SIZE.1) } else { im };
    Some(jpeg(&im, 85))
}

fn crop_bytes(path: &Path, b: (f64, f64, f64, f64)) -> Option<Vec<u8>> {
    let im = open_rgb(path)?;
    let (w, h) = (im.width as f64, im.height as f64);
    let u = w.min(h * 16.0 / 9.0);
    let ux = (w - u) / 2.0;
    Some(jpeg(&im.crop_i((ux + u * b.0) as i64, (h * b.1) as i64, (ux + u * b.2) as i64, (h * b.3) as i64), 90))
}

fn full_bytes(path: &Path) -> Option<Vec<u8>> {
    let im = open_rgb(path)?;
    let im = if im.width > FULL_WIDTH {
        let nh = (im.height as f64 * FULL_WIDTH as f64 / im.width as f64).round_ties_even() as usize;
        im.resize(FULL_WIDTH, nh, Some(Filter::Lanczos))
    } else {
        im
    };
    Some(jpeg(&im, 88))
}

fn party_bytes(path: &Path) -> Option<Vec<u8>> {
    let im = open_rgb(path)?;
    let mut data = Vec::new();
    for (width, q) in [(2560usize, 85u8), (2560, 75), (1920, 80), (1600, 70)] {
        let small = if im.width > width {
            im.resize(width, (im.height as f64 * width as f64 / im.width as f64).round_ties_even() as usize, Some(Filter::Lanczos))
        } else {
            im.clone()
        };
        data = jpeg(&small, q);
        if data.len() <= PARTY_MAX {
            return Some(data);
        }
    }
    Some(data)
}

fn proof_boxes(kind: &str) -> IndexMap<&'static str, (f64, f64, f64, f64)> {
    let mut m = IndexMap::new();
    if kind == "interlink_run" {
        m.insert("evidence", (0.13, 0.06, 0.88, 0.67));
        m.insert("battle", (0.115, 0.295, 0.885, 0.755));
    }
    m
}

/// Proof pictures' bytes kept in memory between tries of a record that didn't go through.
#[derive(Default)]
pub struct Proofs {
    kept: IndexMap<(String, PathBuf, u128, u64), Vec<u8>>,
    size: usize,
    encoded: u64,
    reused: u64,
    ms: f64,
    told: f64,
}

impl Proofs {
    const MAX_N: usize = 48;
    const MAX_BYTES: usize = 24_000_000;

    fn key(how: &str, path: &Path) -> Option<(String, PathBuf, u128, u64)> {
        let m = std::fs::metadata(path).ok()?;
        let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
        Some((how.to_string(), std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()), t, m.len()))
    }

    pub fn get(&mut self, how: &str, path: &Path, make: impl FnOnce() -> Option<Vec<u8>>) -> Option<Vec<u8>> {
        let Some(key) = Self::key(how, path) else { return make() };
        let data = match self.kept.shift_remove(&key) {
            Some(d) => {
                self.reused += 1;
                d
            }
            None => {
                let t = std::time::Instant::now();
                let d = make()?;
                self.ms += t.elapsed().as_secs_f64() * 1000.0;
                self.encoded += 1;
                if Self::key(how, path).as_ref() != Some(&key) {
                    return Some(d);
                }
                self.size += d.len();
                d
            }
        };
        self.kept.insert(key, data.clone());
        while !self.kept.is_empty() && (self.kept.len() > Self::MAX_N || self.size > Self::MAX_BYTES) {
            let (_, v) = self.kept.shift_remove_index(0).unwrap();
            self.size -= v.len();
        }
        Some(data)
    }

    pub fn forget(&mut self, data_dir: &Path, item: &Value) {
        let mut rels: Vec<String> = ["evidence", "battle", "party"].iter().filter(|k| item.get(k).truthy()).map(|k| item.get(k).py_str()).collect();
        if let Some(g) = item.get("gizmos").as_list() {
            rels.extend(g.iter().map(Value::py_str));
        }
        let paths: Vec<PathBuf> = rels.iter().map(|r| std::fs::canonicalize(data_dir.join(r)).unwrap_or_else(|_| data_dir.join(r))).collect();
        let drop: Vec<_> = self.kept.keys().filter(|k| paths.contains(&k.1)).cloned().collect();
        for k in drop {
            if let Some(v) = self.kept.shift_remove(&k) {
                self.size -= v.len();
            }
        }
    }

    pub fn tell(&mut self, now: f64) {
        if now - self.told < 60.0 || (self.encoded == 0 && self.reused == 0) {
            return;
        }
        log::info!(target: "meter", "proof-encode-once: {} proof pictures encoded ({:.0} ms), {} sent again as they were; {} kept ({} KB)",
            self.encoded, self.ms, self.reused, self.kept.len(), self.size / 1024);
        self.told = now;
        self.encoded = 0;
        self.reused = 0;
        self.ms = 0.0;
    }
}

/// The game server (region) you play on, from the game's own settings in the registry.
pub fn game_server() -> Option<String> {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Aniimo\Aniimo").ok()?;
        let mut values: IndexMap<String, String> = IndexMap::new();
        for (name, value) in key.enum_values().flatten() {
            let base = match name.rfind("_h") {
                Some(i) => name[..i].to_string(),
                None => name.clone(),
            };
            let text = match value.vtype {
                winreg::enums::RegType::REG_BINARY => {
                    let b: Vec<u8> = value.bytes.iter().copied().collect();
                    let end = b.iter().rposition(|&c| c != 0).map_or(0, |i| i + 1);
                    String::from_utf8_lossy(&b[..end]).into_owned()
                }
                winreg::enums::RegType::REG_DWORD => u32::from_le_bytes(value.bytes[..4].try_into().ok()?).to_string(),
                _ => value.to_string(),
            };
            values.insert(base, text);
        }
        let current = values.get("serverId").cloned();
        for (name, v) in &values {
            if let Some(acct) = name.strip_prefix("oversea_serverName_")
                && !v.is_empty()
                && current.as_ref().is_none_or(|c| values.get(&format!("oversea_serverId_{acct}")) == Some(c))
            {
                return Some(v.chars().take(40).collect());
            }
        }
        values.get("serverName").filter(|s| !s.is_empty()).map(|s| s.chars().take(40).collect())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn boundary() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// multipart/form-data from text fields and (name, filename, type, bytes) files.
pub fn form(fields: &Dict, files: &[(&str, &str, &str, Vec<u8>)]) -> (Vec<u8>, String) {
    let b = boundary();
    let mut out = Vec::new();
    for (k, v) in fields.iter() {
        if v.is_none() {
            continue;
        }
        out.extend(format!("--{b}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n", k.py_str()).as_bytes());
        out.extend(v.py_str().as_bytes());
        out.extend(b"\r\n");
    }
    for (name, filename, ctype, data) in files {
        out.extend(format!("--{b}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {ctype}\r\n\r\n").as_bytes());
        out.extend(data);
        out.extend(b"\r\n");
    }
    out.extend(format!("--{b}--\r\n").as_bytes());
    (out, format!("multipart/form-data; boundary={b}"))
}

/// The record and its proof pictures, as one multipart body.
pub fn multipart(payload: &Value, files: &IndexMap<String, Vec<u8>>) -> (Vec<u8>, String) {
    let b = boundary();
    let mut out = Vec::new();
    out.extend(format!("--{b}\r\nContent-Disposition: form-data; name=\"submission\"\r\nContent-Type: application/json\r\n\r\n").as_bytes());
    out.extend(payload.json_dumps().as_bytes());
    out.extend(b"\r\n");
    for (field, data) in files {
        if data.is_empty() {
            continue;
        }
        out.extend(format!("--{b}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{field}.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n").as_bytes());
        out.extend(data);
        out.extend(b"\r\n");
    }
    out.extend(format!("--{b}--\r\n").as_bytes());
    (out, format!("multipart/form-data; boundary={b}"))
}

/// How a send went.
enum Sent {
    Ok(u16, Value),
    Http(u16, String),
    Io(String),
}

/// The background sender and the meter's calls into it.
pub struct Submitter {
    pub data: PathBuf,
    pub queue: PathBuf,
    pub client_version: String,
    pub account: Mutex<Option<Account>>,
    pub config: Mutex<Option<Account>>,
    config_stamp: Mutex<Option<std::time::SystemTime>>,
    linked: Mutex<Option<(String, String, bool)>>,
    pub dispute: Mutex<Option<Value>>,
    pub last_result: Mutex<Option<(f64, String)>>,
    /// (payload, reply) after a record reached the leaderboards (the meter's notice).
    pub on_sent: Mutex<Option<Sender<(Value, Value)>>>,
    /// When the meter last said whether a fight is on (queue-wait-fight).
    pub fight_seen: Mutex<Option<(f64, bool)>>,
    proofs: Mutex<Proofs>,
    wake_tx: Sender<()>,
    wake_rx: Mutex<Option<Receiver<()>>>,
    pub stop: AtomicBool,
    dev: Mutex<DevFlags>,
}

fn read_json(path: &Path) -> Option<Value> {
    Value::json_loads(&std::fs::read_to_string(path).ok()?)
}

impl Submitter {
    pub fn new(data: PathBuf) -> Arc<Submitter> {
        let queue = data.join("logs").join("submit_queue");
        let _ = std::fs::create_dir_all(&queue);
        let (tx, rx) = std::sync::mpsc::channel();
        let s = Submitter {
            client_version: std::env::var("ALOGS_VERSION").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "meter-0.1".into()),
            account: Mutex::new(load_account(&data)),
            config: Mutex::new(load_config(&data)),
            config_stamp: Mutex::new(std::fs::metadata(settings::settings_file(&data)).and_then(|m| m.modified()).ok()),
            linked: Mutex::new(None),
            dispute: Mutex::new(read_json(&data.join("character_dispute.json"))),
            last_result: Mutex::new(None),
            on_sent: Mutex::new(None),
            fight_seen: Mutex::new(None),
            proofs: Mutex::new(Proofs::default()),
            wake_tx: tx,
            wake_rx: Mutex::new(Some(rx)),
            stop: AtomicBool::new(false),
            dev: Mutex::new(DevFlags::from_env()),
            queue,
            data,
        };
        match s.config.lock().unwrap().as_ref() {
            Some(c) => log::info!(target: "meter", "leaderboard submissions: on ({})", c.api_url),
            None => log::info!(target: "meter", "leaderboard submissions: off (opt in from the app)"),
        }
        s.retry_big_proofs();
        Arc::new(s)
    }

    fn wake(&self) {
        let _ = self.wake_tx.send(());
    }

    /// Character links turned down for their proof's size (over the site's old 512 KB) go again, once.
    fn retry_big_proofs(&self) {
        let Ok(rd) = std::fs::read_dir(&self.queue) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("character__") && name.ends_with(".json.rejected")) {
                continue;
            }
            let path = e.path();
            let item = read_json(&path);
            let proof = item.as_ref().filter(|i| i.get("proof").truthy()).map(|i| self.data.join(i.get("proof").py_str()));
            let size = proof.and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len());
            let back = PathBuf::from(&path.to_string_lossy()[..path.to_string_lossy().len() - ".rejected".len()]);
            if !(512 * 1024 < size && size <= 1_900_000) || back.exists() {
                continue;
            }
            if std::fs::rename(&path, &back).is_ok() {
                log::info!(target: "meter", "leaderboard: {} sent again (its proof is {} KB, now allowed)", &name[..name.len() - 14], size / 1024);
            }
        }
    }

    /// The leaderboard settings again when settings.json changed.
    pub fn refresh(&self) {
        let stamp = std::fs::metadata(settings::settings_file(&self.data)).and_then(|m| m.modified()).ok();
        let mut s = self.config_stamp.lock().unwrap();
        if stamp != *s {
            *s = stamp;
            *self.account.lock().unwrap() = load_account(&self.data);
            *self.config.lock().unwrap() = load_config(&self.data);
        }
    }

    /// Queue one record (the same kind + run id replaces an older queued copy); nothing while off.
    pub fn enqueue(&self, payload: &Value, evidence: Option<&str>, battle: Option<&str>, party: Option<&str>, gizmos: Option<&[String]>) {
        self.refresh();
        let Some(cfg) = self.config.lock().unwrap().clone() else { return };
        let species = species_names(&self.data.join("icons").join("icons.json"));
        let mut p = send_check(payload, species.as_ref());
        if let Value::Dict(d) = &mut p {
            d.insert("clientVersion".into(), Value::Str(self.client_version.clone()));
            if d.get_str("server").is_none() {
                d.insert("server".into(), game_server().map_or(Value::None, Value::Str));
            }
        }
        let name = safe(&format!("{}__{}", p.get("kind").py_str(), p.get("clientRunId").py_str())) + ".json";
        let s = |o: Option<&str>| if cfg.screenshots { o.map_or(Value::None, |x| Value::Str(x.into())) } else { Value::None };
        let g = if cfg.screenshots {
            gizmos.filter(|g| !g.is_empty()).map_or(Value::None, |g| Value::List(g.iter().map(|x| Value::Str(x.clone())).collect()))
        } else {
            Value::None
        };
        let item = dict([
            ("endpoint", Value::Str("submissions".into())),
            ("payload", p.clone()),
            ("evidence", s(evidence)),
            ("battle", s(battle)),
            ("party", s(party)),
            ("gizmos", g),
            ("queued_at", Value::Float(now())),
        ]);
        let _ = write_json(&self.queue.join(&name), &item, Some(1));
        log::info!(target: "meter", "leaderboard: queued {} {}", p.get("kind").py_str(), p.get("clientRunId").py_str());
        self.wake();
    }

    /// Link your character (with the game menu as proof the site trusts it).
    /// The next link_character goes even if it matches the last one sent.
    pub fn forget_linked(&self) {
        *self.linked.lock().unwrap() = None;
    }

    pub fn link_character(&self, uid: &str, name: &str, proof: Option<&Path>) {
        self.refresh();
        let acct = self.account.lock().unwrap().clone();
        let Some(acct) = acct.filter(|a| a.link_character) else { return };
        let _ = acct;
        if uid.is_empty() {
            return;
        }
        let mut linked = self.linked.lock().unwrap();
        let key = (uid.to_string(), name.to_string(), proof.is_some());
        if linked.as_ref() == Some(&key) {
            return;
        }
        if let Some(l) = linked.as_ref()
            && l.0 == uid
            && l.1 == name
            && l.2
            && proof.is_none()
        {
            return;
        }
        *linked = Some(key);
        let rel = proof.map(|p| p.strip_prefix(&self.data).map_or_else(|_| p.to_string_lossy().into_owned(), |r| r.to_string_lossy().into_owned()));
        let item = dict([
            ("endpoint", Value::Str("characters".into())),
            ("payload", dict([("gameUid", Value::Str(uid.into())), ("name", Value::Str(name.into()))])),
            ("proof", rel.map_or(Value::None, Value::Str)),
            ("queued_at", Value::Float(now())),
        ]);
        let _ = write_json(&self.queue.join(safe(&format!("character__{uid}")) + ".json"), &item, None);
        log::info!(target: "meter", "leaderboard: queued character link {name} (UID {uid}){}", if proof.is_some() { " with the game menu as proof" } else { "" });
        self.wake();
    }

    /// Your character's profile numbers (Collection Progress, or its level and EXP).
    pub fn character_stats(&self, uid: &str, stats: &Value) {
        self.refresh();
        if !self.account.lock().unwrap().as_ref().is_some_and(|a| a.link_character) || uid.is_empty() {
            return;
        }
        let has_level = stats.as_dict().is_some_and(|d| d.get_str("level").is_some());
        let name = if has_level { format!("level__{uid}") } else { format!("stats__{uid}") };
        let item = dict([
            ("endpoint", Value::Str("characters/stats".into())),
            ("payload", dict([("gameUid", Value::Str(uid.into())), ("stats", stats.clone())])),
            ("queued_at", Value::Float(now())),
        ]);
        let _ = write_json(&self.queue.join(safe(&name) + ".json"), &item, None);
        log::info!(target: "meter", "leaderboard: queued profile numbers for UID {uid}");
        self.wake();
    }

    /// Online / AFK / offline for your profile, sent straight away (not queued).
    pub fn presence(self: &Arc<Self>, uid: &str, state: &str, wait: bool) {
        self.refresh();
        let Some(acct) = self.account.lock().unwrap().clone().filter(|a| a.link_character) else { return };
        if uid.is_empty() {
            return;
        }
        let (uid, state, ver) = (uid.to_string(), state.to_string(), self.client_version.clone());
        let go = move || {
            let body = dict([("gameUid", Value::Str(uid)), ("state", Value::Str(state.clone()))]).json_dumps();
            let r = ureq::post(&format!("{}/leaderboard/characters/presence", acct.api_url))
                .set("authorization", &format!("Bearer {}", acct.token))
                .set("content-type", "application/json")
                .set("user-agent", &ver)
                .timeout(Duration::from_secs(10))
                .send_string(&body);
            if let Err(e) = r {
                log::debug!(target: "meter", "presence: {state} not sent: {e}");
            }
        };
        if wait {
            go();
        } else {
            std::thread::spawn(go);
        }
    }

    fn drop_proof(&self, item: &Value) {
        self.proofs.lock().unwrap().forget(&self.data, item);
        let keep = alogs_pyval::py_int(settings::load(&self.data).get("capture").get("keep_hours")).unwrap_or(0);
        if keep != 0 {
            return;
        }
        for k in ["evidence", "battle", "party"] {
            if item.get(k).truthy() {
                let _ = std::fs::remove_file(self.data.join(item.get(k).py_str()));
            }
        }
        for rel in item.get("gizmos").as_list().cloned().unwrap_or_default() {
            let _ = std::fs::remove_file(self.data.join(rel.py_str()));
        }
    }

    /// Records waiting to go.
    pub fn pending(&self) -> usize {
        let _ = std::fs::create_dir_all(&self.queue);
        std::fs::read_dir(&self.queue).map_or(0, |rd| rd.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".json")).count())
    }

    fn post(&self, acct: &Account, path: &str, body: Vec<u8>, ctype: &str) -> Sent {
        let r = ureq::post(&format!("{}/leaderboard/{path}", acct.api_url))
            .set("authorization", &format!("Bearer {}", acct.token))
            .set("content-type", ctype)
            .set("user-agent", &self.client_version)
            .timeout(Duration::from_secs(30))
            .send_bytes(&body);
        match r {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.into_string().unwrap_or_default();
                let v = if text.is_empty() { Value::Dict(Dict::new()) } else { Value::json_loads(&text).unwrap_or(Value::Dict(Dict::new())) };
                Sent::Ok(status, v)
            }
            Err(ureq::Error::Status(code, resp)) => Sent::Http(code, resp.into_string().unwrap_or_default().chars().take(300).collect()),
            Err(e) => Sent::Io(e.to_string()),
        }
    }

    fn send(&self, acct: &Account, item: &Value) -> Sent {
        let endpoint = item.get("endpoint").py_str();
        if (endpoint == "characters" || endpoint == "disputes") && item.get("proof").truthy() {
            let rel = item.get("proof").py_str();
            if let Ok(proof) = std::fs::read(self.data.join(&rel)) {
                let jpg = rel.to_lowercase().ends_with(".jpg");
                let fields = item.get("payload").as_dict().cloned().unwrap_or_default();
                let (body, ctype) = form(&fields, &[("proof", if jpg { "menu.jpg" } else { "menu.png" }, if jpg { "image/jpeg" } else { "image/png" }, proof)]);
                let path = if endpoint == "disputes" { "characters/disputes" } else { "characters" };
                return self.post(acct, path, body, &ctype);
            }
        }
        let mut payload = item.get("payload").clone();
        let kind = payload.get("kind").py_str();
        let boxes = proof_boxes(&kind);
        let mut files: IndexMap<String, Vec<u8>> = IndexMap::new();
        let mut proofs = self.proofs.lock().unwrap();
        for key in ["evidence", "battle"] {
            if !item.get(key).truthy() || (key == "battle" && !files.contains_key("evidence")) {
                continue;
            }
            let path = self.data.join(item.get(key).py_str());
            let main = match boxes.get(key) {
                Some(b) => proofs.get(&format!("crop{b:?}"), &path, || crop_bytes(&path, *b)),
                None => proofs.get("evidence", &path, || evidence_bytes(&path)),
            };
            let full = proofs.get("full", &path, || full_bytes(&path));
            if let (Some(m), Some(f)) = (main, full) {
                files.insert(key.into(), m);
                files.insert(format!("{key}_full"), f);
            }
        }
        if item.get("party").truthy() && files.contains_key("evidence") {
            let path = self.data.join(item.get("party").py_str());
            if let Some(b) = proofs.get("party", &path, || party_bytes(&path)) {
                files.insert("party".into(), b);
            }
        }
        if files.contains_key("evidence") {
            for (i, rel) in item.get("gizmos").as_list().cloned().unwrap_or_default().iter().take(20).enumerate() {
                let path = self.data.join(rel.py_str());
                if let Some(b) = proofs.get("full", &path, || full_bytes(&path)) {
                    files.insert(format!("gizmo{}", i + 1), b);
                }
            }
        }
        drop(proofs);
        let (body, ctype) = if !files.is_empty() {
            if !boxes.is_empty()
                && let Value::Dict(d) = &mut payload
            {
                let mut extra = d.at("extra").as_dict().cloned().unwrap_or_default();
                let pb = Value::Dict(
                    boxes
                        .iter()
                        .filter(|(k, _)| files.contains_key(**k))
                        .map(|(k, b)| (Value::Str((*k).into()), Value::List(vec![Value::Float(b.0), Value::Float(b.1), Value::Float(b.2), Value::Float(b.3)])))
                        .collect(),
                );
                extra.insert("proofBoxes".into(), pb);
                d.insert("extra".into(), Value::Dict(extra));
            }
            multipart(&payload, &files)
        } else {
            (payload.json_dumps().into_bytes(), "application/json".to_string())
        };
        let ep = if ["characters", "characters/stats", "disputes"].contains(&endpoint.as_str()) { endpoint.as_str() } else { "submissions" };
        self.post(acct, ep, body, &ctype)
    }

    fn expire(&self) {
        let Ok(rd) = std::fs::read_dir(&self.queue) else { return };
        let now = std::time::SystemTime::now();
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.ends_with(".rejected") || name.ends_with(".bad")) {
                continue;
            }
            if e.metadata().and_then(|m| m.modified()).is_ok_and(|t| now.duration_since(t).is_ok_and(|d| d.as_secs_f64() > SET_ASIDE_DAYS * 86400.0))
                && std::fs::remove_file(e.path()).is_ok()
            {
                log::info!(target: "meter", "leaderboard: removed {name} (set aside over {} days ago)", SET_ASIDE_DAYS as i64);
            }
        }
    }

    /// The site's Interlink board already shows this player at this best or better (then it isn't sent).
    fn board_has(&self, acct: &Account, payload: &Value) -> bool {
        let (cycle, boss, uid) = (payload.get("cycle"), payload.get("boss").get("id"), payload.get("gameUid"));
        if !(cycle.truthy() && boss.truthy() && uid.truthy()) {
            return false;
        }
        let url = format!("{}/leaderboard/interlink?cycle={}&unlinked=1", acct.api_url, urlencode(&cycle.py_str()));
        let r = ureq::get(&url).set("authorization", &format!("Bearer {}", acct.token)).set("user-agent", &self.client_version).timeout(Duration::from_secs(10)).call();
        let board = match r.ok().and_then(|r| r.into_string().ok()).and_then(|t| Value::json_loads(&t)) {
            Some(b) => b,
            None => {
                log::info!(target: "meter", "leaderboard: board not read before a menu best; sending it");
                return false;
            }
        };
        let n = |v: &Value| v.as_i64().unwrap_or(0);
        let mine = (n(payload.get("stars")), n(payload.get("score")));
        for b in board.get("bosses").as_list().cloned().unwrap_or_default() {
            if b.get("id").py_str() != boss.py_str() {
                continue;
            }
            for e in b.get("entries").as_list().cloned().unwrap_or_default() {
                if e.get("uid").py_str() == uid.py_str() {
                    return (n(e.get("stars")), n(e.get("score"))) >= mine;
                }
            }
        }
        false
    }

    fn standing_sent(&self, payload: &Value) -> bool {
        let (key, value) = standing_key(payload);
        read_json(&self.data.join("logs").join("sent_standings.json")).is_some_and(|s| s.get(&key).py_eq(&value))
    }

    fn remember_standing(&self, payload: &Value) {
        let (key, value) = standing_key(payload);
        let path = self.data.join("logs").join("sent_standings.json");
        let mut sent = match read_json(&path) {
            Some(Value::Dict(d)) => d,
            _ => Dict::new(),
        };
        sent.insert(Value::Str(key), value);
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(&path, Value::Dict(sent).json_dumps());
    }

    fn queued(&self) -> IndexMap<String, u128> {
        let mut out = IndexMap::new();
        if let Ok(rd) = std::fs::read_dir(&self.queue) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.ends_with(".json")
                    && let Ok(t) = e.metadata().and_then(|m| m.modified())
                {
                    out.insert(n, t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
                }
            }
        }
        out
    }

    /// The sending thread: wakes on new records, else every minute (five while nothing waits).
    pub fn run(self: Arc<Self>) {
        let rx = self.wake_rx.lock().unwrap().take().expect("run once");
        let mut delay = 5.0;
        let mut held_since: Option<f64> = None;
        let mut tried: Option<IndexMap<String, u128>> = None;
        while !self.stop.load(Ordering::Relaxed) {
            let woke = match rx.recv_timeout(Duration::from_secs_f64(delay)) {
                Ok(()) => {
                    while rx.try_recv().is_ok() {}
                    true
                }
                Err(RecvTimeoutError::Timeout) => false,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            // Dev switch queue-wait-fight: a retry waits while a fight is on (never a new record)
            let t = now();
            let wait_on = self.dev.lock().unwrap().on("queue-wait-fight");
            if !wait_on {
                held_since = None;
                tried = None;
            } else {
                let seen = *self.fight_seen.lock().unwrap();
                let fighting = seen.is_some_and(|s| s.1 && (0.0..FIGHT_STALE_S).contains(&(t - s.0)));
                let mut new = woke;
                if !woke && fighting && held_since.is_none_or(|h| (0.0..HOLD_MAX_S).contains(&(t - h))) {
                    let q = self.queued();
                    let tr = tried.clone().unwrap_or_default();
                    new = q.iter().any(|(n, at)| tr.get(n) != Some(at));
                    if !q.is_empty() && !new {
                        if held_since.is_none() {
                            held_since = Some(t);
                        }
                        delay = HOLD_LOOK_S;
                        continue;
                    }
                }
                if let Some(h) = held_since.take() {
                    let why = if new { "a new record came" } else if !fighting { "the fight was over" } else { "the longest it waits" };
                    log::info!(target: "meter", "queue-wait-fight: a retry waited {:.0} s for a fight ({why})", (t - h).max(0.0));
                }
            }
            self.refresh();
            self.expire();
            let Some(acct) = self.account.lock().unwrap().clone() else {
                delay = 60.0;
                continue;
            };
            let _ = std::fs::create_dir_all(&self.queue);
            let mut names: Vec<String> =
                std::fs::read_dir(&self.queue).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
            names.sort();
            for name in names {
                if !name.ends_with(".json") {
                    continue;
                }
                let path = self.queue.join(&name);
                let Ok(raw) = std::fs::read_to_string(&path) else { break };
                let item = match Value::json_loads(&raw) {
                    Some(v @ Value::Dict(_)) => v,
                    Some(_) => {
                        let _ = std::fs::rename(&path, path.with_extension("json.bad"));
                        continue;
                    }
                    None => {
                        log::warn!(target: "meter", "leaderboard: {name} is unreadable, set aside");
                        let _ = std::fs::rename(&path, path.with_extension("json.bad"));
                        continue;
                    }
                };
                let ep = item.get("endpoint").py_str();
                let own = ["characters", "characters/stats", "disputes"].contains(&ep.as_str());
                if self.config.lock().unwrap().is_none() && !own {
                    continue;
                }
                let kind = item.get("payload").get("kind").py_str();
                if kind == "eggheist_season" && self.standing_sent(item.get("payload")) {
                    log::info!(target: "meter", "leaderboard: {name} not sent - the site already has this Egg Heist standing");
                    let _ = std::fs::remove_file(&path);
                    self.drop_proof(&item);
                    continue;
                }
                if kind == "interlink_best" && self.board_has(&acct, item.get("payload")) {
                    log::info!(target: "meter", "leaderboard: {name} not sent - the board already has this score for this boss and week");
                    let _ = std::fs::remove_file(&path);
                    self.drop_proof(&item);
                    continue;
                }
                match self.send(&acct, &item) {
                    Sent::Ok(status, reply) => {
                        let id = [reply.get("submissionId"), reply.get("gameUid"), reply.get("disputeId")].into_iter().find(|v| v.truthy()).map_or("None".into(), Value::py_str);
                        log::info!(target: "meter", "leaderboard: sent {name} -> {status} {id}{}", if reply.get("updated").truthy() { " (updated)" } else { "" });
                        if ep == "disputes" || (ep == "characters" && item.get("proof").truthy()) {
                            *self.dispute.lock().unwrap() = None;
                            let _ = std::fs::remove_file(self.data.join("character_dispute.json"));
                        }
                        *self.last_result.lock().unwrap() = Some((now(), format!("sent {}", &name[..name.len() - 5])));
                        if kind == "eggheist_season" {
                            self.remember_standing(item.get("payload"));
                        }
                        if ep == "submissions"
                            && let Some(tx) = self.on_sent.lock().unwrap().as_ref()
                        {
                            let _ = tx.send((item.get("payload").clone(), reply));
                        }
                        if std::fs::read_to_string(&path).is_ok_and(|now_raw| now_raw == raw) {
                            let _ = std::fs::remove_file(&path);
                            self.drop_proof(&item);
                        } else {
                            self.wake();
                        }
                    }
                    Sent::Http(code, msg) => {
                        if code == 409 && ep == "characters" && item.get("proof").truthy() && msg.replace(' ', "").contains("\"dispute\":true") {
                            let p = item.get("payload");
                            let d = dict([("gameUid", p.get("gameUid").clone()), ("name", p.get("name").clone()), ("proof", item.get("proof").clone())]);
                            let _ = std::fs::write(self.data.join("character_dispute.json"), d.json_dumps());
                            *self.dispute.lock().unwrap() = Some(d);
                        }
                        if [400, 409, 413].contains(&code) {
                            log::warn!(target: "meter", "leaderboard: rejected {name}: {code} {msg}");
                            *self.last_result.lock().unwrap() = Some((now(), format!("rejected {}: {}", &name[..name.len() - 5], msg.chars().take(120).collect::<String>())));
                            let _ = std::fs::rename(&path, path.with_extension("json.rejected"));
                            self.drop_proof(&item);
                        } else {
                            log::warn!(target: "meter", "leaderboard: {name} not sent yet: {code} {msg}");
                            *self.last_result.lock().unwrap() = Some((now(), format!("waiting: server said {code}")));
                            break;
                        }
                    }
                    Sent::Io(e) => {
                        log::warn!(target: "meter", "leaderboard: {name} not sent yet: {e}");
                        break;
                    }
                }
            }
            self.proofs.lock().unwrap().tell(now());
            if wait_on {
                tried = Some(self.queued());
            }
            delay = if self.pending() > 0 { 60.0 } else { 300.0 };
        }
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}
