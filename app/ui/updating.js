// The updating window (main.rs --update-splash): shows which build is going in and follows the steps main.rs sends
// ("update-step": installing, starting, failed). Nothing to click; it closes itself.
(() => {
  const to = new URLSearchParams(location.search).get("to");
  if (to) document.querySelector("[data-to]").textContent = `Updating to aLogs ${to}`;
  const step = document.querySelector("[data-step]");
  const steps = {
    installing: "Installing the update…",
    starting: "Starting aLogs…",
    failed: "The update didn't finish. aLogs opens as it was; try again from About › Updates.",
  };
  window.__TAURI__?.event?.listen("update-step", (event) => {
    step.textContent = steps[event.payload] || steps.installing;
    document.body.classList.toggle("failed", event.payload === "failed");
    document.body.classList.toggle("done", event.payload === "starting");
  });
})();
