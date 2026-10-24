//! Owned by W0.2.
//! What the engine tells the shell (UA section 4.2 `event.rs`, design section 1.5). Events are queued
//! from any engine thread and delivered on main after each pump step, never inside an engine call.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use crate::ids::{NativeHandle, PromptId, RequestId, TabId};
use crate::prompt::{PromptEnd, PromptRequest};
use crate::types::{
    Capturing, CertStatus, CommitKind, DownloadRequest, DownloadUpdate, EngineTransition, FindResult, NetError,
    OpenDisposition, Origin, PageChannel, RequestResult, RoutedKey, SslInfo, TerminationStatus, Url, Zoom,
};

/// An event from the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum BrowserEvent {
    /// Something about one tab.
    Tab(TabId, TabEvent),
    /// A popup the shell allowed through `ShellHooks::popup_policy` was created.
    PopupOpened {
        /// The tab that opened it.
        opener: TabId,
        /// The popup's tab id, from the policy.
        tab: TabId,
        /// What it opened.
        url: Url,
        /// Where the page wanted it.
        disposition: OpenDisposition,
    },
    /// A page asks the shell to open a URL elsewhere (middle click, Ctrl+click).
    OpenUrlRequested {
        /// The tab asking.
        opener: TabId,
        /// The URL.
        url: Url,
        /// Where it wants it.
        disposition: OpenDisposition,
        /// Whether a user gesture started it.
        user_gesture: bool,
    },
    /// A routed key was pressed while a page had focus.
    Shortcut {
        /// The tab that had focus, if known.
        tab: Option<TabId>,
        /// What the key does.
        key: RoutedKey,
    },
    /// A question the engine waits on. Answer it with `Engine::answer_prompt`.
    Prompt {
        /// Its id.
        id: PromptId,
        /// The question.
        request: PromptRequest,
    },
    /// The engine ended a prompt before an answer; any later answer is `AlreadyEnded`.
    PromptEnded {
        /// Its id.
        id: PromptId,
        /// Why.
        why: PromptEnd,
    },
    /// A download is about to start; answer with `continue_download` or `cancel_download`.
    DownloadRequested(DownloadRequest),
    /// A download progressed.
    DownloadUpdated(DownloadUpdate),
    /// A slow operation finished.
    RequestFinished {
        /// The id the operation returned.
        id: RequestId,
        /// Its result.
        result: RequestResult,
    },
    /// A message from a page, sent through the renderer process. `origin` and `main_frame` come from the
    /// browser process's own frame record, never from the payload.
    PageMessage {
        /// The tab.
        tab: TabId,
        /// The sending frame's origin.
        origin: Origin,
        /// Whether the sender is the main frame.
        main_frame: bool,
        /// Which channel.
        channel: PageChannel,
        /// The page's JSON text.
        payload: String,
    },
    /// Every user browser has closed (shutdown phase one, design section 3.6 step 3).
    UserBrowsersClosed,
    /// The live-browser count is zero (shutdown phase two, design section 3.6 step 6).
    AllBrowsersClosed,
    /// A second start of the app reached the engine's own singleton (the fallback to the instance socket).
    AppRelaunch {
        /// Its arguments.
        argv: Vec<OsString>,
        /// Its working directory.
        cwd: PathBuf,
    },
    /// An engine failure, for the log.
    EngineError(String),
}

/// An event about one tab.
#[derive(Clone, Debug, PartialEq)]
pub enum TabEvent {
    /// The browser exists; `native` is its window.
    Created {
        /// The page's native window.
        native: NativeHandle,
    },
    /// After `Engine::detach_tab`: the tab is gone, its browser lives on parked (design section 4.2). No
    /// `Closed` follows.
    Detached,
    /// The browser closed. An unrequested `Closed` means the page closed itself (`window.close()`).
    Closed,
    /// The title changed.
    TitleChanged(String),
    /// The address changed.
    AddressChanged(Url),
    /// The favicon changed: a PNG of at most 32 px, or none.
    FaviconChanged(Option<Arc<[u8]>>),
    /// Loading started or stopped.
    LoadingStateChanged {
        /// Whether it is loading.
        loading: bool,
        /// Whether back is possible.
        can_go_back: bool,
        /// Whether forward is possible.
        can_go_forward: bool,
    },
    /// Load progress, 0 to 1.
    LoadProgress(f32),
    /// A main-frame navigation started.
    NavigationStarted {
        /// Where to.
        url: Url,
        /// Whether it is a redirect.
        is_redirect: bool,
    },
    /// The main frame committed.
    Committed {
        /// The committed URL.
        url: Url,
        /// What it replaced.
        kind: CommitKind,
        /// How it started.
        transition: EngineTransition,
        /// The document's MIME type.
        mime: Option<String>,
    },
    /// The main frame failed to load.
    LoadFailed {
        /// The URL.
        url: Url,
        /// The error.
        error: NetError,
    },
    /// A certificate error stopped a load.
    CertificateError {
        /// The URL.
        url: Url,
        /// What is wrong.
        status: CertStatus,
        /// The chain, leaf first, DER.
        der_chain: Vec<Vec<u8>>,
    },
    /// The committed page's security, after commit.
    SecurityChanged(Option<SslInfo>),
    /// The renderer ended.
    RenderProcessGone(TerminationStatus),
    /// A find result.
    FindResult(FindResult),
    /// The zoom changed (UA S:14).
    ZoomChanged(Zoom),
    /// Sound started or stopped (UA S:14).
    AudioStateChanged {
        /// Whether it plays sound.
        audible: bool,
    },
    /// What the page captures changed.
    CaptureChanged(Capturing),
    /// The page entered or left fullscreen.
    FullscreenRequested(bool),
    /// The status text, only when Chromium's status bubble is off.
    StatusText(Option<String>),
    /// The page gained or lost focus.
    FocusChanged {
        /// Whether it has focus.
        focused: bool,
    },
    /// The page gives focus away by tabbing past its last or first element.
    TakeFocus {
        /// Forward rather than backward.
        next: bool,
    },
    /// An extension popup's preferred size.
    PreferredSize {
        /// Width, physical pixels.
        width: u32,
        /// Height, physical pixels.
        height: u32,
    },
}
