//! Owned by W2.1.
//! Overlays: everything that may cover the page (design section 0, UA section 1.7). An overlay is an
//! in-window GPUI element drawn above the window's other elements, plus a hole in the content host
//! so it shows over the page and takes the page's clicks. One [`OverlayHost`] per window draws them,
//! keeps the holes in step with what it drew, lends them the keyboard and closes them by the
//! [`Dismiss`] rules. Nothing leaves the window: an overlay that does not fit is flipped or pushed
//! inside it (`inline::place`).
//!
//! The API, frozen for the units that build on it (W2.2 to W2.6):
//! - A window makes its host with [`OverlayHost::new`] and renders it as its last child; features find
//!   it with [`OverlayHost::of`].
//! - [`OverlayHost::open`] takes an [`OverlaySpec`]: what to show ([`OverlayKind`], an `AnyView`),
//!   where ([`Anchor`] in window pixels, [`Placement`]), whether it takes the keyboard
//!   ([`OverlayFocus`]), what closes it ([`Dismiss`]), the overlay it was opened from, and an
//!   `on_close` callback that runs exactly once with the [`DismissReason`].
//! - [`OverlayHost::open_menu`] (`menu.rs`) shows a [`menu::Menu`]; `toast.rs`, `tooltip.rs` and
//!   `bubble_slot.rs` are small helpers over `open`.
//! - The window listens for [`OverlayEvent`]s: the holes changed (tell the focus policy, whose page
//!   area excludes them) and the keyboard was taken or given back (`Focus::overlay_wants_keyboard`
//!   and back).
//! - [`install`] gives every host the desktop's pointer presses, which close overlays on a press
//!   anywhere outside them, the page included.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    AnyView, AnyWindowHandle, App, Bounds, Context, DispatchPhase, Entity, EventEmitter, FocusHandle,
    Global, InteractiveElement as _, IntoElement, KeyDownEvent, MouseDownEvent, ParentElement as _, Pixels, Point,
    Render, Styled as _, Subscription, WeakEntity, Window, canvas, deferred, div, point, px,
};

use gpui_component::ActiveTheme as _;

use crate::engine_bridge::SharedSurface;
use crate::platform::{Hole, PhysPoint, PhysRect};

pub mod bubble_slot;
pub mod dismiss;
pub mod inline;
pub mod menu;
pub mod toast;
pub mod tooltip;

pub use dismiss::{Dismiss, DismissReason, OverlayId};
pub use inline::{Align, Placement, Spot};

use dismiss::Book;
use inline::Placed;

/// Above any other deferred element of the window (gpui-component's own popovers use 1).
const LAYER_PRIORITY: usize = 1000;

/// What an overlay is, for [`OverlayHost::close_kind`] and the self-test's list.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum OverlayKind {
    Menu,
    Bubble,
    Suggestions,
    Prompt,
    Warning,
    Toast,
    Tooltip,
    Notice,
    LinkPreview,
    TabSearch,
    GroupEditor,
}

/// What an overlay is placed against, in window pixels.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Anchor {
    /// A widget's bounds: a button, the address field, a menu row, the page's rectangle.
    Rect(Bounds<Pixels>),
    /// A point: where a context menu was asked for.
    Point(Point<Pixels>),
}

impl Anchor {
    fn bounds(self) -> Bounds<Pixels> {
        match self {
            Anchor::Rect(r) => r,
            Anchor::Point(p) => Bounds { origin: p, size: gpui::size(px(0.), px(0.)) },
        }
    }
}

/// Whether an overlay takes the keyboard.
#[derive(Clone)]
pub enum OverlayFocus {
    /// Never: its owner keeps the keyboard and drives it (the omnibox's suggestions). It still takes
    /// clicks.
    Passive,
    /// GPUI focuses this handle (inside the overlay's view) when it opens; the window's keyboard goes
    /// to its overlays until the last interactive one closes, then back where it was.
    Interactive(FocusHandle),
}

/// What a menu row or a toast's button runs.
pub type WindowAction = Rc<dyn Fn(&mut Window, &mut App)>;

/// Runs once when the overlay closes, after the update that closed it.
pub type OnClose = Box<dyn FnOnce(DismissReason, &mut App)>;

