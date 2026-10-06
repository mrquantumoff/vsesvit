//! Downloads: where WebView2 saves each file, the live byte counts, and core's downloads list.
//!
//! Every tab hands its `DownloadStarting` here. The file goes to the download folder under a
//! name no file or running download has, or where the user says when they asked to be asked;
//! core records the start, each pause or interruption, and the outcome, a private window's in
//! its private session. Byte counts stay in memory while a download runs. Views subscribe while
//! they are open, and every window's toolbar shows the downloads button once a download started
//! this session.
//!
//! A file of a type that can run code is written under its unconfirmed name and waits for the
//! user to keep or discard it; the window it came from shows Chrome's warning under the
//! downloads button. Every finished file carries the Mark of the Web, so SmartScreen checks it
//! when it is opened.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use vsesvit_core::Profile;
use vsesvit_core::downloads::{self as list, Download, DownloadId, State};
use vsesvit_core::prefs::keys;
use vsesvit_core::private::Browsing;
use windows_core::Result;

use crate::bindings::*;
use crate::browser::Browser;
use crate::sync::live;
use crate::window::BrowserWindow;
use crate::{exec, pickers, platform};

/// Entries the Downloads view lists.
const LISTED: usize = 200;
/// A running download's progress reaches the views at most this often.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// What changed, for the views that show downloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// Byte counts of running downloads.
    Progress,
    /// Entries came, went, or reached a final state.
    List,
}

/// The toolbar's downloads button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Indicator {
    /// No download started this session.
    Hidden,
    Idle,
    /// A download is running.
    Busy,
}

type OnChange = dyn Fn(Change);

/// Called with each change while the view that registered it keeps it alive.
pub(crate) type Subscriber = Rc<OnChange>;

/// A download the engine holds.
struct Live {
    operation: CoreWebView2DownloadOperation,
    /// The kind of window it started in.
    browsing: Browsing,
    /// Where the engine writes: the destination, or its unconfirmed name for a dangerous file.
    path: PathBuf,
    dangerous: bool,
    /// As last stored in core.
    state: State,
    received: u64,
    total: Option<u64>,
    notified: Instant,
    /// Whose tab started it: a dangerous file is warned about there.
    window: Weak<BrowserWindow>,
}

pub(crate) struct Downloads {
    /// The platform's Downloads folder, used while the user has chosen none.
    default_dir: PathBuf,
    live: RefCell<HashMap<DownloadId, Live>>,
    started: Cell<bool>,
    subscribers: RefCell<Vec<Weak<OnChange>>>,
}

impl Downloads {
    /// At startup, before any tab exists: downloads the last run left unfinished read as
    /// failed, since no engine download outlives its process.
    pub fn new(profile: &mut Profile) -> Self {
        match profile.downloads().interrupt_stale() {
            Ok(0) => {}
            Ok(n) => log::info!("{n} download(s) from the last run did not finish"),
            Err(e) => log::warn!("downloads list: {e}"),
        }
        let default_dir = platform::downloads_folder().unwrap_or_else(|e| {
            log::warn!("the Downloads folder: {e}");
            std::env::temp_dir()
        });
        Self {
            default_dir,
            live: RefCell::new(HashMap::new()),
            started: Cell::new(false),
            subscribers: RefCell::new(Vec::new()),
        }
    }
}

impl Browser {
    /// Where downloads go: the folder the user chose, else the platform's Downloads folder.
    /// Read for every download, so a new choice applies to the next one.
    pub fn download_dir(&self) -> PathBuf {
        self.custom_download_dir()
            .unwrap_or_else(|| self.downloads.default_dir.clone())
    }

    pub fn custom_download_dir(&self) -> Option<PathBuf> {
        self.core(|p| p.prefs().get(&keys::DOWNLOADS_DIR))
    }

