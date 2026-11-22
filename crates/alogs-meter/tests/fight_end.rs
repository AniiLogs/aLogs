//! The hold from an Interlink fight's fade to black (issue 154): the fade is watched for only in the last seconds of a
//! followed fight (its countdown, or the boss on its last bar), starts the result screen's hold at once, hands over to
//! the result screen's own hold, and is released at once when the fight is seen to go on. Made-up fights and frames
//! with a fake clock; the input hold is a stand-in (InputBlock::dry), so no keys are ever held while the tests run.

use alogs_capture::input::InputBlock;
use alogs_gamelog::Event;
use alogs_meter::env::Env;
use alogs_meter::fight_end::{ARM_AFTER_S, ARM_LEFT_S, FADE_HOLD_S, KILL_AFTER_S};
use alogs_meter::fights::{Fight, Timer};
use alogs_meter::meter::{Args, Frames, Meter, Out};
use alogs_pil::{Image, Mode};
use alogs_pyval::Value;
use alogs_reader::boss::Row;
use alogs_reader::ocr::NoOcr;
use indexmap::IndexMap;
use std::path::PathBuf;

const T: f64 = 1_800_000_000.0;

fn env(name: &str) -> Env {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let mut env = Env::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("res"), data);
    env.write_files = false;
    env.fixed_now.set(Some(T));
    env
}

/// A live meter with Automation's automatic Battle Data on (as the result screen's hold needs) and the defaults'
/// "hold inputs"; its input hold holds nothing.
fn meter(name: &str) -> Meter {
    let args = Args { cfg: alogs_protocol::settings::defaults(), duration: 180.0, level: None, boss_id: None, replay: false,
                      no_submit: true, no_debug_frames: true, no_overlay: true, no_auto_click: false, player_log: PathBuf::new() };
    let mut m = Meter::new(env(name), args, Box::new(NoOcr), None);
    m.hold = InputBlock::dry();
    m.hwnd = Some(1);
    m
}

fn row(pid: &str, title: &str) -> Row {
    let mut r = Row::new();
    for (k, v) in [("puppetId", pid), ("table", "puppet_data"), ("bossTitle_en", title), ("name_en", title), ("in_boss_rush", "yes")] {
        r.insert(k.into(), v.into());
    }
    r
}

/// An Interlink fight with a `limit` s clock, `into` s in (the clock read just now), hit, on bar x5 at 60%.
fn rush(m: &Meter, limit: f64, into: f64) -> Fight {
    let mut f = Fight::new(&m.env, "Test Boss", 60, row("80000001", "Test Boss"), 1_000_000, true, false, None, None);
    let mut t = Timer::new(limit);
    t.val = Some(limit - into);
    t.at = Some(m.env.now());
    f.timer = Some(t);
    f.limit = Some(limit as i64);
    f.hit = true;
    f.seg = Some(5);
    f.seg_fill = 0.6;
    f
}

fn at(m: &mut Meter, t: f64) {
    m.env.fixed_now.set(Some(t));
}

fn black() -> Image {
    Image::new(Mode::Rgb, 1920, 1080, &[3, 3, 3])
}

fn arena() -> Image {
    Image::new(Mode::Rgb, 1920, 1080, &[70, 90, 120])
}

/// The countdown left at `t`, the clock read `left` s before the end at T.
fn clock_at(m: &mut Meter, left_at_t: f64) {
    let f = m.fight.as_mut().unwrap();
    let tm = f.timer.as_mut().unwrap();
    tm.val = Some(left_at_t);
    tm.at = Some(T);
}

#[derive(Default)]
struct Log(Vec<(String, Value)>);
impl Out for Log {
    fn out(&mut self, kind: &str, data: Value) {
        self.0.push((kind.to_string(), data));
    }
}

/// Scripted frames: one picture, and what each grab asked for (None: the whole frame).
struct Script {
    frame: Image,
    asked: Vec<Option<f64>>,
}
impl Frames for Script {
    fn grab(&mut self, _: usize, top: Option<f64>) -> Option<Image> {
        self.asked.push(top);
        Some(self.frame.clone())
    }
    fn window(&mut self) -> Option<usize> {
        Some(1)
    }
}

