//! Owned by W1.5.
//! A tab's navigation rules, as pure functions: which history transition a commit records, what
//! its tab and the address bar show, and what the session saves (UA section 2.3).

use vsesvit_core::Url;
use vsesvit_core::address::readable_url;
use vsesvit_core::history::Transition;
use vsesvit_engine::{EngineTransition, SslInfo};

/// What the address bar says about the connection.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Security {
    Secure,
    Insecure,
    /// A page of the browser or an extension (`vsesvit:`, `chrome-extension:`, `file:`).
    Internal,
    /// Nothing to say: a blank tab, the new tab page, an error page.
    NotApplicable,
}

/// The visit a commit records. The shell's own reason (typed in the address bar, a bookmark) wins;
/// otherwise the engine's, where history has one.
pub fn history_transition(pending: Option<Transition>, engine: EngineTransition) -> Transition {
    pending.unwrap_or(match engine {
        EngineTransition::Typed | EngineTransition::Generated => Transition::Typed,
        EngineTransition::AutoBookmark => Transition::Bookmark,
        EngineTransition::Reload => Transition::Reload,
        EngineTransition::FormSubmit => Transition::FormSubmit,
        EngineTransition::Link | EngineTransition::BackForward | EngineTransition::Other => Transition::Link,
    })
}

pub fn is_blank_url(url: &Url) -> bool {
    url.as_str() == "about:blank"
}

/// The tab's title: the page's, else its readable address, else "New Tab".
pub fn display_title(page_title: &str, committed: Option<&Url>) -> String {
    if !page_title.trim().is_empty() {
        return page_title.to_owned();
    }
    match committed.filter(|url| !is_blank_url(url)) {
        Some(url) => readable_url(url.as_str()),
        None => "New Tab".to_owned(),
    }
}

/// An error page is never secure, even under `https`; an `https` page whose certificate failed is
/// insecure.
pub fn security(committed: Option<&Url>, error_page: bool, ssl: Option<&SslInfo>) -> Security {
    if error_page {
        return Security::NotApplicable;
    }
    match committed.map(Url::scheme) {
        Some("https") if ssl.is_some_and(|s| !s.secure) => Security::Insecure,
        Some("https") => Security::Secure,
        Some("http") => Security::Insecure,
        Some(_) if !committed.is_some_and(is_blank_url) => Security::Internal,
        _ => Security::NotApplicable,
    }
}

/// What the session saves for a tab: the committed address, else the one still loading, as for a
/// restored or background tab whose first page has not arrived yet.
pub fn session_url(committed: Option<&Url>, requested: Option<&Url>) -> Option<Url> {
    committed.or(requested.filter(|url| !is_blank_url(url))).cloned()
}

#[cfg(test)]
mod tests {
    use vsesvit_engine::CertStatus;

    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn the_shells_reason_wins_over_the_engines() {
        assert_eq!(history_transition(Some(Transition::Bookmark), EngineTransition::Typed), Transition::Bookmark);
        assert_eq!(history_transition(None, EngineTransition::Typed), Transition::Typed);
        assert_eq!(history_transition(None, EngineTransition::AutoBookmark), Transition::Bookmark);
        assert_eq!(history_transition(None, EngineTransition::BackForward), Transition::Link);
        assert_eq!(history_transition(None, EngineTransition::Other), Transition::Link);
    }

    #[test]
    fn display_title_falls_back_to_the_address_then_new_tab() {
        let page = url("https://example.com/a%20b");
        assert_eq!(display_title("News", Some(&page)), "News");
        assert_eq!(display_title("  ", Some(&page)), readable_url(page.as_str()));
        assert_eq!(display_title("", Some(&url("about:blank"))), "New Tab");
        assert_eq!(display_title("", None), "New Tab");
    }

    #[test]
    fn security_follows_the_scheme_the_certificate_and_error_pages() {
        let secure = SslInfo { secure: true, status: CertStatus::NONE, der_chain: vec![], mixed_content: false };
        let broken = SslInfo { secure: false, status: CertStatus::AUTHORITY_INVALID, ..secure.clone() };
        let https = url("https://a.test/");
        assert_eq!(security(Some(&https), false, Some(&secure)), Security::Secure);
        assert_eq!(security(Some(&https), false, None), Security::Secure);
        assert_eq!(security(Some(&https), false, Some(&broken)), Security::Insecure);
        assert_eq!(security(Some(&https), true, Some(&secure)), Security::NotApplicable);
        assert_eq!(security(Some(&url("http://a.test/")), false, None), Security::Insecure);
        assert_eq!(security(Some(&url("file:///tmp/a.html")), false, None), Security::Internal);
        assert_eq!(security(Some(&url("about:blank")), false, None), Security::NotApplicable);
        assert_eq!(security(None, false, None), Security::NotApplicable);
    }

    #[test]
    fn the_session_saves_the_committed_address_else_the_one_loading() {
        let (a, b, blank) = (url("https://a.test/"), url("https://b.test/"), url("about:blank"));
        assert_eq!(session_url(Some(&a), Some(&b)), Some(a.clone()));
        assert_eq!(session_url(None, Some(&b)), Some(b));
        assert_eq!(session_url(None, Some(&blank)), None);
        assert_eq!(session_url(Some(&blank), None), Some(blank));
        assert_eq!(session_url(None, None), None);
    }
}
