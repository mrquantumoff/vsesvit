//! Extension machinery of the spike, the same on both OSes: a registry of every browser by
//! label, hidden browsers in the platform's hidden host, DevTools methods on any browser's
//! page session with the result returned to the ctl client, console capture (the helper's
//! bridge), Chrome-created ("foreign") browsers neutralized and reopened as tabs, a cookie
//! access filter, prefs, content settings and the cookie store.
use super::{CefEvent, NativeHandle, Shared, cef_str, child_window_info, on_ui, on_ui_later};
use crate::ctl::Reply;
use cef::*;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Default)]
pub struct ExtState {
    pub hidden_parent: AtomicU64,
    inner: Mutex<Inner>,
    next_msg: AtomicI32,
    pub cookie_block: Mutex<Vec<String>>,
    otr: Mutex<Option<RequestContext>>,
    pub active: Mutex<String>,
}

#[derive(Default)]
struct Inner {
    by_id: HashMap<i32, Entry>,
    labels: HashMap<String, i32>,
    waiting: HashMap<String, Reply>,
    /// Hidden browsers whose creator waits for the first main-frame load.
    loading: HashMap<i32, Reply>,
    pending: HashMap<(i32, i32), (Reply, bool)>,
    foreign: HashMap<i32, Foreign>,
}

struct Entry {
    browser: Browser,
    label: String,
    kind: String,
    obs: Option<Registration>,
}

struct Foreign {
    created: Instant,
    done: bool,
}

fn label_for(kind: &str, id: i32) -> String {
    match kind {
        "tab" => format!("b{id}"),
        "auxpopup" => "popup".into(),
        "default" => format!("f{id}"),
        k => k.strip_prefix("hidden:").unwrap_or(k).to_string(),
    }
}

pub fn label_of(shared: &Shared, id: i32) -> String {
    shared.ext.inner.lock().unwrap().by_id.get(&id).map(|e| e.label.clone()).unwrap_or_else(|| format!("?{id}"))
}

/// The shell's private (off-the-record) request context, created on first use.
pub fn otr(shared: &Shared) -> RequestContext {
    let mut o = shared.ext.otr.lock().unwrap();
    o.get_or_insert_with(|| {
        crate::log("ext: created off-the-record request context");
        request_context_create_context(Some(&RequestContextSettings::default()), None).expect("otr context")
    })
    .clone()
}

fn ctx_desc(shared: &Shared, ctx: Option<RequestContext>) -> String {
    let Some(c) = ctx else { return "none".into() };
    let mut otr_ctx = shared.ext.otr.lock().unwrap().clone();
    let same_otr = otr_ctx.as_mut().map(|o| c.is_same(Some(o))).unwrap_or(0);
    let mut global = request_context_get_global_context();
    let same_global = global.as_mut().map(|g| c.is_same(Some(g))).unwrap_or(0);
    format!("is_global={} same_as_global={same_global} same_as_shell_private={same_otr} cache_path={:?}", c.is_global(), CefString::from(&c.cache_path()).to_string())
}

pub fn register(shared: &Arc<Shared>, kind: &str, b: &mut Browser, handle: NativeHandle, url: &str) {
    let id = b.identifier();
    let label = label_for(kind, id);
    let waiting = {
        let mut i = shared.ext.inner.lock().unwrap();
        i.labels.insert(label.clone(), id);
        i.by_id.insert(id, Entry { browser: b.clone(), label: label.clone(), kind: kind.to_string(), obs: None });
        i.waiting.remove(&label)
    };
    crate::log(&format!("ext: browser {label} id={id} kind={kind} handle=0x{handle:x}"));
    if let Some(r) = waiting {
        shared.ext.inner.lock().unwrap().loading.insert(id, r);
    }
    if kind == "default" {
        foreign_created(shared, b, handle, url);
    }
}

/// First main-frame load end (or error) of a hidden browser answers its `hidden` command.
pub fn load_end(shared: &Shared, id: i32, status: &str) {
    let r = shared.ext.inner.lock().unwrap().loading.remove(&id);
    if let Some(r) = r {
        r.send(format!("OK {} id={id} {status}", label_of(shared, id)));
    }
}

