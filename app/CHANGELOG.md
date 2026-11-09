# aLogs — patch notes

Each `## x.y.z` section becomes that version's GitHub Release notes (`npm run release`). Write for players: what
changed for them, not how it was done. Group under the headings that apply: **New**, **Improved**, **Fixed**.

## 0.1.36

### New
- **Egg Heist runs have their party:** who played, their lead Aniimo and their team are read from the Party screen and sent with each match.
- **Egg Heist's Party and rank screens go with the run:** both pictures are in the run's Battle History. Inputs are held for a moment while the end and
  rank screens are read, so a press can't skip past them.
- **Show an example of the DPS window:** DPS Meter › Overlay can now show it with made-up numbers, so you can place it without a fight.
- **Each part of aLogs runs on its own switch:** the DPS meter, Sanctum Helper and sending to the leaderboards turn
  on and off independently, and none needs another. With the DPS meter off, your records are still read and sent
  (nothing of the meter shows over the game), and Sanctum Helper keeps running.
- **The button over the game turns everything off, and back on:** one press turns off every part it controls; the
  next turns back on the ones that were on (sending to the leaderboards only if it was on before). Pick which parts
  it controls in General Settings › The button over the game. A notice says "aLogs off" or "aLogs on" with what it
  switched; turn it off in General Settings › Notifications.
- **A status line in the sidebar** under a part that needs a look: not signed in to the leaderboards, Sanctum Helper
  needing .NET 9, or a part stopped after errors.
- **Windows 10: capture only the game window:** a new switch in General Settings keeps other programs' overlays
  (Discord, Steam, notifications, windows on top) out of what aLogs reads and out of your proof pictures. Windows then
  draws a yellow border round the game, only while aLogs is capturing. About explains both ways.

### Improved
- **Checking for updates copes with GitHub hiccups:** it tries again when a download server is having trouble, and
  an update it found still shows if its download fails.
- **Teams and buffs on the Party screen read faster:** a read takes under half the time it did, and much less when
  the screen hasn't changed, with the same answers.
- **A menu best the site already has isn't sent again:** before sending the best score shown on the battle prep
  screen, aLogs checks the site's board; if your row there already has that score (from a run you or a teammate
  sent), nothing goes up.
- **An unchanged Egg Heist rank isn't sent again:** opening Rank Info with the same rank, rank points and stars the
  site already has sends nothing, even after aLogs restarts.
- **Notices belong to their part:** each shows only while the part it comes from is on, and they all show with the
  DPS meter off. "Tell me when a record is sent" moved to Leaderboards › Sending; where notices show stays in General
  Settings › Notifications, and your spot is kept.
- **A problem in one part no longer stops the others:** if sending to the leaderboards or the DPS window keeps
  failing, only that part stops and the sidebar says so; turn it off and on to try again.
- **Lighter with only sending on:** with the DPS meter off, bosses out in the world aren't followed.
- **Battle Data opens sooner after an Interlink result:** View Battle Data is clicked as soon as the result screen
  has been read (often within a second), instead of about 4 seconds in, and no longer waits on a fresh picture of
  the game (when that stalled, the click came late or not at all). The game picture no longer drops out for several
  seconds right after the result screen shows, which held the click up by 7 to 27 s. On a busy graphics card, aLogs no
  longer waits for it while holding the picture back: the meter always has the newest finished picture to read.
