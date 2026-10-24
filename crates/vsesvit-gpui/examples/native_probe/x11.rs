//! Owned by W1.1.
//! The X11 checks of the platform probe (design section 7.1): GPUI windows with `platform::Surface`
//! and `platform::Os`, and a plain x11rb window on a second connection standing in for Chromium's
//! page. The probe's connection is also the observer: it reads geometry, stacking, SHAPE, focus and
//! parents straight from the server. `--scale N` runs it at another GPUI scale.

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AppContext as _, AsyncApp, Bounds, Context, FocusHandle, IntoElement, KeyDownEvent, ParentElement as _, Render,
    Styled as _, Window, WindowBounds, WindowHandle, WindowKind, WindowOptions, canvas, div, point, px, rgb, size,
    InteractiveElement as _,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use vsesvit_gpui::platform::x11::scale::GPUI_SCALE_VAR;
use vsesvit_gpui::platform::{Desktop as _, FocusOutcome, FocusTarget, Hole, NativeHandle, Os, PhysPoint, PhysRect, SurfaceOps};
use vsesvit_testenv::TestEnv;
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

const BAR: u32 = 0x33_55_aa;
const CONTENT: u32 = 0xaa_33_99;
const PAGE: u32 = 0x00_c0_00;
const FOREIGN: u32 = 0x80_80_80;
const TIMEOUT: Duration = Duration::from_secs(5);
const XK_A: u32 = 0x61;

pub fn run(env: TestEnv) -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(scale) = args.iter().position(|a| a == "--scale").and_then(|i| args.get(i + 1)) {
        // SAFETY: no other thread exists yet; GPUI reads the variable when it starts.
        unsafe { std::env::set_var(GPUI_SCALE_VAR, scale) };
    }
    println!("native_probe: the guard passed in {}", env.describe());
    gpui_platform::application().run(move |cx| {
        cx.spawn(async move |cx| {
            let failed = probe(cx, &env).await;
            println!("native_probe: {}", if failed == 0 { "all checks passed".to_owned() } else { format!("{failed} checks failed") });
            std::process::exit(i32::from(failed != 0));
        })
        .detach();
    });
    ExitCode::FAILURE
}

type Slot = Rc<RefCell<Option<Box<dyn SurfaceOps>>>>;

struct ProbeView {
    surface: Slot,
    bar: f32,
    keys: Rc<RefCell<Vec<String>>>,
    focus: FocusHandle,
}

impl Render for ProbeView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let surface = self.surface.clone();
        let keys = self.keys.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(CONTENT))
            .track_focus(&self.focus)
            .on_key_down(move |e: &KeyDownEvent, _, _| keys.borrow_mut().push(e.keystroke.key.clone()))
            .child(div().w_full().h(px(self.bar)).flex_none().bg(rgb(BAR)))
            .child(
                div().w_full().flex_1().child(
                    canvas(
                        move |bounds, window, _| {
                            let s = window.scale_factor();
                            let p = |v: gpui::Pixels| (f32::from(v) * s).round();
                            let rect = PhysRect {
                                x: p(bounds.origin.x) as i32,
                                y: p(bounds.origin.y) as i32,
                                width: p(bounds.size.width) as u32,
                                height: p(bounds.size.height) as u32,
                            };
                            if let Some(surface) = surface.borrow_mut().as_mut() {
                                surface.set_content_rect(rect);
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .size_full(),
                ),
            )
    }
}

/// A GPUI window under test.
struct Win {
    handle: WindowHandle<ProbeView>,
    xid: u32,
    surface: Slot,
    keys: Rc<RefCell<Vec<String>>>,
}

impl Win {
    fn with<R>(&self, cx: &mut AsyncApp, f: impl FnOnce(&mut dyn SurfaceOps) -> R) -> R {
        let mut slot = self.surface.borrow_mut();
        let r = f(slot.as_mut().expect("attached").as_mut());
        drop(slot);
        // Let GPUI see whatever the call changed before the next check reads the server.
        let _ = self.handle.update(cx, |_, window, _| window.refresh());
        r
    }

    fn update<R>(&self, cx: &mut AsyncApp, f: impl FnOnce(&mut ProbeView, &mut Window, &mut Context<ProbeView>) -> R) -> Result<R, String> {
        self.handle.update(cx, |view, window, cx| f(view, window, cx)).map_err(|e| e.to_string())
    }
}

fn xid_of(window: &Window) -> u32 {
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Xcb(h)) => h.window.get(),
        other => panic!("not an X11 window: {other:?}"),
    }
}

