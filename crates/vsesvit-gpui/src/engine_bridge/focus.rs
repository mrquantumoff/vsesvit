//! Owned by W1.6.
//! The page-focus policy (design section 1.5), the one caller of `SurfaceOps::focus` and of the
//! engine's `set_page_focus`, `focus_page` and `blur_page`.
//!
//! Each tab's policy is a function of three facts about its window: is it active, is the tab its
//! selected tab, and who has the keyboard. [`Policy`] keeps those facts and, after every change,
//! answers with the [`Effect`]s that bring the engine and the native windows in line: the policies
//! that changed, then at most one native focus move. [`Focus`] runs the effects with no borrow held,
//! because a focus call can run engine callbacks and window procedures synchronously (section 1.6).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{App, BackgroundExecutor, WindowId};
use vsesvit_engine::{Engine, KeyboardOwner, NativeHandle, PageFocus, PhysRect, TabId};

use crate::platform::{FocusOutcome, FocusTarget, PhysPoint, SurfaceOps};

/// A window's surface, shared between its `BrowserWindow` (layout) and [`Focus`] (focus moves).
/// A focus move can run window procedures synchronously, so the window's own code takes it with
/// `try_borrow_mut` and never holds it across a call into the shell; `Focus` does the same.
pub type SharedSurface = Rc<RefCell<Box<dyn SurfaceOps>>>;

/// How old a delivered pointer press may be and still explain a page taking focus by itself
/// (section 1.5: Chromium can take X focus before the press reaches the shell).
pub const PRESS_WINDOW: Duration = Duration::from_millis(500);

/// Where a window's selected page sits on the screen, for pointer presses: the GPUI window's own
/// native handle (`Desktop::toplevel_at` names it under the pointer), the page's rectangle and the
/// interactive overlays over it (the surface's interactive holes), in screen pixels. A press on an
/// overlay is GPUI's, not the page's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageArea {
    pub toplevel: NativeHandle,
    pub rect: PhysRect,
    pub holes: Vec<PhysRect>,
}

fn inside(r: &PhysRect, at: PhysPoint) -> bool {
    at.x >= r.x && at.y >= r.y && i64::from(at.x) < i64::from(r.x) + i64::from(r.width) && i64::from(at.y) < i64::from(r.y) + i64::from(r.height)
}

impl PageArea {
    fn contains(&self, at: PhysPoint, toplevel: Option<NativeHandle>) -> bool {
        toplevel == Some(self.toplevel) && inside(&self.rect, at) && !self.holes.iter().any(|h| inside(h, at))
    }
}

/// A press with the toplevel `Desktop::toplevel_at` found under it, read once when it arrived.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Press {
    pub at: PhysPoint,
    pub toplevel: Option<NativeHandle>,
}

/// What [`Focus`] does to make the engine and the native windows match the facts, in order.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    /// `Engine::set_page_focus`.
    Publish(TabId, PageFocus),
    /// The keyboard goes to GPUI's widgets in this window.
    FocusChrome(WindowId),
    /// The keyboard goes to the page: `SurfaceOps::focus(Page)`, then `focus_page` once it is done.
    FocusPage { window: WindowId, tab: TabId, native: NativeHandle },
    /// `Engine::blur_page`.
    Blur(TabId),
}

#[derive(Debug)]
struct WindowFacts {
    active: bool,
    keyboard: KeyboardOwner,
    selected: Option<TabId>,
    area: Option<PageArea>,
    /// The page last given the keyboard while the window was active, so it is focused once per change.
    focused_page: Option<TabId>,
}

#[derive(Debug, Default)]
struct TabFacts {
    window: Option<WindowId>,
    native: Option<NativeHandle>,
    published: Option<PageFocus>,
}

/// The focus facts and the policy they imply. Pure: every method returns the effects to run.
#[derive(Debug, Default)]
pub(crate) struct Policy {
    windows: HashMap<WindowId, WindowFacts>,
    tabs: BTreeMap<TabId, TabFacts>,
    /// Delivered presses younger than [`PRESS_WINDOW`].
    recent: Vec<(Press, Instant)>,
}

impl Policy {
    /// The policy the facts give a tab. A tab in no known window is refused, as a new browser is.
    pub fn of(&self, tab: TabId) -> PageFocus {
        let window = self.tabs.get(&tab).and_then(|t| t.window).and_then(|w| self.windows.get(&w));
        match window {
            Some(w) => PageFocus { window_active: w.active, tab_selected: w.selected == Some(tab), keyboard: w.keyboard },
            None => PageFocus::NEW_BROWSER,
        }
    }