pub fn unregister(shared: &Shared, id: i32) {
    let mut i = shared.ext.inner.lock().unwrap();
    if let Some(r) = i.loading.remove(&id) {
        r.send("ERR closed before load");
    }
    if let Some(e) = i.by_id.remove(&id) {
        i.labels.remove(&e.label);
    }
    i.foreign.remove(&id);
    let dead: Vec<(i32, i32)> = i.pending.keys().filter(|k| k.0 == id).cloned().collect();
    for k in dead {
        if let Some((r, _)) = i.pending.remove(&k) {
            r.send("ERR browser closed");
        }
    }
}

// ------------------------------------------------------------------ foreign browsers

/// A browser Chrome created on its own (default_client). Neutralized synchronously, before
/// Chrome could show it; reopened as a shell tab once its URL commits (or after a timeout).
fn foreign_created(shared: &Arc<Shared>, b: &mut Browser, handle: NativeHandle, url: &str) {
    let id = b.identifier();
    let policy = std::env::var("SPIKE_FOREIGN").unwrap_or_else(|_| "reopen".into());
    let shown = crate::platform::is_shown(handle);
    let t = Instant::now();
    let neutral = if policy == "none" { "skipped (SPIKE_FOREIGN=none)".to_string() } else {
        match crate::platform::neutralize(handle, shared.ext.hidden_parent.load(Ordering::Relaxed)) {
            Ok(s) => format!("ok: {s}"),
            Err(e) => format!("FAILED: {e}"),
        }
    };
    let ctx = b.host().and_then(|h| h.request_context());
    crate::log(&format!("foreign: f{id} handle=0x{handle:x} url_at_create={url:?} shown_at_callback={shown} neutralize {neutral} in {:?} ctx: {}", t.elapsed(), ctx_desc(shared, ctx)));
    if policy != "reopen" {
        return;
    }
    shared.ext.inner.lock().unwrap().foreign.insert(id, Foreign { created: Instant::now(), done: false });
    let wait: i64 = std::env::var("SPIKE_FOREIGN_WAIT").ok().and_then(|v| v.parse().ok()).unwrap_or(2500);
    let s2 = shared.clone();
    on_ui_later(wait, move || {
        let pending = s2.ext.inner.lock().unwrap().foreign.get(&id).is_some_and(|f| !f.done);
        if pending {
            let cur = s2.ext.inner.lock().unwrap().by_id.get(&id).and_then(|e| e.browser.main_frame()).map(|f| CefString::from(&f.url()).to_string()).unwrap_or_default();
            let url = if cur.is_empty() { "about:blank".to_string() } else { cur };
            reopen(&s2, id, &url, &format!("timeout {wait} ms"));
        }
    });
}

pub fn address(shared: &Arc<Shared>, id: i32, url: &str) {
    let pending = shared.ext.inner.lock().unwrap().foreign.get(&id).is_some_and(|f| !f.done);
    if pending {
        crate::log(&format!("foreign: f{id} address {url:?}"));
        if !url.is_empty() && url != "about:blank" {
            reopen(shared, id, url, "committed");
        }
    }
}

fn reopen(shared: &Arc<Shared>, id: i32, url: &str, why: &str) {
    let (browser, age) = {
        let mut i = shared.ext.inner.lock().unwrap();
        let age = i.foreign.get_mut(&id).map(|f| {
            f.done = true;
            f.created.elapsed()
        });
        (i.by_id.get(&id).map(|e| e.browser.clone()), age)
    };
    let Some(b) = browser else { return };
    let ctx = b.host().and_then(|h| h.request_context());
    crate::log(&format!("foreign: f{id} reopen url={url} why={why} after {age:?} ctx: {}", ctx_desc(shared, ctx.clone())));
    shared.send(CefEvent::OpenTab { url: url.to_string(), ctx, why: format!("foreign f{id} {why}") });
    // Closing the foreign browser at once can destroy its request context (an incognito
    // profile dies with its last browser) before the reopened tab exists in it.
    let delay: i64 = std::env::var("SPIKE_FOREIGN_CLOSE_DELAY").ok().and_then(|v| v.parse().ok()).unwrap_or(1500);
    let close = move || {
        if let Some(h) = b.host() {
            h.close_browser(1);
        }
    };
    if delay <= 0 { close() } else { on_ui_later(delay, close) }
}

