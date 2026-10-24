// Proves the helper loaded at all (e.g. with Enhanced Safe Browsing on), independent of the bridge page.
fetch("http://report.vsesvit.test/report", {
  method: "POST",
  body: JSON.stringify({ ext: "helper", event: "sw-start", id: chrome.runtime.id, version: chrome.runtime.getManifest().version, t: Date.now() }),
}).catch(() => {});
