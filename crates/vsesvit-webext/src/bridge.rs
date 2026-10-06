//! JS -> Rust calls (`script-message-with-reply-received`) and Rust -> JS events.
//!
//! Two handlers per extension per `UserContentManager`: content scripts reach
//! `Extension::handler` from the extension's isolated world, extension pages reach
//! `Extension::page_handler` from the default world. The closure knows the extension and
//! where the call came from, so the payload never has to be trusted for that. The page
//! handler is also visible to whatever else shares the view (a web page in the same
//! tab, a foreign iframe), which is why page calls must carry `Extension::page_token`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use javascriptcore as jsc;
use serde_json::{Map, Value, json};
use vsesvit_core::ext_storage::StorageChange;
use webkit::prelude::*;
use webkit::{gio, glib};

use vsesvit_core::extensions::ExtensionId;

use crate::dnr;
use crate::dnr_rules::{STATIC_RULE_BUDGET, Scope};
use crate::extension::{Alarm, Extension, ViewId};
use crate::filters;
use crate::menus::ItemId;
use crate::messaging::{self, PortEvent, Wake};
use crate::notifications::{self, Activation, Priority, Shown};
use crate::protocol::{self, Call, Dispatch, Dispatched, Method, NO_RECEIVER, Replies, Sender};
use crate::runtime::Inner;
use crate::tabs::{NewTab, TabId, TabInfo};
use crate::web_navigation::FrameQuery;
use crate::windows::{self, NewWindow, WINDOW_ID_CURRENT, WindowInfo, WindowQuery, WindowScope, WindowUpdate};

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

    pub(crate) fn tab(self) -> Option<TabId> {
        match self {
            Origin::Content { tab } | Origin::TabPage { tab } => Some(tab),
            Origin::Page { .. } => None,
        }
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
        let origin = match origin {
            Origin::Page { view } => Origin::Page { view: ext.caller_view(view, call.url.as_deref()) },
            other => other,
        };
        dispatch(&inner, &ext, origin, call, reply);
        true
    })
}

fn dispatch(inner: &Rc<Inner>, ext: &Rc<Extension>, origin: Origin, call: Call, reply: Reply) {
    if !origin.is_page() && !call.method.allowed_in_content_script() {
        return reply.err(&format!("{} is not available in content scripts", call.method));
    }
    match call.method {
        Method::RuntimeSendMessage => send_message(inner, ext, origin, call, reply),
        Method::RuntimeConnect | Method::TabsConnect => connect(inner, ext, origin, call, reply),
        Method::PortPostMessage | Method::PortDisconnect | Method::PortReceive => port_call(inner, ext, origin, &call, reply),
        Method::TabsSendMessage => send_to_tab(inner, ext, origin, &call, reply),
        Method::ScriptingExecuteScript => execute_script(inner, ext, &call, reply),
        Method::StorageGet | Method::StorageSet | Method::StorageRemove | Method::StorageClear | Method::StorageGetBytesInUse => {
            reply.finish(storage(inner, ext, &call));
        }
        Method::RuntimeOpenOptionsPage => reply.finish(open_options_page(inner, ext)),
        Method::RuntimeReload => {
            reply.ok(None);
            // Not while the calling page is still handling its own call.
            let (inner, id) = (Rc::downgrade(inner), ext.id.clone());
            glib::idle_add_local_once(move || {
                if let Some(inner) = inner.upgrade() {
                    crate::runtime::reload(&inner, &id);
                }
            });
        }
        Method::TabsQuery
        | Method::TabsGet
        | Method::TabsGetCurrent
        | Method::TabsCreate
        | Method::TabsUpdate
        | Method::TabsMove
        | Method::TabsRemove
        | Method::TabsReload => reply.finish(tabs(inner, ext, origin, &call)),
        Method::WindowsGet
        | Method::WindowsGetCurrent
        | Method::WindowsGetLastFocused
        | Method::WindowsGetAll
        | Method::WindowsCreate
        | Method::WindowsUpdate
        | Method::WindowsRemove => reply.finish(windows(inner, ext, origin, &call)),
        Method::ScriptingInsertCss | Method::ScriptingRemoveCss => css(inner, ext, &call, reply),
        Method::ScriptingRegisterContentScripts
        | Method::ScriptingGetRegisteredContentScripts
        | Method::ScriptingUpdateContentScripts
        | Method::ScriptingUnregisterContentScripts => reply.finish(dynamic_scripts(inner, ext, &call)),
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
        Method::ContextMenusCreate | Method::ContextMenusUpdate | Method::ContextMenusRemove | Method::ContextMenusRemoveAll => {
            reply.finish(context_menus(inner, ext, &call));
        }
        Method::CommandsGetAll => reply.finish(commands(inner, ext)),
        Method::NotificationsCreate
        | Method::NotificationsUpdate
        | Method::NotificationsClear
        | Method::NotificationsGetAll
        | Method::NotificationsGetPermissionLevel => reply.finish(notifications(inner, ext, &call)),
        Method::DnrUpdateDynamicRules | Method::DnrUpdateSessionRules | Method::DnrUpdateEnabledRulesets => update_rules(inner, ext, &call, reply),
        Method::DnrGetDynamicRules
        | Method::DnrGetSessionRules
        | Method::DnrGetEnabledRulesets
        | Method::DnrGetAvailableStaticRuleCount
        | Method::DnrIsRegexSupported => reply.finish(rules(ext, &call)),
        Method::WebNavigationGetFrame | Method::WebNavigationGetAllFrames => reply.finish(web_navigation(inner, ext, &call)),
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
    for view in inner.tab_views(ext) {
        emit(&view, Some(&ext.world), event, args);
    }
}

/// `area` as the shim names it (see [`protocol::area_name`]).
pub(crate) fn storage_changed(inner: &Inner, ext: &Extension, area: &str, changes: &[StorageChange]) {
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
    let args = [Value::Object(map), json!(area)];
    emit_to_pages(inner, ext, "storage.onChanged", &args);
    // Content scripts have no `storage.session`, as by default in Chrome.
    if area != "session" {
        emit_to_tabs(inner, ext, "storage.onChanged", &args);
    }
}

// --- messaging --------------------------------------------------------------------------

/// The context at one end of a port: the extension it belongs to and where it runs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PortContext {
    pub ext: ExtensionId,
    pub origin: Origin,
}

/// Answers the `port.receive` calls a change in the port table woke.
pub(crate) fn wake(wakes: Vec<Wake<Reply>>) {
    for (reply, events) in wakes {
        reply.ok(Some(Value::Array(events.iter().map(PortEvent::to_json).collect())));
    }
}

