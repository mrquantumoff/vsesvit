//! Owned by W2.1.
//! The overlay host in a TestAppContext window over `FakeSurface` and `FakeDesktop` (UA U04's
//! bookkeeping): holes follow what was drawn, every overlay closes exactly once with its reason,
//! presses inside and outside, the keyboard's loan and return, menus by mouse and by keyboard.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, IntoElement, Modifiers,
    MouseButton, Pixels, Point, Render, Subscription, TestAppContext, VisualTestContext,
    Window, div, point, px, size,
};

use super::menu::{Command, Menu, MenuView};
use super::toast::Toast;
use super::*;
use crate::engine_bridge::SharedSurface;
use crate::platform::fake::{self, FakeDesktop, FakeSurface};
use crate::platform::{Desktop as _, PhysPoint};

type Log = Rc<RefCell<Vec<String>>>;

/// A window: a button-sized chrome strip, a page slot below it, and the host.
struct Root {
    host: Entity<OverlayHost>,
    focus: FocusHandle,
    _watch: Subscription,
}

impl Focusable for Root {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().track_focus(&self.focus).child(div().h(px(40.)).w_full()).child(self.host.clone())
    }
}

/// A passive panel 100 by 50.
struct Panel;

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(100.)).h(px(50.))
    }
}

/// An interactive panel with its own focus.
struct Popover {
    focus: FocusHandle,
}

impl Render for Popover {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).w(px(120.)).h(px(60.))
    }
}

struct Rig {
    root: Entity<Root>,
    host: Entity<OverlayHost>,
    surface: SharedSurface,
    desktop: Rc<FakeDesktop>,
    events: Log,
}

fn rig(cx: &mut TestAppContext) -> (Rig, &mut VisualTestContext) {
    let desktop = Rc::new(FakeDesktop::init().unwrap());
    let presses = desktop.clone();
    cx.update(|cx| {
        crate::theme::init(cx);
        install(move || presses.pointer_presses(), cx);
    });
    let surface: Rc<RefCell<Option<SharedSurface>>> = Rc::default();
    let slot = surface.clone();
    let events: Log = Rc::default();
    let log = events.clone();
    let (root, vcx) = cx.add_window_view(move |window, cx| {
        let shared: SharedSurface = Rc::new(RefCell::new(fake::attach(window).unwrap()));
        *slot.borrow_mut() = Some(shared.clone());
        let host = cx.new(|cx| OverlayHost::new(Some(shared), window, cx));
        let log2 = log.clone();
        let watch = cx.subscribe(&host, move |_, _, event: &OverlayEvent, _| log2.borrow_mut().push(format!("{event:?}")));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Root { host, focus, _watch: watch }
    });
    vcx.run_until_parked();
    let host = root.read_with(vcx, |root, _| root.host.clone());
    let surface = surface.borrow().clone().unwrap();
    (Rig { root, host, surface, desktop, events }, vcx)
}

impl Rig {
    fn open(&self, cx: &mut VisualTestContext, spec: impl FnOnce(&mut Window, &mut gpui::App) -> OverlaySpec) -> OverlayId {
        let host = self.host.clone();
        let id = cx.update(|window, cx| {
            let spec = spec(window, cx);
            host.update(cx, |host, cx| host.open(spec, window, cx))
        });
        cx.run_until_parked();
        id
    }

    fn close(&self, cx: &mut VisualTestContext, id: OverlayId) {
        let host = self.host.clone();
        cx.update(|window, cx| host.update(cx, |host, cx| host.close(id, window, cx)));
        cx.run_until_parked();
    }

    fn info(&self, cx: &mut VisualTestContext) -> Vec<OverlayInfo> {
        self.host.read_with(cx, |host, _| host.open_overlays())
    }

    fn rect(&self, cx: &mut VisualTestContext, id: OverlayId) -> Bounds<Pixels> {
        self.info(cx).into_iter().find(|o| o.id == id).and_then(|o| o.rect).expect("drawn")
    }

    fn holes(&self) -> Vec<Hole> {
        self.surface.borrow().as_any().downcast_ref::<FakeSurface>().unwrap().holes().to_vec()
    }

    /// A press on the screen at a window point, as `Desktop::pointer_presses` reports one.
    fn screen_press(&self, cx: &mut VisualTestContext, at: Point<Pixels>) {
        self.queue_press(cx, at);
        cx.run_until_parked();
    }