// ------------------------------------------------------------------ console

pub fn console(shared: &Shared, id: i32, kind: &str, msg: &str, src: &str) {
    let label = label_of(shared, id);
    if kind == "hidden:bridge" {
        if let Some(json) = msg.strip_prefix("vsesvit-bridge ") {
            crate::log(&format!("bridge-event: {json}"));
            return;
        }
    }
    crate::log(&format!("console[{label}] {msg} ({src})"));
}

// ------------------------------------------------------------------ DevTools page sessions

wrap_dev_tools_message_observer! {
    pub struct Observer { shared: Arc<Shared> }

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self, browser: Option<&mut Browser>, message_id: i32, success: i32, result: Option<&[u8]>) {
            let id = browser.as_ref().map(|b| b.identifier()).unwrap_or(-1);
            let text = result.map(|r| String::from_utf8_lossy(r).into_owned()).unwrap_or_default();
            let p = self.shared.ext.inner.lock().unwrap().pending.remove(&(id, message_id));
            let Some((reply, eval)) = p else { return };
            let answer = if success == 0 {
                format!("ERR {text}")
            } else if eval {
                match serde_json::from_str::<Value>(&text) {
                    Ok(v) if v.get("exceptionDetails").is_some() => {
                        let d = &v["exceptionDetails"];
                        format!("EXC {}", d["exception"]["description"].as_str().or(d["text"].as_str()).unwrap_or("?"))
                    }
                    Ok(v) => match v["result"].get("value") {
                        Some(val) => format!("OK {val}"),
                        None => format!("OK {}", v["result"]),
                    },
                    Err(_) => format!("OK {text}"),
                }
            } else {
                format!("OK {text}")
            };
            crate::log(&format!("devtools: {} msg={message_id} -> {}", label_of(&self.shared, id), crate::cdp::trunc(&answer, 3000)));
            reply.send(answer);
        }
        fn on_dev_tools_event(&self, browser: Option<&mut Browser>, method: Option<&CefString>, params: Option<&[u8]>) {
            let m = cef_str(method);
            if m.starts_with("Extensions.") || m.starts_with("Target.") {
                let id = browser.as_ref().map(|b| b.identifier()).unwrap_or(-1);
                let p = params.map(|r| String::from_utf8_lossy(r).into_owned()).unwrap_or_default();
                crate::log(&format!("devtools event {}: {m} {}", label_of(&self.shared, id), crate::cdp::trunc(&p, 800)));
            }
        }
    }
}

fn resolve(shared: &Shared, target: &str) -> Option<i32> {
    let i = shared.ext.inner.lock().unwrap();
    let t = if target == "active" { shared.ext.active.lock().unwrap().clone() } else { target.to_string() };
    i.labels.get(&t).cloned()
}

/// `execute_dev_tools_method` on `target`'s page session; the result goes to `reply`.
pub fn exec(shared: &Arc<Shared>, target: &str, method: &str, params: Value, eval: bool, reply: Reply) {
    let Some(id) = resolve(shared, target) else {
        reply.send(format!("ERR no browser {target}"));
        return;
    };
    let host = {
        let mut i = shared.ext.inner.lock().unwrap();
        let Some(e) = i.by_id.get_mut(&id) else { return };
        let host = e.browser.host();
        if e.obs.is_none() {
            if let Some(h) = &host {
                let mut obs = Observer::new(shared.clone());
                e.obs = h.add_dev_tools_message_observer(Some(&mut obs));
            }
        }
        host
    };
    let Some(host) = host else {
        reply.send("ERR no host");
        return;
    };
    let msg = 1000 + shared.ext.next_msg.fetch_add(1, Ordering::SeqCst);
    shared.ext.inner.lock().unwrap().pending.insert((id, msg), (reply.clone(), eval));
    let mut dict = parse_json(Some(&CefString::from(params.to_string().as_str())), JsonParserOptions::default()).and_then(|v| v.dictionary());
    let rc = host.execute_dev_tools_method(msg, Some(&CefString::from(method)), dict.as_mut());
    crate::log(&format!("devtools: {target} {method} {} -> msg {msg} rc={rc}", crate::cdp::trunc(&params.to_string(), 400)));
    if rc == 0 {
        shared.ext.inner.lock().unwrap().pending.remove(&(id, msg));
        reply.send("ERR execute_dev_tools_method returned 0");
    }
}

