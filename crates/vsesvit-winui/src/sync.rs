//! Sync with a Vsesvit sync server (`vsesvit-sync`): signing in, the rounds that move this
//! profile's records, their schedule, and the state Settings shows. The network steps run on
//! worker threads; the profile is read and written here, on the UI thread, between them.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use vsesvit_core::Profile;
use vsesvit_core::crdt::Seq;
use vsesvit_core::extensions::toolbar;
use vsesvit_core::prefs::keys;
use vsesvit_core::sync::{Changed, DataType};
use vsesvit_sync::status::{State, Status};
use vsesvit_sync::{
    Account, Encryption, Error, Http, Passphrase, PassphraseJob, Round, SignIn, now_secs,
};

use crate::browser::Browser;
use crate::exec;
use crate::window::BrowserWindow;

/// The first sync waits this long after the first window shows.
const FIRST_SYNC: Duration = Duration::from_secs(10);
/// How often the schedule looks for local changes.
const TICK: Duration = Duration::from_secs(2);
/// A local change syncs no sooner than this after the last sync started, so a burst of edits
/// is one sync.
const CHANGE_GAP: Duration = Duration::from_secs(10);
/// Without local changes, a sync this often downloads what other devices sent.
const SYNC_INTERVAL: Duration = Duration::from_secs(60);
/// Quitting waits at most this long for the final sync.
const FINAL_SYNC_WAIT: Duration = Duration::from_secs(3);

/// What an open dialog runs after a sync applied records.
pub(crate) type Applied = dyn Fn(&Changed);

/// The app's sync: where it stands, and what running it needs.
pub(crate) struct SyncController {
    state: RefCell<State>,
    http: Http,
    /// A sync is running. It can outlive the sign-in it started for, so it is not `State`'s
    /// `syncing`.
    running: Cell<bool>,
    /// Counts sign-ins begun and cancelled. A sign-in's steps go on only while it is the latest.
    attempts: Cell<u64>,
    canceller: RefCell<Option<Arc<AtomicBool>>>,
    /// When the last sync started, and the profile's `change_seq` then.
    last_start: Cell<Option<(Instant, Seq)>>,
    /// `change_seq` when the last sync that completed started: nothing is left to upload while
    /// it has not moved.
    uploaded: Cell<Option<Seq>>,
    /// Called after every change of `state`, while their owners (an open Settings) keep them.
    listeners: RefCell<Vec<Weak<dyn Fn()>>>,
    /// Called after a sync applied records, while their owners (an open dialog) keep them.
    applied: RefCell<Vec<Weak<Applied>>>,
}

impl SyncController {
    /// Signed in when the profile holds an account. One it holds but cannot read (its sealed
    /// tokens do not unseal) shows as signed out, with why.
    pub fn load(profile: &mut Profile) -> Self {
        let signed_out = State::SignedOut { error: None };
        let state = match Account::load(&mut profile.sync()) {
            Ok(Some(account)) => next(signed_out, signed_in(&account)),
            Ok(None) => signed_out,
            Err(e) => {
                log::warn!("sync account: {e}");
                State::SignedOut {
                    error: Some(e.to_string()),
                }
            }
        };
        Self {
            state: RefCell::new(state),
            http: Http::new(),
            running: Cell::new(false),
            attempts: Cell::new(0),
            canceller: RefCell::new(None),
            last_start: Cell::new(None),
            uploaded: Cell::new(None),
            listeners: RefCell::default(),
            applied: RefCell::default(),
        }
    }

    pub fn status(&self) -> Status {
        self.state.borrow().status(now_secs())
    }

    pub fn state(&self) -> State {
        self.state.borrow().clone()
    }

    pub fn signed_in(&self) -> bool {
        matches!(*self.state.borrow(), State::SignedIn { .. })
    }

    /// Calls `listener` after every change of the sync state, while the caller keeps it.
    pub fn on_change(&self, listener: &Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(Rc::downgrade(listener));
    }

    /// Calls `listener` with what each sync applied, while the caller keeps it.
    pub fn on_applied(&self, listener: &Rc<Applied>) {
        self.applied.borrow_mut().push(Rc::downgrade(listener));
    }

    /// Tells the dialogs that follow sync what it changed (`Browser::sync_applied`).
    pub fn applied(&self, changed: &Changed) {
        for listener in live(&self.applied) {
            listener(changed);
        }
    }

