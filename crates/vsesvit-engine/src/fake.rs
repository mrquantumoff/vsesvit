//! Owned by W0.2.
//! `FakeEngine`: an in-memory engine for the shell's tests (UA section 4.5). It records every command,
//! keeps a model of its browsers (tabs, parked and hidden ones, with the live count of design section
//! 3.6), runs the prompt protocol through [`PromptLedger`], answers request ids with scripted results,
//! and runs the shell's hooks when a test simulates the page. Events go to the receiver `new` returns.
//!
//! Creation and closing finish at once unless a test holds them per tab (`hold_creation`,
//! `hold_close`), which models a browser whose engine callback arrives later. Navigations commit at
//! once. The page-focus policy is only recorded: refusing Chromium's own focus requests is the glue's
//! `on_set_focus`, which a test asks about with `engine_focus_request`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::command::Engine;
#[cfg(feature = "self-test")]
use crate::command::TestShutdown;
use crate::event::{BrowserEvent, TabEvent};
use crate::extension::{ExtensionCall, ExtensionEvent, ExtensionId, ExtensionReply, ForeignId, ForeignWindowEvent};
use crate::factory::{EngineConfig, EngineDriver, EngineFactory, EngineInitError, EngineParts};
use crate::focus::PageFocus;
use crate::hooks::{NeutralizeFailed, ShellHooks};
use crate::ids::{EngineDownloadId, NativeHandle, ParkedId, PromptId, RequestId, TabId};
use crate::prompt::{PromptAnswer, PromptEnd, PromptError, PromptLedger, PromptRequest, PromptScope, Refused, ScopeState};
use crate::types::{
    Browsing, Chord, CloseMode, CommitKind, EngineError, EnginePrefs, EngineTransition, EngineVersions,
    InitialPage, KeyRouting, NavigationDecision, NavigationRequest, Origin, PagePoint,
    Permission, PhysRect, PopupDecision, PopupRequest, RequestFilter, RequestResult, Setting, TabKind, TabSpec,
    Url, Zoom,
};

#[cfg(test)]
mod tests;

/// The Chromium version the fake reports, the one the design pins (design section 2.1).
const CHROMIUM: &str = "154.0.8037.98";

/// A command the shell gave the engine, in call order. Queries (`versions`, `zoom`, `has_devtools`) are
/// not recorded. Tuple fields follow the method's parameters.
#[derive(Clone, Debug, PartialEq)]
pub enum FakeCommand {
    /// `set_hooks`.
    SetHooks,
    /// `set_key_routing`.
    SetKeyRouting(KeyRouting),
    /// `apply_prefs`.
    ApplyPrefs(EnginePrefs),
    /// `set_request_filter`.
    SetRequestFilter,
    /// `set_document_start_scripts`.
    SetDocumentStartScripts(Vec<String>),
    /// `set_site_setting`.
    SetSiteSetting(Browsing, Origin, Permission, Option<Setting>),
    /// `begin_private_context`.
    BeginPrivateContext,
    /// `end_private_context`.
    EndPrivateContext,
    /// `set_hidden_host`.
    SetHiddenHost(NativeHandle),
    /// `create_tab`, refused or not.
    CreateTab(TabSpec),
    /// `close_tab`.
    CloseTab(TabId, CloseMode),
    /// `set_visible`.
    SetVisible(TabId, bool),
    /// `notify_moved`.
    NotifyMoved(TabId),
    /// `notify_screen_info_changed`.
    NotifyScreenInfoChanged(TabId),
    /// `set_page_focus`.
    SetPageFocus(TabId, PageFocus),
    /// `set_frozen`.
    SetFrozen(TabId, bool),
    /// `focus_page`.
    FocusPage(TabId),
    /// `blur_page`.
    BlurPage(TabId),
    /// `detach_tab`.
    DetachTab(TabId),
    /// `begin_shutdown`.
    BeginShutdown,
    /// `close_internal`.
    CloseInternal,
    /// `load`.
    Load(TabId, Url),
    /// `load_html`: tab, HTML, shown URL.
    LoadHtml(TabId, String, Option<Url>),
    /// `reload`: tab, bypass cache.
    Reload(TabId, bool),
    /// `stop`.
    Stop(TabId),
    /// `go_back`.
    GoBack(TabId),
    /// `go_forward`.
    GoForward(TabId),
    /// `find`: tab, text, forward, find next.
    Find(TabId, String, bool, bool),
    /// `stop_finding`: tab, clear selection.
    StopFinding(TabId, bool),
    /// `set_zoom`.
    SetZoom(TabId, Zoom),
    /// `set_audio_muted`.
    SetAudioMuted(TabId, bool),
    /// `print`.
    Print(TabId),
    /// `print_to_pdf`, with the id it returned.
    PrintToPdf(TabId, PathBuf, RequestId),
    /// `show_devtools`.
    ShowDevtools(TabId, Option<PagePoint>),
    /// `show_console`.
    ShowConsole(TabId),
    /// `close_devtools`.
    CloseDevtools(TabId),
    /// `exit_fullscreen`.
    ExitFullscreen(TabId),
    /// `save_mhtml`, with the id it returned.
    SaveMhtml(TabId, PathBuf, RequestId),
    /// `start_download`.
    StartDownload(TabId, Url),
    /// `eval`, with the id it returned.
    Eval(TabId, String, RequestId),
    /// `capture_page`, with the id it returned.
    CapturePage(TabId, RequestId),
    /// `send_key`.
    SendKey(TabId, Chord),
    /// `answer_prompt`, accepted or not.
    AnswerPrompt(PromptId, PromptAnswer),
    /// `continue_download`.
    ContinueDownload(EngineDownloadId, PathBuf),
    /// `cancel_download`.
    CancelDownload(EngineDownloadId),
    /// `pause_download`.
    PauseDownload(EngineDownloadId),
    /// `resume_download`.
    ResumeDownload(EngineDownloadId),
    /// `cookie_sites`, with the id it returned.
    CookieSites(Browsing, RequestId),
    /// `cookies_for`, with the id it returned.
    CookiesFor(Browsing, Url, RequestId),
    /// `delete_site_data`, with the id it returned.
    DeleteSiteData(Browsing, String, RequestId),
    /// `delete_cookies_for_hosts`.
    DeleteCookiesForHosts(Browsing, Vec<String>),
    /// `clear_browsing_data`, with the id it returned.
    ClearBrowsingData(Browsing, RequestId),
    /// `extension_call`.
    ExtensionCall(ExtensionCall, RequestId),
    /// `close_foreign`.
    CloseForeign(ForeignId),
    /// `test_shutdown`.
    #[cfg(feature = "self-test")]
    TestShutdown(TestShutdown),
}

