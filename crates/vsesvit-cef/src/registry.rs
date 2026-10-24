//! Owned by W1.3.
//! The main-thread registry (design section 1.6): every browser by CEF browser id and `TabId`, its
//! kind, the creations in flight, the prompt callbacks, the hooks, the page-focus policies and the
//! key routing, and from them the live-browser count shutdown waits on (section 3.6).
//!
//! Handlers hold only `Send + Sync` values; everything else lives here, in a `thread_local!` on the
//! thread that started CEF. [`with`] on any other thread logs once and returns `None`, and it never
//! panics, so nothing unwinds through FFI. The rule that makes engine calls re-entrant (section 1.6,
//! rule 2): no borrow of the registry is held across a call out of it. Code clones the browser out,
//! returns from `with`, then calls CEF or a hook. A nested `with` while one is held is a bug; it is
//! refused, logged and counted ([`conflicts`]), which the re-entrancy test asserts is zero.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use vsesvit_engine::{
    Browsing, KeyRouting, NativeHandle, PageFocus, ParkedId, PhysRect, PromptLedger, RequestId, ShellHooks, TabId,
    Url,
};

use crate::client::Job;
use crate::events::Sink;
use crate::handlers::keyboard::KeyInput;
use crate::prompts::{PromptCallback, ScopeAtoms};
use crate::teardown::{LiveCount, Report, Teardown};

/// CEF's id for a browser (`Browser::identifier`).
pub type BrowserId = i32;

/// One request to CEF to create a browser. Every creation gets its own `Client`, which carries this
/// id into `on_after_created`, so the glue never has to guess which request a new browser answers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CreationId(pub u64);

/// CEF's two browser styles.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Style {
    /// A Chrome-style browser: every tab and popup.
    Chrome,
    /// An Alloy-style browser: extension popups and the hidden browsers.
    Alloy,
}

/// What a hidden browser is for (design section 3.7).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HiddenRole {
    /// The DevTools target for site-data clearing, from app start to shutdown step 5.
    Maintenance,
    /// The helper extension's bridge page (section 5.3).
    Bridge,
    /// A short-lived Chrome-style `chrome://extensions` page (section 5.4).
    Management,
    /// A short-lived extension page, for `uninstallSelf` (section 5.4).
    ExtensionPage,
}

/// What a browser is to the glue.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BrowserKind {
    /// A tab of the shell, including an allowed popup and an extension popup.
    Tab(TabId),
    /// A tab's browser kept alive for its downloads after `detach_tab` (section 4.2).
    Parked(ParkedId),
    /// The DevTools window of a tab.
    DevTools(TabId),
    /// A window Chromium opened on its own, through `default_client` (section 5.7).
    Foreign,
    /// A browser in the hidden host.
    Hidden(HiddenRole),
}

impl BrowserKind {
    /// User browsers close in shutdown phase one; the hidden ones in phase two.
    pub fn user(self) -> bool {
        !matches!(self, BrowserKind::Hidden(_))
    }

    /// The shell tab it is, if any.
    pub fn tab(self) -> Option<TabId> {
        match self {
            BrowserKind::Tab(tab) => Some(tab),
            _ => None,
        }
    }
}

/// A browser, as the glue uses it. The real one wraps `cef::Browser` (`client::CefBrowser`); the
/// tests' fake records calls and runs handler paths from inside them, as CEF does.
pub trait BrowserOps {
    /// Its native window.
    fn native(&self) -> NativeHandle;
    /// `close_browser`.
    fn close(&self, force: bool);
    /// The main frame's `load_url`.
    fn load(&self, url: &str);
    /// `reload` or `reload_ignore_cache`.
    fn reload(&self, bypass_cache: bool);
    /// `stop_load`.
    fn stop(&self);
    /// `go_back`.
    fn go_back(&self);
    /// `go_forward`.
    fn go_forward(&self);
    /// `was_hidden`.
    fn was_hidden(&self, hidden: bool);
    /// `notify_move_or_resize_started`.
    fn notify_moved(&self);
    /// `notify_screen_info_changed`.
    fn notify_screen_info_changed(&self);
    /// `set_focus`.
    fn set_focus(&self, focus: bool);
    /// `set_zoom_level`.
    fn set_zoom_level(&self, level: f64);
    /// `zoom_level`.
    fn zoom_level(&self) -> f64;
    /// `set_audio_muted`.
    fn set_audio_muted(&self, muted: bool);
    /// `print`.
    fn print(&self);
    /// `show_dev_tools` in its own window, created with the client of `creation`.
    fn show_devtools(&self, creation: CreationId, inspect_at: Option<(i32, i32)>);
    /// `close_dev_tools`.
    fn close_devtools(&self);
    /// `has_dev_tools`.
    fn has_devtools(&self) -> bool;
    /// `execute_chrome_command`.
    fn execute_chrome_command(&self, command: i32);
    /// `exit_fullscreen`.
    fn exit_fullscreen(&self);
    /// `start_download`.
    fn start_download(&self, url: &str);
    /// `send_key_event`.
    fn send_key(&self, key: &KeyInput);
    /// `execute_dev_tools_method`; false if CEF refused it.
    fn devtools_method(&self, message_id: i32, method: &str, params: &serde_json::Value) -> bool;
    /// `set_auto_resize_enabled`, for extension popups.
    fn set_auto_resize(&self, min: (u32, u32), max: (u32, u32));
    /// The main frame's URL.
    fn url(&self) -> String;
}