    fn apply(&self, event: Event) {
        self.state
            .replace_with(|state| next(std::mem::replace(state, State::SigningIn), event));
        for listener in live(&self.listeners) {
            listener();
        }
    }
}

/// The listeners still kept, collected first so one can add a listener.
pub(crate) fn live<T: ?Sized>(listeners: &RefCell<Vec<Weak<T>>>) -> Vec<Rc<T>> {
    let mut listeners = listeners.borrow_mut();
    listeners.retain(|l| l.strong_count() > 0);
    listeners.iter().filter_map(Weak::upgrade).collect()
}

/// What happens to sync, as its state follows it.
#[derive(Debug)]
enum Event {
    Sync(Progress),
    SigningIn,
    SignedIn(Signed),
    SignInFailed {
        /// Why; `None` when the user cancelled it.
        error: Option<String>,
        /// The account still stored, which was signing in again because its sign-in expired,
        /// as it still is.
        stored: Option<Signed>,
    },
    SignedOut,
    /// What the stored account asks of the passphrase now, after a round or a passphrase step.
    Encryption(Encryption),
}

/// What Settings shows of the account a profile is signed in with.
#[derive(Debug)]
struct Signed {
    name: Option<String>,
    server: String,
    last_synced: Option<u64>,
    encryption: Encryption,
}

/// Where a sync of the signed-in account is.
#[derive(Debug)]
enum Progress {
    Started,
    /// When the last round completed.
    Synced(Option<u64>),
    Failed(String),
    SignInExpired,
}

fn next(state: State, event: Event) -> State {
    match event {
        Event::SigningIn => State::SigningIn,
        Event::SignedIn(Signed {
            name,
            server,
            last_synced,
            encryption,
        }) => State::SignedIn {
            name,
            server,
            last_synced,
            syncing: false,
            error: None,
            needs_sign_in: false,
            encryption,
        },
        Event::SignedOut => State::SignedOut { error: None },
        Event::SignInFailed { error, stored } => match (state, stored) {
            (
                State::SigningIn,
                Some(Signed {
                    name,
                    server,
                    last_synced,
                    encryption,
                }),
            ) => State::SignedIn {
                name,
                server,
                last_synced,
                syncing: false,
                error,
                needs_sign_in: true,
                encryption,
            },
            (State::SigningIn, None) => State::SignedOut { error },
            (state, _) => state,
        },
        Event::Sync(progress) => {
            let State::SignedIn {
                name,
                server,
                last_synced,
                error,
                needs_sign_in,
                encryption,
                ..
            } = state
            else {
                return state;
            };
            let (syncing, last_synced, error, needs_sign_in) = match progress {
                Progress::Started => (true, last_synced, error, needs_sign_in),
                Progress::Synced(at) => (false, at.or(last_synced), None, false),
                Progress::Failed(error) => (false, last_synced, Some(error), false),
                Progress::SignInExpired => (false, last_synced, None, true),
            };
            State::SignedIn {
                name,
                server,
                last_synced,
                syncing,
                error,
                needs_sign_in,
                encryption,
            }
        }
        Event::Encryption(encryption) => match state {
            State::SignedIn {
                name,
                server,
                last_synced,
                syncing,
                error,
                needs_sign_in,
                ..
            } => State::SignedIn {
                name,
                server,
                last_synced,
                syncing,
                error,
                needs_sign_in,
                encryption,
            },
            state => state,
        },
    }
}

fn signed(account: &Account) -> Signed {
    Signed {
        name: account.name().map(str::to_owned),
        server: account.server().to_owned(),
        last_synced: account.last_synced(),
        encryption: account.encryption(),
    }
}

fn signed_in(account: &Account) -> Event {
    Event::SignedIn(signed(account))
}

/// Whether a scheduled sync or Sync Now runs: signed in with a sign-in the provider accepts.
fn due(state: &State) -> bool {
    matches!(
        state,
        State::SignedIn {
            needs_sign_in: false,
            ..
        }
    )
}

/// How a sync ended, as an event; `None` when the profile signed out (or in again) meanwhile,
/// which leaves the state to whatever did that.
fn outcome(result: Result<Option<u64>, Error>) -> Option<Event> {
    match result {
        Ok(last_synced) => Some(Event::Sync(Progress::Synced(last_synced))),
        Err(Error::SignedOut) => None,
        Err(e) if e.needs_sign_in() => Some(Event::Sync(Progress::SignInExpired)),
        Err(e) => Some(Event::Sync(Progress::Failed(e.to_string()))),
    }
}

