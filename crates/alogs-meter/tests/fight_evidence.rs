//! Fight evidence for Interlink runs (fight_shots.rs): six pictures of the fight picked from the looks the meter makes
//! anyway, written after the fight, and sent with the fight's log as an update of the run, never in its first request.
//! Synthetic pictures and made-up values only.

use alogs_meter::env::Env;
use alogs_meter::fight_shots::{self, FIGHT_LOG_MAX, ShotPicker, part_of, top_copy};
use alogs_meter::fights::Fight;
use alogs_meter::leaderboards::{FIGHT_EVIDENCE_RECHECK_S, FIGHT_EVIDENCE_WAIT_S, LATE_BATTLE_DATA_S, MAX_FOLLOWUPS};
use alogs_meter::meter::{Args, Meter};
use alogs_pil::{Image, Mode};
use alogs_pyval::{Dict, Value};
use alogs_reader::ocr::NoOcr;
use std::path::PathBuf;

const T: f64 = 1_800_000_000.0;

// ---------------------------------------------------------------------------------------------------------------------
// The picker

/// A small look whose pixels say which look it was (look number i).
fn look_pic(i: usize) -> Image {
    let mut im = Image::new(Mode::Rgb, 8, 10, &[0, 0, 0]);
    im.data[0] = (i % 256) as u8;
    im.data[1] = (i / 256) as u8;
    im
}

fn look_of(im: &Image) -> usize {
    im.data[0] as usize + 256 * im.data[1] as usize
}

/// Looks every `step` s for `secs` s (the text read's top-part look once a second, band looks otherwise, a forced
/// top-part look when the picker asks), with the team's damage from `damage(t)`. What was forced, and when.
fn fight(p: &mut ShotPicker, secs: f64, step: f64, limit: f64, damage: impl Fn(f64) -> i64) -> Vec<f64> {
    let mut forced = vec![];
    let per_s = (1.0 / step).round() as usize;
    let n = (secs / step).round() as usize;
    for i in 0..=n {
        let t = i as f64 * step;
        let text_look = i % per_s == 0;
        let mut top = text_look;
        if !text_look && p.force_due(t) {
            p.forced_at(t);
            forced.push(t);
            top = true;
        }
        if top {
            p.look(&look_pic(i), 10, t);
        }
        p.damage(damage(t), t, limit, true);
    }
    forced
}

/// Damage rising 10 a look (0.1 s), plus one-off rises at the given times.
fn damage_with(bursts: &'static [(f64, i64)], quiet: Option<(f64, f64)>) -> impl Fn(f64) -> i64 {
    move |t: f64| {
        let mut d = 0;
        let looks = (t / 0.1).round() as i64;
        for k in 1..=looks {
            let at = k as f64 * 0.1;
            if quiet.is_some_and(|(a, b)| at >= a && at < b) {
                continue;
            }
            d += 10;
        }
        d + bursts.iter().filter(|(at, _)| t + 1e-6 >= *at).map(|b| b.1).sum::<i64>()
    }
}

#[test]
fn six_parts_and_the_biggest_rise_of_each_wins() {
    let mut p = ShotPicker::default();
    p.start(true);
    static BURSTS: [(f64, i64); 6] = [(5.0, 500), (12.0, 800), (40.0, 300), (70.0, 900), (75.0, 100), (170.0, 700)];
    // part 5 (120-150 s) is quiet: nobody hit the boss
    let forced = fight(&mut p, 180.0, 0.1, 180.0, damage_with(&BURSTS, Some((120.0, 150.0))));
    let kept = p.finish();
    let got: Vec<(usize, f64)> = kept.iter().map(|s| (s.n, s.t)).collect();
    // each part's picture is the look right after its largest rise; part 4 had only the steady rises (its first one
    // wins), part 5 had none; part 6's first rise came at 150.0 s and was beaten at 170 s
    assert_eq!(got, vec![(1, 12.1), (2, 40.1), (3, 70.1), (4, 90.1), (6, 170.1)]);
    for s in &kept {
        assert_eq!(look_of(&s.im), (s.t * 10.0).round() as usize, "the picture is the look at {} s", s.t);
    }
    for w in forced.windows(2) {
        assert!(w[1] - w[0] >= 1.0 - 1e-9, "forced looks {w:?} less than a second apart");
    }
    assert!(!p.on, "finished");
}

#[test]
fn at_most_one_forced_look_a_second() {
    let mut p = ShotPicker::default();
    p.start(true);
    // every look a bigger rise than the last: a picture is wanted all the time; no text-read looks to take it
    let mut forced = vec![];
    for i in 0..300 {
        let t = i as f64 / 30.0;
        if p.force_due(t) {
            p.forced_at(t);
            forced.push(t);
            p.look(&look_pic(i), 10, t);
        }
        p.damage((i * i) as i64, t, 180.0, true);
    }
    assert_eq!(forced.len(), 10, "{forced:?}");
    assert_eq!(p.forced, 10);
    for w in forced.windows(2) {
        assert!(w[1] - w[0] >= 1.0 - 1e-9, "{w:?}");
    }
}

