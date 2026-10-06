//! What Settings shows for sync, in words both shells share.

use crate::Encryption;
use crate::crypto::Passphrase;

/// Where a profile's sync stands, as the shell tracks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// `error` is why the last sign-in failed, if it did.
    SignedOut { error: Option<String> },
    /// The sync server's sign-in page is open in a tab.
    SigningIn,
    SignedIn {
        name: Option<String>,
        server: String,
        last_synced: Option<u64>,
        syncing: bool,
        /// Why the last sync, or the last attempt to sign in again, failed; cleared by the next
        /// sync that completes.
        error: Option<String>,
        /// The server no longer accepts the session; signing in again fixes it.
        needs_sign_in: bool,
        /// Whether the device syncs, or what it asks of the sync passphrase first.
        encryption: Encryption,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    SignIn,
    Cancel,
    /// Asks for it first: [`passphrase_dialog`], as are the next two.
    SetPassphrase,
    EnterPassphrase,
    SyncNow,
    ChangePassphrase,
    SignOut,
    /// Asks first: [`DELETE_CONFIRMATION`].
    DeleteServerData,
}

/// The confirmation before [`Action::DeleteServerData`]: title, body, and the destructive button.
pub const DELETE_CONFIRMATION: (&str, &str, &str) = (
    "Delete your data on the sync server?",
    "Your bookmarks, history, open tabs, extensions and settings are deleted from the server, along with your sync passphrase, and all your devices sign out. They stay on your devices, which upload them again when they next sign in and a passphrase is set.",
    "Delete",
);

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::SignIn => "Sign In",
            Action::Cancel => "Cancel",
            Action::SetPassphrase => "Set Passphrase…",
            Action::EnterPassphrase => "Enter Passphrase…",
            Action::SyncNow => "Sync Now",
            Action::ChangePassphrase => "Change Passphrase…",
            Action::SignOut => "Sign Out",
            Action::DeleteServerData => "Delete Data on Server…",
        }
    }
}

/// What the dialog behind [`Action::SetPassphrase`], [`Action::EnterPassphrase`] or
/// [`Action::ChangePassphrase`] says. A new passphrase is typed twice.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PassphraseDialog {
    pub title: &'static str,
    pub body: &'static str,
    pub field: &'static str,
    /// The second field's label, for a new passphrase.
    pub confirm: Option<&'static str>,
    pub accept: &'static str,
}

/// The dialog for what `encryption` asks for, or `None` when it asks nothing of the passphrase.
pub fn passphrase_dialog(encryption: Encryption) -> Option<PassphraseDialog> {
    Some(match encryption {
        Encryption::Checking => return None,
        Encryption::Set => PassphraseDialog {
            title: "Set a Sync Passphrase",
            body: "Vsesvit encrypts your bookmarks, history, open tabs, extensions and settings with this passphrase before they leave this device, so the sync server can't read them. You'll enter it on each device you sync. If you forget it, it can't be recovered. If you already set one on another device, don't set a new one: enter that one here once Vsesvit finds it.",
            field: "Passphrase",
            confirm: Some("Confirm passphrase"),
            accept: "Set Passphrase",
        },
        Encryption::Enter => PassphraseDialog {
            title: "Enter Your Sync Passphrase",
            body: "Your synced data is encrypted with your sync passphrase. Enter it to start syncing on this device. If you've forgotten it, delete your data on the sync server and set a new one.",
            field: "Passphrase",
            confirm: None,
            accept: "Start Syncing",
        },
        Encryption::Changed => PassphraseDialog {
            title: "Enter Your New Sync Passphrase",
            body: "Your sync passphrase was changed on another device. Enter the new one to keep syncing. Never enter an old passphrase here.",
            field: "New passphrase",
            confirm: None,
            accept: "Start Syncing",
        },
        Encryption::Ready => PassphraseDialog {
            title: "Change Your Sync Passphrase",
            body: "Vsesvit encrypts your data again with the new passphrase. Your other devices stop syncing until you enter it on each of them. The sync server may keep copies it made earlier, which the old passphrase still opens.",
            field: "New passphrase",
            confirm: Some("Confirm new passphrase"),
            accept: "Change Passphrase",
        },
    })
}

/// Checks what was typed in a [`PassphraseDialog`]: `confirm` is the second field's text, when the
/// dialog has one. `Err` is what to show under the fields.
pub fn check_passphrase(text: &str, confirm: Option<&str>) -> Result<Passphrase, &'static str> {
    if confirm.is_some_and(|c| c != text) {
        return Err("The passphrases don't match");
    }
    Passphrase::new(text.to_owned()).ok_or(TOO_SHORT)
}

const TOO_SHORT: &str = "Use at least 8 characters";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub title: String,
    pub subtitle: String,
    /// Buttons, in order; the first is the suggested one.
    pub actions: Vec<Action>,
    /// `SyncNow` is shown but cannot be pressed.
    pub busy: bool,
    /// The server address can be changed.
    pub server_editable: bool,
}

