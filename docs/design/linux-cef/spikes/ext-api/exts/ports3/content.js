chrome.runtime.onConnect.addListener((port) => {
  port.onMessage.addListener((message) => port.postMessage({ echo: message, name: port.name }));
});

const port = chrome.runtime.connect({ name: "content" });
port.onMessage.addListener((message) => {
  document.documentElement.dataset.portsReply = message;
});
port.postMessage("ping");
