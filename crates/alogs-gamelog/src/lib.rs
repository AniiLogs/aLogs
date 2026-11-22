//! Following the game's own text log (Player.log). Port of `LogWatcher`, `uid_in`, `last_uid` and `scene_of` in aLogs
//! B16 `bossbar/core.py`, plus the small text helpers next to them (`interlink_score`, `same_name`).

pub mod difflib;
mod score;

pub use score::{interlink_score, same_name};

use regex::bytes::Regex as BRegex;
use regex::Regex;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static RX_UID: LazyLock<BRegex> = LazyLock::new(|| {
    BRegex::new(r"callerId=(\d{6,20})|/prod/ClientInfo/(\d{6,20})_client_info_bin|/prod/pet_jewelry/(\d{6,20})")
        .unwrap()
});
static RX_SPACE_UID: LazyLock<BRegex> = LazyLock::new(|| BRegex::new(r"createVoxelScene new>>>id:\d+-(\d{6,20})-\d+").unwrap());
// Python's \S on str: not str.isspace(), which also counts \x1c-\x1f as space
static RX_ENTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"createVoxelScene new>>>id:([^\s\x1c-\x1f]+)").unwrap());
static RX_LEAVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"clearSpace is_destroy:true id:([^\s\x1c-\x1f]+)").unwrap());
static RX_TIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d+)\]").unwrap());
static RX_WANTED: LazyLock<BRegex> =
    LazyLock::new(|| BRegex::new(r"callerId=|/prod/|createVoxelScene|clearSpace|bossrush|fb_roguelike").unwrap());
static RX_WANTED_INFO: LazyLock<BRegex> = LazyLock::new(|| {
    BRegex::new(r"callerId=|/prod/|createVoxelScene|clearSpace|bossrush|fb_roguelike|\]\[info\]").unwrap()
});

fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Your UID from one game-log line (bytes), or None.
pub fn uid_in(raw: &[u8]) -> Option<String> {
    let c = RX_UID.captures(raw)?;
    (1..4).find_map(|i| c.get(i)).map(|m| String::from_utf8_lossy(m.as_bytes()).into_owned())
}

/// `'100-<playerId>-<sceneId>'` -> sceneId (homeland 550, open world 3000 ...); instances give None.
pub fn scene_of(space_id: Option<&str>) -> Option<i64> {
    static RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]+-[0-9]+-([0-9]+)$").unwrap());
    let c = RX.captures(space_id.unwrap_or(""))?;
    c[1].parse().ok()
}

/// Your player id from the end of the game log (last 32 MB): the newest service-call or profile-download UID, else
/// the newest personal-space id. None before login.
pub fn last_uid(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(size.saturating_sub(32 << 20))).ok()?;
    let mut tail = Vec::new();
    f.read_to_end(&mut tail).ok()?;
    last_uid_in(&tail)
}

pub fn last_uid_in(tail: &[u8]) -> Option<String> {
    let ids: Vec<&[u8]> = RX_UID
        .captures_iter(tail)
        .filter_map(|c| (1..4).find_map(|i| c.get(i)).map(|m| m.as_bytes()))
        .collect();
    let ids = if ids.is_empty() {
        RX_SPACE_UID.captures_iter(tail).map(|c| c.get(1).unwrap().as_bytes()).collect()
    } else {
        ids
    };
    ids.last().map(|b| String::from_utf8_lossy(b).into_owned())
}

/// How much of Player.log's head [`game_build`] reads: the version lines sit near the top of a file that grows large.
pub const GAME_BUILD_HEAD: u64 = 2 << 20;

/// The running game's version, from Player.log's head (the game writes it at every start, the file is new each time).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameBuild {
    /// The game's build (`scm[...]` of `PrintGameVersionInfo`).
    pub build: i64,
    /// The newer build the game was offered (`res[...]` of `MatchLuaVerList`): only on an install that isn't up to date.
    pub patch: Option<i64>,
    /// What the game was told to do about it (`act[...]`, e.g. `TipPatch`).
    pub patch_action: Option<String>,
}

