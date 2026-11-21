//! Reading the icon cache (icons.json + PNGs in the data folder) out of the game install: port of `extract` in aLogs
//! B16 `bossbar/icons.py`, with alogs-unity in place of UnityPy. Names come from pmdata.bin, pictures from the
//! sprites in the shipped bundles (by name) and in every bundle of the game's download cache (the newest file wins).
//!
//! One known difference: when an Aniimo's portrait is named by several names equally often, the Python picks
//! whichever its (randomly seeded) set gives first; this takes the first of them in table order.

use crate::bundles;
use crate::icons::{BUFF_SIZE, CACHE_VERSION, HEAD_SIZE, SIM_SIZE};
use alogs_gamedata::gamedata::EnglishText;
use alogs_gamedata::pmdata::PmData;
use alogs_pil::{Filter, Image, Mode};
use alogs_pyval::Value;
use alogs_unity::{CLASS_SPRITE, Env, Obj};
use indexmap::{IndexMap, IndexSet};
use serde_json::{Value as J, json};
use std::path::{Path, PathBuf};

pub const CARD_SIZE: (usize, usize) = (96, 158);
pub const CARD_PREFIX: &str = "ui_img_researchbook_enter_";
pub const BUFF_PREFIXES: &[&str] =
    &["mres_xgui_texture_expng_", "mres_xgui_panel_expng_", "mres_xgui_common_expng_", "ar_xgui_icon_skilli_", "ar_xgui_icon_item_i_"];
pub const HEAD_PREFIXES: &[&str] = &["ar_xgui_image_pet_he_", "ar_xgui_image_petman_"];
pub const FADE_PREFIXES: &[&str] = &["mres_xgui_rpanel_expng_"];
pub const TEAM_FADE: &str = "ui_img_grabeggs_centeritem_mask2";
pub const LEAD_FADE: &str = "ui_img_grabeggs_centeritem_mask1";
const FADES: [(&str, &str); 2] = [("team", TEAM_FADE), ("lead", LEAD_FADE)];

/// `xpt\d+_m(res|exp)_expng_` at the start of a file name.
fn patch_png(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("xpt") else { return false };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && (rest[digits..].starts_with("_mres_expng_") || rest[digits..].starts_with("_mexp_expng_"))
}

fn kind_of(buff_type: &Value) -> Option<&'static str> {
    match buff_type.as_str()? {
        "General" => Some("season"),
        "Special" => Some("offering"),
        _ => None,
    }
}

/// `(r.get('buffIcon') or '').lstrip('$').rsplit('.', 1)[0]`
fn icon_of(v: &Value) -> String {
    let s = if v.truthy() { v.as_str().map(str::to_string).unwrap_or_else(|| v.py_str()) } else { String::new() };
    let s = s.trim_start_matches('$');
    s.rsplit_once('.').map_or(s, |(a, _)| a).to_string()
}

fn table(pm: &mut PmData, name: &str) -> Result<Vec<(Value, Value)>, String> {
    let t = pm.table(name).map_err(|e| e.to_string())?;
    Ok(t.as_dict().map(|d| d.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default())
}

fn mtime(p: &Path) -> f64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_secs_f64())
}

/// icons.py `whole`: a sprite as the game draws it, its drawn part put back where it sits in the whole rect.
fn whole(sprite: &alogs_unity::V, img: Image) -> Image {
    let img = img.convert(Mode::Rgba);
    let rect = sprite.get("m_Rect");
    let off = sprite.get("m_RD").get("textureRectOffset");
    let (Some(rw), Some(rh), Some(ox), Some(oy)) = (rect.get("width").as_f64(), rect.get("height").as_f64(), off.get("x").as_f64(), off.get("y").as_f64()) else {
        return img;
    };
    let (w, h) = (rw.round_ties_even() as i64, rh.round_ties_even() as i64);
    let (iw, ih) = (img.width as i64, img.height as i64);
    if (w, h) == (iw, ih) || w < iw || h < ih {
        return img;
    }
    let mut out = Image::new(Mode::Rgba, w as usize, h as usize, &[0, 0, 0, 0]);
    out.paste(&img, ox.round_ties_even() as i64, h - oy.round_ties_even() as i64 - ih);
    out
}

