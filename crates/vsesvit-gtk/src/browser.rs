//! The browser controller: owns the profile (`vsesvit-core`), the engine session shared by
//! every tab, the WebExtensions runtime, and the closed-tab stack. Windows and tabs call it
//! for everything that touches data or policy: history, bookmarks, the omnibox, session
//! persistence, preferences with a live effect, and extension installs.
//!
//! The runtime's [`TabHost`] is implemented here over the live windows, so `chrome.tabs`
//! sees exactly what the user sees.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;
use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::history::Transition;
use vsesvit_core::prefs::{Startup, TabsPosition, Theme, keys};
use vsesvit_core::sync::Changed;
use vsesvit_core::{Profile, Url};
use vsesvit_webext::{Runtime, TabHost, TabId, TabInfo};
use webkit::prelude::*;

use crate::closed_tabs::ClosedTabs;
use crate::engine::Engine;
use crate::profile::{self, Core};
use crate::tab::{Commit, Tab};
use crate::updates::Updates;
use crate::window::{BrowserWindow, Focus};
use crate::{downloads, omnibox, session};

const CLOSED_TABS_KEPT: usize = 25;
/// How long after the last tab change the session is written.
const SESSION_SAVE_DELAY: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct Browser(pub(crate) Rc<Inner>);