pub fn eval(shared: &Arc<Shared>, target: &str, expr: &str, gesture: bool, reply: Reply) {
    let params = serde_json::json!({ "expression": expr, "awaitPromise": true, "returnByValue": true, "userGesture": gesture });
    exec(shared, target, "Runtime.evaluate", params, true, reply);
}

// ------------------------------------------------------------------ hidden browsers

pub fn create_hidden(shared: &Arc<Shared>, label: &str, chrome_style: bool, url: &str, private: bool, reply: Reply) {
    let parent = shared.ext.hidden_parent.load(Ordering::Relaxed);
    if parent == 0 {
        reply.send("ERR no hidden host");
        return;
    }
    let style = if chrome_style { RuntimeStyle::CHROME } else { RuntimeStyle::ALLOY };
    let wi = child_window_info(parent, Rect { x: 0, y: 0, width: 1024, height: 700 }, style);
    let mut client = super::client::new_client_kind(shared.clone(), format!("hidden:{label}"));
    let mut ctx = private.then(|| otr(shared));
    shared.ext.inner.lock().unwrap().waiting.insert(label.to_string(), reply.clone());
    let ok = browser_host_create_browser(Some(&wi), Some(&mut client), Some(&CefString::from(url)), Some(&BrowserSettings::default()), None, ctx.as_mut());
    crate::log(&format!("ext: create hidden {label} style={} url={url} private={private} -> {ok}", if chrome_style { "chrome" } else { "alloy" }));
    if ok == 0 {
        shared.ext.inner.lock().unwrap().waiting.remove(label);
        reply.send("ERR create_browser failed");
    }
}

pub fn close_label(shared: &Shared, label: &str) -> bool {
    let b = resolve(shared, label).and_then(|id| shared.ext.inner.lock().unwrap().by_id.get(&id).map(|e| e.browser.clone()));
    match b.and_then(|b| b.host()) {
        Some(h) => {
            h.close_browser(1);
            true
        }
        None => false,
    }
}

/// Close every browser the registry knows (tabs, hidden, popup, foreign). Returns how many.
pub fn close_all(shared: &Shared) -> usize {
    let all: Vec<Browser> = shared.ext.inner.lock().unwrap().by_id.values().map(|e| e.browser.clone()).collect();
    for b in &all {
        if let Some(h) = b.host() {
            h.close_browser(1);
        }
    }
    all.len()
}

/// Browsers still alive (created and not yet through on_before_close).
pub fn live(shared: &Shared) -> usize {
    shared.ext.inner.lock().unwrap().by_id.len()
}

pub fn browser_of(shared: &Shared, label: &str) -> Option<Browser> {
    let id = resolve(shared, label)?;
    shared.ext.inner.lock().unwrap().by_id.get(&id).map(|e| e.browser.clone())
}

// ------------------------------------------------------------------ cookie filter

wrap_cookie_access_filter! {
    pub struct BlockFilter { shared: Arc<Shared> }

    impl CookieAccessFilter {
        fn can_send_cookie(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, request: Option<&mut Request>, cookie: Option<&Cookie>) -> i32 {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            crate::log(&format!("cookiefilter: refuse send {} on {url}", cookie.map(|c| c.name.to_string()).unwrap_or_default()));
            0
        }
        fn can_save_cookie(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, request: Option<&mut Request>, _response: Option<&mut Response>, cookie: Option<&Cookie>) -> i32 {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            crate::log(&format!("cookiefilter: refuse save {} on {url}", cookie.map(|c| c.name.to_string()).unwrap_or_default()));
            0
        }
    }
}

