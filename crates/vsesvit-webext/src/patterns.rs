//! Match patterns at the WebKit boundary. Core's `MatchPattern` decides what a pattern
//! means; this module turns pattern *text* into what WebKit's user-script allow/block
//! lists and `web_accessible_resources` globs need, without depending on core (so the
//! tests run on every host).

/// WebKit's `UserContentURLPattern` has no `<all_urls>` and no `*://` scheme, so both are
/// expanded. Other schemes pass through unchanged.
pub fn webkit_patterns(source: &str) -> Vec<String> {
    if source == "<all_urls>" {
        return vec!["http://*/*".into(), "https://*/*".into(), "file://*/*".into()];
    }
    match source.strip_prefix("*://") {
        Some(rest) => vec![format!("http://{rest}"), format!("https://{rest}")],
        None => vec![source.to_owned()],
    }
}

/// Does `url` match the WebExtensions pattern `pattern`? A pure re-implementation of the
/// pattern grammar for the places that have only text: `tabs.query({url})` and
/// `web_accessible_resources.matches`. Invalid patterns match nothing.
pub fn url_matches(pattern: &str, url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    if pattern == "<all_urls>" {
        return matches!(u.scheme(), "http" | "https" | "ws" | "wss" | "ftp" | "file");
    }
    let Some((scheme, rest)) = pattern.split_once("://") else { return false };
    let scheme_ok = match scheme {
        "*" => matches!(u.scheme(), "http" | "https" | "ws" | "wss"),
        s => u.scheme() == s,
    };
    if !scheme_ok {
        return false;
    }
    let Some(slash) = rest.find('/') else { return false };
    let (host_pat, path_pat) = rest.split_at(slash);
    let host_pat = host_pat.rsplit_once(':').map_or(host_pat, |(h, port)| if port == "*" || port.parse::<u16>().is_ok() { h } else { host_pat });
    let host = u.host_str().unwrap_or("").trim_end_matches('.').to_ascii_lowercase();
    let host_ok = match host_pat.to_ascii_lowercase().as_str() {
        "" => u.scheme() == "file",
        "*" => true,
        h => match h.strip_prefix("*.") {
            Some(d) => host == d || host.strip_suffix(d).is_some_and(|p| p.ends_with('.')),
            None => host == h,
        },
    };
    if !host_ok {
        return false;
    }
    let path = match u.query() {
        Some(q) => format!("{}?{}", u.path(), q),
        None => u.path().to_owned(),
    };
    glob(path_pat, &path)
}

/// `*` matches any run of characters; used for `web_accessible_resources` entries too.
pub fn glob(pattern: &str, text: &str) -> bool {
    let (p, t) = (pattern.as_bytes(), text.as_bytes());
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

/// The extension-relative path a runtime API argument names. Chrome resolves these
/// against the extension root, so `/x.js` is `x.js`, and an absolute URL of the
/// extension itself (`chrome.runtime.getURL(..)`, accepted by `action.setPopup` and
/// friends) is its path. `base_url` is `chrome-extension://<host>/`.
pub fn resource_path<'a>(base_url: &str, reference: &'a str) -> &'a str {
    let path = reference.strip_prefix(base_url).or_else(|| (reference == base_url.trim_end_matches('/')).then_some("")).unwrap_or(reference);
    path.trim_start_matches('/')
}

/// May a document at `page_url` load `path` (no leading slash) from an extension whose
/// `web_accessible_resources` entries are `(resources, matches)`? An entry with no
/// `matches` (MV2 lists only resources) is open to every site.
pub fn web_accessible<'a>(entries: impl IntoIterator<Item = (&'a [String], &'a [String])>, path: &str, page_url: &str) -> bool {
    entries.into_iter().any(|(resources, matches)| {
        resources.iter().any(|r| glob(r.trim_start_matches('/'), path))
            && (matches.is_empty() || matches.iter().any(|m| url_matches(m, page_url)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expansion() {
        assert_eq!(webkit_patterns("<all_urls>"), ["http://*/*", "https://*/*", "file://*/*"]);
        assert_eq!(webkit_patterns("*://*.example.com/*"), ["http://*.example.com/*", "https://*.example.com/*"]);
        assert_eq!(webkit_patterns("https://a.test/x*"), ["https://a.test/x*"]);
    }

    #[test]
    fn matching() {
        assert!(url_matches("<all_urls>", "http://127.0.0.1:8080/index.html"));
        assert!(!url_matches("<all_urls>", "chrome-extension://abc/x"));
        assert!(url_matches("*://*/*", "https://x.test/a?b"));
        assert!(url_matches("http://127.0.0.1/*", "http://127.0.0.1:8080/index.html"));
        assert!(url_matches("https://*.example.com/*", "https://a.example.com/"));
        assert!(!url_matches("https://*.example.com/*", "https://notexample.com/"));
        assert!(url_matches("https://example.com/foo*", "https://example.com/foobar?q"));
        assert!(!url_matches("https://example.com/foo*", "https://example.com/bar"));
        assert!(url_matches("file:///*", "file:///tmp/x.html"));
        assert!(!url_matches("garbage", "https://x/"));
        assert!(!url_matches("https://x/*", "not a url"));
    }

    #[test]
    fn accessibility() {
        let res = ["images/*.png".to_owned(), "public.js".to_owned()];
        let sites = ["https://*.allowed.test/*".to_owned()];
        let none: Vec<String> = Vec::new();
        assert!(web_accessible([(&res[..], &sites[..])], "images/a.png", "https://www.allowed.test/page"));
        assert!(!web_accessible([(&res[..], &sites[..])], "images/a.png", "https://other.test/"));
        assert!(!web_accessible([(&res[..], &sites[..])], "secret.js", "https://www.allowed.test/"));
        assert!(web_accessible([(&res[..], &none[..])], "public.js", "https://anything.test/"));
        assert!(!web_accessible(std::iter::empty(), "public.js", "https://anything.test/"));
    }

    #[test]
    fn resource_references_resolve_against_the_extension_root() {
        let base = "chrome-extension://abc/";
        assert_eq!(resource_path(base, "content.js"), "content.js");
        assert_eq!(resource_path(base, "/content.js"), "content.js");
        assert_eq!(resource_path(base, "//images/on.png"), "images/on.png");
        assert_eq!(resource_path(base, "chrome-extension://abc/popup.html"), "popup.html");
        assert_eq!(resource_path(base, "chrome-extension://abc"), "");
        assert_eq!(resource_path(base, "chrome-extension://other/popup.html"), "chrome-extension://other/popup.html");
        assert_eq!(resource_path(base, ""), "");
    }
}
