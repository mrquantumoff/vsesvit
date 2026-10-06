(function () {
  // Tells the runtime about the document it runs in, in every frame of a tab, from a world
  // of its own (web_navigation.rs reads the reports). Only trusted events count: the page
  // shares the DOM and could dispatch look-alikes.
  "use strict";
  const g = globalThis;
  const handler = g.webkit && g.webkit.messageHandlers && g.webkit.messageHandlers.vsesvitFrames;
  if (!handler || g.__vsesvitFrames) return;
  g.__vsesvitFrames = true;
  const token = Array.from(g.crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, "0")).join("");
  // The frame's index among its parent's frames at each level, from the top. Windows of
  // other origins still compare and list their frames.
  function path() {
    const indices = [];
    for (let w = g.window; w !== w.parent; w = w.parent) {
      const siblings = w.parent.frames;
      let i = 0;
      while (i < siblings.length && siblings[i] !== w) i++;
      indices.unshift(i);
    }
    return indices;
  }
  function report(k, extra) {
    try {
      handler.postMessage(Object.assign({ k, d: token, path: path(), url: String(g.location.href) }, extra));
    } catch (_) {}
  }
  report("start");
  g.document.addEventListener("DOMContentLoaded", (e) => { if (e.isTrusted) report("ready"); });
  g.addEventListener("load", (e) => { if (e.isTrusted) report("load"); });
  g.addEventListener("pagehide", (e) => { if (e.isTrusted) report("gone"); });
  g.addEventListener("pageshow", (e) => { if (e.isTrusted && e.persisted) report("shown"); });
  if (g.navigation) {
    // The navigation API tells a fragment from a History API change, and a traversal.
    let same = null;
    g.navigation.addEventListener("navigate", (e) => {
      if (e.isTrusted && e.destination.sameDocument) same = { hash: e.hashChange, traverse: e.navigationType === "traverse" };
    });
    g.navigation.addEventListener("currententrychange", (e) => {
      if (!e.isTrusted || !same) return;
      report("same", same);
      same = null;
    });
  } else {
    g.addEventListener("hashchange", (e) => { if (e.isTrusted) report("same", { hash: true, traverse: false }); });
  }
})();