    /// `None` goes back to the platform's Downloads folder.
    pub fn set_download_dir(&self, dir: Option<&Path>) {
        match dir {
            Some(dir) => self.write_pref(&keys::DOWNLOADS_DIR, &Some(dir.to_path_buf())),
            None => {
                if let Err(e) = self.core(|p| p.prefs().reset(&keys::DOWNLOADS_DIR)) {
                    log::warn!("download folder: {e}");
                }
            }
        }
    }

    pub fn downloads_indicator(&self) -> Indicator {
        let downloads = &self.downloads;
        if downloads
            .live
            .borrow()
            .values()
            .any(|l| l.state == State::InProgress)
        {
            Indicator::Busy
        } else if downloads.started.get() {
            Indicator::Idle
        } else {
            Indicator::Hidden
        }
    }

    /// Newest first.
    pub fn download_list(&self) -> Vec<Download> {
        self.core(|p| p.downloads().list(LISTED))
            .unwrap_or_else(|e| {
                log::warn!("downloads list: {e}");
                Vec::new()
            })
    }

    /// Bytes received and expected so far, while the download runs.
    pub fn download_progress(&self, id: DownloadId) -> Option<(u64, Option<u64>)> {
        let live = self.downloads.live.borrow();
        live.get(&id).map(|l| (l.received, l.total))
    }

    pub fn cancel_download(&self, id: DownloadId) {
        self.operate(id, "cancel", |o| o.Cancel());
    }

    pub fn pause_download(&self, id: DownloadId) {
        self.operate(id, "pause", |o| o.Pause());
    }

    /// Goes on with a paused download, or retries an interrupted one where it stopped.
    pub fn resume_download(&self, id: DownloadId) {
        self.operate(id, "resume", |o| o.Resume());
    }

    /// Runs `f` on the engine's download, then stores what it changed, whether or not the
    /// engine raises `StateChanged` for it.
    fn operate(
        &self,
        id: DownloadId,
        what: &str,
        f: impl FnOnce(&CoreWebView2DownloadOperation) -> Result<()>,
    ) {
        let operation = self
            .downloads
            .live
            .borrow()
            .get(&id)
            .map(|l| l.operation.clone());
        let Some(operation) = operation else {
            return;
        };
        if let Err(e) = f(&operation) {
            log::warn!("{what} download: {e}");
        }
        self.download_state_changed(id, &operation);
    }

    /// Moves a dangerous file the user chose to keep to its own name.
    pub fn keep_download(&self, id: DownloadId) {
        match self.core(|p| p.downloads().keep(id)) {
            Ok(Some(path)) => log::info!("kept {}", path.display()),
            Ok(None) => {}
            Err(e) => log::warn!("keep download: {e}"),
        }
        self.downloads_changed(Change::List);
    }

    /// Deletes a dangerous file the user chose not to keep, and its entry.
    pub fn discard_download(&self, id: DownloadId) {
        if let Err(e) = self.core(|p| p.downloads().discard(id)) {
            log::warn!("discard download: {e}");
        }
        self.downloads_changed(Change::List);
    }

    /// Cancels the running downloads that started in windows of `browsing`'s kind.
    pub fn cancel_downloads(&self, browsing: Browsing) {
        let running: Vec<CoreWebView2DownloadOperation> = self
            .downloads
            .live
            .borrow()
            .values()
            .filter(|l| l.browsing == browsing)
            .map(|l| l.operation.clone())
            .collect();
        for operation in running {
            if let Err(e) = operation.Cancel() {
                log::warn!("cancel download: {e}");
            }
        }
    }

    /// Takes a finished download off the list; the file stays.
    pub fn remove_download(&self, id: DownloadId) {
        if let Err(e) = self.core(|p| p.downloads().remove(id)) {
            log::warn!("remove download: {e}");
        }
        self.downloads_changed(Change::List);
    }

    /// Takes every finished download off the list; the files stay.
    pub fn clear_downloads(&self) {
        if let Err(e) = self.core(|p| p.downloads().clear()) {
            log::warn!("clear downloads: {e}");
        }
        self.downloads_changed(Change::List);
    }

