//! Owned by W1.6.
//! The bridge with `FakeEngine` and the fake platform: the page-focus policy (design section 1.5),
//! prompts, requests, the router and the pump's handle (W1.6's done-when).

use std::cell::{Ref, RefCell};
use std::ffi::OsString;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui::{App, Context, IntoElement, Render, TestAppContext, Window, WindowId};
use vsesvit_engine::{
    AuthChallenger, Browsing, BrowserEvent, ContextMenuParams, Engine, FakeCommand, FakeEngine, FileChooserMode,
    InitialPage, JsDialogKind, KeyboardOwner, MenuModel, NativeHandle, OpenDisposition, PageFocus, PagePoint, PermissionRequest,
    PhysRect, PromptAnswer, PromptEnd, PromptError, PromptRequest, RequestResult, RoutedKey, TabEvent, TabId, TabKind, TabSpec,
    Url,
};

use super::prompts::{self, Owner};
use super::*;
use crate::platform::fake::{self, FakeDesktop, FakeSurface};
use crate::platform::{Desktop, FocusTarget, PhysPoint};

#[derive(Clone, Debug, PartialEq)]
enum Routed {
    Tab(TabId, TabEvent),
    Popup(TabId, TabId),
    OpenUrl(TabId, Url),
    Shortcut(Option<TabId>, RoutedKey),
    Relaunch(Vec<OsString>),
    UserBrowsersClosed,
    AllBrowsersClosed,
}

#[derive(Default)]
struct Recorder(RefCell<Vec<Routed>>);

impl Recorder {
    fn take(&self) -> Vec<Routed> {
        std::mem::take(&mut self.0.borrow_mut())
    }

    fn push(&self, routed: Routed) {
        self.0.borrow_mut().push(routed);
    }
}

impl ShellRoutes for Recorder {
    fn tab(&self, tab: TabId, event: TabEvent, _: &mut App) {
        self.push(Routed::Tab(tab, event));
    }

    fn popup_opened(&self, opener: TabId, tab: TabId, _: Url, _: OpenDisposition, _: &mut App) {
        self.push(Routed::Popup(opener, tab));
    }

    fn open_url(&self, opener: TabId, url: Url, _: OpenDisposition, _: bool, _: &mut App) {
        self.push(Routed::OpenUrl(opener, url));
    }

    fn shortcut(&self, tab: Option<TabId>, key: RoutedKey, _: &mut App) {
        self.push(Routed::Shortcut(tab, key));
    }

    fn app_relaunch(&self, argv: Vec<OsString>, _: PathBuf, _: &mut App) {
        self.push(Routed::Relaunch(argv));
    }

    fn user_browsers_closed(&self, _: &mut App) {
        self.push(Routed::UserBrowsersClosed);
    }

    fn all_browsers_closed(&self, _: &mut App) {
        self.push(Routed::AllBrowsersClosed);
    }
}

struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// The page's rectangle in its host, and the window's place on the screen.
const PAGE: PhysRect = PhysRect { x: 0, y: 0, width: 800, height: 520 };
const SCREEN: PhysRect = PhysRect { x: 100, y: 100, width: 800, height: 600 };
const ON_SCREEN_PAGE: PhysRect = PhysRect { x: 100, y: 180, width: 800, height: 520 };
const IN_PAGE: PhysPoint = PhysPoint { x: 400, y: 400 };
const IN_TOOLBAR: PhysPoint = PhysPoint { x: 400, y: 120 };
/// An interactive overlay over the page, such as the omnibox's suggestions.
const OVERLAY: PhysRect = PhysRect { x: 150, y: 180, width: 300, height: 100 };
const ON_OVERLAY: PhysPoint = PhysPoint { x: 200, y: 200 };

struct Rig {
    fake: Rc<FakeEngine>,
    desktop: Rc<FakeDesktop>,
    routes: Rc<Recorder>,
    engine: Rc<dyn Engine>,
    pump: Option<PumpHandle>,
}

struct Win {
    id: WindowId,
    surface: SharedSurface,
    toplevel: NativeHandle,
}

impl Win {
    fn fake(&self) -> Ref<'_, FakeSurface> {
        Ref::map(self.surface.borrow(), |s| s.as_any().downcast_ref().expect("a FakeSurface"))
    }
}

