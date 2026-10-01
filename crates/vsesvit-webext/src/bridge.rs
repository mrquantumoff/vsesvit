//! JS -> Rust calls (`script-message-with-reply-received`) and Rust -> JS events.
//!
//! Two handlers per extension per `UserContentManager`: content scripts reach
//! `Extension::handler` from the extension's isolated world, extension pages reach
//! `Extension::page_handler` from the default world. The closure knows the extension and
//! where the call came from, so the payload never has to be trusted for that. The page
//! handler is also visible to whatever else shares the view (a web page in the same
//! tab, a foreign iframe), which is why page calls must carry `Extension::page_token`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use javascriptcore as jsc;
use serde_json::{Map, Value, json};
use vsesvit_core::ext_storage::{Area, StorageChange};
use webkit::prelude::*;
use webkit::{gio, glib};

use crate::extension::{Alarm, Extension, ViewId};
use crate::protocol::{self, Call, Dispatched, Method, NO_RECEIVER, Replies, Sender, StorageArea};
use crate::runtime::Inner;
use crate::tabs::{TabId, TabInfo};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// A content script in the extension's isolated world of a tab.
    Content { tab: TabId },
    /// An extension page in a view the runtime built (background, popup).
    Page { view: ViewId },
    /// An extension page in a tab (options page, `tabs.create(getURL(..))`, a link).
    TabPage { tab: TabId },
}

impl Origin {
    fn is_page(self) -> bool {
        !matches!(self, Origin::Content { .. })
    }
}

/// The pending Promise of one `postMessage`. Consumed exactly once.
pub(crate) struct Reply {
    reply: webkit::ScriptMessageReply,
    ctx: jsc::Context,
}

impl Reply {
    fn ok(self, value: Option<Value>) {
        let json = protocol::reply_json(value);
        self.reply.return_value(&jsc::Value::from_json(&self.ctx, &json));
    }

    fn err(self, message: &str) {
        self.reply.return_error_message(message);
    }

    fn finish(self, result: Result<Option<Value>, String>) {
        match result {
            Ok(v) => self.ok(v),
            Err(e) => self.err(&e),
        }
    }
}

pub(crate) fn register(
    inner: &Rc<Inner>,
    ucm: &webkit::UserContentManager,
    ext: &Rc<Extension>,
    origin: Origin,
    world: Option<&str>,
) -> glib::SignalHandlerId {
    let name = if origin.is_page() { &ext.page_handler } else { &ext.handler };
    if !ucm.register_script_message_handler_with_reply(name, world) {
        log::warn!("{}: message handler {} was already registered", ext.id.as_str(), name);
    }
    let weak = Rc::downgrade(inner);
    let ext_id = ext.id.clone();
    ucm.connect_script_message_with_reply_received(Some(name), move |_, value, reply| {
        let Some(ctx) = value.context() else {
            reply.return_error_message("Vsesvit: message without a JavaScript context");
            return true;
        };
        let reply = Reply { reply: reply.clone(), ctx };
        let Some(inner) = weak.upgrade() else {
            reply.err("Vsesvit: the extension runtime has shut down");
            return true;
        };
        let text = value.to_json(0).map(String::from).unwrap_or_default();
        let call = match Call::from_json(&text) {
            Ok(call) => call,
            Err(e) => {
                reply.err(&e);
                return true;
            }
        };
        let Some(ext) = inner.extension(&ext_id) else {
            reply.err("Vsesvit: the extension is no longer loaded");
            return true;
        };
        if origin.is_page() && call.token.as_deref() != Some(ext.page_token.as_str()) {
            reply.err("Vsesvit: the extension bridge is unavailable in this context");
            return true;
        }
        dispatch(&inner, &ext, origin, call, reply);
        true
    })
}

