//! The downloads controller. Core keeps the list of downloads ([`vsesvit_core::downloads`]);
//! this owns the engine's downloads in progress and their live byte counts, decides where
//! each file goes (the download folder, never overwriting a file, or a save dialog when
//! the user asked for one), shows a toast when a download starts and ends, and tells the
//! Downloads view and the windows' header buttons what changed.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::downloads::{Download, DownloadId, State, sanitize, unique_destination};
use vsesvit_core::prefs::keys;

use crate::profile::Core;
use crate::session::now_ms;
use crate::window::BrowserWindow;

/// How often a download in progress tells the subscribers about its byte count.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// How many entries the list shows.
const LIST_LIMIT: usize = 500;

/// What a subscriber is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// An entry was added, finished, or left the list.
    List,
    /// The byte count of a download in progress moved.
    Progress(DownloadId),
}

type Subscriber = Rc<dyn Fn(Change)>;

/// Returned by [`Downloads::subscribe`], to unsubscribe with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Subscription(u64);

/// An engine download that has a destination and a row in core's list.
struct Live {
    handle: webkit::Download,
    received: u64,
    total: Option<u64>,
    notified: Instant,
}

/// Where one engine download is, from the controller's point of view.
#[derive(Clone, Copy)]
enum Phase {
    /// Waiting for a destination: no row in the list yet.
    Deciding,
    Running(DownloadId),
    Ended,
}

pub(crate) struct Downloads {
    core: Core,
    app: glib::WeakRef<adw::Application>,
    /// The platform's Downloads folder, used while the preference names none.
    default_dir: PathBuf,
    live: RefCell<HashMap<DownloadId, Live>>,
    subscribers: RefCell<Vec<(Subscription, Subscriber)>>,
    next_subscription: Cell<u64>,
    started_this_session: Cell<bool>,
}

impl Downloads {
    /// Marks what the last run left in progress as failed, then watches `session`, so it
    /// runs before any download can begin.
    pub(crate) fn new(
        app: &adw::Application,
        core: Core,
        session: &webkit::NetworkSession,
        default_dir: PathBuf,
    ) -> Rc<Self> {
        let interrupted = core.borrow_mut().downloads().interrupt_stale();
        if let Err(e) = interrupted {
            log::warn!("downloads: {e}");
        }
        let downloads = Rc::new(Downloads {
            core,
            app: app.downgrade(),
            default_dir,
            live: RefCell::new(HashMap::new()),
            subscribers: RefCell::new(Vec::new()),
            next_subscription: Cell::new(0),
            started_this_session: Cell::new(false),
        });
        let weak = Rc::downgrade(&downloads);
        session.connect_download_started(move |_, download| match weak.upgrade() {
            Some(downloads) => downloads.track(download),
            None => download.cancel(),
        });
        downloads
    }

    /// The download folder: the preference, or else the platform's Downloads folder. Read
    /// for every download, so a change applies to the next one.
    pub(crate) fn directory(&self) -> PathBuf {
        let chosen = self.core.borrow_mut().prefs().get(&keys::DOWNLOADS_DIR);
        chosen.unwrap_or_else(|| self.default_dir.clone())
    }

    /// Whether a download has started since the browser did.
    pub(crate) fn started_this_session(&self) -> bool {
        self.started_this_session.get()
    }

    /// Newest first.
    pub(crate) fn list(&self) -> Vec<Download> {
        let listed = self.core.borrow_mut().downloads().list(LIST_LIMIT);
        listed.unwrap_or_else(|e| {
            log::warn!("downloads: {e}");
            Vec::new()
        })
    }

    /// The live `(received, total)` of a download in progress.
    pub(crate) fn progress(&self, id: DownloadId) -> Option<(u64, Option<u64>)> {
        self.live.borrow().get(&id).map(|live| (live.received, live.total))
    }

    pub(crate) fn cancel(&self, id: DownloadId) {
        let handle = self.live.borrow().get(&id).map(|live| live.handle.clone());
        if let Some(handle) = handle {
            handle.cancel();
        }
    }

