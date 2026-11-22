//! The game's version from Player.log's head (made-up numbers).

use alogs_gamelog::{GAME_BUILD_HEAD, GameBuild, game_build, game_build_in};
use std::path::PathBuf;

const VERSION: &str = "[17.8][Log]|PrintGameVersionInfo: deploy[xwinpublish], scm[3900001], res[1], lua[1], ucs[1]";
const PATCH: &str = "[48.1][Log]|MatchLuaVerList: Matched cliVer[1.1.3900001], idx[1], cond[lt(v, '1.2')], act[TipPatch], res[3900777], url[], review[], reviewServerGroup[], clientconfig[]";

fn gb(build: i64, patch: Option<i64>, action: Option<&str>) -> Option<GameBuild> {
    Some(GameBuild { build, patch, patch_action: action.map(String::from) })
}

fn temp(name: &str, bytes: &[u8]) -> PathBuf {
    let p = std::env::temp_dir().join(format!("alogs-gamebuild-{}-{name}.log", std::process::id()));
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn both_lines() {
    let log = format!("[0.1][Log]|start\n{VERSION}\n[20.0][Log]|other\n{PATCH}\n[60.0][Log]|later\n");
    assert_eq!(game_build_in(log.as_bytes()), gb(3900001, Some(3900777), Some("TipPatch")));
}

#[test]
fn only_the_version_line() {
    let log = format!("[0.1][Log]|start\n{VERSION}\n[60.0][Log]|later\n");
    assert_eq!(game_build_in(log.as_bytes()), gb(3900001, None, None));
}

#[test]
fn neither_line() {
    assert_eq!(game_build_in(b"[0.1][Log]|start\n[60.0][Log]|later scm[3900001]\n"), None);
    assert_eq!(game_build_in(b""), None);
}

#[test]
fn patch_line_before_the_version_line_is_ignored() {
    let log = format!("{PATCH}\n{VERSION}\n");
    assert_eq!(game_build_in(log.as_bytes()), gb(3900001, None, None));
}

#[test]
fn last_version_line_and_last_patch_line_after_it() {
    let older = VERSION.replace("3900001", "3800001");
    let p2 = PATCH.replace("3900777", "3900888").replace("TipPatch", "ForcePatch");
    let log = format!("{older}\n{PATCH}\n{VERSION}\n{PATCH}\n{p2}\n");
    assert_eq!(game_build_in(log.as_bytes()), gb(3900001, Some(3900888), Some("ForcePatch")));
}

#[test]
fn garbage_values() {
    for bad in ["scm[]", "scm[0]", "scm[-5]", "scm[+5]", "scm[39x01]", "scm[1000000000001]", "scm[99999999999999999999999]", "scm[ 3900001]"] {
        let log = VERSION.replace("scm[3900001]", bad);
        assert_eq!(game_build_in(log.as_bytes()), None, "{bad}");
    }
    let top = VERSION.replace("3900001", "1000000000000");
    assert_eq!(game_build_in(top.as_bytes()), gb(1_000_000_000_000, None, None));
    for bad in ["res[]", "res[0]", "res[abc]", "res[1000000000001]"] {
        let log = format!("{VERSION}\n{}\n", PATCH.replace("res[3900777]", bad));
        assert_eq!(game_build_in(log.as_bytes()), gb(3900001, None, Some("TipPatch")), "{bad}");
    }
}

#[test]
fn action_with_odd_characters_is_left_out() {
    for bad in ["act[]", "act[Tip Patch]", "act[Tip_Patch]", "act[TipPatch1]", "act[Tïp]", "act[ABCDEFGHIJKLMNOPQRSTUVWXY]"] {
        let log = format!("{VERSION}\n{}\n", PATCH.replace("act[TipPatch]", bad));
        assert_eq!(game_build_in(log.as_bytes()), gb(3900001, Some(3900777), None), "{bad}");
    }
    let log = format!("{VERSION}\n{}\n", PATCH.replace("act[TipPatch]", "act[ABCDEFGHIJKLMNOPQRSTUVWX]"));
    assert_eq!(game_build_in(log.as_bytes()), gb(3900001, Some(3900777), Some("ABCDEFGHIJKLMNOPQRSTUVWX")));
}

#[test]
fn crlf_and_invalid_utf8() {
    let log = format!("[0.1][Log]|start\r\n{VERSION}\r\n{PATCH}\r\n");
    assert_eq!(game_build_in(log.as_bytes()), gb(3900001, Some(3900777), Some("TipPatch")));
    let mut raw = b"\xff\xfe junk \xc3\x28\n".to_vec();
    raw.extend_from_slice(VERSION.as_bytes());
    raw.extend_from_slice(b"\r\n\x80\x81\n");
    raw.extend_from_slice(PATCH.as_bytes());
    raw.extend_from_slice(b"\xff\r\n");
    assert_eq!(game_build_in(&raw), gb(3900001, Some(3900777), Some("TipPatch")));
}

#[test]
fn missing_file() {
    assert_eq!(game_build(&std::env::temp_dir().join("alogs-gamebuild-no-such-file.log")), None);
    assert_eq!(game_build(&PathBuf::new()), None);
}

#[test]
fn reads_only_the_head() {
    let filler = |n: usize| {
        let mut f = Vec::with_capacity(n);
        while f.len() < n {
            f.extend_from_slice(b"[1.0][Log]|filler line\n");
        }
        f.truncate(n);
        f
    };
    let cap = GAME_BUILD_HEAD as usize;
    // inside the cap, the file larger than it
    let mut inside = filler(cap - 4096);
    inside.extend_from_slice(format!("\n{VERSION}\n{PATCH}\n").as_bytes());
    inside.extend(filler(cap));
    let p = temp("inside", &inside);
    assert_eq!(game_build(&p), gb(3900001, Some(3900777), Some("TipPatch")));
    // the version line inside, the patch line past the cap
    let mut split = filler(cap - 4096);
    split.extend_from_slice(format!("\n{VERSION}\n").as_bytes());
    split.extend(filler(8192));
    split.extend_from_slice(format!("\n{PATCH}\n").as_bytes());
    let p2 = temp("split", &split);
    assert_eq!(game_build(&p2), gb(3900001, None, None));
    // both past the cap
    let mut outside = filler(cap + 10);
    outside.extend_from_slice(format!("\n{VERSION}\n{PATCH}\n").as_bytes());
    let p3 = temp("outside", &outside);
    assert_eq!(game_build(&p3), None);
    // a newer version line past the cap doesn't count: the head's own is the running game's
    let mut later = format!("{VERSION}\n").into_bytes();
    later.extend(filler(cap + 10));
    later.extend_from_slice(format!("\n{}\n", VERSION.replace("3900001", "3900002")).as_bytes());
    let p4 = temp("later", &later);
    assert_eq!(game_build(&p4), gb(3900001, None, None));
    for p in [p, p2, p3, p4] {
        let _ = std::fs::remove_file(p);
    }
}
