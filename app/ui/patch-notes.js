// Generated from CHANGELOG.md by patch-notes.mjs - edit the changelog, not this file.
window.PATCH_NOTES = [
 {
  "version": "0.1.36",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**Each part of aLogs runs on its own switch:** the DPS meter, Sanctum Helper and sending to the leaderboards turn on and off independently, and none needs another. With the DPS meter off, your records are still read and sent (nothing of the meter shows over the game), and Sanctum Helper keeps running.",
     "**The button over the game turns everything off, and back on:** one press turns off every part it controls; the next turns back on the ones that were on (sending to the leaderboards only if it was on before). Pick which parts it controls in General Settings › The button over the game. A notice says \"aLogs off\" or \"aLogs on\" with what it switched; turn it off in General Settings › Notifications.",
     "**A status line in the sidebar** under a part that needs a look: not signed in to the leaderboards, Sanctum Helper needing .NET 9, or a part stopped after errors."
    ]
   },
   {
    "heading": "Improved",
    "items": [
     "**Notices belong to their part:** each shows only while the part it comes from is on, and they all show with the DPS meter off. \"Tell me when a record is sent\" moved to Leaderboards › Sending; where notices show stays in General Settings › Notifications, and your spot is kept.",
     "**A problem in one part no longer stops the others:** if sending to the leaderboards or the DPS window keeps failing, only that part stops and the sidebar says so; turn it off and on to try again.",
     "**Lighter with only sending on:** with the DPS meter off, bosses out in the world aren't followed."
    ]
   }
  ]
 },
 {
  "version": "0.1.35",
  "intro": "",
  "sections": [
   {
    "heading": "Fixed",
    "items": [
     "**Holo-Battle Sim's Gizmo screen** is read again after a win without a first clear: that Victory screen has no Rewards Earned row, so its coins, gizmo counts and gizmo row sit lower, and aLogs read none of them and gave control back at once. It reads both layouts now, and opens the Gizmo screen at the right spot.",
     "**Holo-Battle Sim's gizmos** are recognised when aLogs starts while you're already in a Sim run."
    ]
   }
  ]
 },
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
 }
];
