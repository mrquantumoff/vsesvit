//! Private browsing, Chrome's incognito: windows whose site choices, zoom and downloads list
//! stay in memory and never reach the database, so nothing private syncs either.
//!
//! One private session per process, as Chrome keeps one off-the-record profile per profile. It
//! starts with the first private window and ends when the last one closes
//! ([`Profile::end_private_session`]); what it kept is gone then. While it lasts, a private
//! window reads the profile's stored state under what the session kept:
//!
//! - site choices made in a private window ([`crate::permissions::SitePermissions`]): prompt
//!   answers, site-info changes, HTTPS-only exceptions
//! - zoom set in a private window ([`crate::zoom::SiteZoom`])
//! - the rows of downloads started in a private window ([`crate::downloads::Downloads`]); the
//!   files stay on disk, as in Chrome
//!
//! Cookies, cache and site storage are the engine's: each shell gives private windows an
//! ephemeral engine session.

use std::collections::HashMap;
use std::fmt;

use crate::Profile;
use crate::downloads::PrivateDownloads;
use crate::permissions::{Origin, Permission, Setting};

/// Which kind of window a tab is in, for its whole life.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Browsing {
    #[default]
    Normal,
    Private,
}

impl Browsing {
    /// What a log line shows for an address a tab of this kind loads. Logs outlive the private
    /// session (Windows writes one into the profile), so a private page's address stays out.
    pub fn loggable(self, address: &dyn fmt::Display) -> &dyn fmt::Display {
        match self {
            Browsing::Normal => address,
            Browsing::Private => &"(a private page)",
        }
    }
}

/// What a private session keeps over the stored state. An entry of `None` masks a stored one:
/// a site the user sent back to ask, or back to the default zoom, in a private window.
#[derive(Default)]
pub(crate) struct PrivateSession {
    pub(crate) sites: HashMap<(Origin, Permission), Option<Setting>>,
    /// Host -> percent, as [`crate::zoom`] stores it.
    pub(crate) zoom: HashMap<String, Option<i64>>,
    pub(crate) downloads: PrivateDownloads,
}

impl Profile {
    /// The last private window closed: forgets what the private session kept. Idempotent.
    pub fn end_private_session(&mut self) {
        let PrivateSession { sites, zoom, downloads } = &mut self.private;
        sites.clear();
        zoom.clear();
        downloads.forget();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_address_stays_out_of_the_log() {
        let url = "https://example.com/?token=secret";
        assert_eq!(format!("loading {}", Browsing::Normal.loggable(&url)), format!("loading {url}"));
        assert_eq!(format!("loading {}", Browsing::Private.loggable(&url)), "loading (a private page)");
    }
}