fn dispatch(inner: &Rc<Inner>, ext: &Rc<Extension>, origin: Origin, call: Call, reply: Reply) {
    if !origin.is_page() && !call.method.allowed_in_content_script() {
        return reply.err(&format!("{} is not available in content scripts", call.method));
    }
    match call.method {
        Method::RuntimeSendMessage => {
            let (inner, target) = (inner.clone(), ext.clone());
            ext.when_background_loaded(move || send_to_pages(&inner, &target, origin, &call, reply));
        }
        Method::TabsSendMessage => send_to_tab(inner, ext, origin, &call, reply),
        Method::ScriptingExecuteScript => execute_script(inner, ext, &call, reply),
        Method::StorageGet | Method::StorageSet | Method::StorageRemove | Method::StorageClear | Method::StorageGetBytesInUse => {
            reply.finish(storage(inner, ext, &call));
        }
        Method::RuntimeOpenOptionsPage => reply.finish(open_options_page(inner, ext)),
        Method::TabsQuery | Method::TabsGet | Method::TabsGetCurrent | Method::TabsCreate | Method::TabsUpdate | Method::TabsRemove | Method::TabsReload => {
            reply.finish(tabs(inner, ext, origin, &call));
        }
        Method::ScriptingInsertCss => reply.finish(insert_css(inner, ext, &call)),
        Method::ActionSetBadgeText
        | Method::ActionGetBadgeText
        | Method::ActionSetTitle
        | Method::ActionGetTitle
        | Method::ActionSetIcon
        | Method::ActionSetPopup
        | Method::ActionGetPopup
        | Method::ActionNoop => reply.finish(action(inner, ext, &call)),
        Method::AlarmsCreate | Method::AlarmsGet | Method::AlarmsGetAll | Method::AlarmsClear | Method::AlarmsClearAll => {
            reply.finish(alarms(inner, ext, &call));
        }
    }
}

// --- events -----------------------------------------------------------------------------

fn run(view: &webkit::WebView, world: Option<&str>, source: &str, what: &str) {
    let what = what.to_owned();
    view.evaluate_javascript(source, world, None, None::<&gio::Cancellable>, move |result| {
        if let Err(e) = result {
            log::debug!("emit {what}: {e}");
        }
    });
}

/// Fire `event` in one context. A context without the shim ignores it.
pub(crate) fn emit(view: &webkit::WebView, world: Option<&str>, event: &str, args: &[Value]) {
    run(view, world, &protocol::emit_source(event, args), event);
}

/// Fire `event` in every page of `ext`: the views the runtime built and the tabs
/// currently showing one of its documents. In a tab the source checks the document's
/// origin itself, so a web page that took the tab over meanwhile sees nothing.
pub(crate) fn emit_to_pages(inner: &Inner, ext: &Extension, event: &str, args: &[Value]) {
    for (_, _, view) in ext.live_views() {
        emit(&view, None, event, args);
    }
    let guarded = protocol::emit_source_in_page(&ext.host, event, args);
    for (_, view) in inner.page_tab_views(ext) {
        run(&view, None, &guarded, event);
    }
}

pub(crate) fn emit_to_tabs(inner: &Inner, ext: &Extension, event: &str, args: &[Value]) {
    for view in inner.tab_views() {
        emit(&view, Some(&ext.world), event, args);
    }
}

pub(crate) fn storage_changed(inner: &Inner, ext: &Extension, area: StorageArea, changes: &[StorageChange]) {
    if changes.is_empty() {
        return;
    }
    let mut map = Map::new();
    for c in changes {
        let mut entry = Map::new();
        if let Some(old) = &c.old_value {
            entry.insert("oldValue".into(), old.clone());
        }
        if let Some(new) = &c.new_value {
            entry.insert("newValue".into(), new.clone());
        }
        map.insert(c.key.clone(), Value::Object(entry));
    }
    let args = [Value::Object(map), json!(area.name())];
    emit_to_pages(inner, ext, "storage.onChanged", &args);
    emit_to_tabs(inner, ext, "storage.onChanged", &args);
}

// --- messaging --------------------------------------------------------------------------

fn sender_for(inner: &Inner, ext: &Extension, origin: Origin, call: &Call) -> Sender {
    let url = call.url.clone();
    let mut sender = Sender { id: ext.id.as_str().to_owned(), origin: url.as_deref().and_then(Sender::origin_of), url, ..Sender::default() };
    match origin {
        Origin::Content { tab } | Origin::TabPage { tab } => {
            sender.tab = inner.tab_info(tab).map(|t| ext.tab_json(&t));
            sender.frame_id = call.top_frame.then_some(0);
        }
        Origin::Page { .. } => {}
    }
    sender
}

/// One context a message is offered to.
struct Target {
    view: webkit::WebView,
    body: Rc<String>,
    world: Option<String>,
}