#[test]
fn a_short_fight_has_fewer_pictures() {
    let mut p = ShotPicker::default();
    p.start(true);
    static BURSTS: [(f64, i64); 1] = [(33.0, 400)];
    fight(&mut p, 50.0, 0.1, 180.0, damage_with(&BURSTS, None));
    let got: Vec<(usize, f64)> = p.finish().iter().map(|s| (s.n, s.t)).collect();
    assert_eq!(got, vec![(1, 0.2), (2, 33.1)], "a fight over at 50 s fills parts 1 and 2 only");
}

#[test]
fn overtime_splits_240_seconds() {
    assert_eq!(part_of(0.0, 180.0), 0);
    assert_eq!(part_of(29.9, 180.0), 0);
    assert_eq!(part_of(30.0, 180.0), 1);
    assert_eq!(part_of(179.9, 180.0), 5);
    assert_eq!(part_of(200.0, 180.0), 5, "past the limit: the last part");
    assert_eq!(part_of(39.9, 240.0), 0);
    assert_eq!(part_of(170.0, 240.0), 4);
    assert_eq!(part_of(239.0, 240.0), 5);
    assert_eq!(part_of(50.0, 0.0), 1, "no limit known: 180 s");
    let mut p = ShotPicker::default();
    p.start(true);
    static BURSTS: [(f64, i64); 2] = [(170.0, 900), (230.0, 900)];
    fight(&mut p, 240.0, 0.1, 240.0, damage_with(&BURSTS, None));
    let got: Vec<(usize, f64)> = p.finish().iter().map(|s| (s.n, s.t)).collect();
    assert_eq!(got, vec![(1, 0.2), (2, 40.1), (3, 80.1), (4, 120.1), (5, 170.1), (6, 230.1)]);
}

#[test]
fn nothing_is_picked_when_off_or_on_a_hand_over() {
    let mut p = ShotPicker::default();
    p.start(false);
    p.damage(0, 0.0, 180.0, true);
    p.damage(100, 0.1, 180.0, true);
    assert!(!p.force_due(1.0) && p.wants().is_none());
    p.start(true);
    p.damage(0, 0.0, 180.0, true);
    p.damage(1_000_000, 0.1, 180.0, false); // a two-boss fight's hand-over: the first boss's last HP counted at once
    assert!(p.wants().is_none(), "the hand-over's jump is no rise");
    p.damage(1_000_010, 0.2, 180.0, true);
    assert_eq!(p.wants(), Some(0));
}

// ---------------------------------------------------------------------------------------------------------------------
// The copy and its shrink

/// A picture whose pixel (x, y) is (x, y, x + y) mod 256.
fn gradient(w: usize, h: usize) -> Image {
    let mut d = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            d.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8]);
        }
    }
    Image::from_raw(Mode::Rgb, w, h, d)
}

#[test]
fn the_copy_is_halved_only_above_1920() {
    let rows = |h: usize| (h as f64 * alogs_meter::meter::TOP_PART) as usize;
    for (w, h, want) in [(3840, 2160, (1920, rows(2160) / 2)), (2560, 1440, (1280, rows(1440) / 2)), (1920, 1080, (1920, rows(1080))), (1280, 720, (1280, rows(720)))] {
        let c = top_copy(&gradient(w, h), rows(h));
        assert_eq!(c.size(), want, "{w}x{h}");
        assert_eq!(c.mode, Mode::Rgb);
    }
    // a top-part look (only the top rows) gives the same copy as a whole frame cropped
    let whole = gradient(2560, 1440);
    let top = top_copy(&whole, 604);
    let part = Image::from_raw(Mode::Rgb, 2560, 604, whole.data[..2560 * 604 * 3].to_vec());
    assert_eq!(top_copy(&part, 604), top);
    // the halving is a 2x2 average: pixel (x, y) of the copy is the mean of (2x..2x+1, 2y..2y+1)
    assert_eq!(&top.data[..3], &[1, 1, 1]); // (0+1+0+1)/4 rounded, (0+0+1+1)/4, (0+1+1+2)/4
    let (x, y) = (100, 50);
    let px = &top.data[(y * 1280 + x) * 3..][..3];
    assert_eq!(px, &[201, 101, 45]); // (300 + 301 + 301 + 302) mod 256 each: 44, 45, 45, 46
    // 1080p: the top rows as they are
    let small = gradient(1920, 1080);
    let c = top_copy(&small, 453);
    assert_eq!(c.data, small.data[..1920 * 453 * 3]);
}

