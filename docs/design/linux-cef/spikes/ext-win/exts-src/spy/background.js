// Spy: what an ordinary extension sees of the shell (tabs/windows), notifications, cookies,
// and the windows it opens itself. Every observation goes to the report server.
const report = (event, data = {}) =>
  fetch("http://report.vsesvit.test/report", { method: "POST", body: JSON.stringify({ ext: "spy", event, t: Date.now(), ...data }) }).catch(() => {});
const ev = [];
const note = (e) => { ev.push({ t: Date.now(), ...e }); report("tabev", e); };
chrome.tabs.onCreated.addListener((t) => note({ what: "tabs.onCreated", id: t.id, windowId: t.windowId, url: t.url || t.pendingUrl, incognito: t.incognito }));
chrome.tabs.onRemoved.addListener((id, i) => note({ what: "tabs.onRemoved", id, windowId: i.windowId }));
chrome.windows.onCreated.addListener((w) => note({ what: "windows.onCreated", id: w.id, type: w.type, incognito: w.incognito }));
chrome.windows.onRemoved.addListener((id) => note({ what: "windows.onRemoved", id }));
chrome.notifications.onClicked.addListener((id) => report("notif", { what: "clicked", id }));
chrome.notifications.onClosed.addListener((id, byUser) => report("notif", { what: "closed", id, byUser }));
chrome.notifications.onButtonClicked.addListener((id, b) => report("notif", { what: "button", id, b }));
if (chrome.notifications.onPermissionLevelChanged) chrome.notifications.onPermissionLevelChanged.addListener((l) => report("notif", { what: "permissionLevel", level: l }));
self.snapshot = async () => {
  const tabs = await chrome.tabs.query({});
  const wins = await chrome.windows.getAll({ populate: true });
  return {
    tabs: tabs.map((t) => ({ id: t.id, windowId: t.windowId, url: t.url, active: t.active, incognito: t.incognito })),
    windows: wins.map((w) => ({ id: w.id, type: w.type, incognito: w.incognito, focused: w.focused, tabs: (w.tabs || []).map((t) => t.url) })),
    events: ev,
  };
};
self.notify = (id, title) =>
  new Promise((r) =>
    chrome.notifications.create(id, { type: "basic", iconUrl: "icon.png", title, message: "spy says hi", buttons: [{ title: "B1" }], requireInteraction: true }, (nid) =>
      r({ nid, err: chrome.runtime.lastError ? chrome.runtime.lastError.message : null })),
  );
self.permLevel = () => new Promise((r) => chrome.notifications.getPermissionLevel((l) => r(l)));
self.clearNotif = (id) => chrome.notifications.clear(id);
self.notifAll = () => chrome.notifications.getAll();
self.open = async (kind, url, opts = {}) => {
  try {
    if (kind === "tab") { const t = await chrome.tabs.create({ url, ...opts }); return { id: t.id, windowId: t.windowId, pendingUrl: t.pendingUrl, url: t.url, incognito: t.incognito }; }
    if (kind === "win") { const w = await chrome.windows.create({ url, ...opts }); return { id: w.id, type: w.type, incognito: w.incognito, tabs: (w.tabs || []).map((t) => t.id) }; }
    if (kind === "options") { await chrome.runtime.openOptionsPage(); return "ok"; }
    return "unknown kind";
  } catch (e) { return "ERR " + e.message; }
};
self.setCookie = async (url, name, value) => { try { const c = await chrome.cookies.set({ url, name, value }); return c ? { name: c.name, value: c.value, domain: c.domain } : null; } catch (e) { return "ERR " + e.message; } };
self.getCookies = async (url) => (await chrome.cookies.getAll({ url })).map((c) => c.name + "=" + c.value);
self.incog = () => chrome.extension.inIncognitoContext;
report("sw-start", { incognito: chrome.extension.inIncognitoContext });
