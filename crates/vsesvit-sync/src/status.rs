//! What Settings shows for sync, in words both shells share.

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
        /// Why the last sync failed; cleared by the next one that completes.
        error: Option<String>,
        /// The server no longer accepts the session; signing in again fixes it.
        needs_sign_in: bool,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    SignIn,
    Cancel,
    SyncNow,
    SignOut,
    /// Asks first: [`DELETE_CONFIRMATION`].
    DeleteServerData,
}

/// The confirmation before [`Action::DeleteServerData`]: title, body, and the destructive button.
pub const DELETE_CONFIRMATION: (&str, &str, &str) = (
    "Delete your data on the sync server?",
    "Your bookmarks, history, open tabs, extensions and settings are deleted from the server, and all your devices sign out. They stay on your devices, which upload them again when they next sign in.",
    "Delete",
);

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::SignIn => "Sign In",
            Action::Cancel => "Cancel",
            Action::SyncNow => "Sync Now",
            Action::SignOut => "Sign Out",
            Action::DeleteServerData => "Delete Data on Server…",
        }
    }
}

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
                    "Sign in to sync bookmarks, history, open tabs, extensions and settings across your devices.".to_owned()
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
            State::SignedIn { name, server, last_synced, syncing, error, needs_sign_in } => {
                let host = crate::host_of(server);
                let title = match name {
                    Some(name) => format!("Signed in as {name}"),
                    None => format!("Signed in to {host}"),
                };
                let subtitle = if *needs_sign_in {
                    "Sign in again to keep syncing.".to_owned()
                } else if *syncing {
                    "Syncing…".to_owned()
                } else if let Some(error) = error {
                    format!("Sync failed: {error}")
                } else {
                    match last_synced {
                        Some(at) => format!("Last synced {}", ago(now_secs.saturating_sub(*at))),
                        None => "Not synced yet".to_owned(),
                    }
                };
                let actions = if *needs_sign_in {
                    vec![Action::SignIn, Action::SignOut]
                } else {
                    vec![Action::SyncNow, Action::SignOut, Action::DeleteServerData]
                };
                Status { title, subtitle, actions, busy: *syncing, server_editable: false }
            }
        }
    }
}

fn ago(secs: u64) -> String {
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
        }
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
        assert_eq!(s.actions, [Action::SyncNow, Action::SignOut, Action::DeleteServerData]);
        assert_eq!(signed_in().status(1030).subtitle, "Last synced just now");
        assert_eq!(signed_in().status(1000 + 3600).subtitle, "Last synced 1 hour ago");
        assert_eq!(signed_in().status(1000 + 3 * 86_400).subtitle, "Last synced 3 days ago");
    }

    #[test]
    fn a_sync_in_progress_an_error_and_an_expired_sign_in_each_show() {
        let State::SignedIn { server, last_synced, .. } = signed_in() else { unreachable!() };
        let with = |syncing, error: Option<&str>, needs_sign_in| {
            State::SignedIn { name: None, server: server.clone(), last_synced, syncing, error: error.map(str::to_owned), needs_sign_in }.status(1000)
        };
        let s = with(true, None, false);
        assert_eq!((s.title.as_str(), s.subtitle.as_str(), s.busy), ("Signed in to vsesvit-service.mrquantumoff.dev", "Syncing…", true));
        let s = State::SignedIn { name: None, server: "not a url".to_owned(), last_synced, syncing: false, error: None, needs_sign_in: false }.status(1000);
        assert_eq!(s.title, "Signed in to not a url");
        assert_eq!(with(false, Some("could not reach x"), false).subtitle, "Sync failed: could not reach x");
        let s = with(false, None, true);
        assert_eq!((s.subtitle.as_str(), s.actions.as_slice()), ("Sign in again to keep syncing.", &[Action::SignIn, Action::SignOut][..]));
    }
}
