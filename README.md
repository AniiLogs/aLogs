# aLogs: Aniiloger's Companion App

An unofficial companion app for [Aniimo](https://aniilogs.github.io): a team DPS meter read from the boss HP bar,
automatic capture of result and Battle Data screens, Sanctum Helper, and optional
[AniiLogs](https://aniilogs.github.io) leaderboard submissions. It only reads the screen and the game's own text log;
it never touches the game's memory or files.

**Download:** the latest release is on the [AniiLogs releases page](https://github.com/AniiLogs/aniilogs.github.io/releases)
(the app updates itself after that). Patch notes are in [app/CHANGELOG.md](app/CHANGELOG.md).

## What's here

| Folder | What it is |
|---|---|
| [`app/`](app) | The desktop app: Tauri 2 (Rust, `src-tauri/`) with a plain HTML/JS interface (`ui/`), its self-updater, the native OCR helper (`ocr-helper/`) and the build and publish scripts |
| [`bossbar/`](bossbar) | The boss DPS meter (Python): screen reading, fight tracking, overlay, screen readers and leaderboard sending; packaged into the app with PyInstaller |
| [`crates/`](crates) | The Rust rewrite, in progress: the app, the meter and Sanctum Helper as one Cargo workspace (see [The Rust rewrite](#the-rust-rewrite)). Betas and releases are still built from `app/` and `bossbar/` |

Sanctum Helper is a separate project, [sargeeva/SanctumHelper](https://github.com/sargeeva/SanctumHelper): to build
the full app, check it out next to `app/` and `bossbar/` as `SanctumHelper/`.

## Building

Windows, with Node.js, Rust, Python 3.12 (PyInstaller, Pillow) and the .NET 9 SDK:

```
cd app
npm install
npm run build:rc      # a test build: meter, OCR helper, Sanctum Helper, app and installer
```

Builds are signed for the self-updater with a key kept outside the project (see `app/tauri-build.mjs`).

## The Rust rewrite

A Rust port of aLogs 0.1.37 B16 and the Sanctum Helper it bundles lives in [`crates/`](crates), with the workspace's
`Cargo.toml` and `Cargo.lock` at the top of the repository. It sits beside the app above and replaces nothing yet. The
plan, the reference refs and how parity is checked are in [docs/rewrite-plan.md](docs/rewrite-plan.md).

```
git clone https://github.com/AniiLogs/aLogs reference/aLogs && git -C reference/aLogs checkout e340b39
git clone https://github.com/AniiLogs/SanctumHelper reference/SanctumHelper && git -C reference/SanctumHelper checkout 8cd1c6a
cargo test
python -I tools/gen_hpcalc_goldens.py     # rewrites the hpcalc goldens from the reference
```

`tools/package-preview.ps1` builds a preview installer from the workspace, without update signing.

## Fixing reported problems

Players send their logs from the app (DPS Meter › Captures & logs › Send logs). Developers read them with
`node tools/log-reports.mjs` and fix the cause in a pull request: see [docs/log-reports.md](docs/log-reports.md),
including the rules that report contents never go into the repository, issues or PRs, and that every report you work
on gets a note.

## License

[MIT](LICENSE), except the third-party parts named in it.
