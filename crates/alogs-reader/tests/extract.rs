//! Extraction from a made-up install (never game files): the font asset in a shipped bundle, its atlas only in the
//! download cache, found through the package manifest; then GameFont::load reads what extract wrote. Plus the
//! icon extraction's naming helpers.

use alogs_reader::bundles;
use alogs_reader::gamefont::{FONT_NAME, GameFont};
use alogs_unity::V;
use alogs_unity::build::{self, Object, Type, class, prim, string, vector};
use indexmap::IndexMap;
use std::collections::HashSet;
use std::path::PathBuf;

fn obj(fields: Vec<(&str, V)>) -> V {
    V::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn font_type() -> Type {
    let f = |l: u8, n: &str| prim(l, "float", n);
    let glyph = class(
        3,
        "Glyph",
        "data",
        vec![
            prim(4, "unsigned int", "m_Index"),
            class(4, "GlyphMetrics", "m_Metrics", vec![f(5, "m_Width"), f(5, "m_Height"), f(5, "m_HorizontalBearingX"), f(5, "m_HorizontalBearingY"), f(5, "m_HorizontalAdvance")]),
            class(4, "GlyphRect", "m_GlyphRect", vec![prim(5, "int", "m_X"), prim(5, "int", "m_Y"), prim(5, "int", "m_Width"), prim(5, "int", "m_Height")]),
        ],
    );
    let ch = class(3, "TMP_Character", "data", vec![prim(4, "unsigned int", "m_Unicode"), prim(4, "unsigned int", "m_GlyphIndex")]);
    let pptr = class(3, "PPtr<Texture2D>", "data", vec![prim(4, "int", "m_FileID"), prim(4, "SInt64", "m_PathID")]);
    Type {
        class_id: 114,
        nodes: class(
            0,
            "MonoBehaviour",
            "Base",
            vec![
                string(1, "m_Name"),
                class(1, "FaceInfo", "m_FaceInfo", vec![prim(2, "int", "m_PointSize"), f(2, "m_AscentLine"), f(2, "m_DescentLine"), f(2, "m_LineHeight")]),
                vector(1, "m_GlyphTable", false, glyph),
                vector(1, "m_CharacterTable", false, ch),
                vector(1, "m_AtlasTextures", false, pptr),
                prim(1, "int", "m_AtlasPadding"),
            ],
        ),
    }
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

fn glyph(index: i64, rect: [i64; 4], m: [f64; 5]) -> V {
    obj(vec![
        ("m_Index", V::Int(index)),
        ("m_Metrics", obj(vec![("m_Width", V::Float(m[0])), ("m_Height", V::Float(m[1])), ("m_HorizontalBearingX", V::Float(m[2])), ("m_HorizontalBearingY", V::Float(m[3])), ("m_HorizontalAdvance", V::Float(m[4]))])),
        ("m_GlyphRect", obj(vec![("m_X", V::Int(rect[0])), ("m_Y", V::Int(rect[1])), ("m_Width", V::Int(rect[2])), ("m_Height", V::Int(rect[3]))])),
    ])
}

fn font(name: &str, atlas: i64) -> V {
    let ch = |u: i64, g: i64| obj(vec![("m_Unicode", V::Int(u)), ("m_GlyphIndex", V::Int(g))]);
    obj(vec![
        ("m_Name", V::Str(name.into())),
        ("m_FaceInfo", obj(vec![("m_PointSize", V::Int(36)), ("m_AscentLine", V::Float(30.5)), ("m_DescentLine", V::Float(-8.25)), ("m_LineHeight", V::Float(40.0))])),
        ("m_GlyphTable", V::List(vec![glyph(3, [1, 2, 3, 2], [3.0, 2.0, 0.5, 2.0, 4.0]), glyph(7, [5, 0, 2, 3], [2.0, 3.0, 0.0, 3.0, 2.5])])),
        // 'A' -> glyph 3, '?' -> glyph 7, 'B' -> a glyph that isn't there (left out)
        ("m_CharacterTable", V::List(vec![ch(65, 3), ch(63, 7), ch(66, 9)])),
        ("m_AtlasTextures", V::List(vec![obj(vec![("m_FileID", V::Int(2)), ("m_PathID", V::Int(atlas))])])),
        ("m_AtlasPadding", V::Int(1)),
    ])
}

const W: usize = 8;
const H: usize = 4;

fn atlas_alpha() -> Vec<u8> {
    (0..W * H).map(|k| (k * 7 + 3) as u8).collect()
}

/// A made-up install in a temp folder; returns (game folder, Player.log).
fn install(tag: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("alogs-extract-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let game = root.join("Aniimo");
    let pm = game.join(r"Aniimo_Data\cvs\res\lua\LuaScripts\Common\Data");
    std::fs::create_dir_all(&pm).unwrap();
    std::fs::write(pm.join("pmdata.bin"), b"").unwrap();
    // the font asset, shipped (with a texture of the same name that isn't its atlas: another path id)
    let shipped = bundles::shipped_dir(&game);
    std::fs::create_dir_all(&shipped).unwrap();
    let decoy = obj(vec![
        ("m_Name", V::Str("UI_Font_Europe_Atlas".into())),
        ("m_Width", V::Int(4)),
        ("m_Height", V::Int(4)),
        ("m_TextureFormat", V::Int(1)),
        ("image data", V::Bytes(vec![9; 16])),
        ("m_StreamData", obj(vec![("offset", V::Int(0)), ("size", V::Int(0)), ("path", V::Str(String::new()))])),
    ]);
    let file = build::serialized_file(
        &[font_type(), texture_type()],
        &[Object { path_id: 11, type_index: 0, value: font("UI_Font_Other", 99) }, Object { path_id: 12, type_index: 0, value: font(FONT_NAME, -5) }, Object { path_id: 13, type_index: 1, value: decoy }],
    );
    std::fs::write(shipped.join("mtmp_xgui_fonts_exasset_0.uab"), build::bundle(&[("CAB-font", file, 4)], 3, 200)).unwrap();
    // the atlas, only in the cache: its pixels in the .resS
    let mut ress = vec![1, 2, 3];
    ress.extend(atlas_alpha());
    let tex = obj(vec![
        ("m_Name", V::Str("UI_Font_Europe_Atlas".into())),
        ("m_Width", V::Int(W as i64)),
        ("m_Height", V::Int(H as i64)),
        ("m_TextureFormat", V::Int(1)),
        ("image data", V::Bytes(vec![])),
        ("m_StreamData", obj(vec![("offset", V::Int(3)), ("size", V::Int((W * H) as i64)), ("path", V::Str("archive:/CAB-atlas/CAB-atlas.resS".into()))])),
    ]);
    let file = build::serialized_file(&[texture_type()], &[Object { path_id: -5, type_index: 0, value: tex }]);
    let hash = "ab0123456789abcdef0123456789abcd";
    let dir = bundles::cache_dir(&game).join("CacheBundleFiles").join("ab").join(hash);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cdata.uab"), build::bundle(&[("CAB-atlas", file, 4), ("CAB-atlas.resS", ress, 0)], 1, 1 << 16)).unwrap();
    let man = bundles::cache_dir(&game).join("ManifestFiles");
    std::fs::create_dir_all(&man).unwrap();
    let name = b"ar_xgui_font_atlase_0_png_1.uab";
    let mut m = b"OOY\0 some header \x05\x00\x00\x00".to_vec();
    m.push(name.len() as u8);
    m.extend(name);
    m.extend((0..16).map(|k| u8::from_str_radix(&hash[2 * k..2 * k + 2], 16).unwrap()));
    m.extend(b"\x01\x00\x00\x00 and more");
    std::fs::write(man.join("PackageManifest_DefaultPackage_100.bytes"), m).unwrap();
    let log = root.join("Player.log");
    std::fs::write(&log, format!("Mono path[0] = '{}/Aniimo_Data/Managed'\n", game.display())).unwrap();
    (game, log)
}

#[test]
fn font_extracts_from_a_made_up_install() {
    let (game, log) = install("font");
    assert_eq!(bundles::game(Some(&log)).map(|g| g.1.to_string_lossy().to_lowercase()), Some(game.to_string_lossy().to_lowercase()));
    let names = bundles::cached_by_name(&game);
    assert_eq!(names.keys().collect::<Vec<_>>(), ["ar_xgui_font_atlase_0_png_1.uab"]);
    let cache = game.parent().unwrap().join("data").join("gamefont");
    let f = GameFont::load(&cache, Some(&log), FONT_NAME).expect("extracted");
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(cache.join("gamefont.json")).unwrap()).unwrap();
    let want = serde_json::json!({
        "version": 1, "point_size": 36, "ascent": 30.5, "descent": -8.25, "line_height": 40.0, "padding": 1, "atlas_height": H,
        "glyphs": {"65": [1, 2, 3, 2, 0.5, 2.0, 4.0], "63": [5, 0, 2, 3, 0.0, 3.0, 2.5]},
    });
    assert_eq!(meta, want);
    assert_eq!(meta.as_object().unwrap().keys().collect::<Vec<_>>(), want.as_object().unwrap().keys().collect::<Vec<_>>());
    // the atlas: the texture's alpha, the right way up
    let a = atlas_alpha();
    let flipped: Vec<u8> = a.chunks(W).rev().flatten().copied().collect();
    assert_eq!((f.atlas.width, f.atlas.height), (W, H));
    assert_eq!(f.atlas.data, flipped);
    assert_eq!(f.glyphs[&65], [1.0, 2.0, 3.0, 2.0, 0.5, 2.0, 4.0]);
    // another font that isn't in the bundles: nothing written, an error
    assert!(GameFont::load(&cache, Some(&log), "UI_Font_Korea").is_err());
    let _ = std::fs::remove_dir_all(game.parent().unwrap());
}

#[test]
fn no_install_is_an_error() {
    let dir = std::env::temp_dir().join(format!("alogs-extract-test-none-{}", std::process::id()));
    let log = dir.join("Player.log");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&log, "nothing here\n").unwrap();
    // (a real install on this PC would be found through Steam: only check it says why when there's none)
    if alogs_gamedata::gamedata::find_game_data(Some(&log)).is_none() {
        assert_eq!(GameFont::load(&dir.join("gamefont"), Some(&log), FONT_NAME).err().as_deref(), Some("the game install was not found"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn manifest_names_need_their_length_byte_and_a_cached_hash() {
    let h = |b: u8| format!("{:02x}", b).repeat(16);
    let hashes: HashSet<String> = [h(0xaa), h(0xbb)].into();
    let mut d = Vec::new();
    for (name, hash) in [(&b"first_bundle.uab"[..], 0xaa), (b"not_cached.uab", 0xcc), (b"x_y.uab", 0xbb)] {
        d.extend(b"\x10\x00\x00");
        d.push(name.len() as u8);
        d.extend(name);
        d.extend([hash; 16]);
    }
    assert_eq!(bundles::manifest_entries(&d, &hashes), [("first_bundle.uab".to_string(), h(0xaa)), ("x_y.uab".to_string(), h(0xbb))]);
}

#[test]
fn head_names_as_the_python_gives_them() {
    use alogs_reader::icons_extract::head_name;
    let mut by: IndexMap<String, Vec<(String, String)>> = IndexMap::new();
    by.insert("10161".into(), vec![("10161".into(), "Own".into()), ("1016102".into(), "Form".into())]);
    by.insert("10300".into(), vec![("1030001".into(), "A".into()), ("1030002".into(), "B".into()), ("1030003".into(), "B".into())]);
    by.insert("10500".into(), vec![("1050011".into(), "Five".into())]);
    assert_eq!(head_name("10161_female", &by).as_deref(), Some("Own"));
    assert_eq!(head_name("10300", &by).as_deref(), Some("B")); // the most common name
    assert_eq!(head_name("1050003", &by).as_deref(), Some("Five")); // a form's portrait no row names: its species'
    assert_eq!(head_name("99999", &by), None);
}

