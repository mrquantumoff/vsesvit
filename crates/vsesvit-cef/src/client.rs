//! Owned by W1.3.
//! The `Client` every browser gets, the browser wrapper the registry holds, and the CEF calls that are
//! not calls on a browser.
//!
//! One client per creation request: it carries the request's `CreationId` into `on_after_created`, so
//! the glue never matches a new browser to a request by order (a popup's client is made in
//! `on_before_popup`; `default_client` serves browsers Chromium creates itself). Each client builds its
//! handlers once and its getters return clones (G8). Handlers hold only `Send + Sync` values: this
//! context and atomics; the rest is in the registry (design section 1.6).

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cef::*;
use vsesvit_engine::{BrowserEvent, Browsing, NativeHandle, Origin, PageChannel};

use crate::contexts::Contexts;
use crate::events::Sink;
use crate::handlers::{
    command, context_menu, dialog, display, download, find, focus, jsdialog, keyboard, life_span, load, permission,
    request,
};
use crate::registry::{self, Backend, BrowserId, BrowserOps, Creation, CreationId};
use crate::{native, strings};

/// What every handler of one client knows. `Send + Sync`.
#[derive(Debug)]
pub struct HandlerCtx {
    /// The creation request this client answers; `None` for `default_client`.
    pub creation: Option<CreationId>,
    /// The event sink, for handlers on threads without the registry.
    pub sink: Sink,
}

/// A client's handlers, built once (G8). The later units' files return `None` until they are built,
/// which leaves Chromium's own behaviour in place.
#[derive(Clone)]
pub struct Handlers {
    life_span: LifeSpanHandler,
    display: DisplayHandler,
    load: LoadHandler,
    keyboard: KeyboardHandler,
    command: CommandHandler,
    focus: FocusHandler,
    request: RequestHandler,
    context_menu: Option<ContextMenuHandler>,
    jsdialog: Option<JsdialogHandler>,
    dialog: Option<DialogHandler>,
    download: Option<DownloadHandler>,
    permission: Option<PermissionHandler>,
    find: Option<FindHandler>,
}

/// A client for a creation (or for `default_client` with `None`).
pub fn build(creation: Option<CreationId>, sink: Sink) -> Client {
    let ctx = Arc::new(HandlerCtx { creation, sink });
    let handlers = Handlers {
        life_span: life_span::LifeSpan::handler(&ctx),
        display: display::Display::handler(&ctx),
        load: load::Load::handler(&ctx),
        keyboard: keyboard::Keyboard::handler(&ctx),
        command: command::Command::handler(&ctx),
        focus: focus::Focus::handler(&ctx),
        request: request::Request::handler(&ctx),
        context_menu: context_menu::handler(&ctx),
        jsdialog: jsdialog::handler(&ctx),
        dialog: dialog::handler(&ctx),
        download: download::handler(&ctx),
        permission: permission::handler(&ctx),
        find: find::handler(&ctx),
    };
    VsesvitClient::new(handlers)
}

wrap_client! {
    pub struct VsesvitClient {
        handlers: Handlers,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(self.handlers.life_span.clone()) }
        fn display_handler(&self) -> Option<DisplayHandler> { Some(self.handlers.display.clone()) }
        fn load_handler(&self) -> Option<LoadHandler> { Some(self.handlers.load.clone()) }
        fn keyboard_handler(&self) -> Option<KeyboardHandler> { Some(self.handlers.keyboard.clone()) }
        fn command_handler(&self) -> Option<CommandHandler> { Some(self.handlers.command.clone()) }
        fn focus_handler(&self) -> Option<FocusHandler> { Some(self.handlers.focus.clone()) }
        fn request_handler(&self) -> Option<RequestHandler> { Some(self.handlers.request.clone()) }
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> { self.handlers.context_menu.clone() }
        fn jsdialog_handler(&self) -> Option<JsdialogHandler> { self.handlers.jsdialog.clone() }
        fn dialog_handler(&self) -> Option<DialogHandler> { self.handlers.dialog.clone() }
        fn download_handler(&self) -> Option<DownloadHandler> { self.handlers.download.clone() }
        fn permission_handler(&self) -> Option<PermissionHandler> { self.handlers.permission.clone() }
        fn find_handler(&self) -> Option<FindHandler> { self.handlers.find.clone() }

        fn on_process_message_received(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, _source_process: ProcessId,
                                       message: Option<&mut ProcessMessage>) -> i32 {
            let (Some(browser), Some(frame), Some(message)) = (browser, frame, message) else { return 0 };
            let name = strings::userfree(&message.name());
            let payload = message.argument_list().map(|a| strings::userfree(&a.string(0))).unwrap_or_default();
            page_message(browser.identifier(), &strings::userfree(&frame.url()), frame.is_main() != 0, &name, payload) as i32
        }
    }
}

