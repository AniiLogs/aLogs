//! Text checks from aLogs B16 `bossbar/core.py`: `interlink_score` and `same_name`.

use std::collections::BTreeSet;

/// An Interlink score checked against its stars. Below the cap a score is in its stars' thousand; at the cap it's
/// anything above. A score whose thousands don't match had a 1 read as a 7: each 7 is tried as a 1, and the one reading
/// that fits is used; when more than one fits, the score is left unread (`None`). A two-boss fight's score is kept as
/// read (how its score is built isn't known).
pub fn interlink_score(score: Option<i64>, stars: Option<i64>, cap: Option<i64>, two_boss: bool) -> Option<i64> {
    let (Some(score), Some(stars)) = (score, stars) else { return score };
    let fits = |s: i64| s.div_euclid(1000) == stars || (cap == Some(stars) && s.div_euclid(1000) > stars);
    if fits(score) {
        return Some(score);
    }
    if two_boss {
        log::info!(target: "meter", "score {score} kept as read: {stars} stars, a two-boss fight (how its score is built isn't known)");
        return Some(score);
    }
    let digits: Vec<char> = score.to_string().chars().collect();
    let sevens: Vec<usize> = digits.iter().enumerate().filter(|(_, c)| **c == '7').map(|(i, _)| i).collect();
    let mut found = BTreeSet::new();
    for mask in 1u64..(1u64 << sevens.len()) {
        let mut d = digits.clone();
        for (j, &i) in sevens.iter().enumerate() {
            if mask >> j & 1 == 1 {
                d[i] = '1';
            }
        }
        let v: i64 = d.iter().collect::<String>().parse().unwrap();
        if fits(v) {
            found.insert(v);
        }
    }
    if found.len() == 1 {
        let fixed = *found.iter().next().unwrap();
        log::info!(target: "meter", "score {score} read as {fixed}: {stars} stars (a 1 read as a 7)");
        return Some(fixed);
    }
    if !found.is_empty() {
        log::info!(target: "meter", "score {score} left unread: {stars} stars, and it could be any of {found:?}");
        return None;
    }
    Some(score)
}

/// `str.casefold()` for the letters names use: lower case, with ß and long s folded the way Python folds them.
fn casefold(s: &str) -> String {
    s.to_lowercase().replace('ß', "ss").replace('ς', "σ").replace('ſ', "s")
}

/// Two OCR reads of one in-game name: equal ignoring case, or off by a misread letter or two (difflib ratio >= 0.85
/// when both have 4 or more letters).
pub fn same_name(a: Option<&str>, b: Option<&str>) -> bool {
    let a = casefold(a.unwrap_or("").trim());
    let b = casefold(b.unwrap_or("").trim());
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || (a.chars().count().min(b.chars().count()) >= 4 && crate::difflib::ratio(&a, &b) >= 0.85)
}