fn options(origin: (f32, f32), extent: (f32, f32), kind: WindowKind) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin: point(px(origin.0), px(origin.1)), size: size(px(extent.0), px(extent.1)) })),
        focus: kind != WindowKind::PopUp,
        kind,
        inactive_frame_interval: None,
        ..Default::default()
    }
}

fn open(cx: &mut AsyncApp, opts: WindowOptions, attach: bool) -> Result<Win, String> {
    let surface: Slot = Rc::default();
    let keys: Rc<RefCell<Vec<String>>> = Rc::default();
    let (slot, log) = (surface.clone(), keys.clone());
    let handle = cx
        .open_window(opts, move |window, cx| {
            if attach {
                *slot.borrow_mut() = Some(vsesvit_gpui::platform::attach(window).expect("attach"));
            }
            let view = cx.new(|cx| ProbeView { surface: slot, bar: 40.0, keys: log, focus: cx.focus_handle() });
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
            view
        })
        .map_err(|e| e.to_string())?;
    let xid = handle.update(cx, |_, window, _| xid_of(window)).map_err(|e| e.to_string())?;
    Ok(Win { handle, xid, surface, keys })
}

/// The probe's own connection: Chromium's stand-in and the observer.
struct Foreign {
    conn: RustConnection,
    root: u32,
    created: RefCell<Vec<u32>>,
}

impl Foreign {
    fn connect() -> Result<Foreign, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let root = conn.setup().roots[screen].root;
        Ok(Foreign { conn, root, created: RefCell::default() })
    }

    fn window(&self, parent: u32, rect: PhysRect, color: u32, mapped: bool) -> Result<u32, String> {
        let id = self.conn.generate_id().map_err(|e| e.to_string())?;
        let events = xproto::EventMask::KEY_PRESS | xproto::EventMask::FOCUS_CHANGE;
        let aux = xproto::CreateWindowAux::new().background_pixel(color).event_mask(events);
        self.conn
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                id,
                parent,
                rect.x as i16,
                rect.y as i16,
                rect.width as u16,
                rect.height as u16,
                0,
                xproto::WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &aux,
            )
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| format!("CreateWindow: {e}"))?;
        if mapped {
            self.conn.map_window(id).map_err(|e| e.to_string())?;
        }
        self.conn.flush().map_err(|e| e.to_string())?;
        self.created.borrow_mut().push(id);
        Ok(id)
    }

    fn destroy(&self, id: u32) {
        let _ = self.conn.destroy_window(id);
        let _ = self.conn.flush();
    }

    fn geometry(&self, id: u32) -> Option<PhysRect> {
        let g = self.conn.get_geometry(id).ok()?.reply().ok()?;
        Some(PhysRect { x: g.x.into(), y: g.y.into(), width: g.width.into(), height: g.height.into() })
    }

    fn attrs(&self, id: u32) -> Option<xproto::GetWindowAttributesReply> {
        self.conn.get_window_attributes(id).ok()?.reply().ok()
    }

    fn viewable(&self, id: u32) -> bool {
        self.attrs(id).is_some_and(|a| a.map_state == xproto::MapState::VIEWABLE)
    }

    fn tree(&self, id: u32) -> Option<xproto::QueryTreeReply> {
        self.conn.query_tree(id).ok()?.reply().ok()
    }

    fn parent(&self, id: u32) -> Option<u32> {
        self.tree(id).map(|t| t.parent)
    }

    fn children(&self, id: u32) -> Vec<u32> {
        self.tree(id).map(|t| t.children).unwrap_or_default()
    }

    /// The child of the root that holds `id` (a WM frame, or `id` itself).
    fn top_ancestor(&self, mut id: u32) -> u32 {
        while let Some(parent) = self.parent(id) {
            if parent == self.root || parent == 0 {
                break;
            }
            id = parent;
        }
        id
    }

    fn focus(&self) -> u32 {
        self.conn.get_input_focus().ok().and_then(|c| c.reply().ok()).map_or(0, |r| r.focus)
    }

    fn set_focus(&self, id: u32) -> Result<(), String> {
        self.conn
            .set_input_focus(xproto::InputFocus::PARENT, id, x11rb::CURRENT_TIME)
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())
    }

    fn to_root(&self, id: u32, x: i32, y: i32) -> Option<PhysPoint> {
        let r = self.conn.translate_coordinates(id, self.root, x as i16, y as i16).ok()?.reply().ok()?;
        Some(PhysPoint { x: r.dst_x.into(), y: r.dst_y.into() })
    }

    /// The deepest window that would get a press at `at`, by the server's own rules (input SHAPE
    /// included): `TranslateCoordinates` one level at a time from the root.
    fn hit(&self, at: PhysPoint) -> u32 {
        let mut w = self.root;
        loop {
            let Some(r) = self.conn.translate_coordinates(self.root, w, at.x as i16, at.y as i16).ok().and_then(|c| c.reply().ok()) else {
                return w;
            };
            if r.child == x11rb::NONE {
                return w;
            }
            w = r.child;
        }
    }

    fn atom(&self, name: &str) -> u32 {
        self.conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map_or(0, |r| r.atom)
    }

    fn window_property(&self, id: u32, name: &str) -> Option<u32> {
        let atom = self.atom(name);
        let reply = self.conn.get_property(false, id, atom, xproto::AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
        reply.value32()?.next()
    }

    fn keycode(&self, keysym: u32) -> Result<u8, String> {
        let setup = self.conn.setup();
        let count = setup.max_keycode - setup.min_keycode + 1;
        let map = self.conn.get_keyboard_mapping(setup.min_keycode, count).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let per = usize::from(map.keysyms_per_keycode.max(1));
        let index = map.keysyms.chunks(per).position(|syms| syms.contains(&keysym)).ok_or("no keycode for the keysym")?;
        Ok(setup.min_keycode + u8::try_from(index).map_err(|e| e.to_string())?)
    }

    /// Key presses this connection received for `window` since the last call.
    fn keys_for(&self, window: u32) -> usize {
        let mut n = 0;
        while let Ok(Some(event)) = self.conn.poll_for_event() {
            if matches!(event, Event::KeyPress(k) if k.event == window) {
                n += 1;
            }
        }
        n
    }

    fn shaped(&self, id: u32) -> bool {
        self.conn.shape_query_extents(id).ok().and_then(|c| c.reply().ok()).is_some_and(|e| e.bounding_shaped)
    }

    fn input_rects(&self, id: u32) -> usize {
        self.conn
            .shape_get_rectangles(id, shape::SK::INPUT)
            .ok()
            .and_then(|c| c.reply().ok())
            .map_or(0, |r| r.rectangles.len())
    }
}

