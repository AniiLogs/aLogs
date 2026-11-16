//! The meter's diagnostics: test pictures and samples (Captures & logs › Save test pictures), how long each screen
//! stayed up, end screens kept after a fight, settings applied without a restart, the tabbed-out note, and whether
//! the meter is idle. Port of those parts of aLogs B16 `bossbar/live.py` and `core.py` (keep_sample).

use crate::meter::Meter;
use crate::results::local;
use alogs_pil::Image;
use alogs_pil::Mode;
use std::collections::HashMap;

pub const TEST_FRAMES_MAX: i64 = 300;
pub const TEST_FRAMES_HOURS: f64 = 3.0;
pub const SAMPLES_KEPT: usize = 5;
pub const SAMPLE_GAP: f64 = 600.0;

fn test_gap(kind: &str) -> f64 {
    match kind {
        "fight" => 5.0,
        "nobar" => 10.0,
        _ => 8.0,
    }
}

/// What the look loop did since the last load line.
#[derive(Default, Clone, Copy)]
pub struct LoadCounts {
    pub whole: i64,
    pub part: i64,
    pub parts: i64,
    pub reads: i64,
}

#[derive(Default)]
pub struct DiagState {
    pub load: LoadCounts,
    pub peak_look: f64,
    test_since: Option<f64>,
    test_last: HashMap<String, f64>,
    test_count: i64,
    /// kind -> (first, last, reads, settled after)
    screens_up: indexmap::IndexMap<String, (f64, f64, i64, Option<f64>)>,
    sample_at: HashMap<String, f64>,
    cfg_checked: f64,
    cfg_stamp: Option<std::time::SystemTime>,
    away_noted: f64,
    away_colours: Option<String>,
}

fn save_in_background(im: &Image, path: std::path::PathBuf, jpeg: bool, prune: Option<(std::path::PathBuf, String, usize)>) {
    let pic = im.convert(Mode::Rgb);
    std::thread::spawn(move || {
        let bytes = if jpeg { Some(crate::submit::jpeg(&pic, 92)) } else { crate::menu::png_bytes(&pic) };
        if let Some(b) = bytes
            && let Err(e) = std::fs::write(&path, b)
        {
            log::info!(target: "meter", "sample picture not kept: {e}");
        }
        if let Some((dir, prefix, keep)) = prune
            && let Ok(rd) = std::fs::read_dir(&dir)
        {
            let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with(&prefix)).collect();
            names.sort();
            let n = names.len();
            for old in names.into_iter().take(n.saturating_sub(keep)) {
                let _ = std::fs::remove_file(dir.join(old));
            }
        }
    });
}

fn hex(p: &[u8]) -> String {
    format!("{:02x}{:02x}{:02x}", p[0], p.get(1).copied().unwrap_or(p[0]), p.get(2).copied().unwrap_or(p[0]))
}

impl Meter {
    fn test_frames_on(&self) -> bool {
        self.args.cfg.get("capture").get("test_frames").truthy()
    }

    /// A few full-size pictures of the screens the readers depend on, kept on this PC (never sent), only while Save
    /// test pictures is on: the newest SAMPLES_KEPT (or `kept`) of each kind.
    pub fn keep_sample(&mut self, im: &Image, kind: &str, gap: Option<f64>, kept: Option<usize>) {
        if !self.test_frames_on() {
            return;
        }
        let now = self.env.now();
        if now - self.diag.sample_at.get(kind).copied().unwrap_or(0.0) < gap.unwrap_or(SAMPLE_GAP) {
            return;
        }
        self.diag.sample_at.insert(kind.to_string(), now);
        let d = self.env.data.join("logs").join("samples");
        if let Err(e) = std::fs::create_dir_all(&d) {
            log::info!(target: "meter", "sample picture not kept: {e}");
            return;
        }
        let jpeg = gap.is_some();
        let path = d.join(format!("{kind}_{}.{}", local(now, "%Y%m%d_%H%M%S"), if jpeg { "jpg" } else { "png" }));
        save_in_background(im, path, jpeg, Some((d, format!("{kind}_"), kept.unwrap_or(SAMPLES_KEPT))));
    }