static RX_VERSION_LINE: LazyLock<BRegex> = LazyLock::new(|| BRegex::new(r"PrintGameVersionInfo:[^\r\n]*").unwrap());
static RX_PATCH_LINE: LazyLock<BRegex> = LazyLock::new(|| BRegex::new(r"MatchLuaVerList:[^\r\n]*").unwrap());
static RX_SCM: LazyLock<BRegex> = LazyLock::new(|| BRegex::new(r"(?:^|[\s,:])scm\[([^\]\r\n]*)\]").unwrap());
static RX_RES: LazyLock<BRegex> = LazyLock::new(|| BRegex::new(r"(?:^|[\s,:])res\[([^\]\r\n]*)\]").unwrap());
static RX_ACT: LazyLock<BRegex> = LazyLock::new(|| BRegex::new(r"(?:^|[\s,:])act\[([^\]\r\n]*)\]").unwrap());

/// A build number: 1 to 10^12, digits only.
fn build_number(b: &[u8]) -> Option<i64> {
    if b.is_empty() || b.len() > 13 || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n: i64 = std::str::from_utf8(b).ok()?.parse().ok()?;
    (1..=1_000_000_000_000).contains(&n).then_some(n)
}

/// The game's version from (the head of) Player.log: the last `PrintGameVersionInfo` line and the last
/// `MatchLuaVerList` after it. None without a valid build.
pub fn game_build_in(raw: &[u8]) -> Option<GameBuild> {
    let line = RX_VERSION_LINE.find_iter(raw).last()?;
    let build = build_number(RX_SCM.captures(line.as_bytes())?.get(1)?.as_bytes())?;
    let mut out = GameBuild { build, patch: None, patch_action: None };
    if let Some(p) = RX_PATCH_LINE.find_iter(&raw[line.end()..]).last() {
        let p = p.as_bytes();
        out.patch = RX_RES.captures(p).and_then(|c| build_number(c.get(1)?.as_bytes()));
        out.patch_action = RX_ACT
            .captures(p)
            .map(|c| c.get(1).unwrap().as_bytes())
            .filter(|a| !a.is_empty() && a.len() <= 24 && a.iter().all(u8::is_ascii_alphabetic))
            .map(|a| String::from_utf8_lossy(a).into_owned());
    }
    Some(out)
}

/// The running game's version from Player.log (its first [`GAME_BUILD_HEAD`] bytes only). None for a missing file
/// or one without a version line.
pub fn game_build(path: &Path) -> Option<GameBuild> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(path).ok()?.take(GAME_BUILD_HEAD).read_to_end(&mut head).ok()?;
    game_build_in(&head)
}

/// What the log watcher reports, with the delay (seconds between the game writing the line and us reading it) when
/// the line had a time.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Uid(String),
    Enter(String, Option<f64>),
    Leave(String, Option<f64>),
    /// An Interlink scene, in the space entered last (None if none yet).
    BossRush(Option<String>, Option<f64>),
    /// A Holo-Battle Sim scene.
    HoloSim(Option<String>, Option<f64>),
}

/// The clock the watcher dates lines with: now, and a local wall-clock time to unix seconds (`time.mktime`).
pub trait Clock: Send {
    fn now(&self) -> f64;
    /// `None` for a date or time that doesn't exist.
    fn local_to_unix(&self, y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Option<f64>;
}

/// Days from 1970-01-01 to a proleptic Gregorian date (`None` for a date that doesn't exist).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> Option<i64> {
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&m) || d < 1 || d > dim[m as usize - 1] {
        return None;
    }
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn valid_hms(h: u32, mi: u32, s: u32) -> bool {
    h <= 23 && mi <= 59 && s <= 61 // strptime's %S allows 60 and 61
}

/// The PC's own clock and time zone.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> f64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
    }
    fn local_to_unix(&self, y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Option<f64> {
        use chrono::TimeZone;
        days_from_civil(y, mo, d)?;
        if !valid_hms(h, mi, s) {
            return None;
        }
        let date = chrono::NaiveDate::from_ymd_opt(y, mo, d)?;
        let t = chrono::Local.from_local_datetime(&date.and_hms_opt(h, mi, 0)?).earliest()?;
        Some(t.timestamp() as f64 + f64::from(s))
    }
}

