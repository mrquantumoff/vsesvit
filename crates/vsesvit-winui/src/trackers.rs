//! Tracking protection (`vsesvit_core::trackers`) in WebView2: each tab's engine view raises
//! `WebResourceRequested` for requests to the tracker list's domains, and the tab answers the
//! ones its page's protection blocks with an empty 403. WebView2's own tracking prevention is
//! off (see `Engine::set_up_profile`).
//!
//! A tab judges every request by the page in its main frame as of that navigation's start: its
//! host, and the level the site gets (Off where the user turned protection off for it).

use std::collections::{BTreeSet, HashSet};

use vsesvit_core::Url;
use vsesvit_core::trackers::{TrackerList, TrackingProtection};
use windows_core::{Interface, Result};

use crate::bindings::*;

/// Tracking protection on one tab's page.
pub(crate) struct Protection {
    page_host: String,
    level: TrackingProtection,
    /// The tracker domains blocked on the page.
    blocked: BTreeSet<String>,
    /// The list domains the tab's engine view raises requests for.
    filtered: HashSet<String>,
}

impl Default for Protection {
    fn default() -> Self {
        Self {
            page_host: String::new(),
            level: TrackingProtection::Off,
            blocked: BTreeSet::new(),
            filtered: HashSet::new(),
        }
    }
}

impl Protection {
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
    /// request to the page's own host, its document's included, always loads: the list counts
    /// a company's trackers first party on its own sites.
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

    pub fn blocked(&self) -> &BTreeSet<String> {
        &self.blocked
    }

    /// Makes `core` raise `WebResourceRequested` for requests to the domains of `list` it does
    /// not raise it for yet: from the page, its frames and their dedicated workers. Service and
    /// shared workers are left out: WebView2 raises their requests in every web view with such a
    /// filter, and none of them can tell which page a worker serves.
    pub fn add_filters(&mut self, core: &CoreWebView2, list: &TrackerList) -> Result<()> {
        let core = core.cast::<ICoreWebView2_Manual>()?;
        for domain in list.domains() {
            if self.filtered.contains(domain) {
                continue;
            }
            for uri in filters(domain) {
                core.AddWebResourceRequestedFilter(
                    &uri,
                    CoreWebView2WebResourceContext::All,
                    CoreWebView2WebResourceRequestSourceKinds::Document,
                )?;
            }
            self.filtered.insert(domain.to_owned());
        }
        Ok(())
    }
}

/// WebView2 URI wildcards for requests to `domain` and its subdomains. `?` is the `/` or `:`
/// after the host; the wildcards also match other URIs that merely contain the host, which
/// `TrackerList::blocks` then lets load.
fn filters(domain: &str) -> [String; 2] {
    [format!("*://{domain}?*"), format!("*://*.{domain}?*")]
}

fn host(url: &str) -> Option<String> {
    Url::parse(url).ok()?.host_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vsesvit_core::trackers::Category;

    /// WebView2's wildcard match: `*` any run of characters, `?` exactly one.
    fn matches(pattern: &[u8], text: &[u8]) -> bool {
        match (pattern.split_first(), text.split_first()) {
            (None, _) => text.is_empty(),
            (Some((b'*', rest)), _) => {
                matches(rest, text) || text.split_first().is_some_and(|(_, t)| matches(pattern, t))
            }
            (Some((b'?', rest)), Some((_, t))) => matches(rest, t),
            (Some((p, rest)), Some((c, t))) => p == c && matches(rest, t),
            (Some(_), None) => false,
        }
    }

    fn raised(domain: &str, uri: &str) -> bool {
        filters(domain)
            .iter()
            .any(|f| matches(f.as_bytes(), uri.as_bytes()))
    }

    #[test]
    fn filters_raise_requests_to_the_domain_and_its_subdomains_on_any_port() {
        assert!(raised("doubleclick.net", "https://doubleclick.net/"));
        assert!(raised(
            "doubleclick.net",
            "https://ad.g.doubleclick.net/pagead?x=1"
        ));
        assert!(raised(
            "localhost",
            "http://localhost:8123/tracker/pixel.png"
        ));
        assert!(!raised("doubleclick.net", "https://notdoubleclick.net/"));
        assert!(!raised("localhost", "http://127.0.0.1:8123/trackers.html"));
    }

    #[test]
    fn a_page_counts_the_trackers_it_blocks_until_its_next_document() {
        let list = TrackerList::bundled()
            .clone()
            .with_tracker("localhost", Category::Analytics);
        let mut page = Protection::default();
        page.navigation_starting(
            "http://127.0.0.1:8123/trackers.html",
            TrackingProtection::Standard,
        );
        assert!(!page.blocks(&list, "http://127.0.0.1:8123/trackers.html"));
        assert!(page.blocks(&list, "http://localhost:8123/tracker/pixel.png"));
        assert!(page.blocks(&list, "http://localhost:8123/tracker/other.png"));
        assert_eq!(page.blocked().iter().collect::<Vec<_>>(), ["localhost"]);
        page.new_document();
        assert!(page.blocked().is_empty());

        page.navigation_starting("http://localhost:8123/", TrackingProtection::Standard);
        assert!(
            !page.blocks(&list, "http://localhost:8123/"),
            "a tracker's own site loads"
        );
        page.navigation_starting(
            "http://127.0.0.1:8123/trackers.html",
            TrackingProtection::Off,
        );
        assert!(!page.blocks(&list, "http://localhost:8123/tracker/pixel.png"));
    }
}
