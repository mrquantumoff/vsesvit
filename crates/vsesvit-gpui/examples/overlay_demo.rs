//! Owned by W2.1.
//! Overlays over a foreign child (design section 8.5 W2.1): a GPUI window whose content host holds a
//! plain native window standing in for a CEF page, with the window's `OverlayHost` above it. Real
//! input only, inside the test environment: XTEST into the private Xvfb, or messages posted to the
//! window a press at that point reaches on the private desktop. Checks: a menu with a submenu and
//! check and radio rows whose rows take clicks over the page, Escape, an outside press on the page,
//! and a passive list that an entry drives with typed keys while it keeps the keyboard. Screenshots
//! go next to the test environment's files.
//! Runs only under `cargo xtask probe overlay_demo`.

use std::cell::{Cell, RefCell};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, AsyncApp, Bounds, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window,
    WindowBounds, WindowHandle, WindowOptions, canvas, div, point, px, size,
};
use gpui_component::ActiveTheme as _;
use gpui_component::input::{Enter, Input, InputEvent, InputState, MoveDown, MoveUp};
use vsesvit_gpui::engine_bridge::SharedSurface;
use vsesvit_gpui::overlay::menu::{Command, Menu};
use vsesvit_gpui::overlay::{
    Align, Anchor, Dismiss, OverlayEvent, OverlayHost, OverlayId, OverlayKind, OverlaySpec, Placement,
};
use vsesvit_gpui::platform::{Desktop as _, FocusTarget, NativeHandle, Os, PhysRect};

const PAGE: u32 = 0x20_80_c0;
const BAR_H: f32 = 48.;
const TIMEOUT: Duration = Duration::from_secs(5);
const WORDS: [&str; 6] = ["apple", "apricot", "avocado", "banana", "blueberry", "cherry"];

type Log = Rc<RefCell<Vec<String>>>;

fn main() -> ExitCode {
    let env = vsesvit_testenv::require();
    println!("overlay_demo: the guard passed in {}", env.describe());
    gpui_platform::application().with_assets(vsesvit_gpui::theme::icons::VsesvitAssets).run(move |cx: &mut App| {
        vsesvit_gpui::theme::init(cx);
        cx.spawn(async move |cx| {
            let failed = script(cx, &env).await;
            println!("overlay_demo: {}", if failed == 0 { "all checks passed".to_owned() } else { format!("{failed} checks failed") });
            std::process::exit(i32::from(failed != 0));
        })
        .detach();
    });
    ExitCode::FAILURE
}

// ---------------------------------------------------------------- the demo window

/// The passive list under the entry. Its owner, the demo, fills it and moves its highlight.
struct List {
    items: Vec<SharedString>,
    highlighted: Option<usize>,
    rows: Rc<RefCell<Vec<Option<Bounds<Pixels>>>>>,
    demo: WeakEntity<Demo>,
}

