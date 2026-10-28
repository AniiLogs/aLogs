//! "Sign in with Discord" for the app: Discord's page opens in the player's own browser (where they are usually
//! signed in already), never inside the app. When they approve, the API sends that browser back to this app on
//! 127.0.0.1 with a one-time code. This module only listens for that one visit; the page trades the code for a
//! desktop token, together with a secret only it holds (PKCE), so the code is useless to anyone else.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static LISTENER: Mutex<Option<TcpListener>> = Mutex::new(None);
/// Set by "Cancel sign-in": the wait stops (it held the sign-in button for up to 5 minutes after a closed tab).
static CANCELLED: AtomicBool = AtomicBool::new(false);
const WAIT: Duration = Duration::from_secs(5 * 60);

/// Starts listening on a free local port (only reachable from this PC); returns the port for the sign-in link.
#[tauri::command]
pub fn auth_listen_start() -> Result<u16, String> {
    CANCELLED.store(false, Ordering::SeqCst);
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    *LISTENER.lock().unwrap_or_else(|e| e.into_inner()) = Some(listener);
    Ok(port)
}

/// Stops a sign-in that's waiting for the browser.
#[tauri::command]
pub fn auth_listen_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

/// Waits (up to 5 minutes) for the browser to come back, answers it with a "you can go back to aLogs" page, and
/// returns the one-time code. Async, so the window stays responsive while the player is in their browser.
#[tauri::command]
pub async fn auth_listen_wait() -> Result<String, String> {
    let listener = LISTENER.lock().unwrap_or_else(|e| e.into_inner()).take().ok_or("Sign-in wasn't started.")?;
    let started = Instant::now();
    while started.elapsed() < WAIT {
        if CANCELLED.load(Ordering::SeqCst) {
            return Err("Sign-in cancelled.".into());
        }
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(150));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        let _ = BufReader::new(&stream).read_line(&mut line);
        // "GET /callback?code=...&error=... HTTP/1.1"; anything else (a favicon request) is turned away
        let target = line.split_whitespace().nth(1).unwrap_or("");
        let Some(query) = target.strip_prefix("/callback?") else {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        };
        let param = |name: &str| {
            query.split('&').find_map(|pair| pair.strip_prefix(&format!("{name}=")).map(|v| v.to_string()))
        };
        let (code, error) = (param("code"), param("error"));
        let ok = code.as_deref().map_or(false, |c| c.len() >= 32 && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
        let message = if ok { "You're signed in. You can close this tab and go back to aLogs." } else { "Sign-in was cancelled. You can close this tab." };
        let page = format!(
            "<!doctype html><meta charset=\"utf-8\"><title>aLogs</title><body style=\"font:16px system-ui;background:#0b1215;color:#f4f4e9;display:grid;place-items:center;height:90vh\"><p>{message}</p></body>"
        );
        let _ = stream.write_all(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{page}",
            page.len()
        ).as_bytes());
        return match (ok, error) {
            (true, _) => Ok(code.unwrap_or_default()),
            (false, Some(_)) => Err("Sign-in was cancelled in the browser.".into()),
            _ => Err("The browser came back without a sign-in code.".into()),
        };
    }
    Err("Sign-in timed out (5 minutes). Try again.".into())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    // the browser coming back: a favicon request is turned away, the callback hands over the code
    #[test]
    fn browser_comes_back() {
        let port = super::auth_listen_start().unwrap();
        let visit = move |path: &'static str| std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).unwrap();
            let mut out = String::new(); let _ = s.read_to_string(&mut out); out
        });
        let fav = visit("/favicon.ico");
        let cb = visit("/callback?code=abcdefghijklmnopqrstuvwxyz0123456789-_AB");
        let code = tauri::async_runtime::block_on(super::auth_listen_wait()).unwrap();
        assert_eq!(code, "abcdefghijklmnopqrstuvwxyz0123456789-_AB");
        let (fav, cb) = (fav.join().unwrap(), cb.join().unwrap());
        assert!(fav.starts_with("HTTP/1.1 404") || cb.contains("signed in"), "{fav} / {cb}");
        // a cancelled sign-in
        let port = super::auth_listen_start().unwrap();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /callback?error=cancelled HTTP/1.1\r\n\r\n").unwrap();
        });
        assert!(tauri::async_runtime::block_on(super::auth_listen_wait()).unwrap_err().contains("cancelled"));
    }
}
