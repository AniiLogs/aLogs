//! Reading Unity asset bundles: the part of UnityPy (1.25) the Python meter used to take the game's UI font atlas
//! and icons out of the player's own install.
//!
//! - [`bundle`]: UnityFS archives (uncompressed, LZMA, LZ4 and LZ4HC blocks), blocks decompressed as they're needed.
//! - [`serialized`]: the serialized files inside (format 17 and up, Unity 2019+) and their objects, read through the
//!   type trees the bundles carry ([`value::V`]).
//! - [`build`]: writing small made-up bundles, for tests.
//! - [`Env`]: bundles loaded together, as `UnityPy.load(*paths)`: objects, resources (`.resS`) found by name across
//!   them, `Texture2D` and `Sprite` pictures (`texture`, `sprite`) with the same pixels UnityPy gives.

pub mod build;
pub mod bundle;
mod read;
pub mod serialized;
pub mod sprite;
pub mod texture;
pub mod value;

use bundle::Bundle;
use serialized::SerializedFile;
use std::path::{Path, PathBuf};
pub use value::V;

pub type Result<T> = std::result::Result<T, String>;

pub const CLASS_TEXTURE2D: i32 = 28;
pub const CLASS_MONOBEHAVIOUR: i32 = 114;
pub const CLASS_SPRITE: i32 = 213;
pub const CLASS_SPRITE_ATLAS: i32 = 687078895;

/// An object: which serialized file of the [`Env`] and which entry of its object table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obj {
    pub file: usize,
    pub index: usize,
}

/// Bundles loaded together (`UnityPy.load(*paths)`).
pub struct Env {
    pub bundles: Vec<Bundle>,
    /// (bundle index, node index, the file)
    pub files: Vec<(usize, usize, SerializedFile)>,
}

impl Env {
    /// Loads the bundles; an unreadable one is an error, as in UnityPy.
    pub fn load<P: AsRef<Path>>(paths: &[P]) -> Result<Env> {
        let mut env = Env { bundles: Vec::new(), files: Vec::new() };
        for p in paths {
            env.add(Bundle::open(p.as_ref())?)?;
        }
        Ok(env)
    }

    /// Bundles already opened (from memory, say).
    pub fn load_bundles(bundles: Vec<Bundle>) -> Result<Env> {
        let mut env = Env { bundles: Vec::new(), files: Vec::new() };
        for b in bundles {
            env.add(b)?;
        }
        Ok(env)
    }

    pub fn add(&mut self, b: Bundle) -> Result<()> {
        let bi = self.bundles.len();
        for (ni, node) in b.nodes.iter().enumerate() {
            let low = node.path.to_lowercase();
            if low.ends_with(".ress") || low.ends_with(".resource") {
                continue;
            }
            if let Ok(f) = SerializedFile::parse(&b, node) {
                self.files.push((bi, ni, f));
            }
        }
        self.bundles.push(b);
        Ok(())
    }

    /// Every object, file by file in load order (`env.objects`).
    pub fn objects(&self) -> impl Iterator<Item = Obj> + '_ {
        self.files.iter().enumerate().flat_map(|(fi, (_, _, f))| (0..f.objects.len()).map(move |index| Obj { file: fi, index }))
    }

    pub fn class_id(&self, o: Obj) -> i32 {
        let f = &self.files[o.file].2;
        f.class_of(&f.objects[o.index])
    }

    pub fn path_id(&self, o: Obj) -> i64 {
        self.files[o.file].2.objects[o.index].path_id
    }

    pub fn source(&self, o: Obj) -> &PathBuf {
        &self.bundles[self.files[o.file].0].path
    }

    fn raw(&self, o: Obj, len: Option<usize>) -> Result<Vec<u8>> {
        let (bi, ni, f) = &self.files[o.file];
        let ob = &f.objects[o.index];
        let n = len.map_or(ob.size as usize, |l| l.min(ob.size as usize));
        self.bundles[*bi].read(self.bundles[*bi].nodes[*ni].offset + ob.start, n)
    }

    /// The object read through its type tree (`read_typetree()`).
    pub fn read(&self, o: Obj) -> Result<V> {
        let f = &self.files[o.file].2;
        let data = self.raw(o, None)?;
        f.read_object(&f.objects[o.index], &data)
    }

    /// The object's name without reading the rest (`peek_name()`): the leading `m_Name` string of a named object.
    pub fn peek_name(&self, o: Obj) -> Option<String> {
        let f = &self.files[o.file].2;
        let t = f.tree_of(&f.objects[o.index])?;
        if t.nodes.get(1).map(|n| n.name.as_str()) != Some("m_Name") {
            return None;
        }
        let head: [u8; 4] = self.raw(o, Some(4)).ok()?.try_into().ok()?;
        let len = if f.big_endian { i32::from_be_bytes(head) } else { i32::from_le_bytes(head) };
        if len < 0 || len as u64 > f.objects[o.index].size as u64 {
            return None;
        }
        let d = self.raw(o, Some(4 + len as usize)).ok()?;
        Some(String::from_utf8_lossy(&d[4..]).into_owned())
    }

    /// The first object with this path id, in any file (how the Python looks a texture up).
    pub fn find(&self, path_id: i64, class_id: i32) -> Option<Obj> {
        self.objects().find(|&o| self.path_id(o) == path_id && self.class_id(o) == class_id)
    }

    /// A pointer's object: in its own file when `m_FileID` is 0, else (externals aren't followed) the first object
    /// with that path id anywhere in the loaded bundles.
    pub fn deref(&self, from: Obj, pptr: &V, class_id: i32) -> Option<Obj> {
        let pid = pptr.get("m_PathID").as_i64()?;
        if pid == 0 {
            return None;
        }
        if pptr.get("m_FileID").as_i64() == Some(0) {
            let f = &self.files[from.file].2;
            if let Some(index) = f.objects.iter().position(|ob| ob.path_id == pid) {
                return Some(Obj { file: from.file, index });
            }
        }
        self.find(pid, class_id)
    }

    /// A stream's bytes (`archive:/CAB-…/CAB-….resS`), found by its file name in any loaded bundle, as UnityPy's
    /// `get_resource_data` does.
    pub fn resource(&self, path: &str, offset: u64, size: usize) -> Result<Vec<u8>> {
        let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
        let names = [base.to_string(), format!("{stem}.resource"), format!("{stem}.assets.resS"), format!("{stem}.resS")];
        for name in &names {
            let low = name.to_lowercase();
            for b in &self.bundles {
                if let Some(n) = b.nodes.iter().find(|n| n.path.rsplit('/').next().unwrap_or(&n.path).to_lowercase() == low) {
                    if offset + size as u64 > n.size {
                        return Err(format!("{path}: data out of range"));
                    }
                    return b.read(n.offset + offset, size);
                }
            }
        }
        Err(format!("resource {path} not found"))
    }
}