wrap_resource_request_handler! {
    pub struct Rrh { shared: Arc<Shared> }

    impl ResourceRequestHandler {
        fn cookie_access_filter(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, request: Option<&mut Request>) -> Option<CookieAccessFilter> {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            let host = url.split("://").nth(1).unwrap_or("").split(['/', ':']).next().unwrap_or("").to_string();
            let blocked = self.shared.ext.cookie_block.lock().unwrap().iter().any(|h| *h == host);
            blocked.then(|| BlockFilter::new(self.shared.clone()))
        }
    }
}

pub fn resource_request_handler(shared: &Arc<Shared>) -> Option<ResourceRequestHandler> {
    if shared.ext.cookie_block.lock().unwrap().is_empty() { None } else { Some(Rrh::new(shared.clone())) }
}

wrap_cookie_visitor! {
    pub struct Collect { out: Arc<Mutex<Vec<String>>> }

    impl CookieVisitor {
        fn visit(&self, cookie: Option<&Cookie>, _count: i32, _total: i32, _delete_cookie: Option<&mut i32>) -> i32 {
            if let Some(c) = cookie {
                self.out.lock().unwrap().push(format!("{}={} (domain {} path {})", c.name, c.value, c.domain, c.path));
            }
            1
        }
    }
}

// ------------------------------------------------------------------ prefs

pub fn pref_json(ctx: &RequestContext, name: &str) -> Option<String> {
    let mut v = ctx.preference(Some(&CefString::from(name)))?;
    Some(CefString::from(&write_json(Some(&mut v), JsonWriterOptions::default())).to_string())
}

pub fn set_pref_json(ctx: &RequestContext, name: &str, json: &str) -> Result<(), String> {
    let mut v = parse_json(Some(&CefString::from(json)), JsonParserOptions::default()).ok_or("bad json")?;
    let mut err = CefString::from("");
    if ctx.set_preference(Some(&CefString::from(name)), Some(&mut v), Some(&mut err)) == 1 { Ok(()) } else { Err(err.to_string()) }
}

fn ctx_arg(shared: &Shared, private: bool) -> Option<RequestContext> {
    if private { Some(otr(shared)) } else { request_context_get_global_context() }
}

fn setting_type(s: &str) -> Option<ContentSettingTypes> {
    match s {
        "cookies" => Some(ContentSettingTypes::COOKIES),
        "notifications" => Some(ContentSettingTypes::NOTIFICATIONS),
        _ => None,
    }
}

fn setting_value(s: &str) -> ContentSettingValues {
    match s {
        "allow" => ContentSettingValues::ALLOW,
        "block" => ContentSettingValues::BLOCK,
        "ask" => ContentSettingValues::ASK,
        _ => ContentSettingValues::DEFAULT,
    }
}

// ------------------------------------------------------------------ ctl

