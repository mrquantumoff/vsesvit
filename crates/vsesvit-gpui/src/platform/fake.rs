//! Owned by W1.1.
//! `FakeSurface` and `FakeDesktop`: the platform for `TestAppContext` tests, which have no display.
//! They keep the state a real backend would leave behind and a journal of every call, which outlives
//! `detach` so a test can check what happened before it. They never replace the `Surface` and `Os`
//! aliases (design section 1.4).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::WindowId;

use super::{AuxHost, Desktop, FocusOutcome, FocusTarget, Hole, NativeHandle, NativeSurface, PhysPoint, PhysRect, PlatformError, SurfaceOps};

/// `fake::attach(window)`: what `platform::attach` is in the binary.
pub fn attach(window: &gpui::Window) -> Result<Box<dyn SurfaceOps>, PlatformError> {
    Ok(Box::new(<FakeSurface as NativeSurface>::attach(window)?))
}

thread_local! {
    /// Far from FakeEngine's page handles (0x1000 + n).
    static NEXT_HANDLE: Cell<u64> = const { Cell::new(0xfa00_0000) };
}

fn next_handle() -> NativeHandle {
    NEXT_HANDLE.with(|n| {
        n.set(n.get() + 1);
        NativeHandle::from_raw(n.get())
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceCall {
    SetContentRect(PhysRect),
    ShowPage { page: NativeHandle, visible: bool },
    SetHoles(Vec<Hole>),
    Focus { target: FocusTarget, outcome: FocusOutcome },
    MovePage { page: NativeHandle, to: NativeHandle },
    ParkPage { page: NativeHandle, hidden_host: NativeHandle },
    OpenAux { aux: NativeHandle, rect: PhysRect },
    SetAuxRect { aux: NativeHandle, rect: PhysRect },
    CloseAux(NativeHandle),
    Capture,
    Detach,
}

/// One surface's calls in order, shared with the test.
pub type Journal = Rc<RefCell<Vec<SurfaceCall>>>;

pub struct FakeSurface {
    host: NativeHandle,
    content: PhysRect,
    pages: Vec<(NativeHandle, bool)>,
    holes: Vec<Hole>,
    focused: FocusTarget,
    window_active: bool,
    aux: Vec<(NativeHandle, PhysRect)>,
    journal: Journal,
}

impl FakeSurface {
    pub fn journal(&self) -> Journal {
        self.journal.clone()
    }

    pub fn content_rect(&self) -> PhysRect {
        self.content
    }

    /// `Some(shown)` for a page in this surface's host.
    pub fn page(&self, page: NativeHandle) -> Option<bool> {
        self.pages.iter().find(|(p, _)| *p == page).map(|&(_, shown)| shown)
    }

    pub fn holes(&self) -> &[Hole] {
        &self.holes
    }

    pub fn focused(&self) -> FocusTarget {
        self.focused
    }

    /// While false, `focus` is refused, as when another toplevel is active.
    pub fn set_window_active(&mut self, active: bool) {
        self.window_active = active;
    }

    pub fn aux_rect(&self, aux: &AuxHost) -> Option<PhysRect> {
        self.aux.iter().find(|(a, _)| *a == aux.native()).map(|&(_, r)| r)
    }

    fn log(&self, call: SurfaceCall) {
        self.journal.borrow_mut().push(call);
    }

    fn take_page(&mut self, page: NativeHandle) -> Result<(), PlatformError> {
        let before = self.pages.len();
        self.pages.retain(|(p, _)| *p != page);
        if self.pages.len() == before {
            return Err(PlatformError::new(format!("{page:?} is not a page of this surface")));
        }
        if self.focused == FocusTarget::Page(page) {
            self.focused = FocusTarget::Chrome;
        }
        Ok(())
    }
}

impl NativeSurface for FakeSurface {
    const TRANSPARENT_CONTENT: bool = false;

    fn attach(_window: &gpui::Window) -> Result<FakeSurface, PlatformError> {
        Ok(FakeSurface {
            host: next_handle(),
            content: PhysRect::default(),
            pages: Vec::new(),
            holes: Vec::new(),
            focused: FocusTarget::Chrome,
            window_active: true,
            aux: Vec::new(),
            journal: Journal::default(),
        })
    }

    fn host(&self) -> NativeHandle {
        self.host
    }

    fn set_content_rect(&mut self, rect: PhysRect) {
        self.log(SurfaceCall::SetContentRect(rect));
        self.content = rect;
    }

    fn show_page(&mut self, page: NativeHandle, visible: bool) {
        self.log(SurfaceCall::ShowPage { page, visible });
        match self.pages.iter_mut().find(|(p, _)| *p == page) {
            Some(entry) => entry.1 = visible,
            None => self.pages.push((page, visible)),
        }
    }

    fn set_holes(&mut self, holes: &[Hole]) {
        self.log(SurfaceCall::SetHoles(holes.to_vec()));
        self.holes = holes.to_vec();
    }

    /// Refused while the window is inactive, and for a page that is hidden or not here (X11 cannot
    /// focus an unviewable window).
    fn focus(&mut self, target: FocusTarget) -> FocusOutcome {
        let possible = match target {
            FocusTarget::Chrome => true,
            FocusTarget::Page(page) => self.page(page) == Some(true),
        };
        let outcome = if self.window_active && possible { FocusOutcome::Done } else { FocusOutcome::Refused };
        if outcome == FocusOutcome::Done {
            self.focused = target;
        }
        self.log(SurfaceCall::Focus { target, outcome });
        outcome
    }

    fn move_page_to(&mut self, page: NativeHandle, to: &mut FakeSurface) -> Result<(), PlatformError> {
        self.take_page(page)?;
        self.log(SurfaceCall::MovePage { page, to: to.host });
        to.pages.push((page, false));
        Ok(())
    }

    fn park_page(&mut self, page: NativeHandle, hidden_host: NativeHandle) -> Result<(), PlatformError> {
        self.take_page(page)?;
        self.log(SurfaceCall::ParkPage { page, hidden_host });
        Ok(())
    }

    fn open_aux(&mut self, rect: PhysRect) -> Result<AuxHost, PlatformError> {
        let aux = next_handle();
        self.log(SurfaceCall::OpenAux { aux, rect });
        self.aux.push((aux, rect));
        Ok(AuxHost(aux))
    }

    fn set_aux_rect(&mut self, aux: &AuxHost, rect: PhysRect) {
        self.log(SurfaceCall::SetAuxRect { aux: aux.native(), rect });
        if let Some(entry) = self.aux.iter_mut().find(|(a, _)| *a == aux.native()) {
            entry.1 = rect;
        }
    }

    fn close_aux(&mut self, aux: AuxHost) {
        self.log(SurfaceCall::CloseAux(aux.native()));
        self.aux.retain(|(a, _)| *a != aux.native());
    }

    /// Blank, as large as the content rect reaches.
    fn capture(&self) -> Result<image::RgbaImage, PlatformError> {
        self.log(SurfaceCall::Capture);
        let right = (self.content.x.max(0) as u32 + self.content.width).max(1);
        let bottom = (self.content.y.max(0) as u32 + self.content.height).max(1);
        Ok(image::RgbaImage::new(right, bottom))
    }

    fn detach(self) {
        self.log(SurfaceCall::Detach);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopCall {
    AdoptPopup { popup: WindowId, owner: WindowId },
    SetDialogParent { dialog: WindowId, parent: WindowId },
    Raise { window: WindowId, token: Option<String> },
    Neutralize { window: NativeHandle, ok: bool },
}

#[derive(Default)]
struct DesktopState {
    calls: Vec<DesktopCall>,
    presses: Vec<UnboundedSender<PhysPoint>>,
    /// Bottom to top, in screen pixels.
    toplevels: Vec<(NativeHandle, PhysRect)>,
    gone: Vec<NativeHandle>,
    neutralized: Vec<NativeHandle>,
}

pub struct FakeDesktop {
    hidden: NativeHandle,
    state: RefCell<DesktopState>,
}

impl FakeDesktop {
    /// A press anywhere on the screen, to every receiver of `pointer_presses`.
    pub fn press(&self, at: PhysPoint) {
        self.state.borrow_mut().presses.retain(|tx| tx.unbounded_send(at).is_ok());
    }

    /// One of our browser windows at `rect`, above every one placed before.
    pub fn place_toplevel(&self, window: NativeHandle, rect: PhysRect) {
        let mut state = self.state.borrow_mut();
        state.toplevels.retain(|(w, _)| *w != window);
        state.toplevels.push((window, rect));
    }

    /// The window no longer exists, so neutralizing it fails.
    pub fn destroy_window(&self, window: NativeHandle) {
        self.state.borrow_mut().gone.push(window);
    }

    pub fn calls(&self) -> Vec<DesktopCall> {
        self.state.borrow().calls.clone()
    }

    /// Windows now inside the hidden host.
    pub fn neutralized(&self) -> Vec<NativeHandle> {
        self.state.borrow().neutralized.clone()
    }

    fn log(&self, call: DesktopCall) {
        self.state.borrow_mut().calls.push(call);
    }
}

fn id(window: &gpui::Window) -> WindowId {
    window.window_handle().window_id()
}

impl Desktop for FakeDesktop {
    fn init() -> Result<FakeDesktop, PlatformError> {
        Ok(FakeDesktop { hidden: next_handle(), state: RefCell::default() })
    }

    fn adopt_popup(&self, popup: &gpui::Window, owner: &gpui::Window) {
        self.log(DesktopCall::AdoptPopup { popup: id(popup), owner: id(owner) });
    }

    fn set_dialog_parent(&self, dialog: &gpui::Window, parent: &gpui::Window) {
        self.log(DesktopCall::SetDialogParent { dialog: id(dialog), parent: id(parent) });
    }

    fn toplevel_at(&self, at: PhysPoint) -> Option<NativeHandle> {
        let state = self.state.borrow();
        let inside = |r: &PhysRect| at.x >= r.x && at.y >= r.y && at.x < r.x + r.width as i32 && at.y < r.y + r.height as i32;
        state.toplevels.iter().rev().find(|(_, r)| inside(r)).map(|&(w, _)| w)
    }

    fn pointer_presses(&self) -> UnboundedReceiver<PhysPoint> {
        let (tx, rx) = unbounded();
        self.state.borrow_mut().presses.push(tx);
        rx
    }

    fn raise_for_invocation(&self, window: &gpui::Window, token: Option<&str>) {
        self.log(DesktopCall::Raise { window: id(window), token: token.map(str::to_owned) });
    }

    fn hidden_host(&self) -> NativeHandle {
        self.hidden
    }

    fn neutralize(&self, window: NativeHandle) -> Result<(), PlatformError> {
        let ok = !self.state.borrow().gone.contains(&window);
        self.log(DesktopCall::Neutralize { window, ok });
        if !ok {
            return Err(PlatformError::new(format!("{window:?} does not exist")));
        }
        self.state.borrow_mut().neutralized.push(window);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt as _;
    use gpui::{Context, IntoElement, Render, TestAppContext, Window, WindowHandle};

    use super::*;

    struct Blank;

    impl Render for Blank {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            gpui::Empty
        }
    }

    fn window(cx: &mut TestAppContext) -> WindowHandle<Blank> {
        cx.add_window(|_, _| Blank)
    }

    /// The seam the shell holds: `fake::attach` as a `Box<dyn SurfaceOps>`.
    fn surface(cx: &mut TestAppContext) -> Box<dyn SurfaceOps> {
        window(cx).update(cx, |_, window, _| attach(window)).unwrap().unwrap()
    }

    fn fake(s: &dyn SurfaceOps) -> &FakeSurface {
        s.as_any().downcast_ref().unwrap()
    }

    fn fake_mut(s: &mut dyn SurfaceOps) -> &mut FakeSurface {
        s.as_any_mut().downcast_mut().unwrap()
    }

    const PAGE: NativeHandle = NativeHandle::from_raw(0x1001);
    const RECT: PhysRect = PhysRect { x: 0, y: 80, width: 800, height: 520 };

    #[gpui::test]
    fn pages_follow_the_content_rect_and_show_hide(cx: &mut TestAppContext) {
        let mut s = surface(cx);
        s.set_content_rect(RECT);
        s.show_page(PAGE, true);
        assert_eq!(fake(&*s).content_rect(), RECT);
        assert_eq!(fake(&*s).page(PAGE), Some(true));
        s.show_page(PAGE, false);
        assert_eq!(fake(&*s).page(PAGE), Some(false));
        let hole = Hole { rect: PhysRect { x: 10, y: 90, width: 100, height: 50 }, interactive: true };
        s.set_holes(&[hole]);
        assert_eq!(fake(&*s).holes(), &[hole]);
        s.set_holes(&[]);
        assert!(fake(&*s).holes().is_empty());
    }

    #[gpui::test]
    fn focus_is_refused_while_inactive_and_for_a_hidden_page(cx: &mut TestAppContext) {
        let mut s = surface(cx);
        assert_eq!(s.focus(FocusTarget::Page(PAGE)), FocusOutcome::Refused);
        s.show_page(PAGE, true);
        assert_eq!(s.focus(FocusTarget::Page(PAGE)), FocusOutcome::Done);
        assert_eq!(fake(&*s).focused(), FocusTarget::Page(PAGE));
        fake_mut(&mut *s).set_window_active(false);
        assert_eq!(s.focus(FocusTarget::Chrome), FocusOutcome::Refused);
        assert_eq!(fake(&*s).focused(), FocusTarget::Page(PAGE));
        fake_mut(&mut *s).set_window_active(true);
        assert_eq!(s.focus(FocusTarget::Chrome), FocusOutcome::Done);
    }

    #[gpui::test]
    fn a_moved_page_arrives_hidden_in_the_other_host(cx: &mut TestAppContext) {
        let (mut a, mut b) = (surface(cx), surface(cx));
        a.show_page(PAGE, true);
        a.move_page_to(PAGE, &mut *b).unwrap();
        assert_eq!(fake(&*a).page(PAGE), None);
        assert_eq!(fake(&*b).page(PAGE), Some(false));
        assert!(a.move_page_to(PAGE, &mut *b).is_err(), "the page is no longer in a");
    }

    #[gpui::test]
    fn a_parked_page_leaves_before_detach_and_the_journal_outlives_it(cx: &mut TestAppContext) {
        let desktop = FakeDesktop::init().unwrap();
        let mut s = surface(cx);
        let journal = fake(&*s).journal();
        s.show_page(PAGE, true);
        s.park_page(PAGE, desktop.hidden_host()).unwrap();
        assert_eq!(fake(&*s).page(PAGE), None);
        s.detach();
        let calls = journal.borrow();
        let park = calls.iter().position(|c| *c == SurfaceCall::ParkPage { page: PAGE, hidden_host: desktop.hidden_host() });
        assert!(park.is_some_and(|p| calls[p + 1..] == [SurfaceCall::Detach]), "{calls:?}");
    }

    #[gpui::test]
    fn aux_hosts_open_move_and_close(cx: &mut TestAppContext) {
        let mut s = surface(cx);
        let aux = s.open_aux(RECT).unwrap();
        assert_ne!(aux.native(), s.host());
        let moved = PhysRect { x: 20, ..RECT };
        s.set_aux_rect(&aux, moved);
        assert_eq!(fake(&*s).aux_rect(&aux), Some(moved));
        let native = aux.native();
        s.close_aux(aux);
        assert_eq!(fake(&*s).journal().borrow().last(), Some(&SurfaceCall::CloseAux(native)));
    }

    #[gpui::test]
    fn capture_covers_the_content_rect(cx: &mut TestAppContext) {
        let mut s = surface(cx);
        s.set_content_rect(RECT);
        let image = s.capture().unwrap();
        assert_eq!((image.width(), image.height()), (800, 600));
    }

    #[gpui::test]
    fn hosts_are_distinct_handles(cx: &mut TestAppContext) {
        let (a, b) = (surface(cx), surface(cx));
        let desktop = FakeDesktop::init().unwrap();
        assert_ne!(a.host(), b.host());
        assert_ne!(a.host(), desktop.hidden_host());
        assert_ne!(a.host(), PAGE);
    }

    #[gpui::test]
    fn the_desktop_records_window_calls(cx: &mut TestAppContext) {
        let desktop = FakeDesktop::init().unwrap();
        let (a, b) = (window(cx), window(cx));
        let (a_id, b_id) = (a.window_id(), b.window_id());
        a.update(cx, |_, wa, cx| {
            b.update(cx, |_, wb, _| {
                desktop.adopt_popup(wb, wa);
                desktop.set_dialog_parent(wb, wa);
                desktop.raise_for_invocation(wa, Some("id_TIME1"));
            })
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            desktop.calls(),
            [
                DesktopCall::AdoptPopup { popup: b_id, owner: a_id },
                DesktopCall::SetDialogParent { dialog: b_id, parent: a_id },
                DesktopCall::Raise { window: a_id, token: Some("id_TIME1".into()) },
            ]
        );
    }

    #[test]
    fn toplevel_at_finds_the_topmost_window_under_the_point() {
        let desktop = FakeDesktop::init().unwrap();
        let (low, high) = (NativeHandle::from_raw(1), NativeHandle::from_raw(2));
        desktop.place_toplevel(low, PhysRect { x: 0, y: 0, width: 100, height: 100 });
        desktop.place_toplevel(high, PhysRect { x: 50, y: 50, width: 100, height: 100 });
        assert_eq!(desktop.toplevel_at(PhysPoint { x: 10, y: 10 }), Some(low));
        assert_eq!(desktop.toplevel_at(PhysPoint { x: 60, y: 60 }), Some(high));
        assert_eq!(desktop.toplevel_at(PhysPoint { x: 200, y: 10 }), None);
    }

    #[test]
    fn every_receiver_sees_every_press() {
        let desktop = FakeDesktop::init().unwrap();
        let (mut a, mut b) = (desktop.pointer_presses(), desktop.pointer_presses());
        let at = PhysPoint { x: 3, y: 4 };
        desktop.press(at);
        b.close();
        desktop.press(at);
        assert_eq!(futures::executor::block_on(a.next()), Some(at));
        assert_eq!(futures::executor::block_on(a.next()), Some(at));
        assert_eq!(futures::executor::block_on(b.next()), Some(at));
    }

    #[test]
    fn neutralize_moves_a_window_into_the_hidden_host_and_fails_for_a_gone_one() {
        let desktop = FakeDesktop::init().unwrap();
        let (alive, gone) = (NativeHandle::from_raw(10), NativeHandle::from_raw(11));
        desktop.destroy_window(gone);
        assert!(desktop.neutralize(alive).is_ok());
        assert!(desktop.neutralize(gone).is_err());
        assert_eq!(desktop.neutralized(), [alive]);
    }
}
