//! Owned by W1.5.
//! `BrowserState` with `FakeEngine` (UA U05 acceptance, design section 8.3 row W1.5).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{Context, Entity, IntoElement, Render, TestAppContext, Window};
use vsesvit_core::Url;
use vsesvit_core::private::Browsing;
use vsesvit_core::session::{SessionSnapshot, TabSnapshot, WindowSnapshot};
use vsesvit_core::tab_groups::{GroupColor, GroupId, Row, TabGroup};
use vsesvit_core::{OpenOptions, Profile};
use vsesvit_engine::{BrowserEvent, FakeCommand, InitialPage, TabEvent, TabId};

use super::testing::{HIDDEN_HOST, Harness};
use super::*;
use crate::downloads::keep_alive::ParkDecision;
use crate::platform::fake::{self, FakeSurface, SurfaceCall};
use crate::platform::SurfaceOps;
use crate::tab::model::Insert;

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn events(cx: &mut TestAppContext, h: &Harness) -> Rc<RefCell<Vec<BrowserStateEvent>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|cx| cx.subscribe(&h.state, move |_, e: &BrowserStateEvent, _| sink.borrow_mut().push(e.clone())).detach());
    seen
}

fn open(cx: &mut TestAppContext, h: &Harness, browsing: Browsing, urls: &[&str]) -> WindowId {
    let targets = urls.iter().map(|u| url(u)).collect();
    let window = h.state.update(cx, |state, cx| state.open_window(browsing, targets, cx));
    h.create_browsers(cx, window);
    window
}

fn close(cx: &mut TestAppContext, h: &Harness, tab: TabId, reason: CloseReason) {
    h.state.update(cx, |state, cx| state.close_tab(tab, reason, None, cx));
    h.route(cx);
}

fn saved_tab(path: &str, pinned: bool, group: Option<&TabGroup>) -> TabSnapshot {
    TabSnapshot {
        id: vsesvit_core::session::TabId::new(),
        url: url(&format!("https://restore.test{path}")),
        title: format!("Saved {path}"),
        pinned,
        last_active_ms: 0,
        group: group.cloned(),
        restore_state: None,
    }
}

fn saved_window(tabs: Vec<TabSnapshot>, active_tab: usize) -> WindowSnapshot {
    WindowSnapshot { tabs, active_tab, bounds: None, maximized: false }
}

fn restore(cx: &mut TestAppContext, h: &Harness, windows: Vec<WindowSnapshot>, active_window: usize) {
    let snapshot = SessionSnapshot { device_name: "pc".into(), windows, active_window };
    h.core.borrow_mut().session().save(&snapshot).unwrap();
    h.state.update(cx, |state, cx| state.open_startup_windows(Vec::new(), cx));
    h.route(cx);
}

fn paths(cx: &mut TestAppContext, h: &Harness, window: WindowId) -> Vec<String> {
    h.tabs_of(cx, window)
        .into_iter()
        .map(|id| h.tab(cx, id).unwrap().read_with(cx, |t, _| t.session_url().map(|u| u.path().to_owned()).unwrap_or_default()))
        .collect()
}

fn window_ids(cx: &mut TestAppContext, h: &Harness) -> Vec<WindowId> {
    h.state.read_with(cx, |s, _| s.windows().iter().map(|w| w.id).collect())
}

#[gpui::test]
fn the_session_is_saved_two_seconds_after_the_last_change(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "throttle");
    open(cx, &h, Browsing::Normal, &["https://a.test/"]);
    cx.executor().advance_clock(Duration::from_millis(1900));
    assert!(h.saved_urls().is_empty(), "not before two seconds");
    cx.executor().advance_clock(Duration::from_millis(200));
    assert_eq!(h.saved_urls(), [["https://a.test/"]]);

    let window = window_ids(cx, &h)[0];
    let tab = h.state.update(cx, |state, cx| {
        let tab = state.new_tab_entity(Browsing::Normal, InitialPage::Url(url("https://b.test/")), None, cx);
        state.attach_tab(window, &tab, Insert::End, false, cx);
        tab
    });
    cx.executor().advance_clock(Duration::from_millis(1000));
    tab.update(cx, |tab, cx| tab.load(url("https://c.test/"), None, cx));
    let moved = WindowGeometry { bounds: Some((10, 10, 800, 600)), maximized: false };
    h.state.update(cx, |state, cx| state.set_window_geometry(window, moved, cx));
    cx.executor().advance_clock(Duration::from_millis(1100));
    assert_eq!(h.saved_urls(), [["https://a.test/", "https://c.test/"]], "the first change's timer carries the later one");
}

