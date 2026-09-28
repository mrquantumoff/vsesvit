// Runs in the main world of every document before the page's scripts. It keeps the handlers
// the page gives `navigator.mediaSession`, so the sidebar player can skip tracks, and exposes
// `__vsesvitMedia` for the shell's `ExecuteScript` calls: the page's playback state, its
// play/pause and track actions, and the picture-in-picture presentation, which shows the playing
// video over the whole (small) page while the tab is in the sidebar.
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
  let shown = null;
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
      hide();
      if (!on) return false;
      const video = current();
      if (!(video instanceof HTMLVideoElement) || !video.videoWidth || video.hasAttribute("popover")) return false;
      document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
      video.setAttribute("data-vsesvit-pip", "");
      video.setAttribute("popover", "manual");
      try { video.showPopover(); } catch (e) { hide(); return false; }
      shown = video;
      return true;
    },
  };
  Object.defineProperty(window, "__vsesvitMedia", { value: Object.freeze(api) });
})();
