//! The Lost Sanctum layouts (Assets/layouts.json, read from Sanctum Helper's install: the map art is the game's and is
//! never copied here) and the chest sets a run can still have. Port of `LayoutDb.cs` and `ChestSets.cs`.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Door {
    pub world: Vec<f64>,
    pub img: Vec<f64>,
    pub color: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Room {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind_no: i64,
    pub kind: String,
    pub prefab: String,
    pub size: Option<i64>,
    #[serde(rename = "imgRect")]
    pub img_rect: Option<Vec<f64>>,
    pub door: Option<Door>,
    pub world: Vec<f64>,
    pub img: Vec<f64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Marker {
    pub kind: String,
    pub name: String,
    pub room: i64,
    pub group: String,
    pub world: Vec<f64>,
    pub img: Vec<f64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct WorldToImage {
    pub ox: f64,
    pub oy: f64,
    pub sx: f64,
    pub sz: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct DoorVector {
    pub angle: f64,
    pub distance: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub scene: i64,
    #[serde(rename = "layout")]
    pub number: i64,
    pub kind: String,
    #[serde(rename = "miniMapImageScale")]
    pub mini_map_image_scale: Option<Vec<f64>>,
    pub difficulties: Vec<String>,
    pub image: String,
    pub layers: Vec<String>,
    #[serde(rename = "imageSize")]
    pub image_size: Vec<i64>,
    #[serde(rename = "worldToImage")]
    pub world_to_image: WorldToImage,
    #[serde(rename = "doorVector")]
    pub door_vector: DoorVector,
    pub rooms: Vec<Room>,
    pub markers: Vec<Marker>,
    #[serde(rename = "upperRects")]
    pub upper_rects: Vec<Vec<f64>>,
}

impl Layout {
    pub fn is_island(&self) -> bool {
        self.kind == "island"
    }

    /// Could a player at this map-art spot be on the upper floor? Always yes without rooms for it.
    pub fn may_be_upstairs(&self, x: f64, y: f64, margin: f64) -> bool {
        self.upper_rects.is_empty() || self.upper_rects.iter().any(|r| x >= r[0] - margin && x <= r[2] + margin && y >= r[1] - margin && y <= r[3] + margin)
    }

    pub fn orange(&self) -> Option<&Room> {
        self.rooms.iter().find(|r| r.kind == "orange")
    }

    pub fn blue(&self) -> Option<&Room> {
        self.rooms.iter().find(|r| r.kind == "blue")
    }

    pub fn has_doors(&self) -> bool {
        self.orange().is_some() && self.blue().is_some()
    }

    /// Map-art pixels per metre.
    pub fn px_per_metre(&self) -> f64 {
        self.world_to_image.sx.abs()
    }

    pub fn key_rooms(&self) -> impl Iterator<Item = &Room> {
        self.rooms.iter().filter(|r| r.kind == "key")
    }

    /// The boss rooms (Level_Room_30_BOSS: the builder tags them "boss"; they reuse the start door's roomType 3).
    pub fn boss_rooms(&self) -> impl Iterator<Item = &Room> {
        self.rooms.iter().filter(|r| r.kind == "boss")
    }

    /// Is this a room whose Legendary chest is always there (a key room, or a boss room: its one spawner of it has no
    /// alternative), not only a chance?
    pub fn sure_room(&self, room: i64) -> bool {
        self.rooms.iter().any(|r| r.id == room && (r.kind == "key" || r.kind == "boss"))
    }

    /// A room's square on the map art (x0, y0, x1, y1): its `imgRect`, or from the side its prefab names
    /// (Level_Room_30_BOSS is 30 m) around its centre. None when neither is known.
    pub fn room_rect(&self, r: &Room) -> Option<[f64; 4]> {
        if let Some(q) = r.img_rect.as_ref().filter(|q| q.len() == 4) {
            return Some([q[0], q[1], q[2], q[3]]);
        }
        let side = r.size.or_else(|| r.prefab.trim_start_matches('$').strip_prefix("Level_Room_")?.split('_').next()?.parse().ok())?;
        let (w, h) = (&self.world_to_image, side as f64 / 2.0);
        if r.world.len() < 2 || w.sx == 0.0 || side <= 0 {
            return None;
        }
        let (x, z) = (r.world[0], r.world[1]);
        Some([w.ox + (x - h) * w.sx, w.oy - (z + h) * w.sz, w.ox + (x + h) * w.sx, w.oy - (z - h) * w.sz])
    }

    pub fn nest_count(&self) -> usize {
        self.markers.iter().filter(|m| m.kind == "nest").count()
    }

    pub fn title(&self) -> String {
        if self.is_island() { "Egg Heist island".into() } else { format!("Layout {}", self.number) }
    }

    /// The name shown on maps ("Sanctum 53", "Egg Heist island"), in the app's language.
    pub fn display_title(&self) -> String {
        if self.is_island() { crate::lang::t("Egg Heist island") } else { crate::lang::f("Sanctum {0}", &[&self.number]) }
    }

    pub fn compass(&self) -> &'static str {
        const D: [&str; 8] = ["north", "north-east", "east", "south-east", "south", "south-west", "west", "north-west"];
        D[(((self.door_vector.angle + 22.5) / 45.0).floor() as i64).rem_euclid(8) as usize]
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct LayoutDb {
    #[serde(rename = "gameDataVersion")]
    pub game_data_version: String,
    pub difficulties: Vec<String>,
    pub grades: HashMap<String, HashMap<String, Vec<i64>>>,
    pub layouts: Vec<Layout>,
    #[serde(skip)]
    pub root: PathBuf,
}

impl LayoutDb {
    pub fn load(assets: &Path) -> Result<LayoutDb, String> {
        let text = std::fs::read_to_string(assets.join("layouts.json")).map_err(|e| e.to_string())?;
        let mut db: LayoutDb = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        db.root = assets.to_path_buf();
        Ok(db)
    }

    pub fn image_path(&self, l: &Layout) -> PathBuf {
        self.root.join(l.image.replace('/', std::path::MAIN_SEPARATOR_STR))
    }

    /// Floor 0 = the ground floor image, 1.. = upper floors (same coordinates).
    pub fn floor_path(&self, l: &Layout, floor: usize) -> PathBuf {
        if floor == 0 { self.image_path(l) } else { self.root.join(l.layers[floor - 1].replace('/', std::path::MAIN_SEPARATOR_STR)) }
    }

    pub fn floor_count(l: &Layout) -> usize {
        1 + l.layers.len()
    }

    /// Layouts the game can draw for a difficulty (all team-mode layouts when unknown).
    pub fn candidates(&self, difficulty: Option<&str>) -> Vec<&Layout> {
        self.layouts.iter().filter(|l| !l.difficulties.is_empty() && difficulty.is_none_or(|d| l.difficulties.iter().any(|x| x == d))).collect()
    }

    pub fn by_number(&self, n: i64) -> Option<&Layout> {
        self.layouts.iter().find(|l| l.number == n)
    }

    pub fn island(&self) -> Option<&Layout> {
        self.layouts.iter().find(|l| l.is_island())
    }

    /// The Sanctum layout of a game scene (200NN is Layout NN), as the game's log names it while loading it.
    pub fn by_scene(&self, scene: i64) -> Option<&Layout> {
        self.layouts.iter().find(|l| l.scene == scene && !l.is_island())
    }
}

/// How far a chest seen may be from a set's chest and still be it, in metres.
pub const REACH: f64 = 1.5;
const TIERS: [&str; 4] = ["simple", "exquisite", "luxurious", "legendary"];

/// The grade in a marker's group ("G4宝箱" is 4), or 0.
pub fn grade(group: &str) -> i64 {
    let b = group.as_bytes();
    for i in 0..b.len().saturating_sub(1) {
        if b[i] == b'G' && (b'1'..=b'6').contains(&b[i + 1]) {
            return i64::from(b[i + 1] - b'0');
        }
    }
    0
}

pub fn is_chest(m: &Marker) -> bool {
    TIERS.contains(&m.kind.as_str())
}

/// A layout's graded sets by room: room, then grade, then that set's chests.
pub fn sets_of(l: &Layout) -> BTreeMap<i64, BTreeMap<i64, Vec<Marker>>> {
    let mut rooms: BTreeMap<i64, BTreeMap<i64, Vec<Marker>>> = BTreeMap::new();
    for m in &l.markers {
        let g = grade(&m.group);
        if g == 0 || !is_chest(m) {
            continue;
        }
        rooms.entry(m.room).or_default().entry(g).or_default().push(m.clone());
    }
    rooms
}

/// The grades a difficulty hands this room, of the ones it has sets for; all of them when it isn't known.
pub fn allowed(grades: &HashMap<String, HashMap<String, Vec<i64>>>, has: impl IntoIterator<Item = i64>, difficulty: Option<&str>) -> Vec<i64> {
    let mut all: Vec<i64> = has.into_iter().collect();
    all.sort();
    let Some(diff) = difficulty else { return all };
    if all.is_empty() {
        return all;
    }
    let kind = if *all.last().unwrap() <= 3 { "hallway" } else { "room" };
    let mut handed = BTreeSet::new();
    for name in diff.split('/').map(str::trim).filter(|s| !s.is_empty()) {
        let Some(g) = grades.get(name).and_then(|by| by.get(kind)) else { return all };
        handed.extend(g.iter().copied());
    }
    let left: Vec<i64> = all.iter().copied().filter(|g| handed.contains(g)).collect();
    if left.is_empty() { all } else { left }
}

/// One run of one layout: what is still possible in each room.
pub struct Run {
    pub layout: i64,
    pub difficulty: Option<String>,
    pub version: i64,
    sets: BTreeMap<i64, BTreeMap<i64, Vec<Marker>>>,
    possible: BTreeMap<i64, Vec<i64>>,
    seen: BTreeSet<i64>,
}

fn dist(a: &[f64], x: f64, z: f64) -> f64 {
    ((a[0] - x).powi(2) + (a[1] - z).powi(2)).sqrt()
}

impl Run {
    pub fn new(grades: &HashMap<String, HashMap<String, Vec<i64>>>, l: &Layout, difficulty: Option<&str>) -> Run {
        let sets = sets_of(l);
        let possible = sets.iter().map(|(room, by)| (*room, allowed(grades, by.keys().copied(), difficulty))).collect();
        Run { layout: l.number, difficulty: difficulty.map(String::from), version: 0, sets, possible, seen: BTreeSet::new() }
    }

    pub fn possible(&self, room: i64) -> Vec<i64> {
        self.possible.get(&room).cloned().unwrap_or_default()
    }

    pub fn confirmed(&self, room: i64) -> bool {
        self.seen.contains(&room) && self.possible.get(&room).is_some_and(|p| p.len() == 1)
    }

    /// Should this marker be drawn? Yes unless it belongs to a graded set that is ruled out.
    pub fn shows(&self, m: &Marker) -> bool {
        let g = grade(&m.group);
        g == 0 || self.possible.get(&m.room).is_none_or(|p| p.contains(&g))
    }

    /// A chest was seen at a spot (world x, z): in each room with a possible set that has one there, the others go.
    pub fn saw(&mut self, x: f64, z: f64, tier: Option<&str>) -> bool {
        let mut any = false;
        for (room, by) in &self.sets {
            let p = &self.possible[room];
            let fit: Vec<i64> = p.iter().copied().filter(|g| by[g].iter().any(|c| tier.is_none_or(|t| c.kind == t) && dist(&c.world, x, z) <= REACH)).collect();
            if fit.is_empty() {
                continue;
            }
            any = true;
            self.seen.insert(*room);
            if fit.len() < p.len() {
                self.possible.insert(*room, fit);
            }
            self.version += 1;
        }
        any
    }

    /// A chest's prompt was up with the player at this picture spot: the nearest possible chest within `within` m,
    /// unless another spot is about as near.
    pub fn saw_near(&mut self, img_x: f64, img_y: f64, px_per_metre: f64, others: &[Marker], within: f64, clear: f64) -> Option<Marker> {
        let ppm = px_per_metre.max(1e-6);
        let away = |c: &Marker| ((c.img[0] - img_x).powi(2) + (c.img[1] - img_y).powi(2)).sqrt() / ppm;
        let apart = |a: &Marker, b: &Marker| dist(&a.world, b.world[0], b.world[1]);
        let mut near: Vec<(Marker, f64)> = vec![];
        for (room, by) in &self.sets {
            for g in &self.possible[room] {
                for c in &by[g] {
                    let d = away(c);
                    if d <= within + clear {
                        near.push((c.clone(), d));
                    }
                }
            }
        }
        let (best, best_d) = near.iter().fold(None::<&(Marker, f64)>, |b, n| if b.is_none_or(|b| n.1 < b.1) { Some(n) } else { b }).cloned()?;
        if best_d > within || near.iter().any(|n| n.1 < best_d + clear && apart(&n.0, &best) > REACH) || others.iter().any(|o| away(o) < best_d + clear && apart(o, &best) > REACH) {
            return None;
        }
        self.saw(best.world[0], best.world[1], None).then_some(best)
    }
}
