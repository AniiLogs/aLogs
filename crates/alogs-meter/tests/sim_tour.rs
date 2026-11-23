//! The Gizmo tour's text helpers against the Python (tools/gen_simtour_goldens.py).

use alogs_meter::sim_tour::{sim_count, sim_names, sim_stitch};
use serde_json::Value as J;

fn strs(v: &J) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

#[test]
fn tour_helpers_match_python() {
    let g: J = serde_json::from_str(include_str!("data/simtour_goldens.json")).unwrap();
    let gizmos: Vec<(String, String)> = g["gizmos"].as_object().unwrap().iter().map(|(n, v)| (n.clone(), v[0].as_str().unwrap().to_string())).collect();
    for c in g["names"].as_array().unwrap() {
        assert_eq!(sim_names(c[0].as_str().unwrap(), &gizmos, c[1].as_str()), strs(&c[2]), "sim_names {c}");
    }
    for c in g["stitch"].as_array().unwrap() {
        let (rows, view) = (strs(&c[0]), strs(&c[1]));
        assert_eq!(sim_stitch(&rows, &view), strs(&c[2]), "sim_stitch {c}");
        assert_eq!(sim_count(&rows), c[3].as_i64().unwrap(), "sim_count {c}");
    }
}

/// A view with one name not read (a row cut at the list's edge read as something else, or one the OCR missed) lines up
/// with the list so far instead of being added whole (22 Nov: 38 picks of 32, rows taken twice or three times).
#[test]
fn a_view_missing_one_name_still_lines_up() {
    let v = |s: &str| -> Vec<String> { s.split(',').filter(|x| !x.is_empty()).map(String::from).collect() };
    let list = v("Energizing Bracer α,Reflective Barrier α,Flamewreath Armor,Lossless Power Supply,Blessing of Protection,Flawless Defense,Resolute Defense,Charging Shield");
    let cases = [
        // a name the view missed inside the overlap: kept once
        ("Energizing Bracer α,Reflective Barrier α,Flamewreath Armor,Lossless Power Supply,Blessing of Protection",
         "Reflective Barrier α,Lossless Power Supply,Blessing of Protection,Flawless Defense"),
        // a name the views before missed: it goes in its place
        ("Energizing Bracer α,Reflective Barrier α,Flamewreath Armor,Blessing of Protection",
         "Reflective Barrier α,Flamewreath Armor,Lossless Power Supply,Blessing of Protection,Flawless Defense"),
        // the cut first row read as another name
        ("Energizing Bracer α,Reflective Barrier α,Flamewreath Armor,Lossless Power Supply,Blessing of Protection",
         "Charging Shield,Flamewreath Armor,Lossless Power Supply,Blessing of Protection,Flawless Defense"),
        // the list's last name read so far not in the view (cut at its bottom before, at its top now)
        ("Energizing Bracer α,Reflective Barrier α,Flamewreath Armor,Lossless Power Supply,Blessing of Protection,Flawless Defense",
         "Flamewreath Armor,Lossless Power Supply,Blessing of Protection,Resolute Defense,Charging Shield"),
    ];
    for (rows, view) in cases {
        let got = sim_stitch(&v(rows), &v(view));
        assert_eq!(got, list[..got.len()].to_vec(), "{rows} + {view}");
        assert!(got.len() >= 6, "{got:?}");
    }
    // the plain cases as before: an exact overlap, a view that adds nothing, nothing in common
    assert_eq!(sim_stitch(&list[..5], &list[3..8]), list);
    assert_eq!(sim_stitch(&list, &list[4..8]), list);
    assert_eq!(sim_stitch(&v("A,B"), &v("C,D")), v("A,B,C,D"));
    assert_eq!(sim_stitch(&v("A,B"), &v("B,C")), v("A,B,C"));
    // a real repeat (two of the same gizmo) at the seam is kept
    assert_eq!(sim_stitch(&v("A,B,C"), &v("B,C,C,D")), v("A,B,C,C,D"));
}

/// More picks read for a series than its badge says is a misread: the series is left unread. Equal or fewer is a read.
#[test]
fn a_series_read_over_its_badge_is_unread() {
    use alogs_meter::sim_tour::series_overread;
    let r = |s: &str| -> Vec<String> { s.split(',').map(String::from).collect() };
    let defense = r("Reflective Barrier α,Reflective Barrier α,Flawless Defense,Flawless Defense,Resolute Defense,Charging Shield,Refined Steel");
    assert_eq!(sim_count(&defense), 9);
    assert!(series_overread(&defense, Some(6), false));
    assert!(!series_overread(&r("Reflective Barrier α,Flawless Defense,Resolute Defense,Charging Shield"), Some(6), false));
    assert!(!series_overread(&r("Reflective Barrier α,Flawless Defense"), Some(6), true), "short of its badge: read, whatever All says");
    assert!(series_overread(&defense, None, true) && !series_overread(&defense, None, false));
}

/// The Victory screen's prompt in either mode lets the tour click at once; anything else is waited for a moment.
#[test]
fn the_victory_screens_prompt_in_either_mode() {
    use alogs_meter::sim_tour::victory_mode;
    for (prompt, mode) in [
        ("click anywhere to close", Some("mouse mode")),
        ("clickanywheretoclose", Some("mouse mode")),
        ("anywhere to", Some("mouse mode")),
        ("confirm", Some("controller mode")),
        ("a confirm", Some("controller mode")),
        ("", None),
        ("victory", None),
    ] {
        assert_eq!(victory_mode(prompt), mode, "{prompt:?}");
    }
}

/// "All" is clicked, and its count read, where the row is with a mouse and where it is with a controller (the filter
/// column sits lower then): its middle at 0.161 and 0.181 of the height, the rows 0.078 apart, the text 0.035 tall.
#[test]
fn all_is_reached_in_both_layouts_of_the_filter_column() {
    use alogs_meter::sim_tour::{GIZMO_ALL, GIZMO_ALL_COUNT};
    for middle in [0.161, 0.181] {
        assert!((GIZMO_ALL.1 - middle).abs() < 0.078 / 2.0 - 0.015, "the click at {} is well inside the row at {middle}", GIZMO_ALL.1);
        assert!(GIZMO_ALL_COUNT.1 < middle - 0.0175 && GIZMO_ALL_COUNT.3 > middle + 0.0175, "the count's box holds the text at {middle}");
        // ...and none of the next row's ("Energy 5", 0.078 lower)
        assert!(GIZMO_ALL_COUNT.3 < middle + 0.078 - 0.0175, "the count's box stops above the next row from {middle}");
    }
}

/// How many views of the Gizmo list a run's gizmos need: fewer kept when the list stops moving means a scroll didn't
/// take (a run went out with 7 of its 11 gizmos from the first view alone).
#[test]
fn the_views_a_gizmo_list_needs() {
    use alogs_meter::sim_tour::views_needed;
    for (gizmos, views) in [(0, 1), (1, 1), (6, 1), (7, 2), (9, 2), (10, 3), (11, 3), (12, 3), (13, 4), (30, 9)] {
        assert_eq!(views_needed(gizmos), views, "{gizmos} gizmos");
    }
}
