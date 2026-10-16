chrome.runtime.sendMessage({ type: "whoami" }).then((r) => {
  if (window === top) document.documentElement.dataset.spyTab = JSON.stringify(r);
}, (e) => { document.documentElement.dataset.spyTab = "error " + e; });
chrome.runtime.onMessage.addListener((m, sender, respond) => {
  if (m === "ping-tab") respond({ pong: location.href, top: window === top });
});
