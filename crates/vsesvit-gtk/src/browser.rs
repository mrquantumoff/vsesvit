//! The browser controller: owns the profile (`vsesvit-core`), the engine session shared by
//! every tab, the WebExtensions runtime, and the closed-tab stack. Windows and tabs call it
//! for everything that touches data or policy: history, bookmarks, the omnibox, session
//! persistence, preferences with a live effect, and extension installs.
//!
//! A window is normal or private for its whole life, and so are its tabs ([`Browsing`]). The
//! private session (see `vsesvit_core::private`) lasts from its first window until its last
//! closes: its tabs share an ephemeral engine session and a closed-tab stack of their own, and
//! every write a tab causes (history, favicons, zoom, site settings, downloads, the session
//! file) goes through one function that leaves private tabs out or keeps their part in
//! memory. A private window sees only private tabs, and a normal one only normal ones.
//!
//! The runtime's [`TabHost`] is implemented here over the live windows, so `chrome.tabs`
//! sees exactly what the user sees.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use serde::Serialize;
use serde::de::DeserializeOwned;
use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::extensions::commands::{self, ExtensionShortcuts};
use vsesvit_core::cookies::ThirdPartyCookies;
use vsesvit_core::favicons::FaviconFetch;
use vsesvit_core::extensions::private::ALLOWED_IN_PRIVATE;
use vsesvit_core::extensions::{ExtensionId, toolbar};
use vsesvit_core::history::Transition;
use vsesvit_core::https_only::{self, Reach};
use vsesvit_core::memory_saver::{self, Sweep, TabActivity};
use vsesvit_core::onboarding;
use vsesvit_core::prefs::{Pref, Startup, TabsPosition, Theme, UpdateChannel, keys};
use vsesvit_core::private::Browsing;
use vsesvit_core::shortcuts::Keymap;
use vsesvit_core::sync::Changed;
use vsesvit_core::tab_search::{self, Listed, Row};
use vsesvit_core::trackers::TrackingProtection;
use vsesvit_core::{Profile, Url};
use vsesvit_webext::{
    ActionInfo, NewTab, NewWindow, Runtime, TabHost, TabId, TabInfo, WindowId, WindowInfo,
    WindowState, WindowUpdate,
};
use webkit::prelude::*;

use crate::closed_tabs::{ClosedKey, ClosedTabs};
use crate::cookies::Cookies;
use crate::dialogs::Windowed;
use crate::downloads::Downloads;
use crate::engine::{self, Engine};
use crate::profile::{self, Core};
use crate::sync::Syncer;
use crate::tab::{Commit, Tab};
use crate::trackers::Trackers;
use crate::updates::Updates;
use crate::window::{BrowserWindow, Focus};
use crate::{dialogs, favicons, keymap, location, omnibox, permissions, session};

const CLOSED_TABS_KEPT: usize = 25;
/// How many sites one background favicon fetch looks up.
const FAVICON_BATCH: usize = 32;
/// How long after the last tab change the session is written.
const SESSION_SAVE_DELAY: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct Browser(pub(crate) Rc<Inner>);

/// Shows a preference again after sync changed it; `false` once its widgets are gone.
type PrefView = Box<dyn Fn(&Browser) -> bool>;

pub(crate) struct Inner {
    app: adw::Application,
    core: Core,
    engine: Engine,
    runtime: Runtime,
    trackers: Trackers,
    cookies: Cookies,
    /// Which http URLs HTTPS-only upgrades (the self-test adds its local server).
    https_reach: Cell<Reach>,
    downloads: Rc<Downloads>,
    closed_tabs: RefCell<ClosedTabs<ClosedTab>>,
    private_closed_tabs: RefCell<ClosedTabs<ClosedTab>>,
    /// The private session's engine side, while it lasts.
    private: RefCell<Option<PrivateEngine>>,
    /// Why the runtime could not load an enabled extension, by extension.
    extension_errors: RefCell<HashMap<ExtensionId, String>>,
    next_tab_id: Cell<u32>,
    next_window_id: Cell<u32>,
    /// The clock of tabs' use, for tab search's order: ticks each time a tab is opened or
    /// selected, its window activated, or it is closed.
    last_use: Cell<u64>,
    /// The pending debounced session save, if any.
    session_save: RefCell<Option<glib::SourceId>>,
    /// Set once the application has shut down: the session saved then is final.
    shut_down: Cell<bool>,
    favicon_fetch: Cell<FetchState>,
    /// What else shows bookmarks (an open Bookmarks window), refreshed with the bars.
    bookmark_views: RefCell<Vec<Weak<dyn Fn()>>>,
    /// An open History window, refreshed when sync brings history or other devices' tabs.
    history_views: RefCell<Vec<Weak<dyn Fn()>>>,
    /// Open Settings controls, shown again when sync changes a preference; each is dropped once
    /// it returns false.
    pref_views: RefCell<Vec<PrefView>>,
    /// Bookmarks, History and Downloads, each in a window of its own while open.
    windowed: RefCell<Vec<(Windowed, glib::WeakRef<adw::Window>)>>,
    updates: Option<Updates>,
    sync: Syncer,
    /// This run created the profile: the first window opens the welcome, once.
    welcome: Cell<bool>,
}

/// The engine side of the private session, made for its first tab and dropped when its last
/// window closes, so that the next private window starts with nothing.
struct PrivateEngine {
    session: webkit::NetworkSession,
    /// [`Downloads::watch`]'s handler on it.
    downloads: glib::SignalHandlerId,
}

/// The background fetch of bookmarked sites' icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FetchState {
    Idle,
    Running,
    /// Bookmarks changed while a fetch ran: look again when it ends.
    RunningStale,
}

/// Enough of a closed tab to bring it back with its history, and to find it in tab search.
pub(crate) struct ClosedTab {
    pub(crate) uri: String,
    pub(crate) title: String,
    pub(crate) favicon: Option<gdk::Texture>,
    pub(crate) state: Option<webkit::WebViewSessionState>,
    pub(crate) position: i32,
    pub(crate) pinned: bool,
    /// When it was closed, on the clock of [`Tab::used`].
    pub(crate) used: u64,
}

/// What a tab search row leads to: an open tab, or a closed one in the stack.
pub(crate) type TabHit = tab_search::Hit<TabId, ClosedKey>;

impl Browser {
    /// Wraps an open profile. The extension runtime is created here, before any tab web
    /// view exists, because every tab's view is built with the runtime's content manager.
    pub(crate) fn new(app: &adw::Application, profile: Profile) -> Self {
        let core: Core = Rc::new(RefCell::new(profile));
        let engine = Engine::new(&mut core.borrow_mut());
        let trackers = Trackers::new(core.clone());
        let cookies = Cookies::new(core.clone(), engine.session());
        let downloads = Downloads::new(app, core.clone(), engine.session(), profile::downloads_dir());
        let updates_automatic = core.borrow_mut().prefs().get(&keys::UPDATES_AUTOMATIC);
        let updates_channel = core.borrow_mut().prefs().get(&keys::UPDATES_CHANNEL);
        let welcome = !crate::SCRIPTED.get() && onboarding::should_show(&mut core.borrow_mut());
        let inner = Rc::new_cyclic(|weak: &Weak<Inner>| {
            let host: Rc<dyn TabHost> = Rc::new(Host(weak.clone()));
            let runtime = Runtime::new(core.clone(), engine.session(), host);
            let sync = Syncer::new(weak.clone(), &mut core.borrow_mut());
            Inner {
                app: app.clone(),
                core,
                engine,
                runtime,
                trackers,
                cookies,
                https_reach: Cell::new(Reach::Public),
                downloads,
                closed_tabs: RefCell::new(ClosedTabs::new(CLOSED_TABS_KEPT)),
                private_closed_tabs: RefCell::new(ClosedTabs::new(CLOSED_TABS_KEPT)),
                private: RefCell::new(None),
                extension_errors: RefCell::new(HashMap::new()),
                next_tab_id: Cell::new(1),
                next_window_id: Cell::new(1),
                last_use: Cell::new(0),
                session_save: RefCell::new(None),
                shut_down: Cell::new(false),
                favicon_fetch: Cell::new(FetchState::Idle),
                bookmark_views: RefCell::new(Vec::new()),
                history_views: RefCell::new(Vec::new()),
                pref_views: RefCell::new(Vec::new()),
                windowed: RefCell::new(Vec::new()),
                updates: Updates::new(app, updates_automatic, updates_channel),
                sync,
                welcome: Cell::new(welcome),
            }
        });
        let weak = Rc::downgrade(&inner);
        app.connect_window_removed(move |_, window| {
            if window.is::<BrowserWindow>()
                && let Some(inner) = weak.upgrade()
            {
                inner.runtime.windows_changed();
            }
        });
        let weak = Rc::downgrade(&inner);
        inner.runtime.connect_actions_changed(move || {
            if let Some(inner) = weak.upgrade() {
                for window in Browser(inner).windows() {
                    window.refresh_extension_actions();
                }
            }
        });
        let weak = Rc::downgrade(&inner);
        app.connect_window_removed(move |_, removed| {
            let Some(browser) = weak.upgrade().map(Browser) else { return };
            let private = |window: &BrowserWindow| window.browsing() == Browsing::Private;
            if removed.downcast_ref().is_some_and(private) && !browser.windows().iter().any(|w| private(w) && w != removed) {
                browser.end_private_session();
            }
        });
        let browser = Browser(inner);
        permissions::seed_notifications(&browser);
        browser
    }

