// Generated from CHANGELOG.md by patch-notes.mjs - edit the changelog, not this file.
window.PATCH_NOTES = [
 {
  "version": "0.1.28",
  "intro": "",
  "sections": [
   {
    "heading": "New",
    "items": [
     "**Holo-Battle Sim on the leaderboards:** after a win, aLogs reads the line under \"Victory\" (the battle time, the stage such as \"Training of Water\" or \"Trial of Fire\", and the difficulty) and sends your time to that stage's board, with the Victory screen as proof.",
     "**An on/off switch for screen capture, in the game's own icon row:** the aLogs icon sits just left of the game's Chat icon at the bottom left (it follows Chat when the row moves), drawn like the game's icons, with its hotkey under it: Insert by default, a key the game doesn't use; change it, make it a combination like Ctrl+Ins, or turn it off in General Settings › Screen capture. A controller works too: pick any buttons held together, the Xbox button included, and the button shows them while you play with a controller. Bright while aLogs is watching, dim while it's off; one click or the hotkey turns screen capture off (no DPS meter, no Sanctum Helper, nothing read or sent; only the game's icon row is checked, so the button can hide with the icons) or back on. It shows only when the game shows its icons, and clicking it never takes you out of the game. Hold Alt and drag it anywhere else if you like; lock it, use a picture of your own or put it back in the row in General Settings › Screen capture.",
     "**60 readings a second in fights** (DPS Meter › Meter): each drop of HP and the kill are timed to 1/60 s (was 30 a second). 30 is still there for a lighter meter; damage totals are the same either way.",
     "**Sanctum Helper's map over the game's minimap** (Sanctum Helper › On-screen map › Position, experimental): the corner or round map sits exactly on the game's minimap, solid, and the game's minimap is still read underneath. On Windows 10 the map is kept out of recordings while it's there.",
     "**Help test aLogs** (DPS Meter › Captures & logs › Save test pictures): full-size pictures of boss fights and of every screen the meter reads, at your own resolution, so the developers can check aLogs reads everything right on every screen size. Up to 300 a day, for 3 hours after you turn it on; they stay on your PC for 7 days and go nowhere unless you share them.",
     "**Priority** (General Settings › Performance): how much of a busy PC aLogs, the meter and Sanctum Helper may take. Not set (the default: Windows decides), Lower (the game and OBS always come first), Normal, or Higher for keeping the meter on pace no matter what.",
     "**A name:** the app is now **aLogs: Aniiloger's Companion App** (aLogs for short), in its window, the sidebar, About, the installer and the tray. Your settings, records and install stay where they were.",
     "**A new icon:** Eklue's emblem inside an HP dial and a break bar, ringed by six of the game's biggest buffs, on the desktop, the taskbar, the tray, the installer and the sidebar.",
     "**aLogs updates itself:** it checks for new versions on its own and shows them in the sidebar, with a progress bar while one downloads. **Install now** closes aLogs, installs with just a progress bar (no installer to click through) and opens the new version; **Next time aLogs opens** installs it then. Updates download in the background unless you turn that off (About › Updates), nothing installs without your choice, and only one downloaded update is ever kept. Betas update the same way: About › Updates › Get beta updates, and anyone already on a beta keeps getting betas until the release catches up. About shows exactly which build you have (for example \"0.1.28 · RC10\"). (This version still installs the usual way.)",
     "**Setup guide:** a few short steps when aLogs starts: sign in with Discord, choose what goes to the leaderboards, and how result screens are handled. It turns itself off once you finish or skip it, and General Settings can show it again.",
     "**Sign in with Discord** in the sidebar, under the meter's state: Discord's sign-in page opens in a window of aLogs and closes by itself once you've approved (close it to cancel); aLogs only gets a one-time code from Discord, never what you type there. Once signed in it shows who you are (click it for the account settings).",
     "**DPS Meter › Overlay,** a tab of its own, with a picture of each spot the DPS window can sit in, per mode: out in the world right of the minimap (new, clear of the quest list and the rewards after a catch) or under the quest list; in Interlink the top-left corner next to Esc (new) or under the stars and timer. Every spot scales to your screen (16:9, 16:10 and ultrawide). If you had moved the window, it starts from its spot again.",
     "**A dark background for the DPS window** (DPS Meter › Overlay): a slider from none to solid, with a live preview, styled like Interlink's stars and timer panel, to keep the numbers readable over bright effects.",
     "**The fight's result stays on screen:** when a boss goes down, the DPS window shows the final numbers (the last hit counted, \"Defeated\", the fight's time) instead of disappearing. Out in the world you choose how long (10 seconds by default) in DPS Meter › Overlay.",
     "**Being attacked while you're in another window:** like the game's own \"Under attack\" warning: while you're in another window, aLogs watches your HP bar, and when it drops, \"You're being attacked in Aniimo\" pops up over your other windows (for 2 seconds every 5, until the hits stop), with the red flash around the screen's edges the game shows when you're hit. Only real hits count, in a boss fight or anywhere else. Click the text to go back to the game. A fight that ran mostly while you were away shows no result.",
     "**Boss kill sound** (DPS Meter › Meter › Audio cues, on by default): out in the world, the sound the game plays with the killing blow dates the end of the fight to the hit itself. aLogs listens to Aniimo's audio only (nothing else on your PC), only during a boss fight, and records nothing. Without it (or with the game muted) the fight ends 3 seconds before the Catch Countdown.",
     "**Egg Heist after each match:** the rank screen that follows a match updates your season standing on the leaderboard, and your profile gets the Rank Points the match added and your progress bar (\"1337/1350 +18\"); at Egg Emperor, your egg count. Rank Info also sends where your next rank starts, for your profile.",
     "**Photos:** put the pictures you take in Aniimo on your AniiLogs profile. The new Photos page shows the ones the game saved on this PC (Settings › Storage set to Local, or Local and cloud); pick the ones to upload, or turn on **Upload new photos automatically** (a new photo goes up a few seconds after you take it). Each keeps its date, your character, the Aniimo in it and where you took it, and shows wherever your profile shows: only to you while it's private, to everyone once it's public. Uploaded as high-quality WebP (up to 2560 px), and you can remove any of them again.",
     "**Your collection on your profile:** open your Profile in Aniimo (Esc › Profile) and your AniiLogs profile gets its Collection Progress (Aniimo Point, Sparkling Aniimo, Style Points and Prismana Aniimo) with the game's icons, and your character's gender. Only your own Profile counts (it shows your UID); a friend's is never sent. Your level (from the EXP card) and your server (Aniimo-Americas, …) show beside your character's name too.",
     "**Online on your profile:** while Aniimo and aLogs are running, your AniiLogs profile shows **Online** with a green dot, and **AFK** once Aniimo hasn't been the window in front, or nobody has touched the keyboard, mouse or controller, for 20 minutes, and **Offline** (red) when either isn't running. Don't want it shown? Turn it off on your profile (Edit profile › Show my online status) or in DPS Meter › Leaderboards & account.",
     "**Report a problem** (Dashboard): something wrong with aLogs? Send your logs to the AniiLogs developers in one click, with a note about what went wrong if you like. One report covers the whole app: the meter's log, your 20 newest fights' readings, your profile notes, Sanctum Helper's log and both modules' settings (never your sign-in), with your game UID, the time and which build you have. Add up to 5 screenshots if they help: pick them or paste one (a Win+Shift+S snip works). Reports are kept privately and cleared once the problem is fixed, after 30 days at most.",
     "**Profile notes:** the meter notes your **catches** (\"Irisal Lv 55\") and your **level and current EXP** each time the EXP card shows, for your AniiLogs profile. It keeps the card's final numbers (not ones caught mid-animation), and never notes EXP going backwards."
    ]
   },
   {
    "heading": "Improved",
    "items": [
     "**New bosses without waiting for an aLogs update:** the meter reads the boss list (names and HP) from your own game files after each game patch, like the Interlink weeks, so a boss a patch adds is recognized right away.",
     "**The meter reacts almost instantly:** it looks for a boss bar 20 times a second, anywhere on screen, and its full search is about ten times cheaper than before. During a fight it reads the bar 30 times a second (was 10). Result screens, Battle Data and the Catch Countdown are read without ever pausing the meter, and text is read about four times faster (a full screen in ~60 ms instead of ~220 ms) with exactly the same results. The meter starts in a fraction of a second, and between fights it eases off while you're in another window.",
     "**Out in the world the fight's clock starts at the first hit**, not when the boss's bar shows up as you get near it, so standing around before a fight no longer drags your DPS down.",
     "**The overlay only shows over the game:** tab out (to Discord, a browser…) and it gets out of the way at once, then comes back when you return to the game.",
     "The line under the boss bar now reads **HP** (what the boss has left) instead of \"Total\", and the numbers on and under the bar have a soft dark shadow, so they stay readable over bright effects.",
     "**DPS Meter settings in tabs:** Meter, Overlay, Results, Captures & logs, and Leaderboards & account. Each thing lives in one place: the log folders, Report a problem and your character (name, UID, proof, disputes) are on the Dashboard; you sign in from the sidebar, and Leaderboards & account shows your account and what's being sent; the game window's title and **Overlays show in recordings and screenshots** (one switch for the DPS window, the HP labels and Sanctum Helper's maps) are in General Settings for the meter and Sanctum Helper alike; updates install from the sidebar; and Sanctum Helper's version and credits are in About, which links aLogs' own code too (github.com/AniiLogs/aLogs).",
     "**Changing a setting no longer restarts the meter**, so a fight in progress isn't lost. Only the overlay's and the game window's settings still restart it.",
     "**Sanctum Helper keeps up with you:** the view cone follows the cone's edges instead of its brightest part (bright floor and walls no longer pull it sideways), the cone and your character's arrow follow fast turns without trailing, your spot on the map moves with you (dashes show in about half the time), and the open full map follows panning and zooming right away. Its map matching is 2–3 times faster, with the same results.",
     "**Sanctum Helper knows the Whisperwake update's Egg Heist layouts:** Normal no longer has layout 60, and Nightmare and Chaos now draw from 12 layouts, with maps for all of them (the rebuilt 31, 33 and 38 and the new 61 and 62).",
     "**Sanctum Helper uses far less memory:** it held about 600 MB from startup, even with no run, and grew run after run. Maps now load when you enter a Sanctum and are let go after the run, or a minute after the game closes.",
     "Leaderboard records say which aLogs version and build sent them, so a future board that needs a newer aLogs can ask for it. Older versions keep sending to the boards they already use."
    ]
   },
   {
    "heading": "Fixed",
    "items": [
     "**Boss HP and BREAK gauges after the Whisperwake update:** the game changed how boss HP grows with level (bosses using its main HP curve rise more slowly from level 56, a new curve above 65, and a new formula for the Umbral bosses) and gave Interlink bosses new BREAK gauges and more HP. The meter's numbers now match the game's own formulas at every level, and its built-in boss list (used when your game files can't be read) has the new bosses and Interlink weeks.",
     "**\"You're being attacked\" in town at full HP:** the small HP number is often misread, and in a still scene the same misread can come back again and again. A hit now has to hold for three readings in a row, and a single small drop only counts when another follows within 10 seconds; a number flipping between the same two readings is ignored.",
     "**Interlink's new star rules:** since the Whisperwake update a fight is rated out of 20 stars (the middle boss counts double, 80 a week). A run that ended early with 15 to 19 stars counted as a clear on the fastest-clear board; a clear now takes the full 20. Runs also say which boss of the week they were (left, middle or right) and which part of the season, and the two-boss middle fights from November 18 count for both bosses.",
     "**The boss bar at 720p:** it was often not found at all (its thin end lines blur at that size), and once its ×N counter was mistaken for the bar's end. Found and read right at 720p, 900p, 1080p, 1440p and 4K now.",
     "**The ×N counter at 720p and 4K:** a single digit right after the \"x\" (×4) touched it once scaled and wasn't read.",
     "**Teammates' names on result screens** lost their first letter at some sizes; read whole at every common resolution now.",
     "**Boss names on the battle prep screen** in a week the game data doesn't list yet are matched to the right boss instead of kept as read (\"Omega Tucking$\").",
     "**No more lag from aLogs while you play or stream:** aLogs now watches the game on your graphics card, the way OBS does, instead of copying the whole 4K screen up to 20 times a second (about 50 ms each, which could make the game stutter, worse with OBS running). The meter and Sanctum Helper share one capture, and each gets only the part of the screen it reads (Sanctum Helper's map corner, the boss bar area), at its own pace. Sanctum Helper no longer copies the screen 60 times a second during an Egg Heist run. On Windows 11 only the game window is captured, so the DPS window and map show on stream as usual; on Windows 10 they're kept out of the meter's view (and out of display captures) so it never reads itself. Tabbed out on Windows 10, aLogs still watches the game a few times a second; where Windows can't copy the game window, Windows shows a yellow border around the game while you're away. The logs say how the game is captured and how long it takes.",
     "**Closing aLogs is instant:** it used to hang for about 10 seconds while it waited on the meter and Sanctum Helper. They now finish up on their own (an open fight is still saved) and close within a fraction of a second.",
     "**No more filling up your drive:** the meter's text reader left a small picture behind for nearly every read, in Windows' Temp folder, and never cleaned up (on one PC: 600,000 files, 1.8 GB). It now keeps none, and the first start clears what earlier versions left. Each fight's per-frame log is also cleared after 30 days (your fight results, profile notes and leaderboard records are kept). Records the leaderboards turned down are cleared after 7 days (records still to send wait until they go up); what the meter read off result screens goes with their pictures, and Sanctum Helper's debug frames go when you turn them off.",
     "**Less memory while capturing at 4K:** aLogs keeps at most 128 MB of capture buffers (it could hold over 250 MB).",
     "**The fight's time is right through BREAKs and ultimates:** the meter keeps reading the boss's HP under the BREAK overlay (it used to freeze there, which gave impossible sub-10-second kills), and when an ultimate finishes the boss, the fight runs to the killing blow, not only to when the cutscene hid the bar.",
     "**Short names are read:** a name like \"0\" read from the game menu was taken for a misread and dropped, and Windows' text reader often couldn't read a name of one or two characters at all, so that player's character never got a name and the leaderboards (which list players by name) hid their records. Short names are now read letter by letter against the game's own font, and the **Profile** screen (Esc › Profile) confirms your name too: your name card and UID together, whatever picture your card has. Opening either one is enough.",
     "**No more misread names on your profile:** while the game menu slid open, one blurred read could become your character's name (a letter misread, or \"1\" for \"0\"), and a correct read right after it could be lost on the way to the site. Now your first name is still linked on its own, but once it's proven nothing read later replaces it. Renamed your character? **Link character** on the Dashboard reads your Profile (Esc › Profile) and asks you to confirm the name before it's sent.",
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
     "The party on the prep screen is read wherever it sits (it moves with the boss's challenge text), all four slots are recognized (3P and 4P were missed), and your own slot is found by your green name, leader or not.",
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
     "**Themes:** General Settings has a new Themes tab with the AniiLogs website's themes (Night, Meadow, Emberpup, Pawney) and Custom colors. It changes the app window only; the overlay in the game stays the same.",
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
