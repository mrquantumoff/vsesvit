//! `CefClient` and the handlers the shell needs. Handlers are built once and
//! cached in the client (libcef calls the getters on every event).
use super::{cef_str, CefEvent, DialogMode, KeyMode, NativeHandle, PopupMode, Shared, ShellAction};
use cef::*;
use std::sync::Arc;
use windows_sys::Win32::UI::WindowsAndMessaging::{WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_TABSTOP, WS_VISIBLE};

fn bid(b: &Option<&mut Browser>) -> i32 {
    b.as_ref().map(|b| b.identifier()).unwrap_or(-1)
}

pub fn new_client(shared: Arc<Shared>) -> Client {
    SpikeClient::new(
        SpikeLifeSpan::new(shared.clone()),
        SpikeDisplay::new(shared.clone()),
        SpikeLoad::new(shared.clone()),
        SpikeKeyboard::new(shared.clone()),
        SpikeCommand::new(shared.clone()),
        SpikeFocus::new(shared.clone()),
        SpikeJsDialog::new(shared.clone()),
        SpikeDialog::new(shared.clone()),
        SpikeDownload::new(shared.clone()),
        SpikePermission::new(shared.clone()),
    )
}

wrap_client! {
    pub struct SpikeClient {
        life_span: LifeSpanHandler,
        display: DisplayHandler,
        load: LoadHandler,
        keyboard: KeyboardHandler,
        command: CommandHandler,
        focus: FocusHandler,
        jsdialog: JsdialogHandler,
        dialog: DialogHandler,
        download: DownloadHandler,
        permission: PermissionHandler,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(self.life_span.clone()) }
        fn display_handler(&self) -> Option<DisplayHandler> { Some(self.display.clone()) }
        fn load_handler(&self) -> Option<LoadHandler> { Some(self.load.clone()) }
        fn keyboard_handler(&self) -> Option<KeyboardHandler> { Some(self.keyboard.clone()) }
        fn command_handler(&self) -> Option<CommandHandler> { Some(self.command.clone()) }
        fn focus_handler(&self) -> Option<FocusHandler> { Some(self.focus.clone()) }
        fn jsdialog_handler(&self) -> Option<JsdialogHandler> { Some(self.jsdialog.clone()) }
        fn dialog_handler(&self) -> Option<DialogHandler> { Some(self.dialog.clone()) }
        fn download_handler(&self) -> Option<DownloadHandler> { Some(self.download.clone()) }
        fn permission_handler(&self) -> Option<PermissionHandler> { Some(self.permission.clone()) }
    }
}

wrap_life_span_handler! {
    pub struct SpikeLifeSpan { shared: Arc<Shared> }

    impl LifeSpanHandler {
        fn on_before_popup(&self, browser: Option<&mut Browser>, _frame: Option<&mut Frame>, popup_id: i32,
            target_url: Option<&CefString>, _target_frame_name: Option<&CefString>, target_disposition: WindowOpenDisposition,
            user_gesture: i32, _popup_features: Option<&PopupFeatures>, window_info: Option<&mut WindowInfo>,
            client: Option<&mut Option<Client>>, _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>, _no_javascript_access: Option<&mut i32>) -> i32 {
            let opener = bid(&browser);
            let url = cef_str(target_url);
            self.shared.log(format!("cef: on_before_popup opener={opener} popup_id={popup_id} url={url} disposition={} gesture={user_gesture} mode={:?}",
                target_disposition.get_raw(), self.shared.popup_mode));
            match self.shared.popup_mode {
                PopupMode::Default => 0,
                PopupMode::CancelNewTab => {
                    self.shared.send(CefEvent::PopupAsTab { opener_id: opener, url });
                    1
                }
                PopupMode::Child => {
                    // Re-parent the popup into the GPUI window. Without a parent CEF opens a
                    // Views top-level window (chrome_child_window.cc OnPopupBrowserViewCreated).
                    let parent = self.shared.parent();
                    let rect = self.shared.content_rect_dip.lock().unwrap().clone();
                    if let Some(wi) = window_info {
                        // Assign field by field: replacing the whole struct would also write back
                        // `window_name` (a CefString), which is dropped by the out-param glue (G3).
                        wi.parent_window = sys::HWND(parent as usize as *mut _);
                        wi.style = WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS | WS_TABSTOP | WS_VISIBLE;
                        wi.ex_style = 0;
                        wi.bounds = rect;
                        wi.runtime_style = RuntimeStyle::CHROME;
                        wi.windowless_rendering_enabled = 0;
                    }
                    if let Some(c) = client {
                        *c = Some(new_client(self.shared.clone()));
                    }
                    0
                }
            }
        }
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(b) = browser else { return };
            let host = b.host();
            let hwnd: NativeHandle = host.as_ref().map(|h| h.window_handle().0 as usize as u64).unwrap_or(0);
            let opener_id = host.as_ref().map(|h| h.opener_identifier()).unwrap_or(0);
            let style = host.as_ref().map(|h| h.runtime_style().get_raw()).unwrap_or(99);
            self.shared.log(format!("cef: on_after_created id={} popup={} hwnd=0x{hwnd:x} opener={opener_id} runtime_style={style} on_ui_thread={}",
                b.identifier(), b.is_popup(), currently_on(ThreadId::UI)));
            self.shared.send(CefEvent::AfterCreated { id: b.identifier(), browser: b.clone(), is_popup: b.is_popup() != 0, hwnd, opener_id });
        }
        fn do_close(&self, browser: Option<&mut Browser>) -> i32 {
            self.shared.log(format!("cef: do_close id={}", bid(&browser)));
            0
        }
        fn on_before_close(&self, browser: Option<&mut Browser>) {
            let id = bid(&browser);
            self.shared.log(format!("cef: on_before_close id={id}"));
            self.shared.send(CefEvent::BeforeClose { id });
        }
    }
}

