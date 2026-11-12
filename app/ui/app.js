(() => {
  const invoke = window.__TAURI__.core.invoke;
  // Same defaults as bossbar/settings.py (the meter fills in anything missing the same way).
  const DEFAULTS = {
    app: { welcome_done: false, show_guide: true, start_with_windows: false, close_to_tray: true, priority: "low", auto_download: true, beta_updates: false,
      photos_auto: false, photos_auto_since: 0 },
    meter: { enabled: true },
    audio_cues: { boss_kill: true },
    game: { window_title: "Aniimo", player_log: "" },
    overlay: { show: true, dps_window: true, hp_labels: true, in_recordings: true, backdrop: 0, world_position: "map", dps_locked: true, dps_example: false, dps_reset: 0,
      rush_position: "corner", result_seconds: 10 },
    capture: { game_window_only: false, look_interval: 0.05, fight_rate: 30, test_frames: false, damage_numbers: false, debug_frames: false, end_screens: false, nobar_frames: false, keep_hours: 0 },
    results: { auto_open_battle_data: true, sim_gizmos: true, sim_gate: false, hold_inputs: true, hold_controller: false, hold_seconds: 60, egg_fade_hold: false, egg_auto_next: true },
    leaderboard: { submit: false, automation: false, api_url: "https://aniilogs-api.pages.dev/api", token: "", link_character: true, include_screenshots: true, notify_sent: true },
    // every plug-in's notices over the game: where and how (each plug-in's own on/off is on its page), and the button's
    notifications: { place: "top-right", motion: "fade", locked: true, button: true },
    // the settings the player changed themselves ("capture.keep_hours", ...): a default that changes in a later version
    // only moves the ones not listed, so a choice is never overwritten
    user_set: [],
  };
  let settings = structuredClone(DEFAULTS);
  let status = {};

  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
  const get = (path) => path.split(".").reduce((value, key) => value?.[key], settings);
  // What this window changed since its last save: only those are written over the file as it is then, so settings
  // saved meanwhile by another page (Sanctum Helper's on/off) or the button over the game are never overwritten by
  // this window's older copy (they were: turning Sanctum Helper off didn't stick past the next save).
  const changed = new Set();
  // a setting the player changed themselves goes on the list (see DEFAULTS.user_set)
  const markUserSet = (path) => {
    const list = Array.isArray(settings.user_set) ? settings.user_set : (settings.user_set = []);
    if (!list.includes(path)) { list.push(path); changed.add("user_set"); }
  };
  const set = (path, value) => {
    const keys = path.split(".");
    const last = keys.pop();
    keys.reduce((value, key) => (value[key] ??= {}), settings)[last] = value;
    changed.add(path);
  };
  // Defaults filled in under what was saved; keys this window doesn't know (another page's) are kept as they are.
  function merge(base, extra) {
    const out = structuredClone(base);
    for (const [key, value] of Object.entries(extra || {})) {
      if (out[key] && typeof out[key] === "object" && value && typeof value === "object") out[key] = merge(out[key], value);
      else out[key] = value;
    }
    return out;
  }
  // Settings kept under an older name, moved to where they are now - on load and on every save (a save starts from the
  // file, so a choice only moved in memory would be lost). Where notices show was the leaderboards'
  // (leaderboard.notify_place, notify_motion, notify_locked) until they became every plug-in's (notifications.*).
  function migrateStored(stored) {
    const old = stored?.leaderboard;
    if (!old) return stored;
    for (const key of ["place", "motion", "locked"]) {
      if (old[`notify_${key}`] === undefined) continue;
      stored.notifications = stored.notifications || {};
      if (stored.notifications[key] === undefined) stored.notifications[key] = old[`notify_${key}`];
      delete old[`notify_${key}`];
    }
    return stored;
  }
  async function saveChanges() {
    const fresh = merge(DEFAULTS, migrateStored(await invoke("get_settings")));
    for (const path of changed) {
      const keys = path.split(".");
      const last = keys.pop();
      keys.reduce((value, key) => (value[key] ??= {}), fresh)[last] = get(path);
    }
    await invoke("save_settings", { settings: fresh });
    changed.clear();
    settings = fresh;
  }

  // ---------------------------------------------------------------- toast
  let toastTimer = 0;
  // A short notice in the corner. Errors stay until closed (×) so there's time to read them; others go after a few
  // seconds, longer for longer text.
  function toast(text, bad = false) {
    const node = $("[data-toast]");
    node.replaceChildren(Object.assign(document.createElement("span"), { textContent: text }));
    node.classList.toggle("bad", bad);
    clearTimeout(toastTimer);
    if (bad) {
      const close = Object.assign(document.createElement("button"), { type: "button", className: "toast-close", textContent: "×", title: "Close" });
      close.setAttribute("aria-label", "Close");
      close.addEventListener("click", () => { node.hidden = true; });
      node.append(close);
      node.setAttribute("role", "alert");
    } else {
      node.removeAttribute("role");
      toastTimer = setTimeout(() => { node.hidden = true; }, Math.max(3200, text.length * 60));
    }
    node.hidden = false;
  }
  window.alogsToast = toast;   // Sanctum Helper's page uses the same notice, so errors stay until closed there too

  // ---------------------------------------------------------------- navigation
  $$("[data-page]").forEach((button) => button.addEventListener("click", () => {
    $$("[data-page]").forEach((b) => b.classList.toggle("is-active", b === button));
    $$("[data-page-body]").forEach((page) => page.classList.toggle("is-active", page.dataset.pageBody === button.dataset.page));
    placeSite();
  }));

  // ---------------------------------------------------------------- the AniiLogs website
  // The live site in a second webview (site.rs) laid exactly over the frame under the tabs, so it is always the same as
  // aniilogs.github.io. It is hidden whenever another page is open, and follows the frame when the window is resized.
  let sitePage = "/";                 // Home first (inside aLogs the site hides its logo, its only way home)
  let siteSignedIn = false;   // the site view got its one-time sign-in from the app's account
  async function siteHandoff() {
    const token = get("leaderboard.token");
    if (!token || siteSignedIn) return null;
    try {
      const response = await fetch(`${api()}/auth/app/site-handoff`, { method: "POST", headers: { authorization: `Bearer ${token}` } });
      if (!response.ok) return null;
      siteSignedIn = true;
      return (await response.json()).handoff || null;
    } catch { return null; }
  }
  async function placeSite() {
    const page = $('[data-page-body="site"]');
    const open = page.classList.contains("is-active");
    const frame = $("[data-site-frame]").getBoundingClientRect();
    const theme = (() => { try { return JSON.parse(localStorage.getItem("alogs:theme:v1") || "{}").theme; } catch { return null; } })();
    const auth = open ? await siteHandoff() : null;
    invoke("site_view", open
      ? { path: sitePage, theme: theme && theme !== "custom" ? theme : null, uid: status.character?.uid || null, auth,
          x: frame.left, y: frame.top, w: frame.width, h: frame.height }
      : { path: null, theme: null, uid: null, auth: null, x: 0, y: 0, w: 0, h: 0 }).catch((error) => toast(String(error), true));
  }
  $$("[data-site-tab]").forEach((tab) => tab.addEventListener("click", () => {
    sitePage = tab.dataset.siteTab;
    $$("[data-site-tab]").forEach((other) => other.setAttribute("aria-selected", String(other === tab)));
    placeSite();
  }));
  new ResizeObserver(() => { if ($('[data-page-body="site"]').classList.contains("is-active")) placeSite(); }).observe($("[data-site-frame]"));
  // DPS Meter: its sections are tabs across the top of the page
  // Tabs across the top of a page (DPS Meter, Leaderboards, General Settings): each page switches only its own panels.
  function showTab(page, name) {
    const body = $(`[data-page-body="${page}"]`);
    $$("[data-tab]", body).forEach((tab) => tab.setAttribute("aria-selected", String(tab.dataset.tab === name)));
    $$("[data-panel]", body).forEach((panel) => panel.classList.toggle("is-active", panel.dataset.panel === name));
  }
  $$("[data-tab]").forEach((tab) => tab.addEventListener("click", () => showTab(tab.closest("[data-page-body]").dataset.pageBody, tab.dataset.tab)));

  // ---------------------------------------------------------------- themes
  // The AniiLogs site's themes (public/theme-shell.js): same presets, same colours, plus Custom. Only this window's
  // look changes, never the overlay. Kept in this window's own storage, not settings.json: it's this window's
  // look only, nothing the meter needs.
  const THEME_KEY = "alogs:theme:v1";
  const THEMES = {
    default: { label: "Night", colors: { background: "#0b1215", backgroundAlt: "#10242a", surface: "#121d21", surfaceRaised: "#1a2a2f",
      border: "#315057", text: "#f4f4e9", muted: "#9bb5b2", primary: "#62d9b3", secondary: "#2d9d82", highlight: "#f4bd4f" },
      accentText: "#79e5c2", coral: "#f48972" },
    meadow: { label: "Meadow", colors: { background: "#f8f2d7", backgroundAlt: "#ccebe4", surface: "#fffaf0", surfaceRaised: "#f3edd8",
      border: "#b7d2c7", text: "#24473f", muted: "#506a62", primary: "#4fcda5", secondary: "#258c76", highlight: "#f4bd4f" },
      accentText: "#1c756b", goldText: "#8a5a00", coral: "#b84c3b", coralText: "#a33d2d" },
    emberpup: { label: "Emberpup", colors: { background: "#111218", backgroundAlt: "#2b1815", surface: "#1c1d27", surfaceRaised: "#292a36",
      border: "#4b4650", text: "#fff4e8", muted: "#bdb4b5", primary: "#ff742e", secondary: "#d9471d", highlight: "#ffc857" },
      accentText: "#ffad82", coral: "#ff742e" },
    pawney: { label: "Pawney", colors: { background: "#0e1120", backgroundAlt: "#171b3a", surface: "#171b2d", surfaceRaised: "#232844",
      border: "#444d76", text: "#f5f4ff", muted: "#aeb3cf", primary: "#7a84f7", secondary: "#4d57bc", highlight: "#ee8bd4" },
      accentText: "#adb5ff", coral: "#ee8bd4" },
  };
  const COLOR_LABELS = { background: "Background", backgroundAlt: "Background glow", surface: "Panels", surfaceRaised: "Raised panels",
    border: "Borders", text: "Text", muted: "Secondary text", primary: "Accent", secondary: "Second accent", highlight: "Highlight" };
  const hex = (value, fallback) => (/^#[0-9a-f]{6}$/iu.test(String(value || "")) ? String(value).toLowerCase() : fallback);
  const luminance = (color) => {
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(color.slice(i, i + 2), 16) / 255)
      .map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const contrast = (a, b) => { const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
  const readable = (background) => (contrast("#08110f", background) >= contrast("#fffdf3", background) ? "#08110f" : "#fffdf3");
  const mixColor = (a, b, share) => `#${[1, 3, 5].map((i) => Math.round(parseInt(a.slice(i, i + 2), 16) * share
    + parseInt(b.slice(i, i + 2), 16) * (1 - share)).toString(16).padStart(2, "0")).join("")}`;
  function savedTheme() {
    try {
      const saved = JSON.parse(localStorage.getItem(THEME_KEY) || "{}");
      return { theme: saved.theme === "custom" || THEMES[saved.theme] ? saved.theme : "default", custom: saved.custom || {} };
    } catch {
      return { theme: "default", custom: {} };
    }
  }
  function customColors(custom) {
    return Object.fromEntries(Object.entries(THEMES.default.colors).map(([key, fallback]) => [key, hex(custom[key], fallback)]));
  }
  function applyTheme(theme, custom) {
    const preset = THEMES[theme];
    const c = preset ? preset.colors : customColors(custom);
    const light = luminance(c.background) > 0.4;
    // accent as text: the preset's own readable shade; for Custom, the accent itself when it reads on the panels
    const accentText = preset ? preset.accentText : (contrast(c.primary, c.surface) >= 3 ? c.primary : c.text);
    // the highlight as text (notices, paths): the light theme's gold doesn't read on its pale panels
    const goldText = preset ? preset.goldText ?? c.highlight : (contrast(c.highlight, c.surface) >= 4.5 ? c.highlight : c.text);
    // the coral as text (.note-warn: the English OCR warning's title, an out-of-date version's): 4.5:1 on its box (8%
    // coral) over the page and the panels. Meadow's coral gave 4.08:1 (3.61 on the page's glow), and Custom's fixed
    // coral 2.27:1 on a white one; Custom takes it where it reads, else Meadow's darker one, else its own text colour
    const coral = preset ? preset.coral : "#f48972";
    const coralText = preset ? preset.coralText ?? coral : [coral, THEMES.meadow.coralText].find((shade) =>
      [c.background, c.backgroundAlt, c.surface].every((base) => contrast(shade, mixColor(coral, base, 0.08)) >= 4.5)) ?? c.text;
    const vars = { "--bg": c.background, "--bg-2": c.backgroundAlt, "--panel": c.surface, "--panel-2": c.surfaceRaised, "--line": c.border,
      "--text": c.text, "--muted": c.muted, "--accent": c.primary, "--accent-2": c.secondary, "--gold": c.highlight,
      "--coral": coral, "--coral-text": coralText, "--accent-text": accentText, "--gold-text": goldText, "--on-accent": readable(c.primary) };
    Object.entries(vars).forEach(([name, value]) => document.documentElement.style.setProperty(name, value));
    document.documentElement.style.colorScheme = light ? "light" : "dark";
  }
  function saveTheme(theme, custom, share = true) {
    try { localStorage.setItem(THEME_KEY, JSON.stringify({ theme, custom })); } catch { /* the theme still applies now */ }
    applyTheme(theme, custom);
    renderThemes();
    if (share) void shareTheme(theme, custom);           // signed in: the website follows (shared with the website, below)
  }
  function renderThemes() {
    const { theme, custom } = savedTheme();
    const box = $("[data-themes]");
    const swatch = (colors) => {
      const strip = document.createElement("span");
      strip.className = "theme-swatch";
      strip.style.background = colors.background;
      for (const key of ["surface", "primary", "highlight", "text"]) {
        const dot = document.createElement("i");
        dot.style.background = colors[key];
        strip.append(dot);
      }
      return strip;
    };
    const cards = [...Object.entries(THEMES).map(([id, preset]) => [id, preset.label, preset.colors]), ["custom", "Custom", customColors(custom)]]
      .map(([id, label, colors]) => {
        const card = document.createElement("button");
        card.type = "button";
        card.className = `theme-card${id === theme ? " is-active" : ""}`;
        card.setAttribute("aria-pressed", String(id === theme));
        card.append(swatch(colors), Object.assign(document.createElement("span"), { textContent: label }));
        card.addEventListener("click", () => saveTheme(id, id === "custom" && !Object.keys(custom).length
          ? { ...THEMES[theme]?.colors ?? customColors(custom) } : custom));   // Custom starts from the theme you had
        return card;
      });
    box.replaceChildren(...cards);
    const editor = $("[data-theme-custom]");
    editor.hidden = theme !== "custom";
    if (theme !== "custom") return;
    const colors = customColors(custom);
    editor.querySelector("[data-theme-colors]").replaceChildren(...Object.entries(COLOR_LABELS).map(([key, label]) => {
      const row = document.createElement("label");
      row.className = "theme-color";
      const input = Object.assign(document.createElement("input"), { type: "color", value: colors[key] });
      input.addEventListener("input", () => applyTheme("custom", { ...colors, [key]: input.value }));
      input.addEventListener("change", () => saveTheme("custom", { ...customColors(savedTheme().custom), [key]: input.value }));
      row.append(input, Object.assign(document.createElement("span"), { textContent: label }));
      return row;
    }));
  }
  $("[data-theme-reset]").addEventListener("click", () => saveTheme("custom", { ...THEMES.default.colors }));
  applyTheme(savedTheme().theme, savedTheme().custom);
  renderThemes();

  // ---------------------------------------------------------------- screen capture switch
  // DPS Meter › Run the DPS meter: the same switch as the button over the game (overlay_button.rs); the button's own
  // changes (a click, its right-click menu) arrive as "capture-switch" events.
  let captureOn = true;      // the switch as last shown (the sidebar's "Meter off" follows it)
  function showCaptureSwitch(info) {
    if (!info) return;
    $("[data-capture-meter]").checked = info.capture_on;   // DPS Meter › Meter › Run the DPS meter
    $('[data-nav-switch="dps"]').setAttribute("aria-checked", String(Boolean(info.capture_on)));
    captureOn = info.capture_on;
    $("[data-capture-button]").checked = info.show;
    $("[data-capture-lock]").checked = info.locked;
    $("[data-capture-always]").checked = Boolean(info.always);
    if (info.hide_ms !== undefined) $("[data-capture-hide]").value = String(info.hide_ms);
    $("[data-capture-hide]").disabled = Boolean(info.always);   // always shown: nothing to hide
    $("[data-capture-default-icon]").disabled = !info.custom_icon;
    $("[data-capture-reset]").disabled = info.in_row !== false;
    if (!capturingKey) $("[data-capture-hotkey]").textContent = info.hotkey_label || "None";
    if (!pickingPad) $("[data-capture-pad]").textContent = info.pad_label || "None";
  }
  async function captureSwitch() {
    // listening first, each on its own: a failed first read no longer leaves the page deaf to the button's changes
    try { await window.__TAURI__.event.listen("capture-switch", (event) => showCaptureSwitch(event.payload)); } catch { /* none */ }
    // the button over the game switches every plug-in: all off, or back on as they were. The meter's own switch comes
    // with "capture-switch"; Sanctum Helper's and the leaderboards' follow here
    try {
      await window.__TAURI__.event.listen("plugins-switch", (event) => {
        const on = event.payload || {};
        for (const p of pluginList) {
          if (typeof on[p.id] !== "boolean" || !String(p.switch).startsWith("/")) continue;   // the meter's: capture-switch
          set(p.switch.slice(1).split("/").join("."), on[p.id]);
        }
        fill();
      });
    } catch { /* none */ }
    try {
      await window.__TAURI__.event.listen("sanctum-switch", (event) => {
        settings.sanctum = { ...(settings.sanctum || {}), enabled: Boolean(event.payload) };
        document.dispatchEvent(new CustomEvent("sanctum-enabled", { detail: Boolean(event.payload) }));
      });
    } catch { /* none */ }
    try {
      showCaptureSwitch(await invoke("capture_switch"));
    } catch {
      // an app without the switch
    }
  }
  const setSwitch = async (change) => showCaptureSwitch(await invoke("set_capture_switch", change));
  $("[data-capture-meter]").addEventListener("change", (event) => setSwitch({ captureOn: event.target.checked }));
  // the sidebar's switches: flipping one doesn't open its page
  async function setSanctum(on) {
    try {
      const stored = await invoke("get_settings");
      stored.sanctum = { ...(stored.sanctum || {}), enabled: on };
      await invoke("save_settings", { settings: stored });
      settings.sanctum = { ...(settings.sanctum || {}), enabled: on };
      showSanctumSwitch(on);
      document.dispatchEvent(new CustomEvent("sanctum-enabled", { detail: on }));   // its page's toggle follows
      toast(on ? "Sanctum Helper is starting" : "Sanctum Helper stopped");
    } catch (error) {
      toast(String(error), true);
    }
  }
  // Overlay › Placement: forget every mode's dragged spot (the meter watches overlay.dps_reset)
  $("[data-dps-reset]")?.addEventListener("click", async () => {
    try {
      const stored = await invoke("get_settings");
      stored.overlay = { ...(stored.overlay || {}), dps_reset: (Number(stored.overlay?.dps_reset) || 0) + 1 };
      await invoke("save_settings", { settings: stored });
      settings.overlay = { ...(settings.overlay || {}), dps_reset: stored.overlay.dps_reset };
      toast("The DPS window goes back to its default spots the next time it shows");
    } catch (error) {
      toast(String(error), true);
    }
  });
  const showSanctumSwitch = (on) => $('[data-nav-switch="sanctum"]').setAttribute("aria-checked", String(on !== false));
  document.addEventListener("sanctum-enabled", (event) => showSanctumSwitch(event.detail));
  for (const sw of $$("[data-nav-switch]")) {
    const flip = (event) => {
      event.preventDefault(); event.stopPropagation();
      const on = sw.getAttribute("aria-checked") !== "true";
      if (sw.dataset.navSwitch === "dps") void setSwitch({ captureOn: on });
      else if (sw.dataset.navSwitch === "leaderboards") { set("leaderboard.submit", on); markUserSet("leaderboard.submit"); fill(); scheduleSave(on ? "Sending to the leaderboards" : "Not sending to the leaderboards"); }
      else void setSanctum(on);
    };
    sw.addEventListener("click", flip);
    sw.addEventListener("keydown", (event) => { if (event.key === " " || event.key === "Enter") flip(event); });
  }
  $("[data-capture-button]").addEventListener("change", (event) => setSwitch({ show: event.target.checked }));
  $("[data-capture-lock]").addEventListener("change", (event) => setSwitch({ locked: event.target.checked }));
  $("[data-capture-always]").addEventListener("change", (event) => setSwitch({ always: event.target.checked }));
  $("[data-capture-hide]").addEventListener("change", (event) => setSwitch({ hideMs: Number(event.target.value) }));
  $("[data-capture-reset]").addEventListener("click", () => setSwitch({ resetPosition: true }));
  $("[data-capture-default-icon]").addEventListener("click", () => setSwitch({ defaultIcon: true }));
  $("[data-capture-choose-icon]").addEventListener("click", async () => showCaptureSwitch(await invoke("capture_choose_icon")));

  // The hotkey: the next key pressed, shown under the button the way the game labels its own keys. Keys the game
  // uses by default get a warning (they'd do both); the hotkey only works while the game is in front.
  const GAME_KEYS = new Set(["Tab", "Enter", "Escape", "Space", "ShiftLeft", "ShiftRight", "ControlLeft", "ControlRight",
    "AltLeft", "AltRight", "Backquote", "Minus", "Equal", "Backslash", "Comma", "Period", "Slash", "PageUp", "PageDown",
    "Delete", "End", "ArrowLeft", "ArrowRight", "NumpadMultiply"]);
  const gameUses = (code) => GAME_KEYS.has(code) || /^(Key[A-Z]|Digit\d|F([1-9]|1[0-2]))$/.test(code);
  const KEY_LABELS = { Insert: "Ins", Home: "Home", Pause: "Pause", ScrollLock: "ScrLk", BracketLeft: "[", BracketRight: "]",
    Semicolon: ";", Quote: "'", NumLock: "NumLk", NumpadDivide: "Num /", NumpadSubtract: "Num -", NumpadAdd: "Num +",
    NumpadEnter: "Num Ent", NumpadDecimal: "Num .", ArrowUp: "Up", ArrowDown: "Down" };
  const keyLabel = (event) => KEY_LABELS[event.code] || (event.code.startsWith("Numpad") ? `Num ${event.code.slice(6)}`
    : event.code.replace(/^Key|^Digit/, "")).slice(0, 8);
  var capturingKey = false;   // var: showCaptureSwitch above reads it
  $("[data-capture-hotkey]").addEventListener("click", (event) => {
    capturingKey = true;
    event.target.textContent = "Press a key…";
    $("[data-capture-hotkey-note]").textContent = "";
  });
  const stopPicking = async () => {                                              // cancelled: show the key as it is
    if (!capturingKey) return;
    capturingKey = false;
    showCaptureSwitch(await invoke("capture_switch"));
  };
  window.addEventListener("blur", () => void stopPicking());
  document.addEventListener("mousedown", (event) => {
    if (!event.target.closest("[data-capture-hotkey]")) void stopPicking();      // clicked anywhere else
  });
  window.addEventListener("keydown", async (event) => {
    if (!capturingKey) return;
    event.preventDefault();
    if (["Shift", "Control", "Alt", "Meta"].includes(event.key)) return;           // a modifier alone isn't a hotkey
    if (event.code === "Escape") { await stopPicking(); return; }
    capturingKey = false;
    // with Ctrl, Alt or Shift held: a combination (RegisterHotKey's MOD_ALT 1, MOD_CONTROL 2, MOD_SHIFT 4)
    const mods = (event.altKey ? 1 : 0) | (event.ctrlKey ? 2 : 0) | (event.shiftKey ? 4 : 0);
    const label = [event.ctrlKey && "Ctrl", event.altKey && "Alt", event.shiftKey && "Shift", keyLabel(event)].filter(Boolean).join("+");
    $("[data-capture-hotkey-note]").textContent = !mods && gameUses(event.code)
      ? `The game uses ${label} too: it would do both. A key like Ins, Home or Pause is safer.` : "";
    await setSwitch({ hotkey: { vk: event.keyCode, mods, label } });
  }, true);
  $("[data-capture-hotkey-clear]").addEventListener("click", () => setSwitch({ hotkey: { vk: 0, label: "" } }));

  // The controller hotkey: the buttons held together until they're let go (gamepad.rs bits; LT/RT past half way)
  const PAD_NAMES = [[0x100, "LB"], [0x200, "RB"], [0x10000, "LT"], [0x20000, "RT"], [0x400, "Xbox"], [0x20, "View"],
    [0x10, "Menu"], [0x40, "LS"], [0x80, "RS"], [0x1, "Up"], [0x2, "Down"], [0x4, "Left"], [0x8, "Right"], [0x1000, "A"],
    [0x2000, "B"], [0x4000, "X"], [0x8000, "Y"]];
  const padLabel = (buttons) => PAD_NAMES.filter(([bit]) => buttons & bit).map(([, name]) => name).join("+");
  var pickingPad = false;   // var: showCaptureSwitch above reads it
  $("[data-capture-pad]").addEventListener("click", async (event) => {
    if (pickingPad) return;
    pickingPad = true;
    const button = event.currentTarget, note = $("[data-capture-pad-note]");
    button.textContent = "Hold the buttons…"; note.textContent = "";
    let most = 0;
    const until = Date.now() + 10000;
    try {
      while (Date.now() < until) {
        const held = await invoke("capture_pad_held");
        if (held) most |= held;
        else if (most) break;                                   // let go: that's the combination
        button.textContent = most ? padLabel(most) : "Hold the buttons…";
        await new Promise((resolve) => setTimeout(resolve, 40));
      }
      pickingPad = false;
      if (most) await setSwitch({ pad: { buttons: most, label: padLabel(most) } });
      else { note.textContent = "No controller button pressed (is a controller connected?)"; showCaptureSwitch(await invoke("capture_switch")); }
    } catch {
      pickingPad = false;
      showCaptureSwitch(await invoke("capture_switch"));
    }
  });
  $("[data-capture-pad-clear]").addEventListener("click", () => setSwitch({ pad: { buttons: 0, label: "" } }));

  // ---------------------------------------------------------------- plug-ins
  // The one list of plug-ins (bossbar/plugins.json, the meter reads it too): which the button over the game switches
  // (General Settings, made from the list) and each one's status under its name in the sidebar.
  let pluginList = [];
  async function loadPlugins() {
    try { pluginList = (await invoke("plugins")).plugins || []; } catch { pluginList = []; }
    const anchor = $("[data-button-controls]");
    if (anchor) {
      anchor.replaceChildren();
      // every plug-in by default: in DEFAULTS too, which each save starts from (with the file)
      DEFAULTS.button = { controls: Object.fromEntries(pluginList.map((p) => [p.id, true])) };
      settings = merge(DEFAULTS, settings);
      for (const p of pluginList) {
        const label = document.createElement("label");
        label.className = "toggle";
        const input = document.createElement("input");
        input.type = "checkbox";
        input.dataset.k = `button.controls.${p.id}`;
        const strong = document.createElement("strong");
        strong.textContent = p.name;
        const small = document.createElement("small");
        small.textContent = p.about || "";
        const text = document.createElement("div");
        text.append(strong, small);
        label.append(input, document.createElement("span"), text);
        anchor.append(label);
        bindSetting(input);
      }
    }
    for (const p of pluginList) {
      const nav = $(`.nav [data-page="${p.page}"] > span:first-child`);
      if (nav && !nav.querySelector("[data-plugin-status]")) {
        const line = document.createElement("small");
        line.className = "nav-status";
        line.dataset.pluginStatus = p.id;
        nav.append(line);
      }
    }
    fill();
  }
  // Each plug-in's state, from the meter (status.json: the DPS meter, the leaderboards) and Sanctum Helper (its own):
  // nothing shown while it's simply on or off, a short line when it needs a look ("Not signed in", "Stopped: errors").
  let sanctumLook = { at: 0, state: null };
  async function pluginStatuses() {
    if (Date.now() - sanctumLook.at > 3000) {
      sanctumLook.at = Date.now();
      try { sanctumLook.state = await invoke("sanctum_state"); } catch { sanctumLook.state = null; }
    }
    const meter = (status && status.plugins) || {};
    const sanctumOn = settings.sanctum?.enabled !== false;
    const s = sanctumLook.state;
    const sanctum = !sanctumOn ? { state: "off" }
      : !s ? { state: "on" }
      : !s.installed ? { state: "attention", why: "Not in this build" }
      : s.runtimeMissing ? { state: "attention", why: "Needs .NET 9 (its page)" }
      : s.standaloneRunning ? { state: "attention", why: "Another copy is running" }
      : s.starting ? { state: "waiting", why: "Starting" }
      : { state: "on" };
    const shown = { dps: meter.dps, leaderboards: meter.leaderboards, sanctum };
    for (const line of $$("[data-plugin-status]")) {
      const st = shown[line.dataset.pluginStatus] || {};
      const why = st.state === "failed" ? "Stopped after errors" : st.state === "on" || st.state === "off" ? "" : (st.why || "");
      line.textContent = why ? why.charAt(0).toUpperCase() + why.slice(1).split(":")[0] : "";
      line.title = st.why || "";
      line.dataset.state = st.state || "";
    }
  }

  // ---------------------------------------------------------------- settings form
  function fill() {
    for (const input of $$("[data-k]")) {
      const value = get(input.dataset.k);
      if (input.type === "checkbox") input.checked = Boolean(value);
      else if (input.type === "radio") input.checked = input.value === String(value);
      else input.value = value ?? "";
    }
    backdropPreview();
    tokenHint();
    dependents();
  }
  // Settings that only do something with another one on: greyed out while it's off. The automation needs sending on
  // (it only collects proof for the leaderboards), and everything it does needs the automation on.
  function dependents() {
    const sending = Boolean(get("leaderboard.submit"));
    const auto = sending && Boolean(get("leaderboard.automation"));
    $('[data-nav-switch="leaderboards"]').setAttribute("aria-checked", String(sending));
    $('[data-k="leaderboard.automation"]').disabled = !sending;
    $('[data-guide-k="leaderboard.automation"]').disabled = !sending;   // the setup guide's copy (fill() runs on its changes)
    for (const key of ["results.auto_open_battle_data", "results.sim_gizmos", "results.sim_gate", "results.egg_fade_hold", "results.egg_auto_next",
      "results.hold_inputs", "results.hold_seconds"]) $(`[data-k="${key}"]`).disabled = !auto;
    $('[data-k="results.hold_controller"]').disabled = !auto || !status.hidhide_installed;
  }
  // DPS Meter › Overlay: the background slider's preview follows the slider while it moves (saved on release)
  function backdropPreview(value = get("overlay.backdrop") || 0) {
    $("[data-backdrop-value]").textContent = `${value}%`;
    const a = value / 100;     // like the overlay's card: darkest on the left, easing to 40% of that on the right
    $("[data-backdrop-card]").style.background = `linear-gradient(to right, rgba(22, 24, 34, ${a}), rgba(22, 24, 34, ${a * 0.4}))`;
  }
  $("#backdropRange").addEventListener("input", (event) => backdropPreview(Number(event.target.value)));
  function tokenHint() {
    const token = get("leaderboard.token");
    const hint = $("[data-token-hint]");
    void showAccount();
    if (!token) hint.textContent = get("leaderboard.submit") ? "Sign in with Discord (or add a desktop token) to start sending records." : "";
    else hint.textContent = token.startsWith("alb_") ? "Token saved on this PC." : "That doesn't look like a desktop token (they start with alb_).";
  }
  // ---------------------------------------------------------------- Sign in with Discord
  // Discord's page opens in the player's own browser; the API sends that browser back to the app on 127.0.0.1 with a
  // one-time code (auth.rs), which only redeems together with the secret made here (PKCE) for a desktop token.
  const api = () => String(get("leaderboard.api_url") || DEFAULTS.leaderboard.api_url).replace(/\/+$/u, "");
  const b64url = (bytes) => btoa(String.fromCharCode(...new Uint8Array(bytes))).replace(/\+/gu, "-").replace(/\//gu, "_").replace(/=+$/u, "");
  let accountShown = null;
  // the sidebar: "Sign in to sync" while signed out, your name (opens the account settings) once signed in
  // The account in the sidebar: a Discord-styled pill (avatar or Discord logo, your name, a chevron) like the site's,
  // opening a small menu above it. Signed out, the "Sign in with Discord" pill takes its place.
  function showNavAccount(name, avatar = null) {
    $("[data-nav-sign-in]").hidden = Boolean(name);
    const nav = $("[data-nav-account]");
    nav.hidden = !name;
    $("[data-nav-account-name]").textContent = name || "";
    nav.title = name ? `Signed in as ${name}` : "";
    const img = $("[data-nav-avatar]");
    const logo = $(".discord-logo", nav);
    const useLogo = () => { img.hidden = true; img.removeAttribute("src"); logo.style.display = ""; };
    // an avatar that fails to load (a stale hash, offline) falls back to the Discord logo, never a broken image
    img.onerror = useLogo;
    if (name && avatar) { img.hidden = false; logo.style.display = "none"; img.src = avatar; } else useLogo();
    if (!name) closeAccountMenu();
  }
  const accountMenu = $("[data-nav-account-menu]");
  function closeAccountMenu() { accountMenu.hidden = true; $("[data-nav-account]").setAttribute("aria-expanded", "false"); }
  $("[data-nav-sign-in]").addEventListener("click", (event) => signIn(event.currentTarget));
  $("[data-nav-account]").addEventListener("click", (event) => {
    event.stopPropagation();
    accountMenu.hidden = !accountMenu.hidden;
    event.currentTarget.setAttribute("aria-expanded", String(!accountMenu.hidden));
  });
  $$("[data-account-go]").forEach((item) => item.addEventListener("click", () => {
    closeAccountMenu();
    const go = item.dataset.accountGo;
    if (go === "profile") { $('[data-page="site"]').click(); $('[data-site-tab="/?view=profile"]').click(); }
    else if (go === "account") { $('[data-page="leaderboards"]').click(); showTab("leaderboards", "account"); }
    else $("[data-sign-out]").click();
  }));
  document.addEventListener("click", (event) => { if (!event.target.closest(".nav-account-wrap")) closeAccountMenu(); });
  document.addEventListener("keydown", (event) => { if (event.key === "Escape") closeAccountMenu(); });
  async function showAccount() {
    const token = get("leaderboard.token") || "";
    const state = $("[data-account-state]");
    if (!state || accountShown === token) return;
    accountShown = token;
    showNavAccount(null);
    $("[data-sign-out]").hidden = !token;
    showBeta(false);
    if (!token) { state.textContent = "Not signed in."; return; }
    state.textContent = "Checking your account…";
    try {
      const response = await fetch(`${api()}/auth/app/account`, { headers: { authorization: `Bearer ${token}` } });
      const body = await response.json().catch(() => ({}));
      state.textContent = response.ok ? `Signed in as ${body.account?.name || "your Discord account"}.`
        : "This sign-in no longer works (signed out or revoked). Sign in again.";
      showNavAccount(response.ok ? body.account?.name || "Discord" : null, response.ok ? body.account?.avatarUrl || null : null);   // signed out: the sidebar offers sign-in
      // testers: the Beta Menu (the site answers beta and permissions next to account, not inside it)
      showBeta(response.ok && (body.beta ?? body.account?.beta) === true, body.permissions || body.account?.permissions || []);
    } catch {
      state.textContent = "Signed in (couldn't reach AniiLogs to check right now).";
      showNavAccount("Discord");
    }
  }
  // Signing in starts from the sidebar (or the setup guide); the account tab shows the result and signs out.
  let signingIn = null;   // the button of a sign-in waiting for the browser: clicking it again cancels
  async function signIn(button) {
    const state = $("[data-account-state]");
    if (signingIn) { void invoke("auth_listen_cancel"); return; }
    signingIn = button;
    const label = button.innerHTML;   // the Discord logo and text, put back after
    try {
      const verifier = b64url(crypto.getRandomValues(new Uint8Array(32)));
      const challenge = b64url(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)));
      const port = await invoke("auth_listen_start");
      // Discord's page in a window of aLogs' own (a custom API address the window won't show goes to the browser)
      const start = `${api()}/auth/discord/start?app_port=${port}&app_challenge=${challenge}`;
      let inApp = true;
      try { await invoke("auth_window_open", { url: start }); }
      catch { inApp = false; await invoke("open_target", { target: start }); }
      state.textContent = inApp ? "Finish signing in in the Discord window…" : "Finish signing in with your browser, then come back here…";
      button.textContent = "Cancel sign-in";
      const code = await invoke("auth_listen_wait");
      const response = await fetch(`${api()}/auth/app/exchange`, {
        method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ code, verifier, device: "this PC" }) });
      const body = await response.json().catch(() => ({}));
      if (!response.ok || !body.token) throw new Error(body.error || `Sign-in failed (${response.status}).`);
      set("leaderboard.token", body.token);
      setTimeout(() => void syncAccountPrefs(), 0);
      fill(); scheduleSave(`Signed in as ${body.account?.name || "your Discord account"}`);
      siteSignedIn = false; placeSite();
    } catch (error) {
      accountShown = null; void showAccount();
      const text = String(error.message || error);
      if (text !== "Sign-in cancelled.") toast(text, true);
    }
    void invoke("auth_window_close");
    button.innerHTML = label;
    signingIn = null;
  }
  $("[data-sign-out]").addEventListener("click", async () => {
    const token = get("leaderboard.token");
    if (token) {
      try { await fetch(`${api()}/auth/app/sign-out`, { method: "POST", headers: { authorization: `Bearer ${token}` } }); } catch { /* signed out here anyway */ }
    }
    siteSignedIn = false;   // signed out: the site view mustn't keep the old account's sign-in for the next one
    set("leaderboard.token", ""); fill(); scheduleSave("Signed out");
  });

  let saveTimer = 0;
  function scheduleSave(message = "Saved") {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      try {
        await saveChanges();
        if (message) toast(message);
      } catch (error) {
        toast(`Couldn't save: ${error}`, true);
      }
    }, 500);
  }
  // ---------------------------------------------------------------- shared with the website
  // Settings the website has too, kept with your AniiLogs account so changing one changes both: the theme and "Show my
  // online status" (also the site's Edit profile). Read when aLogs opens, after signing in and whenever this window
  // comes back to the front (you may have changed them on the site); sent when you change them here.
  async function accountPrefs(body) {
    if (!get("leaderboard.token")) return null;
    const response = await fetch(`${api()}/account/preferences`, body
      ? { method: "POST", headers: { ...authHeaders(), "content-type": "application/json" }, body: JSON.stringify(body) }
      : { headers: authHeaders() });
    if (!response.ok) throw new Error((await response.json().catch(() => ({}))).error || `The server answered ${response.status}.`);
    return response.json();
  }
  async function shareTheme(theme, custom) {
    try {
      await accountPrefs({ theme: theme === "custom" ? { id: "custom", custom: customColors(custom) } : { id: theme } });
    } catch {
      // offline: it's shared the next time it changes
    }
  }
  async function syncAccountPrefs() {
    const box = $("[data-presence-toggle]");
    box.disabled = !get("leaderboard.token");
    let prefs;
    try { prefs = await accountPrefs(); } catch { return; }
    if (!prefs) return;
    box.checked = prefs.showPresence;
    const { theme, custom } = savedTheme();
    const shared = prefs.theme;
    if (!shared) { void shareTheme(theme, custom); return; }       // nothing on the account yet: this window's theme
    if ((shared.id === "custom" || THEMES[shared.id])
      && (shared.id !== theme || (shared.id === "custom" && JSON.stringify(customColors(shared.custom || {})) !== JSON.stringify(customColors(custom))))) {
      saveTheme(shared.id, shared.custom || {}, false);
    }
  }
  $("[data-presence-toggle]").addEventListener("change", async (event) => {
    const box = event.target;
    try {
      box.checked = (await accountPrefs({ showPresence: box.checked })).showPresence;
    } catch (error) {
      box.checked = !box.checked;
      toast(`Couldn't change your online status: ${error.message || error}`, true);
    }
  });
  window.addEventListener("focus", () => void syncAccountPrefs());

  // ---------------------------------------------------------------- Photos
  // The game's photos (photos.rs finds them and lets this page show them) and which are on the profile already
  // (the API). Picked ones are made into WebP here, up to 2560 px wide, and uploaded with what the game recorded.
  const photos = { list: [], folder: null, uploaded: new Map(), selected: new Set(), refused: new Set(), busy: false, loaded: false };
  const convertFileSrc = window.__TAURI__.core.convertFileSrc;
  const authHeaders = () => ({ authorization: `Bearer ${get("leaderboard.token")}` });

  async function loadPhotos() {
    try {
      const found = await invoke("game_photos");
      photos.list = (found.photos || []).map((p) => (p.localTime && p.takenAt
        // a time from the file name is local time read as UTC: to real time (it was hours off away from UTC)
        ? { ...p, takenAt: p.takenAt + new Date(p.takenAt * 1000).getTimezoneOffset() * 60, localTime: false } : p));
      photos.folder = found.folder;
    } catch {
      photos.list = [];
    }
    photos.uploaded = new Map();
    if (get("leaderboard.token")) {
      try {
        const response = await fetch(`${api()}/profile/photos`, { headers: authHeaders() });
        if (response.ok) for (const photo of (await response.json()).photos || []) photos.uploaded.set(photo.gamePhotoId, photo.id);
      } catch {
        // offline: everything just shows as not uploaded yet
      }
    }
    for (const id of photos.selected) if (photos.uploaded.has(id)) photos.selected.delete(id);
    photos.loaded = true;
    renderPhotos();
  }

  function renderPhotos() {
    const signedIn = Boolean(get("leaderboard.token"));
    const onProfile = photos.list.filter((p) => photos.uploaded.has(p.id)).length;
    $("[data-photos-state]").textContent = !photos.folder
      ? "Aniimo's photo folder wasn't found yet: start the game once with aLogs running."
      : !photos.list.length ? "No photos yet. Take one in Aniimo (with Storage set to Local, or Local and cloud) and it shows up here."
      : !signedIn ? `${photos.list.length} ${photos.list.length === 1 ? "photo" : "photos"}. Sign in with the Discord button in the sidebar to put them on your profile.`
      : `${photos.list.length} ${photos.list.length === 1 ? "photo" : "photos"} · ${onProfile} on your profile. Click photos to pick them.`;
    const upload = $("[data-photos-upload]");
    if (!photos.busy) upload.textContent = photos.selected.size ? `Upload ${photos.selected.size} selected` : "Upload selected";
    upload.disabled = photos.busy || !signedIn || !photos.selected.size;
    const grid = $("[data-photos-grid]");
    // another page open (the automatic upload checks every minute): no grid of full-size pictures held in memory;
    // opening Photos loads it again
    if (!grid.offsetParent) { grid.replaceChildren(); return; }
    grid.replaceChildren(...photos.list.map((photo) => {
      const uploadedId = photos.uploaded.get(photo.id);
      const card = document.createElement("div");
      card.className = `photo-card${uploadedId ? " is-uploaded" : ""}${photos.selected.has(photo.id) ? " is-selected" : ""}`;
      const img = document.createElement("img");
      img.loading = "lazy"; img.alt = ""; img.src = convertFileSrc(photo.file);
      const meta = document.createElement("div");
      meta.className = "photo-meta";
      const when = document.createElement("span");
      when.textContent = photo.takenAt ? new Date(photo.takenAt * 1000).toLocaleString([], { dateStyle: "medium", timeStyle: "short" }) : "";
      meta.append(when);
      if (uploadedId) {
        const on = document.createElement("span");
        on.className = "photo-on"; on.textContent = "✓ On profile";
        const remove = document.createElement("button");
        remove.type = "button"; remove.className = "link"; remove.textContent = "Remove";
        remove.addEventListener("click", (event) => { event.stopPropagation(); void removePhoto(photo, uploadedId); });
        meta.append(on, remove);
      } else {
        const check = document.createElement("input");
        check.type = "checkbox"; check.className = "photo-check"; check.tabIndex = -1; check.checked = photos.selected.has(photo.id);
        card.append(check);
        card.addEventListener("click", () => {
          if (photos.busy) return;
          if (photos.selected.has(photo.id)) photos.selected.delete(photo.id); else photos.selected.add(photo.id);
          renderPhotos();
        });
      }
      card.prepend(img);
      card.append(meta);
      return card;
    }));
  }

  // A photo as WebP, at most 2560 px wide (the 4K PNGs are 6-13 MB; this is ~0.5-1.5 MB and looks the same).
  async function photoWebp(photo) {
    const bitmap = await createImageBitmap(await (await fetch(convertFileSrc(photo.file))).blob());
    const scale = Math.min(1, 2560 / bitmap.width);
    const width = Math.round(bitmap.width * scale), height = Math.round(bitmap.height * scale);
    const canvas = new OffscreenCanvas(width, height);
    canvas.getContext("2d").drawImage(bitmap, 0, 0, width, height);
    bitmap.close();
    let blob = await canvas.convertToBlob({ type: "image/webp", quality: 0.9 });
    if (blob.size > 3.8 * 1024 * 1024) blob = await canvas.convertToBlob({ type: "image/webp", quality: 0.78 });
    return { blob, width, height };
  }

  async function uploadPhoto(photo) {
    const { blob, width, height } = await photoWebp(photo);
    const form = new FormData();
    form.set("photo", blob, "photo.webp");
    form.set("meta", JSON.stringify({ gamePhotoId: photo.id, gameUid: photo.gameUid, takenAt: photo.takenAt, sceneId: photo.sceneId,
      pos: photo.pos, aniimo: photo.aniimo, width, height }));
    const response = await fetch(`${api()}/profile/photos`, { method: "POST", headers: authHeaders(), body: form });
    const result = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(result.error || `The server answered ${response.status}.`);
    photos.uploaded.set(photo.id, result.id);
  }

  async function uploadSelected() {
    const picked = photos.list.filter((p) => photos.selected.has(p.id));
    if (!picked.length || photos.busy) return;
    photos.busy = true;
    const button = $("[data-photos-upload]");
    let done = 0;
    try {
      for (const photo of picked) {
        button.textContent = `Uploading ${done + 1} of ${picked.length}…`; button.disabled = true;
        await uploadPhoto(photo);
        photos.selected.delete(photo.id); done += 1;
        renderPhotos();
      }
      toast(done === 1 ? "Photo added to your profile" : `${done} photos added to your profile`);
    } catch (error) {
      toast(`Couldn't upload ${done ? "all the photos" : "the photo"}: ${error.message || error}`, true);
    } finally {
      photos.busy = false;
      renderPhotos();
    }
  }

  async function removePhoto(photo, uploadedId) {
    try {
      const response = await fetch(`${api()}/profile/photos/${encodeURIComponent(uploadedId)}`, { method: "DELETE", headers: authHeaders() });
      if (!response.ok) throw new Error((await response.json().catch(() => ({}))).error || `The server answered ${response.status}.`);
      photos.uploaded.delete(photo.id);
      renderPhotos();
      toast("Photo removed from your profile");
    } catch (error) {
      toast(`Couldn't remove the photo: ${error.message || error}`, true);
    }
  }

  $("[data-photos-upload]").addEventListener("click", () => void uploadSelected());
  $("[data-photos-refresh]").addEventListener("click", () => void loadPhotos());
  $("[data-photos-select]").addEventListener("click", () => {
    for (const photo of photos.list) if (!photos.uploaded.has(photo.id)) photos.selected.add(photo.id);
    renderPhotos();
  });
  $('[data-page="photos"]').addEventListener("click", () => void loadPhotos());

  // Upload new photos automatically: photos taken since that was switched on go up on their own, a few seconds after
  // the game saves them. Every 3 s the app only checks when the game last saved a photo (no site request); once that
  // has changed and then held still for a check (the game has finished writing), the new ones are sent.
  let photoStamp = null, photoPending = false;
  async function uploadNewPhotos() {
    await loadPhotos();
    const since = get("app.photos_auto_since");
    // only the character you're playing (other characters or accounts on this PC have their own photos), and not one
    // the site already turned down (it blocked every photo after it, with the same error each time)
    const uid = status.character?.uid;
    if (!uid) return;
    const fresh = photos.list.filter((p) => !photos.uploaded.has(p.id) && !photos.refused.has(p.id) && p.gameUid === uid
      && (p.takenAt || 0) >= since);
    if (!fresh.length) return;
    photos.busy = true;
    let added = 0;
    try {
      for (const photo of fresh) {
        try { await uploadPhoto(photo); added += 1; }
        catch (error) { photos.refused.add(photo.id); toast(`Couldn't upload a new photo: ${error.message || error}`, true); }
      }
      if (added) toast(added === 1 ? "New photo added to your profile" : `${added} new photos added to your profile`);
    } finally {
      photos.busy = false;
      renderPhotos();
    }
  }
  setInterval(async () => {
    if (!get("app.photos_auto")) {
      if (get("app.photos_auto_since")) { set("app.photos_auto_since", 0); scheduleSave(""); }
      photoStamp = null;
      return;
    }
    if (!get("leaderboard.token") || photos.busy) return;
    if (!get("app.photos_auto_since")) { set("app.photos_auto_since", Math.floor(Date.now() / 1000)); scheduleSave(""); return; }
    let stamp;
    try { stamp = await invoke("photos_stamp"); } catch { return; }
    if (stamp !== photoStamp) {
      photoPending = true;   // (the first look too: photos taken while aLogs was closed)
      photoStamp = stamp;
      return;
    }
    if (!photoPending) return;
    photoPending = false;
    // not during a fight (photos.rs photos_wait); it
    // stays pending and this asks again in 3 s. Asked there because `status` here stops refreshing while the window is hidden
    let wait = false;
    try { wait = await invoke("photos_wait"); } catch { /* upload as always */ }
    if (wait) { photoPending = true; return; }
    await uploadNewPhotos();
  }, 3000);

  function bindSetting(input) {
    input.addEventListener("change", () => {
      let value = input.type === "checkbox" ? input.checked : input.value.trim();
      if (input.dataset.type === "number") value = Number(value);
      if (input.dataset.validate === "token" && value && !value.startsWith("alb_")) {
        tokenHint();
        $("[data-token-hint]").textContent = "That doesn't look like a desktop token (they start with alb_). Not saved.";
        return;
      }
      set(input.dataset.k, value);
      markUserSet(input.dataset.k);
      tokenHint();
      dependents();
      scheduleSave();
    });
  }
  for (const input of $$("[data-k]")) bindSetting(input);
  $("[data-reveal]").addEventListener("click", (event) => {
    const field = $('[data-k="leaderboard.token"]');
    field.type = field.type === "password" ? "text" : "password";
    event.currentTarget.textContent = field.type === "password" ? "Show" : "Hide";
  });

  // ---------------------------------------------------------------- buttons
  $$("[data-open]").forEach((button) => button.addEventListener("click", async () => {
    if (button.dataset.install !== undefined) return;            // a downloaded update: installUpdate handles it
    try {
      await invoke("open_target", { target: button.dataset.open });
    } catch (error) {
      toast(String(error), true);
    }
  }));
  // Report a problem (Dashboard): a dialog with an optional note; Send logs zips the app's logs (scrubbed) and sends
  // them with the game UID it last saw.
  const report = $("[data-report]");
  const closeReport = () => { report.hidden = true; };
  $("[data-report-open]").addEventListener("click", () => {
    report.hidden = false;
    $("[data-logs-note]").focus();
  });
  $("[data-report-close]").addEventListener("click", closeReport);

  // Screenshots for the report (optional, up to 5): picked from files or pasted from the clipboard. Big ones (a 4K
  // PNG is 6-13 MB) are made into WebP, at most 2560 px wide, to fit the API's 4 MB a picture.
  const reportShots = [];
  const SHOT_TYPES = ["image/png", "image/jpeg", "image/webp"];
  async function shotBlob(file) {
    if (SHOT_TYPES.includes(file.type) && file.size <= 3.8 * 1024 * 1024) return file;
    const bitmap = await createImageBitmap(file);
    const scale = Math.min(1, 2560 / bitmap.width);
    const canvas = new OffscreenCanvas(Math.round(bitmap.width * scale), Math.round(bitmap.height * scale));
    canvas.getContext("2d").drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    bitmap.close();
    let blob = await canvas.convertToBlob({ type: "image/webp", quality: 0.9 });
    if (blob.size > 3.8 * 1024 * 1024) blob = await canvas.convertToBlob({ type: "image/webp", quality: 0.7 });
    return blob;
  }
  function renderShots() {
    $("[data-logs-shots]").replaceChildren(...reportShots.map((shot, index) => {
      const item = document.createElement("div");
      item.className = "report-shot";
      const img = document.createElement("img");
      img.src = shot.url; img.alt = `Screenshot ${index + 1}`;
      const remove = document.createElement("button");
      remove.type = "button"; remove.className = "report-shot-remove"; remove.textContent = "×"; remove.title = "Remove";
      remove.addEventListener("click", () => { URL.revokeObjectURL(shot.url); reportShots.splice(index, 1); renderShots(); });
      item.append(img, remove);
      return item;
    }));
    $("[data-logs-shots-add]").disabled = reportShots.length >= 5;
  }
  async function addShots(files) {
    for (const file of files) {
      if (!file.type.startsWith("image/")) continue;
      if (reportShots.length >= 5) { toast("Up to 5 screenshots per report.", true); break; }
      try {
        const blob = await shotBlob(file);
        reportShots.push({ blob, url: URL.createObjectURL(blob) });
      } catch {
        toast("That picture couldn't be read.", true);
      }
    }
    renderShots();
  }
  function clearShots() {
    for (const shot of reportShots) URL.revokeObjectURL(shot.url);
    reportShots.length = 0;
    renderShots();
  }
  $("[data-logs-shots-add]").addEventListener("click", () => $("[data-logs-shots-file]").click());
  $("[data-logs-shots-file]").addEventListener("change", (event) => {
    void addShots([...event.target.files]);
    event.target.value = "";
  });
  document.addEventListener("paste", (event) => {
    if (report.hidden) return;
    const files = [...(event.clipboardData?.files || [])].filter((file) => file.type.startsWith("image/"));
    if (!files.length) return;
    event.preventDefault();
    void addShots(files);
  });
  report.addEventListener("click", (event) => { if (event.target === report) closeReport(); });
  document.addEventListener("keydown", (event) => { if (event.key === "Escape" && !report.hidden) closeReport(); });
  // One report to the site: the logs, a note and any screenshots. With devFeedback the logs also carry what the tester
  // said about the Dev switches (dev-feedback.json, made in dev.rs), which is how Beta Menu > Dev sends it (dev-menu.js).
  // Throws with words for the user.
  const NO_UID = "aLogs hasn't seen your game UID yet: open Aniimo with the meter running, then try again.";
  async function sendReport({ note = "", shots = [], devFeedback = false } = {}) {
    const uid = status.character?.uid;
    if (!uid) throw new Error(NO_UID);
    const zip = Uint8Array.from(atob(await invoke("collect_logs", { devFeedback })), (c) => c.charCodeAt(0));
    const version = await window.__TAURI__.app.getVersion();
    const rc = await invoke("build_number").catch(() => 0);
    const form = new FormData();
    form.set("uid", uid);
    form.set("note", note);
    form.set("app_version", rc ? `${version}+rc.${rc}` : version);
    form.set("logs", new Blob([zip], { type: "application/zip" }), "logs.zip");
    shots.forEach((shot, index) => form.append("screenshot", shot, `screenshot-${index + 1}`));
    const response = await fetch(`${api()}/logs/reports`, { method: "POST", body: form });
    const result = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(result.error || `The server answered ${response.status}.`);
  }
  window.alogsSendReport = sendReport;

  $("[data-logs-send]").addEventListener("click", async () => {
    const button = $("[data-logs-send]");
    if (!status.character?.uid) {
      toast(NO_UID, true);
      return;
    }
    button.disabled = true;
    button.textContent = "Sending…";
    try {
      await sendReport({ note: $("[data-logs-note]").value.trim(), shots: reportShots.map((shot) => shot.blob) });
      $("[data-logs-note]").value = "";
      const shots = reportShots.length;
      clearShots();
      closeReport();
      toast(shots ? `Logs and ${shots === 1 ? "a screenshot" : `${shots} screenshots`} sent. Thanks!` : "Logs sent. Thanks!");
    } catch (error) {
      toast(`Couldn't send the logs: ${error.message || error}`, true);
    } finally {
      button.disabled = false;
      button.textContent = "Send logs";
    }
  });

  // ---------------------------------------------------------------- Beta Menu (testers only)
  // The tests to do and bug reports, sent like Report a problem but always with the logs and the tester's sign-in
  // (testers have the developer role; the server checks it). Shown only when the account says "beta": the developer
  // role with Developer Mode on (players' log reports need more: administrators). The test list is shared: it comes
  // from the server (beta-tests.json, written for each RC, is the fallback offline and what the owner publishes), and
  // only an account with the "beta.manage" permission (the owner today) can publish it or retire a test. Each tester's
  // own results come back with it, so "Hide what I marked Working" follows them; notes stay in this window until sent.
  const BETA_KEY = "alogs:beta:v1";
  const BETA_HIDE_KEY = "alogs:beta:hide-working";
  const BUG_SHOTS = 3, BUG_CARDS = 10;
  const AREAS = ["DPS meter", "Overlay / DPS window", "Capture button", "Sanctum Helper", "Leaderboards & profile", "Photos",
    "Sign-in & account", "Install & updates", "Something else"];
  const SEVERITY = [["blocks", "Stops me testing"], ["wrong", "Wrong result"], ["annoying", "Annoying"], ["looks", "Looks off"]];
  let bundledTests = { build: "", items: [] };   // this build's own list
  let betaDeleted = [];                          // tests the site deleted 30 days after they were retired
  let betaTests = { build: "", items: [] };      // what's shown: the shared list (retired ones only for the owner)
  let betaShared = false;                        // false: the server couldn't be reached, the bundled list is shown
  let betaOwner = false;
  let hideWorking = false;
  let beta = { build: "", tests: {}, bugs: [] };
  const bugShots = new Map();   // bug card key -> [{ blob, url }]: pictures stay in memory only (not in the saved notes)
  let activeBug = null;         // where a pasted picture goes
  const el = (tag, className, props = {}) => Object.assign(document.createElement(tag), className ? { className } : {}, props);
  const newBug = () => ({ key: crypto.randomUUID(), title: "", area: "", severity: "", text: "" });
  const dayLabel = (seconds) => new Date(seconds * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short" });
  let betaSaveTimer = 0;
  function saveBeta() {
    clearTimeout(betaSaveTimer);
    betaSaveTimer = setTimeout(() => {
      try { localStorage.setItem(BETA_KEY, JSON.stringify(beta)); } catch { /* kept for this session anyway */ }
    }, 300);
  }
  async function loadBeta() {
    try { bundledTests = await (await fetch("beta-tests.json")).json(); } catch { bundledTests = { build: "", items: [] }; }
    try { beta = { ...beta, ...JSON.parse(localStorage.getItem(BETA_KEY) || "{}") }; } catch { /* a fresh start */ }
    try { hideWorking = localStorage.getItem(BETA_HIDE_KEY) === "1"; } catch { /* off */ }
    $("[data-beta-hide-working]").checked = hideWorking;
    if (!Array.isArray(beta.bugs) || !beta.bugs.length) beta.bugs = [newBug()];
    betaTests = bundledTests;
    showTests();
    renderBugs();
  }
  // The shared list and this tester's own last results ({ id: { working, at } }), each time the Beta Menu opens.
  async function refreshBetaList() {
    try {
      const response = await fetch(`${api()}/beta/tests`, { headers: authHeaders() });
      if (!response.ok) throw new Error(String(response.status));
      const body = await response.json();
      betaTests = { build: body.build || bundledTests.build, items: Array.isArray(body.items) ? body.items : [] };
      betaDeleted = Array.isArray(body.deleted) ? body.deleted : [];
      for (const [id, mine] of Object.entries(body.mine || {})) {
        const state = (beta.tests[id] ??= { tested: false, result: "", note: "" });
        if (!state.sentAt || mine.at > state.sentAt) Object.assign(state, { sent: mine.working ? "working" : "broken", sentAt: mine.at });
      }
      betaShared = true;
    } catch {
      betaTests = bundledTests;
      betaShared = false;
    }
    showTests();
  }
  function showTests() {
    // a test no longer on the shared list loses its notes (kept while offline)
    if (betaShared) {
      const ids = new Set(betaTests.items.map((item) => item.id));
      beta.tests = Object.fromEntries(Object.entries(beta.tests || {}).filter(([id]) => ids.has(id)));
    }
    beta.build = betaTests.build;
    $("[data-beta-build]").textContent = betaTests.build;
    $("[data-beta-offline]").hidden = betaShared;
    renderOwner();
    renderTests();
    saveBeta();
  }
  // Testers get all of it. On a beta build everyone gets the menu, with just its Dev tab (dev-menu.js).
  let betaTester = false, betaBuild = false;
  function showBeta(on, permissions = []) {
    betaTester = on;
    $("[data-beta-nav]").hidden = !on && !betaBuild;
    $$("[data-beta-tester]").forEach((tab) => { tab.hidden = !on; });
    betaOwner = on && permissions.includes("beta.manage");
    if (on) void refreshBetaList();
    if (!on && betaBuild) showTab("beta", "dev");
    if (!on && !betaBuild && $('[data-page-body="beta"]').classList.contains("is-active")) $('[data-page="dashboard"]').click();
  }
  invoke("sanctum_state").then((found) => {
    betaBuild = found?.beta === true;
    $("[data-beta-build-only]").hidden = !betaBuild;
    showBeta(betaTester, betaOwner ? ["beta.manage"] : []);
  }).catch(() => {});
  $('[data-page="beta"]').addEventListener("click", () => { if (betaTester) void refreshBetaList(); });
  $("[data-beta-hide-working]").addEventListener("change", (event) => {
    hideWorking = event.target.checked;
    try { localStorage.setItem(BETA_HIDE_KEY, hideWorking ? "1" : "0"); } catch { /* this session only */ }
    renderTests();
  });

  // The owner's part: publish this build's list (new tests added, wording updated, retired ones stay retired) and
  // retire or bring back a test for everyone.
  async function ownerCall(path, body) {
    const response = await fetch(`${api()}/beta/tests${path}`, { method: "POST", headers: { ...authHeaders(), "content-type": "application/json" },
      body: JSON.stringify(body) });
    const result = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(result.error || `The server answered ${response.status}.`);
    return result;
  }
  function renderOwner() {
    const box = $("[data-beta-owner]");
    box.hidden = !betaOwner;
    if (!betaOwner) return;
    // (a test deleted 30 days after it was retired isn't "new": the site says which)
    const listed = new Set([...betaTests.items.map((item) => item.id), ...(betaDeleted || [])]);
    const fresh = bundledTests.items.filter((item) => !listed.has(item.id)).length;
    $("[data-beta-owner-text]").textContent = !betaShared ? "The shared list couldn't be reached, so this is the build's own list."
      : fresh ? `This build (${bundledTests.build}) has ${fresh === 1 ? "1 test" : `${fresh} tests`} the shared list doesn't have yet.`
        : `The shared list has every test from ${bundledTests.build}.`;
    $("[data-beta-publish]").disabled = !betaShared;
  }
  $("[data-beta-publish]").addEventListener("click", async (event) => {
    const button = event.currentTarget;
    button.disabled = true;
    try {
      const result = await ownerCall("", { build: bundledTests.build, items: bundledTests.items });
      toast(`Published ${bundledTests.build}: ${result.added ?? 0} added, ${result.updated ?? 0} updated${result.skipped ? `, ${Array.isArray(result.skipped) ? result.skipped.length : result.skipped} already deleted` : ""}.`);
      await refreshBetaList();
    } catch (error) {
      toast(`Couldn't publish: ${error.message || error}`, true);
    } finally {
      button.disabled = false;
    }
  });
  async function retireTest(item, retired) {
    try {
      await ownerCall(`/${encodeURIComponent(item.id)}/retire`, { retired });
      toast(retired ? `Retired "${item.title}" for everyone.` : `"${item.title}" is back on everyone's list.`);
      await refreshBetaList();
    } catch (error) {
      toast(`Couldn't change it: ${error.message || error}`, true);
    }
  }

  function testCard(item) {
    const state = (beta.tests[item.id] ??= { tested: false, result: "", note: "" });
    const card = el("article", "beta-card");
    const head = el("div", "beta-card-head");
    const tick = el("input", "beta-tick", { type: "checkbox", checked: state.tested, title: "I tested this" });
    tick.setAttribute("aria-label", `Tested: ${item.title}`);
    const title = el("div", "beta-title");
    const name = el("strong", "", { textContent: item.title });
    if (state.sent) {
      name.append(" ", el("span", `beta-sent-tag${state.sent === "broken" ? " bad" : ""}`,
        { textContent: `You sent: ${state.sent === "working" ? "Working" : "Not working"}${state.sentAt ? ` · ${dayLabel(state.sentAt)}` : ""}` }));
    }
    title.append(name, el("small", "", { textContent: item.how }));
    const result = el("select", "beta-result");
    result.setAttribute("aria-label", `Result: ${item.title}`);
    result.append(el("option", "", { value: "", textContent: "Working?", disabled: true }),
      el("option", "", { value: "working", textContent: "Working" }), el("option", "", { value: "broken", textContent: "Not working" }));
    result.value = state.result;
    head.append(tick, title, result);
    const more = item.details?.length ? el("button", "beta-more", { type: "button", textContent: "More", title: "Show the details" }) : null;
    head.append(more || el("span"));   // the dropdowns line up whether a card has More or not
    const note = el("input", "beta-note", { type: "text", maxLength: 1000, value: state.note, placeholder: "Notes (optional): what you saw, where, which settings" });
    const need = el("p", "beta-need", { textContent: "Choose Working or Not working for a test you ticked.", hidden: true });
    card.append(head, need, note);
    if (more) {
      const details = el("div", "beta-details", { hidden: true });
      details.append(...item.details.map((line) => el("p", "", { textContent: line })));
      card.insertBefore(details, need);
      more.addEventListener("click", () => {
        details.hidden = !details.hidden;
        card.classList.toggle("is-open", !details.hidden);
        more.textContent = details.hidden ? "More" : "Less";
      });
    }
    if (betaOwner) {
      const owner = el("div", "beta-owner-row");
      const counts = item.results ? `${item.results.working ?? 0} working, ${item.results.broken ?? 0} not working` : "";
      const retire = el("button", "link", { type: "button", textContent: "Retire for everyone" });
      retire.addEventListener("click", () => void retireTest(item, true));
      owner.append(el("span", "hint", { textContent: counts ? `Testers so far: ${counts}` : "No results yet" }), retire);
      card.append(owner);
    }
    const update = () => {
      card.classList.toggle("is-tested", state.tested);
      card.classList.toggle("is-broken", state.result === "broken");
      if (!state.tested || state.result) { card.classList.remove("needs-result"); need.hidden = true; }
      testsSummary();
      saveBeta();
    };
    tick.addEventListener("change", () => { state.tested = tick.checked; update(); });
    result.addEventListener("change", () => { state.result = result.value; state.tested = tick.checked = true; update(); });
    note.addEventListener("input", () => { state.note = note.value; saveBeta(); });
    card.classList.toggle("is-tested", state.tested);
    card.classList.toggle("is-broken", state.result === "broken");
    card.dataset.id = item.id;
    return card;
  }
  const activeTests = () => betaTests.items.filter((item) => !item.retired);
  // hidden by the filter: what this tester last sent as Working and hasn't ticked again since
  const hiddenByFilter = (item) => hideWorking && beta.tests[item.id]?.sent === "working" && !beta.tests[item.id]?.tested;
  // Each test under the plug-in it tests (its "plugin": the list's id, or "core" for aLogs itself), in the list's order
  function pluginOfTest(item) {
    return item.plugin || bundledTests.items.find((own) => own.id === item.id)?.plugin || "core";
  }
  function renderTests() {
    const shown = activeTests().filter((item) => !hiddenByFilter(item));
    const hidden = activeTests().length - shown.length;
    const sections = [{ id: "core", name: "aLogs itself" }, ...pluginList.map((p) => ({ id: p.id, name: p.name }))];
    const known = new Set(sections.map((s) => s.id));
    $("[data-beta-tests]").replaceChildren(...sections.flatMap((section) => {
      const mine = shown.filter((item) => (known.has(pluginOfTest(item)) ? pluginOfTest(item) : "core") === section.id);
      return mine.length ? [el("h3", "beta-section", { textContent: section.name }), ...mine.map(testCard)] : [];
    }));
    $("[data-beta-hidden]").textContent = hidden ? `(${hidden} hidden)` : "";
    const retired = betaOwner ? betaTests.items.filter((item) => item.retired) : [];
    $("[data-beta-retired]").hidden = !retired.length;
    $("[data-beta-retired-count]").textContent = String(retired.length);
    $("[data-beta-retired-list]").replaceChildren(...retired.map((item) => {
      const row = el("li");
      const back = el("button", "link", { type: "button", textContent: "Bring back" });
      back.addEventListener("click", () => void retireTest(item, false));
      const gone = item.deletesAt ? ` (deleted ${new Date(item.deletesAt * 1000).toLocaleDateString()})` : "";
      row.append(el("span", "", { textContent: item.title + gone }), back);
      return row;
    }));
    testsSummary();
  }
  function testsSummary() {
    const items = activeTests();
    const states = items.map((item) => beta.tests[item.id] || {});
    const tested = states.filter((s) => s.tested).length;
    const broken = states.filter((s) => s.tested && s.result === "broken").length;
    const open = states.filter((s) => !s.tested && !s.sent).length;
    $("[data-beta-count]").textContent = `${tested}/${items.length}`;
    $("[data-beta-tests-summary]").textContent = tested
      ? `${tested} to send${broken ? `, ${broken} not working` : ""}, ${open} never tested. Your logs go with them.`
      : `Nothing ticked yet. ${open} never tested.`;
    $("[data-beta-tests-send]").disabled = !tested;
  }

  function bugCard(bug, index) {
    const card = el("article", "beta-card bug-card");
    const head = el("div", "bug-head");
    head.append(el("strong", "", { textContent: `Bug ${index + 1}` }));
    if (beta.bugs.length > 1) {
      const remove = el("button", "bug-remove", { type: "button", textContent: "×", title: "Remove this bug" });
      remove.setAttribute("aria-label", `Remove bug ${index + 1}`);
      remove.addEventListener("click", () => {
        for (const shot of bugShots.get(bug.key) || []) URL.revokeObjectURL(shot.url);
        bugShots.delete(bug.key);
        beta.bugs = beta.bugs.filter((b) => b !== bug);
        renderBugs();
        saveBeta();
      });
      head.append(remove);
    }
    const field = (label, input) => { const f = el("label", "field"); f.append(el("span", "", { textContent: label }), input); return f; };
    const title = el("input", "", { type: "text", maxLength: 120, value: bug.title, placeholder: "e.g. The DPS window froze after a BREAK" });
    const area = el("select");
    area.append(el("option", "", { value: "", textContent: "Choose…" }), ...AREAS.map((a) => el("option", "", { value: a, textContent: a })));
    area.value = bug.area;
    const severity = el("select");
    severity.append(el("option", "", { value: "", textContent: "Choose…" }), ...SEVERITY.map(([v, t]) => el("option", "", { value: v, textContent: t })));
    severity.value = bug.severity;
    const text = el("textarea", "", { rows: 4, maxLength: 4000, value: bug.text,
      placeholder: "What you did, what happened, and what you expected instead. When it happened helps too (e.g. \"about 21:40, second Interlink run\")." });
    const pair = el("div", "bug-pair");
    pair.append(field("Where", area), field("How bad", severity));
    const shots = el("div", "field");
    const list = el("div", "report-shots");
    const add = el("button", "", { type: "button", textContent: "Add screenshots" });
    const file = el("input", "", { type: "file", accept: "image/png,image/jpeg,image/webp", multiple: true, hidden: true });
    const row = el("div", "row labeled");
    row.append(add, file, el("span", "hint", { textContent: `Up to ${BUG_SHOTS}, or paste with Ctrl+V. Crop out anything private.` }));
    shots.append(el("span", "", { textContent: "Screenshots (optional)" }), list, row);
    const need = el("p", "beta-need", { textContent: "A bug needs a title and what happened.", hidden: true });
    card.append(head, field("Title", title), pair, field("What happened", text), shots, need);
    const renderShots = () => {
      const mine = bugShots.get(bug.key) || [];
      list.replaceChildren(...mine.map((shot, i) => {
        const item = el("div", "report-shot");
        const remove = el("button", "report-shot-remove", { type: "button", textContent: "×", title: "Remove" });
        remove.addEventListener("click", () => { URL.revokeObjectURL(shot.url); mine.splice(i, 1); renderShots(); bugsSummary(); });
        item.append(el("img", "", { src: shot.url, alt: `Screenshot ${i + 1}` }), remove);
        return item;
      }));
      add.disabled = mine.length >= BUG_SHOTS;
    };
    card.addShots = async (files) => {
      const mine = bugShots.get(bug.key) || [];
      bugShots.set(bug.key, mine);
      for (const f of files) {
        if (!f.type.startsWith("image/")) continue;
        if (mine.length >= BUG_SHOTS) { toast(`Up to ${BUG_SHOTS} screenshots per bug.`, true); break; }
        try { const blob = await shotBlob(f); mine.push({ blob, url: URL.createObjectURL(blob) }); } catch { toast("That picture couldn't be read.", true); }
      }
      renderShots();
      bugsSummary();
    };
    add.addEventListener("click", () => file.click());
    file.addEventListener("change", () => { void card.addShots([...file.files]); file.value = ""; });
    const changed = () => {
      Object.assign(bug, { title: title.value, area: area.value, severity: severity.value, text: text.value });
      if (bug.title.trim() && bug.text.trim()) { card.classList.remove("needs-result"); need.hidden = true; }
      bugsSummary();
      saveBeta();
    };
    for (const input of [title, area, severity, text]) input.addEventListener(input.tagName === "SELECT" ? "change" : "input", changed);
    card.addEventListener("focusin", () => { activeBug = card; });
    renderShots();
    return card;
  }
  function renderBugs() {
    const cards = beta.bugs.map(bugCard);
    $("[data-bugs]").replaceChildren(...cards);
    activeBug = cards.at(-1);
    $("[data-bug-add]").disabled = beta.bugs.length >= BUG_CARDS;
    bugsSummary();
  }
  const bugFilled = (bug) => bug.title.trim() || bug.text.trim() || (bugShots.get(bug.key) || []).length;
  function bugsSummary() {
    const filled = beta.bugs.filter(bugFilled).length;
    $("[data-bug-count]").textContent = filled ? String(filled) : "";
    $("[data-bugs-summary]").textContent = filled ? `${filled === 1 ? "1 bug" : `${filled} bugs`}, with your logs.` : "";
    $("[data-bugs-send]").disabled = !filled;
  }
  $("[data-bug-add]").addEventListener("click", () => {
    beta.bugs.push(newBug());
    renderBugs();
    saveBeta();
    $("[data-bugs] .bug-card:last-child input").focus();
  });
  document.addEventListener("paste", (event) => {
    if (!$('[data-page-body="beta"]').classList.contains("is-active") || !$('[data-panel="bugs"]').classList.contains("is-active")) return;
    const files = [...(event.clipboardData?.files || [])].filter((file) => file.type.startsWith("image/"));
    if (!files.length || !activeBug) return;
    event.preventDefault();
    void activeBug.addShots(files);
  });

  // Both tabs send one report each (kind "beta-tests" or "beta-bugs"), always with the logs.
  async function sendBeta(kind, button, fields, shots) {
    const uid = status.character?.uid;
    if (!uid) { toast("aLogs hasn't seen your game UID yet: open Aniimo with the meter running, then try again.", true); return null; }
    if (!get("leaderboard.token")) { toast("Sign in with Discord first: the Beta Menu sends as you.", true); return null; }
    const label = button.textContent;
    button.disabled = true;
    button.textContent = "Sending…";
    try {
      const zip = Uint8Array.from(atob(await invoke("collect_logs")), (c) => c.charCodeAt(0));
      const version = await window.__TAURI__.app.getVersion();
      const rc = await invoke("build_number").catch(() => 0);
      const form = new FormData();
      form.set("kind", kind);
      form.set("uid", uid);
      form.set("app_version", rc ? `${version}+rc.${rc}` : version);
      // the checklist's build for its tests; bugs are this aLogs' own ("0.1.33", "0.1.33 B2"): the shared list's
      // build stayed "0.1.28-RC43" for bugs sent from 0.1.31
      const running = `${version} ${await invoke("build_label").catch(() => "")}`.trim();
      form.set("build", kind === "beta-tests" ? betaTests.build : running);
      for (const [key, value] of Object.entries(fields)) form.set(key, JSON.stringify(value));
      form.set("logs", new Blob([zip], { type: "application/zip" }), "logs.zip");
      for (const [name, blob] of shots) form.append("screenshot", blob, name);
      const response = await fetch(`${api()}/logs/beta`, { method: "POST", headers: authHeaders(), body: form });
      const result = await response.json().catch(() => ({}));
      if (!response.ok) throw new Error(result.error || `The server answered ${response.status}.`);
      return result;
    } catch (error) {
      toast(`Couldn't send: ${error.message || error}`, true);
      return null;
    } finally {
      button.disabled = false;
      button.textContent = label;
    }
  }
  function showSent(title, lines, reportId) {
    $("[data-beta-sent-title]").textContent = title;
    $("[data-beta-sent-list]").replaceChildren(...lines.map((line) => el("li", "", { textContent: line })));
    $("[data-beta-sent-id]").textContent = reportId ? `Report ${reportId}, with your logs.` : "With your logs.";
    $("[data-beta-sent]").hidden = false;
  }
  $("[data-beta-sent-close]").addEventListener("click", () => { $("[data-beta-sent]").hidden = true; });
  $("[data-beta-tests-send]").addEventListener("click", async (event) => {
    const ticked = activeTests().filter((item) => beta.tests[item.id]?.tested);
    const missing = ticked.filter((item) => !beta.tests[item.id].result);
    $$("[data-beta-tests] .beta-card").forEach((card) => {
      const bad = missing.some((item) => item.id === card.dataset.id);
      card.classList.toggle("needs-result", bad);
      $(".beta-need", card).hidden = !bad;
    });
    if (missing.length) {
      $(`[data-beta-tests] .beta-card[data-id="${missing[0].id}"]`).scrollIntoView({ behavior: "smooth", block: "center" });
      toast(`Choose Working or Not working for ${missing.length === 1 ? "the test" : `the ${missing.length} tests`} you ticked.`, true);
      return;
    }
    const tests = activeTests().map((item) => {
      const s = beta.tests[item.id] || {};
      return { id: item.id, title: item.title, tested: Boolean(s.tested), working: s.tested ? s.result === "working" : null, note: (s.note || "").trim() };
    });
    const result = await sendBeta("beta-tests", event.currentTarget, { tests }, []);
    if (!result) return;
    const broken = ticked.filter((item) => beta.tests[item.id].result === "broken");
    showSent("Test results sent. Thanks!", [
      `${ticked.length} tested: ${ticked.length - broken.length} working, ${broken.length} not working`,
      ...broken.map((item) => `Not working: ${item.title}`),
      `${activeTests().length - ticked.length} not in this send (they stay here with your notes)`,
    ], result.reportId);
    const now = Math.floor(Date.now() / 1000);                // sent: remembered for the filter, the rest kept
    for (const item of ticked) beta.tests[item.id] = { tested: false, result: "", note: "", sent: beta.tests[item.id].result, sentAt: now };
    renderTests();
    saveBeta();
  });
  $("[data-bugs-send]").addEventListener("click", async (event) => {
    const filled = beta.bugs.filter(bugFilled);
    const cards = $$("[data-bugs] .bug-card");
    const incomplete = filled.filter((bug) => !bug.title.trim() || !bug.text.trim());
    beta.bugs.forEach((bug, i) => {
      const bad = incomplete.includes(bug);
      cards[i].classList.toggle("needs-result", bad);
      $(".beta-need", cards[i]).hidden = !bad;
    });
    if (incomplete.length) {
      cards[beta.bugs.indexOf(incomplete[0])].scrollIntoView({ behavior: "smooth", block: "center" });
      toast("Every bug needs a title and what happened.", true);
      return;
    }
    const shots = [];
    const bugs = filled.map((bug, i) => {
      const mine = bugShots.get(bug.key) || [];
      mine.forEach((shot, k) => shots.push([`bug-${i + 1}-${k + 1}`, shot.blob]));
      return { title: bug.title.trim(), area: bug.area || null, severity: bug.severity || null, text: bug.text.trim(), screenshots: mine.length };
    });
    const result = await sendBeta("beta-bugs", event.currentTarget, { bugs }, shots);
    if (!result) return;
    showSent(bugs.length === 1 ? "Bug report sent. Thanks!" : `${bugs.length} bug reports sent. Thanks!`,
      bugs.map((bug) => `${bug.title}${bug.screenshots ? ` (${bug.screenshots === 1 ? "1 screenshot" : `${bug.screenshots} screenshots`})` : ""}`),
      result.reportId);
    for (const list of bugShots.values()) for (const shot of list) URL.revokeObjectURL(shot.url);
    bugShots.clear();
    beta.bugs = [newBug()];
    renderBugs();
    saveBeta();
  });
  void loadBeta();

  $("[data-dispute-send]").addEventListener("click", async () => {
    try {
      await invoke("send_character_dispute", { note: $("[data-dispute-note]").value.trim() });
      toast("Dispute saved: it's sent with your game-menu proof within a few minutes.");
    } catch (error) {
      toast(String(error), true);
    }
  });
  $("[data-hidhide-setup]").addEventListener("click", async () => {
    try {
      const found = await invoke("setup_controller_blocking");
      void showPads();
      toast(found ? `Set up: ${found === 1 ? "1 controller device" : `${found} controller devices`} will be blocked. If it doesn't block yet, unplug and replug the controller once.`
        : "Controller blocking is installed, but no controller is connected. Connect it and press the button again.", !found);
    } catch (error) {
      toast(String(error), true);
    }
  });

  $("[data-hidhide-test]").addEventListener("click", async (event) => {
    const button = event.currentTarget; const out = $("[data-hidhide-result]");
    button.disabled = true; out.hidden = false; out.dataset.from = "test"; out.textContent = "Testing: your controller disappears for a second…";
    try {
      out.textContent = (await invoke("test_controller_blocking")).join(" · ");
    } catch (error) {
      out.textContent = String(error);
    }
    button.disabled = false; void showPads();
  });
  // What controller blocking sees: each connected controller by name and whether it is on the block list
  async function showPads() {
    let info;
    try { info = await invoke("controller_status"); } catch { return; }
    const list = $("[data-pads]");
    const rows = (info.controllers || []).map((pad) => {
      const li = document.createElement("li");
      // lights up while you press a button on this controller, so you can tell which one it is
      const light = document.createElement("span"); light.className = "pad-light" + (pad.pressedMsAgo != null && pad.pressedMsAgo < 700 ? " on" : "");
      light.title = "Lights up while you press a button on this controller";
      const name = document.createElement("strong"); name.textContent = pad.name;
      const kind = document.createElement("span"); kind.className = "tag"; kind.textContent = pad.xinput ? "XInput" : "DirectInput";
      const state = document.createElement("span");
      const ready = info.installed && info.filters && pad.attached && pad.blocked;
      state.className = "tag " + (!info.installed ? "" : ready ? "ok" : "warn");
      state.textContent = !info.installed ? "detected"
        : !info.filters ? "detected · driver not attached: press Set up"
        : !pad.hidAttached ? "detected · turn it off and on (or press Set up)"
        : !pad.attached ? "detected · XInput side not attached yet: re-apply it in reWASD, turn it off and on, or restart Windows"
        : pad.blocked ? "detected · ready to block" : "detected · not set up yet";
      li.append(light, name, kind, state);
      if (info.installed && info.filters) {
        // its own Block switch: with several controllers, block only the ones you play with
        const toggle = document.createElement("label"); toggle.className = "pad-block";
        const box = document.createElement("input"); box.type = "checkbox"; box.checked = Boolean(pad.blocked);
        box.addEventListener("change", async () => {
          try { await invoke("set_controller_blocked", { id: pad.id, on: box.checked }); } catch (error) { toast(String(error), true); }
          void showPads();
        });
        toggle.append(box, document.createTextNode(" Block"));
        li.append(toggle);
      }
      return li;
    });
    if (!rows.length) {
      const li = document.createElement("li"); li.className = "none";
      li.textContent = "No controller connected. Plug it in (or turn it on) and it shows up here.";
      rows.push(li);
    }
    list.replaceChildren(...rows);
    const pads = info.controllers || [];
    const testable = info.installed && pads.some((pad) => pad.blocked && pad.attached);
    const test = $("[data-hidhide-test]");
    test.disabled = !testable;
    // a greyed-out button says why
    test.title = testable ? "Hides your controller for a second to prove the game would lose it"
      : !info.installed || !info.filters ? "Press Set up controller blocking first"
      : !pads.length ? "Connect your controller first"
      : pads.some((pad) => !pad.attached) ? "Turn the controller off and on (re-apply it in reWASD for a virtual one), or restart Windows once"
      : "Press Set up controller blocking first";
    const why = $("[data-hidhide-result]");
    if (!testable && why.dataset.from !== "test") { why.hidden = false; why.textContent = "Test blocking: " + test.title.charAt(0).toLowerCase() + test.title.slice(1) + "."; why.dataset.from = "hint"; }
    if (testable && why.dataset.from === "hint") { why.hidden = true; why.dataset.from = ""; }
    if (info.hiding) $("[data-hidhide-state]").textContent = "Your controller is hidden from the game right now (the meter is holding inputs).";
  }
  // refreshed while Leaderboards › Automation is open (often, so the "which one is it" light follows your presses)
  setInterval(() => { if ($('[data-hidhide-test]').offsetParent) void showPads(); }, 400);

  // ---------------------------------------------------------------- status
  const n = (value) => (value === null || value === undefined ? "—" : Number(value).toLocaleString());
  function text(key, value) {
    for (const node of $$(`[data-s="${key}"]`)) node.textContent = value;
  }
  let siteUid = null;
  function render() {
    if ((status.character?.uid || null) !== siteUid) {               // the game switched characters: their checklists
      siteUid = status.character?.uid || null;
      if ($('[data-page-body="site"]').classList.contains("is-active")) placeSite();
    }
    const running = Boolean(status.running);
    $("[data-meter-dot]").classList.toggle("on", running);
    // the meter always runs while aLogs is open; a gap only means it is (re)starting
    const meterOn = captureOn;                               // the button over the game is the meter's switch
    $("[data-meter-state]").textContent = !meterOn ? "Meter off" : running ? "Meter running" : "Meter starting…";
    const fresh = running && status.time && Date.now() / 1000 - status.time < 5;
    text("game", !meterOn ? "Meter off (the button over the game, or DPS Meter › Meter)" : !running ? "Meter starting…" : !fresh ? "Starting…"
      : status.game_window ? "Game window found" : "Waiting for the game");
    text("capture", fresh && status.frame ? `${status.frame[0]}×${status.frame[1]} via ${status.capture}` : "");
    text("bar", fresh ? (status.boss_bar ? "Boss bar on screen" : "No boss bar") : "—");
    const line = fresh ? String(status.status || "").replace("...", "…") : "";
    text("status", line.charAt(0).toUpperCase() + line.slice(1));   // the meter writes some lines lowercase
    const fight = fresh ? status.fight : null;
    // boss_of: a two-boss middle fight's boss on the bar, [1, 2] or [2, 2] (the meter's write_status); its damage and
    // max HP are then the run's, both bosses' so far
    const of = Array.isArray(fight?.boss_of) ? ` · boss ${fight.boss_of[0]}/${fight.boss_of[1]}` : "";
    text("fight", fight ? `${fight.boss} Lv ${fight.level}${of}${fight.segment ? ` · x${fight.segment}` : ""} — ${n(fight.avg_dps)} DPS` : "No fight in progress.");
    text("fight-detail", fight ? `Team damage ${n(fight.damage)} of ${n(fight.max_hp)} HP · ${fight.seconds}s · ${n(fight.recent_dps)} DPS over the last 5 s` : "");
    const character = status.character || {};
    const who = character.uid ? `${character.name || "Name appears once you open the game menu (Esc) or your Profile"} · UID ${character.uid}` : "Waiting for the game log…";
    text("character", who);
    $("[data-logs-uid]").textContent = character.uid ? `Sent as UID ${character.uid}.` : "Waiting for your game UID: open Aniimo with the meter running.";
    text("character-proof", !character.uid ? "" : character.verified
      ? "Proven: read from the game, next to your UID."
      : "Not proven yet: open the game menu (Esc or Start) or your Profile once while aLogs is running.");
    renderLink(fresh ? character.link : null);
    const dispute = character.dispute;
    $("[data-dispute]").hidden = !dispute;
    if (dispute) $("[data-dispute-text]").textContent = `Another AniiLogs account already proved UID ${dispute.gameUid} as its character. `
      + `If "${dispute.name}" is yours, send a dispute: a developer compares both game-menu proofs and moves the character to the right account.`;
    const lb = status.leaderboard || {};
    text("lb", !get("leaderboard.submit") ? "Off." : !get("leaderboard.token") ? "On, but not signed in: use the Discord button in the sidebar." : lb.on ? "On" : "On (starts with the meter).");
    text("lb-detail", lb.on ? `· ${lb.pending || 0} waiting to send${lb.last ? ` · last: ${lb.last[1]}` : ""}` : "");
    text("data-dir", status.data_dir || "…");
    $("[data-hidhide-state]").textContent = status.hidhide_installed
      ? "Controller blocking is installed. Press the setup button again whenever you use a different controller."
      : "Setting it up installs a small driver (Windows asks for admin approval once). Without it, only the keyboard and mouse can be blocked.";
    $('[data-k="results.hold_controller"]').disabled = !status.hidhide_installed || !get("leaderboard.submit") || !get("leaderboard.automation");
    // Windows 10 can't capture the game window without a yellow border: About explains what that means for streams
    document.querySelectorAll("[data-win10-capture]").forEach((el) => { el.hidden = status.borderless_capture !== false; });
    renderOcr();
  }
  // Windows' OCR in English: the meter reads the game's text with it (boss names and levels, result screens, Battle
  // Data, the prep and Party screens). Without it (status.json's ocr.english false: no OCR language, another one, or
  // the OCR helper didn't start) runs aren't read, and before only meter.log said so. The Dashboard and the DPS Meter
  // page show a warning then, never with English OCR (true) nor when the helper doesn't say (null), with a button that
  // installs it (install_english_ocr: Windows' admin prompt, then minutes of installing; the meter then restarts to
  // pick it up, and the warning goes once the restarted meter's status says English).
  let ocrJob = null;   // the install: {state: "running", since} / "declined" / "failed" (message) / "installed" (restart, at)
  const OCR_NOT_READ = "Result screens, boss names and levels, Battle Data and the battle prep and Party screens";
  const OCR_SETTINGS = "Windows Settings › Time & language › Language (Language & region on Windows 11) › Add a language › English (United States)";
  const languageName = (tag) => {
    try { return new Intl.DisplayNames(["en"], { type: "language" }).of(tag) || tag; } catch { return tag; }
  };
  function renderOcr() {
    const ocr = status.ocr || {};
    // only from a status the running meter wrote (fresh, as render()'s): status.json stays on disk between sessions,
    // so after English was added and Windows restarted, the last session's "english": false showed the warning, with
    // an install button that worked, until the new meter's first write
    const fresh = Number(status.time) > Date.now() / 1000 - 5;
    const missing = Boolean(status.running) && fresh && ocr.english === false;
    const running = ocrJob?.state === "running";
    // installed: the old meter's status until the restarted one writes its own (a minute at most: a meter switched off
    // meanwhile never does)
    const restarting = ocrJob?.state === "installed" && !(Number(status.time) > ocrJob.at) && Date.now() / 1000 - ocrJob.at < 60;
    if (ocrJob?.state === "installed" && !restarting && !missing) ocrJob = null;      // English now: done
    const shown = missing || running || restarting;
    for (const box of $$("[data-ocr-warn]")) box.hidden = !shown;
    if (!shown) return;
    const noHelper = ocr.started === false;
    // switching (status.json's ocr.switching): the meter found English OCR on the PC while it ran, but one of its OCR
    // helpers hasn't restarted with it yet (it tries again every minute). Before, this said the PC has no OCR language
    // (or reads in another one) and offered to install English OCR, with an admin prompt, though it's installed
    const switching = !noHelper && ocr.switching === true;
    const other = !noHelper && !switching && ocr.language ? languageName(ocr.language) : "";      // "Korean (South Korea)"
    for (const node of $$("[data-ocr-title]")) node.textContent = noHelper
      ? "aLogs' text reader (OCR) didn't start, so the meter can't read the game's screens."
      : switching ? "English OCR is installed on this PC, but the meter hasn't switched all of its text reading to it yet."
      : other ? `Windows reads text (OCR) in ${other} on this PC, not English, so the meter can't read the game's screens reliably.`
      : "Windows has no text recognition (OCR) language on this PC, so the meter can't read the game's screens.";
    for (const node of $$("[data-ocr-what]")) node.textContent = switching
      ? "Until it has, some of the game's screens may not be read, so a run may not be recorded or sent to the leaderboards. The meter tries again every minute."
      : other ? `${OCR_NOT_READ} are misread or not read, so your runs may not be recorded or sent to the leaderboards. The game's text is English: installing Windows' English OCR fixes it.`
      : `${OCR_NOT_READ} can't be read, so your runs aren't recorded or sent to the leaderboards.${noHelper ? "" : " Installing Windows' English OCR fixes it."}`;
    const seconds = running ? Math.floor((Date.now() - ocrJob.since) / 1000) : 0;
    const took = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
    const state = running ? `Installing English OCR… ${took}. Approve Windows' admin prompt; installing can then take several minutes. aLogs keeps running meanwhile.`
      : restarting ? "English OCR is installed. The meter is restarting to use it…"
      : switching ? ""                                          // the title says it's installed
      : ocrJob?.state === "installed" ? (ocrJob.restart ? "English OCR is installed: restart Windows to finish it, then open aLogs again."
        : "English OCR is installed, but the meter doesn't see it yet: restart Windows, then open aLogs again.")
      : ocrJob?.message || "";
    for (const node of $$("[data-ocr-state]")) {
      node.hidden = !state; node.textContent = state;
      node.className = ocrJob?.state === "failed" || ocrJob?.state === "declined" ? "note-warn" : "";
    }
    for (const node of $$("[data-ocr-progress]")) node.hidden = !running && !restarting;
    for (const button of $$("[data-ocr-install]")) {
      // installing OCR can't start a helper that won't start; switching: it's installed
      button.closest(".row").hidden = noHelper || switching;
      button.disabled = running || restarting;
      button.textContent = running ? "Installing…" : ocrJob?.state === "failed" || ocrJob?.state === "declined" ? "Try again" : "Install English OCR";
    }
    // Settings' page is Language on Windows 10, Language & region on 11; adding English (United States) there
    // installs its OCR with it (a required language feature)
    for (const node of $$("[data-ocr-manual]")) node.textContent = noHelper
      ? "Quit aLogs (tray icon › Quit) and open it again. If this stays, reinstall aLogs, or press Report a problem on the Dashboard."
      : switching ? "If this stays for more than a few minutes, quit aLogs (tray icon › Quit) and open it again."
      : ocrJob?.state === "failed" ? `You can add it yourself: ${OCR_SETTINGS}. Windows adds its text recognition (OCR) with it.`
      : `Windows asks for admin approval to install it. Or add it yourself: ${OCR_SETTINGS}.`;
  }
  async function installOcr() {
    if (ocrJob?.state === "running") return;
    ocrJob = { state: "running", since: Date.now() };
    renderOcr();
    const tick = setInterval(renderOcr, 1000);                  // the time it's taken so far; the window stays free
    let outcome;
    try { outcome = await invoke("install_english_ocr"); }
    catch (error) { outcome = { state: "failed", message: `Windows couldn't install English OCR (${error}).` }; }
    clearInterval(tick);
    ocrJob = outcome?.state === "installed" ? { ...outcome, at: Date.now() / 1000 } : outcome || { state: "failed", message: "" };
    renderOcr();
    void refresh();
  }
  for (const button of $$("[data-ocr-install]")) button.addEventListener("click", installOcr);
  // Link character: the meter reads your Profile screen (three reads that agree) and you confirm what it read.
  let linkShown = false;      // a request from this window, so a finished one shows "Linked" once and then clears
  function renderLink(link) {
    const box = $("[data-link]");
    box.hidden = !linkShown || !link;
    $("[data-link-start]").disabled = Boolean(linkShown && link && link.state !== "done" && link.state !== "timeout");
    if (box.hidden) return;
    const found = link.state === "found";
    $("[data-link-text]").textContent = link.state === "waiting"
      ? "Open your Profile in Aniimo (Esc › Profile) and leave it on screen for a few seconds."
      : found ? `Read from your Profile: “${link.name}” with UID ${link.uid}. Link this name to your character?`
      : link.state === "done" ? `Linked: ${link.name} is your character's name now.`
      : "Your Profile didn't show up within 2 minutes.";
    $("[data-link-confirm]").hidden = !found;
    $("[data-link-retry]").hidden = link.state !== "timeout";
    $("[data-link-cancel]").textContent = link.state === "done" ? "Close" : "Cancel";
  }
  async function linkAction(action, extra = {}) {
    try {
      await invoke("character_link", { action, uid: null, name: null, ...extra });
    } catch (error) {
      toast(`Couldn't link your character: ${error}`, true);
    }
  }
  $("[data-link-start]").addEventListener("click", async () => { linkShown = true; await linkAction("request"); refresh(); });
  $("[data-link-retry]").addEventListener("click", async () => { await linkAction("request"); refresh(); });
  $("[data-link-confirm]").addEventListener("click", async () => {
    const link = status.character?.link;
    if (link?.state === "found") await linkAction("confirm", { uid: link.uid, name: link.name });
    refresh();
  });
  $("[data-link-cancel]").addEventListener("click", async () => { linkShown = false; await linkAction("cancel"); refresh(); });

  async function refresh() {
    try {
      status = await invoke("get_status");
    } catch {
      status = {};
    }
    render();
    void pluginStatuses();
  }

  // ---------------------------------------------------------------- first run
  // ---------------------------------------------------------------- setup guide
  // Shown when aLogs starts while General Settings › "Show the setup guide" is on (new installs and after this
  // update); finishing or skipping it turns that off. Its switches are the same settings as the pages behind them.
  const guide = $("[data-guide]");
  const guideSteps = $$("[data-step]", guide);
  let guideAt = 1;
  function guideShow(step) {
    guideAt = Math.max(1, Math.min(guideSteps.length, step));
    for (const el of guideSteps) el.hidden = Number(el.dataset.step) !== guideAt;
    $("[data-guide-dots]").replaceChildren(...guideSteps.map((_, i) => {
      const dot = document.createElement("li"); dot.classList.toggle("on", i + 1 === guideAt); return dot;
    }));
    $("[data-guide-back]").hidden = guideAt === 1;
    $("[data-guide-next]").textContent = guideAt === guideSteps.length ? "Done" : "Next";
    $("[data-guide-skip]").hidden = guideAt === guideSteps.length;
    for (const box of $$("[data-guide-k]", guide)) box.checked = Boolean(get(box.dataset.guideK));
    guideAccount();
  }
  function guideAccount() {
    const token = get("leaderboard.token");
    $("[data-guide-account]").textContent = token ? ($("[data-account-state]")?.textContent || "Signed in.") : "Not signed in.";
    $("[data-guide-sign-in]").hidden = Boolean(token);
  }
  function openGuide() { guide.hidden = false; guideShow(1); }
  async function closeGuide() {
    guide.hidden = true;
    set("app.show_guide", false);
    set("app.welcome_done", true);
    fill();
    try { await saveChanges(); } catch (error) { toast(String(error), true); }
    refresh();
  }
  for (const box of $$("[data-guide-k]", guide)) {
    box.addEventListener("change", () => { set(box.dataset.guideK, box.checked); fill(); scheduleSave(); });
  }
  $("[data-guide-next]").addEventListener("click", () => (guideAt === guideSteps.length ? closeGuide() : guideShow(guideAt + 1)));
  $("[data-guide-back]").addEventListener("click", () => guideShow(guideAt - 1));
  $("[data-guide-skip]").addEventListener("click", closeGuide);
  $("[data-guide-open]").addEventListener("click", openGuide);
  // the account step uses the same sign-in as Leaderboards › Account, and follows it
  $("[data-guide-sign-in]").addEventListener("click", (event) => signIn(event.currentTarget));
  new MutationObserver(() => { if (!guide.hidden) guideAccount(); }).observe($("[data-account-state]"), { childList: true, characterData: true, subtree: true });

  // About: the last five updates' notes, from the GitHub Releases (loadReleases); the copy built into the app
  // (ui/patch-notes.js, from CHANGELOG.md) shows until they load, or when GitHub can't be reached. **bold** is the only markup.
  function renderPatchNotes(current, releases = window.PATCH_NOTES || []) {
    const box = $("[data-patch-notes]");
    const rich = (text) => {
      const node = document.createElement("span");
      text.split(/\*\*(.+?)\*\*/u).forEach((part, i) => {
        node.append(i % 2 ? Object.assign(document.createElement("strong"), { textContent: part }) : part);
      });
      return node;
    };
    const shown = releases.slice(0, 5);
    // A beta: its version isn't released yet, so GitHub's list doesn't have it; its notes so far (built into the app)
    // go on top
    if (current && !releases.some((release) => release.version === current) && releases.every((release) => newer(current, release.version))) {
      const own = (window.PATCH_NOTES || []).find((release) => release.version === current);
      if (own) shown.unshift({ ...own, beta: true });
    }
    // A 6th slot when the installed version is older than all five: how far behind it is, with its own notes if known.
    const behind = current ? releases.filter((release) => newer(release.version, current)).length : 0;
    if (current && behind && !shown.some((release) => release.version === current)) {
      const own = releases.find((release) => release.version === current) || (window.PATCH_NOTES || []).find((release) => release.version === current);
      shown.push({ version: current, outdated: behind, intro: own?.intro || "", sections: own?.sections || [] });
    }
    box.replaceChildren(...shown.map((release) => {
      if (release.outdated) {
        const item = document.createElement("details");
        item.className = "note is-outdated";
        const plural = release.outdated === 1 ? "" : "s";
        item.append(Object.assign(document.createElement("summary"), { textContent: `${release.version} (installed, ${release.outdated} update${plural} behind)` }),
          Object.assign(document.createElement("p"), { className: "note-warn",
            textContent: `Your version is very out of date: ${release.outdated} update${plural} ${release.outdated === 1 ? "has" : "have"} come out since. Update to get everything listed above.` }));
        if (release.intro) item.append(Object.assign(document.createElement("p"), { textContent: release.intro }));
        for (const section of release.sections) {
          item.append(Object.assign(document.createElement("h3"), { textContent: section.heading }));
          const list = document.createElement("ul");
          for (const text of section.items) {
            const li = document.createElement("li");
            li.append(rich(text));
            list.append(li);
          }
          item.append(list);
        }
        return item;
      }
      const isNew = Boolean(current) && newer(release.version, current);
      const item = document.createElement("details");
      item.className = `note${isNew ? " is-new" : ""}`;
      const summary = document.createElement("summary");
      summary.textContent = release.beta ? `${release.version} beta (installed; these notes grow until it's released)`
        : release.version === current ? `${release.version} (installed)`
        : isNew ? `${release.version} (new, not installed yet)` : release.version;
      item.append(summary);
      if (release.intro) item.append(Object.assign(document.createElement("p"), { textContent: release.intro }));
      for (const section of release.sections) {
        item.append(Object.assign(document.createElement("h3"), { textContent: section.heading }));
        const list = document.createElement("ul");
        for (const text of section.items) {
          const li = document.createElement("li");
          li.append(rich(text));
          list.append(li);
        }
        item.append(list);
      }
      return item;
    }));
  }

  // ---------------------------------------------------------------- updates
  // The GitHub Releases (the newest is what the Companion page's Download button gives): the last five fill Patch notes,
  // and when the newest is newer than this install there is an update button next to Patch notes and a dot on About.
  const RELEASES_API = "https://api.github.com/repos/AniiLogs/aniilogs.github.io/releases?per_page=20";
  const newer = (a, b) => {
    const [x, y] = [a, b].map((v) => String(v).split(".").map((part) => Number.parseInt(part, 10) || 0));
    for (let i = 0; i < 3; i += 1) if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) > (y[i] || 0);
    return false;
  };
  // A release's notes (CHANGELOG.md format) as {intro, sections}; the Install part is for the website only.
  function parseNotes(body) {
    const notes = { intro: "", sections: [] };
    let section = null;
    for (const line of String(body || "").replace(/\r\n/gu, "\n").split("\n")) {
      if (/^## /u.test(line)) break;
      const heading = /^### (.+)$/u.exec(line);
      if (heading) { section = { heading: heading[1].trim(), items: [] }; notes.sections.push(section); }
      else if (line.startsWith("- ") && section) section.items.push(line.slice(2).trim());
      else if (line.trim() && !section) notes.intro = `${notes.intro} ${line.trim()}`.trim();
    }
    return notes;
  }
  async function loadReleases(current) {
    try {
      const response = await fetch(RELEASES_API, { headers: { accept: "application/vnd.github+json" } });
      if (!response.ok) return;
      const releases = (await response.json())
        .filter((release) => !release.draft && !release.prerelease && /^companion-v\d+\.\d+\.\d+$/u.test(release.tag_name || ""))
        .map((release) => ({ version: release.tag_name.slice("companion-v".length), url: release.html_url, ...parseNotes(release.body) }))
        .sort((a, b) => (newer(a.version, b.version) ? -1 : newer(b.version, a.version) ? 1 : 0));
      if (!releases.length) return;
      renderPatchNotes(current, releases);                // installing an update is the sidebar's job
    } catch {
      // offline or GitHub unreachable: try again at the next check
    }
  }

  // Self-update (update.rs): the app downloads a newer build in the background; then the sidebar button and About's
  // update button restart aLogs into it. Before that, About's button still links to the download page.
  function showUpdate(info) {
    if (!info) return;
    const label = `aLogs ${info.version}${info.beta ? ` B${info.beta}` : info.rc ? ` (build ${info.rc})` : ""}`;
    const opened = !$("[data-update-box]").hidden;
    $("[data-update-pill]").hidden = opened;
    $("[data-update-pill]").textContent = info.next_start ? `${label} · installs next start` : `Update available · ${label}`;
    $("[data-update-progress]").hidden = true;
    $("[data-update-title]").textContent = info.next_start ? `${label} installs next time aLogs opens`
      : info.downloaded ? `${label} is ready to install` : `${label} is available`;
    $("[data-install-later]").hidden = Boolean(info.next_start);
    for (const button of $$("[data-update-box] button")) button.disabled = false;
    $("[data-update-note]").textContent = $("[data-update-title]").textContent + ".";
  }
  function showProgress(p) {                         // the download, from update.rs (update-progress)
    if ($("[data-update-box]").hidden) {             // a download on its own (automatic): on the button
      $("[data-update-pill]").hidden = false;
      $("[data-update-pill]").textContent = p.percent >= 0 ? `Downloading an update · ${p.percent}%` : "Downloading an update…";
      return;
    }
    $("[data-update-progress]").hidden = false;
    $("[data-update-bar]").style.width = `${Math.max(2, p.percent)}%`;
    $("[data-update-title]").textContent = p.percent >= 0 ? `Downloading the update · ${p.percent}%` : "Downloading the update…";
  }
  async function installUpdate() {
    for (const button of $$("[data-update-box] button")) button.disabled = true;
    try {
      await invoke("install_update");                  // downloads if needed; then aLogs closes and the installer runs
    } catch (error) {
      toast(String(error), true);
      for (const button of $$("[data-update-box] button")) button.disabled = false;
    }
  }
  $("[data-install-update]").addEventListener("click", installUpdate);
  $("[data-update-pill]").addEventListener("click", () => {
    $("[data-update-pill]").hidden = true;
    $("[data-update-box]").hidden = false;
  });
  $("[data-install-later]").addEventListener("click", async () => {
    for (const button of $$("[data-update-box] button")) button.disabled = true;
    try {
      showUpdate(await invoke("install_update_later"));
      toast("The update installs next time aLogs opens");
    } catch (error) {
      toast(String(error), true);
      for (const button of $$("[data-update-box] button")) button.disabled = false;
    }
  });
  $("[data-check-update]").addEventListener("click", async () => {
    const note = $("[data-update-note]");
    note.textContent = "Checking…";
    try {
      const info = await invoke("check_update");
      if (info) { showUpdate(info); $("[data-update-pill]").click(); }      // asked for: straight to the choice
      else note.textContent = "aLogs is up to date.";
    } catch (error) {
      note.textContent = `Couldn't check: ${error}`;
    }
  });

  async function start() {
    let version = "";
    try {
      version = await window.__TAURI__.app.getVersion();
      const build = await invoke("build_label").catch(() => "");
      $("[data-app-version]").textContent = build ? `${version} · ${build}` : version;
    } catch {
      // version is cosmetic
    }
    renderPatchNotes(version);
    void loadReleases(version);
    // About's Sanctum Helper line: its version and map data's game build (it has no About of its own)
    $('[data-page="about"]').addEventListener("click", async () => {
      try {
        const info = (await invoke("sanctum_state"))?.info || {};
        const line = $("[data-sanctum-credit]");
        line.textContent = line.textContent.replace(/^Includes Sanctum Helper( v[\d.]+)? by sargeeva(, map data from game build \S+)?/u,
          `Includes Sanctum Helper${info.version ? ` v${info.version}` : ""} by sargeeva${/\d/u.test(info.gameDataVersion || "") ? `, map data from game build ${info.gameDataVersion}` : ""}`);
      } catch {
        // Sanctum Helper not installed or not started: the plain credit stays
      }
    });
    setInterval(() => loadReleases(version), 6 * 3600 * 1000);
    try {
      const stored = await invoke("get_settings");
      migrateStored(stored);
      const overlay = stored?.overlay;                    // settings from before 0.1.28: hide_from_capture, the opposite
      if (overlay && overlay.in_recordings === undefined && overlay.hide_from_capture !== undefined) overlay.in_recordings = !overlay.hide_from_capture;
      settings = merge(DEFAULTS, stored);
      showSanctumSwitch(settings.sanctum?.enabled ?? true);
      if (!settings.app.priority) settings.app.priority = "low";   // "Not set" before B8: lower is the default now
      if (settings.overlay.world_position === "quests") settings.overlay.world_position = "right";   // B9: under the quests is gone
      // the fight rate's default was 60 before B6, now 30: a rate nobody picked is 30
      if (settings.capture.fight_rate !== 30 && !(settings.user_set || []).includes("capture.fight_rate")) settings.capture.fight_rate = 30;
    } catch {
      settings = structuredClone(DEFAULTS);
    }
    fill();
    await loadPlugins();
    await refresh();
    void syncAccountPrefs();
    try {
      showUpdate(await invoke("update_state"));
      await window.__TAURI__.event.listen("update-ready", (event) => showUpdate(event.payload));
      await window.__TAURI__.event.listen("update-progress", (event) => showProgress(event.payload));
    } catch {
      // older app without the updater
    }
    void captureSwitch().then(async () => {
      if (get("meter.enabled") === false) {               // "Run the DPS meter" was its own setting before B7
        set("meter.enabled", true);
        await saveChanges();
        await setSwitch({ captureOn: false });
      }
    });
    if (settings.app.show_guide !== false) openGuide();
    // once a second while the window is up; nothing while it sits in the tray, and fresh the moment it's shown
    setInterval(() => { if (!document.hidden) void refresh(); }, 1000);
    document.addEventListener("visibilitychange", () => { if (!document.hidden) void refresh(); });
  }
  start();
})();