/// A 3:00 fight: no watch at 2:30 left (a fade there starts nothing, and isn't even looked for); the watch opens in
/// its last seconds; a fade then holds the inputs at once; the result screen's own hold carries on; it ends as today.
#[test]
fn a_three_minute_fight_is_held_from_its_fade() {
    let mut m = meter("fe-3min");
    m.fight = Some(rush(&m, 180.0, 30.0));
    m.fight_end_tick(T);
    assert!(m.fight_end.armed.is_none(), "2:30 left: no watch");
    m.fight_end_look(&black(), T, true);
    m.fight_end_look(&black(), T + 0.03, true);
    assert_eq!(m.fight_end.checks, 0, "no fade check outside the window");
    assert!(!m.hold.active());
    // 0:06 left: not yet; 0:05: open
    clock_at(&mut m, 6.0);
    m.fight_end_tick(T);
    assert!(m.fight_end.armed.is_none(), "6 s left: not yet");
    at(&mut m, T + 1.0);
    m.fight_end_tick(T + 1.0);
    let (_, why) = m.fight_end.armed.clone().expect("5 s left: the watch is open");
    assert_eq!(why, "0:05 left");
    // the fight's own picture: looked at, nothing held
    m.fight_end_look(&arena(), T + 1.0, true);
    assert_eq!(m.fight_end.checks, 1);
    assert!(m.fight_end.held.is_none());
    // the fade, two looks: held from the second
    at(&mut m, T + 6.4);
    m.fight_end_tick(T + 6.4);
    m.fight_end_look(&black(), T + 6.4, true);
    assert!(!m.hold.active(), "one black picture is not yet a fade");
    m.fight_end_look(&black(), T + 6.43, true);
    assert!(m.hold.active(), "the fade holds the inputs");
    assert_eq!(m.fight_end.held, Some(T + 6.43));
    // the result screen comes up and is read: its own hold (the automatic Battle Data session) takes over
    at(&mut m, T + 7.2);
    m.fight = None; // ended by the result screen
    let mut frames = Script { frame: arena(), asked: vec![] };
    let heads: IndexMap<String, String> = [("lbl".to_string(), "Challenge Score Completion Time".to_string())].into_iter().collect();
    m.read_screen(&mut frames, &mut Log::default(), &arena(), T + 7.2, &heads);
    assert!(m.auto_active(), "the result screen's hold started as today");
    assert!(m.fight_end.held.is_none() && m.fight_end.armed.is_none(), "handed over");
    assert!(m.hold.active(), "and the inputs stay held");
    m.fight_end_tick(T + 7.3);
    assert!(m.hold.active(), "the watch's own checks leave the session's hold alone");
    // the session ends (Battle Data read, the run sent): the inputs come back
    m.end_auto("Battle Data read");
    assert!(!m.hold.active());
}

/// The same with Overtime's 4:00 clock: at 3:00 into the fight (1:00 left) nothing is watched; in its last seconds it is.
#[test]
fn the_overtime_clock_opens_the_watch_at_its_own_end() {
    let mut m = meter("fe-4min");
    m.fight = Some(rush(&m, 240.0, 180.0));
    m.fight_end_tick(T);
    assert!(m.fight_end.armed.is_none(), "3:00 into an Overtime fight is not its end");
    clock_at(&mut m, 4.0);
    m.fight_end_tick(T);
    assert_eq!(m.fight_end.armed.as_ref().map(|a| a.1.as_str()), Some("0:04 left"));
    at(&mut m, T + 4.5);
    m.fight_end_look(&black(), T + 4.5, true);
    m.fight_end_look(&black(), T + 4.53, true);
    assert!(m.hold.active());
    // no result screen comes: the hold ends by itself
    at(&mut m, T + 4.53 + FADE_HOLD_S + 0.1);
    m.fight_end_tick(T + 4.53 + FADE_HOLD_S + 0.1);
    assert!(!m.hold.active() && m.fight_end.held.is_none());
}

