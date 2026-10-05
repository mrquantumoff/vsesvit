const log = [];
window.portsLog = () => log.slice();

chrome.runtime.onConnect.addListener((port) => {
  log.push("connect:" + port.name + ":" + (port.sender.tab ? "tab" : "page"));
  port.onMessage.addListener((message) => {
    if (message === "ping") port.postMessage("pong");
  });
  port.onDisconnect.addListener(() => log.push("disconnect:" + port.name));
});

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (message === "log") respond(log);
  if (message === "background-page") {
    chrome.runtime.getBackgroundPage((page) => respond(page === window && chrome.extension.getBackgroundPage() === window));
    return true;
  }
  return false;
});

chrome.runtime.onMessageExternal.addListener((message, sender, respond) => {
  respond({ external: message, from: sender.id });
});

chrome.runtime.onConnectExternal.addListener((port) => {
  port.onMessage.addListener((message) => port.postMessage({ echo: message, from: port.sender.id, name: port.name }));
});