fn send_to_pages(inner: &Rc<Inner>, ext: &Rc<Extension>, origin: Origin, call: &Call, reply: Reply) {
    let sender = sender_for(inner, ext, origin, call);
    let body = Rc::new(protocol::dispatch_source(call.arg(0), &sender));
    let guarded = Rc::new(protocol::dispatch_source_in_page(&ext.host, call.arg(0), &sender));
    let mut targets: Vec<Target> = ext
        .live_views()
        .into_iter()
        .filter(|(id, _, _)| origin != Origin::Page { view: *id })
        .map(|(_, _, view)| Target { view, body: body.clone(), world: None })
        .collect();
    targets.extend(
        inner
            .page_tab_views(ext)
            .into_iter()
            .filter(|(tab, _)| origin != Origin::TabPage { tab: *tab })
            .map(|(_, view)| Target { view, body: guarded.clone(), world: None }),
    );
    deliver(targets, reply);
}

fn send_to_tab(inner: &Rc<Inner>, ext: &Rc<Extension>, origin: Origin, call: &Call, reply: Reply) {
    let Some(tab) = TabId::from_json(call.arg(0)) else { return reply.err("tabs.sendMessage: tabId must be an integer") };
    let Some(view) = inner.host.web_view(tab) else { return reply.err(&format!("No tab with id: {}.", tab.0)) };
    let sender = sender_for(inner, ext, origin, call);
    let message = call.arg(1);
    let mut targets = vec![Target { view: view.clone(), body: Rc::new(protocol::dispatch_source(message, &sender)), world: Some(ext.world.clone()) }];
    if view.uri().is_some_and(|u| ext.owns_url(&u)) {
        targets.push(Target { view, body: Rc::new(protocol::dispatch_source_in_page(&ext.host, message, &sender)), world: None });
    }
    deliver(targets, reply);
}

/// Deliver the message to every target at once; the sender gets the answer [`Replies`]
/// settles on.
fn deliver(targets: Vec<Target>, reply: Reply) {
    if targets.is_empty() {
        return reply.err(NO_RECEIVER);
    }
    let state = Rc::new(RefCell::new((Replies::new(targets.len()), Some(reply))));
    for target in targets {
        let state = state.clone();
        target.view.call_async_javascript_function(&target.body, None, target.world.as_deref(), None, None::<&gio::Cancellable>, move |result| {
            let dispatched = match result {
                Ok(value) => Dispatched::parse(value.to_json(0).as_deref()),
                Err(e) => {
                    log::debug!("message dispatch: {e}");
                    Dispatched { none: true, value: None, error: None }
                }
            };
            let settled = {
                let (replies, reply) = &mut *state.borrow_mut();
                replies.settle(dispatched).and_then(|answer| Some((reply.take()?, answer)))
            };
            if let Some((reply, answer)) = settled {
                reply.finish(answer);
            }
        });
    }
}

// --- storage ----------------------------------------------------------------------------

fn storage(inner: &Rc<Inner>, ext: &Rc<Extension>, call: &Call) -> Result<Option<Value>, String> {
    let area = StorageArea::from_arg(call.arg(0))?;
    let core_area = match area {
        StorageArea::Local => Area::Local,
        StorageArea::Sync => Area::Sync,
    };
    let core_err = |e: vsesvit_core::Error| e.to_string();
    let changes = {
        let mut profile = inner.profile.borrow_mut();
        let mut store = profile.ext_storage();
        match call.method {
            Method::StorageGet => {
                let keys = key_list(call.arg(1))?;
                let items = store.get(&ext.id, core_area, keys.as_deref()).map_err(core_err)?;
                return Ok(Some(Value::Object(items.into_iter().collect())));
            }
            Method::StorageGetBytesInUse => {
                let keys = key_list(call.arg(1))?;
                let n = store.bytes_in_use(&ext.id, core_area, keys.as_deref()).map_err(core_err)?;
                return Ok(Some(json!(n)));
            }
            Method::StorageSet => {
                let items: BTreeMap<String, Value> =
                    call.arg(1).as_object().ok_or("storage.set: items must be an object")?.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                store.set(&ext.id, core_area, items).map_err(core_err)?
            }
            Method::StorageRemove => {
                let keys = key_list(call.arg(1))?.ok_or("storage.remove: keys required")?;
                store.remove(&ext.id, core_area, &keys).map_err(core_err)?
            }
            Method::StorageClear => store.clear(&ext.id, core_area).map_err(core_err)?,
            _ => unreachable!("not a storage method"),
        }
    };
    storage_changed(inner, ext, area, &changes);
    Ok(None)
}

