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
use vsesvit_core::private::Browsing;
use webkit::glib;
use webkit::prelude::*;

use crate::bridge::{self, Origin, PortContext, Reply};
use crate::dnr_rules::{Rules, Saved};
use crate::dynamic_scripts::Scripts;
use crate::extension::Extension;
use crate::lifecycle::{self, InstallEvent, LoadReason};
use crate::menus::{Entry, ItemId, Target};
use crate::messaging::Ports;
use crate::notifications::{self, Activation, Shown};
use crate::protocol::Sender;
use crate::tabs::{NewTab, TabHost, TabId, TabInfo};
use crate::web_navigation::{Event, EventKind, FrameId, Frames, Load, Report};
use crate::windows::{self, WindowId, WindowInfo};
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
    /// The windows as last reported, which the next report is compared with.
    windows: RefCell<Vec<WindowInfo>>,
    actions_changed: RefCell<Vec<Rc<dyn Fn()>>>,
    pub(crate) pending_filters: Cell<usize>,
    pub(crate) filters_waiters: RefCell<Vec<Box<dyn FnOnce()>>>,
    pub(crate) next_view: Cell<u64>,
    pub(crate) ports: RefCell<Ports<PortContext, Reply>>,
    /// The frame script every tab gets (see [`watch_frames`]).
    frames_script: webkit::UserScript,
}

/// The frame script's world, which no extension can share: no extension id has a colon.
const FRAMES_WORLD: &str = "vsesvit:frames";
const FRAMES_HANDLER: &str = "vsesvitFrames";

struct TabState {
    ucm: webkit::UserContentManager,
    browsing: Browsing,
    /// The content-script and page handlers of each attached extension.
    handlers: BTreeMap<ExtensionId, [glib::SignalHandlerId; 2]>,
    last: Option<TabInfo>,
    /// Where the tab was after the last change to any tab's place; `None` until the shell
    /// first puts it in a window.
    placed: Option<(WindowId, u32)>,
    frames: Frames,
    /// webNavigation events from before the shell put the tab in a window.
    held: Vec<Event>,
    /// The tab whose page opened this one, until its first navigation says so.
    opener: Option<TabId>,
}

