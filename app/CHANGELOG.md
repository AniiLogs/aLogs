# aLogs — patch notes

Each `## x.y.z` section becomes that version's GitHub Release notes (`npm run release`). Write for players: what
changed for them, not how it was done. Group under the headings that apply: **New**, **Improved**, **Fixed**.

## 0.1.37

### New
- **A Dev menu in the Beta Menu (betas only):** a tab each for Core, the DPS meter, Sanctum Helper and the leaderboards: proposed changes to their speed, smoothness, accuracy and memory, each with its own switch, with filters by risk and type, so each can be tested alone. Anyone on a beta sees it. All are off at every start unless you make one Persistent, which starts it on through restarts and updates until you undo it or that setting changes in an update. Only make one Persistent once you've tested it and decided it's no risk for you. With all off, it works as in 0.1.36.

### Improved
- **Seven tested speed-ups from the Dev menu are now standard** (and leave it): faster boss HP and xN counter reads in fights, a cheaper screen-change check, proof pictures made once when a record is sent again, no update check or photo upload during a boss fight, and fewer repeated capture lines in the log. Same readings as before.
- **Sanctum Helper says the difficulty you chose each time you open the party screen** ("Normal Mode chosen."), not only when it changes.

### Fixed
- **Your Aniimo are known by their picture, not their name:** now that Aniimo can be given their own names, the lead Aniimo on each Party screen card is read from its head portrait, like the team below it, so the right species goes with your runs and Egg Heist matches whatever you called them.
- **Sanctum Helper keeps your place on the map:** after you close the in-game map it no longer moves you to a spot it only guessed, and when you leave a Sanctum it looks at all ten island doors and puts you at the one you went in by, not at some other blue door.
- **A record no longer gets lost when you quit or restart aLogs right after a run:** a best run waits up to 45 s for its Battle Data before it is sent; quitting during that wait used to drop it. It is now sent (or queued for the next start) as aLogs closes.
- **Battle Data you open late still goes with your run:** if you open Battle Data yourself more than 45 s after the result screen (for example with Leaderboards › Automation off), your run, already sent without it, is sent again with its Battle Data, up to 3 minutes later.
- **The button over the game blinks less on older installs:** if it was still set to hide after 0.3 s, the old default, it now waits 0.75 s like new installs. If you want 0.3 s, pick it again in General Settings › The button over the game and it stays.
- **Holo-Battle Sim: aLogs sees the exit gate's button in more places:** on some grounds it wasn't found, so your press at the gate took no control. And on the Victory screen the Gizmo screen now opens at once instead of up to 20 s later, by when the game could have moved on.
- **Holo-Battle Sim with a controller: the Gizmo screen opens too.** With a controller the Victory screen opens Data Collection with RS, and aLogs' click did nothing (or closed the screen). aLogs now moves the mouse first, waits for the game's mouse prompt, then clicks the gizmo row.
- **Holo-Battle Sim: the Gizmo screen is read even when the Victory screen's gizmo icons aren't recognised,** and the Victory screen is read once its right side has finished fading in.
- **Holo-Battle Sim: every gizmo is read, quickly:** on the Victory screen aLogs opens the Gizmo screen, clicks All and captures the list as it scrolls down, in a few seconds, then reads the names once you have control back (upgraded Cores like Fervor Mask α count as 2). A notice says when it's scanning. While it does, your keyboard, mouse and controller don't reach the game, and it stops clicking if the game leaves the Sim.
- **Settings text:** "Capture only the game window" now says the yellow border can still show briefly with it off, and "Block from the fade to black" no longer says it's off by default.

## 0.1.36