/// The fake's shutdown state (design section 3.6's teardown states, without the failure exit).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FakePhase {
    /// Before `begin_shutdown`.
    Running,
    /// After `begin_shutdown`: user browsers close, no new ones.
    ClosingUser,
    /// After `close_internal`: the hidden browsers close too.
    ClosingInternal,
    /// The live count reached zero after `close_internal`.
    Drained,
}

/// What became of an admitted prompt's callback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptOutcome {
    /// Answered; the answer action ran.
    Answered(PromptAnswer),
    /// Ended by a navigation, a close or shutdown; the cancel action ran.
    Cancelled(PromptEnd),
    /// Ended by an engine-side end (`Withdrawn`, `EngineReset`, `MenuDismissed`); the callback was dropped
    /// without a call.
    Dropped(PromptEnd),
}

/// An IO-thread prompt between the engine thread that asked and its admission on main (design section
/// 1.5, Admission). Made by `FakeEngine::queue_io_prompt`, consumed by `FakeEngine::admit`.
#[derive(Debug)]
pub struct Queued {
    request: PromptRequest,
    scope: Option<PromptScope<TabId>>,
}

/// The fake's model of one tab's page.
#[derive(Clone, Debug, PartialEq)]
pub struct FakeTab {
    /// From the spec.
    pub kind: TabKind,
    /// From the spec.
    pub browsing: Browsing,
    /// From the spec.
    pub parent: NativeHandle,
    /// From the spec.
    pub bounds: PhysRect,
    /// From the spec.
    pub scale_factor: f32,
    /// From the spec, then `set_visible`.
    pub visible: bool,
    /// From the spec.
    pub opener: Option<TabId>,
    /// The page's native window, once created.
    pub native: Option<NativeHandle>,
    /// The committed URL.
    pub url: Option<Url>,
    /// Back/forward entries, oldest first.
    pub history: Vec<Url>,
    /// The current entry in `history`.
    pub history_index: usize,
    /// The document generation, raised on every new document.
    pub generation: u64,
    /// `set_zoom`.
    pub zoom: Zoom,
    /// `set_audio_muted`.
    pub muted: bool,
    /// `set_frozen`.
    pub frozen: bool,
    /// DevTools open.
    pub devtools: bool,
}

impl FakeTab {
    fn from_spec(spec: &TabSpec) -> FakeTab {
        FakeTab {
            kind: spec.kind,
            browsing: spec.browsing,
            parent: spec.parent,
            bounds: spec.bounds,
            scale_factor: spec.scale_factor,
            visible: spec.visible,
            opener: spec.opener,
            native: None,
            url: None,
            history: Vec::new(),
            history_index: 0,
            generation: 0,
            zoom: Zoom(1.0),
            muted: false,
            frozen: false,
            devtools: false,
        }
    }

    fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Role {
    Tab(TabId),
    /// Parked, with the tab it was.
    Parked(ParkedId, TabId),
    /// A browser Chromium opened itself, hidden until the shell closes it.
    Foreign(ForeignId),
    Internal,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Life {
    Creating,
    Live,
    Closing,
}

/// A counted browser, from the create request to its close.
struct Browser {
    role: Role,
    life: Life,
    page: Option<FakeTab>,
    /// What the tab commits once its creation finishes while running.
    first_load: Option<(Url, EngineTransition)>,
}

impl Browser {
    fn tab(&self) -> Option<TabId> {
        match self.role {
            Role::Tab(tab) | Role::Parked(_, tab) => Some(tab),
            Role::Foreign(_) | Role::Internal => None,
        }
    }

    fn user(&self) -> bool {
        self.role != Role::Internal
    }
}

struct State {
    events: UnboundedSender<BrowserEvent>,
    commands: Vec<FakeCommand>,
    phase: FakePhase,
    browsers: BTreeMap<u64, Browser>,
    last_id: u64,
    hidden_host: Option<NativeHandle>,
    hooks: Option<Rc<dyn ShellHooks>>,
    page_focus: BTreeMap<TabId, PageFocus>,
    focused: Option<TabId>,
    prompts: PromptLedger<TabId, ()>,
    outcomes: Vec<(PromptId, PromptOutcome)>,
    refused: Vec<(PromptRequest, Refused)>,
    scripted: VecDeque<RequestResult>,
    pending: Vec<RequestId>,
    /// Pending `Uninstall` calls, answered by their extension's `Uninstalled`.
    uninstalls: Vec<(RequestId, ExtensionId)>,
    last_foreign: u64,
    hold_creation: BTreeSet<TabId>,
    hold_close: BTreeSet<TabId>,
    downloading: BTreeSet<TabId>,
    beforeunload: BTreeSet<TabId>,
    unload_prompts: BTreeMap<PromptId, TabId>,
    user_closed_sent: bool,
    wakes: Option<UnboundedSender<Duration>>,
}

/// The fake engine. Main thread only, like the real one.
pub struct FakeEngine {
    state: RefCell<State>,
    pump_steps: Cell<u64>,
    driver_shut_down: Cell<bool>,
}

impl FakeEngine {
    /// A running fake with no browsers, and the receiver of its events.
    pub fn new() -> (Rc<FakeEngine>, UnboundedReceiver<BrowserEvent>) {
        let (events, receiver) = unbounded();
        let state = State {
            events,
            commands: Vec::new(),
            phase: FakePhase::Running,
            browsers: BTreeMap::new(),
            last_id: 0,
            hidden_host: None,
            hooks: None,
            page_focus: BTreeMap::new(),
            focused: None,
            prompts: PromptLedger::new(),
            outcomes: Vec::new(),
            refused: Vec::new(),
            scripted: VecDeque::new(),
            pending: Vec::new(),
            uninstalls: Vec::new(),
            last_foreign: 0,
            hold_creation: BTreeSet::new(),
            hold_close: BTreeSet::new(),
            downloading: BTreeSet::new(),
            beforeunload: BTreeSet::new(),
            unload_prompts: BTreeMap::new(),
            user_closed_sent: false,
            wakes: None,
        };
        let engine = FakeEngine { state: RefCell::new(state), pump_steps: Cell::new(0), driver_shut_down: Cell::new(false) };
        (Rc::new(engine), receiver)
    }

    /// A driver for this engine, as `EngineFactory::initialize` returns one.
    pub fn driver(self: &Rc<Self>) -> Box<dyn EngineDriver> {
        Box::new(FakeDriver { engine: self.clone() })
    }

    // Inspection.

    /// Every command so far, in order.
    pub fn commands(&self) -> Vec<FakeCommand> {
        self.state.borrow().commands.clone()
    }

    /// Every command so far, clearing the record.
    pub fn take_commands(&self) -> Vec<FakeCommand> {
        std::mem::take(&mut self.state.borrow_mut().commands)
    }

    /// The shutdown state.
    pub fn phase(&self) -> FakePhase {
        self.state.borrow().phase
    }

    /// The live-browser count: every browser from its create request to its close, hidden ones included.
    pub fn live_browsers(&self) -> usize {
        self.state.borrow().browsers.len()
    }

    /// The live user browsers: tabs, popups and parked browsers.
    pub fn live_user_browsers(&self) -> usize {
        self.state.borrow().browsers.values().filter(|b| b.user()).count()
    }

    /// The tabs whose browser exists and is not closing, in id order.
    pub fn tabs(&self) -> Vec<TabId> {
        let state = self.state.borrow();
        let mut tabs: Vec<TabId> = state
            .browsers
            .values()
            .filter_map(|b| match b.role {
                Role::Tab(tab) if b.life != Life::Closing => Some(tab),
                _ => None,
            })
            .collect();
        tabs.sort();
        tabs
    }

    /// The model of a tab's page, while its browser is the tab's.
    pub fn tab(&self, tab: TabId) -> Option<FakeTab> {
        let state = self.state.borrow();
        state.browser_of(tab).and_then(|id| state.browsers[&id].page.clone())
    }

    /// The parked browsers.
    pub fn parked(&self) -> Vec<ParkedId> {
        let state = self.state.borrow();
        state
            .browsers
            .values()
            .filter_map(|b| match b.role {
                Role::Parked(parked, _) => Some(parked),
                _ => None,
            })
            .collect()
    }

    /// The last page-focus policy published for a tab.
    pub fn page_focus(&self, tab: TabId) -> Option<PageFocus> {
        self.state.borrow().page_focus.get(&tab).copied()
    }

    /// The tab whose page has keyboard focus.
    pub fn focused(&self) -> Option<TabId> {
        self.state.borrow().focused
    }

    /// The hidden host, once set.
    pub fn hidden_host(&self) -> Option<NativeHandle> {
        self.state.borrow().hidden_host
    }

    /// How many pump steps the driver ran.
    pub fn pump_steps(&self) -> u64 {
        self.pump_steps.get()
    }

    /// Whether the driver's `shutdown` ran.
    pub fn driver_shut_down(&self) -> bool {
        self.driver_shut_down.get()
    }

    // Events.

    /// Delivers any event to the shell, as if the engine sent it.
    pub fn inject(&self, event: BrowserEvent) {
        self.state.borrow().emit(event);
    }

    /// Sends a wake request on the channel `EngineDriver::schedule` returned. False before `schedule`.
    pub fn wake(&self, delay: Duration) -> bool {
        self.state.borrow().wakes.as_ref().is_some_and(|w| w.unbounded_send(delay).is_ok())
    }

    // Lifecycle.

    /// The tab's browser will finish creation only at `complete_creation`.
    pub fn hold_creation(&self, tab: TabId) {
        self.state.borrow_mut().hold_creation.insert(tab);
    }

    /// The engine finishes a held creation (`on_after_created`). After shutdown began, the browser is
    /// closed at once and stays counted until its close finishes. False if no creation was pending.
    pub fn complete_creation(&self, tab: TabId) -> bool {
        let mut state = self.state.borrow_mut();
        state.hold_creation.remove(&tab);
        let pending = state.browsers.iter().find(|(_, b)| b.role == Role::Tab(tab) && b.life == Life::Creating);
        let Some((&id, _)) = pending else { return false };
        if state.phase == FakePhase::Running {
            state.created(id);
        } else {
            state.begin_close(id);
        }
        true
    }

    /// The browser that is or was this tab will finish closing only at `complete_close`.
    pub fn hold_close(&self, tab: TabId) {
        self.state.borrow_mut().hold_close.insert(tab);
    }

    /// The engine finishes a held close (`on_before_close`). False if no close was pending.
    pub fn complete_close(&self, tab: TabId) -> bool {
        let mut state = self.state.borrow_mut();
        state.hold_close.remove(&tab);
        let closing = state.browsers.iter().find(|(_, b)| b.tab() == Some(tab) && b.life == Life::Closing);
        let Some((&id, _)) = closing else { return false };
        state.finish_close(id);
        true
    }

    /// The page closes itself (`window.close()`): an unrequested `TabEvent::Closed` follows.
    pub fn close_from_page(&self, tab: TabId) {
        let mut state = self.state.borrow_mut();
        if let Some(id) = state.browser_of(tab) {
            state.begin_close(id);
        }
    }

    /// Whether the page has a `beforeunload` handler. With one, `close_tab(.., RunUnloadHandlers)` raises a
    /// `BeforeUnload` prompt; the tab closes when the answer is to leave.
    pub fn set_beforeunload(&self, tab: TabId, handler: bool) {
        let mut state = self.state.borrow_mut();
        if handler {
            state.beforeunload.insert(tab);
        } else {
            state.beforeunload.remove(&tab);
        }
    }

    /// Chromium asks to focus the tab's page on its own (`on_set_focus`). Granted only when the last
    /// published policy allows it (none published: `NEW_BROWSER`); a grant focuses the page.
    pub fn engine_focus_request(&self, tab: TabId) -> bool {
        let mut state = self.state.borrow_mut();
        let allowed = state.page_focus.get(&tab).copied().unwrap_or(PageFocus::NEW_BROWSER).allows();
        if allowed {
            state.focus(tab);
        }
        allowed
    }

    /// The page commits a same-document navigation (fragment, History API): the generation stays, so open
    /// prompts stay open.
    pub fn commit_same_document(&self, tab: TabId, url: Url) {
        let mut state = self.state.borrow_mut();
        let Some(page) = state.live_page(tab) else { return };
        page.history.truncate(page.history_index + 1);
        page.history.push(url.clone());
        page.history_index = page.history.len() - 1;
        page.url = Some(url.clone());
        let committed =
            TabEvent::Committed { url: url.clone(), kind: CommitKind::SameDocument, transition: EngineTransition::Link, mime: None };
        state.emit(BrowserEvent::Tab(tab, committed));
        state.emit(BrowserEvent::Tab(tab, TabEvent::AddressChanged(url)));
    }

    /// Whether the tab has a running download, which keeps its browser alive after `detach_tab`.
    pub fn set_downloading(&self, tab: TabId, downloading: bool) {
        let mut state = self.state.borrow_mut();
        if downloading {
            state.downloading.insert(tab);
        } else {
            state.downloading.remove(&tab);
        }
    }

    /// A parked browser's last download ended: the engine closes it. False if no such browser.
    pub fn end_parked_downloads(&self, parked: ParkedId) -> bool {
        let mut state = self.state.borrow_mut();
        let found = state.browsers.iter().find(|(_, b)| matches!(b.role, Role::Parked(p, _) if p == parked));
        let Some((&id, _)) = found else { return false };
        state.begin_close(id);
        true
    }

    // Prompts.

    /// A UI-thread prompt, admitted at once in its tab's current document. `Prompt` follows on admission.
    pub fn push_prompt(&self, request: PromptRequest) -> Result<PromptId, Refused> {
        let queued = self.queue_io_prompt(request);
        self.admit(queued)
    }

    /// An IO-thread prompt: its scope is taken now, its admission waits for `admit`, so a test can close
    /// or navigate the tab, or begin shutdown, in between.
    pub fn queue_io_prompt(&self, request: PromptRequest) -> Queued {
        let state = self.state.borrow();
        let scope = request.tab().map(|tab| PromptScope { browser: tab, generation: state.generation(tab).unwrap_or(0) });
        Queued { request, scope }
    }

    /// Admits a queued prompt on main. A refused prompt never reaches the shell; its cancel action runs
    /// (recorded in `refused_prompts`).
    pub fn admit(&self, queued: Queued) -> Result<PromptId, Refused> {
        let mut state = self.state.borrow_mut();
        let State { prompts, browsers, .. } = &mut *state;
        match prompts.admit(queued.request.kind(), queued.scope, |tab| scope_state(browsers, tab), ()) {
            Ok(id) => {
                state.emit(BrowserEvent::Prompt { id, request: queued.request });
                Ok(id)
            }
            Err((why, ())) => {
                state.refused.push((queued.request, why));
                Err(why)
            }
        }
    }

    /// The tab's main frame commits a new document: its generation rises and its prompts end with
    /// `Navigated`. Emits no navigation events.
    pub fn commit_document(&self, tab: TabId) {
        self.state.borrow_mut().commit_document(tab);
    }

    /// The engine ends a prompt with any reason. Engine-side ends drop the callback; others cancel it.
    pub fn end_prompt(&self, id: PromptId, why: PromptEnd) -> Result<(), PromptError> {
        let mut state = self.state.borrow_mut();
        state.prompts.end(id)?;
        state.ended(id, why);
        Ok(())
    }

    /// The open prompts, oldest first.
    pub fn open_prompts(&self) -> Vec<PromptId> {
        self.state.borrow().prompts.open_ids()
    }

    /// What became of each admitted prompt that is no longer open, in order.
    pub fn prompt_outcomes(&self) -> Vec<(PromptId, PromptOutcome)> {
        self.state.borrow().outcomes.clone()
    }

    /// The prompts admission refused, with why; each one's cancel action ran.
    pub fn refused_prompts(&self) -> Vec<(PromptRequest, Refused)> {
        self.state.borrow().refused.clone()
    }

    // Requests.

    /// The result the next request-id operation gets at once. Without one, it stays pending until
    /// `finish_request`.
    pub fn script_result(&self, result: RequestResult) {
        self.state.borrow_mut().scripted.push_back(result);
    }

    /// Finishes a pending request. False if it is not pending.
    pub fn finish_request(&self, id: RequestId, result: RequestResult) -> bool {
        let mut state = self.state.borrow_mut();
        let Some(at) = state.pending.iter().position(|p| *p == id) else { return false };
        state.pending.remove(at);
        state.emit(BrowserEvent::RequestFinished { id, result });
        true
    }

    /// The requests without a result yet.
    pub fn pending_requests(&self) -> Vec<RequestId> {
        self.state.borrow().pending.clone()
    }

    // Extensions (design section 5.4).

    /// Chromium's extension system reports an event. `Uninstalled` also answers the pending `Uninstall`
    /// calls for its id, as the engine does.
    pub fn extension_event(&self, event: ExtensionEvent) {
        let mut state = self.state.borrow_mut();
        let removed = match &event {
            ExtensionEvent::Uninstalled { id } => Some(id.clone()),
            _ => None,
        };
        state.emit(BrowserEvent::Extension(event));
        let Some(removed) = removed else { return };
        let calls = std::mem::take(&mut state.uninstalls);
        for (call, id) in calls {
            let Some(at) = state.pending.iter().position(|p| *p == call) else { continue };
            if id == removed {
                state.pending.remove(at);
                state.emit(BrowserEvent::RequestFinished { id: call, result: RequestResult::Extension(Ok(ExtensionReply::Done)) });
            } else {
                state.uninstalls.push((call, id));
            }
        }
    }

    /// Chromium opens a browser itself (`on_after_created` through `default_client`). Counted at once;
    /// `ShellHooks::neutralize_foreign` hides it (no hooks: it fails). While running, a hidden one is
    /// `Opened` and a failure is closed and `Refused`; after that it is closed without an event.
    pub fn open_foreign(&self, browsing: Browsing) -> ForeignId {
        let (foreign, hooks) = {
            let mut state = self.state.borrow_mut();
            state.last_foreign += 1;
            let foreign = ForeignId(state.last_foreign);
            let id = state.add_browser(Role::Foreign(foreign), None, None);
            state.finish_creation(id);
            (foreign, state.hooks.clone())
        };
        let native = NativeHandle::from_raw(0x2000 + foreign.0);
        let no_hooks = || Err(NeutralizeFailed { reason: "no hooks".to_owned() });
        let neutralized = hooks.map_or_else(no_hooks, |h| h.neutralize_foreign(native));
        let mut state = self.state.borrow_mut();
        let Some(id) = state.foreign_browser(foreign) else { return foreign };
        let running = state.phase == FakePhase::Running;
        match neutralized {
            Ok(()) if running => state.emit(BrowserEvent::ForeignWindow(ForeignWindowEvent::Opened { id: foreign, browsing })),
            Ok(()) => state.begin_close(id),
            Err(NeutralizeFailed { reason }) => {
                state.begin_close(id);
                if running {
                    state.emit(BrowserEvent::ForeignWindow(ForeignWindowEvent::Refused { id: foreign, reason }));
                }
            }
        }
        foreign
    }

    /// A foreign browser has its URL (`None`: it stayed blank): `Ready`. Ignored once it is closing.
    pub fn foreign_ready(&self, foreign: ForeignId, url: Option<Url>) {
        let state = self.state.borrow();
        if state.foreign_browser(foreign).is_some() {
            state.emit(BrowserEvent::ForeignWindow(ForeignWindowEvent::Ready { id: foreign, url }));
        }
    }

    /// The foreign browsers that live and are not closing, in id order.
    pub fn foreign_browsers(&self) -> Vec<ForeignId> {
        let state = self.state.borrow();
        state
            .browsers
            .values()
            .filter_map(|b| match b.role {
                Role::Foreign(foreign) if b.life != Life::Closing => Some(foreign),
                _ => None,
            })
            .collect()
    }

    // The page, through the shell's hooks.

    /// The page navigates on its own: runs `ShellHooks::navigation_policy` (no hooks: `Allow`) and, for an
    /// allowed or redirected main-frame navigation, commits it.
    pub fn navigate_from_page(&self, request: NavigationRequest) -> NavigationDecision {
        let hooks = self.state.borrow().hooks.clone();
        let decision = hooks.map_or(NavigationDecision::Allow, |h| h.navigation_policy(&request));
        if request.is_main_frame {
            let target = match &decision {
                NavigationDecision::Allow => Some(request.url.clone()),
                NavigationDecision::Redirect(url) => Some(url.clone()),
                NavigationDecision::Cancel => None,
            };
            if let Some(url) = target {
                self.state.borrow_mut().navigate(request.tab, url, request.transition, Entry::Push);
            }
        }
        decision
    }

    /// The page opens a popup: runs `ShellHooks::popup_policy` (no hooks: `Block`). Allowed, the popup is a
    /// new counted tab, created, opened and navigated. After shutdown began it is blocked without asking;
    /// a decision that names an existing tab, or arrives after the hook began shutdown, is blocked too.
    pub fn open_popup(&self, request: PopupRequest) -> PopupDecision {
        let hooks = {
            let state = self.state.borrow();
            if state.phase != FakePhase::Running {
                return PopupDecision::Block;
            }
            state.hooks.clone()
        };
        let decision = hooks.map_or(PopupDecision::Block, |h| h.popup_policy(&request));
        if let PopupDecision::AllowAsTab { tab, parent, bounds, scale_factor } = decision.clone() {
            {
                let state = self.state.borrow();
                if state.phase != FakePhase::Running || state.browser_of(tab).is_some() {
                    return PopupDecision::Block;
                }
            }
            let browsing = self.tab(request.opener).map_or(Browsing::Normal, |t| t.browsing);
            let spec = TabSpec {
                tab,
                kind: TabKind::Page,
                browsing,
                parent,
                bounds,
                scale_factor,
                visible: false,
                page: InitialPage::Blank,
                opener: Some(request.opener),
            };
            let mut state = self.state.borrow_mut();
            let first_load = Some((request.url.clone(), EngineTransition::Link));
            let id = state.add_browser(Role::Tab(tab), Some(FakeTab::from_spec(&spec)), first_load);
            state.emit(BrowserEvent::PopupOpened {
                opener: request.opener,
                tab,
                url: request.url.clone(),
                disposition: request.disposition,
            });
            if !state.hold_creation.contains(&tab) {
                state.created(id);
            }
        }
        decision
    }

    fn record(&self, command: FakeCommand) {
        self.state.borrow_mut().commands.push(command);
    }

    fn request(&self, command: impl FnOnce(RequestId) -> FakeCommand) -> RequestId {
        let mut state = self.state.borrow_mut();
        state.last_id += 1;
        let id = RequestId(state.last_id);
        state.commands.push(command(id));
        match state.scripted.pop_front() {
            Some(result) => state.emit(BrowserEvent::RequestFinished { id, result }),
            None => state.pending.push(id),
        }
        id
    }

    fn with_page(&self, tab: TabId, change: impl FnOnce(&mut FakeTab)) {
        let mut state = self.state.borrow_mut();
        if let Some(id) = state.browser_of(tab)
            && let Some(page) = state.browsers.get_mut(&id).and_then(|b| b.page.as_mut())
        {
            change(page);
        }
    }
}

fn scope_state(browsers: &BTreeMap<u64, Browser>, tab: TabId) -> Option<ScopeState> {
    browsers.values().find(|b| b.role == Role::Tab(tab)).map(|b| ScopeState {
        generation: b.page.as_ref().map_or(0, |p| p.generation),
        closing: b.life == Life::Closing,
    })
}

impl State {
    fn emit(&self, event: BrowserEvent) {
        // A test that dropped the receiver does not want the events.
        let _ = self.events.unbounded_send(event);
    }

    /// The browser that is this tab, closing or not.
    fn browser_of(&self, tab: TabId) -> Option<u64> {
        self.browsers.iter().find(|(_, b)| b.role == Role::Tab(tab)).map(|(id, _)| *id)
    }

    /// The live foreign browser, not closing.
    fn foreign_browser(&self, foreign: ForeignId) -> Option<u64> {
        self.browsers.iter().find(|(_, b)| b.role == Role::Foreign(foreign) && b.life == Life::Live).map(|(id, _)| *id)
    }

    fn live_page(&mut self, tab: TabId) -> Option<&mut FakeTab> {
        let id = self.browser_of(tab)?;
        let browser = self.browsers.get_mut(&id)?;
        if browser.life != Life::Live {
            return None;
        }
        browser.page.as_mut()
    }

    fn generation(&self, tab: TabId) -> Option<u64> {
        let id = self.browser_of(tab)?;
        self.browsers[&id].page.as_ref().map(|p| p.generation)
    }

    fn add_browser(&mut self, role: Role, page: Option<FakeTab>, first_load: Option<(Url, EngineTransition)>) -> u64 {
        self.last_id += 1;
        let id = self.last_id;
        self.browsers.insert(id, Browser { role, life: Life::Creating, page, first_load });
        id
    }

    /// Creation finished while running: `Created`, then the first page.
    fn created(&mut self, id: u64) {
        self.finish_creation(id);
        let Some(browser) = self.browsers.get_mut(&id) else { return };
        if let (Role::Tab(tab), Some((url, transition))) = (browser.role, browser.first_load.take()) {
            self.navigate(tab, url, transition, Entry::Push);
        }
    }

    fn focus(&mut self, tab: TabId) {
        if self.live_page(tab).is_none() || self.focused == Some(tab) {
            return;
        }
        if let Some(old) = self.focused.replace(tab) {
            self.emit(BrowserEvent::Tab(old, TabEvent::FocusChanged { focused: false }));
        }
        self.emit(BrowserEvent::Tab(tab, TabEvent::FocusChanged { focused: true }));
    }

    fn finish_creation(&mut self, id: u64) {
        self.last_id += 1;
        let native = NativeHandle::from_raw(0x1000 + self.last_id);
        let Some(browser) = self.browsers.get_mut(&id) else { return };
        browser.life = Life::Live;
        if let (Role::Tab(tab), Some(page)) = (browser.role, browser.page.as_mut()) {
            page.native = Some(native);
            self.emit(BrowserEvent::Tab(tab, TabEvent::Created { native }));
        }
    }

    fn begin_close(&mut self, id: u64) {
        let Some(browser) = self.browsers.get_mut(&id) else { return };
        browser.life = Life::Closing;
        let tab = browser.tab();
        let role = browser.role;
        if let Role::Tab(tab) = role {
            self.end_prompts_of(tab, PromptEnd::TabClosed);
            if self.focused == Some(tab) {
                self.focused = None;
            }
        }
        if tab.is_none_or(|t| !self.hold_close.contains(&t)) {
            self.finish_close(id);
        }
    }

    fn finish_close(&mut self, id: u64) {
        let Some(browser) = self.browsers.remove(&id) else { return };
        if let Role::Tab(tab) = browser.role {
            self.emit(BrowserEvent::Tab(tab, TabEvent::Closed));
        }
        self.check_drained();
    }

    fn check_drained(&mut self) {
        if self.phase == FakePhase::Running {
            return;
        }
        if !self.user_closed_sent && !self.browsers.values().any(Browser::user) {
            self.user_closed_sent = true;
            self.emit(BrowserEvent::UserBrowsersClosed);
        }
        if self.phase == FakePhase::ClosingInternal && self.browsers.is_empty() {
            self.phase = FakePhase::Drained;
            self.emit(BrowserEvent::AllBrowsersClosed);
        }
    }

    fn begin_shutdown(&mut self) {
        if self.phase != FakePhase::Running {
            return;
        }
        self.phase = FakePhase::ClosingUser;
        for (id, ()) in self.prompts.shut() {
            self.ended(id, PromptEnd::Shutdown);
        }
        // A browser still creating is closed when its creation finishes (`complete_creation`).
        let closing: Vec<u64> =
            self.browsers.iter().filter(|(_, b)| b.user() && b.life == Life::Live).map(|(id, _)| *id).collect();
        for id in closing {
            self.begin_close(id);
        }
        self.check_drained();
    }

    fn close_internal(&mut self) {
        self.begin_shutdown();
        if self.phase != FakePhase::ClosingUser {
            return;
        }
        self.phase = FakePhase::ClosingInternal;
        let internal: Vec<u64> = self.browsers.iter().filter(|(_, b)| !b.user()).map(|(id, _)| *id).collect();
        for id in internal {
            self.begin_close(id);
        }
        self.check_drained();
    }

    fn ended(&mut self, id: PromptId, why: PromptEnd) {
        let outcome = if why.engine_side() { PromptOutcome::Dropped(why) } else { PromptOutcome::Cancelled(why) };
        self.outcomes.push((id, outcome));
        self.emit(BrowserEvent::PromptEnded { id, why });
    }

    fn end_prompts_of(&mut self, tab: TabId, why: PromptEnd) {
        for (id, ()) in self.prompts.end_browser(tab) {
            self.ended(id, why);
        }
    }

    fn commit_document(&mut self, tab: TabId) {
        if let Some(page) = self.live_page(tab) {
            page.generation += 1;
            self.end_prompts_of(tab, PromptEnd::Navigated);
        }
    }

    /// A main-frame navigation to a new document, committed at once.
    fn navigate(&mut self, tab: TabId, url: Url, transition: EngineTransition, entry: Entry) {
        let Some(page) = self.live_page(tab) else { return };
        let (back, forward) = (page.can_go_back(), page.can_go_forward());
        match entry {
            Entry::Push => {
                page.history.truncate(page.history_index + 1);
                page.history.push(url.clone());
                page.history_index = page.history.len() - 1;
            }
            Entry::Same => {}
            Entry::Go(index) => page.history_index = index,
        }
        page.url = Some(url.clone());
        let (can_go_back, can_go_forward) = (page.can_go_back(), page.can_go_forward());
        self.emit(BrowserEvent::Tab(tab, TabEvent::NavigationStarted { url: url.clone(), is_redirect: false }));
        self.emit(BrowserEvent::Tab(
            tab,
            TabEvent::LoadingStateChanged { loading: true, can_go_back: back, can_go_forward: forward },
        ));
        self.commit_document(tab);
        self.emit(BrowserEvent::Tab(
            tab,
            TabEvent::Committed { url: url.clone(), kind: CommitKind::Document, transition, mime: None },
        ));
        self.emit(BrowserEvent::Tab(tab, TabEvent::AddressChanged(url)));
        self.emit(BrowserEvent::Tab(tab, TabEvent::LoadingStateChanged { loading: false, can_go_back, can_go_forward }));
    }

    /// Moves one entry back or forward in the history and commits it.
    fn traverse(&mut self, tab: TabId, back: bool) {
        let Some(page) = self.live_page(tab) else { return };
        let possible = if back { page.can_go_back() } else { page.can_go_forward() };
        if !possible {
            return;
        }
        let index = if back { page.history_index - 1 } else { page.history_index + 1 };
        let url = page.history[index].clone();
        self.navigate(tab, url, EngineTransition::BackForward, Entry::Go(index));
    }
}

/// What a navigation does to the history.
#[derive(Copy, Clone)]
enum Entry {
    /// A new entry after the current one.
    Push,
    /// The current entry again.
    Same,
    /// The entry at this index.
    Go(usize),
}

fn about_blank() -> Url {
    Url::parse("about:blank").expect("a valid URL")
}

impl Engine for FakeEngine {
    fn set_hooks(&self, hooks: Rc<dyn ShellHooks>) {
        let old = {
            let mut state = self.state.borrow_mut();
            state.commands.push(FakeCommand::SetHooks);
            state.hooks.replace(hooks)
        };
        drop(old);
    }

    fn set_key_routing(&self, routing: KeyRouting) {
        self.record(FakeCommand::SetKeyRouting(routing));
    }

    fn apply_prefs(&self, prefs: &EnginePrefs) {
        self.record(FakeCommand::ApplyPrefs(prefs.clone()));
    }

    fn set_request_filter(&self, _filter: Arc<dyn RequestFilter>) {
        self.record(FakeCommand::SetRequestFilter);
    }

    fn set_document_start_scripts(&self, scripts: Vec<String>) {
        self.record(FakeCommand::SetDocumentStartScripts(scripts));
    }

    fn set_site_setting(&self, browsing: Browsing, origin: &Origin, permission: Permission, setting: Option<Setting>) {
        self.record(FakeCommand::SetSiteSetting(browsing, origin.clone(), permission, setting));
    }

    fn begin_private_context(&self) {
        self.record(FakeCommand::BeginPrivateContext);
    }

    fn end_private_context(&self) {
        self.record(FakeCommand::EndPrivateContext);
    }

    fn versions(&self) -> EngineVersions {
        EngineVersions { engine: "FakeEngine".to_owned(), chromium: CHROMIUM.to_owned() }
    }

    fn set_hidden_host(&self, host: NativeHandle) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::SetHiddenHost(host));
        if state.hidden_host.is_none() && state.phase == FakePhase::Running {
            state.hidden_host = Some(host);
            let maintenance = state.add_browser(Role::Internal, None, None);
            state.finish_creation(maintenance);
        }
    }

