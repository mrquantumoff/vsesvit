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

chrome.runtime.onConnect.addListener((port) => {
  port.onMessage.addListener((message) => {
    if (message === "ping") port.postMessage("pong:" + port.name);
  });
});
