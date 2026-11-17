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
| [`crates/`](crates) | The app, the meter and Sanctum Helper in Rust, one Cargo workspace (see [The Rust rewrite](#the-rust-rewrite)): betas and releases are built from it. With them: the data files the meter ships (boss list, Interlink tables, xN digits: `alogs-meter/res/`), the list of plug-ins (`alogs-protocol/plugins.json`), the app's UI and its pictures (`alogs-app/ui/`) and HidHide's driver (`alogs-app/vendor/`) |
| [`app/`](app) | The build and publish scripts, the patch notes (`CHANGELOG.md`) and the native OCR helper (`ocr-helper/`) |
| [`tools/`](tools) | Log reports, preview builds, and the generators of the Rust tests' goldens (they run against the reference checkouts) |

Sanctum Helper is a separate project, [sargeeva/SanctumHelper](https://github.com/sargeeva/SanctumHelper). aLogs ships
its Rust port (`crates/alogs-sanctum`), but its map art comes from the project itself: to build the full app, check it
out next to `app/` and `crates/` as `SanctumHelper/` (git-ignored; the game's art is never committed).

## Building

Windows, with Node.js and Rust (stable, MSVC). No Python or .NET SDK: everything shipped is built by Cargo (Python
only runs the golden generators in `tools/`).

```
cd app
npm install
npm run build:rc      # a test build: OCR helper, meter, Sanctum Helper, app and installer
```

It writes the About page's patch notes from `app/CHANGELOG.md`, checks the shipped boss list and Interlink HP bars
against the installed game (`npm run check:data`: a difference stops the build), builds the OCR helper and the
workspace's meter and Sanctum Helper, lays out what the installer carries in `crates/alogs-app/resources`
(`app/resources.mjs`), then builds the app and its installer: `target/release/bundle/nsis/aLogs_<version>_RC<n>_x64-setup.exe`.
Builds are signed for the self-updater with a key kept outside the project (see `app/tauri-build.mjs`). Without the
key, `tools/package-preview.ps1` builds the same thing as an unsigned preview (RC 9000, so published builds of the
same version don't replace it).

## The Rust rewrite

A Rust port of aLogs 0.1.37 B16 and the Sanctum Helper it bundles lives in [`crates/`](crates), with the workspace's
`Cargo.toml` and `Cargo.lock` at the top of the repository. Betas are built from it since 17 Nov 2026 (see
[Building](#building)), and the B16 app it was ported from (the Python meter in `bossbar/`, the app in
`app/src-tauri/` and `app/ui/`) left the repository the same day: it lives on in the history and in the reference
checkout below. The plan, the reference refs and how parity is checked are in [docs/rewrite-plan.md](docs/rewrite-plan.md).

```
git clone https://github.com/AniiLogs/aLogs reference/aLogs && git -C reference/aLogs checkout e340b39
git clone https://github.com/AniiLogs/SanctumHelper reference/SanctumHelper && git -C reference/SanctumHelper checkout 8cd1c6a
cargo test --workspace                    # what GitHub runs; without reference/ the parity tests skip themselves
python -I tools/gen_hpcalc_goldens.py     # rewrites the hpcalc goldens from the reference
```

## Fixing reported problems

Players send their logs from the app (DPS Meter › Captures & logs › Send logs). Developers read them with
`node tools/log-reports.mjs` and fix the cause in a pull request: see [docs/log-reports.md](docs/log-reports.md),
including the rules that report contents never go into the repository, issues or PRs, and that every report you work
on gets a note.

## License

[MIT](LICENSE), except the third-party parts named in it.