/// Commands every shell understands. Returns false when `line` is not one of them.
/// Runs on the CEF UI thread (GPUI main thread with the external pump).
pub fn ctl(shared: &Arc<Shared>, line: &str, reply: &Reply) -> bool {
    let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
    let private = arg.ends_with(" otr");
    let arg_np = arg.strip_suffix(" otr").unwrap_or(arg);
    let reply = reply.clone();
    match cmd {
        "cdp" | "cdptargets" | "cdpsw" | "cdpin" => crate::cdp::ctl(line.to_string(), reply),
        // hidden LABEL alloy|chrome URL [otr]
        "hidden" => {
            let mut it = arg_np.splitn(3, ' ');
            let (label, style, url) = (it.next().unwrap_or(""), it.next().unwrap_or("alloy"), it.next().unwrap_or("about:blank"));
            create_hidden(shared, label, style == "chrome", url, private, reply);
        }
        "closeh" => reply.send(if close_label(shared, arg) { "OK" } else { "ERR no such browser" }),
        // dt TARGET METHOD [JSON]
        "dt" => {
            let mut it = arg.splitn(3, ' ');
            let (t, m, p) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or("{}"));
            let params = serde_json::from_str(p).unwrap_or(serde_json::json!({}));
            exec(shared, t, m, params, false, reply);
        }
        // ev / evg TARGET EXPR (evg: with a user gesture)
        "ev" | "evg" => {
            let (t, e) = arg.split_once(' ').unwrap_or((arg, "1"));
            eval(shared, t, e, cmd == "evg", reply);
        }
        "labels" => {
            let i = shared.ext.inner.lock().unwrap();
            let mut v: Vec<String> = i
                .by_id
                .iter()
                .map(|(id, e)| format!("{}:{id}:{}:{}", e.label, e.kind, e.browser.main_frame().map(|f| CefString::from(&f.url()).to_string()).unwrap_or_default()))
                .collect();
            v.sort();
            reply.send(format!("OK {}", v.join(" | ")));
        }
        "ctx" => match browser_of(shared, arg) {
            Some(b) => reply.send(format!("OK {}", ctx_desc(shared, b.host().and_then(|h| h.request_context())))),
            None => reply.send("ERR no browser"),
        },
        "pref" => match ctx_arg(shared, private) {
            Some(c) => reply.send(format!("OK {}", pref_json(&c, arg_np).unwrap_or_else(|| "null".into()))),
            None => reply.send("ERR no context"),
        },
        // setpref NAME JSON [otr]
        "setpref" => {
            let (name, json) = arg_np.split_once(' ').unwrap_or((arg_np, "null"));
            match ctx_arg(shared, private) {
                Some(c) => {
                    let r = set_pref_json(&c, name, json);
                    crate::log(&format!("setpref {name} = {} -> {r:?}", crate::cdp::trunc(json, 300)));
                    reply.send(match r {
                        Ok(()) => "OK".to_string(),
                        Err(e) => format!("ERR {e}"),
                    });
                }
                None => reply.send("ERR no context"),
            }
        }
        // cs URL TYPE VALUE [otr]; csget URL TYPE [otr]
        "cs" | "csget" => {
            let mut it = arg_np.split_whitespace();
            let (url, ty, val) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or("default"));
            match (ctx_arg(shared, private), setting_type(ty)) {
                (Some(c), Some(t)) => {
                    let u = CefString::from(url);
                    if cmd == "cs" {
                        c.set_content_setting(Some(&u), Some(&u), t.clone(), setting_value(val));
                    }
                    let now = c.content_setting(Some(&u), Some(&u), t);
                    reply.send(format!("OK {url} {ty} = {}", now.get_raw()));
                }
                _ => reply.send("ERR bad context or type"),
            }
        }
        // cookies URL [otr]
        "cookies" => match ctx_arg(shared, private).and_then(|c| c.cookie_manager(None)) {
            Some(cm) => {
                let out = Arc::new(Mutex::new(Vec::new()));
                let mut v = Collect::new(out.clone());
                cm.visit_url_cookies(Some(&CefString::from(arg_np)), 1, Some(&mut v));
                on_ui_later(800, move || {
                    let l = out.lock().unwrap().clone();
                    reply.send(format!("OK [{}]", l.join("; ")));
                });
            }
            None => reply.send("ERR no cookie manager"),
        },
        // cookieblock HOST (adds) | cookieblock - (clears)
        "cookieblock" => {
            let mut b = shared.ext.cookie_block.lock().unwrap();
            if arg == "-" { b.clear() } else { b.push(arg.to_string()) }
            reply.send(format!("OK {:?}", *b));
        }
        _ => return false,
    }
    true
}

#[allow(dead_code)]
pub fn later(ms: i64, f: impl FnOnce() + Send + 'static) {
    on_ui_later(ms, f)
}

#[allow(dead_code)]
pub fn now(f: impl FnOnce() + Send + 'static) {
    on_ui(f)
}
