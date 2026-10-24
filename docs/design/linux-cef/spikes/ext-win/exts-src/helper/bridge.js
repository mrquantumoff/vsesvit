// The shell's two-way channel (design §5.3): shell -> helper is Runtime.evaluate of
// vsesvitBridge.call(<json>) on this page's session; helper -> shell is console.debug lines
// prefixed "vsesvit-bridge ", read from this browser's console only.
const me = chrome.runtime.id;
const emit = (o) => console.debug("vsesvit-bridge " + JSON.stringify(o));
const info = (i) => ({
  id: i.id, name: i.name, version: i.version, enabled: i.enabled, disabledReason: i.disabledReason || null,
  installType: i.installType, permissions: i.permissions, hostPermissions: i.hostPermissions, mayDisable: i.mayDisable, type: i.type,
});
chrome.management.onInstalled.addListener((i) => i.id !== me && emit({ event: "installed", info: info(i) }));
chrome.management.onUninstalled.addListener((id) => id !== me && emit({ event: "uninstalled", id }));
chrome.management.onEnabled.addListener((i) => i.id !== me && emit({ event: "enabled", id: i.id }));
chrome.management.onDisabled.addListener((i) => i.id !== me && emit({ event: "disabled", id: i.id, reason: i.disabledReason || null }));
window.vsesvitBridge = {
  async call(req) {
    switch (req.op) {
      case "ping": return { pong: me };
      case "list": return (await chrome.management.getAll()).filter((i) => i.id !== me).map(info);
      case "get": return info(await chrome.management.get(req.id));
      case "setEnabled": await chrome.management.setEnabled(req.id, req.enabled); return { ok: true };
      case "emitTest": emit({ event: "test", n: req.n }); return { ok: true };
      default: throw new Error("unknown op " + req.op);
    }
  },
};
emit({ event: "ready", id: me });
