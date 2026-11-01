# aLogs — patch notes

Each `## x.y.z` section becomes that version's GitHub Release notes (`npm run release`). Write for players: what
changed for them, not how it was done. Group under the headings that apply: **New**, **Improved**, **Fixed**.

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
- **Screen capture rebuilt:** aLogs now watches the game on your graphics card, the way OBS does, instead of copying
  the whole screen many times a second. Far less lag and stutter, especially at 4K and while streaming, and lighter
  between fights. It runs at a lower priority by default (General Settings › Performance).
- **Overlays in recordings:** the DPS window and Sanctum Helper's maps show in screenshots and recordings on Windows
  10 and 11 when you want them; pictures sent to the leaderboards stay the game alone.
- **A faster meter:** it finds boss bars and reads screens much faster, and changing a setting no longer restarts it.
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
- **An on/off switch for the meter** (DPS Meter › Meter & overlay › Run the DPS meter), for when you only want
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
