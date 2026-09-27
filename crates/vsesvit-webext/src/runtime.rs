//! The runtime handle the GTK shell drives. See the crate docs for the contract.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use serde_json::{Value, json};
use vsesvit_core::Profile;
use vsesvit_core::ext_storage::StorageChange;
use vsesvit_core::extensions::{ExtensionId, InstalledExtension};
use webkit::glib;
use webkit::prelude::*;

use crate::bridge::{self, Origin};
use crate::extension::{Extension, ViewKind};
use crate::protocol::StorageArea;
use crate::tabs::{TabHost, TabId, TabInfo};
use crate::{filters, scheme, views};

/// One toolbar action, for the shell to render.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionInfo {
    pub extension: ExtensionId,
    pub title: String,
    pub icon: Option<PathBuf>,
    pub has_popup: bool,
    pub badge_text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("ruleset {path}: {reason}")]
    Ruleset { path: PathBuf, reason: String },
}

/// Cheap to clone; every clone is the same runtime.
#[derive(Clone)]
pub struct Runtime(Rc<Inner>);

pub(crate) struct Inner {
    pub(crate) profile: Rc<RefCell<Profile>>,
    pub(crate) session: webkit::NetworkSession,
    pub(crate) host: Rc<dyn TabHost>,
    pub(crate) context: webkit::WebContext,
    pub(crate) filter_store: webkit::UserContentFilterStore,
    state_dir: PathBuf,
    ui_locale: String,
    pub(crate) extensions: RefCell<BTreeMap<ExtensionId, Rc<Extension>>>,
    tabs: RefCell<BTreeMap<TabId, TabState>>,
    actions_changed: RefCell<Vec<Rc<dyn Fn()>>>,
    pub(crate) pending_filters: Cell<usize>,
    pub(crate) filters_waiters: RefCell<Vec<Box<dyn FnOnce()>>>,
    pub(crate) next_view: Cell<u64>,
}

struct TabState {
    ucm: webkit::UserContentManager,
    handlers: BTreeMap<ExtensionId, glib::SignalHandlerId>,
    last: Option<TabInfo>,
}

impl Runtime {
    /// Registers the `chrome-extension` scheme on the default `WebContext`. One per process.
    pub fn new(profile: Rc<RefCell<Profile>>, session: &webkit::NetworkSession, host: Rc<dyn TabHost>) -> Runtime {
        let state_dir = profile.borrow().paths().root.join("webext");
        for sub in ["filters", "installed"] {
            if let Err(e) = std::fs::create_dir_all(state_dir.join(sub)) {
                log::warn!("{}: {e}", state_dir.join(sub).display());
            }
        }
        let context = webkit::WebContext::default().expect("WebKit default web context");
        let filter_store = webkit::UserContentFilterStore::new(&state_dir.join("filters").to_string_lossy());
        let inner = Rc::new(Inner {
            profile,
            session: session.clone(),
            host,
            context,
            filter_store,
            state_dir,
            ui_locale: crate::i18n::ui_locale(),
            extensions: RefCell::new(BTreeMap::new()),
            tabs: RefCell::new(BTreeMap::new()),
            actions_changed: RefCell::new(Vec::new()),
            pending_filters: Cell::new(0),
            filters_waiters: RefCell::new(Vec::new()),
            next_view: Cell::new(1),
        });
        scheme::register(&inner);
        Runtime(inner)
    }

    /// The context every tab WebView must use (it is `WebContext::default()`).
    pub fn web_context(&self) -> &webkit::WebContext {
        &self.0.context
    }

    /// Load (or reload) an installed extension: content scripts into every tab, the
    /// background context, the action, and the DNR rulesets (compiled asynchronously,
    /// see [`Runtime::on_filters_ready`]).
    pub fn load(&self, installed: &InstalledExtension) -> Result<(), LoadError> {
        if self.0.extension(&installed.id).is_some() {
            self.unload(&installed.id);
        }
        let ext = Rc::new(Extension::build(installed, &self.0.ui_locale)?);
        self.0.extensions.borrow_mut().insert(ext.id.clone(), ext.clone());
        {
            let mut tabs = self.0.tabs.borrow_mut();
            for (tab, state) in tabs.iter_mut() {
                attach(&self.0, &ext, *tab, state);
            }
        }
        filters::compile(&self.0, &ext);
        let event = self.0.install_event(&ext);
        views::start_background(&self.0, &ext, event);
        self.0.notify_actions_changed();
        log::info!("{} {}: loaded from {}", ext.id.as_str(), ext.version, ext.dir.display());
        Ok(())
    }

