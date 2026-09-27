//! Turns typed text and command-line arguments into URLs to load.
//!
//! This is the shell's stand-in until the omnibox policy of `vsesvit-core` is wired in: a URL
//! with a scheme the engine can show loads as is, anything else that parses as a host gets
//! `https://`. Text that is neither, such as words to search for, resolves to nothing.

use std::ffi::OsStr;
use std::path::Path;

use url::Url;

const NAVIGABLE_SCHEMES: &[&str] = &["http", "https", "file", "about", "data", "webkit"];

pub(crate) fn resolve_typed(text: &str) -> Option<Url> {
    let text = text.trim();
    if let Ok(url) = Url::parse(text) {
        if is_navigable(&url) {
            return Some(url);
        }
        // `localhost:8080` parses with the scheme `localhost`. Any other scheme we cannot show
        // (`javascript:`, `mailto:`) is refused rather than guessed at.
        let rest = &text[url.scheme().len() + 1..];
        if !rest.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
    }
    if text.is_empty() || text.starts_with(['/', '.', '~']) || text.contains(char::is_whitespace) {
        return None;
    }
    Url::parse(&format!("https://{text}"))
        .ok()
        .filter(|url| url.host_str().is_some_and(|host| !host.is_empty()))
}

/// A command-line target: a URL, a file relative to the invoking process's working directory,
/// or a bare host.
pub(crate) fn resolve_cli_target(
    arg: &OsStr,
    cwd: &Path,
    is_file: impl Fn(&Path) -> bool,
) -> Option<Url> {
    if let Some(url) = arg.to_str().and_then(|text| Url::parse(text).ok())
        && is_navigable(&url)
    {
        return Some(url);
    }
    let path = cwd.join(arg);
    if path.is_absolute() && is_file(&path) {
        return Url::from_file_path(&path).ok();
    }
    arg.to_str().and_then(resolve_typed)
}

fn is_navigable(url: &Url) -> bool {
    NAVIGABLE_SCHEMES.contains(&url.scheme())
        && (!matches!(url.scheme(), "http" | "https") || url.host_str().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Option<String> {
        resolve_typed(text).map(String::from)
    }

    #[test]
    fn full_urls_load_as_given() {
        assert_eq!(
            typed("https://example.com/a?b#c").as_deref(),
            Some("https://example.com/a?b#c")
        );
        assert_eq!(
            typed("  http://example.com  ").as_deref(),
            Some("http://example.com/")
        );
        assert_eq!(
            typed("file:///tmp/x.html").as_deref(),
            Some("file:///tmp/x.html")
        );
        assert_eq!(typed("about:blank").as_deref(), Some("about:blank"));
    }

    #[test]
    fn hosts_get_https() {
        assert_eq!(
            typed("example.com").as_deref(),
            Some("https://example.com/")
        );
        assert_eq!(
            typed("example.com/path").as_deref(),
            Some("https://example.com/path")
        );
        assert_eq!(
            typed("localhost:8080").as_deref(),
            Some("https://localhost:8080/")
        );
        assert_eq!(
            typed("127.0.0.1:8000/x").as_deref(),
            Some("https://127.0.0.1:8000/x")
        );
        assert_eq!(typed("[::1]:8080").as_deref(), Some("https://[::1]:8080/"));
    }

    #[test]
    fn text_that_is_not_an_address_resolves_to_nothing() {
        assert_eq!(typed(""), None);
        assert_eq!(typed("   "), None);
        assert_eq!(typed("rust lifetimes"), None);
        assert_eq!(typed("javascript:alert(1)"), None);
        assert_eq!(typed("mailto:someone@example.com"), None);
        assert_eq!(typed("./page.html"), None);
    }

    #[test]
    fn cli_targets_prefer_urls_then_existing_files_then_hosts() {
        let cwd = Path::new("/home/u");
        let exists =
            |p: &Path| p == Path::new("/home/u/page.html") || p == Path::new("/abs/x.html");
        let resolve =
            |arg: &str| resolve_cli_target(OsStr::new(arg), cwd, exists).map(String::from);
        assert_eq!(
            resolve("https://example.com").as_deref(),
            Some("https://example.com/")
        );
        assert_eq!(
            resolve("page.html").as_deref(),
            Some("file:///home/u/page.html")
        );
        assert_eq!(
            resolve("/abs/x.html").as_deref(),
            Some("file:///abs/x.html")
        );
        assert_eq!(
            resolve("example.com").as_deref(),
            Some("https://example.com/")
        );
        assert_eq!(
            resolve("missing.html").as_deref(),
            Some("https://missing.html/")
        );
        assert_eq!(resolve("./missing.html"), None);
    }
}
