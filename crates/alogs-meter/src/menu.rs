//! Your character's name: the game menu (your name with your UID under it), the Profile screen (and its Collection
//! Progress), and Link character (the app's button). Port of those parts of aLogs B16 `bossbar/live.py` and
//! `leaderboards.py` (proof_crop, prove_name, link_poll, link_read).

use crate::meter::Meter;
use crate::screens::Heads;
use crate::team::{character_verified, known_characters, remember_character};
use alogs_pil::ops::{expand, invert};
use alogs_pil::{Filter, Image, Mode};
use alogs_pyval::{Dict, Value};
use alogs_reader::{hangul, names as nr, prep};
use indexmap::IndexMap;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const LINK_FILE: &str = "character_link.json";
pub const LINK_SECONDS: f64 = 120.0;
pub const LINK_READS: usize = 3;
pub const PROOF_BYTES: usize = 1_800_000;
/// Collection Progress on the Profile screen: the value right of each label.
pub const PROFILE_STATS: [(&str, (f64, f64, f64, f64)); 4] = [
    ("aniimoPoint", (0.112, 0.512, 0.150, 0.545)),
    ("sparklingAniimo", (0.234, 0.512, 0.259, 0.545)),
    ("stylePoints", (0.112, 0.560, 0.150, 0.593)),
    ("prismanaAniimo", (0.234, 0.560, 0.259, 0.593)),
];

/// Link character in progress.
#[derive(Clone, Debug)]
pub struct Link {
    pub request: f64,
    pub state: &'static str,
    pub reads: Vec<String>,
    pub name: Option<String>,
    pub uid: Option<String>,
    pub proof: Option<PathBuf>,
}

#[derive(Default)]
pub struct MenuState {
    pub proven: Option<(String, String)>,
    candidate: Option<(String, String)>,
    pub link: Option<Link>,
    stats_candidate: Option<(String, String)>,
    stats_sent: Option<(String, String)>,
    profile_read: f64,
    miss_logged: f64,
    other_logged: f64,
    /// The EXP card's reads while it's up: (time, level, exp, needed).
    exp_card: Vec<(f64, i64, i64, i64)>,
    exp_last: Option<std::collections::HashMap<String, (i64, i64)>>,
    profile_seen: IndexMap<String, f64>,
    level_sent: Option<String>,
}

/// A profile note: ('exp', level, exp, needed) or ('catch', name, level).
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileEvent {
    pub kind: &'static str,
    pub a: String,
    pub b: i64,
    pub c: Option<i64>,
}

fn digits_of(uid_text: &str) -> String {
    uid_text.replace('O', "0").chars().filter(char::is_ascii_digit).collect()
}

pub(crate) fn png_bytes(im: &Image) -> Option<Vec<u8>> {
    let rgb = im.convert(Mode::Rgb);
    let mut out = vec![];
    {
        let mut enc = png::Encoder::new(&mut out, rgb.width as u32, rgb.height as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Best);
        enc.write_header().ok()?.write_image_data(&rgb.data).ok()?;
    }
    Some(out)
}

/// A character proof as `base`.png, or a JPEG (shrunk as needed) when the PNG is over the site's limit.
pub fn save_proof(pic: &Image, base: &Path) -> Option<PathBuf> {
    let with = |ext: &str| PathBuf::from(format!("{}{ext}", base.to_string_lossy()));
    for ext in [".png", ".jpg"] {
        let _ = std::fs::remove_file(with(ext));
    }
    let png = png_bytes(pic)?;
    if png.len() <= PROOF_BYTES {
        std::fs::write(with(".png"), png).ok()?;
        return Some(with(".png"));
    }
    let mut pic = pic.clone();
    let mut quality = 90u8;
    let buf = loop {
        let buf = crate::submit::jpeg(&pic, quality);
        if buf.len() <= PROOF_BYTES || pic.height < 400 {
            break buf;
        }
        if quality > 75 {
            quality -= 5;
        } else {
            pic = pic.resize(pic.width * 4 / 5, pic.height * 4 / 5, Some(Filter::Lanczos));
        }
    };
    std::fs::write(with(".jpg"), buf).ok()?;
    Some(with(".jpg"))
}

