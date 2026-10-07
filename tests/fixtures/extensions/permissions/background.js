// permissions.onAdded and onRemoved as [name, permissions]; the harness takes them with
// runtime.sendMessage("events"), cookies.onChanged as [removed, name, value] with "cookies",
// and with "unprompted" what a request as the background started, when no user acted, came to.
const events = [];
const cookies = [];
chrome.permissions.onAdded.addListener((p) => events.push(["onAdded", p]));
chrome.permissions.onRemoved.addListener((p) => events.push(["onRemoved", p]));
chrome.cookies.onChanged.addListener(({ removed, cookie }) => cookies.push([removed, cookie.name, cookie.value]));
const unprompted = chrome.permissions.request({ permissions: ["alarms"] }).then((granted) => granted, (e) => e.message);

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message === "events") sendResponse(events.splice(0));
  if (message === "cookies") sendResponse(cookies.splice(0));
  if (message === "unprompted") {
    unprompted.then(sendResponse);
    return true;
  }
});
