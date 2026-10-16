use crate::UiMsg;
use cef::*;
use std::{
    collections::HashMap,
    sync::{mpsc::Sender, Arc, Mutex},
};

/// Shared Rust state reachable from every handler. CEF calls handlers on
/// several threads (UI, IO, renderer...), so it must be Send + Sync.
pub struct Shared {
    pub tx: Mutex<Sender<UiMsg>>,
    pub browsers: Mutex<HashMap<i32, Browser>>,
    pub view_size: Mutex<(i32, i32)>,
    pub scale: Mutex<f32>,
    pub last_frame: Mutex<Option<(Vec<u8>, i32, i32)>>,
    pub pending_prompts: Mutex<HashMap<u64, PermissionPromptCallback>>,
}

impl Shared {
    pub fn new(tx: Sender<UiMsg>) -> Arc<Self> {
        Arc::new(Self {
            tx: Mutex::new(tx),
            browsers: Mutex::default(),
            view_size: Mutex::new((800, 600)),
            scale: Mutex::new(1.0),
            last_frame: Mutex::default(),
            pending_prompts: Mutex::default(),
        })
    }
    fn send(&self, m: UiMsg) {
        let _ = self.tx.lock().unwrap().send(m);
    }
}

fn id_of(b: &Option<&mut Browser>) -> i32 {
    b.as_ref().map(|b| b.identifier()).unwrap_or(-1)
}

pub fn s(v: &str) -> CefString {
    CefString::from(v)
}

/// Create a windowless (OSR, Alloy-style) browser. Call on the UI thread.
pub fn create_osr_browser(tx: Sender<UiMsg>, ctx: Option<RequestContext>, url: &str) -> Option<Browser> {
    let shared = Shared::new(tx);
    let mut client = VsesvitClient::new(shared);
    let window_info = WindowInfo::default().set_as_windowless(0 /* parent XID or 0 */);
    let settings = BrowserSettings {
        windowless_frame_rate: 60,
        background_color: 0xFFFF_FFFF,
        ..Default::default()
    };
    let mut ctx = ctx;
    browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut client),
        Some(&s(url)),
        Some(&settings),
        None,
        ctx.as_mut(),
    )
}

/// Create a windowed browser as an X11 child of `parent_xid` (Chrome or Alloy style).
pub fn create_child_browser(shared: Arc<Shared>, parent_xid: u64, url: &str) -> i32 {
    let mut client = VsesvitClient::new(shared);
    let window_info = WindowInfo {
        runtime_style: RuntimeStyle::CHROME,
        ..Default::default()
    }
    .set_as_child(parent_xid as _, &Rect { x: 0, y: 40, width: 1200, height: 760 });
    browser_host_create_browser(
        Some(&window_info),
        Some(&mut client),
        Some(&s(url)),
        Some(&BrowserSettings::default()),
        None,
        None,
    )
}

wrap_client! {
    pub struct VsesvitClient {
        shared: Arc<Shared>,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(VsesvitLifeSpan::new(self.shared.clone()))
        }
        fn load_handler(&self) -> Option<LoadHandler> {
            Some(VsesvitLoad::new(self.shared.clone()))
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(VsesvitDisplay::new(self.shared.clone()))
        }
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(VsesvitRequest::new(self.shared.clone()))
        }
        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(VsesvitDownload::new(self.shared.clone()))
        }
        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(VsesvitPermission::new(self.shared.clone()))
        }
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(VsesvitContextMenu::new(self.shared.clone()))
        }
        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(VsesvitKeyboard::new(self.shared.clone()))
        }
        fn jsdialog_handler(&self) -> Option<JsdialogHandler> {
            Some(VsesvitJsDialog::new(self.shared.clone()))
        }
        fn dialog_handler(&self) -> Option<DialogHandler> {
            Some(VsesvitDialog::new(self.shared.clone()))
        }
        fn focus_handler(&self) -> Option<FocusHandler> {
            Some(VsesvitFocus::new(self.shared.clone()))
        }
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(VsesvitRender::new(self.shared.clone()))
        }

        // Browser-process side of renderer -> browser IPC.
        fn on_process_message_received(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> i32 {
            let Some(msg) = message else { return 0 };
            let name = CefString::from(&msg.name()).to_string();
            if name != "vsesvit.echo" {
                return 0;
            }
            let Some(args) = msg.argument_list() else { return 0 };
            let (id, text) = (args.int(0), CefString::from(&args.string(1)).to_string());
            // Reply to the same frame.
            if let (Some(frame), Some(mut reply)) = (frame, process_message_create(Some(&s("vsesvit.echo.reply")))) {
                if let Some(list) = reply.argument_list() {
                    list.set_int(0, id);
                    list.set_string(1, Some(&s(&text)));
                }
                frame.send_process_message(ProcessId::RENDERER, Some(&mut reply));
            }
            1
        }
    }
}

