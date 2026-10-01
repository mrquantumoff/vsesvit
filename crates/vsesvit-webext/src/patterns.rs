//! Match patterns at the WebKit boundary. Core's `MatchPattern` decides what a pattern
//! means; this module turns pattern *text* into WebKit's user-script allow/block lists,
//! matches the raw patterns `tabs.query({url})` gets from JavaScript, and decides
//! `web_accessible_resources`. Nothing here needs WebKit, so the tests run on every host.

use vsesvit_core::extensions::manifest::WebAccessible;

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

/// Does `url` match the WebExtensions pattern `pattern`? A re-implementation of the
/// pattern grammar for `tabs.query({url})`, whose patterns are raw text from JavaScript
/// that core never parsed. Invalid patterns match nothing.
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

/// The URL `tabs.create` / `tabs.update` may navigate a tab to, from the `url` an
/// extension passed. A relative URL resolves against the calling extension page (`caller`)
/// or else the extension root (`base_url`, `chrome-extension://<host>/`), as in Chrome.
/// Web pages, `data:`, `about:blank` and the extension's own pages are allowed; Chrome
/// refuses `javascript:` (use `scripting`), and `file:` needs a file-access grant this
/// runtime does not have, so those and every other scheme are refused.
pub fn navigation_url(base_url: &str, caller: Option<&str>, raw: &str) -> Result<String, String> {
    let base = caller.filter(|c| c.starts_with(base_url)).unwrap_or(base_url);
    let url = url::Url::parse(base).and_then(|b| b.join(raw)).map_err(|_| format!("Invalid url: \"{raw}\"."))?;
    let own = url.as_str().starts_with(base_url);
    match url.scheme() {
        "http" | "https" | "data" => Ok(url.into()),
        "about" if url.path() == "blank" => Ok(url.into()),
        "chrome-extension" if own => Ok(url.into()),
        "javascript" => Err("JavaScript URLs are not allowed in API based tab navigation. Use the scripting API instead.".into()),
        _ => Err(format!("Cannot navigate to \"{raw}\".")),
    }
}

/// May a document at `page_url` load `path` (no leading slash) from an extension with
/// these `web_accessible_resources` entries? Core gives MV2's bare resource list
/// `<all_urls>`, so an entry with no `matches` (an MV3 entry naming only `extension_ids`)
/// is open to no web page, as in Chrome.
pub fn web_accessible(entries: &[WebAccessible], path: &str, page_url: &str) -> bool {
    let Ok(page) = url::Url::parse(page_url) else { return false };
    entries.iter().any(|w| w.resources.iter().any(|r| glob(r.trim_start_matches('/'), path)) && w.matches.iter().any(|m| m.matches(&page)))
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
        use vsesvit_core::extensions::manifest::MatchPattern;
        let entry = |matches: &[&str]| WebAccessible {
            resources: vec!["images/*.png".into(), "public.js".into()],
            matches: matches.iter().map(|m| MatchPattern::parse(m).unwrap()).collect(),
        };
        let sites = [entry(&["https://*.allowed.test/*"])];
        assert!(web_accessible(&sites, "images/a.png", "https://www.allowed.test/page"));
        assert!(!web_accessible(&sites, "images/a.png", "https://other.test/"));
        assert!(!web_accessible(&sites, "secret.js", "https://www.allowed.test/"));
        assert!(!web_accessible(&sites, "images/a.png", ""));
        // An MV3 entry without `matches` (only `extension_ids`) is open to no web page.
        assert!(!web_accessible(&[entry(&[])], "public.js", "https://anything.test/"));
        // MV2's bare list, as core normalizes it.
        let legacy = [entry(&["<all_urls>"])];
        assert!(web_accessible(&legacy, "public.js", "http://127.0.0.1:8080/"));
        assert!(!web_accessible(&legacy, "public.js", "chrome-extension://abc/"));
        assert!(!web_accessible(&[], "public.js", "https://anything.test/"));
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

    #[test]
    fn tab_navigation_resolves_relative_urls_and_refuses_script_and_file_urls() {
        let base = "chrome-extension://abc/";
        let ok = |caller: Option<&str>, raw: &str| navigation_url(base, caller, raw).unwrap();
        assert_eq!(ok(Some("chrome-extension://abc/popup/popup.html"), "options.html"), "chrome-extension://abc/popup/options.html");
        assert_eq!(ok(Some("chrome-extension://abc/_generated_background_page.html"), "/welcome.html"), "chrome-extension://abc/welcome.html");
        assert_eq!(ok(None, "options.html"), "chrome-extension://abc/options.html");
        assert_eq!(ok(Some("https://evil.example/dir/"), "x.html"), "chrome-extension://abc/x.html");
        assert_eq!(ok(None, "https://example.com/"), "https://example.com/");
        assert_eq!(ok(None, "about:blank"), "about:blank");
        assert_eq!(ok(None, "data:text/html,hi"), "data:text/html,hi");
        for refused in ["javascript:alert(1)", "JavaScript:alert(1)", "file:///etc/passwd", "chrome-extension://other/x.html", "mailto:a@b.test", "about:config"] {
            assert!(navigation_url(base, None, refused).is_err(), "{refused} must be refused");
        }
        assert!(navigation_url(base, None, "javascript:alert(1)").unwrap_err().contains("scripting"));
    }
}