#[gpui::test]
fn nothing_is_written_while_no_normal_window_is_left(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "no-normal");
    open(cx, &h, Browsing::Private, &["https://private.test/"]);
    cx.executor().advance_clock(Duration::from_secs(5));
    h.state.update(cx, |state, cx| state.save_session_now(cx));
    assert!(h.saved_urls().is_empty(), "a private window is never saved");

    let normal = open(cx, &h, Browsing::Normal, &["https://a.test/"]);
    assert!(h.saved_urls().is_empty());
    let tab = h.tabs_of(cx, normal)[0];
    close(cx, &h, tab, CloseReason::Window);
    h.state.update(cx, |state, cx| state.window_closed(normal, cx));
    cx.executor().advance_clock(Duration::from_secs(5));
    h.state.update(cx, |state, cx| state.save_session_now(cx));
    assert_eq!(h.saved_urls(), [["https://a.test/"]], "closing the last normal window keeps its session");
}

#[gpui::test]
fn a_window_whose_tabs_are_closing_with_it_leaves_the_session(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "closing-window");
    open(cx, &h, Browsing::Normal, &["https://stays.test/"]);
    let closing = open(cx, &h, Browsing::Normal, &["https://goes.test/"]);
    let tab = h.tabs_of(cx, closing)[0];
    h.engine.hold_close(tab);
    close(cx, &h, tab, CloseReason::Window);
    assert_eq!(h.tabs_of(cx, closing), [tab], "its browser has not closed yet");
    h.state.update(cx, |state, cx| state.save_session_now(cx));
    assert_eq!(h.saved_urls(), [["https://stays.test/"]]);
}

#[gpui::test]
fn a_page_that_keeps_itself_open_leaves_its_tab_and_window_as_they_were(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "beforeunload");
    let window = open(cx, &h, Browsing::Normal, &["https://form.test/"]);
    let tab = h.tabs_of(cx, window)[0];
    h.engine.set_beforeunload(tab, true);
    close(cx, &h, tab, CloseReason::Window);
    let [prompt] = h.engine.open_prompts()[..] else { panic!("the page asks") };
    assert!(h.tab(cx, tab).unwrap().read_with(cx, |t, _| t.is_ending()));
    vsesvit_engine::Engine::answer_prompt(&*h.engine, prompt, vsesvit_engine::PromptAnswer::BeforeUnload { leave: false }).unwrap();
    h.state.update(cx, |s, cx| s.close_refused(tab, cx));
    h.route(cx);
    assert_eq!(h.tabs_of(cx, window), [tab], "the tab stays");
    let entry = h.tab(cx, tab).unwrap();
    assert!(entry.read_with(cx, |t, _| !t.is_ending() && t.native().is_some()), "with its browser");
    assert_eq!(h.state.read_with(cx, |s, _| s.windows_of(Browsing::Normal)), [window], "its window is no longer closing");
    entry.update(cx, |t, cx| t.load(url("https://after.test/"), None, cx));
    h.route(cx);
    h.state.update(cx, |s, cx| s.save_session_now(cx));
    assert_eq!(h.saved_urls(), [["https://after.test/"]], "the tab navigates and the session saves it");
    h.engine.set_beforeunload(tab, false);
    close(cx, &h, tab, CloseReason::Tab);
    assert!(h.tabs_of(cx, window).is_empty(), "a later close closes it");
}

#[gpui::test]
fn a_window_closed_with_tabs_still_open_closes_them_before_the_private_context_ends(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "window-leftovers");
    let window = open(cx, &h, Browsing::Private, &["https://a.test/"]);
    let tab = h.tabs_of(cx, window)[0];
    h.state.update(cx, |s, cx| s.window_closed(window, cx));
    h.route(cx);
    let commands = h.engine.commands();
    let closed = commands.iter().position(|c| matches!(c, FakeCommand::CloseTab(t, _) if *t == tab)).expect("its browser closes");
    let ended = commands.iter().position(|c| c == &FakeCommand::EndPrivateContext).expect("the context ends");
    assert!(closed < ended);
    assert!(h.tab(cx, tab).is_none());
    assert!(h.state.read_with(cx, |s, _| s.window(window).is_none()));
}

