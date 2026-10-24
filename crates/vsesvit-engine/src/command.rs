//! Owned by W0.2.
//! The `Engine` trait: everything the shell asks of the engine (UA section 4.2 `command.rs`, with design
//! section 1.5's edits). Geometry, show/hide and reparenting of a page are the shell's platform layer's
//! work on the page's native window, so they are not here.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use crate::focus::PageFocus;
use crate::hooks::ShellHooks;
use crate::ids::{EngineDownloadId, NativeHandle, ParkedId, PromptId, RequestId, TabId};
use crate::prompt::{PromptAnswer, PromptError};
use crate::types::{
    Browsing, Chord, CloseMode, EngineError, EnginePrefs, EngineVersions, KeyRouting, Origin, PagePoint, Permission,
    RequestFilter, Setting, TabSpec, Url, Zoom,
};

/// The engine, as the shell sees it. A main-thread object (`Rc<dyn Engine>`): every method is called on
/// the main thread, never blocks, and may be called from inside a path an engine callback triggered, so
/// each is re-entrant (design section 1.6). Slow results arrive as events tagged with a [`RequestId`].
/// Calls naming a tab the engine does not know are ignored.
pub trait Engine {
    // Set-up.

    /// Installs the shell's synchronous hooks.
    fn set_hooks(&self, hooks: Rc<dyn ShellHooks>);
    /// Sets which keys the engine hands to the shell while a page has focus.
    fn set_key_routing(&self, routing: KeyRouting);
    /// Applies the engine-side preferences.
    fn apply_prefs(&self, prefs: &EnginePrefs);
    /// Installs the IO-thread request and cookie filter.
    fn set_request_filter(&self, filter: Arc<dyn RequestFilter>);
    /// Scripts run at document start in every page from now on.
    fn set_document_start_scripts(&self, scripts: Vec<String>);
    /// Mirrors a stored site setting into the engine; `None` clears it.
    fn set_site_setting(&self, browsing: Browsing, origin: &Origin, permission: Permission, setting: Option<Setting>);
    /// Creates the private request context, before the first private tab.
    fn begin_private_context(&self);
    /// Drops the private request context once its browsers have closed.
    fn end_private_context(&self);
    /// The engine's versions.
    fn versions(&self) -> EngineVersions;
    /// The process-wide hidden window. Once, at app start, before any tab. The engine creates its hidden
    /// browsers there (design sections 1.3 and 3.7).
    fn set_hidden_host(&self, host: NativeHandle);

    // Tab lifecycle and presentation.

    /// Creates a tab's browser; `TabEvent::Created` follows. Refused with `ShuttingDown` once shutdown began.
    fn create_tab(&self, spec: TabSpec) -> Result<(), EngineError>;
    /// Closes a tab's browser; `TabEvent::Closed` follows, unless the page keeps itself open.
    fn close_tab(&self, tab: TabId, mode: CloseMode);
    /// Tells the engine whether the page is shown (`was_hidden`). The shell maps and unmaps the window.
    fn set_visible(&self, tab: TabId, visible: bool);
    /// The page's window moved or resized: closes `<select>` popups.
    fn notify_moved(&self, tab: TabId);
    /// The page's screen or scale changed.
    fn notify_screen_info_changed(&self, tab: TabId);
    /// Publishes the tab's page-focus policy; the engine refuses every focus request it does not allow.
    /// `PageFocus::NEW_BROWSER` before `create_tab`, then before every `load` and on every change.
    fn set_page_focus(&self, tab: TabId, focus: PageFocus);
    /// Freezes or thaws the page (Memory Saver).
    fn set_frozen(&self, tab: TabId, frozen: bool);
    /// Gives the page keyboard focus. Only after the platform focused its window.
    fn focus_page(&self, tab: TabId);
    /// Takes keyboard focus from the page.
    fn blur_page(&self, tab: TabId);
    /// Ends the tab but keeps its browser, which the shell has already moved into the hidden host, until
    /// its last download ends (design section 4.2). `TabEvent::Detached` follows; no `Closed`.
    fn detach_tab(&self, tab: TabId) -> Result<ParkedId, EngineError>;
    /// Shutdown phase one (design section 3.6 step 3): no new browsers, every open prompt ends with
    /// `Shutdown`, every user browser closes. `BrowserEvent::UserBrowsersClosed` follows. Idempotent.
    fn begin_shutdown(&self);
    /// Shutdown phase two (step 5): the hidden browsers close. `BrowserEvent::AllBrowsersClosed` follows
    /// once the live count is zero. Called before `begin_shutdown`, it begins shutdown first. Idempotent.
    fn close_internal(&self);

    // Navigation.

