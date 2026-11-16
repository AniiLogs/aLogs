// The first-install window (main.rs --install-splash): the two choices and Install, then it follows the steps
// update.rs sends ("update-step": installing, starting, failed) and closes itself.
(() => {
  const to = new URLSearchParams(location.search).get("to");
  if (to) document.querySelector("[data-to]").textContent = `aLogs ${to.replace(/ B(\d+)$/, " Beta $1")}`;
  const step = document.querySelector("[data-step]");
  // a full, still icon while the choices are up; the bars cycle once it is installing (no progress to show)
  const loader = window.AlogsLoader?.create(document.querySelector("[data-loader]")).set({ done: 1, active: false });
  const invoke = window.__TAURI__?.core?.invoke;
  const steps = {
    installing: "Setting aLogs up on this PC…",
    starting: "Starting aLogs…",
    failed: "That didn't finish. Opening the full installer…",
  };
  document.querySelector("[data-go]").addEventListener("click", () => {
    document.body.classList.remove("choosing");
    step.textContent = steps.installing;
    loader?.loop();
    invoke?.("install_go", {
      desktop: document.querySelector("[data-desktop]").checked,
      autostart: document.querySelector("[data-autostart]").checked,
    });
  });
  document.querySelector("[data-cancel]").addEventListener("click", () => invoke?.("install_cancel"));
  window.__TAURI__?.event?.listen("update-step", (event) => {
    step.textContent = steps[event.payload] || steps.installing;
    document.body.classList.toggle("failed", event.payload === "failed");
    document.body.classList.toggle("done", event.payload === "starting");
    if (event.payload === "starting") loader?.set({ done: 1 });
    else if (event.payload === "failed") loader?.set({ section: 0, total: 0, active: false });
  });
})();
