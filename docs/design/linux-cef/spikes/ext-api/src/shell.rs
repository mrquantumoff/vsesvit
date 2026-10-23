//! The GPUI side: one browser window with a tab strip, a toolbar and a content
//! area that hosts one Chrome-style CEF browser per tab as a native X11 child.
use crate::cefglue::{self, CefEvent, Shared, ShellAction};
use crate::pump::PumpStats;
use crate::xwin::{XConn, XRect, write_png};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::prelude::*;
use gpui::{
    AnyWindowHandle, App, Bounds, Context, Entity, FocusHandle, Focusable, KeyBinding, MouseButton, Pixels,
    SharedString, Subscription, Window, WindowBounds, WindowKind, WindowOptions, actions, anchored, canvas,
    deferred, div, point, px, rgb, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Disableable as _, Root};
use cef::{ImplBrowser as _, ImplBrowserHost as _, ImplFrame as _, ImplJsdialogCallback as _, ImplFileDialogCallback as _, ImplPermissionPromptCallback as _};
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
    xid: u32,
    title: String,
    url: String,
    loading: bool,
    can_back: bool,
    can_fwd: bool,
    favicon: Option<String>,
    closing: bool,
    /// Created by Chrome itself (not a child of our window).
    external: bool,
    kind: &'static str,
    style: u32,
}

#[derive(Default)]
struct Layout {
    /// Content area in GPUI logical pixels (window-relative).
    content: Option<Bounds<Pixels>>,
    scale: f32,
    /// 24-bit host window that parents every CEF child of this GPUI window.
    host: u32,
    /// Last geometry pushed to the host (physical px, GPUI-window-relative).
    applied_host: Option<XRect>,
    /// Last geometry pushed to a CEF child: (xid, physical rect, host-relative).
    applied: Option<(u32, XRect)>,
    /// SHAPE cut-out currently applied to the host (host-relative physical px).
    shaped: Option<(u32, XRect)>,
}

/// Who owns the X keyboard focus inside this window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyFocus {
    /// GPUI chrome: X focus on the focus proxy (keys propagate to GPUI's toplevel).
    Chrome,
    /// The active tab's CEF child window.
    Page,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuMode {
    Off,
    /// In-window GPUI overlay; the CEF child is SHAPE'd so the overlay shows through.
    Shape,
    /// In-window GPUI overlay with no SHAPE (to show the problem).
    Plain,
    /// Separate override-redirect GPUI window (WindowKind::PopUp) above the page.
    Popup,
}

pub struct BrowserWindow {
    shared: Arc<Shared>,
    client: cef::Client,
    x: Rc<XConn>,
    gpui_xid: u32,
    host_xid: u32,
    proxy_xid: u32,
    key_focus: KeyFocus,
    tabs: Vec<Tab>,
    active: usize,
    closed_urls: Vec<String>,
    address: Entity<InputState>,
    focus: FocusHandle,
    layout: Rc<RefCell<Layout>>,
    menu: MenuMode,
    popup: Option<AnyWindowHandle>,
    extra_bar: bool,
    closing_all: bool,
    cef_ready: bool,
    page_focused: bool,
    last_active: Option<bool>,
    last_hovered: Option<bool>,
    stats: Arc<PumpStats>,
    stop_pump: Arc<AtomicBool>,
    _subs: Vec<Subscription>,
    /// What to do with browsers Chrome creates on its own (default_client): keep | close | redirect | reparent.
    adopt: String,
    /// Chrome-created browsers waiting for their first real URL (adopt=redirect): id -> created at.
    pending_redirect: std::collections::HashMap<i32, std::time::Instant>,
    /// Context menu waiting for the shell's choice (SPIKE_MENU=shell).
    pending_menu: Option<(i32, cef::RunContextMenuCallback, Vec<(i32, String)>)>,
    /// Extension-popup experiment: an extra 24-bit host stacked over the page, and its browser.
    popup_host: u32,
    popup_browser: Option<cef::Browser>,
    /// The popup browser's own X window (child of `popup_host`).
    popup_xid: u32,
    popup_style: cef::RuntimeStyle,
}

pub struct ShellInit {
    pub shared: Arc<Shared>,
    pub events: UnboundedReceiver<CefEvent>,
    pub ctl: Option<UnboundedReceiver<String>>,
    pub stats: Arc<PumpStats>,
    pub stop_pump: Arc<AtomicBool>,
}

