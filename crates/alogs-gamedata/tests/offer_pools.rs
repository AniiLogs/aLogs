//! The Special Offerings on offer per Interlink week, from a made-up pmdata.bin built here byte by byte: a week row
//! whose shape shares its values (lists, a number) in another order than its fields, as the game's newer weeks do.
//! Each shared value's offset entry counts back from the entry to the value, as the game reads it.

use alogs_gamedata::gamedata::read_offer_pools;
use alogs_gamedata::pmdata::PmData;

/// A tiny pmdata.bin writer: the string pool, then key sets, classes and objects wherever they're put.
struct Pm {
    d: Vec<u8>,
    strings: Vec<(String, u32)>,
}

impl Pm {
    fn new(mut names: Vec<String>) -> Pm {
        names.sort();
        let mut d = vec![0u8]; // "" at 0
        let mut strings = vec![];
        for n in names {
            strings.push((n.clone(), d.len() as u32));
            d.extend_from_slice(n.as_bytes());
            d.push(0);
        }
        d.extend_from_slice(&[0; 16]); // "" after the last name ends the pool
        Pm { d, strings }
    }
    fn s(&self, name: &str) -> u32 {
        self.strings.iter().find(|(n, _)| n == name).unwrap().1
    }
    fn at(&self) -> u32 {
        self.d.len() as u32
    }
    fn u32(&mut self, v: u32) {
        self.d.extend_from_slice(&v.to_le_bytes());
    }
    fn pad(&mut self) {
        self.d.extend_from_slice(&[0xee; 8]);
    }
    /// A key set of names: its id (4 bytes before its marker).
    fn keys(&mut self, names: &[&str]) -> u32 {
        self.pad();
        let id = self.at();
        self.u32(0);
        self.d.push(1);
        self.u32(names.len() as u32);
        self.u32(0);
        for n in names {
            let o = self.s(n);
            self.u32(o);
        }
        id
    }
    /// A key set of numbers (u16).
    fn nums(&mut self, keys: &[u16]) -> u32 {
        self.pad();
        let id = self.at();
        self.u32(0);
        self.d.push(3);
        self.u32(keys.len() as u32);
        self.u32(keys.len() as u32);
        for k in keys {
            self.d.extend_from_slice(&k.to_le_bytes());
        }
        id
    }
    /// A class: its key set, field types, field offsets, and the bytes of its shared values (put just before the
    /// offsets, as the game does).
    fn class(&mut self, keyset: u32, types: &[u8], offs: &[u8], shared: &[u8]) -> u32 {
        self.pad();
        let types_at = self.at();
        self.d.extend_from_slice(types);
        self.pad();
        self.d.extend_from_slice(shared);
        let offs_at = self.at();
        self.d.push(1);
        self.d.extend_from_slice(offs);
        self.pad();
        let t = self.at();
        self.u32(keyset);
        self.u32(types_at);
        self.u32(offs_at);
        t
    }
    /// An object of class `t` with these 4-byte fields.
    fn obj(&mut self, t: u32, fields: &[u32]) -> u32 {
        self.pad();
        let o = self.at();
        self.u32(t);
        for f in fields {
            self.u32(*f);
        }
        o
    }
    /// An object whose fields are patched in later (pointers to what comes after it).
    fn patch(&mut self, at: u32, v: u32) {
        self.d[at as usize..at as usize + 4].copy_from_slice(&v.to_le_bytes());
    }
}

