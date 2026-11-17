//! An object read through its type tree, like UnityPy's `read_typetree()` dicts.

#[derive(Clone, Debug, PartialEq, Default)]
pub enum V {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// UInt8 / char arrays and TypelessData
    Bytes(Vec<u8>),
    List(Vec<V>),
    /// fields in their type tree's order
    Obj(Vec<(String, V)>),
}

static NULL: V = V::Null;

impl V {
    /// The field, or Null.
    pub fn get(&self, k: &str) -> &V {
        match self {
            V::Obj(f) => f.iter().find(|(n, _)| n == k).map_or(&NULL, |(_, v)| v),
            _ => &NULL,
        }
    }

    pub fn has(&self, k: &str) -> bool {
        matches!(self, V::Obj(f) if f.iter().any(|(n, _)| n == k))
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            V::Int(i) => Some(i),
            V::Bool(b) => Some(b as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            V::Int(i) => Some(i as f64),
            V::Float(f) => Some(f),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            V::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> &[V] {
        match self {
            V::List(l) => l,
            _ => &[],
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            V::Bytes(b) => b,
            _ => &[],
        }
    }
}
