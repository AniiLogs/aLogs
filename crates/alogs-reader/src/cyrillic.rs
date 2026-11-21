//! Cyrillic player names, settled against the game's own font (UI_Font_Europe draws Russian's 66 letters).
//!
//! Many Cyrillic letters are drawn like Latin ones (А В Е К М Н О Р С Т Х, а е о р с у х), so a box's pixels can't
//! say which of the two a name made only of those is: such a name stays as the Latin model reads it. A name is read in
//! Cyrillic only when the Cyrillic model finds a letter in it that no Latin letter looks like (Д, Ж, Я, и...) and the
//! game's font agrees: that letter's ink fits the font's Cyrillic letter, and clearly better than every Latin letter
//! and digit of the same font.

use crate::gamefont::GameFont;
use crate::hangul::{KoreanNames, Letters, MIN_P, NOT_A_LETTER, SPACE, letter_parts};
use alogs_pil::Image;
use std::cell::RefCell;

/// A Cyrillic letter that tells a Cyrillic name: its ink fits the font's letter at least this well...
pub const MIN_FIT: f64 = 0.55;
/// ...and beats every Latin letter and digit of the font by this much.
pub const MARGIN: f64 = 0.1;
/// Every letter of a Cyrillic word fits its character at least this well.
pub const WORD_FIT: f64 = 0.45;
/// The Cyrillic model's box is tidied with specks up to this size gone (the Korean model's 5 thinned я to ч).
pub const SPECK: usize = 3;

/// The Cyrillic letters the game's font draws like a Latin letter or digit: in that font each one's shape fits a
/// Latin one's at 0.75 or more, size aside (the font check compares shapes whatever their size, so м is M, п is n and
/// Б is B), and Ё ё are Ë ë. They never tell a Cyrillic name by themselves.
pub const LOOKALIKES: &str = "АБВЕЁКМНОРСТХавеёкмнопрсух";

/// What a telling letter is weighed against: every Latin letter and digit.
const LATIN: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

pub fn is_cyrillic(c: char) -> bool {
    ('\u{0400}'..='\u{04ff}').contains(&c)
}

pub fn has_cyrillic(t: &str) -> bool {
    t.chars().any(is_cyrillic)
}

/// A Cyrillic letter no Latin letter or digit looks like (Д, Ж, Я, и...).
pub fn telling(c: char) -> bool {
    is_cyrillic(c) && !LOOKALIKES.contains(c)
}

fn one(s: &str) -> Option<char> {
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// The Cyrillic letters a Latin one is drawn like, either case (H: Н and н): what it stands for in a Cyrillic word.
pub fn cyrillic_twins(c: char) -> &'static [&'static str] {
    match c {
        'A' | 'a' => &["А", "а"],
        'B' => &["В", "в", "Б"],
        'C' | 'c' => &["С", "с"],
        'E' | 'e' => &["Е", "е"],
        'H' => &["Н", "н"],
        'K' | 'k' => &["К", "к"],
        'M' | 'm' => &["М", "м"],
        'O' | 'o' => &["О", "о"],
        'P' | 'p' => &["Р", "р"],
        'T' => &["Т", "т"],
        'X' | 'x' => &["Х", "х"],
        'y' => &["у"],
        'n' => &["п"],
        _ => &[],
    }
}

/// One letter as settled: (character, its fit, whether it tells a Cyrillic word, its ink's box, its ink).
type Pick = (String, f64, bool, (usize, usize, usize, usize), Image);

pub struct CyrillicNames {
    /// The font's shapes (KoreanNames' fit, with the European font alone: every letter here is drawn by it).
    shapes: KoreanNames,
    /// The last read's letters and fits, for the log.
    pub last: RefCell<Vec<(String, f64)>>,
}

impl CyrillicNames {
    /// None when the font doesn't draw Cyrillic (then Cyrillic names stay as the Latin model reads them).
    pub fn new(font: GameFont) -> Option<CyrillicNames> {
        let all = "АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдеёжзийклмнопрстуфхцчшщъыьэюя";
        if !all.chars().all(|c| font.glyphs.contains_key(&(c as u32))) {
            return None;
        }
        Some(CyrillicNames { shapes: KoreanNames::new(Some(font), None), last: RefCell::new(vec![]) })
    }

