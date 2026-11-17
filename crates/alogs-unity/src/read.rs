//! A byte cursor, big or little endian.

use crate::Result;

pub struct R<'a> {
    pub d: &'a [u8],
    pub p: usize,
    pub be: bool,
}

macro_rules! num {
    ($name:ident, $t:ty, $n:expr) => {
        pub fn $name(&mut self) -> Result<$t> {
            let b: [u8; $n] = self.take($n)?.try_into().unwrap();
            Ok(if self.be { <$t>::from_be_bytes(b) } else { <$t>::from_le_bytes(b) })
        }
    };
}

impl<'a> R<'a> {
    pub fn new(d: &'a [u8], be: bool) -> R<'a> {
        R { d, p: 0, be }
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.d.len() - self.p.min(self.d.len()) {
            return Err(format!("read past the end ({} + {n} > {})", self.p, self.d.len()));
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }

    pub fn left(&self) -> usize {
        self.d.len().saturating_sub(self.p)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    num!(i16, i16, 2);
    num!(u16, u16, 2);
    num!(i32, i32, 4);
    num!(u32, u32, 4);
    num!(i64, i64, 8);
    num!(u64, u64, 8);
    num!(f32, f32, 4);
    num!(f64, f64, 8);

    pub fn cstr(&mut self) -> Result<String> {
        let rest = &self.d[self.p.min(self.d.len())..];
        let n = rest.iter().position(|&b| b == 0).ok_or("unterminated string")?;
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        self.p += n + 1;
        Ok(s)
    }

    pub fn align(&mut self, n: usize) {
        self.p = self.p.div_ceil(n) * n;
    }
}
