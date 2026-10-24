//! Tracking protection (`vsesvit_core::trackers`) in WebView2: each tab's engine view raises
//! `WebResourceRequested` for requests to the tracker list's domains, and the tab answers the
//! ones its page's protection blocks with an empty 403. WebView2's own tracking prevention is
//! off (see `Engine::set_up_profile`).
//!
//! A tab judges every request by the page in its main frame as of that navigation's start: its
//! host, and the level the site gets (Off where the user turned protection off for it). That
//! record is core's `PageProtection`; this module is the WebView2 half, which asks the engine
//! for the requests worth judging.

use std::collections::HashSet;

use vsesvit_core::trackers::TrackerList;
use windows_core::{Interface, Result};

use crate::bindings::*;

/// The tracker domains one tab's engine view raises requests for.
#[derive(Default)]
pub(crate) struct TrackerFilters {
    filtered: HashSet<String>,
}

impl TrackerFilters {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