impl Render for List {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        *self.rows.borrow_mut() = vec![None; self.items.len()];
        let rows = self.items.iter().enumerate().map(|(i, item)| {
            let (record, demo) = (self.rows.clone(), self.demo.clone());
            div()
                .id(("word", i))
                .relative()
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .when(self.highlighted == Some(i), |d| d.bg(theme.accent).text_color(theme.accent_foreground))
                .child(item.clone())
                .child(canvas(move |b, _, _| record.borrow_mut()[i] = Some(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
                .on_click(move |_, window, cx| {
                    let _ = demo.update(cx, |demo, cx| demo.pick(i, window, cx));
                })
        });
        div().w(px(300.)).p_1().bg(theme.popover).border_1().border_color(theme.border).rounded(theme.radius).children(rows)
    }
}

struct Demo {
    host: Entity<OverlayHost>,
    entry: Entity<InputState>,
    list: Entity<List>,
    list_open: Option<OverlayId>,
    surface: SharedSurface,
    content: Rc<Cell<PhysRect>>,
    button: Rc<Cell<Option<Bounds<Pixels>>>>,
    field: Rc<Cell<Option<Bounds<Pixels>>>>,
    bar_on: bool,
    zoom: usize,
    log: Log,
    _subscriptions: Vec<Subscription>,
}

impl Demo {
    fn new(surface: SharedSurface, log: Log, window: &mut Window, cx: &mut Context<Self>) -> Demo {
        let host = cx.new(|cx| OverlayHost::new(Some(surface.clone()), window, cx));
        let entry = cx.new(|cx| InputState::new(window, cx).placeholder("Type a fruit"));
        let me = cx.entity().downgrade();
        let list = cx.new(|_| List { items: Vec::new(), highlighted: None, rows: Rc::default(), demo: me });
        let keyboard = surface.clone();
        let subscriptions = vec![
            cx.subscribe_in(&entry, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event {
                    this.entry_changed(window, cx);
                }
            }),
            // The demo has no focus policy: an overlay that takes the keyboard takes it from the page.
            cx.subscribe(&host, move |_, _, event: &OverlayEvent, _| {
                if *event == OverlayEvent::KeyboardTaken
                    && let Ok(mut surface) = keyboard.try_borrow_mut()
                {
                    surface.focus(FocusTarget::Chrome);
                }
            }),
        ];
        Demo {
            host,
            entry,
            list,
            list_open: None,
            surface,
            content: Rc::default(),
            button: Rc::default(),
            field: Rc::default(),
            bar_on: true,
            zoom: 1,
            log,
            _subscriptions: subscriptions,
        }
    }

    fn menu(&self, cx: &Context<Self>) -> Menu {
        let me = cx.entity().downgrade();
        let say = |log: &Log, text: String| log.borrow_mut().push(text);
        let (log, toggle) = (self.log.clone(), me.clone());
        let mut zoom = Menu::new();
        for (i, label) in ["90%", "100%", "110%"].into_iter().enumerate() {
            let (log, me) = (self.log.clone(), me.clone());
            zoom = zoom.item(
                Command::run(label, move |_, cx| {
                    say(&log, format!("zoom {label}"));
                    let _ = me.update(cx, |demo, _| demo.zoom = i);
                })
                .radio(self.zoom == i),
            );
        }
        let back = self.log.clone();
        Menu::new()
            .item(Command::run("Back", move |_, _| say(&back, "back".into())))
            .item(
                Command::run("Bookmarks bar", move |_, cx| {
                    let _ = toggle.update(cx, |demo, _| demo.bar_on = !demo.bar_on);
                    say(&log, "bookmarks bar".into());
                })
                .checked(self.bar_on),
            )
            .submenu("Zoom", zoom)
            .separator()
            .item(Command::run("Disabled", |_, _| {}).enabled(false))
    }

    fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(anchor) = self.button.get() else { return };
        let menu = self.menu(cx);
        let log = self.log.clone();
        let on_close = Box::new(move |why, _: &mut App| log.borrow_mut().push(format!("menu closed: {why:?}")));
        self.host.update(cx, |host, cx| host.open_menu(menu, Anchor::Rect(anchor), Placement::Below(Align::Start), Some(on_close), window, cx));
    }

    /// The entry's text changed: the list shows the words it starts, or goes.
    fn entry_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.entry.read(cx).value().to_lowercase();
        let words: Vec<SharedString> =
            if typed.is_empty() { Vec::new() } else { WORDS.iter().filter(|w| w.starts_with(&typed)).map(|w| SharedString::from(*w)).collect() };
        self.list.update(cx, |list, cx| {
            list.items = words.clone();
            list.highlighted = None;
            cx.notify();
        });
        let open = self.list_open.filter(|id| self.host.read(cx).is_open(*id));
        match (open, words.is_empty()) {
            (Some(id), true) => {
                self.host.update(cx, |host, cx| host.close(id, window, cx));
                self.list_open = None;
            }
            (None, false) => {
                let Some(field) = self.field.get() else { return };
                let spec = OverlaySpec::new(OverlayKind::Suggestions, Anchor::Rect(field), Placement::Below(Align::Start), self.list.clone())
                    .dismiss(Dismiss::OWNER);
                self.list_open = Some(self.host.update(cx, |host, cx| host.open(spec, window, cx)));
            }
            _ => {}
        }
    }

    /// Up and Down in the entry move the list's highlight; the entry keeps the keyboard.
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            let n = list.items.len();
            if n > 0 {
                list.highlighted = Some(match (list.highlighted, forward) {
                    (None, true) => 0,
                    (None, false) => n - 1,
                    (Some(i), true) => (i + 1) % n,
                    (Some(i), false) => (i + n - 1) % n,
                });
                cx.notify();
            }
        });
    }

    fn pick(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(word) = self.list.read(cx).items.get(index).cloned() else { return };
        self.log.borrow_mut().push(format!("picked {word}"));
        if let Some(id) = self.list_open.take() {
            self.host.update(cx, |host, cx| host.close(id, window, cx));
        }
        self.entry.update(cx, |entry, cx| entry.set_value(word, window, cx));
    }
}

fn record(slot: Rc<Cell<Option<Bounds<Pixels>>>>) -> impl IntoElement {
    canvas(move |b, _, _| slot.set(Some(b)), |_, _, _, _| {}).absolute().top_0().left_0().size_full()
}