fn game_data() -> Vec<u8> {
    let fillers: Vec<String> = (0..998).map(|i| format!("zz{i:04}")).collect();
    let mut names: Vec<String> = ["Assist", "Special", "bossLv", "bossrush_buff_data", "bossrush_cycle_data", "buffType", "miniGameBuffRef", "miniGameRight", "seasonId", "specialBuffRef"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    names.extend(fillers.iter().cloned());
    let mut pm = Pm::new(names);

    // the table directory: 1000 names, two of them tables
    let mut dir: Vec<&str> = vec!["bossrush_buff_data", "bossrush_cycle_data"];
    dir.extend(fillers.iter().map(String::as_str));
    let dir_keys = pm.keys(&dir);
    let mut types = vec![8u8, 8];
    types.extend(std::iter::repeat_n(1u8, 998));
    let mut offs = vec![4u8, 8];
    offs.extend(std::iter::repeat_n(4u8, 998));
    let dir_class = pm.class(dir_keys, &types, &offs, &[]);
    let dir_obj = pm.obj(dir_class, &[0, 0]);

    // a list: {1: id}
    let one = pm.nums(&[1]);
    let list_class = pm.class(one, &[3], &[4], &[]);
    let list = |pm: &mut Pm, id: u32| pm.obj(list_class, &[id]);
    let special_list = list(&mut pm, 205);
    let assist_list = list(&mut pm, 501);

    // bossrush_buff_data: 102 and 205 Special, 501 Assist
    let buff_keys = pm.keys(&["buffType"]);
    let buff_row = pm.class(buff_keys, &[7], &[4], &[]);
    let rows: Vec<u32> = ["Special", "Special", "Assist"].iter().map(|t| { let s = pm.s(t); pm.obj(buff_row, &[s]) }).collect();
    let ids = pm.nums(&[102, 205, 501]);
    let buff_table = pm.class(ids, &[8, 8, 8], &[4, 8, 12], &[]);
    let buffs = pm.obj(buff_table, &rows);

    // bossrush_cycle_data: week 7, all three fields shared by the row's class, stored special list, season, mini-game
    // list (field order: mini-game list, season, special list)
    let week_keys = pm.keys(&["miniGameBuffRef", "seasonId", "specialBuffRef"]);
    let mut shared = special_list.to_le_bytes().to_vec();
    shared.push(4);
    shared.extend_from_slice(&assist_list.to_le_bytes());
    // values at 0 (special list), 4 (season), 5 (mini-game list) of the 9 bytes before the offset table; entry i sits
    // at table + 1 + i: miniGameBuffRef 9 - 5 + 1 = 5 back, seasonId 9 - 4 + 2 = 7, specialBuffRef 9 + 3 = 12
    let week_class = pm.class(week_keys, &[0x64 + 8, 0x64 + 1, 0x64 + 8], &[5, 7, 12], &shared);
    let week = pm.obj(week_class, &[]);
    // week 8: two shared numbers stored in the same bytes, as the game does (60 and 10012 as "3c 1c 27")
    let week8_keys = pm.keys(&["bossLv", "miniGameRight"]);
    let week8_class = pm.class(week8_keys, &[0x64 + 1, 0x64 + 3], &[4, 4], &[0x3c, 0x1c, 0x27]);
    let week8 = pm.obj(week8_class, &[]);
    let weeks = pm.nums(&[7, 8]);
    let cycle_table = pm.class(weeks, &[8, 8], &[4, 8], &[]);
    let cycles = pm.obj(cycle_table, &[week, week8]);

    pm.patch(dir_obj + 4, buffs);
    pm.patch(dir_obj + 8, cycles);
    pm.pad();
    pm.d
}

#[test]
fn a_week_s_shared_values_are_read_where_their_entries_point() {
    let bytes = game_data();
    let mut pm = PmData::from_bytes(bytes.clone());
    // each shared value from where its entry says, not in field order (which put the mini-game list in the pick list)
    let row = pm.table("bossrush_cycle_data").unwrap();
    let week = row.as_dict().and_then(|d| d.get(&alogs_pyval::Value::Int(7))).unwrap();
    assert_eq!(week.get("specialBuffRef").repr(), "{1: 205}");
    assert_eq!(week.get("miniGameBuffRef").repr(), "{1: 501}");
    assert_eq!(week.get("seasonId").repr(), "4");
    let week8 = row.as_dict().and_then(|d| d.get(&alogs_pyval::Value::Int(8))).unwrap();
    assert_eq!((week8.get("bossLv").repr(), week8.get("miniGameRight").repr()), ("60".into(), "10012".into()), "values sharing bytes");
    assert_eq!(read_offer_pools(&mut PmData::from_bytes(bytes)).unwrap().repr(), "{'7': [205]}");
}

#[test]
fn no_tables_no_pools() {
    let mut pm = PmData::from_bytes(b"\0a\0b\0c\0".to_vec());
    assert_eq!(read_offer_pools(&mut pm).unwrap().repr(), "{}");
}
