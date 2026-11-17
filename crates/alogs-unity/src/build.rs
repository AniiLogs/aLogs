//! Writing small made-up bundles, for tests (never game files): a serialized file (format 22, little endian) with
//! type trees, wrapped in a UnityFS archive with LZ4 or LZMA blocks. The inverse of the readers.

use crate::serialized::TypeNode;
use crate::value::V;

/// A type tree node. `array` marks the `Array` node of a vector or string; `align` its 0x4000 meta flag.
pub fn node(level: u8, ty: &str, name: &str, array: bool, align: bool) -> TypeNode {
    TypeNode { ty: ty.into(), name: name.into(), level, type_flags: array as u8, byte_size: -1, meta: if align { 0x4000 } else { 0 } }
}

/// A plain field.
pub fn prim(level: u8, ty: &str, name: &str) -> Vec<TypeNode> {
    vec![node(level, ty, name, false, false)]
}

/// A string field (always aligned after).
pub fn string(level: u8, name: &str) -> Vec<TypeNode> {
    vec![node(level, "string", name, false, false), node(level + 1, "Array", "Array", true, true), node(level + 2, "SInt32", "size", false, false), node(level + 2, "char", "data", false, false)]
}

/// A vector of `elem` (built at level + 2).
pub fn vector(level: u8, name: &str, align: bool, elem: Vec<TypeNode>) -> Vec<TypeNode> {
    let mut v = vec![node(level, "vector", name, false, false), node(level + 1, "Array", "Array", true, align), node(level + 2, "SInt32", "size", false, false)];
    v.extend(elem);
    v
}

/// A class field holding `fields` (built at level + 1).
pub fn class(level: u8, ty: &str, name: &str, fields: Vec<Vec<TypeNode>>) -> Vec<TypeNode> {
    let mut v = vec![node(level, ty, name, false, false)];
    v.extend(fields.into_iter().flatten());
    v
}

fn children(nodes: &[TypeNode], i: usize) -> Vec<usize> {
    let lv = nodes[i].level;
    nodes.iter().enumerate().skip(i + 1).take_while(|(_, n)| n.level > lv).filter(|(_, n)| n.level == lv + 1).map(|(j, _)| j).collect()
}

fn pad(out: &mut Vec<u8>, n: usize) {
    while !out.len().is_multiple_of(n) {
        out.push(0);
    }
}

/// Writes `v` as node `i` describes it (alignment relative to `out`'s start).
pub fn encode(nodes: &[TypeNode], i: usize, v: &V, out: &mut Vec<u8>) {
    let n = &nodes[i];
    let mut align = n.meta & 0x4000 != 0;
    let int = v.as_i64().unwrap_or(0);
    match n.ty.as_str() {
        "SInt8" | "UInt8" | "char" | "bool" => out.push(int as u8),
        "short" | "SInt16" | "UInt16" | "unsigned short" => out.extend((int as i16).to_le_bytes()),
        "int" | "SInt32" | "UInt32" | "unsigned int" | "Type*" => out.extend((int as i32).to_le_bytes()),
        "long long" | "SInt64" | "UInt64" | "unsigned long long" | "FileSize" => out.extend(int.to_le_bytes()),
        "float" => out.extend((v.as_f64().unwrap_or(0.0) as f32).to_le_bytes()),
        "double" => out.extend(v.as_f64().unwrap_or(0.0).to_le_bytes()),
        "string" => {
            let s = v.as_str().unwrap_or("");
            out.extend((s.len() as i32).to_le_bytes());
            out.extend(s.as_bytes());
            align = true;
        }
        "TypelessData" => {
            out.extend((v.as_bytes().len() as i32).to_le_bytes());
            out.extend(v.as_bytes());
        }
        _ => {
            let ch = children(nodes, i);
            match ch.first() {
                Some(&a) if nodes[a].type_flags & 1 != 0 => {
                    align |= nodes[a].meta & 0x4000 != 0;
                    let data = children(nodes, a)[1];
                    match v {
                        V::Bytes(b) => {
                            out.extend((b.len() as i32).to_le_bytes());
                            out.extend(b);
                        }
                        _ => {
                            out.extend((v.as_list().len() as i32).to_le_bytes());
                            for item in v.as_list() {
                                encode(nodes, data, item, out);
                            }
                        }
                    }
                }
                _ => {
                    for c in ch {
                        encode(nodes, c, v.get(&nodes[c].name), out);
                    }
                }
            }
        }
    }
    if align {
        pad(out, 4);
    }
}

pub struct Type {
    pub class_id: i32,
    pub nodes: Vec<TypeNode>,
}

pub struct Object {
    pub path_id: i64,
    pub type_index: usize,
    pub value: V,
}

