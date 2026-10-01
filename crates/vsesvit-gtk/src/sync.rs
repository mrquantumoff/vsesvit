//! Sync with a Vsesvit sync server (`vsesvit-sync`). The browser owns one [`Syncer`]: it signs
//! the profile in and out, and syncs 10 seconds after the first window opens, then every minute,
//! soon after a local change, when asked, and once more as the browser quits. What touches the
//! profile runs on the UI thread; the network steps run on worker threads. Settings watches the
//! [`State`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::Profile;
use vsesvit_core::crdt::Seq;
use vsesvit_core::prefs::keys;
use vsesvit_sync::status::{Action, State};
use vsesvit_sync::{Account, Error, Http, Round, SignIn};

use crate::browser::{self, Browser};
use crate::window::Focus;

const FIRST_SYNC_DELAY_SECS: u32 = 10;
const TICK_SECS: u32 = 2;
/// How often a sync starts when nothing changes here, for what other devices changed.
const SYNC_INTERVAL: Duration = Duration::from_secs(60);
/// How soon after the last sync started a local change is synced.
const SYNC_SOON: Duration = Duration::from_secs(10);
/// How long quitting waits for the final sync.
const FINAL_SYNC_WAIT: Duration = Duration::from_secs(3);
/// Rounds one sync runs at most; the next tick takes what is left.
const MAX_ROUNDS: usize = 50;

/// Called with the state after every change, and dropped once it returns false.
type Watcher = Box<dyn Fn(&State) -> bool>;

#[derive(Clone)]
pub(crate) struct Syncer(Rc<Inner>);

struct Inner {
    browser: Weak<browser::Inner>,
    http: Http,
    state: RefCell<State>,
    /// A sync is running. Kept apart from `State::SignedIn.syncing`, because a round can outlive
    /// the sign-in it started under.
    running: Cell<bool>,
    deleting: Cell<bool>,
    last_start: Cell<Option<Instant>>,
    /// `Profile::change_seq` when the last sync started; `None` asks for a sync at the next tick.
    start_seq: Cell<Option<Seq>>,
    /// `Profile::change_seq` when the last sync that completed started.
    synced_seq: Cell<Option<Seq>>,
    /// Bumped by every sign-in and cancel, so the later steps of an abandoned sign-in stop.
    attempt: Cell<u64>,
    canceller: RefCell<Option<Arc<AtomicBool>>>,
    watchers: RefCell<Vec<Watcher>>,
    window_seen: Cell<bool>,
    timer: RefCell<Option<glib::SourceId>>,
}

impl Syncer {
    pub(crate) fn new(browser: Weak<browser::Inner>, profile: &mut Profile) -> Syncer {
        Syncer(Rc::new(Inner {
            browser,
            http: Http::new(),
            state: RefCell::new(stored_state(profile, None, false)),
            running: Cell::new(false),
            deleting: Cell::new(false),
            last_start: Cell::new(None),
            start_seq: Cell::new(None),
            synced_seq: Cell::new(None),
            attempt: Cell::new(0),
            canceller: RefCell::default(),
            watchers: RefCell::default(),
            window_seen: Cell::new(false),
            timer: RefCell::default(),
        }))
    }

    /// Calls `show` with the state now and after every change, until it returns false. `show`
    /// holds its widgets weakly, so it returns false once they are gone.
    pub(crate) fn watch(&self, show: impl Fn(&State) -> bool + 'static) {
        if show(&self.0.state.borrow()) {
            self.0.watchers.borrow_mut().push(Box::new(show));
        }
    }

    /// The first window starts the syncs of an account signed in at startup.
    pub(crate) fn window_opened(&self) {
        if !self.0.window_seen.replace(true) && matches!(*self.0.state.borrow(), State::SignedIn { .. }) {
            self.schedule(FIRST_SYNC_DELAY_SECS);
        }
    }

