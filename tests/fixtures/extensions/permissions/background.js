// permissions.onAdded and onRemoved as [name, permissions]; the harness takes them with
// runtime.sendMessage("events"), and with "unprompted" what a request as the background
// started, when no user acted, came to.
const events = [];
chrome.permissions.onAdded.addListener((p) => events.push(["onAdded", p]));
chrome.permissions.onRemoved.addListener((p) => events.push(["onRemoved", p]));
const unprompted = chrome.permissions.request({ permissions: ["alarms"] }).then((granted) => granted, (e) => e.message);

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message === "events") sendResponse(events.splice(0));
  if (message === "unprompted") {
    unprompted.then(sendResponse);
    return true;
  }
});
