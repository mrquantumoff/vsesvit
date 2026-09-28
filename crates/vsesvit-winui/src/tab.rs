//! One tab: its `WebView2` in the window's page grid and the state the tab lists show.
//!
//! The web view is never a tab list item's content (a WebView2 inside `TabViewItem` content
//! measures 0 high); the window shows the selected tab's view and collapses the others.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;
use vsesvit_core::history::Transition;
use vsesvit_core::{new_tab, session};
use windows_core::{IInspectable, Interface, Ref, Result};

use crate::bindings::*;
use crate::browser::CommitKind;
use crate::shortcuts::{self, PageMessage, PageScript};
use crate::store;
use crate::tab_header::TabLook;
use crate::window::BrowserWindow;
use crate::{exec, xaml, zoom};

/// Identifies a tab within this process.
pub(crate) type TabId = u64;

/// What a new tab loads first.
pub(crate) enum Initial {
    Url(String),
    Blank,
    /// The page asked for a new window (`window.open`, target=_blank, Ctrl+click): the tab's
    /// engine view becomes that window, so `window.opener` keeps working.
    Opener(NewWindowRequest),
}

pub(crate) struct NewWindowRequest {
    args: CoreWebView2NewWindowRequestedEventArgs,
    deferral: Deferral,
}

impl NewWindowRequest {
    fn fulfil(self, core: &CoreWebView2) {
        let set = self
            .args
            .SetNewWindow(core)
            .and_then(|()| self.args.SetHandled(true));
        if let Err(e) = set {
            log::warn!("new window request: {e}");
        }
        let _ = self.deferral.Complete();
    }

    fn cancel(self) {
        let _ = self.args.SetHandled(true);
        let _ = self.deferral.Complete();
    }
}

/// The engine's autofill preferences, applied to each tab's engine view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Autofill {
    pub passwords: bool,
    pub forms: bool,
}