### New
- **Each part of aLogs runs on its own switch:** the DPS meter, Sanctum Helper and sending to the leaderboards turn on and off separately, and none needs another. With the DPS meter off, your records are still sent and Sanctum Helper keeps running.
- **The button over the game turns everything off, and back on:** one press turns off the parts it controls, the next brings back the ones that were on. Pick those parts in General Settings › The button over the game.
- **A status line in the sidebar** under a part that needs a look: not signed in to the leaderboards, Sanctum Helper needing .NET 9, or a part stopped after errors.
- **Egg Heist runs have their party:** who played, their lead Aniimo and their team are read from the Party screen and sent with each match.
- **Egg Heist's rank screen is read and sent after a match,** on keyboard or controller, with the points that match gained or lost.
- **aLogs can press Next on Egg Heist's end screen:** with Leaderboards › Automation on, it moves on to the rank screen once the end screen is read. Turn just this off under Sanctum Helper › Egg Heist on the leaderboards.
- **Inputs are held until a run is sent:** with Leaderboards › Automation on, your presses are ignored while a result is read and sent, so nothing gets skipped. The notice shows, then they come back, after 10 seconds at most.
- **Show an example of the DPS window:** DPS Meter › Overlay can show it with made-up numbers, so you can place it without a fight.
- **Windows 10: capture only the game window:** a new switch in General Settings keeps other programs' overlays out of what aLogs reads and out of your proof pictures. Windows then draws a yellow border round the game while aLogs is capturing. About explains both ways.
- **A heads-up about Discord's overlay:** Discord draws it inside the game's own picture, so it ends up in proof pictures. aLogs says so once, with where to turn it off.

### Improved
- **Result, Party, menu and Battle Data screens are read from the game window itself on Windows 10,** so nothing on top of the game gets into what's read or sent. Windows shows its yellow border for a moment while one of these screens is up, even with the switch above off.
- **Runs are sent sooner:** a run goes as soon as its pictures are taken and its numbers are read, and your inputs come back right after.
- **Battle Data opens sooner:** View Battle Data is clicked as soon as the result screen is read, and moving your mouse no longer pulls the click off the button.
- **Party screen reads take under half the time,** with the same answers.
- **Scores the site already has aren't sent again:** this covers a menu best on your board row and an unchanged Egg Heist rank.
- **Checking for updates copes with GitHub hiccups:** it tries again when a download server has trouble, and an update it found still shows if its download fails.
- **The updating window** shows the animated aLogs icon and says which beta it's installing.
- **Notices belong to their part:** each shows only while its part is on. "Tell me when a record is sent" moved to Leaderboards › Sending.
- **A problem in one part no longer stops the others:** only that part stops and the sidebar says so. Turn it off and on to try again.
- **Lighter with only sending on:** with the DPS meter off, bosses out in the world aren't followed.

### Fixed
- **Teams on the Party screen** are read far more often, look-alike Aniimo aren't mixed up, and each run sends its own teams instead of the previous run's.
- **Special Offerings and Season Buffs** are read wherever the game draws the panel, including offerings like Resurgence, a change made late, and someone else's party. Your own lobby's Season Buffs no longer go with their run.
- **Party screens aren't missed or cut short:** one is read right up to the fight, even when its title reads oddly. A name missed on one read no longer drops that player, and lead Aniimo and your own name stay on their cards.
- **Battle Data numbers are read far more strictly:** each comes from its own spot on its own card once the cards stop moving, and only goes out after two reads agree. One player's numbers can no longer land on another's card.
- **Result pictures are the screen that was read,** never a loading screen the game had moved on to.
- **Names go to the leaderboards correctly:** a misread on the Party screen can't replace a clearer read from the result screen, a Korean name read from your menu or Profile stays Korean, and AI teammates the game renames stay marked as AI.
- **Egg Heist results are read properly:** Loot This Match and Total Defeats are sent once two reads agree, a match can't be sent twice, and leaving the rank screen quickly doesn't lose your rank.
- **Sanctum Helper's maps in Egg Heist:** they stay up after Interlink or Holo-Battle Sim and with the "Escape Condition" panel showing, and they go away after you leave. They no longer show in a Pathfinder challenge.
- **Sanctum Helper keeps your spot on the map screen:** backing out of the map (M, or Select on a controller) carries on from where you were.
- **The DPS meter no longer starts on the Party screen:** a player's title banner there could pass for a boss bar.
- **HP labels stay put:** a boss bar read short could put a label over its right end, and then labels disappeared or the HP % froze.
- **Dragging the DPS window or an HP label no longer loses it off screen,** and a spot saved off screen is forgotten so the window comes back.
- **General Settings no longer has a second scrollbar.**