    pub fn add_window(&mut self, window: WindowId) {
        self.windows.entry(window).or_insert(WindowFacts {
            active: false,
            keyboard: KeyboardOwner::Chrome,
            selected: None,
            area: None,
            focused_page: None,
        });
    }

    pub fn remove_window(&mut self, window: WindowId) {
        self.windows.remove(&window);
        for tab in self.tabs.values_mut().filter(|t| t.window == Some(window)) {
            tab.window = None;
        }
    }

    pub fn window_activated(&mut self, window: WindowId, active: bool) -> Vec<Effect> {
        let Some(w) = self.windows.get_mut(&window) else { return Vec::new() };
        w.active = active;
        if !active {
            w.focused_page = None;
        }
        self.sync(window)
    }

    /// The tab is now in `window`, selected or not (an insert, a move from another window, an adopted
    /// popup). A tab selected in another window stops being selected there.
    pub fn join(&mut self, window: WindowId, tab: TabId) -> Vec<Effect> {
        let left = self.place(window, tab);
        self.sync_both(left, window)
    }

    pub fn select(&mut self, window: WindowId, tab: Option<TabId>) -> Vec<Effect> {
        let left = tab.and_then(|tab| self.place(window, tab));
        if let Some(w) = self.windows.get_mut(&window) {
            w.selected = tab;
        }
        self.sync_both(left, window)
    }

    /// Puts the tab in `window`, and returns the window it left, which no longer selects it.
    fn place(&mut self, window: WindowId, tab: TabId) -> Option<WindowId> {
        let before = self.tabs.entry(tab).or_default().window.replace(window);
        let old = before.filter(|&old| old != window)?;
        if let Some(w) = self.windows.get_mut(&old)
            && w.selected == Some(tab)
        {
            w.selected = None;
            w.focused_page = None;
        }
        Some(old)
    }

    fn sync_both(&mut self, left: Option<WindowId>, window: WindowId) -> Vec<Effect> {
        let mut effects = left.map(|old| self.sync(old)).unwrap_or_default();
        effects.extend(self.sync(window));
        effects
    }

    /// A new layout. It also retries a page focus the OS refused, as when the page was not shown yet.
    pub fn set_page_area(&mut self, window: WindowId, area: Option<PageArea>) -> Vec<Effect> {
        let Some(w) = self.windows.get_mut(&window) else { return Vec::new() };
        w.area = area;
        self.sync(window)
    }

    /// The OS refused to give the page the keyboard; the next change of the facts tries again.
    pub fn focus_failed(&mut self, window: WindowId, tab: TabId) {
        if let Some(w) = self.windows.get_mut(&window)
            && w.focused_page == Some(tab)
        {
            w.focused_page = None;
        }
    }

    /// The keyboard goes to GPUI (`Chrome`, `Overlay`) or to the selected page (`Page`). Leaving the
    /// page also forgets earlier presses, so an old click cannot explain the page taking focus back.
    /// An overlay that is an extension popup gets its keyboard back from W4.3, not from here: a
    /// refused page focus always returns to GPUI.
    pub fn set_keyboard(&mut self, window: WindowId, owner: KeyboardOwner) -> Vec<Effect> {
        if owner != KeyboardOwner::Page {
            self.recent.clear();
        }
        let Some(w) = self.windows.get_mut(&window) else { return Vec::new() };
        w.keyboard = owner;
        let (active, left) = (w.active, if owner == KeyboardOwner::Page { None } else { w.focused_page.take() });
        let mut effects = self.sync(window);
        if owner != KeyboardOwner::Page && active {
            effects.push(Effect::FocusChrome(window));
        }
        effects.extend(left.map(Effect::Blur));
        effects
    }

    /// The page asks for the keyboard: a click into it, or the shell's own `focus_page`. Only the
    /// selected tab of a window can have it.
    pub fn page_wants_keyboard(&mut self, tab: TabId) -> Vec<Effect> {
        let Some(window) = self.tabs.get(&tab).and_then(|t| t.window) else { return Vec::new() };
        if self.windows.get(&window).and_then(|w| w.selected) != Some(tab) {
            log::debug!("focus: tab {} is not selected, so it does not get the keyboard", tab.get());
            return Vec::new();
        }
        self.set_keyboard(window, KeyboardOwner::Page)
    }