impl Render for Demo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (surface, content) = (self.surface.clone(), self.content.clone());
        let page = canvas(
            move |bounds, window, _| {
                let s = window.scale_factor();
                let p = |v: Pixels| (f32::from(v) * s).round() as i32;
                let rect = PhysRect { x: p(bounds.origin.x), y: p(bounds.origin.y), width: p(bounds.size.width) as u32, height: p(bounds.size.height) as u32 };
                content.set(rect);
                if let Ok(mut surface) = surface.try_borrow_mut() {
                    surface.set_content_rect(rect);
                }
            },
            |_, _, _, _| {},
        )
        .flex_1()
        .w_full();
        let bar = div()
            .h(px(BAR_H))
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .bg(theme.background)
            .child(
                div()
                    .id("menu-button")
                    .relative()
                    .px_3()
                    .py_1()
                    .rounded(theme.radius)
                    .bg(theme.secondary)
                    .child("Menu")
                    .child(record(self.button.clone()))
                    .on_click(cx.listener(|this, _, window, cx| this.open_menu(window, cx))),
            )
            .child(
                div()
                    .relative()
                    .w(px(300.))
                    .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                        this.step(true, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                        this.step(false, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                        if let Some(i) = this.list.read(cx).highlighted {
                            this.pick(i, window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(Input::new(&self.entry))
                    .child(record(self.field.clone())),
            );
        div().size_full().flex().flex_col().text_color(theme.foreground).child(bar).child(page).child(self.host.clone())
    }
}

// ---------------------------------------------------------------- native input and reads, per OS

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Target {
    Gpui,
    Page,
    Other,
}

#[derive(Copy, Clone, Debug)]
enum Key {
    Down,
    Enter,
    Escape,
    Backspace,
    Char(char),
}

/// What the script needs from the OS, at points in the GPUI window's client area (physical pixels).
trait Native {
    /// A page window in `host`, standing in for a tab's browser.
    fn page(&mut self, host: NativeHandle) -> Result<NativeHandle, String>;
    /// The window a press at `at` reaches.
    fn target_at(&self, at: (i32, i32)) -> Target;
    fn click(&self, at: (i32, i32)) -> Result<(), String>;
    fn hover(&self, at: (i32, i32)) -> Result<(), String>;
    fn key(&self, key: Key) -> Result<(), String>;
    /// Presses the page window received since the last call.
    fn page_presses(&self) -> usize;
}

#[cfg(windows)]
mod native {
    use std::cell::{Cell, RefCell};

    use vsesvit_gpui::platform::NativeHandle;
    use vsesvit_testenv::PrivateInput;
    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, CreateSolidBrush, ScreenToClient};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, GetWindowThreadProcessId, LWA_ALPHA, RegisterClassExW, SetLayeredWindowAttributes,
        WM_LBUTTONDOWN, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED, WindowFromPoint,
    };

    use super::{Key, Native, PAGE, Target};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    }

    thread_local!(static PRESSES: Cell<usize> = const { Cell::new(0) });

    unsafe extern "system" fn page_procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if message == WM_LBUTTONDOWN {
            PRESSES.set(PRESSES.get() + 1);
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    pub struct Win {
        pub gpui: HWND,
        page: RefCell<HWND>,
        input: PrivateInput,
    }

    impl Win {
        pub fn new(gpui: HWND, input: PrivateInput) -> Win {
            Win { gpui, page: RefCell::new(std::ptr::null_mut()), input }
        }

        fn screen(&self, (x, y): (i32, i32)) -> POINT {
            let mut p = POINT { x, y };
            unsafe { ClientToScreen(self.gpui, &mut p) };
            p
        }

        /// The window a press at `at` reaches, and the point in its client area.
        fn reached(&self, at: (i32, i32)) -> Result<(HWND, i16, i16), String> {
            let mut p = self.screen(at);
            let window = unsafe { WindowFromPoint(p) };
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(window, &mut pid) };
            if pid != std::process::id() {
                return Err(format!("{at:?} reaches {window:?}, not a window of ours"));
            }
            unsafe { ScreenToClient(window, &mut p) };
            Ok((window, p.x as i16, p.y as i16))
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    impl Native for Win {
        fn page(&mut self, host: NativeHandle) -> Result<NativeHandle, String> {
            let class = wide("OverlayDemoPage");
            let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(page_procedure),
                hInstance: instance,
                // COLORREF is 0x00BBGGRR.
                hbrBackground: unsafe { CreateSolidBrush(((PAGE & 0xff) << 16) | (PAGE & 0xff00) | (PAGE >> 16)) },
                lpszClassName: class.as_ptr(),
                ..unsafe { std::mem::zeroed() }
            };
            unsafe { RegisterClassExW(&wc) };
            // Layered, so DWM composes it from its own surface, as it does Chromium's output.
            let page = unsafe {
                CreateWindowExW(WS_EX_LAYERED, class.as_ptr(), class.as_ptr(), WS_CHILD | WS_CLIPSIBLINGS, 0, 0, 10, 10, host.hwnd().cast(), std::ptr::null_mut(), instance, std::ptr::null())
            };
            if page.is_null() {
                return Err(format!("CreateWindowExW: {}", std::io::Error::last_os_error()));
            }
            unsafe { SetLayeredWindowAttributes(page, 0, 255, LWA_ALPHA) };
            *self.page.borrow_mut() = page;
            Ok(NativeHandle::from_hwnd(page.cast()))
        }

        fn target_at(&self, at: (i32, i32)) -> Target {
            let window = unsafe { WindowFromPoint(self.screen(at)) };
            if window == self.gpui {
                Target::Gpui
            } else if window == *self.page.borrow() {
                Target::Page
            } else {
                Target::Other
            }
        }

        fn click(&self, at: (i32, i32)) -> Result<(), String> {
            let (window, x, y) = self.reached(at)?;
            self.input.post_click(window, x, y)
        }

        fn hover(&self, at: (i32, i32)) -> Result<(), String> {
            let (window, x, y) = self.reached(at)?;
            self.input.post_mouse_move(window, x, y)
        }

        fn key(&self, key: Key) -> Result<(), String> {
            let vk = match key {
                Key::Down => 0x28,
                Key::Enter => 0x0d,
                Key::Escape => 0x1b,
                Key::Backspace => 0x08,
                Key::Char(c) => c.to_ascii_uppercase() as u16,
            };
            self.input.post_key(self.gpui, vk)
        }

        fn page_presses(&self) -> usize {
            PRESSES.replace(0)
        }
    }
}