/// What CEF must be asked to create.
#[derive(Clone, Debug, PartialEq)]
pub struct Creation {
    /// Its id, carried by its client.
    pub id: CreationId,
    /// The parent window.
    pub parent: NativeHandle,
    /// Where in the parent, physical pixels.
    pub bounds: PhysRect,
    /// The parent's scale.
    pub scale: f32,
    /// Chrome or Alloy.
    pub style: Style,
    /// Its request context.
    pub browsing: Browsing,
    /// The first URL.
    pub url: String,
}

/// The CEF calls that are not calls on a browser. The real one is `client::CefBackend`.
pub trait Backend {
    /// `browser_host_create_browser`; false if CEF refused at once.
    fn create(&self, creation: &Creation) -> bool;
    /// Runs `task` on the main thread after `delay` (`post_delayed_task(TID_UI)`).
    fn post(&self, delay: Duration, task: Job);
    /// Creates (`true`) or drops (`false`) the private request context.
    fn set_private_context(&self, on: bool);
}

/// What happens once a requested browser exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AfterCreate {
    /// Hide the page at once (`was_hidden`).
    pub hidden: bool,
    /// Show this HTML (`InitialPage::Html`).
    pub html: Option<(String, Option<Url>)>,
    /// Size the page by its content (extension popups).
    pub auto_resize: Option<((u32, u32), (u32, u32))>,
    /// It answers `on_before_popup`: opener browser, CEF's popup id, opener tab, URL, disposition.
    pub popup: Option<(BrowserId, i32, TabId, Url, vsesvit_engine::OpenDisposition)>,
    /// Close it as soon as it exists (`close_tab` before creation finished).
    pub close: bool,
}

/// A creation CEF has not finished.
pub struct Pending {
    /// What it will be.
    pub kind: BrowserKind,
    /// Its context.
    pub browsing: Browsing,
    /// Its style.
    pub style: Style,
    /// Whether it counts as live already. DevTools windows count from `on_after_created`, because
    /// `show_dev_tools` cannot report a failure.
    pub counted: bool,
    /// When it was asked for.
    pub requested: Instant,
    /// What to do once it exists.
    pub then: AfterCreate,
}

/// A live browser.
pub struct Entry {
    /// The browser.
    pub page: Rc<dyn BrowserOps>,
    /// What it is.
    pub kind: BrowserKind,
    /// Its context.
    pub browsing: Browsing,
    /// Its style.
    pub style: Style,
    /// Its document generation and closing flag, readable from any thread (section 1.5, Scope).
    pub atoms: Arc<ScopeAtoms>,
    /// When it was created.
    pub since: Instant,
    /// The last callback seen, for the stuck-shutdown log.
    pub last_seen: &'static str,
    /// The shell started the navigation in flight (`load`, `reload`, back, forward); cleared at the
    /// next commit. `NavigationRequest::browser_initiated`.
    pub shell_navigation: bool,
}

/// A shutdown self-test case armed for a tab (design section 3.6, Tests).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Arm {
    /// Close this long after asked.
    DelayClose(Duration),
    /// Never close.
    PinOpen,
    /// Create only once shutdown began.
    HoldCreation,
}