/// The cost a picture adds to its look: copying (and halving) a 4K top part. Printed, not asserted (the time depends
/// on the PC and on the build); run with --release for the shipped meter's number.
#[test]
fn copy_cost_of_a_4k_top_part() {
    let look = gradient(3840, 907);
    let n = 20;
    let t0 = std::time::Instant::now();
    let mut out = 0;
    for _ in 0..n {
        out += top_copy(&look, 907).data.len();
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
    assert_eq!(out, n * 1920 * 453 * 3);
    println!("copy + halve of a 3840x907 top part: {ms:.2} ms a picture ({} build)", if cfg!(debug_assertions) { "debug" } else { "release" });
}

// ---------------------------------------------------------------------------------------------------------------------
// The fight's log

fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn gunzip(gz: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut out = vec![];
    flate2::read::GzDecoder::new(gz).read_to_end(&mut out).unwrap();
    out
}

fn made_up_csv(rows: usize) -> String {
    let mut s = String::from("wall_s,timer_s,timer_ocr,xn_ocr,segment,fill_pct,fill_rgb,edge_rgb,team_damage,avg_dps,recent_dps,note,break_left_pct,break_phase\r\n");
    for i in 0..rows {
        s += &format!("{:.2},{:.1},,,12,{:.2},ff6b66,2b2b2b,{},{},{},,100.0,idle\r\n", i as f64 * 0.09, 180.0 - i as f64 * 0.09, 99.0 - i as f64 * 0.01, i * 137, i * 11, i * 13);
    }
    s
}

#[test]
fn the_log_is_gzipped_as_it_is_with_a_size_cap() {
    let d = tmp("fe-gzip");
    let csv = d.join("20270115_080000_MadeupBoss.csv");
    let text = made_up_csv(2000);
    std::fs::write(&csv, &text).unwrap();
    let gz = fight_shots::gzip_file(&csv, FIGHT_LOG_MAX).unwrap();
    assert_eq!(gunzip(&gz), text.as_bytes(), "the CSV round-trips byte for byte");
    assert!(gz.len() < text.len() / 3, "{} of {}", gz.len(), text.len());
    let over = fight_shots::gzip_file(&csv, gz.len() - 1).unwrap_err();
    assert!(over.contains("over"), "{over}");
    let missing = fight_shots::gzip_file(&d.join("nothing.csv"), FIGHT_LOG_MAX).unwrap_err();
    assert!(missing.contains("missing"), "{missing}");

    // the post-fight writer: no compressed copy for a log over the cap, or for one that's missing
    fight_shots::write_evidence(&d, "small", vec![], csv.clone());
    assert_eq!(gunzip(&std::fs::read(d.join(fight_shots::log_rel("small"))).unwrap()), text.as_bytes());
    // about 1 MB of noise in hex: over 400,000 bytes even compressed
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    let noise: String = (0..125_000).map(|_| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        format!("{x:016x}")
    }).collect::<Vec<_>>().join("\r\n");
    std::fs::write(&csv, noise).unwrap();
    assert!(fight_shots::gzip_file(&csv, FIGHT_LOG_MAX).is_err());
    fight_shots::write_evidence(&d, "big", vec![], csv.clone());
    assert!(!d.join(fight_shots::log_rel("big")).exists(), "over the cap: not kept to send");
    fight_shots::write_evidence(&d, "gone", vec![], d.join("gone.csv"));
    assert!(!d.join(fight_shots::log_rel("gone")).exists());
}

// ---------------------------------------------------------------------------------------------------------------------
// The meter: after the fight, and sending

