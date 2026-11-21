// Generated from CHANGELOG.md by patch-notes.mjs - edit the changelog, not this file.
window.PATCH_NOTES = [
 {
  "version": "0.1.37",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**A Dev menu in the Beta Menu (betas only):** a tab each for Core, the DPS meter and the leaderboards: proposed changes to their speed, smoothness, accuracy and memory, each with its own switch, with filters by risk and type, so each can be tested alone. Anyone on a beta sees it. All are off at every start unless you make one Persistent, which starts it on through restarts and updates until you undo it or that setting changes in an update. Only make one Persistent once you've tested it and decided it's no risk for you. With all off, it works as in 0.1.36.",
     "**Cyrillic names:** teammates with Cyrillic names (Дмитрий, Юля) show on your runs as they are, not as Latin look-alike letters, on the result, Battle Data, Party, menu and Profile screens. A name only of letters drawn the same as Latin ones (Анна, Ева) can't be told apart on screen and still reads in Latin letters."
    ]
   },
   {
    "heading": "Improved",
    "items": [
     "**The DPS meter reads the boss bar more truly and sooner:** a small mark the game draws on the bar is no longer taken for the end of the boss's HP, a fight counts from the moment its bar is first seen (not from when the boss's name is read), a reading far below the last must hold for a moment before it counts, and between fights the meter looks for a bar twice as often. The HP % shows the moment the bar does, without waiting for the boss's name to be read, and it now stands under the bar in front of the HP numbers instead of on the bar.",
     "**The yellow border is gone from menus on Windows 10:** result, Party, menu and Battle Data screens are still read from the game window alone, but aLogs now copies it a way that makes Windows draw no border. The border only shows for the few seconds aLogs takes control to record a leaderboard run, with Capture only the game window switched on, while a window covers the game when you are tabbed out, or on a PC where the new copy comes out black.",
     "**The meter and Sanctum Helper are rebuilt, with less to install:** they work as before, and the download is about 46 MB instead of 80.",
     "**Sanctum Helper no longer needs Microsoft's .NET runtime:** the installer no longer asks to download it.",
     "**Seven tested speed-ups from the Dev menu are now standard** (and leave it): faster boss HP and xN counter reads in fights, a cheaper screen-change check, proof pictures made once when a record is sent again, no update check or photo upload during a boss fight, and fewer repeated capture lines in the log. Same readings as before.",
     "**Installing aLogs for the first time looks like aLogs:** the installer opens a small aLogs window, the same as the updating one, instead of Windows' installer pages: pick a desktop shortcut and starting with Windows, press Install, and aLogs opens when it's done. If anything goes wrong, the full installer opens instead.",
     "**A Performance tab in General Settings:** the options that make aLogs lighter on your PC (or heavier, for finer readings) now sit together, in a section for each add-on, under one main switch, Performance toggles. Off (the default) runs aLogs as it's built; on, your choices apply, and turning it off keeps them for next time. The priority (from General) and how often the meter looks and reads during a fight (from the DPS meter's settings) moved here; if you had changed one, the main switch is on for you, so nothing changes. Two options are new from the Dev tab: the screen capture can wait behind the game and OBS on your graphics card, and Sanctum Helper's corner map can redraw at most about 80 times a second instead of on every frame of a 120 Hz or faster screen.",
     "**See what each part of aLogs uses (General Settings › Performance › Diagnostics, off unless you turn it on):** if aLogs seems to slow your PC, switch it on and report the problem: once a minute the log then says how much of your processor the meter, the app, its windows and Sanctum Helper each take, next to the game and all your other programs together (no other program is named).",
     "**aLogs notes it when its window freezes:** if the window stops answering for 5 seconds (\"Not Responding\"), aLogs writes down which part of the program it was stuck in and the names of the last things the page asked for, and again at 30 seconds. A crash inside aLogs gets a line too. Nothing of yours is in it, and it only leaves your PC inside a report you send (Dashboard › Report a problem), so after a freeze: start aLogs again and send one.",
     "**The program file says which build it is:** alogs-app's Properties › Details shows the beta and build under Product version (\"0.1.37 B15 (RC22)\"), not only \"0.1.37\".",
     "**Sanctum Helper says the difficulty you chose each time you open the party screen** (\"Normal Mode chosen.\"), not only when it changes.",
     "**Sanctum Helper runs with the game:** it starts within a few seconds of the game opening and stops a minute after the game closes (a minimized game keeps it running), instead of running whenever aLogs is open. Its page still works with the game closed, and says it starts with the game.",
     "**Sanctum Helper knows a Sanctum's layout as it loads:** the game's own log names each Sanctum while loading it, and the map now takes the layout from that, instead of working it out from the minimap (6 to 30 seconds, and now and then a wrong first guess). If the game doesn't name it, the minimap decides as before.",
     "**Fewer pictures saved on your PC:** the sample screens the meter kept for checking its readers, and the copies of each leaderboard proof with the DPS window drawn on it, are now saved only while Captures & logs › Save test pictures is on. The proof that's sent is the same.",
     "**Priority (General Settings › Performance) is the one control and does what it says:** Higher runs aLogs, the meter and Sanctum Helper above normal; the meter's text readers run below normal with Lower, normal otherwise. The two Dev menu switches that changed this are gone.",
     "**Player names are read side by side:** the names on the result, Party and Battle Data screens go to aLogs' Latin, Korean and Cyrillic readers at the same time instead of one after another. A screen's names take about half a second, so a run is on its way sooner after the result screen comes up."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "**Battle Data is read when you play with a controller:** the game then shows a button prompt under the cards and draws them a little higher, and aLogs only knew where they sit with mouse and keyboard, so a run's Battle Data numbers and names came out blank. Both layouts are read now.",
     "**A last check before anything is sent to the leaderboards:** no player can go out named twice, and every Aniimo sent is one from the game's own list. A nickname or a half-read name is never sent as a species (the nickname is kept as a nickname), so misreads are stopped in aLogs instead of being fixed on the site afterwards.",
     "**No more stutter while another window is focused (Windows 10):** when you clicked another window, aLogs changed to a slower way of looking at the game, four times a second, and each look made the game hold a frame about twice as long. Now it keeps looking the normal way for as long as no window covers the game: a window on another monitor, for one. With a window over the game, it captures the game window alone on the graphics card, as on Windows 11; Windows 10 draws its yellow border round the game while that runs, behind the window that covers it.",
     "**aLogs is lighter on your PC, with nothing read differently:** work that ran where it couldn't be used now stops. Sanctum Helper no longer copies the screen when you hold its key outside Egg Heist (Left Ctrl in the open world did it about 60 times a second), and its mouse hook, controller reading and map reading rest when they have nothing to do. The controller panel stops reading pads once you leave it, and an updating window can no longer be left running. The meter finds the boss bar about twice as fast, skips reading your HP number again when the picture hasn't changed, and no longer checks every picture for the Egg Heist fade outside a hold.",
     "**Performance › 60 fight reads a second works:** picking it in the Performance tab did nothing; the meter stayed at 30.",
     "**Changing how long the fight result stays up no longer ends a running fight**, and a priority change now reaches the meter's text readers straight away.",
     "**Sanctum Helper keeps your run when the game is minimized** (alt-tab in fullscreen counted as the game closing and forgot the layout after a minute), and its toggle mode no longer turns on from Ctrl pressed in other apps.",
     "**Players' names read more carefully:** a Korean name is no longer sent as a Latin word on a PC where it can't be read (it's left blank and read again), one player can't fill two Party slots, a player is only marked AI after two reads agree, and the Party screen's names are checked against the result screen's, so a misread one takes the result screen's name.",
     "**aLogs opens again from the tray:** after you had visited the Site tab, closing aLogs to the tray and then clicking its tray icon or Open did nothing; it comes back now.",
     "**Your Aniimo are known by their picture, not their name:** now that Aniimo can be given their own names, the lead Aniimo on each Party screen card is read from its head portrait, like the team below it, so the right species goes with your runs and Egg Heist matches whatever you called them.",
     "**Sanctum Helper keeps your place on the map:** after you close the in-game map it no longer moves you to a spot it only guessed, and when you leave a Sanctum it looks at all ten island doors and puts you at the one you went in by, not at some other blue door.",
     "**Interlink runs are sent right away:** a run goes to the leaderboards as soon as its result screen is read, with or without Leaderboards › Automation. It used to wait up to 45 s for its Battle Data, so with nothing opening Battle Data the run and its notice came 45 s late.",
     "**Battle Data is added to the run it belongs to:** once Battle Data is read, whether aLogs opened it or you did, it is added to the run already sent, up to 3 minutes later, and the notice says \"Interlink run updated\". A number or name that a later read of the result screen corrects is added the same way. With Leaderboards › Automation on, your presses stay ignored until Battle Data is read and sent.",
     "**A record no longer gets lost when you quit or restart aLogs right after a run:** quitting before a run was sent used to drop it. It is now sent (or queued for the next start) as aLogs closes.",
     "**Names on the result screen are read once the screen has settled:** aLogs used to go by its first look at the result screen, which could still be fading in, so a name came out wrong and showed up on the boards as an extra \"unclaimed\" player. It now waits until two looks in a row read the same names and numbers, about half a second, before it clicks View Battle Data or sends the run, and takes the proof picture then.",
     "**A \"Dealt the Last Strike\" highlight no longer holds a run up:** that highlight has no number, and aLogs waited for one, so in those runs View Battle Data was clicked about 4 s late.",
     "**The meter keeps counting through a burst of damage:** when a boss lost several health bars within a couple of seconds, the meter stayed on the old bar and showed a fraction of the damage for the rest of the fight, so a kill could be saved with 6% of the boss's HP. It now follows the bar counter: the total is right, and a kill counts the boss's full HP.",
     "**Special Offerings that share an icon are told apart:** a run could be sent as \"Charged Impact or Crushing Momentum\" or \"Essence Leech or Resurgence\". aLogs now checks which one the game offers that week and sends that one.",
     "**Bots are told apart from real players by what the Party screen shows:** a team-mate is counted as a bot only when it has one of the game's bot names and no CP on its card. A real player could be sent as a bot before, and was then left out of the run's credit.",
     "**Game data is read where the game keeps it:** values that many rows of the game's tables share were read from the wrong place, so some came out wrong: an Interlink week's season and its Special Offerings, two bosses' HP multiplier, and the rarity of Holo-Battle Sim gizmos (11 gizmos listed as Special are Normal or Uncommon). They now read as the game has them.",
     "**The button over the game blinks less on older installs:** if it was still set to hide after 0.3 s, the old default, it now waits 0.75 s like new installs. If you want 0.3 s, pick it again in General Settings › The button over the game and it stays.",
     "**Holo-Battle Sim: every gizmo is read, quickly and reliably:** aLogs now sees the exit gate's button on every ground and takes control at your press; on the Victory screen it opens the Gizmo screen at once (with a controller too), clicks All and captures the list as it scrolls, in a few seconds, then reads every gizmo by name once you have control back (upgraded Cores like Fervor Mask α count as 2). A notice says when it's scanning, and while it does your keyboard, mouse and controller don't reach the game.",
     "**Holo-Battle Sim: your team is known by its pictures:** each Aniimo on the Victory screen is read from its card art, so the right species goes with your run even when you gave it a nickname (the nickname rides along). An equipment card in a team slot is no longer taken for an Aniimo.",
     "**Holo-Battle Sim: every gizmo count is read:** the numbers under the gizmo icons are read one by one against the game's own digits. A lone 1 used to be missed, or two counts run together (7 1 2 4 1 read as 71, 24, 11), and the run went without its counts.",
     "**Block from the fade to black is off until you turn it on:** its switch showed off, but on installs that had never changed it the meter blocked button presses anyway.",
     "**Settings text:** \"Block from the fade to black\" no longer says it's off by default.",
     "**Notices are the right size before your first fight:** a notice that showed before any fight since aLogs started (a best read from the battle prep screen, for example) was drawn as if for a 1080p screen: half size on a 4K screen, and in the wrong spot. It now goes by the game window from the start."
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
