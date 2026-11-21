//! Who's who: your character's name by UID, teammates' names kept steady across misreads, Party screen pills, and the
//! screen signatures used to notice screens change. Port of those parts of aLogs B16 `bossbar/core.py`.

use alogs_gamelog::difflib::ratio;
use alogs_gamelog::same_name;
use alogs_pil::{Filter, Image, Mode};
use alogs_pyval::Value;
use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

pub const STEADY_SEEN: i64 = 3;
pub const STEADY_ALIKE: f64 = 0.85;

/// characters.json: `{uid: {'name', 'verified'}}` (older versions wrote `{uid: name}`).
pub fn characters_file(data: &Path) -> IndexMap<String, (Option<String>, bool)> {
    let Some(raw) = std::fs::read_to_string(data.join("characters.json")).ok().and_then(|t| Value::json_loads(&t)) else { return IndexMap::new() };
    let Some(d) = raw.as_dict() else { return IndexMap::new() };
    let mut out = IndexMap::new();
    for (uid, v) in d.iter() {
        if !v.truthy() {
            continue;
        }
        let entry = match v {
            Value::Dict(_) => (v.get("name").as_str().map(String::from), v.get("verified").truthy()),
            other => (Some(other.py_str()), false),
        };
        out.insert(uid.py_str(), entry);
    }
    out
}

pub fn known_characters(data: &Path) -> IndexMap<String, String> {
    characters_file(data).into_iter().filter_map(|(u, (n, _))| n.filter(|n| !n.is_empty()).map(|n| (u, n))).collect()
}

pub fn character_verified(data: &Path, uid: Option<&str>) -> bool {
    uid.filter(|u| !u.is_empty()).is_some_and(|u| characters_file(data).get(u).is_some_and(|c| c.1))
}

/// Save the name for this UID (a menu-verified name is never replaced by a guess).
pub fn remember_character(data: &Path, uid: Option<&str>, name: Option<&str>, verified: bool) {
    let (Some(uid), Some(name)) = (uid.filter(|u| !u.is_empty()), name.filter(|n| !n.is_empty())) else { return };
    let mut known = characters_file(data);
    if let Some(old) = known.get(uid) {
        if old.1 && !verified {
            return;
        }
        if old.0.as_deref() == Some(name) && old.1 == verified {
            return;
        }
    }
    known.insert(uid.to_string(), (Some(name.to_string()), verified));
    let v = Value::Dict(
        known
            .iter()
            .map(|(u, (n, ver))| (Value::Str(u.clone()), alogs_pyval::dict([("name", n.clone().map_or(Value::None, Value::Str)), ("verified", Value::Bool(*ver))])))
            .collect(),
    );
    let _ = alogs_protocol::settings::write_whole(&data.join("characters.json"), &json_unicode(&v));
}

/// `json.dumps(v, ensure_ascii=False)`.
pub fn json_unicode(v: &Value) -> String {
    unescape_unicode(&v.json_dumps())
}

/// JSON text with its `\uXXXX` escapes of non-ASCII characters written as the characters (`ensure_ascii=False`).
pub fn unescape_unicode(s: &str) -> String {
    // undo \uXXXX escapes of non-ASCII (pairs for astral characters)
    let re = regex::Regex::new(r"\\u([0-9a-f]{4})(?:\\u(d[c-f][0-9a-f]{2}))?").unwrap();
    re.replace_all(s, |c: &regex::Captures| {
        let hi = u32::from_str_radix(&c[1], 16).unwrap();
        let ch = if let Some(lo) = c.get(2) {
            let lo = u32::from_str_radix(lo.as_str(), 16).unwrap();
            char::from_u32(0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00))
        } else if hi < 0x20 || hi == 0x22 || hi == 0x5c {
            None
        } else {
            char::from_u32(hi)
        };
        ch.map_or_else(|| c[0].to_string(), |ch| ch.to_string())
    })
    .into_owned()
}

/// `str.casefold()` for names (lower case, ß as ss).
pub fn casefold(s: &str) -> String {
    s.to_lowercase().replace('ß', "ss").replace('ς', "σ")
}

/// Teammates' names as seen on the Party screen, kept steady across misreads.
pub struct Teammates {
    pub path: PathBuf,
    pub seen: IndexMap<String, i64>,
}

