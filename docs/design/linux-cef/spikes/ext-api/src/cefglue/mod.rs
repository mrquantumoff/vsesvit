//! CEF glue. This module never imports `gpui::*`: the `wrap_*!` macros name
//! `App`, `Client`, `Task`... unqualified, which would clash with GPUI's prelude.
//!
//! Threading contract: every handler here runs on a CEF thread (the browser UI
//! thread, which with `external_message_pump` is the GPUI main thread *inside*
//! `do_message_loop_work`, or the IO thread for request handlers). Handlers never
//! touch GPUI state. They push a [`CefEvent`] into an unbounded futures channel;
//! a GPUI foreground task drains it.

pub mod app;
pub mod client;
pub mod idc_names;

use cef::*;
use futures::channel::mpsc::UnboundedSender;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

/// Browser-shell actions that a key press or Chrome command inside the page maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellAction {
    FocusOmnibox,
    NewTab,
    NewWindow,
    CloseTab,
    ReopenTab,
    Fullscreen,
    Print,
    Find,
    DevTools,
    Reload,
    Back,
    Forward,
    NextTab,
    PrevTab,
    ViewSource,
}

/// How keyboard shortcuts typed while the page has focus are intercepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMode {
    /// No interception at all: observe what Chrome does by default.
    None,
    /// Only `CommandHandler::on_chrome_command` (IDC_* ids).
    Command,
    /// Only `KeyboardHandler::on_pre_key_event`.
    PreKey,
    /// Both (recommended): pre-key for reserved shell shortcuts, command handler as the net.
    Both,
}

/// What `on_before_popup` does with `window.open` / `target=_blank`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupMode {
    /// Let Chrome create its default (native) window.
    Default,
    /// Create the popup as another native child of the GPUI window (keeps `window.opener`).
    Child,
    /// Cancel it and open the URL as a new tab (loses `window.opener`).
    CancelNewTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogMode {
    /// Return 0 from the handlers: Chrome style shows its own UI.
    Default,
    /// Handle in the shell (auto-answer in the spike, logged).
    Shell,
}

