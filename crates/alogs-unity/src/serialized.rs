//! Serialized files (the CAB-… files in a bundle): their types with type trees and their object table, and objects
//! read through the type trees (UnityPy's SerializedFile and TypeTreeHelper.read_typetree).

use crate::Result;
use crate::bundle::{Bundle, Node};
use crate::read::R;
use crate::value::V;

/// Unity's shared type-tree strings: an offset with the top bit set points in here.
const COMMON_STRINGS: &str = "AABB\0AnimationClip\0AnimationCurve\0AnimationState\0Array\0Base\0BitField\0bitset\0bool\0char\0\
ColorRGBA\0Component\0data\0deque\0double\0dynamic_array\0FastPropertyName\0first\0float\0Font\0GameObject\0Generic Mono\0\
GradientNEW\0GUID\0GUIStyle\0int\0list\0long long\0map\0Matrix4x4f\0MdFour\0MonoBehaviour\0MonoScript\0m_ByteSize\0m_Curve\0\
m_EditorClassIdentifier\0m_EditorHideFlags\0m_Enabled\0m_ExtensionPtr\0m_GameObject\0m_Index\0m_IsArray\0m_IsStatic\0\
m_MetaFlag\0m_Name\0m_ObjectHideFlags\0m_PrefabInternal\0m_PrefabParentObject\0m_Script\0m_StaticEditorFlags\0m_Type\0\
m_Version\0Object\0pair\0PPtr<Component>\0PPtr<GameObject>\0PPtr<Material>\0PPtr<MonoBehaviour>\0PPtr<MonoScript>\0\
PPtr<Object>\0PPtr<Prefab>\0PPtr<Sprite>\0PPtr<TextAsset>\0PPtr<Texture>\0PPtr<Texture2D>\0PPtr<Transform>\0Prefab\0\
Quaternionf\0Rectf\0RectInt\0RectOffset\0second\0set\0short\0size\0SInt16\0SInt32\0SInt64\0SInt8\0staticvector\0string\0\
TextAsset\0TextMesh\0Texture\0Texture2D\0Transform\0TypelessData\0UInt16\0UInt32\0UInt64\0UInt8\0unsigned int\0\
unsigned long long\0unsigned short\0vector\0Vector2f\0Vector3f\0Vector4f\0m_ScriptingClassIdentifier\0Gradient\0Type*\0\
int2_storage\0int3_storage\0BoundsInt\0m_CorrespondingSourceObject\0m_PrefabInstance\0m_PrefabAsset\0FileSize\0Hash128\0\
RenderingLayerMask\0fixed_array\0EntityId\0LoadableObjectId\0LoadableSceneId\0";

const ALIGN: i32 = 0x4000;

fn string_at(buf: &[u8], off: u32) -> String {
    let (buf, off) = if off & 0x8000_0000 != 0 { (COMMON_STRINGS.as_bytes(), (off & 0x7fff_ffff) as usize) } else { (buf, off as usize) };
    match buf.get(off..) {
        Some(s) => String::from_utf8_lossy(&s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]).into_owned(),
        None => off.to_string(),
    }
}

#[derive(Clone, Debug)]
pub struct TypeNode {
    pub ty: String,
    pub name: String,
    pub level: u8,
    pub type_flags: u8,
    pub byte_size: i32,
    pub meta: i32,
}

#[derive(Clone, Debug, Default)]
pub struct TypeTree {
    pub nodes: Vec<TypeNode>,
}

#[derive(Clone, Debug)]
pub struct SerializedType {
    pub class_id: i32,
    pub tree: Option<TypeTree>,
}

#[derive(Clone, Debug)]
pub struct ObjectInfo {
    pub path_id: i64,
    /// from the file's start
    pub start: u64,
    pub size: u32,
    pub type_index: i32,
}

pub struct SerializedFile {
    pub version: u32,
    pub unity_version: String,
    pub big_endian: bool,
    pub types: Vec<SerializedType>,
    pub objects: Vec<ObjectInfo>,
}

fn read_tree(r: &mut R, version: u32) -> Result<TypeTree> {
    let (count, buf_size) = (r.i32()?, r.i32()?);
    if count < 0 || buf_size < 0 || count as usize * 24 > r.left() {
        return Err("bad type tree".into());
    }
    let mut raw = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (_ver, level, type_flags) = (r.u16()?, r.u8()?, r.u8()?);
        let (ty, name, byte_size, _index, meta) = (r.u32()?, r.u32()?, r.i32()?, r.i32()?, r.i32()?);
        if version >= 19 {
            r.u64()?; // ref type hash
        }
        raw.push((level, type_flags, ty, name, byte_size, meta));
    }
    let buf = r.take(buf_size as usize)?;
    let nodes = raw
        .into_iter()
        .map(|(level, type_flags, ty, name, byte_size, meta)| TypeNode { ty: string_at(buf, ty), name: string_at(buf, name), level, type_flags, byte_size, meta })
        .collect();
    Ok(TypeTree { nodes })
}