impl From<exec::WorkerLost> for Error {
    fn from(lost: exec::WorkerLost) -> Self {
        Error::Profile(std::io::Error::from(lost).into())
    }
}

/// A sign-in that failed with `e`, while the profile still stores `stored`.
fn sign_in_failed(e: &Error, stored: Option<&Account>) -> Event {
    Event::SignInFailed {
        error: match e {
            Error::Cancelled => None,
            e => Some(e.to_string()),
        },
        stored: stored.map(signed),
    }
}

/// The account the profile stores; `None` when it stores none, or one it cannot read.
fn stored_account(browser: &Browser) -> Option<Account> {
    browser
        .core(|p| Account::load(&mut p.sync()))
        .unwrap_or_else(|e| {
            log::warn!("sync account: {e}");
            None
        })
}

/// Whether a scheduled tick syncs: while signed in and not syncing, `SYNC_INTERVAL` after the
/// last sync started, or `CHANGE_GAP` after it when something changed here since. The first
/// tick syncs.
fn tick_syncs(due: bool, running: bool, changed: bool, since_start: Option<Duration>) -> bool {
    if !due || running {
        return false;
    }
    since_start.is_none_or(|since| since >= SYNC_INTERVAL || (changed && since >= CHANGE_GAP))
}

/// Syncs `FIRST_SYNC` after the first window shows, then as `tick_syncs` decides every `TICK`.
pub(crate) async fn schedule(browser: Weak<Browser>) {
    exec::sleep(FIRST_SYNC).await;
    loop {
        let Some(b) = browser.upgrade() else { return };
        let sync = b.sync();
        let last_start = sync.last_start.get();
        let seq = b.core(|p| p.change_seq());
        if tick_syncs(
            due(&sync.state.borrow()),
            sync.running.get(),
            last_start.is_none_or(|(_, at)| at != seq),
            last_start.map(|(at, _)| at.elapsed()),
        ) {
            sync_now(&b);
        }
        drop(b);
        exec::sleep(TICK).await;
    }
}

/// Syncs, unless signed out, waiting for a new sign-in, or syncing already.
pub(crate) fn sync_now(browser: &Rc<Browser>) {
    let sync = browser.sync();
    if sync.running.get() || !due(&sync.state.borrow()) {
        return;
    }
    sync.running.set(true);
    let seq = browser.core(|p| p.change_seq());
    sync.last_start.set(Some((Instant::now(), seq)));
    sync.apply(Event::Sync(Progress::Started));
    exec::spawn(run(Rc::downgrade(browser), seq));
}

async fn run(browser: Weak<Browser>, seq: Seq) {
    let result = rounds(&browser).await;
    let Some(b) = browser.upgrade() else { return };
    b.sync().running.set(false);
    if result.is_ok() {
        b.sync().uploaded.set(Some(seq));
    }
    if let Err(e) = &result {
        log::warn!("sync: {e}");
    }
    match outcome(result) {
        Some(event) => b.sync().apply(event),
        // Signed in again while this sync ran for the old account: the new one has not synced.
        None => sync_now(&b),
    }
}

/// Rounds until neither side has more, at most `vsesvit_sync::MAX_ROUNDS`. `Ok` with when the
/// last one completed.
async fn rounds(browser: &Weak<Browser>) -> Result<Option<u64>, Error> {
    let b = browser.upgrade().ok_or(Error::SignedOut)?;
    let mut account = b
        .core(|p| Account::load(&mut p.sync()))?
        .ok_or(Error::SignedOut)?;
    drop(b);
    for n in 1.. {
        let (round, http) = {
            let b = browser.upgrade().ok_or(Error::SignedOut)?;
            let round = b.core(|p| {
                let types = p.prefs().get(&keys::SYNC_TYPES);
                Round::gather(&mut p.sync(), account, &types)
            })?;
            (round, b.sync().http.clone())
        };
        let exchanged = exec::background(move || round.run(&http)).await?;
        let b = browser.upgrade().ok_or(Error::SignedOut)?;
        let (site_settings, finished) = b.core(|p| {
            let site_settings = p.site_permissions().all();
            (site_settings, exchanged.finish(&mut p.sync()))
        });
        account = finished.account;
        // A round thrown away for a sign-out says nothing of the account signed in now.
        if !matches!(finished.result, Err(Error::SignedOut)) {
            b.sync().apply(Event::Encryption(account.encryption()));
        }
        let synced = finished.result?;
        b.sync_applied(&synced.report.changed, &site_settings);
        // A refused upload waits for a later sync; it is what this one comes to.
        if let Some(outcome) = synced.outcome(n) {
            return outcome.map(|()| account.last_synced());
        }
    }
    unreachable!("the sync ends by its last round")
}

