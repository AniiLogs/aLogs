//! A Special Offering read from an icon several offerings share: the week's pick list (icons.json `offer_pools`, from
//! the game's own data) leaves the one on offer, sent with its id. Made-up ids, names and icons.

use alogs_meter::party_icons::{OfferingRead, offering_of, offering_value};
use alogs_pil::{Image, Mode};
use alogs_reader::icons::{CACHE_VERSION, Icons};

fn cache(tag: &str) -> Icons {
    let dir = std::env::temp_dir().join(format!("alogs-offerings-test-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("buffs")).unwrap();
    for icon in ["shared_a", "shared_b", "own"] {
        Image::new(Mode::Rgba, 8, 8, &[255, 255, 255, 255]).save_png(&dir.join("buffs").join(format!("{icon}.png"))).unwrap();
    }
    let b = |name: &str, icon: &str| serde_json::json!({ "name": name, "kind": "offering", "icon": icon });
    let meta = serde_json::json!({
        "version": CACHE_VERSION, "heads": {},
        "buffs": {
            "11": b("Spark", "shared_a"), "21": b("Ember", "shared_a"),
            "12": b("Echo", "shared_b"), "13": b("Echo", "shared_b"), "22": b("Drain", "shared_b"),
            "14": b("Lone", "own"),
        },
        "offer_pools": { "7": [14, 13, 21], "8": [11, 12, 22], "9": [14] },
    });
    std::fs::write(dir.join("icons.json"), meta.to_string()).unwrap();
    let icons = Icons::open(&dir).expect("the test's icon cache");
    let _ = std::fs::remove_dir_all(&dir);
    icons
}

fn read(ic: &Icons, icon: &str, week: Option<&str>) -> String {
    match offering_of(ic, icon, week) {
        OfferingRead::Some(v) => v.repr(),
        other => format!("{other:?}"),
    }
}

#[test]
fn the_week_s_pick_list_tells_shared_icons_apart() {
    let ic = cache("pools");
    assert_eq!(read(&ic, "shared_a", Some("7")), "{'id': 21, 'name': 'Ember'}");
    assert_eq!(read(&ic, "shared_a", Some("8")), "{'id': 11, 'name': 'Spark'}");
    // two offerings of one name (a newer version of the same offering): the week says which id
    assert_eq!(read(&ic, "shared_b", Some("7")), "{'id': 13, 'name': 'Echo'}");
    // a week that offers two of an icon's offerings can't decide: joined, no id
    assert_eq!(read(&ic, "shared_b", Some("8")), "{'name': 'Drain or Echo'}");
    assert_eq!(read(&ic, "own", Some("7")), "{'id': 14, 'name': 'Lone'}");
}

/// The week's list is known and holds none of the matched icon's offerings: the icon was misread (the game offers
/// only what is on its list), so the look is no read and the looks after it decide.
#[test]
fn an_icon_off_the_week_s_list_is_no_read() {
    let ic = cache("off-list");
    assert_eq!(offering_of(&ic, "shared_a", Some("9")), OfferingRead::OffList("shared_a".into()));
    assert!(offering_of(&ic, "shared_a", Some("9")).value().is_none());
    assert_eq!(offering_of(&ic, "own", Some("8")), OfferingRead::OffList("own".into()));
    // no list for the week: as before
    assert_eq!(read(&ic, "own", Some("99")), "{'id': 14, 'name': 'Lone'}");
}

#[test]
fn without_the_week_s_list_nothing_is_guessed() {
    let ic = cache("none");
    // the week isn't known, or the game data has no list for it: as before, joined
    assert_eq!(read(&ic, "shared_a", None), "{'name': 'Ember or Spark'}");
    assert_eq!(read(&ic, "shared_a", Some("99")), "{'name': 'Ember or Spark'}");
    // one name, two ids: the name, but no id picked at random
    assert_eq!(read(&ic, "shared_b", Some("99")), "{'name': 'Drain or Echo'}");
    assert_eq!(offering_value(&ic, &["12".to_string(), "13".to_string()]).repr(), "{'name': 'Echo'}");
    assert_eq!(read(&ic, "own", None), "{'id': 14, 'name': 'Lone'}");
}