wrap_life_span_handler! {
    pub struct VsesvitLifeSpan {
        shared: Arc<Shared>,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser {
                self.shared.browsers.lock().unwrap().insert(b.identifier(), b.clone());
            }
        }

        // Return 1 to cancel the popup (e.g. open it as a GPUI tab instead).
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: i32,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: i32,
            _popup_features: Option<&PopupFeatures>,
            window_info: Option<&mut WindowInfo>,
            client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut i32>,
        ) -> i32 {
            let _url = target_url.map(|u| u.to_string()).unwrap_or_default();
            // Option A: let it open, but windowless and with our client.
            if let (Some(wi), Some(c)) = (window_info, client) {
                *wi = WindowInfo::default().set_as_windowless(0);
                *c = Some(VsesvitClient::new(self.shared.clone()));
            }
            0
        }

        fn do_close(&self, _browser: Option<&mut Browser>) -> i32 {
            0 // allow close
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser {
                self.shared.browsers.lock().unwrap().remove(&b.identifier());
            }
        }
    }
}

wrap_load_handler! {
    pub struct VsesvitLoad {
        shared: Arc<Shared>,
    }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            _is_loading: i32,
            _can_go_back: i32,
            _can_go_forward: i32,
        ) {
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            error_code: Errorcode,
            _error_text: Option<&CefString>,
            _failed_url: Option<&CefString>,
        ) {
            if sys::cef_errorcode_t::from(error_code) == sys::cef_errorcode_t::ERR_ABORTED {
                return;
            }
        }
    }
}

wrap_display_handler! {
    pub struct VsesvitDisplay {
        shared: Arc<Shared>,
    }

    impl DisplayHandler {
        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            let browser_id = id_of(&browser);
            self.shared.send(UiMsg::Title {
                browser_id,
                title: title.map(|t| t.to_string()).unwrap_or_default(),
            });
        }

        fn on_address_change(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            if frame.map(|f| f.is_main() != 0).unwrap_or(false) {
                let browser_id = id_of(&browser);
                self.shared.send(UiMsg::Address {
                    browser_id,
                    url: url.map(|u| u.to_string()).unwrap_or_default(),
                });
            }
        }

        fn on_favicon_urlchange(&self, _browser: Option<&mut Browser>, icon_urls: Option<&mut CefStringList>) {
            let _urls: Vec<String> = icon_urls
                .map(|l| l.clone().into_iter().collect())
                .unwrap_or_default();
        }

        fn on_cursor_change(
            &self,
            _browser: Option<&mut Browser>,
            _cursor: std::os::raw::c_ulong,
            _type_: CursorType,
            _custom_cursor_info: Option<&CursorInfo>,
        ) -> i32 {
            1 // handled: map CursorType to a GPUI cursor
        }
    }
}

wrap_request_handler! {
    pub struct VsesvitRequest {
        shared: Arc<Shared>,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: i32,
            _is_redirect: i32,
        ) -> i32 {
            let _url = request.map(|r| CefString::from(&r.url()).to_string());
            0 // 0 = allow navigation
        }

        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: i32,
            _is_download: i32,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut i32>,
        ) -> Option<ResourceRequestHandler> {
            Some(VsesvitResourceRequest::new(self.shared.clone()))
        }

        fn on_certificate_error(
            &self,
            _browser: Option<&mut Browser>,
            _cert_error: Errorcode,
            _request_url: Option<&CefString>,
            _ssl_info: Option<&mut Sslinfo>,
            _callback: Option<&mut Callback>,
        ) -> i32 {
            0 // cancel; Chrome-style shows its interstitial
        }

        fn on_render_process_terminated(
            &self,
            _browser: Option<&mut Browser>,
            _status: TerminationStatus,
            _error_code: i32,
            _error_string: Option<&CefString>,
        ) {
        }
    }
}

wrap_resource_request_handler! {
    pub struct VsesvitResourceRequest {
        shared: Arc<Shared>,
    }

    impl ResourceRequestHandler {
        // IO thread.
        fn on_before_resource_load(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            if let Some(r) = request {
                r.set_header_by_name(Some(&s("DNT")), Some(&s("1")), 1);
            }
            ReturnValue::CONTINUE
        }

        fn on_protocol_execution(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            allow_os_execution: Option<&mut i32>,
        ) {
            if let Some(a) = allow_os_execution {
                *a = 0;
            }
        }
    }
}

