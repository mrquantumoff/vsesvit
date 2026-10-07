(async () => {
  const root = document.documentElement;
  try {
    root.dataset.vsesvitUpdateProbe = String(await chrome.runtime.sendMessage("update-probe"));
  } catch (e) {
    root.dataset.vsesvitUpdateProbe = "error: " + e;
  }
})();
