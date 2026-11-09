// Beta Menu > Dev (beta builds only): proposed changes, each its own switch, to test one at a time, with a tab for each plug-in and
// for Core, the parts they share. Off at every start, on purpose, since a build can change what a switch does; the exception is one
// a tester made persistent, so they needn't remember what they liked, which has nothing to do with what goes into a release.
// Temporary: all of this comes out before a real release.
(() => {
  const invoke = window.__TAURI__.core.invoke;
  const root = document.querySelector("[data-dev-menu]");
  if (!root) return;

  const PLUGINS = [["core", "Core"], ["dps", "DPS meter"], ["sanctum", "Sanctum Helper"], ["leaderboards", "Leaderboards"]];
  const INFO = "Persistent: this setting starts ON instead of off at every restart and through updates. It only turns off by itself if the developer changes something about it. Don't check this unless you've tested the setting and decided it's no risk for you.";
  let plugin = "sanctum";
  let state = {};
  let rendered = "";
  // Filters: lit shows, darkened hides, in each row, per plug-in. Not saved: the tab opens with everything showing.
  const hidden = { core: new Set(), dps: new Set(), sanctum: new Set(), leaderboards: new Set() };

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

  async function command(name, args) {
    try {
      await invoke("sanctum_command", { command: name, args });
    } catch (error) {
      window.alogsToast?.(String(error), true);
    }
    void refresh();
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

  // ---------------------------------------------------------------- a plug-in's switches
  function sanctumNodes() {
    const options = state.info?.dev || [];
    if (!state.running) return [el("p", { class: "hint" }, "Sanctum Helper's switches need it running.")];
    if (!options.length) return [el("p", { class: "hint" }, "Loading the switches…")];
    const on = new Set(state.status?.devOn || []);
    const persistent = new Set(state.status?.devPersistent || []);
    const changed = state.status?.devChanged || [];
    const sections = [...new Set(options.map((o) => o.section))];
    const areas = [...new Set(options.map((o) => o.area))];
    const shown = options.filter((o) => !hidden.sanctum.has(o.section) && !hidden.sanctum.has(o.area));
    const nodes = [el("p", { class: "notice" }, `Beta only. All off at every start, except any you make Persistent. ${on.size} on now, ${persistent.size} persistent. With all off, Sanctum Helper works as in 0.1.36.`)];
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
        el("div", { class: "dev-row group-head" }, el("h2", { textContent: section }), el("h2", { class: "dev-persistent" }, "Persistent ", infoIcon())),
        here.map((o) => {
          const box = el("input", { type: "checkbox" });
          box.checked = on.has(o.id);
          box.addEventListener("change", () => void command("dev-set", { id: o.id, on: box.checked }));
          const keep = el("input", { type: "checkbox", "aria-label": `${o.title}: persistent` });
          keep.checked = persistent.has(o.id);
          keep.addEventListener("change", () => void command("dev-persistent", { id: o.id, persistent: keep.checked }));
          return el("div", { class: "dev-row" },
            el("label", { class: "toggle" }, box, el("span"), el("div", {},
              el("strong", { textContent: `${o.title} (${o.area})` }),
              el("small", { textContent: `${o.what} Gain: ${o.gain} Risk: ${o.risk} ${o.tested}.` }))),
            el("div", { class: "dev-persistent" }, keep));
        })));
    }
    return nodes;
  }

  // ---------------------------------------------------------------- frame
  const tabRow = el("div", { class: "tabs dev-plugins", role: "tablist", "aria-label": "Dev switches by plug-in" });
  const body = el("div");
  root.append(tabRow, el("p", { class: "lead", textContent: "Test each proposed change on its own." }), body);
  for (const [id, label] of PLUGINS) {
    tabRow.append(el("button", { type: "button", role: "tab", "data-dev-plugin": id, textContent: label, onclick: () => { plugin = id; draw(true); } }));
  }

  function draw(force = false) {
    const key = JSON.stringify([plugin, state.running, state.status?.devOn, state.status?.devPersistent, state.status?.devChanged, state.info?.dev?.length]);
    if (!force && key === rendered) return;
    rendered = key;
    tip.hidden = true;
    for (const button of tabRow.children) button.setAttribute("aria-selected", String(button.dataset.devPlugin === plugin));
    const name = PLUGINS.find(([id]) => id === plugin)[1];
    body.replaceChildren(...(plugin === "sanctum" ? sanctumNodes() : [el("p", { class: "hint", textContent: `No switches for ${name} yet.` })]));
  }

  async function refresh() {
    if (!root.closest("[data-panel]")?.classList.contains("is-active") || !root.closest("[data-page-body]")?.classList.contains("is-active")) return;
    try { state = await invoke("sanctum_state"); } catch { state = {}; }
    draw();
  }
  setInterval(refresh, 1000);
  document.querySelector('[data-page="beta"]')?.addEventListener("click", () => void refresh());
  document.querySelector('[data-page-body="beta"] [data-tab="dev"]')?.addEventListener("click", () => void refresh());
  draw(true);
})();
