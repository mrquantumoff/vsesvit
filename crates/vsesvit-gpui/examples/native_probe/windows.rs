//! Owned by W1.2.
//! The Windows checks of the platform probe (design sections 7.1, 8.3 W1.2): GPUI windows whose content
//! host holds plain HWNDs standing in for CEF pages, driven through `WinSurface` and `WinDesktop` on the
//! launcher's private desktop. Input is only messages posted to this process's own windows.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, AsyncApp, Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Render, Styled as _, Window, WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowHandle,
    WindowKind, WindowOptions, canvas, div, point, px, rgb, size,
};
use vsesvit_gpui::platform::windows::{
    Desktop as _, FocusOutcome, FocusTarget, Hole, NativeHandle, NativeSurface as _, PhysPoint, PhysRect, WinDesktop,
    WinSurface, window_hwnd,
};
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, CreateSolidBrush, ScreenToClient};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GW_OWNER, GWL_EXSTYLE, GWL_STYLE,
    GetClientRect, GetParent, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, HTBOTTOM,
    HTCAPTION, HTCLOSE, HTLEFT, HTMAXBUTTON, HTMINBUTTON, HTTOP, IsWindow, IsWindowVisible, PostMessageW,
    RegisterClassExW, SW_SHOWNA, SendMessageW, ShowWindow, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST,
    LWA_ALPHA, SetLayeredWindowAttributes, WM_NCMOUSEMOVE, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED,
    WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WindowFromPoint,
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
}

const PAGE: u32 = 0x2080c0;
const TOOLBAR: u32 = 0xe05030;
const OVERLAY: u32 = 0x30c050;
const TITLEBAR: u32 = 0x303030;
const TITLEBAR_H: f32 = 32.;
const BUTTON_W: f32 = 46.;
const SETTLE: Duration = Duration::from_millis(400);

