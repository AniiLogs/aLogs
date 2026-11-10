// Beta Menu > Dev (beta builds only): proposed changes, each its own switch, to test one at a time, with a tab for each plug-in and
// for Core, the parts they share. Sanctum Helper's switches live in its own program; the rest in dev-options.json and dev.rs. One
// render for all of them. Off at every start, on purpose, since a build can change what a switch does; the exception is one
// a tester made persistent, so they needn't remember what they liked, which has nothing to do with what goes into a release.
// "How did it go?": a verdict and a note per switch, kept by the app (dev.rs) with how long the switch was on, and sent for all
// four tabs at once with the logs, the way Report a problem sends them.
// Temporary: all of this comes out before a real release.
(() => {
  const invoke = window.__TAURI__.core.invoke;
  const root = document.querySelector("[data-dev-menu]");
  if (!root) return;

  const PLUGINS = [["core", "Core"], ["dps", "DPS meter"], ["sanctum", "Sanctum Helper"], ["leaderboards", "Leaderboards"]];
  const WORKS_AS = { core: "aLogs works", dps: "the DPS meter works", sanctum: "Sanctum Helper works", leaderboards: "the leaderboards work" };
  const INFO = "Persistent: this setting starts ON instead of off at every restart and through updates. It only turns off by itself if the developer changes something about it. Don't check this unless you've tested the setting and decided it's no risk for you.";
  const VERDICTS = [["", "No verdict yet"], ["better", "Better"], ["same", "No difference"], ["worse", "Worse"], ["broke", "Broke something"]];
  const VERDICT_NAME = Object.fromEntries(VERDICTS);
  // The two that are no use without knowing what happened
  const NEEDS_NOTE = { worse: "Say what got worse, so it can be found in your logs.", broke: "Say what broke, so it can be found in your logs." };
  const TIME_INFO = "Counted a minute at a time, while the game is open.";
  let plugin = "sanctum";
  let state = {};      // Sanctum Helper's, from its program
  let dev = {};        // the others', from the app, and everyone's feedback
  let rendered = "";
  let loaded = false;  // the saved verdicts are here: until then nothing can be chosen, or a late answer would undo it
  let sending = false;
  // Filters: lit shows, darkened hides, in each row, per plug-in. Not saved: the tab opens with everything showing.
  const hidden = { core: new Set(), dps: new Set(), sanctum: new Set(), leaderboards: new Set() };
  // What the tester chose and wrote, by switch ("sanctum:<id>" for Sanctum Helper's). The copy that counts while the app
  // is open: the app is told of every change, and only tells back how long a switch was on and what was sent.
  const drafts = new Map();
  const rows = new Map();     // the rows on screen, by the same key
  const timers = new Map();   // a note still being typed, saved a moment after the last key
  const writes = new Set();   // saves on their way

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

  async function act(name, args) {
    try {
      await invoke(name, args);
    } catch (error) {
      window.alogsToast?.(String(error), true);
    }
    void refresh();
  }

  // ---------------------------------------------------------------- where a plug-in's switches come from
  function view(id) {
    if (id === "sanctum") {
      const options = state.info?.dev || [];
      return {
        options,
        on: state.status?.devOn || [],
        persistent: state.status?.devPersistent || [],
        changed: state.status?.devChanged || [],
        waiting: !state.running ? "Sanctum Helper's switches need it running." : options.length ? null : "Loading the switches…",
        set: (option, on) => act("sanctum_command", { command: "dev-set", args: { id: option, on } }),
        keep: (option, persistent) => act("sanctum_command", { command: "dev-persistent", args: { id: option, persistent } }),
      };
    }
    const options = (dev.options || []).filter((o) => o.plugin === id);
    const mine = (list) => (list || []).filter((option) => options.some((o) => o.id === option));
    return {
      options,
      on: mine(dev.on),
      persistent: mine(dev.persistent),
      changed: mine(dev.changed),
      waiting: options.length ? null : `No switches for ${PLUGINS.find(([p]) => p === id)[1]} yet.`,
      set: (option, on) => act("dev_set", { id: option, on }),
      keep: (option, persistent) => act("dev_persistent", { id: option, persistent }),
    };
  }

  // ---------------------------------------------------------------- the info panel, at the mouse
  const tip = el("div", { class: "dev-tip", hidden: true, role: "tooltip", textContent: INFO });
  document.body.append(tip);
  function placeTip(x, y) {
    tip.hidden = false;
    const box = tip.getBoundingClientRect();
    tip.style.left = `${Math.max(8, Math.min(x + 12, window.innerWidth - box.width - 8))}px`;
    tip.style.top = `${y + 18 + box.height + 8 > window.innerHeight ? Math.max(8, y - box.height - 10) : y + 18}px`;
  }
  function infoIcon() {
    const icon = el("span", { class: "dev-info", "aria-label": INFO, role: "img", tabIndex: 0 }, "ⓘ");
    icon.addEventListener("mouseenter", (event) => placeTip(event.clientX, event.clientY));
    icon.addEventListener("mousemove", (event) => placeTip(event.clientX, event.clientY));
    icon.addEventListener("mouseleave", () => { tip.hidden = true; });
    icon.addEventListener("focus", () => { const r = icon.getBoundingClientRect(); placeTip(r.left, r.bottom - 12); });
    icon.addEventListener("blur", () => { tip.hidden = true; });
    return icon;
  }

  // ---------------------------------------------------------------- filters
  function chip(label, lit, onclick) {
    return el("button", { type: "button", class: `dev-chip${lit ? "" : " is-off"}`, "aria-pressed": String(lit), textContent: label, onclick });
  }
  function filters(sections, areas, showing, total) {
    const off = hidden[plugin];
    const names = [...sections, ...areas];
    const flip = (name) => () => { if (off.has(name)) off.delete(name); else off.add(name); draw(true); };
    const row = (label, list) => el("div", { class: "dev-filter-row" }, el("span", { class: "dev-filter-name", textContent: label }),
      el("div", { class: "dev-chips" }, list.map((name) => chip(name, !off.has(name), flip(name)))));
    const allOn = off.size === 0;
    return el("div", { class: "dev-filters" },
      el("div", { class: "dev-filter-row" }, el("span"), el("div", { class: "dev-chips" }, chip("Show all", allOn, () => {
        if (allOn) names.forEach((name) => off.add(name)); else off.clear();
        draw(true);
      }))),
      row("Risk", sections), row("Type", areas),
      el("p", { class: "hint", textContent: `Showing ${showing} of ${total}` }));
  }

  // ---------------------------------------------------------------- how each switch went
  const keyOf = (pluginId, id) => (pluginId === "sanctum" ? `sanctum:${id}` : id);
  // What the app has for a switch: time on, and what was sent. Only for the same revision: a changed switch starts again.
  const kept = (key, revision) => { const found = dev.feedback?.[key]; return found && found.revision === revision ? found : null; };
  const lacksNote = (draft) => !!NEEDS_NOTE[draft.verdict] && !draft.note.trim();
  const isSent = (key, draft) => {
    const found = kept(key, draft.revision);
    return !!draft.verdict && !!found?.sent_at && found.sent_verdict === draft.verdict && found.sent_note === draft.note.trim();
  };
  const toSend = () => [...drafts].filter(([key, draft]) => draft.verdict && !isSent(key, draft)).map(([key, draft]) => ({ key, ...draft }));

  // Verdicts saved earlier, for every tab: the count at the bottom is for all four.
  function adopt() {
    if (!dev.feedback) return false;
    for (const [key, found] of Object.entries(dev.feedback)) {
      if (!drafts.has(key) && found.verdict) {
        drafts.set(key, { verdict: found.verdict, note: found.note || "", plugin: found.plugin, title: found.title, revision: found.revision });
      }
    }
    const first = !loaded;
    loaded = true;
    return first;
  }

  function draftFor(key, pluginId, option) {
    const revision = option.revision || 0;
    if (!loaded) return { verdict: "", note: "", plugin: pluginId, title: option.title, revision };
    let draft = drafts.get(key);
    if (draft && draft.revision !== revision) {
      // The switch changed since this was said: it was about something else. The app forgets it too.
      const said = draft.verdict;
      draft = null;
      drafts.delete(key);
      if (said) queueMicrotask(() => store(key));
    }
    if (!draft) {
      const found = kept(key, revision);
      draft = { verdict: found?.verdict || "", note: found?.note || "", plugin: pluginId, title: option.title, revision };
      drafts.set(key, draft);
    }
    draft.title = option.title;
    return draft;
  }

  function store(key, wait = 0) {
    clearTimeout(timers.get(key));
    timers.delete(key);
    const write = () => {
      timers.delete(key);
      const draft = drafts.get(key);
      if (!draft) return;
      const call = invoke("dev_feedback_set", { key, plugin: draft.plugin, title: draft.title, revision: draft.revision, verdict: draft.verdict, note: draft.note })
        .catch((error) => window.alogsToast?.(String(error), true))
        .finally(() => writes.delete(call));
      writes.add(call);
    };
    if (wait) timers.set(key, setTimeout(write, wait)); else write();
  }
  async function flush() {
    for (const key of [...timers.keys()]) store(key);
    await Promise.all([...writes]);
  }

  function timeOn(seconds, on) {
    const minutes = Math.floor(seconds / 60);
    if (minutes >= 60) return `on for ${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, "0")} min`;
    if (minutes) return `on for ${minutes} min`;
    return on ? "on, under a minute so far" : "not turned on yet";
  }
  const day = (seconds) => new Date(seconds * 1000).toLocaleDateString("en-GB", { day: "numeric", month: "short" });

  // A row's own parts, changed where they stand: the rows aren't built again while someone is typing in one.
  function paintRow(key) {
    const row = rows.get(key);
    if (!row) return;
    const { draft, select, time, note, need } = row;
    if (select.value !== draft.verdict) select.value = draft.verdict;
    select.dataset.verdict = draft.verdict;
    const sent = isSent(key, draft);
    const text = sent ? `sent ${day(kept(key, draft.revision).sent_at)} · change it to send again` : timeOn(kept(key, draft.revision)?.seconds || 0, row.on);
    if (time.textContent !== text) time.textContent = text;
    time.title = sent ? "" : TIME_INFO;
    note.hidden = !draft.verdict;
    const needed = NEEDS_NOTE[draft.verdict];
    note.placeholder = needed ? "What went wrong? (needed for Worse and Broke something)" : "Notes (optional)";
    note.classList.toggle("is-needed", lacksNote(draft));
    need.hidden = !lacksNote(draft);
    need.textContent = needed || "";
  }

  function verdictParts(pluginId, option, on) {
    const key = keyOf(pluginId, option.id);
    const draft = draftFor(key, pluginId, option);
    const select = el("select", { class: "dev-result", disabled: !loaded, "aria-label": `${option.title}: how did it go?` },
      VERDICTS.map(([value, label]) => el("option", { value, textContent: label })));
    const time = el("span", { class: "dev-time" });
    const note = el("input", { type: "text", class: "dev-note", maxLength: 500, value: draft.note, "data-dev-note": key, "aria-label": `${option.title}: notes` });
    const need = el("p", { class: "dev-need" });
    select.addEventListener("change", () => {
      draft.verdict = select.value;
      store(key);
      paintRow(key);
      paintSend();
      if (lacksNote(draft)) note.focus();
    });
    note.addEventListener("input", () => {
      draft.note = note.value;
      store(key, 600);
      paintRow(key);
      paintSend();
    });
    rows.set(key, { draft, select, time, note, need, on });
    paintRow(key);
    return { cell: el("div", { class: "dev-verdict" }, select, time), note, need };
  }

  // ---------------------------------------------------------------- a plug-in's switches
  function switchNodes(id) {
    const { options, waiting, set, keep: setKeep, ...lists } = view(id);
    if (waiting) return [el("p", { class: "hint" }, waiting)];
    const on = new Set(lists.on);
    const persistent = new Set(lists.persistent);
    const changed = lists.changed;
    const sections = [...new Set(options.map((o) => o.section))];
    const areas = [...new Set(options.map((o) => o.area))];
    const shown = options.filter((o) => !hidden[id].has(o.section) && !hidden[id].has(o.area));
    const nodes = [el("p", { class: "notice" }, `Beta only. All off at every start, except any you make Persistent. ${on.size} on now, ${persistent.size} persistent. With all off, ${WORKS_AS[id]} as in 0.1.36.`)];
    if (changed.length) {
      const names = changed.map((id) => options.find((o) => o.id === id)?.title || id).join(", ");
      nodes.push(el("p", { class: "notice" }, `Changed in this update, so no longer persistent: ${names}. Test it again before making it persistent.`));
    }
    nodes.push(filters(sections, areas, shown.length, options.length));
    if (!shown.length) nodes.push(el("p", { class: "hint" }, "No switches match these filters."));
    for (const section of sections) {
      const here = shown.filter((o) => o.section === section);
      if (!here.length) continue;
      nodes.push(el("div", { class: "group" },
        el("div", { class: "dev-row group-head" }, el("h2", { textContent: section }), el("h2", { class: "dev-how", textContent: "How did it go?" }),
          el("h2", { class: "dev-persistent" }, "Persistent ", infoIcon())),
        here.map((o) => {
          const box = el("input", { type: "checkbox" });
          box.checked = on.has(o.id);
          box.addEventListener("change", () => void set(o.id, box.checked));
          const keep = el("input", { type: "checkbox", "aria-label": `${o.title}: persistent` });
          keep.checked = persistent.has(o.id);
          keep.addEventListener("change", () => void setKeep(o.id, keep.checked));
          const verdict = verdictParts(id, o, on.has(o.id));
          return el("div", { class: "dev-row" },
            el("label", { class: "toggle" }, box, el("span"), el("div", {},
              el("strong", { textContent: `${o.title} (${o.area})` }),
              el("small", { textContent: `${o.what} Gain: ${o.gain} Risk: ${o.risk} ${String(o.tested).replace(/\.$/, "")}.` }))),
            verdict.cell,
            el("div", { class: "dev-persistent" }, keep),
            verdict.note, verdict.need);
        })));
    }
    return nodes;
  }

  // ---------------------------------------------------------------- sending it
  const sendText = el("p");
  const sendButton = el("button", { type: "button", class: "primary", textContent: "Send feedback", onclick: () => void send() });
  let sendShown = "";

  function counts(list) {
    return VERDICTS.slice(1).map(([verdict, name]) => {
      const mine = list.filter((draft) => draft.verdict === verdict);
      const bare = mine.filter(lacksNote).length;
      if (!mine.length) return null;
      const needs = !bare ? "" : mine.length === 1 ? " (needs a note)" : ` (${bare} still ${bare === 1 ? "needs" : "need"} a note)`;
      return [`${mine.length} ${name}`, needs];
    }).filter(Boolean);
  }

  function paintSend() {
    const list = toSend();
    const elsewhere = list.filter((draft) => draft.plugin !== plugin).length;
    const said = counts(list).map(([count, needs]) => count + needs).join(", ");
    const key = JSON.stringify([said, elsewhere, sending, loaded]);
    if (key === sendShown) return;
    sendShown = key;
    sendButton.disabled = sending || !list.length;
    sendButton.textContent = sending ? "Sending…" : "Send feedback";
    if (!list.length) {
      sendText.replaceChildren(el("strong", { textContent: "Nothing to send yet." }),
        " Say how a switch went and it goes up from here with your logs, which switches were on, for how long, and this build.");
      return;
    }
    const other = !elsewhere ? "" : elsewhere === list.length ? " (on other tabs)" : ` (${elsewhere} on other tabs)`;
    sendText.replaceChildren(el("strong", { textContent: `${list.length} to send${other}:` }),
      ` ${said}. Goes up with your logs, which switches were on, for how long, and this build. Switches without a verdict aren't sent.`);
  }

  // The report's own note, read first by whoever opens it: the counts, then a line per switch.
  function reportNote(list) {
    const plugins = Object.fromEntries(PLUGINS);
    const lines = list.map((draft) => `${VERDICT_NAME[draft.verdict]}: ${draft.title} (${plugins[draft.plugin] || draft.plugin})${draft.note.trim() ? `: ${draft.note.trim()}` : ""}`);
    return [`Dev feedback: ${counts(list).map(([count]) => count).join(", ")}.`, ...lines].join("\n").slice(0, 2000);
  }

  async function send() {
    if (sending) return;
    const list = toSend();
    if (!list.length) return;
    const bare = list.find(lacksNote);
    if (bare) {
      // To the row that needs it, on whichever tab it is
      if (PLUGINS.some(([id]) => id === bare.plugin)) plugin = bare.plugin;
      hidden[plugin].clear();
      await refresh(true);
      const input = rows.get(bare.key)?.note;
      input?.scrollIntoView({ block: "center" });
      input?.focus();
      window.alogsToast?.(`${VERDICT_NAME[bare.verdict]} needs a note first: ${bare.title}.`);
      return;
    }
    sending = true;
    paintSend();
    try {
      await flush();
      if (!window.alogsSendReport) throw new Error("this page hasn't finished loading");
      await window.alogsSendReport({ note: reportNote(list), devFeedback: true });
      await invoke("dev_feedback_sent");
      window.alogsToast?.(`Feedback on ${list.length === 1 ? "1 switch" : `${list.length} switches`} sent with your logs. Thanks!`);
    } catch (error) {
      window.alogsToast?.(`Couldn't send the feedback: ${error.message || error}`, true);
    } finally {
      sending = false;
    }
    await refresh(true);
  }

  // ---------------------------------------------------------------- frame
  const tabRow = el("div", { class: "tabs dev-plugins", role: "tablist", "aria-label": "Dev switches by plug-in" });
  const body = el("div");
  root.append(tabRow, el("p", { class: "lead", textContent: "Test each proposed change on its own, then say how it went." }), body,
    el("div", { class: "dev-send" }, sendText, sendButton));
  for (const [id, label] of PLUGINS) {
    tabRow.append(el("button", { type: "button", role: "tab", "data-dev-plugin": id, textContent: label, onclick: () => { plugin = id; draw(true); void refresh(); } }));
  }

  function draw(force = false) {
    const { options, on, persistent, changed, waiting } = view(plugin);
    const key = JSON.stringify([plugin, waiting, on, persistent, changed, options.length, loaded]);
    if (force || key !== rendered) {
      rendered = key;
      tip.hidden = true;
      for (const button of tabRow.children) button.setAttribute("aria-selected", String(button.dataset.devPlugin === plugin));
      // Someone typing a note keeps their place when the rows are built again (a switch went on or off)
      const typing = document.activeElement?.dataset?.devNote;
      const caret = typing ? [document.activeElement.selectionStart, document.activeElement.selectionEnd] : null;
      rows.clear();
      body.replaceChildren(...switchNodes(plugin));
      const input = typing && rows.get(typing)?.note;
      if (input && !input.hidden) {
        input.focus();
        input.setSelectionRange(...caret);
      }
    } else {
      for (const key of rows.keys()) paintRow(key);
    }
    paintSend();
  }

  async function refresh(force = false) {
    if (!root.closest("[data-panel]")?.classList.contains("is-active") || !root.closest("[data-page-body]")?.classList.contains("is-active")) return;
    // The app's every time: it also has how long each switch has been on and what was sent, for every tab
    const asked = [invoke("dev_state").then((found) => { dev = found; }, () => { dev = {}; })];
    if (plugin === "sanctum") asked.push(invoke("sanctum_state").then((found) => { state = found; }, () => { state = {}; }));
    await Promise.all(asked);
    const first = adopt();
    draw(force || first);
  }
  setInterval(refresh, 1000);
  document.querySelector('[data-page="beta"]')?.addEventListener("click", () => void refresh());
  document.querySelector('[data-page-body="beta"] [data-tab="dev"]')?.addEventListener("click", () => void refresh());
  invoke("dev_state").then((found) => { dev = found; adopt(); draw(true); }).catch(() => {});
  draw(true);
})();
