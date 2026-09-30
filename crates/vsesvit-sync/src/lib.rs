//! # vsesvit-sync
//!
//! The client side of `vsesvit_sync_proto`: signing in to a sync server with its OpenID Connect
//! provider, and the rounds that move records between a profile and the server.
//!
//! Work is split the way docs/PLAN.md's threading model asks. What touches the profile runs on
//! the UI thread; what touches the network is a `Send` value run on a worker thread:
//!
//! ```ignore
//! // Sign in (worker, then UI, then worker).
//! let pending = SignIn::start(&http, &server_url)?;          // worker
//! open_tab(pending.authorize_url());                         // UI: the provider's page
//! let account = pending.finish(&http)?;                      // worker: waits for the redirect
//! account.save(&mut profile.sync())?;                        // UI
//!
//! // One sync, a few rounds.
//! let mut account = Account::load(&mut profile.sync())?.unwrap();
//! loop {
//!     let round = Round::gather(&mut profile.sync(), account)?;  // UI
//!     let exchanged = round.run(&http);                          // worker
//!     let finished = exchanged.finish(&mut profile.sync());      // UI: applies, saves the account
//!     account = finished.account;
//!     let synced = finished.result?;
//!     refresh_ui(&synced.report.changed);
//!     if !synced.again { break }
//! }
//! ```
//!
//! | module     | owns                                                                    |
//! |------------|-------------------------------------------------------------------------|
//! | `oidc`     | discovery, the authorization code flow with PKCE on a loopback redirect, tokens |
//! | `server`   | the sync server's routes                                                |
//! | `engine`   | the account a profile is signed in with, and the round                  |
//! | [`status`] | what Settings says about each state, for both shells                    |

mod engine;
mod oidc;
mod server;
pub mod status;

use std::time::Duration;

pub use engine::{Account, Exchanged, Finished, Round, Synced};
pub use oidc::SignIn;

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} is not a valid server address")]
    InvalidServer(String),
    #[error("could not reach {0}")]
    Network(String),
    #[error("the server answered {status}: {message}")]
    Server { status: u16, message: String },
    #[error("the server speaks sync protocol {0}, and this version of Vsesvit speaks {PROTOCOL}", PROTOCOL = vsesvit_sync_proto::PROTOCOL)]
    Protocol(u32),
    #[error("unexpected answer from {0}")]
    Malformed(String),
    #[error("none of the sign-in ports ({0}) is free")]
    NoFreePort(String),
    #[error("the sign-in was refused: {0}")]
    Refused(String),
    #[error("the sign-in expired; sign in again")]
    SignInExpired,
    #[error("the sign-in was cancelled")]
    Cancelled,
    #[error("signed out while syncing")]
    SignedOut,
    #[error("the sign-in took too long")]
    TimedOut,
    #[error(transparent)]
    Profile(#[from] vsesvit_core::Error),
}

impl Error {
    /// The account cannot sync again until the user signs in again.
    pub fn needs_sign_in(&self) -> bool {
        matches!(self, Error::SignInExpired)
    }
}

/// The HTTP client the blocking steps share. Cheap to clone and `Send`.
#[derive(Clone)]
pub struct Http(ureq::Agent);

impl Http {
    pub fn new() -> Http {
        Http(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .user_agent(concat!("vsesvit/", env!("CARGO_PKG_VERSION")))
                .timeout_connect(Some(Duration::from_secs(20)))
                .timeout_recv_response(Some(Duration::from_secs(60)))
                .timeout_recv_body(Some(Duration::from_secs(120)))
                .build()
                .into(),
        )
    }
}

impl Default for Http {
    fn default() -> Http {
        Http::new()
    }
}

/// Parses a server or provider address the user typed or a server named. HTTPS, except on the
/// loopback interface, where a developer runs a server without a certificate. Returns it without
/// a trailing slash.
pub fn normalize_base_url(input: &str) -> Result<String, Error> {
    let invalid = || Error::InvalidServer(input.to_owned());
    let url = url::Url::parse(input.trim()).map_err(|_| invalid())?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    let secure = url.scheme() == "https" || (url.scheme() == "http" && loopback);
    if !secure || url.query().is_some() || url.fragment().is_some() || !url.username().is_empty() {
        return Err(invalid());
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn network(url: &str) -> impl FnOnce(ureq::Error) -> Error + '_ {
    move |e| {
        log::warn!("{url}: {e}");
        Error::Network(host_of(url))
    }
}

fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_else(|| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_are_https_except_on_loopback() {
        assert_eq!(normalize_base_url(" https://sync.example.com/ ").unwrap(), "https://sync.example.com");
        assert_eq!(normalize_base_url("https://example.com/sync/").unwrap(), "https://example.com/sync");
        assert_eq!(normalize_base_url("http://127.0.0.1:8080").unwrap(), "http://127.0.0.1:8080");
        assert_eq!(normalize_base_url("http://localhost:8080/").unwrap(), "http://localhost:8080");
        assert!(normalize_base_url("http://sync.example.com").is_err());
        assert!(normalize_base_url("https://example.com/?a=1").is_err());
        assert!(normalize_base_url("sync.example.com").is_err());
        assert!(normalize_base_url("ftp://example.com").is_err());
    }
}