impl SerializedFile {
    /// Parses a node of the bundle as a serialized file (its header and metadata only).
    pub fn parse(b: &Bundle, node: &Node) -> Result<SerializedFile> {
        if node.size < 48 {
            return Err("too small".into());
        }
        let head = b.read(node.offset, 48)?;
        let mut r = R::new(&head, true);
        let (mut meta_size, _file_size, version, mut data_offset) = (r.u32()? as u64, r.u32()? as u64, r.u32()?, r.u32()? as u64);
        if !(17..=50).contains(&version) {
            return Err(format!("serialized file version {version} not supported"));
        }
        let big_endian = r.u8()? != 0;
        r.take(3)?;
        if version >= 22 {
            meta_size = r.u32()? as u64;
            let _file_size = r.i64()?;
            data_offset = r.i64()? as u64;
            r.i64()?;
        }
        let meta_start = r.p as u64;
        if meta_start + meta_size > node.size {
            return Err("metadata out of range".into());
        }
        let meta = b.read(node.offset + meta_start, meta_size as usize)?;
        let mut r = R::new(&meta, big_endian);
        let unity_version = r.cstr()?;
        let _platform = r.i32()?;
        let type_tree = r.u8()? != 0;
        let n = r.i32()?;
        let mut types = Vec::new();
        for _ in 0..n.max(0) {
            let class_id = r.i32()?;
            let _stripped = r.u8()?;
            let _script_index = r.i16()?;
            if class_id == 114 {
                r.take(16)?; // script id
            }
            r.take(16)?; // old type hash
            let tree = if type_tree { Some(read_tree(&mut r, version)?) } else { None };
            if type_tree && version >= 21 {
                let deps = r.i32()?;
                r.take(4 * deps.max(0) as usize)?;
            }
            types.push(SerializedType { class_id, tree });
        }
        let n = r.i32()?;
        let mut objects = Vec::new();
        for _ in 0..n.max(0) {
            r.align(4);
            let path_id = r.i64()?;
            let start = if version >= 22 { r.i64()? as u64 } else { r.u32()? as u64 } + data_offset;
            let size = r.u32()?;
            let type_index = r.i32()?;
            if start + size as u64 > node.size {
                return Err("object out of range".into());
            }
            objects.push(ObjectInfo { path_id, start, size, type_index });
        }
        Ok(SerializedFile { version, unity_version, big_endian, types, objects })
    }

    pub fn class_of(&self, o: &ObjectInfo) -> i32 {
        self.types.get(o.type_index as usize).map_or(-1, |t| t.class_id)
    }

    pub fn tree_of(&self, o: &ObjectInfo) -> Option<&TypeTree> {
        self.types.get(o.type_index as usize)?.tree.as_ref()
    }

    /// The object (its bytes) read through its type tree.
    pub fn read_object(&self, o: &ObjectInfo, data: &[u8]) -> Result<V> {
        let tree = self.tree_of(o).ok_or("the object has no type tree")?;
        if tree.nodes.is_empty() {
            return Err("empty type tree".into());
        }
        let mut r = R::new(data, self.big_endian);
        read_value(&tree.nodes, 0, &mut r)
    }
}

/// The direct children of node `i`.
fn children(nodes: &[TypeNode], i: usize) -> Vec<usize> {
    let lv = nodes[i].level;
    let mut out = Vec::new();
    for (j, n) in nodes.iter().enumerate().skip(i + 1) {
        if n.level <= lv {
            break;
        }
        if n.level == lv + 1 {
            out.push(j);
        }
    }
    out
}

/// One value of the type tree from node `i` (TypeTreeHelper.read_value).
pub fn read_value(nodes: &[TypeNode], i: usize, r: &mut R) -> Result<V> {
    let n = &nodes[i];
    let mut align = n.meta & ALIGN != 0;
    let v = match n.ty.as_str() {
        "SInt8" => V::Int(r.u8()? as i8 as i64),
        "UInt8" | "char" => V::Int(r.u8()? as i64),
        "short" | "SInt16" => V::Int(r.i16()? as i64),
        "UInt16" | "unsigned short" => V::Int(r.u16()? as i64),
        "int" | "SInt32" => V::Int(r.i32()? as i64),
        "UInt32" | "unsigned int" | "Type*" => V::Int(r.u32()? as i64),
        "long long" | "SInt64" => V::Int(r.i64()?),
        "UInt64" | "unsigned long long" | "FileSize" => V::Int(r.u64()? as i64),
        "float" => V::Float(r.f32()? as f64),
        "double" => V::Float(r.f64()?),
        "bool" => V::Bool(r.u8()? != 0),
        "string" => {
            let len = r.i32()?;
            let s = r.take(len.max(0) as usize)?;
            if let Some(&c) = children(nodes, i).first() {
                align |= nodes[c].meta & ALIGN != 0;
            }
            V::Str(String::from_utf8_lossy(s).into_owned())
        }
        "TypelessData" => {
            let len = r.i32()?;
            V::Bytes(r.take(len.max(0) as usize)?.to_vec())
        }
        "ManagedReferencesRegistry" => return Err("managed references aren't read".into()),
        _ => {
            let ch = children(nodes, i);
            match ch.first() {
                Some(&a) if nodes[a].type_flags & 1 != 0 || nodes[a].ty == "Array" => {
                    align |= nodes[a].meta & ALIGN != 0;
                    let size = r.i32()?;
                    if size < 0 || size as usize > r.left() + 1 {
                        return Err(format!("bad array size {size} for {}", n.name));
                    }
                    let data = *children(nodes, a).get(1).ok_or("array without data")?;
                    if matches!(nodes[data].ty.as_str(), "UInt8" | "char") && children(nodes, data).is_empty() {
                        let b = r.take(size as usize)?.to_vec();
                        if nodes[data].meta & ALIGN != 0 {
                            r.align(4);
                        }
                        V::Bytes(b)
                    } else {
                        let mut items = Vec::with_capacity(size as usize);
                        for _ in 0..size {
                            items.push(read_value(nodes, data, r)?);
                        }
                        V::List(items)
                    }
                }
                _ => {
                    let mut fields = Vec::with_capacity(ch.len());
                    for c in ch {
                        fields.push((nodes[c].name.clone(), read_value(nodes, c, r)?));
                    }
                    V::Obj(fields)
                }
            }
        }
    };
    if align {
        r.align(4);
    }
    Ok(v)
}
