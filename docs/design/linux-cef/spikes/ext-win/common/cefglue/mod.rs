//! CEF glue shared by the Linux and the Windows spike shell. Never imports `gpui::*` (the
//! `wrap_*!` macros name `App`, `Client`, `Task`... unqualified). OS-specific bits are
//! `cfg`-gated here or live in `crate::platform`.
//!
//! Threading: handlers run on CEF threads. With `external_message_pump` the browser UI thread
//! is GPUI's main thread inside `do_message_loop_work`. Handlers never touch GPUI state; they
//! push a [`CefEvent`] into a channel a GPUI task drains, or work on `ext::ExtState`.

pub mod app;
pub mod client;
pub mod ext;
pub mod idc_names;

use cef::*;
use futures::channel::mpsc::UnboundedSender;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

/// Native window handle (HWND or XID) widened to u64.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpMode {
    External,
    MultiThreaded,
}

pub enum CefEvent {
    ContextInitialized,
    AfterCreated { id: i32, browser: Browser, is_popup: bool, handle: NativeHandle, opener_id: i32, kind: String },
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
    ContextMenu { id: i32, callback: RunContextMenuCallback, items: Vec<(i32, String)> },
    AutoResize { id: i32, w: i32, h: i32 },
    /// ext.rs: open `url` as a shell tab in exactly this request context (foreign reopen).
    OpenTab { url: String, ctx: Option<RequestContext>, why: String },
}

/// State shared between CEF handlers (any thread) and the GPUI shell.
pub struct Shared {
    pub events: UnboundedSender<CefEvent>,
    pub pump: UnboundedSender<i64>,
    /// Parent for child popups: the content host.
    pub parent_handle: AtomicU64,
    /// Content rectangle in DIP, relative to the parent.
    pub content_rect_dip: Mutex<Rect>,
    pub key_mode: KeyMode,
    pub popup_mode: PopupMode,
    pub dialog_mode: DialogMode,
    pub pump_mode: PumpMode,
    pub start: Instant,
    pub pump_requests: AtomicU64,
    pub page_focus_allowed: AtomicBool,
    pub menu_shell: bool,
    pub download_dir: String,
    pub ext: ext::ExtState,
    idc: OnceLock<HashMap<i32, String>>,
}

impl Shared {
    pub fn new(events: UnboundedSender<CefEvent>, pump: UnboundedSender<i64>, download_dir: String) -> Arc<Self> {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        Arc::new(Self {
            events,
            pump,
            parent_handle: AtomicU64::new(0),
            content_rect_dip: Mutex::new(Rect { x: 0, y: 0, width: 800, height: 600 }),
            key_mode: match env("SPIKE_KEYS").as_str() {
                "none" => KeyMode::None,
                "cmd" => KeyMode::Command,
                "key" => KeyMode::PreKey,
                _ => KeyMode::Both,
            },
            popup_mode: match env("SPIKE_POPUPS").as_str() {
                "default" => PopupMode::Default,
                "tab" => PopupMode::CancelNewTab,
                _ => PopupMode::Child,
            },
            dialog_mode: match env("SPIKE_DIALOGS").as_str() {
                "default" => DialogMode::Default,
                _ => DialogMode::Shell,
            },
            pump_mode: match env("SPIKE_PUMP").as_str() {
                "mt" => PumpMode::MultiThreaded,
                _ => PumpMode::External,
            },
            start: Instant::now(),
            pump_requests: AtomicU64::new(0),
            page_focus_allowed: AtomicBool::new(true),
            menu_shell: env("SPIKE_MENU") == "shell",
            download_dir,
            ext: ext::ExtState::default(),
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

/// Run `f` on CEF's UI thread after `ms` milliseconds.
pub fn on_ui_later<F: FnOnce() + Send + 'static>(ms: i64, f: F) {
    let mut task = FnTask::new(Arc::new(Mutex::new(Some(Box::new(f)))));
    if post_delayed_task(ThreadId::UI, Some(&mut task), ms) == 0 {
        crate::log("cef: post_delayed_task(UI) failed");
    }
}

/// A `WindowInfo` for a child of `parent` (HWND or XID) with `rect` in DIP.
pub fn child_window_info(parent: NativeHandle, rect: Rect, style: RuntimeStyle) -> WindowInfo {
    #[cfg(windows)]
    let p = sys::HWND(parent as usize as *mut _);
    #[cfg(not(windows))]
    let p = parent as _;
    WindowInfo { runtime_style: style, ..Default::default() }.set_as_child(p, &rect)
}
