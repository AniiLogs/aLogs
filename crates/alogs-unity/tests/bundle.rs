//! Made-up bundles (never game files) read back: objects through their type trees, textures inline and in a .resS,
//! and a tight sprite masked by its mesh. The pictures' md5s are UnityPy 1.25's picture code on the same bundle (write
//! it with `ALOGS_SAMPLE_BUNDLE=<path> cargo test -p alogs-unity`, then `python tools/unitypy_sample_md5.py <path>`).

use alogs_unity::build::{self, Object, Type, class, prim, string, vector};
use alogs_unity::{Env, V, bundle::Bundle};

fn obj(fields: Vec<(&str, V)>) -> V {
    V::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
fn i(v: i64) -> V {
    V::Int(v)
}
fn f(v: f64) -> V {
    V::Float(v)
}
fn s(v: &str) -> V {
    V::Str(v.into())
}
fn pptr(pid: i64) -> V {
    obj(vec![("m_FileID", i(0)), ("m_PathID", i(pid))])
}

fn pptr_t(level: u8, name: &str) -> Vec<alogs_unity::serialized::TypeNode> {
    class(level, "PPtr<Texture2D>", name, vec![prim(level + 1, "int", "m_FileID"), prim(level + 1, "SInt64", "m_PathID")])
}

fn rectf(level: u8, name: &str) -> Vec<alogs_unity::serialized::TypeNode> {
    class(level, "Rectf", name, vec![prim(level + 1, "float", "x"), prim(level + 1, "float", "y"), prim(level + 1, "float", "width"), prim(level + 1, "float", "height")])
}

fn texture_type() -> Type {
    Type {
        class_id: 28,
        nodes: class(
            0,
            "Texture2D",
            "Base",
            vec![
                string(1, "m_Name"),
                prim(1, "int", "m_Width"),
                prim(1, "int", "m_Height"),
                prim(1, "int", "m_TextureFormat"),
                prim(1, "TypelessData", "image data"),
                class(1, "StreamingInfo", "m_StreamData", vec![prim(2, "UInt64", "offset"), prim(2, "unsigned int", "size"), string(2, "path")]),
            ],
        ),
    }
}

fn sprite_type() -> Type {
    let channel = class(5, "ChannelInfo", "data", vec![prim(6, "UInt8", "stream"), prim(6, "UInt8", "offset"), prim(6, "UInt8", "format"), prim(6, "UInt8", "dimension")]);
    let submesh = class(4, "SubMesh", "data", vec![prim(5, "unsigned int", "firstByte"), prim(5, "unsigned int", "indexCount"), prim(5, "int", "topology")]);
    let ib = vector(2, "m_IndexBuffer", true, prim(4, "UInt8", "data"));
    Type {
        class_id: 213,
        nodes: class(
            0,
            "Sprite",
            "Base",
            vec![
                string(1, "m_Name"),
                rectf(1, "m_Rect"),
                prim(1, "float", "m_PixelsToUnits"),
                vector(1, "m_AtlasTags", false, string(3, "data")),
                class(1, "PPtr<SpriteAtlas>", "m_SpriteAtlas", vec![prim(2, "int", "m_FileID"), prim(2, "SInt64", "m_PathID")]),
                class(
                    1,
                    "SpriteRenderData",
                    "m_RD",
                    vec![
                        pptr_t(2, "texture"),
                        pptr_t(2, "alphaTexture"),
                        vector(2, "m_SubMeshes", false, submesh),
                        ib,
                        class(
                            2,
                            "VertexData",
                            "m_VertexData",
                            vec![prim(3, "unsigned int", "m_VertexCount"), vector(3, "m_Channels", false, channel), prim(3, "TypelessData", "m_DataSize")],
                        ),
                        rectf(2, "textureRect"),
                        class(2, "Vector2f", "textureRectOffset", vec![prim(3, "float", "x"), prim(3, "float", "y")]),
                        prim(2, "unsigned int", "settingsRaw"),
                    ],
                ),
            ],
        ),
    }
}

fn pattern(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn texture(name: &str, w: i64, h: i64, format: i64, inline: Vec<u8>, stream: Option<(u64, usize, &str)>) -> V {
    let (off, size, path) = stream.unwrap_or((0, 0, ""));
    obj(vec![
        ("m_Name", s(name)),
        ("m_Width", i(w)),
        ("m_Height", i(h)),
        ("m_TextureFormat", i(format)),
        ("image data", V::Bytes(inline)),
        ("m_StreamData", obj(vec![("offset", i(off as i64)), ("size", i(size as i64)), ("path", s(path))])),
    ])
}

/// A 7x5 tight sprite (pixels to units 10) on the 16x12 RGBA texture at (3, 4): a quad of two triangles with one
/// corner cut off (5 vertices), positions centred on the pivot.
fn sprite() -> V {
    let verts: [[f32; 3]; 5] = [[-0.35, -0.25, 0.0], [0.35, -0.25, 0.0], [0.35, 0.05, 0.0], [0.1, 0.25, 0.0], [-0.35, 0.25, 0.0]];
    let mut data: Vec<u8> = verts.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    data.resize(64, 0); // stream 0 (5 x 12 bytes), aligned to 16
    data.extend([0u8; 40]); // stream 1: UV0, all 0 (so UnityPy masks rather than copies triangles)
    let idx: Vec<u8> = [0u16, 1, 2, 0, 2, 3, 0, 3, 4].iter().flat_map(|v| v.to_le_bytes()).collect();
    let ch = |stream: i64, dim: i64| obj(vec![("stream", i(stream)), ("offset", i(0)), ("format", i(0)), ("dimension", i(dim))]);
    let mut channels = vec![ch(0, 3)];
    channels.extend((1..4).map(|_| ch(0, 0)));
    channels.push(ch(1, 2));
    channels.extend((5..14).map(|_| ch(0, 0)));
    let rect = |x: f64, y: f64, w: f64, h: f64| obj(vec![("x", f(x)), ("y", f(y)), ("width", f(w)), ("height", f(h))]);
    obj(vec![
        ("m_Name", s("UI_Test_Sprite")),
        ("m_Rect", rect(0.0, 0.0, 9.0, 7.0)),
        ("m_PixelsToUnits", f(10.0)),
        ("m_AtlasTags", V::List(vec![])),
        ("m_SpriteAtlas", pptr(0)),
        (
            "m_RD",
            obj(vec![
                ("texture", pptr(-77)),
                ("alphaTexture", pptr(0)),
                ("m_SubMeshes", V::List(vec![obj(vec![("firstByte", i(0)), ("indexCount", i(9)), ("topology", i(0))])])),
                ("m_IndexBuffer", V::Bytes(idx)),
                ("m_VertexData", obj(vec![("m_VertexCount", i(5)), ("m_Channels", V::List(channels)), ("m_DataSize", V::Bytes(data))])),
                ("textureRect", rect(3.0, 4.2, 7.0, 5.0)),
                ("textureRectOffset", obj(vec![("x", f(1.0)), ("y", f(1.0))])),
                ("settingsRaw", i(64)),
            ]),
        ),
    ])
}

/// The sample bundle: a serialized file (an Alpha8 texture in the .resS, an RGBA32 one and a DXT1 one inline, the
/// sprite) and its .resS, in `block` byte blocks compressed with `kind`.
fn sample(kind: u32, block: usize) -> Vec<u8> {
    let alpha = pattern(10 * 6, 3);
    let mut ress = pattern(5, 9); // something before it
    let off = ress.len() as u64;
    ress.extend(&alpha);
    let types = [texture_type(), sprite_type()];
    let objects = [
        Object { path_id: 5, type_index: 0, value: texture("UI_Test_Atlas", 10, 6, 1, vec![], Some((off, alpha.len(), "archive:/CAB-test/CAB-test.resS"))) },
        Object { path_id: -77, type_index: 0, value: texture("UI_Test_Rgba", 16, 12, 4, pattern(16 * 12 * 4, 5), None) },
        Object { path_id: 9, type_index: 0, value: texture("UI_Test_Dxt1", 6, 7, 10, pattern(4 * 8, 11), None) },
        Object { path_id: 1234567890123, type_index: 1, value: sprite() },
    ];
    let file = build::serialized_file(&types, &objects);
    build::bundle(&[("CAB-test", file, 4), ("CAB-test.resS", ress, 0)], kind, block)
}

fn md5(im: &alogs_pil::Image) -> String {
    format!("{:x}", md5::compute(&im.data))
}

/// UnityPy's pictures of the sample (tools/unitypy_sample_md5.py): name -> (size, mode, md5 of the bytes)
const WANT: [(&str, (usize, usize), &str, &str); 4] = [
    ("UI_Test_Atlas", (10, 6), "RGBA", "0eca6655ca33e4d047ad4f31012ea9a2"),
    ("UI_Test_Rgba", (16, 12), "RGBA", "8966daa4fffa970e1470c05bc33212d3"),
    ("UI_Test_Dxt1", (6, 7), "RGBA", "61d4071689d9c632023c55b5840df56c"),
    ("UI_Test_Sprite", (7, 5), "RGBA", "27c5ce3f924fc3b5ae8a0447c7f95c93"),
];

fn check(env: &Env) {
    let mut seen = 0;
    for o in env.objects().collect::<Vec<_>>() {
        let name = env.peek_name(o).unwrap();
        let (_, size, mode, want) = WANT.iter().find(|w| w.0 == name).unwrap();
        let im = if env.class_id(o) == 213 { env.sprite(o).unwrap() } else { env.texture(o).unwrap() };
        assert_eq!(((im.width, im.height), im.mode.name(), md5(&im).as_str()), (*size, *mode, *want), "{name}");
        seen += 1;
    }
    assert_eq!(seen, 4);
}

#[test]
fn sample_matches_unitypy() {
    if let Some(p) = std::env::var_os("ALOGS_SAMPLE_BUNDLE") {
        std::fs::write(p, sample(3, 64)).unwrap();
    }
    for (kind, block) in [(3, 64), (3, 1 << 17), (1, 100), (0, 1 << 20)] {
        let env = Env::load_bundles(vec![Bundle::from_bytes("sample", sample(kind, block)).unwrap()]).unwrap();
        check(&env);
    }
}

#[test]
fn type_trees_read_back() {
    let env = Env::load_bundles(vec![Bundle::from_bytes("sample", sample(3, 50)).unwrap()]).unwrap();
    let objs: Vec<_> = env.objects().collect();
    assert_eq!(objs.iter().map(|&o| env.path_id(o)).collect::<Vec<_>>(), [5, -77, 9, 1234567890123]);
    let t = env.read(objs[0]).unwrap();
    assert_eq!(t.get("m_StreamData").get("path").as_str(), Some("archive:/CAB-test/CAB-test.resS"));
    assert_eq!(t.get("m_StreamData").get("size").as_i64(), Some(60));
    let sp = env.read(objs[3]).unwrap();
    assert_eq!(sp, sprite().as_f32());
    assert_eq!(env.find(-77, 28), Some(objs[1]));
}

trait F32 {
    fn as_f32(&self) -> V;
}
impl F32 for V {
    /// floats as they come back from a float field
    fn as_f32(&self) -> V {
        match self {
            V::Float(x) => V::Float(*x as f32 as f64),
            V::List(l) => V::List(l.iter().map(F32::as_f32).collect()),
            V::Obj(o) => V::Obj(o.iter().map(|(k, v)| (k.clone(), v.as_f32())).collect()),
            v => v.clone(),
        }
    }
}
