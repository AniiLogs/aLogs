//! SanctumHelper.exe --hosted end to end, as the aLogs app drives it: info.json and status.json appear, a
//! render-layout command makes its picture, quit closes it. Settings in a temp folder, a made-up Player.log, a window
//! title nothing has. Skipped without reference/.

use std::path::PathBuf;
use std::time::{Duration, Instant};

fn wait_for(p: &std::path::Path, secs: u64) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if p.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn hosted_mode_answers_the_app() {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/SanctumHelper/app/SanctumHelper/Assets");
    if !assets.exists() {
        eprintln!("skipped: reference/ not cloned");
        return;
    }
    let dir = std::env::temp_dir().join(format!("alogs-hosted-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Player.log"), "made-up line\n").unwrap();
    std::fs::write(dir.join("settings.json"), r#"{"WindowTitle":"no window is called this 9c1e"}"#).unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_SanctumHelper"))
        .args(["--hosted", "--assets"])
        .arg(&assets)
        .arg("--player-log")
        .arg(dir.join("Player.log"))
        .env("SANCTUM_DIR", &dir)
        .spawn()
        .unwrap();
    let hosted = dir.join("hosted");
    let ok_status = wait_for(&hosted.join("status.json"), 20);
    std::fs::write(hosted.join("cmd-1-1.json"), r#"{"command":"render-layout","layout":53,"width":640,"height":400}"#).unwrap();
    let ok_render = wait_for(&hosted.join("layout-53.png"), 10);
    let status = std::fs::read_to_string(hosted.join("status.json")).unwrap_or_default();
    let info = std::fs::read_to_string(hosted.join("info.json")).unwrap_or_default();
    std::fs::write(hosted.join("cmd-2-1.json"), r#"{"command":"quit"}"#).unwrap();
    let t = Instant::now();
    let mut exited = None;
    while t.elapsed() < Duration::from_secs(10) {
        if let Ok(Some(s)) = child.try_wait() {
            exited = Some(s);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if exited.is_none() {
        let _ = child.kill();
    }
    std::fs::remove_dir_all(&dir).ok();
    assert!(ok_status, "no status.json");
    assert!(ok_render, "no layout picture");
    let st: serde_json::Value = serde_json::from_str(&status).unwrap();
    assert_eq!(st["gameFound"], false);
    assert_eq!(st["keyName"], "Left Ctrl");
    let inf: serde_json::Value = serde_json::from_str(&info).unwrap();
    assert!(inf["layouts"].as_array().is_some_and(|l| l.len() > 20));
    assert!(exited.is_some_and(|s| s.success()), "didn't quit on the command");
}