/// An overlay to open.
pub struct OverlaySpec {
    pub kind: OverlayKind,
    pub anchor: Anchor,
    pub placement: Placement,
    pub content: AnyView,
    pub focus: OverlayFocus,
    pub dismiss: Dismiss,
    /// The overlay it opens from (a submenu's menu): it closes with it, and a press inside either
    /// counts as inside both.
    pub parent: Option<OverlayId>,
    /// Paint only: on Windows the page keeps its clicks under it (on X11 SHAPE cuts paint and input
    /// together). Tooltips and the link preview.
    pub pass_through: bool,
    pub on_close: Option<OnClose>,
}

impl OverlaySpec {
    /// A passive overlay that the [`Dismiss::TRANSIENT`] rules close.
    pub fn new(kind: OverlayKind, anchor: Anchor, placement: Placement, content: impl Into<AnyView>) -> OverlaySpec {
        OverlaySpec {
            kind,
            anchor,
            placement,
            content: content.into(),
            focus: OverlayFocus::Passive,
            dismiss: Dismiss::TRANSIENT,
            parent: None,
            pass_through: false,
            on_close: None,
        }
    }

    pub fn interactive(mut self, focus: FocusHandle) -> OverlaySpec {
        self.focus = OverlayFocus::Interactive(focus);
        self
    }

    pub fn dismiss(mut self, dismiss: Dismiss) -> OverlaySpec {
        self.dismiss = dismiss;
        self
    }

    pub fn child_of(mut self, parent: OverlayId) -> OverlaySpec {
        self.parent = Some(parent);
        self
    }

    pub fn pass_through(mut self) -> OverlaySpec {
        self.pass_through = true;
        self
    }

    pub fn on_close(mut self, on_close: impl FnOnce(DismissReason, &mut App) + 'static) -> OverlaySpec {
        self.on_close = Some(Box::new(on_close));
        self
    }
}

/// One open overlay as the self-test and tests see it.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayInfo {
    pub id: OverlayId,
    pub kind: OverlayKind,
    pub parent: Option<OverlayId>,
    /// Where it was drawn last, in window pixels; `None` before its first frame.
    pub rect: Option<Bounds<Pixels>>,
    pub interactive: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OverlayEvent {
    /// The holes in the content host changed: [`OverlayHost::screen_holes`] is what the focus
    /// policy's page area must leave out.
    HolesChanged,
    /// An interactive overlay took the window's keyboard.
    KeyboardTaken,
    /// The last interactive overlay closed; the keyboard goes back to its owner.
    KeyboardReturned,
}

/// What the host keeps per overlay.
struct Slot {
    kind: OverlayKind,
    placement: Placement,
    content: AnyView,
    focus: OverlayFocus,
    pass_through: bool,
    on_close: Option<OnClose>,
}

impl Slot {
    fn handle(&self) -> Option<&FocusHandle> {
        match &self.focus {
            OverlayFocus::Interactive(handle) => Some(handle),
            OverlayFocus::Passive => None,
        }
    }
}

type Presses = Rc<RefCell<UnboundedReceiver<PhysPoint>>>;
type Drawn = Rc<RefCell<Vec<(OverlayId, Bounds<Pixels>)>>>;

/// Where hosts get pointer presses from (`Desktop::pointer_presses`, one receiver per host).
struct PressSource(Rc<dyn Fn() -> UnboundedReceiver<PhysPoint>>);

impl Global for PressSource {}

/// Every window's host, for [`OverlayHost::of`].
#[derive(Default)]
struct Hosts(HashMap<gpui::WindowId, WeakEntity<OverlayHost>>);

impl Global for Hosts {}

/// Gives every overlay host made from now on the screen's pointer presses, so a press anywhere
/// outside an overlay closes it: on the page, in another window. Boot passes
/// `Desktop::pointer_presses`; without it only presses inside the GPUI window dismiss.
pub fn install(presses: impl Fn() -> UnboundedReceiver<PhysPoint> + 'static, cx: &mut App) {
    cx.set_global(PressSource(Rc::new(presses)));
}

