const report = (event, data = {}) =>
  fetch("http://report.vsesvit.test/report", { method: "POST", body: JSON.stringify({ ext: "spy", event, t: Date.now(), ...data }) }).catch(() => {});
window.spyPopup = (async () => {
  const r = { hasFocus: document.hasFocus(), size: [innerWidth, innerHeight] };
  try { r.activeCurrent = (await chrome.tabs.query({ active: true, currentWindow: true })).map((t) => ({ id: t.id, url: t.url })); } catch (e) { r.err = e.message; }
  try { const c = await chrome.tabs.getCurrent(); r.current = c ? { id: c.id, windowId: c.windowId } : null; } catch (e) { r.currentErr = e.message; }
  report("popup", r);
  return r;
})();
window.addEventListener("focus", () => report("popup-focus", { hasFocus: document.hasFocus() }));
document.getElementById("field").addEventListener("keydown", (e) => report("popup-key", { key: e.key }));