    /// Takes the entry off the list; the file stays.
    pub(crate) fn remove(&self, id: DownloadId) {
        let removed = self.core.borrow_mut().downloads().remove(id);
        if let Err(e) = removed {
            log::warn!("downloads: {e}");
        }
        self.notify(Change::List);
    }

    /// Takes every entry that is not in progress off the list; the files stay.
    pub(crate) fn clear(&self) {
        let cleared = self.core.borrow_mut().downloads().clear();
        if let Err(e) = cleared {
            log::warn!("downloads: {e}");
        }
        self.notify(Change::List);
    }

    /// Calls `f` after every change until [`Downloads::unsubscribe`].
    pub(crate) fn subscribe(&self, f: impl Fn(Change) + 'static) -> Subscription {
        let token = Subscription(self.next_subscription.get());
        self.next_subscription.set(token.0 + 1);
        self.subscribers.borrow_mut().push((token, Rc::new(f)));
        token
    }

    pub(crate) fn unsubscribe(&self, token: Subscription) {
        self.subscribers.borrow_mut().retain(|(t, _)| *t != token);
    }

    fn notify(&self, change: Change) {
        // A subscriber may subscribe or unsubscribe while it runs.
        let subscribers: Vec<_> = self.subscribers.borrow().iter().map(|(_, f)| f.clone()).collect();
        for f in subscribers {
            f(change);
        }
    }

    // The engine's side.

    fn track(self: &Rc<Self>, download: &webkit::Download) {
        let phase = Rc::new(Cell::new(Phase::Deciding));
        let weak = Rc::downgrade(self);
        download.connect_decide_destination(glib::clone!(
            #[strong]
            weak,
            move |download, suggested| {
                match weak.upgrade() {
                    Some(downloads) => downloads.decide_destination(download, suggested),
                    None => download.cancel(),
                }
                true
            }
        ));
        download.connect_created_destination(glib::clone!(
            #[strong]
            weak,
            #[strong]
            phase,
            move |download, destination| {
                if let Some(downloads) = weak.upgrade()
                    && let Some(id) = downloads.started(download, Path::new(destination))
                {
                    phase.set(Phase::Running(id));
                }
            }
        ));
        download.connect_received_data(glib::clone!(
            #[strong]
            weak,
            #[strong]
            phase,
            move |download, _| {
                if let (Some(downloads), Phase::Running(id)) = (weak.upgrade(), phase.get()) {
                    downloads.progressed(id, download);
                }
            }
        ));
        // WebKit emits `finished` after `failed` too; the first one ends the download.
        download.connect_failed(glib::clone!(
            #[strong]
            weak,
            #[strong]
            phase,
            move |download, error| {
                let cancelled = error.matches(webkit::DownloadError::CancelledByUser);
                if !cancelled {
                    log::warn!("download of {} failed: {error}", describe(download));
                }
                let running = phase.replace(Phase::Ended);
                let Some(downloads) = weak.upgrade() else { return };
                if let Phase::Running(id) = running {
                    let state = if cancelled { State::Cancelled } else { State::Failed };
                    downloads.ended(id, state, download);
                }
                if !cancelled && let Some(window) = downloads.window_for(download) {
                    window.toast(adw::Toast::new(&format!("Download of “{}” failed", describe(download))));
                }
            }
        ));
        download.connect_finished(glib::clone!(
            #[strong]
            weak,
            move |download| {
                let Phase::Running(id) = phase.replace(Phase::Ended) else { return };
                let Some(downloads) = weak.upgrade() else { return };
                downloads.ended(id, State::Completed, download);
                downloads.completed_toast(download);
            }
        ));
    }

