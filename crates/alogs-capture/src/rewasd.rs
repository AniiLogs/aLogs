//! reWASD (a controller remapper): while aLogs has the Holo-Battle Sim's end, the controller is switched to a reWASD
//! slot with nothing mapped, and back after, with reWASD's own command line. Set up in <data>/rewasd.json:
//! {"id": "<device ID>", "slot": "slot1", "hold_slot": "slot4"}. Port of aLogs B16 `bossbar/rewasd.py`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const SLOTS: [&str; 4] = ["slot1", "slot2", "slot3", "slot4"];
static HELD: AtomicBool = AtomicBool::new(false);

pub fn exe() -> PathBuf {
    PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into())).join("reWASD").join("reWASDCommandLine.exe")
}

#[derive(Clone, Debug)]
pub struct Config {
    pub id: String,
    pub slot: String,
    pub hold_slot: String,
}

/// The setup from <data>/rewasd.json, or None when it's missing, incomplete or reWASD isn't installed.
pub fn config(data: &Path) -> Option<Config> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(data.join("rewasd.json")).ok()?).ok()?;
    let s = |k: &str| match v.get(k) {
        Some(serde_json::Value::String(x)) if !x.is_empty() => Some(x.clone()),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    };
    let (id, slot, hold) = (s("id")?, s("slot")?, s("hold_slot")?);
    if !SLOTS.contains(&slot.as_str()) || !SLOTS.contains(&hold.as_str()) || slot == hold || !exe().is_file() {
        return None;
    }
    Some(Config { id, slot, hold_slot: hold })
}

fn select(device: String, slot: String, why: &'static str) {
    let mut cmd = std::process::Command::new(exe());
    cmd.args(["select_slot", "--id", &device, "--slot", &slot]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    match cmd.output() {
        Ok(r) => {
            let text = if r.stdout.is_empty() { r.stderr } else { r.stdout };
            let out: String = String::from_utf8_lossy(&text).trim().replace('\n', " ").chars().take(160).collect();
            let code = r.status.code().unwrap_or(-1);
            log::info!(target: "meter", "reWASD: {slot} selected ({why}){}{}", if code == 0 { String::new() } else { format!(" - exit {code}") },
                if out.is_empty() { String::new() } else { format!(": {out}") });
        }
        Err(e) => log::info!(target: "meter", "reWASD: {slot} not selected ({why}): {e}"),
    }
}

/// Switch the controller to the empty slot (on) or back to its own (off), without waiting for reWASD.
pub fn hold(c: Option<&Config>, on: bool) -> bool {
    let Some(c) = c else { return false };
    if HELD.load(Ordering::SeqCst) == on {
        return false;
    }
    HELD.store(on, Ordering::SeqCst);
    let (id, slot) = (c.id.clone(), if on { c.hold_slot.clone() } else { c.slot.clone() });
    std::thread::spawn(move || select(id, slot, if on { "aLogs has control" } else { "control given back" }));
    true
}
