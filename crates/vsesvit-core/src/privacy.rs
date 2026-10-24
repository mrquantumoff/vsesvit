//! The privacy snapshot: everything a request filter or a navigation hook needs from the
//! profile's privacy settings, as plain data that does not borrow the [`Profile`].
//!
//! The profile is `!Send` and only its owner's thread may borrow it. A filter the engine runs
//! on another thread (CEF's IO thread), or one that must not fail because the profile is
//! borrowed right now, reads a [`Snapshot`] instead: the shell captures one on the profile's
//! thread after every change that matters (the privacy preferences, a site setting, a sync) and
//! publishes it, for example in an `Arc`. Every question the shell asked the profile before, it
//! asks the snapshot with the same answer, which the tests check against the profile.
//!
//! [`PageProtection`] is the other half of tracking protection: what one tab's page has blocked.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use crate::cookies::{self, SiteRules, ThirdPartyCookies};
use crate::https_only::{self, Reach};
use crate::permissions::{Origin, Permission, Setting};
use crate::prefs::keys;
use crate::private::Browsing;
use crate::trackers::{TrackerList, TrackingProtection};
use crate::{Profile, Url};

/// The privacy settings as they were when [`Snapshot::capture`] read them.
#[derive(Clone, Debug)]
pub struct Snapshot {
    list: Arc<TrackerList>,
    level: TrackingProtection,
    /// Sites the user turned tracking protection off for.
    protection_off: HashSet<Origin>,
    third_party_cookies: ThirdPartyCookies,
    /// Sites whose cookies are set to Allow, which also lets the sites they embed use theirs.
    cookies_allowed: HashSet<Origin>,
    cookie_rules: SiteRules,
    https_only: bool,
    /// Sites the user chose to load over http anyway, as a normal and a private window see them.
    http_allowed: [HashSet<Origin>; 2],
}

impl Snapshot {
    /// Reads the settings from `p`; `list` is the tracker list to block from.
    pub fn capture(p: &mut Profile, list: Arc<TrackerList>) -> Snapshot {
        let stored = p.site_permissions().all();
        let allowing = |permission: Permission| -> HashSet<Origin> {
            stored
                .iter()
                .filter(|s| s.permission == permission && s.setting == Setting::Allow)
                .map(|s| s.origin.clone())
                .collect()
        };
        let protection_off = allowing(Permission::Trackers);
        let cookies_allowed = allowing(Permission::Cookies);
        // A private session keeps its own choices over the stored ones, and a reset there masks
        // a stored Allow, so each origin is asked in the kind of window that reads it.
        let mut http_sites: BTreeSet<Origin> = stored.iter().filter(|s| s.permission == Permission::Http).map(|s| s.origin.clone()).collect();
        http_sites.extend(p.private.sites.keys().filter(|(_, permission)| *permission == Permission::Http).map(|(origin, _)| origin.clone()));
        let mut http_allowed = [HashSet::new(), HashSet::new()];
        for origin in http_sites {
            for browsing in [Browsing::Normal, Browsing::Private] {
                if https_only::allowed(p, browsing, &origin) {
                    http_allowed[slot(browsing)].insert(origin.clone());
                }
            }
        }
        Snapshot {
            list,
            level: p.prefs().get(&keys::TRACKING_PROTECTION),
            protection_off,
            third_party_cookies: p.prefs().get(&keys::THIRD_PARTY_COOKIES),
            cookies_allowed,
            cookie_rules: cookies::site_rules(p),
            https_only: p.prefs().get(&keys::HTTPS_ONLY),
            http_allowed,
        }
    }

    /// The tracker list requests are judged by.
    pub fn tracker_list(&self) -> &Arc<TrackerList> {
        &self.list
    }

    /// The protection pages of `origin` get: the user's level, or Off on a site they turned it off
    /// for. A page with no origin gets the level. ([`crate::trackers::level_for`].)
    pub fn tracker_level(&self, origin: Option<&Origin>) -> TrackingProtection {
        match origin {
            Some(origin) if self.protection_off.contains(origin) => TrackingProtection::Off,
            _ => self.level,
        }
    }

    /// The list domain a request to `request_host` matches when the page at `page_host` gets
    /// `level` ([`TrackerList::blocks`]).
    pub fn tracker_blocks(&self, level: TrackingProtection, page_host: &str, request_host: &str) -> Option<&str> {
        self.list.blocks(level, page_host, request_host)
    }