wrap_download_handler! {
    pub struct VsesvitDownload {
        shared: Arc<Shared>,
    }

    impl DownloadHandler {
        fn can_download(
            &self,
            _browser: Option<&mut Browser>,
            _url: Option<&CefString>,
            _request_method: Option<&CefString>,
        ) -> i32 {
            1
        }

        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            _download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> i32 {
            let name = suggested_name.map(|n| n.to_string()).unwrap_or_default();
            if let Some(cb) = callback {
                // Keep a clone if you need to ask the user first (async is allowed).
                cb.cont(Some(&s(&format!("/home/user/Downloads/{name}"))), 0);
            }
            1 // handled
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            _callback: Option<&mut DownloadItemCallback>,
        ) {
            if let Some(item) = download_item {
                let _pct = item.percent_complete();
                let _done = item.is_complete() != 0;
            }
        }
    }
}

wrap_permission_handler! {
    pub struct VsesvitPermission {
        shared: Arc<Shared>,
    }

    impl PermissionHandler {
        fn on_request_media_access_permission(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _requesting_origin: Option<&CefString>,
            _requested_permissions: u32,
            callback: Option<&mut MediaAccessCallback>,
        ) -> i32 {
            if let Some(cb) = callback {
                cb.cancel();
            }
            1
        }

        fn on_show_permission_prompt(
            &self,
            _browser: Option<&mut Browser>,
            prompt_id: u64,
            _requesting_origin: Option<&CefString>,
            _requested_permissions: u32,
            callback: Option<&mut PermissionPromptCallback>,
        ) -> i32 {
            if let Some(cb) = callback {
                // Clone = add_ref; resolve later from GPUI with cb.cont(PermissionRequestResult::ACCEPT).
                self.shared.pending_prompts.lock().unwrap().insert(prompt_id, cb.clone());
            }
            1
        }

        fn on_dismiss_permission_prompt(
            &self,
            _browser: Option<&mut Browser>,
            prompt_id: u64,
            _result: PermissionRequestResult,
        ) {
            self.shared.pending_prompts.lock().unwrap().remove(&prompt_id);
        }
    }
}

wrap_context_menu_handler! {
    pub struct VsesvitContextMenu {
        shared: Arc<Shared>,
    }

    impl ContextMenuHandler {
        fn on_before_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            if let Some(m) = model {
                m.add_separator();
                // Version-safe numeric id (raw sys::IDC_* numbers change between CEF versions).
                let id = unsafe { sys::cef_id_for_command_id_name(cef::resources::IDC_VIEW_SOURCE.as_ptr()) };
                if id >= 0 {
                    m.add_item(id, Some(&s("View source")));
                }
            }
        }

        // Return 1 and later call callback.cont(command_id, flags) to show a GPUI menu.
        fn run_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
            callback: Option<&mut RunContextMenuCallback>,
        ) -> i32 {
            let (Some(p), Some(m), Some(cb)) = (params, model, callback) else { return 0 };
            let _at = (p.xcoord(), p.ycoord());
            let _items: Vec<(i32, String)> = (0..m.count())
                .map(|i| (m.command_id_at(i), CefString::from(&m.label_at(i)).to_string()))
                .collect();
            cb.cancel();
            1
        }
    }
}

wrap_keyboard_handler! {
    pub struct VsesvitKeyboard {
        shared: Arc<Shared>,
    }

    impl KeyboardHandler {
        fn on_pre_key_event(
            &self,
            _browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            _os_event: Option<&mut sys::XEvent>,
            is_keyboard_shortcut: Option<&mut i32>,
        ) -> i32 {
            let Some(e) = event else { return 0 };
            // Ctrl+L (windows_key_code 'L') -> let the GPUI omnibox take it.
            if e.windows_key_code == 'L' as i32 && (e.modifiers & sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0) != 0 {
                if let Some(k) = is_keyboard_shortcut {
                    *k = 1;
                }
            }
            0
        }
    }
}

