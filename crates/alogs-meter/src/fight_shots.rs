//! Fight evidence for an Interlink run the meter followed and submits: up to six pictures of the fight and the fight's
//! own log, sent to the leaderboards as an update of the run (never in its first request), for the site's staff to
//! judge. Nothing is judged here, and there is no switch: it goes whenever the run is submitted.
//!
//! The pictures are the top part of the screen the meter already reads about once a second in a fight (TOP_PART):
//! no new capture request, no second reader, no window copy. The fight's expected length (its time limit: 180 s, 240 s
//! with Overtime) is split into six equal parts, and each part keeps one picture: the look right after the largest rise
//! of the team's damage seen in that part so far. A rise larger than its part's best asks for the next look to be a
//! top-part look (a band look otherwise), at most one such forced look a second. The kept pictures stay in memory as
//! RGB (halved when the source is wider than 1920 px) and are written as JPEGs once the fight is over, off the loop.

use alogs_pil::{Image, Mode};
use alogs_pyval::{Value, dict};
use std::path::{Path, PathBuf};

/// The fight's expected length is split into this many parts, one picture each at most.
pub const PARTS: usize = 6;
/// At most one forced top-part look this often.
pub const FORCE_GAP_S: f64 = 1.0;
/// A picture wider than this is halved.
pub const SHOT_WIDTH: usize = 1920;
pub const SHOT_QUALITY: u8 = 80;
/// The fight's log is sent gzip-compressed, and only when that is at most this many bytes.
pub const FIGHT_LOG_MAX: usize = 400_000;
/// The time limit when the meter knows none.
pub const DEFAULT_LIMIT_S: f64 = 180.0;

/// One kept picture: its part (1..PARTS), its time into the fight and the picture.
pub struct Shot {
    pub n: usize,
    pub t: f64,
    pub im: Image,
}

/// The picker's state during one fight.
#[derive(Default)]
pub struct ShotPicker {
    /// Collecting for this fight (an Interlink fight, live, with submissions on).
    pub on: bool,
    /// The fight's expected length as known at its start (its time limit: 240 s with Overtime, else 180 s).
    pub limit_s: f64,
    /// The largest rise seen in each part so far.
    best: [i64; PARTS],
    last_damage: Option<i64>,
    /// The part the next top-part look is a picture for.
    want: Option<usize>,
    last_forced: Option<f64>,
    /// At most one per part, in the order they were taken.
    pub kept: Vec<Shot>,
    /// How many looks were made top-part looks for a picture.
    pub forced: u64,
    /// Time spent copying pictures, in ms (for the fight's log line).
    pub copy_ms: f64,
}

/// Which part of the fight `elapsed` falls in (0-based), for a fight `limit_s` long.
pub fn part_of(elapsed: f64, limit_s: f64) -> usize {
    let limit = if limit_s > 0.0 { limit_s } else { DEFAULT_LIMIT_S };
    ((elapsed.max(0.0) / (limit / PARTS as f64)).floor() as usize).min(PARTS - 1)
}

/// The top `rows` of a look as RGB, halved when wider than SHOT_WIDTH (a 4K top part becomes 1920 wide, 1440p's
/// 1280; 1080p's stays as it is). The halving is a 2x2 average.
pub fn top_copy(im: &Image, rows: usize) -> Image {
    let rgb;
    let im = if im.mode == Mode::Rgb {
        im
    } else {
        rgb = im.convert(Mode::Rgb);
        &rgb
    };
    let (w, rows) = (im.width, rows.min(im.height));
    if w <= SHOT_WIDTH {
        return Image::from_raw(Mode::Rgb, w, rows, im.data[..w * rows * 3].to_vec());
    }
    let (ow, oh) = (w / 2, rows / 2);
    let stride = w * 3;
    let mut out = vec![0u8; ow * oh * 3];
    for (y, o) in out.chunks_exact_mut(ow * 3).enumerate() {
        let r0 = &im.data[2 * y * stride..(2 * y + 1) * stride];
        let r1 = &im.data[(2 * y + 1) * stride..(2 * y + 2) * stride];
        for x in 0..ow {
            for c in 0..3 {
                let i = 6 * x + c;
                o[3 * x + c] = ((u16::from(r0[i]) + u16::from(r0[i + 3]) + u16::from(r1[i]) + u16::from(r1[i + 3]) + 2) >> 2) as u8;
            }
        }
    }
    Image::from_raw(Mode::Rgb, ow, oh, out)
}

impl ShotPicker {
    /// A new fight: collecting or not.
    pub fn start(&mut self, on: bool) {
        *self = ShotPicker { on, ..Default::default() };
    }

    /// Should this look (a band look otherwise) be a top-part look for a picture?
    pub fn force_due(&self, now: f64) -> bool {
        self.on && self.want.is_some() && self.last_forced.is_none_or(|t| now - t >= FORCE_GAP_S)
    }

    /// This look was made a top-part look for a picture.
    pub fn forced_at(&mut self, now: f64) {
        self.last_forced = Some(now);
        self.forced += 1;
    }

    /// Is a picture wanted from the next look that holds the top part?
    pub fn wants(&self) -> Option<usize> {
        if self.on { self.want } else { None }
    }

