//! The app-wide controller: the engine, the open windows, recently closed tabs, and the points
//! where vsesvit-core plugs in.
//!
//! Everything here runs on the UI thread. The "Integration points" block below is the complete
//! list of places the next step connects to `vsesvit_core::Profile`; until then each one does
//! only what the shell can do honestly on its own and says so in the log.

use std::cell::{Cell, RefCell};
use std::hash::{BuildHasher, RandomState};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use crate::bindings::*;
use crate::bookmarks_bar::BarItem;
use crate::config::{Config, Mode};
use crate::engine::{self, Engine};
use crate::popup::ExtensionAction;
use crate::updates::{self, Action, Trigger, Updates};
use crate::window::{BrowserWindow, Show};
use crate::{app, automation, exec, omnibox, platform, shortcuts};

/// Recently closed tabs kept for Ctrl+Shift+T.
const CLOSED_TABS_KEPT: usize = 25;

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

pub(crate) struct Browser {
    config: Config,
    engine: Engine,
    page_nonce: String,
    page_script: Rc<str>,
    windows: RefCell<Vec<Rc<BrowserWindow>>>,
    closed_tabs: RefCell<Vec<ClosedTab>>,
    next_tab_id: Cell<u64>,
    bookmarks_bar_visible: Cell<bool>,
    extension_actions: RefCell<Vec<ExtensionAction>>,
    updates: Updates,
    updates_automatic: Cell<bool>,
    me: Weak<Browser>,
}

thread_local! {
    static BROWSER: RefCell<Option<Rc<Browser>>> = const { RefCell::new(None) };
}

/// Starts the browser from `OnLaunched`. Failures end the process with a message.
pub(crate) fn launch(config: Config) {
    exec::spawn(async move {
        let interactive = config.mode.is_interactive();
        if let Err(e) = start(config).await {
            log::error!("startup failed: {e}");
            if interactive {
                platform::message_box(
                    &format!(
                        "Vsesvit could not start the web engine.\n\n{e}\n\n\
                         Vsesvit needs the Microsoft Edge WebView2 Runtime."
                    ),
                    MB_OK | MB_ICONERROR,
                );
            }
            app::exit(1);
        }
    });
}