/// Through the meter's own look: outside the window (no fight, a world fight, mid-fight) black frames start nothing
/// and are never checked, and the fight is followed on the top part as before; in the window every look is a whole
/// frame and two black looks hold.
#[test]
fn a_fade_outside_the_window_starts_nothing() {
    let mut m = meter("fe-outside");
    let mut log = Log::default();
    let mut frames = Script { frame: black(), asked: vec![] };
    // a menu, the open world: no fight
    for i in 0..5 {
        at(&mut m, T + f64::from(i) * 0.1);
        m.step(&mut frames, &mut log);
    }
    // a world boss in its last seconds: not an Interlink fight
    let mut world = Fight::new(&m.env, "Test Boss", 50, row("2100000001", "Test Boss"), 100_000, false, false, None, None);
    world.hit = true;
    world.seg = Some(1);
    world.seg_fill = 0.01;
    m.fight = Some(world);
    m.bar_seen = m.env.now();
    m.fight_end_tick(m.env.now());
    m.fight_end_look(&black(), m.env.now(), true);
    // mid-fight in Interlink, 1:40 left, the bar up
    let b = alogs_reader::bar::Bar { yc: 194, left_bracket: 600.0, span: 700.0, brackets_found: true, world: false, right_red: 0.0, track_left: 610,
                                     track_right: 1290, thickness: 13, frame: (1920, 1080) };
    at(&mut m, T + 1.0);
    m.fight = Some(rush(&m, 180.0, 80.0));
    m.frame_size = (1920, 1080);
    for i in 0..5 {
        let t = T + 1.0 + f64::from(i) * 0.05;
        at(&mut m, t);
        m.bar = Some(b.clone());
        m.bar_seen = t;
        m.step(&mut frames, &mut log);
    }
    assert_eq!(m.fight_end.checks, 0, "no fade check ran");
    assert!(m.fight_end.armed.is_none() && m.fight_end.held.is_none());
    assert!(!m.hold.active());
    assert!(frames.asked.iter().rev().take(5).any(|a| a.is_some()), "the fight's looks are top parts as before: {:?}", frames.asked);
    // the same fight's last seconds: whole frames, and the fade holds
    clock_at(&mut m, 3.0);
    frames.asked.clear();
    for i in 0..2 {
        let t = T + 0.1 + f64::from(i) * 0.04;
        at(&mut m, t);
        m.bar = Some(b.clone());
        m.bar_seen = t;
        m.step(&mut frames, &mut log);
    }
    assert!(frames.asked.iter().all(Option::is_none), "every look a whole frame: {:?}", frames.asked);
    assert_eq!(m.fight_end.checks, 2);
    assert!(m.hold.active(), "the fade in the window holds");
}

/// A fade inside the window and then the fight's bar on screen again: released at once, and the
/// watch can open again. Through the meter's look, on a real bar picture.
#[test]
fn a_fight_that_goes_on_is_released_at_once() {
    let mut m = meter("fe-goes-on");
    // a picture of a boss bar
    let frames_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../alogs-reader/tests/data/frames");
    let mut names: Vec<_> = std::fs::read_dir(&frames_dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "png")).collect();
    names.sort();
    let (pic, b) = names
        .iter()
        .find_map(|p| {
            let pic = Image::open_png(p)?.convert(Mode::Rgb);
            let b = alogs_reader::bar::find_bar(&pic, false, 8)?;
            alogs_reader::bar::verify_bar(&pic, &b).then_some((pic, b))
        })
        .expect("a bar picture");
    m.fight = Some(rush(&m, 180.0, 177.0));
    m.fight_end_tick(T);
    assert!(m.fight_end.armed.is_some());
    m.fight_end_look(&black(), T, true);
    m.fight_end_look(&black(), T + 0.03, true);
    assert!(m.hold.active());
    // the bar is back (a fade mid-fight, Overtime going on, a two-boss change): the meter's next look releases
    let size = pic.size();
    let mut frames = Script { frame: pic, asked: vec![] };
    at(&mut m, T + 0.5);
    m.bar = Some(b);
    m.bar_seen = T + 0.5;
    m.frame_size = size;
    m.step(&mut frames, &mut Log::default());
    assert!(m.fight_end.held.is_none(), "released when the bar was seen again");
    assert!(!m.hold.active());
    m.fight_end_tick(T + 0.6);
    assert!(m.fight_end.armed.is_some(), "the watch opens again: the end is still near");
    assert!(m.fight_end.blacks == 0);
}

/// The press that got through on 21 Nov came within about a second of the fade, before the result screen was read:
/// now the inputs are held from the fade's look itself, before any screen read.
#[test]
fn a_press_right_after_the_fade_is_held() {
    let mut m = meter("fe-press");
    m.fight = Some(rush(&m, 180.0, 179.4)); // the 21 Nov fight: the clock's zero 0.4 s before the bar's end
    m.fight_end_tick(T);
    at(&mut m, T + 0.6);
    m.fight_end_tick(T + 0.6);
    m.fight_end_look(&black(), T + 0.6, true);
    at(&mut m, T + 0.63);
    m.fight_end_look(&black(), T + 0.63, true);
    assert!(m.hold.active(), "held 33 ms into the fade");
    assert!(!m.auto_active() && m.rs.result_seen == 0.0, "before any result screen was seen");
    // with the game in the background nothing is held (as today's result-screen hold)
    let mut m = meter("fe-press-bg");
    m.fight = Some(rush(&m, 180.0, 179.0));
    m.fight_end_tick(T);
    m.fight_end_look(&black(), T, false);
    m.fight_end_look(&black(), T + 0.03, false);
    assert!(!m.hold.active());
    // and nothing is watched without Automation's automatic Battle Data (the result screen's hold's own gate)
    let mut m = meter("fe-press-off");
    m.args.no_auto_click = true;
    m.fight = Some(rush(&m, 180.0, 179.0));
    m.fight_end_tick(T);
    assert!(m.fight_end.armed.is_none());
}

