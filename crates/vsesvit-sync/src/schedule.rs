//! The sync scheduler: when the next sync is due, how the sync state follows a sync, a sign-in and
//! the passphrase step, with the clock passed in (docs/design/linux-cef/design.md section 4.3,
//! row 12). Nothing here reads the time, starts a timer or touches the profile: a shell keeps its
//! own timers and calls these functions on each tick and each event.

use std::time::{Duration, Instant};

use vsesvit_core::crdt::Seq;

use crate::status::State;
use crate::{Account, Encryption, Error};

/// The first sync waits this long after the first window shows.
pub const FIRST_SYNC: Duration = Duration::from_secs(10);
/// How often the schedule looks for local changes.
pub const TICK: Duration = Duration::from_secs(2);
/// A local change syncs no sooner than this after the last sync started, so a burst of edits is
/// one sync.
pub const CHANGE_GAP: Duration = Duration::from_secs(10);
/// Without local changes, a sync this often downloads what other devices sent.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(60);
/// Quitting waits at most this long for the final sync.
pub const FINAL_SYNC_WAIT: Duration = Duration::from_secs(3);
/// Setting the passphrase and deleting the server's data wait this long for a running sync to end.
pub const IDLE_WAIT: Duration = Duration::from_secs(120);

/// Whether a scheduled sync or Sync Now runs: signed in with a sign-in the provider accepts, and
/// no sync running. `running` is the shell's own flag, not the state's `syncing`: it also covers a
/// sync that outlives the sign-in it started for, and the steps that hold syncs off.
pub fn should_sync(state: &State, running: bool) -> bool {
    !running
        && matches!(
            state,
            State::SignedIn {
                needs_sign_in: false,
                ..
            }
        )
}

/// When the last sync started, and what it left to upload.
#[derive(Clone, Copy, Debug, Default)]
pub struct Schedule {
    /// When the last sync started, and the profile's `change_seq` then.
    last_start: Option<(Instant, Seq)>,
    /// `change_seq` when the last sync that completed started: nothing is left to upload while it
    /// has not moved.
    uploaded: Option<Seq>,
}

impl Schedule {
    /// Whether a tick at `now` syncs, with the profile at `seq`: while `should_sync`,
    /// `SYNC_INTERVAL` after the last sync started, or `CHANGE_GAP` after it when something
    /// changed here since. The first tick syncs.
    pub fn due(&self, state: &State, running: bool, now: Instant, seq: Seq) -> bool {
        if !should_sync(state, running) {
            return false;
        }
        let Some((start, started_at)) = self.last_start else {
            return true;
        };
        let since = now.saturating_duration_since(start);
        since >= SYNC_INTERVAL || (started_at != seq && since >= CHANGE_GAP)
    }

    /// A sync starts at `now` with the profile at `seq`.
    pub fn started(&mut self, now: Instant, seq: Seq) {
        self.last_start = Some((now, seq));
    }

    /// The sync that started with the profile at `seq` completed.
    pub fn completed(&mut self, seq: Seq) {
        self.uploaded = Some(seq);
    }

    /// Whether quitting at `seq` has a final sync to run: signed in, no sync running, and
    /// something changed since the last sync that completed.
    pub fn final_sync_due(&self, state: &State, running: bool, seq: Seq) -> bool {
        should_sync(state, running) && self.uploaded != Some(seq)
    }
}

/// What happens to sync, as its state follows it.
#[derive(Debug)]
pub enum Event {
    Sync(Progress),
    SigningIn,
    SignedIn(Signed),
    SignInFailed {
        /// Why; `None` when the user cancelled it.
        error: Option<String>,
        /// The account still stored, which was signing in again because its sign-in expired, as
        /// it still is.
        stored: Option<Signed>,
    },
    SignedOut,
    /// What the stored account asks of the passphrase now, after a round or a passphrase step.
    Encryption(Encryption),
}

/// What Settings shows of the account a profile is signed in with.
#[derive(Debug)]
pub struct Signed {
    pub name: Option<String>,
    pub server: String,
    pub last_synced: Option<u64>,
    pub encryption: Encryption,
}

/// Where a sync of the signed-in account is.
#[derive(Debug)]
pub enum Progress {
    Started,
    /// When the last round completed.
    Synced(Option<u64>),
    Failed(String),
    SignInExpired,
}