pub fn open(cx: &mut App, init: ShellInit) {
    bind_keys(cx);
    let (w, h) = std::env::var("SPIKE_WIN")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?))))
        .unwrap_or((1280., 800.));
    let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        app_id: Some("dev.mrquantumoff.VsesvitSpike".into()),
        titlebar: Some(gpui::TitlebarOptions { title: Some("Vsesvit spike".into()), ..Default::default() }),
        // Default throttles an "inactive" window to 30 fps; a browser should not throttle.
        inactive_frame_interval: if std::env::var("SPIKE_THROTTLE").is_ok() { Some(std::time::Duration::from_millis(33)) } else { None },
        ..Default::default()
    };
    let handle = cx
        .open_window(opts, |window, cx| {
            let view = cx.new(|cx| BrowserWindow::new(init, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("open window");
    let _ = handle.update(cx, |_, window, _| window.activate_window());
}

fn x11_window_id(window: &Window) -> u32 {
    match raw_window_handle::HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Xcb(h)) => h.window.get(),
        Ok(RawWindowHandle::Xlib(h)) => h.window as u32,
        other => panic!("GPUI is not on X11: {other:?}"),
    }
}

impl BrowserWindow {
    fn new(init: ShellInit, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let gpui_xid = x11_window_id(window);
        crate::log(&format!("gpui: window xid=0x{gpui_xid:x} scale={}", window.scale_factor()));
        let x = Rc::new(XConn::connect().expect("x11rb"));
        let host_xid = x.create_host(gpui_xid, XRect { x: 0, y: 0, w: 1, h: 1 }, 0x1f2937).expect("host window");
        let proxy_xid = x.find_child_named(gpui_xid, "gpui-focus-proxy").expect("patched GPUI focus proxy");
        crate::log(&format!("x11: content host window 0x{host_xid:x} (depth 24), focus proxy 0x{proxy_xid:x}, inside 0x{gpui_xid:x}"));
        init.shared.parent_xid.store(host_xid as u64, Ordering::Relaxed);
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
            crate::log(&format!("gpui: window activation changed active={active} x_focus=0x{:x}", this.x.focus()));
            if active {
                // The WM focused our toplevel: hand X focus to whoever owns the keyboard.
                this.apply_x_focus();
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
        // CEF events -> this entity.
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
                while let Some(line) = ctl.next().await {
                    if this.update_in(cx, |this, window, cx| this.on_ctl(&line, window, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
        }
        let client = cefglue::client::new_client(init.shared.clone());
        Self {
            shared: init.shared,
            client,
            x,
            gpui_xid,
            host_xid,
            proxy_xid,
            key_focus: KeyFocus::Page,
            tabs: Vec::new(),
            active: 0,
            closed_urls: Vec::new(),
            address,
            focus: cx.focus_handle(),
            layout: Rc::new(RefCell::new(Layout { scale: window.scale_factor(), host: host_xid, ..Default::default() })),
            menu: MenuMode::Off,
            popup: None,
            extra_bar: false,
            closing_all: false,
            cef_ready: false,
            page_focused: false,
            last_active: None,
            last_hovered: None,
            stats: init.stats,
            stop_pump: init.stop_pump,
            _subs: subs,
            adopt: std::env::var("SPIKE_ADOPT").unwrap_or_else(|_| "keep".into()),
            pending_redirect: Default::default(),
            pending_menu: None,
            popup_host: 0,
            popup_browser: None,
            popup_xid: 0,
            popup_style: cef::RuntimeStyle::CHROME,
        }
    }

    // ---------------------------------------------------------------- tabs

    fn content_rect_dip(&self) -> cef::Rect {
        let l = self.layout.borrow();
        match l.content {
            // Host-relative: the CEF child fills the host window.
            Some(b) => cef::Rect {
                x: 0,
                y: 0,
                width: (f32::from(b.size.width) as i32).max(1),
                height: (f32::from(b.size.height) as i32).max(1),
            },
            None => cef::Rect { x: 0, y: 0, width: 1280, height: 700 },
        }
    }

    fn new_tab(&mut self, url: &str, cx: &mut Context<Self>) {
        let rect = self.content_rect_dip();
        let wi = cef::WindowInfo { runtime_style: cef::RuntimeStyle::CHROME, ..Default::default() }
            .set_as_child(self.host_xid as _, &rect);
        let settings = cef::BrowserSettings::default();
        let mut client = self.client.clone();
        let ok = cef::browser_host_create_browser(
            Some(&wi),
            Some(&mut client),
            Some(&cef::CefString::from(url)),
            Some(&settings),
            None,
            None,
        );
        crate::log(&format!("gpui: create_browser url={url} rect={}x{}+{}+{} -> {ok}", rect.width, rect.height, rect.x, rect.y));
        self.tabs.push(Tab { url: url.into(), title: "Loading…".into(), ..Default::default() });
        cx.notify();
    }

    fn browser(&self) -> Option<&cef::Browser> {
        self.tabs.get(self.active).and_then(|t| t.browser.as_ref())
    }

    fn activate(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if idx >= self.tabs.len() {
            return;
        }
        for (i, t) in self.tabs.iter().enumerate() {
            if i != idx && t.xid != 0 && !t.external {
                self.x.map(t.xid, false);
                if let Some(h) = t.browser.as_ref().and_then(|b| b.host()) {
                    h.was_hidden(1);
                }
            }
        }
        self.active = idx;
        let t = &self.tabs[idx];
        if t.xid != 0 && !t.external {
            self.layout.borrow_mut().applied = None; // force a configure in the next prepaint
            self.apply_geometry_now();
            self.x.map(t.xid, true);
            if let Some(h) = t.browser.as_ref().and_then(|b| b.host()) {
                h.was_hidden(0);
            }
            self.apply_x_focus();
        }
        let url = t.url.clone();
        self.address.update(cx, |a, cx| a.set_value(url, window, cx));
        crate::log(&format!("gpui: activated tab {idx} id={:?}", self.tabs[idx].id));
        cx.notify();
    }

    fn apply_geometry_now(&self) {
        let xid = self.tabs.get(self.active).map(|t| t.xid).unwrap_or(0);
        let crop = self.tabs.get(self.active).filter(|t| t.kind == "default").map(|_| self.shared.embed_crop).unwrap_or(0);
        apply_geometry_crop(&self.layout, &self.x, xid, &self.shared, crop);
    }

    fn close_tab(&mut self, idx: usize) {
        if let Some(t) = self.tabs.get_mut(idx) {
            if let Some(b) = &t.browser {
                t.closing = true;
                self.closed_urls.push(t.url.clone());
                if let Some(h) = b.host() {
                    h.close_browser(0);
                }
                crate::log(&format!("gpui: close_browser(0) tab {idx} id={:?}", t.id));
            }
        }
    }

    fn begin_quit(&mut self, cx: &mut Context<Self>) {
        self.closing_all = true;
        for t in &mut self.tabs {
            if let Some(h) = t.browser.as_ref().and_then(|b| b.host()) {
                t.closing = true;
                h.close_browser(1);
            }
        }
        if self.tabs.iter().all(|t| t.browser.is_none()) {
            self.tabs.clear();
            self.finish_quit(cx);
        }
    }

    fn finish_quit(&mut self, cx: &mut Context<Self>) {
        crate::log("gpui: all browsers closed; quitting GPUI");
        self.stop_pump.store(true, Ordering::SeqCst);
        if let Some(p) = self.popup.take() {
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
            f.load_url(Some(&cef::CefString::from(url.as_str())));
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
        self.shared.page_focus_allowed.store(f == KeyFocus::Page, Ordering::SeqCst);
        self.page_focused = f == KeyFocus::Page;
        self.apply_x_focus();
    }

    /// Put X keyboard focus where `key_focus` says. Never on the toplevel itself.
    fn apply_x_focus(&self) {
        let target = match self.key_focus {
            KeyFocus::Chrome => self.proxy_xid,
            KeyFocus::Page => match self.tabs.get(self.active) {
                Some(t) if t.xid != 0 && !t.external => t.xid,
                _ => self.proxy_xid,
            },
        };
        // Only move focus *within* our toplevel. If another toplevel (a native Chrome
        // window, another app) holds X focus, setting it would steal focus from the
        // window the WM considers active and start a focus fight with the WM.
        let current = self.x.focus();
        if current == target {
            // already there
        } else if current <= 1 || self.x.is_within(current, self.gpui_xid) {
            // (current <= 1: None/PointerRoot, i.e. no window manager has focused anything, as on Xvfb)
            self.x.set_focus(target);
        } else {
            crate::log(&format!("focus: not stealing X focus from 0x{current:x} (not ours)"));
            return;
        }
        if self.key_focus == KeyFocus::Page {
            if let Some(h) = self.browser().and_then(|b| b.host()) {
                h.set_focus(1);
            }
        }
    }

    /// GPUI chrome takes keyboard focus: X focus to the proxy, then GPUI focus.
    fn focus_omnibox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_key_focus(KeyFocus::Chrome);
        self.address.update(cx, |a, cx| {
            a.focus(window, cx);
            a.select_all(window, cx);
        });
        crate::log(&format!("gpui: focus_omnibox -> x_focus=0x{:x}", self.x.focus()));
        cx.notify();
    }

    fn focus_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx); // park GPUI focus on the window root (no caret)
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
            CefEvent::AfterCreated { id, browser, is_popup, xid, opener_id, kind, style } => {
                let parent = self.x.conn_parent(xid as u32);
                if kind == "popuphost" {
                    crate::log(&format!("gpui: popup-host browser id={id} style={style} xid=0x{xid:x} x_parent=0x{parent:x}"));
                    // Extension popups size themselves to their content (Chrome: 25x25 to 800x600).
                    if let Some(h) = browser.host() {
                        h.set_auto_resize_enabled(1, Some(&cef::Size { width: 25, height: 25 }), Some(&cef::Size { width: 800, height: 600 }));
                    }
                    self.popup_xid = xid as u32;
                    self.popup_browser = Some(browser);
                    return;
                }
                if kind == "default" {
                    let toplevel = parent == self.x.root;
                    crate::log(&format!("gpui: Chrome-created browser id={id} style={style} popup={is_popup} xid=0x{xid:x} x_parent=0x{parent:x} toplevel={toplevel} adopt={}", self.adopt));
                    match self.adopt.as_str() {
                        "close" => {
                            if let Some(h) = browser.host() { h.close_browser(1); }
                            crate::log(&format!("gpui: adopt=close: close_browser(1) on id={id}"));
                            return;
                        }
                        "redirect" => {
                            self.pending_redirect.insert(id, std::time::Instant::now());
                            // keep it in the tab list as external until its URL is known
                        }
                        "reparent" if xid != 0 => {
                            // Move Chrome's own toplevel into our host window.
                            let r = self.layout.borrow().applied_host.unwrap_or(XRect { x: 0, y: 0, w: 1280, h: 700 });
                            let ok = self.x.reparent(xid as u32, self.host_xid, 0, 0, r.w, r.h);
                            crate::log(&format!("gpui: adopt=reparent: 0x{xid:x} -> host 0x{:x} {ok:?}", self.host_xid));
                        }
                        _ => {}
                    }
                }
                let slot = (!is_popup && kind == "shell").then(|| self.tabs.iter().position(|t| t.id.is_none())).flatten();
                let parent = self.x.conn_parent(xid as u32);
                let ours = parent == self.host_xid;
                let idx = slot.unwrap_or_else(|| {
                    self.tabs.push(Tab { title: if kind == "default" { "Chrome window".into() } else { "Popup".into() }, ..Default::default() });
                    self.tabs.len() - 1
                });
                crate::log(&format!("gpui: tab {idx} <- browser id={id} kind={kind} popup={is_popup} opener={opener_id} xid=0x{xid:x} x_parent=0x{parent:x} (ours={ours})"));
                let t = &mut self.tabs[idx];
                t.id = Some(id);
                t.browser = Some(browser);
                t.xid = xid as u32;
                t.kind = kind;
                t.style = style;
                t.external = !ours;
                if ours || std::env::var("SPIKE_ACTIVATE_EXTERNAL").is_ok() {
                    self.activate(idx, window, cx);
                } else {
                    cx.notify();
                }
            }
            CefEvent::AutoResize { id, w, h } => {
                if self.popup_browser.as_ref().is_some_and(|b| b.identifier() == id) && self.popup_host != 0 && w > 0 && h > 0 {
                    // Keep the popup's top-right corner where the action button would be.
                    let s = self.layout.borrow().scale;
                    let (pw, ph) = ((w as f32 * s) as u32, (h as f32 * s) as u32);
                    if let Some(g) = self.x.geometry(self.popup_host) {
                        let r = XRect { x: g.x + g.w as i32 - pw as i32, y: g.y, w: pw, h: ph };
                        self.x.configure(self.popup_host, r);
                        if self.popup_xid != 0 { self.x.configure(self.popup_xid, XRect { x: 0, y: 0, w: pw, h: ph }); }
                        crate::log(&format!("gpui: popup host resized to {r:?} (content {w}x{h} DIP)"));
                    }
                }
            }
            CefEvent::BeforeClose { id } => {
                self.pending_redirect.remove(&id);
                if self.popup_browser.as_ref().is_some_and(|b| b.identifier() == id) {
                    crate::log(&format!("gpui: popup-host browser id={id} closed"));
                    self.popup_browser = None;
                    if self.popup_host != 0 { self.x.map(self.popup_host, false); }
                }
                if let Some(i) = find(&self.tabs, id) {
                    let t = self.tabs.remove(i);
                    drop(t.browser); // release our reference
                    if self.active >= self.tabs.len() && !self.tabs.is_empty() {
                        self.active = self.tabs.len() - 1;
                    } else if i < self.active {
                        self.active -= 1;
                    }
                    let mut l = self.layout.borrow_mut();
                    l.applied = None;
                    l.shaped = None;
                    drop(l);
                    if !self.tabs.is_empty() && !self.closing_all {
                        let a = self.active;
                        self.activate(a, window, cx);
                    }
                }
                crate::log(&format!("gpui: tabs left {}", self.tabs.len()));
                if self.closing_all && self.tabs.iter().all(|t| t.browser.is_none()) {
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
                if let Some(t0) = self.pending_redirect.get(&id).copied() {
                    if !url.is_empty() && url != "about:blank" {
                        self.pending_redirect.remove(&id);
                        crate::log(&format!("gpui: adopt=redirect: Chrome window id={id} reached {url} after {:?}; closing it and opening a GPUI tab", t0.elapsed()));
                        if let Some(b) = find(&self.tabs, id).and_then(|i| self.tabs[i].browser.clone()) {
                            if let Some(h) = b.host() { h.close_browser(1); }
                        }
                        self.new_tab(&url, cx);
                    }
                }
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
                    window.toggle_fullscreen();
                }
            }
            CefEvent::GotFocus { id } => {
                crate::log(&format!("cef-ev: got_focus id={id} x_focus=0x{:x}", self.x.focus()));
                // A click in the page (or allowed programmatic focus): the page owns the
                // keyboard. Pin X focus to the child so keys don't follow the pointer.
                window.focus(&self.focus, cx);
                self.set_key_focus(KeyFocus::Page);
                cx.notify();
            }
            CefEvent::TakeFocus { id, next } => {
                // Tab/Shift+Tab walked out of the page: give the keyboard to the chrome.
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
                crate::log(&format!("cef-ev: file dialog id={id} mode={mode} title={title:?} -> choose /etc/hostname"));
                let mut paths = cef::CefStringList::new();
                paths.append("/etc/hostname");
                callback.cont(Some(&mut paths));
            }
            CefEvent::Download { id, name, url } => crate::log(&format!("cef-ev: download id={id} {name} {url}")),
            CefEvent::PermissionPrompt { id, prompt_id, origin, perms, callback } => {
                crate::log(&format!("cef-ev: permission prompt id={id} prompt={prompt_id} origin={origin} perms=0x{perms:x} -> ACCEPT"));
                callback.cont(cef::PermissionRequestResult::ACCEPT);
            }
            CefEvent::Console { id, message } => crate::log(&format!("cef-ev: console id={id} {message}")),
            CefEvent::ContextMenu { id, callback, items } => {
                crate::log(&format!("gpui: context menu for id={id} waiting for the shell: {items:?}"));
                if let Some((_, old, _)) = self.pending_menu.take() { use cef::ImplRunContextMenuCallback as _; old.cancel(); }
                self.pending_menu = Some((id, callback, items));
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
            ShellAction::Fullscreen => window.toggle_fullscreen(),
            ShellAction::Reload => {
                if let Some(b) = self.browser() {
                    b.reload()
                }
            }
            ShellAction::Back => {
                if let Some(b) = self.browser() {
                    b.go_back()
                }
            }
            ShellAction::Forward => {
                if let Some(b) = self.browser() {
                    b.go_forward()
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

    fn on_ctl(&mut self, line: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
        crate::log(&format!("ctl> {line}"));
        match cmd {
            "new" => self.new_tab(if arg.is_empty() { "about:blank" } else { arg }, cx),
            "nav" => self.navigate(arg, window, cx),
            "switch" => self.activate(arg.parse().unwrap_or(0), window, cx),
            "close" => self.close_tab(arg.parse().unwrap_or(self.active)),
            "back" => self.do_action(ShellAction::Back, window, cx),
            "fwd" => self.do_action(ShellAction::Forward, window, cx),
            "reload" => self.do_action(ShellAction::Reload, window, cx),
            "stop" => {
                if let Some(b) = self.browser() {
                    b.stop_load()
                }
            }
            "js" => {
                if let Some(f) = self.browser().and_then(|b| b.main_frame()) {
                    f.execute_java_script(Some(&cef::CefString::from(arg)), Some(&cef::CefString::from("spike://ctl")), 1);
                }
            }
            "omnibox" => self.focus_omnibox(window, cx),
            "page" => self.focus_page(window, cx),
            "menu" => {
                self.set_menu(
                    match arg {
                        "shape" => MenuMode::Shape,
                        "plain" => MenuMode::Plain,
                        "popup" => MenuMode::Popup,
                        _ => MenuMode::Off,
                    },
                    window,
                    cx,
                );
            }
            "bar" => {
                self.extra_bar = !self.extra_bar;
                cx.notify();
            }
            "resize" => {
                let mut it = arg.split_whitespace().filter_map(|v| v.parse::<f32>().ok());
                if let (Some(w), Some(h)) = (it.next(), it.next()) {
                    window.resize(size(px(w), px(h)));
                }
            }
            "fullscreen" => window.toggle_fullscreen(),
            "shot" | "shotraw" => {
                let default = crate::env_dir().join("shot.png").to_string_lossy().into_owned();
                let path = if arg.is_empty() { default.as_str() } else { arg };
                let t = std::time::Instant::now();
                match self.x.capture_mode(self.gpui_xid, cmd == "shot") {
                    Ok((w, h, px)) => {
                        let r = write_png(path, w, h, &px);
                        crate::log(&format!("gpui: screenshot {w}x{h} -> {path} {r:?} in {:?}", t.elapsed()));
                    }
                    Err(e) => crate::log(&format!("gpui: screenshot failed: {e:#}")),
                }
            }
            "tree" => {
                let mut out = Vec::new();
                self.x.tree(self.gpui_xid, 1, &mut out);
                let _ = self.host_xid;
                crate::log(&format!("x11 tree of 0x{:x}:\n{}", self.gpui_xid, out.join("\n")));
            }
            "roottree" => {
                let mut out = Vec::new();
                self.x.tree(self.x.root, 1, &mut out);
                crate::log(&format!("x11 tree of root:\n{}", out.join("\n")));
            }
            "state" => self.dump_state(window, cx),
            "stats" => {
                let s = &self.stats;
                let r = |a: &std::sync::atomic::AtomicU64| a.load(Ordering::Relaxed);
                crate::log(&format!(
                    "pump: schedule_requests={} cef_work={} glib_rounds={} glib_dispatch={} glib_fd_wakes={} loop_wakes={} timer_ticks={}",
                    r(&self.shared.pump_requests), r(&s.cef_work), r(&s.glib_rounds), r(&s.glib_dispatch), r(&s.glib_wakes), r(&s.loop_wakes), r(&s.ticks)
                ));
                crate::log(&format!("pump: {}", s.summary()));
            }
            "chrome" => {
                // Execute a Chrome command on the active browser: chrome IDC_NAME
                let id = {
                    let c = std::ffi::CString::new(arg).unwrap_or_default();
                    unsafe { cef::sys::cef_id_for_command_id_name(c.as_ptr()) }
                };
                if let Some(h) = self.browser().and_then(|b| b.host()) {
                    crate::log(&format!("gpui: execute_chrome_command {arg}={id} can={}", h.can_execute_chrome_command(id)));
                    h.execute_chrome_command(id, cef::WindowOpenDisposition::CURRENT_TAB);
                }
            }
            "closeext" => {
                // Close browsers Chrome created on its own (native Chrome windows).
                let ext: Vec<usize> = self.tabs.iter().enumerate().filter(|(_, t)| t.external).map(|(i, _)| i).collect();
                for i in ext {
                    if let Some(h) = self.tabs[i].browser.as_ref().and_then(|b| b.host()) {
                        h.close_browser(1);
                    }
                }
            }
            "unfullscreen" => {
                if window.is_fullscreen() {
                    window.toggle_fullscreen();
                }
            }
            "quit" => self.begin_quit(cx),
            "menupick" => {
                use cef::ImplRunContextMenuCallback as _;
                if let Some((id, cb, items)) = self.pending_menu.take() {
                    let pick = arg.parse::<i32>().ok().or_else(|| items.iter().find(|(_, l)| l.contains(arg)).map(|(i, _)| *i));
                    match pick {
                        Some(c) => { crate::log(&format!("gpui: menupick id={id} -> cont({c})")); cb.cont(c, cef::EventFlags::default()); }
                        None => { crate::log(&format!("gpui: menupick: no item {arg:?}; cancel")); cb.cancel(); }
                    }
                } else { crate::log("gpui: menupick: no pending menu"); }
            }
            "menucancel" => {
                use cef::ImplRunContextMenuCallback as _;
                if let Some((_, cb, _)) = self.pending_menu.take() { cb.cancel(); crate::log("gpui: menu cancelled"); }
            }
            "adopt" => { self.adopt = arg.to_string(); crate::log(&format!("gpui: adopt mode {arg}")); }
            "adoptsync" => { *self.shared.adopt_sync.lock().unwrap() = arg.to_string(); crate::log(&format!("gpui: adopt_sync mode {arg:?}")); }
            "showext" => {
                // Map a hidden/adopted Chrome-created browser's toplevel into the content host (cropped), as a tab.
                let idx: usize = arg.parse().unwrap_or(usize::MAX);
                if let Some(t) = self.tabs.get(idx) {
                    let r = self.layout.borrow().applied_host.unwrap_or(XRect { x: 0, y: 0, w: 1280, h: 700 });
                    let crop = self.shared.embed_crop;
                    let res = self.x.reparent(t.xid, self.host_xid, 0, -crop, r.w, r.h + crop as u32);
                    crate::log(&format!("gpui: showext tab {idx} 0x{:x} -> host (crop {crop}): {res:?}", t.xid));
                    self.tabs[idx].external = false;
                    self.activate(idx, window, cx);
                }
            }
            "popuphost" => {
                // popuphost <alloy|chrome> URL  : an extension popup page in a native child stacked over the page
                let (style, url) = arg.split_once(' ').unwrap_or(("chrome", arg));
                self.open_popup_host(style == "alloy", url);
            }
            "popupclose" => {
                if let Some(h) = self.popup_browser.take().and_then(|b| b.host()) { h.close_browser(1); }
                if self.popup_host != 0 { self.x.map(self.popup_host, false); }
            }
            "popupjs" => {
                if let Some(f) = self.popup_browser.as_ref().and_then(|b| b.main_frame()) {
                    f.execute_java_script(Some(&cef::CefString::from(arg)), Some(&cef::CefString::from("spike://ctl")), 1);
                }
            }
            "xfocus" => {
                let xid = u32::from_str_radix(arg.trim_start_matches("0x"), 16).unwrap_or(0);
                self.x.set_focus(xid);
                crate::log(&format!("gpui: XSetInputFocus 0x{xid:x}"));
            }
            "focustab" => {
                // X focus straight to tab N's child (as a click into it would), without switching tabs.
                if let Some(t) = self.tabs.get(arg.parse::<usize>().unwrap_or(0)) {
                    self.x.set_focus(t.xid);
                    if let Some(h) = t.browser.as_ref().and_then(|b| b.host()) { h.set_focus(1); }
                    crate::log(&format!("gpui: focustab -> 0x{:x}", t.xid));
                }
            }
            "devtools" => {
                if let Some(h) = self.browser().and_then(|b| b.host()) {
                    let wi = cef::WindowInfo { runtime_style: cef::RuntimeStyle::CHROME, ..Default::default() };
                    let mut client = cefglue::client::new_client_kind(self.shared.clone(), "devtools");
                    h.show_dev_tools(Some(&wi), Some(&mut client), Some(&cef::BrowserSettings::default()), None);
                }
            }
            _ => crate::log(&format!("ctl: unknown command {cmd}")),
        }
    }

    fn open_popup_host(&mut self, alloy: bool, url: &str) {
        if let Some(h) = self.popup_browser.take().and_then(|b| b.host()) {
            h.close_browser(1);
        }
        let l = self.layout.borrow();
        let s = l.scale;
        let (cx0, cy0, cw) = l.content.map(|b| (f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width))).unwrap_or((0., 80., 1280.));
        drop(l);
        let (w, h) = (360.0f32, 300.0f32);
        let r = XRect { x: ((cx0 + cw - w - 8.) * s) as i32, y: ((cy0 + 4.) * s) as i32, w: (w * s) as u32, h: (h * s) as u32 };
        if self.popup_host == 0 {
            self.popup_host = self.x.create_host(self.gpui_xid, r, 0xfafafa).expect("popup host");
        } else {
            self.x.configure(self.popup_host, r);
            self.x.map(self.popup_host, true);
        }
        self.x.raise(self.popup_host);
        let style = if alloy { cef::RuntimeStyle::ALLOY } else { cef::RuntimeStyle::CHROME };
        self.popup_style = style;
        let wi = cef::WindowInfo { runtime_style: style, ..Default::default() }
            .set_as_child(self.popup_host as _, &cef::Rect { x: 0, y: 0, width: w as i32, height: h as i32 });
        let mut client = cefglue::client::new_client_kind(self.shared.clone(), "popuphost");
        let ok = cef::browser_host_create_browser(Some(&wi), Some(&mut client), Some(&cef::CefString::from(url)),
            Some(&cef::BrowserSettings::default()), None, None);
        crate::log(&format!("gpui: popup host 0x{:x} at {r:?}: create_browser style={} url={url} -> {ok}", self.popup_host, if alloy { "alloy" } else { "chrome" }));
    }

    fn dump_state(&self, window: &Window, cx: &App) {
        let mut s = format!(
            "state: key_focus={:?} proxy=0x{:x} active_tab={} gpui_active={} gpui_hovered={} x_focus=0x{:x} gpui_xid=0x{:x} omnibox_focused={} page_focused={} scale={} menu={:?}\n",
            self.key_focus,
            self.proxy_xid,
            self.active,
            window.is_window_active(),
            window.is_window_hovered(),
            self.x.focus(),
            self.gpui_xid,
            self.address.read(cx).focus_handle(cx).is_focused(window),
            self.page_focused,
            window.scale_factor(),
            self.menu
        );
        let l = self.layout.borrow();
        s += &format!("  content={:?} applied={:?} shaped={:?}\n", l.content, l.applied, l.shaped);
        for (i, t) in self.tabs.iter().enumerate() {
            let geo = (t.xid != 0).then(|| self.x.geometry(t.xid)).flatten();
            s += &format!(
                "  tab {i}: id={:?} kind={} style={} xid=0x{:x} geo={:?} ext={} loading={} title={:?} url={}\n",
                t.id, t.kind, t.style, t.xid, geo, t.external, t.loading, t.title, t.url
            );
        }
        crate::log(&s);
    }

    // ---------------------------------------------------------------- overlay

    fn set_menu(&mut self, mode: MenuMode, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = self.popup.take() {
            let _ = p.update(cx, |_, w, _| w.remove_window());
        }
        self.menu = mode;
        if mode == MenuMode::Popup {
            self.open_popup_menu(window, cx);
        }
        if mode != MenuMode::Shape {
            let shaped = self.layout.borrow_mut().shaped.take();
            if let Some((xid, _)) = shaped {
                self.x.shape_reset(xid);
            }
        }
        crate::log(&format!("gpui: menu mode {mode:?}"));
        cx.notify();
    }

    fn menu_rect(&self) -> Option<Bounds<Pixels>> {
        let c = self.layout.borrow().content?;
        // A dropdown hanging from the toolbar's right end, overlapping the page.
        Some(Bounds { origin: point(c.origin.x + c.size.width - px(272.), c.origin.y - px(6.)), size: size(px(260.), px(220.)) })
    }

    fn open_popup_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.menu_rect() else { return };
        let scale = window.scale_factor();
        // Screen position of our toplevel (physical px), then convert to GPUI logical.
        let (sx, sy) = self.x.translate_to_root(self.gpui_xid).unwrap_or((0, 0));
        let origin = point(px(sx as f32 / scale) + r.origin.x, px(sy as f32 / scale) + r.origin.y);
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds { origin, size: r.size })),
            kind: WindowKind::PopUp,
            focus: false,
            titlebar: None,
            ..Default::default()
        };
        match cx.open_window(opts, |_, cx| cx.new(|_| MenuPopup)) {
            Ok(h) => {
                crate::log(&format!("gpui: popup window opened at screen {origin:?}"));
                self.popup = Some(h.into());
            }
            Err(e) => crate::log(&format!("gpui: popup window failed: {e:#}")),
        }
    }
}

fn menu_items() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .children(["New tab", "New window", "History", "Downloads", "Bookmarks", "Settings", "Exit"].into_iter().map(|s| {
            div().px_2().py_1().rounded_md().hover(|d| d.bg(rgb(0xdbeafe))).child(s)
        }))
}