    /// The same press, left in the host's queue.
    fn queue_press(&self, cx: &mut VisualTestContext, at: Point<Pixels>) {
        let (origin, scale) = cx.update(|window, _| (window.bounds().origin, window.scale_factor()));
        let phys = |o: Pixels, v: Pixels| (f32::from(o) * scale).round() as i32 + (f32::from(v) * scale).round() as i32;
        self.desktop.press(PhysPoint { x: phys(origin.x, at.x), y: phys(origin.y, at.y) });
    }

    fn menu(&self, cx: &mut VisualTestContext, id: OverlayId) -> Entity<MenuView> {
        self.host.read_with(cx, |host, _| host.content(id)).unwrap().downcast::<MenuView>().unwrap()
    }

    fn row(&self, cx: &mut VisualTestContext, id: OverlayId, row: usize) -> Point<Pixels> {
        let menu = self.menu(cx, id);
        menu.read_with(cx, |menu, _| menu.row_bounds()[row]).expect("row drawn").center()
    }
}

fn closes(log: &Log) -> impl Fn(DismissReason, &mut gpui::App) + 'static {
    let log = log.clone();
    move |why, _| log.borrow_mut().push(format!("{why:?}"))
}

fn button() -> Anchor {
    Anchor::Rect(Bounds { origin: point(px(20.), px(5.)), size: size(px(30.), px(30.)) })
}

fn panel(kind: OverlayKind) -> impl FnOnce(&mut Window, &mut gpui::App) -> OverlaySpec {
    move |_, cx| OverlaySpec::new(kind, button(), Placement::Below(Align::Start), cx.new(|_| Panel))
}

#[gpui::test]
fn the_holes_follow_what_was_drawn(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let id = rig.open(cx, panel(OverlayKind::Bubble));
    let rect = rig.rect(cx, id);
    assert_eq!(rect, Bounds { origin: point(px(20.), px(39.)), size: size(px(100.), px(50.)) }, "under the button");
    let scale = cx.update(|window, _| window.scale_factor());
    let want = Hole { rect: crate::window::page_area::physical(rect, scale), interactive: true };
    assert_eq!(rig.holes(), [want]);
    assert_eq!(rig.host.read_with(cx, |h, _| h.holes().to_vec()), [want]);
    assert_eq!(rig.host.read_with(cx, |h, _| h.screen_holes().len()), 1);

    let tip = rig.open(cx, |_, cx| OverlaySpec::new(OverlayKind::Tooltip, button(), Placement::Below(Align::End), cx.new(|_| Panel)).pass_through());
    assert_eq!(rig.holes().iter().filter(|h| !h.interactive).count(), 1, "a pass-through overlay is a hole that takes no clicks");
    assert_eq!(rig.host.read_with(cx, |h, _| h.screen_holes().len()), 1, "and the page keeps its presses there");
    rig.close(cx, tip);
    rig.close(cx, id);
    assert!(rig.holes().is_empty(), "the content host is whole again");
    assert!(rig.events.borrow().iter().filter(|e| *e == "HolesChanged").count() >= 2);
}

#[gpui::test]
fn every_overlay_closes_once_with_its_reason(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let log: Log = Rc::default();
    let on_close = closes(&log);
    let parent = rig.open(cx, move |_, cx| OverlaySpec::new(OverlayKind::Menu, button(), Placement::Below(Align::Start), cx.new(|_| Panel)).on_close(on_close));
    let on_close = closes(&log);
    let child = rig.open(cx, move |_, cx| {
        OverlaySpec::new(OverlayKind::Menu, button(), Placement::Beside, cx.new(|_| Panel)).child_of(parent).on_close(on_close)
    });
    rig.close(cx, parent);
    rig.close(cx, parent);
    assert_eq!(*log.borrow(), ["ParentClosed", "Owner"]);
    assert!(!rig.host.read_with(cx, |h, _| h.is_open(child) || h.is_open(parent)));

    let on_close = closes(&log);
    rig.open(cx, move |_, cx| OverlaySpec::new(OverlayKind::Warning, button(), Placement::Below(Align::Start), cx.new(|_| Panel)).dismiss(Dismiss::OWNER).on_close(on_close));
    let weak = rig.host.downgrade();
    let root = rig.root.downgrade();
    cx.update(|window, _| window.remove_window());
    drop(rig);
    cx.run_until_parked();
    cx.cx.update(|_| {});
    assert!(root.upgrade().is_none() && weak.upgrade().is_none());
    assert_eq!(log.borrow().last().map(String::as_str), Some("WindowClosed"), "a window that goes closes its overlays too");
}

