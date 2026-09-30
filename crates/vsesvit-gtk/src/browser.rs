//! The browser controller: owns the profile (`vsesvit-core`), the engine session shared by
//! every tab, the WebExtensions runtime, and the closed-tab stack. Windows and tabs call it
//! for everything that touches data or policy: history, bookmarks, the omnibox, session
//! persistence, preferences with a live effect, and extension installs.
//!
//! The runtime's [`TabHost`] is implemented here over the live windows, so `chrome.tabs`
//! sees exactly what the user sees.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::favicons::FaviconFetch;
use vsesvit_core::extensions::{ExtensionId, toolbar};
use vsesvit_core::history::Transition;
use vsesvit_core::onboarding;
use vsesvit_core::prefs::{Pref, Startup, TabsPosition, Theme, UpdateChannel, keys};
use vsesvit_core::shortcuts::Keymap;
use vsesvit_core::sync::Changed;
use vsesvit_core::{Profile, Url};
use vsesvit_webext::{Runtime, TabHost, TabId, TabInfo};
use webkit::prelude::*;

use crate::closed_tabs::ClosedTabs;
use crate::downloads::Downloads;
use crate::engine::Engine;
use crate::profile::{self, Core};
use crate::sync::Syncer;
use crate::tab::{Commit, Tab};
use crate::updates::Updates;
use crate::window::{BrowserWindow, Focus};
use crate::{dialogs, favicons, keymap, omnibox, permissions, session};

const CLOSED_TABS_KEPT: usize = 25;
/// How many sites one background favicon fetch looks up.
const FAVICON_BATCH: usize = 32;
/// How long after the last tab change the session is written.
const SESSION_SAVE_DELAY: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct Browser(pub(crate) Rc<Inner>);

pub(crate) struct Inner {
    app: adw::Application,
    core: Core,
    engine: Engine,
    runtime: Runtime,
    downloads: Rc<Downloads>,
    closed_tabs: RefCell<ClosedTabs<ClosedTab>>,
    /// Why the runtime could not load an enabled extension, by extension.
    extension_errors: RefCell<HashMap<ExtensionId, String>>,
    next_tab_id: Cell<u32>,
    next_window_id: Cell<u32>,
    /// The pending debounced session save, if any.
    session_save: RefCell<Option<glib::SourceId>>,
    /// Set once the application has shut down: the session saved then is final.
    shut_down: Cell<bool>,
    favicon_fetch: Cell<FetchState>,
    /// What else shows bookmarks (an open Bookmarks dialog), refreshed with the bars.
    bookmark_views: RefCell<Vec<Weak<dyn Fn()>>>,
    /// An open History dialog, refreshed when sync brings history.
    history_views: RefCell<Vec<Weak<dyn Fn()>>>,
    updates: Option<Updates>,
    sync: Syncer,
    /// This run created the profile: the first window opens the welcome, once.
    welcome: Cell<bool>,
}

/// The background fetch of bookmarked sites' icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FetchState {
    Idle,
    Running,
    /// Bookmarks changed while a fetch ran: look again when it ends.
    RunningStale,
}

/// Enough of a closed tab to bring it back with its history.
pub(crate) struct ClosedTab {
    pub(crate) uri: String,
    pub(crate) state: Option<webkit::WebViewSessionState>,
    pub(crate) position: i32,
}

impl Browser {
    /// Wraps an open profile. The extension runtime is created here, before any tab web
    /// view exists, because every tab's view is built with the runtime's content manager.
    pub(crate) fn new(app: &adw::Application, profile: Profile) -> Self {
        let core: Core = Rc::new(RefCell::new(profile));
        let engine = Engine::new(&mut core.borrow_mut());
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
                downloads,
                closed_tabs: RefCell::new(ClosedTabs::new(CLOSED_TABS_KEPT)),
                extension_errors: RefCell::new(HashMap::new()),
                next_tab_id: Cell::new(1),
                next_window_id: Cell::new(1),
                session_save: RefCell::new(None),
                shut_down: Cell::new(false),
                favicon_fetch: Cell::new(FetchState::Idle),
                bookmark_views: RefCell::new(Vec::new()),
                history_views: RefCell::new(Vec::new()),
                updates: Updates::new(app, updates_automatic, updates_channel),
                sync,
                welcome: Cell::new(welcome),
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
        let browser = Browser(inner);
        permissions::seed_notifications(&browser);
        browser
    }

