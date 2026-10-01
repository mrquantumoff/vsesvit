// Runs in the main world of every document before the page's scripts. It keeps the handlers
// the page gives `navigator.mediaSession`, so the sidebar player can skip tracks, and exposes
// `__vsesvitMedia` for the shell's `ExecuteScript` calls: the page's playback state, its
// play/pause and track actions, and the picture-in-picture presentation, which shows the playing
// video over the whole (small) page while the tab is in the sidebar. Sites rearrange their
// layout for the small page and may move or replace the video, which ends its presentation, so
// while it is wanted it is checked and shown again.
(() => {
  if (window.__vsesvitMedia || window !== window.top) return;
  const handlers = new Map();
  const session = navigator.mediaSession;
  if (session && typeof MediaSession === "function") {
    const setActionHandler = MediaSession.prototype.setActionHandler;
    MediaSession.prototype.setActionHandler = function (action, handler) {
      if (this === session) {
        if (typeof handler === "function") handlers.set(action, handler);
        else handlers.delete(action);
      }
      return setActionHandler.call(this, action, handler);
    };
  }
  const media = () => Array.from(document.querySelectorAll("video, audio"));
  const current = () => {
    const all = media();
    return all.find((m) => !m.paused && !m.ended)
      || all.find((m) => m.currentTime > 0 && !m.ended)
      || all.find((m) => m instanceof HTMLVideoElement && m.readyState > 0)
      || null;
  };
  const call = (action) => {
    const handler = handlers.get(action);
    if (!handler) return false;
    try { handler({ action }); } catch (e) { /* the page's own error */ }
    return true;
  };
  const sheet = new CSSStyleSheet();
  sheet.replaceSync(`
    video[data-vsesvit-pip]:popover-open {
      position: fixed !important; inset: 0 !important; width: 100vw !important; height: 100vh !important;
      max-width: none !important; max-height: none !important; margin: 0 !important; padding: 0 !important;
      border: 0 !important; transform: none !important; object-fit: contain !important; background: black !important;
    }
    html:has(video[data-vsesvit-pip]) { overflow: hidden !important; }`);
  // The largest artwork of the media session, as an absolute http(s) address.
  const artwork = (meta) => {
    const size = (a) => Math.max(0, ...String(a.sizes || "").split(/\s+/).map((s) => parseInt(s, 10) || 0));
    const best = Array.from((meta && meta.artwork) || []).sort((a, b) => size(b) - size(a))[0];
    try {
      const url = best && new URL(best.src, location.href);
      return url && (url.protocol === "https:" || url.protocol === "http:") ? url.href : "";
    } catch (e) {
      return "";
    }
  };
  let shown = null;
  let wanted = false;
  let checking = 0;
  const hide = () => {
    if (!shown) return;
    const video = shown;
    shown = null;
    try { video.hidePopover(); } catch (e) { /* already hidden */ }
    video.removeAttribute("popover");
    video.removeAttribute("data-vsesvit-pip");
    document.adoptedStyleSheets = document.adoptedStyleSheets.filter((s) => s !== sheet);
  };
  const api = {
    state() {
      const m = current();
      const meta = session && session.metadata;
      return {
        playing: !!m && !m.paused,
        title: (meta && meta.title) || "",
        artist: (meta && meta.artist) || "",
        video: m instanceof HTMLVideoElement && m.videoWidth > 0,
        previous: handlers.has("previoustrack"),
        next: handlers.has("nexttrack"),
        artwork: artwork(meta),
      };
    },
    act(action) {
      const m = current();
      if (action === "playpause") {
        const pause = m ? !m.paused : !!session && session.playbackState === "playing";
        if (call(pause ? "pause" : "play")) return;
        if (m && pause) m.pause();
        else if (m) m.play().catch(() => {});
        return;
      }
      call(action);
    },
    pip(on) {
      wanted = on;
      clearInterval(checking);
      hide();
      if (!on) return false;
      checking = setInterval(keep, 500);
      return show();
    },
  };
  const show = () => {
    const video = current();
    if (!(video instanceof HTMLVideoElement) || !video.videoWidth || video.hasAttribute("popover")) return false;
    if (!document.adoptedStyleSheets.includes(sheet)) document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
    video.setAttribute("data-vsesvit-pip", "");
    video.setAttribute("popover", "manual");
    // Before showing, so that hide() undoes all of the above if showPopover throws (as it does
    // for a fullscreen video).
    shown = video;
    try { video.showPopover(); } catch (e) { hide(); return false; }
    return true;
  };
  // Shows the playing video again if the page moved it (which closes a popover), replaced it,
  // or dropped the style sheet.
  const keep = () => {
    if (!wanted) return;
    const video = current();
    const intact = shown && shown === video && shown.isConnected && shown.matches(":popover-open")
      && document.adoptedStyleSheets.includes(sheet);
    if (intact) return;
    hide();
    show();
  };
  addEventListener("resize", () => { if (wanted) setTimeout(keep, 50); });
  Object.defineProperty(window, "__vsesvitMedia", { value: Object.freeze(api) });
})();
