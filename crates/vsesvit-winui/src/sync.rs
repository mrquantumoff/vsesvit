//! Sync with a Vsesvit sync server (`vsesvit-sync`): signing in, the rounds that move this
//! profile's records, their schedule, and the state Settings shows. The network steps run on
//! worker threads; the profile is read and written here, on the UI thread, between them.
//! Signing in names and pictures the profile after the account, as Chrome does.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use vsesvit_core::Profile;
use vsesvit_core::crdt::Seq;
use vsesvit_core::extensions::toolbar;
use vsesvit_core::prefs::keys;
use vsesvit_core::private::Browsing;
use vsesvit_core::profiles::{AccountDetails, AccountPicture, ProfileEntry};
use vsesvit_core::sync::{Changed, DataType};
use vsesvit_sync::schedule::{
    Event, FINAL_SYNC_WAIT, FIRST_SYNC, IDLE_WAIT, Progress, Schedule, TICK, next, settle,
    should_sync, sign_in_failed, signed_in, stored_state,
};
use vsesvit_sync::status::{State, Status};
use vsesvit_sync::{
    Account, Encryption, Error, Http, Passphrase, PassphraseJob, Round, SignIn, SignedIn,
    now_secs,
};

use crate::browser::Browser;
use crate::exec;
use crate::window::BrowserWindow;

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
    /// Bumped whenever the profile takes an account's details and by every sign-out, so a
    /// picture still on its way for an earlier sign-in is dropped.
    details: Cell<u64>,
    canceller: RefCell<Option<Arc<AtomicBool>>>,
    schedule: Cell<Schedule>,
    /// Called after every change of `state`, while their owners (an open Settings) keep them.
    listeners: RefCell<Vec<Weak<dyn Fn()>>>,
    /// Called after a sync applied records, while their owners (an open dialog) keep them.
    applied: RefCell<Vec<Weak<Applied>>>,
}

impl SyncController {
    /// Signed in when the profile holds an account. One it holds but cannot read (its sealed
    /// tokens do not unseal) shows as signed out, with why.
    pub fn load(profile: &mut Profile) -> Self {
        let account = Account::load(&mut profile.sync());
        if let Err(e) = &account {
            log::warn!("sync account: {e}");
        }
        let state = stored_state(account.as_ref().map(Option::as_ref));
        Self {
            state: RefCell::new(state),
            http: Http::new(),
            running: Cell::new(false),
            attempts: Cell::new(0),
            details: Cell::new(0),
            canceller: RefCell::new(None),
            schedule: Cell::default(),
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

    fn started(&self, seq: Seq) {
        let mut schedule = self.schedule.get();
        schedule.started(Instant::now(), seq);
        self.schedule.set(schedule);
    }

    fn completed(&self, seq: Seq) {
        let mut schedule = self.schedule.get();
        schedule.completed(seq);
        self.schedule.set(schedule);
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

impl From<exec::WorkerLost> for Error {
    fn from(lost: exec::WorkerLost) -> Self {
        Error::Profile(std::io::Error::from(lost).into())
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

/// Syncs `FIRST_SYNC` after the first window shows, then as `Schedule::due` decides every `TICK`.
pub(crate) async fn schedule(browser: Weak<Browser>) {
    exec::sleep(FIRST_SYNC).await;
    loop {
        let Some(b) = browser.upgrade() else { return };
        let sync = b.sync();
        let seq = b.core(|p| p.change_seq());
        if sync.schedule.get().due(
            &sync.state.borrow(),
            sync.running.get(),
            Instant::now(),
            seq,
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
    if !should_sync(&sync.state.borrow(), sync.running.get()) {
        return;
    }
    sync.running.set(true);
    let seq = browser.core(|p| p.change_seq());
    sync.started(seq);
    sync.apply(Event::Sync(Progress::Started));
    exec::spawn(run(Rc::downgrade(browser), seq));
}

async fn run(browser: Weak<Browser>, seq: Seq) {
    let result = rounds(&browser).await;
    let Some(b) = browser.upgrade() else { return };
    b.sync().running.set(false);
    if result.is_ok() {
        b.sync().completed(seq);
    }
    if let Err(e) = &result {
        log::warn!("sync: {e}");
    }
    match settle(result.as_ref().copied()) {
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
        // The account's cookies belong in the normal profile: never in a private window.
        match window.upgrade().filter(|w| w.browsing() == Browsing::Normal) {
            Some(window) => {
                if let Err(e) = window.open_url_tab(pending.authorize_url(), true) {
                    log::warn!("opening the sign-in page: {e}");
                }
            }
            None => b.open_in_normal_window(pending.authorize_url()),
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
        let saved = finished.and_then(|signed_in| {
            b.core(|p| signed_in.account.save_signed_in(&mut p.sync()))?;
            Ok(signed_in)
        });
        match saved {
            Ok(SignedIn { account, details }) => {
                log::info!("sync: signed in to {}", account.server());
                b.sync().apply(signed_in(&account));
                sync_now(&b);
                take_account_details(&b, details);
            }
            Err(e) => {
                log::warn!("sync sign-in: {e}");
                b.sync()
                    .apply(sign_in_failed(&e, stored_account(&b).as_ref()));
            }
        }
    });
}

/// Names and pictures the profile after the account just signed in, unless the user set its
/// details by hand. The picture is fetched on a worker, so it holds up neither the sign-in nor a
/// sync, and taken here, so the list it shows is the newest.
pub(crate) fn take_account_details(browser: &Rc<Browser>, details: AccountDetails) {
    let sync = browser.sync();
    let latest = sync.details.get() + 1;
    sync.details.set(latest);
    let Some(home) = browser.home().cloned() else {
        return;
    };
    let registry = match home
        .dir
        .take_account_details(&home.id, details.name.as_deref(), None)
    {
        Ok(registry) => registry,
        Err(e) => {
            log::warn!("the profile list: {e}");
            return;
        }
    };
    let by_hand = registry
        .get(&home.id)
        .is_none_or(ProfileEntry::set_by_hand);
    browser.set_profiles(registry);
    let Some(url) = details.picture.filter(|_| !by_hand) else {
        return;
    };
    let browser = Rc::downgrade(browser);
    exec::spawn(async move {
        let Ok(Some(picture)) = exec::background(move || AccountPicture::fetch(&url)).await else {
            return;
        };
        let Some(browser) = browser
            .upgrade()
            .filter(|b| b.sync().details.get() == latest)
        else {
            return;
        };
        match home.dir.take_account_details(&home.id, None, Some(&picture)) {
            Ok(registry) => browser.set_profiles(registry),
            Err(e) => log::warn!("the profile list: {e}"),
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
    let sync = browser.sync();
    sync.details.set(sync.details.get() + 1);
    sync.apply(Event::SignedOut);
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
    if !sync
        .schedule
        .get()
        .final_sync_due(&sync.state.borrow(), sync.running.get(), seq)
    {
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
        let idle = exec::wait_for(IDLE_WAIT, Duration::from_millis(100), || {
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
        let idle = exec::wait_for(IDLE_WAIT, Duration::from_millis(100), || {
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
