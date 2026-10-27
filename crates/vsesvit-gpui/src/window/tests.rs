//! Owned by W1.7.
//! Browser windows over `FakeEngine`, the fake platform and the real bridge and state (UA U08's
//! TestAppContext acceptance, design section 8.3 row W1.7): the layouts and their probe, closing
//! windows and tabs, the title, focus on a tab switch, the sidebar's key, the download keep-alive on a
//! window's close, and the transparent content rect.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{AnyWindowHandle, Bounds, Entity, TestAppContext, VisualTestContext, WindowBackgroundAppearance, px};
use vsesvit_core::prefs::{TabsPosition, keys};
use vsesvit_core::private::Browsing;
use vsesvit_core::profiles::{Home, ProfileColor, ProfilesDir};
use vsesvit_core::{OpenOptions, Profile, Url};
use vsesvit_engine::{BrowserEvent, FakeCommand, FakeEngine, KeyboardOwner, NativeHandle, TabEvent, TabId};

use super::layout::classify;
use super::{BrowserWindow, WindowHost, frame};
use crate::app::quit::{self, EndedSlot, QuitParts};
use crate::app::shell::{Core, ProfileLocation, Routes, Shutdown};
use crate::app::{self, StartArgs};
use crate::downloads::keep_alive::ParkDecision;
use crate::engine_bridge::{self, DriverSlot};
use crate::platform::fake::{self, FakeDesktop, FakeSurface, SurfaceCall};
use crate::platform::{Desktop as _, FocusOutcome, FocusTarget};
use crate::state::testing::scratch_dir;
use crate::state::{BrowserState, Invocation, KeepAlive, WindowId};

const TOPLEVEL: NativeHandle = NativeHandle::from_raw(0x7070);

struct Rig {
    state: Entity<BrowserState>,
    fake: Rc<FakeEngine>,
    _root: PathBuf,
}

/// What a test starts the app with.
struct Setup {
    targets: &'static [&'static str],
    position: TabsPosition,
    transparent: bool,
    home: Option<Home>,
}

impl Default for Setup {
    fn default() -> Setup {
        Setup { targets: &["https://a.test/"], position: TabsPosition::Left, transparent: false, home: None }
    }
}

/// The app as boot starts it, over a scratch profile: the bridge, the shutdown sequence, the state and
/// its first window from `targets`.
fn rig(cx: &mut TestAppContext, name: &str, setup: Setup) -> Rig {
    let root = match &setup.home {
        Some(home) => home.root(),
        None => scratch_dir(&format!("window-{name}")),
    };
    let mut profile = Profile::open(&root, OpenOptions::default()).expect("a scratch profile");
    profile.prefs().set(&keys::TABS_POSITION, &setup.position).unwrap();
    let core: Core = Rc::new(RefCell::new(profile));
    let (fake, events) = FakeEngine::new();
    let driver: DriverSlot = Rc::new(RefCell::new(Some(fake.driver())));
    let desktop = Rc::new(FakeDesktop::init().unwrap());
    let host = WindowHost {
        attach: fake::attach,
        toplevel: |_| Some(TOPLEVEL),
        raise: Rc::new(|_, _| {}),
        transparent: setup.transparent,
    };
    let targets = setup.targets.iter().map(|t| t.to_string()).collect();
    let location = ProfileLocation { root: root.clone(), home: setup.home };
    let state = cx.update(|cx| {
        crate::theme::init(cx);
        let presses = desktop.clone();
        crate::overlay::install(move || presses.pointer_presses(), cx);
        let started = engine_bridge::start(cx, fake.clone(), events, driver.clone(), desktop.clone(), Rc::new(Routes::default()));
        let pump = started.pump;
        let parts = QuitParts {
            engine: started.engine.clone(),
            shell: Rc::new(Shutdown),
            stop_pump: Box::new(move || pump.halt()),
            driver,
            ended: EndedSlot::default(),
        };
        quit::install(parts, cx);
        let engine = started.engine.clone();
        let args = StartArgs {
            core,
            engine: started.engine,
            location,
            scripted: true,
            hidden_host: desktop.hidden_host(),
            first: Invocation { targets, cwd: root.clone(), startup_id: None },
        };
        let state = app::start(cx, args, host);
        engine.set_hidden_host(desktop.hidden_host());
        state
    });
    cx.run_until_parked();
    Rig { state, fake, _root: root }
}

