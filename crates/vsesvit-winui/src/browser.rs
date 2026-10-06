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

use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode, InsertAt};
use vsesvit_core::extensions::commands::extension_commands;
use vsesvit_core::extensions::toolbar::{self, Layout};
use vsesvit_core::favicons::FaviconFetch;
use vsesvit_core::history::Transition;
use vsesvit_core::https_only::{self, Reach};
use vsesvit_core::permissions::SiteSetting;
use vsesvit_core::prefs::{Pref, Scope, TabsPosition, Theme, UpdateChannel, homepage_url, keys};
use vsesvit_core::search::{SelectionAction, Suggestions};
use vsesvit_core::session::SessionSnapshot;
use vsesvit_core::shortcuts::Keymap;
use vsesvit_core::suggest::{Queries, SuggestRequest};
use vsesvit_core::sync::Changed;
use vsesvit_core::trackers::TrackerList;
use vsesvit_core::{Profile, Url, onboarding};
use windows_core::Interface;

use crate::bindings::{CoreWebView2BrowsingDataKinds, ICoreWebView2Profile2};
use crate::bookmark_editor::{self, Edit, FolderChoice, Target};
use crate::bookmarks_bar::{self, BarItem};
use crate::config::{Config, Mode};
use crate::cookies::ExitClearing;
use crate::dialogs::{Dialog, DialogWindow};
use crate::downloads::Downloads;
use crate::engine::{self, Engine};
use crate::extensions::ExtensionHost;
use crate::popup::ExtensionAction;
use crate::session::{self, TabPlan, WindowPlan};
use crate::shortcuts::Bindings;
use crate::sync::{PrefEffect, SyncController};
use crate::updates::{self, Action, Trigger, Updates};
use crate::window::{Backdrop, BrowserWindow, Show, WindowPrefs};
use crate::{app, cli, cookies, exec, instance, omnibox, platform, shortcuts, sync};

/// Recently closed tabs kept for Ctrl+Shift+T.
const CLOSED_TABS_KEPT: usize = 25;
/// Tab changes are saved to the session this long after the last one.
const SESSION_SAVE_DELAY: Duration = Duration::from_secs(2);
const SUGGESTIONS: usize = 8;
/// Bookmarked pages whose icons one preload round fetches.
const FAVICON_BATCH: usize = 24;
const WELCOME_WAIT: Duration = Duration::from_secs(10);

/// Whether the vertical tab list is collapsed to favicons. Per device: it depends on the
/// screen, so it does not sync.
const TAB_PANE_COLLAPSED: Pref<bool> = Pref {
    key: "tabs.pane_collapsed",
    scope: Scope::Local,
    default: || false,
};

/// The vertical tab list's width when expanded, in view pixels. Per device, like collapsing it.
const TAB_PANE_WIDTH: Pref<u32> = Pref {
    key: "tabs.pane_width",
    scope: Scope::Local,
    default: || 240,
};

/// The window material. Per device: only the Windows shell has one.
const WINDOW_BACKDROP: Pref<Backdrop> = Pref {
    key: "window.backdrop",
    scope: Scope::Local,
    default: || Backdrop::Mica,
};