pub(crate) struct Inner {
    app: adw::Application,
    core: Core,
    engine: Engine,
    runtime: Runtime,
    downloads_dir: PathBuf,
    closed_tabs: RefCell<ClosedTabs<ClosedTab>>,
    next_tab_id: Cell<u32>,
    next_window_id: Cell<u32>,
    /// The pending debounced session save, if any.
    session_save: RefCell<Option<glib::SourceId>>,
    updates: Option<Updates>,
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
        let paths = core.borrow().paths().clone();
        let engine = Engine::new(&paths);
        let downloads_dir = profile::downloads_dir();
        downloads::watch(engine.session(), downloads_dir.clone(), app);
        let updates_automatic = core.borrow_mut().prefs().get(&keys::UPDATES_AUTOMATIC);
        let inner = Rc::new_cyclic(|weak: &Weak<Inner>| {
            let host: Rc<dyn TabHost> = Rc::new(Host(weak.clone()));
            let runtime = Runtime::new(core.clone(), engine.session(), host);
            Inner {
                app: app.clone(),
                core,
                engine,
                runtime,
                downloads_dir,
                closed_tabs: RefCell::new(ClosedTabs::new(CLOSED_TABS_KEPT)),
                next_tab_id: Cell::new(1),
                next_window_id: Cell::new(1),
                session_save: RefCell::new(None),
                updates: Updates::new(app, updates_automatic),
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
        Browser(inner)
    }

    /// Applies the profile's preferences and brings the extension runtime in line with the
    /// profile: loads every enabled extension, then reconciles against the synced desired
    /// state (installs missing store extensions, unloads ones removed elsewhere).
    pub(crate) fn start(&self) {
        self.apply_theme();
        let installed = self.core().borrow_mut().extensions().list();
        match installed {
            Ok(list) => {
                for ext in list.into_iter().filter(|e| e.enabled) {
                    if let Err(e) = self.runtime().load(&ext) {
                        log::warn!("extension {}: {e}", ext.id.as_str());
                    }
                }
            }
            Err(e) => log::warn!("cannot list extensions: {e}"),
        }
        self.reconcile_extensions();
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

    pub(crate) fn downloads_dir(&self) -> &Path {
        &self.0.downloads_dir
    }

    /// `None` when this copy does not update itself.
    pub(crate) fn updates(&self) -> Option<&Updates> {
        self.0.updates.as_ref()
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
    /// according to the startup preference, plus any URLs from the command line.
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

    /// Called from the application's `shutdown`. The windows are gone by then, so this only
    /// flushes; the last window saves the session as it closes.
    pub(crate) fn shutdown(&self) {
        self.save_session_now();
    }

    // Tabs.

    pub(crate) fn tab_closed(&self, tab: &Tab, position: i32) {
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

    /// Titles arrive after the commit.
    pub(crate) fn title_changed(&self, tab: &Tab) {
        if let (Some(uri), Some(title)) = (tab.committed_uri(), tab.web_view().title())
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

    /// The star button or Ctrl+D: bookmarks the page into the bookmarks bar, or removes
    /// every bookmark of its URL.
    pub(crate) fn star_clicked(&self, window: &BrowserWindow) {
        let Some(tab) = window.selected_tab() else { return };
        let Some(url) = tab.committed_uri().and_then(|u| Url::parse(&u).ok()) else {
            return;
        };
        let title = tab.display_title();
        let result = {
            let mut p = self.core().borrow_mut();
            let mut bookmarks = p.bookmarks();
            let existing = bookmarks.find_by_url(&url);
            if existing.is_empty() {
                bookmarks
                    .add_url(BookmarkId::TOOLBAR, InsertAt::End, &title, &url)
                    .map(|_| "Bookmark added to the bookmarks bar")
            } else {
                existing
                    .iter()
                    .try_for_each(|node| bookmarks.remove(node.id))
                    .map(|()| "Bookmark removed")
            }
        };
        match result {
            Ok(message) => window.toast(adw::Toast::builder().title(message).timeout(2).build()),
            Err(e) => window.toast(adw::Toast::new(&format!("Cannot change the bookmark: {e}"))),
        }
        self.bookmarks_changed();
    }

    /// After any bookmark write: every window's bar and star follow the new tree.
    pub(crate) fn bookmarks_changed(&self) {
        for window in self.windows() {
            window.refresh_bookmarks_bar();
            window.sync_star();
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

    pub(crate) fn bookmarks_bar_visible(&self) -> bool {
        self.core().borrow_mut().prefs().get(&keys::SHOW_BOOKMARKS_BAR)
    }

    pub(crate) fn set_bookmarks_bar_visible(&self, shown: bool) {
        let set = self.core().borrow_mut().prefs().set(&keys::SHOW_BOOKMARKS_BAR, &shown);
        if let Err(e) = set {
            log::warn!("prefs: {e}");
        }
        for window in self.windows() {
            window.set_bookmarks_bar_visible(shown);
        }
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

    /// `updates.automatic`, a local preference: whether this installation checks for and
    /// downloads updates on its own.
    pub(crate) fn updates_automatic(&self) -> bool {
        self.core().borrow_mut().prefs().get(&keys::UPDATES_AUTOMATIC)
    }

    /// The Settings switch: writes the preference and starts or stops the checks.
    pub(crate) fn set_updates_automatic(&self, automatic: bool) {
        let set = self.core().borrow_mut().prefs().set(&keys::UPDATES_AUTOMATIC, &automatic);
        if let Err(e) = set {
            log::warn!("prefs: {e}");
        }
        if let Some(updates) = &self.0.updates {
            updates.set_automatic(automatic);
        }
    }

    /// The homepage preference as a URL. `about:home`, the default, means a blank tab.
    pub(crate) fn homepage(&self) -> Option<Url> {
        let text = self.core().borrow_mut().prefs().get(&keys::HOMEPAGE);
        let text = text.trim();
        if text.is_empty() || text == "about:home" {
            return None;
        }
        Url::parse(text).ok()
    }

    /// What the shell refreshes after a sync engine applied remote records
    /// (`ApplyReport::changed`). No sync engine exists yet, so the only caller is the
    /// development action `app.debug-apply-sync`.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(crate) fn sync_applied(&self, changed: &Changed) {
        if changed.bookmarks {
            self.bookmarks_changed();
        }
        if changed.extensions {
            self.reconcile_extensions();
        }
        for (ext, changes) in &changed.ext_storage {
            self.runtime().storage_sync_changed(ext, changes);
        }
        if !changed.prefs.is_empty() {
            self.apply_theme();
            let position = self.tabs_position();
            let bar = self.bookmarks_bar_visible();
            for window in self.windows() {
                window.apply_layout(position);
                window.set_bookmarks_bar_visible(bar);
            }
        }
    }

    // Session.

    /// About two seconds after the last tab change.
    pub(crate) fn schedule_session_save(&self) {
        let mut pending = self.0.session_save.borrow_mut();
        if pending.is_some() {
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
        if self.windows().is_empty() {
            return;
        }
        let snapshot = session::snapshot(self);
        let saved = self.core().borrow_mut().session().save(&snapshot);
        if let Err(e) = saved {
            log::warn!("cannot save the session: {e}");
        }
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
                        url: tab.web_view().uri().map(String::from).unwrap_or_default(),
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
