//! One tab: its `WebView2` in the window's page grid and the state the tab lists show.
//!
//! The web view is never a tab list item's content (a WebView2 inside `TabViewItem` content
//! measures 0 high); the window shows the selected tab's view and collapses the others.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use serde_json::json;
use vsesvit_core::history::Transition;
use vsesvit_core::https_only::{self, Cause, Next, Upgrades};
use vsesvit_core::permissions::{Origin, Permission};
use vsesvit_core::trackers::{self, TrackerList, TrackingProtection};
use vsesvit_core::{Url, new_tab, session, view_source};
use windows_core::{HSTRING, IInspectable, Interface, Ref, Result};
use windows_future::IAsyncOperation;

use crate::bindings::*;
use crate::browser::{Browser, CommitKind};
use crate::permissions::{Requested, TabPermissions};
use crate::session::now_ms;
use crate::shortcuts::{self, PageMessage, PageScript};
use crate::store;
use crate::tab_header::{Audio, TabLook};
use crate::window::BrowserWindow;
use crate::media::{self, MediaAction, Playback};
use crate::trackers::Protection;
use crate::{capturing, connection, cookies, exec, platform, xaml, zoom};

/// Identifies a tab within this process.
pub(crate) type TabId = u64;

/// How often a page that may capture is asked what it captures.
const CAPTURE_POLL: Duration = Duration::from_millis(500);

/// The name WebView2 would give a View page source item of its own (lower camel case English).
pub(crate) const VIEW_SOURCE_ITEM: &str = "viewPageSource";

/// What a new tab loads first.
pub(crate) enum Initial {
    Url(String),
    Blank,
    /// The page asked for a new window (`window.open`, target=_blank, Ctrl+click): the tab's
    /// engine view becomes that window, so `window.opener` keeps working.
    Opener(NewWindowRequest),
}

/// A deferred `window.open`. Dropping one that was not fulfilled cancels it, so whichever way
/// opening its tab failed, the page's call returns `null` instead of WebView2 opening a window
/// of its own.
pub(crate) struct NewWindowRequest {
    args: CoreWebView2NewWindowRequestedEventArgs,
    /// Taken once the request is answered.
    deferral: Option<Deferral>,
}

impl NewWindowRequest {
    fn fulfil(mut self, core: &CoreWebView2) {
        let set = self
            .args
            .SetNewWindow(core)
            .and_then(|()| self.args.SetHandled(true));
        if let Err(e) = set {
            log::warn!("new window request: {e}");
        }
        if let Some(deferral) = self.deferral.take() {
            let _ = deferral.Complete();
        }
    }
}

impl Drop for NewWindowRequest {
    fn drop(&mut self) {
        if let Some(deferral) = self.deferral.take() {
            let _ = self.args.SetHandled(true);
            let _ = deferral.Complete();
        }
    }
}

/// How far the tab's current navigation has got.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Load {
    #[default]
    Idle,
    /// The navigation was requested; nothing of the new page has arrived.
    Started,
    /// The new document is arriving.
    Committed,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TabState {
    /// The committed URL; empty until the first commit.
    pub url: String,
    pub title: String,
    pub load: Load,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub starred: bool,
    pub fullscreen: bool,
    pub zoom: zoom::Level,
    /// The engine reports the page audible (it holds this a moment after the sound stops).
    pub audible: bool,
    pub muted: bool,
}