// Page messages (design section 1.5, A.12).

/// The page channels and the origins allowed on each. The renderer gets the list in `extra_info`;
/// every message is checked here again, against the browser process's own frame record.
static PAGE_ORIGINS: Mutex<Vec<(PageChannel, String)>> = Mutex::new(Vec::new());

fn channel_message(channel: PageChannel) -> &'static str {
    match channel {
        PageChannel::Store => vsesvit_cef_child::STORE_MESSAGE,
    }
}

fn channel_of(message: &str) -> Option<PageChannel> {
    [PageChannel::Store].into_iter().find(|c| channel_message(*c) == message)
}

fn page_origins() -> Vec<(PageChannel, String)> {
    let mut list = PAGE_ORIGINS.lock().map(|l| l.clone()).unwrap_or_default();
    list.push((PageChannel::Store, "https://chromewebstore.google.com".to_owned()));
    list
}

/// Allows one more origin on a channel, for browsers created from now on. The shell's list is fixed;
/// the CEF smoke run adds its fixture origin with this.
pub fn allow_page_origin(channel: PageChannel, origin: &Origin) {
    if let Ok(mut list) = PAGE_ORIGINS.lock() {
        list.push((channel, origin.as_str().to_owned()));
    }
}

/// The logic of `on_process_message_received`. True when the message was a page message (delivered or
/// dropped); false for a message the glue does not know.
pub fn page_message(browser: BrowserId, frame_url: &str, main_frame: bool, name: &str, payload: String) -> bool {
    let Some(channel) = channel_of(name) else { return false };
    let origin = vsesvit_engine::Url::parse(frame_url).ok().and_then(|u| Origin::of(&u));
    let allowed = origin
        .as_ref()
        .is_some_and(|o| page_origins().iter().any(|(c, allowed)| *c == channel && allowed == o.as_str()));
    let tab = registry::tab_of(browser);
    match (origin, tab) {
        (Some(origin), Some(tab)) if allowed && main_frame => {
            if let Some(sink) = registry::sink() {
                sink.send(BrowserEvent::PageMessage { tab, origin, main_frame, channel, payload });
            }
        }
        _ => log::warn!(
            "vsesvit-cef: dropped a {name} message from browser {browser} (main frame: {main_frame}, allowed origin: {allowed})"
        ),
    }
    true
}

/// The `extra_info` of a browser creation: the page channels' allow lists, for the renderer.
pub fn extra_info() -> Option<DictionaryValue> {
    let extra = dictionary_value_create()?;
    let mut channels = dictionary_value_create()?;
    for channel in [PageChannel::Store] {
        let mut list = list_value_create()?;
        let origins: Vec<String> = page_origins().into_iter().filter(|(c, _)| *c == channel).map(|(_, o)| o).collect();
        list.set_size(origins.len());
        for (i, origin) in origins.iter().enumerate() {
            list.set_string(i, Some(&CefString::from(origin.as_str())));
        }
        channels.set_list(Some(&CefString::from(channel_message(channel))), Some(&mut list));
    }
    extra.set_dictionary(Some(&CefString::from(vsesvit_cef_child::CHANNELS_KEY)), Some(&mut channels));
    Some(extra)
}

