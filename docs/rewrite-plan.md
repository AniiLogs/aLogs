# aLogs in Rust: rewrite plan

Local only: no GitHub remote until the maintainer and sargeeva approve.

## Reference (what we're matching)

| Repo | Ref | Why |
| --- | --- | --- |
| AniiLogs/aLogs | `e340b39` (detached), version 0.1.37 | B16 is the pre-release "aLogs 0.1.37 B16" on AniiLogs/aniilogs.github.io (`companion-v0.1.37-beta`, installer uploaded 2026-11-16 01:04 UTC). aLogs has no releases or tags of its own. `app/betas.json` reads `"0.1.37": 15` at e340b39, so `tauri-build.mjs` built it as beta 16. The next commit, `7788616` "0.1.37 B16 published", changes only the counter. |
| AniiLogs/SanctumHelper | `8cd1c6a` on `alogs-integration` (detached), version 1.0.0 | Nothing pins it. `app/build-sanctum.ps1` publishes whatever `../SanctumHelper` holds. `main` (84c1d28) has no `--hosted` mode and no `RedrawCap`, which B16's `sanctum.rs` relies on. `alogs-integration` has both, and 8cd1c6a is its last commit before the B16 build. |

Clones: `reference/aLogs`, `reference/SanctumHelper` (git-ignored here, never committed).

## What the original is

aLogs is three programs and a web UI that talk through files, shared memory and child processes:

| Part | Language, size | Code | What it does |
| --- | --- | --- | --- |
| App shell | Rust (Tauri 2), ~9.6k lines | `app/src-tauri/src/*.rs` | Window, tray, settings, starts the meter and Sanctum Helper, screen capture (GDI + Windows.Graphics.Capture), shared capture `Local\aLogs.Capture.v2`, button over the game, audio cues (WASAPI + FFT), controller blocking (HidHide), photos, Discord sign-in, self-updater (signed GitHub feeds, minisign), watchdog, Dev menu |
| UI | HTML/JS, ~4k lines | `app/ui/` | Pages for every plug-in, Beta and Dev menus, updater and installer windows |
| OCR helper | Rust, 305 lines | `app/ocr-helper/` | Windows.Media.Ocr plus two ONNX name models (tract) over stdin/stdout |
| Meter ("bossbar") | Python 3.12 + Pillow, ~13k lines, PyInstaller | `bossbar/` | Core reading loop (`live.py`, `core.py`), screen readers (`reader.py`, `xn_digits.py`, `namereader.py`, `gamefont.py`, `dmgnumbers.py`), fights (`fights.py`), end screens (`results.py`, `eggheist.py`), game data and HP math (`gamedata.py`, `hpcalc.py`), overlays and notices (`dps_overlay.py`, `notice_window.py`, `layered.py`), leaderboards (`leaderboards.py`, `submit.py`), settings, Dev switches, plug-ins |
| Sanctum Helper | C# .NET 9 WPF + OpenCvSharp, ~10.4k lines | `SanctumHelper/app/SanctumHelper/` | Lost Sanctum map: vision (`Vision.cs`), engine (`Engine.cs`), overlay (`OverlayWindow.cs`), game log, chest sets, `--hosted` contract (`Hosted.cs`, `SharedCapture.cs`) |

The process and file contracts are written down in `reference/aLogs/docs/plugins.md` and `docs/shared-capture.md`:
`status.json`, `notices/`, `cmd-*.json`, `plugins.json`, the shared capture layout and Sanctum Helper's `--hosted` rules.
These contracts are what let us swap one program at a time.

## Approach

- **Keep the Tauri shell and the HTML/JS UI.** They're already Rust and HTML. Rewriting the UI would only add parity risk.
  The shell moves into the workspace as `alogs-app` once the meter is ported.
- **Port the Python meter first, then Sanctum Helper, one drop-in at a time.** Each new Rust exe keeps the old one's command
  line, files and shared-memory contracts, so the shipping B16 shell can run it in place of `bossbar.exe` or
  `SanctumHelper.exe`. We test it there before anything else changes.
- **Pure logic first, Windows I/O last.** Logic crates have no Windows dependencies and test on any OS.
- One process (meter and Sanctum Helper in the app) is an option after parity, not a goal.

## Workspace layout

