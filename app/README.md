# aLogs: Aniiloger's Companion App (desktop app)

The suite's desktop app. "aLogs" is its short name: the program, install and data folders, and the updater use it
(`../crates/alogs-app/tauri.conf.json`, `../crates/alogs-app/src/main.rs` `APP_DIR`,
`../crates/alogs-protocol/src/settings.rs` `APP_DIR`, `../crates/alogs-app/installer-hooks.nsh`), so it must not
change; the full name is what people see (window, About, installer, releases).

This folder holds the scripts that build and publish it, its patch notes (`CHANGELOG.md`), the beta bookkeeping
(`betas.json`) and the native OCR helper (`ocr-helper/`). The app itself is in the Rust workspace:

- **Tauri 2** shell (Rust, `../crates/alogs-app`) with a plain HTML/JS interface (`../crates/alogs-app/ui`) — WebView2
  comes with Windows.
- Runs the boss DPS meter as a helper process (`resources/meter/alogs-meter.exe`, built from `../crates/alogs-meter`,
  with the data files in `../crates/alogs-meter/res`).
- Menus: Dashboard (live status), Meter & overlay, Results (auto View Battle Data, input hold, controller),
  Captures & logs, Leaderboards & account (opt-in submissions, desktop token, character link), General, About.
- First run asks whether to submit to the leaderboards (off by default).
- Tray icon; closing the window keeps it in the tray (setting); optional start with Windows.

The app and the meter share `%LOCALAPPDATA%\aLogs\meter\settings.json`; the meter writes `status.json` there for
the dashboard, plus all logs and records under `logs\`.

## Build

Betas and releases are built from the Rust workspace (`../crates`, the app in `../crates/alogs-app`); the scripts
here drive it (`paths.mjs` says where everything is):

```
npm install
npm run build:rc       # a test build: OCR helper, meter, Sanctum Helper, app + NSIS installer, signed for the updater
npm run build          # the same after bumping the version (bump-version.mjs)
```

Output: `../target/release/bundle/nsis/aLogs_<version>_RC<n>_x64-setup.exe` and its `.sig`, and the build's map in
`../target/release/bundle/maps/`. Two PCs build betas, so the RC number is one past the highest of this PC's last
build of that version (and the B16 app's counter in `src-tauri/target/release/bundle/rc-<version>.txt`, left in a
checkout from before that app left the repository), the RC `betas.json`
records as published, and the one the beta feed announces (`rc-number.mjs`, tested by `npm test`); the beta number is
one past `betas.json`. `npm run beta` refuses a build whose RC the feed already has, and records the new RC in
`betas.json` with the beta number.

## Release

Write the version's patch notes in `CHANGELOG.md` first (a `## <version>` section, for players), then:

```
npm run release -- --dry-run   # show the notes and the installer that would go up
npm run release                # publish companion-v<version> as the latest GitHub Release
```

The Companion page on the site downloads `releases/latest/download/AniiLogs-Companion-Setup.exe`, so the newest
release is what players get. `npm run release -- --notes-only` rewrites an existing release's notes.

The installer is per-user (no admin). Controller blocking uses HidHide's signed driver (Nefarius, MIT; the driver
files are bundled from `../crates/alogs-app/vendor/hidhide/`): the app installs just the driver when you press Results > Controller >
Set up (one admin prompt; no HidHide program, shortcut or Programs entry) and the uninstaller removes it again.