wrap_display_handler! {
    pub struct SpikeDisplay { shared: Arc<Shared> }

    impl DisplayHandler {
        fn on_address_change(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            if frame.map(|f| f.is_main() != 0).unwrap_or(false) {
                self.shared.send(CefEvent::Address { id: bid(&browser), url: cef_str(url) });
            }
        }
        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            self.shared.send(CefEvent::Title { id: bid(&browser), title: cef_str(title) });
        }
        fn on_favicon_urlchange(&self, browser: Option<&mut Browser>, icon_urls: Option<&mut CefStringList>) {
            // Take the list (cloning a lent CefStringList loses its pointer, G5).
            let urls: Vec<String> = icon_urls.map(|l| std::mem::take(l).into_iter().collect()).unwrap_or_default();
            self.shared.send(CefEvent::Favicon { id: bid(&browser), urls });
        }
        fn on_fullscreen_mode_change(&self, browser: Option<&mut Browser>, fullscreen: i32) {
            self.shared.send(CefEvent::Fullscreen { id: bid(&browser), on: fullscreen != 0 });
        }
        fn on_console_message(&self, browser: Option<&mut Browser>, _level: LogSeverity, message: Option<&CefString>,
                              _source: Option<&CefString>, _line: i32) -> i32 {
            self.shared.send(CefEvent::Console { id: bid(&browser), message: cef_str(message) });
            0
        }
    }
}

wrap_load_handler! {
    pub struct SpikeLoad { shared: Arc<Shared> }

    impl LoadHandler {
        fn on_loading_state_change(&self, browser: Option<&mut Browser>, is_loading: i32, can_go_back: i32, can_go_forward: i32) {
            self.shared.send(CefEvent::LoadingState { id: bid(&browser), loading: is_loading != 0, can_back: can_go_back != 0, can_fwd: can_go_forward != 0 });
        }
        fn on_load_end(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, http_status_code: i32) {
            if frame.map(|f| f.is_main() != 0).unwrap_or(false) {
                self.shared.send(CefEvent::LoadEnd { id: bid(&browser), status: http_status_code });
            }
        }
        fn on_load_error(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, error_code: Errorcode,
                         error_text: Option<&CefString>, failed_url: Option<&CefString>) {
            if frame.map(|f| f.is_main() != 0).unwrap_or(false) {
                self.shared.send(CefEvent::LoadError { id: bid(&browser), code: error_code.get_raw() as i32, text: cef_str(error_text), url: cef_str(failed_url) });
            }
        }
    }
}

// Windows virtual-key codes (ui/events/keycodes/keyboard_codes.h).
const VK_TAB: i32 = 0x09;
const VK_LEFT: i32 = 0x25;
const VK_RIGHT: i32 = 0x27;
const VK_F4: i32 = 0x73;
const VK_F5: i32 = 0x74;
const VK_F6: i32 = 0x75;
const VK_F11: i32 = 0x7A;
const VK_F12: i32 = 0x7B;

