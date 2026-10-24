//! Owned by W1.5.
//! One tab: the shell's mirror of its engine browser, fed by the router through `apply`, and the
//! navigation calls the chrome makes on it (UA sections 1.4 and 5.2). Owned by `BrowserState`, held
//! by exactly one window's `TabListModel`.

use std::sync::Arc;

use gpui::{Context, EventEmitter, Image, ImageFormat, SharedString};
use vsesvit_core::Url;
use vsesvit_core::history::Transition;
use vsesvit_core::private::Browsing;
use vsesvit_engine::{
    Capturing, CloseMode, CommitKind, EngineError, FindResult, InitialPage, NativeHandle, PhysRect, SslInfo, TabEvent,
    TabId, TabKind, TabSpec, Zoom,
};

use crate::app::shell::Shell;

pub mod content;
pub mod error_page;
pub mod model;
pub mod navigation;
pub mod permissions;
pub mod prompts;
pub mod sleep;

pub use navigation::Security;

#[derive(Clone, Debug, PartialEq)]
pub enum TabChanged {
    Title,
    Favicon,
    Loading,
    Progress,
    Committed(CommitKind),
    History,
    Zoom,
    Find(FindResult),
    Capture,
    Audio,
    Security,
    Fullscreen(bool),
    FocusChanged(bool),
    TakeFocus { next: bool },
    /// The page closed itself (`window.close()`): the tab ends.
    ClosedByPage,
    /// The close or park the shell asked for is done: the tab ends.
    EngineClosed,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TabIcon {
    Page(Arc<Image>),
    Default,
}

/// Where the tab's engine browser is in its life.
#[derive(Clone, Debug, PartialEq)]
enum Life {
    /// No browser yet; it will show `page` once its window creates one.
    Unborn(InitialPage),
    Creating,
    Live(NativeHandle),
    /// `close_tab` was asked; `Closed` ends it, unless the page's `beforeunload` keeps it open.
    Closing(Option<NativeHandle>),
    /// Parked in the hidden host; `Detached` ends it.
    Parking,
    Ended,
}

/// A committed visit, for the history hook.
#[derive(Clone, Debug, PartialEq)]
pub struct Visit {
    pub url: Url,
    pub transition: Transition,
    pub kind: CommitKind,
}

pub struct Tab {
    id: TabId,
    browsing: Browsing,
    opener: Option<TabId>,
    session_id: vsesvit_core::session::TabId,
    life: Life,
    page_title: String,
    /// A restored tab's saved title, shown until its page reports one.
    restored_title: String,
    committed: Option<Url>,
    /// The address being loaded, before it commits.
    requested: Option<Url>,
    error_page: bool,
    ssl: Option<SslInfo>,
    loading: bool,
    progress: f32,
    can_go_back: bool,
    can_go_forward: bool,
    zoom: Zoom,
    muted: bool,
    audible: bool,
    capturing: Capturing,
    favicon: Option<Arc<Image>>,
    find: Option<FindResult>,
    used: u64,
    last_active_ms: i64,
    typed: Option<String>,
    /// How the next commit was reached, when the shell knows better than the engine.
    pending_transition: Option<Transition>,
    visit: Option<Visit>,
}

impl EventEmitter<TabChanged> for Tab {}

impl Tab {
    pub fn new(id: TabId, browsing: Browsing, page: InitialPage, opener: Option<TabId>) -> Tab {
        let requested = match &page {
            InitialPage::Url(url) => Some(url.clone()),
            InitialPage::Html { .. } | InitialPage::Blank => None,
        };
        Tab {
            id,
            browsing,
            opener,
            session_id: vsesvit_core::session::TabId::new(),
            life: Life::Unborn(page),
            page_title: String::new(),
            restored_title: String::new(),
            committed: None,
            requested,
            error_page: false,
            ssl: None,
            loading: false,
            progress: 0.0,
            can_go_back: false,
            can_go_forward: false,
            zoom: Zoom(1.0),
            muted: false,
            audible: false,
            capturing: Capturing::default(),
            favicon: None,
            find: None,
            used: 0,
            last_active_ms: 0,
            typed: None,
            pending_transition: None,
            visit: None,
        }
    }

