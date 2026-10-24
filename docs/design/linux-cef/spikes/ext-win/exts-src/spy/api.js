// The spy's controls, callable from an extension page (the shell opens options.html in a hidden
// browser and evaluates spy.*). Events are recorded by background.js and the report server.
const rep = (event, data = {}) =>
  fetch("http://report.vsesvit.test/report", { method: "POST", body: JSON.stringify({ ext: "spy", event, t: Date.now(), ...data }) }).catch(() => {});
window.spy = {
  async snapshot() {
    const tabs = await chrome.tabs.query({});
    const wins = await chrome.windows.getAll({ populate: true });
    return {
      tabs: tabs.map((t) => ({ id: t.id, windowId: t.windowId, url: t.url, active: t.active, incognito: t.incognito })),
      windows: wins.map((w) => ({ id: w.id, type: w.type, incognito: w.incognito, focused: w.focused, tabs: (w.tabs || []).map((t) => t.url) })),
    };
  },
  notify: (id, title) =>
    new Promise((r) =>
      chrome.notifications.create(id, { type: "basic", iconUrl: "icon.png", title, message: "spy says hi", buttons: [{ title: "B1" }], requireInteraction: true }, (nid) =>
        r({ nid, err: chrome.runtime.lastError ? chrome.runtime.lastError.message : null })),
    ),
  permLevel: () => new Promise((r) => chrome.notifications.getPermissionLevel((l) => r(l))),
  clearNotif: (id) => chrome.notifications.clear(id),
  notifAll: () => chrome.notifications.getAll(),
  async open(kind, url, opts = {}) {
    try {
      if (kind === "tab") { const t = await chrome.tabs.create({ url, ...opts }); return { id: t.id, windowId: t.windowId, pendingUrl: t.pendingUrl, url: t.url, incognito: t.incognito }; }
      if (kind === "win") { const w = await chrome.windows.create({ url, ...opts }); return { id: w.id, type: w.type, incognito: w.incognito, tabs: (w.tabs || []).map((t) => t.id) }; }
      if (kind === "options") { await chrome.runtime.openOptionsPage(); return "ok"; }
      return "unknown kind";
    } catch (e) { return "ERR " + e.message; }
  },
  async setCookie(url, name, value) { try { const c = await chrome.cookies.set({ url, name, value }); return c ? { name: c.name, value: c.value, domain: c.domain } : null; } catch (e) { return "ERR " + e.message; } },
  async getCookies(url) { return (await chrome.cookies.getAll({ url })).map((c) => c.name + "=" + c.value); },
  incog: () => chrome.extension.inIncognitoContext,
  rep,
};
