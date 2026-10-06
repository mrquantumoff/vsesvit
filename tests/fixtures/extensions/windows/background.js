// Every windows and tabs event as [name, ...arguments], a window or tab by its id; the
// harness takes them with runtime.sendMessage("events").
const events = [];
const record = (name) => (...args) => events.push([name, ...args.map((a) => (a && typeof a === "object" && "id" in a ? a.id : a))]);
for (const name of ["onCreated", "onRemoved", "onFocusChanged", "onBoundsChanged"]) chrome.windows[name].addListener(record("windows." + name));
for (const name of ["onCreated", "onRemoved", "onMoved", "onDetached", "onAttached"]) chrome.tabs[name].addListener(record("tabs." + name));

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message === "events") sendResponse(events.splice(0));
});