impl TabState {
    pub fn loading(&self) -> bool {
        self.load != Load::Idle
    }
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
    /// The `view-source:` address the shell last loaded. WebView2 reports the address of the
    /// page it shows the source of, so this is what the tab shows while that page is current.
    view_source: RefCell<Option<String>>,
    /// How the navigation the shell started came about, for history. Page-initiated
    /// navigations have none and count as links.
    transition: RefCell<PendingTransition>,
    last_active_ms: Cell<i64>,
    favicon_generation: Cell<u64>,
    /// The engine's `Security.visibleSecurityStateChanged` reports: the page's TLS connection
    /// and certificate chain, which the lock's popup shows.
    security: RefCell<connection::Reports>,
    /// Pinned tabs lead the tab list.
    pinned: Cell<bool>,
    permissions: TabPermissions,
    /// The page's capture is being polled (see `capturing`).
    capture_polled: Cell<bool>,
    /// The shortcut world in each DevTools session: the page's own (`""`) and each attached
    /// frame's.
    shortcut_worlds: RefCell<HashMap<String, shortcuts::World>>,
    /// The zoom remembered for the page's site, while the page is not at it.
    zoom_memory: RefCell<zoom::Memory>,
    /// Tracking protection on the page (see `trackers`).
    trackers: RefCell<Protection>,
    /// HTTPS-only's upgrades and warning page in the tab.
    https: RefCell<Upgrades>,
    /// Cookie controls in the tab's DevTools sessions (see `cookies`).
    cookies: cookies::Sessions,
    /// The navigation the tab last stopped to load its https URL or the warning instead.
    stopped: Cell<Option<u64>>,
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
            view_source: RefCell::new(None),
            transition: RefCell::default(),
            last_active_ms: Cell::new(now_ms()),
            favicon_generation: Cell::new(0),
            security: RefCell::default(),
            pinned: Cell::new(false),
            permissions: TabPermissions::default(),
            capture_polled: Cell::new(false),
            shortcut_worlds: RefCell::default(),
            zoom_memory: RefCell::default(),
            trackers: RefCell::default(),
            https: RefCell::default(),
            cookies: cookies::Sessions::default(),
            stopped: Cell::new(None),
            closed: Cell::new(false),
        }))
    }

    /// What the tab lists show.
    pub fn look(&self) -> TabLook {
        let state = self.state.borrow();
        TabLook {
            title: state.title.clone(),
            favicon: self.favicon.borrow().clone(),
            loading: state.loading(),
            audio: Audio::of(state.audible, state.muted),
            pinned: self.pinned.get(),
            capturing: self.permissions.capturing(),
        }
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned.get()
    }

    pub fn set_pinned(&self, pinned: bool) {
        self.pinned.set(pinned);
        self.notify();
    }

    pub fn set_muted(&self, muted: bool) {
        if let Some(core) = self.core.get()
            && let Err(e) = core.cast::<ICoreWebView2_8>().and_then(|c| c.SetIsMuted(muted))
        {
            log::warn!("tab {}: mute: {e}", self.id);
        }
    }

    fn audio_changed(&self) {
        let Some(core) = self.core.get().and_then(|c| c.cast::<ICoreWebView2_8>().ok()) else {
            return;
        };
        let audible = core.IsDocumentPlayingAudio().unwrap_or(false);
        let muted = core.IsMuted().unwrap_or(false);
        {
            let mut state = self.state.borrow_mut();
            state.audible = audible;
            state.muted = muted;
        }
        if let Some(window) = self.window() {
            window.tab_audio_changed(self);
        }
        self.notify();
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

    /// The engine's report on the page's connection (`Security.visibleSecurityStateChanged`).
    pub fn security_report(&self) -> Option<String> {
        self.security.borrow().current()
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
                return;
            }
        };
        if self.closed.get() {
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
        if let Some(browser) = self.browser() {
            self.apply_autofill(&settings, browser.autofill_forms());
            let clear = async || cookies::clear_at_start(&browser, &core).await;
            if let Err(e) = browser.engine().set_up_profile(&core, clear).await {
                log::warn!("tab {}: engine profile: {e}", self.id);
            }
        }
        self.wire(&core)?;
        self.inject(&core, page_script).await?;
        let _ = self.core.set(core.clone());
        if let Some(browser) = self.browser() {
            self.filter_trackers(&browser.trackers());
        }
        Ok(core)
    }

    /// Runs the shortcut script in its isolated world of every new document (see `shortcuts`),
    /// frames from other sites included (see `attached`), and listens to its binding; runs the
    /// store script in the main world (see `store`).
    async fn inject(self: &Rc<Self>, core: &CoreWebView2, script: &PageScript) -> Result<()> {
        self.add_world(String::new(), script);
        for event in shortcuts::CONTEXT_EVENTS {
            core.GetDevToolsProtocolEventReceiver(event)?
                .DevToolsProtocolEventReceived(on(
                    self,
                    move |tab, args: &CoreWebView2DevToolsProtocolEventReceivedEventArgs| {
                        let session = session_of(args);
                        if let (Some(world), Ok(params)) = (
                            tab.shortcut_worlds.borrow_mut().get_mut(&session),
                            args.ParameterObjectAsJson(),
                        ) {
                            world.track(event, &params.to_string());
                        }
                    },
                ))?
                .forget();
        }
        core.GetDevToolsProtocolEventReceiver("Target.attachedToTarget")?
            .DevToolsProtocolEventReceived(on(
                self,
                |tab, args: &CoreWebView2DevToolsProtocolEventReceivedEventArgs| {
                    if let Ok(params) = args.ParameterObjectAsJson() {
                        tab.attached(&params.to_string());
                    }
                },
            ))?
            .forget();
        core.GetDevToolsProtocolEventReceiver("Target.detachedFromTarget")?
            .DevToolsProtocolEventReceived(on(
                self,
                |tab, args: &CoreWebView2DevToolsProtocolEventReceivedEventArgs| {
                    let params = args.ParameterObjectAsJson().map(|p| p.to_string());
                    if let Some((session, _)) =
                        params.ok().and_then(|p| shortcuts::attached_session(&p))
                    {
                        tab.shortcut_worlds.borrow_mut().remove(&session);
                        tab.cookies.detached(&session);
                    }
                },
            ))?
            .forget();
        core.GetDevToolsProtocolEventReceiver("Runtime.bindingCalled")?
            .DevToolsProtocolEventReceived(on(
                self,
                |tab, args: &CoreWebView2DevToolsProtocolEventReceivedEventArgs| {
                    tab.binding_called(args);
                },
            ))?
            .forget();
        core.GetDevToolsProtocolEventReceiver("Security.visibleSecurityStateChanged")?
            .DevToolsProtocolEventReceived(on(
                self,
                |tab, args: &CoreWebView2DevToolsProtocolEventReceivedEventArgs| {
                    tab.security
                        .borrow_mut()
                        .reported(args.ParameterObjectAsJson().ok().map(|j| j.to_string()));
                    tab.notify();
                },
            ))?
            .forget();
        let calls = shortcuts::world_calls(script).into_iter().chain([
            ("Security.enable", "{}".to_owned()),
            (
                "Page.addScriptToEvaluateOnNewDocument",
                json!({ "source": store::MAIN_WORLD_SCRIPT }).to_string(),
            ),
            (
                "Page.addScriptToEvaluateOnNewDocument",
                json!({ "source": media::MAIN_WORLD_SCRIPT }).to_string(),
            ),
            (
                "Page.addScriptToEvaluateOnNewDocument",
                json!({ "source": capturing::MAIN_WORLD_SCRIPT }).to_string(),
            ),
            ("Target.setAutoAttach", shortcuts::AUTO_ATTACH.to_owned()),
        ]);
        for (method, params) in calls {
            core.CallDevToolsProtocolMethodAsync(method, &params)?
                .await?;
        }
        self.set_up_cookies(core, "").await;
        self.apply_shortcuts(core, "").await
    }

    fn add_world(&self, session: String, script: &PageScript) {
        let world = shortcuts::World {
            name: script.world.clone(),
            ..shortcuts::World::default()
        };
        self.shortcut_worlds.borrow_mut().insert(session, world);
    }

    /// A session `AUTO_ATTACH` attached, waiting to start: a frame from another site gets the
    /// shortcut world the page has, so the keys it takes work there too.
    fn attached(self: &Rc<Self>, params: &str) {
        let Some((session, frame)) = shortcuts::attached_session(params) else {
            return;
        };
        let (Some(core), Some(browser)) = (self.core.get().cloned(), self.browser()) else {
            return;
        };
        let tab = self.clone();
        exec::spawn(async move {
            let set_up = if frame {
                tab.set_up_frame(&core, &session, &browser.page_script()).await
            } else {
                Ok(())
            };
            if let Err(e) = set_up {
                log::debug!("tab {}: frame session {session}: {e}", tab.id);
            }
            if frame {
                tab.set_up_cookies(&core, &session).await;
            }
            // A frame the setup failed in still starts, without the shortcuts.
            let run = devtools_in(&core, &session, "Runtime.runIfWaitingForDebugger", "{}").await;
            if let Err(e) = run {
                log::debug!("tab {}: starting frame session {session}: {e}", tab.id);
            }
        });
    }

    /// Sets up the cookie controls in DevTools session `session`; the page or frame loads
    /// without them if that fails.
    async fn set_up_cookies(&self, core: &CoreWebView2, session: &str) {
        let script = self.browser().and_then(|b| cookies::block_script(&b));
        if let Err(e) = self.cookies.set_up(core, session, script.as_deref()).await {
            log::warn!(
                "tab {}: cookie controls in session {session:?}: {e}",
                self.id
            );
        }
    }

    /// Applies changed cookie controls to the documents to come: third-party cookies `blocked`
    /// or not on the page, and the block script. The calls go out at once, so a reload after
    /// this loads under them.
    pub fn apply_cookies(&self, blocked: bool, script: Option<&str>) {
        if let Some(core) = self.core.get() {
            self.cookies.apply_all(core, blocked, script);
        }
    }

    async fn set_up_frame(
        &self,
        core: &CoreWebView2,
        session: &str,
        script: &PageScript,
    ) -> Result<()> {
        self.add_world(session.to_owned(), script);
        let calls = shortcuts::world_calls(script)
            .into_iter()
            .chain([("Target.setAutoAttach", shortcuts::AUTO_ATTACH.to_owned())]);
        for (method, params) in calls {
            devtools_in(core, session, method, &params).await?;
        }
        self.apply_shortcuts(core, session).await
    }

    /// Gives the page script the key sets in effect: in documents to come, and in the ones
    /// already loaded, so a changed shortcut applies without a reload.
    pub fn shortcuts_changed(self: &Rc<Self>) {
        let Some(core) = self.core.get().cloned() else {
            return;
        };
        let tab = self.clone();
        exec::spawn(async move {
            let sessions: Vec<String> = tab.shortcut_worlds.borrow().keys().cloned().collect();
            for session in sessions {
                if let Err(e) = tab.apply_shortcuts(&core, &session).await {
                    log::warn!("tab {}: keyboard shortcuts in session {session:?}: {e}", tab.id);
                }
            }
        });
    }

    /// Gives the shortcut world in DevTools session `session` the key sets in effect.
    async fn apply_shortcuts(&self, core: &CoreWebView2, session: &str) -> Result<()> {
        let script = shortcuts::current().keys_script();
        let Some(world) = self.shortcut_worlds.borrow().get(session).map(|w| w.name.clone()) else {
            return Ok(());
        };
        let added = devtools_in(
            core,
            session,
            "Page.addScriptToEvaluateOnNewDocument",
            &json!({ "source": script, "worldName": world }).to_string(),
        )
        .await?;
        let identifier = serde_json::from_str::<serde_json::Value>(&added)
            .ok()
            .and_then(|v| v["identifier"].as_str().map(str::to_owned));
        // The newer script runs after the older one, so replacing it cannot leave a new
        // document with stale keys.
        let Some((replaced, contexts)) =
            self.shortcut_worlds.borrow_mut().get_mut(session).map(|world| {
                let replaced = std::mem::replace(&mut world.keys_script, identifier);
                (replaced, world.contexts.clone())
            })
        else {
            return Ok(());
        };
        if let Some(old) = replaced {
            devtools_in(
                core,
                session,
                "Page.removeScriptToEvaluateOnNewDocument",
                &json!({ "identifier": old }).to_string(),
            )
            .await?;
        }
        for context in contexts {
            let params = json!({ "expression": script, "contextId": context }).to_string();
            let evaluated = devtools_in(core, session, "Runtime.evaluate", &params).await;
            // A context can go away between the event and this call.
            if let Err(e) = evaluated {
                log::debug!("tab {}: shortcut world {context}: {e}", self.id);
            }
        }
        Ok(())
    }

    /// WebView2's Save As dialog for the page, and how it ended.
    pub async fn save_as(&self) -> Result<CoreWebView2SaveAsUIResult> {
        let core = self.core.get().ok_or_else(windows_core::Error::empty)?;
        core.cast::<ICoreWebView2_25>()?.ShowSaveAsUIAsync()?.await
    }

    /// WebView2's print preview for the page, as Chrome's Print.
    pub fn print(&self) -> Result<()> {
        let core = self.core.get().ok_or_else(windows_core::Error::empty)?;
        core.cast::<ICoreWebView2_16>()?
            .ShowPrintUI(CoreWebView2PrintDialogKind::Browser)
    }

    pub fn open_devtools(&self) -> Result<()> {
        let core = self.core.get().ok_or_else(windows_core::Error::empty)?;
        core.OpenDevToolsWindow()
    }

    pub fn navigate(&self, url: &str) {
        let Some(core) = self.core.get() else {
            log::warn!("tab {}: navigate before the engine view is ready", self.id);
            return;
        };
        *self.view_source.borrow_mut() = view_source::viewed_url(url).map(|_| url.to_owned());
        self.https.borrow_mut().leave();
        if let Err(e) = core.Navigate(url) {
            log::warn!("tab {}: navigate to {url}: {e}", self.id);
        }
    }

    /// The new tab page, loaded as HTML at `about:blank` so the tab still reads as blank.
    fn show_new_tab_page(&self, core: &CoreWebView2) {
        let shown = match self.browser() {
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
        if self.core.get().is_some() {
            let loading = self.state.borrow().loading();
            self.transition.borrow_mut().requested(transition, loading);
        }
        self.navigate(url);
    }

    pub fn go_back(&self) {
        if let Some(core) = self.core.get() {
            self.https.borrow_mut().leave();
            let _ = core.GoBack();
        }
    }

    pub fn go_forward(&self) {
        if let Some(core) = self.core.get() {
            self.https.borrow_mut().leave();
            let _ = core.GoForward();
        }
    }

    pub fn reload_or_stop(&self) {
        let Some(core) = self.core.get() else { return };
        let loading = self.state.borrow().loading();
        if loading {
            let _ = core.Stop();
        } else {
            self.reload();
        }
    }

    /// Reloads the page; on HTTPS-only's warning, tries its https URL again, as Chrome does.
    pub fn reload(&self) {
        if let Some(core) = self.core.get() {
            let loading = self.state.borrow().loading();
            self.transition
                .borrow_mut()
                .requested(Transition::Reload, loading);
            let warning = self.https.borrow().warning().cloned();
            match warning {
                Some(url) => self.navigate(url.as_str()),
                None => {
                    let _ = core.Reload();
                }
            }
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

    /// What the page is playing; `None` before it has a document with the media script.
    pub async fn playback(&self) -> Option<Playback> {
        let json = self.eval(media::STATE_SCRIPT).await.ok()?;
        media::parse_state(&json)
    }

    pub async fn media_action(&self, action: MediaAction) {
        if let Err(e) = self.eval(&media::act_script(action)).await {
            log::debug!("tab {}: media action: {e}", self.id);
        }
    }

    /// Shows the playing video over the whole page (`on`), or the page again. Whether a video
    /// is shown.
    pub async fn present_pip(&self, on: bool) -> bool {
        self.eval(&media::pip_script(on))
            .await
            .is_ok_and(|result| result == "true")
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

    /// Zooms the page as its keyboard shortcuts do, one after another, with real key presses:
    /// focuses the page, and a moment later sends them only if the window is still in the
    /// foreground with the page focused and nothing held (`zoom::press`). Only for a window in
    /// the foreground. When the presses were for the site's level (`restoring`) and were not
    /// sent, that level is wanted again, for the next focus or selection.
    pub fn zoom(&self, steps: Vec<zoom::Step>, restoring: bool) {
        self.focus_page();
        let (window, id) = (self.window.clone(), self.id);
        // The page takes the focus a moment after the web view does.
        exec::spawn(async move {
            exec::sleep(Duration::from_millis(50)).await;
            let Some(window) = window.upgrade() else {
                return;
            };
            let Some(tab) = window.tab(id) else {
                return;
            };
            if restoring && !tab.zoom_memory.borrow().awaits_presses() {
                log::debug!("tab {id}: its zoom changed before the keys went; not sent");
                return;
            }
            let sent = match platform::window_handle(window.xaml_window()) {
                Ok(hwnd) => zoom::press(hwnd, || tab.page_has_focus(), &steps),
                Err(_) => Err(zoom::NotSent::Background),
            };
            if let Err(why) = sent {
                log::info!("tab {id}: zoom keys not sent: {why:?}");
                if restoring {
                    tab.zoom_memory.borrow_mut().put_back();
                }
            }
        });
    }

    /// Whether the keyboard focus is in the page, so key presses reach it.
    fn page_has_focus(&self) -> bool {
        let Ok(view) = self.view.cast::<DependencyObject>() else {
            return false;
        };
        let Ok(root) = self.view.cast::<UIElement>().and_then(|v| v.XamlRoot()) else {
            return false;
        };
        let mut node = FocusManager::GetFocusedElementWithRoot(&root)
            .and_then(|f| f.cast::<DependencyObject>())
            .ok();
        while let Some(element) = node {
            if xaml::same_object(&element, &view) {
                return true;
            }
            node = VisualTreeHelper::GetParent(&element).ok();
        }
        false
    }

    /// The zoom remembered for the page's site, while the page is not at it.
    pub fn wanted_zoom(&self) -> Option<zoom::Level> {
        self.zoom_memory.borrow().wanted()
    }

    /// Takes the level [`Tab::wanted_zoom`] says, for the shell to press its way there; it is
    /// not wanted again unless [`Tab::zoom`] finds it cannot send the presses, so presses that
    /// went out are never repeated.
    pub fn take_wanted_zoom(&self) -> Option<zoom::Level> {
        self.zoom_memory.borrow_mut().take_wanted()
    }

    /// A zoom the page reported with the window drawn at `scale`: a change is remembered for
    /// the page's site once it settles, and a new document that starts at another level than
    /// its site's is taken there when it can be.
    fn zoom_reported(&self, window: &BrowserWindow, report: zoom::Report, scale: f64) {
        let Some(browser) = window.browser() else {
            return;
        };
        // The document's own address: its first report can arrive before its commit does.
        let url = self
            .core
            .get()
            .and_then(|core| core.Source().ok())
            .and_then(|source| vsesvit_core::Url::parse(&source).ok());
        let remembered = match &url {
            Some(url) => browser.core(|p| p.site_zoom().get(url)).unwrap_or_else(|e| {
                log::warn!("site zoom: {e}");
                vsesvit_core::zoom::DEFAULT
            }),
            None => vsesvit_core::zoom::DEFAULT,
        };
        let change = self
            .zoom_memory
            .borrow_mut()
            .reported(report, zoom::Level::of_factor(remembered), scale);
        if let (Some(change), Some(url)) = (change, url) {
            let (window, id) = (self.window.clone(), self.id);
            exec::spawn(async move {
                exec::sleep(zoom::SETTLE).await;
                let Some(window) = window.upgrade() else {
                    return;
                };
                let settled = window
                    .tab(id)
                    .and_then(|tab| tab.zoom_memory.borrow().settled(change, window.scale()));
                if let (Some(level), Some(browser)) = (settled, window.browser())
                    && let Err(e) = browser.core(|p| p.site_zoom().set(&url, level.factor()))
                {
                    log::warn!("site zoom: {e}");
                }
            });
        }
        if matches!(report, zoom::Report::Start(_)) {
            window.take_to_site_zoom(self);
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

    pub fn set_autofill(&self, forms: bool) {
        if let Some(settings) = self.core.get().and_then(|c| c.Settings().ok()) {
            self.apply_autofill(&settings, forms);
        }
    }

    /// Password saving stays off: Vsesvit leaves passwords to a password manager's extension. A
    /// runtime too old for these settings keeps its defaults; the tab still works.
    fn apply_autofill(&self, settings: &CoreWebView2Settings, forms: bool) {
        let applied = settings.cast::<ICoreWebView2Settings4>().and_then(|settings| {
            settings.SetIsPasswordAutosaveEnabled(false)?;
            settings.SetIsGeneralAutofillEnabled(forms)
        });
        if let Err(e) = applied {
            log::warn!("tab {}: autofill settings: {e}", self.id);
        }
    }

    /// Makes the engine view raise the requests `list` may block (see `trackers`).
    pub fn filter_trackers(&self, list: &TrackerList) {
        let Some(core) = self.core.get() else { return };
        if let Err(e) = self.trackers.borrow_mut().add_filters(core, list) {
            log::warn!("tab {}: tracker request filters: {e}", self.id);
        }
    }

    /// The tracker domains tracking protection blocked on the page.
    pub fn blocked_trackers(&self) -> Vec<String> {
        self.trackers.borrow().blocked().iter().cloned().collect()
    }

    /// Turns tracking protection on or off for the page's site, and loads the page again under
    /// it, as Firefox does.
    pub fn set_tracking_protection(&self, on: bool) {
        let (Some(browser), Some(origin)) = (self.browser(), self.origin()) else {
            return;
        };
        if let Err(e) = browser.core(|p| trackers::set_allowed(p, &origin, !on)) {
            log::warn!(
                "tab {}: tracking protection for {}: {e}",
                self.id,
                origin.as_str()
            );
            return;
        }
        log::info!(
            "tab {}: tracking protection {} for {}",
            self.id,
            if on { "on" } else { "off" },
            origin.as_str()
        );
        self.reload();
    }

    /// A request that may be a tracker's: answered with an empty 403 when the page's protection
    /// blocks it.
    fn resource_requested(&self, args: &CoreWebView2WebResourceRequestedEventArgs) {
        let Some(browser) = self.browser() else {
            return;
        };
        let Ok(uri) = args.Request().and_then(|r| r.Uri()) else {
            return;
        };
        let before = self.trackers.borrow().blocked().len();
        if !self.trackers.borrow_mut().blocks(&browser.trackers(), &uri) {
            return;
        }
        log::debug!("tab {}: blocked tracker {uri}", self.id);
        let answered = browser
            .engine()
            .environment()
            .CreateWebResourceResponse(None::<&IRandomAccessStream>, 403, "Forbidden", "")
            .and_then(|response| args.SetResponse(&response));
        if let Err(e) = answered {
            log::warn!("tab {}: blocking {uri}: {e}", self.id);
        }
        if self.trackers.borrow().blocked().len() > before {
            self.notify();
        }
    }

    /// Feeds HTTPS-only the main-frame navigation that starts, and stops it when the tab loads
    /// its https URL or the warning page instead. The shell's own navigations leave the warning
    /// first, so a navigation here that is no redirect is the page's.
    fn stopped_for_https(
        self: &Rc<Self>,
        args: &CoreWebView2NavigationStartingEventArgs,
        uri: &str,
    ) -> bool {
        let (Some(browser), Ok(url)) = (self.browser(), Url::parse(uri)) else {
            return false;
        };
        let cause = if args.IsRedirected().unwrap_or(false) {
            Cause::Redirect
        } else {
            Cause::Link
        };
        let upgrade = browser.https_upgrade(&url);
        let next = self.https.borrow_mut().starting(&url, cause, upgrade);
        // The replacement loads once the stopped navigation is done with.
        let stop = |then: Box<dyn FnOnce(&Tab)>| {
            self.stopped.set(args.NavigationId().ok());
            if let Err(e) = args.SetCancel(true) {
                log::warn!("tab {}: stopping {uri}: {e}", self.id);
            }
            let tab = self.clone();
            exec::spawn(async move { then(&tab) });
        };
        match next {
            Next::Load => false,
            Next::Allow(url) => {
                log::info!("tab {}: continuing to {url} without a secure connection", self.id);
                if let Err(e) = browser.core(|p| https_only::allow(p, &url)) {
                    log::warn!("tab {}: HTTPS-only exception for {url}: {e}", self.id);
                }
                false
            }
            Next::Upgrade(https) => {
                log::debug!("tab {}: upgrading {uri} to {https}", self.id);
                stop(Box::new(move |tab| {
                    if let Some(core) = tab.core.get()
                        && let Err(e) = core.Navigate(https.as_str())
                    {
                        log::warn!("tab {}: navigate to {https}: {e}", tab.id);
                    }
                }));
                true
            }
            Next::Warn(http) => {
                stop(Box::new(move |tab| tab.show_https_warning(&http)));
                true
            }
        }
    }

    /// HTTPS-only's warning that `url` has no secure connection, shown at `url`.
    fn show_https_warning(&self, url: &Url) {
        log::info!("tab {}: {url} has no secure connection", self.id);
        let Some(core) = self.core.get() else { return };
        if let Err(e) = core.NavigateToString(&https_only::warning_page(url)) {
            log::warn!("tab {}: HTTPS-only warning: {e}", self.id);
        }
    }

    /// Releases the engine view. The window removes the XAML parts.
    pub fn close(&self) {
        if !self.closed.replace(true) {
            self.permissions.close();
            let _ = self.view.Close();
        }
    }

    fn window(&self) -> Option<Rc<BrowserWindow>> {
        self.window.upgrade()
    }

    fn browser(&self) -> Option<Rc<Browser>> {
        self.window()?.browser()
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
                let uri = args.Uri().unwrap_or_default();
                if tab.stopped_for_https(args, &uri) {
                    return;
                }
                let origin = Origin::parse(&uri);
                let level = tab.browser().map_or(TrackingProtection::Off, |b| {
                    b.core(|p| trackers::level_for(p, origin.as_ref()))
                });
                tab.trackers.borrow_mut().navigation_starting(&uri, level);
                if let (Some(core), Some(browser)) = (tab.core.get(), tab.browser()) {
                    let blocked = cookies::third_party_blocked(&browser, origin.as_ref());
                    tab.cookies.navigation_starting(core, blocked);
                }
                *tab.requested.borrow_mut() = uri;
                tab.transition.borrow_mut().starting();
                tab.state.borrow_mut().load = Load::Started;
                tab.security.borrow_mut().navigation_starting();
                tab.notify();
            },
        ))?
        .forget();
        core.ContentLoading(on(self, |tab, _: &CoreWebView2ContentLoadingEventArgs| {
            tab.https.borrow_mut().committed();
            tab.state.borrow_mut().load = Load::Committed;
            tab.security.borrow_mut().new_document();
            tab.trackers.borrow_mut().new_document();
            tab.committed(CommitKind::NewDocument);
        }))?
        .forget();
        core.SourceChanged(on(
            self,
            |tab, args: &CoreWebView2SourceChangedEventArgs| {
                if args.IsNewDocument().unwrap_or(true) {
                    tab.security.borrow_mut().new_document();
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
                let id = args.NavigationId().ok();
                if id.is_some() && tab.stopped.get() == id {
                    tab.stopped.set(None);
                    return;
                }
                let success = args.IsSuccess().unwrap_or(true);
                let status = args
                    .WebErrorStatus()
                    .unwrap_or(CoreWebView2WebErrorStatus::Unknown);
                if !success {
                    log::info!("tab {}: navigation failed, web error {}", tab.id, status.0);
                }
                tab.transition.borrow_mut().completed();
                navigation_ended(&mut tab.state.borrow_mut(), &mut tab.requested.borrow_mut());
                tab.refresh_history();
                tab.notify();
                // A stopped load is no failed upgrade.
                let ended = success || status == CoreWebView2WebErrorStatus::OperationCanceled;
                let warning = tab.https.borrow_mut().finished(ended);
                if let Some(url) = warning {
                    tab.show_https_warning(&url);
                }
                if let Some(browser) = tab.browser() {
                    let rules = browser.core(vsesvit_core::cookies::site_rules);
                    cookies::delete_blocked(&browser, rules);
                }
            },
        ))?
        .forget();
        core.DocumentTitleChanged(signal(self, |tab| tab.title_changed()))?
            .forget();
        let audio = core.cast::<ICoreWebView2_8>()?;
        audio
            .IsDocumentPlayingAudioChanged(signal(self, |tab| tab.audio_changed()))?
            .forget();
        audio
            .IsMutedChanged(signal(self, |tab| tab.audio_changed()))?
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
        // A saved page never raises `DownloadStarting`, yet WebView2 shows its own download
        // flyout for it; the shell keeps that flyout hidden for every download.
        core.cast::<ICoreWebView2_9>()?
            .IsDefaultDownloadDialogOpenChanged(|core, _| {
                if let Some(core) = core.as_ref().and_then(|c| c.cast::<ICoreWebView2_9>().ok())
                    && core.IsDefaultDownloadDialogOpen().unwrap_or(false)
                {
                    let _ = core.CloseDefaultDownloadDialog();
                }
            })?
            .forget();
        core.PermissionRequested(on(
            self,
            |tab, args: &CoreWebView2PermissionRequestedEventArgs| {
                tab.permission_requested(args);
            },
        ))?
        .forget();
        core.cast::<ICoreWebView2_27>()?
            .ScreenCaptureStarting(on(
                self,
                |tab, args: &CoreWebView2ScreenCaptureStartingEventArgs| {
                    tab.screen_capture_starting(args);
                },
            ))?
            .forget();
        core.cast::<ICoreWebView2_11>()?
            .ContextMenuRequested(on(
                self,
                |tab, args: &CoreWebView2ContextMenuRequestedEventArgs| {
                    for added in [tab.add_selection_item(args), tab.add_view_source_item(args)] {
                        if let Err(e) = added {
                            log::warn!("tab {}: context menu: {e}", tab.id);
                        }
                    }
                },
            ))?
            .forget();
        core.WebResourceRequested(on(
            self,
            |tab, args: &CoreWebView2WebResourceRequestedEventArgs| {
                tab.resource_requested(args);
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
        let prompt_gone = self
            .permissions
            .committed(&url, kind == CommitKind::NewDocument);
        if prompt_gone && let Some(window) = self.window() {
            window.permission_prompt_gone(self.id);
        }
        let transition = match kind {
            CommitKind::NewDocument => self.transition.borrow_mut().new_document(),
            CommitKind::SameDocument => self.transition.borrow_mut().same_document(),
        };
        if matches!(transition, Transition::Typed | Transition::Bookmark)
            && let Some(window) = self.window()
        {
            window.forget_openers();
        }
        if let Some(browser) = self.browser() {
            self.permissions
                .refresh_site(&browser, Origin::parse(&url).as_ref());
            self.watch_capture();
            if self.https.borrow().warning().is_some() {
                browser.session_changed();
            } else {
                let starred = browser.navigation_committed(&url, kind, transition);
                self.state.borrow_mut().starred = starred;
            }
        }
        self.notify();
    }

    fn refresh_url(&self) {
        let Some(core) = self.core.get() else { return };
        match core.Source() {
            Ok(url) => {
                let warning = self.https.borrow().warning().map(Url::to_string);
                let url = if let Some(warning) = warning {
                    warning
                } else if url.is_empty() {
                    self.requested.borrow().clone()
                } else {
                    shown_url(url, self.view_source.borrow().as_deref())
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
        if let Some(browser) = self.browser()
            && self.https.borrow().warning().is_none()
        {
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
        if let (Some(png), Some(browser)) = (&png, self.browser()) {
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
            deferral: Some(deferral),
        };
        window.open_tab_from(self.id, Initial::Opener(request), background);
    }

    /// Chrome's item for selected text, right after Copy: a search for it on the default
    /// engine, or its address, opened in a new tab next to this one.
    fn add_selection_item(
        self: &Rc<Self>,
        args: &CoreWebView2ContextMenuRequestedEventArgs,
    ) -> Result<()> {
        let target = args.ContextMenuTarget()?;
        if !target.HasSelection()? {
            return Ok(());
        }
        let Some(browser) = self.browser() else {
            return Ok(());
        };
        let Some(action) = browser.selection_action(&target.SelectionText()?) else {
            return Ok(());
        };
        let item = self.new_tab_item(&browser, &action.label, action.url.to_string())?;
        let items = args.MenuItems()?;
        match item_named(&items, "copy")? {
            Some(index) => items.InsertAt(index + 1, &item),
            None => items.Append(&item),
        }
    }

    /// Chrome's View page source on the page's own menu, right before Inspect, which WebView2's
    /// menu lacks.
    fn add_view_source_item(
        self: &Rc<Self>,
        args: &CoreWebView2ContextMenuRequestedEventArgs,
    ) -> Result<()> {
        let target = args.ContextMenuTarget()?;
        if target.Kind()? != CoreWebView2ContextMenuTargetKind::Page
            || target.HasLinkUri()?
            || target.IsEditable()?
        {
            return Ok(());
        }
        let items = args.MenuItems()?;
        let (Some(url), None, Some(browser)) = (
            view_source::source_url(&self.state.borrow().url),
            item_named(&items, VIEW_SOURCE_ITEM)?,
            self.browser(),
        ) else {
            return Ok(());
        };
        let item = self.new_tab_item(&browser, "View page source", url)?;
        match item_named(&items, "inspectElement")? {
            Some(index) => items.InsertAt(index, &item),
            None => items.Append(&item),
        }
    }

    /// A context menu item that opens `url` in a new tab next to this one.
    fn new_tab_item(
        self: &Rc<Self>,
        browser: &Browser,
        label: &str,
        url: String,
    ) -> Result<CoreWebView2ContextMenuItem> {
        let item = browser
            .engine()
            .environment()
            .cast::<ICoreWebView2Environment9>()?
            .CreateContextMenuItem(
                label,
                None::<&IRandomAccessStream>,
                CoreWebView2ContextMenuItemKind::Command,
            )?;
        let tab = Rc::downgrade(self);
        item.CustomItemSelected(move |_, _| {
            let (tab, url) = (tab.clone(), url.clone());
            // After the menu has closed, not from inside its event.
            exec::spawn(async move {
                if let Some(tab) = tab.upgrade()
                    && let Some(window) = tab.window()
                {
                    window.open_tab_from(tab.id, Initial::Url(url), false);
                }
            });
        })?
        .forget();
        Ok(item)
    }

    pub fn permissions(&self) -> &TabPermissions {
        &self.permissions
    }

    /// The committed page's site; `None` for an opaque origin (`data:`, `about:`, `file:`).
    pub fn origin(&self) -> Option<Origin> {
        Origin::parse(&self.state.borrow().url)
    }

    fn permission_requested(self: &Rc<Self>, args: &CoreWebView2PermissionRequestedEventArgs) {
        let Some(window) = self.window() else { return };
        let Some(browser) = window.browser() else {
            return;
        };
        match self.permissions.request(&browser, args) {
            // The prompt opens on the next turn, not inside the engine's event.
            Ok(Requested::Waiting) => exec::spawn(async move { window.show_permission_prompt() }),
            Ok(Requested::Settled) => {}
            Err(e) => log::warn!("tab {}: permission request: {e}", self.id),
        }
        self.watch_capture();
    }

    fn screen_capture_starting(self: &Rc<Self>, args: &CoreWebView2ScreenCaptureStartingEventArgs) {
        let Some(browser) = self.browser() else {
            return;
        };
        let source = args
            .OriginalSourceFrameInfo()
            .and_then(|frame| frame.Source())
            .ok()
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| self.state().url);
        let origin = Origin::parse(&source);
        if self
            .permissions
            .blocks(&browser, origin.as_ref(), Permission::ScreenShare)
        {
            log::info!("tab {}: screen sharing is blocked for {source}", self.id);
            if let Err(e) = args.SetCancel(true) {
                log::warn!("tab {}: cancel screen capture: {e}", self.id);
            }
            return;
        }
        self.permissions.watch();
        self.watch_capture();
    }

    /// Polls what the page captures while a capture may be live (see `capturing`).
    pub fn watch_capture(self: &Rc<Self>) {
        if !self.permissions.watched() || self.capture_polled.replace(true) {
            return;
        }
        let tab = self.clone();
        exec::spawn(async move {
            while !tab.closed.get() && tab.permissions.watched() {
                tab.poll_capture().await;
                exec::sleep(CAPTURE_POLL).await;
            }
            tab.capture_polled.set(false);
        });
    }

    async fn poll_capture(&self) {
        // A page between documents cannot answer; the next poll reads the new one.
        let Ok(json) = self.eval(capturing::STATE_SCRIPT).await else {
            return;
        };
        if self
            .permissions
            .set_capturing(capturing::parse_state(&json))
        {
            self.notify();
        }
    }

    /// Ends the page's capture that `permission` governs: a Stop button, or a block while in use.
    pub fn stop_capture(self: &Rc<Self>, permission: Permission) {
        let Some(script) = capturing::stop_script(permission) else {
            return;
        };
        let tab = self.clone();
        exec::spawn(async move {
            if let Err(e) = tab.eval(&script).await {
                log::debug!("tab {}: stop {permission:?}: {e}", tab.id);
            }
            tab.poll_capture().await;
        });
    }

    fn binding_called(&self, args: &CoreWebView2DevToolsProtocolEventReceivedEventArgs) {
        let Some(window) = self.window() else { return };
        let Ok(event) = args.ParameterObjectAsJson() else {
            return;
        };
        let in_frame = !session_of(args).is_empty();
        match shortcuts::parse_binding_call(&event, &shortcuts::current(), in_frame) {
            // Runs on the next turn: the command may close this tab's web view (Ctrl+W).
            Some(PageMessage::Key(command)) => exec::spawn(async move { window.run(command) }),
            Some(PageMessage::BackgroundLink(url)) => {
                *self.background_link.borrow_mut() = Some((url, Instant::now()));
            }
            Some(PageMessage::Store(request)) => {
                exec::spawn(store::answer(window, self.id, request));
            }
            Some(PageMessage::Zoom { ratio, start }) => {
                let scale = window.scale();
                let level = zoom::Level(zoom::percent(ratio, scale));
                if self.state.borrow().zoom != level {
                    log::debug!("tab {}: zoom {}", self.id, level.label());
                    self.state.borrow_mut().zoom = level;
                    self.notify();
                }
                let report = if start {
                    zoom::Report::Start(level)
                } else {
                    zoom::Report::Change(level)
                };
                self.zoom_reported(&window, report, scale);
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

/// `source` as the tab shows it: the `view-source:` address the shell loaded while that is the
/// page WebView2 reports.
fn shown_url(source: String, view_source: Option<&str>) -> String {
    match view_source {
        Some(shown) if view_source::viewed_url(shown) == Some(source.as_str()) => shown.to_owned(),
        _ => source,
    }
}

/// Where the context menu item called `name` is.
fn item_named(
    items: &windows_collections::IVector<CoreWebView2ContextMenuItem>,
    name: &str,
) -> Result<Option<u32>> {
    Ok((0..items.Size()?).find(|&i| {
        items
            .GetAt(i)
            .and_then(|m| m.Name())
            .is_ok_and(|n| n == name)
    }))
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

/// Calls a DevTools method in `session`: the page's own when it is empty, else one `AUTO_ATTACH`
/// attached.
pub(crate) async fn devtools_in(
    core: &CoreWebView2,
    session: &str,
    method: &str,
    params: &str,
) -> Result<String> {
    Ok(call_in(core, session, method, params)?.await?.to_string_lossy())
}

/// [`devtools_in`]'s call, which goes out as it is made.
pub(crate) fn call_in(
    core: &CoreWebView2,
    session: &str,
    method: &str,
    params: &str,
) -> Result<IAsyncOperation<HSTRING>> {
    if session.is_empty() {
        core.CallDevToolsProtocolMethodAsync(method, params)
    } else {
        core.cast::<ICoreWebView2_11>()?
            .CallDevToolsProtocolMethodForSessionAsync(session, method, params)
    }
}

/// The DevTools session an event came from; empty for the page's own.
fn session_of(args: &CoreWebView2DevToolsProtocolEventReceivedEventArgs) -> String {
    args.cast::<ICoreWebView2DevToolsProtocolEventReceivedEventArgs2>()
        .and_then(|args| args.SessionId())
        .unwrap_or_default()
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

/// `NavigationCompleted`, whether or not the navigation committed. One that committed nothing in
/// a tab that has shown no page (a download, Stop) leaves no URL for a saved session to load
/// again; an error page commits.
fn navigation_ended(state: &mut TabState, requested: &mut String) {
    if state.load != Load::Committed && state.url.is_empty() {
        requested.clear();
    }
    state.load = Load::Idle;
}

/// How the navigation the shell started came about, until it commits or ends without a new
/// document (a download, Stop, an error). The events here carry no navigation ids: the shell's
/// navigation is the first to start after it asked, and one already under way then ends first.
#[derive(Default)]
struct PendingTransition {
    transition: Option<Transition>,
    /// The shell's navigation has started.
    started: bool,
    /// Navigations still to end that were under way when the shell asked.
    superseded: u32,
}

impl PendingTransition {
    /// The shell navigates; `loading`: a navigation is under way.
    fn requested(&mut self, transition: Transition, loading: bool) {
        // An earlier request that has not started yet ends too, superseded.
        let waiting = self.transition.is_some() && !self.started;
        *self = Self {
            transition: Some(transition),
            started: false,
            superseded: u32::from(loading) + u32::from(waiting),
        };
    }

    /// `NavigationStarting`, also for each redirect.
    fn starting(&mut self) {
        self.started = self.transition.is_some();
    }

    /// A new document commits.
    fn new_document(&mut self) -> Transition {
        if !self.started {
            return Transition::Link;
        }
        std::mem::take(self).transition.unwrap_or(Transition::Link)
    }

    /// A same-document commit: a typed fragment, which never starts, or the page's own.
    fn same_document(&mut self) -> Transition {
        if self.started {
            return Transition::Link;
        }
        std::mem::take(self).transition.unwrap_or(Transition::Link)
    }

    /// `NavigationCompleted`, whether or not it committed.
    fn completed(&mut self) {
        if self.superseded > 0 {
            self.superseded -= 1;
        } else if self.started {
            *self = Self::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Load, PendingTransition, TabState, Transition, display_title, navigation_ended, shown_url,
    };

    #[test]
    fn a_view_source_tab_shows_its_view_source_address_while_on_that_page() {
        let shown = Some("view-source:http://e.test/a.html");
        assert_eq!(
            shown_url("http://e.test/a.html".to_owned(), shown),
            "view-source:http://e.test/a.html"
        );
        assert_eq!(
            shown_url("http://e.test/b.html".to_owned(), shown),
            "http://e.test/b.html"
        );
        assert_eq!(
            shown_url("http://e.test/a.html".to_owned(), None),
            "http://e.test/a.html"
        );
    }

    #[test]
    fn a_first_navigation_that_never_commits_leaves_no_url_to_restore() {
        let mut state = TabState {
            load: Load::Started,
            ..TabState::default()
        };
        let mut requested = "https://e.test/file.bin".to_owned();
        navigation_ended(&mut state, &mut requested);
        assert_eq!((state.load, requested.as_str()), (Load::Idle, ""));

        let mut state = TabState {
            load: Load::Committed,
            ..TabState::default()
        };
        let mut requested = "data:text/html,page".to_owned();
        navigation_ended(&mut state, &mut requested);
        assert_eq!(requested, "data:text/html,page");
    }

    #[test]
    fn a_typed_address_commits_as_typed_through_redirects() {
        let mut t = PendingTransition::default();
        t.requested(Transition::Typed, false);
        t.starting();
        t.starting();
        assert_eq!(t.new_document(), Transition::Typed);
        t.completed();
        t.starting();
        assert_eq!(t.new_document(), Transition::Link);
    }

    #[test]
    fn a_typed_download_does_not_mark_the_next_link() {
        let mut t = PendingTransition::default();
        t.requested(Transition::Typed, false);
        t.starting();
        t.completed();
        t.starting();
        assert_eq!(t.new_document(), Transition::Link);
    }

    #[test]
    fn the_load_a_typed_address_replaces_ends_unrelated() {
        let mut t = PendingTransition::default();
        t.requested(Transition::Typed, true);
        t.starting();
        t.completed();
        assert_eq!(t.new_document(), Transition::Typed);
    }

    #[test]
    fn a_page_commit_before_the_typed_navigation_starts_is_a_link() {
        let mut t = PendingTransition::default();
        t.requested(Transition::Bookmark, true);
        assert_eq!(t.new_document(), Transition::Link);
        t.completed();
        t.starting();
        assert_eq!(t.new_document(), Transition::Bookmark);
    }

    #[test]
    fn a_typed_fragment_is_typed_once() {
        let mut t = PendingTransition::default();
        t.requested(Transition::Typed, false);
        assert_eq!(t.same_document(), Transition::Typed);
        t.starting();
        assert_eq!(t.new_document(), Transition::Link);
    }

    #[test]
    fn the_page_changing_its_address_during_a_typed_navigation_is_a_link() {
        let mut t = PendingTransition::default();
        t.requested(Transition::Typed, false);
        t.starting();
        assert_eq!(t.same_document(), Transition::Link);
        assert_eq!(t.new_document(), Transition::Typed);
    }

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