pub fn run(env: vsesvit_testenv::TestEnv) -> ExitCode {
    println!("native_probe: windows checks in {}", env.describe());
    let failed = Rc::new(Cell::new(None::<u32>));
    let result = failed.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        cx.spawn(async move |cx| {
            let mut report = Report::default();
            script(&mut report, cx, &env).await;
            println!("native_probe: {} passed, {} failed", report.passed, report.failed);
            result.set(Some(report.failed));
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    match failed.get() {
        Some(0) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

#[derive(Default)]
struct Report {
    passed: u32,
    failed: u32,
}

impl Report {
    fn check(&mut self, name: &str, result: Result<String, String>) {
        match result {
            Ok(detail) => {
                self.passed += 1;
                println!("native_probe: ok   {name}: {detail}");
            }
            Err(detail) => {
                self.failed += 1;
                println!("native_probe: FAIL {name}: {detail}");
            }
        }
    }
}

fn expect(ok: bool, detail: String) -> Result<String, String> {
    if ok { Ok(detail) } else { Err(detail) }
}

#[derive(Copy, Clone, PartialEq)]
enum Layout {
    /// Title bar, a toolbar, the content below.
    Normal { toolbar: f32 },
    /// The content fills the client area, up to its top edge.
    Full,
}

struct Probe {
    layout: Layout,
    /// In window pixels (logical).
    overlay: Option<Bounds<Pixels>>,
    surface: Rc<RefCell<Option<WinSurface>>>,
    content: Rc<Cell<PhysRect>>,
}

impl Render for Probe {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let (surface, content) = (self.surface.clone(), self.content.clone());
        let page = canvas(
            move |bounds, window, _| {
                let rect = phys(bounds, window.scale_factor());
                content.set(rect);
                if let Some(surface) = surface.borrow_mut().as_mut() {
                    surface.set_content_rect(rect);
                }
            },
            |_, _, _, _| {},
        )
        .flex_1()
        .w_full();
        let mut root = div().size_full().relative().flex().flex_col();
        if let Layout::Normal { toolbar } = self.layout {
            let button = |area, colour| div().w(px(BUTTON_W)).h_full().bg(rgb(colour)).window_control_area(area);
            root = root
                .child(
                    div()
                        .h(px(TITLEBAR_H))
                        .w_full()
                        .flex()
                        .bg(rgb(TITLEBAR))
                        .child(div().flex_1().h_full().window_control_area(WindowControlArea::Drag))
                        .child(button(WindowControlArea::Min, 0x505050))
                        .child(button(WindowControlArea::Max, 0x606060))
                        .child(button(WindowControlArea::Close, 0xc03030)),
                )
                .child(div().h(px(toolbar)).w_full().bg(rgb(TOOLBAR)));
        }
        root = root.child(page);
        if let Some(o) = self.overlay {
            root = root.child(div().absolute().left(o.origin.x).top(o.origin.y).w(o.size.width).h(o.size.height).bg(rgb(OVERLAY)));
        }
        root
    }
}

fn phys(bounds: Bounds<Pixels>, scale: f32) -> PhysRect {
    let x = (f32::from(bounds.origin.x) * scale).round() as i32;
    let y = (f32::from(bounds.origin.y) * scale).round() as i32;
    let right = (f32::from(bounds.origin.x + bounds.size.width) * scale).round() as i32;
    let bottom = (f32::from(bounds.origin.y + bounds.size.height) * scale).round() as i32;
    PhysRect { x, y, width: (right - x).max(0) as u32, height: (bottom - y).max(0) as u32 }
}

/// One probe window: its GPUI handle, its HWND and its surface.
struct Win {
    handle: WindowHandle<Probe>,
    hwnd: HWND,
    surface: Rc<RefCell<Option<WinSurface>>>,
    content: Rc<Cell<PhysRect>>,
    scale: f32,
}

impl Win {
    fn with<R>(&self, f: impl FnOnce(&mut WinSurface) -> R) -> R {
        f(self.surface.borrow_mut().as_mut().expect("attached"))
    }

    fn host(&self) -> HWND {
        self.with(|s| s.host()).hwnd()
    }

    fn update<R>(&self, cx: &mut AsyncApp, f: impl FnOnce(&mut Probe, &mut Window, &mut Context<Probe>) -> R) -> R {
        self.handle.update(cx, f).expect("probe window")
    }

    fn set_layout(&self, cx: &mut AsyncApp, layout: Layout) {
        self.update(cx, |probe, _, cx| {
            probe.layout = layout;
            cx.notify();
        });
    }

    /// Detach the surface, then close the window.
    fn close(self, cx: &mut AsyncApp) {
        if let Some(surface) = self.surface.borrow_mut().take() {
            surface.detach();
        }
        self.update(cx, |_, window, _| window.remove_window());
    }
}

fn options(bounds: (f32, f32, f32, f32), kind: WindowKind, focus: bool) -> WindowOptions {
    let (x, y, w, h) = bounds;
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin: point(px(x), px(y)), size: size(px(w), px(h)) })),
        titlebar: Some(gpui::TitlebarOptions { title: Some("native_probe".into()), appears_transparent: true, ..Default::default() }),
        focus,
        show: true,
        kind,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

fn open(cx: &mut AsyncApp, bounds: (f32, f32, f32, f32), focus: bool) -> Result<Win, String> {
    let surface = Rc::new(RefCell::new(None));
    let content = Rc::new(Cell::new(PhysRect::default()));
    let (s, c) = (surface.clone(), content.clone());
    let handle = cx
        .open_window(options(bounds, WindowKind::Normal, focus), move |window, cx| {
            match WinSurface::attach(window) {
                Ok(attached) => *s.borrow_mut() = Some(attached),
                Err(e) => println!("native_probe: attach failed: {e}"),
            }
            cx.new(|_| Probe { layout: Layout::Normal { toolbar: 40. }, overlay: None, surface: s, content: c })
        })
        .map_err(|e| e.to_string())?;
    if surface.borrow().is_none() {
        return Err("WinSurface::attach failed".into());
    }
    let (hwnd, scale) = handle
        .update(cx, |_, window, _| (window_hwnd(window).map_err(|e| e.to_string()), window.scale_factor()))
        .map_err(|e| e.to_string())?;
    Ok(Win { handle, hwnd: hwnd?, surface, content, scale })
}

async fn settle(cx: &mut AsyncApp) {
    cx.background_executor().timer(SETTLE).await;
}

// ---------------------------------------------------------------- plain windows standing in for CEF

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

thread_local!(static PAGE_KEYS: RefCell<Vec<(HWND, usize)>> = const { RefCell::new(Vec::new()) });

/// Records every key-down a page window receives, as Chromium's window would take it.
unsafe extern "system" fn page_procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_KEYDOWN {
        PAGE_KEYS.with_borrow_mut(|k| k.push((window, wparam)));
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn page_class() -> Vec<u16> {
    thread_local!(static DONE: Cell<bool> = const { Cell::new(false) });
    let class = wide("NativeProbePage");
    if !DONE.get() {
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(page_procedure),
            hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
            // COLORREF is 0x00BBGGRR.
            hbrBackground: unsafe { CreateSolidBrush(((PAGE & 0xff) << 16) | (PAGE & 0xff00) | (PAGE >> 16)) },
            lpszClassName: class.as_ptr(),
            ..unsafe { std::mem::zeroed() }
        };
        unsafe { RegisterClassExW(&wc) };
        DONE.set(true);
    }
    class
}

/// A hidden child of `parent`, as CEF creates a tab's browser window. Layered, so DWM composes it from
/// its own surface as it does Chromium's DirectX output: a plain GDI child of GPUI's
/// `WS_EX_NOREDIRECTIONBITMAP` window has no surface to paint into and never shows.
fn page(parent: HWND) -> HWND {
    let class = page_class();
    unsafe {
        let w = CreateWindowExW(WS_EX_LAYERED, class.as_ptr(), class.as_ptr(), WS_CHILD | WS_CLIPSIBLINGS, 0, 0, 10, 10, parent, std::ptr::null_mut(), GetModuleHandleW(std::ptr::null()), std::ptr::null());
        if w.is_null() {
            println!("native_probe: CreateWindowExW(page) failed: {}", std::io::Error::last_os_error());
        }
        SetLayeredWindowAttributes(w, 0, 255, LWA_ALPHA);
        w
    }
}

/// A shown toplevel of this process, as Chromium creates one of its own (DevTools, a native bubble).
fn foreign_toplevel() -> HWND {
    let class = page_class();
    unsafe {
        let w = CreateWindowExW(0, class.as_ptr(), class.as_ptr(), WS_OVERLAPPEDWINDOW, 300, 300, 400, 300, std::ptr::null_mut(), std::ptr::null_mut(), GetModuleHandleW(std::ptr::null()), std::ptr::null());
        ShowWindow(w, SW_SHOWNA);
        w
    }
}

// ---------------------------------------------------------------- Win32 reads

fn screen(window: HWND, x: i32, y: i32) -> POINT {
    let mut p = POINT { x, y };
    unsafe { ClientToScreen(window, &mut p) };
    p
}

fn rect_in(window: HWND, child: HWND) -> PhysRect {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(child, &mut r) };
    let mut p = POINT { x: r.left, y: r.top };
    unsafe { ScreenToClient(window, &mut p) };
    PhysRect { x: p.x, y: p.y, width: (r.right - r.left) as u32, height: (r.bottom - r.top) as u32 }
}

