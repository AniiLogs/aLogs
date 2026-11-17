//! alogs_groups against the Python (tools/gen_pcstats_goldens.py), and the counters answering on this PC.

use std::collections::HashMap;

#[test]
fn groups_match_python() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("data/groups_goldens.json")).unwrap();
    for c in cases.as_array().unwrap() {
        let table: HashMap<u32, (u32, String)> = c["table"].as_object().unwrap().iter()
            .map(|(k, v)| (k.parse().unwrap(), (v[0].as_u64().unwrap() as u32, v[1].as_str().unwrap().to_string()))).collect();
        let starts: HashMap<u32, Option<f64>> = c["starts"].as_object().unwrap().iter().map(|(k, v)| (k.parse().unwrap(), v.as_f64())).collect();
        let helpers: Vec<u32> = c["helpers"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
        let u = |k: &str| c[k].as_u64().unwrap() as u32;
        let g = alogs_pcstats::alogs_groups(&table, u("app"), u("meter"), u("sanctum"), &helpers, &mut |p| starts.get(&p).copied().flatten());
        for (k, v) in c["groups"].as_object().unwrap() {
            let want: Vec<u32> = v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect();
            assert_eq!(g[k.as_str()].iter().copied().collect::<Vec<_>>(), want, "{k} in {c}");
        }
    }
}

#[cfg(windows)]
#[test]
fn the_counters_answer() {
    let mut cpu = alogs_pcstats::CpuLoad::default();
    assert!(cpu.sample(0, 0).is_none(), "the first sample measures nothing yet");
    let t = std::time::Instant::now();
    while t.elapsed().as_millis() < 50 {}
    let s = cpu.sample(0, 0).expect("a second sample");
    assert!(s.pc.is_some() && s.meter.is_some() && s.game.is_none());
    let (free, total) = alogs_pcstats::ram().unwrap();
    assert!(free > 0.0 && total >= free);
    assert!(alogs_pcstats::process_mb(std::process::id()).unwrap() > 0.0);
    assert!(alogs_pcstats::process_table().contains_key(&std::process::id()));
    assert!(alogs_pcstats::system_line(0).starts_with("PC: graphics"));
}