#[cfg(target_os = "linux")]
mod native {
    use std::cell::Cell;

    use vsesvit_gpui::platform::NativeHandle;
    use vsesvit_testenv::PrivateInput;
    use x11rb::connection::Connection as _;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{self, ConnectionExt as _};
    use x11rb::rust_connection::RustConnection;

    use super::{Key, Native, PAGE, Target};

    pub struct X {
        conn: RustConnection,
        root: u32,
        pub gpui: u32,
        page: Cell<u32>,
        presses: Cell<usize>,
        input: PrivateInput,
    }

    impl X {
        pub fn new(gpui: u32, input: PrivateInput) -> Result<X, String> {
            let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
            let root = conn.setup().roots[screen].root;
            Ok(X { conn, root, gpui, page: Cell::new(0), presses: Cell::new(0), input })
        }

        fn root_point(&self, (x, y): (i32, i32)) -> Result<(i16, i16), String> {
            let r = self.conn.translate_coordinates(self.gpui, self.root, x as i16, y as i16).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
            Ok((r.dst_x, r.dst_y))
        }

        fn keycode(&self, keysym: u32) -> Result<u8, String> {
            let setup = self.conn.setup();
            let count = setup.max_keycode - setup.min_keycode + 1;
            let map = self.conn.get_keyboard_mapping(setup.min_keycode, count).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
            let per = usize::from(map.keysyms_per_keycode.max(1));
            let index = map.keysyms.chunks(per).position(|syms| syms.first() == Some(&keysym) || syms.contains(&keysym)).ok_or(format!("no keycode for keysym 0x{keysym:x}"))?;
            Ok(setup.min_keycode + u8::try_from(index).map_err(|e| e.to_string())?)
        }
    }

    impl Native for X {
        fn page(&mut self, host: NativeHandle) -> Result<NativeHandle, String> {
            let id = self.conn.generate_id().map_err(|e| e.to_string())?;
            let aux = xproto::CreateWindowAux::new().background_pixel(PAGE).event_mask(xproto::EventMask::BUTTON_PRESS);
            self.conn
                .create_window(x11rb::COPY_DEPTH_FROM_PARENT, id, host.xid(), 0, 0, 10, 10, 0, xproto::WindowClass::INPUT_OUTPUT, x11rb::COPY_FROM_PARENT, &aux)
                .map_err(|e| e.to_string())?
                .check()
                .map_err(|e| format!("CreateWindow: {e}"))?;
            self.conn.flush().map_err(|e| e.to_string())?;
            self.page.set(id);
            Ok(NativeHandle::from_xid(id))
        }

        /// The deepest window a press there reaches, by the server's own rules (input SHAPE
        /// included), then which of ours it is in.
        fn target_at(&self, at: (i32, i32)) -> Target {
            let Ok((x, y)) = self.root_point(at) else { return Target::Other };
            let mut w = self.root;
            let mut path = Vec::new();
            while let Some(r) = self.conn.translate_coordinates(self.root, w, x, y).ok().and_then(|c| c.reply().ok())
                && r.child != x11rb::NONE
            {
                w = r.child;
                path.push(w);
            }
            match path.last() {
                Some(&deepest) if deepest == self.page.get() => Target::Page,
                Some(_) if path.contains(&self.gpui) => Target::Gpui,
                _ => Target::Other,
            }
        }

