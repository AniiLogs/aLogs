//! Which game screen is up, and handing it to its reader. Port of `screen_crops`, `screen_spotted`, `party_up`,
//! `read_screen`, `check_battle_data`, `screen_read_due` and the screen box tables in aLogs B16 `bossbar/live.py`.
//!
//! Live, the reads are made on an OCR helper of their own (`screen_bg`, ocr_worker.rs) and handled on a later look;
//! replays and tests read in place. New player names are read in place too (the Python's `names_first` only reads
//! them first on its helper, to shorten the wait).

use crate::meter::{Frames, Meter, Out};
use alogs_gamelog::difflib::ratio;
use alogs_pil::{Image, Mode};
use alogs_reader::prep;
use indexmap::IndexMap;

pub const INK_MIN: u64 = 40;
pub const FULL_READ_S: f64 = 5.0;
pub const FULL_HOLD_S: f64 = 3.0;
pub const PARTY_KEEP_S: f64 = 30.0;

/// (name, preparation: true = text, false = bright, box as shares of the 16:9 UI area).
pub const SCREEN_BOXES: [(&str, bool, (f64, f64, f64, f64)); 17] = [
    ("h", true, (0.065, 0.035, 0.17, 0.08)),
    ("cs", true, (0.35, 0.075, 0.65, 0.155)),
    ("ph", true, (0.39, 0.005, 0.60, 0.045)),
    ("bs", true, (0.785, 0.03, 0.97, 0.09)),
    ("lbl", true, (0.2, 0.425, 0.83, 0.47)),
    ("cc", true, (0.38, 0.07, 0.62, 0.125)),
    ("me", true, (0.657, 0.068, 0.86, 0.104)),
    ("uid", true, (0.657, 0.104, 0.745, 0.134)),
    ("eh", false, (0.03, 0.08, 0.52, 0.17)),
    ("pl", false, (0.455, 0.118, 0.545, 0.195)),
    ("pe", false, (0.44, 0.222, 0.56, 0.278)),
    ("pc", false, (0.055, 0.485, 0.16, 0.555)),
    ("pb", false, (0.05, 0.555, 0.17, 0.595)),
    ("ep", true, (0.72, 0.555, 0.83, 0.605)),
    ("et", true, (0.36, 0.885, 0.64, 0.955)),
    ("es", true, (0.465, 0.73, 0.55, 0.80)),
    ("sv", false, (0.36, 0.198, 0.65, 0.233)),
];
pub const QUICK_BOXES: [&str; 4] = ["h", "cs", "lbl", "cc"];

/// The boxes that can't show where the game log has you.
pub fn place_skip(place: Option<&str>) -> &'static [&'static str] {
    match place {
        Some("interlink") => &["ph", "cc", "pl", "pe", "pc", "pb", "eh", "ep", "es", "et", "sv"],
        Some("holo-sim") => &["ph", "cs", "lbl", "bs", "cc", "pc", "pb", "eh", "ep", "es", "et"],
        Some("egg heist") => &["ph", "cs", "lbl", "bs", "cc", "sv", "pc", "pb"],
        Some("world") => &["cs", "lbl", "sv"],
        _ => &[],
    }
}

pub type Heads = IndexMap<String, String>;

fn letters(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_lowercase()).collect()
}

/// Whether a screen's title starts with this word, by its letters.
pub fn title_is(head: &str, word: &str) -> bool {
    letters(head).starts_with(word)
}

fn g<'a>(h: &'a Heads, k: &str) -> String {
    h.get(k).map_or(String::new(), |s| s.to_lowercase())
}

/// The line under Egg Heist's rank screen ("Tap anywhere to continue").
pub fn rank_line(h: &Heads) -> bool {
    let line = g(h, "et");
    line.contains("continue") && (line.contains("tap") || line.contains("press"))
}

pub fn rank_screen(h: &Heads) -> bool {
    let ep = regex::Regex::new(r"^\s*\d{1,6}\s*/\s*\d{1,6}\s*$").unwrap();
    let es = regex::Regex::new(r"^\s*[xX×]\s*\d{1,6}\s*$").unwrap();
    rank_line(h) || ep.is_match(h.get("ep").map_or("", String::as_str)) || es.is_match(h.get("es").map_or("", String::as_str))
}

