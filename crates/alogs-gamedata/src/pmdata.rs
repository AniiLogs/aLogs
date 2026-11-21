//! The game's packed data tables (LuaScripts/Common/Data/pmdata.bin). Port of `PmData` in aLogs B16
//! `bossbar/gamedata.py`, kept step for step: the same heuristics find the string pool, key sets, classes and objects,
//! and the same inputs give the same tables (checked by tests/pmdata.rs on synthetic files through both).
//!
//! Where the Python would raise (a read past the end of the file, a dict changing size while it's iterated) this
//! returns `Err`, so callers that catch those (`read_rush_rules`) behave the same.

use alogs_pyval::{Dict, Value};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmError(pub String);

impl fmt::Display for PmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PmError {}

pub type Result<T> = std::result::Result<T, PmError>;

fn err<T>(what: &str) -> Result<T> {
    Err(PmError(what.to_string()))
}

/// A key in a key set: a string-pool offset, or `('#', number)` from a numeric key set.
#[derive(Clone, Debug)]
enum Key {
    Str(u32),
    Num(Value),
}

#[derive(Clone, Debug)]
struct KeySet {
    n: u32,
    a: u32,
    keys: Vec<Key>,
}

#[derive(Clone, Debug)]
struct Class {
    keyset: i64,
    a: u32,
    types: Vec<u8>,
    offs: Vec<u64>,
}

const fn size(t: u8) -> usize {
    match t {
        0 => 8,
        1 | 2 | 9 => 1,
        3 | 4 => 2,
        _ => 4,
    }
}

pub struct PmData {
    d: Vec<u8>,
    end: usize,
    strings: HashSet<u32>,
    keysets: Vec<(i64, KeySet)>, // insertion order, like the Python dict
    keyset_at: HashMap<i64, usize>,
    classes: HashMap<u32, Option<usize>>,
    class_list: Vec<Class>,
    consts: HashMap<u32, HashMap<usize, usize>>,
}

fn find(hay: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    let end = end.min(hay.len());
    if start > end || needle.len() > end - start {
        return None;
    }
    hay[start..end].windows(needle.len()).position(|w| w == needle).map(|p| p + start)
}

impl PmData {
    pub fn open(path: &std::path::Path) -> std::io::Result<PmData> {
        Ok(PmData::from_bytes(std::fs::read(path)?))
    }

    pub fn from_bytes(d: Vec<u8>) -> PmData {
        // the sorted string pool at the start of the file
        let mut pos = 0usize;
        let mut prev: &[u8] = b"";
        let mut pool = Vec::new();
        while let Some(e) = find(&d, b"\0", pos, d.len()) {
            let s = &d[pos..e];
            if s < prev && pool.len() > 10 {
                break;
            }
            if std::str::from_utf8(s).is_err() {
                break;
            }
            pool.push(pos as u32);
            prev = s;
            pos = e + 1;
        }
        let mut strings: HashSet<u32> = pool.into_iter().collect();
        strings.remove(&0);
        let mut pm = PmData {
            d,
            end: pos,
            strings,
            keysets: Vec::new(),
            keyset_at: HashMap::new(),
            classes: HashMap::new(),
            class_list: Vec::new(),
            consts: HashMap::new(),
        };
        pm.scan_keysets();
        pm
    }

    fn u32(&self, q: i64) -> Result<u32> {
        if q < 0 || q as usize + 4 > self.d.len() {
            return err("unpack_from requires a buffer of at least 4 bytes");
        }
        let q = q as usize;
        Ok(u32::from_le_bytes(self.d[q..q + 4].try_into().unwrap()))
    }

    fn byte(&self, q: i64) -> Result<u8> {
        // Python d[q]: negative indexes count from the end
        let len = self.d.len() as i64;
        let i = if q < 0 { q + len } else { q };
        if i < 0 || i >= len {
            return err("index out of range");
        }
        Ok(self.d[i as usize])
    }

    fn bytes<const N: usize>(&self, q: i64) -> Result<[u8; N]> {
        if q < 0 || q as usize + N > self.d.len() {
            return err("unpack_from requires a larger buffer");
        }
        Ok(self.d[q as usize..q as usize + N].try_into().unwrap())
    }