pub fn next(state: State, event: Event) -> State {
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

/// The state of a profile that has just loaded `account`: signed in when it holds one, signed out
/// when it holds none, and signed out with why when it holds one it cannot read (its sealed
/// tokens do not unseal).
pub fn stored_state(account: Result<Option<&Account>, &Error>) -> State {
    let signed_out = State::SignedOut { error: None };
    match account {
        Ok(Some(account)) => next(signed_out, signed_in(account)),
        Ok(None) => signed_out,
        Err(e) => State::SignedOut {
            error: Some(e.to_string()),
        },
    }
}

pub fn signed(account: &Account) -> Signed {
    Signed {
        name: account.name().map(str::to_owned),
        server: account.server().to_owned(),
        last_synced: account.last_synced(),
        encryption: account.encryption(),
    }
}

pub fn signed_in(account: &Account) -> Event {
    Event::SignedIn(signed(account))
}

/// How a sync ended, as an event; `None` when the profile signed out (or in again) meanwhile,
/// which leaves the state to whatever did that.
pub fn settle(result: Result<Option<u64>, &Error>) -> Option<Event> {
    match result {
        Ok(last_synced) => Some(Event::Sync(Progress::Synced(last_synced))),
        Err(Error::SignedOut) => None,
        Err(e) if e.needs_sign_in() => Some(Event::Sync(Progress::SignInExpired)),
        Err(e) => Some(Event::Sync(Progress::Failed(e.to_string()))),
    }
}

/// A sign-in that failed with `e`, while the profile still stores `stored`.
pub fn sign_in_failed(e: &Error, stored: Option<&Account>) -> Event {
    Event::SignInFailed {
        error: match e {
            Error::Cancelled => None,
            e => Some(e.to_string()),
        },
        stored: stored.map(signed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::Action;

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

    fn stored_account() -> Account {
        serde_json::from_value(serde_json::json!({
            "sign_in": "s", "server": "https://sync.example.com", "name": "Demir",
            "limits": { "max_batch": 1, "max_record_bytes": 1, "max_request_bytes": 1 },
            "download_cursor": 0, "upload_cursors": {}, "last_synced": 100,
        }))
        .expect("an account")
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
        assert!(should_sync(&synced, false));
    }

    #[test]
    fn an_expired_sign_in_stops_syncs_until_the_next_sign_in() {
        let expired = next(signed_in_state(), Event::Sync(Progress::SignInExpired));
        assert_eq!(fields(&expired), (Some(100), false, None, true));
        assert!(!should_sync(&expired, false));
        assert!(!should_sync(&State::SignedOut { error: None }, false));
        assert!(!should_sync(&State::SigningIn, false));
        let signing_in = next(expired, Event::SigningIn);
        assert!(should_sync(&next(signing_in, signed_in_event()), false));
    }

    #[test]
    fn a_sync_running_holds_the_next_one_off_whatever_the_state_says() {
        assert!(should_sync(&signed_in_state(), false));
        assert!(!should_sync(&signed_in_state(), true));
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
        let account = stored_account();
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
            !should_sync(&failed, false),
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
    fn a_profile_starts_signed_in_signed_out_or_signed_out_with_why() {
        let account = stored_account();
        let state = stored_state(Ok(Some(&account)));
        assert_eq!(fields(&state), (Some(100), false, None, false));
        assert_eq!(stored_state(Ok(None)), State::SignedOut { error: None });
        let e = Error::TimedOut;
        assert_eq!(
            stored_state(Err(&e)),
            State::SignedOut {
                error: Some("the sign-in took too long".to_owned())
            }
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
        let off = next(syncing, Event::Encryption(Encryption::Off));
        assert_eq!(fields(&off), (None, true, None, false));
        assert_eq!(
            off.status(0).actions,
            [
                Action::SyncNow,
                Action::SetPassphrase,
                Action::SignOut,
                Action::DeleteServerData
            ]
        );
        let ready = next(off, Event::Encryption(Encryption::Ready));
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
        assert!(settle(Err(&Error::SignedOut)).is_none());
        let signed_out = State::SignedOut { error: None };
        let late = next(signed_out.clone(), Event::Sync(Progress::Synced(Some(1))));
        assert_eq!(late, signed_out);
        assert!(matches!(
            settle(Err(&Error::SignInExpired)),
            Some(Event::Sync(Progress::SignInExpired))
        ));
        assert!(matches!(
            settle(Err(&Error::Network("x".into()))),
            Some(Event::Sync(Progress::Failed(e))) if e == "could not reach x"
        ));
        assert!(matches!(
            settle(Ok(Some(5))),
            Some(Event::Sync(Progress::Synced(Some(5))))
        ));
    }

    #[test]
    fn a_tick_syncs_soon_after_a_change_and_now_and_then_without_one() {
        let s = Duration::from_secs;
        let t0 = Instant::now();
        let state = signed_in_state();
        let mut schedule = Schedule::default();
        assert!(schedule.due(&state, false, t0, Seq(7)), "the first tick");
        schedule.started(t0, Seq(7));
        assert!(
            !schedule.due(&state, false, t0 + s(4), Seq(8)),
            "too soon after the last start"
        );
        assert!(schedule.due(&state, false, t0 + s(10), Seq(8)));
        assert!(
            !schedule.due(&state, false, t0 + s(30), Seq(7)),
            "nothing changed"
        );
        assert!(
            schedule.due(&state, false, t0 + s(60), Seq(7)),
            "downloads"
        );
        assert!(
            !schedule.due(&state, true, t0 + s(90), Seq(8)),
            "one sync at a time"
        );
        assert!(
            !schedule.due(&State::SigningIn, false, t0 + s(90), Seq(8)),
            "signed out, or waiting for a new sign-in"
        );
        let expired = next(state, Event::Sync(Progress::SignInExpired));
        assert!(!schedule.due(&expired, false, t0 + s(90), Seq(8)));
    }

    #[test]
    fn quitting_syncs_a_last_time_only_for_changes_the_last_sync_did_not_take() {
        let state = signed_in_state();
        let mut schedule = Schedule::default();
        assert!(schedule.final_sync_due(&state, false, Seq(1)), "never synced");
        schedule.started(Instant::now(), Seq(1));
        assert!(
            schedule.final_sync_due(&state, false, Seq(1)),
            "a sync that has not completed took nothing"
        );
        schedule.completed(Seq(1));
        assert!(!schedule.final_sync_due(&state, false, Seq(1)));
        assert!(schedule.final_sync_due(&state, false, Seq(2)));
        assert!(!schedule.final_sync_due(&state, true, Seq(2)));
        assert!(!schedule.final_sync_due(&State::SigningIn, false, Seq(2)));
    }
}
