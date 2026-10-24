// Reporter: tells the spike's report server (POST /report) what this extension sees on every
// service-worker start, install/update and command. NAME is the manifest name's last word.
const M = chrome.runtime.getManifest();
const NAME = M.name.split(" ").pop();
const VERSION = M.version;
const report = (event, data = {}) =>
  fetch("http://report.vsesvit.test/report", {
    method: "POST",
    body: JSON.stringify({ ext: NAME, version: VERSION, event, t: Date.now(), id: chrome.runtime.id, ...data }),
  }).catch(() => {});
const tryGet = (area) => area.get().catch((e) => "ERR " + (e.message || e));

const started = (async () => {
  const localBefore = await tryGet(chrome.storage.local);
  const syncBefore = await tryGet(chrome.storage.sync);
  const sessionBefore = await tryGet(chrome.storage.session);
  await report("sw-start", { localBefore, syncBefore, sessionBefore, incognito: chrome.extension.inIncognitoContext });
  const starts = (localBefore.starts || 0) + 1;
  await chrome.storage.local.set({ starts, firstVersion: localBefore.firstVersion || VERSION, ["ran-" + VERSION]: true });
  await chrome.storage.sync.set({ syncStarts: (syncBefore.syncStarts || 0) + 1 }).catch(() => {});
  await chrome.storage.session.set({ sessionStarts: (sessionBefore.sessionStarts || 0) + 1 }).catch(() => {});
})();

chrome.runtime.onInstalled.addListener((d) => report("onInstalled", { reason: d.reason, previousVersion: d.previousVersion }));
chrome.runtime.onStartup.addListener(() => report("onStartup"));
if (chrome.commands) chrome.commands.onCommand.addListener((name, tab) => report("command", { name, tab: tab && tab.id }));

chrome.runtime.onMessage.addListener((m, sender, respond) => {
  if (m?.type === "visit") {
    started.then(async () => {
      const { visits = 0 } = await chrome.storage.local.get("visits");
      await chrome.storage.local.set({ visits: visits + 1 });
      respond({ name: NAME, version: VERSION, visits: visits + 1, incognito: !!sender.tab?.incognito });
    });
    return true;
  }
  return false;
});

// For the update test: the shell (through CDP) or a test calls self.checkUpdate().
self.checkUpdate = async () => {
  const r = await chrome.runtime.requestUpdateCheck();
  await report("requestUpdateCheck", { result: r });
  return r;
};

// For the shell's checks over a page session: what this extension holds right now.
self.state = async () => ({ name: NAME, version: VERSION, local: await chrome.storage.local.get(), incognito: chrome.extension.inIncognitoContext });