fn client_size(window: HWND) -> (i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetClientRect(window, &mut r) };
    (r.right, r.bottom)
}

fn at(p: POINT) -> HWND {
    unsafe { WindowFromPoint(p) }
}

fn visible(w: HWND) -> bool {
    unsafe { IsWindowVisible(w) != 0 }
}

fn alive(w: HWND) -> bool {
    unsafe { IsWindow(w) != 0 }
}

fn parent(w: HWND) -> HWND {
    unsafe { GetParent(w) }
}

fn lparam(x: i32, y: i32) -> isize {
    ((y as u16 as u32) << 16 | x as u16 as u32) as i32 as isize
}

/// A posted message, only ever to a window of this process.
fn post(window: HWND, message: u32, wparam: usize, x: i32, y: i32) -> Result<(), String> {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if pid != std::process::id() {
        return Err(format!("{window:?} is not ours"));
    }
    if unsafe { PostMessageW(window, message, wparam, lparam(x, y)) } == 0 {
        return Err("PostMessageW failed".into());
    }
    Ok(())
}

fn hit_test(window: HWND, p: POINT) -> u32 {
    unsafe { SendMessageW(window, WM_NCHITTEST, 0, lparam(p.x, p.y)) as u32 }
}

fn near(a: [u8; 4], colour: u32) -> bool {
    let want = [(colour >> 16) as u8, (colour >> 8) as u8, colour as u8];
    a.iter().zip(want).all(|(x, y)| x.abs_diff(y) <= 8)
}