#[gpui::test]
fn a_tab_that_closes_itself_goes_on_the_closed_tab_stack(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "page-close");
    let window = open(cx, &h, Browsing::Normal, &["https://a.test/", "https://b.test/"]);
    let tab = h.tabs_of(cx, window)[1];
    h.engine.close_from_page(tab);
    h.route(cx);
    assert!(!h.tabs_of(cx, window).contains(&tab));
    let closed: Vec<String> = h.state.read_with(cx, |s, _| s.closed_tabs(Browsing::Normal).iter().map(|t| t.url.clone()).collect());
    assert_eq!(closed, ["https://b.test/"]);
}

#[gpui::test]
fn closing_the_last_tab_of_the_last_window_keeps_it_in_the_session(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "last-tab");
    let window = open(cx, &h, Browsing::Normal, &["https://kept.test/"]);
    let tab = h.tabs_of(cx, window)[0];
    close(cx, &h, tab, CloseReason::Tab);
    assert!(h.tabs_of(cx, window).is_empty());
    h.state.update(cx, |state, cx| state.window_closed(window, cx));
    cx.executor().advance_clock(Duration::from_secs(5));
    assert_eq!(h.saved_urls(), [["https://kept.test/"]]);
}

#[gpui::test]
fn a_restored_session_keeps_its_active_window_first(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "active-first");
    let a = saved_window(vec![saved_tab("/a", false, None)], 0);
    let b = saved_window(vec![saved_tab("/b", false, None)], 0);
    let opened = events(cx, &h);
    restore(cx, &h, vec![a, b], 0);
    let windows = window_ids(cx, &h);
    assert_eq!(windows.len(), 2);
    assert_eq!(paths(cx, &h, windows[0]), ["/a"], "the active window comes first");
    let order: Vec<BrowserStateEvent> = opened.borrow().iter().filter(|e| matches!(e, BrowserStateEvent::OpenWindow(_))).cloned().collect();
    assert_eq!(order.last(), Some(&BrowserStateEvent::OpenWindow(windows[0])), "and opens last, on top");
    let resaved = h.state.read_with(cx, |s, cx| s.snapshot(cx));
    let order: Vec<&str> = resaved.windows.iter().map(|w| w.tabs[0].url.path()).collect();
    assert_eq!(order, ["/a", "/b"], "a save keeps the order");
}

#[gpui::test]
fn a_record_whose_active_window_is_not_first_restores_it_first(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "active-second");
    let a = saved_window(vec![saved_tab("/a", false, None)], 0);
    let b = saved_window(vec![saved_tab("/b", false, None)], 0);
    restore(cx, &h, vec![a, b], 1);
    let windows = window_ids(cx, &h);
    assert_eq!(paths(cx, &h, windows[0]), ["/b"]);
}

#[gpui::test]
fn restored_tabs_keep_their_ids_titles_and_selection(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "restored-tabs");
    let tabs = vec![saved_tab("/a", false, None), saved_tab("/b", false, None)];
    let ids: Vec<_> = tabs.iter().map(|t| t.id).collect();
    restore(cx, &h, vec![saved_window(tabs, 1)], 0);
    let window = window_ids(cx, &h)[0];
    let shown: Vec<_> = h
        .tabs_of(cx, window)
        .into_iter()
        .map(|id| h.tab(cx, id).unwrap().read_with(cx, |t, _| (t.session_id(), t.display_title().to_string())))
        .collect();
    assert_eq!(shown, [(ids[0], "Saved /a".to_owned()), (ids[1], "Saved /b".to_owned())]);
    let selected = h.state.read_with(cx, |s, cx| s.window(window).unwrap().tabs.read(cx).selected());
    assert_eq!(selected, Some(h.tabs_of(cx, window)[1]));
}

#[gpui::test]
fn pinned_tabs_are_restored_pinned_and_saved_again(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "pinned");
    restore(cx, &h, vec![saved_window(vec![saved_tab("/a", true, None), saved_tab("/b", false, None)], 1)], 0);
    let window = window_ids(cx, &h)[0];
    let shown: Vec<bool> = h.state.read_with(cx, |s, cx| {
        let model = s.window(window).unwrap().tabs.read(cx);
        model.tabs().iter().map(|&t| model.is_pinned(t)).collect()
    });
    assert_eq!(shown, [true, false]);
    let resaved = h.state.read_with(cx, |s, cx| s.snapshot(cx));
    let pins: Vec<(&str, bool)> = resaved.windows[0].tabs.iter().map(|t| (t.url.path(), t.pinned)).collect();
    assert_eq!(pins, [("/a", true), ("/b", false)]);
}