impl Meter {
    /// The player's UID, however the game log gave it: also brings back the name saved for it.
    pub fn set_uid(&mut self, uid: &str) {
        if uid.is_empty() || (self.uid.as_deref() == Some(uid) && self.character_name.is_some()) {
            return;
        }
        let changed = self.uid.as_deref() != Some(uid);
        self.uid = Some(uid.to_string());
        self.character_name = known_characters(&self.env.data).get(uid).cloned().or(if changed { None } else { self.character_name.clone() });
        if let Some(n) = &self.character_name {
            log::info!(target: "meter", "character for uid {uid}: {n} (saved)");
            if let Some(s) = &self.submitter {
                s.link_character(uid, n, None);
            }
        }
    }

    /// The menu's name for game_menu: read by the name models when your UID is under it, else the screen read's.
    pub fn menu_name(&mut self, im: &Image, u: f64, ux: f64, h: f64, heads: &Heads) {
        let uid_text = heads.get("uid").cloned().unwrap_or_default();
        let ok = self.uid.as_ref().is_some_and(|u| digits_of(&uid_text).contains(u.as_str()));
        if !ok {
            let me = heads.get("me").cloned().unwrap_or_default();
            self.game_menu(&me, &uid_text, Some(im), u, ux, h, "menu", true);
            return;
        }
        // (the Python reads new names first on its helper, names_first: that only shortens the wait, the reads are the same)
        let b = (ux + u * 0.657, h * 0.068, ux + u * 0.86, h * 0.104);
        let mut crops = IndexMap::new();
        crops.insert("me".to_string(), prep::prep_text(im, b, u / 2000.0, 190, 3.0, 24));
        let mut grey = IndexMap::new();
        grey.insert("me".to_string(), prep::grey_crop(im, b, 48));
        let mut sizes = IndexMap::new();
        sizes.insert("me".to_string(), 24usize);
        let name = self.ocr.read(&crops, &sizes, &grey).get("me").cloned().unwrap_or_default();
        self.game_menu(&name, &uid_text, Some(im), u, ux, h, "menu", true);
    }

    /// The menu or Profile shows your name with your UID: when that UID is the game log's, the name is yours.
    #[allow(clippy::too_many_arguments)]
    pub fn game_menu(&mut self, name: &str, uid_text: &str, im: Option<&Image>, u: f64, ux: f64, h: f64, place: &str, sure: bool) {
        static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
        let digits = digits_of(uid_text);
        let mut name = WS.replace_all(name.trim_matches(|c| c == ' ' || c == '/' || c == '|' || c == '.'), " ").into_owned();
        let mut sure = sure;
        let uid_seen = self.uid.as_ref().is_some_and(|u| digits.contains(u.as_str()));
        if place == "menu" && uid_seen && name.chars().count() <= 2 && !hangul::has_hangul(&name) && u != 0.0
            && let Some(im) = im
        {
            let (n, s) = self.short_name(im, u, ux, h);
            name = n.unwrap_or_default();
            sure = s;
        }
        let n = name.chars().count();
        if !uid_seen || !(1..=40).contains(&n) || (n == 1 && !name.chars().all(char::is_alphanumeric)) {
            let now = self.env.now();
            if uid_seen && now - self.menu.miss_logged > 60.0 {
                self.menu.miss_logged = now;
                log::info!(target: "meter", "game {place}: UID {} on screen but its name was not read (OCR {})", self.uid.clone().unwrap_or_default(), alogs_pyval::str_repr(&name));
            }
            return;
        }
        if !name.chars().next().is_some_and(char::is_alphanumeric) {
            sure = false;
        }
        if place == "profile" && self.menu.link.as_ref().is_some_and(|l| l.state == "waiting") {
            self.link_read(&name, im, u, ux, h);
            return;
        }
        if !sure {
            return;
        }
        let uid = self.uid.clone().unwrap();
        let known = if character_verified(&self.env.data, Some(&uid)) { known_characters(&self.env.data).get(&uid).cloned() } else { None };
        if let Some(k) = known
            && k != name
        {
            let now = self.env.now();
            if now - self.menu.other_logged > 60.0 {
                self.menu.other_logged = now;
                log::info!(target: "meter", "game {place}: read {} with UID {uid}, but your proven name is {} - left as it is (to change it: Link character in the app, with your Profile open)",
                    alogs_pyval::str_repr(&name), alogs_pyval::str_repr(&k));
            }
            return;
        }
        let pair = (uid.clone(), name.clone());
        if self.menu.proven.as_ref() == Some(&pair) {
            return;
        }
        let seen = self.menu.candidate.replace(pair.clone());
        if seen.as_ref() != Some(&pair) {
            return;
        }
        let linking = self.submitter.as_ref().is_some_and(|s| {
            s.refresh();
            s.account.lock().unwrap().as_ref().is_some_and(|a| a.link_character)
        });
        let proof = if linking { im.and_then(|im| self.proof_crop(im, u, ux, h, place, "")) } else { None };
        self.prove_name(&name, place, proof, false);
    }