fn key_list(v: &Value) -> Result<Option<Vec<String>>, String> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(vec![s.clone()])),
        Value::Array(items) => items.iter().map(|k| k.as_str().map(str::to_owned).ok_or_else(|| "storage keys must be strings".to_owned())).collect::<Result<Vec<_>, _>>().map(Some),
        _ => Err("storage keys must be a string, an array or null".into()),
    }
}

// --- tabs -------------------------------------------------------------------------------

fn tabs(inner: &Rc<Inner>, ext: &Rc<Extension>, origin: Origin, call: &Call) -> Result<Option<Value>, String> {
    let host = &inner.host;
    let find = |id: TabId| host.tabs().into_iter().find(|t| t.id == id);
    let visible = |t: &TabInfo| ext.tab_json(t);
    Ok(match call.method {
        Method::TabsQuery => Some(Value::Array(host.tabs().iter().filter(|t| t.matches_query(call.arg(0), ext.sees_tab(t))).map(visible).collect())),
        Method::TabsGet => {
            let id = TabId::from_json(call.arg(0)).ok_or("tabs.get: tabId must be an integer")?;
            Some(visible(&find(id).ok_or_else(|| format!("No tab with id: {}.", id.0))?))
        }
        Method::TabsGetCurrent => match origin {
            Origin::TabPage { tab } => find(tab).as_ref().map(visible),
            _ => None,
        },
        Method::TabsCreate => {
            let props = call.arg(0);
            let url = props.get("url").and_then(Value::as_str).unwrap_or("about:blank");
            let active = props.get("active").and_then(Value::as_bool).unwrap_or(true);
            let id = host.create_tab(url, active).ok_or("tabs.create: the browser refused to open a tab")?;
            Some(find(id).as_ref().map(visible).unwrap_or_else(|| {
                let mut tab = json!({ "id": id.0, "active": active });
                if ext.has_permission("tabs") || ext.host_access(url, Some(id)) {
                    tab["url"] = json!(url);
                }
                tab
            }))
        }
        Method::TabsUpdate => {
            let id = match TabId::from_json(call.arg(0)) {
                Some(id) => id,
                None => host.tabs().into_iter().find(|t| t.active).map(|t| t.id).ok_or("tabs.update: no active tab")?,
            };
            let props = call.arg(1);
            let url = props.get("url").and_then(Value::as_str);
            let active = props.get("active").and_then(Value::as_bool);
            if !host.update_tab(id, url, active) {
                return Err(format!("No tab with id: {}.", id.0));
            }
            find(id).as_ref().map(visible)
        }
        Method::TabsRemove => {
            let ids: Vec<TabId> = match call.arg(0) {
                Value::Array(items) => items.iter().filter_map(TabId::from_json).collect(),
                other => TabId::from_json(other).into_iter().collect(),
            };
            for id in ids {
                if !host.remove_tab(id) {
                    return Err(format!("No tab with id: {}.", id.0));
                }
            }
            None
        }
        Method::TabsReload => {
            let id = match TabId::from_json(call.arg(0)) {
                Some(id) => id,
                None => host.tabs().into_iter().find(|t| t.active).map(|t| t.id).ok_or("tabs.reload: no active tab")?,
            };
            host.web_view(id).ok_or_else(|| format!("No tab with id: {}.", id.0))?.reload();
            None
        }
        _ => unreachable!("not a tabs method"),
    })
}

fn open_options_page(inner: &Rc<Inner>, ext: &Rc<Extension>) -> Result<Option<Value>, String> {
    let page = ext.manifest.options_page.as_ref().ok_or("This extension has no options page")?;
    inner.host.create_tab(&ext.url(page.as_str()), true).ok_or("the browser refused to open a tab")?;
    Ok(None)
}

// --- scripting --------------------------------------------------------------------------

/// The tab view an injection may target: the `scripting` permission, and host access to
/// what the tab shows (host permissions, or an `activeTab` grant), as Chrome requires.
fn injection_target(inner: &Inner, ext: &Extension, injection: &Value, api: &str) -> Result<webkit::WebView, String> {
    let tab = TabId::from_json(&injection["target"]["tabId"]).ok_or("target.tabId must be an integer")?;
    if !ext.has_permission("scripting") {
        return Err(format!("{api} requires the \"scripting\" permission"));
    }
    let view = inner.host.web_view(tab).ok_or_else(|| format!("No tab with id: {}.", tab.0))?;
    let url = view.uri().map(String::from).unwrap_or_default();
    if !ext.host_access(&url, Some(tab)) {
        return Err(format!("Cannot access contents of url \"{url}\". Extension manifest must request permission to access this host."));
    }
    Ok(view)
}