## 0.1.35

### Fixed
- **Holo-Battle Sim's Gizmo screen** is read again after a win without a first clear: that Victory screen has no
  Rewards Earned row, so its coins, gizmo counts and gizmo row sit lower, and aLogs read none of them and gave
  control back at once. It reads both layouts now, and opens the Gizmo screen at the right spot.
- **Holo-Battle Sim's gizmos** are recognised when aLogs starts while you're already in a Sim run.

## 0.1.34

### Fixed
- **Holo-Battle Sim wins** are sent again when the last boss's bar vanishes on the kill: the Victory screen is read
  even while the meter is still waiting for that bar, so leaving it quickly no longer loses the run.

## 0.1.33

### New
- **A notice when a record is sent:** each time a run, a Holo-Battle Sim win or your Egg Heist rank reaches the
  leaderboards, a small notice over the game says what was sent and to which board ("Interlink run sent · Alpha
  Scorchhowl · 17 stars · 17,061"). Set it up in General Settings › **Notifications**: where it shows (top right,
  top middle, middle right, middle left, or anywhere you drag it), how it comes in (fade, slide or drop), or turn it
  off. Sanctum Helper's notices ("Chaos Mode found") show the same way and wait their turn, so two never overlap.

### Fixed
- **Beta Menu bug reports** say which aLogs you're on, not an old test list's build.

## 0.1.32

### Fixed
- **Sanctum Helper's Auto difficulty** reads the party screen again when you queue for Egg Heist from your homeland,
  and catches a difficulty picked just before you start.

## 0.1.31

### New
- **Two-boss Interlink stages:** when the middle stage has two bosses one after the other, aLogs follows them as one
  run on one clock. The DPS window and the Dashboard show **Boss 1/2**, then **2/2**, and the run goes to your Battle
  History and the leaderboards under the stage's first boss, with both bosses' damage and each boss's part. Both
  bosses count toward the middle's 40 stars, and your best is kept for the stage, whichever boss the run ended on.
- **A warning when Windows can't read the game's text:** without Windows' English text recognition (OCR), result
  screens, boss names, Battle Data and the prep and Party screens can't be read, so runs aren't recorded or sent. The
  Dashboard and the DPS Meter page now say so, and **Install English OCR** adds it for you (Windows asks for admin
  permission). English added in Windows Settings while aLogs runs is picked up without restarting.

### Improved
- **Interlink time limit:** the run's clock is read off the game's timer as the fight starts (or from the Overtime
  offering on the Party screen), so with Overtime (4:00) a kill after 3:00 counts as a clear.
- **Player names:** names on the result and Battle Data screens are read as they appear on screen, and the name in
  your game menu, which links your character, is read by the same name reader.
- **Result screens:** the time and each player's stats are read with the game's own digits.
- **Battle Data:** the cards are counted on screen, so each number lands on its own card even when the text reader
  misses one.
- **Smoother and lighter:** the first read of a result, Party or Battle Data screen no longer holds the meter up for
  about a second, and the name reader uses far less memory.
- **The button over the game blinks less:** it now waits 0.75 s by default before hiding with the game's icons. If
  yours still blinks, pick a longer wait in General Settings › The button over the game.

### Fixed
- **Interlink HP bars:** the counter is read from ×21 down (×17 to ×21 were skipped), and damage follows the game's
  own bars, so an Interlink run's damage and DPS are right from its first bar.
- **Interlink fights** no longer start on a boss level misread as the bar fades in, which ended the fight a minute
  later and started it over.
- **Bosses above your Aniimo's level:** the game shows their level in red, which aLogs couldn't read, so the fight
  was never recorded. It's read now.
