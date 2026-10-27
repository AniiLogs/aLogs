# aLogs: Aniiloger's Companion App (desktop app)

The suite's desktop app. "aLogs" is its short name: the program, install and data folders, and the updater use it
(`src-tauri/tauri.conf.json`, `src-tauri/src/main.rs` `APP_DIR`, `bossbar/settings.py` `APP_DIR`,
`installer-hooks.nsh`), so it must not change; the full name is what people see (window, About, installer, releases).

- **Tauri 2** shell (Rust, `src-tauri/`) with a plain HTML/JS interface (`ui/`) — WebView2 comes with Windows.
- Runs the boss DPS meter as a helper process (`resources/meter/meter.exe`, packaged from `../bossbar`).
- Menus: Dashboard (live status), Meter & overlay, Results (auto View Battle Data, input hold, controller),
  Captures & logs, Leaderboards & account (opt-in submissions, desktop token, character link), General, About.
- First run asks whether to submit to the leaderboards (off by default).
- Tray icon; closing the window keeps it in the tray (setting); optional start with Windows.

The app and the meter share `%LOCALAPPDATA%\aLogs\meter\settings.json`; the meter writes `status.json` there for
the dashboard, plus all logs and records under `logs\`.

## Build

```
npm install
npm run build          # packages the meter (PyInstaller), then builds the app + NSIS installer
```

Output: `src-tauri/target/release/bundle/nsis/aLogs_<version>_x64-setup.exe`.

## Release

Write the version's patch notes in `CHANGELOG.md` first (a `## <version>` section, for players), then:

```
npm run release -- --dry-run   # show the notes and the installer that would go up
npm run release                # publish companion-v<version> as the latest GitHub Release
```

The Companion page on the site downloads `releases/latest/download/AniiLogs-Companion-Setup.exe`, so the newest
release is what players get. `npm run release -- --notes-only` rewrites an existing release's notes.

The installer is per-user (no admin). Controller blocking uses HidHide's signed driver (Nefarius, MIT; the driver
files are bundled from `bossbar/vendor/hidhide/`): the app installs just the driver when you press Results > Controller >
Set up (one admin prompt; no HidHide program, shortcut or Programs entry) and the uninstaller removes it again.
