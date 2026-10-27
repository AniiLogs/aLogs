# Aniimo boss DPS meter (prototype)

Team DPS from the boss HP bar: reads the bar, level, boss title, segment counter (`xN`) and countdown timer
from the screen, turns the bar fill into damage with the decoded boss HP formulas, and divides by time.
"Team" means everything that moves the bar: you, your pets, AI bots and co-op players.

It only looks at screen pixels; it never touches the game's memory or files.

## Run it

```
python live.py                 # overlay (drag to move, right-click to close) + console
python live.py --no-overlay    # console only
python bossbar.py shot.png     # read saved screenshots instead
```

Needs: Python with Pillow, Windows 10/11. Nothing else is installed.

## How it captures (same approach as Sanctum Helper)

- `PrintWindow(hwnd, PW_CLIENTONLY | PW_RENDERFULLCONTENT)` - built-in Windows GDI. Renders the game window into a
  bitmap even when it is covered, and shows **no yellow border** (it is not Windows Graphics Capture).
- If that keeps returning blank frames, it switches to a plain screen copy (`BitBlt`); the game then has to be
  visible. The overlay is hidden from screen captures (`SetWindowDisplayAffinity`), so it never reads itself.
- Play in Borderless or Windowed mode (exclusive fullscreen can give blank frames).

Measured on a 3840×2160 game window: capture ~75 ms, bar search ~50 ms, OCR ~15 ms per text field.

## Accuracy check

Boss rush run on 2026-10-25 (Omega Sherro, Lv 60, 4 players), replayed through the current logic:
meter **40,665** team DPS vs the game's own Battle Data **40,607** (sum of the four players' Average DPS).
The decoded max HP for that boss (24,450,500) matched the game's numbers to ~0.4%.

Every time the game's Battle Data screen appears, the meter saves a full screenshot and the OCR'd stats of
every player (Average DPS, BREAK Value, Damage Taken, Heal Amount) to `logs/battle_data/<time>.png/.txt`.
Right after a tracked fight it also writes the meter's number next to the game's team DPS and stores a
measured max HP per boss in `calibration.json`.

## Fight tracking

- **Boss locked per fight.** Boss rush mode stays on once `xN` is seen (or the game log shows a boss rush
  space). A different boss is accepted only after 3 consistent clean reads; garbled names are ignored.
- **Bar locked per fight.** Its position is checked through the two brackets each frame; a short dropout
  (effects, menus) skips frames instead of ending the fight.
- **Fill without assuming a color.** It follows the bar from the left until the color jumps (flash, ghost,
  next segment underneath, empty track), so it works for every segment color.
- **Segments.** The counter (`xN`) is read every frame by matching digit shapes (`xn_digits.py`,
  templates in `xn_templates.json`; Windows OCR misses single digits), with OCR as a second opinion.
  Two agreeing counter reads set or correct the segment; a bar that refills and holds for two frames
  moves to the next one. Frames whose bar doesn't start with the live-HP red (flashes, full ghost, the
  yellow BREAK overlay) are skipped. Damage follows the current state, so corrections fix the total.
- **Boss rush entry by cycle.** When two entries share a title, the one in the running cycle
  (`boss_rush_cycles.json`) is used.
- **Timer smoothed against the clock.** Misreads (e.g. 177, 199, 477) are ignored. The timer never ends a
  fight; the game log and the bar disappearing do.
- **Game log.** Player.log is followed from its end (it is flushed about every 50 ms). A scene change
  (`createVoxelScene`) ends the current fight; `100-<player>-<scene>` spaces mean "not boss rush".
- Whichever of timer and game log notices the end first ends the fight.

## Overlays

Nothing is shown until a boss fight is on screen; everything places itself from the game window and the bar.
- **DPS window**: left side, under the timer panel. Average team DPS, last-5-s DPS, team damage, time.
  Stays 15 s after the fight with the result, then hides. Right-click closes the meter.
- **HP labels**: white shadowed text like the game's own, attached to the boss bar:
  total HP % left centred above the boss name, this bar's HP (`415.7K / 1.22M`) right of the `xN` box,
  and `Total 8.89M / 22.41M` under the bar's right end. Hidden when the bar is gone.

Drag any of them to nudge it: the offset is kept relative to the bar / game window (`overlay_positions.json`).
Double-click an HP label to put it back at its default spot.

Both show in screenshots and recordings; `--hide-from-capture` keeps them out.

## Leaderboard submissions

Records go to the AniiLogs leaderboards (`/leaderboard/` on the site) when `submit_config.json` exists here:

