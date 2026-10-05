let queue = Promise.resolve();
const log = (entry) => {
  queue = queue.then(async () => {
    const { log } = await chrome.storage.local.get({ log: [] });
    await chrome.storage.local.set({ log: log.concat([entry]) });
  });
};

chrome.runtime.onInstalled.addListener(() => {
  const created = (what) => () => log({ created: what, error: chrome.runtime.lastError ? chrome.runtime.lastError.message : null });
  chrome.contextMenus.create({ id: "parent", title: "Menus parent", contexts: ["all"] });
  chrome.contextMenus.create({ id: "link", parentId: "parent", title: "Link with %s", contexts: ["link"], targetUrlPatterns: ["*://*/page2.html"] });
  chrome.contextMenus.create({ id: "selection", parentId: "parent", title: "Find \u201c%s\u201d", contexts: ["selection"] });
  chrome.contextMenus.create({ id: "frame", title: "Menus frame", contexts: ["frame"], documentUrlPatterns: ["http://127.0.0.1/page2.html"] });
  chrome.contextMenus.create({ id: "check", type: "checkbox", title: "Menus check", checked: true });
  chrome.contextMenus.create({ id: "action", title: "Menus action", contexts: ["action"] });
  chrome.contextMenus.create({ id: "parent", title: "Again" }, created("duplicate"));
  chrome.contextMenus.create({ title: "Without an id" }, created("generated"));
});

chrome.contextMenus.onClicked.addListener((info, tab) => log({ clicked: info, tab: tab ? tab.id : null }));
