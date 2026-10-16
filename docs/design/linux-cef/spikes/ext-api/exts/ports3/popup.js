const port = chrome.runtime.connect({ name: "popup" });
port.onMessage.addListener((message) => {
  document.title = "ports-popup:" + message;
});
port.postMessage("ping");