fn meter(name: &str, screenshots: bool) -> Meter {
    let data = tmp(name);
    std::fs::create_dir_all(data.join("logs").join("fights")).unwrap();
    let env = Env::new(data.join("res"), data.clone());
    env.fixed_now.set(Some(T));
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: true,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: true, player_log: PathBuf::new() };
    let mut m = Meter::new(env, args, Box::new(NoOcr), None);
    m.uid = Some("1".into());
    std::fs::write(data.join("settings.json"),
        format!(r#"{{"leaderboard": {{"submit": true, "token": "alb_test", "api_url": "http://127.0.0.1:9", "include_screenshots": {screenshots}}}}}"#)).unwrap();
    m.submitter = Some(alogs_meter::submit::Submitter::new(data));
    m
}

fn interlink_fight(m: &Meter, csv: &str) -> Fight {
    let mut row = alogs_reader::boss::Row::new();
    for (k, v) in [("puppetId", "2101"), ("table", "puppet_data"), ("bossTitle_en", "Made-up Boss")] {
        row.insert(k.into(), v.into());
    }
    let mut f = Fight::new(&m.env, "Made-up Boss", 50, row, 100_000, true, false, None, None);
    f.scene_ok = true;
    f.csv_name = csv.into();
    f
}

fn read(path: &PathBuf) -> Value {
    Value::json_loads(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn after_the_fight_the_pictures_are_written_and_the_summary_names_them() {
    let mut m = meter("fe-after", true);
    let f = interlink_fight(&m, "fight_x");
    std::fs::write(m.env.data.join("logs").join("fight_x.csv"), made_up_csv(42)).unwrap();
    // a 1440p fight: two pictures kept
    m.shots.start(true);
    let look = gradient(2560, 1440);
    m.shots.damage(0, 0.0, 180.0, true);
    m.shots.damage(500, 12.38, 180.0, true);
    assert!(m.shots.look(&look, 604, 12.44));
    m.shots.damage(1500, 70.1, 180.0, true);
    assert!(m.shots.look(&look, 604, 70.13));
    let mut f = f;
    f.rows = 42;
    m.fight_evidence_done(&f);
    m.fight_evidence_job.take().unwrap().join().unwrap();
    for n in [1, 3] {
        let p = m.env.data.join(fight_shots::shot_rel("fight_x", n));
        let mut d = jpeg_decoder::Decoder::new(std::io::Cursor::new(std::fs::read(&p).unwrap()));
        d.decode().unwrap();
        let info = d.info().unwrap();
        assert_eq!((info.width, info.height), (1280, 302), "{}", p.display());
    }
    assert!(!m.env.data.join(fight_shots::shot_rel("fight_x", 2)).exists());
    let gz = std::fs::read(m.env.data.join(fight_shots::log_rel("fight_x"))).unwrap();
    assert_eq!(gunzip(&gz), made_up_csv(42).as_bytes());

    // the summary, when it is written (the result screen), names them with their times
    m.fight_summary(&f, vec![("result", Value::json_loads(r#"{"rating_stars": 3, "score": 1000, "time_s": 90, "cleared": true, "screenshot": null, "players": []}"#).unwrap())]);
    let s = read(&m.run_path("fight_x"));
    assert_eq!(s.get("fight_shots").json_dumps(),
        r#"[{"n": 1, "t": 12.4, "file": "logs/screens/fight_fight_x_1.jpg"}, {"n": 3, "t": 70.1, "file": "logs/screens/fight_fight_x_3.jpg"}]"#);
    assert_eq!(s.get("fight_log").json_dumps(), r#"{"file": "logs/screens/fightlog_fight_x.csv.gz", "rows": 42}"#);
    // not another fight's
    let g = interlink_fight(&m, "fight_y");
    m.fight_summary(&g, vec![]);
    assert!(read(&m.run_path("fight_y")).as_dict().unwrap().get_str("fight_shots").is_none());
}

#[test]
fn a_fight_counts_its_csv_rows() {
    let data = tmp("fe-rows");
    let env = Env::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("res"), data.clone());
    env.fixed_now.set(Some(T));
    let mut row = alogs_reader::boss::Row::new();
    for (k, v) in [("puppetId", "2100000001"), ("table", "puppet_data"), ("bossTitle_en", "Test Boss"), ("name_en", "Test Boss"), ("in_boss_rush", "no")] {
        row.insert(k.into(), v.into());
    }
    let mut f = Fight::new(&env, "Test Boss", 50, row, 100_000, false, false, None, None);
    let grades = alogs_reader::boss::load_grades(&env.res, None).unwrap();
    for i in 0..5 {
        f.update(&env, Some(1.0 - i as f64 * 0.01), [255, 107, 102], [43, 43, 43], None, None, &grades, 180.0, Some(T + i as f64), true);
    }
    f.close(&env);
    let text = std::fs::read_to_string(data.join("logs").join(format!("{}.csv", f.csv_name))).unwrap();
    assert_eq!(f.rows as usize, text.lines().count() - 1, "{text}");
    assert!(f.rows >= 1);
}

const RESULT_PIC: &str = "logs/screens/result_20270115_080000.jpg";
const BD_PIC: &str = "logs/battle_data/20270115_080010.jpg";

fn pic(m: &Meter, rel: &str) {
    let p = m.env.data.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"made-up picture").unwrap();
}

fn on_disk(m: &Meter, rel: &str) -> bool {
    m.env.data.join(rel).is_file()
}

/// A finished Interlink run's summary with its result screen read, its fight evidence written (pictures of parts 1 and
/// 3 and the log), no Battle Data yet.
fn summary(m: &Meter, csv: &str, pictures: bool) -> PathBuf {
    let shot = if pictures { format!(r#""{RESULT_PIC}""#) } else { "null".into() };
    let a = fight_shots::shot_rel(csv, 1);
    let b = fight_shots::shot_rel(csv, 3);
    let g = fight_shots::log_rel(csv);
    let j = format!(r#"{{"boss": "Made-up Boss", "puppetId": "2101", "level": 50, "uid": "1", "cycle": "c1", "started": "2027-01-15 08:00:00",
        "bossrush": true, "interlink_scene": true, "meter": {{"csv": "{csv}.csv", "damage": 1000, "seconds": 90.0, "dps": 11}},
        "result": {{"rating_stars": 3, "score": 1000, "time_s": 90, "cleared": true, "screenshot": {shot},
                    "players": [{{"slot": 0, "name": "Made-up One", "self": true, "stat": "DMG Dealt", "value": 1000}}]}},
        "fight_shots": [{{"n": 1, "t": 12.4, "file": "{a}"}}, {{"n": 3, "t": 70.1, "file": "{b}"}}],
        "fight_log": {{"file": "{g}", "rows": 42}}}}"#);
    if pictures {
        pic(m, RESULT_PIC);
    }
    for r in [&a, &b, &g] {
        pic(m, r);
    }
    let path = m.run_path(csv);
    std::fs::write(&path, j).unwrap();
    path
}

fn edit(path: &PathBuf, f: impl FnOnce(&mut Dict)) {
    let Value::Dict(mut s) = Value::json_loads(&std::fs::read_to_string(path).unwrap()).unwrap() else { unreachable!() };
    f(&mut s);
    std::fs::write(path, Value::Dict(s).json_dumps()).unwrap();
}

fn battle_data(m: &Meter, path: &PathBuf, dps: i64, shot: Option<&str>) {
    if let Some(s) = shot {
        pic(m, s);
    }
    let bd = Value::json_loads(&format!(r#"{{"players": [{{"slot": 0, "name": "Made-up One", "avg_dps": {dps}, "break": 1, "taken": 2, "heal": 3}}],
        "team_avg_dps": {dps}, "screenshot": {}}}"#, shot.map_or("null".into(), |s| format!("\"{s}\"")))).unwrap();
    edit(path, |s| s.insert("battle_data".into(), bd));
}

fn queue_file(m: &Meter, csv: &str) -> PathBuf {
    m.env.data.join("logs").join("submit_queue").join(format!("interlink_run__{csv}.json"))
}

fn queued(m: &Meter, csv: &str) -> Option<Value> {
    std::fs::read_to_string(queue_file(m, csv)).ok().and_then(|t| Value::json_loads(&t))
}

/// The sender got the copy up: it leaves the queue and its files are gone (as Submitter::drop_proof does).
fn uploaded(m: &Meter, csv: &str) -> Value {
    let item = queued(m, csv).expect("a copy was queued");
    std::fs::remove_file(queue_file(m, csv)).unwrap();
    for k in ["evidence", "battle", "party"] {
        if item.get(k).truthy() {
            let _ = std::fs::remove_file(m.env.data.join(item.get(k).py_str()));
        }
    }
    for f in alogs_meter::submit::fight_files(&item) {
        let _ = std::fs::remove_file(m.env.data.join(f));
    }
    item
}

fn at(m: &mut Meter, t: f64) {
    m.env.fixed_now.set(Some(t));
    m.check_followup(t);
}

fn avg_dps(item: &Value) -> Option<i64> {
    item.get("payload").get("players").as_list().and_then(|l| l.first()).and_then(|p| p.get("avgDps").as_i64())
}

fn extra(item: &Value) -> &Value {
    item.get("payload").get("extra")
}

fn fight_parts(item: &Value) -> Vec<i64> {
    item.get("fight").as_list().map(|l| l.iter().filter_map(|e| e.get("n").as_i64()).collect()).unwrap_or_default()
}

const SHOTS: &str = r#"[{"n": 1, "t": 12.4}, {"n": 3, "t": 70.1}]"#;

#[test]
fn never_in_the_first_request() {
    let mut m = meter("fe-first", true);
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path);
    let first = queued(&m, "fight_a").unwrap();
    assert!(fight_parts(&first).is_empty() && !first.get("fightlog").truthy(), "{}", first.json_dumps());
    let e = extra(&first).as_dict().unwrap();
    assert!(e.get_str("fightShots").is_none() && e.get_str("fightLog").is_none());
    assert_eq!(first.get("evidence").py_str(), RESULT_PIC);
    let s = read(&path);
    assert_eq!(s.get("fight_due").as_f64(), Some(T + FIGHT_EVIDENCE_WAIT_S));
    assert!(on_disk(&m, &s.get("late_result").py_str()), "a copy of the result screen is kept to send the evidence with");

    // while that first copy still waits in the queue (offline), the evidence waits too
    at(&mut m, T + 7.0);
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.5);
    let still = queued(&m, "fight_a").unwrap();
    assert!(fight_parts(&still).is_empty(), "the first copy wasn't joined by the evidence");
    assert_eq!(m.rs.followup.as_ref().unwrap().1, T + FIGHT_EVIDENCE_WAIT_S + 0.5 + FIGHT_EVIDENCE_RECHECK_S, "looked at again shortly");
    uploaded(&m, "fight_a");
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.5 + FIGHT_EVIDENCE_RECHECK_S + 0.1);
    let upd = queued(&m, "fight_a").expect("once the first copy is up, the evidence goes");
    assert_eq!(fight_parts(&upd), vec![1, 3]);
}

/// A live meter whose sends hold the player's inputs (Automation and "hold inputs" on, the game in front); the hold
/// is a stand-in that holds nothing.
fn holding(name: &str) -> Meter {
    let mut m = meter(name, true);
    m.args.replay = false;
    m.hold = alogs_capture::input::InputBlock::dry();
    if let Value::Dict(d) = &mut m.args.cfg {
        d.insert("leaderboard".into(), alogs_pyval::dict([("submit", Value::Bool(true)), ("automation", Value::Bool(true))]));
        d.insert("results".into(), alogs_pyval::dict([("hold_inputs", Value::Bool(true))]));
    }
    assert!(m.holding_for_send());
    m
}

/// Battle Data read a few seconds after the result: its update (which holds the player's inputs) carries none of the
/// evidence; the evidence goes after it, alone, in a request that holds nothing, and counts apart from the updates.
#[test]
fn with_battle_data_it_goes_after_it_alone() {
    let mut m = holding("fe-with-bd");
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path);
    assert!(m.send_waiting.is_some(), "the first send holds inputs");
    let first = uploaded(&m, "fight_a");
    assert!(fight_parts(&first).is_empty());
    m.send_waiting = None;
    at(&mut m, T + 6.5);
    assert!(queued(&m, "fight_a").is_none());
    m.env.fixed_now.set(Some(T + 9.0));
    battle_data(&m, &path, 500, Some(BD_PIC));
    m.send_late_battle_data("fight_a");
    let upd = queued(&m, "fight_a").expect("the Battle Data update");
    assert!(m.send_waiting.is_some(), "the Battle Data update holds inputs");
    assert_eq!(upd.get("battle").py_str(), BD_PIC);
    assert!(fight_parts(&upd).is_empty() && !upd.get("fightlog").truthy(), "no evidence in a request that holds inputs");
    let e = extra(&upd).as_dict().unwrap();
    assert!(e.get_str("fightShots").is_none() && e.get_str("fightLog").is_none());
    // its wait is over, but the Battle Data update isn't out yet: the evidence waits for it
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.1);
    assert!(fight_parts(&queued(&m, "fight_a").unwrap()).is_empty());
    uploaded(&m, "fight_a");
    m.send_waiting = None;
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.1 + FIGHT_EVIDENCE_RECHECK_S + 0.1);
    let ev = queued(&m, "fight_a").expect("the evidence's own request");
    assert_eq!(m.send_waiting, None, "it holds nothing");
    assert_eq!(fight_parts(&ev), vec![1, 3]);
    assert_eq!(ev.get("fight").as_list().unwrap()[1].get("file").py_str(), fight_shots::shot_rel("fight_a", 3));
    assert_eq!(ev.get("fightlog").py_str(), fight_shots::log_rel("fight_a"));
    assert!(ev.get("evidence").py_str().starts_with("logs/screens/resend_"), "with the result screen");
    assert!(!ev.get("battle").truthy() && !ev.get("party").truthy());
    assert_eq!(extra(&ev).get("fightShots").json_dumps(), SHOTS);
    assert_eq!(extra(&ev).get("fightLog").json_dumps(), r#"{"rows": 42}"#);
    assert_eq!(avg_dps(&ev), Some(500), "the run's numbers as they stand");
    assert_eq!(read(&path).get("followups").as_i64(), Some(1), "counted apart from the updates");
    uploaded(&m, "fight_a");

    at(&mut m, T + 60.0);
    assert!(queued(&m, "fight_a").is_none(), "once");
    // a later update (a new Battle Data read) keeps the entries, without the files again
    battle_data(&m, &path, 501, None);
    m.send_late_battle_data("fight_a");
    let later = queued(&m, "fight_a").unwrap();
    assert!(fight_parts(&later).is_empty() && !later.get("fightlog").truthy());
    assert_eq!(extra(&later).get("fightShots").json_dumps(), SHOTS);
}

