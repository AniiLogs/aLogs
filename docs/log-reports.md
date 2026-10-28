# Log reports: fixing what players report

Players send their logs from aLogs (DPS Meter › Captures & logs › **Send logs**), with an optional note about what
went wrong. Developers, and the AI agents they work with, download those reports, find the cause, and fix it in a
pull request to this repository.

## Who can read them

Only AniiLogs accounts with the **developer role** (granted on the website). Everyone else can send reports but never
read them. Ask the maintainer for the role. Developers read reports and leave notes on them; only the maintainer
clears them.

## Setup (once)

1. Sign in on [aniilogs.github.io](https://aniilogs.github.io) with Discord, open your account page, and under
   **Desktop app tokens** create a token (label it e.g. "log reports"). It's shown once: copy it.
2. Save it where the tool looks for it, outside any repository:
   - Windows: `%USERPROFILE%\.aniilogs\dev-token` (just the token, on one line)
   - macOS/Linux: `~/.aniilogs/dev-token`
   - or set `ALOGS_DEV_TOKEN` for the session.

Never commit the token or paste it into a chat, issue or PR. Revoke it on the same page if it leaks.

## Commands (Node 18+, nothing to install)

```
node tools/log-reports.mjs list                 # open reports, newest first: id, UID, time (UTC), size, app build,
                                                # screenshots, the player's note and every developer's notes
node tools/log-reports.mjs list --all           # cleared reports too (their notes stay; their logs are gone)
node tools/log-reports.mjs get <id> [folder]    # download and unzip (default ./log-reports/<id>, which git ignores),
                                                # with the player's screenshots (screenshot-1.png, ...)
node tools/log-reports.mjs note <id> "<text>"   # leave a note on it (see below)
node tools/log-reports.mjs clear <id>           # maintainer only: delete its logs once the fix is in
```

A report's logs are deleted automatically after 30 days; its line and notes stay.

## What's in a report

The logs are text. Private data is scrubbed by the app before sending: no Discord sign-in, Windows user or PC name,
tokens, Discord IDs or emails. The game UID and in-game name stay, since the report is about that character.
A player may add up to 5 screenshots of their own (PNG, JPEG or WebP); those are sent as they chose them, so treat
them like the logs: private, never copied into code, commits, issues or PRs.

| File | What it is |
|---|---|
| `app.txt` | App version and RC build, whether the meter was running, when it was sent (unix time) |
| `meter.log`, `meter.log.1` | The meter's log (rotates at 5 MB). `INFO` lines are the story: fights started and ended, the kill and how it was dated, BREAKs, result screens, Battle Data, Catch Countdown, profile notes, leaderboard sends. `DEBUG` lines are per-look details such as why no boss bar was found. |
| `fights/<date>_<time>_<Boss>.csv` | One per boss fight, about 10 rows a second: `wall_s` (seconds since the bar appeared), `timer_s`/`timer_ocr` (Interlink timer), `xn_ocr`/`segment` (Interlink xN bar), `fill_pct` (bar fill read), `fill_rgb`/`edge_rgb` (colours at the bar's start and fill edge), `team_damage`, `avg_dps`, `dps_5s`, `note` (events), `break_left_pct`, `break_phase` (drain, broken, refill). The last row says how the fight ended. |
| `profile_events.csv` | Profile notes: level and EXP from the EXP card, catches |
| `time_check.csv`, `egg_matches.csv` | Fight time compared with the game's Battle Data; Egg Heist end screens |
| `status.json` | The meter's state when it was sent: game window, capture, current fight, character |
| `settings.json` | The meter's settings (sign-in removed) |
| `sanctum/` | Sanctum Helper's `log.txt`, settings and state |

Where the code lives: the meter is `bossbar/` (Python; `live.py` runs fights, `reader.py` reads the screen), the app
is `app/` (Tauri: Rust in `app/src-tauri/src`, UI in `app/ui`). Fight rows are written by `Fight.update` in
`bossbar/live.py`; the fight's end is in `end_fight`.

## Working on a fix

- Read the player's note and any notes already on the report (someone may be on it), then `meter.log` around the
  fight they mean (the fight CSV's file name is its start time), then that fight's CSV.
- Fix the cause in a branch and open a PR. Describe the problem and the fix in your own words.
- **Always leave a note** on every report you worked on, even if you found nothing or didn't finish, so the
  maintainer has the record:
  `node tools/log-reports.mjs note <id> "Found: … Changed: … PR: <link> Verified: how / not yet"`.
  One report, one or more notes; a report you only looked at gets a note saying what you ruled out.
- **Don't clear reports.** The maintainer reads the notes and clears a report once its fix is in.
- **Never put report contents in the repository, an issue or a PR:** no log excerpts with UIDs or names, no
  attached files. PRs are public; the reports are not. Quote numbers and timings, not identities. A note is
  private (developers only), but keep UIDs and names out of it too.
