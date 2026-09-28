// Every document of an extension popup, to make it behave as in Chrome's popup view.
//
// WebView2 makes each web view its own window with one tab, so to the popup "the current window"
// is the popup itself, its active tab is the popup page, and tabs.getCurrent() finds a tab.
// Queries for the current or last focused window answer with the tabs of the browser window the
// popup was opened from instead (`opener` lists them in order, as the shell saw them), and
// getCurrent() finds none, as in Chrome.
//
// Chrome also keeps the popup sized to its content, and the view has that size before the popup's
// scripts get their first tab answers. Here the top document reports its size to the shell
// whenever it may have changed, and tab answers wait for the first resize (or a timeout).
((opener) => {
  const MIN = 25;
  const [MAX_WIDTH, MAX_HEIGHT] = [800, 600];
  const top = window === window.top && typeof chrome?.webview?.postMessage === "function";

  let fitted = Promise.resolve();
  if (top) {
    let resolveFitted;
    fitted = new Promise((resolve) => {
      resolveFitted = resolve;
      setTimeout(resolve, 1000);
    });
    let sent = "";
    const measure = () => {
      const root = document.documentElement;
      const old = root.style.width;
      root.style.width = "max-content";
      const width = Math.ceil(root.getBoundingClientRect().width);
      root.style.width = old;
      const height = Math.ceil(root.getBoundingClientRect().height);
      const clamp = (v, max) => Math.min(Math.max(v, MIN), max);
      return [clamp(width, MAX_WIDTH), clamp(height, MAX_HEIGHT)];
    };
    const matches = ([w, h]) => Math.abs(innerWidth - w) <= 1 && Math.abs(innerHeight - h) <= 1;
    let queued = false;
    const report = () => {
      if (queued) return;
      queued = true;
      requestAnimationFrame(() => {
        queued = false;
        const size = measure();
        mutations.takeRecords();
        if (matches(size)) resolveFitted();
        if (String(size) === sent) return;
        sent = String(size);
        chrome.webview.postMessage(JSON.stringify({ popupSize: size }));
      });
    };
    // Measuring changes the root's style for a moment; that is not a change to report.
    const mutations = new MutationObserver(report);
    addEventListener("resize", () => {
      if (sent && matches(sent.split(",").map(Number))) resolveFitted();
      report();
    });
    const observe = () => {
      const resized = new ResizeObserver(report);
      resized.observe(document.documentElement);
      resized.observe(document.body);
      mutations.observe(document.documentElement, {
        subtree: true,
        childList: true,
        attributes: true,
        characterData: true,
      });
      report();
    };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", observe, { once: true });
    else observe();
    addEventListener("load", report);
  }

  const tabs = globalThis.chrome?.tabs;
  if (typeof tabs?.query !== "function") return;
  const original = tabs.query;
  // The callback form, which extension pages of every manifest version have.
  const query = (info) => new Promise((resolve) => original.call(tabs, info ?? {}, (found) => resolve(found ?? [])));
  const CURRENT = -2; // chrome.windows.WINDOW_ID_CURRENT
  // A tab's place in its window, which only the shell knows.
  const PLACE = ["active", "highlighted", "index", "windowId", "currentWindow", "lastFocusedWindow"];
  const answer = (result, callback) => {
    if (typeof callback !== "function") return result;
    result.then(callback, () => callback());
  };

  const openerTabs = async (info) => {
    const rest = Object.fromEntries(Object.entries(info).filter(([k]) => !PLACE.includes(k)));
    const candidates = (await query(rest)).sort((a, b) => (b.lastAccessed ?? 0) - (a.lastAccessed ?? 0));
    const found = [];
    opener.forEach(({ url, active }, index) => {
      const at = candidates.findIndex((t) => t.url === url || t.pendingUrl === url);
      if (at < 0) return;
      const [tab] = candidates.splice(at, 1);
      found.push({ ...tab, active, highlighted: active, index });
    });
    return found.filter(
      (t) =>
        (info.active === undefined || t.active === info.active) &&
        (info.highlighted === undefined || t.highlighted === info.highlighted) &&
        (info.index === undefined || t.index === info.index),
    );
  };

  tabs.query = function (info, callback) {
    const here = info && (info.currentWindow === true || info.lastFocusedWindow === true || info.windowId === CURRENT);
    const result = fitted.then(() => (here && opener.length > 0 ? openerTabs(info) : query(info)));
    return answer(result, callback);
  };
  tabs.getCurrent = (callback) => answer(fitted.then(() => undefined), callback);
})