    fn test_frame_due(&mut self, kind: &str, now: f64) -> bool {
        if !self.test_frames_on() || self.args.replay {
            self.diag.test_since = None;
            return false;
        }
        if self.diag.test_since.is_none() {
            self.diag.test_since = Some(now);
            self.diag.test_last.clear();
            let d = self.env.data.join("logs").join("test_frames").join(local(now, "%Y%m%d"));
            self.diag.test_count = count_files(&d);
            log::info!(target: "meter", "test pictures: saving for {} hours ({} already today)", TEST_FRAMES_HOURS, self.diag.test_count);
        }
        if now - self.diag.test_since.unwrap() > TEST_FRAMES_HOURS * 3600.0 || self.diag.test_count >= TEST_FRAMES_MAX {
            return false;
        }
        now - self.diag.test_last.get(kind).copied().unwrap_or(0.0) >= test_gap(kind)
    }

    /// A screen the meter read: when it first showed and how many reads saw it.
    pub fn screen_up(&mut self, kind: &str, now: f64) {
        let e = self.diag.screens_up.get(kind).copied().unwrap_or((now, now, 0, None));
        self.diag.screens_up.insert(kind.to_string(), (e.0, now, e.2 + 1, e.3));
        self.screens_gone(now);
    }

    /// The reader got what it needed from this screen: how long after it first showed.
    pub fn screen_settled(&mut self, kind: &str, now: f64) {
        if let Some(e) = self.diag.screens_up.get_mut(kind)
            && e.3.is_none()
        {
            e.3 = Some(now - e.0);
        }
    }

    /// Each screen gone for 3 s: one line, how long it stayed up and how many reads it took.
    pub fn screens_gone(&mut self, now: f64) {
        let gone: Vec<String> = self.diag.screens_up.iter().filter(|(_, e)| now - e.1 >= 3.0).map(|(k, _)| k.clone()).collect();
        for kind in gone {
            let (first, last, n, settle) = self.diag.screens_up.shift_remove(&kind).unwrap();
            log::info!(target: "meter", "screen up: {kind} for {:.1} s ({n} read{}{})", last - first, if n == 1 { "" } else { "s" },
                settle.map_or(String::new(), |s| format!(", read after {s:.1} s")));
        }
    }

    /// Save test pictures: whole frames, lossless, for the accuracy tests; logs/test_frames/<day>/<kind>/.
    pub fn test_frame(&mut self, im: &Image, kind: &str, now: f64) {
        if kind != "fight" {
            self.screen_up(kind, now);
        }
        if !self.frame_full || !self.test_frame_due(kind, now) {
            return;
        }
        self.diag.test_last.insert(kind.to_string(), now);
        self.diag.test_count += 1;
        let d = self.env.data.join("logs").join("test_frames").join(local(now, "%Y%m%d")).join(kind);
        if std::fs::create_dir_all(&d).is_err() {
            return;
        }
        let path = d.join(format!("{}_{:03}_{}x{}.png", local(now, "%H%M%S"), self.diag.test_count, im.width, im.height));
        save_in_background(im, path, false, None);
        if self.diag.test_count >= TEST_FRAMES_MAX {
            log::info!(target: "meter", "test pictures: {TEST_FRAMES_MAX} today, no more");
        }
    }

    /// For 45 s after a fight: a full frame every 3 s to logs/end_screens/<fight>/.
    pub fn save_end_screens(&mut self, im: &Image, now: f64) {
        let Some(a) = self.after_fight.clone() else { return };
        if !self.args.cfg.get("capture").get("end_screens").truthy() {
            return;
        }
        if now > a.1 {
            self.after_fight = None;
            return;
        }
        if now - a.2 < 3.0 {
            return;
        }
        self.after_fight = Some((a.0.clone(), a.1, now));
        let folder = format!("end_screens/{}", a.0);
        let name = local(now, "%H%M%S");
        let (data, pic) = (self.env.data.clone(), im.clone());
        std::thread::spawn(move || {
            crate::results::save_picture(&data, &pic, &folder, &name, 1920);
        });
    }

