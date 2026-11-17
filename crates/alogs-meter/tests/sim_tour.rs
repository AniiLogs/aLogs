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

/// How many views of the Gizmo list a run's gizmos need: fewer kept when the list stops moving means a scroll didn't
/// take (a run went out with 7 of its 11 gizmos from the first view alone).
#[test]
fn the_views_a_gizmo_list_needs() {
    use alogs_meter::sim_tour::views_needed;
    for (gizmos, views) in [(0, 1), (1, 1), (6, 1), (7, 2), (9, 2), (10, 3), (11, 3), (12, 3), (13, 4), (30, 9)] {
        assert_eq!(views_needed(gizmos), views, "{gizmos} gizmos");
    }
}