/// Shortcuts the shell owns even while the page has focus (Chrome's reserved set plus a few).
pub fn shortcut_for(ev: &KeyEvent) -> Option<ShellAction> {
    let ctrl = ev.modifiers & (sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0 as u32) != 0;
    let shift = ev.modifiers & (sys::cef_event_flags_t::EVENTFLAG_SHIFT_DOWN.0 as u32) != 0;
    let alt = ev.modifiers & (sys::cef_event_flags_t::EVENTFLAG_ALT_DOWN.0 as u32) != 0;
    let k = ev.windows_key_code;
    let ch = |c: char| k == c as i32;
    Some(match () {
        _ if ctrl && !shift && ch('L') => ShellAction::FocusOmnibox,
        _ if alt && ch('D') => ShellAction::FocusOmnibox,
        _ if k == VK_F6 => ShellAction::FocusOmnibox,
        _ if ctrl && !shift && ch('T') => ShellAction::NewTab,
        _ if ctrl && shift && ch('T') => ShellAction::ReopenTab,
        _ if ctrl && !shift && ch('N') => ShellAction::NewWindow,
        _ if ctrl && !shift && ch('W') => ShellAction::CloseTab,
        _ if ctrl && k == VK_F4 => ShellAction::CloseTab,
        _ if k == VK_F11 => ShellAction::Fullscreen,
        _ if ctrl && !shift && ch('P') => ShellAction::Print,
        _ if ctrl && !shift && ch('F') => ShellAction::Find,
        _ if k == VK_F12 => ShellAction::DevTools,
        _ if ctrl && shift && ch('I') => ShellAction::DevTools,
        _ if ctrl && !shift && ch('U') => ShellAction::ViewSource,
        _ if k == VK_F5 => ShellAction::Reload,
        _ if ctrl && !shift && ch('R') => ShellAction::Reload,
        _ if alt && k == VK_LEFT => ShellAction::Back,
        _ if alt && k == VK_RIGHT => ShellAction::Forward,
        _ if ctrl && !shift && k == VK_TAB => ShellAction::NextTab,
        _ if ctrl && shift && k == VK_TAB => ShellAction::PrevTab,
        _ => return None,
    })
}

wrap_keyboard_handler! {
    pub struct SpikeKeyboard { shared: Arc<Shared> }

    impl KeyboardHandler {
        // Called before the page sees the key. Return 1 = consumed (the page never sees it).
        fn on_pre_key_event(&self, browser: Option<&mut Browser>, event: Option<&KeyEvent>,
                            _os_event: Option<&mut sys::MSG>, _is_keyboard_shortcut: Option<&mut i32>) -> i32 {
            let Some(e) = event else { return 0 };
            let id = bid(&browser);
            let t = e.type_.get_raw();
            if t == KeyEventType::RAWKEYDOWN.get_raw() || t == KeyEventType::KEYDOWN.get_raw() {
                self.shared.log(format!("cef: on_pre_key_event id={id} type={t} vk=0x{:x} native={} mods=0x{:x}",
                    e.windows_key_code, e.native_key_code, e.modifiers));
            }
            if !matches!(self.shared.key_mode, KeyMode::PreKey | KeyMode::Both) { return 0; }
            let Some(action) = shortcut_for(e) else { return 0 };
            if t == KeyEventType::RAWKEYDOWN.get_raw() || t == KeyEventType::KEYDOWN.get_raw() {
                self.shared.send(CefEvent::Action { id, action, via: "on_pre_key_event" });
            }
            1 // swallow down/up/char of shell shortcuts
        }
        fn on_key_event(&self, browser: Option<&mut Browser>, event: Option<&KeyEvent>, _os_event: Option<&mut sys::MSG>) -> i32 {
            if let Some(e) = event {
                let t = e.type_.get_raw();
                if t == KeyEventType::RAWKEYDOWN.get_raw() {
                    self.shared.log(format!("cef: on_key_event (unhandled by page) id={} vk=0x{:x} mods=0x{:x}", bid(&browser), e.windows_key_code, e.modifiers));
                }
            }
            0
        }
    }
}