    /// (name, sure) for a name of one or two characters: the game's font first, else OCR after a drawn label.
    pub fn short_name(&mut self, im: &Image, u: f64, ux: f64, h: f64) -> (Option<String>, bool) {
        let b = (ux + u * 0.657, h * 0.068, ux + u * 0.86, h * 0.104);
        let k = u / 2000.0;
        if let Some(name) = self.name_reader().and_then(|r| r.read(im, b, u, 3, nr::NAME_PX_4K, None, nr::CHARS)) {
            return (Some(name), true);
        }
        let mut crops = IndexMap::new();
        crops.insert("a".to_string(), prep::prep_labelled(im, b, k, "Player name:", 3.0));
        crops.insert("b".to_string(), prep::prep_labelled(im, b, k, "Player name:", 4.0));
        let r = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
        let a = prep::after_label(r.get("a").map(String::as_str), "Player name:");
        let bb = prep::after_label(r.get("b").map(String::as_str), "Player name:");
        let mut name = if !a.is_empty() && (a == bb || bb.is_empty()) {
            Some(a)
        } else if !bb.is_empty() && a.is_empty() {
            Some(bb)
        } else if a.is_empty() {
            None
        } else {
            Some(a)
        };
        if let Some(n) = &name
            && n.chars().count() == 1
            && "0Oo".contains(n.as_str())
            && let Some(aspect) = prep::ink_aspect(im, b, 200)
        {
            name = Some(if aspect < 0.78 { "0".into() } else if n == "o" { "o".into() } else { "O".into() });
        }
        (name, false)
    }

