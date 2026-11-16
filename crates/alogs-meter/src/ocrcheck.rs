//! Windows OCR's language: a note at start (with a warning when it isn't English), and a look every OCR_RECHECK
//! seconds for English added while the meter runs, which restarts the meter's OCR helpers in place. Port of
//! `ocr_language_note` (aLogs B16 `bossbar/core.py`) and `ocr_recheck` / `ocr_look` / `ocr_warn` (`live.py`).

use crate::meter::Meter;
use alogs_reader::ocr::{Ocr, Reader, ocr_english};
use std::sync::mpsc::Receiver;

pub const OCR_RECHECK: f64 = 60.0;
pub const OCR_SAY_AGAIN: f64 = 3600.0;
pub const NO_ENGLISH_READS: &str = "boss names and levels, result screens, Battle Data, the battle prep and Party screens";
const FIX: &str = "Windows Settings > Time & language > Language (Language & region on Windows 11) > Add a language > English (United States), or Install English OCR on aLogs' DPS Meter page";

#[derive(Default)]
pub struct OcrCheck {
    pub checked: f64,
    probe: Option<Receiver<Option<String>>>,
    said: indexmap::IndexMap<String, (String, f64)>,
}

/// Once at start, in meter.log: the language Windows OCR reads in, with a warning when it isn't English or there's none.
pub fn ocr_language_note(ocr: &dyn Reader) {
    let lang = ocr.language();
    if !ocr.started() {
        log::warn!(target: "meter", "Windows OCR: none (its helper didn't start): {NO_ENGLISH_READS} can't be read, so runs aren't recorded or sent");
    } else {
        match lang.as_deref() {
            None => log::info!(target: "meter", "Windows OCR: language not said (an OCR helper from before this version: ocr-helper.exe or ocr_win.ps1)"),
            Some(l) if ocr_english(Some(l)) == Some(true) => log::info!(target: "meter", "Windows OCR: {l}"),
            Some(l) if !l.is_empty() => log::warn!(target: "meter", "Windows OCR: {l}, not English: {NO_ENGLISH_READS} can't be read reliably, so runs may not be recorded or sent. Fix: {FIX}"),
            Some(_) => log::warn!(target: "meter", "Windows OCR: none (no OCR language on this PC): {NO_ENGLISH_READS} can't be read, so runs aren't recorded or sent. Fix: {FIX}"),
        }
    }
}

impl Meter {
    /// The meter's OCR readers that don't read English (its own, then the HP number's).
    fn ocr_not_english(&self) -> Vec<&'static str> {
        let mut out = vec![];
        if ocr_english(self.ocr.language().as_deref()) == Some(false) {
            out.push("the meter's own");
        }
        if let Some(h) = &self.hp_ocr
            && ocr_english(h.language().as_deref()) == Some(false)
        {
            out.push("the HP number's");
        }
        out
    }

    /// Without English OCR: has Windows got it since? A helper started every OCR_RECHECK seconds says; English, and
    /// every helper without it restarts in place.
    pub fn ocr_recheck(&mut self, now: f64) {
        if let Some(rx) = &self.ocr_check.probe
            && let Ok(language) = rx.try_recv()
        {
            self.ocr_check.probe = None;
            self.ocr_look_done(language, now);
        }
        if !self.ocr.started() || self.ocr_not_english().is_empty() || now - self.ocr_check.checked < OCR_RECHECK || self.ocr_check.probe.is_some() {
            return;
        }
        self.ocr_check.checked = now;
        let exe = self.env.res.join("ocr-helper.exe");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok(look) = Ocr::new(&exe) {
                let _ = tx.send(look.language());
            }
        });
        self.ocr_check.probe = Some(rx);
    }

    fn ocr_look_done(&mut self, language: Option<String>, now: f64) {
        if ocr_english(language.as_deref()) != Some(true) {
            return;
        }
        let short = self.ocr_not_english();
        let was = if short.first() == Some(&"the HP number's") { self.hp_ocr.as_ref().and_then(|h| h.language()) } else { self.ocr.language() };
        let mut said = indexmap::IndexMap::new();
        for whose in &short {
            let r = match *whose {
                "the meter's own" => self.ocr.restart(),
                _ => self.hp_ocr.as_ref().map_or(Ok(()), |h| h.restart()),
            };
            if let Err(e) = r {
                said.insert(whose.to_string(), format!("OCR helper did not restart ({whose}): {e}"));
            }
        }
        let short = self.ocr_not_english();
        if !short.is_empty() {
            let langs: Vec<String> = short.iter().map(|w| {
                let l = if *w == "the meter's own" { self.ocr.language() } else { self.hp_ocr.as_ref().and_then(|h| h.language()) };
                format!("{w}: {}", l.filter(|l| !l.is_empty()).unwrap_or_else(|| "none".into()))
            }).collect();
            said.insert("still".into(), format!("Windows OCR: English was added while the meter ran, but not every OCR helper restarted with it ({}), so the game's text isn't all read yet: another look in {OCR_RECHECK} s", langs.join(", ")));
            self.ocr_warn(said, now);
            return;
        }
        self.ocr_check.said.clear();
        log::info!(target: "meter", "Windows OCR: {} now (was {}): English was added while the meter ran, so its OCR helpers restarted and the game's screens are read",
            self.ocr.language().unwrap_or_default(), was.filter(|w| !w.is_empty()).unwrap_or_else(|| "none".into()));
    }

    /// A look's warnings in meter.log when the last look didn't give that line, and again OCR_SAY_AGAIN on.
    fn ocr_warn(&mut self, said: indexmap::IndexMap<String, String>, now: f64) {
        let last = std::mem::take(&mut self.ocr_check.said);
        for (which, line) in said {
            if let Some(was) = last.get(&which)
                && was.0 == line
                && now - was.1 < OCR_SAY_AGAIN
            {
                self.ocr_check.said.insert(which, was.clone());
                continue;
            }
            log::warn!(target: "meter", "{line}");
            self.ocr_check.said.insert(which, (line, now));
        }
    }
}