/// The registry.
pub struct Registry {
    /// CEF's non-browser calls.
    pub backend: Rc<dyn Backend>,
    /// The event sink.
    pub sink: Sink,
    /// The shell's hooks.
    pub hooks: Option<Rc<dyn ShellHooks>>,
    /// Live browsers.
    pub entries: BTreeMap<BrowserId, Entry>,
    /// Each tab's browser.
    pub tabs: HashMap<TabId, BrowserId>,
    /// Creations in flight.
    pub pending: BTreeMap<CreationId, Pending>,
    /// Each tab's page-focus policy, published before its browser exists.
    pub focus: HashMap<TabId, PageFocus>,
    /// The keys handed to the shell while a page has focus.
    pub routing: KeyRouting,
    /// Open prompts (section 1.5).
    pub prompts: PromptLedger<BrowserId, Box<dyn PromptCallback>>,
    /// The shutdown state machine.
    pub teardown: Teardown,
    /// The process-wide hidden host, once the shell set it.
    pub hidden_host: Option<NativeHandle>,
    /// The private context exists.
    pub private_open: bool,
    /// The shell ended the private session; the context goes when its last browser has closed.
    pub private_ending: bool,
    /// DevTools method calls in flight, by message id.
    pub devtools: crate::devtools::Calls,
    /// Creations held until shutdown begins (`Arm::HoldCreation`).
    pub held: Vec<Creation>,
    /// Shutdown self-test arms by tab.
    pub arms: HashMap<TabId, Arm>,
    next_creation: u64,
    next_parked: u64,
}

impl Registry {
    /// An empty registry.
    pub fn new(backend: Rc<dyn Backend>, sink: Sink) -> Registry {
        Registry {
            backend,
            sink,
            hooks: None,
            entries: BTreeMap::new(),
            tabs: HashMap::new(),
            pending: BTreeMap::new(),
            focus: HashMap::new(),
            routing: KeyRouting::default(),
            prompts: PromptLedger::new(),
            teardown: Teardown::default(),
            hidden_host: None,
            private_open: false,
            private_ending: false,
            devtools: crate::devtools::Calls::default(),
            held: Vec::new(),
            arms: HashMap::new(),
            next_creation: 0,
            next_parked: 0,
        }
    }

    /// A fresh creation id.
    pub fn next_creation(&mut self) -> CreationId {
        self.next_creation += 1;
        CreationId(self.next_creation)
    }

    /// A fresh parked id.
    pub fn next_parked(&mut self) -> ParkedId {
        self.next_parked += 1;
        ParkedId(self.next_parked)
    }

    /// The live-browser count: every counted creation in flight and every live browser.
    pub fn count(&self) -> LiveCount {
        let mut count = LiveCount::default();
        let kinds = self.pending.values().filter(|p| p.counted).map(|p| p.kind).chain(self.entries.values().map(|e| e.kind));
        for kind in kinds {
            if kind.user() {
                count.user += 1;
            } else {
                count.internal += 1;
            }
        }
        count
    }

    /// The tab's browser entry.
    pub fn tab_entry(&self, tab: TabId) -> Option<&Entry> {
        self.entries.get(self.tabs.get(&tab)?)
    }

    /// The tab a browser is, if any.
    pub fn tab_of(&self, browser: BrowserId) -> Option<TabId> {
        self.entries.get(&browser)?.kind.tab()
    }

    /// The tab's policy; a tab nobody published one for is refused like a new browser.
    pub fn focus_of(&self, tab: TabId) -> PageFocus {
        self.focus.get(&tab).copied().unwrap_or(PageFocus::NEW_BROWSER)
    }

    /// Whether a private browser is alive or in flight.
    pub fn private_alive(&self) -> bool {
        self.entries.values().any(|e| e.browsing == Browsing::Private)
            || self.pending.values().any(|p| p.browsing == Browsing::Private)
            || self.held.iter().any(|c| c.browsing == Browsing::Private)
    }

    /// One line per live browser and creation in flight, for the stuck-shutdown log (section 3.6).
    pub fn live_report(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .entries
            .iter()
            .map(|(id, e)| {
                let url = e.page.url();
                format!(
                    "browser {id}: {:?}, {:?}, {}, alive {:?}, last callback {}",
                    e.kind,
                    e.style,
                    e.browsing.loggable(&url),
                    e.since.elapsed(),
                    e.last_seen
                )
            })
            .collect();
        lines.extend(self.pending.iter().filter(|(_, p)| p.counted).map(|(id, p)| {
            format!("creation {}: {:?}, {:?}, asked {:?} ago, never created", id.0, p.kind, p.style, p.requested.elapsed())
        }));
        lines
    }
}