/// A window's overlays.
pub struct OverlayHost {
    book: Book<Slot>,
    surface: Option<SharedSurface>,
    window: AnyWindowHandle,
    /// What the content host was last given.
    holes: Vec<Hole>,
    /// The interactive holes on the screen, for the focus policy's page area.
    screen_holes: Vec<PhysRect>,
    /// Some while overlays have the window's keyboard: the GPUI focus to give back.
    lent: Option<Option<FocusHandle>>,
    presses: Option<Presses>,
    active: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<OverlayEvent> for OverlayHost {}

impl OverlayHost {
    /// The host of `window`, whose overlays cut holes in `surface` (a window that shows no page
    /// passes `None`).
    pub fn new(surface: Option<SharedSurface>, window: &mut Window, cx: &mut Context<Self>) -> OverlayHost {
        let handle = window.window_handle();
        let me = cx.entity().downgrade();
        cx.default_global::<Hosts>().0.insert(handle.window_id(), me);
        let subscriptions = vec![
            cx.observe_window_bounds(window, |host, window, cx| host.window_changed(window, cx)),
            cx.observe_window_activation(window, |host, window, cx| {
                let active = window.is_window_active();
                let lost = host.active && !active;
                host.active = active;
                if lost {
                    host.window_changed(window, cx);
                }
            }),
        ];
        // Detached: a subscription kept in the host would go with it before the release ran.
        cx.on_release(|host, cx| host.released(cx)).detach();
        let presses = cx.try_global::<PressSource>().map(|source| Rc::new(RefCell::new((source.0)())));
        if let Some(presses) = presses.clone() {
            cx.spawn_in(window, async move |this, cx| {
                while let Some(at) = futures::future::poll_fn(|task| presses.borrow_mut().poll_next_unpin(task)).await {
                    if this.update_in(cx, |host, window, cx| host.screen_press(at, window, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
        }
        OverlayHost {
            book: Book::default(),
            surface,
            window: handle,
            holes: Vec::new(),
            screen_holes: Vec::new(),
            lent: None,
            presses,
            active: window.is_window_active(),
            _subscriptions: subscriptions,
        }
    }

    /// The host of `window`, if it has one.
    pub fn of(window: &Window, cx: &App) -> Option<Entity<OverlayHost>> {
        cx.try_global::<Hosts>()?.0.get(&window.window_handle().window_id())?.upgrade()
    }

    /// Opens an overlay. Presses already queued predate it and are dropped, so the click that opened
    /// it never closes it.
    pub fn open(&mut self, spec: OverlaySpec, window: &mut Window, cx: &mut Context<Self>) -> OverlayId {
        if let Some(presses) = &self.presses {
            while presses.borrow_mut().try_recv().is_ok() {}
        }
        let focus = spec.focus.clone();
        let timeout = spec.dismiss.timeout;
        let slot = Slot {
            kind: spec.kind,
            placement: spec.placement,
            content: spec.content,
            focus: spec.focus,
            pass_through: spec.pass_through,
            on_close: spec.on_close,
        };
        let id = self.book.open(spec.parent, spec.anchor.bounds(), spec.dismiss, slot);
        if let OverlayFocus::Interactive(handle) = focus {
            if self.lent.is_none() {
                self.lent = Some(window.focused(cx));
                cx.emit(OverlayEvent::KeyboardTaken);
            }
            window.focus(&handle, cx);
        }
        if let Some(after) = timeout {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(after).await;
                let _ = this.update_in(cx, |host, window, cx| host.close_with(id, DismissReason::Timeout, window, cx));
            })
            .detach();
        }
        cx.notify();
        id
    }

    /// Closes an overlay and those opened from it. Closing one that is gone does nothing.
    pub fn close(&mut self, id: OverlayId, window: &mut Window, cx: &mut Context<Self>) {
        self.close_with(id, DismissReason::Owner, window, cx);
    }

    /// Closes every overlay of `kind`.
    pub fn close_kind(&mut self, kind: OverlayKind, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<OverlayId> = self.book.iter().filter(|e| e.payload.kind == kind).map(|e| e.id).collect();
        for id in ids {
            self.close_with(id, DismissReason::Owner, window, cx);
        }
    }

    /// Closes `id` (and what opened from it) for `reason`: each closed overlay's `on_close` runs once,
    /// after this update, `id`'s with `reason` and the others' with `ParentClosed`.
    pub fn close_with(&mut self, id: OverlayId, reason: DismissReason, window: &mut Window, cx: &mut Context<Self>) {
        let closed = self.book.close(id);
        if closed.is_empty() {
            return;
        }
        let had_keyboard = closed.iter().any(|e| e.payload.handle().is_some_and(|h| h.contains_focused(window, cx)));
        for mut entry in closed {
            let why = if entry.id == id { reason } else { DismissReason::ParentClosed };
            if let Some(on_close) = entry.payload.on_close.take() {
                App::defer(cx, move |cx| on_close(why, cx));
            }
        }
        self.settle_keyboard(had_keyboard, window, cx);
        cx.notify();
    }

    /// The window is going: every overlay closes with `WindowClosed` and the host lets go of the
    /// surface, which the window detaches next.
    pub fn window_closing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let roots: Vec<OverlayId> = self.book.iter().filter(|e| e.parent.is_none()).map(|e| e.id).collect();
        for root in roots {
            self.close_with(root, DismissReason::WindowClosed, window, cx);
        }
        self.surface = None;
    }

    /// The overlay a menu chain starts from.
    pub fn root_of(&self, id: OverlayId) -> OverlayId {
        self.book.root_of(id)
    }

    /// Moves an overlay to a new anchor (the window was laid out again).
    pub fn reposition(&mut self, id: OverlayId, anchor: Anchor, cx: &mut Context<Self>) {
        if let Some(entry) = self.book.get_mut(id) {
            entry.anchor = anchor.bounds();
            cx.notify();
        }
    }

    pub fn is_open(&self, id: OverlayId) -> bool {
        self.book.get(id).is_some()
    }

    /// What an open overlay shows.
    pub fn content(&self, id: OverlayId) -> Option<AnyView> {
        self.book.get(id).map(|e| e.payload.content.clone())
    }

    /// The open overlays, oldest first.
    pub fn open_overlays(&self) -> Vec<OverlayInfo> {
        self.book
            .iter()
            .map(|e| OverlayInfo { id: e.id, kind: e.payload.kind, parent: e.parent, rect: e.rect, interactive: e.payload.handle().is_some() })
            .collect()
    }

    /// The holes the content host has now, in the window's client area (physical pixels).
    pub fn holes(&self) -> &[Hole] {
        &self.holes
    }

    /// The interactive holes on the screen (physical pixels): presses there are the overlays', not
    /// the page's.
    pub fn screen_holes(&self) -> &[PhysRect] {
        &self.screen_holes
    }

    /// A press at a window point: GPUI's own, or the desktop's converted.
    fn press(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        for root in self.book.outside_press(at) {
            self.close_with(root, DismissReason::OutsidePress, window, cx);
        }
    }

    fn screen_press(&mut self, at: PhysPoint, window: &mut Window, cx: &mut Context<Self>) {
        let scale = window.scale_factor();
        let (ox, oy) = screen_origin(window);
        let local = point(px((at.x - ox) as f32 / scale), px((at.y - oy) as f32 / scale));
        self.press(local, window, cx);
    }

    fn window_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for root in self.book.window_changed() {
            self.close_with(root, DismissReason::WindowChanged, window, cx);
        }
        self.publish_screen_holes(window, cx);
    }

    /// After a close: the keyboard goes to the newest interactive overlay left, or, when none is
    /// left, back where it was before the first one opened. Focus moved elsewhere meanwhile (a press
    /// into the address field) stays there.
    fn settle_keyboard(&mut self, had_keyboard: bool, window: &mut Window, cx: &mut Context<Self>) {
        let newest = self.book.iter().rev().find_map(|e| e.payload.handle().cloned());
        match newest {
            Some(handle) => {
                if had_keyboard {
                    window.focus(&handle, cx);
                }
            }
            None => {
                let Some(previous) = self.lent.take() else { return };
                if had_keyboard && let Some(previous) = previous {
                    window.focus(&previous, cx);
                }
                cx.emit(OverlayEvent::KeyboardReturned);
            }
        }
    }

    /// The frame drew the overlays at `drawn`: record it and bring the holes in line.
    fn frame_done(&mut self, drawn: Vec<(OverlayId, Bounds<Pixels>)>, window: &mut Window, cx: &mut Context<Self>) {
        self.book.set_rects(&drawn);
        let scale = window.scale_factor();
        let corner = (f32::from(cx.theme().radius) * scale).ceil() as u32;
        let holes: Vec<Hole> = self
            .book
            .iter()
            .filter_map(|e| e.rect.map(|r| (crate::window::page_area::physical(r, scale), !e.payload.pass_through)))
            .flat_map(|(rect, interactive)| rounded(rect, corner).into_iter().map(move |rect| Hole { rect, interactive }))
            .collect();
        if holes == self.holes {
            return;
        }
        if let Some(surface) = &self.surface {
            let Ok(mut surface) = surface.try_borrow_mut() else {
                log::debug!("overlay: the surface is busy; the holes follow next frame");
                window.refresh();
                return;
            };
            surface.set_holes(&holes);
        }
        self.holes = holes;
        self.publish_screen_holes(window, cx);
    }

    fn publish_screen_holes(&mut self, window: &Window, cx: &mut Context<Self>) {
        let (ox, oy) = screen_origin(window);
        let screen: Vec<PhysRect> =
            self.holes.iter().filter(|h| h.interactive).map(|h| PhysRect { x: h.rect.x + ox, y: h.rect.y + oy, ..h.rect }).collect();
        if screen != self.screen_holes {
            self.screen_holes = screen;
            cx.emit(OverlayEvent::HolesChanged);
        }
    }

    /// The window went: every overlay still open closes, so each `on_close` still runs once.
    fn released(&mut self, cx: &mut App) {
        let window = self.window.window_id();
        if cx.try_global::<Hosts>().is_some_and(|hosts| hosts.0.get(&window).is_some_and(|h| h.upgrade().is_none())) {
            cx.global_mut::<Hosts>().0.remove(&window);
        }
        let ids: Vec<OverlayId> = self.book.iter().filter(|e| e.parent.is_none()).map(|e| e.id).collect();
        for id in ids {
            for mut entry in self.book.close(id) {
                let why = if entry.id == id { DismissReason::WindowClosed } else { DismissReason::ParentClosed };
                if let Some(on_close) = entry.payload.on_close.take() {
                    cx.defer(move |cx| on_close(why, cx));
                }
            }
        }
    }
}

/// The holes for a panel with corners of radius `corner`: the panel less what lies outside its
/// rounded corners, as a staircase of rows (one rectangle per run of rows with the same inset), so
/// the page shows there. On X11 the shape cuts paint too, so a plain rectangle would show the
/// window's own background in the corners; on Windows only the input region follows.
fn rounded(rect: PhysRect, corner: u32) -> Vec<PhysRect> {
    if corner == 0 || rect.width <= corner * 2 || rect.height <= corner * 2 {
        return vec![rect];
    }
    let r = corner as f32;
    let inset = |row: u32| {
        let dy = r - row as f32 - 0.5;
        (r - (r * r - dy * dy).max(0.).sqrt()).round() as u32
    };
    let mut top: Vec<(u32, u32, u32)> = Vec::new();
    for row in 0..corner {
        match top.last_mut() {
            Some((_, rows, last)) if *last == inset(row) => *rows += 1,
            _ => top.push((row, 1, inset(row))),
        }
    }
    let band = |y: i32, height: u32, inset: u32| PhysRect { x: rect.x + inset as i32, y, width: rect.width - inset * 2, height };
    let middle = band(rect.y + corner as i32, rect.height - corner * 2, 0);
    let bottom = rect.y + rect.height as i32;
    top.iter()
        .map(|&(row, rows, inset)| band(rect.y + row as i32, rows, inset))
        .chain(std::iter::once(middle))
        .chain(top.iter().map(|&(row, rows, inset)| band(bottom - (row + rows) as i32, rows, inset)))
        .collect()
}

/// The window's client origin on the screen, in physical pixels, rounded as the page's rectangle is.
fn screen_origin(window: &Window) -> (i32, i32) {
    let scale = window.scale_factor();
    let origin = window.bounds().origin;
    ((f32::from(origin.x) * scale).round() as i32, (f32::from(origin.y) * scale).round() as i32)
}

impl Render for OverlayHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let drawn: Drawn = Rc::default();
        let mut layer = div().absolute().top_0().left_0().size_full();
        for entry in self.book.iter() {
            let id = entry.id;
            let mut panel = div().id(("overlay", id.raw())).occlude().child(entry.payload.content.clone());
            if entry.dismiss.escape {
                let this = this.clone();
                panel = panel.on_key_down(move |event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        cx.stop_propagation();
                        let _ = this.update(cx, |host, cx| host.close_with(id, DismissReason::Escape, window, cx));
                    }
                });
            }
            let sink = drawn.clone();
            layer = layer.child(Placed::new(entry.anchor, entry.payload.placement, panel, move |rect, _, _| sink.borrow_mut().push((id, rect))));
        }
        let listen = this.clone();
        let frame = canvas(
            move |_, window, cx| {
                let drawn = drawn.take();
                let _ = this.update(cx, |host, cx| host.frame_done(drawn, window, cx));
            },
            move |_, _, window, _| {
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = listen.update(cx, |host, cx| host.press(event.position, window, cx));
                    }
                });
            },
        )
        .size_0();
        deferred(layer.child(frame)).with_priority(LAYER_PRIORITY)
    }
}

#[cfg(test)]
mod tests;