struct MenuPopup;
impl Render for MenuPopup {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0xfff7d6)).border_2().border_color(rgb(0xd97706)).text_color(rgb(0x111111)).child(menu_items())
    }
}

/// Push the content rectangle to the active CEF child (physical px) if it changed,
/// and publish the DIP rectangle for popups. Called from prepaint and on tab switch.
fn apply_geometry(layout: &Rc<RefCell<Layout>>, x: &XConn, xid: u32, shared: &Shared) {
    apply_geometry_crop(layout, x, xid, shared, 0)
}

/// `crop`: pixels of Chrome UI hidden above the host's top edge (an embedded Chrome-created window).
fn apply_geometry_crop(layout: &Rc<RefCell<Layout>>, x: &XConn, xid: u32, shared: &Shared, crop: i32) {
    let mut l = layout.borrow_mut();
    let Some(b) = l.content else { return };
    let s = l.scale;
    // GPUI logical px -> X11 physical px (window-relative).
    let hr = XRect {
        x: (f32::from(b.origin.x) * s).round() as i32,
        y: (f32::from(b.origin.y) * s).round() as i32,
        w: (f32::from(b.size.width) * s).round().max(1.) as u32,
        h: (f32::from(b.size.height) * s).round().max(1.) as u32,
    };
    *shared.content_rect_dip.lock().unwrap() =
        cef::Rect { x: 0, y: 0, width: f32::from(b.size.width) as i32, height: f32::from(b.size.height) as i32 };
    if l.applied_host != Some(hr) {
        x.configure(l.host, hr);
        crate::log(&format!("x11: configure host 0x{:x} -> {}x{}+{}+{} (scale {s})", l.host, hr.w, hr.h, hr.x, hr.y));
        l.applied_host = Some(hr);
    }
    let cr = XRect { x: 0, y: -crop, w: hr.w, h: hr.h + crop as u32 };
    if xid != 0 && l.applied != Some((xid, cr)) {
        x.configure(xid, cr);
        crate::log(&format!("x11: configure child 0x{xid:x} -> {}x{}", cr.w, cr.h));
        l.applied = Some((xid, cr));
    }
}

