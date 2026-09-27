//! One tab: its `TabViewItem` header in the strip and its `WebView2` in the window's page grid.
//!
//! The web view is not the item's content (a WebView2 inside `TabViewItem` content measures 0
//! high); the window shows the selected tab's view and collapses the others.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use windows_core::{IInspectable, Interface, Ref, Result};

use crate::bindings::*;
use crate::browser::CommitKind;
use crate::shortcuts::{self, PageMessage};
use crate::window::BrowserWindow;
use crate::{exec, xaml};

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
}

pub(crate) struct Tab {
    pub id: TabId,
    window: Weak<BrowserWindow>,
    item: TabViewItem,
    header: TabHeader,
    view: WebView2,
    core: OnceCell<CoreWebView2>,
    state: RefCell<TabState>,
    background_link: RefCell<Option<(String, Instant)>>,
    /// The URI of the last main-frame navigation request; WebView2 reports an empty `Source`
    /// for some documents (data: URLs), and then this is what was committed.
    requested: RefCell<String>,
    favicon_generation: Cell<u64>,
    closed: Cell<bool>,
}

impl Tab {
    pub fn new(id: TabId, window: Weak<BrowserWindow>) -> Result<Rc<Self>> {
        let header = TabHeader::new()?;
        let item = TabViewItem::new()?;
        item.SetHeader(&header.root)?;
        let view = WebView2::new()?;
        Ok(Rc::new(Self {
            id,
            window,
            item,
            header,
            view,
            core: OnceCell::new(),
            state: RefCell::new(TabState {
                title: "New tab".into(),
                ..TabState::default()
            }),
            background_link: RefCell::new(None),
            requested: RefCell::new(String::new()),
            favicon_generation: Cell::new(0),
            closed: Cell::new(false),
        }))
    }

    pub fn item(&self) -> &TabViewItem {
        &self.item
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

    pub fn has_favicon(&self) -> bool {
        self.header.has_favicon.get()
    }

    pub fn is_ready(&self) -> bool {
        self.core.get().is_some()
    }

    /// Creates the engine view in `environment`, then loads `initial`.
    pub async fn start(
        self: Rc<Self>,
        environment: CoreWebView2Environment,
        page_script: Rc<str>,
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
            Initial::Blank => self.navigate("about:blank"),
            Initial::Opener(request) => request.fulfil(&core),
        }
    }

    async fn ensure_core(
        self: &Rc<Self>,
        environment: &CoreWebView2Environment,
        page_script: &str,
    ) -> Result<CoreWebView2> {
        self.view
            .cast::<IWebView22>()?
            .EnsureCoreWebView2WithEnvironmentAsync(environment)?
            .await?;
        let core = self.view.CoreWebView2()?;
        let settings = core.Settings()?;
        settings.SetAreDevToolsEnabled(true)?;
        settings.SetIsWebMessageEnabled(true)?;
        self.wire(&core)?;
        core.AddScriptToExecuteOnDocumentCreatedAsync(page_script)?
            .await?;
        let _ = self.core.set(core.clone());
        Ok(core)
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
        let _ = if loading { core.Stop() } else { core.Reload() };
    }

