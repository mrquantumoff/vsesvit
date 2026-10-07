//! The downloads controller. Core keeps the list of downloads ([`vsesvit_core::downloads`]);
//! this owns the engine's downloads in progress and their live byte counts, decides where
//! each file goes (the download folder, never overwriting a file, or a save dialog when
//! the user asked for one), shows a toast when a download starts and ends, and tells the
//! Downloads view and the windows' header buttons what changed.
//!
//! A file of a type that can run code is written under its unconfirmed name and waits for the
//! user to keep or discard it, in the Downloads view or in the warning the window shows under
//! its downloads button. WebKitGTK cannot pause a download, so nothing offers to.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::downloads::{
    Download, DownloadId, Progress, State, Transfer, is_dangerous, listed_in, sanitize, status_line, unconfirmed_path,
    unique_destination,
};
use vsesvit_core::prefs::keys;
use vsesvit_core::private::Browsing;
use webkit::prelude::*;

use crate::dialogs::plain_toast;
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
    /// Written under its unconfirmed name, to wait for the user once it is complete.
    dangerous: bool,
    transfer: Transfer,
    notified: Instant,
}

/// The destination of a download being placed, while the engine writes a file that can run
/// code under its unconfirmed name.
type Held = Rc<RefCell<Option<PathBuf>>>;

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
    /// Whether a download started in the private session, whose windows alone show it.
    private_started: Cell<bool>,
    /// The URI [`Downloads::download_to`] asked for and where it goes, until it starts.
    chosen: RefCell<Option<(String, PathBuf)>>,
    /// Destinations handed out whose files are not made yet, so two downloads at once never
    /// get the same one.
    reserved: RefCell<HashSet<PathBuf>>,
    /// The private session's engine downloads that have not ended, which end with it.
    private: RefCell<Vec<webkit::Download>>,
}