/// Signs in with the server the `sync.server` preference names: the provider's page opens in a
/// new tab of `window` (or of the newest window), `opened` runs, and once the user finishes
/// there the profile is signed in and syncs. A saved account is replaced once the sign-in
/// finishes: Sign In meets one only when the provider no longer accepts it, and a sign-in that
/// fails leaves it asking for another.
pub(crate) fn sign_in(
    browser: &Rc<Browser>,
    window: Weak<BrowserWindow>,
    opened: impl FnOnce() + 'static,
) {
    let sync = browser.sync();
    if *sync.state.borrow() == State::SigningIn {
        return;
    }
    let attempt = sync.attempts.get() + 1;
    sync.attempts.set(attempt);
    sync.apply(Event::SigningIn);
    let server = browser.core(|p| p.prefs().get(&keys::SYNC_SERVER));
    let http = sync.http.clone();
    let browser = Rc::downgrade(browser);
    exec::spawn(async move {
        let h = http.clone();
        let started = exec::background(move || SignIn::start(&h, &server))
            .await
            .unwrap_or_else(|lost| Err(lost.into()));
        let Some(b) = latest(&browser, attempt) else {
            return;
        };
        let pending = match started {
            Ok(pending) => pending,
            Err(e) => {
                log::warn!("sync sign-in: {e}");
                b.sync()
                    .apply(sign_in_failed(&e, stored_account(&b).as_ref()));
                return;
            }
        };
        *b.sync().canceller.borrow_mut() = Some(pending.canceller());
        match window.upgrade().or_else(|| b.windows().pop()) {
            Some(window) => {
                if let Err(e) = window.open_url_tab(pending.authorize_url(), true) {
                    log::warn!("opening the sign-in page: {e}");
                }
            }
            None => log::warn!("no window for the sign-in page"),
        }
        opened();
        drop(b);
        let finished = exec::background(move || pending.finish(&http))
            .await
            .unwrap_or_else(|lost| Err(lost.into()));
        let Some(b) = latest(&browser, attempt) else {
            return;
        };
        b.sync().canceller.take();
        let saved = finished.and_then(|account| {
            b.core(|p| account.save_signed_in(&mut p.sync()))?;
            Ok(account)
        });
        match saved {
            Ok(account) => {
                log::info!("sync: signed in to {}", account.server());
                b.sync().apply(signed_in(&account));
                sync_now(&b);
            }
            Err(e) => {
                log::warn!("sync sign-in: {e}");
                b.sync()
                    .apply(sign_in_failed(&e, stored_account(&b).as_ref()));
            }
        }
    });
}

/// The browser, while `attempt` is the latest sign-in.
fn latest(browser: &Weak<Browser>, attempt: u64) -> Option<Rc<Browser>> {
    browser
        .upgrade()
        .filter(|b| b.sync().attempts.get() == attempt)
}

/// Stops waiting for the provider's page; what the sign-in was doing is dropped.
pub(crate) fn cancel_sign_in(browser: &Browser) {
    let sync = browser.sync();
    sync.attempts.set(sync.attempts.get() + 1);
    if let Some(canceller) = sync.canceller.take() {
        canceller.store(true, Ordering::Relaxed);
    }
    sync.apply(sign_in_failed(
        &Error::Cancelled,
        stored_account(browser).as_ref(),
    ));
}