    /// Applies the profile's preferences, starts compiling tracking protection's and the cookie
    /// rules' blockers, deletes the data of sites to clear on exit, and brings the extension
    /// runtime in line with the profile: loads every enabled extension, then reconciles against
    /// the synced desired state (installs missing store extensions, unloads ones removed
    /// elsewhere). Then Memory Saver sweeps the tabs every [`memory_saver::SWEEP_EVERY`].
    pub(crate) fn start(&self) {
        self.apply_theme();
        self.apply_keymap();
        self.trackers().apply();
        self.cookies().apply();
        self.cookies().clear_at_start();
        let installed = self.core().borrow_mut().extensions().list();
        match installed {
            Ok(list) => {
                for ext in list.into_iter().filter(|e| e.enabled) {
                    // A failure is logged and shown on the extensions page.
                    let _ = self.load_into_runtime(&ext);
                }
            }
            Err(e) => log::warn!("cannot list extensions: {e}"),
        }
        self.reconcile_extensions();
        self.preload_favicons();
        let weak = Rc::downgrade(&self.0);
        glib::timeout_add_local(memory_saver::SWEEP_EVERY, move || match weak.upgrade() {
            Some(inner) => {
                Browser(inner).sleep_idle_tabs(Instant::now());
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
    }

    pub(crate) fn app(&self) -> &adw::Application {
        &self.0.app
    }

    pub(crate) fn core(&self) -> &Core {
        &self.0.core
    }

    pub(crate) fn engine(&self) -> &Engine {
        &self.0.engine
    }

    pub(crate) fn runtime(&self) -> &Runtime {
        &self.0.runtime
    }

    pub(crate) fn trackers(&self) -> &Trackers {
        &self.0.trackers
    }

    pub(crate) fn cookies(&self) -> &Cookies {
        &self.0.cookies
    }

    /// Runs `f` once tracking protection and the cookie rules are on every tab and the data of
    /// sites to clear is gone, so no page loads before them.
    pub(crate) fn when_blockers_applied(&self, f: impl FnOnce() + 'static) {
        let cookies = self.cookies().clone();
        self.trackers().when_applied(move || cookies.when_applied(f));
    }

    /// The https URL a navigation to `url` in a tab of `browsing`'s kind loads instead, under
    /// HTTPS-only.
    pub(crate) fn https_upgrade(&self, browsing: Browsing, url: &Url) -> Option<Url> {
        https_only::upgrade(&mut self.core().borrow_mut(), browsing, url, self.0.https_reach.get())
    }

    #[cfg(feature = "self-test")]
    pub(crate) fn set_https_reach(&self, reach: Reach) {
        self.0.https_reach.set(reach);
    }

    pub(crate) fn downloads(&self) -> &Rc<Downloads> {
        &self.0.downloads
    }

    /// The engine session of a tab of `browsing`'s kind: the profile's, or the private
    /// session's, made for its first tab with its own downloads handler.
    pub(crate) fn network_session(&self, browsing: Browsing) -> webkit::NetworkSession {
        match browsing {
            Browsing::Normal => self.engine().session().clone(),
            Browsing::Private => {
                let mut private = self.0.private.borrow_mut();
                let engine = private.get_or_insert_with(|| {
                    let session = Engine::ephemeral_session();
                    let downloads = self.downloads().watch(&session, Browsing::Private);
                    self.cookies().set_private_session(Some(&session));
                    PrivateEngine { session, downloads }
                });
                engine.session.clone()
            }
        }
    }

    /// The last private window closed. As in Chrome its downloads still running are
    /// cancelled, and everything the private session kept goes: its closed tabs, what core
    /// kept for it, its engine session with the cookies and cache in it.
    fn end_private_session(&self) {
        self.downloads().cancel_private();
        if let Some(engine) = self.0.private.take() {
            engine.session.disconnect(engine.downloads);
            self.cookies().set_private_session(None);
        }
        self.0.private_closed_tabs.borrow_mut().clear();
        self.core().borrow_mut().end_private_session();
    }

    #[cfg(feature = "self-test")]
    pub(crate) fn private_session_lasts(&self) -> bool {
        self.0.private.borrow().is_some()
    }

    /// Why the runtime is not running this enabled extension, when it failed to load it.
    pub(crate) fn extension_error(&self, id: &ExtensionId) -> Option<String> {
        self.0.extension_errors.borrow().get(id).cloned()
    }

    pub(crate) fn set_extension_error(&self, id: &ExtensionId, error: Option<String>) {
        let mut errors = self.0.extension_errors.borrow_mut();
        match error {
            Some(error) => errors.insert(id.clone(), error),
            None => errors.remove(id),
        };
    }

    /// `None` when this copy does not update itself.
    pub(crate) fn updates(&self) -> Option<&Updates> {
        self.0.updates.as_ref()
    }

    pub(crate) fn sync(&self) -> &Syncer {
        &self.0.sync
    }

    /// Set when the user chose to restart into an installed update.
    pub(crate) fn restart_program(&self) -> Option<PathBuf> {
        self.0.updates.as_ref().and_then(Updates::restart_program)
    }

    pub(crate) fn allocate_tab_id(&self) -> TabId {
        let id = self.0.next_tab_id.get();
        self.0.next_tab_id.set(id + 1);
        TabId(id)
    }

    pub(crate) fn allocate_window_id(&self) -> u32 {
        let id = self.0.next_window_id.get();
        self.0.next_window_id.set(id + 1);
        id
    }

    /// Most recently focused first.
    pub(crate) fn windows(&self) -> Vec<BrowserWindow> {
        self.0
            .app
            .windows()
            .into_iter()
            .filter_map(|w| w.downcast().ok())
            .collect()
    }

    /// The windows of `browsing`'s kind, most recently focused first.
    pub(crate) fn windows_of(&self, browsing: Browsing) -> Vec<BrowserWindow> {
        self.windows().into_iter().filter(|window| window.browsing() == browsing).collect()
    }

    /// The most recently focused normal window, else a new one with a blank tab: where the
    /// browser opens what comes from outside any window (another invocation, an extension,
    /// sync), which must not land among private tabs.
    pub(crate) fn normal_window(&self) -> BrowserWindow {
        let window = self.windows_of(Browsing::Normal).into_iter().next();
        window.unwrap_or_else(|| self.open_window(Browsing::Normal, &[]))
    }

    pub(crate) fn find_tab(&self, id: TabId) -> Option<(BrowserWindow, Tab)> {
        self.windows().into_iter().find_map(|window| {
            let tab = window.tabs().into_iter().find(|t| t.id() == id)?;
            Some((window, tab))
        })
    }

    // Windows.

    /// The first windows of a run: the previous session, the homepage or a blank tab,
    /// according to the startup preference, plus any URLs from the command line. On the
    /// run that created the profile the first window opens the welcome over them.
    pub(crate) fn open_startup_windows(&self, targets: &[Url]) {
        let startup = self.pref(&keys::STARTUP);
        let restored = match startup {
            Startup::RestoreSession => {
                let saved = self.core().borrow_mut().session().restore();
                match saved {
                    Ok(Some(snapshot)) => session::restore(self, snapshot),
                    Ok(None) => 0,
                    Err(e) => {
                        log::warn!("cannot restore the session: {e}");
                        0
                    }
                }
            }
            Startup::Homepage | Startup::NewTab => 0,
        };
        if restored == 0 && targets.is_empty() {
            let window = BrowserWindow::new(self);
            match (startup, self.homepage()) {
                (Startup::Homepage, Some(url)) => {
                    window.open_tab(Some(url.as_str()), None, Focus::Foreground);
                }
                _ => window.new_tab(),
            }
            window.present();
        }
        if !targets.is_empty() {
            // Into the restored window, or a window of their own: no blank tab beside them.
            let window = self
                .windows_of(Browsing::Normal)
                .into_iter()
                .next()
                .unwrap_or_else(|| BrowserWindow::new(self));
            window.open_tabs(targets);
            window.present();
        }
        if self.0.welcome.take()
            && let Some(window) = self.windows_of(Browsing::Normal).into_iter().next()
        {
            dialogs::welcome::present(&window);
        }
    }

    /// Opens a window of `browsing`'s kind with a tab for each target, the first one selected,
    /// or a blank tab.
    pub(crate) fn open_window(&self, browsing: Browsing, targets: &[Url]) -> BrowserWindow {
        let window = BrowserWindow::with_browsing(self, browsing);
        if targets.is_empty() {
            window.new_tab();
        }
        window.open_tabs(targets);
        window.present();
        window
    }

    pub(crate) fn present(&self) {
        self.normal_window().present();
    }

    /// `kind`'s own window, while it is open. A closed one can outlive its closing for as long
    /// as its widgets' handlers hold it.
    pub(crate) fn windowed(&self, kind: Windowed) -> Option<adw::Window> {
        self.0
            .windowed
            .borrow()
            .iter()
            .find(|(k, _)| *k == kind)
            .and_then(|(_, window)| window.upgrade())
            .filter(|window| window.is_visible())
    }

    pub(crate) fn add_windowed(&self, kind: Windowed, window: &adw::Window) {
        let mut windowed = self.0.windowed.borrow_mut();
        windowed.retain(|(k, w)| *k != kind && w.upgrade().is_some_and(|w| w.is_visible()));
        windowed.push((kind, window.downgrade()));
    }

    /// Called from the application's `shutdown`: the last session write, deleting the data of
    /// sites to clear on exit, then the final sync. Nothing writes the session afterwards, so
    /// tearing the windows down for a restart cannot overwrite it.
    pub(crate) fn shutdown(&self) {
        self.save_session_now();
        self.0.shut_down.set(true);
        self.cookies().clear_at_exit();
        self.0.sync.final_sync();
    }

    // Tabs.

    pub(crate) fn tab_closed(&self, tab: &Tab, position: i32, pinned: bool) {
        permissions::closed(tab);
        self.runtime().tab_closed(tab.id(), false);
        self.schedule_session_save();
        let Some(uri) = tab.committed_uri().filter(|uri| uri != "about:blank") else {
            return;
        };
        let web_view = tab.web_view();
        let closed = ClosedTab {
            uri,
            title: tab.display_title(),
            favicon: web_view.favicon(),
            state: web_view.session_state(),
            position,
            pinned,
            used: self.tick(),
        };
        self.closed_tabs(tab.browsing()).borrow_mut().push(closed);
    }

    /// The tabs closed in windows of `browsing`'s kind.
    fn closed_tabs(&self, browsing: Browsing) -> &RefCell<ClosedTabs<ClosedTab>> {
        match browsing {
            Browsing::Normal => &self.0.closed_tabs,
            Browsing::Private => &self.0.private_closed_tabs,
        }
    }

    /// A tab that goes away without being closed one by one: with its window, or a
    /// `window.open` view that never showed.
    pub(crate) fn tab_discarded(&self, tab: &Tab, window_closing: bool) {
        permissions::closed(tab);
        self.runtime().tab_closed(tab.id(), window_closing);
    }

    /// A window took `tab`: a new tab, or one from another window.
    pub(crate) fn tab_attached(&self, tab: &Tab) {
        self.runtime().tab_attached(tab.id());
    }

    pub(crate) fn tab_moved(&self, tab: &Tab) {
        self.runtime().tab_moved(tab.id());
        self.schedule_session_save();
    }

    pub(crate) fn tab_activated(&self, tab: &Tab) {
        self.tab_used(tab);
        self.runtime().tab_activated(tab.id());
        self.schedule_session_save();
    }

    /// Memory Saver's sweep at `now`: the tabs left alone long enough go to sleep.
    pub(crate) fn sleep_idle_tabs(&self, now: Instant) {
        let sweep = Sweep::new(&mut self.core().borrow_mut(), now);
        for window in self.windows() {
            let selected = window.selected_tab();
            for tab in window.tabs() {
                let Some(url) = tab.committed_uri() else { continue };
                let activity = TabActivity {
                    url: &url,
                    shown: selected.as_ref() == Some(&tab),
                    pinned: window.is_pinned(&tab),
                    audible: tab.web_view().is_playing_audio(),
                    capturing: tab.capturing().any(),
                    related: tab.is_related(),
                };
                if tab.sleeps(&sweep, &activity) {
                    self.sleep_unless_unsaved(tab);
                }
            }
        }
    }

    /// Puts `tab` to sleep unless its page holds form input not yet submitted, which keeps it
    /// awake for another delay, and unless it was selected or closed while the page answered.
    fn sleep_unless_unsaved(&self, tab: Tab) {
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let script = memory_saver::UNSAVED_INPUT_SCRIPT;
            let answer = tab.web_view().evaluate_javascript_future(script, None, None).await;
            if answer.is_ok_and(|value| memory_saver::has_unsaved_input(&value.to_str())) {
                tab.keep_awake(Instant::now());
                return;
            }
            let background = tab.window().is_some_and(|window| window.selected_tab().as_ref() != Some(&tab));
            if background && let Some(inner) = weak.upgrade() {
                tab.sleep();
                Browser(inner).schedule_session_save();
            }
        });
    }

    /// The tab was opened or selected, or its window activated: tab search lists it first now.
    pub(crate) fn tab_used(&self, tab: &Tab) {
        tab.set_used(self.tick());
    }

    fn tick(&self) -> u64 {
        let now = self.0.last_use.get() + 1;
        self.0.last_use.set(now);
        now
    }

    pub(crate) fn can_reopen_closed_tab(&self, browsing: Browsing) -> bool {
        !self.closed_tabs(browsing).borrow().is_empty()
    }

    /// The tab last closed in a window of `window`'s kind, back in `window`.
    pub(crate) fn reopen_closed_tab(&self, window: &BrowserWindow) {
        let closed = self.closed_tabs(window.browsing()).borrow_mut().pop();
        if let Some(closed) = closed {
            window.restore_closed(&closed);
        }
    }

    // Tab search.

    /// Tab search's rows for `query` in a window of `browsing`'s kind: the open tabs of that
    /// kind, then the ones closed in its windows, as core narrows and orders them.
    pub(crate) fn search_tabs(&self, browsing: Browsing, query: &str) -> Vec<Row<TabId, ClosedKey>> {
        let open = self
            .windows()
            .iter()
            .flat_map(BrowserWindow::tabs)
            .filter(|tab| tab.browsing() == browsing)
            .map(|tab| Listed {
                key: tab.id(),
                title: tab.display_title(),
                url: tab.session_uri().unwrap_or_default(),
                used: tab.used(),
            })
            .collect();
        let closed = self
            .closed_tabs(browsing)
            .borrow()
            .iter()
            .map(|(key, tab)| Listed { key, title: tab.title.clone(), url: tab.uri.clone(), used: tab.used })
            .collect();
        tab_search::rows(query, open, closed)
    }

    /// The icon of the tab `hit`, listed in a window of `browsing`'s kind, leads to, as its tab
    /// shows it or showed it when closed.
    pub(crate) fn tab_icon(&self, browsing: Browsing, hit: TabHit) -> Option<gdk::Texture> {
        match hit {
            tab_search::Hit::Open(id) => self.find_tab(id).and_then(|(_, tab)| tab.web_view().favicon()),
            tab_search::Hit::Closed(key) => self.closed_tabs(browsing).borrow().get(key).and_then(|tab| tab.favicon.clone()),
        }
    }

    /// Tab search's choice: selects an open tab in its window and raises that window, or
    /// reopens a closed one in `window`. Nothing happens for a tab gone since it was listed.
    pub(crate) fn go_to_tab(&self, window: &BrowserWindow, hit: TabHit) {
        match hit {
            tab_search::Hit::Open(id) => {
                if let Some((owner, tab)) = self.find_tab(id) {
                    owner.select_tab(&tab);
                    owner.present();
                }
            }
            tab_search::Hit::Closed(key) => {
                let closed = self.closed_tabs(window.browsing()).borrow_mut().take(key);
                if let Some(closed) = closed {
                    window.restore_closed(&closed);
                }
            }
        }
    }

    // History and bookmarks.

    /// Every committed main-frame navigation records a visit (not for an error page, nor in a
    /// private window).
    pub(crate) fn navigation_committed(&self, tab: &Tab, uri: &str, commit: Commit) {
        let transition = tab.take_pending_transition().unwrap_or(Transition::Link);
        if commit != Commit::SameDocument
            && let Ok(url) = Url::parse(uri)
        {
            self.show_at_site_zoom(tab, &url);
        }
        if commit != Commit::ErrorPage
            && tab.browsing() == Browsing::Normal
            && let Ok(url) = Url::parse(uri)
        {
            let recorded = self.core().borrow_mut().history().record_visit(&url, transition);
            if let Err(e) = recorded {
                log::warn!("history: {e}");
            }
        }
        self.runtime().tab_updated(tab.id());
        self.schedule_session_save();
    }

    /// WebKit keeps a view's zoom from page to page, so a new document is shown at the level
    /// remembered for its site (100% for a site with none) instead of the previous page's.
    fn show_at_site_zoom(&self, tab: &Tab, url: &Url) {
        let level = match self.core().borrow_mut().site_zoom(tab.browsing()).get(url) {
            Ok(level) => level,
            Err(e) => {
                log::warn!("site zoom: {e}");
                return;
            }
        };
        let web_view = tab.web_view();
        // Setting it notifies `zoom_changed`, which must not find the profile borrowed.
        if (web_view.zoom_level() - level).abs() > f64::EPSILON {
            web_view.set_zoom_level(level);
        }
    }

    /// The tab's zoom changed, by the user or by [`Browser::show_at_site_zoom`]: remember it
    /// for the site the tab is on, in memory only for a private tab.
    pub(crate) fn zoom_changed(&self, tab: &Tab) {
        let Some(url) = tab.committed_uri().and_then(|uri| Url::parse(&uri).ok()) else {
            return;
        };
        let level = tab.web_view().zoom_level();
        if let Err(e) = self.core().borrow_mut().site_zoom(tab.browsing()).set(&url, level) {
            log::warn!("site zoom: {e}");
        }
    }

    /// Titles arrive after the commit, and are written to history under the committed URI,
    /// with two exceptions. WebKit clears the title as the next document commits, before it
    /// reports the commit, so an empty title would land on the page being left. An error
    /// page's title is ours, not that of the URI that failed. A private tab writes none.
    pub(crate) fn title_changed(&self, tab: &Tab) {
        if tab.browsing() == Browsing::Normal
            && !tab.shows_error_page()
            && let (Some(uri), Some(title)) = (tab.committed_uri(), tab.web_view().title())
            && !title.is_empty()
            && let Ok(url) = Url::parse(&uri)
        {
            let set = self.core().borrow_mut().history().set_title(&url, &title);
            if let Err(e) = set {
                log::warn!("history title: {e}");
            }
        }
        self.runtime().tab_updated(tab.id());
        self.schedule_session_save();
    }

    /// In-memory lookup, safe on every navigation.
    pub(crate) fn is_bookmarked(&self, uri: Option<&str>) -> bool {
        let Some(url) = uri.and_then(|u| Url::parse(u).ok()) else {
            return false;
        };
        self.core().borrow_mut().bookmarks().is_bookmarked(&url)
    }

    /// The star button or Ctrl+D: bookmarks the page into the bookmarks bar and opens the
    /// "Bookmark added" bubble, or opens the "Edit bookmark" bubble on its bookmark.
    pub(crate) fn star_clicked(&self, window: &BrowserWindow) {
        let Some(tab) = window.selected_tab() else { return };
        let Some(url) = tab.committed_uri().and_then(|u| Url::parse(&u).ok()) else {
            return;
        };
        let title = tab.display_title();
        let result = {
            let mut p = self.core().borrow_mut();
            let mut bookmarks = p.bookmarks();
            match bookmarks.find_by_url(&url).into_iter().next() {
                Some(node) => Ok((Some(node), false)),
                None => bookmarks
                    .add_url(BookmarkId::TOOLBAR, InsertAt::End, &title, &url)
                    .map(|id| (bookmarks.get(id), true)),
            }
        };
        let (node, added) = match result {
            Ok((Some(node), added)) => (node, added),
            Ok((None, _)) => return,
            Err(e) => {
                window.toast(dialogs::plain_toast(&format!("Cannot bookmark the page: {e}")));
                return;
            }
        };
        if added {
            // The page's icon arrived before it was bookmarked.
            self.save_favicon(&tab);
            self.bookmarks_changed();
        }
        window.show_bookmark_bubble(node, added);
    }

    /// A tab shows a new icon: kept when its page or site is bookmarked, and shown on the
    /// bookmarks when it differs from the one they had.
    pub(crate) fn favicon_changed(&self, tab: &Tab) {
        if self.save_favicon(tab) {
            self.bookmarks_changed();
        }
    }

    /// Returns whether the stored icon changed. A private tab's icon is never stored.
    fn save_favicon(&self, tab: &Tab) -> bool {
        let (Some(uri), Some(icon), Browsing::Normal) = (tab.committed_uri(), tab.web_view().favicon(), tab.browsing()) else {
            return false;
        };
        favicons::record(&mut self.core().borrow_mut(), &uri, &icon)
    }

    /// After any bookmark write: every window's bar and star, and every other view of the
    /// bookmarks, follow the new tree, and new sites get their icons.
    pub(crate) fn bookmarks_changed(&self) {
        for window in self.windows() {
            window.refresh_bookmarks_bar();
            window.sync_star();
        }
        refresh_views(&self.0.bookmark_views);
        self.preload_favicons();
    }

    /// Runs `refresh` after every bookmark change for as long as the caller keeps it.
    pub(crate) fn watch_bookmarks(&self, refresh: &Rc<dyn Fn()>) {
        self.0.bookmark_views.borrow_mut().push(Rc::downgrade(refresh));
    }

    /// Runs `refresh` after sync changes the history or other devices' tabs, for as long as the
    /// caller keeps it.
    pub(crate) fn watch_history(&self, refresh: &Rc<dyn Fn()>) {
        self.0.history_views.borrow_mut().push(Rc::downgrade(refresh));
    }

    /// Calls `show` after sync changes preferences or search engines, until it returns false.
    /// `show` holds its widgets weakly, so it returns false once they are gone.
    pub(crate) fn watch_prefs(&self, show: impl Fn(&Browser) -> bool + 'static) {
        self.0.pref_views.borrow_mut().push(Box::new(show));
    }

    /// Fetches the icons of bookmarked sites that have none, without visiting them: core
    /// picks the pages, a worker thread fetches them, and core stores the results here.
    /// Anything stored refreshes the bookmarks, which looks for the next batch.
    pub(crate) fn preload_favicons(&self) {
        let state = &self.0.favicon_fetch;
        if state.get() != FetchState::Idle {
            state.set(FetchState::RunningStale);
            return;
        }
        let pages = match self.core().borrow_mut().favicons().missing(FAVICON_BATCH) {
            Ok(pages) => pages,
            Err(e) => {
                log::warn!("favicons to fetch: {e}");
                return;
            }
        };
        if pages.is_empty() {
            return;
        }
        state.set(FetchState::Running);
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let fetched = gio::spawn_blocking(move || FaviconFetch::new(pages).run()).await;
            let Some(browser) = weak.upgrade().map(Browser) else { return };
            let stale = browser.0.favicon_fetch.replace(FetchState::Idle) == FetchState::RunningStale;
            let changed = match fetched {
                Ok(results) => browser.core().borrow_mut().favicons().commit_fetched(results).unwrap_or_else(|e| {
                    log::warn!("storing fetched favicons: {e}");
                    false
                }),
                Err(_) => {
                    log::warn!("the favicon fetch panicked");
                    false
                }
            };
            if changed {
                browser.bookmarks_changed();
            } else if stale {
                browser.preload_favicons();
            }
        });
    }

    /// The loaded extensions' toolbar actions, in install order. The runtime lists them by
    /// id, which the stable sort keeps for any the profile does not list.
    pub(crate) fn extension_actions(&self) -> Vec<ActionInfo> {
        let installed = self.installed_extensions();
        let mut actions = self.runtime().actions();
        actions.sort_by_key(|a| installed.iter().position(|e| e.id == a.extension).unwrap_or(usize::MAX));
        actions
    }

    /// Which of the extension actions `available` (ids in install order) the toolbar shows,
    /// and in what order, per the synced preference.
    pub(crate) fn extension_toolbar(&self, available: &[String]) -> toolbar::Layout {
        let saved = self.core().borrow_mut().prefs().get(&toolbar::TOOLBAR);
        toolbar::layout(available, &saved)
    }

    pub(crate) fn pin_extension(&self, id: &str, pinned: bool) {
        self.change_toolbar(|available, saved| toolbar::set_pinned(available, saved, id, pinned));
    }

    /// Moves a pinned action to `to` among the pinned ones.
    pub(crate) fn move_extension(&self, id: &str, to: usize) {
        self.change_toolbar(|available, saved| toolbar::move_pinned(available, saved, id, to));
    }

    fn change_toolbar(&self, change: impl FnOnce(&[String], &[toolbar::Entry]) -> Vec<toolbar::Entry>) {
        let available: Vec<String> = self.extension_actions().iter().map(|a| a.extension.as_str().to_owned()).collect();
        let saved = {
            let mut profile = self.core().borrow_mut();
            let mut prefs = profile.prefs();
            let next = change(&available, &prefs.get(&toolbar::TOOLBAR));
            prefs.set(&toolbar::TOOLBAR, &next)
        };
        if let Err(e) = saved {
            log::warn!("extension toolbar: {e}");
        }
        for window in self.windows() {
            window.refresh_extension_actions();
        }
    }

    // Omnibox.

    /// Every edit in the address bar: core's suggestions (search, typed URL, bookmarks,
    /// history) followed by matching open tabs, and the inline completion when allowed. The
    /// search engine's suggestions are fetched on a worker thread and join the rows when they
    /// arrive, unless the user typed again or stopped editing first. A private window asks the
    /// engine for none.
    pub(crate) fn omnibox_changed(&self, window: &BrowserWindow, text: &str, allow_inline: bool) {
        let address = window.address_bar();
        address.set_suggestions(omnibox::suggestions(self, window, text, allow_inline, None));
        let private = window.browsing() == Browsing::Private;
        let request = match self.core().borrow_mut().omnibox().suggest_request(text, address.queries(), private) {
            Ok(Some(request)) => request,
            Ok(None) => return,
            Err(e) => {
                log::warn!("search suggestions: {e}");
                return;
            }
        };
        let (window, text) = (window.downgrade(), text.to_owned());
        glib::spawn_future_local(async move {
            let Ok(Some(found)) = gio::spawn_blocking(move || request.run()).await else { return };
            let Some(window) = window.upgrade().filter(|_| found.is_current()) else { return };
            let rows = omnibox::suggestions(window.browser(), &window, &text, allow_inline, Some(&found)).rows;
            window.address_bar().refill_suggestions(rows);
        });
    }

    /// Enter in the address bar: core decides between a URL and a search.
    pub(crate) fn omnibox_activated(&self, window: &BrowserWindow, text: &str) {
        let resolved = self.core().borrow_mut().omnibox().resolve(text);
        match resolved {
            Ok(Some(target)) => window.navigate_with(target.url().as_str(), Transition::Typed),
            Ok(None) => window.focus_page(),
            Err(e) => window.toast(dialogs::plain_toast(&format!("Cannot resolve the address: {e}"))),
        }
    }

    // Preferences with a live effect.

    pub(crate) fn pref<T: DeserializeOwned>(&self, pref: &Pref<T>) -> T {
        self.core().borrow_mut().prefs().get(pref)
    }

    /// Writes a preference, logging a failure. The `set_*` methods that call it also apply it.
    pub(crate) fn set_pref<T: Serialize>(&self, pref: &Pref<T>, value: &T) {
        let set = self.core().borrow_mut().prefs().set(pref, value);
        if let Err(e) = set {
            log::warn!("prefs: {e}");
        }
    }

    pub(crate) fn tabs_position(&self) -> TabsPosition {
        self.pref(&keys::TABS_POSITION)
    }

    /// Writes the synced preference and re-lays out every open window at once.
    pub(crate) fn set_tabs_position(&self, position: TabsPosition) {
        self.set_pref(&keys::TABS_POSITION, &position);
        for window in self.windows() {
            window.apply_layout(position);
        }
    }

    pub(crate) fn switch(&self, pref: &Pref<bool>) -> bool {
        self.pref(pref)
    }

    /// Puts a preference back to its default, logging a failure.
    pub(crate) fn reset_pref<T>(&self, pref: &Pref<T>) {
        let reset = self.core().borrow_mut().prefs().reset(pref);
        if let Err(e) = reset {
            log::warn!("prefs: {e}");
        }
    }

    pub(crate) fn bookmarks_bar_visible(&self) -> bool {
        self.switch(&keys::SHOW_BOOKMARKS_BAR)
    }

    pub(crate) fn set_bookmarks_bar_visible(&self, shown: bool) {
        self.set_pref(&keys::SHOW_BOOKMARKS_BAR, &shown);
        for window in self.windows() {
            window.set_bookmarks_bar_visible(shown);
        }
    }

    pub(crate) fn home_button_visible(&self) -> bool {
        self.switch(&keys::SHOW_HOME_BUTTON)
    }

    pub(crate) fn set_home_button_visible(&self, shown: bool) {
        self.set_pref(&keys::SHOW_HOME_BUTTON, &shown);
        for window in self.windows() {
            window.set_home_button_visible(shown);
        }
    }

    pub(crate) fn compact_address_bar(&self) -> bool {
        self.switch(&keys::COMPACT_ADDRESS_BAR)
    }

    pub(crate) fn set_compact_address_bar(&self, compact: bool) {
        self.set_pref(&keys::COMPACT_ADDRESS_BAR, &compact);
        for window in self.windows() {
            window.set_compact_address_bar(compact);
        }
    }

    pub(crate) fn full_urls(&self) -> bool {
        self.switch(&keys::SHOW_FULL_URLS)
    }

    pub(crate) fn set_full_urls(&self, full: bool) {
        self.set_pref(&keys::SHOW_FULL_URLS, &full);
        for window in self.windows() {
            window.set_full_urls(full);
        }
    }

    /// Pop-ups, smooth scrolling, hardware acceleration and spell checking: the engine applies
    /// them to every view.
    pub(crate) fn set_engine_switch(&self, pref: &Pref<bool>, on: bool) {
        self.set_pref(pref, &on);
        self.engine().apply_prefs(&mut self.core().borrow_mut());
    }

    /// Turns spell checking in `language`, an installed dictionary, on or off.
    pub(crate) fn set_spellcheck_language(&self, language: &str, on: bool) {
        let chosen = engine::dictionaries().choose(self.pref(&keys::SPELLCHECK_LANGUAGES), language, on);
        self.set_pref(&keys::SPELLCHECK_LANGUAGES, &Some(chosen));
        self.engine().apply_prefs(&mut self.core().borrow_mut());
    }

    /// The Settings choice of tracking protection: writes the synced preference and gives every
    /// tab the blocker for it.
    pub(crate) fn set_tracking_protection(&self, level: TrackingProtection) {
        self.set_pref(&keys::TRACKING_PROTECTION, &level);
        self.trackers().apply();
    }

    /// The Settings choice of third-party cookies: writes the synced preference and sets the
    /// session's policy for it.
    pub(crate) fn set_third_party_cookies(&self, choice: ThirdPartyCookies) {
        self.set_pref(&keys::THIRD_PARTY_COOKIES, &choice);
        self.cookies().apply();
    }

    pub(crate) fn theme(&self) -> Theme {
        self.pref(&keys::THEME)
    }

    pub(crate) fn set_theme(&self, theme: Theme) {
        self.set_pref(&keys::THEME, &theme);
        self.apply_theme();
    }

    fn apply_theme(&self) {
        let scheme = match self.theme() {
            Theme::System => adw::ColorScheme::Default,
            Theme::Light => adw::ColorScheme::ForceLight,
            Theme::Dark => adw::ColorScheme::ForceDark,
        };
        adw::StyleManager::default().set_color_scheme(scheme);
    }

    /// The Settings switch for `updates.automatic`, a local preference: writes it and
    /// starts or stops this installation's checks.
    pub(crate) fn set_updates_automatic(&self, automatic: bool) {
        self.set_pref(&keys::UPDATES_AUTOMATIC, &automatic);
        if let Some(updates) = &self.0.updates {
            updates.set_automatic(automatic);
        }
    }

    /// The Settings choice of `updates.channel`, a local preference: writes it and checks the
    /// new channel, even with automatic updates off, because the user just asked for it.
    pub(crate) fn set_updates_channel(&self, channel: UpdateChannel) {
        self.set_pref(&keys::UPDATES_CHANNEL, &channel);
        if let Some(updates) = &self.0.updates {
            updates.set_channel(channel);
        }
    }

    pub(crate) fn keymap(&self) -> Keymap {
        self.core().borrow_mut().prefs().keymap()
    }

    /// The enabled extensions' commands with the shortcuts `keymap` gives them.
    pub(crate) fn extension_shortcuts(&self, keymap: &Keymap) -> ExtensionShortcuts {
        keymap.extension_shortcuts(commands::extension_commands(&self.installed_extensions()))
    }

    /// Every change to the shortcuts goes through here: `edit` changes the stored keymap and
    /// every window's shortcuts follow at once.
    pub(crate) fn edit_keymap<T>(&self, edit: impl FnOnce(&mut Keymap) -> T) -> T {
        let mut edited = self.keymap();
        let out = edit(&mut edited);
        if let Err(e) = self.core().borrow_mut().prefs().set_keymap(&edited) {
            log::warn!("prefs: {e}");
        }
        keymap::apply(self.app(), &edited, &self.extension_shortcuts(&edited));
        out
    }

    /// Also after the enabled extensions change, which changes their commands.
    pub(crate) fn apply_keymap(&self) {
        let keymap = self.keymap();
        keymap::apply(self.app(), &keymap, &self.extension_shortcuts(&keymap));
    }

    /// The homepage preference as a URL, or `None` for the new tab page.
    pub(crate) fn homepage(&self) -> Option<Url> {
        location::homepage_url(&self.pref(&keys::HOMEPAGE))
    }

    /// What the shell refreshes after sync applied remote records (`ApplyReport::changed`). The
    /// address bar reads the search engines each time, so only Settings shows them again.
    pub(crate) fn sync_applied(&self, changed: &Changed) {
        if changed.bookmarks {
            self.bookmarks_changed();
        }
        if changed.history || changed.sessions {
            refresh_views(&self.0.history_views);
        }
        if !changed.prefs.is_empty() || changed.search_engines {
            self.0.pref_views.borrow_mut().retain(|show| show(self));
        }
        if changed.extensions {
            self.reconcile_extensions();
        }
        if changed.prefs.iter().any(|key| key == ALLOWED_IN_PRIVATE.key) {
            self.runtime().allowed_in_private_changed();
        }
        for (ext, changes) in &changed.ext_storage {
            self.runtime().storage_sync_changed(ext, changes);
        }
        if changed.site_permissions {
            permissions::enforce(self);
        }
        if changed.site_permissions || changed.prefs.iter().any(|key| key == keys::TRACKING_PROTECTION.key) {
            self.trackers().apply();
        }
        if changed.site_permissions || changed.prefs.iter().any(|key| key == keys::THIRD_PARTY_COOKIES.key) {
            self.cookies().apply();
        }
        if changed.prefs.iter().any(|key| key == keys::SHORTCUTS.key) {
            self.apply_keymap();
        }
        if !changed.prefs.is_empty() {
            self.apply_theme();
            self.engine().apply_prefs(&mut self.core().borrow_mut());
            for window in self.windows() {
                window.apply_prefs();
                window.refresh_extension_actions();
            }
        }
    }

    // Session.

    /// About two seconds after the last tab change.
    pub(crate) fn schedule_session_save(&self) {
        let mut pending = self.0.session_save.borrow_mut();
        if pending.is_some() || self.0.shut_down.get() {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        *pending = Some(glib::timeout_add_local_once(SESSION_SAVE_DELAY, move || {
            let Some(inner) = weak.upgrade() else { return };
            // The source is gone once this runs; forget its id instead of removing it.
            inner.session_save.borrow_mut().take();
            Browser(inner).save_session_now();
        }));
    }

    /// Writes the session now, with the normal windows that exist. Nothing is written when no
    /// normal window is left, so quitting, or closing the last normal window while a private one
    /// stays, never overwrites the last real session with an empty one.
    pub(crate) fn save_session_now(&self) {
        if let Some(pending) = self.0.session_save.borrow_mut().take() {
            pending.remove();
        }
        if self.windows_of(Browsing::Normal).is_empty() || self.0.shut_down.get() {
            return;
        }
        let snapshot = session::snapshot(self);
        let saved = self.core().borrow_mut().session().save(&snapshot);
        if let Err(e) = saved {
            log::warn!("cannot save the session: {e}");
        }
    }

    /// Whether `window` is the one normal window left, which saves the session before it goes
    /// so that the next start restores it.
    pub(crate) fn is_last_normal_window(&self, window: &BrowserWindow) -> bool {
        window.browsing() == Browsing::Normal && self.windows_of(Browsing::Normal).len() <= 1
    }
}