    pub fn reload(&self) {
        if let Some(core) = self.core.get() {
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
                tab.header.set_loading(true);
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
                tab.header.set_loading(false);
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
        core.WebMessageReceived(on(
            self,
            |tab, args: &CoreWebView2WebMessageReceivedEventArgs| {
                tab.web_message(args);
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
        let browser = self
            .window()
            .and_then(|w| w.browser())
            .filter(|_| url != "about:blank");
        if let Some(browser) = browser {
            let starred = browser.navigation_committed(&url, kind);
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
        self.header.set_title(&title);
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
        let image = async {
            let core15 = core.cast::<ICoreWebView2_15>()?;
            if core15.FaviconUri()?.is_empty() {
                return Ok(None);
            }
            let stream = core15
                .GetFaviconAsync(CoreWebView2FaviconImageFormat::Png)?
                .await?;
            let bitmap = BitmapImage::new()?;
            bitmap
                .cast::<BitmapSource>()?
                .SetSourceAsync(&stream)?
                .await?;
            Ok::<_, windows_core::Error>(Some(bitmap.cast::<ImageSource>()?))
        }
        .await;
        if self.favicon_generation.get() != generation || self.closed.get() {
            return;
        }
        match image {
            Ok(image) => self.header.set_favicon(image.as_ref()),
            Err(e) => {
                log::debug!("tab {}: favicon: {e}", self.id);
                self.header.set_favicon(None);
            }
        }
    }

    fn new_window_requested(self: &Rc<Self>, args: &CoreWebView2NewWindowRequestedEventArgs) {
        let Some(window) = self.window() else { return };
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

    fn web_message(&self, args: &CoreWebView2WebMessageReceivedEventArgs) {
        let Some(window) = self.window() else { return };
        let Some(browser) = window.browser() else {
            return;
        };
        let Ok(message) = args.TryGetWebMessageAsString() else {
            return;
        };
        match shortcuts::parse_page_message(&message, browser.page_nonce()) {
            // Runs on the next turn: the command may close this tab's web view (Ctrl+W).
            Some(PageMessage::Key(command)) => exec::spawn(async move { window.run(command) }),
            Some(PageMessage::BackgroundLink(url)) => {
                *self.background_link.borrow_mut() = Some((url, Instant::now()));
            }
            None => {}
        }
    }
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

const HEADER_XAML: &str = r#"
<Grid {ns} ColumnSpacing="8">
  <Grid.ColumnDefinitions>
    <ColumnDefinition Width="16"/>
    <ColumnDefinition Width="*"/>
  </Grid.ColumnDefinitions>
  <FontIcon x:Name="DefaultIcon" Glyph="&#xE774;" FontSize="14" VerticalAlignment="Center"/>
  <Image x:Name="Favicon" Width="16" Height="16" VerticalAlignment="Center" Visibility="Collapsed"/>
  <ProgressRing x:Name="Spinner" Width="16" Height="16" MinWidth="16" MinHeight="16"
                VerticalAlignment="Center" IsActive="False" Visibility="Collapsed"/>
  <TextBlock x:Name="Title" Grid.Column="1" Text="New tab" TextTrimming="CharacterEllipsis"
             TextWrapping="NoWrap" VerticalAlignment="Center"/>
</Grid>"#;

struct TabHeader {
    root: FrameworkElement,
    default_icon: UIElement,
    favicon: Image,
    spinner: ProgressRing,
    title: TextBlock,
    loading: Cell<bool>,
    has_favicon: Cell<bool>,
}

impl TabHeader {
    fn new() -> Result<Self> {
        let root: FrameworkElement = xaml::load(HEADER_XAML)?;
        Ok(Self {
            default_icon: xaml::find(&root, "DefaultIcon")?,
            favicon: xaml::find(&root, "Favicon")?,
            spinner: xaml::find(&root, "Spinner")?,
            title: xaml::find(&root, "Title")?,
            root,
            loading: Cell::new(false),
            has_favicon: Cell::new(false),
        })
    }

    fn set_title(&self, title: &str) {
        let _ = self.title.SetText(title);
        let _ = xaml::boxed(title).and_then(|tip| ToolTipService::SetToolTip(&self.root, &tip));
    }

    fn set_loading(&self, loading: bool) {
        self.loading.set(loading);
        self.update_icon();
    }

    fn set_favicon(&self, image: Option<&ImageSource>) {
        let _ = self.favicon.SetSource(image);
        self.has_favicon.set(image.is_some());
        self.update_icon();
    }

    fn update_icon(&self) {
        let loading = self.loading.get();
        let favicon = !loading && self.has_favicon.get();
        let _ = self.spinner.SetIsActive(loading);
        let _ = xaml::set_visible(&self.spinner, loading);
        let _ = xaml::set_visible(&self.favicon, favicon);
        let _ = xaml::set_visible(&self.default_icon, !loading && !favicon);
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