    /// A look that holds the top part (`rows` of it), `t` seconds into the fight: kept when a picture is wanted, in
    /// place of its part's earlier one.
    pub fn look(&mut self, im: &Image, rows: usize, t: f64) -> bool {
        let Some(part) = self.wants() else { return false };
        if rows == 0 || im.width == 0 {
            return false;
        }
        self.want = None;
        let t0 = std::time::Instant::now();
        let shot = Shot { n: part + 1, t: alogs_pil::resample::round_digits(t, 1), im: top_copy(im, rows) };
        self.copy_ms += t0.elapsed().as_secs_f64() * 1000.0;
        self.kept.retain(|s| s.n != shot.n);
        self.kept.push(shot);
        true
    }

    /// The team's damage after a look, `elapsed` seconds into a fight `limit_s` long: a rise larger than its part's
    /// best asks for a picture from the next top-part look. `counts`: false on a look whose damage jumped for another
    /// reason (a two-boss fight's hand-over), which only moves the baseline.
    pub fn damage(&mut self, damage: i64, elapsed: f64, limit_s: f64, counts: bool) {
        if !self.on {
            return;
        }
        let rise = self.last_damage.map_or(0, |d| damage - d);
        self.last_damage = Some(damage);
        if !counts || rise <= 0 {
            return;
        }
        let part = part_of(elapsed, limit_s);
        if rise > self.best[part] {
            self.best[part] = rise;
            self.want = Some(part);
        }
    }

    /// The fight is over: its kept pictures by part, and the picker off.
    pub fn finish(&mut self) -> Vec<Shot> {
        let mut kept = std::mem::take(&mut self.kept);
        kept.sort_by_key(|s| s.n);
        self.on = false;
        self.want = None;
        kept
    }
}

/// A fight's evidence once the fight is over: what its summary records (`fight_shots`, `fight_log`).
#[derive(Clone, Debug)]
pub struct FightEvidence {
    pub csv_name: String,
    /// [{"n", "t", "file"}] (paths relative to the data folder).
    pub shots: Value,
    /// {"file", "rows"}: the compressed log to send and the CSV's data rows.
    pub log: Value,
}

/// The picture of part `n` of a fight, relative to the data folder.
pub fn shot_rel(csv_name: &str, n: usize) -> String {
    format!("logs/screens/fight_{csv_name}_{n}.jpg")
}

/// The fight's compressed log (a temporary copy, removed once sent), relative to the data folder.
pub fn log_rel(csv_name: &str) -> String {
    format!("logs/screens/fightlog_{csv_name}.csv.gz")
}

/// What the summary records of a finished fight's kept pictures and log.
pub fn evidence_of(csv_name: &str, shots: &[Shot], rows: u64) -> FightEvidence {
    let list = shots
        .iter()
        .map(|s| dict([("n", Value::Int(s.n as i64)), ("t", Value::Float(s.t)), ("file", Value::Str(shot_rel(csv_name, s.n)))]))
        .collect();
    FightEvidence {
        csv_name: csv_name.to_string(),
        shots: Value::List(list),
        log: dict([("file", Value::Str(log_rel(csv_name))), ("rows", Value::Int(rows as i64))]),
    }
}

/// A file written whole under its name (a reader never sees half of it).
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// gzip of a file's bytes as they are; None (with the reason) when it is missing or comes out over `max` bytes.
pub fn gzip_file(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let raw = std::fs::read(path).map_err(|e| format!("{} is missing ({e})", path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())))?;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&raw).map_err(|e| e.to_string())?;
    let gz = enc.finish().map_err(|e| e.to_string())?;
    if gz.len() > max {
        return Err(format!("{} bytes compressed, over {max}", gz.len()));
    }
    Ok(gz)
}

/// After the fight, off the meter's loop: each kept picture as a JPEG (quality SHOT_QUALITY) and the fight's CSV
/// gzip-compressed beside them (skipped, with one line, when missing or over FIGHT_LOG_MAX).
pub fn write_evidence(data: &Path, csv_name: &str, shots: Vec<Shot>, csv: PathBuf) {
    let t0 = std::time::Instant::now();
    let mut bytes = 0;
    for s in &shots {
        let jpg = crate::submit::jpeg(&s.im, SHOT_QUALITY);
        bytes += jpg.len();
        if let Err(e) = write_whole(&data.join(shot_rel(csv_name, s.n)), &jpg) {
            log::info!(target: "meter", "fight evidence: picture {} not kept: {e}", s.n);
        }
    }
    let log = match gzip_file(&csv, FIGHT_LOG_MAX) {
        Ok(gz) => match write_whole(&data.join(log_rel(csv_name)), &gz) {
            Ok(()) => format!("its log {} KB compressed", gz.len() / 1024),
            Err(e) => format!("its log not kept: {e}"),
        },
        Err(why) => {
            log::info!(target: "meter", "fight evidence: the fight's log is not sent: {why}");
            "no log".into()
        }
    };
    log::info!(target: "meter", "fight evidence: {} pictures ({} KB) and {log}, written in {:.0} ms", shots.len(), bytes / 1024, t0.elapsed().as_secs_f64() * 1000.0);
}