    pub fn id(&self) -> TabId {
        self.id
    }

    pub fn browsing(&self) -> Browsing {
        self.browsing
    }

    pub fn opener(&self) -> Option<TabId> {
        self.opener
    }

    pub fn session_id(&self) -> vsesvit_core::session::TabId {
        self.session_id
    }

    /// A restored tab keeps its saved id, so the session keeps naming the same tab, and shows its
    /// saved title until its page reports one.
    pub fn restored(&mut self, session_id: vsesvit_core::session::TabId, title: String) {
        self.session_id = session_id;
        self.restored_title = title;
    }

    pub fn display_title(&self) -> SharedString {
        let title = if self.page_title.trim().is_empty() && !self.restored_title.is_empty() {
            self.restored_title.clone()
        } else {
            navigation::display_title(&self.page_title, self.committed.as_ref())
        };
        title.into()
    }

    pub fn page_title(&self) -> &str {
        &self.page_title
    }

    pub fn committed_url(&self) -> Option<&Url> {
        self.committed.as_ref()
    }

    pub fn session_url(&self) -> Option<Url> {
        navigation::session_url(self.committed.as_ref(), self.requested.as_ref())
    }

    /// The address on screen, for copying: none for a blank tab or the new tab page.
    pub fn link(&self) -> Option<&Url> {
        self.committed.as_ref().filter(|url| !navigation::is_blank_url(url))
    }

    /// Nothing requested or shown yet: a new tab waiting for an address.
    pub fn is_blank(&self) -> bool {
        self.committed.as_ref().or(self.requested.as_ref()).is_none_or(navigation::is_blank_url)
    }

    pub fn shows_error_page(&self) -> bool {
        self.error_page
    }

    pub fn icon(&self) -> TabIcon {
        self.favicon.clone().map_or(TabIcon::Default, TabIcon::Page)
    }

    pub fn security(&self) -> Security {
        navigation::security(self.committed.as_ref(), self.error_page, self.ssl.as_ref())
    }

    pub fn ssl(&self) -> Option<&SslInfo> {
        self.ssl.as_ref()
    }

    pub fn loading(&self) -> bool {
        self.loading
    }

    pub fn progress(&self) -> f32 {
        self.progress
    }

    pub fn can_go_back(&self) -> bool {
        self.can_go_back
    }

    pub fn can_go_forward(&self) -> bool {
        self.can_go_forward
    }

    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn audible(&self) -> bool {
        self.audible
    }

    pub fn capturing(&self) -> Capturing {
        self.capturing
    }

    pub fn find_result(&self) -> Option<FindResult> {
        self.find
    }

    /// The page's native window, while its browser lives in a window.
    pub fn native(&self) -> Option<NativeHandle> {
        match self.life {
            Life::Live(native) => Some(native),
            _ => None,
        }
    }

    /// The shell asked the engine to close or park it, or it already ended.
    pub fn is_ending(&self) -> bool {
        matches!(self.life, Life::Closing(_) | Life::Parking | Life::Ended)
    }

    /// The shell's use clock when the tab was last opened or selected: tab search lists it by this.
    pub fn used(&self) -> u64 {
        self.used
    }

    pub fn set_used(&mut self, tick: u64) {
        self.used = tick;
    }

    pub fn last_active_ms(&self) -> i64 {
        self.last_active_ms
    }

    pub fn mark_active(&mut self, ms: i64) {
        self.last_active_ms = ms;
    }

    /// Address-bar text typed in this tab but not submitted, kept while another tab is shown.
    pub fn set_typed(&mut self, text: Option<String>) {
        self.typed = text;
    }

    pub fn take_typed(&mut self) -> Option<String> {
        self.typed.take()
    }

    pub fn pending_transition(&self) -> Option<Transition> {
        self.pending_transition
    }

    /// The last commit, once: the history hook records it.
    pub fn take_visit(&mut self) -> Option<Visit> {
        self.visit.take()
    }

