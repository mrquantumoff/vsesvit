//! The app-wide controller: the profile, the engine, the open windows and recently closed tabs,
//! and every place the shell reads or writes vsesvit-core.
//!
//! Everything here runs on the UI thread. The `Profile` is borrowed for one core call at a time
//! (`Browser::core`) and never across a XAML call, because core never calls back into the shell
//! but XAML raises events synchronously.

use std::cell::{Cell, RefCell};
use std::hash::{BuildHasher, RandomState};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::history::Transition;
use vsesvit_core::prefs::{Pref, Scope, TabsPosition, Theme, keys};
use vsesvit_core::session::SessionSnapshot;
use vsesvit_core::{Profile, Url};

use crate::bookmarks_bar::{self, BarItem};
use crate::config::{Config, Mode};
use crate::engine::Engine;
use crate::extensions::ExtensionHost;
use crate::popup::ExtensionAction;
use crate::session::{self, TabPlan, WindowPlan};
use crate::updates::{self, Action, Trigger, Updates};
use crate::window::{BrowserWindow, Show, WindowPrefs};
use crate::{app, cli, exec, instance, omnibox, platform, shortcuts};

/// Recently closed tabs kept for Ctrl+Shift+T.
const CLOSED_TABS_KEPT: usize = 25;
/// Tab changes are saved to the session this long after the last one.
const SESSION_SAVE_DELAY: Duration = Duration::from_secs(2);
const SUGGESTIONS: usize = 8;