    pub fn unload(&self, id: &ExtensionId) {
        let Some(ext) = self.0.extensions.borrow_mut().remove(id) else { return };
        {
            let mut tabs = self.0.tabs.borrow_mut();
            for state in tabs.values_mut() {
                detach(&ext, state);
            }
        }
        ext.clear_alarms();
        if let Some(bg) = ext.background.borrow_mut().take() {
            bg.load_uri("about:blank");
        }
        ext.views.borrow_mut().clear();
        self.0.notify_actions_changed();
        log::info!("{}: unloaded", id.as_str());
    }

    pub fn loaded(&self) -> Vec<ExtensionId> {
        self.0.extensions.borrow().keys().cloned().collect()
    }

    /// The `UserContentManager` to build `tab`'s WebView with. Created on first use with
    /// every loaded extension attached; the same object on later calls.
    pub fn user_content_manager(&self, tab: TabId) -> webkit::UserContentManager {
        if let Some(state) = self.0.tabs.borrow().get(&tab) {
            return state.ucm.clone();
        }
        let mut state = TabState { ucm: webkit::UserContentManager::new(), handlers: BTreeMap::new(), last: None };
        let extensions: Vec<Rc<Extension>> = self.0.extensions.borrow().values().cloned().collect();
        for ext in &extensions {
            attach(&self.0, ext, tab, &mut state);
        }
        let ucm = state.ucm.clone();
        self.0.tabs.borrow_mut().insert(tab, state);
        ucm
    }

    /// The shell reports a navigation or title change; extensions see `tabs.onUpdated`
    /// with the fields that changed since the last report.
    pub fn tab_updated(&self, tab: TabId) {
        let Some(info) = self.0.tab_info(tab) else { return };
        let previous = {
            let mut tabs = self.0.tabs.borrow_mut();
            let Some(state) = tabs.get_mut(&tab) else { return };
            state.last.replace(info.clone())
        };
        let mut change = serde_json::Map::new();
        if previous.as_ref().is_none_or(|p| p.url != info.url) {
            change.insert("url".into(), json!(info.url));
        }
        if previous.as_ref().is_none_or(|p| p.title != info.title) {
            change.insert("title".into(), json!(info.title));
        }
        change.insert("status".into(), json!("complete"));
        self.0.emit_to_all_pages("tabs.onUpdated", &[json!(tab.0), Value::Object(change), info.to_json()]);
    }

    pub fn tab_activated(&self, tab: TabId) {
        let window_id = self.0.tab_info(tab).map(|t| t.window_id).unwrap_or(1);
        self.0.emit_to_all_pages("tabs.onActivated", &[json!({ "tabId": tab.0, "windowId": window_id })]);
    }

    pub fn tab_closed(&self, tab: TabId) {
        let removed = self.0.tabs.borrow_mut().remove(&tab);
        let Some(mut state) = removed else { return };
        let extensions: Vec<Rc<Extension>> = self.0.extensions.borrow().values().cloned().collect();
        for ext in &extensions {
            detach(ext, &mut state);
        }
        let window_id = state.last.as_ref().map(|t| t.window_id).unwrap_or(1);
        self.0.emit_to_all_pages("tabs.onRemoved", &[json!(tab.0), json!({ "windowId": window_id, "isWindowClosing": false })]);
    }

    pub fn actions(&self) -> Vec<ActionInfo> {
        self.0
            .extensions
            .borrow()
            .values()
            .filter_map(|ext| {
                let state = ext.action.borrow().clone()?;
                Some(ActionInfo {
                    extension: ext.id.clone(),
                    title: state.title,
                    icon: state.icon,
                    has_popup: !state.popup.is_empty(),
                    badge_text: state.badge_text,
                })
            })
            .collect()
    }

    /// Called after load/unload and after `action.setBadgeText`/`setTitle`/`setIcon`/`setPopup`.
    pub fn connect_actions_changed(&self, f: impl Fn() + 'static) {
        self.0.actions_changed.borrow_mut().push(Rc::new(f));
    }

    /// The user clicked the action. Returns the popup WebView (already loading) when the
    /// action has a popup; the shell owns it and drops it to close. Otherwise fires
    /// `action.onClicked` with `tab` and returns `None`.
    pub fn activate_action(&self, id: &ExtensionId, tab: Option<TabId>) -> Option<webkit::WebView> {
        let ext = self.0.extension(id)?;
        let state = ext.action.borrow().clone()?;
        if state.popup.is_empty() {
            let tab_json = tab.and_then(|t| self.0.tab_json(t)).unwrap_or(Value::Null);
            bridge::emit_to_pages(&ext, "action.onClicked", &[tab_json]);
            return None;
        }
        let view = views::build(&self.0, &ext, ViewKind::Popup);
        view.load_uri(&ext.url(&state.popup));
        Some(view)
    }