/// Signs the profile out; its records stay here and on the server. A sync running meanwhile
/// changes nothing when it ends (`Exchanged::finish` checks the sign-in).
pub(crate) fn sign_out(browser: &Browser) {
    let account = browser.core(|p| {
        let mut store = p.sync();
        let account = Account::load(&mut store).ok().flatten();
        Account::forget(&mut store).map(|()| account)
    });
    let account = match account {
        Ok(account) => account,
        Err(e) => {
            log::warn!("signing out of sync: {e}");
            return;
        }
    };
    browser.sync().apply(Event::SignedOut);
    if let Some(account) = account {
        let http = browser.sync().http.clone();
        exec::spawn(async move {
            if let Err(e) = exec::background(move || account.revoke(&http)).await {
                log::warn!("revoking the sync session: {e}");
            }
        });
    }
}

/// As the browser quits, after the session is saved: one round of what changed since the last
/// sync, if anything did, which quitting waits `FINAL_SYNC_WAIT` for at most. A round that has
/// not come back by then is dropped; it never refreshes tokens, so dropping it loses nothing.
pub(crate) fn final_sync(browser: &Browser) {
    let sync = browser.sync();
    let seq = browser.core(|p| p.change_seq());
    if sync.running.get() || !due(&sync.state.borrow()) || sync.uploaded.get() == Some(seq) {
        return;
    }
    let gathered = browser.core(|p| {
        let account = Account::load(&mut p.sync())?.ok_or(Error::SignedOut)?;
        let types = p.prefs().get(&keys::SYNC_TYPES);
        Round::gather(&mut p.sync(), account, &types)
    });
    let round = match gathered {
        Ok(round) => round,
        Err(e) => {
            log::warn!("final sync: {e}");
            return;
        }
    };
    let (send, receive) = std::sync::mpsc::channel();
    let http = sync.http.clone();
    let spawned = std::thread::Builder::new()
        .name("vsesvit-final-sync".into())
        .spawn(move || {
            let _ = send.send(round.run(&http));
        });
    if let Err(e) = spawned {
        log::warn!("final sync: {e}");
        return;
    }
    let Ok(exchanged) = receive.recv_timeout(FINAL_SYNC_WAIT) else {
        log::info!("final sync: no answer in {FINAL_SYNC_WAIT:?}; quitting without it");
        return;
    };
    let result = browser.core(|p| exchanged.finish(&mut p.sync())).result;
    match result.and_then(|synced| synced.refused.map_or(Ok(()), Err)) {
        Ok(()) => log::info!("final sync: done"),
        Err(e) => log::info!("final sync: {e}"),
    }
}

/// Stores which types this device syncs, and syncs with them.
pub(crate) fn set_types(browser: &Rc<Browser>, types: &[DataType]) {
    browser.write_pref(&keys::SYNC_TYPES, &types.to_vec());
    sync_now(browser);
}

/// Deletes what the server holds for the account, then signs out as [`sign_out`] does. `done`
/// gets why it failed, if it did; the profile then stays signed in.
pub(crate) fn delete_server_data(
    browser: &Rc<Browser>,
    done: impl FnOnce(Option<String>) + 'static,
) {
    let browser = Rc::downgrade(browser);
    exec::spawn(async move {
        // A round running meanwhile could upload again what the deletion removes.
        let idle = exec::wait_for(Duration::from_secs(120), Duration::from_millis(100), || {
            let b = browser.upgrade()?;
            (!b.sync().running.replace(true)).then_some(())
        })
        .await;
        let Some(b) = browser.upgrade() else { return };
        if idle.is_none() {
            done(Some("a sync is still running; try again".to_owned()));
            return;
        }
        let account = b.core(|p| Account::load(&mut p.sync()));
        let http = b.sync().http.clone();
        drop(b);
        let deleted = match account {
            Ok(Some(account)) => exec::background(move || account.delete_server_data(&http))
                .await
                .unwrap_or_else(|lost| Err(lost.into())),
            Ok(None) => Err(Error::SignedOut),
            Err(e) => Err(e),
        };
        let Some(b) = browser.upgrade() else { return };
        b.sync().running.set(false);
        match deleted {
            Ok(account) => {
                log::info!("sync: deleted the data on {}", account.server());
                sign_out(&b);
                done(None);
            }
            Err(e) => {
                log::warn!("deleting the data on the sync server: {e}");
                done(Some(e.to_string()));
            }
        }
    });
}

