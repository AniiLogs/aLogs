// Generated from CHANGELOG.md by patch-notes.mjs - edit the changelog, not this file.
window.PATCH_NOTES = [
 {
  "version": "0.1.28",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**A name:** the app is now **aLogs: Aniiloger's Companion App** (aLogs for short), in its window, the sidebar, About, the installer and the tray. Your settings, records and install stay where they were.",
     "**A new icon:** Eklue's emblem inside an HP dial and a break bar, ringed by six of the game's biggest buffs, on the desktop, the taskbar, the tray, the installer and the sidebar.",
     "**aLogs updates itself:** it checks for new versions on its own and shows them in the sidebar, with a progress bar while one downloads. **Install now** closes aLogs, installs with just a progress bar (no installer to click through) and opens the new version; **Next time aLogs opens** installs it then. Updates download in the background unless you turn that off (General › Updates), nothing installs without your choice, and only one downloaded update is ever kept. Betas update the same way: General › Get beta updates, and anyone already on a beta keeps getting betas until the release catches up. About shows exactly which build you have (for example \"0.1.28 · RC10\"). (This version still installs the usual way.)",
     "**Setup guide:** a few short steps when aLogs starts: sign in with Discord, choose what goes to the leaderboards, and how result screens are handled. It turns itself off once you finish or skip it, and General can show it again.",
     "**Sign in to sync** in the sidebar, under the meter's state: one click to sign in with Discord, and once signed in it shows who you are (click it for the account settings).",
     "**DPS Meter › Overlay,** a tab of its own, with a picture of each spot the DPS window can sit in, per mode: out in the world right of the minimap (new, clear of the quest list and the rewards after a catch) or under the quest list; in Interlink the top-left corner next to Esc (new) or under the stars and timer. Every spot scales to your screen (16:9, 16:10 and ultrawide). If you had moved the window, it starts from its spot again.",
     "**A dark background for the DPS window** (DPS Meter › Overlay): a slider from none to solid, with a live preview, styled like Interlink's stars and timer panel, to keep the numbers readable over bright effects.",
     "**The fight's result stays on screen:** when a boss goes down, the DPS window shows the final numbers (the last hit counted, \"Defeated\", the fight's time) instead of disappearing. Out in the world you choose how long (10 seconds by default) in DPS Meter › Overlay.",
     "**Being attacked while you're in another window:** like the game's own \"Under attack\" warning, \"You're being attacked in Aniimo\" pops up for 2 seconds every 5 while your boss fight goes on, with the red flash around the screen's edges the game shows when you're hit. Click the text to go back to the game. A fight that ran mostly while you were away shows no result.",
     "**Boss kill sound** (DPS Meter › Meter › Audio cues, on by default): out in the world, the sound the game plays with the killing blow dates the end of the fight to the hit itself. aLogs listens to Aniimo's audio only (nothing else on your PC), only during a boss fight, and records nothing. Without it (or with the game muted) the fight ends 3 seconds before the Catch Countdown, as before.",
     "**Egg Heist after each match:** the rank screen that follows a match updates your season standing on the leaderboard, and your profile gets the Rank Points the match added and your progress bar (\"1337/1350 +18\"); at Egg Emperor, your egg count. Rank Info also sends where your next rank starts, for your profile.",
     "**Report a problem** (Dashboard): something wrong with aLogs? Send your logs to the AniiLogs developers in one click, with a note about what went wrong if you like. One report covers the whole app: the meter's log, your 20 newest fights' readings, your profile notes, Sanctum Helper's log and both modules' settings (never your sign-in), with your game UID, the time and which build you have. Reports are kept privately and cleared once the problem is fixed, after 30 days at most.",
     "**Profile notes:** the meter notes your **catches** (\"Irisal Lv 55\") and your **level and current EXP** each time the EXP card shows, for your AniiLogs profile. It keeps the card's final numbers (not ones caught mid-animation), and never notes EXP going backwards."
    ]
   },
   {
    "heading": "Improved",
    "items": [
     "**The meter reacts almost instantly:** it looks for a boss bar 20 times a second, anywhere on screen, and its full search is about ten times cheaper than before. During a fight it reads the bar 30 times a second (was 10). Result screens, Battle Data and the Catch Countdown are read without ever pausing the meter, and text is read about four times faster (a full screen in ~60 ms instead of ~220 ms) with exactly the same results. The meter starts in a fraction of a second, and between fights it eases off while you're in another window.",
     "**Out in the world the fight's clock starts at the first hit**, not when the boss's bar shows up as you get near it, so standing around before a fight no longer drags your DPS down.",
     "**The overlay only shows over the game:** tab out (to Discord, a browser…) and it gets out of the way at once, then comes back when you return to the game.",
     "The line under the boss bar now reads **HP** (what the boss has left) instead of \"Total\", and the numbers on and under the bar have a soft dark shadow, so they stay readable over bright effects.",
     "**DPS Meter settings in tabs:** Meter, Overlay, Results, Captures & logs, and Leaderboards & account. Each thing lives in one place: the log folders, Report a problem and your character (name, UID, proof, disputes) are on the Dashboard; you sign in from the sidebar, and Leaderboards & account shows your account and what's being sent; the game window's title and **Overlays show in recordings and screenshots** (one switch for the DPS window, the HP labels and Sanctum Helper's maps) are in General Settings for the meter and Sanctum Helper alike; updates install from the sidebar; and Sanctum Helper's version and credits are in About, which links aLogs' own code too (github.com/AniiLogs/aLogs).",
     "**Changing a setting no longer restarts the meter**, so a fight in progress isn't lost. Only the overlay's and the game window's settings still restart it.",
     "**Sanctum Helper keeps up with you:** the view cone follows the cone's edges instead of its brightest part (bright floor and walls no longer pull it sideways), the cone and your character's arrow follow fast turns without trailing, your spot on the map moves with you (dashes show in about half the time), and the open full map follows panning and zooming right away. Its map matching is 2-3 times faster, with the same results.",
     "**Sanctum Helper uses far less memory:** it held about 600 MB from startup, even with no run, and grew run after run. Maps now load when you enter a Sanctum and are let go after the run.",
     "Leaderboard records say which aLogs version and build sent them, so a future board that needs a newer aLogs can ask for it. Older versions keep sending to the boards they already use."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "**Closing aLogs is instant:** it used to hang for about 10 seconds while it waited on the meter and Sanctum Helper. They now finish up on their own (an open fight is still saved) and close within a fraction of a second.",
     "**No more filling up your drive:** the meter's text reader left a small picture behind for nearly every read, in Windows' Temp folder, and never cleaned up (on one PC: 600,000 files, 1.8 GB). It now keeps none, and the first start clears what earlier versions left. Each fight's per-frame log is also cleared after 30 days (your fight results, profile notes and leaderboard records are kept).",
     "**The fight's time is right through BREAKs and ultimates:** the meter keeps reading the boss's HP under the BREAK overlay (it used to freeze there, which gave impossible sub-10-second kills), and when an ultimate finishes the boss, the fight runs to the killing blow, not only to when the cutscene hid the bar.",
     "**One-character names:** a name like \"0\" read from the game menu was taken for a misread and dropped, so that player's character never got a name and the leaderboards (which list players by name) hid their records.",
     "**Egg Heist rank always has its season:** when Rank Info's season title hadn't appeared yet, the rank was filed without a season, as a separate entry; it now takes the current season.",
     "**Egg Heist matches reach the leaderboards:** in Team Mode the end screen's Loot This Match and Total Defeats sit higher than in solo, and the meter read neither, so matches were sent without their score and never showed.",
     "**Out in the world fights end at the kill:** the meter ran each fight about 2.5 seconds past the killing blow, through the slow-motion kill-cam to the Catch Countdown, so your DPS showed 12-17% low.",
     "**The last-5-seconds DPS no longer spikes after a BREAK:** while BREAK hides the boss's HP, its damage shows all at once when the bar comes back, and the meter used to count it as dealt in a split second (100,000+ DPS for a moment). It's now spread over the time it really took.",
     "One misread frame of the boss bar (often as it slides in) no longer freezes the fight's damage until the boss drops below it: a sudden big drop only counts once a following reading confirms it.",
     "**The xN counter reads right at 720p:** below 1080p \"x16\" could be read as \"x6\" in Interlink.",
     "**Sanctum Helper on 16:10 and ultrawide screens:** it read the minimap from the wrong spot (on 3440x1440 well to the left of it); it now finds the game's UI the way the meter does on every screen shape.",
     "Sanctum Helper's run timer no longer flickers between \"seen\" and \"lost\".",
     "Fewer false boss bars: during a fight the meter only follows that fight's own bar (never another bar-like shape while the boss's bar is hidden), shapes far smaller than any boss bar are ignored, and static shapes in the lobby, Interlink's rooms or Egg Heist (menu frames, red slivers) that name no boss and never change are ignored after 10 seconds.",
     "The numbers under the boss bar disappear with the bar: they no longer float over ultimates, cutscenes or the Catch Countdown.",
     "A fight starts on the first read of the boss's name even when \"Lv\" is misread (LV, LY, LN), up to a second sooner.",
     "An open Battle Data screen (one opened again from a menu) is read once, not every 30 seconds.",
     "\"No boss bar\" diagnostic pictures are only taken in Interlink, not in the lobby."
    ]
   }
  ]
 },
 {
  "version": "0.1.27",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**Egg Heist matches:** the meter reads each match's end screen (escaped or failed, difficulty, time, Loot This Match, Total Defeats, chests today) and sends it to the new **Egg Heist › Matches** board, with the screen as proof. The Rank Info screen's Egg Emperor stars are read at every resolution now too.",
     "**Every screen at every common resolution:** results, prep screens, Challenge Stats, Battle Data and Egg Heist read the same at 720p, 1080p, 1440p and 4K (checked against real screenshots at each size).",
     "**Sign in with Discord, right in the app** (DPS Meter › Leaderboards & account): Discord's page opens in your normal browser, you approve, and aLogs is signed in. No more copying a desktop token from the website. The AniiLogs page inside the app is signed in with the same account automatically, and Sign out works from the app too.",
     "**AniiLogs inside the app:** a new AniiLogs page shows the website itself (Leaderboards, Map, Aniilog, Item-log, Team Builder and Profiles as tabs), always identical to aniilogs.github.io. Sign in there once and your marks sync with the site. Links that leave AniiLogs open in your browser.",
     "**Checklists per character:** map, Aniilog and Item-log progress is kept for each character (UID), so characters on different servers have their own. In the app it follows the character you're logged into; on the website you pick it in Settings › Linked characters. Each character starts from what you had tracked before.",
     "**Far less disk space:** the meter reads each screen's numbers into your records and only keeps a picture when it's needed (leaderboard proof, until it has been sent). Kept pictures are small JPEGs (~0.25 MB instead of ~5 MB), leftovers are cleared every hour, and the diagnostic captures are off by default. DPS Meter › Captures & logs › Keep screenshots can keep them for 6 hours, 1 to 7 days, or for good.",
     "**Only your best runs go to the leaderboards,** once each: a run is sent when it beats one of your bests on that boss that week: score (most stars, then highest score), fastest clear, or your own Average DPS, BREAK, damage taken or healing from Battle Data. Every other run stays in your own records. The boards can rank by each of these.",
     "**Screenshots as proof:** with \"Attach the result screenshot\" on, a best run goes with its result screen and its Battle Data, each a small 1280×720 picture. Anyone can hover the picture icon next to your entry to see them (Summary and Battle Data tabs). Your PC keeps no copy once they're sent.",
     "**An on/off switch for the meter** (DPS Meter › Meter & overlay › Run the DPS meter), for when you only want Sanctum Helper or the AniiLogs pages.",
     "**A much smaller app:** Sanctum Helper now uses Microsoft's .NET Desktop Runtime (9 or newer) instead of carrying its own ~150 MB copy. The installer offers to install the runtime if it's missing, and so does the Sanctum Helper page.",
     "**Leaderboards by server:** records now say which game server they were played on (the app reads it from the game's settings, e.g. Aniimo-Americas), and the AniiLogs boards have a Server picker."
    ]
   },
   {
    "heading": "Improved",
    "items": [
     "**Egg Heist: block from the fade to black** (Results, off by default, experimental): when a match ends, button presses (and your controller, if it's blocked too) are held back from the fade to black until the end screen is read, so a stray press can't skip it. Needs Sanctum Helper running.",
     "The AniiLogs page's **Profiles** tab opens your own profile directly, with a player search at the top.",
     "**The meter notices the boss bar almost instantly.** It checks the spot where bars appeared before ten times a second, which takes under a millisecond, so a fight starts on the bar's first frame. The full search (now about 40 ms at any resolution, down from 155 ms at 1080p) only runs when a bar shows up somewhere new.",
     "**Interlink fights start the instant the bar appears:** the prep screen already said which boss it is, so the meter no longer waits to read the name above the bar (it still reads it, to double-check).",
     "**Controller blocking is built in.** The installer no longer installs HidHide. Results › Controller › Set up installs just its small driver (Windows asks for admin approval once), with no extra program, shortcut or Programs entry. Setup now also blocks the XInput side of Xbox-style controllers, which most games read. Uninstalling aLogs removes the driver again. The installer is about 8 MB smaller.",
     "**See that controller blocking works:** Results › Controller lists each connected controller by name (XInput or DirectInput). Press a button and its row lights up, so you know which controller is which. Each one has its own Block switch, and **Test blocking** hides them for a second to prove the game would lose them, then shows them again. Works with virtual controllers from reWASD too.",
     "Set up now repairs a HidHide install that never attached to your controllers (it looked installed but blocked nothing)."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "The meter no longer mistakes a File Explorer folder (or anything else) called \"Aniimo\" for the game: the dashboard said \"Game window found\" with the game closed. It now only takes the game's own window.",
     "The AniiLogs page loads the website, and opening it no longer leaves an invisible layer that stopped tabs and buttons on the other pages from being clicked.",
     "**The dashboard no longer gets stuck on \"Starting…\"** and your character no longer looks reset after an update: the hourly screenshot cleanup could delete the meter's (empty) send-queue folder, which stopped its status updates. Right after the game starts, the dashboard now says it's waiting for the game log: your UID is read fresh each session (so it's always the character you're actually on), and your saved name follows the moment it appears.",
     "Fixed an occasional \"int too long to convert\" error when capturing the game window.",
     "**No more endless \".NET\" prompts.** Updating could leave pieces of an older Sanctum Helper behind, which made Windows ask you to install .NET every time it started. The installer now replaces the bundled programs cleanly, closes Sanctum Helper before updating, and counts any .NET Desktop Runtime 9 or newer (including .NET 10) as already installed.",
     "**No more wrong or misread boss names on the boards.** In Interlink the meter only matches this week's three bosses and always uses their real names, so \"Tucking\" or \"Omega Tuckin8\" become Omega Tucking. A run only goes to the leaderboards when the game's own log confirms you were in an Interlink scene. Fights in the open world are never counted as Interlink, and the meter remembers the scene for all three bosses of a visit, even if it started partway through.",
     "The note over the result screen no longer lingers in the old spot for a second when Battle Data opens or the screen closes: it disappears as soon as the screen changes and comes back in the right place."
    ]
   }
  ]
 },
 {
  "version": "0.1.26",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**Proof of your character:** when the app reads your name in the game menu (Esc or Start), it sends that part of the menu (your picture, name and UID) to AniiLogs as proof. Proven characters can't be claimed by anyone else.",
     "**Disputes:** if another account proved your UID first, DPS Meter › Leaderboards & account offers to send a dispute with your proof, for a developer to check.",
     "**Your whole team on the boards:** runs now say which party members are real players and which are AI, so every real teammate gets their own leaderboard row. When two app users send the same run, each one's UID is recorded on the other's run."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "The party on the prep screen is read wherever it sits (it moves with the boss's challenge text), all four slots are recognised (3P and 4P were missed), and your own slot is found by your green name, leader or not.",
     "Teammates' names are read the same way on every screen, so one player never shows under two spellings.",
     "In Interlink, a boss name read without \"Omega\" could be taken for the open-world Alpha of the same name, with wrong HP. Only this week's Interlink bosses are considered now, and a far-off HP reading is never saved."
    ]
   }
  ]
 },
 {
  "version": "0.1.25",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**Your name from the game menu:** opening the menu (Esc or Start) shows your name with your UID under it. The app reads it there, checks the UID is yours, and from then on uses that name, correcting any earlier guess."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "In someone else's party (when you aren't 1P), the app could take another player's name as yours and link it to your account. It now only saves your name when it's sure: your saved name is on the result screen, or everyone else in the party was AI."
    ]
   }
  ]
 },
 {
  "version": "0.1.24",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**Themes:** General Settings has a new Themes tab with the AniiLogs website's themes (Night, Meadow, Emberpup, Pawney) and Custom colours. It changes the app window only; the overlay in the game stays the same.",
     "**Update notice:** when a new version is out, About gets a dot in the sidebar and an Update button next to the patch notes that opens the download page.",
     "**Patch notes stay current:** About shows the last five releases straight from GitHub, so you can read what's new before you update. Each one opens when you click it. If your version is older than all of them, a sixth entry says how far behind you are."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "Your character name could show as \"Name appears after your first result screen\" after an update or restart until you finished another fight. The saved name now comes back as soon as the game is running."
    ]
   }
  ]
 }
];
