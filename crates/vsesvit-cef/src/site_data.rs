//! Owned by W3.1.
//! `Storage.clearDataForOrigin` and the cookie calls; clear at start and at exit (design section 3.7).
//! The bodies below are W1.3's dispatch stubs (design section 8.1): each answers at once.

use vsesvit_engine::{Browsing, RequestId, RequestResult, Url};

use crate::registry;

const UNAVAILABLE: &str = "not available yet";

/// `Engine::cookie_sites`.
pub fn cookie_sites(_browsing: Browsing) -> RequestId {
    registry::answer_now(RequestResult::CookieSites(Vec::new()))
}

/// `Engine::cookies_for`.
pub fn cookies_for(_browsing: Browsing, _url: &Url) -> RequestId {
    registry::answer_now(RequestResult::Cookies(Vec::new()))
}

/// `Engine::delete_site_data`.
pub fn delete_site_data(_browsing: Browsing, _site: &str) -> RequestId {
    registry::answer_now(RequestResult::SiteDataDeleted(Err(UNAVAILABLE.into())))
}

/// `Engine::delete_cookies_for_hosts`.
pub fn delete_cookies_for_hosts(_browsing: Browsing, _hosts: &[String]) {}

/// `Engine::clear_browsing_data`.
pub fn clear_browsing_data(_browsing: Browsing) -> RequestId {
    registry::answer_now(RequestResult::BrowsingDataCleared(Err(UNAVAILABLE.into())))
}