    /// Settings changed in the app apply without a restart: read again when the file changes.
    pub fn reload_settings(&mut self, now: f64) {
        if now - self.diag.cfg_checked < 1.0 || self.args.replay {
            return;
        }
        self.diag.cfg_checked = now;
        let Ok(stamp) = std::fs::metadata(alogs_protocol::settings::settings_file(&self.env.data)).and_then(|m| m.modified()) else { return };
        if self.diag.cfg_stamp.is_none_or(|s| s == stamp) {
            if self.diag.cfg_stamp.is_none() {
                self.args.cfg = alogs_protocol::settings::load(&self.env.data);
                self.args.no_auto_click = !alogs_protocol::settings::automation(&self.args.cfg, Some("auto_open_battle_data"));
                self.args.no_debug_frames = !self.args.cfg.get("capture").get("debug_frames").truthy();
            }
            self.diag.cfg_stamp = Some(stamp);
            return;
        }
        self.diag.cfg_stamp = Some(stamp);
        self.args.cfg = alogs_protocol::settings::load(&self.env.data);
        self.args.no_auto_click = !alogs_protocol::settings::automation(&self.args.cfg, Some("auto_open_battle_data"));
        self.args.no_debug_frames = !self.args.cfg.get("capture").get("debug_frames").truthy();
        log::info!(target: "meter", "settings changed - applied");
    }

    /// Every 3 s while you're tabbed out during a fight: what the meter gets, and how long ago it saw the boss bar.
    pub fn away_note(&mut self, im: Option<&Image>, foreground: bool) {
        let now = self.env.now();
        let Some(f) = &self.fight else { return };
        if self.hwnd.is_none() || self.args.replay || foreground || now - self.diag.away_noted < 3.0 {
            return;
        }
        self.diag.away_noted = now;
        let picture = match im {
            None => "no picture".to_string(),
            Some(im) => {
                let g = im.convert(Mode::L).reduce((im.width / 64).max(1));
                let hi = g.data.iter().copied().max().unwrap_or(0);
                format!("a picture from the app's capture{}", if hi < 20 { " (black)" } else { "" })
            }
        };
        log::info!(target: "meter", "tabbed out mid-fight ({}): {picture}, boss bar seen {:.1} s ago", f.title, now - self.bar_seen);
        if let (Some(im), Some((span, yc))) = (im, f.bar_geom)
            && now - self.bar_seen > 1.0
            && self.diag.away_colours.as_deref() != Some(f.csv_name.as_str())
        {
            self.diag.away_colours = Some(f.csv_name.clone());
            let x0 = im.width as f64 / 2.0 - span / 2.0;
            let y = (yc as usize).min(im.height - 1);
            let row: Vec<String> = (0..9).map(|k| hex(&im.getpixel(((x0 + span * k as f64 / 8.0) as usize).min(im.width - 1), y))).collect();
            let rgb = im.convert(Mode::Rgb).reduce((im.width / 32).max(1));
            let mean = alogs_pil::ops::Stat::new(&rgb, None).mean();
            let m: Vec<u8> = mean.iter().map(|v| v.round_ties_even() as u8).collect();
            log::info!(target: "meter", "tabbed out, no bar: colours along its row (y {yc:.0}) {}; picture {}x{}, average {}", row.join(" "), im.width, im.height, hex(&m));
        }
    }

