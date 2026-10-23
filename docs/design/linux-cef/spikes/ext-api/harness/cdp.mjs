// spike-ext-api CDP harness (Node >= 22: global WebSocket and fetch).
//
//   node cdp.mjs daemon                 hold one browser-target session on SPIKE_CDP_PORT (9293),
//                                       auto-attach to every target (flatten), log target events,
//                                       extension-worker console messages and action-state bindings
//                                       to $SPIKE_ENV/cdp.log; serve commands on 127.0.0.1:9294.
//   node cdp.mjs eval SEL EXPR [gesture] evaluate EXPR (awaited, by value) in the target SEL:
//                                         sw:<ext id|name>   the extension's service worker
//                                         page:<substring>   a page target whose URL contains it
//                                         id:<targetId>
//   node cdp.mjs targets                list targets
//   node cdp.mjs browser METHOD [JSON]  send METHOD on the browser session
//   node cdp.mjs send SEL METHOD [JSON] send METHOD on SEL's session
//
// WRAP=1 (daemon): inject a wrapper for chrome.action setters into every extension service
// worker while it waits for the debugger on start, reporting calls via Runtime.addBinding.
import fs from "node:fs";
import http from "node:http";

const PORT = Number(process.env.SPIKE_CDP_PORT || 9293);
const DPORT = 9294;
const ENV = process.env.SPIKE_ENV; // set by ../env.sh
const LOG = `${ENV}/cdp.log`;
const ids = JSON.parse(fs.readFileSync(`${ENV}/ids.json`, "utf8")); // written by ../run.sh (ids.py)
const extId = (s) => (ids[s] ? ids[s].id : s);
const nameOf = (url) => {
  const m = /^chrome-extension:\/\/([a-p]{32})/.exec(url || "");
  if (!m) return null;
  for (const [k, v] of Object.entries(ids)) if (v.id === m[1]) return k;
  return m[1];
};
const t0 = Date.now();
const log = (s) => fs.appendFileSync(LOG, `[${((Date.now() - t0) / 1000).toFixed(3)} ${new Date().toISOString().slice(11, 23)}] ${s}\n`);

const WRAPPER = `(() => {
  if (globalThis.__vsesvitWrapped) return "already";
  const report = (method, args) => {
    try {
      const plain = JSON.stringify(args, (k, v) => {
        if (typeof ImageData !== "undefined" && v instanceof ImageData) return { imageData: [v.width, v.height, v.data.length] };
        return v;
      });
      globalThis.__vsesvitAction(JSON.stringify({ method, args: JSON.parse(plain) }));
    } catch (e) { globalThis.__vsesvitAction(JSON.stringify({ method, error: String(e) })); }
  };
  const wrap = () => {
    const a = globalThis.chrome && chrome.action;
    if (!a) return false;
    for (const m of ["setBadgeText", "setBadgeBackgroundColor", "setBadgeTextColor", "setTitle", "setIcon", "setPopup", "enable", "disable"]) {
      const orig = a[m];
      if (typeof orig !== "function") continue;
      a[m] = function (...args) { report(m, args); return orig.apply(this, args); };
    }
    return true;
  };
  globalThis.__vsesvitWrapped = true;
  return wrap() ? "wrapped" : "no chrome.action yet";
})()`;

class Cdp {
  constructor(url) { this.url = url; this.next = 1; this.pending = new Map(); this.handlers = []; }
  open() {
    return new Promise((res, rej) => {
      this.ws = new WebSocket(this.url);
      this.ws.onopen = () => res();
      this.ws.onerror = (e) => rej(new Error("ws error " + (e.message || "")));
      this.ws.onmessage = (m) => {
        const msg = JSON.parse(m.data);
        if (msg.id && this.pending.has(msg.id)) { const { res, rej } = this.pending.get(msg.id); this.pending.delete(msg.id); msg.error ? rej(Object.assign(new Error(msg.error.message), { cdp: msg.error })) : res(msg.result); }
        else for (const h of this.handlers) h(msg);
      };
      this.ws.onclose = () => { for (const { rej } of this.pending.values()) rej(new Error("closed")); this.pending.clear(); if (this.onclose) this.onclose(); };
    });
  }
  send(method, params = {}, sessionId) {
    const id = this.next++;
    const msg = { id, method, params };
    if (sessionId) msg.sessionId = sessionId;
    return new Promise((res, rej) => { this.pending.set(id, { res, rej }); this.ws.send(JSON.stringify(msg)); });
  }
}

async function browserCdp() {
  const v = await (await fetch(`http://127.0.0.1:${PORT}/json/version`)).json();
  const c = new Cdp(v.webSocketDebuggerUrl);
  await c.open();
  return c;
}

