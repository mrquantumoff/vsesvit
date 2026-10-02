(function (config) {
  // chrome.* / browser.* for one extension context. One function expression, which the
  // runtime applies to the context's configuration (protocol::bootstrap), so nothing is
  // declared in the global scope and the bootstrap can run more than once per world:
  //   { id, host, handler, token?, kind: "content" | "page", manifest,
  //     i18n: { locale, messages }, permissions, hostPermissions, optionsPage }
  // `host` is the extension's URL host (not the id for Gecko ids). Calls that need the
  // browser go through window.webkit.messageHandlers[handler] (a Promise-returning
  // postMessage); see src/protocol.rs for the wire format and what `token` is for.
  "use strict";
  const g = globalThis;
  if (g.__vsesvit) return;
  const isPage = config.kind === "page";
  const baseUrl = "chrome-extension://" + config.host + "/";
  const messageHandlers = g.webkit && g.webkit.messageHandlers;
  const handler = messageHandlers && messageHandlers[config.handler];

  function isTop() {
    try { return g.window === g.window.top; } catch (_) { return false; }
  }

  function post(method, args) {
    if (!handler) return Promise.reject(new Error("Vsesvit: the extension bridge is unavailable in this context"));
    let promise;
    try {
      promise = handler.postMessage({ m: method, a: args, u: String(g.location && g.location.href), top: isTop(), t: config.token });
    } catch (e) {
      return Promise.reject(e instanceof Error ? e : new Error(String(e)));
    }
    return promise.then(
      (reply) => (reply && typeof reply === "object" && "v" in reply) ? reply.v : undefined,
      (e) => { throw new Error(e && e.message ? e.message : String(e)); });
  }

  // --- events -------------------------------------------------------------------------
  const events = new Map();
  class ExtensionEvent {
    constructor(name) { this.name = name; this.listeners = new Set(); events.set(name, this); }
    addListener(fn) { if (typeof fn === "function") this.listeners.add(fn); }
    removeListener(fn) { this.listeners.delete(fn); }
    hasListener(fn) { return this.listeners.has(fn); }
    hasListeners() { return this.listeners.size > 0; }
    dispatch(...args) {
      for (const fn of Array.from(this.listeners)) {
        try { fn(...args); } catch (e) { console.error("Vsesvit: listener for " + this.name + " threw", e); }
      }
    }
  }

  // --- callback / promise duality -----------------------------------------------------
  let lastError = null;
  function settle(promise, callback) {
    if (typeof callback !== "function") return promise;
    promise.then(
      (value) => { lastError = null; try { callback(value); } finally { lastError = null; } },
      (e) => { lastError = { message: e && e.message ? e.message : String(e) }; try { callback(); } finally { lastError = null; } });
    return undefined;
  }
  // Chrome's callback form: a function as the last argument.
  function takeCallback(args) {
    return args.length && typeof args[args.length - 1] === "function" ? args.pop() : undefined;
  }
  // `mapArgs` turns the caller's arguments (callback removed) into the ones posted.
  function bridged(method, fixedArgs, mapArgs) {
    return function (...args) {
      const callback = takeCallback(args);
      if (mapArgs) args = mapArgs(args);
      return settle(post(method, fixedArgs ? fixedArgs.concat(args) : args), callback);
    };
  }
  function local(fn) {
    return function (...args) {
      const callback = takeCallback(args);
      let promise;
      try { promise = Promise.resolve(fn(...args)); } catch (e) { promise = Promise.reject(e); }
      return settle(promise, callback);
    };
  }

  // --- runtime ----------------------------------------------------------------------
  const runtime = {
    id: config.id,
    getURL: (path) => baseUrl + String(path == null ? "" : path).replace(/^\/+/, ""),
    getManifest: () => JSON.parse(JSON.stringify(config.manifest)),
    getPlatformInfo: local(() => ({ os: "linux", arch: "x86-64", nacl_arch: "x86-64" })),
    onMessage: new ExtensionEvent("runtime.onMessage"),
    onInstalled: new ExtensionEvent("runtime.onInstalled"),
    onStartup: new ExtensionEvent("runtime.onStartup"),
    onConnect: new ExtensionEvent("runtime.onConnect"),
    onSuspend: new ExtensionEvent("runtime.onSuspend"),
    connect() { throw new Error("runtime.connect is not supported by Vsesvit"); },
    sendMessage(...args) {
      const callback = takeCallback(args);
      // sendMessage(message), (message, options), (extensionId, message), (extensionId, message, options)
      let message = args[0], options = args[1];
      const looksLikeOptions = (o) => o && typeof o === "object" && Object.keys(o).every((k) => k === "includeTlsChannelId");
      if (args.length >= 3 || (args.length === 2 && typeof args[0] === "string" && !looksLikeOptions(args[1]))) {
        if (args[0] != null && args[0] !== config.id) {
          return settle(Promise.reject(new Error("Vsesvit: messaging other extensions is not supported")), callback);
        }
        message = args[1]; options = args[2];
      }
      return settle(post("runtime.sendMessage", [message === undefined ? null : message, options || null]), callback);
    },
  };
  Object.defineProperty(runtime, "lastError", { get: () => lastError, enumerable: true });

  // --- storage ----------------------------------------------------------------------
  function normalizeKeys(keys) {
    if (keys == null) return { keys: null, defaults: null };
    if (typeof keys === "string") return { keys: [keys], defaults: null };
    if (Array.isArray(keys)) return { keys: keys.map(String), defaults: null };
    if (typeof keys === "object") return { keys: Object.keys(keys), defaults: keys };
    throw new TypeError("storage keys must be a string, an array, an object or null");
  }
  function storageArea(name, quota) {
    const area = {
      get(keys, callback) {
        if (typeof keys === "function") { callback = keys; keys = null; }
        let spec;
        try { spec = normalizeKeys(keys); } catch (e) { return settle(Promise.reject(e), callback); }
        const promise = post("storage.get", [name, spec.keys]).then((items) => {
          const out = Object.assign({}, items || {});
          if (spec.defaults) for (const k of Object.keys(spec.defaults)) if (!(k in out)) out[k] = spec.defaults[k];
          return out;
        });
        return settle(promise, callback);
      },
      set: bridged("storage.set", [name]),
      remove(keys, callback) {
        const list = Array.isArray(keys) ? keys.map(String) : [String(keys)];
        return settle(post("storage.remove", [name, list]), callback);
      },
      clear: bridged("storage.clear", [name]),
      getBytesInUse(keys, callback) {
        if (typeof keys === "function") { callback = keys; keys = null; }
        const list = keys == null ? null : (Array.isArray(keys) ? keys.map(String) : [String(keys)]);
        return settle(post("storage.getBytesInUse", [name, list]), callback);
      },
      onChanged: new ExtensionEvent("storage." + name + ".onChanged"),
    };
    Object.assign(area, quota);
    return area;
  }
  const storage = {
    local: storageArea("local", { QUOTA_BYTES: 10485760 }),
    sync: storageArea("sync", { QUOTA_BYTES: 102400, QUOTA_BYTES_PER_ITEM: 8192, MAX_ITEMS: 512, MAX_WRITE_OPERATIONS_PER_HOUR: 1800, MAX_WRITE_OPERATIONS_PER_MINUTE: 120 }),
    onChanged: new ExtensionEvent("storage.onChanged"),
  };

  // --- i18n --------------------------------------------------------------------------
  const messages = (config.i18n && config.i18n.messages) || {};
  const uiLocale = (config.i18n && config.i18n.locale) || "en";
  const rtl = /^(ar|fa|he|iw|ps|sd|ug|ur|yi|dv|ckb)(_|$)/i.test(uiLocale);
  // Chrome's predefined messages; `@@extension_id` is the URL host, which is what
  // extensions build `chrome-extension://` URLs from.
  const predefined = {
    "@@extension_id": config.host,
    "@@ui_locale": uiLocale,
    "@@bidi_dir": rtl ? "rtl" : "ltr",
    "@@bidi_reversed_dir": rtl ? "ltr" : "rtl",
    "@@bidi_start_edge": rtl ? "right" : "left",
    "@@bidi_end_edge": rtl ? "left" : "right",
  };
  function getMessage(name, substitutions) {
    const key = String(name).toLowerCase();
    if (Object.prototype.hasOwnProperty.call(predefined, key)) return predefined[key];
    const entry = messages[key];
    if (!entry || typeof entry.message !== "string") return "";
    const subs = substitutions == null ? [] : (Array.isArray(substitutions) ? substitutions : [substitutions]);
    const sub = (ref) => {
      const n = parseInt(ref, 10);
      return n >= 1 && n <= 9 ? String(subs[n - 1] == null ? "" : subs[n - 1]) : "";
    };
    const placeholders = entry.placeholders || {};
    const byName = {};
    for (const k of Object.keys(placeholders)) byName[k.toLowerCase()] = String(placeholders[k].content == null ? "" : placeholders[k].content);
    // One pass over the template, as in Chrome, so substituted text is never rescanned.
    // Only the message's own placeholder names are `$NAME$`, so `$1$` is still `$1`.
    const expand = (s) => s.replace(/\$([1-9])|\$\$/g, (m, d) => d ? sub(d) : "$");
    const names = Object.keys(byName).filter((n) => /^[a-z0-9_@]+$/.test(n));
    const token = new RegExp((names.length ? "\\$(" + names.join("|") + ")\\$|" : "()") + "\\$([1-9])|\\$\\$", "gi");
    return entry.message.replace(token, (m, ph, d) => ph ? expand(byName[ph.toLowerCase()]) : d ? sub(d) : "$");
  }
  const i18n = {
    getMessage,
    getUILanguage: () => uiLocale.replace("_", "-"),
    getAcceptLanguages: local(() => [uiLocale.replace("_", "-")]),
    detectLanguage: local(() => ({ isReliable: false, languages: [] })),
  };

  // --- permissions (answered from the manifest) --------------------------------------
  const grantedPermissions = new Set(config.permissions || []);
  const grantedOrigins = config.hostPermissions || [];
  // A requested origin pattern is granted when a host permission covers every URL it
  // matches. As in patterns.rs, `<all_urls>` leaves `file:` out and a `file:` pattern
  // covers nothing: local files need a file-access grant this runtime does not offer.
  const webSchemes = ["http", "https", "ws", "wss"];
  function parsePattern(s) {
    if (s === "<all_urls>") return { schemes: webSchemes.concat("ftp"), host: "*", port: "*", path: "/*" };
    const m = /^(\*|[a-z][a-z0-9+.-]*):\/\/([^/]*)(\/.*)$/.exec(String(s));
    if (!m || m[1] === "file") return null;
    const hp = /^(.*?)(?::(\*|\d+))?$/.exec(m[2].toLowerCase());
    return { schemes: m[1] === "*" ? webSchemes : [m[1]], host: hp[1], port: hp[2] || "*", path: m[3] };
  }
  function hostCovers(g, r) {
    if (g === "*") return true;
    if (!g.startsWith("*.")) return g === r;
    const d = g.slice(2), h = r.startsWith("*.") ? r.slice(2) : r;
    return h === d || h.endsWith("." + d);
  }
  function covers(g, r) {
    const path = new RegExp("^" + g.path.split("*").map((x) => x.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*") + "$");
    return r.schemes.every((s) => g.schemes.includes(s)) && hostCovers(g.host, r.host) && (g.port === "*" || g.port === r.port) && path.test(r.path);
  }
  function originGranted(pattern) {
    const r = parsePattern(pattern);
    return !!r && grantedOrigins.some((g) => { const gp = parsePattern(g); return !!gp && covers(gp, r); });
  }
  function contains(perms) {
    const p = (perms && perms.permissions) || [];
    const o = (perms && perms.origins) || [];
    return p.every((x) => grantedPermissions.has(x)) && o.every(originGranted);
  }
  const permissions = {
    contains: local(contains),
    getAll: local(() => ({ permissions: Array.from(grantedPermissions), origins: grantedOrigins.slice() })),
    request: local(contains),
    remove: local(() => false),
    onAdded: new ExtensionEvent("permissions.onAdded"),
    onRemoved: new ExtensionEvent("permissions.onRemoved"),
  };

  const api = { runtime, storage, i18n, permissions };

  // --- extension pages only -----------------------------------------------------------
  if (isPage) {
    runtime.openOptionsPage = bridged("runtime.openOptionsPage");
    runtime.getBackgroundPage = local(() => { throw new Error("runtime.getBackgroundPage is not supported by Vsesvit"); });
    runtime.reload = () => { post("runtime.reload", []).catch(() => {}); };

    // A classic MV3 service worker runs as the generated background page's script. The
    // page loads what the worker imports by string literal ahead of it (extension.rs), so
    // importScripts only checks that it did; anything else cannot load synchronously.
    const background = config.manifest && config.manifest.background;
    if (background && background.service_worker && background.type !== "module" && String(g.location && g.location.href) === baseUrl + "_generated_background_page.html") {
      const worker = new URL(String(background.service_worker), baseUrl);
      g.importScripts = function (...urls) {
        for (const u of urls) {
          const url = new URL(String(u), worker).href;
          if (!Array.from(g.document.scripts).some((s) => s.src === url)) {
            throw new Error("Vsesvit: importScripts(" + JSON.stringify(String(u)) + ") cannot load " + url + "; only scripts named by string literals in the extension's own files are imported");
          }
        }
      };
    }

    const tabs = {
      query: bridged("tabs.query"),
      get: bridged("tabs.get"),
      getCurrent: bridged("tabs.getCurrent"),
      create: bridged("tabs.create"),
      update: bridged("tabs.update", null, (args) => {
        const [tabId, props] = args.length >= 2 ? args : [null, args[0]];
        return [tabId, props || {}];
      }),
      remove: bridged("tabs.remove"),
      reload: bridged("tabs.reload", null, (args) => [typeof args[0] === "number" ? args[0] : null]),
      sendMessage(tabId, message, options, callback) {
        if (typeof options === "function") { callback = options; options = null; }
        return settle(post("tabs.sendMessage", [tabId, message === undefined ? null : message, options || null]), callback);
      },
      onUpdated: new ExtensionEvent("tabs.onUpdated"),
      onActivated: new ExtensionEvent("tabs.onActivated"),
      onRemoved: new ExtensionEvent("tabs.onRemoved"),
      onCreated: new ExtensionEvent("tabs.onCreated"),
      TAB_ID_NONE: -1,
    };
    const scripting = {
      executeScript(injection, callback) {
        const copy = Object.assign({}, injection);
        if (typeof copy.func === "function") copy.func = String(copy.func);
        if (typeof copy.function === "function") { copy.func = String(copy.function); delete copy.function; }
        return settle(post("scripting.executeScript", [copy]), callback);
      },
      insertCSS: bridged("scripting.insertCSS"),
      removeCSS: local(() => undefined),
    };
    const action = {
      setBadgeText: bridged("action.setBadgeText"),
      getBadgeText: bridged("action.getBadgeText"),
      setTitle: bridged("action.setTitle"),
      getTitle: bridged("action.getTitle"),
      setIcon: bridged("action.setIcon"),
      setPopup: bridged("action.setPopup"),
      getPopup: bridged("action.getPopup"),
      setBadgeBackgroundColor: bridged("action.noop"),
      getBadgeBackgroundColor: local(() => [0, 0, 0, 0]),
      setBadgeTextColor: bridged("action.noop"),
      enable: bridged("action.noop"),
      disable: bridged("action.noop"),
      isEnabled: local(() => true),
      onClicked: new ExtensionEvent("action.onClicked"),
    };
    const alarmName = (args) => [String(args[0] == null ? "" : args[0])];
    const alarms = {
      create: bridged("alarms.create", null, (args) => {
        const [name, info] = args.length >= 2 ? [args[0], args[1]] : ["", args[0]];
        return [String(name == null ? "" : name), info || {}];
      }),
      get: bridged("alarms.get", null, alarmName),
      getAll: bridged("alarms.getAll"),
      clear: bridged("alarms.clear", null, alarmName),
      clearAll: bridged("alarms.clearAll"),
      onAlarm: new ExtensionEvent("alarms.onAlarm"),
    };
    const currentWindow = () => ({ id: 1, focused: true, incognito: false, type: "normal", state: "normal", alwaysOnTop: false });
    const windows = {
      WINDOW_ID_NONE: -1,
      WINDOW_ID_CURRENT: -2,
      getCurrent: local(currentWindow),
      getLastFocused: local(currentWindow),
      getAll: local(() => [currentWindow()]),
      onFocusChanged: new ExtensionEvent("windows.onFocusChanged"),
      onCreated: new ExtensionEvent("windows.onCreated"),
      onRemoved: new ExtensionEvent("windows.onRemoved"),
    };
    Object.assign(api, {
      tabs, scripting, action, browserAction: action, alarms, windows,
      extension: { getURL: runtime.getURL, inIncognitoContext: false, getViews: () => [], getBackgroundPage: () => null },
    });
  }

  // --- runtime -> page entry points ---------------------------------------------------
  function dispatchMessage(message, sender) {
    const listeners = Array.from(runtime.onMessage.listeners);
    if (listeners.length === 0) return { none: true };
    return new Promise((resolve) => {
      let settled = false;
      let pending = 0;
      const respond = (value) => { if (!settled) { settled = true; resolve(value === undefined ? {} : { v: value }); } };
      const fail = (e) => { if (!settled) { settled = true; resolve({ e: e && e.message ? e.message : String(e) }); } };
      for (const fn of listeners) {
        let ret;
        try { ret = fn(message, sender, respond); } catch (e) { console.error("Vsesvit: runtime.onMessage listener threw", e); continue; }
        if (ret === true) pending++;
        else if (ret && typeof ret.then === "function") { pending++; ret.then(respond, fail); }
      }
      if (!settled && pending === 0) { settled = true; resolve({}); }
    });
  }

  function emit(name, ...args) {
    const ev = events.get(name);
    if (ev) ev.dispatch(...args);
    if (name === "storage.onChanged" && storage[args[1]]) storage[args[1]].onChanged.dispatch(args[0]);
  }

  Object.defineProperty(g, "__vsesvit", { value: Object.freeze({ dispatchMessage, emit, id: config.id, kind: config.kind }), configurable: false, enumerable: false });
  g.chrome = api;
  g.browser = api;
})