impl Drop for Foreign {
    fn drop(&mut self) {
        for &id in self.created.borrow().iter() {
            let _ = self.conn.destroy_window(id);
        }
        let _ = self.conn.flush();
    }
}

async fn sleep(cx: &AsyncApp, d: Duration) {
    cx.background_executor().timer(d).await;
}

async fn until(cx: &mut AsyncApp, what: &str, mut ok: impl FnMut(&mut AsyncApp) -> bool) -> Result<(), String> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if ok(cx) {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("timed out waiting for {what}"));
        }
        sleep(cx, Duration::from_millis(20)).await;
    }
}

fn ensure(ok: bool, what: impl FnOnce() -> String) -> Result<(), String> {
    if ok { Ok(()) } else { Err(what()) }
}

fn near(a: [u8; 4], rgb: u32) -> bool {
    let want = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
    a[..3].iter().zip(want).all(|(&got, want)| got.abs_diff(want) <= 12)
}

struct Report {
    failed: u32,
}

impl Report {
    fn record(&mut self, name: &str, result: Result<(), String>) {
        match result {
            Ok(()) => println!("native_probe: ok {name}"),
            Err(e) => {
                self.failed += 1;
                println!("native_probe: FAIL {name}: {e}");
            }
        }
    }
}

/// What every check works on.
struct Rig {
    x: Foreign,
    desktop: Os,
    a: Win,
    page: u32,
    proxy: u32,
    host: u32,
    scale: f32,
}