impl XConn {
    fn conn_parent(&self, xid: u32) -> u32 {
        use x11rb::protocol::xproto::ConnectionExt as _;
        self.conn.query_tree(xid).ok().and_then(|c| c.reply().ok()).map(|t| t.parent).unwrap_or(0)
    }
}

impl Focusable for BrowserWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BrowserWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (active, hovered) = (window.is_window_active(), window.is_window_hovered());
        if self.last_active != Some(active) || self.last_hovered != Some(hovered) {
            crate::log(&format!("gpui: render sees active={active} hovered={hovered}"));
            (self.last_active, self.last_hovered) = (Some(active), Some(hovered));
        }
        self.layout.borrow_mut().scale = window.scale_factor();
        let theme = cx.theme().clone();
        let active_xid = self.tabs.get(self.active).filter(|t| !t.external).map(|t| t.xid).unwrap_or(0);
        let active_crop = self.tabs.get(self.active).filter(|t| t.kind == "default").map(|_| self.shared.embed_crop).unwrap_or(0);
        let tab = self.tabs.get(self.active);
        let (loading, can_back, can_fwd) = tab.map(|t| (t.loading, t.can_back, t.can_fwd)).unwrap_or_default();

        // ---- tab strip
        let tabs = div().flex().flex_row().gap_1().px_2().pt_1().bg(rgb(0xe5e7eb)).children(
            self.tabs.iter().enumerate().map(|(i, t)| {
                let label: SharedString = format!(
                    "{}{} {}",
                    if t.loading { "⟳ " } else { "" },
                    t.title.chars().take(22).collect::<String>(),
                    if t.favicon.is_some() { "★" } else { "" }
                )
                .into();
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
                    .child(
                        div()
                            .id(("close", i))
                            .px_1()
                            .rounded_sm()
                            .hover(|d| d.bg(rgb(0xfca5a5)))
                            .child("×")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.close_tab(i);
                            })),
                    )
            }),
        );
        let tabs = tabs.child(
            Button::new("new-tab").label("+").ghost().compact().on_click(cx.listener(|this, _, window, cx| {
                this.new_tab("about:blank", cx);
                this.focus_omnibox(window, cx);
            })),
        );

        // ---- toolbar
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
                    if let Some(b) = this.browser() {
                        b.stop_load()
                    }
                }))
            } else {
                Button::new("reload").label("⟳").ghost().compact().on_click(cx.listener(|this, _, w, cx| this.do_action(ShellAction::Reload, w, cx)))
            })
            .child(div().flex_1().child(Input::new(&self.address)))
            .child(Button::new("menu").label("☰").ghost().compact().on_click(cx.listener(|this, _, w, cx| {
                let next = if this.menu == MenuMode::Off { MenuMode::Shape } else { MenuMode::Off };
                this.set_menu(next, w, cx);
            })));

        // ---- content area: measures itself and drives the CEF child geometry.
        let layout = self.layout.clone();
        let x = self.x.clone();
        let shared = self.shared.clone();
        let content = div().flex_1().w_full().bg(rgb(0x1f2937)).child(
            canvas(
                move |bounds, window, _cx| {
                    {
                        let mut l = layout.borrow_mut();
                        l.content = Some(bounds);
                        l.scale = window.scale_factor();
                    }
                    apply_geometry_crop(&layout, &x, active_xid, &shared, active_crop);
                },
                |_, _, _, _| {},
            )
            .size_full(),
        );

        // ---- in-window overlay (dropdown) that overlaps the page
        let overlay = if matches!(self.menu, MenuMode::Shape | MenuMode::Plain) {
            self.menu_rect().map(|r| {
                let shape = self.menu == MenuMode::Shape;
                let (layout, x) = (self.layout.clone(), self.x.clone());
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
                                    // Cut the overlay out of the active CEF child (child-relative px).
                                    if !shape {
                                        return;
                                    }
                                    let mut l = layout.borrow_mut();
                                    let (Some(c), s) = (l.content, l.scale) else { return };
                                    let pad = 8.0; // room for the shadow
                                    let hole = XRect {
                                        x: ((f32::from(r.origin.x - c.origin.x) - pad) * s).floor() as i32,
                                        y: ((f32::from(r.origin.y - c.origin.y) - pad) * s).floor() as i32,
                                        w: ((f32::from(r.size.width) + 2. * pad) * s).ceil() as u32,
                                        h: ((f32::from(r.size.height) + 2. * pad) * s).ceil() as u32,
                                    };
                                    let host = l.host;
                                    if l.shaped != Some((host, hole)) {
                                        let full = ((f32::from(c.size.width) * s).round() as u32, (f32::from(c.size.height) * s).round() as u32);
                                        // Shaping the host clips every CEF child inside it.
                                        x.shape_exclude(host, full, &[hole]);
                                        crate::log(&format!("x11: SHAPE host 0x{host:x} hole {hole:?}"));
                                        l.shaped = Some((host, hole));
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

        // ---- status bar
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
            .child(format!("window active={active} hovered={hovered}"))
            .child(format!("page_focus={}", self.page_focused))
            .child(format!("title={title}"))
            .child(format!("favicon={fav}"));

        div()
            .id("browser-window")
            .key_context("BrowserWindow")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            // Any mouse-down that reaches GPUI is on the chrome (page clicks go to the CEF
            // child window). Pull X keyboard focus back from the CEF child.
            .capture_any_mouse_down(cx.listener(|this, _ev, _w, _cx| {
                if this.key_focus != KeyFocus::Chrome || this.x.focus() != this.proxy_xid {
                    crate::log("gpui: chrome click -> keyboard to chrome (XSetInputFocus proxy)");
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