/// A fixed "now" and UTC wall clock: tests (the reference goldens are made with `time.mktime = calendar.timegm`).
pub struct FixedClock(pub f64);

impl Clock for FixedClock {
    fn now(&self) -> f64 {
        self.0
    }
    fn local_to_unix(&self, y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Option<f64> {
        if !valid_hms(h, mi, s) {
            return None;
        }
        Some((days_from_civil(y, mo, d)? * 86400 + i64::from(h) * 3600 + i64::from(mi) * 60 + i64::from(s)) as f64)
    }
}

/// Follows the end of Player.log and reports scene changes. Never reads the old (huge) part of the file.
pub struct LogWatcher {
    pub path: PathBuf,
    pub uid: Option<String>,
    /// The space the game entered last.
    pub current: Option<String>,
    /// Seconds between the game logging a line and us seeing it (the last 20).
    pub delays: VecDeque<f64>,
    /// The unfinished last line of the newest read.
    pub buf: Vec<u8>,
    pub events: Vec<Event>,
    clock: Box<dyn Clock>,
}

pub const DELAYS: usize = 20;

impl LogWatcher {
    pub fn new(path: PathBuf, clock: Box<dyn Clock>) -> LogWatcher {
        LogWatcher { path, uid: None, current: None, delays: VecDeque::new(), buf: Vec::new(), events: Vec::new(), clock }
    }

    /// The events since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    fn push_delay(&mut self, d: f64) {
        if self.delays.len() == DELAYS {
            self.delays.pop_front();
        }
        self.delays.push_back(d);
    }

    fn delay(&mut self, line: &str) -> Option<f64> {
        let c = RX_TIME.captures(line)?;
        let stamp = c.get(1).unwrap().as_str();
        let (whole, frac) = stamp.split_once('.').unwrap();
        let (date, time) = whole.split_once(' ').unwrap();
        let num = |s: &str| s.parse::<u32>().ok();
        let dp: Vec<&str> = date.split('-').collect();
        let tp: Vec<&str> = time.split(':').collect();
        let y = dp[0].parse::<i32>().ok()?;
        let (mo, d, h, mi, s) = (num(dp[1])?, num(dp[2])?, num(tp[0])?, num(tp[1])?, num(tp[2])?);
        let base = self.clock.local_to_unix(y, mo, d, h, mi, s)?;
        let frac: f64 = format!("0.{frac}").parse().ok()?;
        let t = base + frac;
        let d = self.clock.now() - t;
        if -5.0 < d && d < 600.0 {
            self.push_delay(d);
        }
        Some(d)
    }

    /// Started in the middle of a visit: the last space entered, and whether it is an Interlink or Sim one (the game
    /// writes those scene lines only once, right after entering).
    pub fn look_back(&mut self, size: u64, span: u64) {
        use std::io::{Read, Seek, SeekFrom};
        let tail = (|| {
            let mut f = std::fs::File::open(&self.path).ok()?;
            f.seek(SeekFrom::Start(size.saturating_sub(span))).ok()?;
            let mut tail = Vec::new();
            f.take(span).read_to_end(&mut tail).ok()?;
            Some(tail)
        })();
        let Some(tail) = tail else { return };
        self.look_back_in(&tail);
    }

    pub fn look_back_in(&mut self, tail: &[u8]) {
        let (mut rush, mut sim, mut last) = (HashSet::new(), HashSet::new(), None::<String>);
        for raw in tail.split(|&b| b == b'\n') {
            let m = if has(raw, b"createVoxelScene") {
                RX_ENTER.captures(&String::from_utf8_lossy(raw)).map(|c| c[1].to_string())
            } else {
                None
            };
            if let Some(m) = m {
                last = Some(m);
            } else if has(raw, b"fb_bossrush") && last.is_some() {
                rush.insert(last.clone().unwrap());
            } else if has(raw, b"fb_roguelike") && last.is_some() {
                sim.insert(last.clone().unwrap());
            }
        }
        if let Some(last) = last {
            self.current = Some(last.clone());
            self.events.push(Event::Enter(last.clone(), None));
            if rush.contains(&last) {
                self.events.push(Event::BossRush(Some(last.clone()), None));
            }
            if sim.contains(&last) {
                self.events.push(Event::HoloSim(Some(last), None));
            }
        }
    }