/// Runs the views' refreshes, forgetting the views that are gone. The list is not borrowed while
/// they run, so a refresh may add a view.
fn refresh_views(views: &RefCell<Vec<Weak<dyn Fn()>>>) {
    let live: Vec<Rc<dyn Fn()>> = {
        let mut views = views.borrow_mut();
        views.retain(|view| view.strong_count() > 0);
        views.iter().filter_map(Weak::upgrade).collect()
    };
    for refresh in live {
        refresh();
    }
}

/// The runtime's view of the tabs, read from the live windows.
struct Host(Weak<Inner>);

impl Host {
    fn browser(&self) -> Option<Browser> {
        self.0.upgrade().map(Browser)
    }

    fn window(&self, id: WindowId) -> Option<BrowserWindow> {
        self.browser()?.windows().into_iter().find(|w| w.id() == id.0)
    }
}

/// What `chrome.windows` shows of `window`. Its size is the one on screen once it is shown.
fn window_info(window: &BrowserWindow) -> WindowInfo {
    let minimized = window
        .surface()
        .and_downcast::<gdk::Toplevel>()
        .is_some_and(|toplevel| toplevel.state().contains(gdk::ToplevelState::MINIMIZED));
    let state = if minimized {
        WindowState::Minimized
    } else if window.is_fullscreen() {
        WindowState::Fullscreen
    } else if window.is_maximized() {
        WindowState::Maximized
    } else {
        WindowState::Normal
    };
    let (width, height) = if window.is_mapped() && window.width() > 0 {
        (window.width(), window.height())
    } else {
        window.default_size()
    };
    WindowInfo {
        id: WindowId(window.id()),
        focused: window.is_active(),
        browsing: window.browsing(),
        state,
        width: width.max(0).cast_unsigned(),
        height: height.max(0).cast_unsigned(),
    }
}

