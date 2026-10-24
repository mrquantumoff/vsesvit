//! The downloads a shell is running now: the bookkeeping around the engine's downloads.
//!
//! Core's list ([`super::Downloads`]) stores each download's start and outcome. [`Tracker`] holds
//! what exists only while the engine runs a download: its handle, its live byte counts
//! ([`Transfer`]), where its file is being written, and which tab started it. It never touches
//! the engine or the profile. The handle `H` is whatever the shell cancels, pauses or resumes a
//! download with (WebView2's operation, CEF's callback), and `K` names a tab (the shell's id).
//! The shell feeds it what the engine reports and acts on what it returns.
//!
//! - **Reservations.** A destination handed out is held until the download starts or the shell
//!   [`release`](Tracker::release)s it, so two downloads at once never get the same name.
//! - **The hold.** A file that can run code is written under its [`unconfirmed_path`] and ends
//!   [`State::Unconfirmed`] instead of [`State::Completed`], until the user keeps it.
//! - **Throttle.** [`Tracker::progressed`] says when a view should hear about a byte count: at
//!   most every [`PROGRESS_INTERVAL`], by the clock the caller passes in.
//! - **Engine states.** The engine reports a [`Reported`]; [`state_for`] is the list's state.
//! - **Parking.** [`Tracker::has_running_for`] tells whether closing a tab would end a download
//!   a normal window started (design section 4.2, "Download keep-alive").

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{DownloadId, Progress, State, Transfer, is_dangerous, listed_in, unconfirmed_path, unique_destination};
use crate::private::Browsing;

/// Entries the downloads list shows.
pub const LIST_LIMIT: usize = 500;

/// A running download's progress reaches the views at most this often.
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// The toolbar's downloads button.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Indicator {
    /// No download this window lists has started this session.
    Hidden,
    Idle,
    /// A download is running.
    Busy,
}

/// What the engine says a download is doing, in terms every engine can give.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Reported {
    Running,
    /// The user paused it.
    Paused,
    Completed,
    /// The user cancelled it.
    Cancelled,
    /// Something stopped it. The engine can resume it, or it cannot and the download failed.
    Interrupted { resumable: bool },
}

/// The list's state for the engine's report; a complete file that can run code waits for the
/// user.
pub fn state_for(reported: Reported, dangerous: bool) -> State {
    match reported {
        Reported::Running => State::InProgress,
        Reported::Paused => State::Paused,
        Reported::Completed if dangerous => State::Unconfirmed,
        Reported::Completed => State::Completed,
        Reported::Cancelled => State::Cancelled,
        Reported::Interrupted { resumable: true } => State::Interrupted,
        Reported::Interrupted { resumable: false } => State::Failed,
    }
}

/// Where a download's file goes, and where the engine writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The file's own name, which the list stores.
    pub destination: PathBuf,
    /// Where the engine writes: the destination, or its unconfirmed name.
    pub written: PathBuf,
    /// Whether the file can run code, so it waits to be kept.
    pub dangerous: bool,
}

/// A download that has a destination and a row in the list.
pub struct Started<H, K> {
    pub handle: H,
    pub browsing: Browsing,
    /// The tab that started it, when the engine can say.
    pub tab: Option<K>,
    pub placement: Placement,
    pub total: Option<u64>,
}

/// What changed for a download whose state moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settled<H, K> {
    pub state: State,
    /// Where the file is, or is being, written.
    pub written: PathBuf,
    pub browsing: Browsing,
    pub tab: Option<K>,
    pub handle: H,
}

struct Live<H, K> {
    handle: H,
    browsing: Browsing,
    tab: Option<K>,
    written: PathBuf,
    dangerous: bool,
    /// As last stored in the list.
    state: State,
    transfer: Transfer,
    notified: Instant,
}

/// The engine's downloads in progress. See the module docs.
pub struct Tracker<H, K> {
    live: HashMap<DownloadId, Live<H, K>>,
    reserved: HashSet<PathBuf>,
    started: bool,
    /// Whether a download started in the private session, whose windows alone show it.
    started_private: bool,
}

impl<H, K> Default for Tracker<H, K> {
    fn default() -> Self {
        Self { live: HashMap::new(), reserved: HashSet::new(), started: false, started_private: false }
    }
}