    /// `st(o)`: the zero-ended string at `o` (to the last byte but one when there's no zero, as `d[o:-1]`).
    fn st(&self, o: u32) -> String {
        let o = o as usize;
        let stop = match find(&self.d, b"\0", o, self.d.len()) {
            Some(e) => e,
            None => self.d.len().saturating_sub(1),
        };
        if o >= stop {
            return String::new();
        }
        String::from_utf8_lossy(&self.d[o..stop]).into_owned()
    }

    fn off_of(&self, name: &str) -> Option<u32> {
        let mut pat = vec![0u8];
        pat.extend_from_slice(name.as_bytes());
        pat.push(0);
        find(&self.d, &pat, 0, self.d.len()).map(|i| i as u32 + 1)
    }

    fn add_keyset(&mut self, k: i64, ks: KeySet) {
        match self.keyset_at.get(&k) {
            Some(&i) => self.keysets[i].1 = ks,
            None => {
                self.keyset_at.insert(k, self.keysets.len());
                self.keysets.push((k, ks));
            }
        }
    }

    /// Every key block: byte 1, u32 n keys, u32 array part, then n string offsets.
    fn scan_keysets(&mut self) {
        let l = self.d.len();
        let mut q = self.end;
        loop {
            let Some(found) = find(&self.d, b"\x01", q, l.saturating_sub(9)) else { break };
            q = found;
            let n = self.u32(q as i64 + 1).unwrap_or(0);
            let a = self.u32(q as i64 + 5).unwrap_or(u32::MAX);
            if 0 < n
                && n < 400_000
                && a < 400_000
                && q + 9 + 4 * n as usize <= l
                && self.strings.contains(&self.u32(q as i64 + 9).unwrap())
            {
                let step = if n < 5000 { 1 } else { (n / 50).max(1) };
                let ok = (0..n)
                    .step_by(step as usize)
                    .all(|i| self.strings.contains(&self.u32(q as i64 + 9 + 4 * i as i64).unwrap()));
                if ok {
                    let keys =
                        (0..n).map(|i| Key::Str(self.u32(q as i64 + 9 + 4 * i as i64).unwrap())).collect::<Vec<_>>();
                    self.add_keyset(q as i64 - 4, KeySet { n, a, keys });
                    q += 9 + 4 * n as usize;
                    continue;
                }
            }
            q += 1;
        }
    }

    fn numeric_keyset(&self, k: u32) -> Result<Option<KeySet>> {
        let k = k as usize;
        if !(self.end < k && k + 13 < self.d.len()) {
            return Ok(None);
        }
        let tag = self.d[k + 4];
        let w: i64 = match tag {
            0 => 8,
            5 => 4,
            3 => 2,
            1 => 1,
            _ => return Ok(None),
        };
        let n = self.u32(k as i64 + 5)?;
        if !(0 < n && n < 400_000 && self.u32(k as i64 + 9)? == n) {
            return Ok(None);
        }
        let mut keys = Vec::with_capacity(n as usize);
        for i in 0..n as i64 {
            let q = k as i64 + 13 + w * i;
            let v = match tag {
                0 => Value::Float(f64::from_le_bytes(self.bytes::<8>(q)?)),
                5 => Value::Int(i64::from(i32::from_le_bytes(self.bytes::<4>(q)?))),
                3 => Value::Int(i64::from(u16::from_le_bytes(self.bytes::<2>(q)?))),
                _ => Value::Int(i64::from(self.byte(q)?)),
            };
            keys.push(Key::Num(v));
        }
        Ok(Some(KeySet { n, a: 0, keys }))
    }