async fn start(config: Config) -> windows_core::Result<()> {
    let engine = Engine::create(&config.engine_dir()).await?;
    let page_nonce = nonce();
    let page_script: Rc<str> = shortcuts::page_script(&page_nonce).into();
    let updates = if config.mode.is_interactive() {
        Updates::detect()
    } else {
        Updates::disabled("scripted runs do not update".into())
    };
    let browser = Rc::new_cyclic(|me| Browser {
        config,
        engine,
        page_nonce,
        page_script,
        windows: RefCell::new(Vec::new()),
        closed_tabs: RefCell::new(Vec::new()),
        next_tab_id: Cell::new(1),
        bookmarks_bar_visible: Cell::new(true),
        extension_actions: RefCell::new(Vec::new()),
        updates,
        updates_automatic: Cell::new((vsesvit_core::prefs::keys::UPDATES_AUTOMATIC.default)()),
        me: me.clone(),
    });
    BROWSER.with_borrow_mut(|b| *b = Some(browser.clone()));

    let show = if browser.config.mode.is_interactive() {
        Show::Activate
    } else {
        Show::NoActivate
    };
    let window = browser.open_window(&browser.config.start_urls.clone(), show)?;
    if !browser.config.load_extensions.is_empty() {
        exec::spawn(browser.clone().load_session_extensions(window));
    }
    if let Mode::UiSmoke { out_dir } = &browser.config.mode {
        exec::spawn(automation::ui_smoke(browser.clone(), out_dir.clone()));
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

impl Browser {
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

    pub fn next_tab_id(&self) -> u64 {
        self.next_tab_id.replace(self.next_tab_id.get() + 1)
    }

    pub fn windows(&self) -> Vec<Rc<BrowserWindow>> {
        self.windows.borrow().clone()
    }

    /// Opens a window with a tab per URL, or one blank tab.
    pub fn open_window(
        &self,
        urls: &[String],
        show: Show,
    ) -> windows_core::Result<Rc<BrowserWindow>> {
        let browser = self.me.upgrade().ok_or_else(windows_core::Error::empty)?;
        let window = BrowserWindow::create(&browser, show)?;
        self.windows.borrow_mut().push(window.clone());
        window.set_bookmarks_bar_visible(self.bookmarks_bar_visible.get());
        window.set_bookmarks_bar(&self.bookmarks_bar_items());
        window.set_extension_actions(&self.extension_actions());
        window.show_update(self.updates.banner().as_ref());
        window.open_start_tabs(urls)?;
        Ok(window)
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
            self.save_session();
            updates::install_on_exit(self);
            app::exit(0);
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

    pub fn bookmarks_bar_visible(&self) -> bool {
        self.bookmarks_bar_visible.get()
    }

    pub fn set_bookmarks_bar_visible(&self, visible: bool) {
        self.bookmarks_bar_visible.set(visible);
        for window in self.windows() {
            window.set_bookmarks_bar_visible(visible);
        }
    }

    /// `--load-extension`: loads each folder into the engine once a web view exists (the
    /// WebView2 profile is reached through one) and shows its action button.
    async fn load_session_extensions(self: Rc<Self>, window: Rc<BrowserWindow>) {
        let Some(profile) = window.engine_profile().await else {
            log::error!("--load-extension: no engine profile");
            return;
        };
        for dir in self.config.load_extensions.clone() {
            match engine::add_extension(&profile, &dir).await {
                Ok(added) => {
                    log::info!(
                        "loaded extension {} ({}) from {}",
                        added.name,
                        added.id,
                        dir.display()
                    );
                    match ExtensionAction::from_unpacked(&added.id, &dir) {
                        Ok(action) => self.extension_actions.borrow_mut().push(action),
                        Err(e) => log::warn!("extension {}: {e}", added.id),
                    }
                }
                Err(e) => log::error!("--load-extension {}: {e}", dir.display()),
            }
        }
        let actions = self.extension_actions.borrow().clone();
        for window in self.windows() {
            window.set_extension_actions(&actions);
        }
    }

    pub fn extension_actions(&self) -> Vec<ExtensionAction> {
        self.extension_actions.borrow().clone()
    }

    pub fn updates(&self) -> &Updates {
        &self.updates
    }

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

    // ---- Integration points for vsesvit-core ----

    /// Every committed main-frame navigation. Returns whether the URL is bookmarked.
    /// Core: `history().record_visit(&url, Transition::Link)`, then `bookmarks().is_bookmarked`.
    pub fn navigation_committed(&self, url: &str, kind: CommitKind) -> bool {
        log::debug!("committed ({kind:?}) {url}");
        self.session_changed();
        false
    }

    /// Core: `history().set_title(&url, title)`.
    pub fn title_changed(&self, url: &str, title: &str) {
        log::debug!("title of {url}: {title}");
    }

    /// The star button and Ctrl+D. Returns the new bookmarked state.
    /// Core: remove the existing bookmark or `add_url(...)`, then refresh the bookmarks bar.
    pub fn star_clicked(&self, url: &str, title: &str) -> bool {
        log::info!(
            "bookmarking {url} ({title}) needs the profile store, which is not connected yet"
        );
        false
    }

    /// The bookmarks bar's contents, in display order.
    /// Core: `bookmarks().children(BookmarkId::TOOLBAR)` mapped to `BarItem`s.
    pub fn bookmarks_bar_items(&self) -> Vec<BarItem> {
        Vec::new()
    }

    /// Suggestions for the address box. Core: `omnibox().suggest(text, 8)`.
    pub fn omnibox_text_changed(&self, text: &str) -> Vec<String> {
        log::trace!("omnibox input {text:?}");
        Vec::new()
    }

    /// What Enter in the address box loads. Core: `omnibox().resolve(text)`.
    pub fn omnibox_submitted(&self, text: &str) -> Option<String> {
        omnibox::navigation_target(text)
    }

    /// The Extensions dialog's install box (store URL, extension id, `.crx`, or folder).
    /// Core: `extensions().prepare_install(InstallSource::parse(source)?)`, `job.run()` on a
    /// worker thread, `commit` on the UI thread, then load the committed folder into the engine.
    pub fn install_extension(&self, source: &str) -> Result<(), String> {
        log::info!("install requested for {source:?}");
        Err("Installing extensions needs the profile store, which is not connected yet.".into())
    }

    /// Tabs were opened, closed, moved or navigated. Core: debounced `session().save(..)`.
    pub fn session_changed(&self) {
        log::trace!("session changed: {} window(s)", self.windows.borrow().len());
    }

    /// Before the browser exits normally or restarts to update.
    /// Core: flush the debounced `session().save(..)` now.
    pub fn save_session(&self) {
        log::debug!(
            "saving the session: {} window(s)",
            self.windows.borrow().len()
        );
    }

    /// Core: `prefs().get(keys::UPDATES_AUTOMATIC)`.
    pub fn updates_automatic(&self) -> bool {
        self.updates_automatic.get()
    }

    /// Core: `prefs().set(keys::UPDATES_AUTOMATIC, on)`.
    pub fn set_updates_automatic(&self, on: bool) {
        self.updates_automatic.set(on);
        if on {
            exec::spawn(updates::check(self.me.clone(), Trigger::Scheduled));
        }
    }

    pub fn profile_dir(&self) -> PathBuf {
        self.config.profile_dir.clone()
    }
}

/// Drops the browser (and with it every window and web view) before XAML shuts down.
pub(crate) fn shutdown() {
    let browser = BROWSER.with_borrow_mut(Option::take);
    drop(browser);
}
