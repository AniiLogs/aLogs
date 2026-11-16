//! Windows OCR through aLogs' native helper (app/ocr-helper, ocr-helper.exe): grey pixels on stdin, one line back per
//! picture. Port of `Ocr`, `NoOcr`, `ready_language`, `ocr_english` and `sweep_temp` in aLogs B16 `bossbar/reader.py`
//! (the native helper only: the PowerShell fallback was for builds without it).

use crate::hangul::{self, KoreanNames};
use crate::prep::name_signature;
use alogs_pil::{Image, Mode};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::Instant;

pub const TEMP_PREFIX: &str = "bossbar_";
/// How long an unsure name read (one character or none) answers its box from the cache.
pub const UNSURE_S: f64 = 30.0;

/// The OCR language in a helper's first line: "READY\ten-US" -> Some("en-US"), "READY\t" -> Some(""), a plain "READY"
/// -> None (an older helper that doesn't say). Anything else: it didn't start.
pub fn ready_language(line: &str) -> Result<Option<String>, String> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line == "READY" {
        return Ok(None);
    }
    match line.strip_prefix("READY\t") {
        Some(l) => Ok(Some(l.trim().to_string())),
        None => Err("Windows OCR helper did not start".into()),
    }
}

/// Does Windows OCR read English? None when the helper doesn't say.
pub fn ocr_english(language: Option<&str>) -> Option<bool> {
    language.map(|l| l.split('-').next().unwrap_or("").to_lowercase() == "en")
}

