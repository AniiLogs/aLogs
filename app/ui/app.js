(() => {
  const invoke = window.__TAURI__.core.invoke;
  // Same defaults as bossbar/settings.py (the meter fills in anything missing the same way).
  const DEFAULTS = {
    app: { welcome_done: false, show_guide: true, start_with_windows: false, close_to_tray: true, auto_download: true, beta_updates: false,
      photos_auto: false, photos_auto_since: 0 },
    meter: { enabled: true },
    audio_cues: { boss_kill: true },
    game: { window_title: "Aniimo", player_log: "" },
    overlay: { show: true, dps_window: true, hp_labels: true, in_recordings: true, backdrop: 0, world_position: "map", rush_position: "corner", result_seconds: 10 },
    capture: { look_interval: 0.05, debug_frames: false, end_screens: false, nobar_frames: false, keep_hours: 0 },
    results: { auto_open_battle_data: true, hold_inputs: true, hold_controller: false, hold_seconds: 60, egg_fade_hold: false },
    leaderboard: { submit: false, api_url: "https://aniilogs-api.pages.dev/api", token: "", link_character: true, include_screenshots: true },
  };
  let settings = structuredClone(DEFAULTS);
  let status = {};

  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
  const get = (path) => path.split(".").reduce((value, key) => value?.[key], settings);
  const set = (path, value) => {
    const keys = path.split(".");
    const last = keys.pop();
    keys.reduce((value, key) => value[key], settings)[last] = value;
  };
  function merge(base, extra) {
    const out = structuredClone(base);
    for (const [key, value] of Object.entries(extra || {})) {
      if (out[key] && typeof out[key] === "object" && value && typeof value === "object") out[key] = merge(out[key], value);
      else if (key in out) out[key] = value;
    }
    return out;
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

  // ---------------------------------------------------------------- navigation
  $$("[data-page]").forEach((button) => button.addEventListener("click", () => {
    $$("[data-page]").forEach((b) => b.classList.toggle("is-active", b === button));
    $$("[data-page-body]").forEach((page) => page.classList.toggle("is-active", page.dataset.pageBody === button.dataset.page));
    placeSite();
  }));

  // ---------------------------------------------------------------- the AniiLogs website
  // The live site in a second webview (site.rs) laid exactly over the frame under the tabs, so it is always the same as
  // aniilogs.github.io. It is hidden whenever another page is open, and follows the frame when the window is resized.
  let sitePage = "/leaderboard/";
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
  // Tabs across the top of a page (DPS Meter, General Settings): each page switches only its own panels.
  function showTab(page, name) {
    const body = $(`[data-page-body="${page}"]`);
    $$("[data-tab]", body).forEach((tab) => tab.setAttribute("aria-selected", String(tab.dataset.tab === name)));
    $$("[data-panel]", body).forEach((panel) => panel.classList.toggle("is-active", panel.dataset.panel === name));
  }
  const showDpsTab = (name) => showTab("dps", name);
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
      accentText: "#1c756b", coral: "#b84c3b" },
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
    const vars = { "--bg": c.background, "--bg-2": c.backgroundAlt, "--panel": c.surface, "--panel-2": c.surfaceRaised, "--line": c.border,
      "--text": c.text, "--muted": c.muted, "--accent": c.primary, "--accent-2": c.secondary, "--gold": c.highlight,
      "--coral": preset ? preset.coral : "#f48972", "--accent-text": accentText, "--on-accent": readable(c.primary) };
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
  function showNavAccount(name) {
    $("[data-nav-sign-in]").hidden = Boolean(name);
    const nav = $("[data-nav-account]");
    nav.hidden = !name;
    nav.textContent = name ? `Signed in as ${name}` : "";
  }
  $("[data-nav-sign-in]").addEventListener("click", (event) => signIn(event.currentTarget));
  $("[data-nav-account]").addEventListener("click", () => { $('[data-page="dps"]').click(); showDpsTab("leaderboard"); });
  async function showAccount() {
    const token = get("leaderboard.token") || "";
    const state = $("[data-account-state]");
    if (!state || accountShown === token) return;
    accountShown = token;
    showNavAccount(null);
    $("[data-sign-out]").hidden = !token;
    if (!token) { state.textContent = "Not signed in."; return; }
    state.textContent = "Checking your account…";
    try {
      const response = await fetch(`${api()}/auth/app/account`, { headers: { authorization: `Bearer ${token}` } });
      const body = await response.json().catch(() => ({}));
      state.textContent = response.ok ? `Signed in as ${body.account?.name || "your Discord account"}.`
        : "This sign-in no longer works (signed out or revoked). Sign in again.";
      showNavAccount(response.ok ? body.account?.name || "Discord" : null);   // signed out: the sidebar offers sign-in
    } catch {
      state.textContent = "Signed in (couldn't reach AniiLogs to check right now).";
      showNavAccount("Discord");
    }
  }
  // Signing in starts from the sidebar (or the setup guide); the account tab shows the result and signs out.
  async function signIn(button) {
    const state = $("[data-account-state]");
    button.disabled = true;
    try {
      const verifier = b64url(crypto.getRandomValues(new Uint8Array(32)));
      const challenge = b64url(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)));
      const port = await invoke("auth_listen_start");
      await invoke("open_target", { target: `${api()}/auth/discord/start?app_port=${port}&app_challenge=${challenge}` });
      state.textContent = "Finish signing in in your browser, then come back here…";
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
      toast(String(error.message || error), true);
    }
    button.disabled = false;
  }
  $("[data-sign-out]").addEventListener("click", async () => {
    const token = get("leaderboard.token");
    if (token) {
      try { await fetch(`${api()}/auth/app/sign-out`, { method: "POST", headers: { authorization: `Bearer ${token}` } }); } catch { /* signed out here anyway */ }
    }
    set("leaderboard.token", ""); fill(); scheduleSave("Signed out");
  });

  let saveTimer = 0;
  function scheduleSave(message = "Saved") {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      try {
        await invoke("save_settings", { settings });
        if (message) toast(message);
      } catch (error) {
        toast(`Could not save: ${error}`, true);
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
      toast(`Couldn't change it: ${error.message || error}`, true);
    }
  });
  window.addEventListener("focus", () => void syncAccountPrefs());

  // ---------------------------------------------------------------- Photos
  // The game's photos (photos.rs finds them and lets this page show them) and which are on the profile already
  // (the API). Picked ones are made into WebP here, up to 2560 px wide, and uploaded with what the game recorded.
  const photos = { list: [], folder: null, uploaded: new Map(), selected: new Set(), busy: false, loaded: false };
  const convertFileSrc = window.__TAURI__.core.convertFileSrc;
  const authHeaders = () => ({ authorization: `Bearer ${get("leaderboard.token")}` });

  async function loadPhotos() {
    try {
      const found = await invoke("game_photos");
      photos.list = found.photos || [];
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
      : !photos.list.length ? "No photos yet. Take one in Aniimo (with Storage set to Local) and it shows up here."
      : !signedIn ? `${photos.list.length} photos. Sign in with the Discord button in the sidebar to put them on your profile.`
      : `${photos.list.length} photos · ${onProfile} on your profile. Click photos to pick them.`;
    const upload = $("[data-photos-upload]");
    if (!photos.busy) upload.textContent = photos.selected.size ? `Upload ${photos.selected.size} selected` : "Upload selected";
    upload.disabled = photos.busy || !signedIn || !photos.selected.size;
    const grid = $("[data-photos-grid]");
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

  // Upload new photos automatically: once a minute, photos taken since that was switched on go up on their own.
  setInterval(async () => {
    if (!get("app.photos_auto")) {
      if (get("app.photos_auto_since")) { set("app.photos_auto_since", 0); scheduleSave(""); }
      return;
    }
    if (!get("leaderboard.token") || photos.busy) return;
    if (!get("app.photos_auto_since")) { set("app.photos_auto_since", Math.floor(Date.now() / 1000)); scheduleSave(""); return; }
    await loadPhotos();
    const since = get("app.photos_auto_since");
    const fresh = photos.list.filter((p) => !photos.uploaded.has(p.id) && (p.takenAt || 0) >= since);
    if (!fresh.length) return;
    photos.busy = true;
    try {
      for (const photo of fresh) await uploadPhoto(photo);
      toast(fresh.length === 1 ? "New photo added to your profile" : `${fresh.length} new photos added to your profile`);
    } catch (error) {
      toast(`Couldn't upload a new photo: ${error.message || error}`, true);
    } finally {
      photos.busy = false;
      renderPhotos();
    }
  }, 60 * 1000);

  for (const input of $$("[data-k]")) {
    input.addEventListener("change", () => {
      let value = input.type === "checkbox" ? input.checked : input.value.trim();
      if (input.dataset.type === "number") value = Number(value);
      if (input.dataset.validate === "token" && value && !value.startsWith("alb_")) {
        tokenHint();
        $("[data-token-hint]").textContent = "That doesn't look like a desktop token (they start with alb_). Not saved.";
        return;
      }
      set(input.dataset.k, value);
      tokenHint();
      scheduleSave();
    });
  }
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
  $("[data-logs-send]").addEventListener("click", async () => {
    const button = $("[data-logs-send]");
    const uid = status.character?.uid;
    if (!uid) {
      toast("aLogs hasn't seen your game UID yet: open Aniimo with the meter running, then try again.", true);
      return;
    }
    button.disabled = true;
    button.textContent = "Sending…";
    try {
      const zip = Uint8Array.from(atob(await invoke("collect_logs")), (c) => c.charCodeAt(0));
      const version = await window.__TAURI__.app.getVersion();
      const rc = await invoke("build_number").catch(() => 0);
      const form = new FormData();
      form.set("uid", uid);
      form.set("note", $("[data-logs-note]").value.trim());
      form.set("app_version", rc ? `${version}+rc.${rc}` : version);
      form.set("logs", new Blob([zip], { type: "application/zip" }), "logs.zip");
      reportShots.forEach((shot, index) => form.append("screenshot", shot.blob, `screenshot-${index + 1}`));
      const response = await fetch(`${api()}/logs/reports`, { method: "POST", body: form });
      const result = await response.json().catch(() => ({}));
      if (!response.ok) throw new Error(result.error || `The server answered ${response.status}.`);
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
  $("[data-dispute-send]").addEventListener("click", async () => {
    try {
      await invoke("send_character_dispute", { note: $("[data-dispute-note]").value.trim() });
      toast("Dispute queued: it goes out with the game-menu proof");
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
    if (!testable && why.dataset.from !== "test") { why.hidden = false; why.textContent = "Test blocking: " + test.title.toLowerCase() + "."; why.dataset.from = "hint"; }
    if (testable && why.dataset.from === "hint") { why.hidden = true; why.dataset.from = ""; }
    if (info.hiding) $("[data-hidhide-state]").textContent = "Your controller is hidden from the game right now (the meter is holding inputs).";
  }
  // refreshed while the Results tab is open (often, so the "which one is it" light follows your presses)
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
    const meterOn = get("meter.enabled") !== false;
    $("[data-meter-state]").textContent = !meterOn ? "Meter off" : running ? "Meter running" : "Meter starting…";
    const fresh = running && status.time && Date.now() / 1000 - status.time < 5;
    text("game", !meterOn ? "Meter off (DPS Meter › Meter)" : !running ? "Meter starting…" : !fresh ? "Starting…"
      : status.game_window ? "Game window found" : "Waiting for the game");
    text("capture", fresh && status.frame ? `${status.frame[0]}×${status.frame[1]} via ${status.capture}` : "");
    text("bar", fresh ? (status.boss_bar ? "Boss bar on screen" : "No boss bar") : "—");
    text("status", fresh ? status.status || "" : "");
    const fight = fresh ? status.fight : null;
    text("fight", fight ? `${fight.boss} Lv ${fight.level}${fight.segment ? ` · x${fight.segment}` : ""} — ${n(fight.avg_dps)} DPS` : "No fight in progress.");
    text("fight-detail", fight ? `Team damage ${n(fight.damage)} of ${n(fight.max_hp)} HP · ${fight.seconds}s · ${n(fight.recent_dps)} DPS over the last 5 s` : "");
    const character = status.character || {};
    const who = character.uid ? `${character.name || "Name appears after your first result screen"} · UID ${character.uid}` : "Waiting for the game log…";
    text("character", who);
    $("[data-logs-uid]").textContent = character.uid ? `Sent as UID ${character.uid}.` : "Waiting for your game UID: open Aniimo with the meter running.";
    text("character-proof", !character.uid ? "" : character.verified
      ? "Proven: read from the game, right with your UID."
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
    $('[data-k="results.hold_controller"]').disabled = !status.hidhide_installed;
  }
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
    try { await invoke("save_settings", { settings }); } catch (error) { toast(String(error), true); }
    refresh();
  }
  for (const box of $$("[data-guide-k]", guide)) {
    box.addEventListener("change", () => { set(box.dataset.guideK, box.checked); fill(); scheduleSave(); });
  }
  $("[data-guide-next]").addEventListener("click", () => (guideAt === guideSteps.length ? closeGuide() : guideShow(guideAt + 1)));
  $("[data-guide-back]").addEventListener("click", () => guideShow(guideAt - 1));
  $("[data-guide-skip]").addEventListener("click", closeGuide);
  $("[data-guide-open]").addEventListener("click", openGuide);
  // the account step uses the same sign-in as DPS Meter › Leaderboards & account, and follows it
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
            textContent: `Your version is very out of date: ${release.outdated} update${plural} came out since. Update to get everything listed above.` }));
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
      summary.textContent = release.version === current ? `${release.version} (installed)`
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
    const label = `aLogs ${info.version}${info.rc ? ` (build ${info.rc})` : ""}`;
    $("[data-update-box]").hidden = false;
    $("[data-update-progress]").hidden = true;
    $("[data-update-title]").textContent = info.next_start ? `${label} installs next time aLogs opens`
      : info.downloaded ? `${label} is ready to install` : `${label} is available`;
    $("[data-install-later]").hidden = Boolean(info.next_start);
    for (const button of $$("[data-update-box] button")) button.disabled = false;
    $("[data-update-note]").textContent = $("[data-update-title]").textContent + ".";
  }
  function showProgress(p) {                         // the download, from update.rs (update-progress)
    $("[data-update-box]").hidden = false;
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
      if (info) showUpdate(info);
      else note.textContent = "aLogs is up to date.";
    } catch (error) {
      note.textContent = `Couldn't check: ${error}`;
    }
  });

  async function start() {
    let version = "";
    try {
      version = await window.__TAURI__.app.getVersion();
      const rc = await invoke("build_number").catch(() => 0);
      $("[data-app-version]").textContent = rc ? `${version} · RC${rc}` : version;
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
          `Includes Sanctum Helper${info.version ? ` v${info.version}` : ""} by sargeeva${info.gameDataVersion ? `, map data from game build ${info.gameDataVersion}` : ""}`);
      } catch {
        // Sanctum Helper not installed or not started: the plain credit stays
      }
    });
    setInterval(() => loadReleases(version), 6 * 3600 * 1000);
    try {
      const stored = await invoke("get_settings");
      const overlay = stored?.overlay;                    // settings from before 0.1.28: hide_from_capture, the opposite
      if (overlay && overlay.in_recordings === undefined && overlay.hide_from_capture !== undefined) overlay.in_recordings = !overlay.hide_from_capture;
      settings = merge(DEFAULTS, stored);
    } catch {
      settings = structuredClone(DEFAULTS);
    }
    fill();
    await refresh();
    void syncAccountPrefs();
    try {
      showUpdate(await invoke("update_state"));
      await window.__TAURI__.event.listen("update-ready", (event) => showUpdate(event.payload));
      await window.__TAURI__.event.listen("update-progress", (event) => showProgress(event.payload));
    } catch {
      // older app without the updater
    }
    if (settings.app.show_guide !== false) openGuide();
    // once a second while the window is up; nothing while it sits in the tray, and fresh the moment it's shown
    setInterval(() => { if (!document.hidden) void refresh(); }, 1000);
    document.addEventListener("visibilitychange", () => { if (!document.hidden) void refresh(); });
  }
  start();
})();