/// A serialized file (format 22) holding the objects.
pub fn serialized_file(types: &[Type], objects: &[Object]) -> Vec<u8> {
    let mut meta = Vec::new();
    meta.extend(b"2022.3.62f1\0");
    meta.extend(19i32.to_le_bytes());
    meta.push(1);
    meta.extend((types.len() as i32).to_le_bytes());
    for t in types {
        meta.extend(t.class_id.to_le_bytes());
        meta.push(0);
        meta.extend((-1i16).to_le_bytes());
        if t.class_id == 114 {
            meta.extend([0; 16]);
        }
        meta.extend([0; 16]);
        let mut strings = Vec::new();
        let mut off = |s: &str| {
            let o = strings.len() as u32;
            strings.extend(s.as_bytes());
            strings.push(0);
            o
        };
        let mut nodes = Vec::new();
        for (i, n) in t.nodes.iter().enumerate() {
            nodes.extend(1u16.to_le_bytes());
            nodes.push(n.level);
            nodes.push(n.type_flags);
            nodes.extend(off(&n.ty).to_le_bytes());
            nodes.extend(off(&n.name).to_le_bytes());
            nodes.extend(n.byte_size.to_le_bytes());
            nodes.extend((i as i32).to_le_bytes());
            nodes.extend(n.meta.to_le_bytes());
            nodes.extend(0u64.to_le_bytes());
        }
        meta.extend((t.nodes.len() as i32).to_le_bytes());
        meta.extend((strings.len() as i32).to_le_bytes());
        meta.extend(nodes);
        meta.extend(strings);
        meta.extend(0i32.to_le_bytes()); // type dependencies
    }
    let mut data = Vec::new();
    let mut table = Vec::new();
    for o in objects {
        pad(&mut data, 8);
        let start = data.len();
        encode(&types[o.type_index].nodes, 0, &o.value, &mut data);
        table.push((o.path_id, start, data.len() - start, o.type_index));
    }
    meta.extend((objects.len() as i32).to_le_bytes());
    for (pid, start, size, ti) in table {
        pad(&mut meta, 4);
        meta.extend(pid.to_le_bytes());
        meta.extend((start as i64).to_le_bytes());
        meta.extend((size as u32).to_le_bytes());
        meta.extend((ti as i32).to_le_bytes());
    }
    meta.extend([0; 12]); // scripts, externals, ref types
    meta.push(0); // user information
    let data_offset = (48 + meta.len()).div_ceil(16) * 16;
    let mut out = Vec::new();
    out.extend([0u8; 8]);
    out.extend(22u32.to_be_bytes());
    out.extend([0u8; 4]);
    out.extend([0u8; 4]); // little endian + reserved
    out.extend((meta.len() as u32).to_be_bytes());
    out.extend(((data_offset + data.len()) as i64).to_be_bytes());
    out.extend((data_offset as i64).to_be_bytes());
    out.extend(0i64.to_be_bytes());
    out.extend(meta);
    out.resize(data_offset, 0);
    out.extend(data);
    out
}

/// LZMA as Unity stores it: the 5 property bytes, then the stream (no size).
fn lzma(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let opts = lzma_rs::compress::Options { unpacked_size: lzma_rs::compress::UnpackedSize::SkipWritingToHeader };
    lzma_rs::lzma_compress_with_options(&mut std::io::Cursor::new(data), &mut out, &opts).unwrap();
    out
}

fn compress(kind: u32, data: &[u8]) -> Vec<u8> {
    match kind {
        0 => data.to_vec(),
        1 => lzma(data),
        _ => lz4_flex::block::compress(data),
    }
}

/// A UnityFS archive (format 8, the block info LZ4HC-flagged and padded as the game's are) of the named files
/// (`(path, bytes, node flags)`: 4 for a serialized file), its data cut into `block_size` blocks compressed with
/// `kind` (0 none, 1 LZMA, 3 LZ4HC).
pub fn bundle(files: &[(&str, Vec<u8>, u32)], kind: u32, block_size: usize) -> Vec<u8> {
    let mut data = Vec::new();
    let mut nodes = Vec::new();
    for (path, bytes, flags) in files {
        nodes.push((data.len() as i64, bytes.len() as i64, *flags, *path));
        data.extend(bytes);
    }
    let mut info = vec![0u8; 16];
    let chunks: Vec<&[u8]> = data.chunks(block_size.max(1)).collect();
    let packed: Vec<Vec<u8>> = chunks.iter().map(|c| compress(kind, c)).collect();
    info.extend((chunks.len() as i32).to_be_bytes());
    for (c, p) in chunks.iter().zip(&packed) {
        info.extend((c.len() as u32).to_be_bytes());
        info.extend((p.len() as u32).to_be_bytes());
        info.extend((kind as u16 | 0x40).to_be_bytes());
    }
    info.extend((nodes.len() as i32).to_be_bytes());
    for (off, size, flags, path) in nodes {
        info.extend(off.to_be_bytes());
        info.extend(size.to_be_bytes());
        info.extend(flags.to_be_bytes());
        info.extend(path.as_bytes());
        info.push(0);
    }
    let cinfo = lz4_flex::block::compress(&info);
    let mut out = Vec::new();
    out.extend(b"UnityFS\0");
    out.extend(8u32.to_be_bytes());
    out.extend(b"5.x.x\0");
    out.extend(b"2022.3.62f1\0");
    let size_at = out.len();
    out.extend(0i64.to_be_bytes());
    out.extend((cinfo.len() as u32).to_be_bytes());
    out.extend((info.len() as u32).to_be_bytes());
    out.extend((0x243u32).to_be_bytes());
    pad(&mut out, 16);
    out.extend(cinfo);
    pad(&mut out, 16);
    for p in packed {
        out.extend(p);
    }
    let total = out.len() as i64;
    out[size_at..size_at + 8].copy_from_slice(&total.to_be_bytes());
    out
}
