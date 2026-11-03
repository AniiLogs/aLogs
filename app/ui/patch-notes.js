// Generated from CHANGELOG.md by patch-notes.mjs - edit the changelog, not this file.
window.PATCH_NOTES = [
 {
  "version": "0.1.34",
  "intro": "",
  "sections": [
   {
    "heading": "Fixed",
    "items": [
     "**Holo-Battle Sim wins** are sent again when the last boss's bar vanishes on the kill: the Victory screen is read even while the meter is still waiting for that bar, so leaving it quickly no longer loses the run."
    ]
   }
  ]
 },
 {
  "version": "0.1.33",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**A notice when a record is sent:** each time a run, a Holo-Battle Sim win or your Egg Heist rank reaches the leaderboards, a small notice over the game says what was sent and to which board (\"Interlink run sent · Alpha Scorchhowl · 17 stars · 17,061\"). Set it up in General Settings › **Notifications**: where it shows (top right, top middle, middle right, middle left, or anywhere you drag it), how it comes in (fade, slide or drop), or turn it off. Sanctum Helper's notices (\"Chaos Mode found\") show the same way and wait their turn, so two never overlap."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "**Beta Menu bug reports** say which aLogs you're on, not an old test list's build."
    ]
   }
  ]
 },
 {
  "version": "0.1.32",
  "intro": "",
  "sections": [
   {
    "heading": "Fixed",
    "items": [
     "**Sanctum Helper's Auto difficulty** reads the party screen again when you queue for Egg Heist from your homeland, and catches a difficulty picked just before you start."
    ]
   }
  ]
 },
 {
  "version": "0.1.31",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**Two-boss Interlink stages:** when the middle stage has two bosses one after the other, aLogs follows them as one run on one clock. The DPS window and the Dashboard show **Boss 1/2**, then **2/2**, and the run goes to your Battle History and the leaderboards under the stage's first boss, with both bosses' damage and each boss's part. Both bosses count toward the middle's 40 stars, and your best is kept for the stage, whichever boss the run ended on.",
     "**A warning when Windows can't read the game's text:** without Windows' English text recognition (OCR), result screens, boss names, Battle Data and the prep and Party screens can't be read, so runs aren't recorded or sent. The Dashboard and the DPS Meter page now say so, and **Install English OCR** adds it for you (Windows asks for admin permission). English added in Windows Settings while aLogs runs is picked up without restarting."
    ]
   },
   {
    "heading": "Improved",
    "items": [
     "**Interlink time limit:** the run's clock is read off the game's timer as the fight starts (or from the Overtime offering on the Party screen), so with Overtime (4:00) a kill after 3:00 counts as a clear.",
     "**Player names:** names on the result and Battle Data screens are read as they appear on screen, and the name in your game menu, which links your character, is read by the same name reader.",
     "**Result screens:** the time and each player's stats are read with the game's own digits.",
     "**Battle Data:** the cards are counted on screen, so each number lands on its own card even when the text reader misses one.",
     "**Smoother and lighter:** the first read of a result, Party or Battle Data screen no longer holds the meter up for about a second, and the name reader uses far less memory.",
     "**The button over the game blinks less:** it now waits 0.75 s by default before hiding with the game's icons. If yours still blinks, pick a longer wait in General Settings › The button over the game."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "**Interlink HP bars:** the counter is read from ×21 down (×17 to ×21 were skipped), and damage follows the game's own bars, so an Interlink run's damage and DPS are right from its first bar.",
     "**Interlink fights** no longer start on a boss level misread as the bar fades in, which ended the fight a minute later and started it over.",
     "**Bosses above your Aniimo's level:** the game shows their level in red, which aLogs couldn't read, so the fight was never recorded. It's read now.",
     "**Battle Data:** each player's Avg DPS, BREAK, damage taken and healing are their own, also when the result screen missed a teammate (the players after them could take the next player's card, yours included).",
     "**You and your teammates in a run:** your row keeps your name and is marked as you, and no teammate is listed twice (with their Battle Data twice). The Party screen's read could take over a right one.",
     "**Teammates in a run:** a teammate whose name or stats on the result screen weren't read is no longer left out of the run, and their Battle Data goes with them.",
     "**One-character names:** a teammate whose name is a single letter or digit shows as it is, not blank or as another character, also when the Party screen misread it.",
     "**Text recognition in any English:** on a PC with English OCR other than US English (UK, Australia, India...), the meter starts and reads the game's screens. Before, it didn't start at all.",
     "**Ultrawide screens:** the boss bar is found at 3440×1440.",
     "**Warning titles** are easy to read in the Meadow theme and in a light Custom theme."
    ]
   }
  ]
 },
 {
  "version": "0.1.30",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**A Leaderboards menu:** everything about the leaderboards, linking your character included, is in one place.",
     "**Holo-Battle Sim end:** after you press End Holo-Battle Sim at the exit gate, aLogs holds the Victory screen and reads every gizmo, then gives control straight back."
    ]
   },
   {
    "heading": "Improved",
    "items": [
     "**A clearer DPS window:** your average DPS up top, then Last 5s, Team damage and Time, with the numbers lined up.",
     "**Ctrl+Alt+F12** gives you control back at any moment, as Ctrl+Shift+F12 does.",
     "**Egg Heist's settings** sit on the Sanctum Helper page."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "**HP labels** show on the boss bar again every fight.",
     "**Battle Data:** a downed teammate's card no longer drops the run's numbers."
    ]
   }
  ]
 }
];