/// The window ends by itself: the countdown past its zero with no fade, the boss's last bar not seen for a while, a
/// scene change; and a killed boss opens it, but not a two-boss fight's first boss.
#[test]
fn the_watch_ends_by_itself() {
    // the clock: open from ARM_LEFT_S before zero to ARM_AFTER_S after
    let mut m = meter("fe-ends-clock");
    m.fight = Some(rush(&m, 180.0, 180.0 - ARM_LEFT_S));
    m.fight_end_tick(T);
    assert!(m.fight_end.armed.is_some());
    let late = T + ARM_LEFT_S + ARM_AFTER_S + 0.1;
    at(&mut m, late);
    m.fight_end_look(&arena(), late, true);
    m.fight_end_tick(late);
    assert!(m.fight_end.armed.is_none(), "no fade within its time: closed");
    let n = m.fight_end.checks;
    m.fight_end_look(&black(), late, true);
    assert_eq!(m.fight_end.checks, n, "and no more checks");
    // the kill: the boss's last bar nearly empty opens it, the bar unseen for KILL_AFTER_S closes it
    let mut m = meter("fe-ends-kill");
    let mut f = rush(&m, 180.0, 100.0);
    f.seg = Some(1);
    f.seg_fill = 0.04;
    m.fight = Some(f);
    m.bar_seen = T;
    m.fight_end_tick(T);
    assert_eq!(m.fight_end.armed.as_ref().map(|a| a.1.as_str()), Some("the boss's last bar at 4%"));
    let mut m2 = meter("fe-ends-kill-6");
    let mut f = rush(&m2, 180.0, 100.0);
    f.seg = Some(1);
    f.seg_fill = 0.06;
    m2.fight = Some(f);
    m2.bar_seen = T;
    m2.fight_end_tick(T);
    assert!(m2.fight_end.armed.is_none(), "6% of the last bar: not yet");
    at(&mut m, T + KILL_AFTER_S + 0.1);
    m.fight_end_tick(T + KILL_AFTER_S + 0.1);
    assert!(m.fight_end.armed.is_none(), "the bar gone 10 s and no fade: closed");
    // a scene change closes it
    let mut m = meter("fe-ends-scene");
    let (tx, rx) = std::sync::mpsc::channel();
    m.events = Some(rx);
    m.fight = Some(rush(&m, 180.0, 178.0));
    m.fight_end_tick(T);
    assert!(m.fight_end.armed.is_some());
    tx.send(Event::Leave("instance".into(), None)).unwrap();
    m.game_events(&mut Log::default());
    assert!(m.fight_end.armed.is_none(), "the game left the fight's scene");
    // ... and ends a hold from the fade
    m.fight_end_tick(T);
    m.fight_end_look(&black(), T, true);
    m.fight_end_look(&black(), T + 0.03, true);
    assert!(m.hold.active());
    tx.send(Event::Enter("home".into(), None)).unwrap();
    m.game_events(&mut Log::default());
    assert!(!m.hold.active() && m.fight_end.held.is_none());
    // a two-boss fight: its first boss on its last bar opens nothing (its partner comes in the same run); the second does
    let mut m = meter("fe-two-boss");
    let mut f = Fight::new(&m.env, "First", 60, row("80000002", "First"), 1_000_000, true, false, Some(row("80000003", "Second")), Some(1));
    f.hit = true;
    f.seg = Some(1);
    f.seg_fill = 0.02;
    m.fight = Some(f);
    m.bar_seen = T;
    m.fight_end_tick(T);
    assert!(m.fight_end.armed.is_none(), "the first of two bosses going down is no end");
    let mut f = m.fight.take().unwrap();
    f.next_boss(&m.env, row("80000003", "Second"), "test");
    f.seg = Some(1);
    f.seg_fill = 0.02;
    m.fight = Some(f);
    m.fight_end_tick(T);
    assert!(m.fight_end.armed.is_some(), "the second boss's last bar is the end");
}
