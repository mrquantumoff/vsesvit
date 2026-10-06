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

const PROBE_ICON = "data:image/svg+xml," + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" rx="12" fill="#3584e4"/></svg>');

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