fn out_dir() -> Option<PathBuf> {
    let env = PathBuf::from(std::env::var_os("VSESVIT_TESTENV")?);
    Some(env.parent()?.parent()?.to_path_buf())
}

fn centre(r: PhysRect) -> (i32, i32) {
    (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2)
}

// ---------------------------------------------------------------- the script

async fn script(report: &mut Report, cx: &mut AsyncApp, env: &vsesvit_testenv::TestEnv) {
    // GPUI quits once its last window closes; this one, never a browser window, keeps it running.
    let keeper = cx.open_window(WindowOptions { show: false, focus: false, ..Default::default() }, |_, cx| {
        cx.new(|_| Probe { layout: Layout::Full, overlay: None, surface: Rc::default(), content: Rc::default() })
    });
    if let Err(e) = keeper {
        return report.check("keeper_window", Err(e.to_string()));
    }
    let desktop = match WinDesktop::init() {
        Ok(d) => Rc::new(d),
        Err(e) => return report.check("desktop", Err(e.to_string())),
    };
    let a = match open(cx, (40., 40., 900., 600.), true) {
        Ok(w) => w,
        Err(e) => return report.check("attach", Err(e)),
    };
    let p1 = page(a.host());
    a.with(|s| s.show_page(NativeHandle::from_hwnd(p1), true));
    settle(cx).await;

    let before = (a.content.get(), rect_in(a.hwnd, a.host()), rect_in(a.hwnd, p1));
    a.set_layout(cx, Layout::Normal { toolbar: 80. });
    settle(cx).await;
    let after = (a.content.get(), rect_in(a.hwnd, a.host()), rect_in(a.hwnd, p1));
    let grew = (40. * a.scale).round() as i32;
    let page_fills = |(content, _, page): (PhysRect, PhysRect, PhysRect)| page.width == content.width && page.height == content.height;
    report.check(
        "geometry",
        expect(
            before.0.width > 0 && before.1 == before.0 && after.1 == after.0 && (after.0.y - before.0.y - grew).abs() <= 1 && page_fills(before) && page_fills(after),
            format!("content {:?} host {:?} -> content {:?} host {:?}, page {}x{}", before.0, before.1, after.0, after.1, after.2.width, after.2.height),
        ),
    );

    a.with(|s| s.show_page(NativeHandle::from_hwnd(p1), false));
    let hidden = !visible(p1);
    a.with(|s| s.show_page(NativeHandle::from_hwnd(p1), true));
    report.check("show_hide", expect(hidden && visible(p1), format!("hidden={hidden} shown={}", visible(p1))));
    settle(cx).await;

    let content = a.content.get();
    let capture = a.with(|s| s.capture());
    report.check(
        "capture",
        match capture {
            Ok(img) => {
                let (cx_, cy_) = centre(content);
                let page_px = img.get_pixel(cx_ as u32, cy_ as u32).0;
                let bar_px = img.get_pixel(10, (TITLEBAR_H * a.scale) as u32 + 5).0;
                if let Some(dir) = out_dir() {
                    let _ = img.save(dir.join("capture.png"));
                }
                expect(near(page_px, PAGE) && near(bar_px, TOOLBAR), format!("{}x{} page {page_px:?} toolbar {bar_px:?}", img.width(), img.height()))
            }
            Err(e) => Err(e.to_string()),
        },
    );

    let overlay_logical = Bounds { origin: point(px(500.), px(140.)), size: size(px(200.), px(150.)) };
    a.update(cx, |probe, _, cx| {
        probe.overlay = Some(overlay_logical);
        cx.notify();
    });
    let overlay = phys(overlay_logical, a.scale);
    let (ox, oy) = centre(overlay);
    let page_point = (content.x + 20, content.y + content.height as i32 - 20);
    a.with(|s| s.set_holes(&[Hole { rect: overlay, interactive: true }]));
    settle(cx).await;
    let over_hole = at(screen(a.hwnd, ox, oy));
    let over_page = at(screen(a.hwnd, page_point.0, page_point.1));
    let painted = a.with(|s| s.capture()).map(|img| img.get_pixel(ox as u32, oy as u32).0);
    a.with(|s| s.set_holes(&[Hole { rect: overlay, interactive: false }]));
    let passive = at(screen(a.hwnd, ox, oy));
    report.check(
        "holes",
        expect(
            over_hole == a.hwnd && over_page == p1 && passive == p1 && painted.as_ref().is_ok_and(|p| near(*p, OVERLAY)),
            format!("interactive hole -> {over_hole:?} (gpui {:?}), page -> {over_page:?} (page {p1:?}), passive hole -> {passive:?}, overlay paint {painted:?}", a.hwnd),
        ),
    );
    a.with(|s| s.set_holes(&[]));
    a.update(cx, |probe, _, cx| {
        probe.overlay = None;
        cx.notify();
    });

    let (into_page, page_focus, back, chrome_focus, active) = a.update(cx, |_, window, _| {
        let into_page = a.with(|s| s.focus(FocusTarget::Page(NativeHandle::from_hwnd(p1))));
        let page_focus = unsafe { GetFocus() };
        let back = a.with(|s| s.focus(FocusTarget::Chrome));
        let chrome_focus = unsafe { GetFocus() };
        (matches!(into_page, FocusOutcome::Done), page_focus, matches!(back, FocusOutcome::Done), chrome_focus, window.is_window_active())
    });
    report.check(
        "focus",
        expect(
            into_page && page_focus == p1 && back && chrome_focus == a.hwnd && active,
            format!("page {into_page} focus={page_focus:?}; chrome {back} focus={chrome_focus:?}; gpui active={active}"),
        ),
    );
    let in_update = a.update(cx, |_, _, cx| {
        let outcome = a.with(|s| s.focus(FocusTarget::Page(NativeHandle::from_hwnd(p1))));
        cx.notify();
        matches!(outcome, FocusOutcome::Done) && unsafe { GetFocus() } == p1
    });
    a.set_layout(cx, Layout::Normal { toolbar: 40. });
    settle(cx).await;
    let relayout = a.content.get().y == before.0.y;
    report.check(
        "focus_in_update",
        expect(in_update && relayout && unsafe { GetFocus() } == p1, format!("SetFocus inside an update: {in_update}; GPUI laid out afterwards: {relayout}")),
    );

    const VK_X: u16 = 0x58;
    PAGE_KEYS.with_borrow_mut(Vec::clear);
    let posted = env.input().and_then(|input| input.post_key(p1, VK_X));
    settle(cx).await;
    let keys = PAGE_KEYS.with_borrow(Clone::clone);
    report.check("keys_reach_page", expect(posted.is_ok() && keys == [(p1, VK_X as usize)], format!("posted {posted:?}; the page saw {keys:?}")));

    let mut presses = desktop.pointer_presses();
    let page_press = screen(p1, 15, 25);
    let chrome_press = screen(a.hwnd, 30, (TITLEBAR_H * a.scale) as i32 + 10);
    let posted = post(p1, WM_LBUTTONDOWN, 1, 15, 25)
        .and_then(|_| post(p1, WM_LBUTTONUP, 0, 15, 25))
        .and_then(|_| post(a.hwnd, WM_LBUTTONDOWN, 1, 30, (TITLEBAR_H * a.scale) as i32 + 10))
        .and_then(|_| post(a.hwnd, WM_LBUTTONUP, 0, 30, (TITLEBAR_H * a.scale) as i32 + 10));
    settle(cx).await;
    let mut seen = Vec::new();
    while let Ok(p) = presses.try_recv() {
        seen.push(p);
    }
    let want = [PhysPoint { x: page_press.x, y: page_press.y }, PhysPoint { x: chrome_press.x, y: chrome_press.y }];
    report.check("pointer_presses", expect(posted.is_ok() && seen == want, format!("posted {posted:?}; saw {seen:?}, want {want:?}")));

    frame_checks(report, cx, &a, p1).await;

    popup_checks(report, cx, &a, &desktop).await;

    let b = match open(cx, (1000., 80., 600., 450.), true) {
        Ok(w) => w,
        Err(e) => return report.check("second_window", Err(e)),
    };
    settle(cx).await;
    let focus_before = unsafe { GetFocus() };
    let refused = a.with(|s| s.focus(FocusTarget::Page(NativeHandle::from_hwnd(p1))));
    report.check(
        "focus_refused",
        expect(
            matches!(refused, FocusOutcome::Refused) && unsafe { GetFocus() } == focus_before,
            format!("inactive window: {}; focus stayed {focus_before:?}", if matches!(refused, FocusOutcome::Refused) { "Refused" } else { "Done" }),
        ),
    );

    let p3 = page(a.host());
    a.with(|s| s.show_page(NativeHandle::from_hwnd(p3), false));
    let moved = {
        let mut from = a.surface.borrow_mut();
        let mut to = b.surface.borrow_mut();
        from.as_mut().unwrap().move_page_to(NativeHandle::from_hwnd(p3), to.as_mut().unwrap())
    };
    report.check("move_page", expect(moved.is_ok() && parent(p3) == b.host(), format!("{moved:?}; parent {:?} (b host {:?})", parent(p3), b.host())));

    let hidden_host = desktop.hidden_host();
    let p2 = page(b.host());
    b.with(|s| s.show_page(NativeHandle::from_hwnd(p2), true));
    let parked = b.with(|s| s.park_page(NativeHandle::from_hwnd(p2), hidden_host));
    let b_hwnd = b.hwnd;
    b.close(cx);
    settle(cx).await;
    report.check(
        "park_page",
        expect(
            parked.is_ok() && !alive(b_hwnd) && alive(p2) && parent(p2) == hidden_host.hwnd() && !visible(p2) && parent(hidden_host.hwnd()) == a.hwnd,
            format!("{parked:?}; window closed={}; page alive={} parent={:?} (hidden host {:?}, in {:?})", !alive(b_hwnd), alive(p2), parent(p2), hidden_host.hwnd(), parent(hidden_host.hwnd())),
        ),
    );

    let foreign = foreign_toplevel();
    let was_visible = visible(foreign);
    let neutralized = desktop.neutralize(NativeHandle::from_hwnd(foreign));
    let style = unsafe { GetWindowLongPtrW(foreign, GWL_STYLE) } as u32;
    let gone = foreign_toplevel();
    unsafe { DestroyWindow(gone) };
    let refused = desktop.neutralize(NativeHandle::from_hwnd(gone));
    report.check(
        "neutralize",
        expect(
            was_visible && neutralized.is_ok() && !visible(foreign) && parent(foreign) == hidden_host.hwnd() && style & WS_CHILD != 0 && refused.is_err(),
            format!("{neutralized:?}: visible {was_visible} -> {}, parent {:?}, child style {}; destroyed window: {refused:?}", visible(foreign), parent(foreign), style & WS_CHILD != 0),
        ),
    );

    let c = match open(cx, (1000., 80., 500., 400.), false) {
        Ok(w) => w,
        Err(e) => return report.check("third_window", Err(e)),
    };
    let (a_hwnd, c_hwnd) = (a.hwnd, c.hwnd);
    a.close(cx);
    settle(cx).await;
    let moved_to_c = parent(hidden_host.hwnd()) == c_hwnd;
    c.close(cx);
    settle(cx).await;
    let anchor = parent(hidden_host.hwnd());
    let anchor_top = !anchor.is_null() && parent(anchor).is_null() && !visible(anchor);
    report.check(
        "hidden_host_outlives_windows",
        expect(
            !alive(a_hwnd) && moved_to_c && !alive(c_hwnd) && anchor_top && anchor != c_hwnd && alive(p2) && alive(foreign),
            format!("after the first window closed it lives in the third: {moved_to_c}; after the last: parent {anchor:?} (hidden); parked page alive {}, neutralized alive {}", alive(p2), alive(foreign)),
        ),
    );
    drop(desktop);
    report.check("desktop_drop", expect(!alive(hidden_host.hwnd()) && !alive(p2), format!("hidden host destroyed {}, its pages with it {}", !alive(hidden_host.hwnd()), !alive(p2))));
}