/// Which screen `read_screen` would act on, or None (the cue for whole frames).
pub fn screen_spotted(h: &Heads) -> Option<&'static str> {
    let head = g(h, "h");
    let hl = letters(&head);
    let pb = regex::Regex::new(r"view|close").unwrap();
    let tests: [(&str, bool); 14] = [
        ("result", g(h, "lbl").contains("score") && g(h, "lbl").contains("time")),
        ("Holo-Battle Sim result", g(h, "sv").contains("battle time")),
        ("challenge", g(h, "cs").contains("challenge")),
        ("battle prep", g(h, "bs").contains("score")),
        ("Catch Countdown", g(h, "cc").contains("catch")),
        ("Egg Heist rank", head.contains("rank info")),
        ("profile", title_is(&head, "profile")),
        ("Egg Heist", g(h, "eh").contains("adventure") || rank_screen(h)),
        ("party", title_is(&head, "party")),
        ("Holo-Battle", hl.starts_with("holobattlei")),
        ("menu", !g(h, "me").is_empty() && !g(h, "uid").is_empty()),
        ("Battle Data", hl.contains("battle") && hl.contains("dat")),
        ("EXP card", g(h, "pl").contains("level")),
        ("catch card", pb.is_match(&g(h, "pb")) && g(h, "pc").contains('/')),
    ];
    tests.iter().find(|t| t.1).map(|t| t.0)
}

/// The screens' extra state the Python keeps on the Meter.
#[derive(Default)]
pub struct ScreensState {
    pub full_read_at: f64,
    pub full_until: f64,
    pub party_title_at: f64,
    pub party_kept_said: f64,
    pub screen_read_sig: Option<Vec<u8>>,
    /// The frame the read now being handled was made from holds only the boxes (the rest black), and the same for
    /// the read on its way on the helper. The Python marked the picture itself; the look that handles a read is a
    /// later one than the look that started it, so that look's own mark (Meter::parts_frame) says nothing about it.
    pub read_parts: bool,
    pub submitted_parts: bool,
}