impl Autofill {
    fn apply(self, settings: &CoreWebView2Settings) -> Result<()> {
        let settings = settings.cast::<ICoreWebView2Settings4>()?;
        settings.SetIsPasswordAutosaveEnabled(self.passwords)?;
        settings.SetIsGeneralAutofillEnabled(self.forms)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TabState {
    /// The committed URL; empty until the first commit.
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub starred: bool,
    pub fullscreen: bool,
    pub zoom: zoom::Level,
}

pub(crate) struct Tab {
    pub id: TabId,
    /// The tab's identity in saved sessions; restored tabs keep theirs.
    pub session_id: session::TabId,
    window: Weak<BrowserWindow>,
    view: WebView2,
    core: OnceCell<CoreWebView2>,
    state: RefCell<TabState>,
    favicon: RefCell<Option<ImageSource>>,
    /// The favicon as the page gave it (PNG), which bookmarks keep.
    favicon_png: RefCell<Option<Vec<u8>>>,
    background_link: RefCell<Option<(String, Instant)>>,
    /// The URI of the last main-frame navigation request; WebView2 reports an empty `Source`
    /// for some documents (data: URLs), and then this is what was committed.
    requested: RefCell<String>,
    /// How the navigation the shell started came about, for history. Page-initiated
    /// navigations have none and count as links.
    transition: Cell<Option<Transition>>,
    last_active_ms: Cell<i64>,
    favicon_generation: Cell<u64>,
    closed: Cell<bool>,
}

impl Tab {
    pub fn new(
        id: TabId,
        session_id: Option<session::TabId>,
        window: Weak<BrowserWindow>,
    ) -> Result<Rc<Self>> {
        Ok(Rc::new(Self {
            id,
            session_id: session_id.unwrap_or_default(),
            window,
            view: WebView2::new()?,
            core: OnceCell::new(),
            state: RefCell::new(TabState {
                title: "New tab".into(),
                ..TabState::default()
            }),
            favicon: RefCell::new(None),
            favicon_png: RefCell::new(None),
            background_link: RefCell::new(None),
            requested: RefCell::new(String::new()),
            transition: Cell::new(None),
            last_active_ms: Cell::new(now_ms()),
            favicon_generation: Cell::new(0),
            closed: Cell::new(false),
        }))
    }

    /// What the tab lists show.
    pub fn look(&self) -> TabLook {
        let state = self.state.borrow();
        TabLook {
            title: state.title.clone(),
            favicon: self.favicon.borrow().clone(),
            loading: state.loading,
        }
    }

    pub fn last_active_ms(&self) -> i64 {
        self.last_active_ms.get()
    }

    pub fn mark_active(&self) {
        self.last_active_ms.set(now_ms());
    }

    pub fn view(&self) -> &WebView2 {
        &self.view
    }

    pub fn core(&self) -> Option<&CoreWebView2> {
        self.core.get()
    }

    pub fn state(&self) -> TabState {
        self.state.borrow().clone()
    }

    /// What the tab shows before its engine view commits anything: the URL it is about to load
    /// and, for a restored tab, the title it had. A session saved meanwhile keeps both.
    pub fn set_planned(&self, url: &str, title: &str) {
        *self.requested.borrow_mut() = url.to_owned();
        if !title.is_empty() {
            self.state.borrow_mut().title = title.to_owned();
        }
    }

    /// The committed URL, or the one being loaded before the first commit.
    pub fn session_url(&self) -> String {
        let url = self.state.borrow().url.clone();
        if url.is_empty() {
            self.requested.borrow().clone()
        } else {
            url
        }
    }

    pub fn has_favicon(&self) -> bool {
        self.favicon.borrow().is_some()
    }

    pub fn favicon_png(&self) -> Option<Vec<u8>> {
        self.favicon_png.borrow().clone()
    }

    pub fn is_ready(&self) -> bool {
        self.core.get().is_some()
    }

    /// Creates the engine view in `environment`, then loads `initial`.
    pub async fn start(
        self: Rc<Self>,
        environment: CoreWebView2Environment,
        page_script: Rc<PageScript>,
        initial: Initial,
    ) {
        let core = match self.ensure_core(&environment, &page_script).await {
            Ok(core) => core,
            Err(e) => {
                log::error!("tab {}: WebView2 initialization failed: {e}", self.id);
                if let Initial::Opener(request) = initial {
                    request.cancel();
                }
                return;
            }
        };
        if self.closed.get() {
            if let Initial::Opener(request) = initial {
                request.cancel();
            }
            let _ = self.view.Close();
            return;
        }
        match initial {
            Initial::Url(url) => self.navigate(&url),
            Initial::Blank => self.show_new_tab_page(&core),
            Initial::Opener(request) => request.fulfil(&core),
        }
    }

    async fn ensure_core(
        self: &Rc<Self>,
        environment: &CoreWebView2Environment,
        page_script: &PageScript,
    ) -> Result<CoreWebView2> {
        self.view
            .cast::<IWebView22>()?
            .EnsureCoreWebView2WithEnvironmentAsync(environment)?
            .await?;
        let core = self.view.CoreWebView2()?;
        let settings = core.Settings()?;
        settings.SetAreDevToolsEnabled(true)?;
        settings.SetIsWebMessageEnabled(false)?;
        if let Some(browser) = self.window().and_then(|w| w.browser()) {
            self.apply_autofill(&settings, browser.autofill());
        }
        self.wire(&core)?;
        self.inject(&core, page_script).await?;
        let _ = self.core.set(core.clone());
        Ok(core)
    }

    /// Runs the shortcut script in its isolated world of every new document (see `shortcuts`)
    /// and listens to its binding; runs the store script in the main world (see `store`).
    async fn inject(self: &Rc<Self>, core: &CoreWebView2, script: &PageScript) -> Result<()> {
        core.GetDevToolsProtocolEventReceiver("Runtime.bindingCalled")?
            .DevToolsProtocolEventReceived(on(
                self,
                |tab, args: &CoreWebView2DevToolsProtocolEventReceivedEventArgs| {
                    tab.binding_called(args);
                },
            ))?
            .forget();
        // Scripts for new documents need the Page domain on, and a binding reaches the worlds
        // created later only while the Runtime domain is on.
        let calls = [
            ("Page.enable", "{}".to_owned()),
            ("Runtime.enable", "{}".to_owned()),
            (
                "Runtime.addBinding",
                json!({ "name": shortcuts::BINDING, "executionContextName": script.world })
                    .to_string(),
            ),
            (
                "Page.addScriptToEvaluateOnNewDocument",
                json!({ "source": script.source, "worldName": script.world }).to_string(),
            ),
            (
                "Page.addScriptToEvaluateOnNewDocument",
                json!({ "source": store::MAIN_WORLD_SCRIPT }).to_string(),
            ),
        ];
        for (method, params) in calls {
            core.CallDevToolsProtocolMethodAsync(method, &params)?
                .await?;
        }
        Ok(())
    }

    pub fn navigate(&self, url: &str) {
        let Some(core) = self.core.get() else {
            log::warn!("tab {}: navigate before the engine view is ready", self.id);
            return;
        };
        if let Err(e) = core.Navigate(url) {
            log::warn!("tab {}: navigate to {url}: {e}", self.id);
        }
    }

    /// The new tab page, loaded as HTML at `about:blank` so the tab still reads as blank.
    fn show_new_tab_page(&self, core: &CoreWebView2) {
        let shown = match self.window().and_then(|w| w.browser()) {
            Some(browser) => browser
                .core(new_tab::page)
                .map_err(|e| e.to_string())
                .and_then(|html| core.NavigateToString(&html).map_err(|e| e.to_string())),
            None => Err("the window is gone".to_owned()),
        };
        if let Err(e) = shown {
            log::warn!("tab {}: new tab page: {e}", self.id);
            self.navigate("about:blank");
        }
    }

    pub fn go_to_new_tab_page(&self) {
        if let Some(core) = self.core.get() {
            self.show_new_tab_page(core);
        }
    }

    /// A navigation the user started from the shell: typed, or a bookmark.
    pub fn navigate_as(&self, url: &str, transition: Transition) {
        self.transition.set(Some(transition));
        self.navigate(url);
    }

    pub fn go_back(&self) {
        if let Some(core) = self.core.get() {
            let _ = core.GoBack();
        }
    }

    pub fn go_forward(&self) {
        if let Some(core) = self.core.get() {
            let _ = core.GoForward();
        }
    }

    pub fn reload_or_stop(&self) {
        let Some(core) = self.core.get() else { return };
        let loading = self.state.borrow().loading;
        if loading {
            let _ = core.Stop();
        } else {
            self.reload();
        }
    }

    pub fn reload(&self) {
        if let Some(core) = self.core.get() {
            self.transition.set(Some(Transition::Reload));
            let _ = core.Reload();
        }
    }

    /// Starts a find in the page with WebView2's find bar. Returns the number of matches.
    pub async fn find(&self, options: CoreWebView2FindOptions) -> Result<i32> {
        let core = self.core.get().ok_or_else(windows_core::Error::empty)?;
        let find = core.cast::<ICoreWebView2_28>()?.Find()?;
        find.StartAsync(&options)?.await?;
        find.MatchCount()
    }

    /// Runs `script` in the page; returns its result as JSON.
    pub async fn eval(&self, script: &str) -> Result<String> {
        let core = self.core.get().ok_or_else(windows_core::Error::empty)?;
        Ok(core.ExecuteScriptAsync(script)?.await?.to_string_lossy())
    }

    /// Calls a Chrome DevTools Protocol method on the page; returns the JSON result.
    pub async fn devtools(&self, method: &str, params: &str) -> Result<String> {
        let core = self.core.get().ok_or_else(windows_core::Error::empty)?;
        Ok(core
            .CallDevToolsProtocolMethodAsync(method, params)?
            .await?
            .to_string_lossy())
    }

    pub fn stop_find(&self) {
        if let Some(core) = self.core.get() {
            let _ = core
                .cast::<ICoreWebView2_28>()
                .and_then(|c| c.Find())
                .and_then(|f| f.Stop());
        }
    }

    /// Zooms the page as its keyboard shortcut does. Only for a window in the foreground.
    pub fn zoom(&self, step: zoom::Step) {
        self.focus_page();
        // The page takes the focus a moment after the web view does.
        exec::spawn(async move {
            exec::sleep(Duration::from_millis(50)).await;
            zoom::press(step);
        });
    }

    pub fn focus_page(&self) {
        let _ = self
            .view
            .cast::<UIElement>()
            .and_then(|v| v.Focus(FocusState::Programmatic));
    }

    pub fn set_starred(&self, starred: bool) {
        self.state.borrow_mut().starred = starred;
        self.notify();
    }

    pub fn set_autofill(&self, autofill: Autofill) {
        if let Some(settings) = self.core.get().and_then(|c| c.Settings().ok()) {
            self.apply_autofill(&settings, autofill);
        }
    }

    /// A runtime too old for these settings keeps its defaults; the tab still works.
    fn apply_autofill(&self, settings: &CoreWebView2Settings, autofill: Autofill) {
        if let Err(e) = autofill.apply(settings) {
            log::warn!("tab {}: autofill settings: {e}", self.id);
        }
    }

    /// Releases the engine view. The window removes the XAML parts.
    pub fn close(&self) {
        if !self.closed.replace(true) {
            let _ = self.view.Close();
        }
    }

    fn window(&self) -> Option<Rc<BrowserWindow>> {
        self.window.upgrade()
    }

    fn notify(&self) {
        if let Some(window) = self.window() {
            window.tab_updated(self);
        }
    }

    fn wire(self: &Rc<Self>, core: &CoreWebView2) -> Result<()> {
        core.NavigationStarting(on(
            self,
            |tab, args: &CoreWebView2NavigationStartingEventArgs| {
                *tab.requested.borrow_mut() = args.Uri().unwrap_or_default();
                tab.state.borrow_mut().loading = true;
                tab.notify();
            },
        ))?
        .forget();
        core.ContentLoading(on(self, |tab, _: &CoreWebView2ContentLoadingEventArgs| {
            tab.committed(CommitKind::NewDocument);
        }))?
        .forget();
        core.SourceChanged(on(
            self,
            |tab, args: &CoreWebView2SourceChangedEventArgs| {
                if args.IsNewDocument().unwrap_or(true) {
                    tab.refresh_url();
                } else {
                    tab.committed(CommitKind::SameDocument);
                }
            },
        ))?
        .forget();
        core.HistoryChanged(signal(self, |tab| {
            tab.refresh_history();
            tab.notify();
        }))?
        .forget();
        core.NavigationCompleted(on(
            self,
            |tab, args: &CoreWebView2NavigationCompletedEventArgs| {
                if !args.IsSuccess().unwrap_or(true) {
                    log::info!(
                        "tab {}: navigation failed, web error {}",
                        tab.id,
                        args.WebErrorStatus().map(|s| s.0).unwrap_or(-1)
                    );
                }
                tab.state.borrow_mut().loading = false;
                tab.refresh_history();
                tab.notify();
            },
        ))?
        .forget();
        core.DocumentTitleChanged(signal(self, |tab| tab.title_changed()))?
            .forget();
        core.cast::<ICoreWebView2_15>()?
            .FaviconChanged(signal(self, |tab| {
                exec::spawn(tab.clone().load_favicon());
            }))?
            .forget();
        core.ContainsFullScreenElementChanged(signal(self, |tab| {
            let fullscreen = tab
                .core
                .get()
                .and_then(|c| c.ContainsFullScreenElement().ok())
                .unwrap_or(false);
            tab.state.borrow_mut().fullscreen = fullscreen;
            if let Some(window) = tab.window() {
                window.tab_fullscreen_changed(tab, fullscreen);
            }
        }))?
        .forget();
        core.NewWindowRequested(on(
            self,
            |tab, args: &CoreWebView2NewWindowRequestedEventArgs| {
                tab.new_window_requested(args);
            },
        ))?
        .forget();
        core.WindowCloseRequested(signal(self, |tab| {
            // Closing the web view from inside its own event is deferred to the next turn.
            if let Some(window) = tab.window() {
                let id = tab.id;
                exec::spawn(async move { window.close_tab(id) });
            }
        }))?
        .forget();
        core.cast::<ICoreWebView2_4>()?
            .DownloadStarting(on(
                self,
                |tab, args: &CoreWebView2DownloadStartingEventArgs| {
                    if let Some(window) = tab.window()
                        && let Some(browser) = window.browser()
                    {
                        browser.download_starting(&window, args);
                    }
                },
            ))?
            .forget();
        core.ProcessFailed(on(
            self,
            |tab, args: &CoreWebView2ProcessFailedEventArgs| {
                log::error!(
                    "tab {}: engine process failed, kind {}",
                    tab.id,
                    args.ProcessFailedKind().map(|k| k.0).unwrap_or(-1)
                );
            },
        ))?
        .forget();
        Ok(())
    }

    fn committed(self: &Rc<Self>, kind: CommitKind) {
        self.refresh_url();
        let url = self.state.borrow().url.clone();
        let transition = match kind {
            CommitKind::NewDocument => self.transition.take().unwrap_or(Transition::Link),
            CommitKind::SameDocument => Transition::Link,
        };
        if let Some(browser) = self.window().and_then(|w| w.browser()) {
            let starred = browser.navigation_committed(&url, kind, transition);
            self.state.borrow_mut().starred = starred;
        }
        self.notify();
    }

    fn refresh_url(&self) {
        let Some(core) = self.core.get() else { return };
        match core.Source() {
            Ok(url) => {
                let url = if url.is_empty() {
                    self.requested.borrow().clone()
                } else {
                    url
                };
                log::debug!("tab {}: at {url}", self.id);
                self.state.borrow_mut().url = url;
            }
            Err(e) => log::warn!("tab {}: source: {e}", self.id),
        }
        self.refresh_history();
        self.notify();
    }

    fn refresh_history(&self) {
        let Some(core) = self.core.get() else { return };
        let (back, forward) = (
            core.CanGoBack().unwrap_or(false),
            core.CanGoForward().unwrap_or(false),
        );
        let mut state = self.state.borrow_mut();
        state.can_go_back = back;
        state.can_go_forward = forward;
    }

    fn title_changed(&self) {
        let Some(core) = self.core.get() else { return };
        let url = self.state.borrow().url.clone();
        let title = display_title(core.DocumentTitle().unwrap_or_default(), &url);
        self.state.borrow_mut().title = title.clone();
        if let Some(browser) = self.window().and_then(|w| w.browser()) {
            browser.title_changed(&url, &title);
        }
        self.notify();
    }

    async fn load_favicon(self: Rc<Self>) {
        let generation = self.favicon_generation.get() + 1;
        self.favicon_generation.set(generation);
        let Some(core) = self.core.get().cloned() else {
            return;
        };
        let icon = async {
            let core15 = core.cast::<ICoreWebView2_15>()?;
            if core15.FaviconUri()?.is_empty() {
                return Ok(None);
            }
            let stream = core15
                .GetFaviconAsync(CoreWebView2FaviconImageFormat::Png)?
                .await?;
            let png = xaml::read_all(&stream).await?;
            let image = xaml::png_image(&png).await?;
            Ok::<_, windows_core::Error>(Some((image, png)))
        }
        .await;
        if self.favicon_generation.get() != generation || self.closed.get() {
            return;
        }
        let (image, png) = icon
            .unwrap_or_else(|e| {
                log::debug!("tab {}: favicon: {e}", self.id);
                None
            })
            .unzip();
        *self.favicon.borrow_mut() = image;
        if let (Some(png), Some(browser)) = (&png, self.window().and_then(|w| w.browser())) {
            browser.record_favicon(&self.state().url, png);
        }
        *self.favicon_png.borrow_mut() = png;
        self.notify();
    }

    fn new_window_requested(self: &Rc<Self>, args: &CoreWebView2NewWindowRequestedEventArgs) {
        let Some(window) = self.window() else { return };
        // WebView2 turns Edge's popup blocker off and leaves this to the app: like a browser,
        // open only the windows a user gesture asked for, unless the user allows pop-ups.
        let blocking = window.browser().is_none_or(|b| b.blocks_popups());
        if blocking && !args.IsUserInitiated().unwrap_or(false) {
            let url = args.Uri().unwrap_or_default();
            log::info!("tab {}: blocked a popup to {url}: no user gesture", self.id);
            let _ = args.SetHandled(true);
            return;
        }
        let deferral = match args.GetDeferral() {
            Ok(deferral) => deferral,
            Err(e) => {
                log::warn!("new window request: {e}");
                return;
            }
        };
        let url = args.Uri().unwrap_or_default();
        let background = self
            .background_link
            .take()
            .is_some_and(|(link, at)| link == url && at.elapsed() < Duration::from_secs(3));
        let request = NewWindowRequest {
            args: args.clone(),
            deferral,
        };
        window.open_tab_from(self.id, Initial::Opener(request), background);
    }

    fn binding_called(&self, args: &CoreWebView2DevToolsProtocolEventReceivedEventArgs) {
        let Some(window) = self.window() else { return };
        let Ok(event) = args.ParameterObjectAsJson() else {
            return;
        };
        match shortcuts::parse_binding_call(&event) {
            // Runs on the next turn: the command may close this tab's web view (Ctrl+W).
            Some(PageMessage::Key(command)) => exec::spawn(async move { window.run(command) }),
            Some(PageMessage::BackgroundLink(url)) => {
                *self.background_link.borrow_mut() = Some((url, Instant::now()));
            }
            Some(PageMessage::Store(request)) => {
                exec::spawn(store::answer(window, self.id, request));
            }
            Some(PageMessage::Zoom(ratio)) => {
                let scale = window
                    .xaml_root()
                    .and_then(|r| r.RasterizationScale())
                    .unwrap_or(1.0);
                let level = zoom::Level(zoom::percent(ratio, scale));
                if self.state.borrow().zoom != level {
                    log::debug!("tab {}: zoom {}", self.id, level.label());
                    self.state.borrow_mut().zoom = level;
                    self.notify();
                }
            }
            None => {}
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// What a tab shows as its title: the document's, else its URL, else "New tab".
fn display_title(document_title: String, url: &str) -> String {
    match (document_title.trim(), url) {
        ("" | "about:blank", "" | "about:blank") => "New tab".to_owned(),
        ("", url) => url.to_owned(),
        _ => document_title,
    }
}

/// A handler for events that carry no arguments (WebView2 passes null): runs while the tab lives.
fn signal(
    tab: &Rc<Tab>,
    handler: impl Fn(&Rc<Tab>) + 'static,
) -> impl Fn(Ref<'_, CoreWebView2>, Ref<'_, IInspectable>) + 'static {
    let tab = Rc::downgrade(tab);
    move |_, _| {
        if let Some(tab) = tab.upgrade() {
            handler(&tab);
        }
    }
}

/// Wraps a tab event handler: it runs only while the tab is alive and gets non-null args.
fn on<A: Interface + 'static>(
    tab: &Rc<Tab>,
    handler: impl Fn(&Rc<Tab>, &A) + 'static,
) -> impl Fn(Ref<'_, CoreWebView2>, Ref<'_, A>) + 'static {
    let tab = Rc::downgrade(tab);
    move |_, args| {
        if let (Some(tab), Some(args)) = (tab.upgrade(), args.as_ref()) {
            handler(&tab, args);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::display_title;

    #[test]
    fn titles_fall_back_to_url_then_new_tab() {
        assert_eq!(
            display_title("Example".into(), "https://e.test/"),
            "Example"
        );
        assert_eq!(
            display_title("  ".into(), "https://e.test/"),
            "https://e.test/"
        );
        assert_eq!(display_title(String::new(), "about:blank"), "New tab");
        assert_eq!(
            display_title("about:blank".into(), "about:blank"),
            "New tab"
        );
        assert_eq!(display_title(String::new(), ""), "New tab");
    }
}
