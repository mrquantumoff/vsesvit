const log = [];
const record = (...entry) => {
  log.push(entry);
  chrome.storage.local.set({ log: log.slice() });
};

chrome.notifications.onClicked.addListener((id) => record("clicked", id));
chrome.notifications.onButtonClicked.addListener((id, index) => record("button", id, index));
chrome.notifications.onClosed.addListener((id, byUser) => record("closed", id, byUser));
chrome.notifications.onPermissionLevelChanged.addListener((level) => record("permission", level));
