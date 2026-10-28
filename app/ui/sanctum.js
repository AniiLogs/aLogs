// Sanctum Helper page: its options as tabs across the top. The options are Sanctum Helper's own settings.json
// (edited through sanctum_save; it reloads them live); the choices come from its hosted\info.json and what it
// sees from hosted\status.json (see app/src-tauri/src/sanctum.rs).
(() => {
  const invoke = window.__TAURI__.core.invoke;
  const root = document.querySelector("[data-sanctum]");
  if (!root) return;

  // Sanctum Helper's defaults (Settings.cs), used until it has written its settings file once.
  const DEFAULTS = {
    HoldKey: 0xa2, ToggleMode: false, MapOpacity: 0.55, MarkerScale: 1, ShowMapImage: true, ShowLabels: true,
    ShowViewCone: true, CaptionPosition: "Below", CaptionLayout: true, CaptionDifficulty: true, UseGameLog: true,
    ShowNotifications: true, HoldButton: 0, GameLanguage: "Auto", Language: "Auto", ShowPlayer: true,
    Layers: { doors: true, nest: true, key: true, challenge: true, legendary: true, luxurious: true, exquisite: false, simple: false, pot: false, pile: false },
    RecordableOverlay: true, MiniMapMode: "Corner", MiniMapCorner: "TopRight", MiniMapSize: 300, MiniMapZoom: 0.33,
    MiniMapOpacity: 0.85, OverlayZoom: 0.55, BigMapZoom: 0.8, BigMapSize: 0.8, OverlayOpacity: 0.65, IslandMap: true,
    Difficulty: "Auto", ForcedLayout: 0, TrackMinimap: true, WindowTitle: "Aniimo", SaveDebugFrames: false,
  };
  // Marker layers as Sanctum Helper's settings window groups them: (group, key, label, icon).
  const LAYERS = [
    ["Objectives", "doors", "Orange & blue doors", "door_orange"],
    ["Objectives", "nest", "Egg nests", "nest"],
    ["Objectives", "key", "Key rooms & their doors", "key"],
    ["Objectives", "challenge", "Timed challenge room", "challenge"],
    ["Treasure", "legendary", "Legendary sarcophagi", "chest_gold"],
    ["Treasure", "luxurious", "Luxurious chests", "chest_purple"],
    ["Treasure", "exquisite", "Exquisite chests", "chest_blue"],
    ["Treasure", "simple", "Simple chests", "chest_white"],
    ["Everything else", "pot", "Pots & vats", "chest"],
    ["Everything else", "pile", "Piles & fossils", "chest"],
  ];
  const TABS = [
    ["status", "Status", "What Sanctum Helper sees right now."],
    ["overlay", "Map overlay", "The layout lined up on the in-game map while you hold the overlay key."],
    ["mini", "On-screen map", "A live map that follows you while the in-game map is closed."],
    ["markers", "Markers", "Choose what's drawn on every map. Icons are the game's own."],
    ["detection", "Detection", "How the difficulty, the layout and your position are found."],
    ["general", "General", "Language, recording and compatibility."],
    ["layouts", "Layout browser", "Every Lost Sanctum layout with its doors, key rooms and nests."],
  ];

  let state = { settings: {}, info: {}, status: null };
  let s = structuredClone(DEFAULTS);
  let activeTab = "status";
  let rendered = "";

  const el = (tag, props = {}, ...children) => {
    const node = document.createElement(tag);
    for (const [key, value] of Object.entries(props)) {
      if (key === "class") node.className = value;
      else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
      else if (key in node) node[key] = value;
      else node.setAttribute(key, value);
    }
    node.append(...children.flat().filter((c) => c !== null && c !== undefined && c !== false));
    return node;
  };
  function toast(text, bad = false) {
    const node = document.querySelector("[data-toast]");
    node.textContent = text;
    node.classList.toggle("bad", bad);
    node.hidden = false;
    clearTimeout(toast.timer);
    toast.timer = setTimeout(() => { node.hidden = true; }, 3200);
  }

  // ---------------------------------------------------------------- saving
  let pending = {};
  let saveTimer = 0;
  function change(key, value) {
    s[key] = value;
    pending[key] = value;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      const changes = pending;
      pending = {};
      try {
        await invoke("sanctum_save", { changes });
      } catch (error) {
        toast(String(error), true);
      }
    }, 250);
  }
  async function command(name, args) {
    try {
      await invoke("sanctum_command", { command: name, args: args || null });
    } catch (error) {
      toast(String(error), true);
    }
  }

  // ---------------------------------------------------------------- controls
  function toggle(key, title, hint, onChange) {
    const input = el("input", { type: "checkbox", checked: Boolean(s[key]) });
    input.addEventListener("change", () => (onChange ? onChange(input.checked) : change(key, input.checked)));
    return el("label", { class: "toggle" }, input, el("span"), el("div", {}, el("strong", { textContent: title }), hint && el("small", { textContent: hint })));
  }
  function slider(key, title, hint, min, max, step, format = (v) => `${Math.round(v * 100)}%`, disabled = false) {
    const value = el("span", { class: "slider-value", textContent: format(s[key]) });
    const input = el("input", { type: "range", min, max, step, value: s[key], disabled });
    input.addEventListener("input", () => { value.textContent = format(Number(input.value)); });
    input.addEventListener("change", () => change(key, Math.round(Number(input.value) * 100) / 100));
    return el("div", { class: "setting" }, el("div", {}, el("strong", { textContent: title }), hint && el("small", { textContent: hint })), el("div", { class: "slider" }, input, value));
  }
  function select(key, title, hint, options, onChange) {
    const input = el("select", {}, options.map(([value, label]) => el("option", { value: String(value), textContent: label, selected: String(s[key]) === String(value) })));
    input.addEventListener("change", () => {
      const value = typeof s[key] === "number" ? Number(input.value) : input.value;
      if (onChange) onChange(value);
      else change(key, value);
    });
    return el("div", { class: "setting" }, el("div", {}, el("strong", { textContent: title }), hint && el("small", { textContent: hint })), input);
  }
  const group = (title, ...children) => el("div", { class: "group" }, title && el("h2", { textContent: title }), children);
  const card = (label, value, sub, good) => el("article", { class: "card" }, el("h2", { textContent: label }), el("p", { class: good ? "good" : "", textContent: value }), el("small", { textContent: sub || "" }));

  // ---------------------------------------------------------------- tabs
  const info = () => state.info || {};
  const status = () => state.status || {};

  function statusTab() {
    const st = status();
    const found = Boolean(st.gameFound);
    const inRun = Boolean(st.inRun);
    const layout = st.layout;
    const nodes = [];
    if (state.standaloneRunning) {
      nodes.push(el("p", { class: "notice" }, "Another copy of Sanctum Helper is running on its own (its tray icon). Quit it from its tray menu so aLogs can run it here; aLogs tries again every few seconds."));
    } else if (!state.installed) {
      nodes.push(el("p", { class: "notice" }, "Sanctum Helper is not included in this build of aLogs."));
    } else if (state.runtimeMissing) {
      // it runs on Microsoft's .NET Desktop Runtime instead of carrying its own copy (keeps aLogs ~150 MB smaller)
      const install = el("button", { type: "button", class: "primary", textContent: "Install the .NET Desktop Runtime",
        onclick: async () => {
          install.disabled = true; install.textContent = "Downloading and installing…";
          try { await invoke("install_dotnet_desktop"); install.textContent = "Installed: starting Sanctum Helper…"; }
          catch (error) { install.disabled = false; install.textContent = "Install the .NET Desktop Runtime"; toast(String(error), true); }
        } });
      nodes.push(el("div", { class: "notice" }, el("p", { textContent: "Sanctum Helper needs Microsoft's .NET Desktop Runtime 9 or newer (a free ~55 MB download from Microsoft). Windows asks for admin approval to install it." }), install));
    }
    nodes.push(group(null, toggle("__enabled", "Run Sanctum Helper", "The Egg Heist companion: the whole Lost Sanctum layout on your map, nests, key rooms and where you are.", async (on) => {
      try {
        const settings = await invoke("get_settings");
        settings.sanctum = { ...(settings.sanctum || {}), enabled: on };
        await invoke("save_settings", { settings });
        state.enabled = on;
        toast(on ? "Sanctum Helper starting" : "Sanctum Helper stopped");
      } catch (error) {
        toast(String(error), true);
      }
    })));
    nodes.push(el("div", { class: "cards" },
      card("Game", !state.running ? (state.enabled ? "Starting…" : "Off") : found ? "Found" : "Not running", found ? "Watching the game window" : "Start Aniimo to begin", found),
      card("Egg Heist run", inRun ? "In a run" : "Not in a run", inRun ? "Run timer on screen" : "Overlays stay hidden", inRun),
      card("Difficulty", st.difficulty || "—", st.difficultySource ? `From ${st.difficultySource}` : ""),
      card("Layout", layout ? layout.title : "Not found yet", layout ? `Blue door ${layout.compass} · via ${layout.source}` : inRun ? "Explore, or open the full map" : "Found during a run", Boolean(layout)),
    ));
    nodes.push(el("div", { class: "row" },
      el("button", { class: "primary", textContent: "New run", title: "Forget the detected layout and start fresh", onclick: () => command("new-run").then(() => toast("New run")) }),
      el("button", { textContent: "Position on-screen map…", onclick: () => command("position-map").then(() => toast("Drag the map on the game screen")) }),
      el("button", { textContent: "Open log folder", onclick: () => command("open-logs") }),
    ));
    const capture = st.sharedCapture ? "Reading the game through aLogs' shared capture." : st.windowCapture === false ? "Reading the screen (window capture doesn't work for this game window)." : "Reading the game window.";
    nodes.push(el("p", { class: "hint" }, state.running ? capture : ""));
    if (st.lastMatch) nodes.push(el("p", { class: "hint faint" }, `Last map match: ${st.lastMatch}`));
    nodes.push(group("How it works",
      el("p", { class: "hint" }, "1. Pick a difficulty: the party screen is read automatically before the run starts."),
      el("p", { class: "hint" }, "2. Explore the Sanctum: the layout is recognised from your minimap. Opening the full map identifies it instantly."),
      el("p", { class: "hint" }, `3. Hold ${st.keyName || "the overlay key"}: with the map open, the whole layout lined up on it; map closed, a big map that follows you.`),
    ));
    return nodes;
  }

  function keyButton(kind) {
    const st = status();
    const capturing = st.capturing === kind;
    const label = capturing ? (kind === "key" ? "Press a key…" : "Press a button…") : (kind === "key" ? st.keyName : st.padName) || "—";
    return el("button", {
      class: capturing ? "capturing" : "",
      textContent: label,
      disabled: !state.running,
      onclick: () => command(kind === "key" ? "capture-key" : "capture-pad"),
      oncontextmenu: kind === "pad" ? (event) => { event.preventDefault(); command("clear-pad"); } : undefined,
    });
  }

  function overlayTab() {
    return [
      group("Hotkey",
        el("div", { class: "setting" }, el("div", {}, el("strong", { textContent: "Overlay key" }), el("small", { textContent: "Click, then press a keyboard key or a middle / side mouse button. Esc cancels. The key still reaches the game." })), keyButton("key")),
        el("div", { class: "setting" }, el("div", {}, el("strong", { textContent: "Controller button" }), el("small", { textContent: "A second overlay key on an Xbox-style controller. Click, then press the button (Esc cancels); right-click to clear." })), keyButton("pad")),
        toggle("ToggleMode", "Press to toggle", "Press once to show the overlay and again to hide it, instead of holding the key."),
      ),
      group("On the full map",
        toggle("ShowMapImage", "Draw the whole layout", "Shows rooms that are still under fog, lined up on the in-game map."),
        slider("MapOpacity", "Layout opacity", "How strongly the layout covers the game's map.", 0.1, 1, 0.05),
        slider("MarkerScale", "Marker size", "Size of door, nest, key and chest icons on every map.", 0.6, 2, 0.05),
        toggle("ShowLabels", "Labels", "Names under markers, such as Blue door, Nest or Gold key door."),
        toggle("ShowPlayer", "Show my position", "Your arrow, tracked from the game's minimap, pointing the way you face."),
        toggle("ShowViewCone", "Camera view cone", "The white wedge from your arrow showing where the camera looks, read from the game's minimap."),
      ),
    ];
  }

  function miniTab() {
    const mode = s.MiniMapMode;
    const overlay = mode === "Overlay";
    const off = mode === "Off";
    const styles = info().styles || [];
    const zoomKey = overlay ? "OverlayZoom" : "MiniMapZoom";
    const opacityKey = overlay ? "OverlayOpacity" : "MiniMapOpacity";
    return [
      group("Style",
        el("div", { class: "style-cards" }, styles.map((style) => el("button", {
          class: `style-card${style.value === mode ? " is-active" : ""}`,
          onclick: () => { change("MiniMapMode", style.value); draw(true); },
        }, el("strong", { textContent: style.name }), el("small", { textContent: style.description })))),
        el("p", { class: "hint" }, "Out on the island the corner map is always used; your chosen style comes back inside the Sanctum."),
      ),
      group("Placement",
        el("div", { class: "setting" },
          el("div", {}, el("strong", { textContent: "Position" }), el("small", { textContent: "Pick a preset, or drag the map exactly where you want it on screen." })),
          el("div", { class: "row" },
            select("MiniMapCorner", "", "", [["TopRight", "Top right"], ["TopCenter", "Top centre"], ["OverGame", "Over the game's minimap (experimental)"]]).lastChild,
            el("button", { textContent: "Drag to position…", disabled: overlay || off || !state.running, onclick: () => command("position-map") }),
          ),
        ),
      ),
      group("Caption",
        select("CaptionPosition", "Caption", "A line above or below the map with the Sanctum number and difficulty (all styles but see-through).", [["Below", "Below the map"], ["Above", "Above the map"], ["Off", "Off"]]),
        toggle("CaptionLayout", "Sanctum number", "Which Sanctum layout this run is, e.g. Sanctum 53."),
        toggle("CaptionDifficulty", "Difficulty", "Normal, Hard, Nightmare or Chaos, in its own colour."),
      ),
      group("Look",
        slider("MiniMapSize", "Size", "Width of the map panel (at 1080p; scales with your resolution).", 180, 520, 10, (v) => `${Math.round(v)} px`, overlay || off),
        slider(zoomKey, "Zoom", "How much of the Sanctum fits in view. Kept separately for each style.", 0.25, 1.5, 0.05, undefined, off),
        slider(opacityKey, "Opacity", "Lower it to see more of the game through the map.", 0.15, 1, 0.05, undefined, off),
      ),
      group("Zoom map",
        el("p", { class: "hint" }, "Hold the overlay key with the in-game map closed (inside the Sanctum) for a big map that follows you."),
        slider("BigMapSize", "Zoom map size", "Height of the zoom map, as a share of the screen.", 0.4, 0.95, 0.05),
        slider("BigMapZoom", "Zoom map zoom", "Higher shows more detail around you.", 0.4, 1.6, 0.05),
      ),
    ];
  }

  const icons = {};
  function layerIcon(name) {
    const img = el("img", { class: "layer-icon", alt: "" });
    if (icons[name]) img.src = icons[name];
    else invoke("sanctum_icon", { name }).then((url) => { if (url) { icons[name] = url; img.src = url; } }).catch(() => {});
    return img;
  }
  function markersTab() {
    const groups = [...new Set(LAYERS.map(([g]) => g))];
    return [
      ...groups.map((name) => group(name, el("div", { class: "layer-grid" }, LAYERS.filter(([g]) => g === name).map(([, key, label, icon]) => {
        const input = el("input", { type: "checkbox", checked: (s.Layers || {})[key] ?? DEFAULTS.Layers[key] });
        input.addEventListener("change", () => {
          s.Layers = { ...(s.Layers || {}), [key]: input.checked };
          change("Layers", { [key]: input.checked });
        });
        return el("label", { class: "layer" }, input, layerIcon(icon), el("span", { textContent: label }));
      })))),
      el("p", { class: "hint" }, "Applies to the full-map overlay, the on-screen map and the zoom map."),
    ];
  }

  function detectionTab() {
    const difficulties = (info().difficulties || []).map((d) => [d.value, d.name]);
    const layouts = (info().layouts || []).map((l) => [l.number, `${l.title} (${l.difficulties.join("/")}, blue door ${l.compass})`]);
    return [
      group("Run",
        select("Difficulty", "Difficulty", "Auto reads the party goal from the party screen before the run.", [["Auto", "Auto"], ...difficulties]),
        select("ForcedLayout", "Layout", "Auto recognises it from your minimap as you explore, or straight away from the full map's door icons.", [[0, "Auto (detect)"], ...layouts]),
      ),
      group("Tracking",
        toggle("TrackMinimap", "Track my position", "Follows you from the game's minimap. Needed for the on-screen and zoom maps."),
        toggle("IslandMap", "Track me on the island", "Outside the Sanctum, show where you are on the island with its entrances and the Egg Boat."),
        toggle("UseGameLog", "Read the game's log", "Knows at once when you step out to the island, go back into the Sanctum or start a new run, from the text log the game writes."),
      ),
      group("Troubleshooting",
        toggle("SaveDebugFrames", "Save screenshots when matching fails", "Keeps the frames that couldn't be matched in the log folder, for bug reports."),
      ),
    ];
  }

  function generalTab() {
    const languages = (info().languages || []).map((l) => [l.code, l.name]);
    const st = status();
    return [
      group("Language",
        select("GameLanguage", "Game language", "The language you play Aniimo in, for reading the party screen. Auto reads it from the game's own settings.", [["Auto", "Auto"], ...languages]),
        st.ocrMissingLanguage && el("p", { class: "hint warn" }, `Windows has no text recognition for the game's language (${st.ocrMissingLanguage}), so the party screen is read in English. Add it under Windows Settings > Time & language > Language (optional features), or set the difficulty by hand.`),
        select("Language", "Sanctum Helper language", "The language of the overlay and notices. Game terms use the game's own wording in that language.", [["Auto", `Same as the game${info().gameLanguage ? ` (${info().gameLanguage})` : ""}`], ...languages]),
      ),
      group("App",
        toggle("ShowNotifications", "Notifications", "A short notice in the top-right corner when the difficulty or the Sanctum is found."),
      ),
      el("p", { class: "hint" }, "Sanctum Helper starts and stops with aLogs (General Settings › Start aLogs with Windows). The game window's title and whether overlays show in recordings are set once for all of aLogs, in General Settings."),
    ];
  }
  let browserLayout = 0;
  function layoutsTab() {
    const layouts = info().layouts || [];
    if (!layouts.length) return [el("p", { class: "hint" }, state.running ? "Loading the layouts…" : "The layout browser needs Sanctum Helper running.")];
    if (!layouts.some((l) => l.number === browserLayout)) browserLayout = layouts[0].number;
    const picked = layouts.find((l) => l.number === browserLayout);
    const image = el("img", { class: "layout-image", alt: picked.title });
    const select = el("select", {}, layouts.map((l) => el("option", { value: l.number, textContent: `${l.title} · ${l.difficulties.join(" / ")} · blue door ${l.compass}`, selected: l.number === browserLayout })));
    select.addEventListener("change", () => { browserLayout = Number(select.value); draw(true); });
    const keys = picked.keyRooms === 1 ? "1 key room" : `${picked.keyRooms} key rooms`;
    const nests = picked.nests === 1 ? "1 possible egg nest spot" : `${picked.nests} possible egg nest spots`;
    loadLayoutImage(picked.number, image);
    return [
      el("div", { class: "field" }, select),
      el("div", { class: "layout-frame" }, image),
      el("p", { class: "hint" }, `${picked.title}: blue door ${picked.compass} of the orange door. ${keys}, ${nests}. Small gold chests outside key rooms are spots where a legendary sarcophagus can appear.`),
    ];
  }
  const layoutImages = {};
  async function loadLayoutImage(number, img) {
    if (layoutImages[number]) { img.src = layoutImages[number]; return; }
    if (!state.running) return;
    await command("render-layout", { layout: number, width: 900, height: 600 });
    for (let i = 0; i < 20 && browserLayout === number; i++) {
      await new Promise((resolve) => setTimeout(resolve, 250));
      const url = await invoke("sanctum_layout_image", { layout: number }).catch(() => null);
      if (url) { layoutImages[number] = url; img.src = url; return; }
    }
  }

  const BUILDERS = { status: statusTab, overlay: overlayTab, mini: miniTab, markers: markersTab, detection: detectionTab, general: generalTab, layouts: layoutsTab };

  // ---------------------------------------------------------------- frame
  const tabRow = el("div", { class: "tabs", role: "tablist" });
  const subtitle = el("p", { class: "lead" });
  const body = el("div", { class: "tab-body" });
  root.append(tabRow, subtitle, body);
  for (const [id, label] of TABS) {
    tabRow.append(el("button", { role: "tab", "data-tab": id, textContent: label, onclick: () => { activeTab = id; draw(true); } }));
  }

  /** Redraws the active tab. Live-only tabs (status, hotkeys) redraw on every poll; the rest only when asked or
   *  when what they show changes, so a slider being dragged isn't rebuilt under the mouse. */
  function draw(force = false) {
    const key = JSON.stringify([activeTab, state.running, state.standaloneRunning, state.enabled, Boolean(state.info?.layouts),
      activeTab === "status" || activeTab === "overlay" ? state.status : null,
      activeTab === "general" ? state.status?.ocrMissingLanguage : null]);
    if (!force && key === rendered) return;
    if (!force && document.activeElement && body.contains(document.activeElement) && activeTab !== "status") return;
    rendered = key;
    for (const button of tabRow.children) button.setAttribute("aria-selected", String(button.dataset.tab === activeTab));
    subtitle.textContent = TABS.find(([id]) => id === activeTab)[2];
    body.replaceChildren(...BUILDERS[activeTab]());
    const enabled = body.querySelector("input[type=checkbox]");
    if (activeTab === "status" && enabled) enabled.checked = state.enabled;
  }

  async function refresh() {
    if (document.hidden || !root.closest(".page").classList.contains("is-active")) return;
    try {
      const next = await invoke("sanctum_state");
      const settings = await invoke("get_settings").catch(() => ({}));
      state = { ...next, enabled: settings?.sanctum?.enabled ?? true };
      if (!Object.keys(pending).length && next.settings && typeof next.settings === "object") {
        s = { ...structuredClone(DEFAULTS), ...next.settings, Layers: { ...DEFAULTS.Layers, ...(next.settings.Layers || {}) } };
      }
    } catch {
      // keep the last state
    }
    draw();
  }
  document.querySelector('[data-page="sanctum"]')?.addEventListener("click", () => { rendered = ""; refresh(); });
  setInterval(refresh, 1000);
})();
