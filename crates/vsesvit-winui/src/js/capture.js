// Runs in the main world of every top-level document before the page's scripts. It follows
// the tracks `getUserMedia` and `getDisplayMedia` hand the page, and exposes
// `__vsesvitCapture` for the shell's `ExecuteScript` calls: what the page captures right now,
// and `stop(source)`, which ends those tracks as a revoked device would (with `ended`).
//
// The track and stream methods it calls later were taken before the page ran, and the API is a
// frozen, non-writable property, so the page cannot swap them out from under the shell. Tracks
// are held weakly: a track the page dropped is the engine's to stop, as without this script.
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

  /** { ref: WeakRef<MediaStreamTrack>, source: "camera" | "microphone" | "screen" } */
  const followed = [];
  const follow = (t, source) => followed.push({ ref: new Weak(t), source });
  const sourceOf = (t) => {
    const entry = followed.find((e) => apply(deref, e.ref, []) === t);
    return entry ? entry.source : null;
  };
  const wrap = (name, screen) => {
    const original = devices[name];
    if (typeof original !== "function") return;
    devices[name] = function () {
      const opened = apply(original, this, arguments);
      return apply(then, opened, [(s) => {
        for (const t of apply(getTracks, s, [])) {
          const video = apply(kind, t, []) === "video";
          follow(t, screen ? "screen" : video ? "camera" : "microphone");
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
    const sources = apply(getTracks, this, []).map(sourceOf);
    apply(getTracks, copy, []).forEach((t, i) => sources[i] && follow(t, sources[i]));
    return copy;
  };

  const live = () => {
    const out = [];
    for (let i = followed.length - 1; i >= 0; i--) {
      const t = apply(deref, followed[i].ref, []);
      if (t && apply(readyState, t, []) === "live") out.push([t, followed[i].source]);
      else followed.splice(i, 1);
    }
    return out;
  };
  const api = {
    state() {
      const state = { camera: false, microphone: false, screen: false };
      for (const [, source] of live()) state[source] = true;
      return state;
    },
    stop(source) {
      let stopped = 0;
      for (const [t, s] of live()) {
        if (s !== source) continue;
        apply(stopTrack, t, []);
        apply(dispatch, t, [new PlainEvent("ended")]);
        stopped++;
      }
      return stopped;
    },
  };
  Object.defineProperty(window, "__vsesvitCapture", { value: Object.freeze(api) });
})();