impl Teammates {
    pub fn load(path: PathBuf) -> Teammates {
        let seen = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| Value::json_loads(&t))
            .and_then(|v| v.as_dict().map(|d| d.iter().filter_map(|(k, v)| alogs_pyval::py_int(v).map(|n| (k.py_str(), n))).collect()))
            .unwrap_or_default();
        Teammates { path, seen }
    }

    /// The name as lookalike letters fold together (accents dropped, 0/O/o, 1/l/I/|).
    pub fn fold(name: &str) -> String {
        let plain: String = name.nfkd().filter(|c| canonical_combining_class(*c) == 0).collect();
        casefold(&plain).chars().map(|c| match c {
            '0' => 'o',
            '1' | 'i' | '|' => 'l',
            c => c,
        }).collect()
    }

    pub fn steady(&self, name: &str) -> String {
        if name.is_empty() || name.chars().count() < 4 || self.seen.get(name).copied().unwrap_or(0) >= 2 {
            return name.to_string();
        }
        let a = Self::fold(name);
        let mut best: Option<(f64, i64, &String)> = None;
        for (k, &n) in &self.seen {
            if n >= STEADY_SEEN && k != name && k.chars().count() >= 4 {
                let cand = (ratio(&a, &Self::fold(k)), n, k);
                if best.is_none_or(|b| (cand.0, cand.1, cand.2) > (b.0, b.1, b.2)) {
                    best = Some(cand);
                }
            }
        }
        if let Some(b) = best.filter(|b| b.0 >= STEADY_ALIKE) {
            log::info!(target: "meter", "teammate: {name:?} read as {:?} (seen {} times)", b.2, b.1);
            return b.2.clone();
        }
        name.to_string()
    }

    pub fn saw(&mut self, name: &str) {
        if name.chars().count() < 2 {
            return;
        }
        *self.seen.entry(name.to_string()).or_insert(0) += 1;
        let v = Value::Dict(self.seen.iter().map(|(k, n)| (Value::Str(k.clone()), Value::Int(*n))).collect());
        let _ = alogs_protocol::settings::write_whole(&self.path, &json_unicode_indent(&v));
    }
}

fn json_unicode_indent(v: &Value) -> String {
    unescape_unicode(&v.json_dumps_indent(1))
}

/// A Party screen pill's read ("2P @ Flora O") as (its slot, the name), the pill being the i-th; None without a name.
pub fn party_pill(pill: &str, i: i64) -> Option<(i64, String)> {
    let lead = regex::Regex::new(r"^[\s.,]+([1-4]\s*[Pp]|[Il]\s*P)").unwrap();
    let pill = lead.replace(pill, "$1").into_owned();
    let glued = regex::Regex::new(r"^\s*([1-4]\s*[Pp]|[Il]\s*P)([^\s/|])").unwrap();
    let pill = glued.replace(&pill, "$1 $2").into_owned();
    let badge_word = regex::Regex::new(r"^(?:[1-4][Pp]|[Il]P)$").unwrap();
    let words: Vec<&str> = regex::Regex::new(r"[\s/|]+")
        .unwrap()
        .split(&pill)
        .filter(|w| {
            let n = w.chars().count();
            let cs: Vec<char> = w.chars().collect();
            !badge_word.is_match(w)
                && (n >= 3 || (n == 2 && cs[0].is_uppercase() && !cs[1].is_uppercase()) || alogs_reader::hangul::has_hangul(w) || alogs_reader::cyrillic::has_cyrillic(w))
        })
        .collect();
    let name: String = words.concat();
    if name.is_empty() {
        return None;
    }
    let badge = regex::Regex::new(r"^\s*([1-4])\s*[Pp]|^\s*([Il])\s*P").unwrap();
    let slot = match badge.captures(&pill) {
        Some(c) => {
            let g = c.get(1).or(c.get(2)).unwrap().as_str();
            let d = if g == "I" || g == "l" { 1 } else { g.parse::<i64>().unwrap() };
            d - 1
        }
        None => i,
    };
    Some((slot, name))
}

/// A 48x27 grey thumbnail of the frame, to notice the screen changing.
pub fn screen_sig(im: &Image) -> Vec<u8> {
    im.resize(48, 27, Some(Filter::Nearest)).convert(Mode::L).tobytes()
}

/// `screen_sig` of the frame's top `top_h` rows, picked straight off the frame.
pub fn top_sig_direct(im: &Image, top_h: f64) -> Vec<u8> {
    im.transform_affine(48, 27, [im.width as f64 / 48.0, 0.0, 0.0, 0.0, top_h / 27.0, 0.0], Filter::Nearest, &[]).convert(Mode::L).tobytes()
}

/// Did the screen change, not just animate? Mean difference per pixel over `limit`.
pub fn screen_changed(a: &[u8], b: &[u8], limit: f64) -> bool {
    let s: u64 = a.iter().zip(b).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    s as f64 / a.len() as f64 > limit
}

pub fn same(a: Option<&str>, b: Option<&str>) -> bool {
    same_name(a, b)
}
