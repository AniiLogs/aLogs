//! Turning OCR text into values: the boss title and level, the xN counter, the countdown. Port of `parse_title`,
//! `parse_title_pair`, `strip_sparkling`, `parse_xn`, `parse_timer` and `norm` in aLogs B16 `bossbar/reader.py`.

use alogs_gamelog::difflib::ratio;
use regex::Regex;
use std::sync::LazyLock;

/// Lower-case letters only.
pub fn norm(s: Option<&str>) -> String {
    s.unwrap_or("").to_lowercase().chars().filter(|c| c.is_ascii_lowercase()).collect()
}

fn digit_fix(c: char) -> char {
    match c {
        'O' | 'o' | 'Q' | 'D' => '0',
        'I' | 'l' => '1',
        'S' => '5',
        'B' => '8',
        'Z' => '2',
        'G' => '6',
        c => c,
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `(level, title)` from the title's OCR text ("Lv.60 Young Omega Sherro").
pub fn parse_title(t: Option<&str>) -> (Option<i64>, String) {
    static RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"L\s*[vVyYnN]\s*[.•·,]?\s*([0-9OoQDIlSBZG]+)").unwrap());
    let t = t.unwrap_or("");
    let m = RX.captures(t);
    let mut digits: String = m.as_ref().map_or(String::new(), |c| c[1].chars().map(digit_fix).collect());
    if digits.chars().count() > 1 {
        let s = digits.trim_start_matches('0');
        digits = if s.is_empty() { "0".into() } else { s.into() };
    }
    // (a symbol glued right after the digits, "Lv.6% Alpha", is no clean read: the "5" of 65 came
    // out as "%" and the fight ran on Lv 6's HP; a space, the end or a letter may follow)
    let glued = m.as_ref().is_some_and(|c| t[c.get(0).unwrap().end()..].chars().next().is_some_and(|n| !(n.is_whitespace() || n.is_alphanumeric())));
    let mut level = if !glued && !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        digits.chars().take(2).collect::<String>().parse::<i64>().ok()
    } else {
        None
    };
    if level.is_some_and(|l| !(1..=80).contains(&l)) {
        level = None;
    }
    let rest = match &m {
        Some(c) => &t[c.get(0).unwrap().end()..],
        None => t,
    };
    let title = rest.trim_matches(|c| c == ' ' || c == '|' || c == 'I' || c == 'l');
    (level, strip_sparkling(&collapse_ws(title)))
}

/// `parse_title` of the white read; when it has no level, the level from the red read.
pub fn parse_title_pair(white: Option<&str>, red: Option<&str>) -> (Option<i64>, String) {
    let (mut level, title) = parse_title(white);
    if level.is_none()
        && let Some(r) = red.filter(|r| !r.is_empty())
    {
        level = parse_title(Some(r)).0;
    }
    (level, title)
}

/// "Sparkling- Young Omega Sherro" -> "Young Omega Sherro" (the word is often garbled).
pub fn strip_sparkling(title: &str) -> String {
    static PATS: LazyLock<[(Regex, f64); 2]> = LazyLock::new(|| {
        [
            (Regex::new(r"^\s*([^-–—~]{1,13}?)\s*[-–—~]+\s*(\S.*)").unwrap(), 0.5),
            (Regex::new(r"^\s*(\S{6,12})\s+(\S.*)").unwrap(), 0.7),
        ]
    });
    for (rx, need) in PATS.iter() {
        if let Some(c) = rx.captures(title) {
            let head = norm(Some(&c[1]));
            if !head.is_empty() && ratio(&head, "sparkling") >= *need {
                return c[2].to_string();
            }
        }
    }
    title.to_string()
}

fn xn_fix(c: char) -> char {
    match c {
        'l' | 'I' | 'i' | '|' => '1',
        'O' | 'o' | 'D' | 'Q' => '0',
        'S' | 's' => '5',
        c => c,
    }
}