        fn click(&self, at: (i32, i32)) -> Result<(), String> {
            let (x, y) = self.root_point(at)?;
            self.input.click(x, y)
        }

        fn hover(&self, at: (i32, i32)) -> Result<(), String> {
            let (x, y) = self.root_point(at)?;
            self.input.move_pointer(x, y)
        }

        fn key(&self, key: Key) -> Result<(), String> {
            let keysym = match key {
                Key::Down => 0xff54,
                Key::Enter => 0xff0d,
                Key::Escape => 0xff1b,
                Key::Backspace => 0xff08,
                Key::Char(c) => c as u32,
            };
            self.input.key(self.keycode(keysym)?)
        }

        fn page_presses(&self) -> usize {
            while let Ok(Some(event)) = self.conn.poll_for_event() {
                if matches!(event, Event::ButtonPress(b) if b.event == self.page.get()) {
                    self.presses.set(self.presses.get() + 1);
                }
            }
            self.presses.replace(0)
        }
    }
}

// ---------------------------------------------------------------- the script

struct Rig {
    handle: WindowHandle<Demo>,
    view: Entity<Demo>,
    host: Entity<OverlayHost>,
    surface: SharedSurface,
    native: Box<dyn Native>,
    log: Log,
    scale: f32,
}

impl Rig {
    fn phys(&self, p: gpui::Point<Pixels>) -> (i32, i32) {
        ((f32::from(p.x) * self.scale).round() as i32, (f32::from(p.y) * self.scale).round() as i32)
    }

    fn overlays(&self, cx: &mut AsyncApp) -> Vec<vsesvit_gpui::overlay::OverlayInfo> {
        cx.update(|cx| self.host.read(cx).open_overlays())
    }

    /// Row `row` of the menu overlay `id`, in physical client pixels.
    fn menu_row(&self, cx: &mut AsyncApp, id: OverlayId, row: usize) -> Option<(i32, i32)> {
        let bounds = cx.update(|cx| {
            let view = self.host.read(cx).content(id)?.downcast::<vsesvit_gpui::overlay::menu::MenuView>().ok()?;
            view.read(cx).row_bounds().get(row).copied().flatten()
        })?;
        Some(self.phys(bounds.center()))
    }

    fn list_row(&self, cx: &mut AsyncApp, row: usize) -> Option<(i32, i32)> {
        let bounds = cx.update(|cx| self.view.read(cx).list.read(cx).rows.borrow().get(row).copied().flatten())?;
        Some(self.phys(bounds.center()))
    }

