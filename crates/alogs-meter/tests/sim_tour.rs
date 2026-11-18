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