/// `chrome.runtime.MessageSender` for a call from `caller`'s context at `origin`, with the
/// tab as `receiver` may see it.
fn sender_for(inner: &Inner, caller: &Extension, receiver: &Extension, origin: Origin, call: &Call) -> Sender {
    let url = call.url.clone();
    let mut sender = Sender { id: caller.id.as_str().to_owned(), origin: url.as_deref().and_then(Sender::origin_of), url, ..Sender::default() };
    if let Some(tab) = origin.tab() {
        sender.tab = inner.tab_for(receiver, tab).map(|t| receiver.tab_json(&t));
        sender.frame_id = call.top_frame.then_some(0);
    }
    sender
}

/// The extension another one names in `runtime.sendMessage` or `runtime.connect`, if it is
/// loaded and takes messages from `caller`. Chrome tells the caller nothing more.
fn external_target(inner: &Inner, caller: &Extension, id: &str) -> Option<Rc<Extension>> {
    let id = ExtensionId::parse(id).ok()?;
    inner.extension(&id).filter(|target| messaging::accepts_extension(&target.manifest, caller.id.as_str()))
}

/// Who a `runtime.sendMessage` or `runtime.connect` goes to: the extension named in
/// argument `i`, else the caller's own.
fn runtime_target(inner: &Inner, ext: &Rc<Extension>, call: &Call, i: usize) -> (Option<Rc<Extension>>, bool) {
    match call.arg(i).as_str() {
        Some(id) => (external_target(inner, ext, id), true),
        None => (Some(ext.clone()), false),
    }
}

/// One context a message or connection is offered to.
struct Target {
    view: webkit::WebView,
    world: Option<String>,
    /// A tab's default world, which may show a web page by now (see [`protocol::page_guard`]).
    guarded: bool,
    origin: Origin,
}

impl Target {
    fn call(&self, host: &str, dispatch: Dispatch, done: impl FnOnce(Dispatched) + 'static) {
        let body = if self.guarded { protocol::dispatch_source_in_page(host, dispatch) } else { protocol::dispatch_source(dispatch) };
        self.view.call_async_javascript_function(&body, None, self.world.as_deref(), None, None::<&gio::Cancellable>, move |result| {
            done(match result {
                Ok(value) => Dispatched::parse(value.to_json(0).as_deref()),
                Err(e) => {
                    log::debug!("dispatch: {e}");
                    Dispatched { none: true, value: None, error: None }
                }
            });
        });
    }
}

/// `ext`'s pages (its views, then the tabs showing one of its documents), but `except`.
fn page_targets(inner: &Inner, ext: &Extension, except: Option<Origin>) -> Vec<Target> {
    let views = ext.live_views().into_iter().map(|(id, _, view)| Target { view, world: None, guarded: false, origin: Origin::Page { view: id } });
    let tabs = inner.page_tab_views(ext).into_iter().map(|(tab, view)| Target { view, world: None, guarded: true, origin: Origin::TabPage { tab } });
    views.chain(tabs).filter(|t| Some(t.origin) != except).collect()
}

/// The view of `tab`, a tab `ext` runs in.
fn tab_view(inner: &Inner, ext: &Extension, tab: TabId) -> Result<webkit::WebView, String> {
    inner.tab_for(ext, tab).and_then(|_| inner.host.web_view(tab)).ok_or_else(|| format!("No tab with id: {}.", tab.0))
}

/// `ext`'s content scripts in `tab`'s top frame, and its page if the tab shows one.
fn tab_targets(inner: &Inner, ext: &Extension, tab: TabId) -> Result<Vec<Target>, String> {
    let view = tab_view(inner, ext, tab)?;
    let shows_page = view.uri().is_some_and(|u| ext.owns_url(&u));
    let mut targets = vec![Target { view: view.clone(), world: Some(ext.world.clone()), guarded: false, origin: Origin::Content { tab } }];
    if shows_page {
        targets.push(Target { view, world: None, guarded: true, origin: Origin::TabPage { tab } });
    }
    Ok(targets)
}

fn send_message(inner: &Rc<Inner>, ext: &Rc<Extension>, origin: Origin, call: Call, reply: Reply) {
    let (Some(target), external) = runtime_target(inner, ext, &call, 2) else { return reply.err(NO_RECEIVER) };
    let (inner, caller) = (inner.clone(), ext.clone());
    target.clone().when_background_loaded(move || {
        let sender = sender_for(&inner, &caller, &target, origin, &call);
        let targets = page_targets(&inner, &target, (!external).then_some(origin));
        deliver(&target, targets, Dispatch::Message { message: call.arg(0), sender: &sender, external }, reply);
    });
}

fn send_to_tab(inner: &Rc<Inner>, ext: &Rc<Extension>, origin: Origin, call: &Call, reply: Reply) {
    let Some(tab) = TabId::from_json(call.arg(0)) else { return reply.err("tabs.sendMessage: tabId must be an integer") };
    let targets = match tab_targets(inner, ext, tab) {
        Ok(targets) => targets,
        Err(e) => return reply.err(&e),
    };
    let sender = sender_for(inner, ext, ext, origin, call);
    deliver(ext, targets, Dispatch::Message { message: call.arg(1), sender: &sender, external: false }, reply);
}

