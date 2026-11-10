# Plug-ins

aLogs is a core with plug-ins. Each plug-in has its own switch and settings page, and owns its notices and log lines.
None depends on another being on. The core is what they share: the game reader, the screen capture, the notices
window and the button over the game.

The list of plug-ins is [`bossbar/plugins.json`](../bossbar/plugins.json). The app reads it through
`app/src-tauri/src/plugins.rs` (the `plugins` command) and the meter through `bossbar/plugins.py`. Everything that
treats the plug-ins as a set uses this list, so a new plug-in doesn't need new wiring there:
- the button over the game, and its choice of which plug-ins it controls;
- the sidebar statuses;
- the toast owners;
- whether the meter process reads the game.

## Who owns what

| Part | Owner | Code |
| --- | --- | --- |
| Reading loop, OCR, game log, names, settings, diagnostics | core | `bossbar/live.py`, `core.py` |
| Following a boss fight on its HP bar | core (the DPS meter shows it; the leaderboards send it) | `bossbar/fights.py` |
| Reading end screens into runs (result, Battle Data, Holo-Battle Sim, Egg Heist, Challenge Stats) | core (the DPS meter keeps them as your results; the leaderboards send them) | `bossbar/results.py` |
| Notices over the game, for every plug-in | core | `bossbar/notice_window.py` |
| The button over the game | core | `app/src-tauri/src/overlay_button.rs` |
| The boss kill sound (Audio cues, on the DPS meter's page) | DPS meter | `app/src-tauri/src/audio.rs` |
| Screen capture | core | `app/src-tauri/src/capture.rs`, [shared-capture.md](shared-capture.md) |
| DPS window, HP labels, end-screen notes, "You're being attacked" | DPS meter | `bossbar/dps_overlay.py` |
| Deciding and sending runs, proof automation, character link, online status | Leaderboards | `bossbar/leaderboards.py`, `submit.py` |
| The Lost Sanctum layout, nests, key rooms | Sanctum Helper | its own program (`SanctumHelper/`) |

## The list's fields

| Field | Meaning |
| --- | --- |
| `id`, `name`, `page` | How the plug-in is known, and its page in the app (`data-page`). |
| `switch` | A `settings.json` pointer (`/leaderboard/submit`), or `"meter"` for the DPS meter's own switch (`overlay_button.json` `capture_on`). |
| `default` | Its switch before the player touches it. |
| `opt_in` | Never turned on except by the player. A press of the button with nothing remembered skips it. |
| `needs` | `reader`: the meter process must read the game. `capture`: the app's screen capture. `input`: aLogs may take control of the game. |
| `notices` | Each notice kind it owns, with the setting that turns it off (`null`: only its switch). |
| `log` | Its tag in `meter.log` (`[leaderboards] run sent`). The core's lines have none. |

## Rules every plug-in follows

- **Its own switch only.** Whether it runs depends on its switch, nothing else. The meter reads the game while any
  plug-in that `needs` the `reader` is on (`plugins.meter_mode`):
  - DPS meter on: the meter reads and shows.
  - DPS meter off, leaderboards on: the meter still reads, but nothing of it shows. Bosses out in the world aren't
    followed.
  - Neither on: the meter rests.
- **The button.** It switches the plug-ins picked in General Settings › The button over the game
  (`button.controls.<id>`, all of them by default).
  - With any of those on, a press turns them all off and remembers which were on (`overlay_button.json` `resume`).
  - The next press turns those back on.
  - Its own notice ("aLogs off" / "aLogs on") is `notifications.button`.
- **Its notices are its own.** A notice of a plug-in that's off, or of a kind its setting turns off, is dropped
  (`plugins.notice_owners`). Where notices show is the core's (`notifications.place`, `motion`, `locked`).
- **Its errors are its own.** The core calls a plug-in's code through `plugins.guarded`:
  - An error there is logged under the plug-in's tag and counted, and the reading loop carries on.
  - Three errors within a minute stop the plug-in until its switch is turned off and on again, or aLogs restarts.
  - The sidebar then shows "Stopped after errors".
  - The DPS overlay's window loop counts its errors the same way.
- **Its status.** The meter writes each plug-in's state to `status.json` under `plugins`:
  `on`, `off`, `resting`, `waiting`, `attention` or `failed`, with a reason. The app shows a short line under the
  plug-in's name in the sidebar when it needs a look.

## Adding a plug-in

1. Add it to `bossbar/plugins.json`. The Rust test `plugins::tests::the_list_reads` lists the ids, so update it too.
2. Give it a page in `app/ui/index.html`, with `data-page` matching `page`, and a sidebar switch.
3. If it runs inside the meter:
   - Put its code in its own file, with a mixin the `Meter` takes (see `leaderboards.py`).
   - Use `log = logging.getLogger('meter.<log>')`.
   - Wrap the methods the core calls with `guarded('<id>', fallback)`.
4. If it's its own program: start and stop it from the app like Sanctum Helper (below). It reads the game picture
   from the shared capture ([shared-capture.md](shared-capture.md)) and hands its notices over the same way.

## Adding a Dev switch

A proposed change that testers can try alone (Beta Menu › Dev, beta builds only). With its switch off, the code does exactly
what it did before. All are off at every start, except what a tester made Persistent.

1. Add it to [`app/ui/dev-options.json`](../app/ui/dev-options.json), in its section's place: `id`; `plugin` (`core`, `dps`
   or `leaderboards`, whoever owns the code in the table above); `section` (`No known risk`, `Low risk`, `High risk`);
   `area` (`CPU`, `Smoothness`, `Accuracy`, `Memory`, `Speed`); `title`, `what`, `gain`, `risk`, `tested` for players, in plain
   words, no file names; `revision` (1). Be honest: `tested` says `Not tested yet` unless you tested it, and what you
   tested it on.
