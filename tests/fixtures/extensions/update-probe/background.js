// Answers "<running version>:<first version that ran>". The first version is kept in
// chrome.storage.local, so an update that keeps the extension's data answers "2.0:1.0".
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message !== "update-probe") return false;
  const version = chrome.runtime.getManifest().version;
  chrome.storage.local.get("first").then(async ({ first }) => {
    if (first === undefined) {
      first = version;
      await chrome.storage.local.set({ first });
    }
    sendResponse(version + ":" + first);
  });
  return true;
});
