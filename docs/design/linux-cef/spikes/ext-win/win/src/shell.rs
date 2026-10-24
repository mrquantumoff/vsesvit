//! The GPUI side: one browser window with a tab strip, a toolbar and a content area that
//! hosts one Chrome-style CEF browser per tab as a native child HWND.
use crate::cefglue::{self, CefEvent, PumpMode, Shared, ShellAction, on_ui};
use crate::ctl::{CtlMsg, Reply};
use crate::platform;
use crate::pump::PumpStats;
use crate::win::{self, Hwnd, PRect};
use cef::{ImplBrowser as _, ImplBrowserHost as _, ImplFileDialogCallback as _, ImplFrame as _, ImplJsdialogCallback as _, ImplPermissionPromptCallback as _};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::prelude::*;
use gpui::{
    AnyWindowHandle, App, Bounds, Context, Entity, FocusHandle, Focusable, KeyBinding, MouseButton, Pixels, SharedString, Subscription, Window, WindowBounds,
    WindowKind, WindowOptions, actions, anchored, canvas, deferred, div, point, px, rgb, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Disableable as _, Root};
use raw_window_handle::RawWindowHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

actions!(spike, [FocusOmnibox, NewTab, CloseTab, Reload, Back, Forward, NextTab, Quit]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-l", FocusOmnibox, Some("BrowserWindow")),
        KeyBinding::new("alt-d", FocusOmnibox, Some("BrowserWindow")),
        KeyBinding::new("ctrl-t", NewTab, Some("BrowserWindow")),
        KeyBinding::new("ctrl-w", CloseTab, Some("BrowserWindow")),
        KeyBinding::new("f5", Reload, Some("BrowserWindow")),
        KeyBinding::new("alt-left", Back, Some("BrowserWindow")),
        KeyBinding::new("alt-right", Forward, Some("BrowserWindow")),
        KeyBinding::new("ctrl-tab", NextTab, Some("BrowserWindow")),
        KeyBinding::new("ctrl-q", Quit, Some("BrowserWindow")),
    ]);
}

#[derive(Default)]
struct Tab {
    id: Option<i32>,
    browser: Option<cef::Browser>,
    hwnd: Hwnd,
    title: String,
    url: String,
    loading: bool,
    can_back: bool,
    can_fwd: bool,
    favicon: Option<String>,
    closing: bool,
    /// Created by Chrome itself (not a child of our window).
    external: bool,
}

#[derive(Default)]
struct Layout {
    /// Content area in GPUI logical pixels (client-relative).
    content: Option<Bounds<Pixels>>,
    scale: f32,
    /// Parent of every CEF child: the content host, or the GPUI HWND in direct mode.
    host: Hwnd,
    direct: bool,
    applied_host: Option<PRect>,
    applied: Option<(Hwnd, PRect)>,
    /// Region cut-out currently applied: (window, hole) in that window's physical px.
    region: Option<(Hwnd, PRect)>,
}