wrap_jsdialog_handler! {
    pub struct VsesvitJsDialog {
        shared: Arc<Shared>,
    }

    impl JsdialogHandler {
        fn on_jsdialog(
            &self,
            _browser: Option<&mut Browser>,
            _origin_url: Option<&CefString>,
            _dialog_type: JsdialogType,
            _message_text: Option<&CefString>,
            _default_prompt_text: Option<&CefString>,
            callback: Option<&mut JsdialogCallback>,
            _suppress_message: Option<&mut i32>,
        ) -> i32 {
            if let Some(cb) = callback {
                cb.cont(1, None); // show a GPUI dialog, then cont(success, input)
            }
            1
        }

        fn on_before_unload_dialog(
            &self,
            _browser: Option<&mut Browser>,
            _message_text: Option<&CefString>,
            _is_reload: i32,
            callback: Option<&mut JsdialogCallback>,
        ) -> i32 {
            if let Some(cb) = callback {
                cb.cont(1, None);
            }
            1
        }
    }
}

wrap_dialog_handler! {
    pub struct VsesvitDialog {
        shared: Arc<Shared>,
    }

    impl DialogHandler {
        fn on_file_dialog(
            &self,
            _browser: Option<&mut Browser>,
            _mode: FileDialogMode,
            _title: Option<&CefString>,
            _default_file_path: Option<&CefString>,
            _accept_filters: Option<&mut CefStringList>,
            _accept_extensions: Option<&mut CefStringList>,
            _accept_descriptions: Option<&mut CefStringList>,
            callback: Option<&mut FileDialogCallback>,
        ) -> i32 {
            if let Some(cb) = callback {
                let mut paths = CefStringList::new();
                paths.append("/home/user/file.txt");
                cb.cont(Some(&mut paths));
            }
            1
        }
    }
}

wrap_focus_handler! {
    pub struct VsesvitFocus {
        shared: Arc<Shared>,
    }

    impl FocusHandler {
        fn on_set_focus(&self, _browser: Option<&mut Browser>, _source: FocusSource) -> i32 {
            0 // allow
        }
        fn on_got_focus(&self, _browser: Option<&mut Browser>) {}
        fn on_take_focus(&self, _browser: Option<&mut Browser>, _next: i32) {}
    }
}

wrap_render_handler! {
    pub struct VsesvitRender {
        shared: Arc<Shared>,
    }

    impl RenderHandler {
        // DIP size of the view; must be non-zero.
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(r) = rect {
                let (w, h) = *self.shared.view_size.lock().unwrap();
                *r = Rect { x: 0, y: 0, width: w.max(1), height: h.max(1) };
            }
        }

        fn screen_info(&self, _browser: Option<&mut Browser>, info: Option<&mut ScreenInfo>) -> i32 {
            let Some(i) = info else { return 0 };
            i.device_scale_factor = *self.shared.scale.lock().unwrap();
            1
        }

        // UI thread. BGRA, width*height*4 bytes, upper-left origin.
        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: i32,
            height: i32,
        ) {
            if type_ != PaintElementType::VIEW || buffer.is_null() {
                return;
            }
            let len = (width * height * 4) as usize;
            let bytes = unsafe { std::slice::from_raw_parts(buffer, len) }.to_vec();
            *self.shared.last_frame.lock().unwrap() = Some((bytes, width, height));
        }
    }
}

/// Input forwarding from the UI toolkit into an OSR browser.
pub fn forward_input(browser: &Browser) {
    let Some(host) = browser.host() else { return };
    let ev = MouseEvent { x: 10, y: 20, modifiers: 0 };
    host.send_mouse_move_event(Some(&ev), 0);
    host.send_mouse_click_event(Some(&ev), MouseButtonType::LEFT, 0, 1);
    host.send_mouse_click_event(Some(&ev), MouseButtonType::LEFT, 1, 1);
    host.send_mouse_wheel_event(Some(&ev), 0, -120);
    let key = KeyEvent {
        type_: KeyEventType::CHAR,
        windows_key_code: 'a' as i32,
        character: 'a' as u16,
        unmodified_character: 'a' as u16,
        ..Default::default()
    };
    host.send_key_event(Some(&key));
    host.set_focus(1);
    host.was_resized();
    host.was_hidden(0);
    host.notify_screen_info_changed();
}

/// Preferred shape: build handler objects once, hand out cheap RefGuard clones
/// (CEF calls client.get_*_handler() on every event).
wrap_client! {
    pub struct CachedClient {
        display: DisplayHandler,
        life_span: LifeSpanHandler,
        render: RenderHandler,
    }

    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.display.clone())
        }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(self.life_span.clone())
        }
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render.clone())
        }
    }
}

pub fn cached_client(shared: Arc<Shared>) -> Client {
    CachedClient::new(
        VsesvitDisplay::new(shared.clone()),
        VsesvitLifeSpan::new(shared.clone()),
        VsesvitRender::new(shared),
    )
}
