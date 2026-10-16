// What a popup sees about "its" tab and window; posted to the fixture server and kept in the worker.
(async () => {
  const q = async (f) => { try { return await f(); } catch (e) { return "ERR " + (e.message || e); } };
  const tabs = (l) => l.map((t) => ({ id: t.id, windowId: t.windowId, active: t.active, url: t.url }));
  const data = {
    href: location.href,
    activeCurrent: await q(async () => tabs(await chrome.tabs.query({ active: true, currentWindow: true }))),
    current: await q(async () => { const w = await chrome.windows.getCurrent({ populate: true }); return { id: w.id, type: w.type, tabs: w.tabs.map((t) => t.url) }; }),
    getCurrentTab: await q(async () => { const t = await chrome.tabs.getCurrent(); return t ? { id: t.id, windowId: t.windowId, url: t.url } : null; }),
    viewsPopup: await q(async () => chrome.extension.getViews({ type: "popup" }).length),
    viewsTab: await q(async () => chrome.extension.getViews({ type: "tab" }).length),
    contexts: await q(async () => (await chrome.runtime.getContexts({})).map((c) => [c.contextType, c.documentUrl, c.tabId, c.windowId])),
  };
  document.title = "spy-popup";
  document.getElementById("out").textContent = JSON.stringify(data, null, 1);
  chrome.runtime.sendMessage({ type: "popup-report", data });
  fetch("http://127.0.0.1:8793/report/popup", { method: "POST", body: JSON.stringify(data) }).catch(() => {});
})();
document.getElementById("close").onclick = () => window.close();
