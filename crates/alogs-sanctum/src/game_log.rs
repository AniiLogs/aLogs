//! Where the game says the player is, from its own text log (Player.log, read-only and shared). Each area loaded is
//! logged as "createVoxelScene new>>>id:..." right after a connect to that area's server; inside an Egg Heist run the
//! island and the Sanctum keep their ids, and a run starts outside, so the first dungeon area of a run is the island.
//! Port of `GameLog.cs`.

use regex::Regex;
use std::collections::{BTreeSet, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Unknown,
    Elsewhere,
    Island,
    Sanctum,
    Homeland,
}

const PENDING_FOR: f64 = 3.0;
static CONNECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"connect to worldx-[a-z]+-[a-z]+-([a-z]+)-").unwrap());
static SCENE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"createVoxelScene new>>>id:(\S+)").unwrap());
static SANCTUM_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"scene='Level_FB_Random_Map_(\d+)'").unwrap());
static HOMELAND_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+-\d+-550$").unwrap());
static MESH: LazyLock<Regex> = LazyLock::new(|| Regex::new("Convex Mesh from source mesh \"([^\"]+)\"").unwrap());

pub struct GameLog {
    pub place: Place,
    pub available: bool,
    /// The game scene of this run's Sanctum as the game named it while loading (200NN is Layout NN), or None.
    pub sanctum_scene: Option<i64>,
    pub sanctum_named_at: f64,
    pub visit: i64,
    pub mesh_seq: i64,
    pub mesh_at: f64,
    /// Dev switches rock-log and log-island.
    pub rock_log: bool,
    pub log_island: bool,
    /// Changes seen live (not while catching up at startup): (new place, a new run).
    pub changes: Vec<(Place, bool)>,
    meshes: BTreeSet<String>,
    interlink: bool,
    pending_id: Option<String>,
    pending_at: f64,
    outside: HashSet<String>,
    path: PathBuf,
    pos: u64,
    partial: Vec<u8>,
    server: Option<String>,
    island_id: Option<String>,
    sanctum_id: Option<String>,
    catching_up: bool,
    clock: Box<dyn Fn() -> f64 + Send>,
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

impl GameLog {
    pub fn new(path: Option<PathBuf>) -> GameLog {
        let path = path.unwrap_or_else(|| PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("AppData").join("LocalLow").join("Aniimo").join("Aniimo").join("Player.log"));
        GameLog {
            place: Place::Unknown, available: false, sanctum_scene: None, sanctum_named_at: 0.0, visit: 0, mesh_seq: 0, mesh_at: 0.0, rock_log: false,
            log_island: false, changes: vec![], meshes: BTreeSet::new(), interlink: false, pending_id: None, pending_at: 0.0, outside: HashSet::new(),
            path, pos: 0, partial: vec![], server: None, island_id: None, sanctum_id: None, catching_up: true, clock: Box::new(now),
        }
    }

    /// A clock of its own (tests).
    pub fn with_clock(mut self, clock: Box<dyn Fn() -> f64 + Send>) -> GameLog {
        self.clock = clock;
        self
    }

    /// The island id of the current run.
    pub fn run_id(&self) -> Option<&str> {
        self.island_id.as_deref()
    }

    pub fn meshes(&self) -> Vec<String> {
        self.meshes.iter().cloned().collect()
    }

    /// Reads what the game added since last time (the whole log the first time, to know where a run already stands).
    pub fn poll(&mut self) {
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            self.available = false;
            return;
        };
        let len = f.metadata().map_or(0, |m| m.len());
        if len < self.pos {
            // the game restarted and began a new log
            self.pos = 0;
            self.partial.clear();
            self.server = None;
            self.island_id = None;
            self.sanctum_id = None;
            self.pending_id = None;
            self.sanctum_scene = None;
            self.set(Place::Unknown, false);
        }
        self.available = true;
        if self.pos == 0 && len > 8 << 20 {
            self.pos = catch_up_start(&mut f, len);
        }
        if len == self.pos {
            self.catching_up = false;
            self.settle(false);
            return;
        }
        if f.seek(SeekFrom::Start(self.pos)).is_err() {
            return;
        }
        let mut buf = vec![0u8; (len - self.pos).min(4 << 20) as usize];
        let Ok(n) = f.read(&mut buf) else { return };
        self.pos += n as u64;
        self.partial.extend_from_slice(&buf[..n]);
        let Some(last) = self.partial.iter().rposition(|b| *b == b'\n') else { return };
        let text = String::from_utf8_lossy(&self.partial[..last]).into_owned();
        self.partial.drain(..=last);
        for line in text.split('\n') {
            self.line(line);
        }
        if self.pos >= len {
            self.catching_up = false;
        }
        self.settle(false);
    }

