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
     "**A status line in the sidebar** under a part that needs a look: not signed in to the leaderboards, Sanctum Helper needing .NET 9, or a part stopped after errors.",
     "**Windows 10: capture only the game window:** a new switch in General Settings keeps other programs' overlays (Discord, Steam, notifications, windows on top) out of what aLogs reads and out of your proof pictures. Windows then draws a yellow border round the game, only while aLogs is capturing. About explains both ways."
    ]
   },
   {
    "heading": "Improved",
    "items": [
     "**Checking for updates copes with GitHub hiccups:** it tries again when a download server is having trouble, and an update it found still shows if its download fails.",
     "**Teams and buffs on the Party screen read faster:** a read takes under half the time it did, and much less when the screen hasn't changed, with the same answers.",
     "**A menu best the site already has isn't sent again:** before sending the best score shown on the battle prep screen, aLogs checks the site's board; if your row there already has that score (from a run you or a teammate sent), nothing goes up.",
     "**An unchanged Egg Heist rank isn't sent again:** opening Rank Info with the same rank, rank points and stars the site already has sends nothing, even after aLogs restarts.",
     "**Notices belong to their part:** each shows only while the part it comes from is on, and they all show with the DPS meter off. \"Tell me when a record is sent\" moved to Leaderboards › Sending; where notices show stays in General Settings › Notifications, and your spot is kept.",
     "**A problem in one part no longer stops the others:** if sending to the leaderboards or the DPS window keeps failing, only that part stops and the sidebar says so; turn it off and on to try again.",
     "**Lighter with only sending on:** with the DPS meter off, bosses out in the world aren't followed.",
     "**Battle Data opens sooner after an Interlink result:** View Battle Data is clicked as soon as the result screen has been read (often within a second), instead of about 4 seconds in, and no longer waits on a fresh picture of the game (when that stalled, the click came late or not at all). The game picture no longer drops out for several seconds right after the result screen shows, which held the click up by 7 to 27 s. On a busy graphics card, aLogs no longer waits for it while holding the picture back: the meter always has the newest finished picture to read.",
     "**A run is sent as soon as both pictures are taken:** the result screen, then Battle Data; once every card is read, the run goes at once and aLogs lets go of your inputs, instead of holding on for 5 more seconds. The Battle Data picture is always the screen that was read (one run's was a loading screen when the game moved on first). The numbers go once two reads in a row agree, so a read while Battle Data is still fading in is never sent."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "**General Settings no longer has a second scrollbar:** it left empty space at the bottom of the window.",
     "**Battle Data numbers stay with the right player:** each number is now read from its own place on its own card. A first read could put one player's Damage Taken and Heal on another's card until a later read corrected it.",
     "**Teammates' names are sent as the result screen reads them:** those are the clearer read, and a name misread on the Party screen could replace a right one and add a wrong player to the leaderboards.",
     "**Party cards keep their lead Aniimo and your own name:** a card could go up with no lead Aniimo, and yours with a misread of your name.",
     "**The Party screen is read right up to the fight:** teams changed or an offering chosen late could be missed when the screen's title was hard to read.",
     "**Special Offerings like Resurgence are read:** some were never recognized, so the run went in without its offering. aLogs rebuilds its icons once when it first opens after this update.",
     "**A Party screen is no longer missed when its title reads oddly:** the title could be read with a stray space, and that run then had no teams or buffs. The Profile screen had the same problem.",
     "**The updating window shows the animated aLogs icon,** not a still picture (the app's security settings kept the animation from loading).",
     "**Everyone's team on the Party screen** is read far more often: each player's three other Aniimo are found by their skill icons and compared the way the game draws them, faded into the card, so a card sitting a little off (or higher, once everyone is ready) no longer hides them, and look-alikes (Luminelle and Iris) aren't mixed up. On 26 recent Party screens, 299 of 312 were read, up from about half. Each run now sends its own teams: the newest read of each card counts, so a run no longer goes out with the previous run's teams or with only one or two of them.",
     "**The Special Offering and Season Buffs** are read wherever the game draws the panel (it sits a little higher or lower on some screens), and a later read that misses one no longer drops it.",
     "**Sanctum Helper's map no longer shows in a Pathfinder challenge:** the game loads a challenge like an Egg Heist island, so the island map came up over it. A challenge's \"Victory Conditions\" panel now tells them apart.",
     "**The DPS meter no longer starts on the Party screen:** a player's title banner there could pass for a boss bar and open a fight before the run began.",
     "**Opening Battle Data automatically is faster and sure:** aLogs clicks as soon as the result screen is read (1-4 s, not always 5), and moving your mouse meanwhile no longer pulls the click off the button.",
     "**\"Run sent\" notices come sooner:** a run goes to the leaderboards a few seconds after its Battle Data is read, not only once you leave the screens.",
     "**Every player stays in the run's party:** one Party screen read that missed a name (as the screen fades into the fight) no longer drops that player from the run.",
     "**Teams are read on the first run after an update:** the game's icons load when aLogs starts, not at the Party screen, where rebuilding them could take until the fight had begun.",
     "**A heads-up when Discord's overlay is in the game:** Discord draws its overlay inside the game's own picture, so its voice widget (names and avatars) ended up in proof pictures. aLogs now says so once, with where to turn it off.",
     "**The updating window** shows the aLogs icon as its own loader (its HP and break bars fill while the update installs) and says which beta it's installing (0.1.36 B4, not just 0.1.36). Its icon no longer goes missing."
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
