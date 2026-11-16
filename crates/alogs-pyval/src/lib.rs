//! Python-like values, so code ported from aLogs' Python meter keeps its exact behaviour and can be compared with the
//! reference through `repr()`.
//!
//! - [`Value`] is None/bool/int/float/str/list/dict. Dict keys compare the way Python's do (`1 == 1.0 == True`), and the
//!   first key inserted is the one kept, as in CPython.
//! - [`Value::repr`], [`Value::py_str`] and [`Value::json_dumps`] produce the same text as `repr()`, `str()` and
//!   `json.dumps()` (default separators, `ensure_ascii=True`).

use indexmap::IndexMap;
use std::fmt::Write as _;

#[derive(Clone, Debug)]
pub enum Value {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Value>),
    Dict(Dict),
}

/// A dict key normalised so that keys Python treats as equal hash and compare equal.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Norm {
    None,
    Int(i64),
    Float(u64),
    Str(String),
    Unhashable,
}

fn norm(v: &Value) -> Norm {
    match v {
        Value::None => Norm::None,
        Value::Bool(b) => Norm::Int(i64::from(*b)),
        Value::Int(i) => Norm::Int(*i),
        Value::Float(f) => {
            if f.fract() == 0.0 && f.is_finite() && f.abs() < 9.2e18 {
                Norm::Int(*f as i64)
            } else if *f == 0.0 {
                Norm::Int(0)
            } else {
                Norm::Float(f.to_bits())
            }
        }
        Value::Str(s) => Norm::Str(s.clone()),
        Value::List(_) | Value::Dict(_) => Norm::Unhashable,
    }
}

/// An insertion-ordered dict with Python key semantics.
#[derive(Clone, Debug, Default)]
pub struct Dict {
    map: IndexMap<Norm, (Value, Value)>,
}

impl Dict {
    pub fn new() -> Self {
        Self::default()
    }
    /// `d[k] = v`: a new key goes last; an equal key keeps its first spelling and position.
    pub fn insert(&mut self, k: Value, v: Value) {
        match self.map.get_mut(&norm(&k)) {
            Some(slot) => slot.1 = v,
            None => {
                self.map.insert(norm(&k), (k, v));
            }
        }
    }
    pub fn get(&self, k: &Value) -> Option<&Value> {
        self.map.get(&norm(k)).map(|(_, v)| v)
    }
    pub fn get_str(&self, k: &str) -> Option<&Value> {
        self.map.get(&Norm::Str(k.to_string())).map(|(_, v)| v)
    }
    /// `d.get(k)` with a missing key as None.
    pub fn at(&self, k: &str) -> &Value {
        self.get_str(k).unwrap_or(&Value::None)
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&Value, &Value)> {
        self.map.values().map(|(k, v)| (k, v))
    }
    pub fn keys(&self) -> impl Iterator<Item = &Value> {
        self.map.values().map(|(k, _)| k)
    }
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.map.values().map(|(_, v)| v)
    }
}

impl FromIterator<(Value, Value)> for Dict {
    fn from_iter<I: IntoIterator<Item = (Value, Value)>>(it: I) -> Self {
        let mut d = Dict::new();
        for (k, v) in it {
            d.insert(k, v);
        }
        d
    }
}

pub static NONE: Value = Value::None;

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}
impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Int(i)
    }
}
impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::Float(f)
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Value::List(v)
    }
}
impl From<Dict> for Value {
    fn from(d: Dict) -> Self {
        Value::Dict(d)
    }
}