#[gpui::test]
fn grouped_tabs_are_restored_in_their_groups_and_saved_again(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "grouped");
    let work = TabGroup { id: GroupId::new(), title: "Work".into(), color: GroupColor::Blue, collapsed: true };
    let tabs = vec![saved_tab("/a", false, Some(&work)), saved_tab("/b", false, Some(&work)), saved_tab("/c", false, None)];
    restore(cx, &h, vec![saved_window(tabs, 2)], 0);
    let window = window_ids(cx, &h)[0];
    let rows = h.state.read_with(cx, |s, cx| s.window(window).unwrap().tabs.read(cx).rows());
    let hidden: Vec<(Option<GroupId>, bool)> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Tab { group, hidden, .. } => Some((*group, *hidden)),
            Row::Header(_) => None,
        })
        .collect();
    assert_eq!(hidden, [(Some(work.id), true), (Some(work.id), true), (None, false)], "collapsed, selected tab outside");
    let resaved = h.state.read_with(cx, |s, cx| s.snapshot(cx));
    let groups: Vec<Option<TabGroup>> = resaved.windows[0].tabs.iter().map(|t| t.group.clone()).collect();
    assert_eq!(groups, [Some(work.clone()), Some(work), None]);
}

#[gpui::test]
fn a_launch_opens_the_startup_windows_raises_the_last_window_or_opens_a_new_one(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "invocations");
    let seen = events(cx, &h);
    let launch = |targets: &[&str]| Invocation { targets: targets.iter().map(|t| t.to_string()).collect(), ..Invocation::default() };

    let (outcome, errors) = h.state.update(cx, |s, cx| s.handle_invocation(launch(&["https://a.test/", "not a target"]), cx));
    assert_eq!(outcome, InvocationOutcome::StartupWindows, "no normal window: the startup windows");
    assert_eq!(errors, ["vsesvit: cannot open not a target"]);
    let first = window_ids(cx, &h)[0];
    assert_eq!(paths(cx, &h, first), ["/"]);

    let (outcome, _) = h.state.update(cx, |s, cx| s.handle_invocation(Invocation { startup_id: Some("t1".into()), ..launch(&[]) }, cx));
    assert_eq!(outcome, InvocationOutcome::Raised(first), "nothing to open: the last normal window comes forward");
    cx.run_until_parked();
    assert!(seen.borrow().contains(&BrowserStateEvent::RaiseWindow { window: first, startup_id: Some("t1".into()) }));

    let (outcome, _) = h.state.update(cx, |s, cx| s.handle_invocation(launch(&["https://b.test/"]), cx));
    let InvocationOutcome::NewWindow(second) = outcome else { panic!("{outcome:?}") };
    assert_ne!(second, first);
    assert_eq!(h.tabs_of(cx, second).len(), 1, "its targets in a window of their own, no blank tab");
}

#[gpui::test]
fn a_launch_with_only_private_windows_open_starts_afresh_in_a_normal_one(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "invocation-private");
    let private = open(cx, &h, Browsing::Private, &[]);
    let (outcome, _) = h.state.update(cx, |s, cx| s.handle_invocation(Invocation::default(), cx));
    assert_eq!(outcome, InvocationOutcome::StartupWindows);
    let normal = h.state.read_with(cx, |s, _| s.windows_of(Browsing::Normal));
    assert_eq!(normal.len(), 1);
    assert_ne!(normal[0], private);
}

#[gpui::test]
fn releasing_the_profile_frees_its_lock(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "release");
    open(cx, &h, Browsing::Normal, &["https://a.test/"]);
    open(cx, &h, Browsing::Private, &["https://b.test/"]);
    h.state.update(cx, |s, cx| s.schedule_session_save(cx));
    let Harness { state, core, root, engine, .. } = h;
    cx.update(|cx| drop(cx.remove_global::<crate::app::shell::Shell>()));
    drop(state);
    drop(engine);
    cx.run_until_parked();
    let profile = Rc::try_unwrap(core).unwrap_or_else(|core| panic!("{} other holders of the profile", Rc::strong_count(&core) - 1));
    drop(profile);
    let reopened = Profile::open(&root, OpenOptions::default());
    assert!(reopened.is_ok(), "the profile is still held: {:?}", reopened.err());
}