impl State {
    pub fn status(&self, now_secs: u64) -> Status {
        match self {
            State::SignedOut { error } => Status {
                title: "Not signed in".to_owned(),
                subtitle: error.clone().unwrap_or_else(|| {
                    "Sign in to sync bookmarks, history, open tabs, extensions and settings across your devices, encrypted with a passphrase only you know.".to_owned()
                }),
                actions: vec![Action::SignIn],
                busy: false,
                server_editable: true,
            },
            State::SigningIn => Status {
                title: "Signing in…".to_owned(),
                subtitle: "Finish signing in on the page that opened.".to_owned(),
                actions: vec![Action::Cancel],
                busy: false,
                server_editable: false,
            },
            State::SignedIn { name, server, last_synced, syncing, error, needs_sign_in, encryption } => {
                let host = crate::host_of(server);
                let title = match name {
                    Some(name) => format!("Signed in as {name}"),
                    None => format!("Signed in to {host}"),
                };
                let asks = match encryption {
                    Encryption::Checking => Some("Checking for a sync passphrase…"),
                    Encryption::Set => Some("Set a sync passphrase to start syncing. Your data is encrypted with it before it leaves this device."),
                    Encryption::Enter => Some("Enter your sync passphrase to start syncing on this device."),
                    Encryption::Changed => Some("Your sync passphrase was changed on another device. Enter the new one to keep syncing."),
                    Encryption::Ready => None,
                };
                let subtitle = if *needs_sign_in {
                    match error {
                        Some(error) => format!("Sign-in failed: {error}"),
                        None => "Sign in again to keep syncing.".to_owned(),
                    }
                } else if let Some(error) = error.as_ref().filter(|_| !*syncing) {
                    format!("Sync failed: {error}")
                } else if let Some(asks) = asks {
                    asks.to_owned()
                } else if *syncing {
                    "Syncing…".to_owned()
                } else {
                    match last_synced {
                        Some(at) => format!("Last synced {}", ago(now_secs.saturating_sub(*at))),
                        None => "Not synced yet".to_owned(),
                    }
                };
                let actions = if *needs_sign_in {
                    vec![Action::SignIn, Action::SignOut]
                } else {
                    match encryption {
                        Encryption::Checking => vec![Action::SignOut, Action::DeleteServerData],
                        Encryption::Set => vec![Action::SetPassphrase, Action::SignOut, Action::DeleteServerData],
                        Encryption::Enter | Encryption::Changed => vec![Action::EnterPassphrase, Action::SignOut, Action::DeleteServerData],
                        Encryption::Ready => vec![Action::SyncNow, Action::ChangePassphrase, Action::SignOut, Action::DeleteServerData],
                    }
                };
                Status { title, subtitle, actions, busy: *syncing, server_editable: false }
            }
        }
    }
}

