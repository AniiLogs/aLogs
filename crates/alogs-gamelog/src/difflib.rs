//! `difflib.SequenceMatcher(None, a, b).ratio()` from Python's standard library, on characters, with its
//! "autojunk" rule (in a `b` of 200 or more, an element making up over 1% of it is ignored as popular).

use std::collections::HashMap;

pub struct SequenceMatcher {
    a: Vec<char>,
    b: Vec<char>,
    b2j: HashMap<char, Vec<usize>>,
}

impl SequenceMatcher {
    pub fn new(a: &str, b: &str) -> SequenceMatcher {
        let a: Vec<char> = a.chars().collect();
        let b: Vec<char> = b.chars().collect();
        let mut b2j: HashMap<char, Vec<usize>> = HashMap::new();
        for (i, &c) in b.iter().enumerate() {
            b2j.entry(c).or_default().push(i);
        }
        let n = b.len();
        if n >= 200 {
            let ntest = n / 100 + 1;
            b2j.retain(|_, idx| idx.len() <= ntest);
        }
        SequenceMatcher { a, b, b2j }
    }

    /// The longest matching block in `a[alo..ahi]` and `b[blo..bhi]` (earliest in `a`, then in `b`, on ties),
    /// extended as the Python does (there's no junk here, only popular elements).
    fn find_longest_match(&self, alo: usize, ahi: usize, blo: usize, bhi: usize) -> (usize, usize, usize) {
        let (a, b) = (&self.a, &self.b);
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for i in alo..ahi {
            let mut newj2len: HashMap<usize, usize> = HashMap::new();
            if let Some(js) = self.b2j.get(&a[i]) {
                for &j in js {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = j.checked_sub(1).and_then(|p| j2len.get(&p)).copied().unwrap_or(0) + 1;
                    newj2len.insert(j, k);
                    if k > bestsize {
                        besti = i + 1 - k;
                        bestj = j + 1 - k;
                        bestsize = k;
                    }
                }
            }
            j2len = newj2len;
        }
        // the Python then grows the match over equal elements on both sides (with no junk function every element
        // counts as "not junk", popular ones included)
        while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi && bestj + bestsize < bhi && a[besti + bestsize] == b[bestj + bestsize] {
            bestsize += 1;
        }
        (besti, bestj, bestsize)
    }

    pub fn matching_blocks(&self) -> Vec<(usize, usize, usize)> {
        let (la, lb) = (self.a.len(), self.b.len());
        let mut queue = vec![(0, la, 0, lb)];
        let mut blocks = Vec::new();
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let (i, j, k) = self.find_longest_match(alo, ahi, blo, bhi);
            if k > 0 {
                blocks.push((i, j, k));
                if alo < i && blo < j {
                    queue.push((alo, i, blo, j));
                }
                if i + k < ahi && j + k < bhi {
                    queue.push((i + k, ahi, j + k, bhi));
                }
            }
        }
        blocks.sort_unstable();
        // collapse adjacent blocks
        let mut out: Vec<(usize, usize, usize)> = Vec::new();
        let (mut i1, mut j1, mut k1) = (0, 0, 0);
        for (i2, j2, k2) in blocks {
            if i1 + k1 == i2 && j1 + k1 == j2 {
                k1 += k2;
            } else {
                if k1 > 0 {
                    out.push((i1, j1, k1));
                }
                (i1, j1, k1) = (i2, j2, k2);
            }
        }
        if k1 > 0 {
            out.push((i1, j1, k1));
        }
        out.push((la, lb, 0));
        out
    }

    pub fn ratio(&self) -> f64 {
        let matches: usize = self.matching_blocks().iter().map(|b| b.2).sum();
        let total = self.a.len() + self.b.len();
        if total == 0 { 1.0 } else { 2.0 * matches as f64 / total as f64 }
    }
}

pub fn ratio(a: &str, b: &str) -> f64 {
    SequenceMatcher::new(a, b).ratio()
}