    fn logged(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn capture(&self, cx: &mut AsyncApp, name: &str) -> Result<image::RgbaImage, String> {
        // Between GPUI updates: on Windows PrintWindow re-enters GPUI's window procedure.
        let _ = self.handle.update(cx, |_, window, _| window.refresh());
        let image = self.surface.borrow().capture().map_err(|e| e.to_string())?;
        if let Some(dir) = out_dir() {
            let path = dir.join(name);
            image.save(&path).map_err(|e| e.to_string())?;
            println!("overlay_demo: saved {}", path.display());
        }
        Ok(image)
    }
}

fn out_dir() -> Option<std::path::PathBuf> {
    let env = std::path::PathBuf::from(std::env::var_os("VSESVIT_TESTENV")?);
    Some(env.parent()?.parent()?.to_path_buf())
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

async fn script(cx: &mut AsyncApp, env: &vsesvit_testenv::TestEnv) -> u32 {
    let mut failed = 0;
    let rig = match setup(cx, env).await {
        Ok(rig) => rig,
        Err(e) => {
            println!("overlay_demo: FAIL setup: {e}");
            return 1;
        }
    };
    for (name, result) in [
        ("menu_over_the_page", menu_over_the_page(cx, &rig).await),
        ("radio_row_takes_a_click_over_the_page", radio_row(cx, &rig).await),
        ("check_row_takes_a_click_over_the_page", check_row(cx, &rig).await),
        ("escape_closes_the_menu", escape(cx, &rig).await),
        ("outside_press_on_the_page", outside_press(cx, &rig).await),
        ("passive_list_driven_from_an_entry", passive_list(cx, &rig).await),
    ] {
        match result {
            Ok(detail) => println!("overlay_demo: ok   {name}: {detail}"),
            Err(detail) => {
                failed += 1;
                println!("overlay_demo: FAIL {name}: {detail}");
            }
        }
    }
    failed
}

async fn setup(cx: &mut AsyncApp, env: &vsesvit_testenv::TestEnv) -> Result<Rig, String> {
    let desktop = Rc::new(cx.update(|_| <Os as vsesvit_gpui::platform::Desktop>::init()).map_err(|e| e.to_string())?);
    let presses = desktop.clone();
    cx.update(|cx| vsesvit_gpui::overlay::install(move || presses.pointer_presses(), cx));
    let log: Log = Rc::default();
    let slot: Rc<RefCell<Option<SharedSurface>>> = Rc::default();
    let (s, l) = (slot.clone(), log.clone());
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin: point(px(60.), px(60.)), size: size(px(900.), px(600.)) })),
        titlebar: Some(gpui::TitlebarOptions { title: Some("overlay_demo".into()), ..Default::default() }),
        window_background: vsesvit_gpui::window::frame::background(<vsesvit_gpui::platform::Surface as vsesvit_gpui::platform::NativeSurface>::TRANSPARENT_CONTENT),
        inactive_frame_interval: None,
        ..Default::default()
    };
    let handle = cx
        .open_window(options, move |window, cx| {
            let surface: SharedSurface = Rc::new(RefCell::new(vsesvit_gpui::platform::attach(window).expect("attach")));
            *s.borrow_mut() = Some(surface.clone());
            cx.new(|cx| Demo::new(surface, l, window, cx))
        })
        .map_err(|e| e.to_string())?;
    let surface = slot.borrow().clone().ok_or("no surface")?;
    let (view, host, scale) = handle.update(cx, |demo, window, cx| (cx.entity(), demo.host.clone(), window.scale_factor())).map_err(|e| e.to_string())?;
    let input = env.input()?;
    #[cfg(windows)]
    let mut native: Box<dyn Native> = {
        let hwnd = handle.update(cx, |_, window, _| vsesvit_gpui::platform::windows::window_hwnd(window)).map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
        Box::new(native::Win::new(hwnd, input))
    };
    #[cfg(target_os = "linux")]
    let mut native: Box<dyn Native> = {
        use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
        let xid = handle
            .update(cx, |_, window, _| match window.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Xcb(h)) => Ok(h.window.get()),
                other => Err(format!("not an X11 window: {other:?}")),
            })
            .map_err(|e| e.to_string())??;
        Box::new(native::X::new(xid, input)?)
    };
    let host_handle = surface.borrow().host();
    let page = native.page(host_handle)?;
    surface.borrow_mut().show_page(page, true);
    let content = view.clone();
    until(cx, "the page to be laid out", |cx| cx.update(|cx| content.read(cx).content.get().width > 1)).await?;
    sleep(cx, Duration::from_millis(400)).await;
    let rig = Rig { handle, view, host, surface, native, log, scale };
    let (x, y) = page_point(cx, &rig);
    ensure(rig.native.target_at((x, y)) == Target::Page, || format!("the page is not under {x},{y}: {:?}", rig.native.target_at((x, y))))?;
    Ok(rig)
}

/// A point on the page far from every overlay: its bottom-right corner.
fn page_point(cx: &mut AsyncApp, rig: &Rig) -> (i32, i32) {
    let c = cx.update(|cx| rig.view.read(cx).content.get());
    (c.x + c.width as i32 - 40, c.y + c.height as i32 - 40)
}

/// Clicks the Menu button and waits for the menu to be drawn.
async fn open_menu(cx: &mut AsyncApp, rig: &Rig) -> Result<OverlayId, String> {
    let button = cx.update(|cx| rig.view.read(cx).button.get()).ok_or("the Menu button was not laid out")?;
    rig.native.click(rig.phys(button.center()))?;
    until(cx, "the menu", |cx| rig.overlays(cx).first().is_some_and(|o| o.kind == OverlayKind::Menu && o.rect.is_some())).await?;
    sleep(cx, Duration::from_millis(150)).await;
    Ok(rig.overlays(cx)[0].id)
}