    /// Creates the tab's browser in its window: `parent` is the window's content host. The window
    /// calls it once it can place the page.
    pub fn create_browser(
        &mut self,
        parent: NativeHandle,
        bounds: PhysRect,
        scale_factor: f32,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), EngineError> {
        let Life::Unborn(page) = &self.life else { return Ok(()) };
        let spec = TabSpec {
            tab: self.id,
            kind: TabKind::Page,
            browsing: self.browsing,
            parent,
            bounds,
            scale_factor,
            visible,
            page: page.clone(),
            opener: self.opener,
        };
        Shell::engine(cx).create_tab(spec)?;
        self.life = Life::Creating;
        Ok(())
    }

    /// Loads `url`; `transition` says how the user reached it when the engine cannot know.
    pub fn load(&mut self, url: Url, transition: Option<Transition>, cx: &mut Context<Self>) {
        self.pending_transition = transition;
        self.requested = Some(url.clone());
        match &mut self.life {
            Life::Unborn(page) => *page = InitialPage::Url(url),
            Life::Creating | Life::Live(_) => Shell::engine(cx).load(self.id, &url),
            Life::Closing(_) | Life::Parking | Life::Ended => {}
        }
    }

    /// The new tab page, at `about:blank`.
    pub fn load_new_tab_page(&mut self, cx: &mut Context<Self>) {
        let page = vsesvit_core::new_tab::page(&mut Shell::core(cx).borrow_mut(), self.browsing);
        let html = match page {
            Ok(html) => html,
            Err(e) => {
                log::warn!("new tab page: {e}");
                return;
            }
        };
        self.pending_transition = None;
        self.requested = None;
        match &mut self.life {
            Life::Unborn(page) => *page = InitialPage::Html { html, shown_url: None },
            Life::Creating | Life::Live(_) => Shell::engine(cx).load_html(self.id, html, None),
            Life::Closing(_) | Life::Parking | Life::Ended => {}
        }
    }

    pub fn reload(&mut self, bypass_cache: bool, cx: &mut Context<Self>) {
        if let Some(id) = self.engine_tab() {
            Shell::engine(cx).reload(id, bypass_cache);
        }
    }

    /// A stopped load commits nothing, so the transition set for it must not go to a later visit.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.pending_transition = None;
        if let Some(id) = self.engine_tab() {
            Shell::engine(cx).stop(id);
        }
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.engine_tab() {
            Shell::engine(cx).go_back(id);
        }
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.engine_tab() {
            Shell::engine(cx).go_forward(id);
        }
    }

    pub fn set_muted(&mut self, muted: bool, cx: &mut Context<Self>) {
        self.muted = muted;
        if let Some(id) = self.engine_tab() {
            Shell::engine(cx).set_audio_muted(id, muted);
        }
        cx.emit(TabChanged::Audio);
    }

    /// Asks the engine to close the browser, running the page's `beforeunload`. Returns false when
    /// there is no browser to wait for, so the tab ends now.
    pub(crate) fn close_browser(&mut self, cx: &mut Context<Self>) -> bool {
        match self.life {
            Life::Creating | Life::Live(_) => {
                self.life = Life::Closing(self.native());
                Shell::engine(cx).close_tab(self.id, CloseMode::RunUnloadHandlers);
                true
            }
            Life::Closing(_) | Life::Parking => true,
            Life::Unborn(_) | Life::Ended => {
                self.life = Life::Ended;
                false
            }
        }
    }

    /// The page's `beforeunload` kept it open: the browser lives on as before.
    pub(crate) fn close_refused(&mut self) {
        if let Life::Closing(native) = self.life {
            self.life = native.map_or(Life::Creating, Life::Live);
        }
    }

    /// The page was moved into the hidden host and the engine detached it (design section 4.2).
    pub(crate) fn parked(&mut self) {
        self.life = Life::Parking;
    }