    /// The Profile screen: your name on your name card and your UID at the bottom left.
    pub fn profile_screen(&mut self, im: &Image, now: f64, u: f64, ux: f64, h: f64) {
        if now - self.menu.profile_read < 2.0 {
            return;
        }
        self.menu.profile_read = now;
        let name_box = (ux + u * 0.092, h * 0.139, ux + u * 0.272, h * 0.185);
        let uid_box = (ux + u * 0.03, h * 0.905, ux + u * 0.17, h * 0.945);
        let (ink, bg) = nr::ink_colour(im, name_box, 80, false);
        let mask = ink.and_then(|ink| nr::ink_mask(im, name_box, ink, bg)).map(|m| nr::drop_icon(&m));
        let mut reads = IndexMap::new();
        reads.insert("u".to_string(), prep::prep_text(im, uid_box, u / 2000.0, 190, 3.0, 24));
        if let Some(m) = &mask {
            let k = 2.0 * 2000.0 / u;
            let text = invert(&m.convert(Mode::L)).resize(((m.width as f64 * k) as usize).max(1), ((m.height as f64 * k) as usize).max(1), Some(Filter::Lanczos));
            reads.insert("n".to_string(), expand(&text, 40, &[255]));
        }
        let mut sizes = IndexMap::new();
        sizes.insert("n".to_string(), 40usize);
        let r = self.ocr.read(&reads, &sizes, &IndexMap::new());
        static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
        let mut name = WS.replace_all(r.get("n").map_or("", String::as_str), " ").trim_matches(|c| c == ' ' || c == '/' || c == '|' || c == '.').to_string();
        let mut sure = name.chars().count() > 3 || hangul::has_hangul(&name);
        if let Some(m) = &mask
            && name.chars().count() <= 3
            && !hangul::has_hangul(&name)
            && let Some(matched) = self.name_reader().and_then(|rd| rd.read(im, name_box, u, 3, nr::PROFILE_PX_4K, Some(m.clone()), nr::CHARS))
        {
            name = matched;
            sure = true;
        }
        if name.is_empty()
            && let Some(m) = &mask
        {
            log::info!(target: "meter", "profile: name not read ({}x{} px of text)", m.width, m.height);
        }
        let uid_text = r.get("u").cloned().unwrap_or_default();
        self.game_menu(&name, &uid_text, Some(im), u, ux, h, "profile", sure);
        if self.uid.as_ref().is_some_and(|u| digits_of(&uid_text).contains(u.as_str())) {
            self.profile_stats(im, u, ux, h);
        }
    }