thread_local! {
    static REGISTRY: RefCell<Option<Registry>> = const { RefCell::new(None) };
    static CONFLICTS: Cell<u32> = const { Cell::new(0) };
    static WARNED: Cell<bool> = const { Cell::new(false) };
}

/// Installs the registry on this thread, which becomes the engine's main thread. At `initialize`.
pub fn install(registry: Registry) {
    REGISTRY.with(|cell| *cell.borrow_mut() = Some(registry));
}

/// Takes the registry out, emptying it (shutdown step 8, before `cef::shutdown`). Dropping the value
/// drops every browser, callback and context reference the glue held.
pub fn take() -> Option<Registry> {
    REGISTRY.with(|cell| cell.try_borrow_mut().ok().and_then(|mut r| r.take()))
}

/// Whether this thread holds the registry.
pub fn on_main() -> bool {
    REGISTRY.with(|cell| cell.try_borrow().map(|r| r.is_some()).unwrap_or(true))
}

/// Runs `f` on the registry. `None` on a thread without it (logged once per thread) and while it is
/// already borrowed (logged and counted). `f` must not call CEF, a hook or a browser: clone out, return,
/// then call.
pub fn with<R>(f: impl FnOnce(&mut Registry) -> R) -> Option<R> {
    REGISTRY.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => match guard.as_mut() {
            Some(registry) => Some(f(registry)),
            None => {
                if !WARNED.replace(true) {
                    log::error!("vsesvit-cef: the registry was used off the main thread (or after shutdown); ignored");
                }
                None
            }
        },
        Err(_) => {
            CONFLICTS.set(CONFLICTS.get() + 1);
            log::error!("vsesvit-cef: the registry was re-entered while borrowed; the call was dropped");
            None
        }
    })
}

/// How often [`with`] was refused because the registry was borrowed. Zero unless a path holds a
/// borrow across a call out (a bug).
pub fn conflicts() -> u32 {
    CONFLICTS.get()
}

/// The event sink.
pub fn sink() -> Option<Sink> {
    with(|r| r.sink.clone())
}

/// CEF's non-browser calls.
pub fn backend() -> Option<Rc<dyn Backend>> {
    with(|r| r.backend.clone())
}

/// The browser of a tab, cloned out of the registry.
pub fn page_of(tab: TabId) -> Option<Rc<dyn BrowserOps>> {
    with(|r| r.tab_entry(tab).map(|e| e.page.clone())).flatten()
}

/// A browser by id, cloned out of the registry.
pub fn browser(id: BrowserId) -> Option<Rc<dyn BrowserOps>> {
    with(|r| r.entries.get(&id).map(|e| e.page.clone())).flatten()
}

/// Calls `f` on the tab's browser, outside any registry borrow. Calls naming an unknown tab are
/// ignored (the `Engine` contract).
pub fn on_tab<R>(tab: TabId, f: impl FnOnce(&dyn BrowserOps) -> R) -> Option<R> {
    let page = page_of(tab)?;
    Some(f(&*page))
}

/// The tab a browser is.
pub fn tab_of(browser: BrowserId) -> Option<TabId> {
    with(|r| r.tab_of(browser)).flatten()
}

/// Records the callback seen last on a browser.
pub fn seen(browser: BrowserId, callback: &'static str) {
    with(|r| {
        if let Some(e) = r.entries.get_mut(&browser) {
            e.last_seen = callback;
        }
    });
}

/// The shell's hooks, cloned out.
pub fn hooks() -> Option<Rc<dyn ShellHooks>> {
    with(|r| r.hooks.clone()).flatten()
}

/// Announces the shutdown reports now due (`UserBrowsersClosed`, `AllBrowsersClosed`).
pub fn check_teardown() {
    let due = with(|r| {
        let count = r.count();
        (r.teardown.on_count(count), r.sink.clone())
    });
    let Some((due, sink)) = due else { return };
    for report in due {
        sink.send(match report {
            Report::UserBrowsersClosed => vsesvit_engine::BrowserEvent::UserBrowsersClosed,
            Report::AllBrowsersClosed => vsesvit_engine::BrowserEvent::AllBrowsersClosed,
        });
    }
}