impl Rig {
    fn windows(&self, cx: &mut TestAppContext) -> Vec<WindowId> {
        self.state.read_with(cx, |s, _| s.windows().iter().map(|w| w.id).collect())
    }

    fn handle(&self, cx: &mut TestAppContext, window: WindowId) -> AnyWindowHandle {
        self.state.read_with(cx, |s, _| s.window(window).and_then(|w| w.handle)).expect("the window opened")
    }

    fn view(&self, cx: &mut TestAppContext, window: WindowId) -> Entity<BrowserWindow> {
        let handle = self.handle(cx, window).downcast::<BrowserWindow>().expect("a browser window");
        cx.update(|cx| handle.entity(cx)).unwrap()
    }

    fn visual<'a>(&self, cx: &'a mut TestAppContext, window: WindowId) -> &'a mut VisualTestContext {
        let handle = self.handle(cx, window);
        VisualTestContext::from_window(handle, cx).into_mut()
    }

    /// The window as the active one, for the focus policy: the test platform's own activation goes
    /// through `Window::activate_window`, which the shell never calls.
    fn activate(&self, cx: &mut TestAppContext, window: WindowId) {
        let id = self.handle(cx, window).window_id();
        cx.update(|cx| engine_bridge::focus(cx).window_activated(id, true));
        cx.run_until_parked();
    }

    fn tabs(&self, cx: &mut TestAppContext, window: WindowId) -> Vec<TabId> {
        let view = self.view(cx, window);
        view.read_with(cx, |w, cx| w.tabs.read(cx).tabs().to_vec())
    }

    fn select(&self, cx: &mut TestAppContext, window: WindowId, tab: TabId) {
        let view = self.view(cx, window);
        view.update(cx, |w, cx| w.tabs.update(cx, |m, cx| m.select(tab, cx)));
        cx.run_until_parked();
    }

    fn native(&self, cx: &mut TestAppContext, tab: TabId) -> NativeHandle {
        self.state.read_with(cx, |s, cx| s.tab(tab).and_then(|t| t.read(cx).native())).expect("a live page")
    }

    fn journal(&self, cx: &mut TestAppContext, window: WindowId) -> fake::Journal {
        let view = self.view(cx, window);
        let surface = view.read_with(cx, |w, _| w.surface()).expect("a surface");
        let surface = surface.borrow();
        surface.as_any().downcast_ref::<FakeSurface>().expect("a fake surface").journal()
    }

    fn open_window(&self, cx: &mut TestAppContext, browsing: Browsing, urls: &[&str]) -> WindowId {
        let urls = urls.iter().map(|u| Url::parse(u).unwrap()).collect();
        let window = self.state.update(cx, |s, cx| s.open_window(browsing, urls, cx));
        cx.run_until_parked();
        window
    }
}

#[gpui::test]
fn each_layout_classifies_as_its_pref(cx: &mut TestAppContext) {
    for position in [TabsPosition::Left, TabsPosition::Right, TabsPosition::Top] {
        let mut cx = cx.new_app();
        let rig = rig(&mut cx, &format!("layout-{position:?}"), Setup { position, ..Setup::default() });
        let window = rig.windows(&mut cx)[0];
        let probe = rig.view(&mut cx, window).read_with(&cx, |w, _| w.layout_probe());
        assert_eq!(classify(&probe), Some(position), "{probe:?}");
    }
}

#[gpui::test]
fn ctrl_s_toggles_the_sidebar(cx: &mut TestAppContext) {
    let rig = rig(cx, "ctrl-s", Setup::default());
    let window = rig.windows(cx)[0];
    let view = rig.view(cx, window);
    assert!(view.read_with(cx, |w, _| w.layout_probe().sidebar.is_some()));
    rig.visual(cx, window).simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |w, _| w.layout_probe().sidebar), None);
    rig.visual(cx, window).simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |w, _| classify(&w.layout_probe())), Some(TabsPosition::Left));
}