async fn menu_over_the_page(cx: &mut AsyncApp, rig: &Rig) -> Result<String, String> {
    let menu = open_menu(cx, rig).await?;
    let back = rig.menu_row(cx, menu, 0).ok_or("no Back row")?;
    let page = cx.update(|cx| rig.view.read(cx).content.get());
    ensure(back.1 > page.y, || format!("the Back row at {back:?} is not over the page {page:?}"))?;
    ensure(rig.native.target_at(back) == Target::Gpui, || format!("a press on Back reaches {:?}", rig.native.target_at(back)))?;
    let zoom = rig.menu_row(cx, menu, 2).ok_or("no Zoom row")?;
    rig.native.hover(zoom)?;
    until(cx, "the Zoom submenu", |cx| rig.overlays(cx).iter().any(|o| o.parent == Some(menu) && o.rect.is_some())).await?;
    sleep(cx, Duration::from_millis(150)).await;
    let overlays = rig.overlays(cx);
    let sub = overlays.iter().find(|o| o.parent == Some(menu)).map(|o| o.id).ok_or("no submenu")?;
    let row = rig.menu_row(cx, sub, 1).ok_or("no 100% row")?;
    ensure(rig.native.target_at(row) == Target::Gpui, || format!("a press on 100% reaches {:?}", rig.native.target_at(row)))?;
    let holes = cx.update(|cx| rig.host.read(cx).holes().len());
    ensure(holes > 2, || format!("{holes} holes for a menu and its submenu"))?;
    let shot = rig.capture(cx, "overlay-menu.png")?;
    let painted = |at: (i32, i32)| shot.get_pixel(at.0 as u32, at.1 as u32).0;
    let near_page = |p: [u8; 4]| p[..3].iter().zip([(PAGE >> 16) as u8, (PAGE >> 8) as u8, PAGE as u8]).all(|(a, b)| a.abs_diff(b) <= 12);
    let over = painted(back);
    ensure(!near_page(over), || format!("the menu does not show over the page: {over:?} at {back:?}"))?;
    let sub_rect = overlays.iter().find(|o| o.id == sub).and_then(|o| o.rect).ok_or("the submenu was not drawn")?;
    // The top-right corner: the top-left one lies over the menu it opened from.
    let corner = rig.phys(sub_rect.top_right());
    let corner = (corner.0 - 1, corner.1);
    let corner_pixel = painted(corner);
    let diagonal: Vec<[u8; 4]> = (0..8).map(|d| painted((corner.0 - d, corner.1 + d))).collect();
    ensure(near_page(corner_pixel), || format!("the submenu's rounded corner at {corner:?} shows {diagonal:?} along its diagonal, not the page"))?;
    Ok(format!("menu and submenu drawn over the page in {holes} hole rectangles; presses on Back {back:?} and 100% {row:?} reach GPUI; menu pixel {over:?}, the submenu's corner shows the page {corner_pixel:?}"))
}

async fn radio_row(cx: &mut AsyncApp, rig: &Rig) -> Result<String, String> {
    let menu = rig.overlays(cx).first().map(|o| o.id).ok_or("the menu is not open")?;
    let sub = rig.overlays(cx).iter().find(|o| o.parent == Some(menu)).map(|o| o.id).ok_or("the submenu is not open")?;
    let row = rig.menu_row(cx, sub, 2).ok_or("no 110% row")?;
    rig.native.hover(row)?;
    sleep(cx, Duration::from_millis(100)).await;
    rig.native.click(row)?;
    until(cx, "the 110% row to run", |_| rig.logged().iter().any(|l| l == "zoom 110%")).await?;
    until(cx, "the menu to close", |cx| rig.overlays(cx).is_empty() && cx.update(|cx| rig.host.read(cx).holes().is_empty())).await?;
    sleep(cx, Duration::from_millis(150)).await;
    let zoom = cx.update(|cx| rig.view.read(cx).zoom);
    ensure(zoom == 2, || format!("zoom index {zoom}"))?;
    ensure(rig.native.target_at(row) == Target::Page, || format!("after the menu closed a press there reaches {:?}", rig.native.target_at(row)))?;
    ensure(rig.native.page_presses() == 0, || "the click reached the page".into())?;
    Ok(format!("110% ran, the menu closed with {:?}, its hole closed; the page under it again", rig.logged().iter().rev().find(|l| l.starts_with("menu closed"))))
}

async fn check_row(cx: &mut AsyncApp, rig: &Rig) -> Result<String, String> {
    let menu = open_menu(cx, rig).await?;
    let row = rig.menu_row(cx, menu, 1).ok_or("no Bookmarks bar row")?;
    rig.native.hover(row)?;
    sleep(cx, Duration::from_millis(100)).await;
    rig.native.click(row)?;
    until(cx, "the check row to run", |_| rig.logged().iter().any(|l| l == "bookmarks bar")).await?;
    until(cx, "the menu to close", |cx| rig.overlays(cx).is_empty()).await?;
    let on = cx.update(|cx| rig.view.read(cx).bar_on);
    ensure(!on, || "the check row did not toggle".into())?;
    ensure(rig.native.page_presses() == 0, || "the click reached the page".into())?;
    Ok("Bookmarks bar toggled off by a click over the page".into())
}

async fn escape(cx: &mut AsyncApp, rig: &Rig) -> Result<String, String> {
    open_menu(cx, rig).await?;
    rig.native.key(Key::Escape)?;
    until(cx, "Escape to close the menu", |cx| rig.overlays(cx).is_empty()).await?;
    let last = rig.logged().last().cloned().unwrap_or_default();
    ensure(last == "menu closed: Escape", || format!("last log {last}"))?;
    Ok(last)
}

