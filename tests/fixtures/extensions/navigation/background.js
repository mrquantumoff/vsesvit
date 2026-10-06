// Every webNavigation event and every new tab, in order, and what two filtered listeners
// heard; the harness takes them with runtime.sendMessage("events").
const events = [];
const filtered = { page2: [], localhost: [] };
const names = ["onBeforeNavigate", "onCommitted", "onDOMContentLoaded", "onCompleted", "onErrorOccurred", "onCreatedNavigationTarget", "onReferenceFragmentUpdated", "onHistoryStateUpdated"];
for (const name of names) chrome.webNavigation[name].addListener((details) => events.push([name, details]));
chrome.tabs.onCreated.addListener((tab) => events.push(["tabs.onCreated", { tabId: tab.id }]));
chrome.webNavigation.onCompleted.addListener((d) => filtered.page2.push([d.frameId, d.url]), { url: [{ pathSuffix: "page2.html" }] });
chrome.webNavigation.onCompleted.addListener((d) => filtered.localhost.push([d.frameId, d.url]), { url: [{ hostEquals: "localhost" }, { urlMatches: "^nothing:" }] });

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message === "events") sendResponse({ events: events.splice(0), page2: filtered.page2.splice(0), localhost: filtered.localhost.splice(0) });
});