fn rig(cx: &mut TestAppContext) -> Rig {
    let (fake, events) = FakeEngine::new();
    let desktop = Rc::new(FakeDesktop::init().unwrap());
    let routes = Rc::new(Recorder::default());
    let driver: DriverSlot = Rc::new(RefCell::new(Some(fake.driver())));
    let started = cx.update(|cx| start(cx, fake.clone(), events, driver, desktop.clone(), routes.clone()));
    cx.run_until_parked();
    Rig { fake, desktop, routes, engine: started.engine, pump: Some(started.pump) }
}

static NEXT_TOPLEVEL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0x7000);

/// A browser window with a fake surface, registered with the focus policy, at `SCREEN`.
fn window(cx: &mut TestAppContext, rig: &Rig) -> Win {
    let handle = cx.add_window(|_, _| Blank);
    let surface = handle.update(cx, |_, window, _| fake::attach(window)).unwrap().unwrap();
    let surface: SharedSurface = Rc::new(RefCell::new(surface));
    let toplevel = NativeHandle::from_raw(NEXT_TOPLEVEL.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    rig.desktop.place_toplevel(toplevel, SCREEN);
    let id = handle.window_id();
    cx.update(|cx| {
        let focus = super::focus(cx);
        focus.add_window(id, surface.clone());
        focus.set_page_area(id, Some(PageArea { toplevel, rect: ON_SCREEN_PAGE, holes: vec![OVERLAY] }));
    });
    Win { id, surface, toplevel }
}

fn spec(tab: TabId, win: &Win) -> TabSpec {
    TabSpec {
        tab,
        kind: TabKind::Page,
        browsing: Browsing::Normal,
        parent: win.surface.borrow().host(),
        bounds: PAGE,
        scale_factor: 1.0,
        visible: true,
        page: InitialPage::Blank,
        opener: None,
    }
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

/// Creates a tab, shows its page in the window's surface and lets `Created` reach the policy.
fn tab_in(cx: &mut TestAppContext, rig: &Rig, win: &Win, select: bool) -> TabId {
    let tab = TabId::next();
    let focus = cx.update(|cx| super::focus(cx));
    if select {
        focus.select_tab(win.id, Some(tab));
    } else {
        focus.join(win.id, tab);
    }
    rig.engine.create_tab(spec(tab, win)).unwrap();
    cx.run_until_parked();
    let native = rig.fake.tab(tab).and_then(|t| t.native).expect("created");
    win.surface.borrow_mut().show_page(native, select);
    tab
}

const ACTIVE_PAGE: PageFocus = PageFocus { window_active: true, tab_selected: true, keyboard: KeyboardOwner::Page };

fn focus_of(cx: &mut TestAppContext) -> Focus {
    cx.update(|cx| super::focus(cx))
}

#[gpui::test]
fn the_new_browser_policy_precedes_every_create_tab(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let focus = focus_of(cx);
    focus.window_activated(win.id, true);
    focus.chrome_wants_keyboard(win.id);
    let tabs = [TabId::next(), TabId::next(), TabId::next()];
    focus.select_tab(win.id, Some(tabs[0]));
    rig.fake.take_commands();
    for tab in tabs {
        rig.engine.create_tab(spec(tab, &win)).unwrap();
    }
    let commands = rig.fake.commands();
    for tab in tabs {
        let at = commands.iter().position(|c| matches!(c, FakeCommand::CreateTab(s) if s.tab == tab)).expect("created");
        assert_eq!(commands[at - 1], FakeCommand::SetPageFocus(tab, PageFocus::NEW_BROWSER), "{commands:?}");
    }
}

#[gpui::test]
fn creating_a_live_tab_again_leaves_its_policy(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let tab = tab_in(cx, &rig, &win, true);
    let focus = focus_of(cx);
    focus.window_activated(win.id, true);
    focus.page_wants_keyboard(tab);
    assert!(rig.engine.create_tab(spec(tab, &win)).is_err());
    assert_eq!(rig.fake.page_focus(tab), Some(ACTIVE_PAGE));
    focus.select_tab(win.id, Some(tab));
    assert_eq!(rig.fake.page_focus(tab), Some(ACTIVE_PAGE), "the tab is still the window's");
}

#[gpui::test]
fn a_policy_precedes_every_navigation_the_shell_starts(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let tab = tab_in(cx, &rig, &win, true);
    let focus = focus_of(cx);
    focus.window_activated(win.id, true);
    rig.fake.take_commands();
    let page = url("https://example.com/");
    rig.engine.load(tab, &page);
    rig.engine.load_html(tab, "<p>hi</p>".into(), None);
    rig.engine.reload(tab, false);
    rig.engine.go_back(tab);
    rig.engine.go_forward(tab);
    let commands = rig.fake.take_commands();
    let navigations = [
        FakeCommand::Load(tab, page),
        FakeCommand::LoadHtml(tab, "<p>hi</p>".into(), None),
        FakeCommand::Reload(tab, false),
        FakeCommand::GoBack(tab),
        FakeCommand::GoForward(tab),
    ];
    let policy = PageFocus { window_active: true, tab_selected: true, keyboard: KeyboardOwner::Chrome };
    let expected: Vec<FakeCommand> = navigations.into_iter().flat_map(|n| [FakeCommand::SetPageFocus(tab, policy), n]).collect();
    assert_eq!(commands, expected);
}

#[gpui::test]
fn the_policy_follows_activation_tab_switches_and_the_keyboard_owner(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let (a, b) = (tab_in(cx, &rig, &win, true), tab_in(cx, &rig, &win, false));
    let focus = focus_of(cx);
    let policy = |active, selected, keyboard| PageFocus { window_active: active, tab_selected: selected, keyboard };
    use KeyboardOwner::{Chrome, Overlay, Page};
    assert_eq!(rig.fake.page_focus(a), Some(policy(false, true, Chrome)));
    assert_eq!(rig.fake.page_focus(b), Some(policy(false, false, Chrome)));
    focus.window_activated(win.id, true);
    assert_eq!(rig.fake.page_focus(a), Some(policy(true, true, Chrome)));
    focus.page_wants_keyboard(a);
    assert_eq!(rig.fake.page_focus(a), Some(ACTIVE_PAGE));
    assert_eq!(rig.fake.page_focus(b), Some(policy(true, false, Page)));
    assert!(!rig.fake.engine_focus_request(b), "a background tab never takes focus");
    focus.select_tab(win.id, Some(b));
    assert_eq!(rig.fake.page_focus(a), Some(policy(true, false, Page)));
    assert_eq!(rig.fake.page_focus(b), Some(ACTIVE_PAGE));
    focus.overlay_wants_keyboard(win.id);
    assert_eq!(rig.fake.page_focus(b), Some(policy(true, true, Overlay)));
    assert!(!rig.fake.engine_focus_request(b), "an overlay keeps the keyboard");
    focus.page_wants_keyboard(b);
    focus.window_activated(win.id, false);
    assert_eq!(rig.fake.page_focus(b), Some(policy(false, true, Page)));
    assert!(!rig.fake.engine_focus_request(b), "an inactive window's page never takes focus");
    focus.window_activated(win.id, true);
    assert!(rig.fake.engine_focus_request(b), "the selected page of the active window may");
    focus.chrome_wants_keyboard(win.id);
    assert_eq!(rig.fake.page_focus(b), Some(policy(true, true, Chrome)));
    assert_eq!(win.fake().focused(), FocusTarget::Chrome);
}

#[gpui::test]
fn a_press_in_the_active_windows_page_hands_the_page_the_keyboard(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let tab = tab_in(cx, &rig, &win, true);
    let native = rig.fake.tab(tab).and_then(|t| t.native).unwrap();
    let focus = focus_of(cx);
    focus.window_activated(win.id, true);
    rig.fake.take_commands();

    rig.desktop.press(IN_TOOLBAR);
    cx.run_until_parked();
    assert_eq!(rig.fake.take_commands(), [], "a press in the chrome leaves the keyboard alone");
    rig.desktop.press(ON_OVERLAY);
    cx.run_until_parked();
    assert_eq!(rig.fake.take_commands(), [], "a press on an overlay over the page is GPUI's");

    rig.desktop.press(IN_PAGE);
    cx.run_until_parked();
    let commands = rig.fake.take_commands();
    assert_eq!(commands[..2], [FakeCommand::SetPageFocus(tab, ACTIVE_PAGE), FakeCommand::FocusPage(tab)], "{commands:?}");
    assert_eq!(win.fake().focused(), FocusTarget::Page(native));
    assert_eq!(rig.fake.focused(), Some(tab));
    assert!(
        rig.routes.take().contains(&Routed::Tab(tab, TabEvent::FocusChanged { focused: true })),
        "the engine's own focus event reaches the tab"
    );
    assert_eq!(win.fake().focused(), FocusTarget::Page(native), "and is not given back");
}

#[gpui::test]
fn the_page_gets_focus_page_only_after_its_window_took_the_keyboard(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let tab = tab_in(cx, &rig, &win, true);
    let focus = focus_of(cx);
    focus.window_activated(win.id, true);
    win.surface.borrow_mut().as_any_mut().downcast_mut::<FakeSurface>().unwrap().set_window_active(false);
    rig.fake.take_commands();
    focus.page_wants_keyboard(tab);
    assert_eq!(rig.fake.take_commands(), [FakeCommand::SetPageFocus(tab, ACTIVE_PAGE)], "the OS refused, so no focus_page");
}

#[gpui::test]
fn a_press_does_not_count_in_an_inactive_window_or_over_another_window(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let tab = tab_in(cx, &rig, &win, true);
    let focus = focus_of(cx);
    rig.desktop.press(IN_PAGE);
    cx.run_until_parked();
    assert_ne!(rig.fake.page_focus(tab), Some(ACTIVE_PAGE), "the window is inactive");
    focus.window_activated(win.id, true);
    rig.desktop.place_toplevel(NativeHandle::from_raw(0x6fff), PhysRect { x: 350, y: 350, width: 100, height: 100 });
    rig.desktop.press(IN_PAGE);
    cx.run_until_parked();
    assert_ne!(rig.fake.page_focus(tab), Some(ACTIVE_PAGE), "another window is on top there");
    assert_eq!(win.fake().focused(), FocusTarget::Chrome);
}

#[gpui::test]
fn focus_the_policy_did_not_allow_goes_back_to_its_owner(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let tab = tab_in(cx, &rig, &win, true);
    let focus = focus_of(cx);
    focus.window_activated(win.id, true);
    focus.chrome_wants_keyboard(win.id);
    rig.fake.take_commands();
    let calls_before = win.fake().journal().borrow().len();
    rig.fake.inject(BrowserEvent::Tab(tab, TabEvent::FocusChanged { focused: true }));
    cx.run_until_parked();
    assert_eq!(rig.fake.take_commands(), [FakeCommand::BlurPage(tab)]);
    let journal = win.fake().journal().borrow()[calls_before..].to_vec();
    assert_eq!(journal, [fake::SurfaceCall::Focus { target: FocusTarget::Chrome, outcome: crate::platform::FocusOutcome::Done }]);
    assert_eq!(rig.fake.page_focus(tab).map(PageFocus::allows), Some(false));
}

#[gpui::test]
fn focus_taken_before_its_press_arrived_is_the_click_hand_off(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let tab = tab_in(cx, &rig, &win, true);
    let focus = focus_of(cx);
    focus.window_activated(win.id, true);
    rig.fake.take_commands();
    cx.update(|cx| {
        rig.desktop.press(IN_PAGE);
        router::route(BrowserEvent::Tab(tab, TabEvent::FocusChanged { focused: true }), cx);
    });
    let commands = rig.fake.take_commands();
    assert_eq!(commands[..2], [FakeCommand::SetPageFocus(tab, ACTIVE_PAGE), FakeCommand::FocusPage(tab)], "{commands:?}");
    assert!(!commands.contains(&FakeCommand::BlurPage(tab)), "{commands:?}");
}

#[gpui::test]
fn a_press_that_activated_the_window_explains_focus_for_500_ms(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let tab = tab_in(cx, &rig, &win, true);
    let focus = focus_of(cx);
    rig.desktop.press(IN_PAGE);
    cx.run_until_parked();
    focus.window_activated(win.id, true);
    cx.executor().advance_clock(Duration::from_millis(300));
    rig.fake.inject(BrowserEvent::Tab(tab, TabEvent::FocusChanged { focused: true }));
    cx.run_until_parked();
    assert_eq!(rig.fake.page_focus(tab), Some(ACTIVE_PAGE), "a click 300 ms ago");

    focus.chrome_wants_keyboard(win.id);
    focus.window_activated(win.id, false);
    rig.desktop.press(IN_PAGE);
    cx.run_until_parked();
    focus.window_activated(win.id, true);
    rig.fake.take_commands();
    cx.executor().advance_clock(Duration::from_millis(600));
    rig.fake.inject(BrowserEvent::Tab(tab, TabEvent::FocusChanged { focused: true }));
    cx.run_until_parked();
    assert_eq!(rig.fake.take_commands(), [FakeCommand::BlurPage(tab)], "a click 600 ms ago explains nothing");
}

#[gpui::test]
fn a_click_does_not_explain_focus_once_the_keyboard_moved_after_it(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let tab = tab_in(cx, &rig, &win, true);
    let focus = focus_of(cx);
    rig.desktop.press(IN_PAGE);
    cx.run_until_parked();
    focus.window_activated(win.id, true);
    focus.chrome_wants_keyboard(win.id);
    rig.fake.take_commands();
    rig.fake.inject(BrowserEvent::Tab(tab, TabEvent::FocusChanged { focused: true }));
    cx.run_until_parked();
    assert_eq!(rig.fake.take_commands(), [FakeCommand::BlurPage(tab)], "Ctrl+L after the click keeps the omnibox's keyboard");
}

#[gpui::test]
fn a_page_focus_the_os_refused_is_retried_at_the_next_layout(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let (a, b) = (tab_in(cx, &rig, &win, true), tab_in(cx, &rig, &win, false));
    let native_b = rig.fake.tab(b).and_then(|t| t.native).unwrap();
    let focus = focus_of(cx);
    focus.window_activated(win.id, true);
    focus.page_wants_keyboard(a);
    rig.fake.take_commands();
    focus.select_tab(win.id, Some(b));
    assert!(!rig.fake.take_commands().contains(&FakeCommand::FocusPage(b)), "b's page is not shown yet");
    win.surface.borrow_mut().show_page(native_b, true);
    focus.set_page_area(win.id, Some(PageArea { toplevel: win.toplevel, rect: ON_SCREEN_PAGE, holes: vec![] }));
    assert_eq!(rig.fake.take_commands(), [FakeCommand::FocusPage(b)]);
    assert_eq!(win.fake().focused(), FocusTarget::Page(native_b));
}

#[gpui::test]
fn every_prompt_reaches_its_owner_and_an_unanswered_one_ends_on_prompt_ended(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let tab = tab_in(cx, &rig, &win, true);
    prompts::tests::take_delivered();
    let params = ContextMenuParams { page_url: Some(url("https://example.com/")), plain_page: true, ..Default::default() };
    let requests = [
        (PromptRequest::ContextMenu { tab, at: PagePoint { x: 1, y: 2 }, params, model: MenuModel { entries: vec![] } }, Owner::ContextMenu),
        (PromptRequest::JsDialog { tab, origin: None, kind: JsDialogKind::Alert, message: "hi".into() }, Owner::PagePrompts),
        (PromptRequest::BeforeUnload { tab, is_reload: false }, Owner::PagePrompts),
        (
            PromptRequest::FileChooser { tab, mode: FileChooserMode::Open, title: None, default_path: None, accept: vec![] },
            Owner::PagePrompts,
        ),
        (
            PromptRequest::Auth {
                tab: Some(tab),
                requester: None,
                challenger: AuthChallenger { host: "example.com".into(), port: 443, is_proxy: false },
                realm: None,
                scheme: None,
            },
            Owner::PagePrompts,
        ),
        (PromptRequest::Permission(PermissionRequest { tab, origin: None, permissions: vec![] }), Owner::Permissions),
    ];
    let mut ids = Vec::new();
    for (request, owner) in requests {
        let id = rig.fake.push_prompt(request).unwrap();
        ids.push((id, owner));
    }
    cx.run_until_parked();
    let shown: Vec<_> = ids.iter().map(|&(id, owner)| (owner, id, None)).collect();
    assert_eq!(prompts::tests::take_delivered(), shown);
    assert_eq!(cx.update(|cx| prompts::open(cx)), ids);

    let (menu, _) = ids[0];
    assert_eq!(cx.update(|cx| prompts::answer(cx, menu, PromptAnswer::Files(None))), Err(PromptError::WrongKind));
    assert_eq!(cx.update(|cx| prompts::answer(cx, menu, PromptAnswer::Menu(None))), Ok(()));
    rig.fake.commit_document(tab);
    cx.run_until_parked();
    let ended: Vec<_> = ids[1..].iter().map(|&(id, owner)| (owner, id, Some(PromptEnd::Navigated))).collect();
    assert_eq!(prompts::tests::take_delivered(), ended, "the answered menu is not ended again");
    assert_eq!(cx.update(|cx| prompts::open(cx)), []);
}

#[gpui::test]
async fn requests_complete_when_the_engine_answers(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let win = window(cx, &rig);
    let tab = tab_in(cx, &rig, &win, true);
    let task = cx.update(|cx| requests::eval(cx, tab, "1 + 1"));
    let id = *rig.fake.pending_requests().last().expect("pending");
    assert!(rig.fake.finish_request(id, RequestResult::Eval(Ok("2".into()))));
    cx.run_until_parked();
    assert_eq!(task.await, Some(RequestResult::Eval(Ok("2".into()))));
}

#[gpui::test]
fn the_router_hands_the_shell_its_events(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let (opener, popup) = (TabId::next(), TabId::next());
    let page = url("https://example.com/");
    let events = [
        BrowserEvent::Tab(opener, TabEvent::TitleChanged("t".into())),
        BrowserEvent::PopupOpened { opener, tab: popup, url: page.clone(), disposition: OpenDisposition::NewForegroundTab },
        BrowserEvent::OpenUrlRequested { opener, url: page.clone(), disposition: OpenDisposition::NewBackgroundTab, user_gesture: true },
        BrowserEvent::Shortcut { tab: Some(opener), key: RoutedKey::NextTab },
        BrowserEvent::AppRelaunch { argv: vec!["vsesvit".into()], cwd: PathBuf::from(".") },
        BrowserEvent::UserBrowsersClosed,
        BrowserEvent::AllBrowsersClosed,
        BrowserEvent::EngineError("logged only".into()),
    ];
    for event in events {
        rig.fake.inject(event);
    }
    cx.run_until_parked();
    assert_eq!(
        rig.routes.take(),
        [
            Routed::Tab(opener, TabEvent::TitleChanged("t".into())),
            Routed::Popup(opener, popup),
            Routed::OpenUrl(opener, page),
            Routed::Shortcut(Some(opener), RoutedKey::NextTab),
            Routed::Relaunch(vec!["vsesvit".into()]),
            Routed::UserBrowsersClosed,
            Routed::AllBrowsersClosed,
        ]
    );
}

#[gpui::test]
#[should_panic(expected = "a shell hook reached gpui::App")]
fn a_hook_that_reaches_the_app_panics_in_debug_builds(cx: &mut TestAppContext) {
    let _rig = rig(cx);
    cx.update(|cx| {
        let _hook = hooks::InHook::enter();
        router::route(BrowserEvent::UserBrowsersClosed, cx);
    });
}

#[gpui::test]
fn the_pump_steps_on_wakes_and_its_handle_returns_the_driver(cx: &mut TestAppContext) {
    let mut rig = rig(cx);
    let first = rig.fake.pump_steps();
    assert!(first >= 1, "the first step runs at once");
    cx.executor().advance_clock(pump::ACTIVE_CAP);
    cx.run_until_parked();
    assert!(rig.fake.pump_steps() > first, "the active cap");
    let before = rig.fake.pump_steps();
    assert!(rig.fake.wake(Duration::ZERO));
    cx.run_until_parked();
    assert_eq!(rig.fake.pump_steps(), before + 1, "a wake steps at once");

    let Ok(driver) = rig.pump.take().unwrap().stop() else { panic!("the driver comes back") };
    let stopped = rig.fake.pump_steps();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert_eq!(rig.fake.pump_steps(), stopped, "no step after stop");
    driver.shutdown();
    assert!(rig.fake.driver_shut_down());
}

#[gpui::test]
fn a_halted_pump_leaves_its_driver_in_the_slot(cx: &mut TestAppContext) {
    let (fake, events) = FakeEngine::new();
    let driver: DriverSlot = Rc::new(RefCell::new(Some(fake.driver())));
    let desktop = Rc::new(FakeDesktop::init().unwrap());
    let started = cx.update(|cx| start(cx, fake.clone(), events, driver.clone(), desktop, Rc::new(Recorder::default())));
    cx.run_until_parked();
    started.pump.halt();
    let halted = fake.pump_steps();
    let _ = fake.wake(Duration::ZERO);
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert_eq!(fake.pump_steps(), halted, "no step after halt");
    assert!(driver.borrow().is_some(), "the driver stays for shutdown");
}
