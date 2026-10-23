//! CEF glue, OS-neutral apart from the native handle type. This module never imports
//! `gpui::*`: the `wrap_*!` macros name `App`, `Client`, `Task`... unqualified, which
//! would clash with GPUI's prelude.
//!
//! Threading contract: handlers run on a CEF thread. With `external_message_pump` the
//! browser UI thread is the GPUI main thread *inside* `do_message_loop_work`; with
//! `multi_threaded_message_loop` it is CEF's own thread. Handlers never touch GPUI
//! state. They push a [`CefEvent`] into an unbounded futures channel that a GPUI
//! foreground task drains.

pub mod app;
pub mod client;
pub mod idc_names;

use cef::*;
use futures::channel::mpsc::UnboundedSender;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

/// Native window handle of a CEF browser (HWND on Windows, XID on X11), widened to u64.
pub type NativeHandle = u64;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMode {
    None,
    Command,
    PreKey,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupMode {
    Default,
    Child,
    CancelNewTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogMode {
    Default,
    Shell,
}

/// Which thread is CEF's browser UI thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpMode {
    /// `external_message_pump`: GPUI's main thread, inside `do_message_loop_work`.
    External,
    /// `multi_threaded_message_loop`: CEF's own "CefUIThread".
    MultiThreaded,
}

pub enum CefEvent {
    ContextInitialized,
    AfterCreated { id: i32, browser: Browser, is_popup: bool, hwnd: NativeHandle, opener_id: i32 },
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
}

/// State shared between CEF handlers (any thread) and the GPUI shell.
pub struct Shared {
    pub events: UnboundedSender<CefEvent>,
    pub pump: UnboundedSender<i64>,
    /// Parent for popups created as children (the content host HWND).
    pub parent_handle: AtomicU64,
    /// Content rectangle in DIP (CEF `WindowInfo.bounds` are DIP).
    pub content_rect_dip: Mutex<Rect>,
    pub key_mode: KeyMode,
    pub popup_mode: PopupMode,
    pub dialog_mode: DialogMode,
    pub pump_mode: PumpMode,
    pub start: Instant,
    pub pump_requests: AtomicU64,
    /// Whether CEF may take focus on its own (`FocusHandler::on_set_focus`).
    pub page_focus_allowed: AtomicBool,
    pub download_dir: String,
    idc: OnceLock<HashMap<i32, String>>,
}

impl Shared {
    pub fn new(events: UnboundedSender<CefEvent>, pump: UnboundedSender<i64>, download_dir: String) -> Arc<Self> {
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
        let pump_mode = match env("SPIKE_PUMP").as_str() {
            "mt" => PumpMode::MultiThreaded,
            _ => PumpMode::External,
        };
        Arc::new(Self {
            events,
            pump,
            parent_handle: AtomicU64::new(0),
            content_rect_dip: Mutex::new(Rect { x: 0, y: 0, width: 800, height: 600 }),
            key_mode,
            popup_mode,
            dialog_mode,
            pump_mode,
            start: Instant::now(),
            pump_requests: AtomicU64::new(0),
            page_focus_allowed: AtomicBool::new(true),
            download_dir,
            idc: OnceLock::new(),
        })
    }

    pub fn send(&self, ev: CefEvent) {
        let _ = self.events.unbounded_send(ev);
    }

    pub fn log(&self, msg: impl AsRef<str>) {
        crate::log(msg.as_ref());
    }

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

    pub fn parent(&self) -> NativeHandle {
        self.parent_handle.load(Ordering::Relaxed)
    }
}

pub fn cef_str(s: Option<&CefString>) -> String {
    s.map(|s| s.to_string()).unwrap_or_default()
}

wrap_task! {
    pub struct FnTask {
        f: Arc<Mutex<Option<Box<dyn FnOnce() + Send + 'static>>>>,
    }

    impl Task {
        fn execute(&self) {
            if let Some(f) = self.f.lock().unwrap().take() {
                f();
            }
        }
    }
}

/// Run `f` on CEF's UI thread: immediately when already there, else posted.
pub fn on_ui<F: FnOnce() + Send + 'static>(f: F) {
    if currently_on(ThreadId::UI) != 0 {
        f();
        return;
    }
    let mut task = FnTask::new(Arc::new(Mutex::new(Some(Box::new(f)))));
    if post_task(ThreadId::UI, Some(&mut task)) == 0 {
        crate::log("cef: post_task(UI) failed");
    }
}