    /// Your own Profile's Collection Progress, kept when two reads in a row agree and sent when it changed.
    pub fn profile_stats(&mut self, im: &Image, u: f64, ux: f64, h: f64) {
        let mut stats: IndexMap<&str, i64> = IndexMap::new();
        let mut unsure = IndexMap::new();
        let have_reader = self.name_reader().is_some();
        for (key, b) in PROFILE_STATS {
            let bx = (ux + u * b.0, h * b.1, ux + u * b.2, h * b.3);
            let (ink, bg) = nr::ink_colour(im, bx, 80, false);
            let mask = if have_reader { ink.and_then(|ink| nr::ink_mask(im, bx, ink, bg)) } else { None };
            let v = mask.and_then(|m| self.name_reader().and_then(|r| r.read(im, bx, u, 7, nr::STATS_PX_4K, Some(m), "0123456789")));
            match v.and_then(|v| v.parse::<i64>().ok()) {
                Some(n) => {
                    stats.insert(key, n);
                }
                None => {
                    unsure.insert(key.to_string(), prep::prep_labelled_dark(im, bx, u / 2000.0, "Count:", 3.0));
                }
            }
        }
        if !unsure.is_empty() {
            static COUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)count:?").unwrap());
            static NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d[\d,]*").unwrap());
            for (key, text) in self.ocr.read(&unsure, &IndexMap::new(), &IndexMap::new()) {
                let v = COUNT.replace_all(&text, "");
                let found: Vec<&str> = NUM.find_iter(&v).map(|m| m.as_str()).collect();
                if found.len() != 1 {
                    return;
                }
                let k = PROFILE_STATS.iter().find(|p| p.0 == key).unwrap().0;
                stats.insert(k, found[0].replace(',', "").parse().unwrap_or(0));
            }
        }
        let mut d = Dict::new();
        for (key, _) in PROFILE_STATS {
            d.insert(key.into(), stats.get(key).map_or(Value::None, |v| Value::Int(*v)));
        }
        if let Some(g) = nr::gender_mark(im, (ux + u * 0.092, h * 0.139, ux + u * 0.272, h * 0.185), 60) {
            d.insert("gender".into(), Value::Str(g.into()));
        }
        if let Some(s) = crate::submit::game_server() {
            d.insert("server".into(), Value::Str(s));
        }
        let uid = self.uid.clone().unwrap_or_default();
        let key = (uid.clone(), Value::Dict(d.clone()).repr());
        let seen = self.menu.stats_candidate.replace(key.clone());
        if seen.as_ref() != Some(&key) || self.menu.stats_sent.as_ref() == Some(&key) {
            return;
        }
        self.menu.stats_sent = Some(key);
        log::info!(target: "meter", "profile: collection {} (UID {uid})", Value::Dict(d.clone()).repr());
        if let Some(s) = &self.submitter {
            s.character_stats(&uid, &Value::Dict(d));
        }
    }

    /// The proof the site keeps, in logs/characters: the top of the menu, or the Profile screen's left column.
    pub fn proof_crop(&self, im: &Image, u: f64, ux: f64, h: f64, place: &str, suffix: &str) -> Option<PathBuf> {
        if u == 0.0 {
            return None;
        }
        let d = self.env.data.join("logs").join("characters");
        std::fs::create_dir_all(&d).ok()?;
        let b = if place == "menu" { (0.6, 0.045, 0.8, 0.14) } else { (0.015, 0.06, 0.28, 0.95) };
        let pic = im.crop_i((ux + u * b.0) as i64, (h * b.1) as i64, (ux + u * b.2) as i64, (h * b.3) as i64).convert(Mode::Rgb);
        save_proof(&pic, &d.join(format!("{}{suffix}", self.uid.clone().unwrap_or_else(|| "None".into()))))
    }

    pub fn prove_name(&mut self, name: &str, place: &str, proof: Option<PathBuf>, force: bool) {
        let uid = self.uid.clone().unwrap_or_default();
        self.menu.proven = Some((uid.clone(), name.to_string()));
        log::info!(target: "meter", "game {place}: {name} with UID {uid} - this is your character");
        self.character_name = Some(name.to_string());
        remember_character(&self.env.data, Some(&uid), Some(name), true);
        if let Some(s) = &self.submitter {
            if force {
                s.forget_linked();
            }
            s.link_character(&uid, name, proof.as_deref());
        }
    }

    /// Link character: the app's request in character_link.json, then the player's confirmation.
    pub fn link_poll(&mut self, now: f64) {
        let req = std::fs::read_to_string(self.env.data.join(LINK_FILE)).ok().and_then(|s| Value::json_loads(&s)).and_then(|v| v.as_dict().cloned());
        let request = req.as_ref().and_then(|r| match r.at("request") {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        });
        let Some(request) = request else {
            if self.menu.link.as_ref().is_some_and(|l| l.state == "waiting" || l.state == "found") {
                log::info!(target: "meter", "link character: cancelled");
            }
            self.menu.link = None;
            return;
        };
        if self.menu.link.as_ref().is_none_or(|l| l.request != request) {
            self.menu.link = Some(Link { request, state: "waiting", reads: vec![], name: None, uid: self.uid.clone(), proof: None });
            log::info!(target: "meter", "link character: asked - waiting for the Profile screen");
        }
        let link = self.menu.link.as_mut().unwrap();
        if link.state == "waiting" && now - request > LINK_SECONDS {
            link.state = "timeout";
            log::info!(target: "meter", "link character: the Profile screen did not show up in time");
        }
        let confirm = req.as_ref().unwrap().at("confirm").clone();
        if link.state == "found"
            && confirm.as_dict().is_some()
            && confirm.get("uid").as_str() == link.uid.as_deref()
            && confirm.get("name").as_str() == link.name.as_deref()
            && link.uid == self.uid
        {
            link.state = "done";
            let (name, proof) = (link.name.clone().unwrap(), link.proof.clone());
            log::info!(target: "meter", "link character: {name} confirmed by the player");
            self.prove_name(&name, "profile", proof, true);
        }
    }

    /// One Profile screen read while Link character waits: found once LINK_READS reads in a row agree.
    fn link_read(&mut self, name: &str, im: Option<&Image>, u: f64, ux: f64, h: f64) {
        let Some(link) = self.menu.link.as_mut() else { return };
        link.reads.push(name.to_string());
        if link.reads.len() > LINK_READS {
            link.reads.remove(0);
        }
        if link.reads.len() == LINK_READS && link.reads.iter().all(|r| r == name) {
            let proof = im.and_then(|im| self.proof_crop(im, u, ux, h, "profile", "-link"));
            let link = self.menu.link.as_mut().unwrap();
            link.state = "found";
            link.name = Some(name.to_string());
            link.uid = self.uid.clone();
            link.proof = proof;
            log::info!(target: "meter", "link character: read {} with UID {} - waiting for the player to confirm", alogs_pyval::str_repr(name), self.uid.clone().unwrap_or_default());
        }
    }
}

