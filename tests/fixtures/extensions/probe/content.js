(async () => {
  const root = document.documentElement;
  root.dataset.vsesvitProbe = "content-script";
  try {
    const reply = await chrome.runtime.sendMessage({ type: "hello", href: location.href });
    root.dataset.vsesvitProbe = reply && reply.ok ? "background-replied" : "bad-reply";
    root.dataset.vsesvitVisits = String(reply?.visits ?? "");
  } catch (e) {
    root.dataset.vsesvitProbe = "error: " + e;
  }
  chrome.storage.onChanged.addListener((changes) => {
    if (changes.menuClick) root.dataset.vsesvitProbeMenu = JSON.stringify(changes.menuClick.newValue);
    if (changes.command) root.dataset.vsesvitProbeCommand = JSON.stringify(changes.command.newValue);
    if (changes.notification) root.dataset.vsesvitProbeNotification = JSON.stringify(changes.notification.newValue);
  });
  // The self-test sets data-vsesvit-notify ("keep" to leave the notification shown).
  new MutationObserver(async () => {
    const want = root.dataset.vsesvitNotify;
    if (!want) return;
    delete root.dataset.vsesvitNotify;
    try {
      root.dataset.vsesvitProbeNotified = JSON.stringify(await chrome.runtime.sendMessage({ type: "notify", keep: want === "keep" }));
    } catch (e) {
      root.dataset.vsesvitProbeNotified = JSON.stringify({ error: String(e) });
    }
  }).observe(root, { attributes: true, attributeFilter: ["data-vsesvit-notify"] });
  // The self-test sets data-vsesvit-dnr: "allow" to stop filtering this site, or "clear".
  new MutationObserver(async () => {
    const want = root.dataset.vsesvitDnr;
    if (!want) return;
    delete root.dataset.vsesvitDnr;
    try {
      root.dataset.vsesvitProbeDnr = JSON.stringify(await chrome.runtime.sendMessage({ type: "dnr", want }));
    } catch (e) {
      root.dataset.vsesvitProbeDnr = JSON.stringify({ error: String(e) });
    }
  }).observe(root, { attributes: true, attributeFilter: ["data-vsesvit-dnr"] });
  // The self-test sets data-vsesvit-scripts: "register" for the dynamic content script, or "unregister".
  new MutationObserver(async () => {
    const want = root.dataset.vsesvitScripts;
    if (!want) return;
    delete root.dataset.vsesvitScripts;
    try {
      root.dataset.vsesvitProbeScripts = JSON.stringify(await chrome.runtime.sendMessage({ type: "scripts", want }));
    } catch (e) {
      root.dataset.vsesvitProbeScripts = JSON.stringify({ error: String(e) });
    }
  }).observe(root, { attributes: true, attributeFilter: ["data-vsesvit-scripts"] });
  // The self-test sets data-vsesvit-windows: "open", "move", then "close".
  new MutationObserver(async () => {
    const want = root.dataset.vsesvitWindows;
    if (!want) return;
    delete root.dataset.vsesvitWindows;
    try {
      root.dataset.vsesvitProbeWindows = JSON.stringify(await chrome.runtime.sendMessage({ type: "windows", want }));
    } catch (e) {
      root.dataset.vsesvitProbeWindows = JSON.stringify({ error: String(e) });
    }
  }).observe(root, { attributes: true, attributeFilter: ["data-vsesvit-windows"] });
  const port = chrome.runtime.connect({ name: "probe" });
  port.onMessage.addListener((message) => {
    root.dataset.vsesvitProbePort = message;
    port.disconnect();
  });
  port.postMessage("ping");
})();