/// The evidence waiting in the queue, replaced by an update that holds inputs: that update carries none of it, and
/// the evidence goes again on its own once the update is out.
#[test]
fn a_holding_update_never_takes_the_evidence_over() {
    let mut m = holding("fe-hold-replace");
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path);
    uploaded(&m, "fight_a");
    m.send_waiting = None;
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.1);
    assert_eq!(fight_parts(&queued(&m, "fight_a").unwrap()), vec![1, 3], "the evidence's request, not sent yet");
    m.env.fixed_now.set(Some(T + 30.0));
    battle_data(&m, &path, 500, Some(BD_PIC));
    m.send_late_battle_data("fight_a");
    let upd = queued(&m, "fight_a").unwrap();
    assert!(m.send_waiting.is_some());
    assert_eq!(upd.get("battle").py_str(), BD_PIC);
    assert!(fight_parts(&upd).is_empty() && !upd.get("fightlog").truthy());
    assert!(extra(&upd).as_dict().unwrap().get_str("fightShots").is_none());
    assert!(on_disk(&m, &fight_shots::shot_rel("fight_a", 1)), "its files stay for its own request");
    uploaded(&m, "fight_a");
    m.send_waiting = None;
    at(&mut m, T + 40.0);
    let ev = queued(&m, "fight_a").expect("the evidence again, on its own");
    assert_eq!(fight_parts(&ev), vec![1, 3]);
    assert_eq!(m.send_waiting, None);
}