async fn frame_checks(report: &mut Report, cx: &mut AsyncApp, a: &Win, p1: HWND) {
    let (width, height) = client_size(a.hwnd);
    let bar = (TITLEBAR_H * a.scale) as i32;
    let button = (BUTTON_W * a.scale) as i32;
    let probes = [
        ("close", width - button / 2, bar / 2, HTCLOSE),
        ("max", width - button * 3 / 2, bar / 2, HTMAXBUTTON),
        ("min", width - button * 5 / 2, bar / 2, HTMINBUTTON),
        ("drag", width / 3, bar / 2 + 4, HTCAPTION),
        ("top", width / 3, 1, HTTOP),
    ];
    let mut results = Vec::new();
    let mut ok = true;
    for (name, x, y, want) in probes {
        // GPUI answers from the last mouse position it saw, as after a real move.
        let _ = post(a.hwnd, WM_MOUSEMOVE, 0, x, y);
        cx.background_executor().timer(Duration::from_millis(100)).await;
        let got = hit_test(a.hwnd, screen(a.hwnd, x, y));
        ok &= got == want;
        results.push(format!("{name}={got}/{want}"));
    }
    let mut frame = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(a.hwnd, &mut frame) };
    let (left_edge, bottom_edge) = (screen(a.hwnd, -2, height / 2), screen(a.hwnd, width / 2, height + 2));
    for (name, p, want) in [("left", left_edge, HTLEFT), ("bottom", bottom_edge, HTBOTTOM)] {
        let _ = post(a.hwnd, WM_NCMOUSEMOVE, want as usize, p.x, p.y);
        cx.background_executor().timer(Duration::from_millis(100)).await;
        let got = hit_test(a.hwnd, p);
        ok &= got == want;
        results.push(format!("{name}={got}/{want}"));
    }
    report.check("frame_hit_test", expect(ok, results.join(" ")));

    a.set_layout(cx, Layout::Full);
    settle(cx).await;
    let content = a.content.get();
    let top = at(screen(a.hwnd, width / 3, 1));
    let top_hit = hit_test(a.hwnd, screen(a.hwnd, width / 3, 1));
    let below = at(screen(a.hwnd, width / 3, 40));
    let left = at(left_edge);
    report.check(
        "frame_meets_content",
        expect(
            content.y == 0 && top == a.hwnd && top_hit == HTTOP && below == p1 && left == a.hwnd,
            format!("content {content:?}; top edge -> {top:?} hit {top_hit}; below the band -> {below:?} (page {p1:?}); left border -> {left:?}"),
        ),
    );
    a.set_layout(cx, Layout::Normal { toolbar: 40. });
    settle(cx).await;
}