    /// Applies the profile's preferences and brings the extension runtime in line with the
    /// profile: loads every enabled extension, then reconciles against the synced desired
    /// state (installs missing store extensions, unloads ones removed elsewhere).
    pub(crate) fn start(&self) {
        self.apply_theme();
        self.apply_keymap();
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

    pub(crate) fn downloads(&self) -> &Rc<Downloads> {
        &self.0.downloads
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
        let startup = self.core().borrow_mut().prefs().get(&keys::STARTUP);
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
                .windows()
                .into_iter()
                .next()
                .unwrap_or_else(|| BrowserWindow::new(self));
            for (i, target) in targets.iter().enumerate() {
                let focus = if i == 0 { Focus::Foreground } else { Focus::Background };
                window.open_tab(Some(target.as_str()), None, focus);
            }
            window.present();
        }
        if self.0.welcome.take()
            && let Some(window) = self.windows().into_iter().next()
        {
            dialogs::welcome::present(&window);
        }
    }

    /// Opens a window with a tab for each target, the first one selected, or a blank tab.
    pub(crate) fn open_window(&self, targets: &[Url]) -> BrowserWindow {
        let window = BrowserWindow::new(self);
        if targets.is_empty() {
            window.new_tab();
        }
        for (i, target) in targets.iter().enumerate() {
            let focus = if i == 0 { Focus::Foreground } else { Focus::Background };
            window.open_tab(Some(target.as_str()), None, focus);
        }
        window.present();
        window
    }

    pub(crate) fn present(&self) {
        match self.0.app.active_window() {
            Some(window) => window.present(),
            None => {
                self.open_window(&[]);
            }
        }
    }

    /// Called from the application's `shutdown`: the last session write. Nothing writes it
    /// afterwards, so tearing the windows down for a restart cannot overwrite it.
    pub(crate) fn shutdown(&self) {
        self.save_session_now();
        self.0.shut_down.set(true);
    }

    // Tabs.

    pub(crate) fn tab_closed(&self, tab: &Tab, position: i32) {
        permissions::closed(tab);
        self.runtime().tab_closed(tab.id());
        self.schedule_session_save();
        let Some(uri) = tab.committed_uri().filter(|uri| uri != "about:blank") else {
            return;
        };
        let state = tab.web_view().session_state();
        self.0.closed_tabs.borrow_mut().push(ClosedTab {
            uri,
            state,
            position,
        });
    }

    /// A tab that goes away with its window, without being closed one by one.
    pub(crate) fn tab_discarded(&self, tab: &Tab) {
        permissions::closed(tab);
        self.runtime().tab_closed(tab.id());
    }

    pub(crate) fn tab_activated(&self, tab: &Tab) {
        self.runtime().tab_activated(tab.id());
        self.schedule_session_save();
    }

    pub(crate) fn reopen_closed_tab(&self, window: &BrowserWindow) {
        let closed = self.0.closed_tabs.borrow_mut().pop();
        if let Some(closed) = closed {
            window.restore_closed(&closed);
        }
    }

    // History and bookmarks.