    /// Whether third-party cookies are blocked on pages of `top` in `browsing` windows
    /// ([`cookies::third_party_blocked`]).
    pub fn third_party_cookies_blocked(&self, browsing: Browsing, top: Option<&Origin>) -> bool {
        self.third_party_cookies.blocks(browsing) && !top.is_some_and(|o| self.cookies_allowed.contains(o))
    }

    /// The per-site cookie rules: the hosts set to Block, the sites to clear.
    pub fn cookie_rules(&self) -> &SiteRules {
        &self.cookie_rules
    }

    /// Whether a cookie under `domain` reaches a site set to Block ([`SiteRules::blocks_cookie`]).
    pub fn blocks_cookie(&self, domain: &str) -> bool {
        self.cookie_rules.blocks_cookie(domain)
    }

    /// Whether the user turned HTTPS-only on.
    pub fn https_only(&self) -> bool {
        self.https_only
    }

    /// Whether the user chose to load `origin` over http anyway in `browsing` windows
    /// ([`https_only::allowed`]).
    pub fn https_allowed(&self, browsing: Browsing, origin: &Origin) -> bool {
        self.http_allowed[slot(browsing)].contains(origin)
    }

    /// What a navigation to `url` loads instead under HTTPS-only ([`https_only::upgrade`]).
    pub fn https_upgrade(&self, browsing: Browsing, url: &Url, reach: Reach) -> Option<Url> {
        if !self.https_only {
            return None;
        }
        let https = https_only::upgraded(url, reach)?;
        let origin = Origin::of(url)?;
        (!self.https_allowed(browsing, &origin)).then_some(https)
    }
}

/// Tracking protection on one tab's page: the page's host and level as of the navigation's
/// start, and the tracker domains blocked on it.
#[derive(Clone, Debug)]
pub struct PageProtection {
    page_host: String,
    level: TrackingProtection,
    blocked: BTreeSet<String>,
}

impl Default for PageProtection {
    fn default() -> Self {
        PageProtection { page_host: String::new(), level: TrackingProtection::Off, blocked: BTreeSet::new() }
    }
}

impl PageProtection {
    /// The main frame starts navigating to `url` (also on each redirect), a page `level`
    /// applies to.
    pub fn navigation_starting(&mut self, url: &str, level: TrackingProtection) {
        self.page_host = host(url).unwrap_or_default();
        self.level = level;
    }

    /// A new document commits; what was blocked was the previous page's.
    pub fn new_document(&mut self) {
        self.blocked.clear();
    }

    /// Whether the page's protection blocks a request for `url`, which is then recorded. A
    /// request to the page's own host, its document's included, always loads: the list counts a
    /// company's trackers first party on its own sites.
    pub fn blocks(&mut self, list: &TrackerList, url: &str) -> bool {
        let Some(request_host) = host(url) else {
            return false;
        };
        let Some(domain) = list.blocks(self.level, &self.page_host, &request_host) else {
            return false;
        };
        self.blocked.insert(domain.to_owned());
        true
    }

    /// The tracker domains blocked on the page.
    pub fn blocked(&self) -> &BTreeSet<String> {
        &self.blocked
    }
}

fn slot(browsing: Browsing) -> usize {
    match browsing {
        Browsing::Normal => 0,
        Browsing::Private => 1,
    }
}

fn host(url: &str) -> Option<String> {
    Url::parse(url).ok()?.host_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trackers::Category;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn a_snapshot_crosses_threads() {
        assert_send_sync::<Snapshot>();
    }

    #[test]
    fn a_page_counts_the_trackers_it_blocks_until_its_next_document() {
        let list = TrackerList::bundled().clone().with_tracker("localhost", Category::Analytics);
        let mut page = PageProtection::default();
        page.navigation_starting("http://127.0.0.1:8123/trackers.html", TrackingProtection::Standard);
        assert!(!page.blocks(&list, "http://127.0.0.1:8123/trackers.html"));
        assert!(page.blocks(&list, "http://localhost:8123/tracker/pixel.png"));
        assert!(page.blocks(&list, "http://localhost:8123/tracker/other.png"));
        assert_eq!(page.blocked().iter().collect::<Vec<_>>(), ["localhost"]);
        page.new_document();
        assert!(page.blocked().is_empty());

        page.navigation_starting("http://localhost:8123/", TrackingProtection::Standard);
        assert!(!page.blocks(&list, "http://localhost:8123/"), "a tracker's own site loads");
        page.navigation_starting("http://127.0.0.1:8123/trackers.html", TrackingProtection::Off);
        assert!(!page.blocks(&list, "http://localhost:8123/tracker/pixel.png"));
    }
}