#[gpui::test]
fn ending_the_private_session_cancels_its_downloads_before_its_last_browser_closes(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "private-end");
    let asked: Rc<RefCell<Vec<Vec<FakeCommand>>>> = Rc::default();
    let engine = h.engine.clone();
    let record = asked.clone();
    h.state.update(cx, |s, _| {
        s.keep_alive.end_private_downloads = Rc::new(move |_| record.borrow_mut().push(engine.commands()));
    });
    let seen = events(cx, &h);
    let window = open(cx, &h, Browsing::Private, &["https://a.test/", "https://b.test/"]);
    assert!(h.engine.commands().contains(&FakeCommand::BeginPrivateContext));
    let [first, last] = h.tabs_of(cx, window)[..] else { panic!() };

    close(cx, &h, first, CloseReason::Tab);
    assert!(asked.borrow().is_empty(), "another private tab is open");
    assert!(!h.engine.commands().contains(&FakeCommand::EndPrivateContext));
    assert_eq!(h.state.read_with(cx, |s, _| s.closed_tabs(Browsing::Private).len()), 1);

    close(cx, &h, last, CloseReason::Tab);
    let at_cancel = asked.borrow().first().cloned().expect("the downloads were cancelled");
    assert!(!at_cancel.iter().any(|c| matches!(c, FakeCommand::CloseTab(t, _) if *t == last)), "before the last browser closed");
    let commands = h.engine.commands();
    let closed = commands.iter().position(|c| matches!(c, FakeCommand::CloseTab(t, _) if *t == last)).unwrap();
    let ended = commands.iter().position(|c| c == &FakeCommand::EndPrivateContext).expect("the context ends");
    assert!(closed < ended, "the context ends after the browser closed");
    assert_eq!(asked.borrow().len(), 1);
    assert!(seen.borrow().contains(&BrowserStateEvent::PrivateSessionEnded));
    assert_eq!(h.state.read_with(cx, |s, _| s.closed_tabs(Browsing::Private).len()), 0, "its closed tabs go with it");
    assert_eq!(h.state.read_with(cx, |s, _| s.private_session()), private_session::PrivateSession::Off);
}

#[gpui::test]
fn a_private_tab_that_closes_itself_last_ends_the_session(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "private-page-close");
    let cancelled = Rc::new(RefCell::new(0));
    let count = cancelled.clone();
    h.state.update(cx, |s, _| s.keep_alive.end_private_downloads = Rc::new(move |_| *count.borrow_mut() += 1));
    let window = open(cx, &h, Browsing::Private, &["https://a.test/"]);
    let tab = h.tabs_of(cx, window)[0];
    h.engine.close_from_page(tab);
    h.route(cx);
    assert_eq!(*cancelled.borrow(), 1);
    assert!(h.engine.commands().contains(&FakeCommand::EndPrivateContext));
}

struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

fn surface(cx: &mut TestAppContext) -> Box<dyn SurfaceOps> {
    cx.add_window(|_, _| Blank).update(cx, |_, window, _| fake::attach(window)).unwrap().unwrap()
}

/// Records what the engine had been asked when the page was parked.
struct ParkProbe {
    engine: Rc<vsesvit_engine::FakeEngine>,
    asked: Rc<RefCell<Option<Vec<FakeCommand>>>>,
}