wrap_command_handler! {
    pub struct SpikeCommand { shared: Arc<Shared> }

    impl CommandHandler {
        fn on_chrome_command(&self, browser: Option<&mut Browser>, command_id: i32, disposition: WindowOpenDisposition) -> i32 {
            let id = bid(&browser);
            let name = self.shared.idc_name(command_id);
            self.shared.log(format!("cef: on_chrome_command id={id} cmd={command_id} ({name}) disposition={}", disposition.get_raw()));
            if !matches!(self.shared.key_mode, KeyMode::Command | KeyMode::Both) { return 0; }
            let action = match name.as_str() {
                "IDC_NEW_TAB" => Some(ShellAction::NewTab),
                "IDC_NEW_WINDOW" | "IDC_NEW_INCOGNITO_WINDOW" => Some(ShellAction::NewWindow),
                "IDC_CLOSE_TAB" | "IDC_CLOSE_WINDOW" => Some(ShellAction::CloseTab),
                "IDC_RESTORE_TAB" => Some(ShellAction::ReopenTab),
                "IDC_FULLSCREEN" => Some(ShellAction::Fullscreen),
                "IDC_PRINT" | "IDC_BASIC_PRINT" => Some(ShellAction::Print),
                "IDC_FIND" | "IDC_FIND_NEXT" | "IDC_FIND_PREVIOUS" => Some(ShellAction::Find),
                "IDC_DEV_TOOLS" | "IDC_DEV_TOOLS_CONSOLE" | "IDC_DEV_TOOLS_TOGGLE" | "IDC_DEV_TOOLS_INSPECT" | "IDC_DEV_TOOLS_DEVICES" => Some(ShellAction::DevTools),
                "IDC_FOCUS_LOCATION" | "IDC_FOCUS_SEARCH" | "IDC_FOCUS_TOOLBAR" => Some(ShellAction::FocusOmnibox),
                "IDC_RELOAD" | "IDC_RELOAD_BYPASSING_CACHE" | "IDC_RELOAD_CLEARING_CACHE" => Some(ShellAction::Reload),
                "IDC_BACK" => Some(ShellAction::Back),
                "IDC_FORWARD" => Some(ShellAction::Forward),
                "IDC_SELECT_NEXT_TAB" => Some(ShellAction::NextTab),
                "IDC_SELECT_PREVIOUS_TAB" => Some(ShellAction::PrevTab),
                "IDC_VIEW_SOURCE" => Some(ShellAction::ViewSource),
                _ => None,
            };
            match action {
                Some(action) => { self.shared.send(CefEvent::Action { id, action, via: "on_chrome_command" }); 1 }
                // Block every other Chrome command that would open Chrome-native UI; let
                // harmless page-level ones (zoom, copy...) through.
                None => {
                    let allow = name.starts_with("IDC_ZOOM") || name.starts_with("IDC_CONTENT_CONTEXT") || name == "IDC_STOP"
                        || name.starts_with("IDC_CUT") || name.starts_with("IDC_COPY") || name.starts_with("IDC_PASTE");
                    if allow { 0 } else { 1 }
                }
            }
        }
    }
}

wrap_focus_handler! {
    pub struct SpikeFocus { shared: Arc<Shared> }

    impl FocusHandler {
        fn on_take_focus(&self, browser: Option<&mut Browser>, next: i32) {
            self.shared.send(CefEvent::TakeFocus { id: bid(&browser), next: next != 0 });
        }
        // source: 0 = navigation, 1 = system. Return 1 to refuse focus (the shell's
        // omnibox owns the keyboard, e.g. after Ctrl+T while about:blank loads).
        fn on_set_focus(&self, browser: Option<&mut Browser>, source: FocusSource) -> i32 {
            let allowed = self.shared.page_focus_allowed.load(std::sync::atomic::Ordering::SeqCst);
            self.shared.send(CefEvent::SetFocus { id: bid(&browser), source: source.get_raw() as i32 });
            if allowed { 0 } else { 1 }
        }
        fn on_got_focus(&self, browser: Option<&mut Browser>) {
            self.shared.send(CefEvent::GotFocus { id: bid(&browser) });
        }
    }
}

wrap_jsdialog_handler! {
    pub struct SpikeJsDialog { shared: Arc<Shared> }

    impl JsdialogHandler {
        fn on_jsdialog(&self, browser: Option<&mut Browser>, origin_url: Option<&CefString>, dialog_type: JsdialogType,
                       message_text: Option<&CefString>, default_prompt_text: Option<&CefString>,
                       callback: Option<&mut JsdialogCallback>, _suppress_message: Option<&mut i32>) -> i32 {
            let id = bid(&browser);
            self.shared.log(format!("cef: on_jsdialog id={id} type={} origin={} msg={:?} mode={:?}",
                dialog_type.get_raw(), cef_str(origin_url), cef_str(message_text), self.shared.dialog_mode));
            if self.shared.dialog_mode == DialogMode::Default { return 0; }
            let Some(cb) = callback else { return 0 };
            self.shared.send(CefEvent::JsDialog { id, kind: dialog_type.get_raw() as i32, message: cef_str(message_text),
                default_text: cef_str(default_prompt_text), callback: cb.clone() });
            1
        }
        fn on_before_unload_dialog(&self, browser: Option<&mut Browser>, _message_text: Option<&CefString>, is_reload: i32,
                                   _callback: Option<&mut JsdialogCallback>) -> i32 {
            self.shared.log(format!("cef: on_before_unload_dialog id={} reload={is_reload}", bid(&browser)));
            0
        }
    }
}

