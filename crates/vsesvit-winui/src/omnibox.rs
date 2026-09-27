//! Address-box details around vsesvit-core's omnibox: engine URLs that core does not classify,
//! how suggestions read in the list, and what the box shows for a committed URL.

use vsesvit_core::search::{Suggestion, SuggestionSource};

/// Schemes WebView2 navigates that core's omnibox does not treat as URLs.
const ENGINE_SCHEMES: &[&str] = &["chrome-extension", "blob", "mailto"];

/// `text` itself when it is a URL of one of `ENGINE_SCHEMES`.
pub(crate) fn engine_url(text: &str) -> Option<String> {
    let text = text.trim();
    let (scheme, rest) = text.split_once(':')?;
    let known = ENGINE_SCHEMES
        .iter()
        .any(|s| scheme.eq_ignore_ascii_case(s));
    (known && !rest.is_empty()).then(|| text.to_owned())
}

/// One line of the suggestion list. Labels are unique within a list, because core deduplicates
/// suggestions by URL and every label that is not a search shows its URL.
pub(crate) fn label(suggestion: &Suggestion) -> String {
    let url = suggestion.target.url().as_str();
    match suggestion.source {
        SuggestionSource::Search | SuggestionSource::Typed => suggestion.title.clone(),
        SuggestionSource::Bookmark => format!("\u{2605} {}  \u{2014}  {url}", suggestion.title),
        SuggestionSource::History if suggestion.title == url => url.to_owned(),
        SuggestionSource::History => format!("{}  \u{2014}  {url}", suggestion.title),
    }
}

/// What the address box shows for a committed URL: nothing for the blank page.
pub(crate) fn display_url(url: &str) -> &str {
    if url == "about:blank" { "" } else { url }
}

#[cfg(test)]
mod tests {
    use vsesvit_core::Url;
    use vsesvit_core::search::NavTarget;

    use super::*;

    fn suggestion(source: SuggestionSource, title: &str, url: &str) -> Suggestion {
        Suggestion {
            source,
            title: title.into(),
            target: NavTarget::Url(Url::parse(url).unwrap()),
        }
    }

    #[test]
    fn engine_schemes_pass_through() {
        assert_eq!(
            engine_url(" chrome-extension://abc/popup.html ").as_deref(),
            Some("chrome-extension://abc/popup.html")
        );
        assert_eq!(
            engine_url("mailto:a@b.test").as_deref(),
            Some("mailto:a@b.test")
        );
        assert_eq!(engine_url("https://a.test/"), None);
        assert_eq!(engine_url("blob:"), None);
        assert_eq!(engine_url("vsesvit fixture"), None);
    }

    #[test]
    fn labels_show_titles_and_urls() {
        let b = suggestion(SuggestionSource::Bookmark, "A", "https://a.test/");
        assert_eq!(label(&b), "\u{2605} A  \u{2014}  https://a.test/");
        let h = suggestion(
            SuggestionSource::History,
            "https://h.test/",
            "https://h.test/",
        );
        assert_eq!(label(&h), "https://h.test/");
        let t = suggestion(
            SuggestionSource::Typed,
            "https://t.test/",
            "https://t.test/",
        );
        assert_eq!(label(&t), "https://t.test/");
    }

    #[test]
    fn blank_page_shows_empty_address() {
        assert_eq!(display_url("about:blank"), "");
        assert_eq!(display_url("https://a.test/"), "https://a.test/");
    }
}
