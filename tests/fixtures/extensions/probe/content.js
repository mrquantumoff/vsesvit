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
})();
