//! Owned by W1.6.
//! Where each `BrowserEvent` goes (UA section 4.3). Engine handlers only queue events (section 1.6
//! rule 1); this task delivers them on main after the pump step that queued them, one GPUI update
//! each. The bridge keeps what is its own (focus, prompts, requests); the later features get theirs
//! through their own files' entry points; the rest goes to the shell through [`ShellRoutes`].

use std::ffi::OsString;
use std::path::PathBuf;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::App;
use vsesvit_engine::{BrowserEvent, OpenDisposition, RoutedKey, TabEvent, TabId, Url};

/// The shell's half of the router: the window and tab registries, and shutdown. Installed with
/// `engine_bridge::start`; every method runs inside a GPUI update.
pub trait ShellRoutes {
    /// Every event of one tab (the focus policy has seen it first).
    fn tab(&self, tab: TabId, event: TabEvent, cx: &mut App);
    /// A popup the hooks allowed was created; the shell adopts it as a tab.
    fn popup_opened(&self, opener: TabId, tab: TabId, url: Url, disposition: OpenDisposition, cx: &mut App);
    /// A page asks for a URL elsewhere (middle click, Ctrl+click).
    fn open_url(&self, opener: TabId, url: Url, disposition: OpenDisposition, user_gesture: bool, cx: &mut App);
    /// A routed key was pressed while a page had the keyboard; `keymap::action_for` names its action.
    fn shortcut(&self, tab: Option<TabId>, key: RoutedKey, cx: &mut App);
    /// A second start reached the engine's own singleton.
    fn app_relaunch(&self, argv: Vec<OsString>, cwd: PathBuf, cx: &mut App);
    /// Shutdown phase one ended (design section 3.6 step 3).
    fn user_browsers_closed(&self, cx: &mut App);
    /// The live-browser count reached zero (step 6).
    fn all_browsers_closed(&self, cx: &mut App);
}

pub(crate) fn start(cx: &mut App, mut events: UnboundedReceiver<BrowserEvent>) {
    cx.spawn(async move |cx| {
        while let Some(event) = events.next().await {
            cx.update(|cx| route(event, cx));
        }
    })
    .detach();
}

/// Delivers one event.
pub fn route(event: BrowserEvent, cx: &mut App) {
    let bridge = super::bridge(cx);
    let (focus, routes) = (bridge.focus.clone(), bridge.routes.clone());
    match event {
        BrowserEvent::Tab(tab, event) => {
            match &event {
                TabEvent::Created { native } => focus.created(tab, *native),
                TabEvent::FocusChanged { focused: true } => focus.page_took_focus(tab),
                TabEvent::TakeFocus { .. } => focus.page_gives_up_keyboard(tab),
                TabEvent::Closed | TabEvent::Detached => focus.gone(tab),
                _ => {}
            }
            routes.tab(tab, event, cx);
        }
        BrowserEvent::PopupOpened { opener, tab, url, disposition } => routes.popup_opened(opener, tab, url, disposition, cx),
        BrowserEvent::OpenUrlRequested { opener, url, disposition, user_gesture } => {
            routes.open_url(opener, url, disposition, user_gesture, cx)
        }
        BrowserEvent::Shortcut { tab, key } => routes.shortcut(tab, key, cx),
        BrowserEvent::Prompt { id, request } => super::prompts::opened(id, request, cx),
        BrowserEvent::PromptEnded { id, why } => super::prompts::ended(id, why, cx),
        BrowserEvent::DownloadRequested(request) => crate::downloads::controller::requested(request, cx),
        BrowserEvent::DownloadUpdated(update) => crate::downloads::controller::updated(update, cx),
        BrowserEvent::RequestFinished { id, result } => super::requests::complete(id, result, cx),
        BrowserEvent::PageMessage { tab, origin, main_frame, channel, payload } => {
            crate::extensions::controller::page_message(tab, origin, main_frame, channel, payload, cx)
        }
        BrowserEvent::Extension(event) => crate::extensions::controller::extension_event(event, cx),
        BrowserEvent::ForeignWindow(event) => crate::extensions::controller::foreign_window(event, cx),
        BrowserEvent::UserBrowsersClosed => routes.user_browsers_closed(cx),
        BrowserEvent::AllBrowsersClosed => routes.all_browsers_closed(cx),
        BrowserEvent::AppRelaunch { argv, cwd } => routes.app_relaunch(argv, cwd, cx),
        BrowserEvent::EngineError(error) => log::error!("engine: {error}"),
    }
}