impl Runtime {
    /// Registers the `chrome-extension` scheme on the default `WebContext`. One per process.
    pub fn new(profile: Rc<RefCell<Profile>>, session: &webkit::NetworkSession, host: Rc<dyn TabHost>) -> Runtime {
        let state_dir = profile.borrow().paths().root.join("webext");
        for sub in ["dnr", "filters", "installed", "menus", "scripts"] {
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
            windows: RefCell::new(Vec::new()),
            actions_changed: RefCell::new(Vec::new()),
            pending_filters: Cell::new(0),
            filters_waiters: RefCell::new(Vec::new()),
            next_view: Cell::new(1),
            ports: RefCell::new(Ports::default()),
            frames_script: webkit::UserScript::for_world(
                include_str!("js/frames.js"),
                webkit::UserContentInjectedFrames::AllFrames,
                webkit::UserScriptInjectionTime::Start,
                FRAMES_WORLD,
                &[],
                &[],
            ),
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
        ext.in_private.set(self.0.profile.borrow_mut().extensions().allowed_in_private(&ext.id));
        let event = self.0.install_event(&ext, reason);
        self.0.restore_scripts(&ext, &event);
        self.0.extensions.borrow_mut().insert(ext.id.clone(), ext.clone());
        {
            let mut tabs = self.0.tabs.borrow_mut();
            for (tab, state) in tabs.iter_mut().filter(|(_, state)| ext.runs_in(state.browsing)) {
                attach(&self.0, &ext, *tab, state);
            }
        }
        self.0.restore_rules(&ext, &event);
        filters::compile(&self.0, &ext);
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
        for notification in ext.notifications.borrow_mut().take_all() {
            bridge::withdraw_notification(&ext, &notification);
        }
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

    /// Whether loaded extension `id` runs in tabs of `browsing`'s kind (see
    /// [`Extension::runs_in`]), so the shell offers its action and menu items there.
    pub fn runs_in(&self, id: &ExtensionId, browsing: Browsing) -> bool {
        self.0.extension(id).is_some_and(|ext| ext.runs_in(browsing))
    }

    /// The user's "Allow in private windows" changed, here or through sync: each loaded
    /// extension joins or leaves the open private tabs at once, for loads that start afterwards.
    pub fn allowed_in_private_changed(&self) {
        for ext in self.0.loaded_extensions() {
            let allowed = self.0.profile.borrow_mut().extensions().allowed_in_private(&ext.id);
            if ext.in_private.replace(allowed) == allowed {
                continue;
            }
            let mut tabs = self.0.tabs.borrow_mut();
            for (tab, state) in tabs.iter_mut().filter(|(_, state)| state.browsing == Browsing::Private) {
                if allowed {
                    attach(&self.0, &ext, *tab, state);
                } else {
                    detach(&ext, state);
                    ext.revoke_active_tab(*tab);
                }
            }
        }
    }

    /// The `UserContentManager` to build `tab`'s WebView with, a tab of a `browsing` window for
    /// its whole life. Created on first use with every loaded extension that runs in such tabs
    /// attached; the same object on later calls.
    pub fn user_content_manager(&self, tab: TabId, browsing: Browsing) -> webkit::UserContentManager {
        if let Some(state) = self.0.tabs.borrow().get(&tab) {
            return state.ucm.clone();
        }
        let mut state = TabState {
            ucm: webkit::UserContentManager::new(),
            browsing,
            handlers: BTreeMap::new(),
            last: None,
            placed: None,
            frames: Frames::new(tab),
            held: Vec::new(),
            opener: None,
        };
        watch_frames(&self.0, &state.ucm, tab);
        for ext in self.0.loaded_extensions().iter().filter(|ext| ext.runs_in(browsing)) {
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
        for ext in self.0.loaded_extensions().into_iter().filter(|ext| ext.runs_in(info.browsing)) {
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
        let Some(info) = self.0.tab_info(tab) else { return };
        self.0.emit_about_tab(info.browsing, "tabs.onActivated", &[json!({ "tabId": tab.0, "windowId": info.window_id })]);
    }

    /// The shell put `tab` into a window: a new tab (`tabs.onCreated`), or one from another
    /// window (`tabs.onDetached`, then `tabs.onAttached`). Report it before selecting the tab,
    /// so that, as in Chrome, `tabs.onActivated` comes after.
    pub fn tab_attached(&self, tab: TabId) {
        let Some(info) = self.0.tab_info(tab) else { return };
        let placed = self.0.tabs.borrow().get(&tab).map(|s| s.placed);
        match placed {
            Some(None) => {
                for ext in self.0.loaded_extensions().into_iter().filter(|ext| ext.runs_in(info.browsing)) {
                    bridge::emit_to_pages(&self.0, &ext, "tabs.onCreated", &[ext.tab_json(&info)]);
                }
                self.0.place_tabs();
                // What it loaded meanwhile.
                self.0.navigated(tab, |_| Vec::new());
                return;
            }
            Some(Some((window, index))) if window != info.window_id => {
                self.0.emit_about_tab(info.browsing, "tabs.onDetached", &[json!(tab.0), json!({ "oldWindowId": window, "oldPosition": index })]);
                self.0.emit_about_tab(info.browsing, "tabs.onAttached", &[json!(tab.0), json!({ "newWindowId": info.window_id, "newPosition": info.index })]);
            }
            _ => {}
        }
        self.0.place_tabs();
    }

    /// The shell moved `tab` within its window: `tabs.onMoved`.
    pub fn tab_moved(&self, tab: TabId) {
        let placed = self.0.tabs.borrow().get(&tab).and_then(|s| s.placed);
        if let (Some(info), Some((window, from))) = (self.0.tab_info(tab), placed)
            && window == info.window_id
            && from != info.index
        {
            self.0.emit_about_tab(info.browsing, "tabs.onMoved", &[json!(tab.0), json!({ "windowId": window, "fromIndex": from, "toIndex": info.index })]);
        }
        self.0.place_tabs();
    }

    /// The shell reports each load of `tab`'s top frame, which, with what the frames' own
    /// script says, `chrome.webNavigation` tells extensions (see [`crate::web_navigation`]).
    pub fn tab_load(&self, tab: TabId, load: Load) {
        self.0.navigated(tab, |frames| frames.load(load));
    }

    /// `tab` holds a page that `opener`'s page opened: a link to a new tab or window, or a
    /// `window.open`. Report it before the shell puts the tab in a window; its first
    /// navigation then fires `webNavigation.onCreatedNavigationTarget`.
    pub fn tab_opened_by(&self, tab: TabId, opener: TabId) {
        if let Some(state) = self.0.tabs.borrow_mut().get_mut(&tab) {
            state.opener = Some(opener);
        }
    }

    /// `window_closing` when the tab goes with its window, which extensions are told. A tab
    /// the shell never put in a window was never announced, so nothing says it went.
    pub fn tab_closed(&self, tab: TabId, window_closing: bool) {
        let removed = self.0.tabs.borrow_mut().remove(&tab);
        let Some(mut state) = removed else { return };
        self.0.close_ports(|c| c.origin.tab() == Some(tab));
        for ext in &self.0.loaded_extensions() {
            detach(ext, &mut state);
            ext.revoke_active_tab(tab);
        }
        if let Some((window, _)) = state.placed {
            self.0.emit_about_tab(state.browsing, "tabs.onRemoved", &[json!(tab.0), json!({ "windowId": window, "isWindowClosing": window_closing })]);
        }
        self.0.place_tabs();
    }

    /// The shell's windows changed: one opened or closed, took or lost the focus, or was
    /// resized. Each extension hears what changed since the last report among the windows it
    /// may know ([`windows::changes`]), so focus going to a private window it does not run in
    /// reaches it as `WINDOW_ID_NONE`, as in Chrome.
    pub fn windows_changed(&self) {
        let now = self.0.host.windows();
        let before = self.0.windows.replace(now.clone());
        for ext in self.0.loaded_extensions() {
            let known = |windows: &[WindowInfo]| windows.iter().filter(|w| ext.runs_in(w.browsing)).cloned().collect::<Vec<_>>();
            for event in windows::changes(&known(&before), &known(&now)) {
                bridge::emit_to_pages(&self.0, &ext, event.name(), &event.args());
            }
        }
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
            let tab_json = tab.and_then(|t| self.0.tab_for(&ext, t)).map(|t| ext.tab_json(&t)).unwrap_or(Value::Null);
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
        args.extend(tab.and_then(|t| self.0.tab_for(&ext, t)).map(|t| ext.tab_json(&t)));
        let (inner, target) = (self.0.clone(), ext.clone());
        ext.when_background_loaded(move || bridge::emit_to_pages(&inner, &target, "contextMenus.onClicked", &args));
    }

    /// The user pressed the shortcut of `id`'s command `name` with `tab` selected. As in
    /// Chrome, that grants `activeTab` on the tab and fires `commands.onCommand` with the name
    /// and the tab. The action commands (`_execute_action` and its MV2 names) are the shell's:
    /// it activates the action ([`Runtime::activate_action`]) instead.
    pub fn command(&self, id: &ExtensionId, name: &str, tab: Option<TabId>) {
        let Some(ext) = self.0.extension(id) else { return };
        if !ext.manifest.commands.iter().any(|c| c.name == name && !c.activates_action()) {
            return;
        }
        if let Some(tab) = tab {
            ext.grant_active_tab(tab);
        }
        let mut args = vec![json!(name)];
        args.extend(tab.and_then(|t| self.0.tab_for(&ext, t)).map(|t| ext.tab_json(&t)));
        let (inner, target) = (self.0.clone(), ext.clone());
        ext.when_background_loaded(move || bridge::emit_to_pages(&inner, &target, "commands.onCommand", &args));
    }

    /// The user clicked `id`'s notification `notification` or one of its buttons (the shell's
    /// [`notifications::ACTION`]). As in Chrome, that fires `notifications.onClicked` or
    /// `onButtonClicked` and leaves the notification listed. The Settings button is the
    /// shell's to handle; a notification this run did not show, or a button it lacks, does
    /// nothing.
    pub fn notification_activated(&self, id: &ExtensionId, notification: &str, activation: Activation) {
        let Some(ext) = self.0.extension(id) else { return };
        let Some(shown) = ext.notifications.borrow().shown(notification) else { return };
        let (event, args) = match activation {
            Activation::Click => ("notifications.onClicked", vec![json!(notification)]),
            Activation::Button(index) if index < shown.buttons.len() => ("notifications.onButtonClicked", vec![json!(notification), json!(index)]),
            Activation::Button(_) | Activation::Settings => return,
        };
        let (inner, target) = (self.0.clone(), ext.clone());
        ext.when_background_loaded(move || bridge::emit_to_pages(&inner, &target, event, &args));
    }

    /// The user turned `id`'s notifications on or off in core
    /// (`Extensions::set_notifications_allowed`). Off, its notifications close, each with
    /// `notifications.onClosed`; either way it gets `notifications.onPermissionLevelChanged`.
    pub fn notification_permission_changed(&self, id: &ExtensionId) {
        let Some(ext) = self.0.extension(id) else { return };
        let allowed = self.0.profile.borrow_mut().extensions().notifications_allowed(id);
        let closed = if allowed { Vec::new() } else { ext.notifications.borrow_mut().take_all() };
        let mut events: Vec<(&str, Vec<Value>)> = Vec::new();
        for notification in closed {
            bridge::withdraw_notification(&ext, &notification);
            events.push(("notifications.onClosed", vec![json!(notification), json!(false)]));
        }
        events.push(("notifications.onPermissionLevelChanged", vec![json!(notifications::permission_level(allowed))]));
        let (inner, target) = (self.0.clone(), ext.clone());
        ext.when_background_loaded(move || {
            for (event, args) in events {
                bridge::emit_to_pages(&inner, &target, event, &args);
            }
        });
    }

    /// How `id`'s notification `notification` shows, while it is listed.
    pub fn notification(&self, id: &ExtensionId, notification: &str) -> Option<Shown> {
        self.0.extension(id)?.notifications.borrow().shown(notification)
    }

    /// `storage.sync` changed remotely (a sync engine's `ApplyReport`): fire
    /// `storage.onChanged` in every context of that extension.
    pub fn storage_sync_changed(&self, ext: &ExtensionId, changes: &[StorageChange]) {
        if let Some(ext) = self.0.extension(ext) {
            bridge::storage_changed(&self.0, &ext, crate::protocol::area_name(Area::Sync), changes);
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

/// The frame script, in a world of its own, reports to the runtime from every frame of the
/// tab.
fn watch_frames(inner: &Rc<Inner>, ucm: &webkit::UserContentManager, tab: TabId) {
    ucm.add_script(&inner.frames_script);
    ucm.register_script_message_handler(FRAMES_HANDLER, Some(FRAMES_WORLD));
    let weak = Rc::downgrade(inner);
    ucm.connect_script_message_received(Some(FRAMES_HANDLER), move |_, value| {
        let Some(inner) = weak.upgrade() else { return };
        let text = value.to_json(0).map(String::from).unwrap_or_default();
        match serde_json::from_str::<Report>(&text) {
            Ok(report) => inner.navigated(tab, |frames| frames.report(&report)),
            Err(e) => log::debug!("frame report {text}: {e}"),
        }
    });
}

/// Content scripts (the manifest's and the dynamic ones) in the extension's world, the
/// page shim (default world, the extension's own documents only) and one handler for each,
/// so an extension page the tab navigates to has its API.
fn attach(inner: &Rc<Inner>, ext: &Rc<Extension>, tab: TabId, state: &mut TabState) {
    ext.content.add_to(&state.ucm);
    ext.dynamic_content.borrow().add_to(&state.ucm);
    state.ucm.add_script(&ext.page_script);
    if let Some(filter) = ext.filter.borrow().as_ref() {
        state.ucm.add_filter(filter);
    }
    let content = bridge::register(inner, &state.ucm, ext, Origin::Content { tab }, Some(&ext.world));
    let page = bridge::register(inner, &state.ucm, ext, Origin::TabPage { tab }, None);
    state.handlers.insert(ext.id.clone(), [content, page]);
}

/// Nothing to do in a tab the extension does not run in.
fn detach(ext: &Extension, state: &mut TabState) {
    let Some([content, page]) = state.handlers.remove(&ext.id) else { return };
    ext.content.remove_from(&state.ucm);
    ext.dynamic_content.borrow().remove_from(&state.ucm);
    state.ucm.remove_script(&ext.page_script);
    if let Some(filter) = ext.filter.borrow().as_ref() {
        state.ucm.remove_filter(filter);
    }
    state.ucm.disconnect(content);
    state.ucm.disconnect(page);
    state.ucm.unregister_script_message_handler(&ext.handler, Some(&ext.world));
    state.ucm.unregister_script_message_handler(&ext.page_handler, None);
}

/// `file` as JSON; `None` when it does not exist or does not parse, which is logged.
fn read_json<T: serde::de::DeserializeOwned>(file: &std::path::Path) -> Option<T> {
    match std::fs::read_to_string(file).map(|text| serde_json::from_str(&text)) {
        Ok(Ok(value)) => Some(value),
        Ok(Err(e)) => {
            log::warn!("{}: {e}", file.display());
            None
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            log::warn!("{}: {e}", file.display());
            None
        }
    }
}

fn remove_file(file: &std::path::Path) {
    if let Err(e) = std::fs::remove_file(file)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("{}: {e}", file.display());
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

    /// The managers of the tabs `ext` runs in.
    pub(crate) fn tab_managers(&self, ext: &Extension) -> Vec<webkit::UserContentManager> {
        self.tabs.borrow().values().filter(|s| ext.runs_in(s.browsing)).map(|s| s.ucm.clone()).collect()
    }

    /// The tabs `ext` runs in, of those the shell registered.
    fn tab_ids(&self, ext: &Extension) -> Vec<TabId> {
        self.tabs.borrow().iter().filter(|(_, s)| ext.runs_in(s.browsing)).map(|(id, _)| *id).collect()
    }

    /// WebViews of every tab `ext` runs in. Never called with `tabs` borrowed, since the host
    /// may call back into the runtime.
    pub(crate) fn tab_views(&self, ext: &Extension) -> Vec<webkit::WebView> {
        self.tab_ids(ext).into_iter().filter_map(|id| self.host.web_view(id)).collect()
    }

    /// The tabs `ext` runs in whose view is showing (or loading) one of its own documents.
    pub(crate) fn page_tab_views(&self, ext: &Extension) -> Vec<(TabId, webkit::WebView)> {
        self.tab_ids(ext)
            .into_iter()
            .filter_map(|id| self.host.web_view(id).map(|v| (id, v)))
            .filter(|(_, v)| v.uri().is_some_and(|u| ext.owns_url(&u)))
            .collect()
    }

    pub(crate) fn tab_info(&self, tab: TabId) -> Option<TabInfo> {
        self.host.tabs().into_iter().find(|t| t.id == tab)
    }

    /// The tabs as `ext` may know them: no private one unless it runs there.
    pub(crate) fn tabs_for(&self, ext: &Extension) -> Vec<TabInfo> {
        self.host.tabs().into_iter().filter(|t| ext.runs_in(t.browsing)).collect()
    }

    pub(crate) fn tab_for(&self, ext: &Extension, tab: TabId) -> Option<TabInfo> {
        self.tab_info(tab).filter(|t| ext.runs_in(t.browsing))
    }

    /// The windows as `ext` may know them, most recently focused first: no private one unless
    /// it runs there.
    pub(crate) fn windows_for(&self, ext: &Extension) -> Vec<WindowInfo> {
        self.host.windows().into_iter().filter(|w| ext.runs_in(w.browsing)).collect()
    }

    /// A selected tab at `url`, at the end of the last focused normal window, where Chrome
    /// opens an extension's own pages and its views' links.
    pub(crate) fn open_tab(&self, url: &str) -> Option<TabId> {
        let window = self.host.windows().into_iter().find(|w| w.browsing == Browsing::Normal).map(|w| w.id);
        self.host.create_tab(&NewTab { url: url.to_owned(), active: true, window, index: None })
    }

    /// `tab`'s frames, for `read`, unless `ext` does not run in the tab's kind of window.
    pub(crate) fn frames<T>(&self, ext: &Extension, tab: TabId, read: impl FnOnce(&Frames) -> T) -> Option<T> {
        self.tabs.borrow().get(&tab).filter(|state| ext.runs_in(state.browsing)).map(|state| read(&state.frames))
    }

    /// The tab and frame showing the document `document_id`.
    pub(crate) fn find_document(&self, document_id: &str) -> Option<(TabId, FrameId)> {
        self.tabs.borrow().iter().find_map(|(tab, state)| Some((*tab, state.frames.find_document(document_id)?)))
    }

    /// Applies `change` to `tab`'s frames and fires the webNavigation events it returns, with
    /// the ones held back, once the shell has put the tab in a window, so that
    /// `tabs.onCreated` comes first, as in Chrome. The first top-frame navigation of a tab
    /// another one opened comes after `onCreatedNavigationTarget`.
    pub(crate) fn navigated(&self, tab: TabId, change: impl FnOnce(&mut Frames) -> Vec<Event>) {
        let (browsing, events) = {
            let mut tabs = self.tabs.borrow_mut();
            let Some(state) = tabs.get_mut(&tab) else { return };
            let events = change(&mut state.frames);
            if state.placed.is_none() {
                state.held.extend(events);
                return;
            }
            let mut ready = Vec::new();
            for event in std::mem::take(&mut state.held).into_iter().chain(events) {
                if event.kind == EventKind::BeforeNavigate
                    && event.frame == FrameId::TOP
                    && let Some(source) = state.opener.take()
                {
                    ready.push(Event::created_navigation_target(source, tab, event.details["url"].as_str().unwrap_or_default()));
                }
                ready.push(event);
            }
            (state.browsing, ready)
        };
        if events.is_empty() {
            return;
        }
        let listeners: Vec<Rc<Extension>> = self.loaded_extensions().into_iter().filter(|ext| ext.has_permission("webNavigation") && ext.runs_in(browsing)).collect();
        for event in events {
            let mut details = event.details;
            details.insert("timeStamp".into(), json!(bridge::now_ms()));
            let args = [Value::Object(details)];
            for ext in &listeners {
                bridge::emit_to_pages(self, ext, event.kind.name(), &args);
            }
        }
    }

    /// Notes where every tab now is, which the next move is told from.
    fn place_tabs(&self) {
        let infos = self.host.tabs();
        let mut tabs = self.tabs.borrow_mut();
        for info in infos {
            if let Some(state) = tabs.get_mut(&info.id) {
                state.placed = Some((info.window_id, info.index));
            }
        }
    }

    /// Fires a tab event in the pages of every extension that runs in tabs of the tab's kind.
    fn emit_about_tab(&self, browsing: Browsing, event: &str, args: &[Value]) {
        for ext in self.loaded_extensions().into_iter().filter(|ext| ext.runs_in(browsing)) {
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

    fn rules_file(&self, ext: &Extension) -> PathBuf {
        self.state_dir.join("dnr").join(format!("{}.json", ext.host))
    }

    /// As in Chrome, dynamic rules last until the extension is installed afresh, and the
    /// rulesets it enabled or disabled until its next version.
    fn restore_rules(&self, ext: &Extension, event: &InstallEvent) {
        let file = self.rules_file(ext);
        let saved = match event {
            InstallEvent::Installed => {
                remove_file(&file);
                Saved::default()
            }
            InstallEvent::Updated { .. } => Saved { enabled: None, ..read_json(&file).unwrap_or_default() },
            InstallEvent::Startup | InstallEvent::Nothing => read_json(&file).unwrap_or_default(),
        };
        let (rules, skipped) = Rules::new(&ext.manifest.dnr_rulesets, saved);
        if !skipped.is_empty() {
            log::warn!("{}: saved declarativeNetRequest rules left out: {}", file.display(), crate::dnr::describe_skipped(&skipped));
        }
        *ext.dnr.borrow_mut() = rules;
    }

    pub(crate) fn save_rules(&self, ext: &Extension) {
        let file = self.rules_file(ext);
        let written = serde_json::to_string(&ext.dnr.borrow().saved()).map_err(std::io::Error::other).and_then(|json| std::fs::write(&file, json));
        if let Err(e) = written {
            log::warn!("{}: {e}", file.display());
        }
    }

    fn scripts_file(&self, ext: &Extension) -> PathBuf {
        self.state_dir.join("scripts").join(format!("{}.json", ext.host))
    }

    /// As in Chrome, the dynamic content scripts registered to persist across sessions last
    /// until the extension is installed afresh or updated.
    fn restore_scripts(&self, ext: &Extension, event: &InstallEvent) {
        let file = self.scripts_file(ext);
        if matches!(event, InstallEvent::Installed | InstallEvent::Updated { .. }) {
            remove_file(&file);
            return;
        }
        let saved: Vec<Value> = read_json(&file).unwrap_or_default();
        let (scripts, skipped) = Scripts::restore(&saved, &|reference| ext.script_file(reference));
        for reason in skipped {
            log::warn!("{}: saved dynamic content script left out: {reason}", file.display());
        }
        *ext.dynamic_scripts.borrow_mut() = scripts;
        *ext.dynamic_content.borrow_mut() = ext.build_dynamic_content();
    }

    /// `ext`'s dynamic content scripts changed: every tab gets their new user content, which
    /// applies from the next load as in Chrome, and the ones that persist are saved.
    pub(crate) fn dynamic_scripts_changed(&self, ext: &Extension) {
        let previous = ext.dynamic_content.replace(ext.build_dynamic_content());
        for ucm in self.tab_managers(ext) {
            previous.remove_from(&ucm);
            ext.dynamic_content.borrow().add_to(&ucm);
        }
        let file = self.scripts_file(ext);
        let written = serde_json::to_string(&ext.dynamic_scripts.borrow().saved()).map_err(std::io::Error::other).and_then(|json| std::fs::write(&file, json));
        if let Err(e) = written {
            log::warn!("{}: {e}", file.display());
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
            remove_file(&file);
            return;
        }
        if !ext.lazy_background() {
            return;
        }
        if let Some(menus) = read_json(&file) {
            *ext.menus.borrow_mut() = menus;
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