    /// The page gives the keyboard up (`Engine::blur_page`, `TabEvent::TakeFocus`): GPUI takes it.
    pub fn page_gives_up_keyboard(&mut self, tab: TabId) -> Vec<Effect> {
        let Some(window) = self.tabs.get(&tab).and_then(|t| t.window) else { return vec![Effect::Blur(tab)] };
        match self.windows.get(&window) {
            Some(w) if w.selected == Some(tab) && w.keyboard == KeyboardOwner::Page => self.set_keyboard(window, KeyboardOwner::Chrome),
            _ => Vec::new(),
        }
    }

    /// Before `create_tab`: every new browser starts refused.
    pub fn before_create(&mut self, tab: TabId) -> Vec<Effect> {
        self.tabs.entry(tab).or_default().published = Some(PageFocus::NEW_BROWSER);
        vec![Effect::Publish(tab, PageFocus::NEW_BROWSER)]
    }

    /// Before every navigation the shell starts: the tab's policy, published even if unchanged.
    pub fn before_navigation(&mut self, tab: TabId) -> Vec<Effect> {
        let policy = self.of(tab);
        self.tabs.entry(tab).or_default().published = Some(policy);
        vec![Effect::Publish(tab, policy)]
    }

    /// `TabEvent::Created`: the page's window exists, so it can be focused.
    pub fn created(&mut self, tab: TabId, native: NativeHandle) -> Vec<Effect> {
        let facts = self.tabs.entry(tab).or_default();
        facts.native = Some(native);
        match facts.window {
            Some(window) => self.sync(window),
            None => Vec::new(),
        }
    }

    /// `TabEvent::Closed` or `Detached`: the tab is gone.
    pub fn gone(&mut self, tab: TabId) {
        let Some(facts) = self.tabs.remove(&tab) else { return };
        if let Some(w) = facts.window.and_then(|w| self.windows.get_mut(&w)) {
            if w.selected == Some(tab) {
                w.selected = None;
            }
            if w.focused_page == Some(tab) {
                w.focused_page = None;
            }
        }
    }

    /// A pointer press anywhere. Inside the selected page of the active window it hands that page the
    /// keyboard.
    pub fn pressed(&mut self, press: Press, now: Instant) -> Vec<Effect> {
        self.recent.retain(|(_, at)| now.saturating_duration_since(*at) < PRESS_WINDOW);
        self.recent.push((press, now));
        let target = self
            .windows
            .values()
            .find(|w| w.active && w.area.as_ref().is_some_and(|a| a.contains(press.at, press.toplevel)))
            .and_then(|w| w.selected);
        match target {
            Some(tab) => self.page_wants_keyboard(tab),
            None => Vec::new(),
        }
    }

    /// `TabEvent::FocusChanged { focused: true }`. Expected when the published policy allows it.
    /// Otherwise the presses still pending come first, then a press into the page less than
    /// [`PRESS_WINDOW`] old makes it a click; failing both, the keyboard goes back to its owner.
    pub fn page_took_focus(&mut self, tab: TabId, pending: &[Press], now: Instant) -> Vec<Effect> {
        if self.published_allows(tab) {
            return Vec::new();
        }
        let mut effects: Vec<Effect> = pending.iter().flat_map(|&p| self.pressed(p, now)).collect();
        if self.published_allows(tab) {
            return effects;
        }
        let window = self.tabs.get(&tab).and_then(|t| t.window);
        let facts = window.and_then(|w| self.windows.get(&w));
        let clicked = facts.is_some_and(|w| {
            w.selected == Some(tab)
                && w.area.as_ref().is_some_and(|a| {
                    self.recent.iter().any(|(p, at)| now.saturating_duration_since(*at) < PRESS_WINDOW && a.contains(p.at, p.toplevel))
                })
        });
        if clicked {
            effects.extend(self.page_wants_keyboard(tab));
            return effects;
        }
        log::info!("focus: tab {} took the keyboard without the policy's leave; giving it back", tab.get());
        effects.push(Effect::Blur(tab));
        let (Some(window), Some(w)) = (window, facts) else { return effects };
        let owner_page = (w.keyboard == KeyboardOwner::Page).then_some(w.selected).flatten();
        match owner_page.and_then(|t| self.tabs.get(&t).and_then(|f| f.native).map(|n| (t, n))) {
            Some((owner, native)) if owner != tab => effects.push(Effect::FocusPage { window, tab: owner, native }),
            _ => effects.push(Effect::FocusChrome(window)),
        }
        effects
    }