// Tasks.

/// A closure for CEF's UI thread.
pub type Job = Box<dyn FnOnce() + Send + 'static>;

wrap_task! {
    pub struct FnTask {
        f: Arc<Mutex<Option<Job>>>,
    }

    impl Task {
        fn execute(&self) {
            let f = self.f.lock().ok().and_then(|mut f| f.take());
            if let Some(f) = f {
                f();
            }
        }
    }
}

/// Runs `f` on CEF's UI thread (the main thread) after `delay`. From any thread.
pub fn post_ui_after(delay: Duration, f: Job) {
    let mut task = FnTask::new(Arc::new(Mutex::new(Some(f))));
    let ok = if delay.is_zero() {
        post_task(ThreadId::UI, Some(&mut task))
    } else {
        post_delayed_task(ThreadId::UI, Some(&mut task), delay.as_millis() as i64)
    };
    if ok == 0 {
        log::error!("vsesvit-cef: post_task(UI) failed");
    }
}

/// Runs `f` on CEF's UI thread. From any thread.
pub fn post_ui(f: Job) {
    post_ui_after(Duration::ZERO, f);
}

// The browser wrapper.

/// A `cef::Browser` as the registry holds it, with its DevTools observer registration, which lives
/// exactly as long as the registry entry (G16: dropped before `cef::shutdown`).
pub struct CefBrowser {
    browser: Browser,
    observer: RefCell<Option<Registration>>,
    sink: Sink,
}

impl CefBrowser {
    /// Wraps a browser CEF handed to `on_after_created`.
    pub fn new(browser: Browser, sink: Sink) -> CefBrowser {
        CefBrowser { browser, observer: RefCell::new(None), sink }
    }

    fn host(&self) -> Option<BrowserHost> {
        self.browser.host()
    }
}

impl BrowserOps for CefBrowser {
    fn native(&self) -> NativeHandle {
        self.host().map(|h| native::native_handle(h.window_handle())).unwrap_or(NativeHandle::from_raw(0))
    }

    fn close(&self, force: bool) {
        if let Some(h) = self.host() {
            h.close_browser(force as i32);
        }
    }

    fn load(&self, url: &str) {
        if let Some(frame) = self.browser.main_frame() {
            frame.load_url(Some(&CefString::from(url)));
        }
    }

    fn reload(&self, bypass_cache: bool) {
        if bypass_cache { self.browser.reload_ignore_cache() } else { self.browser.reload() }
    }

    fn stop(&self) {
        self.browser.stop_load();
    }

    fn go_back(&self) {
        self.browser.go_back();
    }

    fn go_forward(&self) {
        self.browser.go_forward();
    }

    fn was_hidden(&self, hidden: bool) {
        if let Some(h) = self.host() {
            h.was_hidden(hidden as i32);
        }
    }

    fn notify_moved(&self) {
        if let Some(h) = self.host() {
            h.notify_move_or_resize_started();
        }
    }

    fn notify_screen_info_changed(&self) {
        if let Some(h) = self.host() {
            h.notify_screen_info_changed();
        }
    }

    fn set_focus(&self, focus: bool) {
        if let Some(h) = self.host() {
            h.set_focus(focus as i32);
        }
    }

    fn set_zoom_level(&self, level: f64) {
        if let Some(h) = self.host() {
            h.set_zoom_level(level);
        }
    }

    fn zoom_level(&self) -> f64 {
        self.host().map(|h| h.zoom_level()).unwrap_or(0.0)
    }

    fn set_audio_muted(&self, muted: bool) {
        if let Some(h) = self.host() {
            h.set_audio_muted(muted as i32);
        }
    }

    fn print(&self) {
        if let Some(h) = self.host() {
            h.print();
        }
    }

