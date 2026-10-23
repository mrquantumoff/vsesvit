// Vsesvit Agent: a shell-owned helper extension that runs chrome.management for the shell.
// The shell reaches it through agent.html (a tab) or this worker (CDP Runtime.evaluate).
const report = (data) =>
  fetch("http://127.0.0.1:8792/report", { method: "POST", body: JSON.stringify({ ext: "agent", t: Date.now(), ...data }) }).catch(() => {});
for (const ev of ["onInstalled", "onUninstalled", "onEnabled", "onDisabled"]) {
  chrome.management[ev].addListener((info) =>
    report({ event: "management", what: ev, target: typeof info === "string" ? info : { id: info.id, name: info.name, enabled: info.enabled, installType: info.installType, version: info.version } }),
  );
}
self.run = async (method, ...args) => {
  try {
    const r = await chrome.management[method](...args);
    report({ event: "run", method, args, ok: true, result: r ?? null });
    return { ok: true, result: r ?? null };
  } catch (e) {
    report({ event: "run", method, args, ok: false, error: String(e.message || e) });
    return { ok: false, error: String(e.message || e) };
  }
};