    /// The router's entry point.
    pub fn apply(&mut self, event: TabEvent, cx: &mut Context<Self>) {
        match event {
            TabEvent::Created { native } => match self.life {
                Life::Creating => self.life = Life::Live(native),
                Life::Closing(None) => self.life = Life::Closing(Some(native)),
                _ => {}
            },
            TabEvent::Detached | TabEvent::Closed => {
                let requested = matches!(self.life, Life::Closing(_) | Life::Parking);
                self.life = Life::Ended;
                cx.emit(if requested { TabChanged::EngineClosed } else { TabChanged::ClosedByPage });
            }
            TabEvent::TitleChanged(title) => {
                self.page_title = title;
                cx.emit(TabChanged::Title);
            }
            TabEvent::AddressChanged(url) => {
                if self.committed.as_ref() != Some(&url) {
                    self.committed = Some(url);
                    cx.emit(TabChanged::Security);
                }
            }
            TabEvent::FaviconChanged(png) => {
                self.favicon = png.map(|png| Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec())));
                cx.emit(TabChanged::Favicon);
            }
            TabEvent::LoadingStateChanged { loading, can_go_back, can_go_forward } => {
                if !loading {
                    // A load that ends without a commit (a download, a stop) leaves no transition,
                    // and the session keeps the page on screen, not the address that never came.
                    self.pending_transition = None;
                    self.requested = None;
                }
                self.loading = loading;
                if (self.can_go_back, self.can_go_forward) != (can_go_back, can_go_forward) {
                    (self.can_go_back, self.can_go_forward) = (can_go_back, can_go_forward);
                    cx.emit(TabChanged::History);
                }
                cx.emit(TabChanged::Loading);
            }
            TabEvent::LoadProgress(progress) => {
                self.progress = progress;
                cx.emit(TabChanged::Progress);
            }
            TabEvent::NavigationStarted { url, .. } => self.requested = Some(url),
            TabEvent::Committed { url, kind, transition, .. } => {
                let transition = navigation::history_transition(self.pending_transition.take(), transition);
                if kind != CommitKind::SameDocument {
                    self.error_page = kind == CommitKind::ErrorPage;
                    self.ssl = None;
                    self.page_title.clear();
                    self.find = None;
                }
                self.restored_title.clear();
                self.committed = Some(url.clone());
                self.visit = Some(Visit { url, transition, kind });
                cx.emit(TabChanged::Committed(kind));
            }
            TabEvent::SecurityChanged(ssl) => {
                self.ssl = ssl;
                cx.emit(TabChanged::Security);
            }
            TabEvent::FindResult(result) => {
                self.find = Some(result);
                cx.emit(TabChanged::Find(result));
            }
            TabEvent::ZoomChanged(zoom) => {
                self.zoom = zoom;
                cx.emit(TabChanged::Zoom);
            }
            TabEvent::AudioStateChanged { audible } => {
                self.audible = audible;
                cx.emit(TabChanged::Audio);
            }
            TabEvent::CaptureChanged(capturing) => {
                self.capturing = capturing;
                cx.emit(TabChanged::Capture);
            }
            TabEvent::FullscreenRequested(on) => cx.emit(TabChanged::Fullscreen(on)),
            TabEvent::FocusChanged { focused } => cx.emit(TabChanged::FocusChanged(focused)),
            TabEvent::TakeFocus { next } => cx.emit(TabChanged::TakeFocus { next }),
            // Error pages (W2.5), crashed pages (W2.5), certificate errors (W2.5), the status text
            // and extension popups' sizes (W4.x) are their owners' to show.
            TabEvent::LoadFailed { .. }
            | TabEvent::CertificateError { .. }
            | TabEvent::RenderProcessGone(_)
            | TabEvent::StatusText(_)
            | TabEvent::PreferredSize { .. } => {}
        }
    }

    /// The id to give the engine, while there is a browser to take the call.
    fn engine_tab(&self) -> Option<TabId> {
        matches!(self.life, Life::Creating | Life::Live(_)).then_some(self.id)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{AppContext, Entity, TestAppContext};
    use vsesvit_engine::EngineTransition;

    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    /// A tab with no browser, fed events by hand.
    fn tab(cx: &mut TestAppContext) -> Entity<Tab> {
        cx.update(|cx| cx.new(|_| Tab::new(TabId::next(), Browsing::Normal, InitialPage::Blank, None)))
    }

    fn commit(cx: &mut TestAppContext, tab: &Entity<Tab>, to: &str) -> Option<Visit> {
        let event =
            TabEvent::Committed { url: url(to), kind: CommitKind::Document, transition: EngineTransition::Link, mime: None };
        tab.update(cx, |tab, cx| {
            tab.apply(event, cx);
            tab.take_visit()
        })
    }

    fn loading(cx: &mut TestAppContext, tab: &Entity<Tab>, loading: bool) {
        let event = TabEvent::LoadingStateChanged { loading, can_go_back: false, can_go_forward: false };
        tab.update(cx, |tab, cx| tab.apply(event, cx));
    }

    #[gpui::test]
    fn a_stopped_load_leaves_no_transition_behind(cx: &mut TestAppContext) {
        let tab = tab(cx);
        tab.update(cx, |tab, cx| {
            tab.load(url("https://a.test/"), Some(Transition::Typed), cx);
            tab.stop(cx);
        });
        assert_eq!(commit(cx, &tab, "https://b.test/").map(|v| v.transition), Some(Transition::Link));
    }

    #[gpui::test]
    fn a_load_that_ends_without_a_commit_leaves_no_transition_behind(cx: &mut TestAppContext) {
        let tab = tab(cx);
        tab.update(cx, |tab, cx| tab.load(url("https://a.test/file.zip"), Some(Transition::Typed), cx));
        loading(cx, &tab, true);
        loading(cx, &tab, false);
        assert_eq!(tab.read_with(cx, |t, _| t.pending_transition()), None, "the download kept the page");
        assert_eq!(tab.read_with(cx, |t, _| t.session_url()), None, "and the session does not save the download");
        assert_eq!(commit(cx, &tab, "https://b.test/").map(|v| v.transition), Some(Transition::Link));
    }

    #[gpui::test]
    fn an_address_typed_over_a_load_still_counts_as_typed(cx: &mut TestAppContext) {
        let tab = tab(cx);
        loading(cx, &tab, true);
        tab.update(cx, |tab, cx| tab.load(url("https://typed.test/"), Some(Transition::Typed), cx));
        assert_eq!(commit(cx, &tab, "https://typed.test/").map(|v| v.transition), Some(Transition::Typed));
    }

    #[gpui::test]
    fn a_tab_before_its_first_commit_is_saved_by_the_address_it_loads(cx: &mut TestAppContext) {
        let tab = tab(cx);
        assert!(tab.read_with(cx, |t, _| t.is_blank() && t.session_url().is_none()));
        tab.update(cx, |tab, cx| tab.load(url("https://a.test/"), None, cx));
        assert!(tab.read_with(cx, |t, _| !t.is_blank()));
        assert_eq!(tab.read_with(cx, |t, _| t.session_url()), Some(url("https://a.test/")));
        assert_eq!(tab.read_with(cx, |t, _| t.display_title().to_string()), "New Tab");
        commit(cx, &tab, "https://a.test/");
        tab.update(cx, |tab, cx| tab.apply(TabEvent::TitleChanged("A".into()), cx));
        assert_eq!(tab.read_with(cx, |t, _| t.display_title().to_string()), "A");
    }

    #[gpui::test]
    fn a_close_the_shell_asked_for_ends_the_tab_and_one_it_did_not_is_the_pages(cx: &mut TestAppContext) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let watch = |cx: &mut TestAppContext, tab: &Entity<Tab>| {
            let seen = events.clone();
            cx.update(|cx| cx.subscribe(tab, move |_, e: &TabChanged, _| seen.borrow_mut().push(e.clone())).detach());
        };
        let asked = tab(cx);
        watch(cx, &asked);
        asked.update(cx, |tab, cx| {
            tab.parked();
            tab.apply(TabEvent::Detached, cx);
        });
        let unasked = tab(cx);
        watch(cx, &unasked);
        unasked.update(cx, |tab, cx| tab.apply(TabEvent::Closed, cx));
        cx.run_until_parked();
        assert_eq!(*events.borrow(), [TabChanged::EngineClosed, TabChanged::ClosedByPage]);
    }
}