impl SurfaceOps for ParkProbe {
    fn host(&self) -> vsesvit_engine::NativeHandle {
        super::testing::CONTENT_HOST
    }
    fn set_content_rect(&mut self, _: vsesvit_engine::PhysRect) {}
    fn show_page(&mut self, _: vsesvit_engine::NativeHandle, _: bool) {}
    fn set_holes(&mut self, _: &[crate::platform::Hole]) {}
    fn focus(&mut self, _: crate::platform::FocusTarget) -> crate::platform::FocusOutcome {
        crate::platform::FocusOutcome::Refused
    }
    fn move_page_to(&mut self, _: vsesvit_engine::NativeHandle, _: &mut dyn SurfaceOps) -> Result<(), crate::platform::PlatformError> {
        unreachable!()
    }
    fn park_page(&mut self, _: vsesvit_engine::NativeHandle, _: vsesvit_engine::NativeHandle) -> Result<(), crate::platform::PlatformError> {
        *self.asked.borrow_mut() = Some(self.engine.commands());
        Ok(())
    }
    fn open_aux(&mut self, _: vsesvit_engine::PhysRect) -> Result<crate::platform::AuxHost, crate::platform::PlatformError> {
        unreachable!()
    }
    fn set_aux_rect(&mut self, _: &crate::platform::AuxHost, _: vsesvit_engine::PhysRect) {}
    fn close_aux(&mut self, _: crate::platform::AuxHost) {}
    fn capture(&self) -> Result<image::RgbaImage, crate::platform::PlatformError> {
        unreachable!()
    }
    fn detach(self: Box<Self>) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[gpui::test]
fn the_page_is_parked_before_the_tab_is_detached(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "park-order");
    park_always(cx, &h);
    let window = open(cx, &h, Browsing::Normal, &["https://a.test/", "https://file.test/big.iso"]);
    let tab = h.tabs_of(cx, window)[1];
    h.engine.take_commands();
    let mut probe = ParkProbe { engine: h.engine.clone(), asked: Rc::default() };
    h.state.update(cx, |s, cx| s.close_tab(tab, CloseReason::Tab, Some(&mut probe), cx));
    assert_eq!(probe.asked.borrow().as_deref(), Some(&[][..]), "nothing asked of the engine before the reparent");
    assert_eq!(h.engine.commands(), [FakeCommand::DetachTab(tab)]);
}

fn park_always(cx: &mut TestAppContext, h: &Harness) {
    h.state.update(cx, |s, _| s.keep_alive.park_decision = Rc::new(|_, _| ParkDecision::Park));
}

fn native(cx: &mut TestAppContext, h: &Harness, tab: TabId) -> vsesvit_engine::NativeHandle {
    h.tab(cx, tab).unwrap().read_with(cx, |t, _| t.native()).expect("a live browser")
}

#[gpui::test]
fn closing_a_downloading_tab_parks_its_page_and_the_tab_ends_when_detached(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "park");
    park_always(cx, &h);
    let window = open(cx, &h, Browsing::Normal, &["https://a.test/", "https://file.test/big.iso"]);
    let tab = h.tabs_of(cx, window)[1];
    let page = native(cx, &h, tab);
    let mut surface = surface(cx);
    surface.show_page(page, false);
    h.engine.set_downloading(tab, true);
    h.engine.take_commands();

    h.state.update(cx, |s, cx| s.close_tab(tab, CloseReason::Tab, Some(&mut *surface), cx));
    let journal = surface.as_any().downcast_ref::<FakeSurface>().unwrap().journal();
    assert!(journal.borrow().contains(&SurfaceCall::ParkPage { page, hidden_host: HIDDEN_HOST }));
    assert_eq!(h.engine.commands(), [FakeCommand::DetachTab(tab)], "parked, then detached; never closed");
    assert!(h.tabs_of(cx, window).contains(&tab), "the tab stays until the engine says it detached");
    assert!(h.tab(cx, tab).unwrap().read_with(cx, |t, _| t.is_ending()));
    h.state.update(cx, |s, cx| s.close_tab(tab, CloseReason::Tab, Some(&mut *surface), cx));
    assert_eq!(h.engine.commands(), [FakeCommand::DetachTab(tab)], "a second close finds it parking");

    h.route(cx);
    assert!(!h.tabs_of(cx, window).contains(&tab), "Detached ends the tab");
    assert!(h.tab(cx, tab).is_none());
    assert_eq!(h.engine.parked().len(), 1, "its browser lives on for the download");
    assert_eq!(h.state.read_with(cx, |s, _| s.closed_tabs(Browsing::Normal).len()), 1);
}

#[gpui::test]
fn a_tab_whose_page_cannot_be_parked_closes(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "park-fails");
    park_always(cx, &h);
    let window = open(cx, &h, Browsing::Normal, &["https://a.test/", "https://b.test/"]);
    let tab = h.tabs_of(cx, window)[1];
    let mut surface = surface(cx);
    h.engine.take_commands();
    h.state.update(cx, |s, cx| s.close_tab(tab, CloseReason::Tab, Some(&mut *surface), cx));
    assert!(matches!(h.engine.commands()[..], [FakeCommand::CloseTab(t, _)] if t == tab), "the surface refused the page");
    h.route(cx);
    assert!(!h.tabs_of(cx, window).contains(&tab));
}