impl Layout {
    /// Rectangle of the content area in physical px, GPUI-client-relative.
    fn content_px(&self) -> Option<PRect> {
        let b = self.content?;
        let s = self.scale;
        Some(PRect {
            x: (f32::from(b.origin.x) * s).round() as i32,
            y: (f32::from(b.origin.y) * s).round() as i32,
            w: (f32::from(b.size.width) * s).round().max(1.) as i32,
            h: (f32::from(b.size.height) * s).round().max(1.) as i32,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyFocus {
    /// GPUI chrome: Win32 focus on the GPUI toplevel HWND.
    Chrome,
    /// The active tab's CEF child HWND.
    Page,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuMode {
    Off,
    /// In-window GPUI overlay; the host gets a window region with a hole for it.
    Region,
    /// In-window GPUI overlay with no region (to show the problem).
    Plain,
    /// Separate `WindowKind::PopUp` GPUI window above the page.
    Popup,
}

pub struct BrowserWindow {
    shared: Arc<Shared>,
    client: cef::Client,
    gpui_hwnd: Hwnd,
    key_focus: KeyFocus,
    tabs: Vec<Tab>,
    active: usize,
    closed_urls: Vec<String>,
    address: Entity<InputState>,
    focus: FocusHandle,
    layout: Rc<RefCell<Layout>>,
    menu: MenuMode,
    popup: Option<(AnyWindowHandle, Hwnd)>,
    extra_bar: bool,
    closing_all: bool,
    cef_ready: bool,
    page_focused: bool,
    last_active: Option<bool>,
    cap_wgc: bool,
    held_mods: Vec<win::ModGuard>,
    other: Hwnd,
    /// Extension popup host (AuxHost): a child of the GPUI window above the content host.
    aux: Hwnd,
    aux_browser: Option<cef::Browser>,
    aux_child: Hwnd,
    /// Right edge and top of the popup in GPUI client physical px.
    aux_anchor: (i32, i32),
    stats: Arc<PumpStats>,
    stop_pump: Arc<AtomicBool>,
    _subs: Vec<Subscription>,
}

pub struct ShellInit {
    pub shared: Arc<Shared>,
    pub events: UnboundedReceiver<CefEvent>,
    pub ctl: Option<UnboundedReceiver<CtlMsg>>,
    pub stats: Arc<PumpStats>,
    pub stop_pump: Arc<AtomicBool>,
}

fn env_pair(key: &str, sep: char, default: (f32, f32)) -> (f32, f32) {
    std::env::var(key)
        .ok()
        .and_then(|v| v.split_once(sep).and_then(|(a, b)| Some((a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?))))
        .unwrap_or(default)
}

/// GPUI's DirectComposition target is created with topmost=true (directx_renderer.rs
/// CreateTargetForHwnd(hwnd, true)): its visuals compose above every child HWND. The page
/// shows only through pixels GPUI leaves transparent. SPIKE_HOLE=0 keeps the opaque window.
pub fn hole_mode() -> bool {
    std::env::var("SPIKE_HOLE").map(|v| v != "0").unwrap_or(true)
}

pub fn open(cx: &mut App, init: ShellInit) {
    bind_keys(cx);
    if hole_mode() {
        let clear = gpui::transparent_black();
        let theme = gpui_component::Theme::global_mut(cx);
        theme.tokens.background = gpui_component::ThemeToken::new(clear, clear.into());
        theme.colors.background = clear;
    }
    let (w, h) = env_pair("SPIKE_WIN", 'x', (1280., 800.));
    // Created hidden; `BrowserWindow::new` parks it off-screen and shows it without
    // activation, so nothing appears on the owner's screen and no foreground changes.
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin: point(px(100.), px(100.)), size: size(px(w), px(h)) })),
        titlebar: Some(gpui::TitlebarOptions { title: Some("Vsesvit spike".into()), ..Default::default() }),
        show: false,
        focus: false,
        inactive_frame_interval: None,
        window_background: if hole_mode() { gpui::WindowBackgroundAppearance::Transparent } else { gpui::WindowBackgroundAppearance::Opaque },
        ..Default::default()
    };
    cx.open_window(opts, |window, cx| {
        let view = cx.new(|cx| BrowserWindow::new(init, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })
    .expect("open window");
}

pub fn hwnd_of(window: &Window) -> Hwnd {
    match raw_window_handle::HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        other => panic!("GPUI is not on Win32: {other:?}"),
    }
}

impl BrowserWindow {
    fn new(init: ShellInit, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let gpui_hwnd = hwnd_of(window);
        let scale = window.scale_factor();
        let (w, h) = env_pair("SPIKE_WIN", 'x', (1280., 800.));
        let (x, y) = env_pair("SPIKE_POS", ',', (-4000., -4000.));
        // Outer size: client size plus a frame guess; GPUI reads the real client size from WM_SIZE.
        win::set_size(gpui_hwnd, (w * scale).round() as i32 + 16, (h * scale).round() as i32 + 40);
        win::move_to(gpui_hwnd, x as i32, y as i32);
        win::show_noactivate(gpui_hwnd);
        let direct = std::env::var("SPIKE_HOST").is_ok_and(|v| v == "direct");
        let host = if direct { gpui_hwnd } else { win::create_host(gpui_hwnd, PRect { x: 0, y: 0, w: 1, h: 1 }).expect("host window") };
        crate::log(&format!(
            "gpui: window hwnd=0x{gpui_hwnd:x} scale={scale} dpi={} exstyle=0x{:x} rect={:?} host=0x{host:x} direct={direct}",
            win::dpi_for_window(gpui_hwnd),
            win::ex_style(gpui_hwnd),
            win::window_rect(gpui_hwnd)
        ));
        init.shared.parent_handle.store(host as u64, Ordering::Relaxed);
        let hidden = platform::create_hidden_host(gpui_hwnd);
        init.shared.ext.hidden_parent.store(hidden as u64, Ordering::Relaxed);
        platform::start_winevent_log();
        crate::log(&format!("gpui: hidden host 0x{hidden:x}"));
        let address = cx.new(|cx| InputState::new(window, cx).placeholder("Search or enter address"));
        let mut subs = vec![cx.subscribe_in(&address, window, |this, state, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { .. } => {
                let url = state.read(cx).value().to_string();
                this.navigate(&url, window, cx);
            }
            InputEvent::Focus => crate::log("gpui: address input focused"),
            InputEvent::Blur => crate::log("gpui: address input blurred"),
            _ => {}
        })];
        subs.push(cx.observe_window_activation(window, |this, window, cx| {
            let active = window.is_window_active();
            crate::log(&format!("gpui: window activation changed active={active} focus=0x{:x} active_window=0x{:x}", win::focus(), win::active_window()));
            if active {
                // DefWindowProc put Win32 focus on the toplevel; hand it to whoever owns the keyboard.
                this.apply_focus();
            }
            cx.notify();
        }));
        {
            let entity = cx.entity().downgrade();
            window.on_window_should_close(cx, move |_window, cx| {
                entity
                    .update(cx, |this, cx| {
                        if this.tabs.is_empty() {
                            true
                        } else {
                            crate::log("gpui: close button pressed; closing browsers first");
                            this.begin_quit(cx);
                            false
                        }
                    })
                    .unwrap_or(true)
            });
        }
        let mut events = init.events;
        cx.spawn_in(window, async move |this, cx| {
            while let Some(ev) = events.next().await {
                if this.update_in(cx, |this, window, cx| this.on_cef(ev, window, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        if let Some(mut ctl) = init.ctl {
            cx.spawn_in(window, async move |this, cx| {
                while let Some(msg) = ctl.next().await {
                    if this.update_in(cx, |this, window, cx| this.on_ctl_msg(msg, window, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
        }
        let client = cefglue::client::new_client(init.shared.clone());
        // Activation only where the user cannot see it (win::init_desktop_guard). On the
        // user's desktop every SetFocus is refused by win::set_focus anyway.
        if win::may_activate() && std::env::var("SPIKE_ACTIVATE").map(|v| v != "0").unwrap_or(true) {
            let prev = win::set_focus(gpui_hwnd);
            crate::log(&format!("gpui: SetFocus(toplevel) prev=0x{prev:x} now=0x{:x} active=0x{:x}", win::focus(), win::active_window()));
        } else {
            crate::log(&format!("gpui: activation skipped (may_activate={})", win::may_activate()));
        }
        let watchdog = std::env::var("SPIKE_WATCHDOG").map(|v| v != "0").unwrap_or(true);
        cx.spawn_in(window, async move |_, cx| {
            if !watchdog {
                return;
            }
            let mut reported = false;
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
                if let Some(msg) = win::safety_violation() {
                    if !reported {
                        crate::log(&format!("SAFETY: {msg}"));
                        reported = true;
                    }
                }
            }
        })
        .detach();
        Self {
            shared: init.shared,
            client,
            gpui_hwnd,
            key_focus: KeyFocus::Page,
            tabs: Vec::new(),
            active: 0,
            closed_urls: Vec::new(),
            address,
            focus: cx.focus_handle(),
            layout: Rc::new(RefCell::new(Layout { scale, host, direct, ..Default::default() })),
            menu: MenuMode::Off,
            popup: None,
            extra_bar: false,
            closing_all: false,
            cef_ready: false,
            page_focused: false,
            last_active: None,
            cap_wgc: std::env::var("SPIKE_CAPTURE").is_ok_and(|v| v == "wgc"),
            held_mods: Vec::new(),
            other: 0,
            aux: 0,
            aux_browser: None,
            aux_child: 0,
            aux_anchor: (0, 0),
            stats: init.stats,
            stop_pump: init.stop_pump,
            _subs: subs,
        }
    }

    // ---------------------------------------------------------------- tabs

    fn host(&self) -> Hwnd {
        self.layout.borrow().host
    }

    /// Initial CEF bounds in DIP, relative to the parent window.
    fn content_rect_dip(&self) -> cef::Rect {
        let l = self.layout.borrow();
        match l.content {
            Some(b) => cef::Rect {
                x: if l.direct { f32::from(b.origin.x) as i32 } else { 0 },
                y: if l.direct { f32::from(b.origin.y) as i32 } else { 0 },
                width: (f32::from(b.size.width) as i32).max(1),
                height: (f32::from(b.size.height) as i32).max(1),
            },
            None => cef::Rect { x: 0, y: 0, width: 1280, height: 700 },
        }
    }

    fn new_tab(&mut self, url: &str, cx: &mut Context<Self>) {
        self.new_tab_in(url, None, cx)
    }

    fn new_tab_in(&mut self, url: &str, mut ctx: Option<cef::RequestContext>, cx: &mut Context<Self>) {
        let rect = self.content_rect_dip();
        let wi = cef::WindowInfo { runtime_style: cef::RuntimeStyle::CHROME, ..Default::default() }
            .set_as_child(cef::sys::HWND(self.host() as usize as *mut _), &rect);
        let settings = cef::BrowserSettings::default();
        let mut client = self.client.clone();
        let private = ctx.is_some();
        let ok = cef::browser_host_create_browser(Some(&wi), Some(&mut client), Some(&cef::CefString::from(url)), Some(&settings), None, ctx.as_mut());
        crate::log(&format!("gpui: create_browser url={url} private_ctx={private} rect={}x{}+{}+{} -> {ok}", rect.width, rect.height, rect.x, rect.y));
        self.tabs.push(Tab { url: url.into(), title: "Loading…".into(), ..Default::default() });
        cx.notify();
    }

    fn browser(&self) -> Option<&cef::Browser> {
        self.tabs.get(self.active).and_then(|t| t.browser.as_ref())
    }

    fn host_of(t: &Tab) -> Option<cef::BrowserHost> {
        t.browser.as_ref().and_then(|b| b.host())
    }

    fn activate(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if idx >= self.tabs.len() {
            return;
        }
        for (i, t) in self.tabs.iter().enumerate() {
            if i != idx && t.hwnd != 0 && !t.external {
                win::show(t.hwnd, false);
                if let Some(h) = Self::host_of(t) {
                    on_ui(move || h.was_hidden(1));
                }
            }
        }
        self.active = idx;
        let t = &self.tabs[idx];
        if let Some(id) = t.id {
            *self.shared.ext.active.lock().unwrap() = format!("b{id}");
        }
        if t.hwnd != 0 && !t.external {
            self.layout.borrow_mut().applied = None;
            self.apply_geometry_now();
            win::show(t.hwnd, true);
            if let Some(h) = Self::host_of(t) {
                on_ui(move || h.was_hidden(0));
            }
            self.apply_focus();
        }
        let url = t.url.clone();
        self.address.update(cx, |a, cx| a.set_value(url, window, cx));
        crate::log(&format!("gpui: activated tab {idx} id={:?}", self.tabs[idx].id));
        cx.notify();
    }

    fn apply_geometry_now(&self) {
        let hwnd = self.tabs.get(self.active).map(|t| t.hwnd).unwrap_or(0);
        apply_geometry(&self.layout, hwnd, &self.shared);
    }

    fn close_tab(&mut self, idx: usize) {
        if let Some(t) = self.tabs.get_mut(idx) {
            if let Some(h) = Self::host_of(t) {
                t.closing = true;
                self.closed_urls.push(t.url.clone());
                on_ui(move || h.close_browser(0));
                crate::log(&format!("gpui: close_browser(0) tab {idx} id={:?}", t.id));
            }
        }
    }

    fn begin_quit(&mut self, cx: &mut Context<Self>) {
        self.closing_all = true;
        for t in &mut self.tabs {
            t.closing = true;
        }
        let n = cefglue::ext::close_all(&self.shared);
        crate::log(&format!("gpui: quit: closing {n} browsers"));
        if cefglue::ext::live(&self.shared) == 0 {
            self.tabs.clear();
            self.finish_quit(cx);
        }
    }

    fn finish_quit(&mut self, cx: &mut Context<Self>) {
        crate::log("gpui: all browsers closed; quitting GPUI");
        self.stop_pump.store(true, Ordering::SeqCst);
        if let Some((p, _)) = self.popup.take() {
            let _ = p.update(cx, |_, w, _| w.remove_window());
        }
        cx.quit();
    }

    fn navigate(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let url = if text.contains("://") || text.starts_with("about:") || text.starts_with("data:") {
            text.to_string()
        } else if text.contains('.') && !text.contains(' ') {
            format!("https://{text}")
        } else {
            format!("https://duckduckgo.com/?q={}", text.replace(' ', "+"))
        };
        if let Some(f) = self.browser().and_then(|b| b.main_frame()) {
            self.focus_page(window, cx);
            on_ui(move || f.load_url(Some(&cef::CefString::from(url.as_str()))));
        } else {
            self.new_tab(&url, cx);
        }
    }

    // ---------------------------------------------------------------- focus

    fn set_key_focus(&mut self, f: KeyFocus) {
        if self.key_focus != f {
            crate::log(&format!("focus: key focus {:?} -> {f:?}", self.key_focus));
        }
        self.key_focus = f;
        self.shared.page_focus_allowed.store(f == KeyFocus::Page && win::may_activate(), Ordering::SeqCst);
        self.page_focused = f == KeyFocus::Page;
        self.apply_focus();
    }

    /// Put Win32 keyboard focus where `key_focus` says: the GPUI toplevel or the child.
    fn apply_focus(&self) {
        let target = match self.key_focus {
            KeyFocus::Chrome => self.gpui_hwnd,
            KeyFocus::Page => match self.tabs.get(self.active) {
                Some(t) if t.hwnd != 0 && !t.external => t.hwnd,
                _ => self.gpui_hwnd,
            },
        };
        // Only move focus within our toplevel. Another toplevel on this thread (a native
        // Chrome window) holding focus is left alone.
        let current = win::focus();
        if current == target {
        } else if current == 0 || win::is_within(current, self.gpui_hwnd) {
            let fg_before = win::foreground_window();
            win::set_focus(target);
            crate::log(&format!("focus: SetFocus(0x{target:x}) -> focus=0x{:x} active_window=0x{:x} foreground before=0x{fg_before:x} after=0x{:x}", win::focus(), win::active_window(), win::foreground_window()));
        } else {
            crate::log(&format!("focus: not stealing focus from 0x{current:x} (not ours)"));
            return;
        }
        if self.key_focus == KeyFocus::Page && win::may_activate() {
            if let Some(h) = self.browser().and_then(|b| b.host()) {
                on_ui(move || h.set_focus(1));
            }
        }
    }

    fn focus_omnibox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_key_focus(KeyFocus::Chrome);
        self.address.update(cx, |a, cx| {
            a.focus(window, cx);
            a.select_all(window, cx);
        });
        crate::log(&format!("gpui: focus_omnibox -> focus=0x{:x}", win::focus()));
        cx.notify();
    }

    fn focus_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.set_key_focus(KeyFocus::Page);
        cx.notify();
    }

    // ---------------------------------------------------------------- CEF events

    fn on_cef(&mut self, ev: CefEvent, window: &mut Window, cx: &mut Context<Self>) {
        let find = |tabs: &Vec<Tab>, id: i32| tabs.iter().position(|t| t.id == Some(id));
        match ev {
            CefEvent::ContextInitialized => {
                self.cef_ready = true;
                let urls = std::env::var("SPIKE_URLS").unwrap_or_else(|_| "https://example.org".into());
                for u in urls.split_whitespace() {
                    self.new_tab(u, cx);
                }
            }
            CefEvent::AfterCreated { id, browser, is_popup, handle, opener_id, kind } => {
                let hwnd = handle as Hwnd;
                if kind == "auxpopup" {
                    self.aux_child = hwnd;
                    self.aux_browser = Some(browser.clone());
                    crate::log(&format!("gpui: popup browser id={id} hwnd=0x{hwnd:x} in aux 0x{:x} parent=0x{:x}", self.aux, win::parent(hwnd)));
                    if let Some(h) = browser.host() {
                        on_ui(move || h.set_auto_resize_enabled(1, Some(&cef::Size { width: 25, height: 25 }), Some(&cef::Size { width: 800, height: 600 })));
                    }
                    return;
                }
                if kind != "tab" {
                    return;
                }
                let slot = (!is_popup).then(|| self.tabs.iter().position(|t| t.id.is_none())).flatten();
                let ours = slot.is_some() || is_popup;
                let idx = slot.unwrap_or_else(|| {
                    self.tabs.push(Tab { title: "Popup".into(), ..Default::default() });
                    self.tabs.len() - 1
                });
                let parent = win::parent(hwnd);
                let (tid, _) = win::thread_of(hwnd);
                crate::log(&format!(
                    "gpui: tab {idx} <- browser id={id} popup={is_popup} opener={opener_id} hwnd=0x{hwnd:x} parent=0x{parent:x} (ours={}) class={:?} tid={tid} main_tid={}",
                    parent == self.host(),
                    win::class_name(hwnd),
                    win::thread_of(self.gpui_hwnd).0
                ));
                let external = !ours || parent != self.host();
                let t = &mut self.tabs[idx];
                t.id = Some(id);
                t.browser = Some(browser);
                t.hwnd = hwnd;
                t.external = external;
                self.activate(idx, window, cx);
            }
            CefEvent::BeforeClose { id } => {
                if self.aux_browser.as_ref().is_some_and(|b| b.identifier() == id) {
                    self.aux_browser = None;
                    self.aux_child = 0;
                    win::show(self.aux, false);
                    crate::log("gpui: popup closed");
                }
                if let Some(i) = find(&self.tabs, id) {
                    let t = self.tabs.remove(i);
                    drop(t.browser);
                    if self.active >= self.tabs.len() && !self.tabs.is_empty() {
                        self.active = self.tabs.len() - 1;
                    } else if i < self.active {
                        self.active -= 1;
                    }
                    let mut l = self.layout.borrow_mut();
                    l.applied = None;
                    l.region = None;
                    drop(l);
                    if !self.tabs.is_empty() && !self.closing_all {
                        let a = self.active;
                        self.activate(a, window, cx);
                    }
                }
                crate::log(&format!("gpui: tabs left {}", self.tabs.len()));
                if self.closing_all && cefglue::ext::live(&self.shared) == 0 {
                    self.tabs.clear();
                    self.finish_quit(cx);
                }
                cx.notify();
            }
            CefEvent::Title { id, title } => {
                crate::log(&format!("cef-ev: title id={id} {title:?}"));
                if let Some(i) = find(&self.tabs, id) {
                    self.tabs[i].title = title;
                }
                cx.notify();
            }
            CefEvent::Address { id, url } => {
                crate::log(&format!("cef-ev: address id={id} {url}"));
                if let Some(i) = find(&self.tabs, id) {
                    self.tabs[i].url = url.clone();
                    let focused = self.address.read(cx).focus_handle(cx).is_focused(window);
                    if i == self.active && !focused {
                        self.address.update(cx, |a, cx| a.set_value(url, window, cx));
                    }
                }
                cx.notify();
            }
            CefEvent::Favicon { id, urls } => {
                crate::log(&format!("cef-ev: favicon id={id} {urls:?}"));
                if let Some(i) = find(&self.tabs, id) {
                    self.tabs[i].favicon = urls.into_iter().next();
                }
                cx.notify();
            }
            CefEvent::LoadingState { id, loading, can_back, can_fwd } => {
                crate::log(&format!("cef-ev: loading id={id} loading={loading} back={can_back} fwd={can_fwd}"));
                if let Some(i) = find(&self.tabs, id) {
                    let t = &mut self.tabs[i];
                    (t.loading, t.can_back, t.can_fwd) = (loading, can_back, can_fwd);
                }
                cx.notify();
            }
            CefEvent::LoadEnd { id, status } => crate::log(&format!("cef-ev: load_end id={id} status={status}")),
            CefEvent::LoadError { id, code, text, url } => crate::log(&format!("cef-ev: load_error id={id} code={code} {text} {url}")),
            CefEvent::Fullscreen { id, on } => {
                crate::log(&format!("cef-ev: fullscreen id={id} on={on} (gpui fullscreen={})", window.is_fullscreen()));
                if on != window.is_fullscreen() {
                    self.toggle_fullscreen(window);
                }
            }
            CefEvent::GotFocus { id } => {
                crate::log(&format!("cef-ev: got_focus id={id} focus=0x{:x}", win::focus()));
                window.focus(&self.focus, cx);
                self.set_key_focus(KeyFocus::Page);
                cx.notify();
            }
            CefEvent::TakeFocus { id, next } => {
                crate::log(&format!("cef-ev: take_focus id={id} next={next}"));
                self.focus_omnibox(window, cx);
            }
            CefEvent::SetFocus { id, source } => crate::log(&format!("cef-ev: set_focus id={id} source={source}")),
            CefEvent::Action { id, action, via } => {
                crate::log(&format!("cef-ev: action {action:?} from page id={id} via {via}"));
                self.do_action(action, window, cx);
            }
            CefEvent::PopupAsTab { opener_id, url } => {
                crate::log(&format!("cef-ev: popup from {opener_id} opened as tab: {url}"));
                self.new_tab(&url, cx);
            }
            CefEvent::JsDialog { id, kind, message, default_text, callback } => {
                crate::log(&format!("cef-ev: js dialog id={id} kind={kind} msg={message:?} default={default_text:?} -> auto-accept"));
                let answer = cef::CefString::from("spike-answer");
                callback.cont(1, if kind == 2 { Some(&answer) } else { None });
            }
            CefEvent::FileDialog { id, mode, title, callback } => {
                let pick = format!("{}\\picked.txt", self.shared.download_dir);
                let _ = std::fs::write(&pick, "picked");
                crate::log(&format!("cef-ev: file dialog id={id} mode={mode} title={title:?} -> choose {pick}"));
                let mut paths = cef::CefStringList::new();
                paths.append(&pick);
                callback.cont(Some(&mut paths));
            }
            CefEvent::Download { id, name, url } => crate::log(&format!("cef-ev: download id={id} {name} {url}")),
            CefEvent::PermissionPrompt { id, prompt_id, origin, perms, callback } => {
                crate::log(&format!("cef-ev: permission prompt id={id} prompt={prompt_id} origin={origin} perms=0x{perms:x} -> ACCEPT"));
                callback.cont(cef::PermissionRequestResult::ACCEPT);
            }
            CefEvent::OpenTab { url, ctx, why } => {
                crate::log(&format!("gpui: open tab {url} ({why}) private_ctx={}", ctx.is_some()));
                self.new_tab_in(&url, ctx, cx);
            }
            CefEvent::AutoResize { id, w, h } => {
                if self.aux_browser.as_ref().is_some_and(|b| b.identifier() == id) && w > 0 && h > 0 {
                    let s = self.layout.borrow().scale;
                    let (pw, ph) = ((w as f32 * s).round() as i32, (h as f32 * s).round() as i32);
                    let (right, top) = self.aux_anchor;
                    let r = PRect { x: right - pw, y: top, w: pw, h: ph };
                    win::set_rect(self.aux, r);
                    win::set_rect(self.aux_child, PRect { x: 0, y: 0, w: pw, h: ph });
                    crate::log(&format!("gpui: popup auto-resize {w}x{h} DIP -> aux {r:?}"));
                }
            }
            CefEvent::ContextMenu { id, callback, .. } => {
                crate::log(&format!("gpui: context menu id={id} cancelled (spike)"));
                cef::ImplRunContextMenuCallback::cancel(&callback);
            }
            CefEvent::Console { id, message } => {
                crate::log(&format!("cef-ev: console id={id} {message}"));
                // `clickel` asks the page for an element centre; click it like a mouse would.
                if let Some(rest) = message.strip_prefix("spike-click ") {
                    let (x, y) = env_pair_str(rest, (0., 0.));
                    let s = self.layout.borrow().scale;
                    let target = self.active_child();
                    crate::log(&format!("ctl: click page ({x},{y}) -> 0x{target:x}"));
                    win::send_click(target, (x * s).round() as i32, (y * s).round() as i32);
                }
            }
        }
    }

    fn do_action(&mut self, action: ShellAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            ShellAction::FocusOmnibox => self.focus_omnibox(window, cx),
            ShellAction::NewTab | ShellAction::NewWindow => {
                self.new_tab("about:blank", cx);
                self.focus_omnibox(window, cx);
            }
            ShellAction::CloseTab => self.close_tab(self.active),
            ShellAction::ReopenTab => {
                if let Some(u) = self.closed_urls.pop() {
                    self.new_tab(&u, cx);
                }
            }
            ShellAction::Fullscreen => self.toggle_fullscreen(window),
            ShellAction::Reload => {
                if let Some(b) = self.browser().cloned() {
                    on_ui(move || b.reload());
                }
            }
            ShellAction::Back => {
                if let Some(b) = self.browser().cloned() {
                    on_ui(move || b.go_back());
                }
            }
            ShellAction::Forward => {
                if let Some(b) = self.browser().cloned() {
                    on_ui(move || b.go_forward());
                }
            }
            ShellAction::NextTab | ShellAction::PrevTab => {
                if !self.tabs.is_empty() {
                    let n = self.tabs.len();
                    let i = if action == ShellAction::NextTab { (self.active + 1) % n } else { (self.active + n - 1) % n };
                    self.activate(i, window, cx);
                }
            }
            ShellAction::Print | ShellAction::Find | ShellAction::DevTools | ShellAction::ViewSource => {
                crate::log(&format!("gpui: shell would handle {action:?} here (spike: no UI)"));
            }
        }
    }

    // ---------------------------------------------------------------- control channel

    /// Whole-window capture with the selected backend: (w, h, rgba, time).
    fn capture(&self) -> anyhow::Result<(u32, u32, Vec<u8>, std::time::Duration)> {
        if self.cap_wgc {
            anyhow::bail!("wgc not built in this spike")
        } else {
            let t = std::time::Instant::now();
            let (w, h, px) = win::capture(self.gpui_hwnd, win::PW_FULL)?;
            Ok((w, h, px, t.elapsed()))
        }
    }

    fn active_child(&self) -> Hwnd {
        self.tabs.get(self.active).filter(|t| !t.external).map(|t| t.hwnd).unwrap_or(0)
    }

    fn on_ctl_msg(&mut self, msg: CtlMsg, window: &mut Window, cx: &mut Context<Self>) {
        let CtlMsg { line, reply } = msg;
        crate::log(&format!("ctl> {line}"));
        if cefglue::ext::ctl(&self.shared, &line, &reply) {
            return;
        }
        let (cmd, arg) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        match cmd {
            "newp" => {
                let ctx = cefglue::ext::otr(&self.shared);
                self.new_tab_in(if arg.is_empty() { "about:blank" } else { arg }, Some(ctx), cx);
                reply.send("ok");
            }
            "tabs" => {
                let list: Vec<String> = self.tabs.iter().enumerate().map(|(i, t)| format!("{i}:b{}:{}", t.id.unwrap_or(-1), t.url)).collect();
                reply.send(format!("OK active={} {}", self.active, list.join(" | ")));
            }
            "popup" => self.open_popup(arg, reply),
            "popupclose" => {
                match self.aux_browser.as_ref().and_then(|b| b.host()) {
                    Some(h) => {
                        on_ui(move || h.close_browser(1));
                        reply.send("ok");
                    }
                    None => reply.send("ERR no popup"),
                }
            }
            "popupfocus" => {
                if self.aux_child != 0 {
                    let prev = win::set_focus(self.aux_child);
                    if let Some(h) = self.aux_browser.as_ref().and_then(|b| b.host()) {
                        on_ui(move || h.set_focus(1));
                    }
                    reply.send(format!("OK prev=0x{prev:x} focus=0x{:x}", win::focus()));
                } else {
                    reply.send("ERR no popup");
                }
            }
            "popupinfo" => {
                let z = win::children(self.gpui_hwnd);
                let order: Vec<String> = z.iter().map(|h| format!("0x{h:x}:{}", win::class_name(*h))).collect();
                reply.send(format!(
                    "OK aux=0x{:x} rect={:?} visible={} child=0x{:x} child_rect={:?} host=0x{:x} host_rect={:?} children_top_first=[{}]",
                    self.aux, win::window_rect(self.aux), win::is_visible(self.aux), self.aux_child, win::window_rect(self.aux_child), self.host(), win::window_rect(self.host()), order.join(", ")
                ));
            }
            "wtops" => {
                let lines: Vec<String> = win::process_toplevels().into_iter().map(|h| format!("{} owner=0x{:x}", win::describe(h), win::owner(h))).collect();
                reply.send(format!("OK {}", lines.join(" || ")));
            }
            // wshot HWND PATH: PrintWindow of one of our windows.
            "wshot" => {
                let (h, path) = arg.split_once(' ').unwrap_or((arg, "w.png"));
                let h = Hwnd::from_str_radix(h.trim_start_matches("0x"), 16).unwrap_or(0);
                if !platform::ours(h) {
                    reply.send("ERR not our window");
                    return;
                }
                match win::capture(h, win::PW_FULL) {
                    Ok((w, hh, px)) => {
                        let _ = win::write_png(path, w, hh, &px);
                        reply.send(format!("OK {w}x{hh} -> {path}"));
                    }
                    Err(e) => reply.send(format!("ERR {e:#}")),
                }
            }
            // wkey HWND KEY [ctrl] [shift] [alt]: posted key to one of our windows.
            "wkey" => {
                let mut it = arg.split_whitespace();
                let h = Hwnd::from_str_radix(it.next().unwrap_or("0").trim_start_matches("0x"), 16).unwrap_or(0);
                let Some(vk) = it.next().and_then(win::vk_from_name) else {
                    reply.send("ERR key");
                    return;
                };
                if !platform::ours(h) {
                    reply.send("ERR not our window");
                    return;
                }
                let mods: Vec<&str> = it.collect();
                let g = win::post_key(h, vk, mods.contains(&"ctrl"), mods.contains(&"shift"), mods.contains(&"alt"));
                self.held_mods.push(g);
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
                    let _ = this.update(cx, |this, _| this.held_mods.clear());
                })
                .detach();
                reply.send("ok");
            }
            // wclick HWND X Y: physical client px of one of our windows.
            "wclick" => {
                let mut it = arg.split_whitespace();
                let h = Hwnd::from_str_radix(it.next().unwrap_or("0").trim_start_matches("0x"), 16).unwrap_or(0);
                let x: i32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let y: i32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                if !platform::ours(h) {
                    reply.send("ERR not our window");
                    return;
                }
                win::send_click(h, x, y);
                reply.send("ok");
            }
            "wclose" => {
                let h = Hwnd::from_str_radix(arg.trim_start_matches("0x"), 16).unwrap_or(0);
                if platform::ours(h) {
                    win::post_close(h);
                    reply.send("ok");
                } else {
                    reply.send("ERR not our window");
                }
            }
            // popupprobe: colour at the popup's centre in a PrintWindow capture of the GPUI window,
            // and which of our windows a press there would hit.
            "popupprobe" => {
                let r = win::window_rect(self.aux);
                let (cx0, cy0) = win::client_origin(self.gpui_hwnd);
                let (px_, py_) = (r.x + r.w / 2 - cx0, r.y + r.h / 2 - cy0);
                let wr = win::window_rect(self.gpui_hwnd);
                let hit = win::deepest_child_at(self.gpui_hwnd, px_, py_);
                let sys = win::window_from_client_point(self.gpui_hwnd, px_, py_);
                let col = match win::capture(self.gpui_hwnd, win::PW_FULL) {
                    Ok((w, _, pxs)) => {
                        let c = win::pixel(&pxs, w, px_ + cx0 - wr.x, py_ + cy0 - wr.y);
                        format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
                    }
                    Err(e) => format!("ERR {e:#}"),
                };
                reply.send(format!("OK centre_client=({px_},{py_}) colour={col} hit=0x{hit:x}:{} window_from_point=0x{sys:x}:{} popup_child=0x{:x} aux=0x{:x}", win::class_name(hit), win::class_name(sys), self.aux_child, self.aux));
            }
            "activeinfo" => reply.send(format!("OK gpui_active={} focus=0x{:x} active_window=0x{:x} gpui=0x{:x}", window.is_window_active(), win::focus(), win::active_window(), self.gpui_hwnd)),
            _ => {
                self.on_ctl(&line, window, cx);
                reply.send("ok");
            }
        }
    }

    /// Extension popup page in an Alloy browser inside the AuxHost: a child of the GPUI
    /// window, above the content host, its right edge under the (imaginary) action button.
    fn open_popup(&mut self, url: &str, reply: Reply) {
        let Some(c) = self.layout.borrow().content_px() else {
            reply.send("ERR no content rect");
            return;
        };
        let right = c.x + c.w - 16;
        let top = c.y;
        self.aux_anchor = (right, top);
        let r = PRect { x: right - 360, y: top, w: 360, h: 300 };
        if self.aux == 0 {
            self.aux = win::create_host(self.gpui_hwnd, r).expect("aux host");
        }
        win::set_rect(self.aux, r);
        win::raise(self.aux);
        win::show(self.aux, true);
        let wi = cefglue::child_window_info(self.aux as u64, cef::Rect { x: 0, y: 0, width: 360, height: 300 }, cef::RuntimeStyle::ALLOY);
        let mut client = cefglue::client::new_client_kind(self.shared.clone(), "auxpopup");
        let ok = cef::browser_host_create_browser(Some(&wi), Some(&mut client), Some(&cef::CefString::from(url)), Some(&cef::BrowserSettings::default()), None, None);
        crate::log(&format!("gpui: popup aux=0x{:x} at {r:?} create_browser alloy {url} -> {ok}", self.aux));
        reply.send(format!("OK aux=0x{:x} create={ok}", self.aux));
    }

    fn on_ctl(&mut self, line: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
        match cmd {
            "new" => self.new_tab(if arg.is_empty() { "about:blank" } else { arg }, cx),
            "nav" => self.navigate(arg, window, cx),
            "switch" => self.activate(arg.parse().unwrap_or(0), window, cx),
            "close" => self.close_tab(arg.parse().unwrap_or(self.active)),
            "back" => self.do_action(ShellAction::Back, window, cx),
            "fwd" => self.do_action(ShellAction::Forward, window, cx),
            "reload" => self.do_action(ShellAction::Reload, window, cx),
            "stop" => {
                if let Some(b) = self.browser().cloned() {
                    on_ui(move || b.stop_load());
                }
            }
            "js" => {
                if let Some(f) = self.browser().and_then(|b| b.main_frame()) {
                    let code = arg.to_string();
                    on_ui(move || f.execute_java_script(Some(&cef::CefString::from(code.as_str())), Some(&cef::CefString::from("spike://ctl")), 1));
                }
            }
            "omnibox" => self.focus_omnibox(window, cx),
            "page" => self.focus_page(window, cx),
            // key NAME [ctrl] [shift] [alt] [to=page|chrome|focus]: synthetic key into one of our windows.
            "key" => {
                let mut it = arg.split_whitespace();
                let Some(vk) = it.next().and_then(win::vk_from_name) else {
                    crate::log("ctl: key: unknown key");
                    return;
                };
                let mods: Vec<&str> = it.collect();
                let to = mods.iter().find_map(|m| m.strip_prefix("to=")).unwrap_or("focus");
                let target = match to {
                    "page" => self.active_child(),
                    "chrome" => self.gpui_hwnd,
                    _ => win::focus(),
                };
                crate::log(&format!("ctl: key vk=0x{vk:x} mods={mods:?} -> 0x{target:x} ({:?})", win::class_name(target)));
                win::send_key(target, vk, mods.contains(&"ctrl"), mods.contains(&"shift"), mods.contains(&"alt"));
            }
            // keypost NAME [ctrl] [shift] [alt] [to=page|chrome|focus]: like `key`, but posted
            // through the message queue the way typed keys arrive.
            "keypost" => {
                let mut it = arg.split_whitespace();
                let Some(vk) = it.next().and_then(win::vk_from_name) else {
                    crate::log("ctl: keypost: unknown key");
                    return;
                };
                let mods: Vec<&str> = it.collect();
                let to = mods.iter().find_map(|m| m.strip_prefix("to=")).unwrap_or("focus");
                let target = match to {
                    "page" => self.active_child(),
                    "chrome" => self.gpui_hwnd,
                    _ => win::focus(),
                };
                crate::log(&format!("ctl: keypost vk=0x{vk:x} mods={mods:?} -> 0x{target:x} ({:?})", win::class_name(target)));
                let g = win::post_key(target, vk, mods.contains(&"ctrl"), mods.contains(&"shift"), mods.contains(&"alt"));
                self.held_mods.push(g);
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
                    let _ = this.update(cx, |this, _| this.held_mods.clear());
                })
                .detach();
            }
            "typepost" => {
                let target = win::focus();
                for c in arg.chars() {
                    if let Some(vk) = win::vk_from_name(&c.to_string()) {
                        drop(win::post_key(target, vk, false, false, false));
                    }
                }
            }
            "clickel" => {
                if let Some(f) = self.browser().and_then(|b| b.main_frame()) {
                    let js = format!("(function(){{var r=document.getElementById('{arg}').getBoundingClientRect();console.log('spike-click '+(r.x+r.width/2)+' '+(r.y+r.height/2))}})()");
                    on_ui(move || f.execute_java_script(Some(&cef::CefString::from(js.as_str())), Some(&cef::CefString::from("spike://ctl")), 0));
                }
            }
            "other" => {
                if self.other == 0 {
                    let r = win::window_rect(self.gpui_hwnd);
                    self.other = win::create_other_toplevel(r.x + 100, r.y + 100);
                }
                crate::log(&format!("other: {} active=0x{:x}", win::describe(self.other), win::active_window()));
            }
            "otherclose" => {
                if self.other != 0 {
                    win::destroy(self.other);
                    self.other = 0;
                }
            }
            "zorder" => {
                let popup = self.popup.as_ref().map(|p| p.1).unwrap_or(0);
                let z = win::z_positions(&[popup, self.other, self.gpui_hwnd]);
                crate::log(&format!(
                    "zorder: popup={:?} (topmost={}) other={:?} main={:?} active=0x{:x} gpui_active={}",
                    z[0],
                    popup != 0 && win::ex_style(popup) & 8 != 0,
                    z[1],
                    z[2],
                    win::active_window(),
                    window.is_window_active()
                ));
            }
            "type" => {
                let target = win::focus();
                for c in arg.chars() {
                    if let Some(vk) = win::vk_from_name(&c.to_string()) {
                        win::send_key(target, vk, false, c.is_ascii_uppercase(), false);
                    }
                }
            }
            // click page|chrome X Y (logical px, relative to that window's client area)
            "click" => {
                let mut it = arg.split_whitespace();
                let which = it.next().unwrap_or("page");
                let x: f32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(10.);
                let y: f32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(10.);
                let s = self.layout.borrow().scale;
                let target = if which == "chrome" { self.gpui_hwnd } else { self.active_child() };
                crate::log(&format!("ctl: click {which} ({x},{y}) -> 0x{target:x}"));
                win::send_click(target, (x * s).round() as i32, (y * s).round() as i32);
            }
            "menu" => {
                self.set_menu(
                    match arg {
                        "region" | "shape" => MenuMode::Region,
                        "plain" => MenuMode::Plain,
                        "popup" => MenuMode::Popup,
                        _ => MenuMode::Off,
                    },
                    window,
                    cx,
                );
            }
            // shotpopup PATH: PrintWindow of the PopUp overlay window.
            "shotpopup" => {
                if let Some((_, p)) = self.popup {
                    match win::capture(p, win::PW_FULL) {
                        Ok((w, h, px)) => {
                            let r = win::write_png(arg, w, h, &px);
                            crate::log(&format!("shotpopup 0x{p:x} {w}x{h} -> {arg} {r:?}"));
                        }
                        Err(e) => crate::log(&format!("shotpopup failed: {e:#}")),
                    }
                }
            }
            "popupfix" => {
                if let Some((_, p)) = self.popup {
                    crate::log(&format!("popup: before owner=0x{:x} exstyle=0x{:x}", win::owner(p), win::ex_style(p)));
                    win::set_owner(p, self.gpui_hwnd);
                    win::set_topmost(p, false);
                    crate::log(&format!("popup: after owner=0x{:x} exstyle=0x{:x} topmost={}", win::owner(p), win::ex_style(p), win::ex_style(p) & 8 != 0));
                }
            }
            "bar" => {
                self.extra_bar = !self.extra_bar;
                cx.notify();
            }
            // resize W H (logical client size). Not window.resize: GPUI's SetWindowPos there
            // lacks SWP_NOACTIVATE.
            "resize" => {
                let (w, h) = env_pair_str(arg, (1000., 650.));
                let s = window.scale_factor();
                let wr = win::window_rect(self.gpui_hwnd);
                let (cw, ch) = win::client_size(self.gpui_hwnd);
                let (ow, oh) = ((w * s).round() as i32 + wr.w - cw, (h * s).round() as i32 + wr.h - ch);
                let hwnd = self.gpui_hwnd;
                // SetWindowPos sends WM_SIZE synchronously. Made from inside this GPUI update,
                // GPUI drops it (SPIKE_RESIZE_INLINE=1 shows that); from a task it is seen.
                if std::env::var("SPIKE_RESIZE_INLINE").is_ok_and(|v| v == "1") {
                    win::set_size(hwnd, ow, oh);
                } else {
                    cx.spawn(async move |_, _| win::set_size(hwnd, ow, oh)).detach();
                }
            }
            "move" => {
                let (x, y) = env_pair_str(arg, (-4000., -4000.));
                win::move_to(self.gpui_hwnd, x as i32, y as i32);
                if let Some(h) = self.browser().and_then(|b| b.host()) {
                    on_ui(move || h.notify_move_or_resize_started());
                }
            }
            "fullscreen" => self.toggle_fullscreen(window),
            "unfullscreen" => {
                if window.is_fullscreen() {
                    self.toggle_fullscreen(window);
                }
            }
            "shot" | "shotpw" => {
                let path = if arg.is_empty() { "shot.png".to_string() } else { arg.to_string() };
                let r = if cmd == "shotpw" {
                    let t = std::time::Instant::now();
                    win::capture(self.gpui_hwnd, win::PW_FULL).map(|(w, h, px)| (w, h, px, t.elapsed()))
                } else {
                    self.capture()
                };
                match r {
                    Ok((w, h, px, took)) => {
                        let r = win::write_png(&path, w, h, &px);
                        crate::log(&format!("gpui: screenshot {cmd} wgc={} {w}x{h} in {took:?} -> {path} {r:?}", self.cap_wgc && cmd == "shot"));
                    }
                    Err(e) => crate::log(&format!("gpui: screenshot {cmd} failed: {e:#}")),
                }
            }
            // probe X Y: colour at a logical GPUI client point, from a fresh capture.
            "probe" => {
                let (x, y) = env_pair_str(arg, (0., 0.));
                let s = window.scale_factor();
                let wr = win::window_rect(self.gpui_hwnd);
                let co = win::client_origin(self.gpui_hwnd);
                let (px_, py_) = ((x * s).round() as i32 + co.0 - wr.x, (y * s).round() as i32 + co.1 - wr.y);
                match self.capture() {
                    Ok((w, _, px, _)) => {
                        let p = win::pixel(&px, w, px_, py_);
                        crate::log(&format!("probe ({x},{y}) = #{:02x}{:02x}{:02x}", p.0, p.1, p.2));
                    }
                    Err(e) => crate::log(&format!("probe failed: {e:#}")),
                }
            }
            // content: fraction of the content area that is not the host background colour.
            "content" => match self.capture() {
                Ok((w, _, px, took)) => {
                    let l = self.layout.borrow();
                    let Some(c) = l.content_px() else { return };
                    // Window-relative: offset by the client origin inside the outer rect.
                    let wr = win::window_rect(self.gpui_hwnd);
                    let co = win::client_origin(self.gpui_hwnd);
                    let rect = PRect { x: c.x + (co.0 - wr.x), y: c.y + (co.1 - wr.y), w: c.w, h: c.h };
                    let f = win::fraction_not(&px, w, rect, (0x1f, 0x29, 0x37));
                    crate::log(&format!("content: rect={rect:?} non_background={f:.3} capture={took:?} wgc={}", self.cap_wgc));
                }
                Err(e) => crate::log(&format!("content failed: {e:#}")),
            },
            "tree" => {
                let mut out = Vec::new();
                win::tree(self.gpui_hwnd, 0, &mut out);
                crate::log(&format!("win tree of 0x{:x}:\n{}", self.gpui_hwnd, out.join("\n")));
            }
            "tops" => {
                let tops = win::process_toplevels();
                let lines: Vec<String> = tops.iter().map(|h| win::describe(*h)).collect();
                crate::log(&format!("process toplevels ({}):\n{}", tops.len(), lines.join("\n")));
            }
            "res" => {
                let r = win::resources();
                crate::log(&format!(
                    "res: handles={} gdi={} user={} toplevels={} windows_under_gpui={}",
                    r.handles,
                    r.gdi,
                    r.user,
                    win::process_toplevels().len(),
                    win::count_descendants(self.gpui_hwnd)
                ));
            }
            "dpi" => {
                crate::log(&format!("dpi: window dpi={} gpui scale={}", win::dpi_for_window(self.gpui_hwnd), window.scale_factor()));
                if let Some(f) = self.browser().and_then(|b| b.main_frame()) {
                    on_ui(move || {
                        f.execute_java_script(
                            Some(&cef::CefString::from("console.log('page: dpr='+devicePixelRatio+' inner='+innerWidth+'x'+innerHeight)")),
                            Some(&cef::CefString::from("spike://ctl")),
                            0,
                        )
                    });
                }
            }
            "raf" => {
                if let Some(f) = self.browser().and_then(|b| b.main_frame()) {
                    on_ui(move || {
                        f.execute_java_script(
                            Some(&cef::CefString::from("(function(){var n=0,t0=performance.now();function s(){n++;if(performance.now()-t0<500)requestAnimationFrame(s);else console.log('page: raf='+n+' in 500ms visibility='+document.visibilityState)}requestAnimationFrame(s)})()")),
                            Some(&cef::CefString::from("spike://ctl")),
                            0,
                        )
                    });
                }
            }
            // shotchild PATH: PrintWindow of the active CEF child HWND itself.
            "shotchild" => {
                let child = self.active_child();
                match win::capture(child, win::PW_FULL) {
                    Ok((w, h, px)) => {
                        let f = win::fraction_not(&px, w, PRect { x: 0, y: 0, w: w as i32, h: h as i32 }, (0x1f, 0x29, 0x37));
                        let r = win::write_png(arg, w, h, &px);
                        crate::log(&format!("shotchild 0x{child:x} {w}x{h} non_background={f:.3} -> {arg} {r:?}"));
                    }
                    Err(e) => crate::log(&format!("shotchild failed: {e:#}")),
                }
            }
            "activate" => {
                let prev = win::set_focus(self.gpui_hwnd);
                crate::log(&format!("activate: prev=0x{prev:x} focus=0x{:x} foreground=0x{:x}", win::focus(), win::foreground_window()));
            }
            "closewin" => win::post_close(self.gpui_hwnd),
            "state" => self.dump_state(window, cx),
            // hittest X Y (logical, GPUI client): the deepest of our windows under that point.
            "hittest" => {
                let (x, y) = env_pair_str(arg, (0., 0.));
                let s = window.scale_factor();
                let hit = win::deepest_child_at(self.gpui_hwnd, (x * s).round() as i32, (y * s).round() as i32);
                let sys = win::window_from_client_point(self.gpui_hwnd, (x * s).round() as i32, (y * s).round() as i32);
                crate::log(&format!("hittest ({x},{y}) -> 0x{hit:x} {:?} window_from_point=0x{sys:x} {:?}", win::class_name(hit), win::class_name(sys)));
            }
            // shotn N: N whole-window captures, timing only.
            "shotn" => {
                let n: usize = arg.parse().unwrap_or(5);
                let mut times = Vec::new();
                for _ in 0..n {
                    if let Ok((_, _, _, t)) = self.capture() {
                        times.push(t.as_secs_f64() * 1000.);
                    }
                }
                let avg = times.iter().sum::<f64>() / times.len().max(1) as f64;
                let max = times.iter().cloned().fold(0., f64::max);
                crate::log(&format!("shotn: n={} ok={} avg={avg:.1}ms max={max:.1}ms wgc={}", n, times.len(), self.cap_wgc));
            }
            "stats" => {
                crate::log(&format!("pump: schedule_requests={} {}", self.shared.pump_requests.load(Ordering::Relaxed), self.stats.summary()));
            }
            "chrome" => {
                let id = {
                    let c = std::ffi::CString::new(arg).unwrap_or_default();
                    unsafe { cef::sys::cef_id_for_command_id_name(c.as_ptr()) }
                };
                if let Some(h) = self.browser().and_then(|b| b.host()) {
                    let name = arg.to_string();
                    on_ui(move || {
                        crate::log(&format!("gpui: execute_chrome_command {name}={id} can={}", h.can_execute_chrome_command(id)));
                        h.execute_chrome_command(id, cef::WindowOpenDisposition::CURRENT_TAB);
                    });
                }
            }
            "closeext" => {
                let ext: Vec<usize> = self.tabs.iter().enumerate().filter(|(_, t)| t.external).map(|(i, _)| i).collect();
                for i in ext {
                    if let Some(h) = Self::host_of(&self.tabs[i]) {
                        on_ui(move || h.close_browser(1));
                    }
                }
            }
            "quit" => self.begin_quit(cx),
            _ => crate::log(&format!("ctl: unknown command {cmd}")),
        }
    }

    /// GPUI fullscreen moves the window onto a monitor; only where the user cannot see it.
    fn toggle_fullscreen(&self, window: &mut Window) {
        if win::may_activate() {
            window.toggle_fullscreen();
        } else {
            crate::log("gpui: fullscreen refused on the input desktop");
        }
    }

    fn dump_state(&self, window: &Window, cx: &App) {
        let mut s = format!(
            "state: address={:?} key_focus={:?} active_tab={} gpui_active={} gpui_hovered={} focus=0x{:x} active_window=0x{:x} foreground=0x{:x} gpui_hwnd=0x{:x} omnibox_focused={} page_focused={} scale={} menu={:?} pump={:?}\n",
            self.address.read(cx).value().to_string(),
            self.key_focus,
            self.active,
            window.is_window_active(),
            window.is_window_hovered(),
            win::focus(),
            win::active_window(),
            win::foreground_window(),
            self.gpui_hwnd,
            self.address.read(cx).focus_handle(cx).is_focused(window),
            self.page_focused,
            window.scale_factor(),
            self.menu,
            self.shared.pump_mode
        );
        let l = self.layout.borrow();
        s += &format!("  content={:?} applied_host={:?} applied={:?} region={:?} host=0x{:x} host_rect={:?}\n", l.content, l.applied_host, l.applied, l.region, l.host, win::window_rect(l.host));
        for (i, t) in self.tabs.iter().enumerate() {
            s += &format!(
                "  tab {i}: id={:?} hwnd=0x{:x} rect={:?} visible={} ext={} loading={} back={} fwd={} title={:?} url={} favicon={:?}\n",
                t.id,
                t.hwnd,
                (t.hwnd != 0).then(|| win::window_rect(t.hwnd)),
                t.hwnd != 0 && win::is_visible(t.hwnd),
                t.external,
                t.loading,
                t.can_back,
                t.can_fwd,
                t.title,
                t.url,
                t.favicon
            );
        }
        crate::log(&s);
    }

    // ---------------------------------------------------------------- overlay

    fn set_menu(&mut self, mode: MenuMode, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((p, _)) = self.popup.take() {
            let _ = p.update(cx, |_, w, _| w.remove_window());
        }
        self.menu = mode;
        if mode == MenuMode::Popup {
            self.open_popup_menu(window, cx);
        }
        if mode != MenuMode::Region {
            let region = self.layout.borrow_mut().region.take();
            if let Some((hwnd, _)) = region {
                win::region_reset(hwnd);
            }
        }
        crate::log(&format!("gpui: menu mode {mode:?}"));
        cx.notify();
    }

    fn menu_rect(&self) -> Option<Bounds<Pixels>> {
        let c = self.layout.borrow().content?;
        Some(Bounds { origin: point(c.origin.x + c.size.width - px(272.), c.origin.y - px(6.)), size: size(px(260.), px(220.)) })
    }

    fn open_popup_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.menu_rect() else { return };
        let scale = window.scale_factor();
        let (cx0, cy0) = win::client_origin(self.gpui_hwnd);
        let origin = point(px(cx0 as f32 / scale) + r.origin.x, px(cy0 as f32 / scale) + r.origin.y);
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds { origin, size: r.size })),
            kind: WindowKind::PopUp,
            focus: false,
            show: false,
            titlebar: None,
            ..Default::default()
        };
        match cx.open_window(opts, |_, cx| cx.new(|_| MenuPopup)) {
            Ok(h) => {
                let hwnd = h.update(cx, |_, w, _| hwnd_of(w)).unwrap_or(0);
                // GPUI would place an off-screen popup at the display centre; position it ourselves.
                win::set_size(hwnd, (f32::from(r.size.width) * scale).round() as i32, (f32::from(r.size.height) * scale).round() as i32);
                win::move_to(hwnd, cx0 + (f32::from(r.origin.x) * scale).round() as i32, cy0 + (f32::from(r.origin.y) * scale).round() as i32);
                win::show_noactivate(hwnd);
                crate::log(&format!("gpui: popup window hwnd=0x{hwnd:x} {} owner=0x{:x}", win::describe(hwnd), win::owner(hwnd)));
                self.popup = Some((h.into(), hwnd));
            }
            Err(e) => crate::log(&format!("gpui: popup window failed: {e:#}")),
        }
    }
}

