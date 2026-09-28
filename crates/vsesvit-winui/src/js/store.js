// Main world of the Chrome Web Store and Edge Add-ons. The stores install through
// chrome.webstorePrivate, which WebView2 gives only the Chrome Web Store and cannot complete (it
// has no install prompt), so installs and removals (and on Edge Add-ons, install state) go to the
// shell instead: as "vsesvit-store" DOM events to the isolated world, which forwards them with
// the origin they came from.
(() => {
  const store = { "chromewebstore.google.com": "chrome", "microsoftedge.microsoft.com": "edge" }[location.hostname];
  if (!store || window !== window.top) return;
  const chrome = (window.chrome ??= {});
  const runtime = (chrome.runtime ??= {});

  let seq = 0;
  const pending = new Map();
  addEventListener("vsesvit-store-reply", (e) => {
    const reply = JSON.parse(e.detail);
    pending.get(reply.seq)?.(reply);
    pending.delete(reply.seq);
  });
  const ask = (op, args = {}) =>
    new Promise((resolve) => {
      const id = ++seq;
      pending.set(id, resolve);
      document.dispatchEvent(new CustomEvent("vsesvit-store", { detail: JSON.stringify({ ...args, seq: id, op }) }));
    });

  // Chrome's convention: a callback sees chrome.runtime.lastError; without one, a promise.
  const answer = (callback, error, value) => {
    if (typeof callback !== "function") {
      return error ? Promise.reject(new Error(error)) : Promise.resolve(value);
    }
    if (!error) return void callback(value);
    Object.defineProperty(runtime, "lastError", { value: { message: error }, configurable: true });
    try {
      callback(value);
    } finally {
      delete runtime.lastError;
    }
  };
  const lastArg = (args) => (typeof args[args.length - 1] === "function" ? args[args.length - 1] : undefined);

  const listeners = () => {
    const set = new Set();
    return {
      set,
      addListener: (f) => set.add(f),
      removeListener: (f) => set.delete(f),
      hasListener: (f) => set.has(f),
    };
  };
  // WebView2's own chrome.management, on the Chrome Web Store only, already lists what the engine
  // loaded; the store reads its install state from it. Elsewhere this stands in for it.
  const emulated = !chrome.management;
  const management = (chrome.management ??= {});
  const installed = listeners();
  const uninstalled = listeners();
  const describe = (e) => ({ ...e, type: "extension", installType: "normal", mayDisable: true, isApp: false });

  const webstore = (chrome.webstorePrivate ??= {});
  webstore.beginInstallWithManifest3 = (details, callback) =>
    ask("install", { store, id: details.id, name: details.localizedName ?? "" }).then((r) => {
      if (r.ok) {
        if (emulated && r.installed) installed.set.forEach((f) => f(describe(r.installed)));
        return answer(callback, null, "");
      }
      return answer(callback, r.error, r.cancelled ? "user_cancelled" : "unknown_error");
    });
  // The install already happened when beginInstallWithManifest3 answered.
  webstore.completeInstall = (id, callback) => answer(callback, null);
  webstore.completeInstallWithCV = (id, cv, callback) => answer(callback, null);
  webstore.install = (...args) => answer(lastArg(args), "Installs start from the Add button");

  if (store === "edge") {
    webstore.getExtensionStatus = (id, manifest, callback) =>
      ask("list").then((r) => {
        const e = r.extensions.find((e) => e.id === id);
        return answer(lastArg([manifest, callback]), null, e ? (e.enabled ? "enabled" : "disabled") : "installable");
      });
    webstore.getFullChromeVersion = (callback) =>
      answer(callback, null, { version_number: navigator.userAgent.match(/Chrome\/([\d.]+)/)?.[1] ?? "" });
    webstore.isInIncognitoMode = (callback) => answer(callback, null, false);
  }

  if (emulated) {
    const list = () => ask("list").then((r) => r.extensions.map(describe));
    management.getAll = (callback) => list().then((all) => answer(callback, null, all));
    management.get = (id, callback) =>
      list().then((all) => {
        const e = all.find((e) => e.id === id);
        return answer(callback, e ? null : `No extension with id ${id}`, e);
      });
    management.onInstalled = installed;
    management.onUninstalled = uninstalled;
    management.onEnabled = listeners();
    management.onDisabled = listeners();
  }
  management.uninstall = (id, ...rest) =>
    ask("uninstall", { store, id }).then((r) => {
      if (emulated && r.ok) uninstalled.set.forEach((f) => f(id));
      return answer(lastArg(rest), r.ok ? null : r.error);
    });
})();