/// Battle Data updates using up all MAX_FOLLOWUPS: the evidence still gets its one request (counted apart, a cap of
/// one of its own).
#[test]
fn the_evidence_is_counted_apart_from_the_updates() {
    let mut m = meter("fe-count", true);
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path);
    uploaded(&m, "fight_a");
    m.env.fixed_now.set(Some(T + 8.0));
    for dps in [500, 501, 502] {
        battle_data(&m, &path, dps, None);
        m.send_late_battle_data("fight_a");
        uploaded(&m, "fight_a");
    }
    assert_eq!(read(&path).get("followups").as_i64(), Some(MAX_FOLLOWUPS));
    battle_data(&m, &path, 503, None);
    m.send_late_battle_data("fight_a");
    assert!(queued(&m, "fight_a").is_none(), "no fourth update");
    // (that reading can't go as an update any more, so it doesn't keep the evidence waiting)
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.1);
    let ev = queued(&m, "fight_a").expect("the evidence still goes");
    assert_eq!(fight_parts(&ev), vec![1, 3]);
    assert_eq!(read(&path).get("followups").as_i64(), Some(MAX_FOLLOWUPS));
    uploaded(&m, "fight_a");
    at(&mut m, T + 60.0);
    assert!(queued(&m, "fight_a").is_none(), "once");
}

