//! Turns command-line arguments into URLs to load.
//!
//! An existing file relative to the invoking process's working directory comes first; other
//! text reads the way the address bar reads it (core's `classify_url`), minus searching: a URL
//! with a scheme the engine can show loads as is, a path or something shaped like a host gets
//! the scheme it needs, and anything else resolves to nothing.

use std::ffi::OsStr;
use std::path::Path;

use url::Url;
use vsesvit_core::search::classify_url;

/// A command-line target: an existing file relative to the invoking process's working
/// directory, else a URL, a path or a host as the address bar reads it.
pub(crate) fn resolve_cli_target(
    arg: &OsStr,
    cwd: &Path,
    is_file: impl Fn(&Path) -> bool,
) -> Option<Url> {
    let path = cwd.join(arg);
    if path.is_absolute() && is_file(&path) {
        return Url::from_file_path(&path).ok();
    }
    arg.to_str().and_then(classify_url).map(|target| target.url().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_targets_prefer_existing_files_then_urls_then_hosts() {
        let cwd = Path::new("/home/u");
        let exists =
            |p: &Path| p == Path::new("/home/u/page.html") || p == Path::new("/abs/x.html");
        let resolve =
            |arg: &str| resolve_cli_target(OsStr::new(arg), cwd, exists).map(String::from);
        assert_eq!(
            resolve("https://example.com/a?b#c").as_deref(),
            Some("https://example.com/a?b#c")
        );
        assert_eq!(
            resolve("file:///tmp/x.html").as_deref(),
            Some("file:///tmp/x.html")
        );
        assert_eq!(resolve("about:blank").as_deref(), Some("about:blank"));
        assert_eq!(
            resolve("page.html").as_deref(),
            Some("file:///home/u/page.html")
        );
        assert_eq!(
            resolve("/abs/x.html").as_deref(),
            Some("file:///abs/x.html")
        );
        assert_eq!(
            resolve("example.com/path").as_deref(),
            Some("https://example.com/path")
        );
        assert_eq!(
            resolve("missing.html").as_deref(),
            Some("https://missing.html/")
        );
        assert_eq!(resolve("[::1]:8080").as_deref(), Some("http://[::1]:8080/"));
        assert_eq!(resolve("./missing.html"), None);
        assert_eq!(resolve("rust lifetimes"), None);
        assert_eq!(resolve("mailto:someone@example.com"), None);
    }

    #[test]
    fn cli_targets_follow_the_address_bar() {
        let cwd = Path::new("/home/u");
        let resolve =
            |arg: &str| resolve_cli_target(OsStr::new(arg), cwd, |_| false).map(String::from);
        assert_eq!(
            resolve("localhost:8080").as_deref(),
            Some("http://localhost:8080/")
        );
        assert_eq!(
            resolve("127.0.0.1:8000/x").as_deref(),
            Some("http://127.0.0.1:8000/x")
        );
        assert_eq!(
            resolve("view-source:https://example.com/").as_deref(),
            Some("view-source:https://example.com/")
        );
        assert_eq!(resolve("javascript:alert(1)"), None);
    }
}