async fn probe(cx: &mut AsyncApp, env: &TestEnv) -> u32 {
    let mut report = Report { failed: 0 };
    let rig = match setup(cx).await {
        Ok(rig) => rig,
        Err(e) => {
            report.record("attach", Err(e));
            return report.failed;
        }
    };
    report.record("attach", attach(&rig));
    report.record("geometry", geometry(cx, &rig).await);
    report.record("show_hide", show_hide(cx, &rig).await);
    report.record("holes", holes(cx, &rig).await);
    report.record("capture", capture(cx, &rig).await);
    report.record("window_manager", window_manager(cx, &rig).await);
    report.record("focus", focus(cx, &rig, env).await);
    report.record("focus_refused", focus_refused(cx, &rig).await);
    report.record("aux", aux(cx, &rig).await);
    // The WM places normal windows itself; a PopUp goes exactly where it is asked, here over the window.
    let origin = rig.x.to_root(rig.a.xid, 0, 0).unwrap_or(PhysPoint { x: 0, y: 0 });
    let over_a = (origin.x as f32 / rig.scale + 100.0, origin.y as f32 / rig.scale + 100.0);
    let popup = open(cx, options(over_a, (160.0, 120.0), WindowKind::PopUp), false);
    let dialog = open(cx, options((760.0, 60.0), (300.0, 200.0), WindowKind::Normal), false);
    match (popup, dialog) {
        (Ok(popup), Ok(dialog)) => {
            report.record("popup", popup_check(cx, &rig, &popup).await);
            report.record("dialog_parent", dialog_parent(cx, &rig, &dialog).await);
            report.record("toplevel_at", toplevel_at(cx, &rig, &popup, &dialog).await);
            report.record("raise", raise(cx, &rig, &dialog).await);
            let _ = popup.update(cx, |_, window, _| window.remove_window());
            let _ = dialog.update(cx, |_, window, _| window.remove_window());
        }
        (popup, dialog) => report.record("popup", Err(format!("opening windows: {:?} {:?}", popup.err(), dialog.err()))),
    }
    report.record("move_and_park", move_and_park(cx, &rig).await);
    report.record("neutralize", neutralize(cx, &rig).await);
    println!("native_probe: skipped pointer_presses: PrivateInput sends no button presses, so no raw XI2 press can be made here");
    report.failed
}

async fn setup(cx: &mut AsyncApp) -> Result<Rig, String> {
    let x = Foreign::connect()?;
    let desktop = cx.update(|_| <Os as vsesvit_gpui::platform::Desktop>::init()).map_err(|e| e.to_string())?;
    let a = open(cx, options((40.0, 40.0), (700.0, 450.0), WindowKind::Normal), true)?;
    until(cx, "the window to map", |_| x.viewable(a.xid)).await?;
    let host = a.with(cx, |s| s.host()).xid();
    until(cx, "the first layout", |_| x.geometry(host).is_some_and(|g| g.width > 1)).await?;
    let page = x.window(host, PhysRect { x: 0, y: 0, width: 10, height: 10 }, PAGE, false)?;
    a.with(cx, |s| s.show_page(NativeHandle::from_xid(page), true));
    until(cx, "the page to show", |_| x.viewable(page)).await?;
    let proxy = x
        .children(a.xid)
        .into_iter()
        .find(|&c| {
            x.conn
                .get_property(false, c, xproto::AtomEnum::WM_NAME, xproto::AtomEnum::STRING, 0, 8)
                .ok()
                .and_then(|r| r.reply().ok())
                .is_some_and(|p| p.value == b"gpui-focus-proxy")
        })
        .ok_or("no focus proxy")?;
    let scale = a.update(cx, |_, window, _| window.scale_factor())?;
    println!("native_probe: window 0x{:x}, host 0x{host:x}, page 0x{page:x}, proxy 0x{proxy:x}, scale {scale}", a.xid);
    Ok(Rig { x, desktop, a, page, proxy, host, scale })
}

fn attach(rig: &Rig) -> Result<(), String> {
    let x = &rig.x;
    let screen = &x.conn.setup().roots[0];
    ensure(x.parent(rig.host) == Some(rig.a.xid), || "the host is not a child of the GPUI window".into())?;
    let g = x.conn.get_geometry(rig.host).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
    ensure(g.depth == screen.root_depth && g.border_width == 0, || format!("host depth {} border {}", g.depth, g.border_width))?;
    let attrs = x.attrs(rig.host).ok_or("no host attributes")?;
    ensure(attrs.visual == screen.root_visual, || "the host does not use the root visual".into())?;
    ensure(attrs.all_event_masks == xproto::EventMask::NO_EVENT, || format!("the host selects events {:?}", attrs.all_event_masks))?;
    let top = x.conn.get_geometry(rig.a.xid).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
    ensure(top.depth == 32, || format!("GPUI's window has depth {}, not 32", top.depth))?;
    ensure(x.parent(rig.page) == Some(rig.host), || "the page is not in the host".into())
}

/// Where the host must be for the current layout: the content element's rectangle in physical pixels.
fn expected(cx: &mut AsyncApp, rig: &Rig) -> Result<PhysRect, String> {
    rig.a.update(cx, |view, window, _| {
        let s = window.scale_factor();
        let vp = window.viewport_size();
        let (w, h) = (f32::from(vp.width), f32::from(vp.height));
        let y = (view.bar * s).round();
        PhysRect { x: 0, y: y as i32, width: (w * s).round() as u32, height: ((h - view.bar) * s).round() as u32 }
    })
}