async fn outside_press(cx: &mut AsyncApp, rig: &Rig) -> Result<String, String> {
    open_menu(cx, rig).await?;
    rig.native.page_presses();
    let at = page_point(cx, rig);
    ensure(rig.native.target_at(at) == Target::Page, || format!("{at:?} is not on the page"))?;
    rig.native.click(at)?;
    until(cx, "the press to close the menu", |cx| rig.overlays(cx).is_empty()).await?;
    sleep(cx, Duration::from_millis(100)).await;
    let last = rig.logged().last().cloned().unwrap_or_default();
    ensure(last == "menu closed: OutsidePress", || format!("last log {last}"))?;
    let presses = rig.native.page_presses();
    ensure(presses == 1, || format!("the page saw {presses} presses"))?;
    Ok(format!("a press on the page at {at:?} closed the menu ({last}); the page got the press"))
}

async fn type_text(cx: &mut AsyncApp, rig: &Rig, text: &str) -> Result<(), String> {
    for c in text.chars() {
        rig.native.key(Key::Char(c))?;
        sleep(cx, Duration::from_millis(60)).await;
    }
    Ok(())
}

async fn passive_list(cx: &mut AsyncApp, rig: &Rig) -> Result<String, String> {
    let field = cx.update(|cx| rig.view.read(cx).field.get()).ok_or("the entry was not laid out")?;
    rig.native.click(rig.phys(field.center()))?;
    sleep(cx, Duration::from_millis(200)).await;
    // The demo has no focus policy: put the X focus (or Win32 focus) back in GPUI after the click.
    let _ = rig.surface.try_borrow_mut().map(|mut s| s.focus(FocusTarget::Chrome));
    let entry_focused = |cx: &mut AsyncApp| {
        rig.handle.update(cx, |demo, window, cx| gpui::Focusable::focus_handle(demo.entry.read(cx), cx).is_focused(window)).unwrap_or(false)
    };
    until(cx, "the entry to take the keyboard", |cx| entry_focused(cx)).await?;
    type_text(cx, rig, "ap").await?;
    until(cx, "the list for \"ap\"", |cx| rig.overlays(cx).iter().any(|o| o.kind == OverlayKind::Suggestions && o.rect.is_some())).await?;
    let items = cx.update(|cx| rig.view.read(cx).list.read(cx).items.clone());
    ensure(items == ["apple", "apricot"], || format!("the list shows {items:?}"))?;
    let info = rig.overlays(cx).into_iter().find(|o| o.kind == OverlayKind::Suggestions).ok_or("no list")?;
    ensure(!info.interactive, || "the list took the keyboard".into())?;
    for _ in 0..2 {
        rig.native.key(Key::Down)?;
        sleep(cx, Duration::from_millis(60)).await;
    }
    let reached = until(cx, "Down Down to reach apricot", |cx| cx.update(|cx| rig.view.read(cx).list.read(cx).highlighted) == Some(1)).await;
    let highlighted = cx.update(|cx| rig.view.read(cx).list.read(cx).highlighted);
    reached.map_err(|e| format!("{e}: the highlight is on {highlighted:?}, the entry has the keyboard: {}", entry_focused(cx)))?;
    ensure(entry_focused(cx), || "the entry lost the keyboard to the list".into())?;
    sleep(cx, Duration::from_millis(150)).await;
    rig.capture(cx, "overlay-list.png")?;
    let row = rig.list_row(cx, 1).ok_or("no apricot row")?;
    ensure(rig.native.target_at(row) == Target::Gpui, || format!("a press on apricot reaches {:?}", rig.native.target_at(row)))?;
    rig.native.key(Key::Enter)?;
    until(cx, "Enter to pick apricot", |_| rig.logged().iter().any(|l| l == "picked apricot")).await?;
    until(cx, "the list to close", |cx| rig.overlays(cx).is_empty()).await?;

    for _ in 0.."pricot".len() {
        rig.native.key(Key::Backspace)?;
        sleep(cx, Duration::from_millis(40)).await;
    }
    until(cx, "the list for \"a\"", |cx| cx.update(|cx| rig.view.read(cx).list.read(cx).items.len()) == 3 && rig.overlays(cx).first().is_some_and(|o| o.rect.is_some())).await?;
    sleep(cx, Duration::from_millis(150)).await;
    let row = rig.list_row(cx, 2).ok_or("no avocado row")?;
    ensure(rig.native.target_at(row) == Target::Gpui, || format!("a press on avocado reaches {:?}", rig.native.target_at(row)))?;
    rig.native.click(row)?;
    until(cx, "a click to pick avocado", |_| rig.logged().iter().any(|l| l == "picked avocado")).await?;
    until(cx, "the list to close", |cx| rig.overlays(cx).is_empty()).await?;
    let text = cx.update(|cx| rig.view.read(cx).entry.read(cx).value().to_string());
    ensure(text == "avocado", || format!("the entry holds {text}"))?;
    ensure(rig.native.page_presses() == 0, || "a click on the list reached the page".into())?;
    Ok("typed \"ap\": apple, apricot under the entry, which kept the keyboard; Down Down Enter picked apricot; Backspace to \"a\", a click on avocado over the page picked it".into())
}
