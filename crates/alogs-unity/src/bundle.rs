//! UnityFS archives: the header, the blocks and directory (block info), and the data, read a block at a time so a
//! bundle's metadata and names can be looked at without decompressing all of it.

use crate::Result;
use crate::read::R;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Archive flags
const COMPRESSION_MASK: u32 = 0x3f;
const BLOCKS_INFO_AT_END: u32 = 0x80;
const BLOCK_INFO_NEEDS_PADDING: u32 = 0x200;

#[derive(Clone, Debug)]
pub struct Node {
    /// where the file starts in the decompressed data
    pub offset: u64,
    pub size: u64,
    pub flags: u32,
    pub path: String,
}

#[derive(Clone, Debug)]
struct Block {
    u_off: u64,
    c_off: u64,
    u_size: u32,
    c_size: u32,
    flags: u16,
}

pub struct Bundle {
    pub path: PathBuf,
    pub unity_version: String,
    pub nodes: Vec<Node>,
    blocks: Vec<Block>,
    src: RefCell<Source>,
    cache: RefCell<HashMap<usize, Rc<Vec<u8>>>>,
}

enum Source {
    File(std::fs::File),
    Mem(Vec<u8>),
}

impl Source {
    fn read_at(&mut self, off: u64, n: usize) -> Result<Vec<u8>> {
        match self {
            Source::File(f) => {
                f.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
                let mut buf = vec![0; n];
                f.read_exact(&mut buf).map_err(|e| format!("bundle cut short: {e}"))?;
                Ok(buf)
            }
            Source::Mem(d) => {
                let s = d.get(off as usize..off as usize + n).ok_or("bundle cut short")?;
                Ok(s.to_vec())
            }
        }
    }
    fn len(&mut self) -> Result<u64> {
        match self {
            Source::File(f) => f.metadata().map(|m| m.len()).map_err(|e| e.to_string()),
            Source::Mem(d) => Ok(d.len() as u64),
        }
    }
}

/// A compressed block (UnityPy's CompressionHelper): 0 none, 1 LZMA (5 property bytes, then the stream), 2/3 LZ4.
pub fn decompress(kind: u32, data: &[u8], size: usize) -> Result<Vec<u8>> {
    match kind {
        0 => Ok(data.to_vec()),
        1 => {
            let mut out = Vec::with_capacity(size);
            let opts = lzma_rs::decompress::Options {
                unpacked_size: lzma_rs::decompress::UnpackedSize::UseProvided(Some(size as u64)),
                ..Default::default()
            };
            lzma_rs::lzma_decompress_with_options(&mut std::io::Cursor::new(data), &mut out, &opts).map_err(|e| format!("LZMA: {e}"))?;
            Ok(out)
        }
        2 | 3 => lz4_flex::block::decompress(data, size).map_err(|e| format!("LZ4: {e}")),
        k => Err(format!("unknown compression {k}")),
    }
}

impl Bundle {
    pub fn open(path: &Path) -> Result<Bundle> {
        let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Bundle::from_source(path.to_path_buf(), Source::File(f))
    }

    pub fn from_bytes(name: &str, data: Vec<u8>) -> Result<Bundle> {
        Bundle::from_source(PathBuf::from(name), Source::Mem(data))
    }

    fn from_source(path: PathBuf, mut src: Source) -> Result<Bundle> {
        let len = src.len()?;
        let head = src.read_at(0, len.min(1024) as usize)?;
        let mut r = R::new(&head, true);
        let sig = r.cstr()?;
        if sig != "UnityFS" {
            return Err(format!("not a UnityFS bundle ({sig:?})"));
        }
        let version = r.u32()?;
        let _player = r.cstr()?;
        let unity_version = r.cstr()?;
        let _size = r.i64()?;
        let (c_info, u_info, flags) = (r.u32()? as usize, r.u32()? as usize, r.u32()?);
        if version >= 7 {
            r.align(16);
        }
        let mut pos = r.p as u64;
        let info_at = if flags & BLOCKS_INFO_AT_END != 0 { len.checked_sub(c_info as u64).ok_or("bad block info size")? } else { pos };
        let raw = src.read_at(info_at, c_info)?;
        let info = decompress(flags & COMPRESSION_MASK, &raw, u_info)?;
        if flags & BLOCKS_INFO_AT_END == 0 {
            pos += c_info as u64;
        }
        if flags & BLOCK_INFO_NEEDS_PADDING != 0 {
            pos = pos.div_ceil(16) * 16;
        }
        let mut r = R::new(&info, true);
        r.take(16)?; // hash
        let count = r.i32()?;
        let (mut u_off, mut c_off) = (0u64, pos);
        let mut blocks = Vec::new();
        for _ in 0..count.max(0) {
            let (u_size, c_size, flags) = (r.u32()?, r.u32()?, r.u16()?);
            blocks.push(Block { u_off, c_off, u_size, c_size, flags });
            u_off += u_size as u64;
            c_off += c_size as u64;
        }
        let n = r.i32()?;
        let mut nodes = Vec::new();
        for _ in 0..n.max(0) {
            let (offset, size, flags) = (r.i64()? as u64, r.i64()? as u64, r.u32()?);
            nodes.push(Node { offset, size, flags, path: r.cstr()? });
        }
        Ok(Bundle { path, unity_version, nodes, blocks, src: RefCell::new(src), cache: RefCell::new(HashMap::new()) })
    }

    fn block(&self, i: usize) -> Result<Rc<Vec<u8>>> {
        if let Some(b) = self.cache.borrow().get(&i) {
            return Ok(b.clone());
        }
        let b = &self.blocks[i];
        let raw = self.src.borrow_mut().read_at(b.c_off, b.c_size as usize)?;
        let data = Rc::new(decompress(b.flags as u32 & COMPRESSION_MASK, &raw, b.u_size as usize)?);
        if data.len() != b.u_size as usize {
            return Err("block decompressed to the wrong size".into());
        }
        self.cache.borrow_mut().insert(i, data.clone());
        Ok(data)
    }

    /// `n` bytes of the decompressed data from `start`.
    pub fn read(&self, start: u64, n: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(n);
        let first = self.blocks.partition_point(|b| b.u_off + b.u_size as u64 <= start);
        let mut at = start;
        for i in first..self.blocks.len() {
            if out.len() == n {
                break;
            }
            let b = &self.blocks[i];
            let data = self.block(i)?;
            let from = (at - b.u_off) as usize;
            let take = (n - out.len()).min(data.len() - from);
            out.extend_from_slice(&data[from..from + take]);
            at += take as u64;
        }
        if out.len() != n {
            return Err("read past the bundle's data".into());
        }
        Ok(out)
    }

    /// The whole of a node's file.
    pub fn node_data(&self, node: &Node) -> Result<Vec<u8>> {
        self.read(node.offset, node.size as usize)
    }

    /// Forgets the decompressed blocks (after a big texture).
    pub fn drop_cache(&self) {
        self.cache.borrow_mut().clear();
    }
}
