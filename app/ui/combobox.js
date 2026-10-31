// A searchable dropdown for long lists (the AniiLogs site's makeCombobox, the same behaviour): it wraps an existing
// <select>, which stays the source of truth. A text box shows what you type and the list narrows on every key -
// anywhere in the text, any case or accent, every word must match - with the matching letters highlighted. Arrows,
// Enter and Esc (Esc keeps the previous choice); clicking outside closes. Picking sets the select and fires its
// normal "change", so the page's own handlers work as before. window.makeCombobox(select) -> { refresh } or null.
(() => {
  const fold = (s) => String(s || "").normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();

  function makeCombobox(select) {
    if (!select || select.dataset.combobox) return null;
    select.dataset.combobox = "1";
    const wrap = document.createElement("div"); wrap.className = "combobox";
    const input = document.createElement("input");
    input.type = "text"; input.className = "combobox-input"; input.autocomplete = "off"; input.spellcheck = false;
    input.setAttribute("role", "combobox"); input.setAttribute("aria-expanded", "false");
    const list = document.createElement("div"); list.className = "combobox-list"; list.setAttribute("role", "listbox"); list.hidden = true;
    select.hidden = true;
    select.parentNode.insertBefore(wrap, select);
    wrap.append(input, list, select);
    let shown = []; let active = -1; let before = select.value;

    const label = () => select.options[select.selectedIndex]?.textContent || "";
    const show = () => { input.value = label(); input.placeholder = ""; };
    function render() {
      const words = fold(input.value).split(/\s+/).filter(Boolean);
      list.replaceChildren(); shown = [];
      for (const option of select.options) {
        const text = option.textContent; const hay = fold(text + " " + (option.dataset.search || ""));
        if (!words.every((w) => hay.includes(w))) continue;
        const row = document.createElement("div");
        row.className = "combobox-option"; row.setAttribute("role", "option"); row.dataset.value = option.value;
        if (option.disabled) row.classList.add("is-disabled");
        // highlight each typed word where it shows in the label
        let html = text.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c]));
        for (const w of words) {
          const at = fold(text).indexOf(w);
          if (at >= 0) { html = html.replace(text.slice(at, at + w.length), (m) => `<mark>${m}</mark>`); }
        }
        row.innerHTML = html;
        row.addEventListener("mousedown", (event) => { event.preventDefault(); pick(option.value); });
        list.append(row); shown.push(option);
      }
      if (!shown.length) { const none = document.createElement("div"); none.className = "combobox-empty"; none.textContent = "No matches"; list.append(none); }
      active = shown.findIndex((o) => o.value === select.value);
      if (active < 0 && shown.length) active = 0;
      mark();
    }
    function mark() {
      [...list.querySelectorAll(".combobox-option")].forEach((row, i) => row.classList.toggle("is-active", i === active));
      const row = list.querySelector(".is-active");             // only the list scrolls (scrollIntoView moved the page)
      if (row) {
        if (row.offsetTop < list.scrollTop) list.scrollTop = row.offsetTop;
        else if (row.offsetTop + row.offsetHeight > list.scrollTop + list.clientHeight) list.scrollTop = row.offsetTop + row.offsetHeight - list.clientHeight;
      }
    }
    function open() {
      if (!list.hidden) return;
      before = select.value; input.value = ""; input.placeholder = label() || "Type to filter";
      list.hidden = false; input.setAttribute("aria-expanded", "true"); render();
    }
    function close(keep = true) {
      if (list.hidden) return;
      if (!keep) select.value = before;
      list.hidden = true; input.setAttribute("aria-expanded", "false"); show();
    }
    function pick(value) {
      const option = [...select.options].find((o) => o.value === value);
      if (!option || option.disabled) return;
      const changed = select.value !== value;
      select.value = value; close();
      if (changed) select.dispatchEvent(new Event("change", { bubbles: true }));
    }
    input.addEventListener("focus", open);
    input.addEventListener("mousedown", () => { if (list.hidden) setTimeout(open); });
    input.addEventListener("input", () => { if (list.hidden) open(); render(); });
    input.addEventListener("blur", () => close(false));
    input.addEventListener("keydown", (event) => {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault(); open();
        if (shown.length) { active = (active + (event.key === "ArrowDown" ? 1 : -1) + shown.length) % shown.length; mark(); }
      } else if (event.key === "Enter") {
        event.preventDefault(); if (shown[active]) pick(shown[active].value);
      } else if (event.key === "Escape") {
        event.preventDefault(); close(false); input.blur();
      }
    });
    show();
    return { wrap, input, list, refresh: show, close };
  }
  window.makeCombobox = makeCombobox;
})();