    /// `cls(t)`: the class at `t` (keys, array part, types, field offsets), cached; `None` when it doesn't look like one.
    fn cls(&mut self, t: u32) -> Result<Option<usize>> {
        if let Some(c) = self.classes.get(&t) {
            return Ok(*c);
        }
        let len = self.d.len();
        let (k, a_, b_) = (self.u32(t as i64)?, self.u32(t as i64 + 4)?, self.u32(t as i64 + 8)?);
        let (a_, b_) = (a_ as usize, b_ as usize);
        let mut result = None;
        if self.end < a_ && a_ < len && self.end < b_ && b_ < len {
            let k64 = i64::from(k);
            if !self.keyset_at.contains_key(&k64)
                && let Some(ks) = self.numeric_keyset(k)?
            {
                self.add_keyset(k64, ks);
            }
            if let Some(&ki) = self.keyset_at.get(&k64) {
                let KeySet { n, a, .. } = self.keysets[ki].1;
                let w = match self.d[b_] {
                    1 => 1usize,
                    3 => 2,
                    7 => 4,
                    _ => 0,
                };
                let total = (n + a) as usize;
                let types: Vec<u8> = self.d[a_..(a_ + total).min(len)].to_vec();
                if w > 0 && !types.is_empty() && !types.iter().any(|&x| (9 < x && x < 0x64) || x > 0x6d) {
                    let offs = (0..total)
                        .map(|i| {
                            let lo = (b_ + 1 + w * i).min(len);
                            let hi = (b_ + 1 + w * (i + 1)).min(len);
                            self.d[lo..hi].iter().rev().fold(0u64, |acc, &x| (acc << 8) | u64::from(x))
                        })
                        .collect();
                    let mut cpos = HashMap::new();
                    let mut p = b_ as i64 - types.iter().filter(|&&x| x >= 0x64).map(|&x| size(x - 0x64) as i64).sum::<i64>();
                    for (i, &x) in types.iter().enumerate() {
                        if x >= 0x64 {
                            cpos.insert(i, p as usize);
                            p += size(x - 0x64) as i64;
                        }
                    }
                    self.consts.insert(t, cpos);
                    self.class_list.push(Class { keyset: k64, a, types, offs });
                    result = Some(self.class_list.len() - 1);
                }
            }
        }
        self.classes.insert(t, result);
        Ok(result)
    }

    fn val(&self, at: i64, ty: u8) -> Result<Value> {
        Ok(match ty {
            0 => Value::Float(f64::from_le_bytes(self.bytes::<8>(at)?)),
            1 => Value::Int(i64::from(self.byte(at)?)),
            2 => Value::Int(i64::from(i8::from_le_bytes(self.bytes::<1>(at)?))),
            3 => Value::Int(i64::from(u16::from_le_bytes(self.bytes::<2>(at)?))),
            4 => Value::Int(i64::from(i16::from_le_bytes(self.bytes::<2>(at)?))),
            5 => Value::Int(i64::from(i32::from_le_bytes(self.bytes::<4>(at)?))),
            6 => Value::Int(i64::from(u32::from_le_bytes(self.bytes::<4>(at)?))),
            7 => {
                let v = self.u32(at)?;
                if self.strings.contains(&v) || v == 0 { Value::Str(self.st(v)) } else { Value::None }
            }
            9 => Value::Bool(self.byte(at)? != 0),
            _ => Value::None,
        })
    }

    /// `tree(o)`: the object at `o` as a dict (a list when it has only an array part), `None` when it isn't one.
    pub fn tree(&mut self, o: i64, depth: u32, maxdepth: u32) -> Result<Value> {
        let len = self.d.len() as i64;
        if !(self.end as i64 <= o && o < len - 12) {
            return Ok(Value::None);
        }
        let t = self.u32(o)?;
        if !(self.end as i64 <= i64::from(t) && i64::from(t) < len - 12) {
            return Ok(Value::None);
        }
        let Some(ci) = self.cls(t)? else { return Ok(Value::None) };
        let c = self.class_list[ci].clone();
        let ks = self.keysets[self.keyset_at[&c.keyset]].1.clone();
        let mut names: Vec<Value> = ks
            .keys
            .iter()
            .map(|k| match k {
                Key::Num(v) => v.clone(),
                Key::Str(off) => Value::Str(self.st(*off)),
            })
            .collect();
        names.extend((0..c.a).map(|j| Value::Int(i64::from(j) + 1)));
        let mut out = Dict::new();
        for (i, name) in names.into_iter().enumerate() {
            let Some(&ty0) = c.types.get(i) else { return err("list index out of range") };
            let (at, ty) = if ty0 >= 0x64 {
                (self.consts[&t][&i] as i64, ty0 - 0x64)
            } else {
                let Some(&off) = c.offs.get(i) else { return err("list index out of range") };
                (o + off as i64, ty0)
            };
            let v = if ty == 8 {
                if depth < maxdepth {
                    let to = self.u32(at)?;
                    self.tree(i64::from(to), depth + 1, maxdepth)?
                } else {
                    Value::None
                }
            } else {
                self.val(at, ty)?
            };
            out.insert(name, v);
        }
        if c.a > 0 && ks.keys.is_empty() {
            let list = (0..c.a)
                .map(|j| out.get(&Value::Int(i64::from(j) + 1)).cloned().ok_or_else(|| PmError("KeyError".into())))
                .collect::<Result<Vec<_>>>()?;
            return Ok(Value::List(list));
        }
        Ok(Value::Dict(out))
    }