async fn popup_checks(report: &mut Report, cx: &mut AsyncApp, a: &Win, desktop: &Rc<WinDesktop>) {
    let content = a.content.get();
    let (mx, my) = centre(content);
    let middle = screen(a.hwnd, mx, my);
    let d = desktop.clone();
    let opened = a.update(cx, |_, window, cx| {
        let popup = cx.open_window(options((300., 250., 260., 200.), WindowKind::PopUp, false), |popup, cx| {
            d.adopt_popup(popup, window);
            cx.new(|_| Probe { layout: Layout::Full, overlay: None, surface: Rc::default(), content: Rc::default() })
        });
        let dialog = cx.open_window(options((1000., 600., 300., 200.), WindowKind::Normal, false), |dialog, cx| {
            d.set_dialog_parent(dialog, window);
            cx.new(|_| Probe { layout: Layout::Full, overlay: None, surface: Rc::default(), content: Rc::default() })
        });
        (popup, dialog)
    });
    let (Ok(popup), Ok(dialog)) = opened else {
        return report.check("popup_owner", Err("could not open the popup and dialog windows".into()));
    };
    let hwnd_of = |h: WindowHandle<Probe>, cx: &mut AsyncApp| h.update(cx, |_, w, _| window_hwnd(w).ok()).ok().flatten();
    let (Some(popup_hwnd), Some(dialog_hwnd)) = (hwnd_of(popup, cx), hwnd_of(dialog, cx)) else {
        return report.check("popup_owner", Err("no HWND".into()));
    };
    let owner_at_once = unsafe { GetWindow(popup_hwnd, GW_OWNER) };
    settle(cx).await;
    let owner = unsafe { GetWindow(popup_hwnd, GW_OWNER) };
    let topmost = unsafe { GetWindowLongPtrW(popup_hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST != 0;
    report.check(
        "popup_owner",
        expect(
            owner_at_once.is_null() && owner == a.hwnd && !topmost,
            format!("owner inside the update {owner_at_once:?} (deferred), after it {owner:?} (main {:?}); topmost={topmost}", a.hwnd),
        ),
    );
    let dialog_owner = unsafe { GetWindow(dialog_hwnd, GW_OWNER) };
    report.check("dialog_parent", expect(dialog_owner == a.hwnd, format!("owner {dialog_owner:?}")));

    let mut popup_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(popup_hwnd, &mut popup_rect) };
    let on_popup = PhysPoint { x: (popup_rect.left + popup_rect.right) / 2, y: (popup_rect.top + popup_rect.bottom) / 2 };
    let over_popup_is = at(POINT { x: on_popup.x, y: on_popup.y });
    let found = desktop.toplevel_at(PhysPoint { x: middle.x, y: middle.y });
    let through_popup = desktop.toplevel_at(on_popup);
    let nowhere = desktop.toplevel_at(PhysPoint { x: -30000, y: -30000 });
    let main = Some(NativeHandle::from_hwnd(a.hwnd));
    report.check(
        "toplevel_at",
        expect(
            found == main && through_popup == main && over_popup_is == popup_hwnd && nowhere.is_none(),
            format!("over the page {found:?}; over our popup ({over_popup_is:?}) {through_popup:?}; off-screen {nowhere:?}; main {main:?}"),
        ),
    );
    for h in [popup, dialog] {
        let _ = h.update(cx, |_, w, _| w.remove_window());
    }
    settle(cx).await;
}