async fn layout_matches(cx: &mut AsyncApp, rig: &Rig) -> Result<(), String> {
    let want = expected(cx, rig)?;
    let page = PhysRect { x: 0, y: 0, width: want.width, height: want.height };
    let mut last = (None, None);
    let result = until(cx, "the host and page to follow the layout", |_| {
        last = (rig.x.geometry(rig.host), rig.x.geometry(rig.page));
        last.0 == Some(want) && last.1 == Some(page)
    })
    .await;
    result.map_err(|e| format!("{e}: want host {want:?} page {page:?}, have {:?} {:?}", last.0, last.1))
}

async fn geometry(cx: &mut AsyncApp, rig: &Rig) -> Result<(), String> {
    layout_matches(cx, rig).await?;
    rig.a.update(cx, |view, _, cx| {
        view.bar = 96.0;
        cx.notify();
    })?;
    layout_matches(cx, rig).await?;
    rig.a.update(cx, |view, _, cx| {
        view.bar = 40.0;
        cx.notify();
    })?;
    layout_matches(cx, rig).await
}

async fn show_hide(cx: &mut AsyncApp, rig: &Rig) -> Result<(), String> {
    let page = NativeHandle::from_xid(rig.page);
    rig.a.with(cx, |s| s.show_page(page, false));
    until(cx, "the page to unmap", |_| rig.x.attrs(rig.page).is_some_and(|a| a.map_state == xproto::MapState::UNMAPPED)).await?;
    rig.a.with(cx, |s| s.show_page(page, true));
    until(cx, "the page to map", |_| rig.x.viewable(rig.page)).await?;
    layout_matches(cx, rig).await
}

/// Window-relative physical rectangles: an interactive hole and a paint-only one, both in the page.
fn hole_rects(rig: &Rig) -> [Hole; 2] {
    let s = rig.scale;
    let r = |x: f32, y: f32, w: f32, h: f32| PhysRect { x: (x * s) as i32, y: (y * s) as i32, width: (w * s) as u32, height: (h * s) as u32 };
    [Hole { rect: r(100.0, 120.0, 200.0, 120.0), interactive: true }, Hole { rect: r(500.0, 300.0, 100.0, 80.0), interactive: false }]
}

fn center(r: PhysRect) -> (i32, i32) {
    (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2)
}

async fn holes(cx: &mut AsyncApp, rig: &Rig) -> Result<(), String> {
    let x = &rig.x;
    let holes = hole_rects(rig);
    let screen = |(px, py): (i32, i32)| x.to_root(rig.a.xid, px, py).ok_or("TranslateCoordinates failed");
    let content = expected(cx, rig)?;
    let in_page = screen((content.x + 20, content.y + 20))?;
    ensure(x.hit(in_page) == rig.page, || "a press in the page does not reach the page".into())?;
    rig.a.with(cx, |s| s.set_holes(&holes));
    ensure(x.shaped(rig.host) && x.input_rects(rig.host) > 0, || "the host has no SHAPE".into())?;
    for hole in holes {
        let at = screen(center(hole.rect))?;
        let hit = x.hit(at);
        ensure(hit == rig.a.xid, || format!("a press in the {hole:?} hole reaches 0x{hit:x}, not GPUI's window 0x{:x}", rig.a.xid))?;
    }
    ensure(x.hit(in_page) == rig.page, || "a press outside the holes no longer reaches the page".into())?;
    rig.a.with(cx, |s| s.set_holes(&[]));
    let at = screen(center(holes[0].rect))?;
    ensure(x.hit(at) == rig.page && !x.shaped(rig.host), || "clearing the holes left a SHAPE".into())
}

async fn capture(cx: &mut AsyncApp, rig: &Rig) -> Result<(), String> {
    let hole = hole_rects(rig)[0];
    rig.a.with(cx, |s| s.set_holes(&[hole]));
    rig.a.update(cx, |_, _, cx| cx.notify())?;
    sleep(cx, Duration::from_millis(300)).await;
    let started = Instant::now();
    let image = rig.a.with(cx, |s| s.capture()).map_err(|e| e.to_string())?;
    let took = started.elapsed();
    rig.a.with(cx, |s| s.set_holes(&[]));
    let content = expected(cx, rig)?;
    let px = |(x, y): (i32, i32)| image.get_pixel_checked(x as u32, y as u32).map(|p| p.0).unwrap_or_default();
    let page = px((content.x + content.width as i32 - 40, content.y + content.height as i32 - 40));
    let bar = px((40, (20.0 * rig.scale) as i32));
    let in_hole = px(center(hole.rect));
    println!("native_probe: capture {}x{} in {took:?}: page {page:?} bar {bar:?} hole {in_hole:?}", image.width(), image.height());
    ensure(near(page, PAGE), || format!("the page pixel is {page:?}, not #{PAGE:06x}"))?;
    ensure(near(bar, BAR), || format!("GPUI's bar pixel is {bar:?}, not #{BAR:06x}"))?;
    ensure(near(in_hole, CONTENT), || format!("the hole shows {in_hole:?}, not GPUI's #{CONTENT:06x}"))
}

