//! Downloads: where WebView2 saves each file, the live byte counts, and core's downloads list.
//!
//! Every tab hands its `DownloadStarting` here. The file goes to the download folder under a
//! name no file or running download has, or where the user says when they asked to be asked;
//! core records the start and the outcome. Byte counts stay in memory while a download runs.
//! Views subscribe while they are open, and every window's toolbar shows the downloads button
//! once a download started this session.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
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

/// A download the engine is running.
struct Live {
    operation: CoreWebView2DownloadOperation,
    path: PathBuf,
    received: u64,
    total: Option<u64>,
    notified: Instant,
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
        if !downloads.live.borrow().is_empty() {
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
        let operation = self
            .downloads
            .live
            .borrow()
            .get(&id)
            .map(|l| l.operation.clone());
        if let Some(Err(e)) = operation.map(|o| o.Cancel()) {
            log::warn!("cancel download: {e}");
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
        window: &BrowserWindow,
        args: &CoreWebView2DownloadStartingEventArgs,
    ) {
        cancel_unplaced(self.place_download(window, args), || args.SetCancel(true));
    }

    fn place_download(
        self: &Rc<Self>,
        window: &BrowserWindow,
        args: &CoreWebView2DownloadStartingEventArgs,
    ) -> Result<()> {
        args.SetHandled(true)?;
        // The engine's default path ends in the name the server suggested.
        let suggested = args.ResultFilePath()?;
        let name = list::sanitize(&suggested);
        let dir = self.download_dir();
        if !self.core(|p| p.prefs().get(&keys::DOWNLOADS_ASK)) {
            let path = self.free_path(&dir, &name);
            return self.begin_download(args, &path);
        }
        let deferral = args.GetDeferral()?;
        let owner = window.window_id();
        let browser = Rc::downgrade(self);
        let args = args.clone();
        exec::spawn(async move {
            let picked = match owner {
                Ok(owner) => pickers::pick_save_file(owner, &dir, &name).await,
                Err(e) => Err(e),
            };
            let placed = match (picked, browser.upgrade()) {
                (Ok(Some(path)), Some(browser)) => browser.begin_download(&args, &path),
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
        let live = self.downloads.live.borrow();
        list::unique_destination(dir, name, |path| {
            path.exists() || live.values().any(|l| l.path == path)
        })
    }

    /// Sends the download to `path` and records it.
    fn begin_download(
        self: &Rc<Self>,
        args: &CoreWebView2DownloadStartingEventArgs,
        path: &Path,
    ) -> Result<()> {
        args.SetResultFilePath(&path.to_string_lossy())?;
        let operation = args.DownloadOperation()?;
        let total = known_total(operation.TotalBytesToReceive()?);
        let url = operation.Uri()?;
        let now = u64::try_from(crate::session::now_ms()).unwrap_or(0);
        let download = self
            .core(|p| p.downloads().start(&url, path, total, now, Browsing::Normal))
            .map_err(|e| windows_core::Error::new(E_FAIL, e.to_string()))?;
        let id = download.id;
        log::info!("downloading {url} to {}", path.display());
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
                path: path.to_owned(),
                received: 0,
                total,
                notified: Instant::now(),
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
        let (Ok(state), Ok(reason)) = (operation.State(), operation.InterruptReason()) else {
            return;
        };
        let Some(state) = final_state(state, reason) else {
            return;
        };
        if self.downloads.live.borrow_mut().remove(&id).is_none() {
            return;
        }
        let (received, total) = counts(operation);
        log::info!("download {} ended {state:?}: {received} bytes", id.0);
        if let Err(e) = self.core(|p| p.downloads().finish(id, state, received, total)) {
            log::warn!("downloads list: {e}");
        }
        self.downloads_changed(Change::List);
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

/// Logs a download that could not be sent to its path or recorded, and cancels it: the engine
/// would otherwise save it with no entry in the list and no way to stop it.
fn cancel_unplaced(placed: Result<()>, cancel: impl FnOnce() -> Result<()>) {
    if let Err(e) = placed {
        log::warn!("download: {e}");
        let _ = cancel();
    }
}

/// The list's state for the engine's; `None` while the download runs.
fn final_state(
    state: CoreWebView2DownloadState,
    reason: CoreWebView2DownloadInterruptReason,
) -> Option<State> {
    if state == CoreWebView2DownloadState::Completed {
        Some(State::Completed)
    } else if state != CoreWebView2DownloadState::Interrupted {
        None
    } else if reason == CoreWebView2DownloadInterruptReason::UserCanceled {
        Some(State::Cancelled)
    } else {
        Some(State::Failed)
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
        assert_eq!(final_state(Engine::InProgress, Reason::None), None);
        assert_eq!(
            final_state(Engine::Completed, Reason::None),
            Some(State::Completed)
        );
        assert_eq!(
            final_state(Engine::Interrupted, Reason::UserCanceled),
            Some(State::Cancelled)
        );
        assert_eq!(
            final_state(Engine::Interrupted, Reason::NetworkFailed),
            Some(State::Failed)
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
