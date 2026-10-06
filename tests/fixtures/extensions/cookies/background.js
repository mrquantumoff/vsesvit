// Every cookies.onChanged event, in order; the harness takes them with runtime.sendMessage("events").
const events = [];
chrome.cookies.onChanged.addListener(({ removed, cause, cookie }) => events.push([removed, cause, cookie.name, cookie.value, cookie.domain, cookie.storeId]));

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message === "events") sendResponse(events.splice(0));
});