    fn published_allows(&self, tab: TabId) -> bool {
        self.tabs.get(&tab).and_then(|t| t.published).is_some_and(PageFocus::allows)
    }

    /// Publishes every changed policy of the window's tabs, then focuses the page that should have
    /// the keyboard if it changed.
    fn sync(&mut self, window: WindowId) -> Vec<Effect> {
        let mut effects = Vec::new();
        let members: Vec<TabId> = self.tabs.iter().filter(|(_, t)| t.window == Some(window)).map(|(&id, _)| id).collect();
        for tab in members {
            let policy = self.of(tab);
            let facts = self.tabs.get_mut(&tab).expect("a member");
            if facts.published != Some(policy) {
                facts.published = Some(policy);
                effects.push(Effect::Publish(tab, policy));
            }
        }
        let Some(w) = self.windows.get(&window) else { return effects };
        let target = if w.active && w.keyboard == KeyboardOwner::Page { w.selected } else { None };
        let target = target.and_then(|t| self.tabs.get(&t).and_then(|f| f.native).map(|n| (t, n)));
        let w = self.windows.get_mut(&window).expect("looked up above");
        if target.map(|(t, _)| t) != w.focused_page {
            w.focused_page = target.map(|(t, _)| t);
            if let Some((tab, native)) = target {
                effects.push(Effect::FocusPage { window, tab, native });
            }
        }
        effects
    }
}

struct Inner {
    engine: Rc<dyn Engine>,
    policy: RefCell<Policy>,
    surfaces: RefCell<HashMap<WindowId, SharedSurface>>,
    presses: RefCell<UnboundedReceiver<PhysPoint>>,
    toplevel_at: Box<dyn Fn(PhysPoint) -> Option<NativeHandle>>,
    clock: BackgroundExecutor,
}

/// The shell's handle on the focus policy. Main thread only; cheap to clone.
#[derive(Clone)]
pub struct Focus(Rc<Inner>);

impl Focus {
    /// `engine` is the engine itself, not the guarded one the rest of the shell holds.
    pub(crate) fn new(
        engine: Rc<dyn Engine>,
        presses: UnboundedReceiver<PhysPoint>,
        toplevel_at: impl Fn(PhysPoint) -> Option<NativeHandle> + 'static,
        clock: BackgroundExecutor,
    ) -> Focus {
        Focus(Rc::new(Inner {
            engine,
            policy: RefCell::default(),
            surfaces: RefCell::default(),
            presses: RefCell::new(presses),
            toplevel_at: Box::new(toplevel_at),
            clock,
        }))
    }

    /// Delivers `Desktop::pointer_presses` to the policy as they arrive.
    pub(crate) fn watch_presses(&self, cx: &mut App) {
        let focus = self.clone();
        cx.spawn(async move |_| {
            while let Some(at) = futures::future::poll_fn(|task| focus.0.presses.borrow_mut().poll_next_unpin(task)).await {
                focus.pointer_pressed(at);
            }
        })
        .detach();
    }

    /// A browser window opened; it starts inactive with the keyboard in its chrome.
    pub fn add_window(&self, window: WindowId, surface: SharedSurface) {
        self.0.surfaces.borrow_mut().insert(window, surface);
        self.0.policy.borrow_mut().add_window(window);
    }

    /// The window closed. Its tabs are closed, parked or moved by then.
    pub fn remove_window(&self, window: WindowId) {
        self.0.surfaces.borrow_mut().remove(&window);
        self.0.policy.borrow_mut().remove_window(window);
    }

    /// GPUI's activation of the window changed.
    pub fn window_activated(&self, window: WindowId, active: bool) {
        self.apply(|p| p.window_activated(window, active));
    }

    /// The window's selected tab changed (or it has none).
    pub fn select_tab(&self, window: WindowId, tab: Option<TabId>) {
        self.apply(|p| p.select(window, tab));
    }

    /// A tab joined the window without being selected: a background tab, a tab moved in.
    pub fn join(&self, window: WindowId, tab: TabId) {
        self.apply(|p| p.join(window, tab));
    }