    /// Straight into the download folder under a free name, or wherever the save dialog
    /// says. Cancelling the dialog cancels the download.
    fn decide_destination(&self, download: &webkit::Download, suggested: &str) {
        let dir = self.directory();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("cannot create {}: {e}", dir.display());
            download.cancel();
            return;
        }
        let ask = self.core.borrow_mut().prefs().get(&keys::DOWNLOADS_ASK);
        if !ask {
            let destination = unique_destination(&dir, suggested, Path::exists);
            match destination.to_str() {
                Some(path) => download.set_destination(path),
                None => download.cancel(),
            }
            return;
        }
        let dialog = gtk::FileDialog::builder()
            .title("Save File")
            .initial_folder(&gio::File::for_path(&dir))
            .initial_name(sanitize(suggested))
            .modal(true)
            .build();
        let window = self.window_for(download);
        let download = download.clone();
        glib::spawn_future_local(async move {
            let chosen = dialog.save_future(window.as_ref()).await.ok().and_then(|file| file.path());
            match chosen.as_deref().and_then(Path::to_str) {
                Some(path) => {
                    // The dialog already asked before replacing a file.
                    download.set_allow_overwrite(true);
                    download.set_destination(path);
                }
                None => download.cancel(),
            }
        });
    }

    /// The download has its file: it goes on the list, and every window shows the
    /// downloads button from now on.
    fn started(&self, download: &webkit::Download, destination: &Path) -> Option<DownloadId> {
        let url = download.request().and_then(|r| r.uri()).map(String::from).unwrap_or_default();
        let total = total_of(download);
        let now = u64::try_from(now_ms()).unwrap_or(0);
        let started = self.core.borrow_mut().downloads().start(&url, destination, total, now);
        let record = match started {
            Ok(record) => record,
            Err(e) => {
                log::warn!("downloads: {e}");
                return None;
            }
        };
        self.live.borrow_mut().insert(
            record.id,
            Live { handle: download.clone(), received: 0, total, notified: Instant::now() },
        );
        self.started_this_session.set(true);
        for window in self.windows() {
            window.show_downloads_button();
        }
        if let Some(window) = self.window_for(download) {
            let toast = adw::Toast::builder()
                .title(format!("Downloading “{}”", file_name(destination)))
                .button_label("Show")
                .action_name("win.show-downloads")
                .build();
            window.toast(toast);
        }
        self.notify(Change::List);
        Some(record.id)
    }

    fn progressed(&self, id: DownloadId, download: &webkit::Download) {
        {
            let mut live = self.live.borrow_mut();
            let Some(entry) = live.get_mut(&id) else { return };
            entry.received = download.received_data_length();
            entry.total = total_of(download);
            if entry.notified.elapsed() < PROGRESS_INTERVAL {
                return;
            }
            entry.notified = Instant::now();
        }
        self.notify(Change::Progress(id));
    }

    fn ended(&self, id: DownloadId, state: State, download: &webkit::Download) {
        self.live.borrow_mut().remove(&id);
        let received = download.received_data_length();
        let total = total_of(download).or((state == State::Completed).then_some(received));
        let finished = self.core.borrow_mut().downloads().finish(id, state, received, total);
        if let Err(e) = finished {
            log::warn!("downloads: {e}");
        }
        self.notify(Change::List);
    }

    fn completed_toast(&self, download: &webkit::Download) {
        let Some(destination) = download.destination() else { return };
        log::info!("downloaded {destination}");
        let Some(window) = self.window_for(download) else { return };
        let path = PathBuf::from(destination.as_str());
        let toast = adw::Toast::builder()
            .title(format!("“{}” downloaded", file_name(&path)))
            .button_label("Open")
            .build();
        toast.connect_button_clicked(glib::clone!(
            #[weak]
            window,
            move |_| open(&window, &path)
        ));
        window.toast(toast);
    }

    fn windows(&self) -> Vec<BrowserWindow> {
        self.app
            .upgrade()
            .map(|app| app.windows().into_iter().filter_map(|w| w.downcast().ok()).collect())
            .unwrap_or_default()
    }

    /// The window of the tab that started the download, or else the most recently used one.
    fn window_for(&self, download: &webkit::Download) -> Option<BrowserWindow> {
        download
            .web_view()
            .and_then(|view| view.root())
            .and_downcast::<BrowserWindow>()
            .or_else(|| self.windows().into_iter().next())
    }
}