/// Deliver the message to every target at once; the sender gets the answer [`Replies`]
/// settles on.
fn deliver(ext: &Extension, targets: Vec<Target>, dispatch: Dispatch, reply: Reply) {
    if targets.is_empty() {
        return reply.err(NO_RECEIVER);
    }
    let state = Rc::new(RefCell::new((Replies::new(targets.len()), Some(reply))));
    for target in targets {
        let state = state.clone();
        target.call(&ext.host, dispatch, move |dispatched| {
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

/// `runtime.connect` and `tabs.connect`. The caller's port opens at once, so what it posts
/// meanwhile is kept, and every context the connection goes to is offered a port of its own.
fn connect(inner: &Rc<Inner>, ext: &Rc<Extension>, origin: Origin, call: Call, reply: Reply) {
    let Some(port) = call.arg(0).as_str().map(str::to_owned) else { return reply.err("port id must be a string") };
    let opened = inner.ports.borrow_mut().open(&port, PortContext { ext: ext.id.clone(), origin });
    if let Err(e) = opened {
        return reply.err(&e);
    }
    reply.ok(None);
    if call.method == Method::TabsConnect {
        // Like messages, connections reach a tab's top frame only.
        let top = call.arg(3).is_null() || call.arg(3).as_u64() == Some(0);
        let targets = TabId::from_json(call.arg(1)).filter(|_| top).and_then(|tab| tab_targets(inner, ext, tab).ok()).unwrap_or_default();
        let sender = sender_for(inner, ext, ext, origin, &call);
        return offer(inner, ext, &port, targets, &call, &sender, false);
    }
    let (Some(target), external) = runtime_target(inner, ext, &call, 1) else {
        let wakes = inner.ports.borrow_mut().seal(&port);
        return wake(wakes);
    };
    let (inner, caller) = (inner.clone(), ext.clone());
    target.clone().when_background_loaded(move || {
        let sender = sender_for(&inner, &caller, &target, origin, &call);
        let targets = page_targets(&inner, &target, (!external).then_some(origin));
        offer(&inner, &target, &port, targets, &call, &sender, external);
    });
}

fn offer(inner: &Rc<Inner>, target_ext: &Extension, opener: &str, targets: Vec<Target>, call: &Call, sender: &Sender, external: bool) {
    let name = call.arg(2).as_str().unwrap_or("");
    for target in targets {
        let offered = inner.ports.borrow_mut().offer(opener, PortContext { ext: target_ext.id.clone(), origin: target.origin });
        let Some(port) = offered else { return };
        let weak = Rc::downgrade(inner);
        let answered = port.clone();
        target.call(&target_ext.host, Dispatch::Connect { port: &port, name, sender, external }, move |dispatched| {
            if let Some(inner) = weak.upgrade() {
                let wakes = inner.ports.borrow_mut().answer(&answered, !dispatched.none);
                wake(wakes);
            }
        });
    }
    let wakes = inner.ports.borrow_mut().seal(opener);
    wake(wakes);
}

/// `port.postMessage`, `port.disconnect` and `port.receive` from `ext`'s context at `origin`.
fn port_call(inner: &Rc<Inner>, ext: &Extension, origin: Origin, call: &Call, reply: Reply) {
    let Some(port) = call.arg(0).as_str() else { return reply.err("port id must be a string") };
    let context = PortContext { ext: ext.id.clone(), origin };
    let wakes = match call.method {
        Method::PortPostMessage => {
            let posted = inner.ports.borrow_mut().post(port, &context, call.arg(1).clone());
            match posted {
                Ok(wakes) => {
                    reply.ok(None);
                    wakes
                }
                Err(e) => return reply.err(&e),
            }
        }
        Method::PortDisconnect => {
            reply.ok(None);
            inner.ports.borrow_mut().disconnect(port, &context)
        }
        Method::PortReceive => inner.ports.borrow_mut().receive(port, &context, reply).into_iter().collect(),
        _ => unreachable!("not a port method"),
    };
    wake(wakes);
}

// --- storage ----------------------------------------------------------------------------

fn storage(inner: &Rc<Inner>, ext: &Rc<Extension>, call: &Call) -> Result<Option<Value>, String> {
    if call.arg(0).as_str() == Some("session") {
        return session_storage(inner, ext, call);
    }
    let area = protocol::storage_area(call.arg(0))?;
    let core_err = |e: vsesvit_core::Error| e.to_string();
    let changes = {
        let mut profile = inner.profile.borrow_mut();
        let mut store = profile.ext_storage();
        match call.method {
            Method::StorageGet => {
                let keys = key_list(call.arg(1))?;
                let items = store.get(&ext.id, area, keys.as_deref()).map_err(core_err)?;
                return Ok(Some(Value::Object(items.into_iter().collect())));
            }
            Method::StorageGetBytesInUse => {
                let keys = key_list(call.arg(1))?;
                let n = store.bytes_in_use(&ext.id, area, keys.as_deref()).map_err(core_err)?;
                return Ok(Some(json!(n)));
            }
            Method::StorageSet => {
                let items: BTreeMap<String, Value> =
                    call.arg(1).as_object().ok_or("storage.set: items must be an object")?.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                store.set(&ext.id, area, items).map_err(core_err)?
            }
            Method::StorageRemove => {
                let keys = key_list(call.arg(1))?.ok_or("storage.remove: keys required")?;
                store.remove(&ext.id, area, &keys).map_err(core_err)?
            }
            Method::StorageClear => store.clear(&ext.id, area).map_err(core_err)?,
            _ => unreachable!("not a storage method"),
        }
    };
    storage_changed(inner, ext, protocol::area_name(area), &changes);
    Ok(None)
}

/// `storage.session`: the same calls on items kept in memory while the extension is loaded.
fn session_storage(inner: &Rc<Inner>, ext: &Rc<Extension>, call: &Call) -> Result<Option<Value>, String> {
    let changes: Vec<StorageChange> = {
        let mut items = ext.session_storage.borrow_mut();
        let selected = |items: &BTreeMap<String, Value>| -> Result<Vec<(String, Value)>, String> {
            let keys = key_list(call.arg(1))?;
            Ok(items.iter().filter(|(k, _)| keys.as_ref().is_none_or(|ks| ks.contains(k))).map(|(k, v)| (k.clone(), v.clone())).collect())
        };
        match call.method {
            Method::StorageGet => return Ok(Some(Value::Object(selected(&items)?.into_iter().collect()))),
            Method::StorageGetBytesInUse => return Ok(Some(json!(selected(&items)?.iter().map(|(k, v)| k.len() + v.to_string().len()).sum::<usize>()))),
            Method::StorageSet => {
                let set = call.arg(1).as_object().ok_or("storage.set: items must be an object")?;
                set.iter()
                    .filter_map(|(key, value)| {
                        let old = items.insert(key.clone(), value.clone());
                        (old.as_ref() != Some(value)).then(|| StorageChange { key: key.clone(), old_value: old, new_value: Some(value.clone()) })
                    })
                    .collect()
            }
            Method::StorageRemove => {
                let keys = key_list(call.arg(1))?.ok_or("storage.remove: keys required")?;
                keys.into_iter().filter_map(|key| items.remove(&key).map(|old| StorageChange { key, old_value: Some(old), new_value: None })).collect()
            }
            Method::StorageClear => std::mem::take(&mut *items).into_iter().map(|(key, old)| StorageChange { key, old_value: Some(old), new_value: None }).collect(),
            _ => unreachable!("not a storage method"),
        }
    };
    storage_changed(inner, ext, "session", &changes);
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
    let scope = window_scope(inner, ext, origin);
    let find = |id: TabId| inner.tab_for(ext, id);
    let tab_or_active = || {
        TabId::from_json(call.arg(0))
            .or_else(|| inner.tabs_for(ext).into_iter().find(|t| t.active && Some(t.window_id) == scope.current).map(|t| t.id))
            .ok_or_else(|| format!("{}: no active tab", call.method.name()))
    };
    let visible = |t: &TabInfo| ext.tab_json(t);
    let no_tab = |id: TabId| format!("No tab with id: {}.", id.0);
    Ok(match call.method {
        Method::TabsQuery => Some(Value::Array(inner.tabs_for(ext).iter().filter(|t| t.matches_query(call.arg(0), ext.sees_tab(t), &scope)).map(visible).collect())),
        Method::TabsGet => {
            let id = TabId::from_json(call.arg(0)).ok_or("tabs.get: tabId must be an integer")?;
            Some(visible(&find(id).ok_or_else(|| no_tab(id))?))
        }
        Method::TabsGetCurrent => match origin {
            Origin::TabPage { tab } => find(tab).as_ref().map(visible),
            _ => None,
        },
        Method::TabsCreate => {
            let props = call.arg(0);
            let url = navigation_url(ext, call, props.get("url").and_then(Value::as_str).unwrap_or("about:blank"))?;
            let active = props.get("active").or(props.get("selected")).and_then(Value::as_bool).unwrap_or(true);
            let window = match props.get("windowId").and_then(Value::as_i64) {
                Some(id) => Some(find_window(inner, ext, &scope, id)?.id),
                None => scope.current,
            };
            let index = props.get("index").and_then(Value::as_u64).map(|i| u32::try_from(i).unwrap_or(u32::MAX));
            let id = host.create_tab(&NewTab { url: url.clone(), active, window, index }).ok_or("tabs.create: the browser refused to open a tab")?;
            Some(find(id).as_ref().map(visible).unwrap_or_else(|| {
                let mut tab = json!({ "id": id.0, "active": active });
                if ext.has_permission("tabs") || ext.host_access(&url, Some(id)) {
                    tab["url"] = json!(url);
                }
                tab
            }))
        }
        Method::TabsUpdate => {
            let id = tab_or_active()?;
            let props = call.arg(1);
            let url = props.get("url").and_then(Value::as_str).map(|u| navigation_url(ext, call, u)).transpose()?;
            let active = props.get("active").and_then(Value::as_bool);
            if find(id).is_none() || !host.update_tab(id, url.as_deref(), active) {
                return Err(no_tab(id));
            }
            find(id).as_ref().map(visible)
        }
        // As in Chrome, the tabs go one after another from `index` (-1 for the end), into
        // `windowId` or each within its own window.
        Method::TabsMove => {
            let (ids, many) = match call.arg(0) {
                Value::Array(items) => (items.iter().map(TabId::from_json).collect::<Option<Vec<_>>>(), true),
                other => (TabId::from_json(other).map(|id| vec![id]), false),
            };
            let ids = ids.ok_or("tabs.move: tabIds must be an integer or an array of integers")?;
            let props = call.arg(1);
            let mut index = props.get("index").and_then(Value::as_i64).ok_or("tabs.move: index must be an integer")?;
            let target = props.get("windowId").and_then(Value::as_i64).map(|id| find_window(inner, ext, &scope, id)).transpose()?;
            let mut moved = Vec::new();
            for id in ids {
                let tab = find(id).ok_or_else(|| no_tab(id))?;
                if target.as_ref().is_some_and(|w| w.browsing != tab.browsing) {
                    return Err(windows::ONLY_SAME_PROFILE.into());
                }
                let window = target.as_ref().map_or(tab.window_id, |w| w.id);
                if !host.move_tab(id, window, u32::try_from(index).ok()) {
                    return Err(no_tab(id));
                }
                let now = find(id).ok_or_else(|| no_tab(id))?;
                index = i64::from(now.index) + 1;
                moved.push(visible(&now));
            }
            match moved.len() {
                0 => return Err("No tabs given.".into()),
                1 if !many => moved.pop(),
                _ => Some(Value::Array(moved)),
            }
        }
        Method::TabsRemove => {
            let ids: Vec<TabId> = match call.arg(0) {
                Value::Array(items) => items.iter().filter_map(TabId::from_json).collect(),
                other => TabId::from_json(other).into_iter().collect(),
            };
            for id in ids {
                if find(id).is_none() || !host.remove_tab(id) {
                    return Err(no_tab(id));
                }
            }
            None
        }
        Method::TabsReload => {
            let id = tab_or_active()?;
            tab_view(inner, ext, id)?.reload();
            None
        }
        _ => unreachable!("not a tabs method"),
    })
}

// --- windows ----------------------------------------------------------------------------

/// The windows `WINDOW_ID_CURRENT` and the window filters mean for `ext`'s call from `origin`:
/// a page in a tab is in that tab's window, any other page in the last focused one it may know.
fn window_scope(inner: &Inner, ext: &Extension, origin: Origin) -> WindowScope {
    let last_focused = inner.windows_for(ext).first().map(|w| w.id);
    let current = origin.tab().and_then(|tab| inner.tab_for(ext, tab)).map(|t| t.window_id).or(last_focused);
    WindowScope { current, last_focused }
}

/// The window `ext`'s `windowId` names, or Chrome's error.
fn find_window(inner: &Inner, ext: &Extension, scope: &WindowScope, id: i64) -> Result<WindowInfo, String> {
    let found = scope.resolve(id).and_then(|w| inner.windows_for(ext).into_iter().find(|x| x.id == w));
    found.ok_or_else(|| if id == WINDOW_ID_CURRENT { windows::NO_CURRENT_WINDOW.to_owned() } else { windows::not_found(id) })
}

fn windows(inner: &Rc<Inner>, ext: &Rc<Extension>, origin: Origin, call: &Call) -> Result<Option<Value>, String> {
    let host = &inner.host;
    let scope = window_scope(inner, ext, origin);
    let window_id = |v: &Value| v.as_i64().ok_or_else(|| format!("{}: windowId must be an integer", call.method.name()));
    let shown = |w: &WindowInfo, populate: bool| {
        let tabs = populate.then(|| host.tabs().iter().filter(|t| t.window_id == w.id).map(|t| ext.tab_json(t)).collect());
        w.to_json(tabs)
    };
    let admitted = |id: i64, query: &WindowQuery| find_window(inner, ext, &scope, id).and_then(|w| if query.admits(&w) { Ok(w) } else { Err(windows::not_found(id)) });
    Ok(Some(match call.method {
        Method::WindowsGet => {
            let query = WindowQuery::parse(call.arg(1));
            shown(&admitted(window_id(call.arg(0))?, &query)?, query.populate)
        }
        Method::WindowsGetCurrent => {
            let query = WindowQuery::parse(call.arg(0));
            shown(&admitted(WINDOW_ID_CURRENT, &query).map_err(|_| windows::NO_CURRENT_WINDOW)?, query.populate)
        }
        Method::WindowsGetLastFocused => {
            let query = WindowQuery::parse(call.arg(0));
            let window = inner.windows_for(ext).into_iter().find(|w| query.admits(w)).ok_or(windows::NO_LAST_FOCUSED_WINDOW)?;
            shown(&window, query.populate)
        }
        Method::WindowsGetAll => {
            let query = WindowQuery::parse(call.arg(0));
            let mut all: Vec<WindowInfo> = inner.windows_for(ext).into_iter().filter(|w| query.admits(w)).collect();
            all.sort_by_key(|w| w.id);
            Value::Array(all.iter().map(|w| shown(w, query.populate)).collect())
        }
        Method::WindowsCreate => {
            let mut new = NewWindow::parse(call.arg(0))?;
            new.urls = new.urls.iter().map(|url| navigation_url(ext, call, url)).collect::<Result<_, _>>()?;
            // As in Chrome, an extension may open a private window where it does not run, but
            // not its own pages in it, and it is not told about the window.
            let unseen = !ext.runs_in(new.browsing);
            if unseen {
                let own = new.urls.iter().find(|url| ext.owns_url(url)).cloned();
                new.urls.retain(|url| !ext.owns_url(url));
                if let (true, Some(url)) = (new.urls.is_empty(), own) {
                    return Err(windows::not_in_private(&url));
                }
            }
            if let Some(id) = new.tab {
                let tab = inner.tab_for(ext, id).ok_or_else(|| format!("No tab with id: {}.", id.0))?;
                if tab.browsing != new.browsing {
                    return Err(windows::ONLY_SAME_PROFILE.into());
                }
            }
            let id = host.create_window(&new).ok_or("windows.create: the browser refused to open a window")?;
            if unseen {
                return Ok(None);
            }
            let window = host.windows().into_iter().find(|w| w.id == id).ok_or("windows.create: the new window closed")?;
            shown(&window, true)
        }
        Method::WindowsUpdate => {
            let window = find_window(inner, ext, &scope, window_id(call.arg(0))?)?;
            if !host.update_window(window.id, &WindowUpdate::parse(call.arg(1))?) {
                return Err(windows::not_found(window.id.0.into()));
            }
            let updated = host.windows().into_iter().find(|w| w.id == window.id).unwrap_or(window);
            shown(&updated, false)
        }
        Method::WindowsRemove => {
            let window = find_window(inner, ext, &scope, window_id(call.arg(0))?)?;
            if !host.remove_window(window.id) {
                return Err(windows::not_found(window.id.0.into()));
            }
            return Ok(None);
        }
        _ => unreachable!("not a windows method"),
    }))
}

/// See [`crate::patterns::navigation_url`]: relative to the calling page, and never a
/// `javascript:` or `file:` URL.
fn navigation_url(ext: &Extension, call: &Call, raw: &str) -> Result<String, String> {
    crate::patterns::navigation_url(&ext.base_url, call.url.as_deref(), raw)
}

fn open_options_page(inner: &Rc<Inner>, ext: &Rc<Extension>) -> Result<Option<Value>, String> {
    let page = ext.manifest.options_page.as_ref().ok_or("This extension has no options page")?;
    inner.open_tab(&ext.url(page.as_str())).ok_or("the browser refused to open a tab")?;
    Ok(None)
}

// --- scripting --------------------------------------------------------------------------

/// The tab view an injection may target, and the guard its source must run behind: the
/// `scripting` permission, and host access (host permissions, or an `activeTab` grant) to
/// what the tab shows, as Chrome requires. That is the committed document, not the URL the
/// web view may still be loading; while a load is pending, the script may land in either,
/// so that URL needs host access too (an `activeTab` grant covers only its own origin, as
/// it ends when the tab leaves it), and the guard stops the script in any other document.
fn injection_target(inner: &Inner, ext: &Extension, injection: &Value, api: &str) -> Result<(webkit::WebView, String), String> {
    let tab = TabId::from_json(&injection["target"]["tabId"]).ok_or("target.tabId must be an integer")?;
    if !ext.has_permission("scripting") {
        return Err(format!("{api} requires the \"scripting\" permission"));
    }
    let view = tab_view(inner, ext, tab)?;
    let denied = |url: &str| format!("Cannot access contents of url \"{url}\". Extension manifest must request permission to access this host.");
    let committed = inner.tab_info(tab).map(|t| t.url).unwrap_or_default();
    if !ext.host_access(&committed, Some(tab)) {
        return Err(denied(&committed));
    }
    let mut documents = vec![committed];
    if let Some(pending) = view.uri().map(String::from).filter(|u| *u != documents[0]) {
        let same_origin = Sender::origin_of(&pending).is_some_and(|o| Sender::origin_of(&documents[0]) == Some(o));
        if !same_origin && !ext.host_access(&pending, None) {
            return Err(denied(&pending));
        }
        documents.push(pending);
    }
    Ok((view, protocol::document_guard(&documents)))
}

/// A statement that ends the injected source unless `guard` holds.
fn guard_statement(guard: &str) -> String {
    format!("if (!{guard}) throw new Error({});\n", protocol::js_string(protocol::CANNOT_ACCESS_PAGE))
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
    let (view, guard) = match injection_target(inner, ext, injection, "scripting.executeScript") {
        Ok(v) => v,
        Err(e) => return reply.err(&e),
    };
    // A navigation may commit between the check above and the script's arrival, so the
    // script first checks which document it is in.
    let guard = guard_statement(&guard);
    // Isolated-world code gets the content-script API whether or not a manifest content
    // script ran there; MAIN-world code gets none, as in Chrome.
    let (world, bootstrap) = match injection["world"].as_str() {
        Some("MAIN") => (None, ""),
        _ => (Some(ext.world.clone()), ext.content_bootstrap.as_str()),
    };
    let what = if injection.get("files").is_some() {
        match read_files(ext, &injection["files"]) {
            Ok(source) => Injection::Files(format!("{guard}{bootstrap}{source}")),
            Err(e) => return reply.err(&e),
        }
    } else if let Some(func) = injection["func"].as_str() {
        let args = injection.get("args").cloned().unwrap_or_else(|| json!([]));
        Injection::Func { body: format!("{guard}{bootstrap}return ({func}).apply(null, {});", protocol::js_literal(&args)) }
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

/// The style sheets a `scripting.insertCSS` or `removeCSS` injection names, each with the
/// key Chrome tells them apart by (its file, or its text) and, when `read`, its text.
fn css_sources(ext: &Extension, injection: &Value, read: bool) -> Result<Vec<(String, String)>, String> {
    let given = |name: &str| injection.get(name).filter(|v| !v.is_null());
    match (given("css"), given("files")) {
        (Some(css), None) => {
            let css = css.as_str().ok_or("css must be a string")?;
            Ok(vec![(format!("css:{css}"), css.to_owned())])
        }
        (None, Some(files)) => files
            .as_array()
            .ok_or("files must be an array")?
            .iter()
            .map(|file| {
                let reference = file.as_str().ok_or("files must be strings")?;
                let missing = || format!("Could not load file: '{reference}'.");
                let path = ext.resource(reference).map_err(|_| missing())?;
                let text = if read { std::fs::read_to_string(path.resolve(&ext.dir)).map_err(|_| missing())? } else { String::new() };
                Ok((format!("file:{}", path.as_str()), text))
            })
            .collect(),
        _ => Err("Exactly one of 'css' and 'files' must be specified.".into()),
    }
}

/// `scripting.insertCSS` and `removeCSS`, answered once the page has the change. Each sheet
/// is a `<style>` that the extension's world keeps under its key, so `removeCSS` takes out
/// the last one inserted with the same file or text, as in Chrome.
fn css(inner: &Inner, ext: &Extension, call: &Call, reply: Reply) {
    let insert = call.method == Method::ScriptingInsertCss;
    let injection = call.arg(0);
    let prepared = css_sources(ext, injection, insert).and_then(|sources| injection_target(inner, ext, injection, call.method.name()).map(|target| (sources, target)));
    let (sources, (view, guard)) = match prepared {
        Ok(prepared) => prepared,
        Err(e) => return reply.err(&e),
    };
    let change = if insert {
        "for(const[key,css]of sources){const s=document.createElement('style');s.textContent=css;(document.head||document.documentElement).appendChild(s);if(!sheets.has(key))sheets.set(key,[]);sheets.get(key).push(s);}"
    } else {
        "for(const[key]of sources){const s=sheets.has(key)&&sheets.get(key).pop();if(s)s.remove();}"
    };
    let source = format!(
        "(function(){{if(!{guard})return;const sources={};const sheets=globalThis.__vsesvitCss||(globalThis.__vsesvitCss=new Map());{change}}})();",
        protocol::js_literal(&json!(sources))
    );
    view.evaluate_javascript(&source, Some(&ext.world), None, None::<&gio::Cancellable>, move |result| reply.finish(result.map(|_| None).map_err(|e| e.to_string())));
}

/// `scripting.registerContentScripts`, `getRegisteredContentScripts`, `updateContentScripts`
/// and `unregisterContentScripts` (see [`crate::dynamic_scripts`]).
fn dynamic_scripts(inner: &Inner, ext: &Extension, call: &Call) -> Result<Option<Value>, String> {
    if !ext.has_permission("scripting") {
        return Err(format!("{} requires the \"scripting\" permission", call.method));
    }
    let files = |reference: &str| ext.script_file(reference);
    {
        let mut scripts = ext.dynamic_scripts.borrow_mut();
        match call.method {
            Method::ScriptingGetRegisteredContentScripts => return Ok(Some(json!(scripts.get(call.arg(0))?))),
            Method::ScriptingRegisterContentScripts => scripts.register(call.arg(0), &files)?,
            Method::ScriptingUpdateContentScripts => scripts.update(call.arg(0), &files)?,
            Method::ScriptingUnregisterContentScripts => scripts.unregister(call.arg(0))?,
            _ => unreachable!("not a dynamic content scripts method"),
        }
    }
    inner.dynamic_scripts_changed(ext);
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

// --- contextMenus -----------------------------------------------------------------------

/// `contextMenus.create(props, generated, onclick)`, `update(id, props, onclick)`,
/// `remove(id)` and `removeAll()`. The shim keeps `onclick` functions and generates the ids
/// Chrome returns synchronously, so it says whether it did.
fn context_menus(inner: &Inner, ext: &Extension, call: &Call) -> Result<Option<Value>, String> {
    if !ext.has_permission("contextMenus") && !ext.has_permission("menus") {
        return Err(format!("{} requires the \"contextMenus\" permission", call.method));
    }
    let id = || ItemId::from_json(call.arg(0)).ok_or_else(|| format!("{}: the id must be a string or an integer", call.method));
    let flag = |i: usize| call.arg(i).as_bool().unwrap_or(false);
    let lazy = ext.lazy_background();
    {
        let mut menus = ext.menus.borrow_mut();
        match call.method {
            Method::ContextMenusCreate => menus.create(call.arg(0), flag(1), lazy, flag(2))?,
            Method::ContextMenusUpdate => menus.update(&id()?, call.arg(1), lazy, flag(2))?,
            Method::ContextMenusRemove => menus.remove(&id()?)?,
            Method::ContextMenusRemoveAll => menus.remove_all(),
            _ => unreachable!("not a contextMenus method"),
        }
    }
    inner.save_menus(ext);
    Ok(None)
}

// --- commands ---------------------------------------------------------------------------

/// `commands.getAll()`: each of the extension's commands, the action ones too, with the
/// shortcut it has now, "" for none.
fn commands(inner: &Inner, ext: &Extension) -> Result<Option<Value>, String> {
    let shortcuts = inner.profile.borrow_mut().extension_shortcuts().map_err(|e| e.to_string())?;
    let commands = shortcuts
        .iter()
        .filter(|(command, _)| command.extension == ext.id)
        .map(|(command, chord)| {
            let shortcut = chord.map(|chord| chord.to_string()).unwrap_or_default();
            json!({ "name": command.command.name, "description": command.command.description, "shortcut": shortcut })
        })
        .collect();
    Ok(Some(Value::Array(commands)))
}

// --- notifications ----------------------------------------------------------------------

/// `notifications.create(id, options, icon)`, `update(id, options, icon)`, `clear(id)`,
/// `getAll()` and `getPermissionLevel()`. The shim makes up a missing id and loads the
/// options' images first, sending the icon as base64 PNG (or `null` when `iconUrl` is absent).
fn notifications(inner: &Inner, ext: &Extension, call: &Call) -> Result<Option<Value>, String> {
    if !ext.has_permission("notifications") {
        return Err(format!("{} requires the \"notifications\" permission", call.method));
    }
    let allowed = inner.profile.borrow_mut().extensions().notifications_allowed(&ext.id);
    if call.method == Method::NotificationsGetPermissionLevel {
        return Ok(Some(json!(notifications::permission_level(allowed))));
    }
    if !allowed {
        return Err(notifications::TURNED_OFF.to_owned());
    }
    let id = call.arg(0).as_str().unwrap_or_default();
    let icon = || call.arg(2).as_str().map(glib::base64_decode).filter(|png| !png.is_empty());
    match call.method {
        Method::NotificationsCreate => {
            let shown = ext.notifications.borrow_mut().create(id, call.arg(1), icon())?;
            show_notification(ext, id, shown);
            Ok(Some(json!(id)))
        }
        Method::NotificationsUpdate => {
            let shown = ext.notifications.borrow_mut().update(id, call.arg(1), icon())?;
            let updated = shown.is_some();
            if let Some(shown) = shown {
                show_notification(ext, id, shown);
            }
            Ok(Some(json!(updated)))
        }
        Method::NotificationsClear => {
            let cleared = ext.notifications.borrow_mut().clear(id);
            if cleared {
                withdraw_notification(ext, id);
                emit_to_pages(inner, ext, "notifications.onClosed", &[json!(id), json!(false)]);
            }
            Ok(Some(json!(cleared)))
        }
        Method::NotificationsGetAll => Ok(Some(Value::Object(ext.notifications.borrow().ids().map(|id| (id.to_owned(), json!(true))).collect()))),
        _ => unreachable!("not a notifications method"),
    }
}

/// The application that shows notifications, as WebKitGTK shows a page's: the process's
/// default `GApplication`, once registered. The harness has none.
fn notifying_application() -> Option<gio::Application> {
    gio::Application::default().filter(|app| app.is_registered())
}

/// Chrome's id for an extension's notification, unique across extensions.
fn notification_id(ext: &Extension, id: &str) -> String {
    format!("{}-{id}", ext.id.as_str())
}

/// Shows (or replaces) `ext`'s notification `id` as a `GNotification` whose clicks invoke the
/// shell's [`notifications::ACTION`], with Chrome's Settings button after the extension's own.
pub(crate) fn show_notification(ext: &Extension, id: &str, shown: Shown) {
    let Some(app) = notifying_application() else {
        log::debug!("{}: notification {id:?} not shown: no registered application", ext.id.as_str());
        return;
    };
    let notification = gio::Notification::new(&shown.title);
    notification.set_body(shown.body.as_deref());
    notification.set_icon(&gio::BytesIcon::new(&glib::Bytes::from_owned(shown.icon)));
    notification.set_priority(match shown.priority {
        Priority::Normal => gio::NotificationPriority::Normal,
        Priority::High => gio::NotificationPriority::High,
        Priority::Urgent => gio::NotificationPriority::Urgent,
    });
    let action = format!("app.{}", notifications::ACTION);
    let target = |activation: Activation| notifications::action_target(ext.id.as_str(), id, activation).to_variant();
    notification.set_default_action_and_target_value(&action, Some(&target(Activation::Click)));
    for (index, title) in shown.buttons.iter().enumerate() {
        notification.add_button_with_target_value(title, &action, Some(&target(Activation::Button(index))));
    }
    notification.add_button_with_target_value("Settings", &action, Some(&target(Activation::Settings)));
    app.send_notification(Some(&notification_id(ext, id)), &notification);
}

pub(crate) fn withdraw_notification(ext: &Extension, id: &str) {
    if let Some(app) = notifying_application() {
        app.withdraw_notification(&notification_id(ext, id));
    }
}

// --- declarativeNetRequest --------------------------------------------------------------

fn dnr_permission(ext: &Extension, call: &Call) -> Result<(), String> {
    match ext.grants {
        Some(_) => Ok(()),
        None => Err(format!("{} requires the \"declarativeNetRequest\" permission", call.method)),
    }
}

/// `updateDynamicRules`, `updateSessionRules` and `updateEnabledRulesets`, which Chrome answers
/// once the rules are in effect: here, once the tabs have the content blocker made from them,
/// so that a page reloaded after the answer sees them.
fn update_rules(inner: &Rc<Inner>, ext: &Rc<Extension>, call: &Call, reply: Reply) {
    if let Err(e) = dnr_permission(ext, call) {
        return reply.err(&e);
    }
    let updated = {
        let mut rules = ext.dnr.borrow_mut();
        match call.method {
            Method::DnrUpdateDynamicRules => rules.update(Scope::Dynamic, call.arg(0)),
            Method::DnrUpdateSessionRules => rules.update(Scope::Session, call.arg(0)),
            Method::DnrUpdateEnabledRulesets => rules.update_enabled(call.arg(0)),
            _ => unreachable!("not a rules update"),
        }
    };
    if let Err(e) = updated {
        return reply.err(&e);
    }
    if call.method != Method::DnrUpdateSessionRules {
        inner.save_rules(ext);
    }
    let change = filters::compile(inner, ext);
    filters::when_compiled(ext, change, move || reply.ok(None));
}

fn rules(ext: &Extension, call: &Call) -> Result<Option<Value>, String> {
    dnr_permission(ext, call)?;
    let rules = ext.dnr.borrow();
    Ok(Some(match call.method {
        Method::DnrGetDynamicRules => json!(rules.get(Scope::Dynamic, call.arg(0))?),
        Method::DnrGetSessionRules => json!(rules.get(Scope::Session, call.arg(0))?),
        Method::DnrGetEnabledRulesets => json!(rules.enabled()),
        Method::DnrGetAvailableStaticRuleCount => json!(STATIC_RULE_BUDGET.saturating_sub(ext.compiles.static_rules.get())),
        // WebKit's regex subset, since a rule WebKit cannot compile blocks nothing.
        Method::DnrIsRegexSupported => {
            let regex = call.arg(0)["regex"].as_str().ok_or("isRegexSupported: regex must be a string")?;
            match dnr::check_webkit_regex(regex) {
                Ok(()) => json!({ "isSupported": true }),
                Err(_) => json!({ "isSupported": false, "reason": "syntaxError" }),
            }
        }
        _ => unreachable!("not a rules query"),
    }))
}

// --- webNavigation ----------------------------------------------------------------------

/// `webNavigation.getFrame(details)` and `getAllFrames(details)`, which answer `null` for a
/// tab or frame that does not exist, as in Chrome.
fn web_navigation(inner: &Inner, ext: &Extension, call: &Call) -> Result<Option<Value>, String> {
    if !ext.has_permission("webNavigation") {
        return Err(format!("{} requires the \"webNavigation\" permission", call.method));
    }
    let details = call.arg(0);
    if call.method == Method::WebNavigationGetAllFrames {
        let tab = TabId::from_json(&details["tabId"]).ok_or("webNavigation.getAllFrames: tabId must be an integer")?;
        return Ok(Some(inner.frames(ext, tab, |frames| Value::Array(frames.all_frames())).unwrap_or(Value::Null)));
    }
    let query = FrameQuery::parse(details)?;
    let found = match &query {
        FrameQuery::Frame { tab, frame } => Some((*tab, *frame)),
        FrameQuery::Document { id, .. } => inner.find_document(id).filter(|(tab, _)| inner.frames(ext, *tab, |_| ()).is_some()),
    };
    if let Some((tab, frame)) = found {
        query.agrees(tab, frame)?;
    }
    let frame = found.and_then(|(tab, frame)| inner.frames(ext, tab, |frames| frames.frame_details(frame)).flatten());
    Ok(Some(frame.map_or(Value::Null, Value::Object)))
}

// --- alarms -----------------------------------------------------------------------------

pub(crate) fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

/// Chrome fires an installed extension's alarms no more often than every 30 seconds.
const MIN_ALARM_MINUTES: f64 = 0.5;
/// Chrome's limit on one extension's alarms.
const MAX_ALARMS: usize = 500;

/// `alarms.create`'s first delay (ms from `now`) and period (minutes) from its `info`,
/// both raised to [`MIN_ALARM_MINUTES`], and whether either was.
fn alarm_timing(info: &Value, now: f64) -> (f64, Option<f64>, bool) {
    let minutes = |k: &str| info.get(k).and_then(Value::as_f64);
    let min_ms = MIN_ALARM_MINUTES * 60_000.0;
    let period = minutes("periodInMinutes").filter(|p| *p > 0.0);
    let delay_ms = if let Some(when) = minutes("when") {
        when - now
    } else if let Some(d) = minutes("delayInMinutes") {
        d * 60_000.0
    } else {
        period.map(|p| p * 60_000.0).unwrap_or(0.0)
    };
    let clamped = delay_ms < min_ms || period.is_some_and(|p| p < MIN_ALARM_MINUTES);
    (delay_ms.max(min_ms), period.map(|p| p.max(MIN_ALARM_MINUTES)), clamped)
}

fn alarms(inner: &Rc<Inner>, ext: &Rc<Extension>, call: &Call) -> Result<Option<Value>, String> {
    match call.method {
        Method::AlarmsCreate => {
            let name = call.arg(0).as_str().unwrap_or("").to_owned();
            if ext.alarms.borrow().len() >= MAX_ALARMS && !ext.alarms.borrow().contains_key(&name) {
                return Err(format!("This extension already has the maximum of {MAX_ALARMS} alarms."));
            }
            let (delay_ms, period, clamped) = alarm_timing(call.arg(1), now_ms());
            if clamped {
                log::warn!("{}: alarm \"{name}\" raised to Chrome's minimum of {MIN_ALARM_MINUTES} minutes", ext.id.as_str());
            }
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

/// A GLib timeout counts milliseconds in 32 bits, and a longer one would wrap around to
/// something short, so an alarm over 49 days away waits in legs: this one, and the
/// milliseconds still to wait after it.
fn alarm_leg(delay_ms: f64) -> (Duration, Option<f64>) {
    let delay_ms = delay_ms.round().max(0.0);
    let leg = delay_ms.min(f64::from(u32::MAX));
    (Duration::from_millis(leg as u64), (delay_ms > leg).then_some(delay_ms - leg))
}

fn schedule_alarm(inner: &Rc<Inner>, ext: &Rc<Extension>, name: String, delay_ms: f64) {
    let weak_inner = Rc::downgrade(inner);
    let weak_ext = Rc::downgrade(ext);
    let (delay, rest) = alarm_leg(delay_ms);
    let alarm_name = name.clone();
    // A periodic alarm is re-armed with a new one-shot source each time, so a changed
    // period applies.
    let source = glib::timeout_add_local_once(delay, move || {
        let (Some(inner), Some(ext)) = (weak_inner.upgrade(), weak_ext.upgrade()) else { return };
        if let Some(rest) = rest {
            if ext.alarms.borrow().contains_key(&alarm_name) {
                schedule_alarm(&inner, &ext, alarm_name, rest);
            }
            return;
        }
        let next = {
            let mut alarms = ext.alarms.borrow_mut();
            let Some(alarm) = alarms.get_mut(&alarm_name) else { return };
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
            schedule_alarm(&inner, &ext, alarm_name, period_ms);
        }
    });
    if let Some(alarm) = ext.alarms.borrow_mut().get_mut(&name) {
        alarm.source = Some(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alarms_fire_no_sooner_than_chrome_allows() {
        assert_eq!(alarm_timing(&json!({ "periodInMinutes": 1e-9 }), 0.0), (30_000.0, Some(0.5), true));
        assert_eq!(alarm_timing(&json!({ "delayInMinutes": 0.0 }), 0.0), (30_000.0, None, true));
        assert_eq!(alarm_timing(&json!({ "when": 1_000.0 }), 0.0), (30_000.0, None, true));
        assert_eq!(alarm_timing(&json!({ "delayInMinutes": 1.0, "periodInMinutes": 0.1 }), 0.0), (60_000.0, Some(0.5), true));
        assert_eq!(alarm_timing(&json!({}), 0.0), (30_000.0, None, true));
        assert_eq!(alarm_timing(&json!({ "periodInMinutes": 2.0 }), 0.0), (120_000.0, Some(2.0), false));
        assert_eq!(alarm_timing(&json!({ "when": 100_000.0 }), 10_000.0), (90_000.0, None, false));
        assert_eq!(alarm_timing(&json!({ "delayInMinutes": 0.5, "periodInMinutes": -1.0 }), 0.0), (30_000.0, None, false));
    }

    #[test]
    fn alarms_beyond_a_glib_timeout_wait_in_legs() {
        let max = f64::from(u32::MAX);
        assert_eq!(alarm_leg(60_000.0), (Duration::from_millis(60_000), None));
        assert_eq!(alarm_leg(-5.0), (Duration::ZERO, None));
        assert_eq!(alarm_leg(max), (Duration::from_millis(u64::from(u32::MAX)), None));
        // 60 days: one full leg, then the rest.
        let days_60 = 60.0 * 86_400_000.0;
        assert_eq!(alarm_leg(days_60), (Duration::from_millis(u64::from(u32::MAX)), Some(days_60 - max)));
    }
}
