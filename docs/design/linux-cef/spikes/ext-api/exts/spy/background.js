// spike-ext-api spy: records what the browser tells an extension. Read with CDP:
// Runtime.evaluate in this worker (globals: ev, whoami, cmds, clicks, alarms, report()).
self.ev = [];
self.whoami = [];
const t0 = Date.now();
const rec = (name) => (...args) => ev.push([Date.now() - t0, name, ...args.map((a) => {
  if (a && typeof a === "object") {
    if ("tabs" in a || "type" in a && "focused" in a) return { id: a.id, type: a.type, focused: a.focused, state: a.state };
    if ("index" in a && "windowId" in a) return { id: a.id, windowId: a.windowId, index: a.index, active: a.active, url: a.url, status: a.status };
  }
  return a;
})]);
for (const n of ["onCreated", "onRemoved", "onFocusChanged", "onBoundsChanged"]) chrome.windows[n].addListener(rec("windows." + n));
for (const n of ["onCreated", "onActivated", "onHighlighted", "onRemoved", "onMoved", "onDetached", "onAttached", "onReplaced"]) chrome.tabs[n].addListener(rec("tabs." + n));
chrome.tabs.onUpdated.addListener((id, change, tab) => { if (change.status || change.url || change.title) ev.push([Date.now() - t0, "tabs.onUpdated", id, change]); });

chrome.runtime.onMessage.addListener((m, sender, respond) => {
  if (m?.type === "whoami") {
    whoami.push({ url: sender.url, frameId: sender.frameId, documentId: sender.documentId, origin: sender.origin,
      tab: sender.tab && { id: sender.tab.id, windowId: sender.tab.windowId, index: sender.tab.index, active: sender.tab.active, url: sender.tab.url } });
    respond({ tab: sender.tab?.id, windowId: sender.tab?.windowId, frameId: sender.frameId });
    return false;
  }
  if (m?.type === "popup-report") { self.popupReports = (self.popupReports || []).concat([m.data]); respond(true); return false; }
  return false;
});

self.cmds = [];
chrome.commands.onCommand.addListener(async (name, tab) => {
  const entry = { at: Date.now() - t0, name, tab: tab && { id: tab.id, windowId: tab.windowId, url: tab.url } };
  if (tab) entry.inject = await tryInject(tab.id);
  cmds.push(entry);
});

self.tryInject = async (tabId) => {
  try {
    const r = await chrome.scripting.executeScript({ target: { tabId }, func: () => location.href });
    return { ok: true, result: r[0]?.result };
  } catch (e) { return { ok: false, error: String(e.message || e) }; }
};

self.clicks = [];
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({ id: "spy-parent", title: "Spy parent", contexts: ["page", "link", "selection"] });
  chrome.contextMenus.create({ id: "spy-child", parentId: "spy-parent", title: "Spy child for %s", contexts: ["selection"] });
  chrome.contextMenus.create({ id: "spy-child2", parentId: "spy-parent", title: "Spy child page", contexts: ["page", "link"] });
  chrome.contextMenus.create({ id: "spy-radio1", parentId: "spy-parent", type: "radio", title: "Spy radio 1", checked: true, contexts: ["page"] });
  chrome.contextMenus.create({ id: "spy-radio2", parentId: "spy-parent", type: "radio", title: "Spy radio 2", contexts: ["page"] });
  chrome.contextMenus.create({ id: "spy-action", title: "Spy action item", contexts: ["action"] });
});
chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const entry = { at: Date.now() - t0, info, tab: tab && { id: tab.id, windowId: tab.windowId, url: tab.url } };
  if (tab) entry.inject = await tryInject(tab.id);
  clicks.push(entry);
});

self.alarms = [];
chrome.alarms.onAlarm.addListener((a) => alarms.push({ at: Date.now() - t0, name: a.name }));

// One call that answers most tab/window-model questions.
self.report = async () => {
  const q = async (f) => { try { return await f(); } catch (e) { return "ERR " + (e.message || e); } };
  const tabs = (l) => l.map((t) => ({ id: t.id, windowId: t.windowId, index: t.index, active: t.active, highlighted: t.highlighted, url: t.url, title: t.title }));
  return {
    all: await q(async () => tabs(await chrome.tabs.query({}))),
    activeCurrent: await q(async () => tabs(await chrome.tabs.query({ active: true, currentWindow: true }))),
    activeLastFocused: await q(async () => tabs(await chrome.tabs.query({ active: true, lastFocusedWindow: true }))),
    active: await q(async () => tabs(await chrome.tabs.query({ active: true }))),
    normal: await q(async () => tabs(await chrome.tabs.query({ windowType: "normal" }))),
    windows: await q(async () => (await chrome.windows.getAll({ populate: true })).map((w) => ({ id: w.id, type: w.type, focused: w.focused, state: w.state, tabs: w.tabs.length, bounds: [w.left, w.top, w.width, w.height] }))),
    current: await q(async () => { const w = await chrome.windows.getCurrent(); return { id: w.id, type: w.type, focused: w.focused }; }),
    lastFocused: await q(async () => { const w = await chrome.windows.getLastFocused(); return { id: w.id, type: w.type, focused: w.focused }; }),
  };
};