```
alogs-rust/
  Cargo.toml                 workspace
  crates/
    alogs-gamedata/          hpcalc ✓, boss catalog, gamedata tables (boss_break, cycles, grades)
    alogs-protocol/          serde types for plugins.json, status.json, notices, cmd files, settings files
    alogs-gamelog/           the game's text log watcher (core/live + GameLog.cs share it)
    alogs-ocr/               Windows.Media.Ocr + ONNX name models (from ocr-helper), in-process
    alogs-capture/           GDI/WGC capture, shared capture v2 (writer and reader)
    alogs-vision/            image primitives: crops, thresholds, template/NCC match, game font
    alogs-reader/            boss bar, HP fill, xN digits, names, damage numbers, screen titles
    alogs-fights/            fight tracking (fights.py)
    alogs-results/           end-screen reads into runs (results.py, eggheist.py)
    alogs-leaderboards/      deciding and sending runs, proof pictures (leaderboards.py, submit.py)
    alogs-overlay/           layered windows: DPS window, HP labels, notices (dps_overlay, notice_window, layered)
    alogs-meter/  (bin)      the reading loop, settings, Dev switches: drop-in for bossbar.exe
    alogs-sanctum/ (bin)     Sanctum Helper engine, vision, overlay, --hosted: drop-in for SanctumHelper.exe
    alogs-app/    (bin)      the Tauri shell (moved in from app/src-tauri), with app/ui as-is
  tools/                     golden-file generators that run the reference Python
  reference/                 the B16 clones (git-ignored)
```

## Crate choices

| Need | Choice | Why |
| --- | --- | --- |
| Shell, UI, updater | `tauri` 2, `tauri-plugin-updater`, `minisign-verify` | What B16 already uses. Same signing keys and feeds, so updates keep working. |
| Windows APIs | `windows` 0.62 / `windows-sys` 0.61 | Same versions as B16. Covers WGC, D3D11, GDI, Media.Ocr, layered windows and HID. |
| Images | `image` + `imageproc`, own small NCC template match | Matches what Pillow does in the meter. Pure Rust; no OpenCV build. |
| Sanctum vision | Same as above, with `opencv` crate only as a fallback | Vision.cs uses a handful of OpenCV ops (threshold, matchTemplate, contours). Porting them keeps the build simple. |
| ONNX name models | `tract-onnx` | What ocr-helper already uses. The models move over as they are. |
| Overlays | Win32 layered windows (`UpdateLayeredWindow`) + `tiny-skia` + `cosmic-text` | Same technique as `layered.py` and the WPF overlay: click-through, per-pixel alpha, no GPU context, captured or hidden in recordings as today. |
| JSON, files | `serde`, `serde_json`, `notify` | The contracts are JSON files. `notify` replaces polling where the original polls. |
| HTTP (leaderboards, uploads) | `ureq` (blocking, rustls) | The meter is a loop with worker threads, not async. Small, no tokio. |
| Logging | `tracing` + rolling file | The "Send logs" reports read these files. Keep the line formats the report tools grep for. |
| Errors | `anyhow` in bins, `thiserror` in libs | |

## Migration order

1. **alogs-gamedata.** `hpcalc` ✓ (this pass), then boss catalog and the JSON/CSV tables. Pure functions, golden-tested.
2. **alogs-protocol.** Types for every file contract, round-trip tested against samples the reference writes.
3. **alogs-gamelog.** Parse and follow the game's text log. Fixtures from `bossbar/tests/log_watcher_same.py` and GameLog.cs.
4. **alogs-vision + alogs-reader.** Bar finding, fill, xN digits (`xn_templates.json`, `xn_sheet.png`), game font, names.
   Tested against the reference's synthetic pictures (`sample_pictures.py`, `bar_resolutions.py`, `resolutions.py`).
5. **alogs-fights, alogs-results.** Port `Meter.step()`, scenario-driven like `two_boss.py`, `sim_victory_*`, `egg_*`.
6. **alogs-ocr, alogs-capture.** Fold ocr-helper in-process. Add a shared capture reader so the meter reads B16's capture.
7. **alogs-meter.** Headless loop, `status.json`, notices out. **First drop-in:** B16 shell runs the Rust meter.
8. **alogs-overlay.** DPS window, HP labels, notice window.
9. **alogs-leaderboards.** Payloads byte-for-byte equal to `submit.run_payload` for the same run. A staging target
   for sends, never the live board while testing.
10. **alogs-app.** Move the shell in, start the Rust meter, drop PyInstaller and the Python build.
11. **alogs-sanctum.** Engine, vision, overlay, `--hosted`. **Second drop-in** for SanctumHelper.exe. Drop the .NET runtime
    from the installer.

## Checking parity