/// No Battle Data: the evidence goes alone FIGHT_EVIDENCE_WAIT_S after the first send, with the result screen as
/// its evidence and no Battle Data picture; once.
#[test]
fn it_goes_alone_when_no_battle_data_comes() {
    let mut m = meter("fe-alone", true);
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path);
    uploaded(&m, "fight_a");
    at(&mut m, T + 6.5);
    assert_eq!(m.rs.followup.as_ref().unwrap().1, T + FIGHT_EVIDENCE_WAIT_S, "next look: when its wait is over");
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S - 0.5);
    assert!(queued(&m, "fight_a").is_none());
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.1);
    let upd = queued(&m, "fight_a").expect("the evidence's own update");
    assert_eq!(fight_parts(&upd), vec![1, 3]);
    assert!(upd.get("fightlog").truthy());
    assert!(upd.get("evidence").py_str().starts_with("logs/screens/resend_"), "the result screen goes with it");
    assert!(!upd.get("battle").truthy() && !upd.get("party").truthy());
    assert_eq!(extra(&upd).get("fightShots").json_dumps(), SHOTS);
    assert_eq!(m.rs.followup.as_ref().unwrap().1, T + LATE_BATTLE_DATA_S, "then only the window's end");
    uploaded(&m, "fight_a");
    at(&mut m, T + 100.0);
    assert!(queued(&m, "fight_a").is_none());
    assert_eq!(read(&path).get("followups").as_i64(), None, "the evidence's request isn't one of the run's updates");
    assert_eq!(read(&path).get("fight_evidence").get("shots").json_dumps(), SHOTS);
}

/// The automatic Battle Data session is running: Battle Data is coming, so the evidence waits for it.
#[test]
fn it_waits_while_battle_data_is_on_its_way() {
    let mut m = meter("fe-auto", true);
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path);
    uploaded(&m, "fight_a");
    m.auto.auto_until = T + 60.0;
    at(&mut m, T + 6.5);
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.1);
    assert!(queued(&m, "fight_a").is_none(), "the session runs: Battle Data will bring it");
    m.auto.auto_until = 0.0; // the session ended without Battle Data
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.1 + FIGHT_EVIDENCE_RECHECK_S + 0.1);
    assert_eq!(fight_parts(&queued(&m, "fight_a").expect("then it goes alone")), vec![1, 3]);
}

/// Another run's window opening (or the meter closing) settles this one: the evidence goes then, on its own.
#[test]
fn it_goes_when_the_window_is_settled() {
    let mut m = meter("fe-settle", true);
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path);
    uploaded(&m, "fight_a");
    m.env.fixed_now.set(Some(T + 8.0));
    m.settle_followup();
    assert_eq!(fight_parts(&queued(&m, "fight_a").expect("sent as the window closes")), vec![1, 3]);
    assert!(m.rs.followup.is_none());
}

/// Without "Attach the screenshots as proof" there's no result screen to send it with: given up on, files removed.
#[test]
fn without_screenshots_it_is_given_up_and_removed() {
    let mut m = meter("fe-no-shots", false);
    let path = summary(&m, "fight_a", false);
    m.decide_run(&path);
    uploaded(&m, "fight_a");
    at(&mut m, T + 6.5);
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.1);
    assert!(queued(&m, "fight_a").is_none());
    let s = read(&path);
    assert!(s.get("fight_skipped").truthy());
    for r in [fight_shots::shot_rel("fight_a", 1), fight_shots::shot_rel("fight_a", 3), fight_shots::log_rel("fight_a")] {
        assert!(!on_disk(&m, &r), "{r}");
    }
    assert_eq!(m.rs.followup.as_ref().unwrap().1, T + LATE_BATTLE_DATA_S, "nothing more is looked at for it");
}