    /// A button from [`vsesvit_sync::status::Status::actions`]. [`Action::DeleteServerData`] asks
    /// first, so Settings calls [`Syncer::delete_server_data`] itself.
    pub(crate) fn act(&self, action: Action) {
        match action {
            Action::SignIn => self.sign_in(),
            Action::Cancel => self.cancel(),
            Action::SyncNow => self.sync_now(),
            Action::SignOut => self.sign_out(),
            Action::DeleteServerData => {}
        }
    }

    pub(crate) fn is_signed_in(&self) -> bool {
        matches!(*self.0.state.borrow(), State::SignedIn { .. })
    }

    /// The user chose other data types: sync them now, or at a tick once the running sync ends.
    pub(crate) fn types_changed(&self) {
        self.0.start_seq.set(None);
        self.sync_now();
    }

    fn browser(&self) -> Option<Browser> {
        self.0.browser.upgrade().map(Browser)
    }

    fn set_state(&self, state: State) {
        self.0.state.replace(state);
        self.notify();
    }

    fn update(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.0.state.borrow_mut());
        self.notify();
    }

    fn notify(&self) {
        let state = self.0.state.borrow().clone();
        self.0.watchers.borrow_mut().retain(|show| show(&state));
    }

    fn schedule(&self, first_secs: u32) {
        self.cancel_timer();
        let weak = Rc::downgrade(&self.0);
        let first = glib::timeout_add_seconds_local_once(first_secs, move || {
            let Some(inner) = weak.upgrade() else { return };
            let weak = Rc::downgrade(&inner);
            let every = glib::timeout_add_seconds_local(TICK_SECS, move || match weak.upgrade() {
                Some(inner) => {
                    Syncer(inner).tick();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            });
            // The one-shot source is already gone, so its id is dropped, not removed.
            *inner.timer.borrow_mut() = Some(every);
            Syncer(inner).tick();
        });
        self.0.timer.replace(Some(first));
    }

    fn cancel_timer(&self) {
        if let Some(timer) = self.0.timer.take() {
            timer.remove();
        }
    }

    fn change_seq(&self) -> Option<Seq> {
        self.browser().map(|browser| browser.core().borrow().change_seq())
    }

    // Syncing.

    fn tick(&self) {
        let changed = self.change_seq() != self.0.start_seq.get();
        let since_start = self.0.last_start.get().map(|at| at.elapsed());
        if due(&self.0.state.borrow(), self.0.running.get(), changed, since_start) {
            self.sync_now();
        }
    }

    fn sync_now(&self) {
        if self.0.deleting.get() || !should_sync(&self.0.state.borrow(), self.0.running.get()) {
            return;
        }
        let start_seq = self.change_seq();
        self.0.start_seq.set(start_seq);
        self.0.last_start.set(Some(Instant::now()));
        self.0.running.set(true);
        self.update(|state| {
            if let State::SignedIn { syncing, .. } = state {
                *syncing = true;
            }
        });
        let syncer = self.clone();
        glib::spawn_future_local(async move {
            let mut synced_at = None;
            let result = syncer.rounds(&mut synced_at).await;
            match &result {
                Ok(()) => syncer.0.synced_seq.set(start_seq),
                Err(e) => log::warn!("sync: {e}"),
            }
            syncer.0.running.set(false);
            syncer.update(|state| settle(state, result, synced_at));
        });
    }

    /// Rounds until neither side has more, each one's changes shown as it lands. `synced_at` is
    /// when the last round completed.
    async fn rounds(&self, synced_at: &mut Option<u64>) -> Result<(), Error> {
        for _ in 0..MAX_ROUNDS {
            let Some(browser) = self.browser() else { return Err(Error::SignedOut) };
            let Some(round) = gather(&mut browser.core().borrow_mut())? else { return Err(Error::SignedOut) };
            drop(browser);
            let http = self.0.http.clone();
            let exchanged = on_worker(move || round.run(&http)).await;
            let Some(browser) = self.browser() else { return Err(Error::SignedOut) };
            let finished = exchanged.finish(&mut browser.core().borrow_mut().sync());
            match finished.result {
                Ok(synced) => {
                    *synced_at = finished.account.last_synced();
                    browser.sync_applied(&synced.report.changed);
                    if !synced.again {
                        return Ok(());
                    }
                }
                // Signed out while the round ran, and maybe in again: the next round is the new
                // account's, if there is one.
                Err(Error::SignedOut) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    // Signing in and out.

    fn sign_in(&self) {
        let Some(browser) = self.browser() else { return };
        let server = browser.core().borrow_mut().prefs().get(&keys::SYNC_SERVER);
        let attempt = self.0.attempt.get() + 1;
        self.0.attempt.set(attempt);
        self.set_state(State::SigningIn);
        let syncer = self.clone();
        glib::spawn_future_local(async move {
            let result = syncer.authorize(attempt, server).await;
            if syncer.0.attempt.get() != attempt {
                return;
            }
            syncer.0.canceller.take();
            let Some(browser) = syncer.browser() else { return };
            let saved = result.and_then(|account| {
                account.save(&mut browser.core().borrow_mut().sync())?;
                Ok(account)
            });
            match saved {
                Ok(account) => {
                    syncer.set_state(signed_in(&account, syncer.0.running.get(), false));
                    syncer.sync_now();
                    syncer.schedule(TICK_SECS);
                }
                Err(e) => {
                    log::warn!("sync sign-in: {e}");
                    let error = (!matches!(e, Error::Cancelled)).then(|| e.to_string());
                    let state = stored_state(&mut browser.core().borrow_mut(), error, true);
                    syncer.set_state(state);
                }
            }
        });
    }

    /// Opens the sync server's sign-in page in a new tab and waits for the user to come back from it.
    async fn authorize(&self, attempt: u64, server: String) -> Result<Account, Error> {
        let http = self.0.http.clone();
        let pending = on_worker(move || SignIn::start(&http, &server)).await?;
        let Some(browser) = self.browser().filter(|_| self.0.attempt.get() == attempt) else {
            return Err(Error::Cancelled);
        };
        let window = browser.windows().into_iter().next().unwrap_or_else(|| browser.open_window(&[]));
        window.open_tab(Some(pending.authorize_url()), None, Focus::Foreground);
        window.present();
        self.0.canceller.replace(Some(pending.canceller()));
        let http = self.0.http.clone();
        on_worker(move || pending.finish(&http)).await
    }

    fn cancel(&self) {
        self.0.attempt.set(self.0.attempt.get() + 1);
        if let Some(canceller) = self.0.canceller.take() {
            canceller.store(true, Ordering::Relaxed);
        }
        if let Some(browser) = self.browser() {
            let state = stored_state(&mut browser.core().borrow_mut(), None, true);
            self.set_state(state);
        }
    }

    /// A round still running is dropped by `Exchanged::finish`, which sees the account gone.
    fn sign_out(&self) {
        let Some(browser) = self.browser() else { return };
        let loaded = Account::load(&mut browser.core().borrow_mut().sync());
        if let Err(e) = loaded.and_then(|account| self.forget(&browser, account)) {
            log::warn!("sync sign-out: {e}");
        }
    }

    /// Signs out, and revokes `account`'s refresh token on a worker thread.
    fn forget(&self, browser: &Browser, account: Option<Account>) -> Result<(), Error> {
        Account::forget(&mut browser.core().borrow_mut().sync())?;
        self.cancel_timer();
        self.set_state(State::SignedOut { error: None });
        if let Some(account) = account {
            let http = self.0.http.clone();
            std::thread::spawn(move || account.revoke(&http));
        }
        Ok(())
    }

    /// Deletes everything the server holds for the account, then signs out. On failure the
    /// profile stays signed in.
    pub(crate) async fn delete_server_data(&self) -> Result<(), Error> {
        let browser = self.browser().ok_or(Error::SignedOut)?;
        let account = Account::load(&mut browser.core().borrow_mut().sync())?.ok_or(Error::SignedOut)?;
        drop(browser);
        self.0.deleting.set(true);
        let http = self.0.http.clone();
        let deleted = on_worker(move || account.delete_server_data(&http)).await;
        self.0.deleting.set(false);
        let account = deleted?;
        let browser = self.browser().ok_or(Error::SignedOut)?;
        self.forget(&browser, Some(account))
    }

    /// As the browser quits, after the session is saved: one round of what changed since the last
    /// sync, given at most [`FINAL_SYNC_WAIT`]. A round still out then is abandoned, which is safe:
    /// a round has nothing it must finish.
    pub(crate) fn final_sync(&self) {
        self.cancel_timer();
        let Some(browser) = self.browser() else { return };
        let changed = self.change_seq() != self.0.synced_seq.get();
        if self.0.deleting.get() || !changed || !should_sync(&self.0.state.borrow(), self.0.running.get()) {
            return;
        }
        let round = match gather(&mut browser.core().borrow_mut()) {
            Ok(Some(round)) => round,
            Ok(None) => return,
            Err(e) => {
                log::warn!("final sync: {e}");
                return;
            }
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let http = self.0.http.clone();
        std::thread::spawn(move || {
            let _ = sender.send(round.run(&http));
        });
        let Ok(exchanged) = receiver.recv_timeout(FINAL_SYNC_WAIT) else {
            log::info!("final sync: no answer in {FINAL_SYNC_WAIT:?}; quitting without it");
            return;
        };
        match exchanged.finish(&mut browser.core().borrow_mut().sync()).result {
            Ok(_) => log::info!("final sync: done"),
            Err(e) => log::info!("final sync: {e}"),
        }
    }
}

/// A round of the data types the user chose; `None` when signed out.
fn gather(profile: &mut Profile) -> Result<Option<Round>, Error> {
    let types = profile.prefs().get(&keys::SYNC_TYPES);
    let mut store = profile.sync();
    match Account::load(&mut store)? {
        Some(account) => Ok(Some(Round::gather(&mut store, account, &types)?)),
        None => Ok(None),
    }
}

/// A panic on the worker is a bug, raised here as it would be on the UI thread.
async fn on_worker<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    gio::spawn_blocking(work)
        .await
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

fn signed_in(account: &Account, syncing: bool, needs_sign_in: bool) -> State {
    State::SignedIn {
        name: account.name().map(str::to_owned),
        server: account.server().to_owned(),
        last_synced: account.last_synced(),
        syncing,
        error: None,
        needs_sign_in,
    }
}

/// The profile's account as Settings shows it while nothing runs: at startup, and after a
/// sign-in that did not finish (`after_sign_in`), which with an account still stored was signing
/// in again because that account's sign-in expired, as it still is. An account that cannot be
/// read, such as behind a locked keyring, shows as signed out with the reason.
fn stored_state(profile: &mut Profile, error: Option<String>, after_sign_in: bool) -> State {
    match Account::load(&mut profile.sync()) {
        Ok(Some(account)) => signed_in(&account, false, after_sign_in),
        Ok(None) => State::SignedOut { error },
        Err(e) => {
            log::warn!("sync account: {e}");
            State::SignedOut { error: Some(e.to_string()) }
        }
    }
}

fn should_sync(state: &State, running: bool) -> bool {
    !running && matches!(state, State::SignedIn { needs_sign_in: false, syncing: false, .. })
}

/// Whether a tick starts a sync: the first of this run, a minute after the last one started, or
/// soon after a local change, but never sooner than [`SYNC_SOON`] after the last one started.
fn due(state: &State, running: bool, changed: bool, since_start: Option<Duration>) -> bool {
    should_sync(state, running)
        && since_start.is_none_or(|since| since >= SYNC_INTERVAL || (changed && since >= SYNC_SOON))
}

/// The state once a sync has ended with `result`. A profile that signed out meanwhile keeps the
/// state the sign-out gave it.
fn settle(state: &mut State, result: Result<(), Error>, synced_at: Option<u64>) {
    let State::SignedIn { last_synced, syncing, error, needs_sign_in, .. } = state else { return };
    *syncing = false;
    if synced_at.is_some() {
        *last_synced = synced_at;
    }
    match result {
        Ok(()) => *error = None,
        Err(Error::SignedOut) => {}
        Err(e) if e.needs_sign_in() => *needs_sign_in = true,
        Err(e) => *error = Some(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(last_synced: u64, syncing: bool, error: Option<&str>, needs_sign_in: bool) -> State {
        State::SignedIn {
            name: None,
            server: "https://sync.example".to_owned(),
            last_synced: Some(last_synced),
            syncing,
            error: error.map(str::to_owned),
            needs_sign_in,
        }
    }

    const FAILED: Option<&str> = Some("could not reach sync.example");

    fn signed_in_state(syncing: bool, needs_sign_in: bool) -> State {
        with(100, syncing, FAILED, needs_sign_in)
    }

    #[test]
    fn only_an_idle_account_that_can_sync_syncs() {
        assert!(should_sync(&signed_in_state(false, false), false));
        assert!(!should_sync(&signed_in_state(false, false), true), "one sync at a time");
        assert!(!should_sync(&signed_in_state(true, false), false));
        assert!(!should_sync(&signed_in_state(false, true), false), "an expired sign-in waits for the user");
        assert!(!should_sync(&State::SignedOut { error: None }, false));
        assert!(!should_sync(&State::SigningIn, false));
    }

    #[test]
    fn a_change_syncs_soon_and_no_change_waits_a_minute() {
        let idle = signed_in_state(false, false);
        let secs = |s| Some(Duration::from_secs(s));
        assert!(due(&idle, false, false, None), "the first tick of a run syncs");
        assert!(!due(&idle, false, true, secs(9)), "not sooner than 10 s after the last start");
        assert!(due(&idle, false, true, secs(10)));
        assert!(!due(&idle, false, false, secs(59)), "nothing changed: wait for the minute");
        assert!(due(&idle, false, false, secs(60)));
        assert!(!due(&idle, true, true, secs(120)), "one sync at a time");
        assert!(!due(&signed_in_state(false, true), false, true, secs(120)), "an expired sign-in waits for the user");
        assert!(!due(&State::SignedOut { error: None }, false, true, None));
    }

    #[test]
    fn a_finished_sync_clears_the_error_and_records_when() {
        let mut state = signed_in_state(true, false);
        settle(&mut state, Ok(()), Some(200));
        assert_eq!(state, with(200, false, None, false));
    }

    #[test]
    fn a_failed_sync_says_why_and_keeps_what_the_rounds_before_it_did() {
        let mut state = signed_in_state(true, false);
        settle(&mut state, Err(Error::Network("sync.example".to_owned())), Some(150));
        assert_eq!(state, with(150, false, FAILED, false));
        let mut state = signed_in_state(true, false);
        settle(&mut state, Err(Error::Network("other.example".to_owned())), None);
        let State::SignedIn { last_synced, error, .. } = state else { unreachable!() };
        assert_eq!((last_synced, error.as_deref()), (Some(100), Some("could not reach other.example")));
    }

    #[test]
    fn an_expired_sign_in_asks_for_another() {
        let mut state = signed_in_state(true, false);
        settle(&mut state, Err(Error::SignInExpired), None);
        assert_eq!(state, signed_in_state(false, true));
    }

    #[test]
    fn a_sign_out_during_a_sync_stays_signed_out() {
        let mut state = State::SignedOut { error: None };
        settle(&mut state, Err(Error::SignedOut), Some(300));
        assert_eq!(state, State::SignedOut { error: None });
        let mut state = signed_in_state(true, false);
        settle(&mut state, Err(Error::SignedOut), None);
        assert_eq!(state, signed_in_state(false, false), "a new sign-in keeps what it shows");
    }
}