- **Battle Data:** each player's Avg DPS, BREAK, damage taken and healing are their own, also when the result screen
  missed a teammate (the players after them could take the next player's card, yours included).
- **You and your teammates in a run:** your row keeps your name and is marked as you, and no teammate is listed twice
  (with their Battle Data twice). The Party screen's read could take over a right one.
- **Teammates in a run:** a teammate whose name or stats on the result screen weren't read is no longer left out of
  the run, and their Battle Data goes with them.
- **One-character names:** a teammate whose name is a single letter or digit shows as it is, not blank or as another
  character, also when the Party screen misread it.
- **Text recognition in any English:** on a PC with English OCR other than US English (UK, Australia, India...), the
  meter starts and reads the game's screens. Before, it didn't start at all.
- **Ultrawide screens:** the boss bar is found at 3440×1440.
- **Warning titles** are easy to read in the Meadow theme and in a light Custom theme.

## 0.1.30

### New
- **A Leaderboards menu:** everything about the leaderboards, linking your character included, is in one place.
- **Holo-Battle Sim end:** after you press End Holo-Battle Sim at the exit gate, aLogs holds the Victory screen and
  reads every gizmo, then gives control straight back.

### Improved
- **A clearer DPS window:** your average DPS up top, then Last 5s, Team damage and Time, with the numbers lined up.
- **Ctrl+Alt+F12** gives you control back at any moment, as Ctrl+Shift+F12 does.
- **Egg Heist's settings** sit on the Sanctum Helper page.

### Fixed
- **HP labels** show on the boss bar again every fight.
- **Battle Data:** a downed teammate's card no longer drops the run's numbers.

## 0.1.29

### New
- **Korean names:** teammates with Korean (Hangul) names show on your runs as they are, not as blank or garbled
  letters.
- **A new text reader for player names:** names on the Party, result and Battle Data screens are read far more
  reliably. Scores, stars and Battle Data numbers are now matched against the game's own digits.

### Improved
- **Teammates keep one name:** a teammate read slightly differently on one run is still sent under their usual name.
- **Leaderboards rank players who run aLogs** and have claimed their character; other teammates are listed in the
  run.
- **HP labels:** the word HP takes the boss bar's colour, HP and BREAK have a thicker outline, and out in the world
  the labels line up with the end of the bar.
- **Opening aLogs while it's running** brings its window back from the tray.
- **Your character's link, profile numbers and online status** work whenever you're signed in, even with "Submit my
  records" off.
- **Report a problem** includes pictures of the boss bar when the meter loses track of it.

### Fixed
- **Scores:** a 1 read as a 7 (17,164 as 77,764) no longer reaches the leaderboards, and star ratings read right at
  every resolution.
- **Battle Data:** a 7-digit BREAK value no longer moves the card's other numbers up a row.
- **Overworld bosses** such as Alpha Bouldus and Alpha Minespine are found and followed again.
- **Holo-Battle Sim:** a win ends the fight on the Victory screen, so the meter hides and the run is read and sent.
- **Log reports:** their pictures arrive intact.
- **Lock the DPS window** shows as on when the window is locked.
- **Sanctum Helper's settings:** the key buttons show the new key, two quick Markers changes both stick, and errors
  stay until you close them.
- **Egg Heist's fade hold** works from its own switch and for as long as its setting allows.

## 0.1.28

### New
- **Sign in with Discord** from the sidebar, in a window of aLogs. Signed in, your account shows as a Discord button
  with a menu: your profile, your account, sign out.
- **aLogs updates itself:** a new version shows in the sidebar; **Install now** or **Next time aLogs opens**. Betas
  are opt-in (About › Updates › Get beta updates).
- **An on/off button in the game's icon row,** next to Chat, with a hotkey (Insert by default) or a controller combo.
  It turns the meter and Sanctum Helper off and on; each also has its own switch in the sidebar.
- **Holo-Battle Sim on the leaderboards:** after a win, your time, stage, difficulty and setup from the Victory
  screen go to that stage's board.
- **Interlink:** every run goes to your Battle History with its party, buffs and Battle Data, under the new 20 / 40
  / 20 star rules.
- **Egg Heist:** your matches and season rank reach the leaderboards.
- **Your profile:** your in-game photos (pick them or upload new ones automatically), collection progress, level and
  EXP, catches, and an Online / AFK / Offline status.
- **DPS window options** (DPS Meter › Overlay): spots for each mode, a dark background, a lock, and the result kept
  on screen after a kill ("Defeated <boss>").
- **"You're being attacked"** pops up over your other windows when you're hit while tabbed out.
- **Boss kill sound:** the game's kill sound dates the end of a world fight to the killing blow.
- **Report a problem** on the Dashboard: send your logs, a note and screenshots to the developers.
- **Sanctum Helper:** its map can sit over the game's minimap (experimental), and it knows the Whisperwake layouts.
- A **setup guide**, a **name** (aLogs: Aniiloger's Companion App) and a **new icon**.

### Improved
- **Screen capture rebuilt:** aLogs now watches the game on your graphics card, the way OBS does. Far less lag and
  stutter, especially at 4K and while streaming, and lighter between fights. It runs at a lower priority by default (General Settings › Performance).
- **Overlays in recordings:** the DPS window and Sanctum Helper's maps show in screenshots and recordings on Windows
  10 and 11 when you want them; pictures sent to the leaderboards stay the game alone.
- **A faster meter:** it finds boss bars and reads screens much faster, and most settings apply without restarting it.
- **New bosses right after a patch:** boss names and HP come from your own game files, including the Whisperwake
  update's new HP and BREAK formulas.
- **Sanctum Helper** keeps up with fast turns, uses far less memory, and works on 16:10 and ultrawide screens.
- Settings are organised in tabs, Updates moved to About, and closing aLogs is instant.

### Fixed
- **Fight times:** world fights start at the first hit and end at the kill, and BREAKs and ultimates no longer cut
  the clock short.
- **Reading the screen at every size:** the boss bar, the ×N counter and names read right from 720p to 4K, and a
  level like "Lv064" no longer reads as Lv 6.
- **Names:** short names, misread names and teammates' names are read right, and Battle Data cards carry each
  player's name.
- **Disk space:** the text reader no longer leaves files in Windows' Temp folder (up to 1.8 GB on one PC).
- **Controller blocking** could stop the keyboard, mouse and controllers after a restart.
- **Sign in again** right after cancelling now always opens the Discord window.

## 0.1.27

### New
- **Egg Heist matches:** the meter reads each match's end screen (escaped or failed, difficulty, time, Loot This
  Match, Total Defeats, chests today) and sends it to the new **Egg Heist › Matches** board, with the screen as
  proof. The Rank Info screen's Egg Emperor stars are read at every resolution now too.
- **Every screen at every common resolution:** results, prep screens, Challenge Stats, Battle Data and Egg Heist
  read the same at 720p, 1080p, 1440p and 4K (checked against real screenshots at each size).
- **Sign in with Discord, right in the app** (DPS Meter › Leaderboards & account): Discord's page opens in your normal
  browser, you approve, and aLogs is signed in. No more copying a desktop token from the website. The AniiLogs page
  inside the app is signed in with the same account automatically, and Sign out works from the app too.
- **AniiLogs inside the app:** a new AniiLogs page shows the website itself (Leaderboards, Map, Aniilog,
  Item-log, Team Builder and Profiles as tabs), always identical to aniilogs.github.io. Sign in there once and your
  marks sync with the site. Links that leave AniiLogs open in your browser.
- **Checklists per character:** map, Aniilog and Item-log progress is kept for each character (UID), so characters
  on different servers have their own. In the app it follows the character you're logged into; on the website you
  pick it in Settings › Linked characters. Each character starts from what you had tracked before.
- **Far less disk space:** the meter reads each screen's numbers into your records and only keeps a picture when it's
  needed (leaderboard proof, until it has been sent). Kept pictures are small JPEGs (~0.25 MB instead of ~5 MB),
  leftovers are cleared every hour, and the diagnostic captures are off by default. DPS Meter › Captures & logs ›
  Keep screenshots can keep them for 6 hours, 1 to 7 days, or for good.
- **Only your best runs go to the leaderboards,** once each: a run is sent when it beats one of your bests on that
  boss that week: score (most stars, then highest score), fastest clear, or your own Average DPS, BREAK, damage taken
  or healing from Battle Data. Every other run stays in your own records. The boards can rank by each of these.
- **Screenshots as proof:** with "Attach the result screenshot" on, a best run goes with its result screen and its
  Battle Data, each a small 1280×720 picture. Anyone can hover the picture icon next to your entry to see them
  (Summary and Battle Data tabs). Your PC keeps no copy once they're sent.
- **An on/off switch for the meter** (DPS Meter › Meter › Run the DPS meter), for when you only want
  Sanctum Helper or the AniiLogs pages.
- **A much smaller app:** Sanctum Helper now uses Microsoft's .NET Desktop Runtime (9 or newer) instead of carrying
  its own ~150 MB copy. The installer offers to install the runtime if it's missing, and so does the Sanctum
  Helper page.
- **Leaderboards by server:** records now say which game server they were played on (the app reads it from the
  game's settings, e.g. Aniimo-Americas), and the AniiLogs boards have a Server picker.

### Improved
- **Egg Heist: block from the fade to black** (Results, off by default, experimental): when a match ends, button
  presses (and your controller, if it's blocked too) are held back from the fade to black until the end screen is
  read, so a stray press can't skip it. Needs Sanctum Helper running.
- The AniiLogs page's **Profiles** tab opens your own profile directly, with a player search at the top.
- **The meter notices the boss bar almost instantly.** It checks the spot where bars appeared before ten times a
  second, which takes under a millisecond, so a fight starts on the bar's first frame. The full search (now about
  40 ms at any resolution, down from 155 ms at 1080p) only runs when a bar shows up somewhere new.
- **Interlink fights start the instant the bar appears:** the prep screen already said which boss it is, so the
  meter no longer waits to read the name above the bar (it still reads it, to double-check).
- **Controller blocking is built in.** The installer no longer installs HidHide. Results › Controller › Set up
  installs just its small driver (Windows asks for admin approval once), with no extra program, shortcut or
  Programs entry. Setup now also blocks the XInput side of Xbox-style controllers, which most games read.
  Uninstalling aLogs removes the driver again. The installer is about 8 MB smaller.
- **See that controller blocking works:** Results › Controller lists each connected controller by name (XInput or
  DirectInput). Press a button and its row lights up, so you know which controller is which. Each one has its own
  Block switch, and **Test blocking** hides them for a second to prove the game would lose them, then shows them
  again. Works with virtual controllers from reWASD too.
- Set up now repairs a HidHide install that never attached to your controllers (it looked installed but blocked
  nothing).

### Fixed
- The meter no longer mistakes a File Explorer folder (or anything else) called "Aniimo" for the game: the dashboard
  said "Game window found" with the game closed. It now only takes the game's own window.
- The AniiLogs page loads the website, and opening it no longer leaves an invisible layer that stopped tabs and
  buttons on the other pages from being clicked.
- **The dashboard no longer gets stuck on "Starting…"** and your character no longer looks reset after an update:
  the hourly screenshot cleanup could delete the meter's (empty) send-queue folder, which stopped its status
  updates. Right after the game starts, the dashboard now says it's waiting for the game log: your UID is read fresh each
  session (so it's always the character you're actually on), and your saved name follows the moment it appears.
- Fixed an occasional "int too long to convert" error when capturing the game window.
- **No more endless ".NET" prompts.** Updating could leave pieces of an older Sanctum Helper behind, which made
  Windows ask you to install .NET every time it started. The installer now replaces the bundled programs cleanly,
  closes Sanctum Helper before updating, and counts any .NET Desktop Runtime 9 or newer (including .NET 10) as
  already installed.
- **No more wrong or misread boss names on the boards.** In Interlink the meter only matches this week's three
  bosses and always uses their real names, so "Tucking" or "Omega Tuckin8" become Omega Tucking. A run only goes to
  the leaderboards when the game's own log confirms you were in an Interlink scene. Fights in the open world are
  never counted as Interlink, and the meter remembers the scene for all three bosses of a visit, even if it started
  partway through.
- The note over the result screen no longer lingers in the old spot for a second when Battle Data opens or the
  screen closes: it disappears as soon as the screen changes and comes back in the right place.

## 0.1.26

### New
- **Proof of your character:** when the app reads your name in the game menu (Esc or Start), it sends that part of
  the menu (your picture, name and UID) to AniiLogs as proof. Proven characters can't be claimed by anyone else.
- **Disputes:** if another account proved your UID first, DPS Meter › Leaderboards & account offers to send a
  dispute with your proof, for a developer to check.
- **Your whole team on the boards:** runs now say which party members are real players and which are AI, so every
  real teammate gets their own leaderboard row. When two app users send the same run, each one's UID is recorded
  on the other's run.

### Fixed
- The party on the prep screen is read wherever it sits (it moves with the boss's challenge text), all four slots
  are recognized (3P and 4P were missed), and your own slot is found by your green name, leader or not.
- Teammates' names are read the same way on every screen, so one player never shows under two spellings.
- In Interlink, a boss name read without "Omega" could be taken for the open-world Alpha of the same name, with
  wrong HP. Only this week's Interlink bosses are considered now, and a far-off HP reading is never saved.

## 0.1.25

### New
- **Your name from the game menu:** opening the menu (Esc or Start) shows your name with your UID under it. The app
  reads it there, checks the UID is yours, and from then on uses that name, correcting any earlier guess.

### Fixed
- In someone else's party (when you aren't 1P), the app could take another player's name as yours and
  link it to your account. It now only saves your name when it's sure: your saved name is on the result screen,
  or everyone else in the party was AI.

## 0.1.24

### New
- **Themes:** General Settings has a new Themes tab with the AniiLogs website's themes (Night, Meadow,
  Emberpup, Pawney) and Custom colors. It changes the app window only; the overlay in the game stays the same.
- **Update notice:** when a new version is out, About gets a dot in the sidebar and an Update button next to the
  patch notes that opens the download page.
- **Patch notes stay current:** About shows the last five releases straight from GitHub, so you can read what's
  new before you update. Each one opens when you click it. If your version is older than all of them, a sixth
  entry says how far behind you are.

### Fixed
- Your character name could show as "Name appears after your first result screen" after an update or restart
  until you finished another fight. The saved name now comes back as soon as the game is running.

## 0.1.23

### New
- **Patch notes in the app:** About now shows what changed in the last five updates.

### Improved
- The installer shows which version you're installing, in its title and on every page.
- Clearer wording across the app: every option now says plainly what it does, and outdated descriptions (where the
  DPS window sits, which HP labels show) match the current meter.
- The notes over the result and Battle Data screens are shorter: the boss, score, stars and time, and the game's
  team DPS next to the meter's.

## 0.1.22

### Improved
- **Interlink clears now follow the game's rules.** A run counts as a clear when it reaches 15 stars or more and the
  leader ends it early. Its time is your clear time on the AniiLogs boss boards.
- Boss boards on the site rank by stars, then score, then fastest clear.

### Fixed
- A run the leader quit early (for example at 0 stars) was sent as a kill, so a 0:23 "fastest kill" could show
  on the boards. Those runs no longer count as clears, and the site hides the ones already sent.
- Late in an Interlink fight, once the xN box next to the HP bar disappears, the meter could misread how much HP
  was left. It now keeps reading the bar correctly to the end.

## 0.1.21

The first public release.

### New
- **Boss DPS meter** in the game's own font, under the stars and timer in Interlink and under your quest list out in
  the world: team DPS, the last five seconds and team damage, with the boss's HP left on its bar.
- **BREAK tracking:** break points left, how fast your team drains the gauge, and how many breaks you landed.
- **Results kept for you:** every result screen and Battle Data screen is saved with its numbers. The app can open
  View Battle Data for you.
- **Leaderboards (opt in):** your Interlink scores, stars and clear times go to the AniiLogs leaderboards under your
  in-game name. AI teammates are marked. Each week's bosses are read from your game files, so new weeks appear on
  their own.
- **Sanctum Helper** (by sargeeva) built in, with its options on their own page.
- **DPS Meter page** with tabs for the meter and overlay, results, captures and logs, and leaderboards and account.
