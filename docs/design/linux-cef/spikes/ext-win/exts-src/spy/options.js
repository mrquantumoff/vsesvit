const report = (event, data = {}) =>
  fetch("http://report.vsesvit.test/report", { method: "POST", body: JSON.stringify({ ext: "spy", event, t: Date.now(), ...data }) }).catch(() => {});
// permissions.request needs a user gesture: the shell evaluates req() with userGesture.
window.req = async (what) => {
  try {
    const ok = await chrome.permissions.request(what === "host" ? { origins: ["https://opt.example.org/*"] } : { permissions: ["bookmarks"] });
    report("perm", { what, ok });
    return ok;
  } catch (e) {
    report("perm", { what, err: e.message });
    return "ERR " + e.message;
  }
};
window.has = async () => chrome.permissions.getAll();
report("options-loaded", {});