    fn create_tab(&self, spec: TabSpec) -> Result<(), EngineError> {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::CreateTab(spec.clone()));
        if state.phase != FakePhase::Running {
            return Err(EngineError::ShuttingDown);
        }
        if state.browser_of(spec.tab).is_some() {
            return Err(EngineError::Failed(format!("tab {} exists", spec.tab.get())));
        }
        let first_load = match &spec.page {
            InitialPage::Url(url) => Some((url.clone(), EngineTransition::Typed)),
            InitialPage::Html { shown_url, .. } => {
                Some((shown_url.clone().unwrap_or_else(about_blank), EngineTransition::Other))
            }
            InitialPage::Blank => None,
        };
        let id = state.add_browser(Role::Tab(spec.tab), Some(FakeTab::from_spec(&spec)), first_load);
        if !state.hold_creation.contains(&spec.tab) {
            state.created(id);
        }
        Ok(())
    }

    fn close_tab(&self, tab: TabId, mode: CloseMode) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::CloseTab(tab, mode));
        let Some(id) = state.browser_of(tab).filter(|id| state.browsers[id].life != Life::Closing) else { return };
        let asks = mode == CloseMode::RunUnloadHandlers && state.beforeunload.contains(&tab);
        if asks && state.browsers[&id].life == Life::Live {
            let request = PromptRequest::BeforeUnload { tab, is_reload: false };
            let scope = Some(PromptScope { browser: tab, generation: state.generation(tab).unwrap_or(0) });
            let State { prompts, browsers, .. } = &mut *state;
            if let Ok(prompt) = prompts.admit(request.kind(), scope, |t| scope_state(browsers, t), ()) {
                state.unload_prompts.insert(prompt, tab);
                state.emit(BrowserEvent::Prompt { id: prompt, request });
                return;
            }
        }
        state.begin_close(id);
    }

    fn set_visible(&self, tab: TabId, visible: bool) {
        self.record(FakeCommand::SetVisible(tab, visible));
        self.with_page(tab, |page| page.visible = visible);
    }

    fn notify_moved(&self, tab: TabId) {
        self.record(FakeCommand::NotifyMoved(tab));
    }

    fn notify_screen_info_changed(&self, tab: TabId) {
        self.record(FakeCommand::NotifyScreenInfoChanged(tab));
    }

    fn set_page_focus(&self, tab: TabId, focus: PageFocus) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::SetPageFocus(tab, focus));
        state.page_focus.insert(tab, focus);
    }

    fn set_frozen(&self, tab: TabId, frozen: bool) {
        self.record(FakeCommand::SetFrozen(tab, frozen));
        self.with_page(tab, |page| page.frozen = frozen);
    }

    fn focus_page(&self, tab: TabId) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::FocusPage(tab));
        state.focus(tab);
    }

    fn blur_page(&self, tab: TabId) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::BlurPage(tab));
        if state.focused == Some(tab) {
            state.focused = None;
            state.emit(BrowserEvent::Tab(tab, TabEvent::FocusChanged { focused: false }));
        }
    }

    fn detach_tab(&self, tab: TabId) -> Result<ParkedId, EngineError> {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::DetachTab(tab));
        if state.phase != FakePhase::Running {
            return Err(EngineError::ShuttingDown);
        }
        let id = state.browser_of(tab).filter(|id| state.browsers[id].life == Life::Live);
        let Some(id) = id else { return Err(EngineError::UnknownTab(tab)) };
        state.end_prompts_of(tab, PromptEnd::TabClosed);
        if state.focused == Some(tab) {
            state.focused = None;
        }
        state.last_id += 1;
        let parked = ParkedId(state.last_id);
        state.browsers.get_mut(&id).expect("found above").role = Role::Parked(parked, tab);
        state.emit(BrowserEvent::Tab(tab, TabEvent::Detached));
        if !state.downloading.contains(&tab) {
            state.begin_close(id);
        }
        Ok(parked)
    }

    fn begin_shutdown(&self) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::BeginShutdown);
        state.begin_shutdown();
    }

    fn close_internal(&self) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::CloseInternal);
        state.close_internal();
    }

    fn load(&self, tab: TabId, url: &Url) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::Load(tab, url.clone()));
        state.navigate(tab, url.clone(), EngineTransition::Typed, Entry::Push);
    }

    fn load_html(&self, tab: TabId, html: String, shown_url: Option<&Url>) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::LoadHtml(tab, html, shown_url.cloned()));
        state.navigate(tab, shown_url.cloned().unwrap_or_else(about_blank), EngineTransition::Other, Entry::Push);
    }

    fn reload(&self, tab: TabId, bypass_cache: bool) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::Reload(tab, bypass_cache));
        if let Some(url) = state.live_page(tab).and_then(|p| p.url.clone()) {
            state.navigate(tab, url, EngineTransition::Reload, Entry::Same);
        }
    }

    fn stop(&self, tab: TabId) {
        self.record(FakeCommand::Stop(tab));
    }

    fn go_back(&self, tab: TabId) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::GoBack(tab));
        state.traverse(tab, true);
    }

    fn go_forward(&self, tab: TabId) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::GoForward(tab));
        state.traverse(tab, false);
    }

    fn find(&self, tab: TabId, text: &str, forward: bool, find_next: bool) {
        self.record(FakeCommand::Find(tab, text.to_owned(), forward, find_next));
    }

    fn stop_finding(&self, tab: TabId, clear_selection: bool) {
        self.record(FakeCommand::StopFinding(tab, clear_selection));
    }

    fn set_zoom(&self, tab: TabId, zoom: Zoom) {
        self.record(FakeCommand::SetZoom(tab, zoom));
        self.with_page(tab, |page| page.zoom = zoom);
    }

    fn zoom(&self, tab: TabId) -> Zoom {
        self.tab(tab).map_or(Zoom(1.0), |page| page.zoom)
    }

    fn set_audio_muted(&self, tab: TabId, muted: bool) {
        self.record(FakeCommand::SetAudioMuted(tab, muted));
        self.with_page(tab, |page| page.muted = muted);
    }

    fn print(&self, tab: TabId) {
        self.record(FakeCommand::Print(tab));
    }

    fn print_to_pdf(&self, tab: TabId, path: PathBuf) -> RequestId {
        self.request(|id| FakeCommand::PrintToPdf(tab, path, id))
    }

    fn show_devtools(&self, tab: TabId, inspect_at: Option<PagePoint>) {
        self.record(FakeCommand::ShowDevtools(tab, inspect_at));
        self.with_page(tab, |page| page.devtools = true);
    }

    fn show_console(&self, tab: TabId) {
        self.record(FakeCommand::ShowConsole(tab));
        self.with_page(tab, |page| page.devtools = true);
    }

    fn close_devtools(&self, tab: TabId) {
        self.record(FakeCommand::CloseDevtools(tab));
        self.with_page(tab, |page| page.devtools = false);
    }

    fn has_devtools(&self, tab: TabId) -> bool {
        self.tab(tab).is_some_and(|page| page.devtools)
    }

    fn exit_fullscreen(&self, tab: TabId) {
        self.record(FakeCommand::ExitFullscreen(tab));
    }

    fn save_mhtml(&self, tab: TabId, path: PathBuf) -> RequestId {
        self.request(|id| FakeCommand::SaveMhtml(tab, path, id))
    }

    fn start_download(&self, tab: TabId, url: &Url) {
        self.record(FakeCommand::StartDownload(tab, url.clone()));
    }

    fn eval(&self, tab: TabId, script: &str) -> RequestId {
        self.request(|id| FakeCommand::Eval(tab, script.to_owned(), id))
    }

    fn capture_page(&self, tab: TabId) -> RequestId {
        self.request(|id| FakeCommand::CapturePage(tab, id))
    }

    fn send_key(&self, tab: TabId, chord: Chord) {
        self.record(FakeCommand::SendKey(tab, chord));
    }

    fn answer_prompt(&self, id: PromptId, answer: PromptAnswer) -> Result<(), PromptError> {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::AnswerPrompt(id, answer.clone()));
        state.prompts.answer(id, &answer)?;
        let closing = state.unload_prompts.remove(&id);
        if let (Some(tab), PromptAnswer::BeforeUnload { leave: true }) = (closing, &answer)
            && let Some(browser) = state.browser_of(tab)
        {
            state.begin_close(browser);
        }
        state.outcomes.push((id, PromptOutcome::Answered(answer)));
        Ok(())
    }

    fn continue_download(&self, id: EngineDownloadId, path: PathBuf) {
        self.record(FakeCommand::ContinueDownload(id, path));
    }

    fn cancel_download(&self, id: EngineDownloadId) {
        self.record(FakeCommand::CancelDownload(id));
    }

    fn pause_download(&self, id: EngineDownloadId) {
        self.record(FakeCommand::PauseDownload(id));
    }

    fn resume_download(&self, id: EngineDownloadId) {
        self.record(FakeCommand::ResumeDownload(id));
    }

    fn cookie_sites(&self, browsing: Browsing) -> RequestId {
        self.request(|id| FakeCommand::CookieSites(browsing, id))
    }

    fn cookies_for(&self, browsing: Browsing, url: &Url) -> RequestId {
        self.request(|id| FakeCommand::CookiesFor(browsing, url.clone(), id))
    }

    fn delete_site_data(&self, browsing: Browsing, site: &str) -> RequestId {
        self.request(|id| FakeCommand::DeleteSiteData(browsing, site.to_owned(), id))
    }

    fn delete_cookies_for_hosts(&self, browsing: Browsing, hosts: &[String]) {
        self.record(FakeCommand::DeleteCookiesForHosts(browsing, hosts.to_vec()));
    }

    fn clear_browsing_data(&self, browsing: Browsing) -> RequestId {
        self.request(|id| FakeCommand::ClearBrowsingData(browsing, id))
    }

    /// Records the call. After shutdown began it fails at once. `Uninstall` waits for its extension's
    /// `Uninstalled` (`extension_event`) and takes no scripted result; the others take one or wait, as
    /// every request does.
    fn extension_call(&self, call: ExtensionCall) -> RequestId {
        let mut state = self.state.borrow_mut();
        state.last_id += 1;
        let id = RequestId(state.last_id);
        state.commands.push(FakeCommand::ExtensionCall(call.clone(), id));
        if state.phase != FakePhase::Running {
            let result = RequestResult::Extension(Err("shutting down".to_owned()));
            state.emit(BrowserEvent::RequestFinished { id, result });
        } else if let ExtensionCall::Uninstall { id: extension } = call {
            state.pending.push(id);
            state.uninstalls.push((id, extension));
        } else if let Some(result) = state.scripted.pop_front() {
            state.emit(BrowserEvent::RequestFinished { id, result });
        } else {
            state.pending.push(id);
        }
        id
    }

    fn close_foreign(&self, foreign: ForeignId) {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::CloseForeign(foreign));
        if let Some(id) = state.foreign_browser(foreign) {
            state.begin_close(id);
        }
    }

    /// `DelayClose` and `PinOpen` hold the tab's close and `HoldCreation` its creation, as
    /// `hold_close` and `hold_creation` do; the test finishes them.
    #[cfg(feature = "self-test")]
    fn test_shutdown(&self, test: TestShutdown) -> Result<(), EngineError> {
        let mut state = self.state.borrow_mut();
        state.commands.push(FakeCommand::TestShutdown(test.clone()));
        match test {
            TestShutdown::DelayClose { tab, .. } | TestShutdown::PinOpen { tab } => state.hold_close.insert(tab),
            TestShutdown::HoldCreation { tab } => state.hold_creation.insert(tab),
        };
        Ok(())
    }
}