/// The matches of `[xX×]\s*([0-9lIi|OoDQSs]{1,2})(?![0-9])`, as Python's `finditer` finds them.
fn xn_matches(t: &str) -> Vec<String> {
    let cs: Vec<char> = t.chars().collect();
    let cls = |c: char| c.is_ascii_digit() || "lIi|OoDQSs".contains(c);
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if matches!(cs[i], 'x' | 'X' | '×') {
            let mut j = i + 1;
            while j < cs.len() && cs[j].is_whitespace() {
                j += 1;
            }
            let mut n = 0;
            while n < 2 && j + n < cs.len() && cls(cs[j + n]) {
                n += 1;
            }
            // greedy two, then one, each needing no digit after it
            let mut hit = None;
            for k in (1..=n).rev() {
                let next = cs.get(j + k);
                if !next.is_some_and(|c| c.is_ascii_digit()) {
                    hit = Some(k);
                    break;
                }
            }
            if let Some(k) = hit {
                out.push(cs[j..j + k].iter().collect());
                i = j + k;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Majority vote over the repeated copies ("x21 / x21 / x21"), x1 up to `top`.
pub fn parse_xn(t: Option<&str>, top: i64) -> Option<i64> {
    let mut votes: Vec<i64> = Vec::new();
    for s in xn_matches(t.unwrap_or("")) {
        let s: String = s.chars().map(xn_fix).collect();
        if !s.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let mut n: i64 = s.parse().unwrap();
        if n > top && s.len() == 2 {
            n = s.replace('7', "1").parse().unwrap();
        }
        if 1 <= n && n <= top {
            votes.push(n);
        }
    }
    // max(set(votes), key=votes.count): the most votes; among equals, the first in the set's own order
    let mut best: Option<(usize, i64)> = None;
    for v in py_set_order(&votes) {
        let c = votes.iter().filter(|&&x| x == v).count();
        if best.is_none_or(|b| c > b.0) {
            best = Some((c, v));
        }
    }
    best.map(|b| b.1)
}

/// The countdown in seconds from "m:ss" (also "m.ss").
pub fn parse_timer(t: Option<&str>) -> Option<i64> {
    let cs: Vec<char> = t.unwrap_or("").chars().collect();
    let dig = |i: usize| cs.get(i).is_some_and(|c| c.is_ascii_digit());
    for start in 0..cs.len() {
        if !dig(start) || (start > 0 && dig(start - 1)) {
            continue;
        }
        for n in [2usize, 1] {
            if !(0..n).all(|k| dig(start + k)) {
                continue;
            }
            let mut j = start + n;
            while j < cs.len() && cs[j].is_whitespace() {
                j += 1;
            }
            if !(j < cs.len() && (cs[j] == ':' || cs[j] == '.')) {
                continue;
            }
            j += 1;
            while j < cs.len() && cs[j].is_whitespace() {
                j += 1;
            }
            if dig(j) && dig(j + 1) && !dig(j + 2) {
                let m: i64 = cs[start..start + n].iter().collect::<String>().parse().unwrap();
                let s: i64 = cs[j..j + 2].iter().collect::<String>().parse().unwrap();
                return Some(m * 60 + s);
            }
        }
    }
    None
}

/// The iteration order of `set(values)` in CPython for small non-negative ints (their hash is themselves): open
/// addressing from slot `hash & mask`, nine linear probes, then perturbed probing; the table grows past 60% full.
pub fn py_set_order(values: &[i64]) -> Vec<i64> {
    fn insert(table: &mut [Option<i64>], v: i64) -> bool {
        let mask = table.len() as u64 - 1;
        let mut perturb = v as u64;
        let mut i = (v as u64) & mask;
        loop {
            let probes = if i + 9 <= mask { 9 } else { 0 };
            for j in 0..=probes {
                match table[(i + j) as usize] {
                    None => {
                        table[(i + j) as usize] = Some(v);
                        return true;
                    }
                    Some(x) if x == v => return false,
                    _ => {}
                }
            }
            perturb >>= 5;
            i = (i * 5 + 1 + perturb) & mask;
        }
    }
    let mut table: Vec<Option<i64>> = vec![None; 8];
    let mut used = 0usize;
    for &v in values {
        if insert(&mut table, v) {
            used += 1;
            let mask = table.len() - 1;
            if used * 5 >= mask * 3 {
                let minused = if used > 50000 { used * 2 } else { used * 4 };
                let mut size = 8;
                while size <= minused {
                    size <<= 1;
                }
                let old: Vec<i64> = table.iter().flatten().copied().collect();
                table = vec![None; size];
                for x in old {
                    insert(&mut table, x);
                }
            }
        }
    }
    table.into_iter().flatten().collect()
}