#[gpui::test]
fn presses_close_from_outside_only(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let log: Log = Rc::default();
    let on_close = closes(&log);
    let id = rig.open(cx, move |_, cx| OverlaySpec::new(OverlayKind::Bubble, button(), Placement::Below(Align::Start), cx.new(|_| Panel)).on_close(on_close));
    let rect = rig.rect(cx, id);

    rig.screen_press(cx, rect.center());
    rig.screen_press(cx, point(px(25.), px(10.)));
    assert!(rig.host.read_with(cx, |h, _| h.is_open(id)), "a press on the overlay or on its anchor leaves it open");
    cx.simulate_mouse_down(rect.center(), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(rect.center(), MouseButton::Left, Modifiers::none());
    assert!(rig.host.read_with(cx, |h, _| h.is_open(id)), "so does a click on it in the window");

    rig.screen_press(cx, point(px(300.), px(300.)));
    assert!(!rig.host.read_with(cx, |h, _| h.is_open(id)), "a press on the page closes it");
    assert_eq!(*log.borrow(), ["OutsidePress"]);

    let on_close = closes(&log);
    let id = rig.open(cx, move |_, cx| OverlaySpec::new(OverlayKind::Bubble, button(), Placement::Below(Align::Start), cx.new(|_| Panel)).on_close(on_close));
    cx.simulate_mouse_down(point(px(300.), px(20.)), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert!(!rig.host.read_with(cx, |h, _| h.is_open(id)), "a press in the window's chrome closes it");

    let list = rig.open(cx, |_, cx| OverlaySpec::new(OverlayKind::Suggestions, button(), Placement::Below(Align::Start), cx.new(|_| Panel)).dismiss(Dismiss::OWNER));
    rig.screen_press(cx, point(px(300.), px(300.)));
    assert!(rig.host.read_with(cx, |h, _| h.is_open(list)), "an owner-only overlay ignores presses");
}

#[gpui::test]
fn a_press_queued_before_an_overlay_opened_does_not_close_it(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.queue_press(cx, point(px(300.), px(300.)));
    let id = rig.open(cx, panel(OverlayKind::Bubble));
    assert!(rig.host.read_with(cx, |h, _| h.is_open(id)));
}

#[gpui::test]
fn interactive_overlays_borrow_the_keyboard_and_give_it_back(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let root_focus = rig.root.read_with(cx, |root, _| root.focus.clone());
    let pop = cx.new(|cx| Popover { focus: cx.focus_handle() });
    let pop_focus = pop.read_with(cx, |p, _| p.focus.clone());
    let content = pop.clone();
    let id = rig.open(cx, move |_, _| OverlaySpec::new(OverlayKind::TabSearch, button(), Placement::Below(Align::Start), content).interactive(pop_focus));
    assert!(cx.update(|window, cx| pop.read(cx).focus.is_focused(window)));
    assert_eq!(rig.events.borrow().iter().filter(|e| *e == "KeyboardTaken").count(), 1);

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!rig.host.read_with(cx, |h, _| h.is_open(id)), "Escape closes it");
    assert!(cx.update(|window, _| root_focus.is_focused(window)), "the keyboard is back where it was");
    assert_eq!(rig.events.borrow().iter().filter(|e| *e == "KeyboardReturned").count(), 1);
}

#[gpui::test]
fn a_window_change_closes_transient_overlays(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let menu = rig.open(cx, panel(OverlayKind::Bubble));
    let list = rig.open(cx, |_, cx| OverlaySpec::new(OverlayKind::Suggestions, button(), Placement::Below(Align::Start), cx.new(|_| Panel)).dismiss(Dismiss::OWNER));
    cx.simulate_resize(size(px(700.), px(500.)));
    cx.run_until_parked();
    assert!(!rig.host.read_with(cx, |h, _| h.is_open(menu)));
    assert!(rig.host.read_with(cx, |h, _| h.is_open(list)));
}

#[gpui::test]
fn a_toast_goes_on_its_timeout_and_a_new_one_replaces_it(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let host = rig.host.clone();
    let first = cx.update(|window, cx| host.update(cx, |h, cx| h.toast(Toast::new("one"), window, cx)));
    let second = cx.update(|window, cx| host.update(cx, |h, cx| h.toast(Toast { timeout: Duration::from_secs(2), ..Toast::new("two") }, window, cx)));
    cx.run_until_parked();
    assert!(!rig.host.read_with(cx, |h, _| h.is_open(first)));
    let rect = rig.rect(cx, second);
    let viewport = cx.update(|window, _| window.viewport_size());
    assert!((rect.bottom() - (viewport.height - crate::theme::tokens::TOAST_BOTTOM)).abs() < px(1.), "at the bottom: {rect:?}");
    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();
    assert!(!rig.host.read_with(cx, |h, _| h.is_open(second)));
}

fn sample_menu(log: &Log) -> Menu {
    let say = |text: &'static str| {
        let log = log.clone();
        move |_: &mut Window, _: &mut gpui::App| log.borrow_mut().push(text.to_owned())
    };
    Menu::new()
        .item(Command::run("Back", say("back")))
        .item(Command::run("Bookmarks bar", say("bar")).checked(true))
        .submenu("Zoom", Menu::new().item(Command::run("90%", say("90")).radio(false)).item(Command::run("100%", say("100")).radio(true)))
        .separator()
        .item(Command::run("Disabled", say("disabled")).enabled(false))
}