/// "just now", "5 minutes ago", "3 hours ago", "2 days ago": how long ago, in seconds, something
/// happened, as both shells word it.
pub fn ago(secs: u64) -> String {
    let (n, unit) = match secs {
        0..60 => return "just now".to_owned(),
        60..3600 => (secs / 60, "minute"),
        3600..86_400 => (secs / 3600, "hour"),
        _ => (secs / 86_400, "day"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_in() -> State {
        State::SignedIn {
            name: Some("Demir".to_owned()),
            server: "https://vsesvit-service.mrquantumoff.dev".to_owned(),
            last_synced: Some(1000),
            syncing: false,
            error: None,
            needs_sign_in: false,
            encryption: Encryption::Ready,
        }
    }

    fn with_encryption(encryption: Encryption) -> State {
        let State::SignedIn { name, server, last_synced, syncing, error, needs_sign_in, .. } = signed_in() else { unreachable!() };
        State::SignedIn { name, server, last_synced, syncing, error, needs_sign_in, encryption }
    }

    #[test]
    fn signed_out_offers_sign_in_and_the_server_field() {
        let s = State::SignedOut { error: None }.status(0);
        assert_eq!((s.title.as_str(), s.actions.as_slice(), s.server_editable), ("Not signed in", &[Action::SignIn][..], true));
        let s = State::SignedOut { error: Some("the sign-in was refused: denied".to_owned()) }.status(0);
        assert_eq!(s.subtitle, "the sign-in was refused: denied");
        let s = State::SigningIn.status(0);
        assert_eq!((s.actions.as_slice(), s.server_editable), (&[Action::Cancel][..], false));
    }

    #[test]
    fn signed_in_says_who_and_when() {
        let s = signed_in().status(1000 + 125);
        assert_eq!((s.title.as_str(), s.subtitle.as_str()), ("Signed in as Demir", "Last synced 2 minutes ago"));
        assert_eq!(s.actions, [Action::SyncNow, Action::ChangePassphrase, Action::SignOut, Action::DeleteServerData]);
        assert_eq!(signed_in().status(1030).subtitle, "Last synced just now");
        assert_eq!(signed_in().status(1000 + 3600).subtitle, "Last synced 1 hour ago");
        assert_eq!(signed_in().status(1000 + 3 * 86_400).subtitle, "Last synced 3 days ago");
    }

    #[test]
    fn a_sync_in_progress_an_error_and_an_expired_sign_in_each_show() {
        let State::SignedIn { server, last_synced, .. } = signed_in() else { unreachable!() };
        let with = |syncing, error: Option<&str>, needs_sign_in| {
            State::SignedIn {
                name: None,
                server: server.clone(),
                last_synced,
                syncing,
                error: error.map(str::to_owned),
                needs_sign_in,
                encryption: Encryption::Ready,
            }
            .status(1000)
        };
        let s = with(true, None, false);
        assert_eq!((s.title.as_str(), s.subtitle.as_str(), s.busy), ("Signed in to vsesvit-service.mrquantumoff.dev", "Syncing…", true));
        let s = State::SignedIn {
            name: None,
            server: "not a url".to_owned(),
            last_synced,
            syncing: false,
            error: None,
            needs_sign_in: false,
            encryption: Encryption::Ready,
        }
        .status(1000);
        assert_eq!(s.title, "Signed in to not a url");
        assert_eq!(with(false, Some("could not reach x"), false).subtitle, "Sync failed: could not reach x");
        let s = with(false, None, true);
        assert_eq!((s.subtitle.as_str(), s.actions.as_slice()), ("Sign in again to keep syncing.", &[Action::SignIn, Action::SignOut][..]));
    }

    #[test]
    fn a_failed_sign_in_again_shows_why() {
        let State::SignedIn { name, server, last_synced, .. } = signed_in() else { unreachable!() };
        let expired = |error: Option<&str>| {
            State::SignedIn {
                name: name.clone(),
                server: server.clone(),
                last_synced,
                syncing: false,
                error: error.map(str::to_owned),
                needs_sign_in: true,
                encryption: Encryption::Ready,
            }
            .status(1000)
        };
        let s = expired(Some("could not reach sync.example"));
        assert!(s.subtitle.contains("could not reach sync.example"), "{}", s.subtitle);
        assert_eq!(s.actions, [Action::SignIn, Action::SignOut]);
        assert_eq!(expired(None).subtitle, "Sign in again to keep syncing.");
    }

    #[test]
    fn until_it_syncs_a_device_asks_for_the_passphrase_and_offers_no_sync() {
        let s = with_encryption(Encryption::Checking).status(1000);
        assert_eq!((s.subtitle.as_str(), s.actions.as_slice()), ("Checking for a sync passphrase…", &[Action::SignOut, Action::DeleteServerData][..]));
        let s = with_encryption(Encryption::Set).status(1000);
        assert_eq!(s.actions, [Action::SetPassphrase, Action::SignOut, Action::DeleteServerData]);
        assert!(s.subtitle.starts_with("Set a sync passphrase"), "{}", s.subtitle);
        for encryption in [Encryption::Enter, Encryption::Changed] {
            let s = with_encryption(encryption).status(1000);
            assert_eq!(s.actions, [Action::EnterPassphrase, Action::SignOut, Action::DeleteServerData]);
        }
        assert!(with_encryption(Encryption::Changed).status(1000).subtitle.contains("changed on another device"));
    }

    #[test]
    fn each_passphrase_step_has_its_dialog() {
        assert_eq!(passphrase_dialog(Encryption::Checking), None);
        let set = passphrase_dialog(Encryption::Set).unwrap();
        assert_eq!((set.accept, set.confirm), ("Set Passphrase", Some("Confirm passphrase")));
        assert_eq!(passphrase_dialog(Encryption::Enter).unwrap().confirm, None, "an existing passphrase is typed once");
        assert_eq!(passphrase_dialog(Encryption::Changed).unwrap().field, "New passphrase");
        assert_eq!(passphrase_dialog(Encryption::Ready).unwrap().accept, "Change Passphrase");
    }

    #[test]
    fn a_new_passphrase_is_long_enough_and_typed_the_same_twice() {
        assert_eq!(check_passphrase("short", None).unwrap_err(), "Use at least 8 characters");
        assert!(TOO_SHORT.contains(&crate::MIN_PASSPHRASE_CHARS.to_string()));
        assert_eq!(check_passphrase("long enough", Some("long enougj")).unwrap_err(), "The passphrases don't match");
        assert!(check_passphrase("long enough", Some("long enough")).is_ok());
        assert!(check_passphrase("long enough", None).is_ok());
    }

    #[test]
    fn how_long_ago_rounds_down_to_the_largest_unit() {
        let cases = [
            (0, "just now"),
            (59, "just now"),
            (60, "1 minute ago"),
            (3599, "59 minutes ago"),
            (3600, "1 hour ago"),
            (5 * 3600, "5 hours ago"),
            (86_399, "23 hours ago"),
            (86_400, "1 day ago"),
            (2 * 86_400, "2 days ago"),
        ];
        for (secs, text) in cases {
            assert_eq!(ago(secs), text, "{secs} s");
        }
    }
}