    /// Nothing on screen to keep up with: no fight or bar, no automatic clicks, not in Interlink, no end screens to save.
    pub fn idle(&mut self) -> bool {
        let now = self.env.now();
        !self.args.replay
            && self.fight.is_none()
            && self.bar.is_none()
            && !self.bossrush_space
            && self.auto.auto_until == 0.0
            && self.sim.tour.is_none()
            && self.egg.hold.is_none()
            && !self.after_fight.as_ref().is_some_and(|a| now < a.1)
            && !self.sanctum_in_run(now)
    }
}

fn count_files(d: &std::path::Path) -> i64 {
    let Ok(rd) = std::fs::read_dir(d) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if !p.to_string_lossy().contains("numbers") {
                n += count_files(&p);
            }
        } else {
            n += 1;
        }
    }
    n
}

pub const BAR_LOST_S: f64 = 60.0;
pub const BAR_LOST_EACH: i64 = 12;
pub const BAR_LOST_MAX: i64 = 60;

#[derive(Default)]
pub struct BarDiag {
    pub sampled: Option<String>,
    pub last_nobar: f64,
    pub nobar_count: i64,
    pub lost_at: Option<f64>,
    pub lost_last: f64,
    pub lost_n: i64,
    pub lost_total: i64,
}

impl Meter {
    /// One full-size frame per fight with the boss bar on it (logs/bar_samples/), with Debug frames on.
    pub fn bar_sample(&mut self, im: &Image, now: f64) {
        let Some(f) = &self.fight else { return };
        if self.bars.sampled.as_deref() == Some(f.csv_name.as_str()) || now - f.created < 8.0 || !self.args.cfg.get("capture").get("debug_frames").truthy() {
            return;
        }
        if matches!(f.brk.phase, "broken" | "refill") {
            return;
        }
        self.bars.sampled = Some(f.csv_name.clone());
        let d = self.env.data.join("logs").join("bar_samples");
        if std::fs::create_dir_all(&d).is_ok() {
            save_in_background(im, d.join(format!("{}.png", f.csv_name)), false, None);
        }
    }

    /// In an Interlink room but no boss bar found: a few full frames, with why.
    pub fn save_nobar(&mut self, im: &Image, now: f64) {
        if !self.args.cfg.get("capture").get("nobar_frames").truthy() || !self.bossrush_space {
            return;
        }
        if now - self.bars.last_nobar < 5.0 || self.bars.nobar_count >= 30 {
            return;
        }
        self.bars.last_nobar = now;
        self.bars.nobar_count += 1;
        let name = local(now, "%Y%m%d_%H%M%S");
        crate::results::save_picture(&self.env.data, im, "nobar", &name, 1920);
        let why = alogs_reader::bar::LAST_FAIL.lock().unwrap().clone();
        let _ = std::fs::write(self.env.data.join("logs").join("nobar").join(format!("{name}.txt")), format!("{why}\n"));
    }

    /// For a minute after a boss bar was lost (outside Interlink): the top of the screen every 5 s, with why.
    pub fn save_bar_lost(&mut self, im: &Image, now: f64) {
        if !self.args.cfg.get("capture").get("nobar_frames").truthy() || self.bossrush_space {
            return;
        }
        let b = &mut self.bars;
        if b.lost_at.is_none_or(|t| now - t > BAR_LOST_S) || now - b.lost_last < 5.0 || b.lost_n >= BAR_LOST_EACH || b.lost_total >= BAR_LOST_MAX {
            return;
        }
        b.lost_last = now;
        b.lost_n += 1;
        b.lost_total += 1;
        let h = if self.frame_size.1 != 0 { self.frame_size.1 } else { im.height };
        let top = im.crop_i(0, 0, im.width as i64, im.height.min((h as f64 * crate::meter::TOP_PART) as usize) as i64);
        let name = local(now, "%Y%m%d_%H%M%S");
        crate::results::save_picture(&self.env.data, &top, "bar_lost", &name, 1920);
        let why = alogs_reader::bar::LAST_FAIL.lock().unwrap().clone();
        let _ = std::fs::write(self.env.data.join("logs").join("bar_lost").join(format!("{name}.txt")), format!("{why}\n"));
    }
}