    pub fn subscribe_downloads(&self, subscriber: &Subscriber) {
        self.downloads
            .subscribers
            .borrow_mut()
            .push(Rc::downgrade(subscriber));
    }

    /// A tab's `DownloadStarting`. Edge's own download flyout stays hidden.
    pub fn download_starting(
        self: &Rc<Self>,
        window: &Rc<BrowserWindow>,
        args: &CoreWebView2DownloadStartingEventArgs,
    ) {
        cancel_unplaced(self.place_download(window, args), || args.SetCancel(true));
    }

    fn place_download(
        self: &Rc<Self>,
        window: &Rc<BrowserWindow>,
        args: &CoreWebView2DownloadStartingEventArgs,
    ) -> Result<()> {
        args.SetHandled(true)?;
        // The engine's default path ends in the name the server suggested.
        let suggested = args.ResultFilePath()?;
        let name = list::sanitize(&suggested);
        let dir = self.download_dir();
        let browsing = window.browsing();
        if !self.core(|p| p.prefs().get(&keys::DOWNLOADS_ASK)) {
            let path = self.free_path(&dir, &name);
            return self.begin_download(args, &path, browsing, Rc::downgrade(window));
        }
        let deferral = args.GetDeferral()?;
        let owner = window.window_id();
        let window = Rc::downgrade(window);
        let browser = Rc::downgrade(self);
        let args = args.clone();
        exec::spawn(async move {
            let picked = match owner {
                Ok(owner) => pickers::pick_save_file(owner, &dir, &name).await,
                Err(e) => Err(e),
            };
            let placed = match (picked, browser.upgrade()) {
                (Ok(Some(path)), Some(browser)) => browser.begin_download(&args, &path, browsing, window),
                (Ok(_), _) => args.SetCancel(true),
                (Err(e), _) => {
                    log::warn!("save dialog: {e}");
                    args.SetCancel(true)
                }
            };
            cancel_unplaced(placed, || args.SetCancel(true));
            let _ = deferral.Complete();
        });
        Ok(())
    }