    /// Where the window's selected page is on the screen; `None` while it shows none. Updated on
    /// every layout (after `show_page`) and window move. A page focus the OS refused is retried here.
    pub fn set_page_area(&self, window: WindowId, area: Option<PageArea>) {
        self.apply(|p| p.set_page_area(window, area));
    }

    /// The omnibox, the tab strip or another GPUI widget took the keyboard.
    pub fn chrome_wants_keyboard(&self, window: WindowId) {
        self.apply(|p| p.set_keyboard(window, KeyboardOwner::Chrome));
    }

    /// An interactive overlay or an extension popup took the keyboard.
    pub fn overlay_wants_keyboard(&self, window: WindowId) {
        self.apply(|p| p.set_keyboard(window, KeyboardOwner::Overlay));
    }

    /// The tab's page should have the keyboard (it is selected; nothing happens otherwise).
    pub fn page_wants_keyboard(&self, tab: TabId) {
        self.apply(|p| p.page_wants_keyboard(tab));
    }

    /// The page gave the keyboard up; GPUI takes it.
    pub fn page_gives_up_keyboard(&self, tab: TabId) {
        self.apply(|p| p.page_gives_up_keyboard(tab));
    }

    /// The last published policy of a tab, for tests and the self-test.
    pub fn published(&self, tab: TabId) -> Option<PageFocus> {
        self.0.policy.borrow().tabs.get(&tab).and_then(|t| t.published)
    }

    /// Whether the tab's browser exists (`TabEvent::Created` arrived and no close since).
    pub(crate) fn has_page(&self, tab: TabId) -> bool {
        self.0.policy.borrow().tabs.get(&tab).is_some_and(|t| t.native.is_some())
    }

    pub(crate) fn before_create(&self, tab: TabId) {
        self.apply(|p| p.before_create(tab));
    }

    pub(crate) fn before_navigation(&self, tab: TabId) {
        self.apply(|p| p.before_navigation(tab));
    }

    pub(crate) fn created(&self, tab: TabId, native: NativeHandle) {
        self.apply(|p| p.created(tab, native));
    }

    pub(crate) fn gone(&self, tab: TabId) {
        self.0.policy.borrow_mut().gone(tab);
    }

    pub(crate) fn pointer_pressed(&self, at: PhysPoint) {
        let press = Press { at, toplevel: (self.0.toplevel_at)(at) };
        let now = self.0.clock.now();
        self.apply(|p| p.pressed(press, now));
    }

    /// `TabEvent::FocusChanged { focused: true }`, with the presses still queued drained first.
    pub(crate) fn page_took_focus(&self, tab: TabId) {
        let mut pending = Vec::new();
        {
            let mut presses = self.0.presses.borrow_mut();
            while let Ok(at) = presses.try_recv() {
                pending.push(at);
            }
        }
        let pending: Vec<Press> = pending.into_iter().map(|at| Press { at, toplevel: (self.0.toplevel_at)(at) }).collect();
        let now = self.0.clock.now();
        self.apply(|p| p.page_took_focus(tab, &pending, now));
    }

    fn apply(&self, change: impl FnOnce(&mut Policy) -> Vec<Effect>) {
        let effects = change(&mut self.0.policy.borrow_mut());
        for effect in effects {
            self.run(effect);
        }
    }

    /// One effect, with no borrow of the policy held: engine and surface calls can re-enter the shell.
    fn run(&self, effect: Effect) {
        let engine = &self.0.engine;
        match effect {
            Effect::Publish(tab, policy) => engine.set_page_focus(tab, policy),
            Effect::Blur(tab) => engine.blur_page(tab),
            Effect::FocusChrome(window) => {
                self.focus_surface(window, FocusTarget::Chrome);
            }
            Effect::FocusPage { window, tab, native } => match self.focus_surface(window, FocusTarget::Page(native)) {
                Some(FocusOutcome::Done) => engine.focus_page(tab),
                outcome => {
                    log::info!("focus: tab {} did not get the keyboard ({outcome:?})", tab.get());
                    self.0.policy.borrow_mut().focus_failed(window, tab);
                }
            },
        }
    }

    fn focus_surface(&self, window: WindowId, target: FocusTarget) -> Option<FocusOutcome> {
        let surface = self.0.surfaces.borrow().get(&window).cloned()?;
        let Ok(mut surface) = surface.try_borrow_mut() else {
            log::error!("focus: the window's surface is borrowed; focus not moved to {target:?}");
            return None;
        };
        Some(surface.focus(target))
    }
}
