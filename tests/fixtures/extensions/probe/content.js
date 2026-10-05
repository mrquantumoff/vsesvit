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
  const port = chrome.runtime.connect({ name: "probe" });
  port.onMessage.addListener((message) => {
    root.dataset.vsesvitProbePort = message;
    port.disconnect();
  });
  port.postMessage("ping");
})();