    /// `name` in `dir`, numbered if a file or a running download has that path already.
    fn free_path(&self, dir: &Path, name: &str) -> PathBuf {
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!("{}: {e}", dir.display());
        }
        list::unique_destination(dir, name, |path| self.taken(path))
    }

    /// Whether a file or a running download has `path`.
    fn taken(&self, path: &Path) -> bool {
        path.exists()
            || self
                .downloads
                .live
                .borrow()
                .values()
                .any(|l| l.path == path)
    }

    /// Sends the download to `path`, or beside it under its unconfirmed name if it can run
    /// code, and records it, in core's private session for a private window's. A destination
    /// whose unconfirmed name another download holds is numbered.
    fn begin_download(
        self: &Rc<Self>,
        args: &CoreWebView2DownloadStartingEventArgs,
        path: &Path,
        browsing: Browsing,
        window: Weak<BrowserWindow>,
    ) -> Result<()> {
        let operation = args.DownloadOperation()?;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let dangerous = list::is_dangerous(&name, operation.MimeType().ok().as_deref());
        let path = match path.parent() {
            Some(dir) if dangerous && self.taken(&list::unconfirmed_path(path)) => {
                self.free_path(dir, &name)
            }
            _ => path.to_owned(),
        };
        let path = path.as_path();
        let written = if dangerous {
            list::unconfirmed_path(path)
        } else {
            path.to_owned()
        };
        args.SetResultFilePath(&written.to_string_lossy())?;
        let total = known_total(operation.TotalBytesToReceive()?);
        let url = operation.Uri()?;
        let now = u64::try_from(crate::session::now_ms()).unwrap_or(0);
        let download = self
            .core(|p| p.downloads().start(&url, path, total, now, browsing))
            .map_err(|e| windows_core::Error::new(E_FAIL, e.to_string()))?;
        let id = download.id;
        log::info!("downloading {url} to {}", written.display());
        let browser = Rc::downgrade(self);
        operation
            .BytesReceivedChanged(move |operation, _| {
                if let (Some(browser), Some(operation)) = (browser.upgrade(), operation.as_ref()) {
                    browser.download_progressed(id, operation);
                }
            })?
            .forget();
        let browser = Rc::downgrade(self);
        operation
            .StateChanged(move |operation, _| {
                if let (Some(browser), Some(operation)) = (browser.upgrade(), operation.as_ref()) {
                    browser.download_state_changed(id, operation);
                }
            })?
            .forget();
        self.downloads.live.borrow_mut().insert(
            id,
            Live {
                operation,
                browsing,
                path: written,
                dangerous,
                state: State::InProgress,
                received: 0,
                total,
                notified: Instant::now(),
                window,
            },
        );
        self.downloads.started.set(true);
        self.downloads_changed(Change::List);
        Ok(())
    }

    fn download_progressed(&self, id: DownloadId, operation: &CoreWebView2DownloadOperation) {
        let (received, total) = counts(operation);
        let due = {
            let mut live = self.downloads.live.borrow_mut();
            let Some(entry) = live.get_mut(&id) else {
                return;
            };
            entry.received = received;
            entry.total = total;
            let due = entry.notified.elapsed() >= PROGRESS_INTERVAL;
            if due {
                entry.notified = Instant::now();
            }
            due
        };
        if due {
            self.downloads_changed(Change::Progress);
        }
    }

    fn download_state_changed(&self, id: DownloadId, operation: &CoreWebView2DownloadOperation) {
        let (Ok(engine), Ok(reason)) = (operation.State(), operation.InterruptReason()) else {
            return;
        };
        let can_resume = operation.CanResume().unwrap_or(false);
        let (state, path, window) = {
            let mut live = self.downloads.live.borrow_mut();
            let Some(entry) = live.get_mut(&id) else {
                return;
            };
            let state = list_state(engine, reason, can_resume, entry.dangerous);
            if state == entry.state {
                return;
            }
            entry.state = state;
            let (path, window) = (entry.path.clone(), entry.window.clone());
            if !state.is_live() {
                live.remove(&id);
            }
            (state, path, window)
        };
        let (received, total) = counts(operation);
        log::info!("download {} is {state:?}: {received} bytes", id.0);
        if engine == CoreWebView2DownloadState::Completed {
            let marked = operation.Uri().map_err(std::io::Error::other);
            if let Err(e) = marked.and_then(|url| mark_of_the_web(&path, &url)) {
                log::warn!("mark of the web on {}: {e}", path.display());
            }
        }
        if let Err(e) = self.core(|p| p.downloads().update(id, state, received, total)) {
            log::warn!("downloads list: {e}");
        }
        self.downloads_changed(Change::List);
        if state == State::Unconfirmed
            && let Some(window) = window.upgrade()
            && let Some(download) = self.download_list().into_iter().find(|d| d.id == id)
        {
            window.warn_about_download(&download);
        }
    }

    fn downloads_changed(&self, change: Change) {
        if change == Change::List {
            let indicator = self.downloads_indicator();
            for window in self.windows() {
                window.show_downloads(indicator);
            }
        }
        for subscriber in live(&self.downloads.subscribers) {
            subscriber(change);
        }
    }
}

/// The file name a row shows, or the whole path if it has none.
pub(crate) fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Logs a download that could not be sent to its path or recorded, and cancels it: the engine
/// would otherwise save it with no entry in the list and no way to stop it.
fn cancel_unplaced(placed: Result<()>, cancel: impl FnOnce() -> Result<()>) {
    if let Err(e) = placed {
        log::warn!("download: {e}");
        let _ = cancel();
    }
}