/// Temp folders of OCR helpers from earlier runs, older than `hours`: gone.
pub fn sweep_temp(hours: f64) {
    let dir = std::env::temp_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs_f64(hours * 3600.0);
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with(TEMP_PREFIX)
            && e.metadata().is_ok_and(|m| m.is_dir() && m.modified().is_ok_and(|t| t < cutoff))
        {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

type Sig = ((usize, usize), Vec<u8>);

struct Helper {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// A reader of text in pictures.
pub trait Reader: Send {
    /// `{name: text}` for `{name: picture}`; `names` marks name boxes (with their padding), `raws` their grey crops.
    fn read(&self, images: &IndexMap<String, Image>, names: &IndexMap<String, usize>, raws: &IndexMap<String, Image>) -> IndexMap<String, String>;
    /// Is every name box in `images` read already (the name cache)?
    fn cached(&self, images: &IndexMap<String, Image>, names: &IndexMap<String, usize>) -> bool;
    fn language(&self) -> Option<String>;
    fn started(&self) -> bool;
    /// Is this name box's read in the name cache for good (`results.sure_read`)?
    fn sure(&self, _img: &Image) -> bool {
        false
    }
    /// A new helper in this one's place (which picks Windows OCR's language again); only an OCR helper has one.
    fn restart(&self) -> Result<(), String> {
        Err("not an OCR helper".into())
    }
}

pub struct Ocr {
    exe: std::path::PathBuf,
    helper: Mutex<(Helper, u64)>, // the process and its generation (restarts)
    language: Mutex<Option<String>>,
    name_cache: Mutex<IndexMap<Sig, (String, Option<f64>)>>,
    epoch: Instant,
    /// Unsure reads kept for UNSURE_S (off in a replay).
    pub keep_unsure: bool,
    pub korean: Option<KoreanNames>,
}

fn launch(exe: &std::path::Path) -> Result<(Helper, Option<String>), String> {
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut first = Vec::new();
    let _ = stdout.read_until(b'\n', &mut first);
    match ready_language(&String::from_utf8_lossy(&first)) {
        Ok(lang) => Ok((Helper { child, stdin, stdout }, lang)),
        Err(e) => {
            let _ = child.kill();
            Err(e)
        }
    }
}

impl Ocr {
    /// Starts the helper at `exe`; Err when it doesn't start.
    pub fn new(exe: &std::path::Path) -> Result<Ocr, String> {
        static SWEPT: std::sync::Once = std::sync::Once::new();
        SWEPT.call_once(|| {
            std::thread::spawn(|| sweep_temp(1.0));
        });
        let (h, lang) = launch(exe)?;
        Ok(Ocr {
            exe: exe.to_path_buf(),
            helper: Mutex::new((h, 0)),
            language: Mutex::new(lang),
            name_cache: Mutex::new(IndexMap::new()),
            epoch: Instant::now(),
            keep_unsure: true,
            korean: None,
        })
    }

    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    /// A new helper in this one's place (which picks Windows OCR's language again).
    pub fn restart(&self) -> Result<(), String> {
        let (h, lang) = launch(&self.exe)?;
        let mut old = {
            let mut g = self.helper.lock().unwrap();
            let generation = g.1 + 1;
            *self.language.lock().unwrap() = lang;
            std::mem::replace(&mut *g, (h, generation)).0
        };
        self.name_cache.lock().unwrap().clear();
        drop(old.stdin);
        if old.child.wait().is_err() {
            let _ = old.child.kill();
        }
        Ok(())
    }

    fn cached_read(&self, sig: &Sig, now: f64) -> Option<String> {
        let mut c = self.name_cache.lock().unwrap();
        let hit = c.get(sig)?.clone();
        if hit.1.is_some_and(|t| t <= now) {
            return None;
        }
        if hit.1.is_some() {
            c.insert(sig.clone(), (hit.0.clone(), Some(now + UNSURE_S)));
        }
        Some(hit.0)
    }

    /// One request: `(flags, picture)` each; one line back each. Returns the lines and the helper generation asked.
    fn ask(&self, work: &[(u8, &Image)], with_flags: bool) -> (Vec<String>, u64) {
        let mut parts: Vec<u8> = Vec::new();
        parts.extend_from_slice(if with_flags { b"OCR2" } else { b"OCR1" });
        parts.extend_from_slice(&(work.len() as u32).to_le_bytes());
        for (flags, img) in work {
            let g = if img.mode == Mode::L { (*img).clone() } else { img.convert(Mode::L) };
            if with_flags {
                parts.push(*flags);
            }
            parts.extend_from_slice(&(g.width as u32).to_le_bytes());
            parts.extend_from_slice(&(g.height as u32).to_le_bytes());
            parts.extend_from_slice(&g.data);
        }
        let mut guard = self.helper.lock().unwrap();
        let generation = guard.1;
        let h = &mut guard.0;
        let mut lines = Vec::with_capacity(work.len());
        if h.stdin.write_all(&parts).and_then(|_| h.stdin.flush()).is_err() {
            return (vec![String::new(); work.len()], generation);
        }
        for _ in work {
            let mut l = Vec::new();
            let _ = h.stdout.read_until(b'\n', &mut l);
            lines.push(String::from_utf8_lossy(&l).trim_end_matches(['\r', '\n']).to_string());
        }
        (lines, generation)
    }

    fn korean_read(&self, t: &Image, hint: &str, letters: &Option<hangul::Letters>) -> Option<String> {
        self.korean.as_ref()?.read(t, hint, letters)
    }
}

impl Reader for Ocr {
    fn restart(&self) -> Result<(), String> {
        Ocr::restart(self)
    }
    fn sure(&self, img: &Image) -> bool {
        self.name_cache.lock().unwrap().get(&name_signature(img)).is_some_and(|h| h.1.is_none())
    }
    fn language(&self) -> Option<String> {
        self.language.lock().unwrap().clone()
    }
    fn started(&self) -> bool {
        true
    }

    fn cached(&self, images: &IndexMap<String, Image>, names: &IndexMap<String, usize>) -> bool {
        let now = self.now();
        names.keys().all(|k| self.cached_read(&name_signature(&images[k]), now).is_some())
    }

    fn read(&self, images: &IndexMap<String, Image>, names: &IndexMap<String, usize>, raws: &IndexMap<String, Image>) -> IndexMap<String, String> {
        let now = self.now();
        let mut known: IndexMap<String, String> = IndexMap::new();
        let mut sigs: HashMap<String, Sig> = HashMap::new();
        for k in names.keys() {
            let s = name_signature(&images[k]);
            if let Some(hit) = self.cached_read(&s, now) {
                known.insert(k.clone(), hit);
            }
            sigs.insert(k.clone(), s);
        }
        let images: IndexMap<&String, &Image> = images.iter().filter(|(k, _)| !known.contains_key(*k)).collect();
        let names: IndexMap<&String, usize> = names.iter().filter(|(k, _)| !known.contains_key(*k)).map(|(k, v)| (k, *v)).collect();
        if images.is_empty() {
            return known;
        }
        let keys: Vec<&String> = images.keys().copied().collect();
        let grey: Vec<&String> = keys.iter().copied().filter(|k| names.contains_key(k) && raws.contains_key(*k)).collect();
        let tidy: IndexMap<&String, Image> = if names.is_empty() {
            IndexMap::new()
        } else {
            keys.iter().filter(|k| names.contains_key(*k)).filter_map(|k| hangul::tidy(images[k], names[k]).map(|t| (*k, t))).collect()
        };
        let mut work: Vec<(u8, &Image)> =
            keys.iter().map(|k| (if names.contains_key(k) && !grey.contains(k) { 4 } else { 0 }, images[k])).collect();
        work.extend(grey.iter().map(|k| (6u8, &raws[*k])));
        work.extend(tidy.values().map(|t| (3u8, t)));
        let with_flags = !names.is_empty();
        let (lines, gen0) = self.ask(&work, with_flags);
        let mut out: IndexMap<String, String> = IndexMap::new();
        for (k, line) in keys.iter().zip(&lines) {
            let (text, _, latin) = hangul::split_reply(line);
            let v = if names.contains_key(k) {
                let l = hangul::clean_latin(&latin);
                if l.is_empty() { text } else { l }
            } else {
                text
            };
            out.insert((*k).clone(), v);
        }
        let mut short: Vec<&String> = Vec::new();
        for (k, line) in grey.iter().zip(&lines[keys.len()..]) {
            let latin = hangul::clean_latin(&hangul::split_reply(line).2);
            if latin.replace(' ', "").chars().count() > 1 {
                out.insert((*k).clone(), latin);
            } else {
                short.push(k);
            }
        }
        let n = keys.len() + grey.len();
        let found: IndexMap<&String, (&Image, Option<hangul::Letters>)> =
            tidy.iter().zip(&lines[n..]).map(|((k, t), line)| (*k, (t, hangul::split_reply(line).1))).collect();
        let korean = |k: &String, hint: &str| found.get(k).and_then(|(t, l)| self.korean_read(t, hint, l));
        let settled: HashMap<&String, Option<String>> =
            short.iter().filter(|k| found.get(*k).is_some_and(|f| hangul::hangul_first(&f.1))).map(|k| (*k, korean(k, ""))).collect();
        let lone_of = |k: &String| found.get(k).and_then(|(t, l)| self.korean.as_ref().map(|kn| kn.lone(t, l))).unwrap_or_default();
        let mut lone: IndexMap<&String, String> = IndexMap::new();
        for k in &short {
            if found.contains_key(*k) && !settled.get(*k).is_some_and(|s| s.as_ref().is_some_and(|x| !x.is_empty())) {
                let c = lone_of(k);
                if !c.is_empty() {
                    lone.insert(*k, c);
                }
            }
        }
        for (k, c) in &lone {
            out.insert((*k).clone(), c.clone());
        }
        let again: Vec<&String> = short
            .iter()
            .copied()
            .filter(|k| !settled.get(*k).is_some_and(|s| s.as_ref().is_some_and(|x| !x.is_empty())) && !lone.contains_key(*k))
            .collect();
        if !again.is_empty() {
            let w: Vec<(u8, &Image)> = again.iter().map(|k| (6u8, images[*k])).collect();
            let (ls, _) = self.ask(&w, true);
            for (k, line) in again.iter().zip(ls) {
                let l = hangul::clean_latin(&hangul::split_reply(&line).2);
                if !l.is_empty() {
                    out.insert((*k).clone(), l);
                }
            }
        }
        let mut vouched: HashSet<&String> = lone.keys().copied().collect();
        for k in found.keys() {
            let name = match settled.get(*k) {
                Some(s) => s.clone(),
                None => korean(k, &out[*k].clone()),
            };
            if let Some(n) = name.filter(|n| !n.is_empty()) {
                out.insert((*k).clone(), n);
                vouched.insert(k);
            }
        }
        for (k, (t, letters)) in &found {
            if vouched.contains(*k) {
                continue;
            }
            if self.korean.is_none() && hangul::looks_hangul(letters) {
                if !out[*k].is_empty() {
                    log::info!(target: "meter", "name box {k}: {:?} read in Latin, but the Korean model reads Hangul there (no game font to settle it): left blank", out[*k]);
                }
                out.insert((*k).clone(), String::new());
            } else if out[*k].replace(' ', "").chars().count() == 1 && hangul::ink_pieces(Some(t)) > 1 {
                log::info!(target: "meter", "name box {k}: {:?} read, but its ink is {} pieces: left blank", out[*k], hangul::ink_pieces(Some(t)));
                out.insert((*k).clone(), String::new());
            }
        }
        {
            let mut cache = self.name_cache.lock().unwrap();
            if gen0 == self.helper.lock().unwrap().1 {
                for k in names.keys() {
                    let unsure = out[*k].replace(' ', "").chars().count() <= 1 && !vouched.contains(*k);
                    cache.shift_remove(&sigs[*k]);
                    if unsure && !self.keep_unsure {
                        continue;
                    }
                    cache.insert(sigs[*k].clone(), (out[*k].clone(), if unsure { Some(now + UNSURE_S) } else { None }));
                    if cache.len() > 256 {
                        cache.shift_remove_index(0);
                    }
                }
            }
        }
        for (k, v) in known {
            out.insert(k, v);
        }
        out
    }
}

impl Drop for Ocr {
    fn drop(&mut self) {
        if let Ok(mut g) = self.helper.lock() {
            let h = &mut g.0;
            let _ = h.stdin.flush();
            let _ = h.child.kill();
        }
    }
}

/// In place of `Ocr` when its helper didn't start: every box reads as nothing.
pub struct NoOcr;

impl Reader for NoOcr {
    fn read(&self, images: &IndexMap<String, Image>, _: &IndexMap<String, usize>, _: &IndexMap<String, Image>) -> IndexMap<String, String> {
        images.keys().map(|k| (k.clone(), String::new())).collect()
    }
    fn cached(&self, _: &IndexMap<String, Image>, _: &IndexMap<String, usize>) -> bool {
        true
    }
    fn language(&self) -> Option<String> {
        Some(String::new())
    }
    fn started(&self) -> bool {
        false
    }
}