#[gpui::test]
fn without_a_download_a_tab_closes_and_ends_when_its_browser_closed(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "close");
    let window = open(cx, &h, Browsing::Normal, &["https://a.test/", "https://b.test/"]);
    let tab = h.tabs_of(cx, window)[1];
    let mut surface = surface(cx);
    h.engine.hold_close(tab);
    h.state.update(cx, |s, cx| s.close_tab(tab, CloseReason::Tab, Some(&mut *surface), cx));
    h.route(cx);
    assert!(h.tabs_of(cx, window).contains(&tab), "the browser has not closed yet");
    assert!(h.engine.commands().iter().any(|c| matches!(c, FakeCommand::CloseTab(t, _) if *t == tab)));
    h.engine.complete_close(tab);
    h.route(cx);
    assert!(!h.tabs_of(cx, window).contains(&tab));
}

#[gpui::test]
fn tabs_closed_with_their_window_stay_off_the_closed_tab_stack(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "closed-stack");
    let window = open(cx, &h, Browsing::Normal, &["https://a.test/", "https://b.test/", "https://c.test/"]);
    let [a, b, c] = h.tabs_of(cx, window)[..] else { panic!() };
    close(cx, &h, b, CloseReason::Tab);
    close(cx, &h, a, CloseReason::Window);
    close(cx, &h, c, CloseReason::Window);
    let closed: Vec<String> = h.state.read_with(cx, |s, _| s.closed_tabs(Browsing::Normal).iter().map(|t| t.url.clone()).collect());
    assert_eq!(closed, ["https://b.test/"]);
    let reopened = h.state.update(cx, |s, _| s.take_closed(Browsing::Normal, None)).map(|t| t.url);
    assert_eq!(reopened.as_deref(), Some("https://b.test/"));
    assert!(!h.state.read_with(cx, |s, _| s.can_reopen_closed_tab(Browsing::Normal)));
}

fn history_title(h: &Harness, address: &str) -> Option<String> {
    let found = h.core.borrow_mut().history().search(address, 10).ok()?;
    found.into_iter().find(|e| e.url.as_str() == address).map(|e| e.title)
}

#[gpui::test]
fn titles_go_to_history_except_error_pages_and_private_tabs(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "history");
    let window = open(cx, &h, Browsing::Normal, &["https://news.test/"]);
    let tab = h.tabs_of(cx, window)[0];
    h.engine.inject(BrowserEvent::Tab(tab, TabEvent::TitleChanged("News".into())));
    h.route(cx);
    assert_eq!(history_title(&h, "https://news.test/").as_deref(), Some("News"));

    let failed = url("https://news.test/");
    let commit = TabEvent::Committed {
        url: failed,
        kind: vsesvit_engine::CommitKind::ErrorPage,
        transition: vsesvit_engine::EngineTransition::Reload,
        mime: None,
    };
    h.engine.inject(BrowserEvent::Tab(tab, commit));
    h.engine.inject(BrowserEvent::Tab(tab, TabEvent::TitleChanged("Problem loading page".into())));
    h.route(cx);
    assert_eq!(history_title(&h, "https://news.test/").as_deref(), Some("News"), "the error page's title is ours");

    let private = open(cx, &h, Browsing::Private, &["https://secret.test/"]);
    let tab = h.tabs_of(cx, private)[0];
    h.engine.inject(BrowserEvent::Tab(tab, TabEvent::TitleChanged("Secret".into())));
    h.route(cx);
    assert_eq!(history_title(&h, "https://secret.test/"), None, "a private tab writes no history");
}

#[gpui::test]
fn a_tab_attached_to_another_window_is_found_there(cx: &mut TestAppContext) {
    let h = Harness::new(cx, "attach");
    let first = open(cx, &h, Browsing::Normal, &["https://a.test/"]);
    let second = open(cx, &h, Browsing::Normal, &["https://b.test/"]);
    let tab: Entity<crate::tab::Tab> = h.state.update(cx, |s, cx| {
        let tab = s.new_tab_entity(Browsing::Normal, InitialPage::Blank, None, cx);
        s.attach_tab(first, &tab, Insert::End, true, cx);
        tab
    });
    let id = tab.read_with(cx, |t, _| t.id());
    assert_eq!(h.state.read_with(cx, |s, _| s.window_of(id)), Some(first));
    h.state.update(cx, |s, cx| {
        s.window(first).unwrap().tabs.clone().update(cx, |m, cx| m.remove(id, cx));
        s.attach_tab(second, &tab, Insert::End, true, cx);
    });
    assert_eq!(h.state.read_with(cx, |s, _| s.window_of(id)), Some(second));
}