    /// How well a letter's ink is this character (0..1), against the game's font.
    pub fn fit(&self, part: &Image, ch: &str) -> f64 {
        self.shapes.fit(part, ch)
    }

    /// How well a letter's ink is the Latin letter or digit it is most like.
    pub fn best_latin(&self, part: &Image) -> f64 {
        LATIN.chars().map(|c| self.fit(part, &c.to_string())).fold(0.0, f64::max)
    }

    /// The name in Cyrillic when the box holds a Cyrillic word the game's font vouches for, else None. `img` is the
    /// box tidied with `SPECK` (`hangul::tidy_with`), `letters` the Cyrillic model's read of it.
    pub fn read(&self, img: &Image, letters: &Option<Letters>) -> Option<String> {
        self.last.borrow_mut().clear();
        let letters = letters.as_ref().filter(|l| !l.is_empty())?;
        // each letter's characters worth weighing: the model's likely Cyrillic letters, Latin letters and digits
        let cands: Vec<Vec<(String, f64)>> = letters
            .iter()
            .map(|(_, picks)| picks.iter().filter(|p| p.1 >= MIN_P && one(&p.0).is_some_and(|c| is_cyrillic(c) || c.is_ascii_alphanumeric())).cloned().collect())
            .collect();
        if !cands.iter().flatten().any(|(c, _)| one(c).is_some_and(telling)) {
            return None;
        }
        let parts = letter_parts(img, letters)?;
        let mut chosen: Vec<Pick> = Vec::new();
        for (i, picks) in cands.iter().enumerate() {
            let Some((part, b)) = parts.get(&i) else { continue };
            let Some((fit, c)) = picks
                .iter()
                .map(|(c, p)| (self.fit(part, c), *p, c))
                .max_by(|x, y| (x.0 + 0.15 * x.1).total_cmp(&(y.0 + 0.15 * y.1)))
                .map(|(f, _, c)| (f, c))
            else {
                continue;
            };
            if fit < NOT_A_LETTER {
                continue;
            }
            let tells = one(c).is_some_and(telling) && fit >= MIN_FIT && fit - self.best_latin(part) >= MARGIN;
            chosen.push((c.clone(), fit, tells, *b, part.clone()));
        }
        *self.last.borrow_mut() = chosen.iter().map(|x| (x.0.clone(), (x.1 * 100.0).round_ties_even() / 100.0)).collect();
        if chosen.is_empty() {
            return None;
        }
        let mut heights: Vec<usize> = chosen.iter().map(|x| x.3.3 - x.3.1).collect();
        heights.sort_unstable();
        let wide = SPACE * heights[heights.len() / 2] as f64;
        let mut words: Vec<Vec<&Pick>> = vec![vec![&chosen[0]]];
        for w in chosen.windows(2) {
            if (w[1].3.0 as f64 - w[0].3.2 as f64) > wide {
                words.push(vec![]);
            }
            words.last_mut().unwrap().push(&w[1]);
        }
        let cyrillic = |word: &Vec<&Pick>| word.iter().any(|x| x.2) && word.iter().all(|x| x.1 >= WORD_FIT);
        if !words.iter().any(cyrillic) {
            return None;
        }
        // in a Cyrillic word a Latin look-alike is its Cyrillic twin, in the case whose shape fits better
        let in_cyrillic = |x: &Pick| -> String {
            let twins = one(&x.0).map_or(&[][..], cyrillic_twins);
            twins.iter().map(|t| (self.fit(&x.4, t), *t)).max_by(|a, b| a.0.total_cmp(&b.0)).map_or(x.0.clone(), |t| t.1.to_string())
        };
        let badge = regex::Regex::new(r"^[1-4][PpРр]$").unwrap();
        let text = |word: &Vec<&Pick>| word.iter().map(|x| x.0.as_str()).collect::<String>();
        let keep: Vec<String> = words
            .iter()
            .filter_map(|w| {
                if cyrillic(w) {
                    Some(w.iter().map(|x| in_cyrillic(x)).collect())
                } else if badge.is_match(&text(w)) {
                    // the Party screen's slot badge (2P) is Latin, whatever the model took its P for
                    Some(text(w).replace(['Р', 'р'], "P"))
                } else {
                    None
                }
            })
            .collect();
        Some(keep.join(" "))
    }
}

