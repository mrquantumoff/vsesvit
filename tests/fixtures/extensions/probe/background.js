chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type !== "hello") return false;
  chrome.storage.local.get({ visits: 0 }).then(({ visits }) => {
    const next = visits + 1;
    chrome.storage.local.set({ visits: next }).then(() => {
      sendResponse({ ok: true, visits: next, fromTab: sender.tab ? true : false });
    });
  });
  return true;
});

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({ id: "probe-page", title: "Vsesvit Probe page item", contexts: ["page"] });
  chrome.contextMenus.create({ id: "probe-action", title: "Vsesvit Probe action item", contexts: ["action"] });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  const click = { id: info.menuItemId, pageUrl: info.pageUrl ?? null, tab: tab?.id ?? null, at: Date.now() };
  chrome.storage.local.set({ menuClick: click });
});

chrome.commands.onCommand.addListener((command, tab) => {
  chrome.storage.local.set({ command: { name: command, tab: tab?.id ?? null, url: tab?.url ?? null, at: Date.now() } });
});

// PNG: WebView2, like Chrome, cannot decode an SVG icon in a service worker.
const PROBE_ICON = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAGUlEQVR42mMwbXnynxLMMGrAqAGjBgwXAwDUdZwfsK+3EAAAAABJRU5ErkJggg==";

// The self-test asks through the content script, so a probe that merely loads shows nothing.
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type !== "notify") return false;
  const api = typeof chrome.notifications;
  if (api === "undefined") {
    sendResponse({ api });
    return false;
  }
  const options = { type: "basic", iconUrl: PROBE_ICON, title: "Vsesvit Probe notification", message: "Sent by the probe", buttons: [{ title: "Open" }] };
  chrome.notifications.create("probe-notification", options).then(
    async (id) => {
      if (!message.keep) await chrome.notifications.clear(id);
      sendResponse({ api, created: id });
    },
    (e) => sendResponse({ api, error: String(e?.message ?? e) }),
  );
  return true;
});

// uBlock Origin Lite's "no filtering on this site" is this rule, above all its static ones.
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type !== "dnr") return false;
  const dnr = chrome.declarativeNetRequest;
  if (typeof dnr?.updateDynamicRules !== "function") {
    sendResponse({ api: "undefined" });
    return false;
  }
  (async () => {
    if (message.want === "allow") {
      const condition = { requestDomains: [new URL(sender.url).hostname], resourceTypes: ["main_frame"] };
      await dnr.updateDynamicRules({ removeRuleIds: [1], addRules: [{ id: 1, priority: 2000000, action: { type: "allowAllRequests" }, condition }] });
    } else {
      await dnr.updateDynamicRules({ removeRuleIds: [1] });
    }
    return { rules: (await dnr.getDynamicRules()).map((r) => r.id) };
  })().then(sendResponse, (e) => sendResponse({ error: String(e?.message ?? e) }));
  return true;
});

// A dynamic content script for the page's host, as uBlock Origin Lite registers its cosmetic filters.
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type !== "scripts") return false;
  const scripting = chrome.scripting;
  if (typeof scripting?.registerContentScripts !== "function") {
    sendResponse({ api: "undefined" });
    return false;
  }
  (async () => {
    if (message.want === "register") {
      const matches = ["*://" + new URL(sender.url).hostname + "/*"];
      await scripting.registerContentScripts([{ id: "probe-dynamic", matches, js: ["dynamic.js"], persistAcrossSessions: false }]);
    } else {
      await scripting.unregisterContentScripts({ ids: ["probe-dynamic"] });
    }
    return { scripts: (await scripting.getRegisteredContentScripts()).map((s) => s.id) };
  })().then(sendResponse, (e) => sendResponse({ error: String(e?.message ?? e) }));
  return true;
});

if (chrome.notifications) {
  const notified = (event) => chrome.storage.local.set({ notification: { ...event, at: Date.now() } });
  chrome.notifications.onClicked.addListener((id) => notified({ event: "clicked", id }));
  chrome.notifications.onButtonClicked.addListener((id, button) => notified({ event: "button", id, button }));
  chrome.notifications.onClosed.addListener((id, byUser) => notified({ event: "closed", id, byUser }));
  chrome.notifications.onPermissionLevelChanged.addListener((level) => notified({ event: "level", level }));
}

chrome.runtime.onConnect.addListener((port) => {
  port.onMessage.addListener((message) => {
    if (message === "ping") port.postMessage("pong:" + port.name);
  });
});