/// Sets, enters or changes the sync passphrase, as `asked` (the encryption the dialog was for)
/// says and the stored account still asks, then syncs. A round running meanwhile would be thrown
/// away, so this waits for a running sync to end and holds syncs off until it is done. `done`
/// gets why it failed, if it did.
pub(crate) fn set_passphrase(
    browser: &Rc<Browser>,
    asked: Encryption,
    passphrase: Passphrase,
    done: impl FnOnce(Option<String>) + 'static,
) {
    let browser = Rc::downgrade(browser);
    exec::spawn(async move {
        let idle = exec::wait_for(Duration::from_secs(120), Duration::from_millis(100), || {
            let b = browser.upgrade()?;
            (!b.sync().running.replace(true)).then_some(())
        })
        .await;
        let Some(b) = browser.upgrade() else { return };
        if idle.is_none() {
            done(Some("a sync is still running; try again".to_owned()));
            return;
        }
        let job = match b.core(|p| Account::load(&mut p.sync())) {
            Ok(Some(account)) if account.encryption() == asked => {
                PassphraseJob::new(account, passphrase).ok_or(Error::KeysChanged)
            }
            Ok(Some(_)) => Err(Error::KeysChanged),
            Ok(None) => Err(Error::SignedOut),
            Err(e) => Err(e),
        };
        drop(b);
        let stored = match job {
            Ok(job) => match exec::background(move || job.run()).await {
                Ok(keys) => match browser.upgrade() {
                    Some(b) => b.core(|p| keys.finish(&mut p.sync())),
                    None => return,
                },
                Err(lost) => Err(lost.into()),
            },
            Err(e) => Err(e),
        };
        let Some(b) = browser.upgrade() else { return };
        b.sync().running.set(false);
        match stored {
            Ok(account) => {
                log::info!("sync: took the passphrase");
                done(None);
                b.sync().apply(Event::Encryption(account.encryption()));
                sync_now(&b);
            }
            Err(e) => {
                log::warn!("sync passphrase: {e}");
                done(Some(e.to_string()));
            }
        }
    });
}

/// The sync state again from the account the profile stores, after a scripted run stored one.
#[cfg(feature = "self-test")]
pub(crate) fn reload(browser: &Browser) {
    browser.sync().apply(match stored_account(browser) {
        Some(account) => signed_in(&account),
        None => Event::SignedOut,
    });
}

/// What the shell applies again after a sync changed a synced preference it holds or shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrefEffect {
    /// Theme, tab list placement, bars, buttons and the media player (`WindowPrefs`).
    Window,
    Keymap,
    Autofill,
    ExtensionToolbar,
    /// Third-party cookies, applied in every tab (`cookies::changed`).
    Cookies,
}

/// The synced preferences with an effect. The others are read where they are used: the home
/// page, startup, the default search engine and suggestions, pop-ups, downloads, and the
/// engine's startup switches.
const PREF_EFFECTS: [(&str, PrefEffect); 12] = [
    (keys::THEME.key, PrefEffect::Window),
    (keys::TABS_POSITION.key, PrefEffect::Window),
    (keys::SHOW_BOOKMARKS_BAR.key, PrefEffect::Window),
    (keys::SHOW_HOME_BUTTON.key, PrefEffect::Window),
    (keys::COMPACT_ADDRESS_BAR.key, PrefEffect::Window),
    (keys::SHOW_FULL_URLS.key, PrefEffect::Window),
    (keys::SHOW_MEDIA_PLAYER.key, PrefEffect::Window),
    (keys::PICTURE_IN_PICTURE.key, PrefEffect::Window),
    (keys::SHORTCUTS.key, PrefEffect::Keymap),
    (keys::AUTOFILL_FORMS.key, PrefEffect::Autofill),
    (toolbar::TOOLBAR.key, PrefEffect::ExtensionToolbar),
    (keys::THIRD_PARTY_COOKIES.key, PrefEffect::Cookies),
];