fn env_pair_str(arg: &str, default: (f32, f32)) -> (f32, f32) {
    let mut it = arg.split(|c: char| c == ' ' || c == ',' || c == 'x').filter(|s| !s.is_empty()).filter_map(|v| v.parse::<f32>().ok());
    match (it.next(), it.next()) {
        (Some(a), Some(b)) => (a, b),
        _ => default,
    }
}

fn menu_items() -> impl IntoElement {
    div().flex().flex_col().gap_1().p_2().children(
        ["New tab", "New window", "History", "Downloads", "Bookmarks", "Settings", "Exit"]
            .into_iter()
            .map(|s| div().px_2().py_1().rounded_md().hover(|d| d.bg(rgb(0xdbeafe))).child(s)),
    )
}

struct MenuPopup;
impl Render for MenuPopup {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0xfff7d6)).border_2().border_color(rgb(0xd97706)).text_color(rgb(0x111111)).child(menu_items())
    }
}

/// Push the content rectangle to the host and the active CEF child (physical px) if it
/// changed, and publish the DIP rectangle for popups. Called from prepaint and on tab switch.
fn apply_geometry(layout: &Rc<RefCell<Layout>>, child: Hwnd, shared: &Shared) {
    let mut l = layout.borrow_mut();
    let Some(b) = l.content else { return };
    let Some(hr) = l.content_px() else { return };
    *shared.content_rect_dip.lock().unwrap() = cef::Rect {
        x: if l.direct { f32::from(b.origin.x) as i32 } else { 0 },
        y: if l.direct { f32::from(b.origin.y) as i32 } else { 0 },
        width: f32::from(b.size.width) as i32,
        height: f32::from(b.size.height) as i32,
    };
    if !l.direct && l.applied_host != Some(hr) {
        win::set_rect(l.host, hr);
        crate::log(&format!("win: host 0x{:x} -> {}x{}+{}+{} (scale {})", l.host, hr.w, hr.h, hr.x, hr.y, l.scale));
        l.applied_host = Some(hr);
    }
    let cr = if l.direct { hr } else { PRect { x: 0, y: 0, w: hr.w, h: hr.h } };
    if child != 0 && l.applied != Some((child, cr)) {
        win::set_rect(child, cr);
        crate::log(&format!("win: child 0x{child:x} -> {}x{}+{}+{}", cr.w, cr.h, cr.x, cr.y));
        l.applied = Some((child, cr));
    }
}

