// Runs in the main world of every top-level document before the page's scripts. It follows
// the tracks `getUserMedia` and `getDisplayMedia` hand the page, and exposes
// `__vsesvitCapture` for the shell's `ExecuteScript` calls: what the page captures right now,
// and `stop(source)`, which ends those tracks as a revoked device would (with `ended`).
//
// The page runs after it and may replace any built-in, so what it calls later is only what it
// saved before the page ran: those built-ins alone are protected. Its lists are objects without
// a prototype, walked by index, so no array method, iterator or setter of the page's comes into
// play. The API is a frozen, non-writable property. Tracks are held weakly: a track the page
// dropped is the engine's to stop, as without this script.
(() => {
  if (window.__vsesvitCapture || window !== window.top) return;
  if (typeof MediaDevices !== "function" || typeof MediaStreamTrack !== "function") return;
  const apply = Reflect.apply;
  const devices = MediaDevices.prototype;
  const track = MediaStreamTrack.prototype;
  const stream = MediaStream.prototype;
  const readyState = Object.getOwnPropertyDescriptor(track, "readyState").get;
  const kind = Object.getOwnPropertyDescriptor(track, "kind").get;
  const stopTrack = track.stop;
  const cloneTrack = track.clone;
  const getTracks = stream.getTracks;
  const cloneStream = stream.clone;
  const dispatch = EventTarget.prototype.dispatchEvent;
  const then = Promise.prototype.then;
  const deref = WeakRef.prototype.deref;
  const Weak = WeakRef;
  const PlainEvent = Event;

  /** By index, below `count`:
   *  { ref: WeakRef<MediaStreamTrack>, source: "camera" | "microphone" | "screen" } */
  let followed = { __proto__: null };
  let count = 0;
  const follow = (t, source) => {
    followed[count++] = { ref: new Weak(t), source };
  };
  const sourceOf = (t) => {
    for (let i = 0; i < count; i++) {
      if (apply(deref, followed[i].ref, []) === t) return followed[i].source;
    }
    return null;
  };
  const wrap = (name, screen) => {
    const original = devices[name];
    if (typeof original !== "function") return;
    devices[name] = function () {
      const opened = apply(original, this, arguments);
      return apply(then, opened, [(s) => {
        const tracks = apply(getTracks, s, []);
        for (let i = 0; i < tracks.length; i++) {
          const video = apply(kind, tracks[i], []) === "video";
          follow(tracks[i], screen ? "screen" : video ? "camera" : "microphone");
        }
        return s;
      }]);
    };
  };
  wrap("getUserMedia", false);
  wrap("getDisplayMedia", true);
  track.clone = function () {
    const copy = apply(cloneTrack, this, arguments);
    const source = sourceOf(this);
    if (source) follow(copy, source);
    return copy;
  };
  stream.clone = function () {
    const copy = apply(cloneStream, this, arguments);
    const from = apply(getTracks, this, []);
    const to = apply(getTracks, copy, []);
    for (let i = 0; i < from.length && i < to.length; i++) {
      const source = sourceOf(from[i]);
      if (source) follow(to[i], source);
    }
    return copy;
  };

  // Forgets the followed tracks that ended or were dropped, and lists the others by index, as
  // { track, source } below `length`.
  const live = () => {
    const kept = { __proto__: null };
    const out = { __proto__: null, length: 0 };
    for (let i = 0; i < count; i++) {
      const entry = followed[i];
      const t = apply(deref, entry.ref, []);
      if (!t || apply(readyState, t, []) !== "live") continue;
      kept[out.length] = entry;
      out[out.length++] = { track: t, source: entry.source };
    }
    followed = kept;
    count = out.length;
    return out;
  };
  const api = {
    state() {
      const state = { camera: false, microphone: false, screen: false };
      const tracks = live();
      for (let i = 0; i < tracks.length; i++) state[tracks[i].source] = true;
      return state;
    },
    stop(source) {
      // The page's `ended` listeners run inside the loop and may follow new tracks, which
      // `tracks` leaves out.
      const tracks = live();
      let stopped = 0;
      for (let i = 0; i < tracks.length; i++) {
        if (tracks[i].source !== source) continue;
        apply(stopTrack, tracks[i].track, []);
        apply(dispatch, tracks[i].track, [new PlainEvent("ended")]);
        stopped++;
      }
      return stopped;
    },
  };
  Object.defineProperty(window, "__vsesvitCapture", { value: Object.freeze(api) });
})();