#[gpui::test]
fn the_page_shows_in_the_window_and_the_others_hide(cx: &mut TestAppContext) {
    let rig = rig(cx, "pages", Setup { targets: &["https://a.test/", "https://b.test/", "https://c.test/"], ..Setup::default() });
    let window = rig.windows(cx)[0];
    let tabs = rig.tabs(cx, window);
    assert_eq!(tabs.len(), 3);
    let natives: Vec<NativeHandle> = tabs.iter().map(|&t| rig.native(cx, t)).collect();
    let fake_tabs: Vec<_> = tabs.iter().map(|&t| rig.fake.tab(t).expect("a browser")).collect();
    assert!(fake_tabs.iter().all(|t| t.parent == fake_tabs[0].parent && t.bounds.width > 0), "every page in the window's host");
    assert_eq!(fake_tabs.iter().map(|t| t.visible).collect::<Vec<_>>(), [true, false, false]);
    let created: Vec<bool> =
        rig.fake.commands().iter().filter_map(|c| if let FakeCommand::CreateTab(spec) = c { Some(spec.visible) } else { None }).collect();
    assert_eq!(created, [true, false, false], "only the selected page is created shown");
    rig.select(cx, window, tabs[2]);
    let view = rig.view(cx, window);
    assert_eq!(view.read_with(cx, |w, _| w.page_slot().shown()), Some((tabs[2], natives[2])));
    assert!(!rig.fake.tab(tabs[0]).unwrap().visible && rig.fake.tab(tabs[2]).unwrap().visible);
    let journal = rig.journal(cx, window);
    assert_eq!(
        journal.borrow().iter().rev().find(|c| matches!(c, SurfaceCall::ShowPage { visible: true, .. })),
        Some(&SurfaceCall::ShowPage { page: natives[2], visible: true })
    );
    let hook = cx.update(|cx| engine_bridge::hooks::snapshot(cx).borrow().tabs.get(&tabs[1]).copied());
    assert_eq!(hook.map(|h| h.parent), Some(fake_tabs[0].parent), "popups open where their opener is");
}

#[gpui::test]
fn an_address_typed_while_the_browser_is_created_loads_once_it_exists(cx: &mut TestAppContext) {
    let rig = rig(cx, "load-while-creating", Setup::default());
    let window = rig.windows(cx)[0];
    let tab = rig.state.update(cx, |s, cx| s.new_tab_entity(Browsing::Normal, vsesvit_engine::InitialPage::Blank, None, cx));
    let id = tab.read_with(cx, |t, _| t.id());
    rig.fake.hold_creation(id);
    rig.state.update(cx, |s, cx| s.attach_tab(window, &tab, crate::tab::model::Insert::End, true, cx));
    cx.run_until_parked();
    assert!(rig.fake.commands().iter().any(|c| matches!(c, FakeCommand::CreateTab(spec) if spec.tab == id)), "the browser is being created");
    let url = Url::parse("https://typed.test/").unwrap();
    tab.update(cx, |t, cx| t.load(url.clone(), None, cx));
    cx.run_until_parked();
    let loads = |rig: &Rig| rig.fake.commands().iter().filter(|c| matches!(c, FakeCommand::Load(t, u) if *t == id && *u == url)).count();
    assert_eq!(loads(&rig), 0, "an engine that has no browser yet would drop it");
    assert!(rig.fake.complete_creation(id));
    cx.run_until_parked();
    assert_eq!(loads(&rig), 1);
    assert_eq!(rig.fake.tab(id).and_then(|t| t.url), Some(url));
}

#[gpui::test]
fn a_popup_and_a_middle_click_open_tabs_next_to_their_opener(cx: &mut TestAppContext) {
    let rig = rig(cx, "popups", Setup { targets: &["https://a.test/", "https://z.test/"], ..Setup::default() });
    let window = rig.windows(cx)[0];
    let [opener, last] = rig.tabs(cx, window)[..] else { panic!("two tabs") };
    let popup_url = Url::parse("https://popup.test/").unwrap();
    let request = vsesvit_engine::PopupRequest {
        opener,
        url: popup_url.clone(),
        disposition: vsesvit_engine::OpenDisposition::NewForegroundTab,
        user_gesture: true,
    };
    let vsesvit_engine::PopupDecision::AllowAsTab { tab: popup, .. } = rig.fake.open_popup(request) else { panic!("allowed") };
    cx.run_until_parked();
    assert_eq!(rig.tabs(cx, window), [opener, popup, last], "after its opener");
    let view = rig.view(cx, window);
    assert_eq!(view.read_with(cx, |w, cx| w.tabs.read(cx).selected()), Some(popup));
    assert_eq!(view.read_with(cx, |w, _| w.page_slot().shown().map(|(t, _)| t)), Some(popup), "the popup's page shows");

    let link = Url::parse("https://link.test/").unwrap();
    let disposition = vsesvit_engine::OpenDisposition::NewBackgroundTab;
    rig.fake.inject(BrowserEvent::OpenUrlRequested { opener, url: link.clone(), disposition, user_gesture: true });
    cx.run_until_parked();
    let tabs = rig.tabs(cx, window);
    assert_eq!(tabs.len(), 4);
    let opened = tabs[2];
    assert_eq!(tabs, [opener, popup, opened, last], "after the opener's run");
    assert_eq!(view.read_with(cx, |w, cx| w.tabs.read(cx).selected()), Some(popup), "a background tab");
    assert_eq!(rig.fake.tab(opened).and_then(|t| t.url), Some(link));
}