fn set_window_state(window: &BrowserWindow, state: WindowState) {
    match state {
        WindowState::Normal => {
            window.unfullscreen();
            window.unmaximize();
            if window_info(window).state == WindowState::Minimized {
                window.present();
            }
        }
        WindowState::Minimized => window.minimize(),
        WindowState::Maximized => {
            window.unfullscreen();
            window.maximize();
        }
        WindowState::Fullscreen => window.fullscreen(),
    }
}

fn set_window_size(window: &BrowserWindow, width: Option<u32>, height: Option<u32>) {
    if width.is_none() && height.is_none() {
        return;
    }
    let (current_width, current_height) = window.default_size();
    let pixels = |size: Option<u32>, current: i32| {
        size.map_or(current, |s| i32::try_from(s).unwrap_or(i32::MAX))
    };
    window.set_default_size(pixels(width, current_width), pixels(height, current_height));
}

impl TabHost for Host {
    fn windows(&self) -> Vec<WindowInfo> {
        self.browser()
            .map(|browser| browser.windows().iter().map(window_info).collect())
            .unwrap_or_default()
    }

    fn tabs(&self) -> Vec<TabInfo> {
        let Some(browser) = self.browser() else { return Vec::new() };
        browser
            .windows()
            .into_iter()
            .flat_map(|window| {
                let window_id = WindowId(window.id());
                let selected = window.selected_tab();
                window
                    .tabs()
                    .into_iter()
                    .enumerate()
                    .map(move |(index, tab)| TabInfo {
                        id: tab.id(),
                        window_id,
                        index: u32::try_from(index).unwrap_or(u32::MAX),
                        // chrome.tabs' `url` is the committed URL, never one still loading.
                        url: tab.committed_uri().unwrap_or_default(),
                        title: tab.display_title(),
                        active: selected.as_ref() == Some(&tab),
                        browsing: tab.browsing(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn create_tab(&self, tab: &NewTab) -> Option<TabId> {
        let browser = self.browser()?;
        let window = match tab.window {
            Some(id) => self.window(id)?,
            None => browser.normal_window(),
        };
        let focus = if tab.active { Focus::Foreground } else { Focus::Background };
        Some(window.open_tab_at(&tab.url, tab.index, focus).id())
    }

    fn update_tab(&self, tab: TabId, url: Option<&str>, active: Option<bool>) -> bool {
        let Some((window, tab)) = self.browser().and_then(|b| b.find_tab(tab)) else {
            return false;
        };
        if let Some(url) = url {
            tab.load(url);
        }
        if active == Some(true) {
            window.select_tab(&tab);
        }
        true
    }

    fn move_tab(&self, tab: TabId, window: WindowId, index: Option<u32>) -> bool {
        let found = self.browser().and_then(|b| b.find_tab(tab));
        let (Some((from, tab)), Some(to)) = (found, self.window(window)) else {
            return false;
        };
        from.move_tab(&tab, &to, index);
        true
    }

    fn remove_tab(&self, tab: TabId) -> bool {
        let Some((window, tab)) = self.browser().and_then(|b| b.find_tab(tab)) else {
            return false;
        };
        window.close_tab(&tab);
        true
    }

    fn web_view(&self, tab: TabId) -> Option<webkit::WebView> {
        self.browser()
            .and_then(|b| b.find_tab(tab))
            .map(|(_, tab)| tab.web_view().clone())
    }

    fn create_window(&self, spec: &NewWindow) -> Option<WindowId> {
        let browser = self.browser()?;
        let window = BrowserWindow::with_browsing(&browser, spec.browsing);
        if let Some((from, tab)) = spec.tab.and_then(|id| browser.find_tab(id)) {
            from.move_tab(&tab, &window, None);
        }
        window.open_tabs(&spec.urls);
        if spec.tab.is_none() && spec.urls.is_empty() {
            window.new_tab();
        }
        set_window_size(&window, spec.width, spec.height);
        set_window_state(&window, spec.state);
        if spec.focused {
            window.present();
        } else {
            window.set_visible(true);
        }
        Some(WindowId(window.id()))
    }

    fn update_window(&self, window: WindowId, update: &WindowUpdate) -> bool {
        let Some(window) = self.window(window) else {
            return false;
        };
        if let Some(state) = update.state {
            set_window_state(&window, state);
        }
        set_window_size(&window, update.width, update.height);
        if update.focused == Some(true) {
            window.present();
        }
        true
    }

    fn remove_window(&self, window: WindowId) -> bool {
        let Some(window) = self.window(window) else {
            return false;
        };
        window.close();
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::test_support::{Reply, Server, browser, wait_until};

    fn history_title(browser: &Browser, url: &str) -> Option<String> {
        let found = browser.core().borrow_mut().history().search(url, 10).ok()?;
        found.into_iter().find(|e| e.url.as_str() == url).map(|e| e.title)
    }

    #[gtk::test]
    fn an_error_page_leaves_the_history_title_of_the_page_that_failed() {
        let offline = Arc::new(AtomicBool::new(false));
        let server = Server::start("127.0.0.1", {
            let offline = offline.clone();
            move |path| match path {
                "/news" if offline.load(Ordering::SeqCst) => Reply::Drop,
                "/news" => Reply::Page("News"),
                _ => Reply::NotFound,
            }
        });
        let browser = browser();
        let url = server.url("/news");
        let window = BrowserWindow::new(&browser);
        let tab = window.open_tab(Some(&url), None, Focus::Foreground);
        wait_until("the visit to get its title", || {
            history_title(&browser, &url).as_deref() == Some("News")
        });

        offline.store(true, Ordering::SeqCst);
        tab.web_view().reload();
        wait_until("the error page", || {
            tab.web_view().title().as_deref() == Some("Problem Loading Page")
        });
        let title = history_title(&browser, &url);
        window.destroy();
        assert_eq!(title.as_deref(), Some("News"));
    }

    #[gtk::test]
    fn leaving_a_page_leaves_its_history_title() {
        let server = Server::start("127.0.0.1", |path| match path {
            "/a" => Reply::Page("A"),
            "/b" => Reply::Page("B"),
            _ => Reply::NotFound,
        });
        let browser = browser();
        let (a, b) = (server.url("/a"), server.url("/b"));
        let window = BrowserWindow::new(&browser);
        let tab = window.open_tab(Some(&a), None, Focus::Foreground);
        wait_until("A to get its title", || history_title(&browser, &a).as_deref() == Some("A"));
        tab.load(&b);
        wait_until("B to get its title", || history_title(&browser, &b).as_deref() == Some("B"));
        let title = history_title(&browser, &a);
        window.destroy();
        assert_eq!(title.as_deref(), Some("A"));
    }

    #[gtk::test]
    fn a_private_session_keeps_its_tabs_out_of_history_and_ends_with_its_last_window() {
        let server = Server::start("127.0.0.1", |_| Reply::Page("Private"));
        let browser = browser();
        let url = server.url("/private");
        let site = Url::parse(&url).unwrap();
        let [first, second] = [(); 2].map(|()| BrowserWindow::with_browsing(&browser, Browsing::Private));
        first.open_tab(None, None, Focus::Background);
        let tab = first.open_tab(Some(&url), None, Focus::Foreground);
        wait_until("the private page's title", || tab.web_view().title().as_deref() == Some("Private"));
        let session = tab.web_view().network_session().expect("the view's network session");
        tab.web_view().set_zoom_level(1.5);
        let zoom = |browsing| browser.core().borrow_mut().site_zoom(browsing).get(&site).unwrap();
        let zoomed = (zoom(Browsing::Private), zoom(Browsing::Normal));
        let visited = history_title(&browser, &url);
        first.close_tab(&tab);
        let kept = browser.can_reopen_closed_tab(Browsing::Private);

        first.destroy();
        let lasts = browser.0.private.borrow().as_ref().is_some_and(|engine| engine.session == session);
        second.destroy();
        let ended = browser.0.private.borrow().is_none() && !browser.can_reopen_closed_tab(Browsing::Private);
        assert!(session.is_ephemeral());
        assert_eq!(visited, None, "a private visit reached history");
        assert_eq!(zoomed, (1.5, 1.0), "the zoom stays in the private session");
        assert!(kept, "the private closed tab is kept");
        assert!(lasts, "the session lasts while a private window is open");
        assert!(ended, "the session ends with the last private window");
        assert_eq!(zoom(Browsing::Private), 1.0, "the private zoom is forgotten");
    }

    #[gtk::test]
    fn bookmarking_a_page_keeps_its_favicon_for_the_bar() {
        let pixels = glib::Bytes::from_owned([200u8, 40, 40, 255].repeat(16 * 16));
        let icon = gtk::gdk::MemoryTexture::new(16, 16, gtk::gdk::MemoryFormat::R8g8b8a8, &pixels, 16 * 4);
        let png = icon.save_to_png_bytes().to_vec();
        let server = Server::start("127.0.0.3", move |path| match path {
            "/" => Reply::Body("text/html", b"<!doctype html><title>Iconic</title><link rel=icon href=/icon.png>".to_vec()),
            "/icon.png" => Reply::Body("image/png", png.clone()),
            _ => Reply::NotFound,
        });
        let browser = browser();
        let url = server.url("/");
        let window = BrowserWindow::new(&browser);
        let tab = window.open_tab(Some(&url), None, Focus::Foreground);
        wait_until("the page's favicon", || tab.web_view().favicon().is_some());
        browser.star_clicked(&window);
        let stored = browser.core().borrow_mut().favicons().get(&Url::parse(&url).unwrap()).unwrap();
        let shown = window.bookmarks_bar().shows_favicon(&url);
        {
            let mut profile = browser.core().borrow_mut();
            let mut bookmarks = profile.bookmarks();
            for node in bookmarks.find_by_url(&Url::parse(&url).unwrap()) {
                bookmarks.remove(node.id).unwrap();
            }
        }
        window.destroy();
        assert!(stored.is_some(), "the favicon is kept once the page is bookmarked");
        assert!(shown, "the bar shows the kept favicon");
    }

    #[gtk::test]
    fn visiting_more_of_a_bookmarked_site_keeps_the_bar_buttons() {
        let png = |rgba: [u8; 4]| {
            let pixels = glib::Bytes::from_owned(rgba.repeat(16 * 16));
            gtk::gdk::MemoryTexture::new(16, 16, gtk::gdk::MemoryFormat::R8g8b8a8, &pixels, 16 * 4).save_to_png_bytes().to_vec()
        };
        let (red, blue) = (png([200, 40, 40, 255]), png([40, 90, 200, 255]));
        let server = Server::start("127.0.0.5", move |path| match path {
            "/" => Reply::Body("text/html", b"<!doctype html><title>Home</title><link rel=icon href=/red.png>".to_vec()),
            "/inbox" => Reply::Body("text/html", b"<!doctype html><title>Inbox</title><link rel=icon href=/blue.png>".to_vec()),
            "/red.png" => Reply::Body("image/png", red.clone()),
            "/blue.png" => Reply::Body("image/png", blue.clone()),
            _ => Reply::NotFound,
        });
        let browser = browser();
        let (url, inbox) = (server.url("/"), Url::parse(&server.url("/inbox")).unwrap());
        let window = BrowserWindow::new(&browser);
        let tab = window.open_tab(Some(&url), None, Focus::Foreground);
        wait_until("the page's favicon", || tab.web_view().favicon().is_some());
        browser.star_clicked(&window);
        let home_icon = browser.core().borrow_mut().favicons().get(&inbox).unwrap();
        let button = window.bookmarks_bar().button_for(&url);
        tab.load(inbox.as_str());
        wait_until("the other page's icon to be kept", || {
            browser.core().borrow_mut().favicons().get(&inbox).unwrap() != home_icon
        });
        let button_after = window.bookmarks_bar().button_for(&url);
        let shown = window.bookmarks_bar().shows_favicon(&url);
        {
            let mut profile = browser.core().borrow_mut();
            let mut bookmarks = profile.bookmarks();
            for node in bookmarks.find_by_url(&Url::parse(&url).unwrap()) {
                bookmarks.remove(node.id).unwrap();
            }
        }
        window.destroy();
        assert!(button.is_some() && button_after == button, "the bar kept its button");
        assert!(shown, "the button still shows the site's icon");
    }

    #[gtk::test]
    fn bookmarked_sites_get_their_icons_without_being_visited() {
        let pixels = glib::Bytes::from_owned([40u8, 90, 200, 255].repeat(16 * 16));
        let icon = gtk::gdk::MemoryTexture::new(16, 16, gtk::gdk::MemoryFormat::R8g8b8a8, &pixels, 16 * 4);
        let png = icon.save_to_png_bytes().to_vec();
        let server = Server::start("127.0.0.4", move |path| match path {
            "/" => Reply::Body("text/html", b"<!doctype html><title>Unvisited</title><link rel=icon href=/icon.png>".to_vec()),
            "/icon.png" => Reply::Body("image/png", png.clone()),
            _ => Reply::NotFound,
        });
        let browser = browser();
        let url = server.url("/");
        let window = BrowserWindow::new(&browser);
        let id = browser.core().borrow_mut().bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Unvisited", &Url::parse(&url).unwrap()).unwrap();
        browser.bookmarks_changed();
        wait_until("the bar to show the fetched icon", || window.bookmarks_bar().shows_favicon(&url));
        browser.core().borrow_mut().bookmarks().remove(id).unwrap();
        window.destroy();
    }

    #[gtk::test]
    fn engine_switches_reach_the_shared_settings_at_once() {
        let browser = browser();
        let settings = browser.engine().settings().clone();
        let state = || {
            (
                settings.is_javascript_can_open_windows_automatically(),
                settings.enables_smooth_scrolling(),
                settings.hardware_acceleration_policy(),
            )
        };
        let defaults = state();
        for pref in [&keys::BLOCK_POPUPS, &keys::SMOOTH_SCROLLING, &keys::HARDWARE_ACCELERATION] {
            browser.set_engine_switch(pref, false);
        }
        let off = state();
        for pref in [&keys::BLOCK_POPUPS, &keys::SMOOTH_SCROLLING, &keys::HARDWARE_ACCELERATION] {
            browser.set_engine_switch(pref, true);
        }
        assert_eq!(defaults, (false, true, webkit::HardwareAccelerationPolicy::Always));
        assert_eq!(off, (true, false, webkit::HardwareAccelerationPolicy::Never));
        assert_eq!(state(), defaults);
    }

    #[gtk::test]
    fn extensions_see_the_committed_url_while_the_next_page_loads() {
        let shown = Server::start("127.0.0.1", |_| Reply::Page("Shown"));
        let requested = Server::start("127.0.0.2", |_| Reply::Hang);
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        let url = shown.url("/");
        let tab = window.open_tab(Some(&url), None, Focus::Foreground);
        wait_until("the first page to commit", || tab.committed_uri().is_some());
        let pending = requested.url("/slow");
        tab.load(&pending);
        wait_until("the next load to start", || {
            tab.web_view().uri().as_deref() == Some(pending.as_str())
        });
        let info = Host(Rc::downgrade(&browser.0))
            .tabs()
            .into_iter()
            .find(|info| info.id == tab.id());
        window.destroy();
        assert_eq!(info.map(|info| info.url), Some(url));
    }

    #[gtk::test]
    fn extension_actions_follow_install_order_not_id_order() {
        use vsesvit_core::extensions::InstallSource;

        let browser = browser();
        let mut dirs: Vec<PathBuf> = ["toolbar-order-a", "toolbar-order-b"]
            .into_iter()
            .map(crate::test_support::scratch_dir)
            .collect();
        // The one installed second gets the smaller id.
        dirs.sort_by_key(|dir| std::cmp::Reverse(ExtensionId::for_unpacked_dir(dir)));
        let (mut ids, mut installed) = (Vec::new(), Vec::new());
        for (i, dir) in dirs.iter().enumerate() {
            let manifest = format!(r#"{{ "manifest_version": 3, "name": "Order {i}", "version": "1.0", "action": {{}} }}"#);
            std::fs::write(dir.join("manifest.json"), manifest).unwrap();
            let source = InstallSource::from_path(dir).unwrap();
            let ext = glib::MainContext::default().block_on(browser.install(source, |_| {})).unwrap().expect("installed");
            installed.push(ext.id.as_str().to_owned());
            ids.push(ext.id);
            // Core lists installs by the millisecond they were made in.
            crate::test_support::settle(Duration::from_millis(5));
        }
        assert!(installed[1] < installed[0], "{installed:?}");
        let ours = |ids: Vec<String>| -> Vec<String> { ids.into_iter().filter(|id| installed.contains(id)).collect() };

        let listed = ours(browser.extension_actions().into_iter().map(|a| a.extension.as_str().to_owned()).collect());
        // A pin change writes every action without an entry yet, in the order it is given.
        browser.pin_extension(&installed[0], true);
        let available: Vec<String> = browser.extension_actions().iter().map(|a| a.extension.as_str().to_owned()).collect();
        let pinned = ours(browser.extension_toolbar(&available).pinned);
        let saved = ours(browser.pref(&toolbar::TOOLBAR).into_iter().map(|e| e.id).collect());
        for id in &ids {
            browser.uninstall_extension(id).ok();
        }
        assert_eq!(listed, installed);
        assert_eq!(pinned, installed);
        assert_eq!(saved, installed);
    }

    #[gtk::test]
    fn a_preference_is_written_and_reset() {
        let browser = browser();
        browser.set_pref(&keys::HOMEPAGE, &"https://example.test/".to_owned());
        let set = browser.core().borrow_mut().prefs().get(&keys::HOMEPAGE);
        browser.reset_pref(&keys::HOMEPAGE);
        let reset = browser.core().borrow_mut().prefs().get(&keys::HOMEPAGE);
        assert_eq!(set, "https://example.test/");
        assert_eq!(reset, "about:home");
    }
}