    fn show_devtools(&self, creation: CreationId, inspect_at: Option<(i32, i32)>) {
        let Some(h) = self.host() else { return };
        let mut client = build(Some(creation), self.sink.clone());
        let point = inspect_at.map(|(x, y)| Point { x, y });
        h.show_dev_tools(Some(&native::window_info_toplevel()), Some(&mut client), Some(&BrowserSettings::default()), point.as_ref());
    }

    fn close_devtools(&self) {
        if let Some(h) = self.host() {
            h.close_dev_tools();
        }
    }

    fn has_devtools(&self) -> bool {
        self.host().is_some_and(|h| h.has_dev_tools() != 0)
    }

    fn execute_chrome_command(&self, command: i32) {
        if let Some(h) = self.host() {
            h.execute_chrome_command(command, WindowOpenDisposition::CURRENT_TAB);
        }
    }

    fn exit_fullscreen(&self) {
        if let Some(h) = self.host() {
            h.exit_fullscreen(1);
        }
    }

    fn start_download(&self, url: &str) {
        if let Some(h) = self.host() {
            h.start_download(Some(&CefString::from(url)));
        }
    }

    fn send_key(&self, key: &keyboard::KeyInput) {
        if let Some(h) = self.host() {
            h.send_key_event(Some(&keyboard::to_cef(key)));
        }
    }

    fn devtools_method(&self, message_id: i32, method: &str, params: &serde_json::Value) -> bool {
        let Some(h) = self.host() else { return false };
        if self.observer.borrow().is_none() {
            let mut observer = crate::devtools::Observer::shared();
            *self.observer.borrow_mut() = h.add_dev_tools_message_observer(Some(&mut observer));
        }
        let mut params = parse_json(Some(&CefString::from(params.to_string().as_str())), JsonParserOptions::default())
            .and_then(|v| v.dictionary());
        h.execute_dev_tools_method(message_id, Some(&CefString::from(method)), params.as_mut()) != 0
    }

    fn set_auto_resize(&self, min: (u32, u32), max: (u32, u32)) {
        if let Some(h) = self.host() {
            let size = |(w, h): (u32, u32)| Size { width: w as i32, height: h as i32 };
            h.set_auto_resize_enabled(1, Some(&size(min)), Some(&size(max)));
        }
    }

    fn url(&self) -> String {
        self.browser.main_frame().map(|f| strings::userfree(&f.url())).unwrap_or_default()
    }
}

// The non-browser calls.

/// CEF's non-browser calls (the registry's `Backend`).
pub struct CefBackend {
    sink: Sink,
    contexts: Contexts,
    background_color: u32,
}

impl CefBackend {
    /// The backend for an engine whose pages show `background_color` before their first paint.
    pub fn new(sink: Sink, background_color: u32) -> CefBackend {
        CefBackend { sink, contexts: Contexts::default(), background_color }
    }
}

impl Backend for CefBackend {
    fn create(&self, creation: &Creation) -> bool {
        let info = native::window_info_child(creation.parent, creation.bounds, creation.scale, creation.style);
        let mut client = build(Some(creation.id), self.sink.clone());
        let settings = BrowserSettings {
            background_color: self.background_color,
            // The shell's link preview replaces Chrome's bubble, a separate toplevel on X11 (Q13).
            chrome_status_bubble: State::DISABLED,
            chrome_zoom_bubble: State::DISABLED,
            ..Default::default()
        };
        let mut extra = extra_info();
        let mut context = self.contexts.for_browsing(creation.browsing);
        if creation.browsing == Browsing::Private && context.is_none() {
            return false;
        }
        browser_host_create_browser(
            Some(&info),
            Some(&mut client),
            Some(&CefString::from(creation.url.as_str())),
            Some(&settings),
            extra.as_mut(),
            context.as_mut(),
        ) != 0
    }

    fn post(&self, delay: Duration, task: Job) {
        post_ui_after(delay, task);
    }

    fn set_private_context(&self, on: bool) {
        self.contexts.set_private(on);
    }
}