#[gpui::test]
fn the_title_names_the_page_the_profile_and_the_kind(cx: &mut TestAppContext) {
    let first = rig(cx, "title", Setup { targets: &[], ..Setup::default() });
    let window = first.windows(cx)[0];
    assert_eq!(first.visual(cx, window).window_title().as_deref(), Some("New Tab - Vsesvit"), "the new tab page");
    let tab = first.tabs(cx, window)[0];
    first.fake.inject(BrowserEvent::Tab(tab, TabEvent::TitleChanged("A page".into())));
    cx.run_until_parked();
    assert_eq!(first.visual(cx, window).window_title().as_deref(), Some("A page - Vsesvit"), "a page's title");
    let private = first.open_window(cx, Browsing::Private, &["https://p.test/"]);
    let tab = first.tabs(cx, private)[0];
    first.fake.inject(BrowserEvent::Tab(tab, TabEvent::TitleChanged("Hidden".into())));
    cx.run_until_parked();
    assert_eq!(first.visual(cx, private).window_title().as_deref(), Some("Hidden - Vsesvit (Private)"), "a private window");

    let mut cx = cx.new_app();
    let dir = ProfilesDir::at(scratch_dir("window-title-profiles"));
    dir.add("Home", ProfileColor::Blue).unwrap();
    let (work, _) = dir.add("Work", ProfileColor::Green).unwrap();
    let second = rig(&mut cx, "title-profiles", Setup { targets: &[], home: Some(Home { dir, id: work }), ..Setup::default() });
    let window = second.windows(&mut cx)[0];
    assert_eq!(second.visual(&mut cx, window).window_title().as_deref(), Some("New Tab - Work - Vsesvit"), "with two profiles");
}

#[gpui::test]
fn a_switch_gives_the_keyboard_to_the_page_or_to_the_address_field_of_a_blank_tab(cx: &mut TestAppContext) {
    let rig = rig(cx, "focus-switch", Setup::default());
    let window = rig.windows(cx)[0];
    rig.activate(cx, window);
    let page = rig.tabs(cx, window)[0];
    let focus = cx.update(|cx| engine_bridge::focus(cx));
    assert_eq!(focus.published(page).map(|p| p.keyboard), Some(KeyboardOwner::Page));
    assert_eq!(rig.fake.focused(), Some(page), "the page has the keyboard");
    rig.visual(cx, window).dispatch_action(crate::app::actions::NewTab);
    cx.run_until_parked();
    let blank = *rig.tabs(cx, window).last().unwrap();
    assert_eq!(focus.published(blank).map(|p| (p.tab_selected, p.keyboard)), Some((true, KeyboardOwner::Chrome)));
    let journal = rig.journal(cx, window);
    let last_focus = journal.borrow().iter().rev().find(|c| matches!(c, SurfaceCall::Focus { .. })).cloned();
    assert_eq!(last_focus, Some(SurfaceCall::Focus { target: FocusTarget::Chrome, outcome: FocusOutcome::Done }), "GPUI has the keyboard");
    rig.select(cx, window, page);
    assert_eq!(focus.published(page).map(|p| p.keyboard), Some(KeyboardOwner::Page), "back to the page");
    assert_eq!(rig.fake.focused(), Some(page));
    let refused = journal.borrow().iter().filter(|c| matches!(c, SurfaceCall::Focus { outcome: FocusOutcome::Refused, .. })).count();
    assert_eq!(refused, 0, "the keyboard waits until the page shows: {:?}", journal.borrow());
}

