//! "You're being attacked": while you're tabbed out, the HP number on your HP bar, read 4 times a second from a
//! strip of its own. Port of `hit_watch` and `hp_number` (aLogs B16 `bossbar/fights.py`). The overlay shows the alert
//! while `hit_at` is under 5 s old.

use crate::meter::{Frames, Meter};
use alogs_pil::Image;
use alogs_reader::ocr::Reader;
use alogs_reader::prep;
use indexmap::IndexMap;
use regex::Regex;
use std::sync::LazyLock;

/// Your HP bar at the bottom middle with its number on it, in the 16:9 UI area.
pub const HP_BAR: (f64, f64, f64, f64) = (0.400, 0.915, 0.604, 0.9519);
pub const HIT_HOLD: i64 = 3;
pub const HIT_BIG: f64 = 0.02;
pub const HIT_AGAIN: f64 = 10.0;
pub const HIT_FLIP: f64 = 30.0;

#[derive(Default)]
pub struct HitState {
    seen: Option<(i64, i64)>,
    pending: Option<(i64, i64)>,
    drop: Option<((i64, i64), i64, Image)>,
    read_at: f64,
    moves: Vec<(f64, i64, i64)>,
    drop_at: f64,
    strip_key: Option<(usize, usize, u64, Vec<u8>)>,
    strip_read: Option<(i64, i64)>,
}

/// Your HP as the game writes it on the bar ("12643/12694") in a picture of HP_BAR: (current, max), or None.
pub fn hp_number(ocr: &dyn Reader, strip: &Image, u: f64) -> Option<(i64, i64)> {
    static HP_TEXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d[\d ]*)\s*/\s*(\d[\d ]*)").unwrap());
    let (w, h) = (strip.width as f64, strip.height as f64);
    let mut crops = IndexMap::new();
    crops.insert("hp".to_string(), prep::prep_white(strip, (w * 0.30, h * 0.36, w * 0.70, h * 0.80), u / 2000.0, 4.0));
    let text = ocr.read(&crops, &IndexMap::new(), &IndexMap::new()).get("hp").cloned().unwrap_or_default().replace(['O', 'o'], "0");
    let c = HP_TEXT.captures(&text)?;
    Some((c[1].replace(' ', "").parse().ok()?, c[2].replace(' ', "").parse().ok()?))
}

fn th(n: i64) -> String {
    let s = n.abs().to_string();
    let mut o = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            o.push(',');
        }
        o.push(ch);
    }
    if n < 0 { format!("-{o}") } else { o }
}

impl Meter {
    /// While you're tabbed out: a lower HP read HIT_HOLD times in a row is a drop; a big one, or two within HIT_AGAIN
    /// seconds, shows the alert. A number flipping between two readings is a misread, never a hit.
    pub fn hit_watch(&mut self, frames: &mut dyn Frames, now: f64) {
        let fg = self.hwnd.is_some_and(|w| frames.is_foreground(w));
        if self.args.replay || self.args.no_overlay || self.hwnd.is_none() || fg || self.frame_size.1 == 0 {
            self.hit.seen = None;
            self.hit.pending = None;
            self.hit.drop = None;
            return;
        }
        if now - self.hit.read_at < 0.25 {
            return;
        }
        self.hit.read_at = now;
        let Some(ocr) = self.hp_ocr.as_deref() else { return };
        let (w, h) = (self.frame_size.0 as f64, self.frame_size.1 as f64);
        let u = w.min(h * 16.0 / 9.0);
        let ux = (w - u) / 2.0;
        let area = ((ux + u * HP_BAR.0) / w, HP_BAR.1, (ux + u * HP_BAR.2) / w, HP_BAR.3);
        let Some(strip) = frames.hp_strip(self.hwnd.unwrap(), area) else { return };
        let key = (strip.width, strip.height, u.to_bits(), strip.data.clone());
        let hp = if self.hit.strip_key.as_ref() == Some(&key) {
            self.hit.strip_read
        } else {
            let hp = hp_number(ocr, &strip, u);
            self.hit.strip_key = Some(key);
            self.hit.strip_read = hp;
            hp
        };
        let Some(hp) = hp.filter(|hp| hp.0 <= hp.1 && hp.0 * 10 >= hp.1) else { return };
        let seen = self.hit.seen;
        if seen.is_none_or(|s| hp.1 != s.1 || hp.0 >= s.0) {
            self.hit.drop = None;
            if Some(hp) != self.hit.pending {
                self.hit.pending = Some(hp);
                return;
            }
            if let Some(s) = seen
                && hp != s
            {
                self.hit.moves.push((now, s.0, hp.0));
            }
            self.hit.seen = Some(hp);
            return;
        }
        let seen = seen.unwrap();
        let n = match &self.hit.drop {
            Some((d, n, _)) if *d == hp => n + 1,
            _ => 1,
        };
        self.hit.drop = Some((hp, n, strip.clone()));
        if n < HIT_HOLD {
            return;
        }
        self.hit.drop = None;
        self.hit.seen = Some(hp);
        let mut moves: Vec<(f64, i64, i64)> = self.hit.moves.iter().copied().filter(|m| now - m.0 < HIT_FLIP).collect();
        let pair = |a: i64, b: i64| (a.min(b), a.max(b));
        let flip = moves.iter().any(|m| pair(m.1, m.2) == pair(seen.0, hp.0));
        let last = self.hit.drop_at;
        moves.push((now, seen.0, hp.0));
        self.hit.moves = moves;
        self.hit.drop_at = now;
        let share = (seen.0 - hp.0) as f64 / hp.1 as f64;
        if flip || (share < HIT_BIG && now - last > HIT_AGAIN) {
            log::info!(target: "meter", "tabbed out: HP {} -> {} of {} not taken for a hit ({})", th(seen.0), th(hp.0), th(hp.1),
                if flip { "it flips between these two (a misread)" } else { "a small drop on its own" });
            return;
        }
        if now - self.hit_at > 5.0 {
            log::info!(target: "meter", "tabbed out: HP {} -> {} of {} ({:.1}%), \"You're being attacked\" shown", th(seen.0), th(hp.0), th(hp.1), share * 100.0);
            let name = crate::results::local(now, "%Y%m%d_%H%M%S");
            crate::results::save_picture(&self.env.data, &strip, "hp_alerts", &name, 1280);
            let d = self.env.data.join("logs").join("hp_alerts");
            if let Ok(rd) = std::fs::read_dir(&d) {
                let mut names: Vec<_> = rd.flatten().map(|e| e.file_name()).collect();
                names.sort();
                let n = names.len();
                for old in names.into_iter().take(n.saturating_sub(10)) {
                    let _ = std::fs::remove_file(d.join(old));
                }
            }
        }
        self.hit_at = now;
    }
}