impl Meter {
    /// Where the game log has you: interlink, holo-sim, egg heist, world, or None.
    pub fn place(&mut self, now: f64) -> Option<&'static str> {
        if self.args.replay || self.space.is_none() {
            return None;
        }
        if self.bossrush_space {
            return Some("interlink");
        }
        if self.holosim_space {
            return Some("holo-sim");
        }
        if self.sanctum_in_run(now) {
            return Some("egg heist");
        }
        Some("world")
    }

    /// The boxes to read, prepared for OCR (a text box with almost no ink left out).
    pub fn screen_crops(&mut self, im: &Image, quick: bool) -> IndexMap<String, Image> {
        let (w, h) = (im.width as f64, im.height as f64);
        let u = w.min(h * 16.0 / 9.0);
        let ux = (w - u) / 2.0;
        let now = self.env.now();
        let mut skip: &[&str] = &[];
        if !quick && now - self.ss.full_read_at < FULL_READ_S {
            skip = place_skip(self.place(now));
        } else if !quick {
            self.ss.full_read_at = now;
        }
        let mut quick_boxes: Vec<&str> = QUICK_BOXES.to_vec();
        if self.holosim_space {
            quick_boxes.push("sv");
        }
        let mut out = IndexMap::new();
        for (name, text, (x0, y0, x1, y1)) in SCREEN_BOXES {
            if (quick && !quick_boxes.contains(&name)) || skip.contains(&name) {
                continue;
            }
            let b = (ux + u * x0, h * y0, ux + u * x1, h * y1);
            let crop = if text { prep::prep_text(im, b, u / 2000.0, 190, 3.0, 24) } else { prep::prep_bright(im, b, u / 2000.0, 2.0) };
            if text && crop.convert(Mode::L).histogram(None)[..128].iter().sum::<u64>() < INK_MIN {
                continue;
            }
            out.insert(name.to_string(), crop);
        }
        out
    }

    /// Whether this read is of the Party screen (its title, or Environment info just after a title read).
    pub fn party_up(&mut self, h: &Heads, now: f64) -> bool {
        let head = h.get("h").cloned().unwrap_or_default();
        if self.fight.is_some() {
            self.ss.party_title_at = 0.0;
            return false;
        }
        if rank_screen(h) {
            return false;
        }
        if title_is(&head, "party") {
            self.ss.party_title_at = now;
            return true;
        }
        if regex::Regex::new(r"operation|heist").unwrap().is_match(&g(h, "ph")) {
            self.ss.party_title_at = now;
            return true;
        }
        if now - self.ss.party_title_at > PARTY_KEEP_S {
            return false;
        }
        let l = letters(&head);
        if !l.is_empty() && ratio(&l, "party") < 0.6 {
            return false;
        }
        if !regex::Regex::new(r"en[a-z]{0,3}ron").unwrap().is_match(&g(h, "bs")) {
            return false;
        }
        self.ss.party_title_at = now;
        if now - self.ss.party_kept_said > 15.0 {
            self.ss.party_kept_said = now;
            log::info!(target: "meter", "Party screen: still up by Environment info (title read {})", alogs_pyval::str_repr(&head));
        }
        true
    }

    /// The game's screens with text, read about once a second (back to back while a fight's bar is hidden).
    pub fn check_battle_data(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64) {
        // (the automatic Battle Data session reads at once too, as in the Python: its second, agreeing read of Battle
        // Data came 1.5 s after the first, with the inputs held meanwhile)
        let quick = self.auto_active() || (self.fight.is_some() && now - self.bar_seen > 0.5);
        // live, the read is made on a helper of its own and handled on a later look, with the frame it was made from
        if let Some((text, (frame, at))) = self.screen_bg.as_ref().and_then(|w| w.take()) {
            self.ss.read_parts = self.ss.submitted_parts;
            self.read_screen(frames, out, &frame, at, &text);
        }
        if self.screen_bg.as_ref().is_some_and(|w| w.busy()) || !self.frame_full || !self.screen_read_due(now, quick) {
            return;
        }
        let sig = self.top_sig.clone().unwrap_or_else(|| crate::team::top_sig_direct(im, im.height as f64 * crate::meter::TOP_PART));
        self.rs.last_results = now;
        self.diag.load.reads += 1;
        self.ss.screen_read_sig = Some(sig);
        let crops = self.screen_crops(im, quick);
        if crops.is_empty() {
            self.ss.read_parts = self.parts_frame;
            self.read_screen(frames, out, im, now, &IndexMap::new()); // nothing on screen to read
            return;
        }
        if let Some(w) = &self.screen_bg {
            self.ss.submitted_parts = self.parts_frame;
            w.submit(crops, (im.clone(), now));
            return;
        }
        let heads = self.ocr.read(&crops, &IndexMap::new(), &IndexMap::new());
        self.ss.read_parts = self.parts_frame;
        self.read_screen(frames, out, im, now, &heads);
    }

    /// Hand the screen that's up to its reader.
    pub fn read_screen(&mut self, frames: &mut dyn Frames, out: &mut dyn Out, im: &Image, now: f64, heads: &Heads) {
        let heads = &self.rank_only_after_match(heads, now);
        self.egg_rank_watch(frames, out, im, heads, now);
        self.egg_gone(frames, out, now);
        self.screens_gone(now);
        if now - self.egg.sent_at < 45.0 {
            self.keep_sample(im, "egg_after", Some(3.0), Some(15));
        }
        let party = self.party_up(heads, now);
        let spotted = screen_spotted(heads).or(if party { Some("party") } else { None });
        self.game_only_gone(frames, spotted, now);
        if let Some(sp) = spotted.filter(|_| !self.args.replay) {
            self.ss.full_until = now + FULL_HOLD_S;
            let fg = self.hwnd.is_some_and(|w| frames.is_foreground(w));
            self.egg_screen_hold(spotted, heads, now, fg);
            if self.game_only(frames, sp, now) {
                self.rs.last_results = 0.0;
                return;
            }
            // a frame of only the boxes is read again whole at once; whole frames go on until FULL_HOLD_S after. (The
            // frame this read was made from, not this look's: tested by the look's own mark, a screen first seen in a
            // boxes-only frame had its first read, and its proof, made from that mostly black frame.)
            if self.ss.read_parts {
                if now - self.pacing.parts_said > 60.0 {
                    self.pacing.parts_said = now;
                    log::info!(target: "meter", "screen read: {sp} spotted, whole frames while it's up");
                }
                self.rs.last_results = 0.0;
                return;
            }
        }
        let (w, h) = (im.width as f64, im.height as f64);
        let u = w.min(h * 16.0 / 9.0);
        let ux = (w - u) / 2.0;
        self.menu_name(im, u, ux, h, heads);
        self.profile_notes(heads, im, now, u, ux, h);
        let cc = g(heads, "cc");
        if cc.contains("catch") {
            if now - self.catch_seen > 30.0 {
                log::info!(target: "meter", "Catch Countdown on screen: {}", alogs_pyval::str_repr(heads.get("cc").map_or("", String::as_str)));
            }
            self.catch_seen = now;
        }
        let lbl = g(heads, "lbl");
        let is_result = lbl.contains("score") && lbl.contains("time");
        let head = heads.get("h").cloned().unwrap_or_default();
        let hl = letters(&head);
        let is_bd = hl.contains("battle") && hl.contains("dat") && !hl.contains("holo") && !hl.contains("buff");
        if self.auto_active() && !is_result && !is_bd && now - self.auto.hold_started > 4.0 {
            self.end_auto("screen changed");
        }
        if is_result {
            let fg = self.hwnd.is_some_and(|w| frames.is_foreground(w));
            if now - self.rs.result_seen >= 30.0 && !self.args.no_auto_click && !self.args.replay && fg {
                self.start_auto(now); // a new result screen: before anything slow
            }
            // the first click once the screen is wholly read (result_ready), else RESULT_PAUSE in; retries every second
            let ready = self.auto.clicks == 0 && self.rs.result_ready >= self.auto.hold_started;
            if self.auto_active() && (now - self.auto.last_click > 1.0 || ready) {
                if self.auto.clicks < 6 {
                    self.open_battle_data(out, im, u, ux, h, fg);
                } else {
                    self.end_auto("Battle Data did not open after 6 clicks");
                }
            }
            self.test_frame(im, "result", now);
            let names = Self::result_names(im, u, ux, h);
            let banner = heads.get("cs").cloned().unwrap_or_default();
            self.result_screen(frames, out, im, now, u, ux, h, &banner, Some(names));
            return;
        }
        if g(heads, "sv").contains("battle time") {
            self.test_frame(im, "sim_result", now);
            self.sim_result(frames, out, im, now, u, ux, h);
            return;
        }
        if g(heads, "cs").contains("challenge") {
            self.test_frame(im, "challenge", now);
            self.challenge_stats(frames, out, im, now, u, ux, h);
            return;
        }
        if g(heads, "bs").contains("score") {
            let pill = heads.get("bs").cloned().unwrap_or_default();
            self.test_frame(im, "prep", now);
            self.prep_screen(frames, out, im, now, u, ux, h, &pill);
            return;
        }
        if head.to_lowercase().contains("rank info") {
            self.test_frame(im, "egg_rank", now);
            self.egg_rank_info(frames, out, im, now, u, ux, h);
            return;
        }
        if title_is(&head, "profile") {
            self.test_frame(im, "profile", now);
            self.profile_screen(im, now, u, ux, h);
            return;
        }
        if party {
            self.test_frame(im, "party", now);
            self.party_layout(heads.get("ph").map_or("", String::as_str));
            self.party_circle(im, now, u, ux, h);
            // (the Python reads new names first on its helper, names_first: that only shortens the wait, the reads are the same)
            if now - self.ps.party_seen >= 2.0 {
                self.party_screen(im, now, u, ux, h);
            }
            return;
        }
        if hl.starts_with("holobattlei") {
            self.interlink_screen(im, now, u, ux, h);
            return;
        }
        if self.egg_screens(frames, out, im, now, u, ux, h, heads) {
            return;
        }
        if heads.get("me").is_some_and(|v| !v.is_empty()) && heads.get("uid").is_some_and(|v| !v.is_empty()) {
            self.test_frame(im, "menu", now);
        }
        if !is_bd {
            return;
        }
        self.test_frame(im, "battle_data", now);
        // Battle Data is open: the inputs stay held BATTLE_DATA_PAUSE more seconds while it's read again and again. The
        // Python's step, left out of the port: the hold then ran to the session's own limit (up to a minute) whenever
        // Battle Data never read complete or the run wasn't sent.
        if self.auto.auto_until != 0.0 && !self.auto.bd_hold {
            self.auto.bd_hold = true;
            self.auto.auto_until = self.auto.auto_until.min(now + crate::leaderboards::BATTLE_DATA_PAUSE);
            log::info!(target: "meter", "Battle Data is open: holding {} s while it is read", crate::leaderboards::BATTLE_DATA_PAUSE);
        }
        let n = Self::bd_card_count(im, u, ux, h);
        let names = Self::bd_names(im, n, u, ux, h);
        self.battle_data(frames, out, im, now, u, ux, h, &head, Some(names));
    }
}