/// The list's state for the engine's. WebView2 reports a pause as an interruption.
fn list_state(
    state: CoreWebView2DownloadState,
    reason: CoreWebView2DownloadInterruptReason,
    can_resume: bool,
    dangerous: bool,
) -> State {
    use CoreWebView2DownloadInterruptReason as Reason;
    match state {
        CoreWebView2DownloadState::Completed if dangerous => State::Unconfirmed,
        CoreWebView2DownloadState::Completed => State::Completed,
        CoreWebView2DownloadState::Interrupted => match reason {
            Reason::UserPaused => State::Paused,
            Reason::UserCanceled => State::Cancelled,
            _ if can_resume => State::Interrupted,
            _ => State::Failed,
        },
        _ => State::InProgress,
    }
}

/// Marks a finished file as downloaded from the Internet, unless the engine already did:
/// SmartScreen checks such a file when it is opened, and a kept file takes the mark along.
fn mark_of_the_web(file: &Path, url: &str) -> std::io::Result<()> {
    let mut stream = file.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    match std::fs::File::create_new(&stream) {
        Ok(mut f) => f.write_all(list::zone_identifier(url).as_bytes()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

fn counts(operation: &CoreWebView2DownloadOperation) -> (u64, Option<u64>) {
    let received = operation
        .BytesReceived()
        .ok()
        .and_then(|n| u64::try_from(n).ok())
        .unwrap_or(0);
    let total = operation.TotalBytesToReceive().ok().and_then(known_total);
    (received, total)
}

/// The engine reports an unknown size (no `Content-Length`) as -1.
fn known_total(total: i64) -> Option<u64> {
    u64::try_from(total).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_states_map_to_list_states() {
        use CoreWebView2DownloadInterruptReason as Reason;
        use CoreWebView2DownloadState as Engine;
        let cases = [
            (
                Engine::InProgress,
                Reason::None,
                false,
                false,
                State::InProgress,
            ),
            (
                Engine::Completed,
                Reason::None,
                false,
                false,
                State::Completed,
            ),
            (
                Engine::Completed,
                Reason::None,
                false,
                true,
                State::Unconfirmed,
            ),
            (
                Engine::Interrupted,
                Reason::UserPaused,
                true,
                false,
                State::Paused,
            ),
            (
                Engine::Interrupted,
                Reason::UserCanceled,
                false,
                false,
                State::Cancelled,
            ),
            (
                Engine::Interrupted,
                Reason::NetworkFailed,
                true,
                false,
                State::Interrupted,
            ),
            (
                Engine::Interrupted,
                Reason::NetworkFailed,
                false,
                false,
                State::Failed,
            ),
            (
                Engine::Interrupted,
                Reason::FileMalicious,
                false,
                true,
                State::Failed,
            ),
        ];
        for (engine, reason, can_resume, dangerous, state) in cases {
            assert_eq!(
                list_state(engine, reason, can_resume, dangerous),
                state,
                "{engine:?} {reason:?} resumable {can_resume} dangerous {dangerous}"
            );
        }
    }

    #[test]
    fn a_finished_file_is_marked_once() {
        let dir = std::env::temp_dir().join(format!("vsesvit-motw-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("setup.exe");
        std::fs::write(&file, b"MZ").unwrap();
        mark_of_the_web(&file, "https://example.com/setup.exe").unwrap();
        mark_of_the_web(&file, "https://other.example/").unwrap();
        let mark = std::fs::read_to_string(format!("{}:Zone.Identifier", file.display()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            mark.unwrap(),
            "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.com/setup.exe\r\n"
        );
    }

    #[test]
    fn failed_placement_cancels_the_engine_download() {
        let cancelled = Cell::new(false);
        let cancel = || {
            cancelled.set(true);
            Ok(())
        };
        cancel_unplaced(Ok(()), cancel);
        assert!(!cancelled.get());
        cancel_unplaced(Err(windows_core::Error::new(E_FAIL, "locked")), cancel);
        assert!(cancelled.get());
    }

    #[test]
    fn unknown_sizes() {
        assert_eq!(known_total(-1), None);
        assert_eq!(known_total(0), Some(0));
        assert_eq!(known_total(10_000_000), Some(10_000_000));
    }
}