impl Downloads {
    /// Marks what the last run left in progress as failed, then watches the profile's
    /// `session`, so it runs before any download can begin.
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
            private_started: Cell::new(false),
            chosen: RefCell::new(None),
            reserved: RefCell::new(HashSet::new()),
            private: RefCell::new(Vec::new()),
        });
        downloads.watch(session, Browsing::Normal);
        downloads
    }

    /// Takes the downloads `session` starts, a session of `browsing`'s windows, whose rows go
    /// to core's list for that kind. Disconnecting the handler stops it.
    pub(crate) fn watch(self: &Rc<Self>, session: &webkit::NetworkSession, browsing: Browsing) -> glib::SignalHandlerId {
        let weak = Rc::downgrade(self);
        session.connect_download_started(move |_, download| match weak.upgrade() {
            Some(downloads) => downloads.track(download, browsing),
            None => download.cancel(),
        })
    }

    /// The private session ended: its downloads still running are cancelled, as Chrome
    /// cancels them when the last incognito window closes. Their files stay.
    pub(crate) fn cancel_private(&self) {
        self.private_started.set(false);
        // Taken first: a cancel ends the download at once, which forgets it.
        for download in self.private.take() {
            download.cancel();
        }
    }

    /// The download folder: the preference, or else the platform's Downloads folder. Read
    /// for every download, so a change applies to the next one.
    pub(crate) fn directory(&self) -> PathBuf {
        let chosen = self.core.borrow_mut().prefs().get(&keys::DOWNLOADS_DIR);
        chosen.unwrap_or_else(|| self.default_dir.clone())
    }

    /// Downloads `uri` from `view` into `path`, which the user already chose in a save dialog.
    pub(crate) fn download_to(&self, view: &webkit::WebView, uri: &str, path: &Path) {
        self.chosen.replace(Some((uri.to_owned(), path.to_owned())));
        view.download_uri(uri);
    }

    /// Whether a download that windows of `browsing`'s kind list has started since the browser
    /// did.
    pub(crate) fn started_this_session(&self, browsing: Browsing) -> bool {
        self.started_this_session.get() || (self.private_started.get() && listed_in(Browsing::Private, browsing))
    }

    /// Newest first, what windows of `browsing`'s kind list.
    pub(crate) fn list(&self, browsing: Browsing) -> Vec<Download> {
        let listed = self.core.borrow_mut().downloads().list(LIST_LIMIT, browsing);
        listed.unwrap_or_else(|e| {
            log::warn!("downloads: {e}");
            Vec::new()
        })
    }

    /// The live byte counts and speed of a download in progress.
    pub(crate) fn progress(&self, id: DownloadId) -> Option<Progress> {
        self.live.borrow().get(&id).map(|live| live.transfer.at(Instant::now()))
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

    /// Moves a file that can run code, which the user chose to keep, to its own name.
    pub(crate) fn keep(&self, id: DownloadId) {
        let kept = self.core.borrow_mut().downloads().keep(id);
        match kept {
            Ok(Some(path)) => log::info!("kept {}", id.browsing().loggable(&path.display())),
            Ok(None) => {}
            Err(e) => log::warn!("downloads: keeping download {}: {e}", id.0),
        }
        self.notify(Change::List);
    }

    /// Deletes a file that can run code, which the user chose not to keep, and its entry.
    pub(crate) fn discard(&self, id: DownloadId) {
        let discarded = self.core.borrow_mut().downloads().discard(id);
        if let Err(e) = discarded {
            log::warn!("downloads: discarding download {}: {e}", id.0);
        }
        self.notify(Change::List);
    }

    /// Takes every entry nothing is left to happen to off the list of `browsing`'s windows; the
    /// files stay.
    pub(crate) fn clear(&self, browsing: Browsing) {
        let cleared = self.core.borrow_mut().downloads().clear(browsing);
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

    fn track(self: &Rc<Self>, download: &webkit::Download, browsing: Browsing) {
        if browsing == Browsing::Private {
            self.private.borrow_mut().push(download.clone());
        }
        let phase = Rc::new(Cell::new(Phase::Deciding));
        let held = Held::default();
        let weak = Rc::downgrade(self);
        let uri = download.request().and_then(|r| r.uri());
        let chosen = self.chosen.borrow_mut().take_if(|(wanted, _)| uri.as_deref() == Some(wanted.as_str())).map(|(_, path)| path);
        let chosen = Cell::new(chosen);
        download.connect_decide_destination(glib::clone!(
            #[strong]
            weak,
            #[strong]
            held,
            move |download, suggested| {
                match (weak.upgrade(), chosen.take()) {
                    (Some(downloads), Some(path)) => {
                        download.set_allow_overwrite(true);
                        match written(download, &path, &held).to_str() {
                            Some(path) => download.set_destination(path),
                            None => refuse(downloads.window_for(download, browsing), download, NOT_UTF8),
                        }
                    }
                    (Some(downloads), None) => downloads.decide_destination(download, suggested, &held, browsing),
                    (None, _) => download.cancel(),
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
                let Some(downloads) = weak.upgrade() else { return };
                downloads.reserved.borrow_mut().remove(Path::new(destination));
                if let Some(id) = downloads.started(download, Path::new(destination), held.take(), browsing) {
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
                    log::warn!("download of {} failed: {error}", browsing.loggable(&describe(download)));
                }
                let running = phase.replace(Phase::Ended);
                let Some(downloads) = weak.upgrade() else { return };
                if let Some(destination) = download.destination() {
                    downloads.reserved.borrow_mut().remove(Path::new(destination.as_str()));
                }
                if let Phase::Running(id) = running {
                    let state = if cancelled { State::Cancelled } else { State::Failed };
                    downloads.ended(id, state, download);
                }
                if !cancelled && let Some(window) = downloads.window_for(download, browsing) {
                    window.toast(plain_toast(&format!("Download of “{}” failed", describe(download))));
                }
            }
        ));
        download.connect_finished(glib::clone!(
            #[strong]
            weak,
            move |download| {
                let Some(downloads) = weak.upgrade() else { return };
                downloads.private.borrow_mut().retain(|d| d != download);
                let Phase::Running(id) = phase.replace(Phase::Ended) else { return };
                match downloads.ended(id, State::Completed, download) {
                    State::Unconfirmed => downloads.warn(id, download, browsing),
                    _ => downloads.completed_toast(download, browsing),
                }
            }
        ));
    }

    /// Straight into the download folder under a free name, or wherever the save dialog
    /// says. Cancelling the dialog cancels the download.
    fn decide_destination(&self, download: &webkit::Download, suggested: &str, held: &Held, browsing: Browsing) {
        let dir = self.directory();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            refuse(self.window_for(download, browsing), download, &format!("Cannot save to {}: {e}", dir.display()));
            return;
        }
        let ask = self.core.borrow_mut().prefs().get(&keys::DOWNLOADS_ASK);
        if !ask {
            let destination = unique_destination(&dir, suggested, |p| p.exists() || self.reserved.borrow().contains(p));
            let destination = written(download, &destination, held);
            match destination.to_str() {
                Some(path) => {
                    download.set_destination(path);
                    self.reserved.borrow_mut().insert(destination.clone());
                }
                None => refuse(self.window_for(download, browsing), download, NOT_UTF8),
            }
            return;
        }
        let dialog = gtk::FileDialog::builder()
            .title("Save File")
            .initial_folder(&gio::File::for_path(&dir))
            .initial_name(sanitize(suggested))
            .modal(true)
            .build();
        let window = self.window_for(download, browsing);
        let download = download.clone();
        let held = held.clone();
        glib::spawn_future_local(async move {
            let chosen = dialog.save_future(window.as_ref()).await.ok().and_then(|file| file.path());
            let chosen = chosen.map(|path| written(&download, &path, &held));
            match chosen.as_deref().map(Path::to_str) {
                Some(Some(path)) => {
                    // The dialog already asked before replacing a file.
                    download.set_allow_overwrite(true);
                    download.set_destination(path);
                }
                Some(None) => refuse(window, &download, NOT_UTF8),
                None => download.cancel(),
            }
        });
    }

    /// The download has its file: it goes on the list, and every window that lists it shows
    /// the downloads button from now on. `held` is where a file that can run code goes once kept.
    fn started(
        &self,
        download: &webkit::Download,
        destination: &Path,
        held: Option<PathBuf>,
        browsing: Browsing,
    ) -> Option<DownloadId> {
        let url = download.request().and_then(|r| r.uri()).map(String::from).unwrap_or_default();
        let total = total_of(download);
        let now = u64::try_from(now_ms()).unwrap_or(0);
        let dangerous = held.is_some();
        let destination = held.as_deref().unwrap_or(destination);
        let started = self.core.borrow_mut().downloads().start(&url, destination, total, now, browsing);
        let record = match started {
            Ok(record) => record,
            Err(e) => {
                log::warn!("downloads: {e}");
                return None;
            }
        };
        self.live.borrow_mut().insert(
            record.id,
            Live { handle: download.clone(), dangerous, transfer: Transfer::new(Instant::now(), total), notified: Instant::now() },
        );
        match browsing {
            Browsing::Normal => self.started_this_session.set(true),
            Browsing::Private => self.private_started.set(true),
        }
        for window in self.windows().into_iter().filter(|w| listed_in(browsing, w.browsing())) {
            window.show_downloads_button();
        }
        if let Some(window) = self.window_for(download, browsing) {
            let toast = adw::Toast::builder()
                .title(format!("Downloading “{}”", file_name(destination)))
                .use_markup(false)
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
            entry.transfer.update(Instant::now(), download.received_data_length(), total_of(download));
            if entry.notified.elapsed() < PROGRESS_INTERVAL {
                return;
            }
            entry.notified = Instant::now();
        }
        self.notify(Change::Progress(id));
    }

    /// Stores how the download ended and returns it: a complete file that can run code waits
    /// for the user, unconfirmed.
    fn ended(&self, id: DownloadId, state: State, download: &webkit::Download) -> State {
        let dangerous = self.live.borrow_mut().remove(&id).is_some_and(|live| live.dangerous);
        let state = if state == State::Completed && dangerous { State::Unconfirmed } else { state };
        let received = download.received_data_length();
        let total = total_of(download).or(matches!(state, State::Completed | State::Unconfirmed).then_some(received));
        let finished = self.core.borrow_mut().downloads().update(id, state, received, total);
        if let Err(e) = finished {
            log::warn!("downloads: {e}");
        }
        self.notify(Change::List);
        state
    }

    /// Warns about an unconfirmed file in the window it came from.
    fn warn(&self, id: DownloadId, download: &webkit::Download, browsing: Browsing) {
        let entry = self.list(browsing).into_iter().find(|d| d.id == id);
        if let (Some(window), Some(entry)) = (self.window_for(download, browsing), entry) {
            window.warn_about_download(&entry);
        }
    }

    fn completed_toast(&self, download: &webkit::Download, browsing: Browsing) {
        let Some(destination) = download.destination() else { return };
        log::info!("downloaded {}", browsing.loggable(&destination));
        let Some(window) = self.window_for(download, browsing) else { return };
        let path = PathBuf::from(destination.as_str());
        let toast = adw::Toast::builder()
            .title(format!("“{}” downloaded", file_name(&path)))
            .use_markup(false)
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

    /// The window of the tab that started the download, one of `browsing`'s windows, or else
    /// the most recently used window that lists it.
    fn window_for(&self, download: &webkit::Download, browsing: Browsing) -> Option<BrowserWindow> {
        download
            .web_view()
            .and_then(|view| view.root())
            .and_downcast::<BrowserWindow>()
            .or_else(|| self.windows().into_iter().find(|w| listed_in(browsing, w.browsing())))
    }
}

const NOT_UTF8: &str = "Cannot save the download: its path is not valid UTF-8";

/// Where the engine writes a download going to `path`: there, or beside it under its
/// unconfirmed name if the file can run code, keeping the destination in `held` until it has
/// started. A destination whose unconfirmed name another download holds is numbered.
fn written(download: &webkit::Download, path: &Path, held: &Held) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mime = download.response().and_then(|r| r.mime_type());
    if !is_dangerous(&name, mime.as_deref()) {
        return path.to_owned();
    }
    let path = match path.parent() {
        Some(dir) if unconfirmed_path(path).exists() => unique_destination(dir, &name, Path::exists),
        _ => path.to_owned(),
    };
    let written = unconfirmed_path(&path);
    held.replace(Some(path));
    written
}

/// Chrome's warning about a downloaded file that can run code, with Keep and Discard, for the
/// window to show under its downloads button.
pub(crate) fn warning(downloads: &Rc<Downloads>, download: &Download) -> gtk::Popover {
    let name = gtk::Label::builder()
        .label(file_name(&download.path))
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .css_classes(["heading"])
        .build();
    let close = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text("Close")
        .css_classes(["flat", "circular"])
        .build();
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    top.append(&name);
    top.append(&close);
    let text = gtk::Label::builder()
        .label(status_line(download, None, false))
        .xalign(0.0)
        .wrap(true)
        .build();
    let keep = gtk::Button::with_mnemonic("_Keep");
    let discard = gtk::Button::builder().use_underline(true).label("_Discard").css_classes(["suggested-action"]).build();
    let buttons = gtk::Box::builder().spacing(8).halign(gtk::Align::End).build();
    buttons.append(&keep);
    buttons.append(&discard);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(6)
        .margin_end(6)
        .margin_top(6)
        .margin_bottom(6)
        .width_request(300)
        .build();
    content.append(&top);
    content.append(&text);
    content.append(&buttons);
    // It opens on its own while the user may be typing, so it takes no focus: a key press must
    // not answer it.
    let popover = gtk::Popover::builder()
        .child(&content)
        .position(gtk::PositionType::Bottom)
        .autohide(false)
        .css_classes(["download-warning"])
        .build();
    close.connect_clicked(glib::clone!(
        #[weak]
        popover,
        move |_| popover.popdown()
    ));
    for (button, kept) in [(keep, true), (discard, false)] {
        let (downloads, id, popover) = (Rc::downgrade(downloads), download.id, popover.downgrade());
        button.connect_clicked(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            match downloads.upgrade() {
                Some(downloads) if kept => downloads.keep(id),
                Some(downloads) => downloads.discard(id),
                None => {}
            }
        });
    }
    popover
}