/// Builds a dict from `(key, value)` pairs in order.
pub fn dict<K: Into<Value>, I: IntoIterator<Item = (K, Value)>>(pairs: I) -> Value {
    Value::Dict(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

impl Value {
    /// Python truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Value::None => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Dict(d) => !d.is_empty(),
        }
    }
    pub fn is_none(&self) -> bool {
        matches!(self, Value::None)
    }
    /// `isinstance(v, int)`: ints and bools.
    pub fn is_int(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Bool(_))
    }
    /// `isinstance(v, (int, float))`: ints, floats and bools.
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Float(_) | Value::Bool(_))
    }
    /// The number as f64 for ints, floats and bools.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Bool(b) => Some(i64::from(*b)),
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Value::Dict(d) => Some(d),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&Vec<Value>> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }
    /// `a or b`.
    pub fn or<'a>(&'a self, other: &'a Value) -> &'a Value {
        if self.truthy() { self } else { other }
    }
    /// `d.get(k)` on a dict; None for a missing key or a value that isn't a dict.
    pub fn get(&self, k: &str) -> &Value {
        self.as_dict().map_or(&NONE, |d| d.at(k))
    }

    /// Python `==`.
    pub fn py_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::None, Value::None) => true,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::List(a), Value::List(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.py_eq(y)),
            (Value::Dict(a), Value::Dict(b)) => {
                a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| v.py_eq(w)))
            }
            (a, b) if a.is_number() && b.is_number() => match (a.as_i64(), b.as_i64()) {
                (Some(x), Some(y)) => x == y,
                _ => a.as_f64() == b.as_f64(),
            },
            _ => false,
        }
    }

    /// `repr(v)`.
    pub fn repr(&self) -> String {
        let mut s = String::new();
        self.write_repr(&mut s);
        s
    }
    fn write_repr(&self, out: &mut String) {
        match self {
            Value::None => out.push_str("None"),
            Value::Bool(true) => out.push_str("True"),
            Value::Bool(false) => out.push_str("False"),
            Value::Int(i) => {
                let _ = write!(out, "{i}");
            }
            Value::Float(f) => out.push_str(&float_repr(*f)),
            Value::Str(s) => out.push_str(&str_repr(s)),
            Value::List(l) => {
                out.push('[');
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    v.write_repr(out);
                }
                out.push(']');
            }
            Value::Dict(d) => {
                out.push('{');
                for (i, (k, v)) in d.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    k.write_repr(out);
                    out.push_str(": ");
                    v.write_repr(out);
                }
                out.push('}');
            }
        }
    }

    /// `str(v)`.
    pub fn py_str(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            v => v.repr(),
        }
    }

    /// `json.dumps(v)` with Python's defaults.
    pub fn json_dumps(&self) -> String {
        let mut s = String::new();
        self.write_json(&mut s);
        s
    }
    fn write_json(&self, out: &mut String) {
        match self {
            Value::None => out.push_str("null"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            Value::Int(i) => {
                let _ = write!(out, "{i}");
            }
            Value::Float(f) => out.push_str(&json_float(*f)),
            Value::Str(s) => out.push_str(&json_str(s)),
            Value::List(l) => {
                out.push('[');
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    v.write_json(out);
                }
                out.push(']');
            }
            Value::Dict(d) => {
                out.push('{');
                for (i, (k, v)) in d.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    let key = match k {
                        Value::Str(s) => s.clone(),
                        Value::Float(f) => json_float(*f),
                        Value::None => "null".into(),
                        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
                        other => other.repr(),
                    };
                    out.push_str(&json_str(&key));
                    out.push_str(": ");
                    v.write_json(out);
                }
                out.push('}');
            }
        }
    }

    /// `json.loads` result for a serde_json value (ints as Int where they fit, keys as str).
    pub fn from_json(j: &serde_json::Value) -> Value {
        match j {
            serde_json::Value::Null => Value::None,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => match n.as_i64() {
                Some(i) => Value::Int(i),
                None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
            },
            serde_json::Value::String(s) => Value::Str(s.clone()),
            serde_json::Value::Array(a) => Value::List(a.iter().map(Value::from_json).collect()),
            serde_json::Value::Object(o) => {
                Value::Dict(o.iter().map(|(k, v)| (Value::Str(k.clone()), Value::from_json(v))).collect())
            }
        }
    }
    /// `json.loads(text)`.
    pub fn json_loads(text: &str) -> Option<Value> {
        serde_json::from_str::<serde_json::Value>(text).ok().map(|j| Value::from_json(&j))
    }
}

