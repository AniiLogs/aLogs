//! The AniiLogs website inside the app: a second webview laid over the content area of the main window (the app's own
//! page draws the tabs and leaves that area empty). It is the live site in its app view (`?embed=app`), so it always
//! looks and behaves exactly like aniilogs.github.io with nothing to update here. Discord sign-in happens inside it;
//! links that leave AniiLogs open in the default browser.

use std::os::windows::process::CommandExt;
use std::process::Command;
use tauri::webview::{NewWindowResponse, WebviewBuilder};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl};

const LABEL: &str = "site";
const SITE: &str = "https://aniilogs.github.io";
const NO_WINDOW: u32 = 0x0800_0000;
/// The page last asked for: showing the same tab again keeps the page as the user left it (scroll, sign-in, filters).
static LAST_PAGE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
/// One call at a time: opening the page and the frame's first resize both ask for the view at once, and two
/// creations would race ("a webview with label `site` already exists").
static BUSY: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Pages the site view may show itself: the site, its API (sign-in round trip) and Discord's sign-in pages.
pub(crate) fn stays_inside(url: &Url) -> bool {
    match url.host_str() {
        Some(host) => url.scheme() == "https"
            && (host == "aniilogs.github.io" || host == "aniilogs-api.pages.dev" || host == "discord.com" || host.ends_with(".discord.com")),
        None => url.scheme() == "about",
    }
}

pub(crate) fn open_outside(url: &Url) {
    if url.scheme() == "https" || url.scheme() == "http" {
        let _ = Command::new("rundll32").args(["url.dll,FileProtocolHandler", url.as_str()]).creation_flags(NO_WINDOW).spawn();
    }
}

/// Show the site at `path` (e.g. "/leaderboard/") in the given part of the window (logical pixels, from the page),
/// or hide it when `path` is None. `theme` is the app's theme, passed on so the site matches the app; `uid` is the
/// character the game is logged into, so the map and checklists show that character's progress.
/// Async on purpose: a synchronous command runs on the main thread, and creating a webview there deadlocks on Windows
/// (the view stayed blank at about:blank, ignored hide/resize, and covered the page so nothing could be clicked).
#[tauri::command]
pub async fn site_view(app: AppHandle, path: Option<String>, theme: Option<String>, uid: Option<String>, auth: Option<String>, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
    let _one_at_a_time = BUSY.lock().unwrap_or_else(|e| e.into_inner());
    let existing = app.get_webview(LABEL);
    let Some(path) = path else {
        if let Some(view) = existing {
            view.hide().map_err(|e| e.to_string())?;
        }
        return Ok(());
    };
    if !path.starts_with('/') {
        return Err("Only AniiLogs pages can be shown here.".into());
    }
    let mut url: Url = format!("{SITE}{path}").parse().map_err(|e: url::ParseError| e.to_string())?;
    url.query_pairs_mut().append_pair("embed", "app");
    if let Some(theme) = theme.filter(|t| t.chars().all(|c| c.is_ascii_alphanumeric()) && !t.is_empty()) {
        url.query_pairs_mut().append_pair("theme", &theme);
    }
    let uid = uid.filter(|u| !u.is_empty() && u.len() <= 24 && u.chars().all(|c| c.is_ascii_digit()));
    if let Some(uid) = &uid {
        url.query_pairs_mut().append_pair("uid", uid);
    }
    // signed in with Discord in the app: the page signs itself in from this one-time handoff in its #fragment
    let auth = auth.filter(|a| a.len() >= 32 && a.len() <= 256 && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    if let Some(auth) = &auth {
        url.set_fragment(Some(&format!("aniilogs_auth={auth}")));
    }
    let page = format!("{path}#{}{}", uid.as_deref().unwrap_or(""), if auth.is_some() { "#auth" } else { "" });    // another character, or a sign-in, reloads the page
    let (position, size) = (LogicalPosition::new(x, y), LogicalSize::new(w.max(1.0), h.max(1.0)));
    if let Some(view) = existing {
        view.set_position(position).map_err(|e| e.to_string())?;
        view.set_size(size).map_err(|e| e.to_string())?;
        // a tab change navigates; showing the same tab again keeps the page as it was left
        let mut last = LAST_PAGE.lock().unwrap_or_else(|e| e.into_inner());
        if *last != page {
            view.navigate(url).map_err(|e| e.to_string())?;
            *last = page;
        }
        view.show().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let window = app.get_window("main").ok_or("The main window is gone.")?;
    let builder = WebviewBuilder::new(LABEL, WebviewUrl::External(url))
        .on_navigation(|url| {
            if stays_inside(url) {
                true
            } else {
                open_outside(url);
                false
            }
        })
        .on_new_window(|url, _features| {
            open_outside(&url);
            NewWindowResponse::Deny
        });
    window.add_child(builder, position, size).map_err(|e| e.to_string())?;
    *LAST_PAGE.lock().unwrap_or_else(|e| e.into_inner()) = page;
    Ok(())
}
