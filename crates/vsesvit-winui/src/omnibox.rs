//! Provisional address-box input handling: a URL with a known scheme is used as is, anything
//! else gets `https://`. The real policy (search on the default engine, suggestions from
//! bookmarks and history) is `vsesvit_core`'s omnibox; `Browser::omnibox_submitted` is where it
//! replaces this.

const SCHEMES: &[&str] = &[
    "http",
    "https",
    "file",
    "about",
    "data",
    "blob",
    "chrome-extension",
    "edge",
    "view-source",
    "mailto",
];

/// The URL to load for `input`, or `None` when there is nothing to load.
pub(crate) fn navigation_target(input: &str) -> Option<String> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    if let Some((scheme, _)) = text.split_once(':')
        && SCHEMES
            .iter()
            .any(|known| scheme.eq_ignore_ascii_case(known))
    {
        return Some(text.to_owned());
    }
    if let Some(path) = windows_path(text) {
        return Some(format!("file:///{}", path.replace('\\', "/")));
    }
    let scheme = if is_loopback(text) { "http" } else { "https" };
    Some(format!("{scheme}://{text}"))
}

fn windows_path(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let drive = bytes.first()?.is_ascii_alphabetic() && bytes.get(1) == Some(&b':');
    let separator = matches!(bytes.get(2), Some(b'\\' | b'/'));
    (drive && separator).then_some(text)
}

/// Loopback servers rarely speak TLS, so they get `http` like other browsers give them.
fn is_loopback(text: &str) -> bool {
    let authority = text.split(['/', '?', '#']).next().unwrap_or_default();
    let host = if authority.starts_with('[') {
        authority
            .split(']')
            .next()
            .map(|h| format!("{h}]"))
            .unwrap_or_default()
    } else {
        authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host)
            .to_owned()
    };
    host.eq_ignore_ascii_case("localhost") || host.starts_with("127.") || host == "[::1]"
}

/// What the address box shows for a committed URL: nothing for the blank page.
pub(crate) fn display_url(url: &str) -> &str {
    if url == "about:blank" { "" } else { url }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_schemes_pass_through() {
        for url in [
            "https://example.com/a?b#c",
            "HTTP://EXAMPLE.COM",
            "about:blank",
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/popup.html",
            "file:///C:/x.html",
            "data:text/html,hi",
        ] {
            assert_eq!(navigation_target(url).as_deref(), Some(url));
        }
    }

    #[test]
    fn bare_hosts_get_https() {
        assert_eq!(
            navigation_target(" example.com ").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            navigation_target("example.com:8443/x").as_deref(),
            Some("https://example.com:8443/x")
        );
    }

    #[test]
    fn loopback_gets_http() {
        assert_eq!(
            navigation_target("127.0.0.1:8080/page2.html").as_deref(),
            Some("http://127.0.0.1:8080/page2.html")
        );
        assert_eq!(
            navigation_target("localhost:3000").as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(
            navigation_target("[::1]:80/").as_deref(),
            Some("http://[::1]:80/")
        );
        assert_eq!(
            navigation_target("localhost.example.com").as_deref(),
            Some("https://localhost.example.com")
        );
    }

    #[test]
    fn windows_paths_become_file_urls() {
        assert_eq!(
            navigation_target(r"C:\sites\index.html").as_deref(),
            Some("file:///C:/sites/index.html")
        );
    }

    #[test]
    fn empty_input_loads_nothing() {
        assert_eq!(navigation_target("   "), None);
    }

    #[test]
    fn blank_page_shows_empty_address() {
        assert_eq!(display_url("about:blank"), "");
        assert_eq!(display_url("https://a.test/"), "https://a.test/");
    }
}