/// `repr(float)`: the shortest digits that read back exactly, fixed notation for 1e-4 <= |x| < 1e16.
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.into();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let sci = format!("{:e}", f); // shortest round-trip digits, e.g. "-1.5e-5"
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if (-4..16).contains(&exp) {
        let point = exp + 1; // digits before the decimal point
        if point <= 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-point) as usize));
            out.push_str(&digits);
        } else if (point as usize) >= digits.len() {
            out.push_str(&digits);
            out.push_str(&"0".repeat(point as usize - digits.len()));
            out.push_str(".0");
        } else {
            out.push_str(&digits[..point as usize]);
            out.push('.');
            out.push_str(&digits[point as usize..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(out, "e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    out
}

fn json_float(f: f64) -> String {
    if f.is_nan() {
        "NaN".into()
    } else if f.is_infinite() {
        if f > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        float_repr(f)
    }
}

/// `str.isprintable()` for one character: false for controls (Cc), format (Cf), surrogates, private use and
/// separators other than the space (Zs, Zl, Zp). Unassigned code points (Cn) count as printable here.
fn printable(c: char) -> bool {
    let u = c as u32;
    let r = |lo: u32, hi: u32| (lo..=hi).contains(&u);
    !(u < 0x20
        || r(0x7f, 0xa0)
        || u == 0xad
        || r(0x600, 0x605)
        || u == 0x61c
        || u == 0x6dd
        || u == 0x70f
        || u == 0x1680
        || u == 0x180e
        || r(0x2000, 0x200f)
        || r(0x2028, 0x202f)
        || u == 0x205f
        || r(0x2060, 0x206f)
        || u == 0x3000
        || r(0xd800, 0xf8ff)
        || u == 0xfeff
        || r(0xfff9, 0xfffb)
        || u >= 0xf0000)
}

/// `repr(str)`.
pub fn str_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c if printable(c) => out.push(c),
            c => {
                let u = c as u32;
                let _ = if u <= 0xff {
                    write!(out, "\\x{u:02x}")
                } else if u <= 0xffff {
                    write!(out, "\\u{u:04x}")
                } else {
                    write!(out, "\\U{u:08x}")
                };
            }
        }
    }
    out.push(q);
    out
}

/// A JSON string as `json.dumps` writes it (`ensure_ascii=True`).
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{:04x}", unit);
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `float(s)` for a str: surrounding whitespace, `_` between digits, inf/nan spellings.
pub fn parse_float(s: &str) -> Option<f64> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    let body = lower.trim_start_matches(['+', '-']);
    let neg = lower.starts_with('-');
    if matches!(body, "inf" | "infinity") {
        return Some(if neg { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    if body == "nan" {
        return Some(f64::NAN);
    }
    if t.contains("__") || t.starts_with('_') || t.ends_with('_') {
        return None;
    }
    let clean: String = t.chars().filter(|&c| c != '_').collect();
    if clean.is_empty() || clean.contains(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E') {
        return None;
    }
    clean.parse::<f64>().ok()
}

/// `int(s)` for a str.
pub fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.contains("__") || t.ends_with('_') {
        return None;
    }
    let clean: String = t.chars().filter(|&c| c != '_').collect();
    clean.parse::<i64>().ok()
}

/// `int(v)`: ints, bools, floats (towards zero) and digit strings.
pub fn py_int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Int(i) => Some(*i),
        Value::Float(f) if f.is_finite() => Some(f.trunc() as i64),
        Value::Str(s) => parse_int(s),
        _ => None,
    }
}

/// `float(v)`.
pub fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::Str(s) => parse_float(s),
        v => v.as_f64(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_matches_python() {
        for (f, want) in [
            (1.0, "1.0"),
            (0.1, "0.1"),
            (1e-05, "1e-05"),
            (0.0001, "0.0001"),
            (1.5e-07, "1.5e-07"),
            (1e16, "1e+16"),
            (1234567890123456.0, "1234567890123456.0"),
            (1.2345678901234567e16, "1.2345678901234568e+16"),
            (-2.5, "-2.5"),
            (123.456, "123.456"),
            (0.0044260089946, "0.0044260089946"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
        ] {
            assert_eq!(float_repr(f), want, "{f:e}");
        }
    }

    #[test]
    fn keys_compare_like_python() {
        let mut d = Dict::new();
        d.insert(Value::Int(1), "a".into());
        d.insert(Value::Float(1.0), "b".into());
        d.insert(Value::Bool(true), "c".into());
        assert_eq!(d.len(), 1);
        assert_eq!(Value::Dict(d).repr(), "{1: 'c'}");
    }

    #[test]
    fn repr_and_json() {
        let v = dict([
            ("a", Value::List(vec![Value::None, Value::Bool(true), Value::Float(2.0), "it's".into()])),
            ("é", Value::Str("x\"y\n".into())),
        ]);
        assert_eq!(v.repr(), r#"{'a': [None, True, 2.0, "it's"], 'é': 'x"y\n'}"#);
        assert_eq!(v.json_dumps(), r#"{"a": [null, true, 2.0, "it's"], "\u00e9": "x\"y\n"}"#);
        let mut k = Dict::new();
        k.insert(Value::Int(3), Value::Float(1e-7));
        assert_eq!(Value::Dict(k).json_dumps(), r#"{"3": 1e-07}"#);
    }
}