/// Whether a WM runs (the launcher starts openbox, design 7.3) and has made our window active.
async fn window_manager(cx: &mut AsyncApp, rig: &Rig) -> Result<(), String> {
    let x = &rig.x;
    let wm = x.window_property(x.root, "_NET_SUPPORTING_WM_CHECK");
    println!("native_probe: window manager check window {wm:?}, X focus 0x{:x}", x.focus());
    ensure(wm.is_some(), || "no window manager runs on this display".into())?;
    until(cx, "GPUI's window to become active", |cx| rig.a.update(cx, |_, w, _| w.is_window_active()).unwrap_or(false)).await
}

async fn focus(cx: &mut AsyncApp, rig: &Rig, env: &TestEnv) -> Result<(), String> {
    let x = &rig.x;
    let input = env.input()?;
    let key = x.keycode(XK_A)?;
    x.keys_for(rig.page);
    let outcome = rig.a.with(cx, |s| s.focus(FocusTarget::Page(NativeHandle::from_xid(rig.page))));
    ensure(outcome == FocusOutcome::Done, || format!("focus(Page) was {outcome:?}"))?;
    ensure(x.focus() == rig.page, || format!("X focus is 0x{:x}, not the page", x.focus()))?;
    input.key(key)?;
    until(cx, "the key to reach the page", |_| x.keys_for(rig.page) > 0).await?;
    sleep(cx, Duration::from_millis(100)).await;
    let active = rig.a.update(cx, |_, w, _| w.is_window_active())?;
    ensure(active, || "GPUI went inactive when the page took focus (the focus patch's Inferior rule)".into())?;

    rig.a.keys.borrow_mut().clear();
    // From inside a GPUI update, where the shell's focus code runs (design 7.1).
    let slot = rig.a.surface.clone();
    let outcome = rig.a.update(cx, move |_, _, _| slot.borrow_mut().as_mut().expect("attached").focus(FocusTarget::Chrome))?;
    ensure(outcome == FocusOutcome::Done, || format!("focus(Chrome) was {outcome:?}"))?;
    ensure(x.focus() == rig.proxy, || format!("X focus is 0x{:x}, not the focus proxy", x.focus()))?;
    input.key(key)?;
    until(cx, "the key to reach GPUI", |_| rig.a.keys.borrow().iter().any(|k| k == "a")).await?;
    ensure(x.keys_for(rig.page) == 0, || "the page also got the chrome's key".into())?;
    let active = rig.a.update(cx, |_, w, _| w.is_window_active())?;
    ensure(active, || "GPUI is inactive with the keyboard on its proxy".into())
}

async fn focus_refused(cx: &mut AsyncApp, rig: &Rig) -> Result<(), String> {
    let x = &rig.x;
    let other = x.window(x.root, PhysRect { x: 1100, y: 600, width: 300, height: 200 }, FOREIGN, true)?;
    until(cx, "the other window to map", |_| x.viewable(other)).await?;
    x.set_focus(other)?;
    let outcome = rig.a.with(cx, |s| s.focus(FocusTarget::Chrome));
    let still = x.focus();
    x.destroy(other);
    ensure(outcome == FocusOutcome::Refused, || format!("focus(Chrome) with another toplevel focused was {outcome:?}"))?;
    ensure(still == other, || format!("X focus moved to 0x{still:x}"))
}