```json
{"api_url": "https://aniilogs-api.pages.dev/api", "token": "alb_..."}
```

Create the token on the leaderboard page ("Desktop meter" section) while signed in; it is shown once. Keep this
file private. What is sent (`submit.py`):
- `interlink_run` - one fight once its result screen is read; updated (same entry) when Battle Data follows.
  Score, stars, time, killed or not, team DPS (Battle Data), meter DPS/damage, each player's stats, screenshot.
- `interlink_week` - Challenge Stats weekly total; `interlink_best` - a boss's best score from the prep screen.

Everything is queued in `logs/submit_queue/` and sent in the background (retries while offline; a record the
server refuses is kept as `.rejected`). `--no-submit` turns it off.

## Scores and results (for the leaderboard)

- **Challenge Stats** (Esc menu): weekly Best Score + stars → `logs/scores.csv`.
- **Battle prep** screen: that boss's Best Score + stars → `logs/scores.csv`.
- **Result screen** after a fight: rating, score, completion time, each player's highlight stat → `logs/results.csv`,
  tied to the fight the meter just tracked; a New Best Record also updates `scores.csv`.
- `logs/fights/<fight>.json` joins the meter's numbers, the result screen and Battle Data of one fight.
- Every recorded value keeps its screenshot in `logs/screens/`.

**Auto View Battle Data** (`inputblock.py`, turn off with `--no-auto-click`): when the result screen shows and the
game is the active window, keyboard keys and mouse buttons are held back (mouse movement still works) so mashing
can't leave the screen, and the meter clicks "View Battle Data" (up to 3 tries, cursor put back). The hold ends when
Battle Data is captured, the screen changes, a boss bar returns, or after 60 s at most. Ctrl+Shift+F12 ends it at
once; Ctrl+Alt+Del always works. Controllers can't be held (that would need a driver).

## Diagnostics

- `logs/meter.log`: the file to send when something goes wrong. Startup info (Windows, screen, args),
  game window and capture mode, frame size, game-log scene events, bar found/lost with the reason,
  fight start (boss row, raw title OCR), segment changes, Battle Data captures, errors with full traceback,
  and once a second a snapshot line: bar position, fill and colors, `xN` digit match and OCR, raw OCR text,
  and the fight state. Capped at 5 MB × 4 files (`meter.log.1` … `.3` are the older parts).
- `logs/<fight>.csv` + `logs/<fight>/`: per-frame readings and bar crops twice a second (`--no-debug-frames` turns the crops off).
- `logs/nobar/`: full frames every 5 s (max 30) when in a dungeon but no bar is found, with the reason.
- `logs/battle_data/`: every Battle Data screen.

## Works at any resolution

The white `[ ]` brackets at the ends of the boss bar are the ruler: every position (fill track, title, `xN`) is a
fraction of the bracket span. The timer is pinned to the top-left of the 16:9 UI area.

Tested (same fight, resized): 1280×720, 1920×1080, 1920×1200, 2560×1440, 3840×2160, plus 4K and half-size crops.
All read level, title, `xN`, timer and fill correctly. Not yet tested on a real ultrawide screen.

## Files

| File | What |
|---|---|
| `live.py` | live meter: capture loop, fight tracking, overlay, CSV log per fight in `logs/` |
| `bossbar.py` | same reading on saved screenshots |
| `reader.py` | bar detection, fill measurement, text crops, boss lookup, damage math |
| `wincapture.py` | PrintWindow / BitBlt capture, overlay hiding |
| `ocr_win.ps1` | Windows built-in OCR, run as a long-lived helper |
| `hpcalc.py`, `boss_catalog.csv`, `boss_rush_grades.csv` | decoded HP formulas and boss data |

## Damage math

- Boss rush: the bar shows only the current star segment. `grade = 17 − N`;
  `damage = maxHP × (cum[grade−1] + (1 − fill) × (cum[grade] − cum[grade−1]))`.
- Other bosses: `damage = maxHP × (1 − fill)`.
- Average DPS = damage ÷ (180 − timer). "Last 5 s" DPS uses the change over the last five seconds.

## Limits

- Max HP is a prediction from decoded formulas; it has not been checked against a real fight yet.
- Text OCR needs Windows' English OCR language (installed by default on English Windows).
- The hit flash covers ~1% of a segment, so single readings in segment 1 can be off by ~2,000 HP.
- Late segments are coarse: the last segment is 50% of max HP.
- Open-world bosses with several equal bars (`mutiHpBar`) aren't handled yet.
- Fill is read ~4 times a second; text once a second.