#[gpui::test]
fn a_switch_between_pages_shows_the_new_page_before_it_gets_the_keyboard(cx: &mut TestAppContext) {
    let rig = rig(cx, "focus-page-to-page", Setup { targets: &["https://a.test/", "https://b.test/"], ..Setup::default() });
    let window = rig.windows(cx)[0];
    rig.activate(cx, window);
    let [a, b] = rig.tabs(cx, window)[..] else { panic!("two tabs") };
    assert_eq!(rig.fake.focused(), Some(a));
    rig.select(cx, window, b);
    assert_eq!(rig.fake.focused(), Some(b));
    let journal = rig.journal(cx, window);
    let refused: Vec<SurfaceCall> =
        journal.borrow().iter().filter(|c| matches!(c, SurfaceCall::Focus { outcome: FocusOutcome::Refused, .. })).cloned().collect();
    assert!(refused.is_empty(), "a hidden page was asked to take the keyboard: {refused:?}");
}

#[gpui::test]
fn a_window_goes_when_its_last_tab_does_and_the_only_window_quits(cx: &mut TestAppContext) {
    let rig = rig(cx, "close-if-empty", Setup::default());
    let first = rig.windows(cx)[0];
    let second = rig.open_window(cx, Browsing::Normal, &["https://b.test/"]);
    let handle = rig.handle(cx, second);
    let journal = rig.journal(cx, second);
    let tab = rig.tabs(cx, second)[0];
    rig.visual(cx, second).dispatch_action(crate::app::actions::CloseTab);
    cx.run_until_parked();
    assert!(rig.state.read_with(cx, |s, _| s.window(second).is_none()), "the state forgot the window");
    assert!(!cx.windows().contains(&handle), "GPUI's window went");
    assert_eq!(journal.borrow().last(), Some(&SurfaceCall::Detach));
    assert!(rig.fake.tab(tab).is_none(), "its page closed first");
    assert!(!cx.update(|cx| quit::is_quitting(cx)));

    rig.visual(cx, first).dispatch_action(crate::app::actions::CloseTab);
    cx.run_until_parked();
    assert!(cx.update(|cx| quit::is_quitting(cx)), "closing the only window's last tab quits");
}

#[gpui::test]
fn closing_a_window_parks_its_downloading_tabs_before_the_surface_goes(cx: &mut TestAppContext) {
    let rig = rig(cx, "close-parks", Setup::default());
    let second = rig.open_window(cx, Browsing::Normal, &["https://a.test/", "https://b.test/", "https://c.test/"]);
    let tabs = rig.tabs(cx, second);
    let natives: Vec<NativeHandle> = tabs.iter().map(|&t| rig.native(cx, t)).collect();
    let downloading = [tabs[0], tabs[2]];
    for tab in downloading {
        rig.fake.set_downloading(tab, true);
    }
    let decide: crate::state::tabs::ParkDecider =
        Rc::new(move |tab, _| if downloading.contains(&tab) { ParkDecision::Park } else { ParkDecision::Close });
    rig.state.update(cx, |s, _| s.set_keep_alive(KeepAlive { park_decision: decide, ..KeepAlive::default() }));
    let journal = rig.journal(cx, second);
    rig.fake.hold_close(tabs[1]);
    assert!(!rig.visual(cx, second).simulate_close(), "the window waits for its tabs");
    cx.run_until_parked();
    assert!(!journal.borrow().contains(&SurfaceCall::Detach), "a page still closing keeps its host");
    assert!(rig.fake.complete_close(tabs[1]));
    cx.run_until_parked();

    let calls = journal.borrow().clone();
    let detach = calls.iter().position(|c| *c == SurfaceCall::Detach).expect("the surface detached");
    let hidden = cx.update(|cx| crate::app::shell::Shell::get(cx).hidden_host);
    for (i, &tab) in tabs.iter().enumerate() {
        let parked = calls.iter().position(|c| *c == SurfaceCall::ParkPage { page: natives[i], hidden_host: hidden });
        if downloading.contains(&tab) {
            assert!(parked.is_some_and(|p| p < detach), "tab {i} parked before the detach: {calls:?}");
        } else {
            assert_eq!(parked, None, "tab {i} closed");
        }
    }
    assert_eq!(rig.fake.parked().len(), 2, "the downloads' browsers live on");
    assert!(rig.state.read_with(cx, |s, _| s.window(second).is_none()));
}