async function daemon() {
  const c = await browserCdp();
  const sessions = new Map(); // sessionId -> targetInfo
  const wrap = process.env.WRAP === "1";
  c.onclose = () => { log("daemon: browser connection closed"); process.exit(0); };
  c.handlers.push(async (msg) => {
    const p = msg.params || {};
    switch (msg.method) {
      case "Target.targetCreated": log(`target+ ${p.targetInfo.type} ${p.targetInfo.targetId.slice(0, 8)} ${p.targetInfo.url} ${nameOf(p.targetInfo.url) ? "(" + nameOf(p.targetInfo.url) + ")" : ""}`); break;
      case "Target.targetDestroyed": log(`target- ${p.targetId.slice(0, 8)}`); break;
      case "Target.targetInfoChanged": if (p.targetInfo.type !== "browser") log(`target~ ${p.targetInfo.type} ${p.targetInfo.targetId.slice(0, 8)} ${p.targetInfo.url} title=${JSON.stringify(p.targetInfo.title)}`); break;
      case "Target.attachedToTarget": {
        const ti = p.targetInfo;
        sessions.set(p.sessionId, ti);
        const ext = nameOf(ti.url);
        const s = p.sessionId;
        try {
          if (ti.type === "service_worker" && ext) {
            await c.send("Runtime.enable", {}, s);
            if (wrap) {
              await c.send("Runtime.addBinding", { name: "__vsesvitAction" }, s);
              const r = await c.send("Runtime.evaluate", { expression: WRAPPER, returnByValue: true }, s);
              log(`wrap ${ext} at start (waiting=${p.waitingForDebugger}): ${JSON.stringify(r.result.value)}`);
            }
          }
        } catch (e) { log(`attach setup failed for ${ti.url}: ${e.message}`); }
        if (p.waitingForDebugger) c.send("Runtime.runIfWaitingForDebugger", {}, s).catch(() => {});
        // nested auto-attach (workers of pages, etc.)
        c.send("Target.setAutoAttach", { autoAttach: true, waitForDebuggerOnStart: wrap, flatten: true }, s).catch(() => {});
        break;
      }
      case "Target.detachedFromTarget": sessions.delete(p.sessionId); break;
      case "Runtime.bindingCalled": {
        const ti = sessions.get(msg.sessionId);
        log(`ACTION ${nameOf(ti && ti.url)} ${p.payload}`);
        break;
      }
      case "Runtime.consoleAPICalled": {
        const ti = sessions.get(msg.sessionId);
        log(`console ${nameOf(ti && ti.url)} ${p.type}: ${p.args.map((a) => a.value ?? a.description ?? a.type).join(" ")}`);
        break;
      }
      case "Runtime.exceptionThrown": {
        const ti = sessions.get(msg.sessionId);
        log(`exception ${nameOf(ti && ti.url)} ${p.exceptionDetails.text} ${p.exceptionDetails.exception?.description ?? ""}`);
        break;
      }
    }
  });
  await c.send("Target.setDiscoverTargets", { discover: true });
  await c.send("Target.setAutoAttach", { autoAttach: true, waitForDebuggerOnStart: wrap, flatten: true });
  log(`daemon: attached to browser on ${PORT} (wrap=${wrap})`);

  const findSession = async (sel) => {
    const { targetInfos } = await c.send("Target.getTargets");
    let ti;
    if (sel.startsWith("sw:")) { const id = extId(sel.slice(3)); ti = targetInfos.find((t) => t.type === "service_worker" && t.url.startsWith(`chrome-extension://${id}/`)); }
    else if (sel.startsWith("page:")) { const sub = sel.slice(5); ti = targetInfos.filter((t) => t.type === "page" || t.type === "other" || t.type === "background_page").find((t) => t.url.includes(sub)); }
    else if (sel.startsWith("tab:")) { const sub = sel.slice(4); ti = targetInfos.find((t) => t.type === "tab" && t.url.includes(sub)); }
    else if (sel.startsWith("id:")) { ti = targetInfos.find((t) => t.targetId.startsWith(sel.slice(3))); }
    if (!ti) throw new Error("no target for " + sel);
    for (const [sid, info] of sessions) if (info.targetId === ti.targetId) return { sid, ti };
    const { sessionId } = await c.send("Target.attachToTarget", { targetId: ti.targetId, flatten: true });
    sessions.set(sessionId, ti);
    return { sid: sessionId, ti };
  };

  http.createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) body += chunk;
    const out = (code, v) => { res.writeHead(code, { "content-type": "application/json" }); res.end(JSON.stringify(v, null, 1)); };
    try {
      const q = JSON.parse(body || "{}");
      if (req.url === "/targets") {
        const { targetInfos } = await c.send("Target.getTargets", q.filter ? { filter: q.filter } : {});
        return out(200, targetInfos.map((t) => ({ type: t.type, id: t.targetId.slice(0, 8), url: t.url, title: t.title, attached: t.attached, opener: t.openerId?.slice(0, 8), ext: nameOf(t.url) })));
      }
      if (req.url === "/eval") {
        const { sid, ti } = await findSession(q.sel);
        const r = await c.send("Runtime.evaluate", { expression: q.expr, awaitPromise: true, returnByValue: true, userGesture: !!q.gesture }, sid);
        return out(200, { target: ti.url, result: r.exceptionDetails ? { exception: r.exceptionDetails.exception?.description ?? r.exceptionDetails.text } : r.result.value });
      }
      if (req.url === "/send") {
        const sid = q.sel ? (await findSession(q.sel)).sid : undefined;
        return out(200, await c.send(q.method, q.params || {}, sid));
      }
      out(404, { error: "unknown" });
    } catch (e) { out(500, { error: e.message, cdp: e.cdp }); }
  }).listen(DPORT, "127.0.0.1");
}

async function client(path, q) {
  const r = await fetch(`http://127.0.0.1:${DPORT}${path}`, { method: "POST", body: JSON.stringify(q) });
  console.log(await r.text());
}

const [cmd, ...args] = process.argv.slice(2);
if (cmd === "daemon") await daemon();
else if (cmd === "targets") await client("/targets", { filter: args[0] ? JSON.parse(args[0]) : undefined });
else if (cmd === "eval") await client("/eval", { sel: args[0], expr: args[1], gesture: args[2] === "gesture" });
else if (cmd === "browser") await client("/send", { method: args[0], params: args[1] ? JSON.parse(args[1]) : {} });
else if (cmd === "send") await client("/send", { sel: args[0], method: args[1], params: args[2] ? JSON.parse(args[2]) : {} });
else console.log("usage: daemon | targets | eval SEL EXPR [gesture] | browser METHOD [JSON] | send SEL METHOD [JSON]");
