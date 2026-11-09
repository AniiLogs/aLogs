// Generated from CHANGELOG.md by patch-notes.mjs - edit the changelog, not this file.
window.PATCH_NOTES = [
 {
  "version": "0.1.37",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**Sanctum Helper has a Dev tab (betas only):** proposed changes to its speed, smoothness, accuracy and memory, each with its own switch, sorted by risk, so each can be tested alone. All are off at every start unless you make one Persistent, which starts it on through restarts and updates until you undo it or that setting changes in an update. Only make one Persistent once you've tested it and decided it's no risk for you. With all off, it works as in 0.1.36."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "**Settings text:** \"Capture only the game window\" now says the yellow border can still show briefly with it off, and \"Block from the fade to black\" no longer says it's off by default."
    ]
   }
  ]
 },
 {
  "version": "0.1.36",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**Each part of aLogs runs on its own switch:** the DPS meter, Sanctum Helper and sending to the leaderboards turn on and off separately, and none needs another. With the DPS meter off, your records are still sent and Sanctum Helper keeps running.",
     "**The button over the game turns everything off, and back on:** one press turns off the parts it controls, the next brings back the ones that were on. Pick those parts in General Settings › The button over the game.",
     "**A status line in the sidebar** under a part that needs a look: not signed in to the leaderboards, Sanctum Helper needing .NET 9, or a part stopped after errors.",
     "**Egg Heist runs have their party:** who played, their lead Aniimo and their team are read from the Party screen and sent with each match.",
     "**Egg Heist's rank screen is read and sent after a match,** on keyboard or controller, with the points that match gained or lost.",
     "**aLogs can press Next on Egg Heist's end screen:** with Leaderboards › Automation on, it moves on to the rank screen once the end screen is read. Turn just this off under Sanctum Helper › Egg Heist on the leaderboards.",
     "**Inputs are held until a run is sent:** with Leaderboards › Automation on, your presses are ignored while a result is read and sent, so nothing gets skipped. The notice shows, then they come back, after 10 seconds at most.",
     "**Show an example of the DPS window:** DPS Meter › Overlay can show it with made-up numbers, so you can place it without a fight.",
     "**Windows 10: capture only the game window:** a new switch in General Settings keeps other programs' overlays out of what aLogs reads and out of your proof pictures. Windows then draws a yellow border round the game while aLogs is capturing. About explains both ways.",
     "**A heads-up about Discord's overlay:** Discord draws it inside the game's own picture, so it ends up in proof pictures. aLogs says so once, with where to turn it off."
    ]
   },
   {
    "heading": "Improved",
    "items": [
     "**Result, Party, menu and Battle Data screens are read from the game window itself on Windows 10,** so nothing on top of the game gets into what's read or sent. Windows shows its yellow border for a moment while one of these screens is up, even with the switch above off.",
     "**Runs are sent sooner:** a run goes as soon as its pictures are taken and its numbers are read, and your inputs come back right after.",
     "**Battle Data opens sooner:** View Battle Data is clicked as soon as the result screen is read, and moving your mouse no longer pulls the click off the button.",
     "**Party screen reads take under half the time,** with the same answers.",
     "**Scores the site already has aren't sent again:** this covers a menu best on your board row and an unchanged Egg Heist rank.",
     "**Checking for updates copes with GitHub hiccups:** it tries again when a download server has trouble, and an update it found still shows if its download fails.",
     "**The updating window** shows the animated aLogs icon and says which beta it's installing.",
     "**Notices belong to their part:** each shows only while its part is on. \"Tell me when a record is sent\" moved to Leaderboards › Sending.",
     "**A problem in one part no longer stops the others:** only that part stops and the sidebar says so. Turn it off and on to try again.",
     "**Lighter with only sending on:** with the DPS meter off, bosses out in the world aren't followed."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "**Teams on the Party screen** are read far more often, look-alike Aniimo aren't mixed up, and each run sends its own teams instead of the previous run's.",
     "**Special Offerings and Season Buffs** are read wherever the game draws the panel, including offerings like Resurgence, a change made late, and someone else's party. Your own lobby's Season Buffs no longer go with their run.",
     "**Party screens aren't missed or cut short:** one is read right up to the fight, even when its title reads oddly. A name missed on one read no longer drops that player, and lead Aniimo and your own name stay on their cards.",
     "**Battle Data numbers are read far more strictly:** each comes from its own spot on its own card once the cards stop moving, and only goes out after two reads agree. One player's numbers can no longer land on another's card.",
     "**Result pictures are the screen that was read,** never a loading screen the game had moved on to.",
     "**Names go to the leaderboards correctly:** a misread on the Party screen can't replace a clearer read from the result screen, a Korean name read from your menu or Profile stays Korean, and AI teammates the game renames stay marked as AI.",
     "**Egg Heist results are read properly:** Loot This Match and Total Defeats are sent once two reads agree, a match can't be sent twice, and leaving the rank screen quickly doesn't lose your rank.",
     "**Sanctum Helper's maps in Egg Heist:** they stay up after Interlink or Holo-Battle Sim and with the \"Escape Condition\" panel showing, and they go away after you leave. They no longer show in a Pathfinder challenge.",
     "**Sanctum Helper keeps your spot on the map screen:** backing out of the map (M, or Select on a controller) carries on from where you were.",
     "**The DPS meter no longer starts on the Party screen:** a player's title banner there could pass for a boss bar.",
     "**HP labels stay put:** a boss bar read short could put a label over its right end, and then labels disappeared or the HP % froze.",
     "**Dragging the DPS window or an HP label no longer loses it off screen,** and a spot saved off screen is forgotten so the window comes back.",
     "**General Settings no longer has a second scrollbar.**"
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
 }
];