/// The window ends with the evidence never sent (the run stayed in the queue all along): its files go.
#[test]
fn the_window_ending_removes_unsent_evidence() {
    let mut m = meter("fe-window", true);
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path); // never uploaded
    at(&mut m, T + 6.5);
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.1);
    let mut t = T + FIGHT_EVIDENCE_WAIT_S + 0.1;
    while m.rs.followup.is_some() && t < T + LATE_BATTLE_DATA_S + 10.0 {
        t = m.rs.followup.as_ref().unwrap().1 + 0.1;
        at(&mut m, t);
    }
    assert!(m.rs.followup.is_none());
    assert!(read(&path).get("fight_skipped").truthy());
    assert!(!on_disk(&m, &fight_shots::shot_rel("fight_a", 1)) && !on_disk(&m, &fight_shots::log_rel("fight_a")));
    assert!(fight_parts(&queued(&m, "fight_a").unwrap()).is_empty(), "the run's own copy is untouched");
}

/// Keep screenshots on: nothing removed, even when the evidence is given up on.
#[test]
fn keep_screenshots_keeps_the_files() {
    let mut m = meter("fe-keep", false);
    std::fs::write(m.env.data.join("settings.json"),
        r#"{"leaderboard": {"submit": true, "token": "alb_test", "api_url": "http://127.0.0.1:9", "include_screenshots": false}, "capture": {"keep_hours": 24}}"#).unwrap();
    let path = summary(&m, "fight_a", false);
    m.decide_run(&path);
    uploaded(&m, "fight_a");
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.1);
    assert!(read(&path).get("fight_skipped").truthy());
    assert!(on_disk(&m, &fight_shots::shot_rel("fight_a", 1)) && on_disk(&m, &fight_shots::log_rel("fight_a")));
}

/// The evidence's queued request, replaced by a newer update that holds nothing (here no hold is possible) before it
/// went: the newer one carries the files.
#[test]
fn a_replacing_update_carries_the_evidence() {
    let mut m = meter("fe-replace", true);
    let path = summary(&m, "fight_a", true);
    m.decide_run(&path);
    uploaded(&m, "fight_a");
    at(&mut m, T + FIGHT_EVIDENCE_WAIT_S + 0.1);
    assert_eq!(fight_parts(&queued(&m, "fight_a").unwrap()), vec![1, 3]);
    m.env.fixed_now.set(Some(T + 30.0));
    battle_data(&m, &path, 500, Some(BD_PIC));
    m.send_late_battle_data("fight_a");
    let upd = queued(&m, "fight_a").unwrap();
    assert_eq!(upd.get("battle").py_str(), BD_PIC);
    assert_eq!(fight_parts(&upd), vec![1, 3], "the replaced copy's evidence goes with this one");
    assert!(upd.get("fightlog").truthy());
    assert_eq!(extra(&upd).get("fightShots").json_dumps(), SHOTS);
}

#[test]
fn the_cleanup_keeps_a_queued_runs_evidence() {
    use std::time::{Duration, SystemTime};
    let data = tmp("fe-cleanup");
    let logs = data.join("logs");
    let old = |rel: &str| {
        let p = data.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(SystemTime::now() - Duration::from_secs(30 * 3600)).unwrap();
    };
    for r in ["logs/screens/fight_a_1.jpg", "logs/screens/fightlog_a.csv.gz", "logs/screens/fight_b_1.jpg", "logs/screens/fightlog_b.csv.gz", "logs/other.csv.gz"] {
        old(r);
    }
    std::fs::create_dir_all(logs.join("submit_queue")).unwrap();
    std::fs::write(logs.join("submit_queue/interlink_run__a.json"),
        r#"{"evidence": "logs/screens/late.jpg", "fight": [{"n": 1, "file": "logs/screens/fight_a_1.jpg"}], "fightlog": "logs/screens/fightlog_a.csv.gz"}"#).unwrap();
    let (files, _) = alogs_meter::housekeeping::clean_old_screenshots(&data, Some(0));
    assert_eq!(files, 2, "the unqueued run's picture and compressed log");
    assert!(data.join("logs/screens/fight_a_1.jpg").exists() && data.join("logs/screens/fightlog_a.csv.gz").exists());
    assert!(!data.join("logs/screens/fight_b_1.jpg").exists() && !data.join("logs/screens/fightlog_b.csv.gz").exists());
    assert!(data.join("logs/other.csv.gz").exists(), "only a compressed log beside the pictures counts");
}