/// icons.py `head_name`: the Aniimo a head portrait (or card art) shows, from its key after the sprite name's prefix.
/// `by_icon`: icon -> [(pet id, name)].
pub fn head_name(key: &str, by_icon: &IndexMap<String, Vec<(String, String)>>) -> Option<String> {
    let icon = key.split('_').next().unwrap_or("");
    let digits = icon.chars().count() == 7 && icon.chars().all(|c| c.is_ascii_digit());
    let keys: Vec<&str> = if digits { vec![icon, &icon[..5]] } else { vec![icon] };
    for k in keys {
        let rows = by_icon.get(k).map(Vec::as_slice).unwrap_or(&[]);
        let k00 = format!("{k}00");
        if let Some((_, n)) = rows.iter().find(|(p, _)| p == k || *p == k00) {
            return Some(n.clone());
        }
        let mut best: Option<(&str, usize)> = None;
        for (_, n) in rows {
            let c = rows.iter().filter(|(_, m)| m == n).count();
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((n, c));
            }
        }
        if let Some((n, _)) = best {
            return Some(n.to_string());
        }
    }
    None
}

fn save(img: &Image, p: PathBuf) -> Result<(), String> {
    img.save_png(&p).map_err(|e| format!("{}: {e}", p.display()))
}

/// Writes `<cache_dir>/heads/*.png`, `cards/*.png`, `buffs/*.png`, `sim/*.png`, the fades and icons.json from the
/// game install. Ok(true) when it found heads and buffs; Ok(false) when the game isn't found or has neither.
pub fn extract(cache_dir: &Path, player_log: Option<&Path>) -> Result<bool, String> {
    let Some((pm_path, game)) = bundles::game(player_log) else { return Ok(false) };
    let mut pm = PmData::open(&pm_path).map_err(|e| e.to_string())?;
    let text = EnglishText::open(&pm_path).map_err(|e| e.to_string())?;
    let en = |v: &Value| text.en(v).map_err(|e| e.to_string());

    let buff_rows: Vec<(Value, Value)> = table(&mut pm, "bossrush_buff_data")?.into_iter().filter(|(_, r)| kind_of(r.get("buffType")).is_some()).collect();
    let mut wanted: IndexSet<String> = buff_rows.iter().map(|(_, r)| icon_of(r.get("buffIcon")).to_lowercase()).collect();
    let buff_icons = wanted.clone();
    // Holo-Battle Sim: per series its name, the series icon, its Core gizmo and the Alpha Transformation it unlocks
    let mut sim: IndexMap<String, IndexMap<&str, String>> = IndexMap::new();
    for (sid, r) in table(&mut pm, "random_buff_series_name")? {
        let mut s = IndexMap::new();
        s.insert("name", en(r.get("buffSeriesName"))?);
        s.insert("core", en(r.get("coreBuffName"))?);
        s.insert("series_icon", icon_of(r.get("buffSeriesIcon")).to_lowercase());
        s.insert("core_icon", icon_of(r.get("coreBuffIcon")).to_lowercase());
        sim.insert(sid.py_str(), s);
    }
    for (sid, r) in table(&mut pm, "rogue_transform_data")? {
        if let Some(s) = sim.get_mut(&sid.py_str()) {
            s.insert("transform", en(r.get("transformName"))?);
            s.insert("transform_icon", icon_of(r.get("transformIcon")).to_lowercase());
        }
    }
    for s in sim.values() {
        for (k, v) in s {
            if k.ends_with("_icon") && !v.is_empty() {
                wanted.insert(v.clone());
            }
        }
    }
    // every Holo-gizmo by name: series id and rarity
    let mut sim_gizmos = serde_json::Map::new();
    for (_, r) in table(&mut pm, "random_buff_data")? {
        if !matches!(r, Value::Dict(_)) {
            continue;
        }
        let name = en(r.get("buffName"))?;
        if !name.is_empty() && !r.get("buffSeries").is_none() && !sim_gizmos.contains_key(&name) {
            let rarity = match r.get("buffRarity").as_f64() {
                Some(10.0) => "Normal",
                Some(5.0) => "Uncommon",
                Some(2.0) => "Core",
                _ => "Special",
            };
            sim_gizmos.insert(name, json!([r.get("buffSeries").py_str(), rarity]));
        }
    }

    let shipped: Vec<PathBuf> = bundles::shipped(&game)
        .into_iter()
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            BUFF_PREFIXES.iter().chain(HEAD_PREFIXES).chain(FADE_PREFIXES).any(|pre| n.starts_with(pre)) || patch_png(&n)
        })
        .collect();
    let patches = bundles::cached(&game);
    // lower name -> (picture, file time); the newest file wins
    let mut art: IndexMap<String, (Image, f64)> = IndexMap::new();
    for f in shipped.iter().chain(&patches) {
        let Ok(env) = Env::load(&[f]) else { continue };
        let stamp = mtime(f);
        let sprites: Vec<Obj> = env.objects().filter(|&o| env.class_id(o) == CLASS_SPRITE).collect();
        for o in sprites {
            let low = env.peek_name(o).unwrap_or_default().to_lowercase();
            let fade = low == TEAM_FADE || low == LEAD_FADE;
            let card = low.starts_with(CARD_PREFIX);
            let head = low.starts_with("ui_pethead_");
            if !(wanted.contains(&low) || fade || card || head) || art.get(&low).is_some_and(|a| a.1 >= stamp) {
                continue;
            }
            let Ok(s) = env.read(o) else { continue };
            let Ok(img) = env.sprite_from(o, &s) else { continue };
            let img = if head || fade || card || buff_icons.contains(&low) { whole(&s, img) } else { img.convert(Mode::Rgba) };
            art.insert(low, (img, stamp));
        }
    }
    for sub in ["heads", "buffs", "cards"] {
        std::fs::create_dir_all(cache_dir.join(sub)).map_err(|e| e.to_string())?;
    }
    // heads: named by the game's own tables only
    let mut forms: IndexMap<String, (String, String)> = IndexMap::new();
    for t in ["pet_prototype_data", "pet_data"] {
        for (pid, r) in table(&mut pm, t)? {
            if !matches!(r, Value::Dict(_)) || !r.get("iconName").truthy() {
                continue;
            }
            let name = en(r.get("name"))?;
            if name.is_empty() {
                continue;
            }
            let icon = r.get("iconName").py_str().split('_').next().unwrap_or("").to_string();
            forms.entry(pid.py_str()).or_insert((icon, name));
        }
    }
    let mut rows: Vec<(&String, &(String, String))> = forms.iter().collect();
    let num = |k: &str| if !k.is_empty() && k.chars().all(|c| c.is_ascii_digit()) { k.parse::<i128>().unwrap_or(0) } else { 0 };
    rows.sort_by_key(|(k, _)| num(k));
    let mut by_icon: IndexMap<String, Vec<(String, String)>> = IndexMap::new();
    for (pid, (icon, name)) in rows {
        by_icon.entry(icon.clone()).or_default().push((pid.clone(), name.clone()));
    }
    let mut heads = serde_json::Map::new();
    for (low, (img, _)) in &art {
        let Some(key) = low.strip_prefix("ui_pethead_") else { continue };
        let Some(name) = head_name(key, &by_icon) else { continue };
        let side = img.width.max(img.height);
        let mut sq = Image::new(Mode::Rgba, side, side, &[0, 0, 0, 0]);
        sq.paste(img, ((side - img.width) / 2) as i64, (side - img.height) as i64);
        save(&sq.resize(HEAD_SIZE, HEAD_SIZE, Some(Filter::Lanczos)), cache_dir.join("heads").join(format!("{key}.png")))?;
        heads.insert(key.to_string(), J::from(name));
    }
    let mut cards = serde_json::Map::new();
    for (low, (img, _)) in &art {
        let Some(key) = low.strip_prefix(CARD_PREFIX) else { continue };
        let Some(name) = head_name(key, &by_icon) else { continue };
        save(&img.resize(CARD_SIZE.0, CARD_SIZE.1, Some(Filter::Lanczos)), cache_dir.join("cards").join(format!("{key}.png")))?;
        cards.insert(key.to_string(), J::from(name));
    }
    let mut buffs = serde_json::Map::new();
    for (k, r) in &buff_rows {
        let low = icon_of(r.get("buffIcon")).to_lowercase();
        let Some((img, _)) = art.get(&low) else { continue };
        save(&img.resize(BUFF_SIZE, BUFF_SIZE, Some(Filter::Lanczos)), cache_dir.join("buffs").join(format!("{low}.png")))?;
        let mut b = serde_json::Map::new();
        b.insert("name".into(), J::from(en(r.get("buffName"))?));
        b.insert("kind".into(), J::from(kind_of(r.get("buffType")).unwrap_or("")));
        b.insert("icon".into(), J::from(low.clone()));
        let lv = r.get("Bufflevel");
        if lv.truthy() {
            let n = lv.as_i64().or_else(|| lv.as_f64().map(|f| f.trunc() as i64)).or_else(|| lv.as_str().and_then(|s| s.trim().parse().ok()));
            b.insert("level".into(), n.map_or(J::Null, J::from));
        }
        buffs.insert(k.py_str(), J::Object(b));
    }
    for (kind, low) in FADES {
        if let Some((img, _)) = art.get(low) {
            save(&img.split().swap_remove(3), cache_dir.join(format!("{kind}_fade.png")))?;
        }
    }
    std::fs::create_dir_all(cache_dir.join("sim")).map_err(|e| e.to_string())?;
    let mut sim_out = serde_json::Map::new();
    for (sid, s) in &sim {
        let mut entry = serde_json::Map::new();
        for k in ["name", "core", "transform"] {
            if let Some(v) = s.get(k).filter(|v| !v.is_empty()) {
                entry.insert(k.into(), J::from(v.clone()));
            }
        }
        for part in ["series", "core", "transform"] {
            let Some((img, _)) = s.get(format!("{part}_icon").as_str()).and_then(|low| art.get(low)) else { continue };
            let mut img = img.clone();
            if part == "transform" {
                // the head end of a tall portrait, as the round frame shows it
                img = img.crop_i(0, 0, img.width as i64, img.width.min(img.height) as i64);
            }
            save(&img.thumbnail_filter(SIM_SIZE, SIM_SIZE, Filter::Lanczos), cache_dir.join("sim").join(format!("{part}_{sid}.png")))?;
            entry.insert(format!("{part}_art"), J::Bool(true));
        }
        sim_out.insert(sid.clone(), J::Object(entry));
    }
    // the Special Offerings on offer in each Interlink week: tells apart offerings that share an icon
    let offer_pools: J = alogs_gamedata::gamedata::read_offer_pools(&mut pm).ok().and_then(|v| serde_json::from_str(&v.json_dumps()).ok()).unwrap_or_else(|| json!({}));
    let ok = !heads.is_empty() && !buffs.is_empty();
    let meta = json!({
        "version": CACHE_VERSION, "build": bundles::build(&game), "heads": heads, "cards": cards, "buffs": buffs, "sim": sim_out,
        "team_fade": art.contains_key(TEAM_FADE), "lead_fade": art.contains_key(LEAD_FADE), "sim_gizmos": sim_gizmos,
        "offer_pools": offer_pools,
    });
    std::fs::write(cache_dir.join("icons.json"), meta.to_string()).map_err(|e| e.to_string())?;
    Ok(ok)
}
