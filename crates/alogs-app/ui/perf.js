// General Settings › Performance (perf-options.js): the main switch, then a section per add-on that opens and closes,
// each option a choice saved as perf.<id>. Drawn before app.js binds the settings form, so the inputs save like any
// other setting (data-k); app.js calls alogsPerf.sync() whenever the form is filled or changed.
(() => {
  const root = document.querySelector("[data-perf-menu]");
  const data = window.ALOGS_PERF;
  if (!root || !data) return;

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

  const master = el("input", { type: "checkbox" });
  master.dataset.k = "perf.on";
  const reset = el("button", { type: "button", textContent: "Back to the defaults" });
  root.append(el("div", { class: "group perf-master" },
    el("label", { class: "toggle" }, master, el("span"),
      el("div", {}, el("strong", { textContent: "Performance toggles" }),
        el("small", { textContent: "Off (the default): aLogs runs as it's built, on every default below, and they can't be changed. On: what you choose below applies. Turn it off at any time to go back; your choices are kept for next time." }))),
    el("div", { class: "row" }, reset, el("span", { class: "hint", "data-perf-changed": "" }))));

  const counts = {};
  for (const [plugin, name, about] of data.sections) {
    const options = data.options.filter((o) => o.plugin === plugin);
    const count = el("span", { class: "perf-count" });
    counts[plugin] = count;
    const body = options.length ? options.map((o) => {
      const select = el("select", {}, o.choices.map(([value, label]) => el("option", { value: String(value), textContent: label })));
      select.dataset.k = `perf.${o.id}`;
      if (o.number) select.dataset.type = "number";
      return el("label", { class: "field" }, el("span", { textContent: o.title }), select, o.hint && el("small", { class: "hint", textContent: o.hint }));
    }) : [el("p", { class: "hint", textContent: "Nothing here yet: options for this add-on move here from the Beta Menu's Dev tab once they're tested." })];
    root.append(el("details", { class: "group perf-section", open: options.length > 0 },
      el("summary", {}, el("strong", { textContent: name }), count),
      el("p", { class: "hint", textContent: about }), body));
  }

  const value = (get, o) => {
    const v = get(`perf.${o.id}`);
    return v === undefined || v === null || v === "" ? o.default : v;
  };
  // Greys out the choices while the main switch is off, and says how many differ from the default.
  function sync(get) {
    const on = Boolean(get("perf.on"));
    root.classList.toggle("perf-off", !on);
    for (const select of root.querySelectorAll("select[data-k]")) select.disabled = !on;
    reset.disabled = !on;
    let all = 0;
    for (const [plugin] of data.sections) {
      const n = data.options.filter((o) => o.plugin === plugin && String(value(get, o)) !== String(o.default)).length;
      all += n;
      counts[plugin].textContent = n ? `${n} changed` : "";
    }
    root.querySelector("[data-perf-changed]").textContent = !all ? "Everything is on its default."
      : `${all} changed from the default${on ? "" : " (not used while this is off)"}.`;
  }
  reset.addEventListener("click", () => {
    for (const o of data.options) {
      const select = root.querySelector(`[data-k="perf.${o.id}"]`);
      if (!select || select.value === String(o.default)) continue;
      select.value = String(o.default);
      select.dispatchEvent(new Event("change"));
    }
  });

  // What each option's own setting should hold: the choice while the main switch is on, the default while it's off.
  function apply(settings) {
    const perf = settings.perf || {};
    for (const o of data.options) {
      const chosen = perf[o.id] === undefined || perf[o.id] === "" ? o.default : perf[o.id];
      const keys = o.key.split(".");
      const last = keys.pop();
      keys.reduce((v, k) => (v[k] ??= {}), settings)[last] = perf.on ? chosen : o.default;
    }
    return settings;
  }
  // Settings saved before the tab existed: an option someone had changed comes over, with the main switch on for them,
  // so nothing they picked changes when they update.
  function migrate(stored) {
    if (!stored || stored.perf) return stored;
    const perf = { on: false };
    for (const o of data.options) {
      const keys = o.key.split(".");
      const old = keys.reduce((v, k) => (v == null ? undefined : v[k]), stored);
      perf[o.id] = o.default;
      if (old === undefined || old === null || old === "" || String(old) === String(o.default)) continue;
      if (o.key === "capture.fight_rate" && !(stored.user_set || []).includes(o.key)) continue;   // an old default, not a choice
      perf[o.id] = o.number ? Number(old) : old;
      perf.on = true;
    }
    stored.perf = perf;
    return stored;
  }
  const defaults = () => Object.fromEntries([["on", false], ...data.options.map((o) => [o.id, o.default])]);
  window.alogsPerf = { sync, apply, migrate, defaults };
})();