    /// Every committed main-frame navigation records a visit (not for an error page).
    pub(crate) fn navigation_committed(&self, tab: &Tab, uri: &str, commit: Commit) {
        let transition = tab.take_pending_transition().unwrap_or(Transition::Link);
        if commit != Commit::SameDocument
            && let Ok(url) = Url::parse(uri)
        {
            self.show_at_site_zoom(tab, &url);
        }
        if commit != Commit::ErrorPage
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
        let level = match self.core().borrow_mut().site_zoom().get(url) {
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
    /// for the site the tab is on.
    pub(crate) fn zoom_changed(&self, tab: &Tab) {
        let Some(url) = tab.committed_uri().and_then(|uri| Url::parse(&uri).ok()) else {
            return;
        };
        let level = tab.web_view().zoom_level();
        if let Err(e) = self.core().borrow_mut().site_zoom().set(&url, level) {
            log::warn!("site zoom: {e}");
        }
    }

    /// Titles arrive after the commit, and are written to history under the committed URI,
    /// with two exceptions. WebKit clears the title as the next document commits, before it
    /// reports the commit, so an empty title would land on the page being left. An error
    /// page's title is ours, not that of the URI that failed.
    pub(crate) fn title_changed(&self, tab: &Tab) {
        if !tab.shows_error_page()
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
                window.toast(adw::Toast::new(&format!("Cannot bookmark the page: {e}")));
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

    /// Returns whether the stored icon changed.
    fn save_favicon(&self, tab: &Tab) -> bool {
        let (Some(uri), Some(icon)) = (tab.committed_uri(), tab.web_view().favicon()) else {
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

    /// Runs `refresh` after sync changes the history, for as long as the caller keeps it.
    pub(crate) fn watch_history(&self, refresh: &Rc<dyn Fn()>) {
        self.0.history_views.borrow_mut().push(Rc::downgrade(refresh));
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
        let available: Vec<String> = self.runtime().actions().iter().map(|a| a.extension.as_str().to_owned()).collect();
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
    /// history) followed by matching open tabs.
    pub(crate) fn omnibox_changed(&self, window: &BrowserWindow, text: &str) {
        let suggestions = omnibox::suggestions(self, window, text);
        window.address_bar().set_suggestions(suggestions);
    }

    /// Enter in the address bar: core decides between a URL and a search.
    pub(crate) fn omnibox_activated(&self, window: &BrowserWindow, text: &str) {
        let resolved = self.core().borrow_mut().omnibox().resolve(text);
        match resolved {
            Ok(Some(target)) => window.navigate_with(target.url().as_str(), Transition::Typed),
            Ok(None) => window.focus_page(),
            Err(e) => window.toast(adw::Toast::new(&format!("Cannot resolve the address: {e}"))),
        }
    }

    // Preferences with a live effect.

    pub(crate) fn tabs_position(&self) -> TabsPosition {
        self.core().borrow_mut().prefs().get(&keys::TABS_POSITION)
    }

    /// Writes the synced preference and re-lays out every open window at once.
    pub(crate) fn set_tabs_position(&self, position: TabsPosition) {
        let set = self.core().borrow_mut().prefs().set(&keys::TABS_POSITION, &position);
        if let Err(e) = set {
            log::warn!("prefs: {e}");
        }
        for window in self.windows() {
            window.apply_layout(position);
        }
    }

    pub(crate) fn switch(&self, pref: &Pref<bool>) -> bool {
        self.core().borrow_mut().prefs().get(pref)
    }

    /// Writes an on/off preference. The `set_*` methods that call it also apply it.
    pub(crate) fn set_switch(&self, pref: &Pref<bool>, on: bool) {
        let set = self.core().borrow_mut().prefs().set(pref, &on);
        if let Err(e) = set {
            log::warn!("prefs: {e}");
        }
    }

    pub(crate) fn bookmarks_bar_visible(&self) -> bool {
        self.switch(&keys::SHOW_BOOKMARKS_BAR)
    }

    pub(crate) fn set_bookmarks_bar_visible(&self, shown: bool) {
        self.set_switch(&keys::SHOW_BOOKMARKS_BAR, shown);
        for window in self.windows() {
            window.set_bookmarks_bar_visible(shown);
        }
    }

    pub(crate) fn home_button_visible(&self) -> bool {
        self.switch(&keys::SHOW_HOME_BUTTON)
    }

    pub(crate) fn set_home_button_visible(&self, shown: bool) {
        self.set_switch(&keys::SHOW_HOME_BUTTON, shown);
        for window in self.windows() {
            window.set_home_button_visible(shown);
        }
    }

    pub(crate) fn compact_address_bar(&self) -> bool {
        self.switch(&keys::COMPACT_ADDRESS_BAR)
    }

    pub(crate) fn set_compact_address_bar(&self, compact: bool) {
        self.set_switch(&keys::COMPACT_ADDRESS_BAR, compact);
        for window in self.windows() {
            window.set_compact_address_bar(compact);
        }
    }

    pub(crate) fn full_urls(&self) -> bool {
        self.switch(&keys::SHOW_FULL_URLS)
    }

    pub(crate) fn set_full_urls(&self, full: bool) {
        self.set_switch(&keys::SHOW_FULL_URLS, full);
        for window in self.windows() {
            window.set_full_urls(full);
        }
    }

    /// Pop-ups, smooth scrolling and hardware acceleration: the engine's one settings
    /// object applies them to every view.
    pub(crate) fn set_engine_switch(&self, pref: &Pref<bool>, on: bool) {
        self.set_switch(pref, on);
        self.engine().apply_prefs(&mut self.core().borrow_mut());
    }

    pub(crate) fn theme(&self) -> Theme {
        self.core().borrow_mut().prefs().get(&keys::THEME)
    }

    pub(crate) fn set_theme(&self, theme: Theme) {
        let set = self.core().borrow_mut().prefs().set(&keys::THEME, &theme);
        if let Err(e) = set {
            log::warn!("prefs: {e}");
        }
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
        self.set_switch(&keys::UPDATES_AUTOMATIC, automatic);
        if let Some(updates) = &self.0.updates {
            updates.set_automatic(automatic);
        }
    }

    /// The Settings choice of `updates.channel`, a local preference: writes it and checks the
    /// new channel, even with automatic updates off, because the user just asked for it.
    pub(crate) fn set_updates_channel(&self, channel: UpdateChannel) {
        let set = self
            .core()
            .borrow_mut()
            .prefs()
            .set(&keys::UPDATES_CHANNEL, &channel);
        if let Err(e) = set {
            log::warn!("prefs: {e}");
        }
        if let Some(updates) = &self.0.updates {
            updates.set_channel(channel);
        }
    }

    pub(crate) fn keymap(&self) -> Keymap {
        self.core().borrow_mut().prefs().keymap()
    }

    /// Every change to the shortcuts goes through here: `edit` changes the stored keymap and
    /// every window's shortcuts follow at once.
    pub(crate) fn edit_keymap<T>(&self, edit: impl FnOnce(&mut Keymap) -> T) -> T {
        let mut edited = self.keymap();
        let out = edit(&mut edited);
        if let Err(e) = self.core().borrow_mut().prefs().set_keymap(&edited) {
            log::warn!("prefs: {e}");
        }
        keymap::apply(self.app(), &edited);
        out
    }

    pub(crate) fn apply_keymap(&self) {
        keymap::apply(self.app(), &self.keymap());
    }

    /// The homepage preference as a URL. `about:home`, the default, means the new tab page.
    pub(crate) fn homepage(&self) -> Option<Url> {
        let text = self.core().borrow_mut().prefs().get(&keys::HOMEPAGE);
        let text = text.trim();
        if text.is_empty() || text == "about:home" {
            return None;
        }
        Url::parse(text).ok()
    }

    /// What the shell refreshes after sync applied remote records (`ApplyReport::changed`).
    /// Search engines and sessions need nothing: the address bar reads the engines each time.
    pub(crate) fn sync_applied(&self, changed: &Changed) {
        if changed.bookmarks {
            self.bookmarks_changed();
        }
        if changed.history {
            refresh_views(&self.0.history_views);
        }
        if changed.extensions {
            self.reconcile_extensions();
        }
        for (ext, changes) in &changed.ext_storage {
            self.runtime().storage_sync_changed(ext, changes);
        }
        if changed.site_permissions {
            permissions::enforce(self);
        }
        if changed.prefs.iter().any(|key| key == keys::SHORTCUTS.key) {
            self.apply_keymap();
        }
        if !changed.prefs.is_empty() {
            self.apply_theme();
            self.engine().apply_prefs(&mut self.core().borrow_mut());
            let position = self.tabs_position();
            let (bar, home) = (self.bookmarks_bar_visible(), self.home_button_visible());
            let (compact, full_urls) = (self.compact_address_bar(), self.full_urls());
            for window in self.windows() {
                window.apply_layout(position);
                window.set_bookmarks_bar_visible(bar);
                window.set_home_button_visible(home);
                window.set_compact_address_bar(compact);
                window.set_full_urls(full_urls);
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

    /// Writes the session now, with the windows that exist. Nothing is written when no
    /// window is left, so quitting never overwrites the last real session with an empty one.
    pub(crate) fn save_session_now(&self) {
        if let Some(pending) = self.0.session_save.borrow_mut().take() {
            pending.remove();
        }
        if self.windows().is_empty() || self.0.shut_down.get() {
            return;
        }
        let snapshot = session::snapshot(self);
        let saved = self.core().borrow_mut().session().save(&snapshot);
        if let Err(e) = saved {
            log::warn!("cannot save the session: {e}");
        }
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
}

impl TabHost for Host {
    fn tabs(&self) -> Vec<TabInfo> {
        let Some(browser) = self.browser() else { return Vec::new() };
        browser
            .windows()
            .into_iter()
            .flat_map(|window| {
                let window_id = window.id();
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
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn create_tab(&self, url: &str, active: bool) -> Option<TabId> {
        let browser = self.browser()?;
        let window = browser
            .windows()
            .into_iter()
            .next()
            .unwrap_or_else(|| browser.open_window(&[]));
        let focus = if active { Focus::Foreground } else { Focus::Background };
        Some(window.open_tab(Some(url), None, focus).id())
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
}