    fn line(&mut self, line: &str) {
        let t = (self.clock)();
        if self.rock_log && !self.catching_up && line.contains("Convex Mesh from source mesh") {
            if let Some(m) = MESH.captures(line)
                && self.meshes.insert(m[1].to_string())
            {
                self.mesh_seq += 1;
                self.mesh_at = t;
            }
            return;
        }
        if line.contains("connect to worldx-")
            && let Some(c) = CONNECT.captures(line)
        {
            let s = c[1].to_string();
            self.interlink = false;
            if s != "dungeon" {
                self.pending_id = None;
                self.sanctum_scene = None;
            }
            self.server = Some(s);
            return;
        }
        if line.contains("Level_FB_Random_Map_") {
            if let Some(n) = SANCTUM_NAME.captures(line).and_then(|m| m[1].parse::<i64>().ok())
                && self.place == Place::Sanctum
            {
                self.sanctum_scene = Some(n);
                self.sanctum_named_at = t;
            }
            return;
        }
        if line.contains("Level_BW_GrabEggs") {
            if self.pending_id.is_some() && self.log_island {
                self.settle(true);
            }
            return;
        }
        if line.contains("fb_bossrush") || line.contains("fb_roguelike") {
            self.pending_id = None;
            self.sanctum_scene = None;
            if !self.interlink {
                self.interlink = true;
                self.island_id = None;
                self.sanctum_id = None;
                self.set(Place::Elsewhere, false);
            }
            return;
        }
        if !line.contains("createVoxelScene") {
            return;
        }
        let Some(m) = SCENE.captures(line) else { return };
        let id = m[1].to_string();
        if self.pending_id.as_ref() != Some(&id) {
            self.visit += 1;
            self.meshes.clear();
        }
        let dungeon = self.server.as_deref() == Some("dungeon");
        if !dungeon {
            self.outside.insert(id.clone());
        }
        if !dungeon || self.interlink || self.outside.contains(&id) {
            self.island_id = None;
            self.sanctum_id = None;
            self.pending_id = None;
            self.set(if HOMELAND_ID.is_match(&id) { Place::Homeland } else { Place::Elsewhere }, false);
            return;
        }
        if let Some(p) = &self.pending_id {
            if *p == id {
                return;
            }
            self.settle(true);
        }
        let new_run = match (&self.island_id, &self.sanctum_id) {
            (None, _) => true,
            (Some(i), Some(s)) => id != *i && id != *s,
            (Some(_), None) => false,
        };
        if new_run {
            if self.pending_id.as_ref() != Some(&id) {
                self.pending_id = Some(id);
                self.pending_at = t;
            }
            self.set(Place::Elsewhere, false);
        } else if self.island_id.as_ref() == Some(&id) {
            self.set(Place::Island, false);
        } else {
            self.sanctum_id = Some(id);
            self.set(Place::Sanctum, false);
        }
    }

    /// The screen showed what Egg Heist never has: this dungeon area isn't Egg Heist after all.
    pub fn not_egg_heist(&mut self) {
        self.pending_id = None;
        self.interlink = true;
        self.island_id = None;
        self.sanctum_id = None;
        self.set(Place::Elsewhere, false);
    }

    fn settle(&mut self, force: bool) {
        let Some(p) = self.pending_id.clone() else { return };
        if !force && (self.clock)() - self.pending_at < PENDING_FOR {
            return;
        }
        self.island_id = Some(p);
        self.sanctum_id = None;
        self.pending_id = None;
        self.sanctum_scene = None;
        self.set(Place::Island, true);
    }

    fn set(&mut self, p: Place, new_run: bool) {
        if p == self.place && !new_run {
            return;
        }
        self.place = p;
        if !self.catching_up {
            self.changes.push((p, new_run));
        }
    }
}

/// Where to start reading a long log: the last connect to a server that isn't an Egg Heist instance (at most 256 MB back).
fn catch_up_start(f: &mut std::fs::File, len: u64) -> u64 {
    const BLOCK: u64 = 4 << 20;
    let marker = b"connect to worldx-";
    let stop = len.saturating_sub(256 << 20);
    let mut end = len;
    let mut buf = vec![0u8; BLOCK as usize + 256];
    while end > stop {
        let start = stop.max(end.saturating_sub(BLOCK));
        let want = (buf.len() as u64).min(len - start) as usize;
        if f.seek(SeekFrom::Start(start)).is_err() {
            break;
        }
        let Ok(n) = f.read(&mut buf[..want]) else { break };
        let mut i = n as i64 - marker.len() as i64 - 64;
        while i >= 0 {
            let iu = i as usize;
            if buf[iu] == b'c' && &buf[iu..iu + marker.len()] == marker {
                let server = String::from_utf8_lossy(&buf[iu + marker.len()..(iu + marker.len() + 48).min(n)]).into_owned();
                if !server.contains("-dungeon-") {
                    let mut s = iu;
                    while s > 0 && buf[s - 1] != b'\n' {
                        s -= 1;
                    }
                    return start + s as u64;
                }
            }
            i -= 1;
        }
        end = end.saturating_sub(BLOCK);
    }
    len.saturating_sub(16 << 20)
}