/// The size the server announced, if it did.
fn total_of(download: &webkit::Download) -> Option<u64> {
    download.response().map(|r| r.content_length()).filter(|&length| length > 0)
}

fn describe(download: &webkit::Download) -> String {
    download
        .destination()
        .map_or_else(|| "file".to_owned(), |d| file_name(Path::new(d.as_str())))
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Opens a file, or a folder in the file manager, with the default application.
pub(crate) fn open(window: &impl IsA<gtk::Window>, path: &Path) {
    let file = gio::File::for_path(path);
    gtk::FileLauncher::new(Some(&file)).launch(Some(window), None::<&gio::Cancellable>, |result| {
        if let Err(e) = result {
            log::warn!("cannot open the download: {e}");
        }
    });
}

/// Opens the file's folder with the file selected, where the file manager allows.
pub(crate) fn show_in_folder(window: &impl IsA<gtk::Window>, path: &Path) {
    let file = gio::File::for_path(path);
    gtk::FileLauncher::new(Some(&file)).open_containing_folder(
        Some(window),
        None::<&gio::Cancellable>,
        |result| {
            if let Err(e) = result {
                log::warn!("cannot show the download: {e}");
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use vsesvit_core::downloads::status_line;

    use super::*;
    use crate::test_support::{
        Reply, STALLED_FILE_SENT, STALLED_FILE_SIZE, Server, browser, scratch_dir, wait_until,
    };
    use crate::window::Focus;

    #[gtk::test]
    fn a_download_shows_live_progress_and_stays_on_the_list_once_cancelled() {
        let server = Server::start("127.0.0.1", |path| match path {
            "/big.bin" => Reply::StalledFile,
            _ => Reply::NotFound,
        });
        let browser = browser();
        let dir = scratch_dir("downloads");
        let set = browser.core().borrow_mut().prefs().set(&keys::DOWNLOADS_DIR, &Some(dir.clone()));
        set.expect("the folder preference is written");
        let downloads = browser.downloads().clone();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let subscription = downloads.subscribe(glib::clone!(
            #[strong]
            changes,
            move |change| changes.borrow_mut().push(change)
        ));
        let window = BrowserWindow::new(&browser);
        window.open_tab(Some(&server.url("/big.bin")), None, Focus::Foreground);

        let in_folder = |d: &Download| d.path.parent() == Some(dir.as_path());
        wait_until("the download to receive its first bytes", || {
            downloads.list().iter().filter(|d| in_folder(d)).any(|d| {
                downloads.progress(d.id).is_some_and(|(received, _)| received == STALLED_FILE_SENT)
            })
        });
        let entry = downloads.list().into_iter().find(|d| in_folder(d)).expect("the entry");
        assert_eq!(entry.state, State::InProgress);
        assert_eq!(entry.path, dir.join("big.bin"));
        assert_eq!(downloads.progress(entry.id), Some((STALLED_FILE_SENT, Some(STALLED_FILE_SIZE))));
        assert_eq!(status_line(&entry, downloads.progress(entry.id), false), "1.0 KB of 1.0 MB");
        assert!(downloads.started_this_session());

        downloads.cancel(entry.id);
        wait_until("the entry to read as cancelled", || {
            downloads.list().iter().any(|d| d.id == entry.id && d.state == State::Cancelled)
        });
        assert_eq!(downloads.progress(entry.id), None, "no live counts once it ended");
        assert_eq!(changes.borrow().first(), Some(&Change::List), "the start is announced");
        assert_eq!(changes.borrow().last(), Some(&Change::List), "the end is announced");

        downloads.unsubscribe(subscription);
        window.destroy();
        let reset = browser.core().borrow_mut().prefs().reset(&keys::DOWNLOADS_DIR);
        reset.expect("the folder preference is reset");
    }
}