impl Focusable for BrowserWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BrowserWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = window.is_window_active();
        if self.last_active != Some(active) {
            crate::log(&format!("gpui: render sees active={active}"));
            self.last_active = Some(active);
        }
        self.layout.borrow_mut().scale = window.scale_factor();
        let theme = cx.theme().clone();
        let active_child = self.active_child();
        let tab = self.tabs.get(self.active);
        let (loading, can_back, can_fwd) = tab.map(|t| (t.loading, t.can_back, t.can_fwd)).unwrap_or_default();

        let tabs = div().flex().flex_row().gap_1().px_2().pt_1().bg(rgb(0xe5e7eb)).children(self.tabs.iter().enumerate().map(|(i, t)| {
            let label: SharedString =
                format!("{}{} {}", if t.loading { "⟳ " } else { "" }, t.title.chars().take(22).collect::<String>(), if t.favicon.is_some() { "★" } else { "" }).into();
            let selected = i == self.active;
            div()
                .id(("tab", i))
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .rounded_t_md()
                .bg(if selected { rgb(0xffffff) } else { rgb(0xd1d5db) })
                .hover(|d| d.bg(rgb(0xf3f4f6)))
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| this.activate(i, window, cx)))
                .child(div().id(("close", i)).px_1().rounded_sm().hover(|d| d.bg(rgb(0xfca5a5))).child("×").on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_tab(i);
                })))
        }));
        let tabs = tabs.child(Button::new("new-tab").label("+").ghost().compact().on_click(cx.listener(|this, _, window, cx| {
            this.new_tab("about:blank", cx);
            this.focus_omnibox(window, cx);
        })));

        let toolbar = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .p_1()
            .bg(rgb(0xffffff))
            .child(Button::new("back").label("←").ghost().compact().disabled(!can_back).on_click(cx.listener(|this, _, w, cx| this.do_action(ShellAction::Back, w, cx))))
            .child(Button::new("fwd").label("→").ghost().compact().disabled(!can_fwd).on_click(cx.listener(|this, _, w, cx| this.do_action(ShellAction::Forward, w, cx))))
            .child(if loading {
                Button::new("stop").label("✕").ghost().compact().on_click(cx.listener(|this, _, _, _| {
                    if let Some(b) = this.browser().cloned() {
                        on_ui(move || b.stop_load());
                    }
                }))
            } else {
                Button::new("reload").label("⟳").ghost().compact().on_click(cx.listener(|this, _, w, cx| this.do_action(ShellAction::Reload, w, cx)))
            })
            .child(div().flex_1().child(Input::new(&self.address)))
            .child(Button::new("menu").label("☰").ghost().compact().on_click(cx.listener(|this, _, w, cx| {
                let next = if this.menu == MenuMode::Off { MenuMode::Region } else { MenuMode::Off };
                this.set_menu(next, w, cx);
            })));

        let layout = self.layout.clone();
        let shared = self.shared.clone();
        let content = div().flex_1().w_full().when(!hole_mode(), |d| d.bg(rgb(0x1f2937))).child(
            canvas(
                move |bounds, window, _cx| {
                    {
                        let mut l = layout.borrow_mut();
                        l.content = Some(bounds);
                        l.scale = window.scale_factor();
                    }
                    apply_geometry(&layout, active_child, &shared);
                },
                |_, _, _, _| {},
            )
            .size_full(),
        );

        let overlay = if matches!(self.menu, MenuMode::Region | MenuMode::Plain) {
            self.menu_rect().map(|r| {
                let cut = self.menu == MenuMode::Region;
                let layout = self.layout.clone();
                deferred(
                    anchored().position(r.origin).child(
                        div()
                            .w(r.size.width)
                            .h(r.size.height)
                            .bg(rgb(0xfff7d6))
                            .border_2()
                            .border_color(rgb(0xd97706))
                            .rounded_md()
                            .shadow_lg()
                            .text_color(rgb(0x111111))
                            .child(menu_items())
                            .child(canvas(
                                move |_, _, _| {
                                    if !cut {
                                        return;
                                    }
                                    let mut l = layout.borrow_mut();
                                    let (Some(c), Some(cp), s) = (l.content, l.content_px(), l.scale) else { return };
                                    let pad = 8.0;
                                    // Hole relative to the window that gets the region (host, or the
                                    // child in direct mode; both have their origin at the content origin).
                                    let hole = PRect {
                                        x: ((f32::from(r.origin.x - c.origin.x) - pad) * s).floor() as i32,
                                        y: ((f32::from(r.origin.y - c.origin.y) - pad) * s).floor() as i32,
                                        w: ((f32::from(r.size.width) + 2. * pad) * s).ceil() as i32,
                                        h: ((f32::from(r.size.height) + 2. * pad) * s).ceil() as i32,
                                    };
                                    let target = if l.direct { active_child } else { l.host };
                                    if target != 0 && l.region != Some((target, hole)) {
                                        win::region_exclude(target, (cp.w, cp.h), &[hole]);
                                        crate::log(&format!("win: region on 0x{target:x} hole {hole:?}"));
                                        l.region = Some((target, hole));
                                    }
                                },
                                |_, _, _, _| {},
                            )),
                    ),
                )
                .with_priority(1)
            })
        } else {
            None
        };

        let title = tab.map(|t| t.title.clone()).unwrap_or_default();
        let fav = tab.and_then(|t| t.favicon.clone()).unwrap_or_default();
        let status = div()
            .flex()
            .flex_row()
            .gap_3()
            .px_2()
            .py_1()
            .text_xs()
            .bg(rgb(0xf3f4f6))
            .child(format!("tab {}/{}", self.active + (!self.tabs.is_empty()) as usize, self.tabs.len()))
            .child(format!("window active={active}"))
            .child(format!("page_focus={}", self.page_focused))
            .child(format!("pump={:?}", if self.shared.pump_mode == PumpMode::External { "ext" } else { "mt" }))
            .child(format!("title={title}"))
            .child(format!("favicon={fav}"));

        div()
            .id("browser-window")
            .key_context("BrowserWindow")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .when(!hole_mode(), |d| d.bg(theme.background))
            .text_color(theme.foreground)
            .text_sm()
            // Any mouse-down that reaches GPUI is on the chrome (page clicks go to the CEF
            // child HWND). Pull Win32 keyboard focus back to the toplevel.
            .capture_any_mouse_down(cx.listener(|this, _ev, _w, _cx| {
                if this.key_focus != KeyFocus::Chrome || win::focus() != this.gpui_hwnd {
                    crate::log("gpui: chrome click -> keyboard to chrome (SetFocus toplevel)");
                    this.set_key_focus(KeyFocus::Chrome);
                }
            }))
            .on_action(cx.listener(|this, _: &FocusOmnibox, w, cx| this.focus_omnibox(w, cx)))
            .on_action(cx.listener(|this, _: &NewTab, w, cx| this.do_action(ShellAction::NewTab, w, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, w, cx| this.do_action(ShellAction::CloseTab, w, cx)))
            .on_action(cx.listener(|this, _: &Reload, w, cx| this.do_action(ShellAction::Reload, w, cx)))
            .on_action(cx.listener(|this, _: &Back, w, cx| this.do_action(ShellAction::Back, w, cx)))
            .on_action(cx.listener(|this, _: &Forward, w, cx| this.do_action(ShellAction::Forward, w, cx)))
            .on_action(cx.listener(|this, _: &NextTab, w, cx| this.do_action(ShellAction::NextTab, w, cx)))
            .on_action(cx.listener(|this, _: &Quit, _w, cx| this.begin_quit(cx)))
            .child(tabs)
            .child(toolbar)
            .when(self.extra_bar, |d| d.child(div().h(px(40.)).bg(rgb(0xc7d2fe)).px_2().child("extra bar (layout change test)")))
            .child(content)
            .child(status)
            .children(overlay)
            .on_mouse_down(MouseButton::Right, |_, _, _| {})
    }
}