2. Guard the change: `devflags.on('<id>')` in the meter (at most one look at the file a second, never raises; put cheap
   conditions first), `crate::dev::on("<id>")` in the app.
3. Test it on and off (`bossbar/tests/dev_switches.py`). That test also fails on an id the list doesn't have.

**Raise `revision` only when the switch's behaviour changes.** That turns it off for everyone who made it Persistent, and
the tab says which changed so they can test it again. Rewording its text isn't a change. A new id starts fresh.

**How did it go?** Next to each switch a tester picks Better, No difference, Worse or Broke something and can add a note
(needed for the last two). The app keeps that with how long the switch was on while the game was open
(`dev-verdicts.json`, by id, `sanctum:<id>` for Sanctum Helper's), for that revision only: a raised revision starts both
again. **Send feedback** sends what hasn't gone up yet, for all four tabs, as one report on the same route as Report a
problem: `dev-feedback.json` inside the logs (each verdict, note, revision and minutes on, what is on now, the build) and
a summary as the report's note. So give every switch a log line its verdict can be checked against: one whose effect can't
be seen in the logs can't become the default.

On a release build `dev::start` loads nothing, every switch reads off and the meter isn't told about the list
(`ALOGS_DEV_ON`). Files, in the meter's data folder: `dev-persistent.json` (`{id: revision}`) and `dev-on.json` (what's on
now, for the meter). Sanctum Helper has its own switches and tab data (`Dev.cs`); the tab draws both the same way.

## Sanctum Helper: what aLogs and it agree on

Sanctum Helper is its own program (C#, .NET 9 Desktop Runtime). aLogs runs it and talks to it only through what's
listed below, so it can change freely inside as long as these hold. The aLogs side is
`app/src-tauri/src/sanctum.rs`. The Sanctum Helper side is `Hosted.cs`, `SharedCapture.cs` and `SharedNotices.cs`.

### Starting and stopping
- aLogs bundles it as `resources\sanctumhelper\SanctumHelper.exe` (`app/build-sanctum.ps1`).
- aLogs starts it as `SanctumHelper.exe --hosted --parent <aLogs pid>`, with the working folder set to the exe's
  folder and no console window. Hosted, it has no tray icon or settings window of its own. It quits when the parent
  is gone.
- aLogs starts it while its switch (`/sanctum/enabled` in aLogs' settings) is on and .NET 9 is installed. It checks
  every 3 s and starts it again if it stopped.
- **Exit code 3:** a standalone copy is already running (its single-instance lock). aLogs waits 15 s before trying
  again and tells the player.
- **To stop it**, aLogs writes a `quit` command (below) and waits up to 3 s for it to save and exit, then ends the
  process.

### Files (all under `%LOCALAPPDATA%\SanctumHelper\`)
| File | Written by | What |
| --- | --- | --- |
| `settings.json` | both | Its options. The Sanctum Helper page edits them (`sanctum_save`, merged key by key; `Layers` merged inside). It reloads the file when it changes. aLogs keeps `WindowTitle` and `RecordableOverlay` in step with its own General Settings (`share_settings`). |
| `hosted\info.json` | Sanctum Helper | The choices its page offers (languages, layers...). |
| `hosted\status.json` | Sanctum Helper | What it sees. It must carry `time` (ms since 1970); one older than 5 s counts as not running. The meter reads `inRun` (an Egg Heist run is on) for Egg Heist's input hold. |
| `hosted\cmd-<ms>-<pid>.json` | aLogs | A command: written to `.tmp` then renamed. Sanctum Helper picks it up and deletes it. `{"command": ...}` is one of `new-run`, `position-map`, `capture-key`, `capture-pad`, `clear-pad`, `render-layout` (`layout`, `width`, `height`), `open-logs`, `quit`. |

### Notices
- While the meter's `status.json` (`%LOCALAPPDATA%\aLogs\meter\status.json`) says `"notices": true` and is under
  5 s old, Sanctum Helper hands its notices to aLogs instead of showing its own.
- Each notice is a JSON file in `%LOCALAPPDATA%\aLogs\meter\notices\`, written whole then renamed. Its name starts
  with .NET ticks (20 digits), so notices sort by time.
- Contents: `{head, detail, accent: "#rrggbb", screen: [x, y, w, h] of the game's client area or null, from: "sanctum"}`.
- `from` is the notice kind in `plugins.json`. aLogs drops `sanctum` notices while Sanctum Helper's switch is off.
  Sanctum Helper's own "Notifications" option (`ShowNotifications`) decides whether it sends any at all.

### The game picture
- It reads the game from aLogs' shared capture (`Local\aLogs.Capture.v2`) and never captures on its own while aLogs
  runs. See [shared-capture.md](shared-capture.md) for the layout and how to ask for an area.
- The DPS meter's switch doesn't affect it: the capture serves whoever asks.