async fn aux(cx: &mut AsyncApp, rig: &Rig) -> Result<(), String> {
    let x = &rig.x;
    let rect = PhysRect { x: 200, y: 150, width: 240, height: 180 };
    let aux = rig.a.with(cx, |s| s.open_aux(rect)).map_err(|e| e.to_string())?;
    let id = aux.native().xid();
    ensure(x.parent(id) == Some(rig.a.xid), || "the aux host is not in the GPUI window".into())?;
    ensure(x.geometry(id) == Some(rect), || format!("aux geometry {:?}", x.geometry(id)))?;
    let order = x.children(rig.a.xid);
    let (host_at, aux_at) = (order.iter().position(|&c| c == rig.host), order.iter().position(|&c| c == id));
    ensure(aux_at > host_at, || "the aux host is below the content host".into())?;
    let moved = PhysRect { x: 260, ..rect };
    rig.a.with(cx, |s| s.set_aux_rect(&aux, moved));
    until(cx, "set_aux_rect to move it", |_| x.geometry(id) == Some(moved)).await?;
    rig.a.with(cx, |s| s.close_aux(aux));
    until(cx, "close_aux to destroy it", |_| x.geometry(id).is_none()).await
}

/// The root's children over `at`, top first: what `toplevel_at` walks, for a failure message.
fn stack_at(x: &Foreign, at: PhysPoint) -> String {
    let mut out = Vec::new();
    for top in x.children(x.root).into_iter().rev() {
        let (Some(a), Some(g)) = (x.attrs(top), x.geometry(top)) else { continue };
        let inside = at.x >= g.x && at.y >= g.y && at.x < g.x + g.width as i32 && at.y < g.y + g.height as i32;
        if a.map_state == xproto::MapState::VIEWABLE && inside {
            out.push(format!("0x{top:x} override={} children={:x?}", a.override_redirect, x.children(top)));
        }
    }
    out.join("; ")
}

async fn popup_check(cx: &mut AsyncApp, rig: &Rig, popup: &Win) -> Result<(), String> {
    let x = &rig.x;
    until(cx, "the popup to map", |_| x.viewable(popup.xid)).await?;
    let (p, a) = (popup.xid, rig.a.xid);
    cx.update(|cx| {
        popup.handle.update(cx, |_, pw, cx| rig.a.handle.update(cx, |_, aw, _| rig.desktop.adopt_popup(pw, aw)))
    })
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let attrs = x.attrs(p).ok_or("no popup attributes")?;
    ensure(attrs.override_redirect, || "the PopUp window is not override-redirect".into())?;
    ensure(x.parent(p) == Some(x.root), || "the PopUp window is not a root child".into())?;
    let order = x.children(x.root);
    let (pa, aa) = (order.iter().position(|&c| c == p), order.iter().position(|&c| c == x.top_ancestor(a)));
    ensure(pa > aa, || format!("the PopUp window stacks below the browser window ({pa:?} <= {aa:?})"))
}

async fn dialog_parent(cx: &mut AsyncApp, rig: &Rig, dialog: &Win) -> Result<(), String> {
    until(cx, "the dialog to map", |_| rig.x.viewable(dialog.xid)).await?;
    cx.update(|cx| {
        dialog.handle.update(cx, |_, dw, cx| rig.a.handle.update(cx, |_, aw, _| rig.desktop.set_dialog_parent(dw, aw)))
    })
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let parent = rig.x.window_property(dialog.xid, "WM_TRANSIENT_FOR");
    ensure(parent == Some(rig.a.xid), || format!("WM_TRANSIENT_FOR is {parent:?}"))
}

async fn toplevel_at(cx: &mut AsyncApp, rig: &Rig, popup: &Win, dialog: &Win) -> Result<(), String> {
    let x = &rig.x;
    let content = expected(cx, rig)?;
    let in_a = x.to_root(rig.a.xid, content.x + content.width as i32 * 3 / 4, content.y + content.height as i32 * 3 / 4).ok_or("no point in the window")?;
    let in_popup = x.to_root(popup.xid, 20, 20).ok_or("no point in the popup")?;
    let in_dialog = x.to_root(dialog.xid, 40, 40).ok_or("no point in the dialog")?;
    let foreign = x.window(x.root, PhysRect { x: 1300, y: 700, width: 200, height: 200 }, FOREIGN, true)?;
    until(cx, "the foreign window to map", |_| x.viewable(foreign)).await?;
    let in_foreign = x.to_root(foreign, 50, 50).ok_or("no point in the foreign window")?;
    let at = |p: PhysPoint| rig.desktop.toplevel_at(p).map(|h| h.xid());
    let a = rig.a.xid;
    let found = [at(in_a), at(in_popup), at(in_dialog), at(in_foreign), at(PhysPoint { x: 1595, y: 995 })];
    let stack = stack_at(x, in_popup);
    x.destroy(foreign);
    let want = [Some(a), Some(a), Some(dialog.xid), None, None];
    ensure(found == want, || format!("window, popup over it, dialog, foreign, empty: {found:x?}, want {want:x?}; over the popup point {in_popup:?}: {stack}"))
}