pub enum CefEvent {
    ContextInitialized,
    AfterCreated { id: i32, browser: Browser, is_popup: bool, xid: u64, opener_id: i32, kind: &'static str, style: u32 },
    BeforeClose { id: i32 },
    Title { id: i32, title: String },
    Address { id: i32, url: String },
    Favicon { id: i32, urls: Vec<String> },
    LoadingState { id: i32, loading: bool, can_back: bool, can_fwd: bool },
    LoadEnd { id: i32, status: i32 },
    LoadError { id: i32, code: i32, text: String, url: String },
    Fullscreen { id: i32, on: bool },
    GotFocus { id: i32 },
    TakeFocus { id: i32, next: bool },
    SetFocus { id: i32, source: i32 },
    Action { id: i32, action: ShellAction, via: &'static str },
    PopupAsTab { opener_id: i32, url: String },
    JsDialog { id: i32, kind: i32, message: String, default_text: String, callback: JsdialogCallback },
    FileDialog { id: i32, mode: i32, title: String, callback: FileDialogCallback },
    Download { id: i32, name: String, url: String },
    PermissionPrompt { id: i32, prompt_id: u64, origin: String, perms: u32, callback: PermissionPromptCallback },
    Console { id: i32, message: String },
    /// `ContextMenuHandler::run_context_menu` in shell mode: the model was dumped to the log;
    /// the shell answers later with `callback.cont(command_id)` (ctl `menupick`).
    ContextMenu { id: i32, callback: RunContextMenuCallback, items: Vec<(i32, String)> },
}

/// State shared between CEF handlers (any thread) and the GPUI shell.
pub struct Shared {
    pub events: UnboundedSender<CefEvent>,
    pub pump: UnboundedSender<i64>,
    /// GPUI toplevel XID, parent for popups created as children.
    pub parent_xid: AtomicU64,
    /// Content rectangle in DIP (CEF `WindowInfo.bounds` are DIP).
    pub content_rect_dip: Mutex<Rect>,
    pub key_mode: KeyMode,
    pub popup_mode: PopupMode,
    pub dialog_mode: DialogMode,
    pub start: Instant,
    pub pump_requests: AtomicU64,
    /// Whether CEF may take focus on its own (`FocusHandler::on_set_focus`). False while
    /// GPUI chrome (omnibox) owns the keyboard, e.g. right after Ctrl+T.
    pub page_focus_allowed: std::sync::atomic::AtomicBool,
    /// SPIKE_MENU=shell: run_context_menu returns 1 and the shell picks the item.
    pub menu_shell: bool,
    /// SPIKE_ADOPT_SYNC: what on_after_created does *synchronously* with a toplevel Chrome
    /// creates on its own (default_client): close | hide | embed | "" (nothing).
    pub adopt_sync: Mutex<String>,
    /// Pixels of Chrome UI (tab strip + toolbar) cropped off the top in adopt "embed" mode.
    pub embed_crop: i32,
    /// Own X connection for synchronous work inside CEF callbacks (no GPUI involved).
    pub x: OnceLock<crate::xwin::XConn>,
    pub holder_xid: AtomicU64,
    idc: OnceLock<HashMap<i32, String>>,
}

impl Shared {
    pub fn new(events: UnboundedSender<CefEvent>, pump: UnboundedSender<i64>) -> Arc<Self> {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let key_mode = match env("SPIKE_KEYS").as_str() {
            "none" => KeyMode::None,
            "cmd" => KeyMode::Command,
            "key" => KeyMode::PreKey,
            _ => KeyMode::Both,
        };
        let popup_mode = match env("SPIKE_POPUPS").as_str() {
            "default" => PopupMode::Default,
            "tab" => PopupMode::CancelNewTab,
            _ => PopupMode::Child,
        };
        let dialog_mode = match env("SPIKE_DIALOGS").as_str() {
            "shell" => DialogMode::Shell,
            _ => DialogMode::Default,
        };
        Arc::new(Self {
            events,
            pump,
            parent_xid: AtomicU64::new(0),
            content_rect_dip: Mutex::new(Rect { x: 0, y: 0, width: 800, height: 600 }),
            key_mode,
            popup_mode,
            dialog_mode,
            start: Instant::now(),
            pump_requests: AtomicU64::new(0),
            page_focus_allowed: std::sync::atomic::AtomicBool::new(true),
            menu_shell: env("SPIKE_MENU") == "shell",
            adopt_sync: Mutex::new(env("SPIKE_ADOPT_SYNC")),
            embed_crop: env("SPIKE_EMBED_CROP").parse().unwrap_or(85),
            x: OnceLock::new(),
            holder_xid: AtomicU64::new(0),
            idc: OnceLock::new(),
        })
    }

    pub fn send(&self, ev: CefEvent) {
        let _ = self.events.unbounded_send(ev);
    }

    pub fn log(&self, msg: impl AsRef<str>) {
        crate::log(msg.as_ref());
    }

    /// Version-safe IDC id -> name (built lazily from `cef_id_for_command_id_name`).
    pub fn idc_name(&self, id: i32) -> String {
        let map = self.idc.get_or_init(|| {
            idc_names::IDC_NAMES
                .iter()
                .filter_map(|n| {
                    let id = unsafe { sys::cef_id_for_command_id_name(n.as_ptr()) };
                    (id >= 0).then(|| (id, n.to_string_lossy().into_owned()))
                })
                .collect()
        });
        map.get(&id).cloned().unwrap_or_else(|| format!("IDC_?{id}"))
    }

    pub fn xconn(&self) -> &crate::xwin::XConn {
        self.x.get_or_init(|| crate::xwin::XConn::connect().expect("x11rb"))
    }

    pub fn parent(&self) -> u64 {
        self.parent_xid.load(Ordering::Relaxed)
    }
}

/// `cef_id_for_command_id_name` for a known IDC constant.
pub fn idc(name: &std::ffi::CStr) -> i32 {
    unsafe { sys::cef_id_for_command_id_name(name.as_ptr()) }
}

pub fn cef_str(s: Option<&CefString>) -> String {
    s.map(|s| s.to_string()).unwrap_or_default()
}