/// The fake's driver: counts pump steps and records its shutdown.
struct FakeDriver {
    engine: Rc<FakeEngine>,
}

impl EngineDriver for FakeDriver {
    fn schedule(&mut self) -> UnboundedReceiver<Duration> {
        let (wakes, receiver) = unbounded();
        self.engine.state.borrow_mut().wakes = Some(wakes);
        receiver
    }

    fn do_work(&self) {
        self.engine.pump_steps.set(self.engine.pump_steps.get() + 1);
    }

    fn shutdown(self: Box<Self>) {
        self.engine.driver_shut_down.set(true);
    }
}

/// A factory whose `initialize` hands out one `FakeEngine`, its driver and its events, and records the
/// config.
pub struct FakeEngineFactory {
    engine: Rc<FakeEngine>,
    events: RefCell<Option<UnboundedReceiver<BrowserEvent>>>,
    config: RefCell<Option<EngineConfig>>,
}

impl FakeEngineFactory {
    /// A factory with a fresh engine.
    pub fn new() -> FakeEngineFactory {
        let (engine, events) = FakeEngine::new();
        FakeEngineFactory { engine, events: RefCell::new(Some(events)), config: RefCell::new(None) }
    }

    /// The engine it hands out.
    pub fn engine(&self) -> Rc<FakeEngine> {
        self.engine.clone()
    }

    /// The config `initialize` received.
    pub fn config(&self) -> Option<EngineConfig> {
        self.config.borrow().clone()
    }
}

impl Default for FakeEngineFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineFactory for FakeEngineFactory {
    fn chromium_version(&self) -> &'static str {
        CHROMIUM
    }

    fn initialize(&self, config: EngineConfig) -> Result<EngineParts, EngineInitError> {
        let events = self.events.borrow_mut().take().ok_or_else(|| EngineInitError::Failed("initialized twice".to_owned()))?;
        *self.config.borrow_mut() = Some(config);
        Ok(EngineParts { engine: self.engine.clone(), driver: self.engine.driver(), events })
    }
}