/// Request ids for the operations that answer later.
pub fn next_request() -> RequestId {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    RequestId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// A request answered at once (`RequestFinished` is queued, so it still arrives after the call).
pub fn answer_now(result: vsesvit_engine::RequestResult) -> RequestId {
    let id = next_request();
    if let Some(sink) = sink() {
        sink.send(vsesvit_engine::BrowserEvent::RequestFinished { id, result });
    }
    id
}

/// A navigation the shell starts on a tab: marked browser-initiated, then `f` runs on the browser
/// outside the borrow.
pub fn navigate(tab: TabId, f: impl FnOnce(&dyn BrowserOps)) {
    let page = with(|r| {
        let id = *r.tabs.get(&tab)?;
        let entry = r.entries.get_mut(&id)?;
        entry.shell_navigation = true;
        Some(entry.page.clone())
    })
    .flatten();
    if let Some(page) = page {
        f(&*page);
    }
}

/// Arms a shutdown self-test case for a tab, before it is created.
pub fn arm(tab: TabId, arm: Arm) {
    with(|r| r.arms.insert(tab, arm));
}

/// The tests' CEF: a backend that records creations and a browser that records calls and can run
/// handler paths from inside them, as CEF does.
#[cfg(test)]
pub mod testing {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::time::Duration;

    use futures::channel::mpsc::UnboundedReceiver;
    use vsesvit_engine::{BrowserEvent, NativeHandle};

    use super::*;
    use crate::events::Sink;

    /// Records creations and posted tasks; refuses creations when told to.
    #[derive(Default)]
    pub struct FakeBackend {
        /// Every creation asked for.
        pub created: RefCell<Vec<Creation>>,
        /// Refuse the next creations.
        pub refuse: Cell<bool>,
        /// Tasks posted to main, with their delay.
        pub posted: RefCell<Vec<(Duration, Job)>>,
        /// The private context's state changes.
        pub private: RefCell<Vec<bool>>,
    }

    impl Backend for FakeBackend {
        fn create(&self, creation: &Creation) -> bool {
            self.created.borrow_mut().push(creation.clone());
            !self.refuse.get()
        }

        fn post(&self, delay: Duration, task: Job) {
            self.posted.borrow_mut().push((delay, task));
        }

        fn set_private_context(&self, on: bool) {
            self.private.borrow_mut().push(on);
        }
    }

    /// A test's engine: the fake backend and the event receiver.
    pub struct Harness {
        /// The backend.
        pub backend: Rc<FakeBackend>,
        /// The events the shell would drain.
        pub events: UnboundedReceiver<BrowserEvent>,
    }

    impl Harness {
        /// Every event queued so far.
        pub fn events(&mut self) -> Vec<BrowserEvent> {
            let mut out = Vec::new();
            while let Ok(event) = self.events.try_recv() {
                out.push(event);
            }
            out
        }

        /// The last creation asked for.
        pub fn last_creation(&self) -> Creation {
            self.backend.created.borrow().last().cloned().expect("a creation was asked for")
        }

        /// Runs the tasks posted so far.
        pub fn run_posted(&self) -> usize {
            let tasks: Vec<_> = self.backend.posted.borrow_mut().drain(..).collect();
            let n = tasks.len();
            for (_, task) in tasks {
                task();
            }
            n
        }

        /// CEF finished `creation`: `on_after_created` with a fake browser.
        pub fn created(&self, creation: CreationId) -> Rc<FakeBrowser> {
            let page = FakeBrowser::new();
            crate::handlers::life_span::after_created(Some(creation), page.clone(), page.id);
            page
        }
    }

    /// Installs a fresh registry on this thread (each test thread has its own).
    pub fn install() -> Harness {
        let backend = Rc::new(FakeBackend::default());
        let (sink, events) = Sink::new();
        super::install(Registry::new(backend.clone(), sink));
        Harness { backend, events }
    }

    /// What a fake browser does from inside a call, as CEF would: run handler paths.
    pub type React = Rc<dyn Fn(&FakeBrowser, &str)>;

    /// A browser that records every call.
    pub struct FakeBrowser {
        /// Its id, unique in the test process.
        pub id: BrowserId,
        /// The calls, by name.
        pub calls: RefCell<Vec<String>>,
        /// Runs after each call is recorded.
        pub react: RefCell<Option<React>>,
        /// The DevTools state.
        pub devtools: Cell<bool>,
    }

    impl FakeBrowser {
        /// A new browser.
        pub fn new() -> Rc<FakeBrowser> {
            static NEXT: AtomicI32 = AtomicI32::new(1000);
            Rc::new(FakeBrowser {
                id: NEXT.fetch_add(1, Ordering::Relaxed),
                calls: RefCell::new(Vec::new()),
                react: RefCell::new(None),
                devtools: Cell::new(false),
            })
        }

        fn call(&self, name: &str) {
            self.calls.borrow_mut().push(name.to_owned());
            let react = self.react.borrow().clone();
            if let Some(react) = react {
                react(self, name);
            }
        }

        /// Whether a call was made.
        pub fn called(&self, name: &str) -> bool {
            self.calls.borrow().iter().any(|c| c == name)
        }
    }

    impl BrowserOps for FakeBrowser {
        fn native(&self) -> NativeHandle {
            NativeHandle::from_raw(0x10_0000 + self.id as u64)
        }
        fn close(&self, force: bool) {
            self.call(if force { "close(force)" } else { "close" });
        }
        fn load(&self, url: &str) {
            self.call(&format!("load {url}"));
        }
        fn reload(&self, _bypass_cache: bool) {
            self.call("reload");
        }
        fn stop(&self) {
            self.call("stop");
        }
        fn go_back(&self) {
            self.call("go_back");
        }
        fn go_forward(&self) {
            self.call("go_forward");
        }
        fn was_hidden(&self, hidden: bool) {
            self.call(&format!("was_hidden {hidden}"));
        }
        fn notify_moved(&self) {
            self.call("notify_moved");
        }
        fn notify_screen_info_changed(&self) {
            self.call("notify_screen_info_changed");
        }
        fn set_focus(&self, focus: bool) {
            self.call(&format!("set_focus {focus}"));
        }
        fn set_zoom_level(&self, level: f64) {
            self.call(&format!("set_zoom_level {level:.3}"));
        }
        fn zoom_level(&self) -> f64 {
            self.call("zoom_level");
            0.0
        }
        fn set_audio_muted(&self, muted: bool) {
            self.call(&format!("set_audio_muted {muted}"));
        }
        fn print(&self) {
            self.call("print");
        }
        fn show_devtools(&self, _creation: CreationId, _inspect_at: Option<(i32, i32)>) {
            self.devtools.set(true);
            self.call("show_devtools");
        }
        fn close_devtools(&self) {
            self.devtools.set(false);
            self.call("close_devtools");
        }
        fn has_devtools(&self) -> bool {
            self.devtools.get()
        }
        fn execute_chrome_command(&self, command: i32) {
            self.call(&format!("execute_chrome_command {command}"));
        }
        fn exit_fullscreen(&self) {
            self.call("exit_fullscreen");
        }
        fn start_download(&self, url: &str) {
            self.call(&format!("start_download {url}"));
        }
        fn send_key(&self, key: &KeyInput) {
            self.call(&format!("send_key {:?} {:#x}", key.kind, key.windows_key_code));
        }
        fn devtools_method(&self, _message_id: i32, method: &str, _params: &serde_json::Value) -> bool {
            self.call(&format!("devtools {method}"));
            true
        }
        fn set_auto_resize(&self, _min: (u32, u32), _max: (u32, u32)) {
            self.call("set_auto_resize");
        }
        fn url(&self) -> String {
            "https://example.com/".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_thread_gets_nothing_and_never_panics() {
        crate::registry::testing::install();
        assert!(with(|_| ()).is_some());
        let other = std::thread::spawn(|| (with(|_| ()).is_none(), on_main())).join().expect("no panic");
        assert_eq!(other, (true, false));
        assert!(on_main());
    }

    #[test]
    fn a_nested_borrow_is_refused_and_counted() {
        crate::registry::testing::install();
        let before = conflicts();
        let inner = with(|_| with(|_| ()));
        assert_eq!(inner, Some(None));
        assert_eq!(conflicts(), before + 1);
    }

    #[test]
    fn the_count_splits_user_and_internal_and_includes_creations() {
        crate::registry::testing::install();
        let tab = TabId::next();
        with(|r| {
            let a = r.next_creation();
            let b = r.next_creation();
            let c = r.next_creation();
            let pending = |kind, counted| Pending {
                kind,
                browsing: Browsing::Normal,
                style: Style::Chrome,
                counted,
                requested: Instant::now(),
                then: AfterCreate::default(),
            };
            r.pending.insert(a, pending(BrowserKind::Tab(tab), true));
            r.pending.insert(b, pending(BrowserKind::Hidden(HiddenRole::Maintenance), true));
            r.pending.insert(c, pending(BrowserKind::DevTools(tab), false));
            assert_eq!(r.count(), LiveCount { user: 1, internal: 1 });
        });
    }
}