    /// Loads a URL.
    fn load(&self, tab: TabId, url: &Url);
    /// Shows HTML at `about:blank` or under `shown_url` (UA S:9).
    fn load_html(&self, tab: TabId, html: String, shown_url: Option<&Url>);
    /// Reloads.
    fn reload(&self, tab: TabId, bypass_cache: bool);
    /// Stops loading.
    fn stop(&self, tab: TabId);
    /// Goes back.
    fn go_back(&self, tab: TabId);
    /// Goes forward.
    fn go_forward(&self, tab: TabId);

    // Page commands.

    /// Finds text; results arrive as `TabEvent::FindResult`.
    fn find(&self, tab: TabId, text: &str, forward: bool, find_next: bool);
    /// Ends finding.
    fn stop_finding(&self, tab: TabId, clear_selection: bool);
    /// Sets the zoom.
    fn set_zoom(&self, tab: TabId, zoom: Zoom);
    /// The zoom.
    fn zoom(&self, tab: TabId) -> Zoom;
    /// Mutes or unmutes.
    fn set_audio_muted(&self, tab: TabId, muted: bool);
    /// Opens the engine's print preview.
    fn print(&self, tab: TabId);
    /// Prints to a PDF file; `RequestResult::PdfPrinted`.
    fn print_to_pdf(&self, tab: TabId, path: PathBuf) -> RequestId;
    /// Opens DevTools, inspecting the element at a point if given.
    fn show_devtools(&self, tab: TabId, inspect_at: Option<PagePoint>);
    /// Opens DevTools on the console.
    fn show_console(&self, tab: TabId);
    /// Closes DevTools.
    fn close_devtools(&self, tab: TabId);
    /// Whether DevTools is open.
    fn has_devtools(&self, tab: TabId) -> bool;
    /// Leaves page fullscreen.
    fn exit_fullscreen(&self, tab: TabId);
    /// Saves the page as MHTML; `RequestResult::MhtmlSaved`.
    fn save_mhtml(&self, tab: TabId, path: PathBuf) -> RequestId;
    /// Downloads a URL in the tab's context.
    fn start_download(&self, tab: TabId, url: &Url);
    /// Evaluates a script in the main frame; `RequestResult::Eval`.
    fn eval(&self, tab: TabId, script: &str) -> RequestId;
    /// Captures the page as a PNG; `RequestResult::Captured`.
    fn capture_page(&self, tab: TabId) -> RequestId;
    /// Sends a key to the page (self-test; the extension-command fallback).
    fn send_key(&self, tab: TabId, chord: Chord);

    // Answers.

    /// Answers a prompt, exactly once. An unknown id gets `Unknown`, an answered or ended one
    /// `AlreadyEnded`; a wrong kind gets `WrongKind` and leaves the prompt open.
    fn answer_prompt(&self, id: PromptId, answer: PromptAnswer) -> Result<(), PromptError>;
    /// Lets a download start, writing to `path`.
    fn continue_download(&self, id: EngineDownloadId, path: PathBuf);
    /// Cancels a download.
    fn cancel_download(&self, id: EngineDownloadId);
    /// Pauses a download.
    fn pause_download(&self, id: EngineDownloadId);
    /// Resumes a download.
    fn resume_download(&self, id: EngineDownloadId);

    // Storage.

    /// Sites with cookies; `RequestResult::CookieSites`.
    fn cookie_sites(&self, browsing: Browsing) -> RequestId;
    /// Cookies sent to a URL; `RequestResult::Cookies`.
    fn cookies_for(&self, browsing: Browsing, url: &Url) -> RequestId;
    /// Deletes a site's cookies and storage; `RequestResult::SiteDataDeleted`.
    fn delete_site_data(&self, browsing: Browsing, site: &str) -> RequestId;
    /// Deletes the cookies of these hosts.
    fn delete_cookies_for_hosts(&self, browsing: Browsing, hosts: &[String]);
    /// Clears the context's browsing data; `RequestResult::BrowsingDataCleared`.
    fn clear_browsing_data(&self, browsing: Browsing) -> RequestId;

    /// Arms one of the shutdown self-test cases (design section 3.6, Tests). An engine without them
    /// answers `Unsupported`, so a check cannot pass by accident.
    #[cfg(feature = "self-test")]
    fn test_shutdown(&self, test: TestShutdown) -> Result<(), EngineError> {
        let _ = test;
        Err(EngineError::Unsupported)
    }
}

/// A shutdown self-test case (design section 3.6, Tests). Armed before the tab is created.
#[cfg(feature = "self-test")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestShutdown {
    /// The tab's browser closes this long after it is asked to.
    DelayClose {
        /// The tab.
        tab: TabId,
        /// How long.
        delay: std::time::Duration,
    },
    /// The tab's browser never finishes closing, so shutdown reaches its cap.
    PinOpen {
        /// The tab.
        tab: TabId,
    },
    /// The tab's browser finishes creation only after `begin_shutdown`, so it is closed at once.
    HoldCreation {
        /// The tab.
        tab: TabId,
    },
}