    /// One whole line (bytes, without its newline): the UID, a scene entered or left, an Interlink or Sim scene.
    fn line(&mut self, raw: &[u8]) {
        if has(raw, b"callerId=") || has(raw, b"/prod/") {
            if let Some(uid) = uid_in(raw)
                && Some(&uid) != self.uid.as_ref()
            {
                self.uid = Some(uid.clone());
                self.events.push(Event::Uid(uid));
            }
            if !has(raw, b"createVoxelScene") {
                return;
            }
        }
        if !has(raw, b"createVoxelScene") && !has(raw, b"clearSpace") && !has(raw, b"bossrush") && !has(raw, b"fb_roguelike")
        {
            if has(raw, b"][info]") && self.delays.len() < DELAYS {
                self.delay(&String::from_utf8_lossy(raw));
            }
            return;
        }
        let line = String::from_utf8_lossy(raw).into_owned();
        let d = self.delay(&line);
        if let Some(c) = RX_ENTER.captures(&line) {
            self.current = Some(c[1].to_string());
            self.events.push(Event::Enter(c[1].to_string(), d));
            return;
        }
        if let Some(c) = RX_LEAVE.captures(&line) {
            self.events.push(Event::Leave(c[1].to_string(), d));
            return;
        }
        if line.contains("fb_bossrush") {
            self.events.push(Event::BossRush(self.current.clone(), d));
        } else if line.contains("fb_roguelike") {
            self.events.push(Event::HoloSim(self.current.clone(), d));
        }
    }

    /// New bytes from the log, after what's left of an unfinished line: each whole line with one of the wanted words
    /// goes to `line`, in order; the unfinished end waits for the next read.
    pub fn feed(&mut self, chunk: &[u8]) {
        let mut data = std::mem::take(&mut self.buf);
        data.extend_from_slice(chunk);
        let cut = data.iter().rposition(|&b| b == b'\n');
        self.buf = data[cut.map_or(0, |c| c + 1)..].to_vec();
        let Some(cut) = cut else { return };
        let mut at = 0;
        // chosen once a read, as the Python does (the delays fill up mid-read without switching)
        let rx: &BRegex = if self.delays.len() < DELAYS { &RX_WANTED_INFO } else { &RX_WANTED };
        loop {
            let Some(m) = rx.find_at(&data[..cut], at) else { return };
            let start = data[..m.start()].iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
            let end = m.start() + data[m.start()..].iter().position(|&b| b == b'\n').unwrap();
            self.line(&data[start..end].to_vec());
            at = end + 1;
        }
    }

    /// Reads what the log gained since `pos` (None: start at its end, after looking back). Returns the new position.
    pub fn poll(&mut self, pos: Option<u64>) -> Option<u64> {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(meta) = std::fs::metadata(&self.path) else { return pos };
        let size = meta.len();
        if pos.is_none() {
            self.look_back(size, 16 << 20);
        }
        let mut pos = match pos {
            Some(p) if size >= p => p,
            _ => {
                self.buf.clear();
                size
            }
        };
        if size > pos {
            let read = (|| {
                let mut f = std::fs::File::open(&self.path).ok()?;
                f.seek(SeekFrom::Start(pos)).ok()?;
                let mut chunk = Vec::new();
                f.take((size - pos).min(64 << 20)).read_to_end(&mut chunk).ok()?;
                Some(chunk)
            })();
            if let Some(chunk) = read {
                pos += chunk.len() as u64;
                self.feed(&chunk);
            }
        }
        Some(pos)
    }

    /// The watcher's loop: polls four times a second, sending events, until `stop` is set.
    pub fn run(mut self, events: std::sync::mpsc::Sender<Event>, stop: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        let mut pos = None;
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            pos = self.poll(pos);
            for e in self.take_events() {
                if events.send(e).is_err() {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
}