fn read_files(ext: &Extension, files: &Value) -> Result<String, String> {
    let mut source = String::new();
    for f in files.as_array().ok_or("files must be an array")? {
        let rel = f.as_str().ok_or("files must be strings")?;
        let path = ext.resource(rel)?.resolve(&ext.dir);
        source.push_str(&std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?);
        source.push('\n');
    }
    Ok(source)
}

enum Injection {
    Files(String),
    Func { body: String },
}

fn execute_script(inner: &Rc<Inner>, ext: &Rc<Extension>, call: &Call, reply: Reply) {
    let injection = call.arg(0);
    let view = match injection_target(inner, ext, injection, "scripting.executeScript") {
        Ok(v) => v,
        Err(e) => return reply.err(&e),
    };
    // Isolated-world code gets the content-script API whether or not a manifest content
    // script ran there; MAIN-world code gets none, as in Chrome.
    let (world, bootstrap) = match injection["world"].as_str() {
        Some("MAIN") => (None, ""),
        _ => (Some(ext.world.clone()), ext.content_bootstrap.as_str()),
    };
    let what = if injection.get("files").is_some() {
        match read_files(ext, &injection["files"]) {
            Ok(source) => Injection::Files(format!("{bootstrap}{source}")),
            Err(e) => return reply.err(&e),
        }
    } else if let Some(func) = injection["func"].as_str() {
        let args = injection.get("args").cloned().unwrap_or_else(|| json!([]));
        Injection::Func { body: format!("{bootstrap}return ({func}).apply(null, {});", protocol::js_literal(&args)) }
    } else {
        return reply.err("scripting.executeScript: either files or func is required");
    };
    let done = move |result: Result<jsc::Value, glib::Error>| match result {
        Ok(value) => {
            let json = value.to_json(0).and_then(|j| serde_json::from_str::<Value>(&j).ok()).unwrap_or(Value::Null);
            reply.ok(Some(json!([{ "frameId": 0, "result": json }])));
        }
        Err(e) => reply.err(&e.to_string()),
    };
    match what {
        Injection::Files(source) => view.evaluate_javascript(&source, world.as_deref(), None, None::<&gio::Cancellable>, done),
        Injection::Func { body } => view.call_async_javascript_function(&body, None, world.as_deref(), None, None::<&gio::Cancellable>, done),
    }
}

fn insert_css(inner: &Rc<Inner>, ext: &Rc<Extension>, call: &Call) -> Result<Option<Value>, String> {
    let injection = call.arg(0);
    let view = injection_target(inner, ext, injection, "scripting.insertCSS")?;
    let css = match injection.get("css").and_then(Value::as_str) {
        Some(css) => css.to_owned(),
        None => read_files(ext, &injection["files"])?,
    };
    let source = format!(
        "(function(){{const s=document.createElement('style');s.textContent={};(document.head||document.documentElement).appendChild(s);}})();",
        protocol::js_string(&css)
    );
    view.evaluate_javascript(&source, Some(&ext.world), None, None::<&gio::Cancellable>, |_| {});
    Ok(None)
}

// --- action -----------------------------------------------------------------------------

fn action(inner: &Rc<Inner>, ext: &Rc<Extension>, call: &Call) -> Result<Option<Value>, String> {
    let details = call.arg(0);
    let result = {
        let mut guard = ext.action.borrow_mut();
        let state = guard.as_mut().ok_or("This extension declares no action")?;
        match call.method {
            Method::ActionSetBadgeText => {
                state.badge_text = details.get("text").and_then(Value::as_str).unwrap_or("").to_owned();
                None
            }
            Method::ActionGetBadgeText => return Ok(Some(json!(state.badge_text))),
            Method::ActionSetTitle => {
                state.title = details.get("title").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| ext.manifest.name.clone());
                None
            }
            Method::ActionGetTitle => return Ok(Some(json!(state.title))),
            Method::ActionSetIcon => {
                let path = match details.get("path") {
                    Some(Value::String(p)) => Some(p.clone()),
                    Some(Value::Object(sizes)) => sizes.iter().max_by_key(|(k, _)| k.parse::<u32>().unwrap_or(0)).and_then(|(_, v)| v.as_str().map(str::to_owned)),
                    _ => None,
                };
                if let Some(p) = path {
                    state.icon = Some(ext.resource(&p)?.resolve(&ext.dir));
                }
                None
            }
            Method::ActionSetPopup => {
                let popup = details.get("popup").and_then(Value::as_str).unwrap_or("");
                state.popup = crate::patterns::resource_path(&ext.base_url, popup).to_owned();
                None
            }
            Method::ActionGetPopup => {
                return Ok(Some(json!(if state.popup.is_empty() { String::new() } else { ext.url(&state.popup) })));
            }
            Method::ActionNoop => return Ok(None),
            _ => unreachable!("not an action method"),
        }
    };
    inner.notify_actions_changed();
    Ok(result)
}

