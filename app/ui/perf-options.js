// General Settings › Performance: the official options, by add-on, under one main switch (perf.on, off by default). Off,
// aLogs runs on every option's default whatever is chosen here; on, the choices apply. A Dev tab switch the owner has
// tested moves here (14 Nov: "official options that i tested"); one that becomes plain better becomes the default.
//
// Each option: id (saved as perf.<id>), the add-on, its title and hint, the choices ([value, label]), the default, and
// `key`: the setting the meter / app already read. The page writes that key on every save (perf.js alogsPerf.apply, from app.js saveChanges): the
// choice while perf.on, the default while it's off, so neither side needs to know about the main switch.
// A new option also goes in bossbar/settings.py DEFAULTS['perf'] (tests/perf_options.py checks they agree).
window.ALOGS_PERF = {
  sections: [
    ["core", "General", "aLogs itself and what every add-on shares: its windows, the screen capture, the priority."],
    ["dps", "DPS meter", "How often the meter looks at the game."],
    ["leaderboards", "Leaderboards", "Reading the result screens and sending records."],
    ["sanctum", "Sanctum Helper", "The map and the party screen reads."],
  ],
  options: [
    {
      id: "priority", plugin: "core", key: "app.priority", default: "low",
      title: "Priority when your PC is busy",
      hint: "For aLogs, the meter and Sanctum Helper, when Windows has to choose who goes first.",
      choices: [["low", "Lower (default): the game and OBS always come first"], ["normal", "Normal"],
                ["high", "Higher: the meter keeps up even when the PC is very busy"]],
    },
    {
      id: "gpu_priority", plugin: "core", key: "capture.gpu_priority", default: "normal",
      title: "Screen capture on the graphics card",
      hint: "Lowest: the copies aLogs asks of your graphics card wait behind the game's own work and OBS's. On a card that's already full, the meter can get its pictures a little later. (Was the Dev switch capture-low-priority.)",
      choices: [["normal", "Normal (default)"], ["lowest", "Lowest: the game and OBS go first"]],
    },
    {
      id: "look_interval", plugin: "dps", key: "capture.look_interval", default: 0.05, number: true,
      title: "How often the meter looks for a boss bar in Interlink and around a fight",
      hint: "Out in the world with no boss around, the meter looks 4 times a second whatever you pick here.",
      choices: [[0.05, "20 times a second (default)"], [0.1, "10 times a second"], [0.25, "4 times a second (lightest)"]],
    },
    {
      id: "fight_rate", plugin: "dps", key: "capture.fight_rate", default: 30, number: true,
      title: "How often the meter reads the boss bar during a fight",
      hint: "Damage totals are the same either way; at 60 times a second, each drop of HP and the kill are timed to 1/60 s.",
      choices: [[30, "30 times a second (default)"], [60, "60 times a second (finest timing, heavier)"]],
    },
    {
      id: "map_redraw", plugin: "sanctum", key: "sanctum.map_redraw", default: "every",
      title: "Corner map redraws while you move",
      hint: "On a 120 Hz or faster screen the Corner map is redrawn on every screen frame while you move or turn. Capped, it's at most about 80 times a second: less drawing work, a slightly less smooth glide. Nothing changes on a 60 Hz screen. (Was the Dev switch redraw-cap.)",
      choices: [["every", "Every screen frame (default)"], ["capped", "At most about 80 times a second (lighter)"]],
    },
  ],
};