- **Golden files from the reference.** For each ported module, a script in `tools/` imports the reference Python (or runs
  the C# in a small harness) and writes inputs → outputs as JSON. Rust tests read it and must match:
  exactly for integers and strings, within 1e-9 relative for floats. The generator runs again when the reference ref changes.
- **The reference's own fixtures.** Shipped data such as `bossbar/boss_catalog.csv` (max HP at 10 levels for every boss)
  is read from `reference/` at test time, not copied in. Tests skip with a note if `reference/` is missing.
- **The reference's tests.** `bossbar/tests/*.py` (53 scripts) are scenario timelines through the real `Meter.step()`, with
  only the screen reads stubbed. We port them as Rust tests with the same timelines and checks, one per module as it lands.
  `.github/workflows/tests.yml` lists the four that need game fonts, test shots or the OCR helper. Those run only locally.
- **Side by side.** Once the meter is a drop-in, both meters run on the same shared capture (the Rust one read-only, quiet)
  and we diff their fight, result and run events over real sessions. Sanctum Helper gets the same treatment.
- **Never** players' logs, UIDs, names, tokens, game files or the signing key in this repo. Log reports stay in their
  git-ignored folder and are only quoted as numbers.

## Status

Ported, each with parity tests against goldens written by the B16 Python (tools/gen_*_goldens.py):

| Crate | From | Checked |
| --- | --- | --- |
| alogs-pyval | (Python values: dict keys, repr, json.dumps) | unit tests |
| alogs-gamedata | hpcalc.py, gamedata.py (pmdata.bin reader, schedule, boss list, grades, drift) | synthetic game data + 150 byte-flipped copies, shipped boss_catalog.csv |
| alogs-protocol | settings.py, plugins.py, devflags.py, notice files | goldens |
| alogs-gamelog | core.LogWatcher, last_uid, scene_of, interlink_score, same_name, difflib | 18k events from a made-up log |
| alogs-pil | the Pillow 12.3 operations the readers use | 846 cases bit-exact |
| alogs-reader | reader.py, xn_digits.py, hangulnames.py, namereader.py, icons.py matchers, gamefont field, OCR helper client | 26 synthetic bar frames, name boxes, parsers |
| alogs-capture | wincapture.py (shared capture reader, audio cues, window and input helpers) | stand-in app test |
| alogs-meter | fights.py (Fight, Timer, BreakTracker, FightsMixin), results.py (result screen, Battle Data, Holo-Battle Sim Victory screen, Egg Heist screens, Challenge Stats), eggheist.py, leaderboards.py (bests, sending runs, late Battle Data, the automatic Battle Data click), live.py screen readers (prep, Party screens and their pictures, Interlink star caps, menu, Profile, Link character), submit.py (payloads, sender), core Teammates/pills/signatures, Meter.step, the alogs-meter binary | scripted fights frame by frame, payloads, local API, screen scenarios with a stand-in OCR, eggheist goldens |

Known differences (documented where they are): Pillow's quantize (plain median cut here), text drawing (ab_glyph,
not FreeType), proof JPEG bytes (jpeg-encoder, not libjpeg), font/icon extraction from the game's Unity bundles (the
Python meter's cache is read).

Since then: the Holo-Battle Sim exit gate and Gizmo tour, input holds (alogs-capture input, focusguard, rewasd), presence,
pcstats (alogs-pcstats), diagnostics, housekeeping (cleanup, game-data refresh), the OCR language checks, the overlay
(gamefont.render bit-exact; layered windows; DPS window, HP labels, end notes, the alert, notices), the stop event and a
clean shutdown. **alogs-app** is moved in (crates/alogs-app, with its ui) and starts the Rust meter; pictures of the game
the UI shows are git-ignored. **alogs-sanctum** is started: settings, layouts and chest sets, the game log, the OpenCV subset Sanctum Helper uses (cv.rs, cv2.rs: byte-for-byte against OpenCvSharp 4.10 goldens from tools/cv-goldens), vision.rs (map art, minimap and island trackers, door locator, floor refiner, icons, heading) and features.rs (ORB, Hamming kNN, RANSAC similarity; MapMatcher), Lang, Windows OCR, the region placement and the party screen reader (Detectors).

Still to port, in order: Sanctum Helper's Engine.cs, OverlayWindow.cs, Hosted.cs (the `--hosted`
contract), SharedCapture/SharedNotices/OwnOverlays, KeyHook, Gamepad, ToastWindow, ChestPrompt, Analyzer,
the app entry; then point alogs-app's sanctum.rs at it. In the meter: OcrWorker / names_first threads and the
top-only / box-part captures and calm pacing (performance; behaviour is the same without them), dmgnumbers (paused in
B16). Sanctum Helper's Dev switches are beta-only and all off by default: the port keeps the default behaviour.