// --- alarms -----------------------------------------------------------------------------

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

fn alarms(inner: &Rc<Inner>, ext: &Rc<Extension>, call: &Call) -> Result<Option<Value>, String> {
    match call.method {
        Method::AlarmsCreate => {
            let name = call.arg(0).as_str().unwrap_or("").to_owned();
            let info = call.arg(1);
            let minutes = |k: &str| info.get(k).and_then(Value::as_f64);
            let period = minutes("periodInMinutes").filter(|p| *p > 0.0);
            let delay_ms = if let Some(when) = minutes("when") {
                (when - now_ms()).max(0.0)
            } else if let Some(d) = minutes("delayInMinutes") {
                d * 60_000.0
            } else {
                period.map(|p| p * 60_000.0).unwrap_or(0.0)
            };
            clear_alarm(ext, &name);
            ext.alarms.borrow_mut().insert(name.clone(), Alarm { scheduled_time_ms: now_ms() + delay_ms, period_minutes: period, source: None });
            schedule_alarm(inner, ext, name, delay_ms);
            Ok(None)
        }
        Method::AlarmsGet => {
            let name = call.arg(0).as_str().unwrap_or("");
            Ok(ext.alarms.borrow().get(name).map(|a| Extension::alarm_json(name, a)))
        }
        Method::AlarmsGetAll => Ok(Some(Value::Array(ext.alarms.borrow().iter().map(|(n, a)| Extension::alarm_json(n, a)).collect()))),
        Method::AlarmsClear => Ok(Some(json!(clear_alarm(ext, call.arg(0).as_str().unwrap_or(""))))),
        Method::AlarmsClearAll => {
            let any = !ext.alarms.borrow().is_empty();
            ext.clear_alarms();
            Ok(Some(json!(any)))
        }
        _ => unreachable!("not an alarms method"),
    }
}

fn clear_alarm(ext: &Extension, name: &str) -> bool {
    match ext.alarms.borrow_mut().remove(name) {
        Some(alarm) => {
            if let Some(source) = alarm.source {
                source.remove();
            }
            true
        }
        None => false,
    }
}

fn schedule_alarm(inner: &Rc<Inner>, ext: &Rc<Extension>, name: String, delay_ms: f64) {
    let weak_inner = Rc::downgrade(inner);
    let weak_ext = Rc::downgrade(ext);
    let delay = Duration::from_millis(delay_ms.round().max(0.0) as u64);
    let fired = Cell::new(false);
    let alarm_name = name.clone();
    let source = glib::timeout_add_local(delay, move || {
        // A one-shot timeout: the alarm is re-armed explicitly so a changed period applies.
        if fired.replace(true) {
            return glib::ControlFlow::Break;
        }
        let (Some(inner), Some(ext)) = (weak_inner.upgrade(), weak_ext.upgrade()) else { return glib::ControlFlow::Break };
        let next = {
            let mut alarms = ext.alarms.borrow_mut();
            let Some(alarm) = alarms.get_mut(&alarm_name) else { return glib::ControlFlow::Break };
            alarm.source = None;
            let payload = Extension::alarm_json(&alarm_name, alarm);
            match alarm.period_minutes {
                Some(p) => {
                    alarm.scheduled_time_ms = now_ms() + p * 60_000.0;
                    (payload, Some(p * 60_000.0))
                }
                None => {
                    alarms.remove(&alarm_name);
                    (payload, None)
                }
            }
        };
        emit_to_pages(&inner, &ext, "alarms.onAlarm", &[next.0]);
        if let Some(period_ms) = next.1 {
            schedule_alarm(&inner, &ext, alarm_name.clone(), period_ms);
        }
        glib::ControlFlow::Break
    });
    if let Some(alarm) = ext.alarms.borrow_mut().get_mut(&name) {
        alarm.source = Some(source);
    }
}