/// Whether the passwords WebView2 saved before Vsesvit stopped saving them are deleted. Per
/// device: they are in this device's engine data.
pub(crate) const PASSWORDS_PURGED: Pref<bool> = Pref {
    key: "passwords.purged",
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

/// Where fetching the icons of bookmarked pages is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Preload {
    Idle,
    Running,
    /// Bookmarks changed during a run: look for pages without an icon once more at its end.
    Requested,
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
    page_script: Rc<shortcuts::PageScript>,
    /// The tracker list tracking protection blocks from (see `trackers`).
    trackers: RefCell<Rc<TrackerList>>,
    /// Which http URLs HTTPS-only upgrades (the self-tests add their local server).
    https_reach: Cell<Reach>,
    windows: RefCell<Vec<Rc<BrowserWindow>>>,
    /// Bookmarks, History, Downloads and Settings, each in a window of its own while open.
    dialog_windows: RefCell<Vec<Rc<DialogWindow>>>,
    closed_tabs: RefCell<Vec<ClosedTab>>,
    next_tab_id: Cell<u64>,
    prefs: Cell<WindowPrefs>,
    pub(crate) extensions: ExtensionHost,
    pub(crate) downloads: Downloads,
    session_save_pending: Cell<bool>,
    favicon_preload: Cell<Preload>,
    /// Called whenever fetched icons were stored, while their owners keep them.
    favicon_listeners: RefCell<Vec<Weak<dyn Fn()>>>,
    /// Set once the last window's session has been saved on close; later saves would only
    /// record an empty session over it.
    session_final: Cell<bool>,
    /// Started as the last window or its last tab closes, while a tab can still reach the engine.
    exit_clearing: RefCell<Option<ExitClearing>>,
    profile_open_ms: u128,
    updates: Updates,
    sync: SyncController,
    pub(crate) site_mirror: crate::permissions::EngineMirror,
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
    // Read before anything can end this first session: the next launch is no first run.
    let welcome = config.mode == Mode::Browse && onboarding::should_show(&mut profile);
    let mut arguments = engine::browser_arguments(|pref| profile.prefs().get(pref));
    if !config.mode.is_interactive() {
        arguments = format!("{arguments} {}", engine::SCRIPTED_ARGUMENTS)
            .trim()
            .to_owned();
    }
    let engine = Engine::create(&profile.paths().engine_data, &arguments).await?;
    let page_script = Rc::new(shortcuts::PageScript::new(&secret()));
    shortcuts::set_current(Bindings::new(profile.prefs().keymap(), Vec::new(), &[]));
    let prefs = WindowPrefs {
        tabs: profile.prefs().get(&keys::TABS_POSITION),
        pane_collapsed: profile.prefs().get(&TAB_PANE_COLLAPSED),
        pane_width: profile.prefs().get(&TAB_PANE_WIDTH),
        theme: profile.prefs().get(&keys::THEME),
        bookmarks_bar: profile.prefs().get(&keys::SHOW_BOOKMARKS_BAR),
        home_button: profile.prefs().get(&keys::SHOW_HOME_BUTTON),
        backdrop: profile.prefs().get(&WINDOW_BACKDROP),
        compact_address: profile.prefs().get(&keys::COMPACT_ADDRESS_BAR),
        full_urls: profile.prefs().get(&keys::SHOW_FULL_URLS),
        media_player: profile.prefs().get(&keys::SHOW_MEDIA_PLAYER),
        pip: profile.prefs().get(&keys::PICTURE_IN_PICTURE),
    };
    let downloads = Downloads::new(&mut profile);
    let updates = if config.mode.is_interactive() {
        Updates::detect()
    } else {
        Updates::disabled("scripted runs do not update".into())
    };
    let sync = SyncController::load(&mut profile);
    let browser = Rc::new_cyclic(|me| Browser {
        config,
        profile: RefCell::new(profile),
        engine,
        page_script,
        trackers: RefCell::new(Rc::new(TrackerList::bundled().clone())),
        https_reach: Cell::new(Reach::Public),
        windows: RefCell::new(Vec::new()),
        dialog_windows: RefCell::new(Vec::new()),
        closed_tabs: RefCell::new(Vec::new()),
        next_tab_id: Cell::new(1),
        prefs: Cell::new(prefs),
        extensions: ExtensionHost::default(),
        downloads,
        session_save_pending: Cell::new(false),
        favicon_preload: Cell::new(Preload::Idle),
        favicon_listeners: RefCell::new(Vec::new()),
        session_final: Cell::new(false),
        exit_clearing: RefCell::new(None),
        profile_open_ms,
        updates,
        sync,
        site_mirror: crate::permissions::EngineMirror::default(),
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
    if welcome && let Some(window) = browser.windows().into_iter().next() {
        exec::spawn(show_welcome(window));
    }
    exec::spawn(browser.clone().start_extensions());
    if !browser.core(|p| p.prefs().get(&PASSWORDS_PURGED)) {
        exec::spawn(browser.clone().purge_saved_passwords());
    }
    crate::permissions::mirror(&browser);
    if browser.config.mode.is_interactive() {
        browser.preload_favicons();
    }
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
    exec::spawn(sync::schedule(Rc::downgrade(&browser)));
    Ok(())
}

/// The welcome over the first window, once that window can hold a dialog.
async fn show_welcome(window: Rc<BrowserWindow>) {
    let ready = exec::wait_for(WELCOME_WAIT, Duration::from_millis(50), || {
        window.xaml_root().ok()
    })
    .await;
    if ready.is_none() {
        log::warn!("the first window never got a XAML root; no welcome");
        return;
    }
    log::info!("first run: showing the welcome");
    window.show_dialog(Dialog::Welcome);
}

/// The running browser, for work posted to the UI thread from a worker thread.
pub(crate) fn current() -> Option<Rc<Browser>> {
    BROWSER.with_borrow(Clone::clone)
}

/// Unguessable: `RandomState` keys come from the OS random source.
fn secret() -> String {
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

    pub fn page_script(&self) -> Rc<shortcuts::PageScript> {
        self.page_script.clone()
    }

    pub fn trackers(&self) -> Rc<TrackerList> {
        self.trackers.borrow().clone()
    }

    /// Blocks from `list` instead, in the tabs already open too (the self-tests add a tracker).
    #[cfg(feature = "self-test")]
    pub fn set_trackers(&self, list: TrackerList) {
        let list = Rc::new(list);
        for tab in self.windows().iter().flat_map(|w| w.tabs_in_order()) {
            tab.filter_trackers(&list);
        }
        *self.trackers.borrow_mut() = list;
    }

    /// The https URL a navigation to `url` loads instead, under HTTPS-only.
    pub fn https_upgrade(&self, url: &Url) -> Option<Url> {
        let reach = self.https_reach.get();
        self.core(|p| https_only::upgrade(p, url, reach))
    }

    #[cfg(feature = "self-test")]
    pub fn set_https_reach(&self, reach: Reach) {
        self.https_reach.set(reach);
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

    pub(crate) fn show_mode(&self) -> Show {
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
        window.set_extension_actions(&self.extension_actions(), &self.extension_toolbar());
        window.show_update(self.updates.banner().as_ref());
        window.show_downloads(self.downloads_indicator());
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

    /// The last window is about to close: its tabs are the session to restore next time. A
    /// window closing because its last tab closed has none left; [`Browser::tab_closing`] saved
    /// the session while it still had that tab.
    pub fn window_closing(&self, window: &BrowserWindow) {
        let last = self
            .windows
            .borrow()
            .iter()
            .all(|w| std::ptr::eq(w.as_ref(), window));
        if last && !self.session_final.get() {
            if !window.tabs_in_order().is_empty() {
                self.save_session_now();
            }
            self.session_final.set(true);
        }
        if last {
            self.start_exit_clearing();
        }
    }

    /// A tab is about to close, leaving `tabs_left` in its window.
    pub(crate) fn tab_closing(&self, tabs_left: usize) {
        let windows = self.windows.borrow().len();
        if save_before_closing_tab(tabs_left, windows, self.session_final.get()) {
            self.save_session_now();
        }
        if tabs_left == 0 && windows <= 1 {
            self.start_exit_clearing();
        }
    }

    /// Starts deleting the data of the sites set to Clear on exit or Block, once, before the
    /// last tabs close (see `cookies`).
    fn start_exit_clearing(&self) {
        if self.exit_clearing.borrow().is_none() {
            let clearing = ExitClearing::start(self);
            *self.exit_clearing.borrow_mut() = Some(clearing);
        }
    }

    pub fn window_closed(&self, window: &BrowserWindow) {
        let owned: Vec<_> = self
            .dialog_windows
            .borrow()
            .iter()
            .filter(|d| d.opened_from(window))
            .cloned()
            .collect();
        for dialog in owned {
            dialog.close();
        }
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
            match self.exit_clearing.take().filter(|c| !c.is_empty()) {
                Some(clearing) => exec::spawn(async move {
                    clearing.finish().await;
                    app::exit(0);
                }),
                None => app::exit(0),
            }
        } else {
            self.session_changed();
        }
    }

    /// Shows `kind` in its own window, opened from `opener`, or brings it forward if it is open.
    pub fn show_dialog_window(&self, kind: Dialog, opener: &Rc<BrowserWindow>) {
        let open = self
            .dialog_windows
            .borrow()
            .iter()
            .find(|d| d.kind() == kind)
            .cloned();
        if let Some(open) = open {
            open.raise();
            return;
        }
        let Some(browser) = self.me.upgrade() else {
            return;
        };
        match DialogWindow::open(&browser, opener, kind) {
            Ok(window) => self.dialog_windows.borrow_mut().push(window),
            Err(e) => log::error!("{kind:?} window: {e}"),
        }
    }

    pub fn dialog_window_closed(&self, window: &DialogWindow) {
        let closed: Vec<_> = self
            .dialog_windows
            .borrow_mut()
            .extract_if(.., |w| std::ptr::eq(w.as_ref(), window))
            .collect();
        drop(closed);
    }

    pub fn remember_closed(&self, tab: ClosedTab) {
        if !omnibox::has_link(&tab.url) {
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

    pub fn can_reopen_closed_tab(&self) -> bool {
        !self.closed_tabs.borrow().is_empty()
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
        let homepage = homepage_url(&homepage).map(String::from);
        let urls = self
            .config
            .start_urls
            .iter()
            .filter_map(|u| self.resolve_input(u))
            .collect();
        session::startup_plan(startup, restored, homepage, urls)
    }

    /// What the Home button opens: the home page, or `None` for the new tab page.
    pub fn home_page(&self) -> Option<String> {
        let homepage = self.core(|p| p.prefs().get(&keys::HOMEPAGE));
        homepage_url(&homepage).map(String::from)
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
                            t.is_pinned(),
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
            crate::window::starred(url.as_str(), |_| p.bookmarks().is_bookmarked(&url))
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

    /// The star button on a page that is not bookmarked: a bookmark of it at the end of the
    /// bookmarks bar.
    pub fn bookmark_page(&self, url: &str, title: &str) -> Option<BookmarkId> {
        let url = Url::parse(url).ok()?;
        let added = self.core(|p| {
            p.bookmarks()
                .add_url(BookmarkId::TOOLBAR, InsertAt::End, title, &url)
        });
        self.bookmarks_changed();
        added
            .inspect_err(|e| log::warn!("bookmark {url}: {e}"))
            .ok()
    }

    /// The first bookmark of `url`, which the star edits.
    pub fn bookmark_of(&self, url: &str) -> Option<BookmarkId> {
        let url = Url::parse(url).ok()?;
        self.core(|p| p.bookmarks().find_by_url(&url).first().map(|n| n.id))
    }

    pub fn bookmark(&self, id: BookmarkId) -> Option<BookmarkNode> {
        self.core(|p| p.bookmarks().get(id))
    }

    /// The folders a bookmark can go to (the bookmarks bar, Other bookmarks, and Mobile
    /// bookmarks when it has any), indented by depth, without `exclude` and its subfolders.
    pub fn bookmark_folders(&self, exclude: Option<BookmarkId>) -> Vec<FolderChoice> {
        self.core(|p| {
            let bookmarks = p.bookmarks();
            let children = |id| bookmarks.children(id);
            let roots: Vec<BookmarkNode> =
                [BookmarkId::TOOLBAR, BookmarkId::OTHER, BookmarkId::MOBILE]
                    .into_iter()
                    .filter(|&id| id != BookmarkId::MOBILE || !children(id).is_empty())
                    .filter_map(|id| bookmarks.get(id))
                    .collect();
            bookmark_editor::folder_choices(&roots, &children, exclude)
        })
    }

    /// Saves what the bookmark editor holds. Returns the bookmark it saved.
    pub fn save_bookmark(&self, edit: &Edit) -> Result<BookmarkId, vsesvit_core::Error> {
        let result = self.core(|p| {
            let mut bookmarks = p.bookmarks();
            match &edit.target {
                Target::Added(id) | Target::Existing(id) => {
                    bookmarks.rename(*id, &edit.name)?;
                    if let Some(url) = &edit.url {
                        bookmarks.set_url(*id, url)?;
                    }
                    if bookmarks.get(*id).is_some_and(|n| n.parent != edit.folder) {
                        bookmarks.move_to(*id, edit.folder, InsertAt::End)?;
                    }
                    Ok(*id)
                }
                Target::NewPage { .. } => match &edit.url {
                    Some(url) => bookmarks.add_url(edit.folder, InsertAt::End, &edit.name, url),
                    None => bookmarks.add_folder(edit.folder, InsertAt::End, &edit.name),
                },
                Target::NewFolder => bookmarks.add_folder(edit.folder, InsertAt::End, &edit.name),
            }
        });
        self.bookmarks_changed();
        result
    }

    /// Deletes a bookmark, or a folder with everything in it.
    pub fn remove_bookmark(&self, id: BookmarkId) {
        if let Err(e) = self.core(|p| p.bookmarks().remove(id)) {
            log::warn!("remove bookmark: {e}");
        }
        self.bookmarks_changed();
    }

    /// Refreshes everything that shows bookmarks: the bars and each tab's star. New bookmarks
    /// get their icons fetched; scripted runs start that themselves.
    pub fn bookmarks_changed(&self) {
        if self.config.mode.is_interactive() {
            self.preload_favicons();
        }
        let items = self.bookmarks_bar_items();
        for window in self.windows() {
            window.set_bookmarks_bar(&items);
            window.refresh_starred(&|url| self.is_bookmarked(url));
        }
    }

    /// The bookmarks bar's contents, in display order, with each link's saved favicon.
    pub fn bookmarks_bar_items(&self) -> Vec<BarItem> {
        self.core(|p| {
            let mut items = {
                let bookmarks = p.bookmarks();
                bookmarks_bar::items_from(BookmarkId::TOOLBAR, &|folder| bookmarks.children(folder))
            };
            bookmarks_bar::fill_icons(&mut items, &mut |url| {
                Url::parse(url)
                    .ok()
                    .and_then(|url| p.favicons().get(&url).ok().flatten())
            });
            items
        })
    }

    /// Keeps a page's favicon when the page or its site is bookmarked, and shows it.
    pub fn record_favicon(&self, url: &str, png: &[u8]) {
        let Ok(url) = Url::parse(url) else { return };
        match self.core(|p| p.favicons().record(&url, png)) {
            Ok(true) => self.bookmarks_changed(),
            Ok(false) => {}
            Err(e) => log::warn!("favicon of {url}: {e}"),
        }
    }

    /// Fetches the icons of bookmarked pages that have none, on a worker thread, a batch at a
    /// time until core has no page left to try, and shows what arrived.
    pub fn preload_favicons(&self) {
        match self.favicon_preload.get() {
            Preload::Idle => {}
            Preload::Running | Preload::Requested => {
                self.favicon_preload.set(Preload::Requested);
                return;
            }
        }
        self.favicon_preload.set(Preload::Running);
        let me = self.me.clone();
        exec::spawn(async move {
            while let Some(browser) = me.upgrade() {
                let pages = browser
                    .core(|p| p.favicons().missing(FAVICON_BATCH))
                    .unwrap_or_else(|e| {
                        log::warn!("pages without an icon: {e}");
                        Vec::new()
                    });
                if pages.is_empty() {
                    if browser.favicon_preload.replace(Preload::Running) == Preload::Requested {
                        continue;
                    }
                    browser.favicon_preload.set(Preload::Idle);
                    break;
                }
                log::info!("fetching the icons of {} bookmarked page(s)", pages.len());
                drop(browser);
                let fetched = exec::background(move || FaviconFetch::new(pages).run()).await;
                let Some(browser) = me.upgrade() else { break };
                // The same pages would come back and fail the same way, so the preload stops.
                let Ok(fetched) = fetched.inspect_err(|e| log::warn!("fetching icons: {e}")) else {
                    browser.favicon_preload.set(Preload::Idle);
                    break;
                };
                match browser.core(|p| p.favicons().commit_fetched(fetched)) {
                    Ok(true) => browser.favicons_arrived(),
                    Ok(false) => {}
                    Err(e) => log::warn!("storing fetched icons: {e}"),
                }
            }
        });
    }

    /// Whether icons are being fetched.
    pub fn preloading_favicons(&self) -> bool {
        self.favicon_preload.get() != Preload::Idle
    }

    /// Calls `listener` after fetched icons were stored, while the caller keeps it.
    pub fn on_favicons_arrived(&self, listener: &Rc<dyn Fn()>) {
        self.favicon_listeners
            .borrow_mut()
            .push(Rc::downgrade(listener));
    }

    fn favicons_arrived(&self) {
        let items = self.bookmarks_bar_items();
        for window in self.windows() {
            window.set_bookmarks_bar(&items);
        }
        for listener in crate::sync::live(&self.favicon_listeners) {
            listener();
        }
    }

    /// Moves a bookmark into `parent`, just before `before` (a child of `parent`), or to the
    /// end when `before` is `None`.
    pub fn move_bookmark_before(
        &self,
        id: BookmarkId,
        parent: BookmarkId,
        before: Option<BookmarkId>,
    ) -> Result<(), vsesvit_core::Error> {
        self.core(|p| {
            let mut bookmarks = p.bookmarks();
            let at = before
                .and_then(|before| {
                    bookmarks
                        .children(parent)
                        .iter()
                        .filter(|n| n.id != id)
                        .position(|n| n.id == before)
                })
                .map_or(InsertAt::End, InsertAt::Index);
            bookmarks.move_to(id, parent, at)
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

    /// Suggestions for the address box, with an inline completion when `allow_inline`.
    pub fn suggest(&self, text: &str, allow_inline: bool) -> Suggestions {
        self.core(|p| p.omnibox().suggest(text, SUGGESTIONS, allow_inline))
            .unwrap_or_else(|e| {
                log::warn!("omnibox suggestions: {e}");
                Suggestions::default()
            })
    }

    /// The default engine's suggestions for `text` to fetch, the newest of `queries`, when they
    /// may be asked for.
    pub fn suggest_request(&self, text: &str, queries: &Queries) -> Option<SuggestRequest> {
        // No window is private yet.
        self.core(|p| p.omnibox().suggest_request(text, queries, false))
            .unwrap_or_else(|e| {
                log::warn!("search suggestions: {e}");
                None
            })
    }

    /// The page context menu's item for selected `text`.
    pub fn selection_action(&self, text: &str) -> Option<SelectionAction> {
        self.core(|p| p.omnibox().for_selection(text))
            .unwrap_or_else(|e| {
                log::warn!("context menu search: {e}");
                None
            })
    }

    /// Shift+Delete on a history suggestion.
    pub fn forget_visit(&self, url: &Url) {
        if let Err(e) = self.core(|p| p.history().delete_url(url)) {
            log::warn!("deleting {url} from history: {e}");
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

    /// Saves the width the user dragged a window's tab list to, and gives it to every window.
    pub fn set_tab_pane_width(&self, width: u32) {
        self.write_pref(&TAB_PANE_WIDTH, &width);
        self.update_prefs(|p| p.pane_width = width);
        for window in self.windows() {
            window.set_pane_width(width);
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

    pub fn home_button_visible(&self) -> bool {
        self.prefs.get().home_button
    }

    pub fn set_home_button_visible(&self, visible: bool) {
        self.write_pref(&keys::SHOW_HOME_BUTTON, &visible);
        self.update_prefs(|p| p.home_button = visible);
        for window in self.windows() {
            window.set_home_button_visible(visible);
        }
    }

    pub fn media_player_visible(&self) -> bool {
        self.prefs.get().media_player
    }

    pub fn set_media_player_visible(&self, visible: bool) {
        self.write_pref(&keys::SHOW_MEDIA_PLAYER, &visible);
        self.update_prefs(|p| p.media_player = visible);
        self.apply_media_switches();
    }

    pub fn pip_enabled(&self) -> bool {
        self.prefs.get().pip
    }

    pub fn set_pip_enabled(&self, enabled: bool) {
        self.write_pref(&keys::PICTURE_IN_PICTURE, &enabled);
        self.update_prefs(|p| p.pip = enabled);
        self.apply_media_switches();
    }

    fn apply_media_switches(&self) {
        let prefs = self.prefs.get();
        for window in self.windows() {
            window.set_media_switches(prefs.media_player, prefs.pip);
        }
    }

    /// Read on each request: pages open windows rarely.
    pub fn blocks_popups(&self) -> bool {
        self.core(|p| p.prefs().get(&keys::BLOCK_POPUPS))
    }

    pub fn autofill_forms(&self) -> bool {
        self.core(|p| p.prefs().get(&keys::AUTOFILL_FORMS))
    }

    /// Gives every open tab the form autofill preference; a new tab reads it as it starts.
    pub fn apply_autofill(&self) {
        let forms = self.autofill_forms();
        for window in self.windows() {
            for tab in window.tabs_in_order() {
                tab.set_autofill(forms);
            }
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
        for window in self.dialog_windows.borrow().clone() {
            window.apply_theme(theme);
        }
    }

    pub fn backdrop(&self) -> Backdrop {
        self.prefs.get().backdrop
    }

    pub fn compact_address(&self) -> bool {
        self.prefs.get().compact_address
    }

    pub fn set_compact_address(&self, compact: bool) {
        self.write_pref(&keys::COMPACT_ADDRESS_BAR, &compact);
        self.update_prefs(|p| p.compact_address = compact);
        for window in self.windows() {
            window.set_compact_address(compact);
        }
    }

    pub fn full_urls(&self) -> bool {
        self.prefs.get().full_urls
    }

    pub fn set_full_urls(&self, full: bool) {
        self.write_pref(&keys::SHOW_FULL_URLS, &full);
        self.update_prefs(|p| p.full_urls = full);
        for window in self.windows() {
            window.set_full_urls(full);
        }
    }

    pub fn set_backdrop(&self, backdrop: Backdrop) {
        self.write_pref(&WINDOW_BACKDROP, &backdrop);
        self.update_prefs(|p| p.backdrop = backdrop);
        for window in self.windows() {
            window.apply_backdrop(backdrop);
        }
        for window in self.dialog_windows.borrow().clone() {
            window.apply_backdrop(backdrop);
        }
    }

    fn update_prefs(&self, f: impl FnOnce(&mut WindowPrefs)) {
        let mut prefs = self.prefs.get();
        f(&mut prefs);
        self.prefs.set(prefs);
    }

    /// Applies `edit` to the keyboard shortcuts, stores them and applies them in every window.
    pub fn edit_keymap(&self, edit: impl FnOnce(&mut Keymap)) {
        let mut keymap = self.core(|p| p.prefs().keymap());
        edit(&mut keymap);
        if let Err(e) = self.core(|p| p.prefs().set_keymap(&keymap)) {
            log::warn!("keyboard shortcuts: {e}");
        }
        self.shortcuts_changed();
    }

    /// Applies the stored keyboard shortcuts, the extensions' included, in every window: after
    /// an edit here, a sync (`sync_applied`) that wrote `keyboard.shortcuts`, or a change to the
    /// extensions or to what the engine has loaded (`sync_extensions`).
    pub fn shortcuts_changed(&self) {
        let keymap = self.core(|p| p.prefs().keymap());
        let commands = match self.core(|p| p.extensions().list()) {
            Ok(installed) => extension_commands(&installed),
            Err(e) => {
                log::warn!("extension shortcuts: {e}");
                Vec::new()
            }
        };
        let bindings = Bindings::new(keymap, commands, &self.extension_actions());
        if shortcuts::set_current(bindings) {
            for window in self.windows() {
                window.shortcuts_changed();
            }
        }
    }

    /// What a sync engine calls after `sync().apply`, with the report's `changed` and the site
    /// settings stored before it (`site_permissions().all()`): what shows the changed data
    /// follows, as after the same edit made here. Open tabs from other devices show nowhere
    /// yet, and `storage.sync` belongs to WebView2 here.
    pub fn sync_applied(&self, changed: &Changed, site_settings_before: &[SiteSetting]) {
        if changed.bookmarks {
            self.bookmarks_changed();
        }
        for effect in sync::pref_effects(&changed.prefs) {
            match effect {
                PrefEffect::Window => self.window_prefs_synced(),
                PrefEffect::Keymap => self.shortcuts_changed(),
                PrefEffect::Autofill => self.apply_autofill(),
                PrefEffect::ExtensionToolbar => self.show_extension_actions(),
                PrefEffect::Cookies => cookies::changed(self),
            }
        }
        if let Some(me) = self.me.upgrade() {
            if changed.extensions {
                me.reconcile_extensions();
            }
            if changed.site_permissions {
                crate::permissions::settings_changed(&me);
                crate::permissions::sync_changed(&me, site_settings_before);
            }
        }
        self.sync.applied(changed);
    }

    /// Reads the synced preferences `WindowPrefs` holds again, and shows what changed in every
    /// window.
    fn window_prefs_synced(&self) {
        let old = self.prefs.get();
        let new = self.core(|p| WindowPrefs {
            tabs: p.prefs().get(&keys::TABS_POSITION),
            theme: p.prefs().get(&keys::THEME),
            bookmarks_bar: p.prefs().get(&keys::SHOW_BOOKMARKS_BAR),
            home_button: p.prefs().get(&keys::SHOW_HOME_BUTTON),
            compact_address: p.prefs().get(&keys::COMPACT_ADDRESS_BAR),
            full_urls: p.prefs().get(&keys::SHOW_FULL_URLS),
            media_player: p.prefs().get(&keys::SHOW_MEDIA_PLAYER),
            pip: p.prefs().get(&keys::PICTURE_IN_PICTURE),
            ..old
        });
        self.prefs.set(new);
        for window in self.windows() {
            if new.tabs != old.tabs {
                window.set_tabs_position(new.tabs);
            }
            if new.theme != old.theme {
                window.apply_theme(new.theme);
            }
            if new.bookmarks_bar != old.bookmarks_bar {
                window.set_bookmarks_bar_visible(new.bookmarks_bar);
            }
            if new.home_button != old.home_button {
                window.set_home_button_visible(new.home_button);
            }
            if new.compact_address != old.compact_address {
                window.set_compact_address(new.compact_address);
            }
            if new.full_urls != old.full_urls {
                window.set_full_urls(new.full_urls);
            }
            if (new.media_player, new.pip) != (old.media_player, old.pip) {
                window.set_media_switches(new.media_player, new.pip);
            }
        }
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

    /// Shows the update state in every window's update bar and in an open Settings.
    pub fn update_state_changed(&self) {
        let banner = self.updates.banner();
        for window in self.windows() {
            window.show_update(banner.as_ref());
        }
        self.updates.changed();
    }

    /// The update bar's button.
    pub fn update_action(&self, action: Action) {
        match action {
            Action::Restart => updates::restart(self),
            Action::Retry => self.check_for_updates(),
        }
    }

    /// A check the user asked for, whose progress and failure every window shows.
    pub fn check_for_updates(&self) {
        exec::spawn(updates::check(self.me.clone(), Trigger::User));
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

    /// Which releases this installation updates to (`updates.channel`, a device-local
    /// preference), read at each check.
    pub fn updates_channel(&self) -> UpdateChannel {
        self.core(|p| p.prefs().get(&keys::UPDATES_CHANNEL))
    }

    /// Drops what the old channel found, a ready update included, and checks the new one.
    pub fn set_updates_channel(&self, channel: UpdateChannel) {
        self.write_pref(&keys::UPDATES_CHANNEL, &channel);
        updates::switch_channel(self);
    }

    // ---- sync ----

    pub fn sync(&self) -> &SyncController {
        &self.sync
    }

    // ---- extensions ----

    pub fn extension_actions(&self) -> Vec<ExtensionAction> {
        self.extensions.actions()
    }

    /// Which actions the toolbar shows, and in what order (the synced `toolbar` preference).
    pub fn extension_toolbar(&self) -> Layout {
        let available = self.toolbar_ids();
        let saved = self.core(|p| p.prefs().get(&toolbar::TOOLBAR));
        toolbar::layout(&available, &saved)
    }

    fn toolbar_ids(&self) -> Vec<String> {
        self.extension_actions().into_iter().map(|a| a.id).collect()
    }

    pub fn set_extension_pinned(&self, id: &str, pinned: bool) {
        self.change_toolbar(|available, saved| toolbar::set_pinned(available, saved, id, pinned));
    }

    /// Moves the pinned action `id` to place `to` among the pinned ones.
    pub fn move_extension(&self, id: &str, to: usize) {
        self.change_toolbar(|available, saved| toolbar::move_pinned(available, saved, id, to));
    }

    fn change_toolbar(
        &self,
        change: impl FnOnce(&[String], &[toolbar::Entry]) -> Vec<toolbar::Entry>,
    ) {
        let available = self.toolbar_ids();
        let saved = self.core(|p| p.prefs().get(&toolbar::TOOLBAR));
        self.write_pref(&toolbar::TOOLBAR, &change(&available, &saved));
        self.show_extension_actions();
    }

    /// Shows the extension actions in every window's toolbar.
    pub fn show_extension_actions(&self) {
        let (actions, layout) = (self.extension_actions(), self.extension_toolbar());
        for window in self.windows() {
            window.set_extension_actions(&actions, &layout);
        }
    }

    /// The WebView2 profile, reached through any tab's engine view.
    pub async fn engine_profile(&self) -> Option<crate::bindings::CoreWebView2Profile> {
        let window = self.windows.borrow().first().cloned()?;
        window.engine_profile().await
    }

    /// Deletes the passwords WebView2 saved before Vsesvit turned password saving off, and
    /// nothing else. Until it succeeds, every start tries again.
    async fn purge_saved_passwords(self: Rc<Self>) {
        let Some(profile) = self.engine_profile().await else {
            log::warn!("saved passwords: the web engine is not ready; trying next start");
            return;
        };
        let purged: windows_core::Result<()> = async {
            profile
                .cast::<ICoreWebView2Profile2>()?
                .ClearBrowsingDataAsync(CoreWebView2BrowsingDataKinds::PasswordAutosave)?
                .await
        }
        .await;
        match purged {
            Ok(()) => {
                log::info!("saved passwords: deleted what WebView2 saved");
                self.write_pref(&PASSWORDS_PURGED, &true);
            }
            Err(e) => log::warn!("saved passwords: {e}; trying next start"),
        }
    }
}

/// Whether closing a tab saves the session first: closing the last tab of the last window closes
/// the browser, and the session to restore next time is the one with that tab in it.
fn save_before_closing_tab(
    tabs_left_in_window: usize,
    windows: usize,
    session_final: bool,
) -> bool {
    tabs_left_in_window == 0 && windows <= 1 && !session_final
}

/// Saves the session, sends what changed since the last sync (`sync::final_sync`), and drops the
/// browser (and with it every window and web view) before XAML shuts down.
pub(crate) fn shutdown() {
    let browser = BROWSER.with_borrow_mut(Option::take);
    if let Some(browser) = &browser
        && !browser.session_final.get()
        && !browser.windows.borrow().is_empty()
    {
        browser.save_session_now();
    }
    if let Some(browser) = &browser {
        sync::final_sync(browser);
    }
    drop(browser);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_last_tab_of_the_last_window_saves_before_it_closes() {
        assert!(save_before_closing_tab(0, 1, false));
        assert!(!save_before_closing_tab(1, 1, false), "other tabs remain");
        assert!(
            !save_before_closing_tab(0, 2, false),
            "other windows remain"
        );
        assert!(
            !save_before_closing_tab(0, 1, true),
            "the session is already final"
        );
    }
}
