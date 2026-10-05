//! The runtime handle the GTK shell drives. See the crate docs for the contract.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::UNIX_EPOCH;

use serde_json::{Value, json};
use vsesvit_core::Profile;
use vsesvit_core::ext_storage::{Area, StorageChange};
use vsesvit_core::extensions::{ExtensionId, InstalledExtension};
use webkit::glib;
use webkit::prelude::*;

use crate::bridge::{self, Origin, PortContext, Reply};
use crate::extension::Extension;
use crate::lifecycle::{self, InstallEvent, LoadReason};
use crate::menus::{Entry, ItemId, Target};
use crate::messaging::Ports;
use crate::protocol::Sender;
use crate::tabs::{TabHost, TabId, TabInfo};
use crate::{filters, patterns, scheme, views};

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
    pub(crate) ports: RefCell<Ports<PortContext, Reply>>,
}

struct TabState {
    ucm: webkit::UserContentManager,
    /// The content-script and page handlers of each attached extension.
    handlers: BTreeMap<ExtensionId, [glib::SignalHandlerId; 2]>,
    last: Option<TabInfo>,
}

impl Runtime {
    /// Registers the `chrome-extension` scheme on the default `WebContext`. One per process.
    pub fn new(profile: Rc<RefCell<Profile>>, session: &webkit::NetworkSession, host: Rc<dyn TabHost>) -> Runtime {
        let state_dir = profile.borrow().paths().root.join("webext");
        for sub in ["filters", "installed", "menus"] {
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
            ui_locale: vsesvit_core::extensions::ui_locale(),
            extensions: RefCell::new(BTreeMap::new()),
            tabs: RefCell::new(BTreeMap::new()),
            actions_changed: RefCell::new(Vec::new()),
            pending_filters: Cell::new(0),
            filters_waiters: RefCell::new(Vec::new()),
            next_view: Cell::new(1),
            ports: RefCell::new(Ports::default()),
        });
        scheme::register(&inner);
        Runtime(inner)
    }

    /// The context every tab WebView must use (it is `WebContext::default()`).
    pub fn web_context(&self) -> &webkit::WebContext {
        &self.0.context
    }

    /// Load (or reload) an installed extension at browser startup or right after an
    /// install: content scripts into every tab, the background context, the action, and
    /// the DNR rulesets (compiled asynchronously, see [`Runtime::on_filters_ready`]).
    /// The background gets `runtime.onInstalled` on the first load of an install or
    /// version, else `runtime.onStartup`.
    pub fn load(&self, installed: &InstalledExtension) -> Result<(), LoadError> {
        self.load_with(installed, LoadReason::Startup)
    }

    /// [`Runtime::load`] with the shell's reason: [`LoadReason::Enable`] fires no
    /// `runtime.onStartup`, as Chrome fires none when the user re-enables an extension.
    pub fn load_with(&self, installed: &InstalledExtension, reason: LoadReason) -> Result<(), LoadError> {
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
        let event = self.0.install_event(&ext, reason);
        self.0.restore_menus(&ext, &event);
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
        self.0.close_ports(|c| c.ext == *id);
        if let Some(bg) = ext.background.borrow_mut().take() {
            bg.load_uri("about:blank");
        }
        ext.views.borrow_mut().clear();
        ext.background_loaded();
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
        for ext in &self.0.loaded_extensions() {
            attach(&self.0, ext, tab, &mut state);
        }
        let ucm = state.ucm.clone();
        self.0.tabs.borrow_mut().insert(tab, state);
        ucm
    }

    /// Whether the page at `source` may navigate a view to `target`, which a shell asks
    /// through each tab's [`Gate`](crate::Gate). As in Chrome, a web page reaches an
    /// extension's pages only where `web_accessible_resources` lets it; the extension itself
    /// reaches them all, and an unloaded extension's URL is left to fail on its own.
    pub fn may_navigate(&self, source: &str, target: &str) -> bool {
        if patterns::may_enter(source, target) {
            return true;
        }
        let Some((host, path)) = scheme::split_uri(target) else { return true };
        self.0.extension_by_host(&host).is_none_or(|ext| ext.web_accessible(&path, source))
    }

    /// Whether `url` is a page of a loaded extension that some page outside it may load.
    pub fn web_reachable(&self, url: &str) -> bool {
        let Some((host, path)) = scheme::split_uri(url) else { return false };
        self.0.extension_by_host(&host).is_some_and(|ext| ext.web_reachable(&path))
    }

    /// The shell reports a navigation or title change; extensions see `tabs.onUpdated`
    /// with the fields that changed since the last report, minus the URL and title when
    /// they may not see the tab's contents. Leaving the origin ends `activeTab` grants.
    pub fn tab_updated(&self, tab: TabId) {
        let Some(info) = self.0.tab_info(tab) else { return };
        let previous = {
            let mut tabs = self.0.tabs.borrow_mut();
            let Some(state) = tabs.get_mut(&tab) else { return };
            state.last.replace(info.clone())
        };
        let url_changed = previous.as_ref().is_none_or(|p| p.url != info.url);
        let title_changed = previous.as_ref().is_none_or(|p| p.title != info.title);
        let origin_changed = previous.as_ref().is_some_and(|p| Sender::origin_of(&p.url) != Sender::origin_of(&info.url));
        for ext in self.0.loaded_extensions() {
            if origin_changed {
                ext.revoke_active_tab(tab);
            }
            let sees = ext.sees_tab(&info);
            let mut change = serde_json::Map::new();
            if url_changed && sees {
                change.insert("url".into(), json!(info.url));
            }
            if title_changed && sees {
                change.insert("title".into(), json!(info.title));
            }
            change.insert("status".into(), json!("complete"));
            bridge::emit_to_pages(&self.0, &ext, "tabs.onUpdated", &[json!(tab.0), Value::Object(change), info.to_json_for(sees)]);
        }
    }

    pub fn tab_activated(&self, tab: TabId) {
        let window_id = self.0.tab_info(tab).map(|t| t.window_id).unwrap_or(1);
        self.0.emit_to_all_pages("tabs.onActivated", &[json!({ "tabId": tab.0, "windowId": window_id })]);
    }

    pub fn tab_closed(&self, tab: TabId) {
        let removed = self.0.tabs.borrow_mut().remove(&tab);
        let Some(mut state) = removed else { return };
        self.0.close_ports(|c| c.origin.tab() == Some(tab));
        for ext in &self.0.loaded_extensions() {
            detach(ext, &mut state);
            ext.revoke_active_tab(tab);
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

    /// The user clicked the action on `tab` (which grants `activeTab` there). When the
    /// action has a popup, `show` gets its WebView (already loading) once it exists, which
    /// may be after this returns; the shell owns it and drops it to close. Otherwise fires
    /// `action.onClicked` with `tab`.
    pub fn activate_action(&self, id: &ExtensionId, tab: Option<TabId>, show: impl FnOnce(webkit::WebView) + 'static) {
        let Some(ext) = self.0.extension(id) else { return };
        let Some(state) = ext.action.borrow().clone() else { return };
        if let Some(tab) = tab {
            ext.grant_active_tab(tab);
        }
        if state.popup.is_empty() {
            let tab_json = tab.and_then(|t| self.0.tab_info(t)).map(|t| ext.tab_json(&t)).unwrap_or(Value::Null);
            bridge::emit_to_pages(&self.0, &ext, "action.onClicked", &[tab_json]);
            return;
        }
        views::open_popup(&self.0, &ext, ext.url(&state.popup), Box::new(show));
    }

    /// What the loaded extensions add to a page's context menu for a click on `target`: one
    /// entry each (see [`crate::menus::Menus::page_entry`]), ordered by extension name as in
    /// Chrome.
    pub fn page_menu(&self, target: &Target) -> Vec<(ExtensionId, Entry)> {
        let mut found: Vec<(String, ExtensionId, Entry)> = self
            .0
            .loaded_extensions()
            .iter()
            .filter_map(|ext| Some((ext.manifest.name.to_lowercase(), ext.id.clone(), ext.menus.borrow().page_entry(&ext.manifest.name, target)?)))
            .collect();
        found.sort_by(|a, b| a.0.cmp(&b.0));
        found.into_iter().map(|(_, id, entry)| (id, entry)).collect()
    }

    /// The items `id` adds to its toolbar action's menu.
    pub fn action_menu(&self, id: &ExtensionId) -> Vec<Entry> {
        self.0.extension(id).map(|ext| ext.menus.borrow().action_entries()).unwrap_or_default()
    }

    /// The user chose `item`, one of `id`'s, in `tab`'s context menu, opened on `target`, or
    /// in its action's menu (no `target`; `tab` is the selected one). As in Chrome, that
    /// grants `activeTab` on the tab and fires `contextMenus.onClicked` with the click and the
    /// tab.
    pub fn menu_clicked(&self, id: &ExtensionId, item: &ItemId, tab: Option<TabId>, target: Option<&Target>) {
        let Some(ext) = self.0.extension(id) else { return };
        let Some(info) = ext.menus.borrow_mut().click(item, target) else { return };
        self.0.save_menus(&ext);
        if let Some(tab) = tab {
            ext.grant_active_tab(tab);
        }
        let mut args = vec![info];
        args.extend(tab.and_then(|t| self.0.tab_info(t)).map(|t| ext.tab_json(&t)));
        let (inner, target) = (self.0.clone(), ext.clone());
        ext.when_background_loaded(move || bridge::emit_to_pages(&inner, &target, "contextMenus.onClicked", &args));
    }

    /// `storage.sync` changed remotely (a sync engine's `ApplyReport`): fire
    /// `storage.onChanged` in every context of that extension.
    pub fn storage_sync_changed(&self, ext: &ExtensionId, changes: &[StorageChange]) {
        if let Some(ext) = self.0.extension(ext) {
            bridge::storage_changed(&self.0, &ext, Area::Sync, changes);
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

/// `runtime.reload()`: load the extension afresh from its install, as a re-enable (Chrome
/// fires no lifecycle event when a packed extension reloads), so its background, alarms
/// and content scripts start over. Tabs showing its pages reload onto the new instance,
/// whose API the old documents can no longer reach.
pub(crate) fn reload(inner: &Rc<Inner>, id: &ExtensionId) {
    let installed = inner.profile.borrow_mut().extensions().get(id);
    let result = match installed {
        Ok(Some(installed)) => Runtime(inner.clone()).load_with(&installed, LoadReason::Enable).map_err(|e| e.to_string()),
        Ok(None) => Err("no longer installed".to_owned()),
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = result {
        log::warn!("{}: runtime.reload: {e}", id.as_str());
        return;
    }
    let Some(ext) = inner.extension(id) else { return };
    for (_, view) in inner.page_tab_views(&ext) {
        view.reload();
    }
}

/// Content scripts in the extension's world, the page shim (default world, the
/// extension's own documents only) and one handler for each, so an extension page the
/// tab navigates to has its API.
fn attach(inner: &Rc<Inner>, ext: &Rc<Extension>, tab: TabId, state: &mut TabState) {
    for script in &ext.scripts {
        state.ucm.add_script(script);
    }
    for style in &ext.styles {
        state.ucm.add_style_sheet(style);
    }
    state.ucm.add_script(&ext.page_script);
    if let Some(filter) = ext.filter.borrow().as_ref() {
        state.ucm.add_filter(filter);
    }
    let content = bridge::register(inner, &state.ucm, ext, Origin::Content { tab }, Some(&ext.world));
    let page = bridge::register(inner, &state.ucm, ext, Origin::TabPage { tab }, None);
    state.handlers.insert(ext.id.clone(), [content, page]);
}

fn detach(ext: &Extension, state: &mut TabState) {
    for script in &ext.scripts {
        state.ucm.remove_script(script);
    }
    for style in &ext.styles {
        state.ucm.remove_style_sheet(style);
    }
    state.ucm.remove_script(&ext.page_script);
    if let Some(filter) = ext.filter.borrow().as_ref() {
        state.ucm.remove_filter(filter);
    }
    if let Some([content, page]) = state.handlers.remove(&ext.id) {
        state.ucm.disconnect(content);
        state.ucm.disconnect(page);
        state.ucm.unregister_script_message_handler(&ext.handler, Some(&ext.world));
        state.ucm.unregister_script_message_handler(&ext.page_handler, None);
    }
}

impl crate::gate::Policy for Runtime {
    fn may_navigate(&self, source: &str, target: &str) -> bool {
        Runtime::may_navigate(self, source, target)
    }

    fn web_reachable(&self, url: &str) -> bool {
        Runtime::web_reachable(self, url)
    }
}

impl Inner {
    pub(crate) fn extension(&self, id: &ExtensionId) -> Option<Rc<Extension>> {
        self.extensions.borrow().get(id).cloned()
    }

    /// Every loaded extension, snapshotted so callers never hold `extensions` borrowed
    /// while calling into an extension or the host (which may load or unload one).
    pub(crate) fn loaded_extensions(&self) -> Vec<Rc<Extension>> {
        self.extensions.borrow().values().cloned().collect()
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

    /// The tabs whose view is showing (or loading) one of `ext`'s own documents.
    pub(crate) fn page_tab_views(&self, ext: &Extension) -> Vec<(TabId, webkit::WebView)> {
        let ids: Vec<TabId> = self.tabs.borrow().keys().copied().collect();
        ids.into_iter()
            .filter_map(|id| self.host.web_view(id).map(|v| (id, v)))
            .filter(|(_, v)| v.uri().is_some_and(|u| ext.owns_url(&u)))
            .collect()
    }

    pub(crate) fn tab_info(&self, tab: TabId) -> Option<TabInfo> {
        self.host.tabs().into_iter().find(|t| t.id == tab)
    }

    pub(crate) fn emit_to_all_pages(&self, event: &str, args: &[Value]) {
        for ext in self.loaded_extensions() {
            bridge::emit_to_pages(self, &ext, event, args);
        }
    }

    /// Contexts that went away lose their ports; see [`Ports::close_where`].
    pub(crate) fn close_ports(&self, gone: impl Fn(&PortContext) -> bool) {
        let wakes = self.ports.borrow_mut().close_where(gone);
        bridge::wake(wakes);
    }

    pub(crate) fn notify_actions_changed(&self) {
        // Cloned first: a listener may connect another listener or query the runtime.
        let listeners: Vec<Rc<dyn Fn()>> = self.actions_changed.borrow().clone();
        for f in listeners {
            f();
        }
    }

    fn menus_file(&self, ext: &Extension) -> PathBuf {
        self.state_dir.join("menus").join(format!("{}.json", ext.host))
    }

    /// Chrome keeps the context menu items of a lazy background (which runs again only for
    /// events) across restarts, and drops them on an install or update, whose
    /// `runtime.onInstalled` makes them again.
    fn restore_menus(&self, ext: &Extension, event: &InstallEvent) {
        let file = self.menus_file(ext);
        if matches!(event, InstallEvent::Installed | InstallEvent::Updated { .. }) {
            if let Err(e) = std::fs::remove_file(&file)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                log::warn!("{}: {e}", file.display());
            }
            return;
        }
        if !ext.lazy_background() {
            return;
        }
        match std::fs::read_to_string(&file).map(|text| serde_json::from_str(&text)) {
            Ok(Ok(menus)) => *ext.menus.borrow_mut() = menus,
            Ok(Err(e)) => log::warn!("{}: {e}", file.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("{}: {e}", file.display()),
        }
    }

    pub(crate) fn save_menus(&self, ext: &Extension) {
        if !ext.lazy_background() {
            return;
        }
        let file = self.menus_file(ext);
        let written = serde_json::to_string(&*ext.menus.borrow()).map_err(std::io::Error::other).and_then(|json| std::fs::write(&file, json));
        if let Err(e) = written {
            log::warn!("{}: {e}", file.display());
        }
    }

    /// See [`lifecycle::install_event`]. The marker is a file under
    /// `<profile>/webext/installed/`; the install stamp is the extension directory's
    /// modification time, which a reinstall (a new directory) changes.
    fn install_event(&self, ext: &Extension, reason: LoadReason) -> InstallEvent {
        let marker = self.state_dir.join("installed").join(&ext.host);
        let previous = std::fs::read_to_string(&marker).ok();
        let stamp = std::fs::metadata(&ext.dir)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_default();
        let (event, updated) = lifecycle::install_event(previous.as_deref(), &ext.version, &stamp, reason);
        if let Some(text) = updated
            && let Err(e) = std::fs::write(&marker, text)
        {
            log::warn!("{}: {e}", marker.display());
        }
        event
    }
}
