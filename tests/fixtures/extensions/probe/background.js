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