/// A forwarded launch raises an older window over a newer one (`_NET_ACTIVE_WINDOW`).
async fn raise(cx: &mut AsyncApp, rig: &Rig, dialog: &Win) -> Result<(), String> {
    let x = &rig.x;
    let active = || x.window_property(x.root, "_NET_ACTIVE_WINDOW");
    until(cx, "the dialog to become active", |_| active() == Some(dialog.xid)).await?;
    cx.update(|cx| rig.a.handle.update(cx, |_, w, _| rig.desktop.raise_for_invocation(w, Some("native_probe_TIME0")))).map_err(|e| e.to_string())?;
    until(cx, "the raised window to become active", |_| active() == Some(rig.a.xid)).await
}

async fn move_and_park(cx: &mut AsyncApp, rig: &Rig) -> Result<(), String> {
    let x = &rig.x;
    let page = NativeHandle::from_xid(rig.page);
    let b = open(cx, options((500.0, 300.0), (700.0, 500.0), WindowKind::Normal), true)?;
    until(cx, "the second window to map", |_| x.viewable(b.xid)).await?;
    let b_host = b.with(cx, |s| s.host()).xid();
    until(cx, "the second window's layout", |_| x.geometry(b_host).is_some_and(|g| g.width > 1)).await?;
    {
        let mut a_slot = rig.a.surface.borrow_mut();
        let mut b_slot = b.surface.borrow_mut();
        let (a_s, b_s) = (a_slot.as_mut().unwrap(), b_slot.as_mut().unwrap());
        a_s.move_page_to(page, b_s.as_mut()).map_err(|e| e.to_string())?;
    }
    ensure(x.parent(rig.page) == Some(b_host), || "the moved page is not in the second host".into())?;
    ensure(!x.viewable(rig.page), || "the moved page arrived shown".into())?;
    b.with(cx, |s| s.show_page(page, true));
    let b_size = x.geometry(b_host).map(|g| (g.width, g.height));
    until(cx, "the moved page to show at the new size", |_| x.viewable(rig.page) && x.geometry(rig.page).map(|g| (g.width, g.height)) == b_size).await?;

    let hidden = rig.desktop.hidden_host();
    b.with(cx, |s| s.park_page(page, hidden)).map_err(|e| format!("park_page: {e}"))?;
    ensure(x.parent(rig.page) == Some(hidden.xid()), || "the parked page is not in the hidden host".into())?;
    let surface = b.surface.borrow_mut().take().unwrap();
    surface.detach();
    b.update(cx, |_, window, _| window.remove_window())?;
    until(cx, "the second window to go", |_| x.attrs(b.xid).is_none()).await?;
    ensure(x.attrs(b_host).is_none(), || "detach left the second host".into())?;
    let attrs = x.attrs(rig.page).ok_or("the parked page died with its window")?;
    ensure(attrs.map_state == xproto::MapState::UNMAPPED, || "the parked page is mapped".into())?;
    ensure(x.parent(rig.page) == Some(hidden.xid()), || "the parked page left the hidden host".into())
}

async fn neutralize(cx: &mut AsyncApp, rig: &Rig) -> Result<(), String> {
    let x = &rig.x;
    let hidden = rig.desktop.hidden_host().xid();
    let foreign = x.window(x.root, PhysRect { x: 900, y: 500, width: 320, height: 240 }, FOREIGN, true)?;
    until(cx, "the Chrome-made window to map", |_| x.viewable(foreign)).await?;
    // Let the WM frame it, as Chromium's windows are by the time `on_after_created` runs.
    sleep(cx, Duration::from_millis(300)).await;
    let framed = x.top_ancestor(foreign) != foreign;
    rig.desktop.neutralize(NativeHandle::from_xid(foreign)).map_err(|e| format!("neutralize: {e}"))?;
    sleep(cx, Duration::from_millis(300)).await;
    ensure(x.parent(foreign) == Some(hidden), || format!("the window is in 0x{:x?}, not the hidden host (framed: {framed})", x.parent(foreign)))?;
    ensure(!x.viewable(foreign), || "the neutralized window is viewable".into())?;
    let gone = x.window(x.root, PhysRect { x: 0, y: 0, width: 10, height: 10 }, FOREIGN, false)?;
    x.destroy(gone);
    let _ = x.conn.get_input_focus().map(|c| c.reply());
    ensure(rig.desktop.neutralize(NativeHandle::from_xid(gone)).is_err(), || "neutralizing a destroyed window succeeded".into())
}