/// Cancels a download the shell cannot place, saying why: WebKit reports the cancel as the
/// user's, which shows nothing.
fn refuse(window: Option<BrowserWindow>, download: &webkit::Download, why: &str) {
    log::warn!("download refused: {why}");
    if let Some(window) = window {
        window.toast(plain_toast(why));
    }
    download.cancel();
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
            downloads.list(Browsing::Normal).iter().filter(|d| in_folder(d)).any(|d| {
                downloads.progress(d.id).is_some_and(|live| live.received == STALLED_FILE_SENT)
            })
        });
        let entry = downloads.list(Browsing::Normal).into_iter().find(|d| in_folder(d)).expect("the entry");
        assert_eq!(entry.state, State::InProgress);
        assert_eq!(entry.path, dir.join("big.bin"));
        let live = downloads.progress(entry.id).expect("live counts");
        assert_eq!((live.received, live.total), (STALLED_FILE_SENT, Some(STALLED_FILE_SIZE)));
        let line = status_line(&entry, Some(live), false);
        assert!(line.contains("/s - 1.0 KB of 1.0 MB"), "{line}");
        assert!(downloads.started_this_session(Browsing::Normal));

        downloads.cancel(entry.id);
        wait_until("the entry to read as cancelled", || {
            downloads.list(Browsing::Normal).iter().any(|d| d.id == entry.id && d.state == State::Cancelled)
        });
        assert_eq!(downloads.progress(entry.id), None, "no live counts once it ended");
        assert_eq!(changes.borrow().first(), Some(&Change::List), "the start is announced");
        assert_eq!(changes.borrow().last(), Some(&Change::List), "the end is announced");

        downloads.unsubscribe(subscription);
        window.destroy();
        let reset = browser.core().borrow_mut().prefs().reset(&keys::DOWNLOADS_DIR);
        reset.expect("the folder preference is reset");
    }

    #[gtk::test]
    fn two_downloads_with_the_same_name_get_files_of_their_own() {
        let server = Server::start("127.0.0.1", |path| match path {
            "/big.bin" => Reply::StalledFile,
            _ => Reply::NotFound,
        });
        let browser = browser();
        let dir = scratch_dir("downloads-same-name");
        let set = browser.core().borrow_mut().prefs().set(&keys::DOWNLOADS_DIR, &Some(dir.clone()));
        set.expect("the folder preference is written");
        let downloads = browser.downloads().clone();
        let window = BrowserWindow::new(&browser);
        let tab = window.open_tab(None, None, Focus::Foreground);
        // Both ask where to go before either has made its file.
        tab.web_view().download_uri(&server.url("/big.bin"));
        tab.web_view().download_uri(&server.url("/big.bin"));

        let in_folder = || -> Vec<Download> {
            downloads.list(Browsing::Normal).into_iter().filter(|d| d.path.parent() == Some(dir.as_path())).collect()
        };
        wait_until("both downloads to start", || {
            in_folder().iter().filter(|d| d.state == State::InProgress).count() == 2
        });
        let mut paths: Vec<PathBuf> = in_folder().into_iter().map(|d| d.path).collect();
        paths.sort();
        assert_eq!(paths, [dir.join("big (1).bin"), dir.join("big.bin")]);

        for entry in in_folder() {
            downloads.cancel(entry.id);
        }
        wait_until("both to end", || in_folder().iter().all(|d| d.state == State::Cancelled));
        window.destroy();
        let reset = browser.core().borrow_mut().prefs().reset(&keys::DOWNLOADS_DIR);
        reset.expect("the folder preference is reset");
    }

    fn button_labelled(widget: &gtk::Widget, label: &str) -> Option<gtk::Button> {
        let own = widget.downcast_ref::<gtk::Button>().filter(|b| b.label().as_deref() == Some(label)).cloned();
        own.or_else(|| std::iter::successors(widget.first_child(), |child| child.next_sibling()).find_map(|child| button_labelled(&child, label)))
    }

    #[gtk::test]
    fn a_script_waits_under_its_unconfirmed_name_until_kept_or_discarded() {
        let server = Server::start("127.0.0.1", |path| match path {
            "/run.sh" => Reply::Body("application/octet-stream", b"#!/bin/sh\n".to_vec()),
            _ => Reply::NotFound,
        });
        let browser = browser();
        let dir = scratch_dir("downloads-dangerous");
        let set = browser.core().borrow_mut().prefs().set(&keys::DOWNLOADS_DIR, &Some(dir.clone()));
        set.expect("the folder preference is written");
        let downloads = browser.downloads().clone();
        let window = BrowserWindow::new(&browser);
        window.present();
        let tab = window.open_tab(None, None, Focus::Foreground);
        let waiting = |seen: Option<DownloadId>| {
            downloads
                .list(Browsing::Normal)
                .into_iter()
                .find(|d| d.path.parent() == Some(dir.as_path()) && d.state == State::Unconfirmed && Some(d.id) != seen)
        };

        tab.web_view().download_uri(&server.url("/run.sh"));
        wait_until("the script to wait for the user", || waiting(None).is_some());
        let first = waiting(None).expect("the entry");
        assert_eq!(first.path, dir.join("run.sh"));
        assert!(!first.path.exists(), "nothing at its name yet");
        assert_eq!(std::fs::read(unconfirmed_path(&first.path)).expect("the unconfirmed file"), b"#!/bin/sh\n");
        wait_until("the warning under the downloads button", || window.download_warning().is_some());
        let warning = window.download_warning().expect("the warning");
        button_labelled(warning.upcast_ref(), "_Keep").expect("Keep").emit_clicked();
        let listed = downloads.list(Browsing::Normal).into_iter().find(|d| d.id == first.id).map(|d| d.state);
        assert_eq!(listed, Some(State::Completed));
        assert_eq!(std::fs::read(&first.path).expect("the kept file"), b"#!/bin/sh\n");
        assert!(!unconfirmed_path(&first.path).exists());

        tab.web_view().download_uri(&server.url("/run.sh"));
        wait_until("the second to wait for the user", || waiting(Some(first.id)).is_some());
        let second = waiting(Some(first.id)).expect("the second entry");
        assert_eq!(second.path, dir.join("run (1).sh"), "the kept file has the name");
        downloads.discard(second.id);
        assert!(!unconfirmed_path(&second.path).exists() && !second.path.exists(), "discarded");
        assert!(downloads.list(Browsing::Normal).iter().all(|d| d.id != second.id), "off the list");

        window.destroy();
        let reset = browser.core().borrow_mut().prefs().reset(&keys::DOWNLOADS_DIR);
        reset.expect("the folder preference is reset");
    }

    fn shows_text(widget: &gtk::Widget, text: &str) -> bool {
        widget.downcast_ref::<gtk::Label>().is_some_and(|label| label.label().contains(text))
            || std::iter::successors(widget.first_child(), |child| child.next_sibling()).any(|child| shows_text(&child, text))
    }

    #[gtk::test]
    fn a_download_into_a_folder_that_cannot_be_made_says_why() {
        let server = Server::start("127.0.0.1", |path| match path {
            "/big.bin" => Reply::StalledFile,
            _ => Reply::NotFound,
        });
        let browser = browser();
        let file = scratch_dir("downloads-blocked").join("file");
        std::fs::write(&file, b"").expect("a file in the way");
        let dir = file.join("sub");
        let set = browser.core().borrow_mut().prefs().set(&keys::DOWNLOADS_DIR, &Some(dir.clone()));
        set.expect("the folder preference is written");
        let window = BrowserWindow::new(&browser);
        window.open_tab(Some(&server.url("/big.bin")), None, Focus::Foreground);

        wait_until("a toast saying why", || shows_text(window.upcast_ref(), "Cannot save to"));
        let downloads = browser.downloads();
        assert!(!downloads.list(Browsing::Normal).iter().any(|d| d.path.starts_with(&file)), "nothing is listed");

        window.destroy();
        let reset = browser.core().borrow_mut().prefs().reset(&keys::DOWNLOADS_DIR);
        reset.expect("the folder preference is reset");
    }
}