impl<H, K> Tracker<H, K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a file, a destination handed out or a download in progress has `path`.
    pub fn taken(&self, path: &Path) -> bool {
        path.exists() || self.reserved.contains(path) || self.live.values().any(|l| l.written == path)
    }

    /// `name` in `dir`, numbered while a file or another download has it or its unconfirmed
    /// name, and held for the caller until [`Tracker::track`] or [`Tracker::release`].
    pub fn reserve(&mut self, dir: &Path, name: &str) -> PathBuf {
        let path = unique_destination(dir, name, |p| self.taken(p));
        self.reserved.insert(path.clone());
        path
    }

    /// Where a download going to `destination`, served as `mime`, is written. A file that can
    /// run code whose unconfirmed name another download holds is numbered. The result is held
    /// like [`Tracker::reserve`]'s.
    pub fn place(&mut self, destination: &Path, mime: Option<&str>) -> Placement {
        let name = destination.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let dangerous = is_dangerous(&name, mime);
        let destination = match destination.parent() {
            Some(dir) if dangerous && self.taken(&unconfirmed_path(destination)) => {
                self.reserved.remove(destination);
                self.reserve(dir, &name)
            }
            _ => {
                self.reserved.insert(destination.to_owned());
                destination.to_owned()
            }
        };
        let written = if dangerous { unconfirmed_path(&destination) } else { destination.clone() };
        Placement { destination, written, dangerous }
    }

    /// Gives up a destination that [`Tracker::reserve`] or [`Tracker::place`] held and no download
    /// took.
    pub fn release(&mut self, destination: &Path) {
        self.reserved.remove(destination);
    }

    /// The download `id` of the list is running now.
    pub fn track(&mut self, id: DownloadId, started: Started<H, K>, now: Instant) {
        let Started { handle, browsing, tab, placement, total } = started;
        self.reserved.remove(&placement.destination);
        match browsing {
            Browsing::Normal => self.started = true,
            Browsing::Private => self.started_private = true,
        }
        self.live.insert(
            id,
            Live {
                handle,
                browsing,
                tab,
                written: placement.written,
                dangerous: placement.dangerous,
                state: State::InProgress,
                transfer: Transfer::new(now, total),
                notified: now,
            },
        );
    }

    /// Takes new byte counts, and says whether the views should hear of them now.
    pub fn progressed(&mut self, id: DownloadId, now: Instant, received: u64, total: Option<u64>) -> bool {
        let Some(entry) = self.live.get_mut(&id) else { return false };
        entry.transfer.update(now, received, total);
        let due = now.saturating_duration_since(entry.notified) >= PROGRESS_INTERVAL;
        if due {
            entry.notified = now;
        }
        due
    }

    /// The engine reports `reported` for `id`. `None` when `id` is not tracked or its state did
    /// not change. A download that is no longer live leaves the tracker.
    pub fn reported(&mut self, id: DownloadId, reported: Reported, now: Instant) -> Option<Settled<H, K>>
    where
        H: Clone,
        K: Clone,
    {
        let entry = self.live.get_mut(&id)?;
        let state = state_for(reported, entry.dangerous);
        if state == entry.state {
            return None;
        }
        entry.state = state;
        entry.transfer.restart(now);
        let settled =
            Settled { state, written: entry.written.clone(), browsing: entry.browsing, tab: entry.tab.clone(), handle: entry.handle.clone() };
        if !state.is_live() {
            self.live.remove(&id);
        }
        Some(settled)
    }

    /// The live byte counts and speed of a download the engine holds.
    pub fn progress(&self, id: DownloadId, now: Instant) -> Option<Progress> {
        self.live.get(&id).map(|l| l.transfer.at(now))
    }

    /// The engine's handle on a download it holds.
    pub fn handle(&self, id: DownloadId) -> Option<&H> {
        self.live.get(&id).map(|l| &l.handle)
    }

    /// The downloads button of a window of `browsing`'s kind: busy while a download that window
    /// lists runs, else shown once such a download started this session.
    pub fn indicator(&self, browsing: Browsing) -> Indicator {
        if self.live.values().any(|l| l.state == State::InProgress && listed_in(l.browsing, browsing)) {
            Indicator::Busy
        } else if self.started || (self.started_private && listed_in(Browsing::Private, browsing)) {
            Indicator::Idle
        } else {
            Indicator::Hidden
        }
    }

    /// Whether `tab` started a download that is running in a normal window's session: closing
    /// the tab would end it unless its browser is parked. Private downloads are never parked,
    /// and a paused or interrupted one is not running.
    pub fn has_running_for(&self, tab: &K) -> bool
    where
        K: PartialEq,
    {
        self.live
            .values()
            .any(|l| l.browsing == Browsing::Normal && l.state == State::InProgress && l.tab.as_ref() == Some(tab))
    }

    /// The private session ended: its windows show no button for its downloads from now on, and
    /// the handles of its downloads in progress are returned to cancel. They leave the tracker
    /// when the engine reports them ended.
    pub fn end_private(&mut self) -> Vec<H>
    where
        H: Clone,
    {
        self.started_private = false;
        self.live.values().filter(|l| l.browsing == Browsing::Private).map(|l| l.handle.clone()).collect()
    }
}