fn open_menu(rig: &Rig, cx: &mut VisualTestContext, log: &Log) -> OverlayId {
    let host = rig.host.clone();
    let menu = sample_menu(log);
    let on_close = closes(log);
    let id = cx.update(|window, cx| host.update(cx, |h, cx| h.open_menu(menu, button(), Placement::Below(Align::Start), Some(Box::new(on_close)), window, cx)));
    cx.run_until_parked();
    id
}

#[gpui::test]
fn a_menu_runs_a_clicked_command_after_closing(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let log: Log = Rc::default();
    let id = open_menu(&rig, cx, &log);
    let disabled = rig.row(cx, id, 4);
    cx.simulate_click(disabled, Modifiers::none());
    cx.run_until_parked();
    assert!(rig.host.read_with(cx, |h, _| h.is_open(id)), "a disabled row does nothing");

    let bar = rig.row(cx, id, 1);
    cx.simulate_click(bar, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(*log.borrow(), ["bar", "Chosen"]);
    assert!(rig.info(cx).is_empty());
    assert!(rig.holes().is_empty());
}

#[gpui::test]
fn hovering_a_submenu_row_opens_it_beside_and_a_click_inside_runs_it(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let log: Log = Rc::default();
    let id = open_menu(&rig, cx, &log);
    let zoom = rig.row(cx, id, 2);
    cx.simulate_mouse_move(zoom, None, Modifiers::none());
    cx.run_until_parked();
    let info = rig.info(cx);
    assert_eq!(info.len(), 2, "{info:?}");
    let sub = info[1].id;
    assert_eq!(info[1].parent, Some(id));
    let (menu_rect, sub_rect) = (rig.rect(cx, id), rig.rect(cx, sub));
    assert!(sub_rect.left() > menu_rect.center().x && sub_rect.left() >= menu_rect.right() - px(8.), "beside the menu: {menu_rect:?} {sub_rect:?}");
    assert_eq!(rig.holes().len(), 2, "one hole per panel");

    let hundred = rig.row(cx, sub, 1);
    rig.screen_press(cx, hundred);
    assert_eq!(rig.info(cx).len(), 2, "a press in the submenu is inside the menu");
    cx.simulate_mouse_move(hundred, None, Modifiers::none());
    cx.simulate_click(hundred, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(*log.borrow(), ["100", "Chosen"]);
    assert!(rig.info(cx).is_empty());
}

#[gpui::test]
fn a_menu_works_from_the_keyboard(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let log: Log = Rc::default();
    let id = open_menu(&rig, cx, &log);
    let menu = rig.menu(cx, id);
    cx.simulate_keystrokes("down down down");
    assert_eq!(menu.read_with(cx, |m, _| m.highlighted()), Some(2), "Back, Bookmarks bar, Zoom");
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    let (_, sub) = menu.read_with(cx, |m, cx| m.open_submenu(cx)).expect("Right opened Zoom");
    let submenu = rig.menu(cx, sub);
    assert_eq!(submenu.read_with(cx, |m, _| m.highlighted()), Some(0));
    cx.simulate_keystrokes("left");
    cx.run_until_parked();
    assert!(!rig.host.read_with(cx, |h, _| h.is_open(sub)) && rig.host.read_with(cx, |h, _| h.is_open(id)), "Left closes only the submenu");
    cx.simulate_keystrokes("enter down enter");
    cx.run_until_parked();
    assert_eq!(*log.borrow(), ["100", "Chosen"]);

    let id = open_menu(&rig, cx, &log);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!rig.host.read_with(cx, |h, _| h.is_open(id)));
    assert_eq!(log.borrow().last().map(String::as_str), Some("Escape"));
}

#[gpui::test]
fn the_host_of_a_window_is_found_from_the_window(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let found = cx.update(|window, cx| OverlayHost::of(window, cx));
    assert_eq!(found, Some(rig.host.clone()));
}