/// Whether the vertical tab list is collapsed to favicons. Per device: it depends on the
/// screen, so it does not sync.
const TAB_PANE_COLLAPSED: Pref<bool> = Pref {
    key: "tabs.pane_collapsed",
    scope: Scope::Local,
    default: || false,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommitKind {
    /// A new document (`ContentLoading`).
    NewDocument,
    /// History API or fragment navigation within the current document.
    SameDocument,
}

pub(crate) struct ClosedTab {
    pub url: String,
    pub title: String,
}

/// What `main` hands the UI: the opened profile and how opening it went.
pub(crate) struct Launch {
    pub config: Config,
    pub profile: Profile,
    /// How long `Profile::open` took, for the self-test report.
    pub profile_open_ms: u128,
}

pub(crate) struct Browser {
    config: Config,
    profile: RefCell<Profile>,
    engine: Engine,
    page_nonce: String,
    page_script: Rc<str>,
    windows: RefCell<Vec<Rc<BrowserWindow>>>,
    closed_tabs: RefCell<Vec<ClosedTab>>,
    next_tab_id: Cell<u64>,
    prefs: Cell<WindowPrefs>,
    pub(crate) extensions: ExtensionHost,
    session_save_pending: Cell<bool>,
    /// Set once the last window's session has been saved on close; later saves would only
    /// record an empty session over it.
    session_final: Cell<bool>,
    profile_open_ms: u128,
    updates: Updates,
    me: Weak<Browser>,
}

thread_local! {
    static BROWSER: RefCell<Option<Rc<Browser>>> = const { RefCell::new(None) };
}

/// Starts the browser from `OnLaunched`. Failures end the process with a message.
pub(crate) fn launch(launch: Launch) {
    exec::spawn(async move {
        let interactive = launch.config.mode.is_interactive();
        if let Err(e) = start(launch).await {
            log::error!("startup failed: {e}");
            if interactive {
                platform::message_box(
                    &format!(
                        "Vsesvit could not start the web engine.\n\n{e}\n\n\
                         Vsesvit needs the Microsoft Edge WebView2 Runtime."
                    ),
                    crate::bindings::MB_OK | crate::bindings::MB_ICONERROR,
                );
            }
            app::exit(1);
        }
    });
}

async fn start(launch: Launch) -> windows_core::Result<()> {
    let Launch {
        config,
        mut profile,
        profile_open_ms,
    } = launch;
    let engine = Engine::create(&profile.paths().engine_data).await?;
    let page_nonce = nonce();
    let page_script: Rc<str> = shortcuts::page_script(&page_nonce).into();
    let prefs = WindowPrefs {
        tabs: profile.prefs().get(&keys::TABS_POSITION),
        pane_collapsed: profile.prefs().get(&TAB_PANE_COLLAPSED),
        theme: profile.prefs().get(&keys::THEME),
        bookmarks_bar: profile.prefs().get(&keys::SHOW_BOOKMARKS_BAR),
    };
    let updates = if config.mode.is_interactive() {
        Updates::detect()
    } else {
        Updates::disabled("scripted runs do not update".into())
    };
    let browser = Rc::new_cyclic(|me| Browser {
        config,
        profile: RefCell::new(profile),
        engine,
        page_nonce,
        page_script,
        windows: RefCell::new(Vec::new()),
        closed_tabs: RefCell::new(Vec::new()),
        next_tab_id: Cell::new(1),
        prefs: Cell::new(prefs),
        extensions: ExtensionHost::default(),
        session_save_pending: Cell::new(false),
        session_final: Cell::new(false),
        profile_open_ms,
        updates,
        me: me.clone(),
    });
    BROWSER.with_borrow_mut(|b| *b = Some(browser.clone()));
    if let Err(e) = instance::deliver_on_this_thread(open_forwarded) {
        log::warn!("forwarded launches: {e}");
    }

    let show = browser.show_mode();
    let plan = browser.startup_plan();
    log::info!(
        "opening {} window(s) with {} tab(s)",
        plan.len(),
        plan.iter().map(|w| w.tabs.len()).sum::<usize>()
    );
    for window in &plan {
        browser.open_window(window, show)?;
    }
    exec::spawn(browser.clone().start_extensions());
    match &browser.config.mode {
        Mode::Browse => {}
        #[cfg(feature = "self-test")]
        Mode::UiSmoke { out_dir } => {
            exec::spawn(crate::automation::ui_smoke(
                browser.clone(),
                out_dir.clone(),
            ));
        }
        #[cfg(feature = "self-test")]
        Mode::SelfTest { out_dir, network } => {
            exec::spawn(crate::selftest::run(
                browser.clone(),
                out_dir.clone(),
                *network,
            ));
        }
        #[cfg(not(feature = "self-test"))]
        Mode::UiSmoke { .. } | Mode::SelfTest { .. } => {
            log::error!("this build has no self-test (the `self-test` feature is off)");
            app::exit(1);
        }
    }
    if !browser.updates.is_disabled() {
        exec::spawn(updates::schedule(Rc::downgrade(&browser)));
    }
    Ok(())
}

/// The running browser, for work posted to the UI thread from a worker thread.
pub(crate) fn current() -> Option<Rc<Browser>> {
    BROWSER.with_borrow(Clone::clone)
}

/// Unguessable by pages: `RandomState` keys come from the OS random source.
fn nonce() -> String {
    let a = RandomState::new().hash_one(std::process::id());
    let b = RandomState::new().hash_one(std::time::SystemTime::now());
    format!("{a:016x}{b:016x}")
}

/// A command line another process forwarded because this one has the profile open.
fn open_forwarded(args: Vec<String>) {
    let Some(browser) = current() else {
        return;
    };
    let urls = match cli::parse(args.into_iter().map(Into::into)) {
        Ok(cli::Parsed::Run(args)) => args.urls,
        Ok(cli::Parsed::Help | cli::Parsed::Version | cli::Parsed::Update(_)) => Vec::new(),
        Err(e) => {
            log::warn!("forwarded command line: {e}");
            Vec::new()
        }
    };
    log::info!("forwarded launch with {} URL(s)", urls.len());
    browser.open_command_line(&urls);
}

impl Browser {
    /// One core call. `f` must not touch XAML (see the module docs).
    pub fn core<R>(&self, f: impl FnOnce(&mut Profile) -> R) -> R {
        f(&mut self.profile.borrow_mut())
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn page_nonce(&self) -> &str {
        &self.page_nonce
    }

    pub fn page_script(&self) -> Rc<str> {
        self.page_script.clone()
    }

    pub fn profile_open_ms(&self) -> u128 {
        self.profile_open_ms
    }

    pub fn next_tab_id(&self) -> u64 {
        self.next_tab_id.replace(self.next_tab_id.get() + 1)
    }

    pub fn windows(&self) -> Vec<Rc<BrowserWindow>> {
        self.windows.borrow().clone()
    }

    pub fn profile_dir(&self) -> PathBuf {
        self.config.profile_dir.clone()
    }

    fn show_mode(&self) -> Show {
        if self.config.mode.is_interactive() {
            Show::Activate
        } else {
            Show::NoActivate
        }
    }

    // ---- windows ----

    pub fn open_window(
        &self,
        plan: &WindowPlan,
        show: Show,
    ) -> windows_core::Result<Rc<BrowserWindow>> {
        let browser = self.me.upgrade().ok_or_else(windows_core::Error::empty)?;
        let window = BrowserWindow::create(&browser, show, self.prefs.get())?;
        self.windows.borrow_mut().push(window.clone());
        window.set_bookmarks_bar(&self.bookmarks_bar_items());
        window.set_extension_actions(&self.extension_actions());
        window.show_update(self.updates.banner().as_ref());
        window.open_planned(plan)?;
        Ok(window)
    }

    pub fn open_blank_window(&self, show: Show) -> windows_core::Result<Rc<BrowserWindow>> {
        self.open_window(&WindowPlan::with_tabs(vec![TabPlan::blank()]), show)
    }

    /// Command-line URLs from a later launch: tabs in the newest window, or a new window.
    pub fn open_command_line(&self, urls: &[String]) {
        let targets: Vec<String> = urls.iter().filter_map(|u| self.resolve_input(u)).collect();
        let newest = self.windows.borrow().last().cloned();
        let result = match newest {
            Some(window) if !targets.is_empty() => {
                let mut opened = Ok(());
                for (i, url) in targets.iter().enumerate() {
                    if let Err(e) = window.open_url_tab(url, i == 0) {
                        opened = Err(e);
                    }
                }
                if self.config.mode.is_interactive() {
                    window.activate();
                }
                opened
            }
            _ => {
                let tabs = if targets.is_empty() {
                    vec![TabPlan::blank()]
                } else {
                    targets.into_iter().map(TabPlan::url).collect()
                };
                self.open_window(&WindowPlan::with_tabs(tabs), self.show_mode())
                    .map(drop)
            }
        };
        if let Err(e) = result {
            log::error!("opening a forwarded launch: {e}");
        }
    }

    /// The last window is about to close: its tabs are the session to restore next time.
    pub fn window_closing(&self, window: &BrowserWindow) {
        let last = self
            .windows
            .borrow()
            .iter()
            .all(|w| std::ptr::eq(w.as_ref(), window));
        if last && !self.session_final.get() {
            self.save_session_now();
            self.session_final.set(true);
        }
    }

    pub fn window_closed(&self, window: &BrowserWindow) {
        let closed: Vec<_> = self
            .windows
            .borrow_mut()
            .extract_if(.., |w| std::ptr::eq(w.as_ref(), window))
            .collect();
        drop(closed);
        let none_left = self.windows.borrow().is_empty();
        if none_left {
            log::info!("last window closed");
            // `window_closing` saved the session; a ready update installs once `app::run` returns.
            updates::queue_install_on_exit(self);
            app::exit(0);
        } else {
            self.session_changed();
        }
    }

    pub fn remember_closed(&self, tab: ClosedTab) {
        if tab.url.is_empty() || tab.url == "about:blank" {
            return;
        }
        let mut closed = self.closed_tabs.borrow_mut();
        closed.push(tab);
        let excess = closed.len().saturating_sub(CLOSED_TABS_KEPT);
        closed.drain(..excess);
    }

    pub fn take_closed(&self) -> Option<ClosedTab> {
        self.closed_tabs.borrow_mut().pop()
    }

    // ---- startup and session ----

    fn startup_plan(&self) -> Vec<WindowPlan> {
        let (startup, restored, homepage) = self.core(|p| {
            let restored = p.session().restore().unwrap_or_else(|e| {
                log::warn!("session restore: {e}");
                None
            });
            (
                p.prefs().get(&keys::STARTUP),
                restored,
                p.prefs().get(&keys::HOMEPAGE),
            )
        });
        let homepage = self.homepage_url(&homepage);
        let urls = self
            .config
            .start_urls
            .iter()
            .filter_map(|u| self.resolve_input(u))
            .collect();
        session::startup_plan(startup, restored, homepage, urls)
    }

    /// The home page as a URL to load; `None` (a new tab) for the default `about:home`.
    fn homepage_url(&self, homepage: &str) -> Option<String> {
        match homepage.trim() {
            "" | "about:home" | "about:blank" => None,
            text => self.resolve_input(text),
        }
    }

    /// Something changed that a restored session should reflect. Saved a moment later, so a
    /// burst of changes is one write.
    pub fn session_changed(&self) {
        if self.session_final.get() || self.session_save_pending.replace(true) {
            return;
        }
        let me = self.me.clone();
        exec::spawn(async move {
            exec::sleep(SESSION_SAVE_DELAY).await;
            if let Some(browser) = me.upgrade() {
                browser.session_save_pending.set(false);
                if !browser.session_final.get() {
                    browser.save_session_now();
                }
            }
        });
    }

    /// Writes the current windows and tabs as this device's session. Returns whether it worked.
    pub fn save_session_now(&self) -> bool {
        let snapshot = self.snapshot();
        match self.core(|p| p.session().save(&snapshot)) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("session save: {e}");
                false
            }
        }
    }

    fn snapshot(&self) -> SessionSnapshot {
        let windows = self
            .windows()
            .iter()
            .filter_map(|window| {
                let tabs = window.tabs_in_order();
                let active = window
                    .active_tab()
                    .and_then(|a| tabs.iter().position(|t| t.id == a.id));
                let snapshots = tabs
                    .iter()
                    .map(|t| {
                        session::tab_snapshot(
                            t.session_id,
                            &t.session_url(),
                            &t.state().title,
                            t.last_active_ms(),
                        )
                    })
                    .collect();
                let (bounds, maximized) =
                    window.bounds().map_or((None, false), |(b, m)| (Some(b), m));
                session::window_snapshot(snapshots, active, bounds, maximized)
            })
            .collect();
        let device_name = self.core(|p| p.prefs().get(&keys::DEVICE_NAME));
        SessionSnapshot {
            device_name: if device_name.is_empty() {
                std::env::var("COMPUTERNAME").unwrap_or_default()
            } else {
                device_name
            },
            windows,
            active_window: 0,
        }
    }

    // ---- history and bookmarks ----

    /// Every committed main-frame navigation. Returns whether the URL is bookmarked.
    pub fn navigation_committed(
        &self,
        url: &str,
        kind: CommitKind,
        transition: Transition,
    ) -> bool {
        log::debug!("committed ({kind:?}, {transition:?}) {url}");
        self.session_changed();
        let Ok(url) = Url::parse(url) else {
            return false;
        };
        self.core(|p| {
            if let Err(e) = p.history().record_visit(&url, transition) {
                log::warn!("history: {e}");
            }
            p.bookmarks().is_bookmarked(&url)
        })
    }

    pub fn title_changed(&self, url: &str, title: &str) {
        let Ok(url) = Url::parse(url) else { return };
        if let Err(e) = self.core(|p| p.history().set_title(&url, title)) {
            log::warn!("history title: {e}");
        }
        self.session_changed();
    }

    pub fn is_bookmarked(&self, url: &str) -> bool {
        Url::parse(url).is_ok_and(|url| self.core(|p| p.bookmarks().is_bookmarked(&url)))
    }

    /// The star button: removes every bookmark of `url`, or adds one to the bookmarks bar.
    pub fn toggle_bookmark(&self, url: &str, title: &str) {
        let Ok(url) = Url::parse(url) else { return };
        let result = self.core(|p| {
            let mut bookmarks = p.bookmarks();
            let existing: Vec<BookmarkId> =
                bookmarks.find_by_url(&url).iter().map(|n| n.id).collect();
            if existing.is_empty() {
                bookmarks
                    .add_url(BookmarkId::TOOLBAR, InsertAt::End, title, &url)
                    .map(drop)
            } else {
                existing.into_iter().try_for_each(|id| bookmarks.remove(id))
            }
        });
        if let Err(e) = result {
            log::warn!("bookmark {url}: {e}");
        }
        self.bookmarks_changed();
    }

    /// Refreshes everything that shows bookmarks: the bars and each tab's star.
    pub fn bookmarks_changed(&self) {
        let items = self.bookmarks_bar_items();
        for window in self.windows() {
            window.set_bookmarks_bar(&items);
            window.refresh_starred(&|url| self.is_bookmarked(url));
        }
    }

    /// The bookmarks bar's contents, in display order.
    pub fn bookmarks_bar_items(&self) -> Vec<BarItem> {
        self.core(|p| {
            let bookmarks = p.bookmarks();
            bookmarks_bar::items_from(BookmarkId::TOOLBAR, &|folder| bookmarks.children(folder))
        })
    }

    // ---- omnibox ----

    /// What Enter in the address box loads for `text`.
    pub fn resolve_input(&self, text: &str) -> Option<String> {
        if let Some(url) = omnibox::engine_url(text) {
            return Some(url);
        }
        match self.core(|p| p.omnibox().resolve(text)) {
            Ok(target) => target.map(|t| t.url().to_string()),
            Err(e) => {
                log::warn!("omnibox: {e}");
                None
            }
        }
    }

    /// Suggestions for the address box: each label and the URL it opens.
    pub fn suggest(&self, text: &str) -> Vec<(String, String)> {
        match self.core(|p| p.omnibox().suggest(text, SUGGESTIONS)) {
            Ok(list) => list
                .iter()
                .map(|s| (omnibox::label(s), s.target.url().to_string()))
                .collect(),
            Err(e) => {
                log::warn!("omnibox suggestions: {e}");
                Vec::new()
            }
        }
    }

    // ---- preferences ----

    pub fn tabs_position(&self) -> TabsPosition {
        self.prefs.get().tabs
    }

    /// Writes `tabs.position` and moves every window's tab list.
    pub fn set_tabs_position(&self, position: TabsPosition) {
        self.write_pref(&keys::TABS_POSITION, &position);
        self.update_prefs(|p| p.tabs = position);
        for window in self.windows() {
            window.set_tabs_position(position);
        }
    }

    pub fn set_tab_pane_collapsed(&self, collapsed: bool) {
        self.write_pref(&TAB_PANE_COLLAPSED, &collapsed);
        self.update_prefs(|p| p.pane_collapsed = collapsed);
        for window in self.windows() {
            window.set_pane_collapsed(collapsed);
        }
    }

    pub fn bookmarks_bar_visible(&self) -> bool {
        self.prefs.get().bookmarks_bar
    }

    pub fn set_bookmarks_bar_visible(&self, visible: bool) {
        self.write_pref(&keys::SHOW_BOOKMARKS_BAR, &visible);
        self.update_prefs(|p| p.bookmarks_bar = visible);
        for window in self.windows() {
            window.set_bookmarks_bar_visible(visible);
        }
    }

    pub fn theme(&self) -> Theme {
        self.prefs.get().theme
    }

    pub fn set_theme(&self, theme: Theme) {
        self.write_pref(&keys::THEME, &theme);
        self.update_prefs(|p| p.theme = theme);
        for window in self.windows() {
            window.apply_theme(theme);
        }
    }

    fn update_prefs(&self, f: impl FnOnce(&mut WindowPrefs)) {
        let mut prefs = self.prefs.get();
        f(&mut prefs);
        self.prefs.set(prefs);
    }

    pub fn write_pref<T: serde::Serialize>(&self, pref: &Pref<T>, value: &T) {
        if let Err(e) = self.core(|p| p.prefs().set(pref, value)) {
            log::warn!("preference {}: {e}", pref.key);
        }
    }

    // ---- updates ----

    pub fn updates(&self) -> &Updates {
        &self.updates
    }

    /// Shows the update state in every window's update bar.
    pub fn update_state_changed(&self) {
        let banner = self.updates.banner();
        for window in self.windows() {
            window.show_update(banner.as_ref());
        }
    }

    /// The update bar's button.
    pub fn update_action(&self, action: Action) {
        match action {
            Action::Restart => updates::restart(self),
            Action::Retry => exec::spawn(updates::check(self.me.clone(), Trigger::User)),
        }
    }

    /// Whether this installation checks for and downloads updates on its own (`updates.automatic`,
    /// a device-local preference).
    pub fn updates_automatic(&self) -> bool {
        self.core(|p| p.prefs().get(&keys::UPDATES_AUTOMATIC))
    }

    pub fn set_updates_automatic(&self, on: bool) {
        self.write_pref(&keys::UPDATES_AUTOMATIC, &on);
        if on {
            exec::spawn(updates::check(self.me.clone(), Trigger::Scheduled));
        }
    }

    // ---- extensions ----

    pub fn extension_actions(&self) -> Vec<ExtensionAction> {
        self.extensions.actions()
    }

    /// The WebView2 profile, reached through any tab's engine view.
    pub async fn engine_profile(&self) -> Option<crate::bindings::CoreWebView2Profile> {
        let window = self.windows.borrow().first().cloned()?;
        window.engine_profile().await
    }
}

/// Saves the session and drops the browser (and with it every window and web view) before XAML
/// shuts down.
pub(crate) fn shutdown() {
    let browser = BROWSER.with_borrow_mut(Option::take);
    if let Some(browser) = &browser
        && !browser.session_final.get()
        && !browser.windows.borrow().is_empty()
    {
        browser.save_session_now();
    }
    drop(browser);
}