wrap_dialog_handler! {
    pub struct SpikeDialog { shared: Arc<Shared> }

    impl DialogHandler {
        fn on_file_dialog(&self, browser: Option<&mut Browser>, mode: FileDialogMode, title: Option<&CefString>,
                          _default_file_path: Option<&CefString>, _accept_filters: Option<&mut CefStringList>,
                          _accept_extensions: Option<&mut CefStringList>, _accept_descriptions: Option<&mut CefStringList>,
                          callback: Option<&mut FileDialogCallback>) -> i32 {
            let id = bid(&browser);
            self.shared.log(format!("cef: on_file_dialog id={id} mode={} title={:?} dialogs={:?}", mode.get_raw(), cef_str(title), self.shared.dialog_mode));
            if self.shared.dialog_mode == DialogMode::Default { return 0; }
            let Some(cb) = callback else { return 0 };
            self.shared.send(CefEvent::FileDialog { id, mode: mode.get_raw() as i32, title: cef_str(title), callback: cb.clone() });
            1
        }
    }
}

wrap_download_handler! {
    pub struct SpikeDownload { shared: Arc<Shared> }

    impl DownloadHandler {
        fn can_download(&self, browser: Option<&mut Browser>, url: Option<&CefString>, _request_method: Option<&CefString>) -> i32 {
            self.shared.log(format!("cef: can_download id={} url={}", bid(&browser), cef_str(url)));
            1
        }
        fn on_before_download(&self, browser: Option<&mut Browser>, download_item: Option<&mut DownloadItem>,
                              suggested_name: Option<&CefString>, callback: Option<&mut BeforeDownloadCallback>) -> i32 {
            let id = bid(&browser);
            let name = cef_str(suggested_name);
            let url = download_item.map(|d| CefString::from(&d.url()).to_string()).unwrap_or_default();
            self.shared.log(format!("cef: on_before_download id={id} name={name} url={url} mode={:?}", self.shared.dialog_mode));
            if self.shared.dialog_mode == DialogMode::Default { return 0; }
            if let Some(cb) = callback {
                let path = format!("{}/{name}", self.shared.download_dir);
                cb.cont(Some(&CefString::from(path.as_str())), 0);
            }
            self.shared.send(CefEvent::Download { id, name, url });
            1
        }
        fn on_download_updated(&self, _browser: Option<&mut Browser>, download_item: Option<&mut DownloadItem>,
                               _callback: Option<&mut DownloadItemCallback>) {
            if let Some(d) = download_item {
                if d.is_complete() != 0 {
                    self.shared.log(format!("cef: download complete path={}", CefString::from(&d.full_path()).to_string()));
                }
            }
        }
    }
}

wrap_permission_handler! {
    pub struct SpikePermission { shared: Arc<Shared> }

    impl PermissionHandler {
        fn on_show_permission_prompt(&self, browser: Option<&mut Browser>, prompt_id: u64, requesting_origin: Option<&CefString>,
                                     requested_permissions: u32, callback: Option<&mut PermissionPromptCallback>) -> i32 {
            let id = bid(&browser);
            let origin = cef_str(requesting_origin);
            self.shared.log(format!("cef: on_show_permission_prompt id={id} prompt={prompt_id} origin={origin} perms=0x{requested_permissions:x} mode={:?}", self.shared.dialog_mode));
            if self.shared.dialog_mode == DialogMode::Default { return 0; }
            let Some(cb) = callback else { return 0 };
            self.shared.send(CefEvent::PermissionPrompt { id, prompt_id, origin, perms: requested_permissions, callback: cb.clone() });
            1
        }
        fn on_dismiss_permission_prompt(&self, browser: Option<&mut Browser>, prompt_id: u64, result: PermissionRequestResult) {
            self.shared.log(format!("cef: on_dismiss_permission_prompt id={} prompt={prompt_id} result={}", bid(&browser), result.get_raw()));
        }
    }
}