/// Each effect of the changed preferences `keys`, once.
pub(crate) fn pref_effects(keys: &[String]) -> Vec<PrefEffect> {
    let mut effects = Vec::new();
    for (key, effect) in PREF_EFFECTS {
        if !effects.contains(&effect) && keys.iter().any(|k| k == key) {
            effects.push(effect);
        }
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use vsesvit_sync::status::Action;

    fn signed_in_state() -> State {
        next(
            State::SignedOut { error: None },
            Event::SignedIn(Signed {
                name: Some("Demir".into()),
                server: "https://sync.example.com".into(),
                last_synced: Some(100),
                encryption: Encryption::Ready,
            }),
        )
    }

    fn fields(state: &State) -> (Option<u64>, bool, Option<&str>, bool) {
        match state {
            State::SignedIn {
                last_synced,
                syncing,
                error,
                needs_sign_in,
                ..
            } => (*last_synced, *syncing, error.as_deref(), *needs_sign_in),
            other => panic!("not signed in: {other:?}"),
        }
    }

    fn signed_in_event() -> Event {
        Event::SignedIn(Signed {
            name: None,
            server: "https://sync.example.com".into(),
            last_synced: None,
            encryption: Encryption::Checking,
        })
    }

    #[test]
    fn a_sync_shows_while_it_runs_then_when_it_completed_or_why_it_failed() {
        let syncing = next(signed_in_state(), Event::Sync(Progress::Started));
        assert_eq!(fields(&syncing), (Some(100), true, None, false));
        let failed = next(
            syncing,
            Event::Sync(Progress::Failed("could not reach x".into())),
        );
        assert_eq!(
            fields(&failed),
            (Some(100), false, Some("could not reach x"), false)
        );
        let again = next(failed, Event::Sync(Progress::Started));
        assert_eq!(
            fields(&again),
            (Some(100), true, Some("could not reach x"), false)
        );
        let synced = next(again, Event::Sync(Progress::Synced(Some(200))));
        assert_eq!(fields(&synced), (Some(200), false, None, false));
        assert!(due(&synced));
    }

    #[test]
    fn an_expired_sign_in_stops_syncs_until_the_next_sign_in() {
        let expired = next(signed_in_state(), Event::Sync(Progress::SignInExpired));
        assert_eq!(fields(&expired), (Some(100), false, None, true));
        assert!(!due(&expired));
        assert!(!due(&State::SignedOut { error: None }));
        assert!(!due(&State::SigningIn));
        let signing_in = next(expired, Event::SigningIn);
        assert!(due(&next(signing_in, signed_in_event())));
    }

    #[test]
    fn a_failed_or_cancelled_sign_in_is_signed_out() {
        let failed = next(State::SigningIn, sign_in_failed(&Error::TimedOut, None));
        let error = Some("the sign-in took too long".to_owned());
        assert_eq!(failed, State::SignedOut { error });
        let cancelled = next(State::SigningIn, sign_in_failed(&Error::Cancelled, None));
        assert_eq!(cancelled, State::SignedOut { error: None });
        let signed_in = next(State::SigningIn, signed_in_event());
        let late = next(signed_in.clone(), sign_in_failed(&Error::Cancelled, None));
        assert_eq!(late, signed_in);
    }

    #[test]
    fn a_failed_or_cancelled_sign_in_again_keeps_the_stored_account_asking_for_another() {
        let account: Account = serde_json::from_value(serde_json::json!({
            "sign_in": "s", "server": "https://sync.example.com", "name": "Demir",
            "limits": { "max_batch": 1, "max_record_bytes": 1, "max_request_bytes": 1 },
            "download_cursor": 0, "upload_cursors": {}, "last_synced": 100,
        }))
        .expect("an account");
        let failed = next(
            State::SigningIn,
            sign_in_failed(&Error::TimedOut, Some(&account)),
        );
        assert_eq!(
            fields(&failed),
            (Some(100), false, Some("the sign-in took too long"), true)
        );
        assert_eq!(
            failed.status(100).subtitle,
            "Sign-in failed: the sign-in took too long"
        );
        assert!(
            !due(&failed),
            "the expired sign-in still waits for the user"
        );
        let cancelled = next(
            State::SigningIn,
            sign_in_failed(&Error::Cancelled, Some(&account)),
        );
        assert_eq!(fields(&cancelled), (Some(100), false, None, true));
        assert!(
            matches!(
                cancelled,
                State::SignedIn {
                    encryption: Encryption::Checking,
                    ..
                }
            ),
            "the stored account has not looked for the account's key record"
        );
        assert_eq!(
            cancelled.status(100).subtitle,
            "Sign in again to keep syncing."
        );
    }

    #[test]
    fn rounds_and_the_passphrase_step_change_only_what_the_device_asks_for() {
        let checking = next(State::SigningIn, signed_in_event());
        assert_eq!(
            checking.status(0).actions,
            [Action::SignOut, Action::DeleteServerData]
        );
        let syncing = next(checking, Event::Sync(Progress::Started));
        let set = next(syncing, Event::Encryption(Encryption::Set));
        assert_eq!(fields(&set), (None, true, None, false));
        assert_eq!(set.status(0).actions[0], Action::SetPassphrase);
        let ready = next(set, Event::Encryption(Encryption::Ready));
        let synced = next(ready, Event::Sync(Progress::Synced(Some(200))));
        assert_eq!(fields(&synced), (Some(200), false, None, false));
        assert_eq!(
            synced.status(200).actions,
            [
                Action::SyncNow,
                Action::ChangePassphrase,
                Action::SignOut,
                Action::DeleteServerData
            ]
        );
        let signed_out = State::SignedOut { error: None };
        assert_eq!(
            next(signed_out.clone(), Event::Encryption(Encryption::Ready)),
            signed_out
        );
    }

    #[test]
    fn a_sync_ended_by_a_sign_out_leaves_the_state_alone() {
        assert!(outcome(Err(Error::SignedOut)).is_none());
        let signed_out = State::SignedOut { error: None };
        let late = next(signed_out.clone(), Event::Sync(Progress::Synced(Some(1))));
        assert_eq!(late, signed_out);
        assert!(matches!(
            outcome(Err(Error::SignInExpired)),
            Some(Event::Sync(Progress::SignInExpired))
        ));
        assert!(matches!(
            outcome(Err(Error::Network("x".into()))),
            Some(Event::Sync(Progress::Failed(e))) if e == "could not reach x"
        ));
        assert!(matches!(
            outcome(Ok(Some(5))),
            Some(Event::Sync(Progress::Synced(Some(5))))
        ));
    }

    #[test]
    fn a_tick_syncs_soon_after_a_change_and_now_and_then_without_one() {
        let s = Duration::from_secs;
        assert!(tick_syncs(true, false, false, None), "the first tick");
        assert!(
            !tick_syncs(true, false, true, Some(s(4))),
            "too soon after the last start"
        );
        assert!(tick_syncs(true, false, true, Some(s(10))));
        assert!(
            !tick_syncs(true, false, false, Some(s(30))),
            "nothing changed"
        );
        assert!(tick_syncs(true, false, false, Some(s(60))), "downloads");
        assert!(
            !tick_syncs(true, true, true, Some(s(90))),
            "one sync at a time"
        );
        assert!(
            !tick_syncs(false, false, true, Some(s(90))),
            "signed out, or waiting for a new sign-in"
        );
    }

    #[test]
    fn each_changed_preference_maps_to_its_effect_once() {
        let keys = |keys: &[&str]| keys.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            pref_effects(&keys(&["tabs.position", "theme", "homepage"])),
            [PrefEffect::Window]
        );
        assert_eq!(
            pref_effects(&keys(&["media.player", "media.picture_in_picture"])),
            [PrefEffect::Window]
        );
        assert_eq!(
            pref_effects(&keys(&[
                "toolbar.extensions",
                "keyboard.shortcuts",
                "autofill.forms"
            ])),
            [
                PrefEffect::Keymap,
                PrefEffect::Autofill,
                PrefEffect::ExtensionToolbar
            ]
        );
        assert_eq!(
            pref_effects(&keys(&["privacy.third_party_cookies"])),
            [PrefEffect::Cookies]
        );
        assert!(pref_effects(&keys(&["startup", "search.default"])).is_empty());
    }

    #[test]
    fn only_kept_listeners_are_called_and_one_can_add_a_listener() {
        let listeners = Rc::new(RefCell::new(Vec::<Weak<dyn Fn()>>::new()));
        let dropped: Rc<dyn Fn()> = Rc::new(|| {});
        listeners.borrow_mut().push(Rc::downgrade(&dropped));
        drop(dropped);
        let added: Rc<dyn Fn()> = Rc::new(|| {});
        let (list, new) = (listeners.clone(), added.clone());
        let adder: Rc<dyn Fn()> = Rc::new(move || list.borrow_mut().push(Rc::downgrade(&new)));
        listeners.borrow_mut().push(Rc::downgrade(&adder));
        let kept = live(&listeners);
        assert_eq!((kept.len(), listeners.borrow().len()), (1, 1));
        for listener in kept {
            listener();
        }
        assert_eq!(live(&listeners).len(), 2);
    }
}