    /// `storage.sync` changed remotely (a sync engine's `ApplyReport`): fire
    /// `storage.onChanged` in every context of that extension.
    pub fn storage_sync_changed(&self, ext: &ExtensionId, changes: &[StorageChange]) {
        if let Some(ext) = self.0.extension(ext) {
            bridge::storage_changed(&self.0, &ext, StorageArea::Sync, changes);
        }
    }

    /// Run `f` once no declarativeNetRequest ruleset is still compiling (immediately if
    /// none is).
    pub fn on_filters_ready(&self, f: impl FnOnce() + 'static) {
        if self.0.pending_filters.get() == 0 {
            f();
        } else {
            self.0.filters_waiters.borrow_mut().push(Box::new(f));
        }
    }

    pub fn pending_filters(&self) -> usize {
        self.0.pending_filters.get()
    }
}

fn attach(inner: &Rc<Inner>, ext: &Rc<Extension>, tab: TabId, state: &mut TabState) {
    for script in &ext.scripts {
        state.ucm.add_script(script);
    }
    for style in &ext.styles {
        state.ucm.add_style_sheet(style);
    }
    if let Some(filter) = ext.filter.borrow().as_ref() {
        state.ucm.add_filter(filter);
    }
    let handler = bridge::register(inner, &state.ucm, ext, Origin::Content { tab }, Some(&ext.world));
    state.handlers.insert(ext.id.clone(), handler);
}

fn detach(ext: &Extension, state: &mut TabState) {
    for script in &ext.scripts {
        state.ucm.remove_script(script);
    }
    for style in &ext.styles {
        state.ucm.remove_style_sheet(style);
    }
    if let Some(filter) = ext.filter.borrow().as_ref() {
        state.ucm.remove_filter(filter);
    }
    if let Some(handler) = state.handlers.remove(&ext.id) {
        state.ucm.disconnect(handler);
        state.ucm.unregister_script_message_handler(&ext.handler, Some(&ext.world));
    }
}

impl Inner {
    pub(crate) fn extension(&self, id: &ExtensionId) -> Option<Rc<Extension>> {
        self.extensions.borrow().get(id).cloned()
    }

    pub(crate) fn tab_managers(&self) -> Vec<webkit::UserContentManager> {
        self.tabs.borrow().values().map(|s| s.ucm.clone()).collect()
    }

    /// WebViews of every tab the shell registered. Never called with `tabs` borrowed,
    /// since the host may call back into the runtime.
    pub(crate) fn tab_views(&self) -> Vec<webkit::WebView> {
        let ids: Vec<TabId> = self.tabs.borrow().keys().copied().collect();
        ids.into_iter().filter_map(|id| self.host.web_view(id)).collect()
    }

    pub(crate) fn tab_info(&self, tab: TabId) -> Option<TabInfo> {
        self.host.tabs().into_iter().find(|t| t.id == tab)
    }

    pub(crate) fn tab_json(&self, tab: TabId) -> Option<Value> {
        self.tab_info(tab).map(|t| t.to_json())
    }

    pub(crate) fn emit_to_all_pages(&self, event: &str, args: &[Value]) {
        let extensions: Vec<Rc<Extension>> = self.extensions.borrow().values().cloned().collect();
        for ext in extensions {
            bridge::emit_to_pages(&ext, event, args);
        }
    }

    pub(crate) fn notify_actions_changed(&self) {
        // Cloned first: a listener may connect another listener or query the runtime.
        let listeners: Vec<Rc<dyn Fn()>> = self.actions_changed.borrow().clone();
        for f in listeners {
            f();
        }
    }

    /// First load of this version fires `runtime.onInstalled`; later loads `onStartup`.
    /// The marker is a file under `<profile>/webext/installed/`.
    fn install_event(&self, ext: &Extension) -> views::InstallEvent {
        let marker = self.state_dir.join("installed").join(&ext.host);
        let previous = std::fs::read_to_string(&marker).ok().map(|s| s.trim().to_owned());
        let event = match previous {
            None => views::InstallEvent::Installed,
            Some(v) if v != ext.version => views::InstallEvent::Updated { previous: v },
            Some(_) => views::InstallEvent::Startup,
        };
        if !matches!(event, views::InstallEvent::Startup)
            && let Err(e) = std::fs::write(&marker, &ext.version)
        {
            log::warn!("{}: {e}", marker.display());
        }
        event
    }
}