#[gpui::test]
fn a_transparent_window_paints_nothing_over_the_page(cx: &mut TestAppContext) {
    #[cfg(windows)]
    const { assert!(<crate::platform::Surface as crate::platform::NativeSurface>::TRANSPARENT_CONTENT, "Windows needs the content rect clear") };
    assert_eq!(frame::background(true), WindowBackgroundAppearance::Transparent);
    let geometry = crate::state::WindowGeometry::default();
    let options = cx.update(|cx| frame::options(geometry, true, cx));
    assert_eq!(options.window_background, WindowBackgroundAppearance::Transparent);

    for transparent in [true, false] {
        let mut cx = cx.new_app();
        let rig = rig(&mut cx, &format!("transparent-{transparent}"), Setup { transparent, ..Setup::default() });
        let window = rig.windows(&mut cx)[0];
        let view = rig.view(&mut cx, window);
        let page = view.read_with(&cx, |w, _| w.layout_probe().page).expect("the page was laid out");
        let painted = rig.visual(&mut cx, window).update(|window, _| {
            let inner = inset(page, 1.).scale(window.scale_factor());
            window.painted_quads().iter().filter(|q| !q.background.is_transparent() && q.bounds.intersects(&inner)).count()
        });
        if transparent {
            assert_eq!(painted, 0, "nothing paints the page's rectangle");
        } else {
            assert!(painted > 0, "an opaque window fills it, which shows the probe sees paint");
        }
    }
}

#[gpui::test]
fn the_address_field_keeps_the_keyboard_it_took_right_after_a_typed_navigation(cx: &mut TestAppContext) {
    let rig = rig(cx, "focus-typed-then-address", Setup::default());
    let window = rig.windows(cx)[0];
    rig.activate(cx, window);
    let page = rig.tabs(cx, window)[0];
    let handle = rig.handle(cx, window).downcast::<BrowserWindow>().expect("a browser window");
    handle
        .update(cx, |view, window, cx| {
            view.submit_address("https://typed.test/", window, cx);
            view.address_takes_keyboard(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let focus = cx.update(|cx| engine_bridge::focus(cx));
    assert_eq!(focus.published(page).map(|p| p.keyboard), Some(KeyboardOwner::Chrome), "the page's next layout took it back");
}

fn inset(b: Bounds<gpui::Pixels>, by: f32) -> Bounds<gpui::Pixels> {
    Bounds { origin: gpui::point(b.origin.x + px(by), b.origin.y + px(by)), size: gpui::size(b.size.width - px(2. * by), b.size.height - px(2. * by)) }
}

#[gpui::test]
fn an_overlay_over_the_page_cuts_a_hole_and_borrows_the_keyboard_from_it(cx: &mut TestAppContext) {
    let rig = rig(cx, "overlay", Setup::default());
    let window = rig.windows(cx)[0];
    rig.activate(cx, window);
    let page = rig.tabs(cx, window)[0];
    let focus = cx.update(|cx| engine_bridge::focus(cx));
    assert_eq!(rig.fake.focused(), Some(page));
    let view = rig.view(cx, window);
    let host = view.read_with(cx, |w, _| w.overlays().clone());
    let content = view.read_with(cx, |w, _| w.page_slot().content()).expect("laid out");
    let at = gpui::point(px(content.x as f32 + 100.), px(content.y as f32 + 100.));
    let menu = crate::overlay::menu::Menu::new().item(crate::overlay::menu::Command::run("One", |_, _| {}));
    let vcx = rig.visual(cx, window);
    let id = vcx.update(|window, cx| {
        host.update(cx, |h, cx| h.open_menu(menu, crate::overlay::Anchor::Point(at), crate::overlay::Placement::AtPoint, None, window, cx))
    });
    cx.run_until_parked();

    assert_eq!(focus.published(page).map(|p| p.keyboard), Some(KeyboardOwner::Overlay), "the menu has the keyboard");
    let holes = host.read_with(cx, |h, _| h.holes().to_vec());
    assert_eq!(holes.len(), 1);
    let surface = view.read_with(cx, |w, _| w.surface()).unwrap();
    assert_eq!(surface.borrow().as_any().downcast_ref::<FakeSurface>().unwrap().holes(), holes.as_slice(), "cut out of the content host");
    let area = view.read_with(cx, |w, _| w.page_slot().area().cloned()).expect("an area");
    assert_eq!(area.holes, host.read_with(cx, |h, _| h.screen_holes().to_vec()), "presses on the menu are not the page's");

    rig.visual(cx, window).simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!host.read_with(cx, |h, _| h.is_open(id)));
    assert_eq!(focus.published(page).map(|p| p.keyboard), Some(KeyboardOwner::Page), "the keyboard went back to the page");
    assert_eq!(rig.fake.focused(), Some(page));
    assert!(view.read_with(cx, |w, _| w.page_slot().area().unwrap().holes.is_empty()));
}