impl Meter {
    /// Overworld notifications for the profile, kept in logs/profile_events.csv: the EXP card (level and EXP) and the
    /// catch card (what you caught).
    pub fn profile_notes(&mut self, heads: &Heads, im: &Image, now: f64, u: f64, ux: f64, h: f64) {
        static PE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d[\d,]*)\s*/\s*(\d[\d,]*)").unwrap());
        static LEVEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,3})\s*$").unwrap());
        static CATCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*([A-Za-z][A-Za-z '-]{1,30}?)\s*/\s*L[vVyY]\.?\s*(\d{1,3})").unwrap());
        static VIEW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)view|close").unwrap());
        let g = |k: &str| heads.get(k).cloned().unwrap_or_default();
        let mut events = vec![];
        if g("pl").to_lowercase().contains("level") {
            let pe = g("pe");
            let m = PE.captures(&pe).map(|c| (c[1].replace(',', "").parse::<i64>().unwrap_or(0), c[2].replace(',', "").parse::<i64>().unwrap_or(0)));
            let mut crops = IndexMap::new();
            crops.insert("n".to_string(), prep::prep_labelled(im, (ux + u * 0.47, h * 0.148, ux + u * 0.53, h * 0.192), u / 2000.0, "Level:", 2.0));
            let n = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new()).get("n").cloned().unwrap_or_default();
            if let (Some(lv), Some((cur, need))) = (LEVEL.captures(&n).and_then(|c| c[1].parse::<i64>().ok()), m)
                && cur <= need
            {
                self.menu.exp_card.push((now, lv, cur, need));
                let n = self.menu.exp_card.len();
                if n > 60 {
                    self.menu.exp_card.drain(..n - 60);
                }
            }
        }
        if let Some(ev) = self.exp_settled(now) {
            events.push(ev);
        }
        let pc = g("pc");
        if let Some(c) = CATCH.captures(&pc)
            && VIEW.is_match(&g("pb"))
        {
            events.push(ProfileEvent { kind: "catch", a: c[1].trim().to_string(), b: c[2].parse().unwrap_or(0), c: None });
        }
        for ev in events {
            let key = format!("{}|{}|{}|{:?}", ev.kind, ev.a, ev.b, ev.c);
            if now - self.menu.profile_seen.get(&key).copied().unwrap_or(-1e9) >= 60.0 {
                self.profile_note(&ev);
            }
            self.menu.profile_seen.insert(key, now);
            if ev.kind == "exp"
                && self.submitter.is_some()
                && let Some(uid) = self.uid.clone()
            {
                let sent = format!("{uid}|{}|{}|{:?}", ev.a, ev.b, ev.c);
                if self.menu.level_sent.as_ref() != Some(&sent) {
                    self.menu.level_sent = Some(sent);
                    let mut d = Dict::new();
                    d.insert("level".into(), Value::Int(ev.a.parse().unwrap_or(0)));
                    d.insert("exp".into(), Value::Int(ev.b));
                    d.insert("expNeeded".into(), ev.c.map_or(Value::None, Value::Int));
                    if let Some(server) = crate::submit::game_server() {
                        d.insert("server".into(), Value::Str(server));
                    }
                    self.submitter.as_ref().unwrap().character_stats(&uid, &Value::Dict(d));
                }
            }
        }
        self.menu.profile_seen.retain(|_, t| now - *t < 120.0);
    }

    /// The EXP card's level and EXP once it's gone (no read for 2 s): its highest read at the level most reads agree
    /// on, kept only when it's not behind the last noted value for this character.
    pub fn exp_settled(&mut self, now: f64) -> Option<ProfileEvent> {
        let card = &self.menu.exp_card;
        if card.is_empty() || now - card.last().unwrap().0 < 2.0 {
            return None;
        }
        let card = std::mem::take(&mut self.menu.exp_card);
        let mut counts: IndexMap<i64, i64> = IndexMap::new();
        for r in &card {
            *counts.entry(r.1).or_insert(0) += 1;
        }
        let (level, votes) = counts.iter().fold((0, 0), |b, (l, n)| if *n > b.1 { (*l, *n) } else { b });
        let best = card.iter().filter(|r| r.1 == level).fold(None::<&(f64, i64, i64, i64)>, |b, r| if b.is_none_or(|b| r.2 > b.2) { Some(r) } else { b }).unwrap();
        let (cur, need) = (best.2, best.3);
        let uid = self.uid.clone().unwrap_or_default();
        if self.menu.exp_last.is_none() {
            let mut known = std::collections::HashMap::new();
            if let Ok(text) = std::fs::read_to_string(self.env.data.join("logs").join("profile_events.csv")) {
                let mut rd = csv::ReaderBuilder::new().has_headers(false).flexible(true).from_reader(text.as_bytes());
                for r in rd.records().flatten() {
                    let num = |i: usize| r.get(i).filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit())).and_then(|v| v.parse::<i64>().ok());
                    if r.len() > 5
                        && r.get(2) == Some("exp")
                        && let (Some(a), Some(b)) = (num(3), num(4))
                    {
                        let e = known.entry(r.get(1).unwrap_or("").to_string()).or_insert((0, 0));
                        *e = (*e).max((a, b));
                    }
                }
            }
            self.menu.exp_last = Some(known);
        }
        let known = self.menu.exp_last.as_mut().unwrap();
        if let Some(last) = known.get(&uid)
            && (level < last.0 || (level > last.0 && votes < 2) || (level == last.0 && cur <= last.1))
        {
            return None;
        }
        known.insert(uid, (level, cur));
        Some(ProfileEvent { kind: "exp", a: level.to_string(), b: cur, c: Some(need) })
    }

    fn profile_note(&self, ev: &ProfileEvent) {
        let th = |n: i64| {
            let s = n.to_string();
            let mut o = String::new();
            for (i, ch) in s.chars().enumerate() {
                if i > 0 && (s.len() - i) % 3 == 0 {
                    o.push(',');
                }
                o.push(ch);
            }
            o
        };
        if ev.kind == "exp" {
            log::info!(target: "meter", "profile: exp - level {}, EXP {} / {}", ev.a, th(ev.b), th(ev.c.unwrap_or(0)));
        } else {
            log::info!(target: "meter", "profile: {} - {} Lv {}", ev.kind, ev.a, ev.b);
        }
        let path = self.env.data.join("logs").join("profile_events.csv");
        let new = !path.exists();
        let _ = std::fs::create_dir_all(self.env.data.join("logs"));
        if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let mut w = csv::WriterBuilder::new().terminator(csv::Terminator::CRLF).from_writer(f);
            if new {
                let _ = w.write_record(["when", "uid", "kind", "name_or_level", "level_or_exp", "exp_needed", "exp_gained"]);
            }
            let _ = w.write_record([crate::results::local(self.env.now(), "%Y-%m-%d %H:%M:%S"), self.uid.clone().unwrap_or_default(), ev.kind.to_string(),
                ev.a.clone(), ev.b.to_string(), ev.c.map_or(String::new(), |c| c.to_string()), String::new()]);
        }
    }
}
