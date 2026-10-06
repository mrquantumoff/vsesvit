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
    // A port's events belong to the port, so only the API's own are reachable by name.
    constructor(name, named = true) { this.name = name; this.listeners = new Set(); if (named) events.set(name, this); }
    addListener(fn) { if (typeof fn === "function") this.listeners.add(fn); }
    removeListener(fn) { this.listeners.delete(fn); }
    hasListener(fn) { return this.listeners.has(fn); }
    hasListeners() { return this.listeners.size > 0; }
    accepts() { return true; }
    dispatch(...args) {
      for (const fn of Array.from(this.listeners)) {
        if (!this.accepts(fn, ...args)) continue;
        try { fn(...args); } catch (e) { console.error("Vsesvit: listener for " + this.name + " threw", e); }
      }
    }
  }
  // An event whose listener may name `{ url: [UrlFilter, ...] }`: it then hears only the
  // events whose `details.url` one of the filters matches. As Chrome documents for
  // webNavigation, a filter's `schemes` and `ports` are ignored.
  const urlTests = {
    hostContains: (u, v) => ("." + u.host).includes(v.toLowerCase()),
    hostEquals: (u, v) => u.host === v.toLowerCase(),
    hostPrefix: (u, v) => u.host.startsWith(v.toLowerCase()),
    hostSuffix: (u, v) => u.host.endsWith(v.toLowerCase()),
    pathContains: (u, v) => u.path.includes(v),
    pathEquals: (u, v) => u.path === v,
    pathPrefix: (u, v) => u.path.startsWith(v),
    pathSuffix: (u, v) => u.path.endsWith(v),
    queryContains: (u, v) => u.query.includes(v),
    queryEquals: (u, v) => u.query === v,
    queryPrefix: (u, v) => u.query.startsWith(v),
    querySuffix: (u, v) => u.query.endsWith(v),
    urlContains: (u, v) => u.url.includes(v),
    urlEquals: (u, v) => u.url === v,
    urlPrefix: (u, v) => u.url.startsWith(v),
    urlSuffix: (u, v) => u.url.endsWith(v),
    urlMatches: (u, v) => new RegExp(v).test(u.url),
    originAndPathMatches: (u, v) => new RegExp(v).test(u.url.split("?")[0]),
    schemes: () => true,
    ports: () => true,
  };
  // The URL without its fragment, and its parts, as Chrome matches them.
  function urlParts(href) {
    let parsed;
    try { parsed = new URL(href); } catch (_) { return null; }
    const url = parsed.href.split("#")[0];
    return { url, host: parsed.hostname, path: parsed.pathname, query: parsed.search.slice(1) };
  }
  class UrlFilteredEvent extends ExtensionEvent {
    constructor(name) { super(name); this.filters = new Map(); }
    addListener(fn, filters) {
      if (typeof fn !== "function") return;
      if (filters != null) {
        const ok = Array.isArray(filters.url) && filters.url.every((f) => f && typeof f === "object" && Object.keys(f).every((k) => k in urlTests));
        if (!ok) throw new TypeError("Error in invocation of " + this.name + ".addListener: filters.url must be a list of events.UrlFilter");
      }
      super.addListener(fn);
      this.filters.set(fn, filters == null ? null : filters.url);
    }
    removeListener(fn) { super.removeListener(fn); this.filters.delete(fn); }
    accepts(fn, details) {
      const filters = this.filters.get(fn);
      if (!filters) return true;
      const parts = urlParts(details && details.url);
      return !!parts && filters.some((f) => Object.keys(f).every((k) => urlTests[k](parts, String(f[k]))));
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
  function withLastError(message, fn) {
    lastError = message ? { message } : null;
    try { fn(); } finally { lastError = null; }
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

  // --- ports ------------------------------------------------------------------------
  // A port receives by keeping one `port.receive` call open, which the runtime answers
  // with what arrived meanwhile: [{ m: message } | { d: error-or-null }, ...].
  const ports = new Map();
  function newPortId() {
    const bytes = new Uint8Array(16);
    g.crypto.getRandomValues(bytes);
    return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  }
  function makePort(id, name, sender) {
    let open = true;
    const close = () => { open = false; ports.delete(id); };
    const port = {
      name,
      onMessage: new ExtensionEvent("Port.onMessage", false),
      onDisconnect: new ExtensionEvent("Port.onDisconnect", false),
      postMessage(message) {
        if (!open) throw new Error("Attempting to use a disconnected port object");
        post("port.postMessage", [id, message === undefined ? null : message]).catch(() => {});
      },
      disconnect() {
        if (!open) return;
        close();
        post("port.disconnect", [id]).catch(() => {});
      },
    };
    if (sender) port.sender = sender;
    ports.set(id, port);
    (async () => {
      while (open) {
        let received;
        try { received = await post("port.receive", [id]); } catch (_) { received = [{ d: null }]; }
        for (const event of received || []) {
          if (!open) return;
          if ("m" in event) { port.onMessage.dispatch(event.m, port); continue; }
          close();
          withLastError(event.d, () => port.onDisconnect.dispatch(port));
          return;
        }
      }
    })();
    return port;
  }
  // The runtime opens the connection before the port asks for its first events.
  function connectPort(method, args, name) {
    const id = newPortId();
    post(method, [id].concat(args)).catch(() => {});
    return makePort(id, name);
  }
  const portName = (info) => info && info.name != null ? String(info.name) : "";
  // Chrome closes the ports of a document that goes away, which the runtime cannot see
  // for a document in a tab.
  g.addEventListener("pagehide", () => { for (const port of Array.from(ports.values())) port.disconnect(); });
  // Another extension's id, or null for this one.
  const otherExtension = (id) => id != null && id !== config.id ? String(id) : null;

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
    // connect(), (connectInfo), (extensionId), (extensionId, connectInfo)
    connect(...args) {
      const [target, info] = typeof args[0] === "string" || args.length >= 2 ? args : [null, args[0]];
      const name = portName(info);
      return connectPort("runtime.connect", [otherExtension(target), name], name);
    },
    sendMessage(...args) {
      const callback = takeCallback(args);
      // sendMessage(message), (message, options), (extensionId, message), (extensionId, message, options)
      let message = args[0], options = args[1], target = null;
      const looksLikeOptions = (o) => o && typeof o === "object" && Object.keys(o).every((k) => k === "includeTlsChannelId");
      if (args.length >= 3 || (args.length === 2 && typeof args[0] === "string" && !looksLikeOptions(args[1]))) {
        target = otherExtension(args[0]);
        message = args[1]; options = args[2];
      }
      return settle(post("runtime.sendMessage", [message === undefined ? null : message, options || null, target]), callback);
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
  // The `onclick` functions this page gave contextMenus, by item id: Chrome calls one only
  // in the page that made the item.
  const menuClicks = new Map();
  const menuKey = (id) => typeof id + ":" + id;

  // --- extension pages only -----------------------------------------------------------
  if (isPage) {
    runtime.openOptionsPage = bridged("runtime.openOptionsPage");
    runtime.onMessageExternal = new ExtensionEvent("runtime.onMessageExternal");
    runtime.onConnectExternal = new ExtensionEvent("runtime.onConnectExternal");
    // A background page reaches its own window, and an action popup reaches the background
    // page that opened it (views.rs); other views have no way to the background's window.
    const background = config.manifest && config.manifest.background;
    const backgroundPageUrl = !background ? null
      : background.page ? new URL(String(background.page), baseUrl).href
      : background.scripts ? baseUrl + "_generated_background_page.html" : null;
    const isBackgroundPage = (w) => { try { return String(w.location.href).split(/[?#]/)[0] === backgroundPageUrl; } catch (_) { return false; } };
    const backgroundPage = () => !backgroundPageUrl ? null : isBackgroundPage(g) ? g : g.opener && isBackgroundPage(g.opener) ? g.opener : null;
    runtime.getBackgroundPage = local(() => {
      const page = backgroundPage();
      if (page) return page;
      throw new Error(backgroundPageUrl ? "Vsesvit: runtime.getBackgroundPage works only in the background page and action popups" : "You do not have a background page.");
    });
    runtime.reload = () => { post("runtime.reload", []).catch(() => {}); };

    // A classic MV3 service worker runs as the generated background page's script. The
    // page loads what the worker imports by string literal ahead of it (extension.rs), so
    // importScripts only checks that it did; anything else cannot load synchronously.
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
      move: bridged("tabs.move", null, (args) => [args[0], args[1] || {}]),
      reload: bridged("tabs.reload", null, (args) => [typeof args[0] === "number" ? args[0] : null]),
      sendMessage(tabId, message, options, callback) {
        if (typeof options === "function") { callback = options; options = null; }
        return settle(post("tabs.sendMessage", [tabId, message === undefined ? null : message, options || null]), callback);
      },
      connect(tabId, info) {
        const name = portName(info);
        return connectPort("tabs.connect", [tabId, name, info && info.frameId != null ? info.frameId : null], name);
      },
      onUpdated: new ExtensionEvent("tabs.onUpdated"),
      onActivated: new ExtensionEvent("tabs.onActivated"),
      onRemoved: new ExtensionEvent("tabs.onRemoved"),
      onCreated: new ExtensionEvent("tabs.onCreated"),
      onMoved: new ExtensionEvent("tabs.onMoved"),
      onDetached: new ExtensionEvent("tabs.onDetached"),
      onAttached: new ExtensionEvent("tabs.onAttached"),
      TAB_ID_NONE: -1,
    };
    const filter = (args) => [args[0] || null];
    const scripting = {
      executeScript(injection, callback) {
        const copy = Object.assign({}, injection);
        if (typeof copy.func === "function") copy.func = String(copy.func);
        if (typeof copy.function === "function") { copy.func = String(copy.function); delete copy.function; }
        return settle(post("scripting.executeScript", [copy]), callback);
      },
      insertCSS: bridged("scripting.insertCSS"),
      removeCSS: bridged("scripting.removeCSS"),
      registerContentScripts: bridged("scripting.registerContentScripts"),
      getRegisteredContentScripts: bridged("scripting.getRegisteredContentScripts", null, filter),
      updateContentScripts: bridged("scripting.updateContentScripts"),
      unregisterContentScripts: bridged("scripting.unregisterContentScripts", null, filter),
      ExecutionWorld: { ISOLATED: "ISOLATED", MAIN: "MAIN" },
      StyleOrigin: { AUTHOR: "AUTHOR", USER: "USER" },
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
    if (grantedPermissions.has("contextMenus") || grantedPermissions.has("menus")) {
      // Chrome returns a new item's id at once, so the shim makes the ids an extension leaves
      // out: from 1 in the background page, which makes nearly all of them, and from a random
      // base in other pages so that two pages' ids do not collide.
      let nextMenuId = isBackgroundPage(g) ? 1 : 1e6 + Math.floor(Math.random() * 1e9);
      const uncheckedError = (e) => console.error("Unchecked runtime.lastError: " + (e && e.message ? e.message : String(e)));
      const contextMenus = {
        ACTION_MENU_TOP_LEVEL_LIMIT: 6,
        ContextType: { ALL: "all", PAGE: "page", FRAME: "frame", SELECTION: "selection", LINK: "link", EDITABLE: "editable", IMAGE: "image", VIDEO: "video", AUDIO: "audio", LAUNCHER: "launcher", BROWSER_ACTION: "browser_action", PAGE_ACTION: "page_action", ACTION: "action" },
        ItemType: { NORMAL: "normal", CHECKBOX: "checkbox", RADIO: "radio", SEPARATOR: "separator" },
        create(props, callback) {
          const p = Object.assign({}, props);
          const onclick = typeof p.onclick === "function" ? p.onclick : null;
          delete p.onclick;
          const generated = p.id == null;
          if (generated) p.id = nextMenuId++;
          const id = p.id;
          const done = post("contextMenus.create", [p, generated, !!onclick]).then(() => { if (onclick) menuClicks.set(menuKey(id), onclick); });
          if (typeof callback === "function") settle(done, callback);
          else done.catch(uncheckedError);
          return id;
        },
        update(id, props, callback) {
          const p = Object.assign({}, props);
          const onclick = typeof p.onclick === "function" ? p.onclick : null;
          delete p.onclick;
          return settle(post("contextMenus.update", [id, p, !!onclick]).then(() => { if (onclick) menuClicks.set(menuKey(id), onclick); }), callback);
        },
        remove(id, callback) {
          return settle(post("contextMenus.remove", [id]).then(() => { menuClicks.delete(menuKey(id)); }), callback);
        },
        removeAll(callback) {
          return settle(post("contextMenus.removeAll", []).then(() => { menuClicks.clear(); }), callback);
        },
        onClicked: new ExtensionEvent("contextMenus.onClicked"),
      };
      // Firefox's name for the same API.
      Object.assign(api, { contextMenus, menus: contextMenus });
    }
    // Chrome gives the API only to an extension whose manifest declares commands.
    if (config.manifest && config.manifest.commands) {
      api.commands = { getAll: bridged("commands.getAll"), onCommand: new ExtensionEvent("commands.onCommand") };
    }
    if (grantedPermissions.has("notifications")) {
      // As in Chrome, every image of the options loads before the call goes out, and a
      // failure fails the call; the icon goes on as PNG, scaled down to fit 128x128.
      const loadImage = (url) => new Promise((resolve, reject) => {
        const image = new Image();
        image.onload = () => resolve(image);
        image.onerror = reject;
        image.src = new URL(String(url), g.location.href).href;
      });
      const iconPng = (image) => {
        const scale = Math.min(1, 128 / image.naturalWidth, 128 / image.naturalHeight);
        const canvas = g.document.createElement("canvas");
        canvas.width = Math.round(image.naturalWidth * scale);
        canvas.height = Math.round(image.naturalHeight * scale);
        canvas.getContext("2d").drawImage(image, 0, 0, canvas.width, canvas.height);
        return canvas.toDataURL("image/png").split(",")[1];
      };
      const withImages = async (id, options) => {
        const copy = Object.assign({}, options);
        const others = [copy.appIconMaskUrl, copy.imageUrl].concat(Array.isArray(copy.buttons) ? copy.buttons.map((b) => b && b.iconUrl) : []);
        try {
          const icon = copy.iconUrl == null ? null : loadImage(copy.iconUrl).then(iconPng);
          const [png] = await Promise.all([icon].concat(others.filter((u) => u != null).map(loadImage)));
          return [id, copy, png];
        } catch (_) {
          throw new Error("Unable to download all specified images.");
        }
      };
      api.notifications = {
        TemplateType: { BASIC: "basic", IMAGE: "image", LIST: "list", PROGRESS: "progress" },
        PermissionLevel: { GRANTED: "granted", DENIED: "denied" },
        // create(options), create(id, options), each with an optional callback.
        create(...args) {
          const callback = takeCallback(args);
          const [id, options] = typeof args[0] === "string" || args.length >= 2 ? args : [null, args[0]];
          return settle(withImages(id || g.crypto.randomUUID(), options).then((posted) => post("notifications.create", posted)), callback);
        },
        update(id, options, callback) {
          return settle(withImages(String(id), options).then((posted) => post("notifications.update", posted)), callback);
        },
        clear: bridged("notifications.clear", null, (args) => [String(args[0])]),
        getAll: bridged("notifications.getAll"),
        getPermissionLevel: bridged("notifications.getPermissionLevel"),
        onClicked: new ExtensionEvent("notifications.onClicked"),
        onButtonClicked: new ExtensionEvent("notifications.onButtonClicked"),
        onClosed: new ExtensionEvent("notifications.onClosed"),
        onPermissionLevelChanged: new ExtensionEvent("notifications.onPermissionLevelChanged"),
        onShowSettings: new ExtensionEvent("notifications.onShowSettings"),
      };
    }
    if (grantedPermissions.has("declarativeNetRequest") || grantedPermissions.has("declarativeNetRequestWithHostAccess")) {
      const options = (args) => [args[0] || {}];
      const enumOf = (values) => Object.fromEntries(values.map((v) => [v.replace(/[A-Z]/g, (c) => "_" + c).toUpperCase(), v]));
      api.declarativeNetRequest = Object.assign({
        updateDynamicRules: bridged("declarativeNetRequest.updateDynamicRules", null, options),
        getDynamicRules: bridged("declarativeNetRequest.getDynamicRules", null, options),
        updateSessionRules: bridged("declarativeNetRequest.updateSessionRules", null, options),
        getSessionRules: bridged("declarativeNetRequest.getSessionRules", null, options),
        updateEnabledRulesets: bridged("declarativeNetRequest.updateEnabledRulesets", null, options),
        getEnabledRulesets: bridged("declarativeNetRequest.getEnabledRulesets"),
        getAvailableStaticRuleCount: bridged("declarativeNetRequest.getAvailableStaticRuleCount"),
        isRegexSupported: bridged("declarativeNetRequest.isRegexSupported", null, options),
        // WebKit says nothing about the requests a content blocker matched, so there is no
        // count to show on the badge.
        setExtensionActionOptions: local(() => undefined),
        ResourceType: enumOf(["main_frame", "sub_frame", "stylesheet", "script", "image", "font", "object", "xmlhttprequest", "ping", "csp_report", "media", "websocket", "webtransport", "webbundle", "other"]),
        RequestMethod: enumOf(["connect", "delete", "get", "head", "options", "patch", "post", "put", "other"]),
        RuleActionType: enumOf(["block", "redirect", "allow", "upgradeScheme", "modifyHeaders", "allowAllRequests"]),
        DomainType: enumOf(["firstParty", "thirdParty"]),
        HeaderOperation: enumOf(["append", "set", "remove"]),
        UnsupportedRegexReason: enumOf(["syntaxError", "memoryLimitExceeded"]),
      }, config.dnr);
    }
    if (grantedPermissions.has("webNavigation")) {
      const details = (args) => [args[0] || {}];
      const values = (list) => Object.fromEntries(list.map((v) => [v.toUpperCase(), v]));
      api.webNavigation = {
        getFrame: bridged("webNavigation.getFrame", null, details),
        getAllFrames: bridged("webNavigation.getAllFrames", null, details),
        onBeforeNavigate: new UrlFilteredEvent("webNavigation.onBeforeNavigate"),
        onCommitted: new UrlFilteredEvent("webNavigation.onCommitted"),
        onDOMContentLoaded: new UrlFilteredEvent("webNavigation.onDOMContentLoaded"),
        onCompleted: new UrlFilteredEvent("webNavigation.onCompleted"),
        onErrorOccurred: new UrlFilteredEvent("webNavigation.onErrorOccurred"),
        onCreatedNavigationTarget: new UrlFilteredEvent("webNavigation.onCreatedNavigationTarget"),
        onReferenceFragmentUpdated: new UrlFilteredEvent("webNavigation.onReferenceFragmentUpdated"),
        onHistoryStateUpdated: new UrlFilteredEvent("webNavigation.onHistoryStateUpdated"),
        // Chrome fires it for a prerendered page swapped in, which WebKit has none of.
        onTabReplaced: new ExtensionEvent("webNavigation.onTabReplaced"),
        TransitionType: values(["link", "typed", "auto_bookmark", "auto_subframe", "manual_subframe", "generated", "start_page", "form_submit", "reload", "keyword", "keyword_generated"]),
        TransitionQualifier: values(["client_redirect", "server_redirect", "forward_back", "from_address_bar"]),
      };
    }
    // In extension pages only, as Chrome's default access level has it.
    storage.session = Object.assign(storageArea("session", { QUOTA_BYTES: 10485760 }), { setAccessLevel: local(() => undefined) });
    const queryOptions = (args) => [args[0] || {}];
    const windows = {
      WINDOW_ID_NONE: -1,
      WINDOW_ID_CURRENT: -2,
      WindowType: { NORMAL: "normal", POPUP: "popup", PANEL: "panel", APP: "app", DEVTOOLS: "devtools" },
      WindowState: { NORMAL: "normal", MINIMIZED: "minimized", MAXIMIZED: "maximized", FULLSCREEN: "fullscreen", LOCKED_FULLSCREEN: "locked-fullscreen" },
      CreateType: { NORMAL: "normal", POPUP: "popup", PANEL: "panel" },
      get: bridged("windows.get", null, (args) => [args[0], args[1] || {}]),
      getCurrent: bridged("windows.getCurrent", null, queryOptions),
      getLastFocused: bridged("windows.getLastFocused", null, queryOptions),
      getAll: bridged("windows.getAll", null, queryOptions),
      create: bridged("windows.create", null, queryOptions),
      update: bridged("windows.update", null, (args) => [args[0], args[1] || {}]),
      remove: bridged("windows.remove"),
      onCreated: new ExtensionEvent("windows.onCreated"),
      onRemoved: new ExtensionEvent("windows.onRemoved"),
      onFocusChanged: new ExtensionEvent("windows.onFocusChanged"),
      onBoundsChanged: new ExtensionEvent("windows.onBoundsChanged"),
    };
    Object.assign(api, {
      tabs, scripting, action, browserAction: action, alarms, windows,
      extension: { getURL: runtime.getURL, inIncognitoContext: false, getViews: () => [], getBackgroundPage: backgroundPage },
    });
  }

  // --- runtime -> page entry points ---------------------------------------------------
  function dispatchMessage(message, sender, external) {
    const listeners = Array.from((external ? runtime.onMessageExternal : runtime.onMessage).listeners);
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

  // `id` is this context's end of a new channel, unused without a listener.
  function dispatchConnect(id, name, sender, external) {
    const event = external ? runtime.onConnectExternal : runtime.onConnect;
    if (!event.hasListeners()) return { none: true };
    event.dispatch(makePort(id, name, sender));
    return {};
  }

  function emit(name, ...args) {
    const ev = events.get(name);
    if (ev) ev.dispatch(...args);
    if (name === "storage.onChanged" && storage[args[1]]) storage[args[1]].onChanged.dispatch(args[0]);
    const onclick = name === "contextMenus.onClicked" && menuClicks.get(menuKey(args[0].menuItemId));
    if (onclick) {
      try { onclick(...args); } catch (e) { console.error("Vsesvit: contextMenus onclick threw", e); }
    }
  }

  Object.defineProperty(g, "__vsesvit", { value: Object.freeze({ dispatchMessage, dispatchConnect, emit, id: config.id, kind: config.kind }), configurable: false, enumerable: false });
  g.chrome = api;
  g.browser = api;
})