    /// A top-level data table by name (the directory object is found by its keys, so patches can move it).
    pub fn table(&mut self, name: &str) -> Result<Value> {
        match self.table_at(name)? {
            Some(to) => self.tree(i64::from(to), 0, 6),
            None => Ok(Value::None),
        }
    }

    /// Where a top-level table's object starts, None when there's no such table.
    fn table_at(&mut self, name: &str) -> Result<Option<u32>> {
        let k = self.off_of(name);
        let count = self.keysets.len();
        let mut idx = 0;
        while idx < self.keysets.len() {
            if self.keysets.len() != count {
                return err("dictionary changed size during iteration");
            }
            let (kk, ks) = self.keysets[idx].clone();
            idx += 1;
            let pos = match k {
                Some(k) if ks.n >= 1000 => ks.keys.iter().position(|x| matches!(x, Key::Str(o) if *o == k)),
                _ => None,
            };
            let Some(i) = pos else { continue };
            let target = (kk as u32).to_le_bytes();
            let mut p = find(&self.d, &target, self.end, self.d.len());
            while let Some(pp) = p {
                if let Some(ci) = self.cls(pp as u32)? {
                    let pat = (pp as u32).to_le_bytes();
                    let mut q = find(&self.d, &pat, self.end, self.d.len());
                    while let Some(qq) = q {
                        let t = self.u32(qq as i64)?;
                        if self.cls(t)? == Some(ci) {
                            let off = self.class_list[ci].offs[i] as i64;
                            return Ok(Some(self.u32(qq as i64 + off)?));
                        }
                        q = find(&self.d, &pat, qq + 1, self.d.len());
                    }
                }
                p = find(&self.d, &target, pp + 1, self.d.len());
            }
        }
        if self.keysets.len() != count {
            return err("dictionary changed size during iteration");
        }
        Ok(None)
    }

    /// Per row of a top-level table: every list (or `{1: .., 2: ..}` dict) its class's shared values (the fields all
    /// rows of that shape have the same value for) point at, read at every byte of that block.
    ///
    /// `tree` reads those shared values in field order, which holds for numbers but not always for the lists: in
    /// bossrush_cycle_data's newer weeks the block holds the same values in another order, so a shared list read in
    /// field order lands on the wrong bytes (and the numbers after it may too). Callers pick the list they need by
    /// what it holds.
    pub fn shared_lists(&mut self, name: &str) -> Result<Vec<(Value, Vec<Value>)>> {
        let Some(to) = self.table_at(name)? else { return Ok(vec![]) };
        let o = i64::from(to);
        let len = self.d.len() as i64;
        if !(self.end as i64 <= o && o < len - 12) {
            return Ok(vec![]);
        }
        let t = self.u32(o)?;
        let Some(ci) = self.cls(t)? else { return Ok(vec![]) };
        let c = self.class_list[ci].clone();
        let ks = self.keysets[self.keyset_at[&c.keyset]].1.clone();
        let mut out = Vec::new();
        for (i, k) in ks.keys.iter().enumerate() {
            let key = match k {
                Key::Num(v) => v.clone(),
                Key::Str(off) => Value::Str(self.st(*off)),
            };
            let (Some(&ty), Some(&off)) = (c.types.get(i), c.offs.get(i)) else { continue };
            if ty != 8 {
                continue;
            }
            let row = i64::from(self.u32(o + off as i64)?);
            if !(self.end as i64 <= row && row < len - 12) {
                continue;
            }
            let rt = self.u32(row)?;
            if self.cls(rt)?.is_none() {
                continue;
            }
            let Some(lo) = self.consts.get(&rt).and_then(|c| c.values().min().copied()) else { continue };
            let hi = self.u32(i64::from(rt) + 8)? as usize;
            let mut lists: Vec<Value> = Vec::new();
            for p in lo..hi.saturating_sub(3) {
                let at = self.u32(p as i64)?;
                if let Ok(v @ (Value::List(_) | Value::Dict(_))) = self.tree(i64::from(at), 0, 1)
                    && v.truthy()
                    && !lists.iter().any(|l| l.py_eq(&v))
                {
                    lists.push(v);
                }
            }
            out.push((key, lists));
        }
        Ok(out)
    }
}
