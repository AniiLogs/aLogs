// aLogs loader: the aLogs icon as a progress meter.
// The HP arc is the total progress. The break bar is the part loading right now.
// Eklue and the buffs show that progress is still moving: each time it moves, Eklue flashes and fades
// back to grey, then a flash runs clockwise around the buffs from the top-right. When nothing moves
// (a stuck download), no new beat starts, so the icon sits still and grey. The beat's speed follows
// how fast it is heading for completion: quick loads beat quickly, slow ones slowly.
//
//   const loader = AlogsLoader.create(element, { sections: 4 });
//   loader.section(0.4);          // the current part is 40% loaded
//   loader.done();                // the current part finished; the next one starts empty
//   loader.set({ done: 2, section: 0.5 });  // or set everything at once
//   loader.set({ done: 1, section: 0.2, total: 0.73 });  // when the source knows its real total
//   loader.set({ ..., active: false });  // numbers changed but nothing really moved (a timed estimate)
//   loader.loop();                // no numbers to show: the bars cycle on their own, with no beat
//
// The same loader as the site's (aniilogs.github.io public/icons/alogs-loader.*); in aLogs the art is img/alogs-loader.svg.
(() => {
  const SVG_URL = "img/alogs-loader.svg";
  // One beat: Eklue flashes and fades over the whole beat; the buffs run inside its first 70 %,
  // so a new beat starts as the last one settles. 2.2 s at about 12 % a second (an 8 s load),
  // shorter when faster and longer when slower, within these limits.
  const BEAT_MS = 2200, FASTEST_MS = 1000, SLOWEST_MS = 3400, BASE_RATE = 0.12, RATE_WINDOW_MS = 3000;
  let markup = null;
  const fetchMarkup = () => (markup ??= fetch(SVG_URL).then((response) => {
    if (!response.ok) throw new Error(`aLogs loader art returned ${response.status}`);
    return response.text();
  }));

  const clamp = (value) => Math.min(1, Math.max(0, Number(value) || 0));

  function create(host, { sections = 1, svg = null } = {}) {
    const model = { sections: Math.max(1, Math.round(sections)), done: 0, section: 0, total: null, looping: true };
    let parts = null;
    let beating = false;
    let movedAt = -Infinity;
    let samples = []; // [time, total] of recent movement, for the speed toward completion
    const beatLength = () => {
      if (samples.length < 2) return BEAT_MS;
      const [[t0, p0], [t1, p1]] = [samples[0], samples[samples.length - 1]];
      const rate = (p1 - p0) / Math.max(0.25, (t1 - t0) / 1000);
      if (!(rate > 0)) return SLOWEST_MS;
      return Math.round(Math.min(SLOWEST_MS, Math.max(FASTEST_MS, BEAT_MS * Math.sqrt(BASE_RATE / rate))));
    };

    const beat = () => {
      if (!parts) return;
      beating = true;
      const length = beatLength();
      parts.root.classList.remove("beat");
      parts.root.style.setProperty("--beat", `${length}ms`);
      void parts.root.getBoundingClientRect(); // restart the CSS animations
      parts.root.classList.add("beat");
      const startedAt = performance.now();
      setTimeout(() => {
        beating = false;
        parts.root.classList.remove("beat");
        if (movedAt > startedAt) beat(); // progress moved during this beat: keep going
      }, length);
    };
    const moved = () => {
      movedAt = performance.now();
      samples.push([movedAt, currentTotal()]);
      samples = samples.filter(([time]) => movedAt - time <= RATE_WINDOW_MS);
      if (!beating) beat();
    };

    const currentTotal = () => (model.done >= model.sections ? 1 : model.total ?? (model.done + model.section) / model.sections);
    const render = () => {
      if (!parts) return;
      parts.root.classList.toggle("loop", model.looping);
      if (model.looping) {
        parts.hp.style.strokeDashoffset = "";
        parts.brk.style.strokeDashoffset = "";
        return;
      }
      const finished = model.done >= model.sections;
      const total = currentTotal();
      parts.hp.style.strokeDashoffset = String(100 - total * 100);
      parts.brk.style.strokeDashoffset = String(100 - (finished ? 1 : model.section) * 100);
      parts.root.setAttribute("aria-valuenow", String(Math.round(total * 100)));
    };

    const mount = (text) => {
      host.innerHTML = text;
      const root = host.querySelector("svg");
      root.setAttribute("aria-valuemin", "0");
      root.setAttribute("aria-valuemax", "100");
      parts = { root, hp: root.querySelector(".hp"), brk: root.querySelector(".brk") };
      render();
    };
    if (svg) mount(svg);
    else fetchMarkup().then(mount, () => { host.hidden = true; });

    const api = {
      // total: the HP arc's own value when the source reports one; otherwise it follows the parts.
      // active: whether this update is real movement (default: whether any number changed).
      set({ sections: count, done, section, total, active } = {}) {
        const before = `${model.looping}|${model.sections}|${model.done}|${model.section}|${model.total}`;
        if (count != null) model.sections = Math.max(1, Math.round(count));
        if (done != null) model.done = Math.min(model.sections, Math.max(0, Math.floor(done)));
        if (section != null) model.section = clamp(section);
        model.total = total == null ? null : clamp(total);
        model.looping = false;
        render();
        const changed = before !== `${model.looping}|${model.sections}|${model.done}|${model.section}|${model.total}`;
        if (active ?? changed) moved();
        return api;
      },
      section(fraction) { return api.set({ section: fraction }); },
      done() { return api.set({ done: model.done + 1, section: 0 }); },
      loop() { model.looping = true; samples = []; render(); return api; },
      get progress() { return model.looping ? null : Math.min(1, model.total ?? (model.done + model.section) / model.sections); },
    };
    return api;
  }

  window.AlogsLoader = { create };
})();