- **A run is sent as soon as both pictures are taken:** the result screen, then Battle Data; once every card is read,
  the run goes at once and aLogs lets go of your inputs, instead of holding on for 5 more seconds. The Battle Data
  picture is always the screen that was read (one run's was a loading screen when the game moved on first). The
  numbers go once two reads in a row agree, so a read while Battle Data is still fading in is never sent.

### Fixed
- **Special Offerings and Season Buffs in someone else's party:** the offering is read there too, and your own lobby's Season Buffs no longer go with that run.
- **Result pictures are of the game itself:** the pictures sent with a run on Windows 10 are copies of the game window, without anything on top of it.
- **Result, Party and menu screens are read from the game itself:** on Windows 10 they now come from the game window, not from what's
  on top of it. Windows shows a yellow border around the game while one of them is up.
- **Dragging the DPS window no longer makes it disappear:** it could jump off screen and stay there. The HP labels had the same
  problem. A spot saved off screen is forgotten, so the window comes back.
- **Sanctum Helper's maps stay up in Egg Heist:** it could fail to recognise an Egg Heist run after Interlink or Holo-Battle Sim,
  and its own "Escape Condition" panel could make it hide the maps.
- **Egg Heist results are read properly:** Loot This Match and Total Defeats are read by the game's own digits and only sent once
  two reads agree, and a match can no longer be sent twice. Loot could be sent as a single digit.
- **General Settings no longer has a second scrollbar:** it left empty space at the bottom of the window.
- **The Special Offering you changed to is the one sent:** its circle on the Party screen is checked twice a second, so a late
  change no longer sends the old one.
- **Fewer blank slots in your team:** a team slot the Party screen couldn't read at first is tried again on the next read, and
  a slot of yours still blank is read from the right side of the screen during the fight.
- **Battle Data numbers are read far more strictly:** each one comes from its own place on its own card, by the game's own
  digits, once the cards have stopped moving, and only goes out after being read the same way twice. Anything less stays
  blank. A first read could put one player's Damage Taken and Heal on another's card until a later read corrected it.
- **AI teammates the game renames stay marked as AI:** one could go to the leaderboards as a real player.
- **Teammates' names are sent as the result screen reads them:** those are the clearer read, and a name misread on the
  Party screen could replace a right one and add a wrong player to the leaderboards.
- **Party cards keep their lead Aniimo and your own name:** a card could go up with no lead Aniimo, and yours with a
  misread of your name.
- **The Party screen is read right up to the fight:** teams changed or an offering chosen late could be missed when the
  screen's title was hard to read.
- **Special Offerings like Resurgence are read:** some were never recognized, so the run went in without its offering.
  aLogs rebuilds its icons once when it first opens after this update.
- **A Party screen is no longer missed when its title reads oddly:** the title could be read with a stray space, and
  that run then had no teams or buffs. The Profile screen had the same problem.
- **The updating window shows the animated aLogs icon,** not a still picture (the app's security settings kept the
  animation from loading).
- **Everyone's team on the Party screen** is read far more often: each player's three other Aniimo are found by their
  skill icons and compared the way the game draws them, faded into the card, so a card sitting a little off (or
  higher, once everyone is ready) no longer hides them, and look-alikes (Luminelle and Iris) aren't mixed up. On 26
  recent Party screens, 299 of 312 were read, up from about half. Each run now sends its own teams: the newest read
  of each card counts, so a run no longer goes out with the previous run's teams or with only one or two of them.
- **The Special Offering and Season Buffs** are read wherever the game draws the panel (it sits a little higher or
  lower on some screens), and a later read that misses one no longer drops it.
- **Sanctum Helper's map no longer shows in a Pathfinder challenge:** the game loads a challenge like an Egg Heist
  island, so the island map came up over it. A challenge's "Victory Conditions" panel now tells them apart.
- **The DPS meter no longer starts on the Party screen:** a player's title banner there could pass for a boss bar
  and open a fight before the run began.
- **Opening Battle Data automatically is faster and sure:** aLogs clicks as soon as the result screen is read (1-4 s,
  not always 5), and moving your mouse meanwhile no longer pulls the click off the button.
- **"Run sent" notices come sooner:** a run goes to the leaderboards a few seconds after its Battle Data is read,
  not only once you leave the screens.
- **Every player stays in the run's party:** one Party screen read that missed a name (as the screen fades into the
  fight) no longer drops that player from the run.
- **Teams are read on the first run after an update:** the game's icons load when aLogs starts, not at the Party
  screen, where rebuilding them could take until the fight had begun.
- **A heads-up when Discord's overlay is in the game:** Discord draws its overlay inside the game's own picture, so
  its voice widget (names and avatars) ended up in proof pictures. aLogs now says so once, with where to turn it off.
- **The updating window** shows the aLogs icon as its own loader (its HP and break bars fill while the update installs)
  and says which beta it's installing (0.1.36 B4, not just 0.1.36). Its icon no longer goes missing.

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
