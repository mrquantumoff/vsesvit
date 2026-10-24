//! Address-box details around vsesvit-core's omnibox: how suggestions read in the list, what the
//! box shows for a committed URL and holds while the user edits it, and Windows' keys for core's
//! edit model (`vsesvit_core::omnibox_edit`).

use vsesvit_core::omnibox_edit::{Edit, Key};
use vsesvit_core::search::{Suggestion, SuggestionSource};

use crate::shortcuts::Mods;

/// One line of the suggestion list. Labels are unique within a list, because core deduplicates
/// suggestions by URL and every label that is not a search shows its URL.
pub(crate) fn label(suggestion: &Suggestion) -> String {
    let url = suggestion.target.url().as_str();
    match suggestion.source {
        SuggestionSource::Search | SuggestionSource::SuggestedSearch => suggestion.title.clone(),
        SuggestionSource::Typed => url.to_owned(),
        SuggestionSource::Bookmark => format!("\u{2605} {}  \u{2014}  {url}", suggestion.title),
        SuggestionSource::History if suggestion.title == url => url.to_owned(),
        SuggestionSource::History => format!("{}  \u{2014}  {url}", suggestion.title),
    }
}

/// What the address box shows for a committed URL: nothing for the blank page.
pub(crate) fn display_url(url: &str) -> &str {
    if url == "about:blank" { "" } else { url }
}

/// Whether `url` is a page's address (not a blank tab or the new tab page).
pub(crate) fn has_link(url: &str) -> bool {
    !display_url(url).is_empty()
}

/// What the address box holds: the page's URL, or the user's edit.
#[derive(Debug)]
pub(crate) enum Address {
    /// The page's URL, as the shell last wrote it.
    Page(String),
    Editing(Edit),
}

impl Address {
    /// Whether `text` in the box is the shell's own writing rather than the user's typing.
    pub fn written(&self, text: &str) -> bool {
        match self {
            Address::Page(written) => written == text,
            Address::Editing(edit) => edit.shown().text == text,
        }
    }

    pub fn edit(&self) -> Option<&Edit> {
        match self {
            Address::Editing(edit) => Some(edit),
            Address::Page(_) => None,
        }
    }
}

/// The address box's own meaning of a key press, if it has one. Tab moves through the list
/// only while it is open, and otherwise moves the focus as usual.
pub(crate) fn key(vk: u16, mods: Mods, list_open: bool) -> Option<Key> {
    const TAB: u16 = 0x09;
    const ENTER: u16 = 0x0D;
    const ESCAPE: u16 = 0x1B;
    const END: u16 = 0x23;
    const UP: u16 = 0x26;
    const RIGHT: u16 = 0x27;
    const DOWN: u16 = 0x28;
    const DELETE: u16 = 0x2E;
    match (vk, mods) {
        (DOWN, Mods::NONE) => Some(Key::Down),
        (UP, Mods::NONE) => Some(Key::Up),
        (TAB, Mods::NONE) if list_open => Some(Key::Down),
        (TAB, Mods::SHIFT) if list_open => Some(Key::Up),
        (ESCAPE, Mods::NONE) => Some(Key::Escape),
        (ENTER, Mods::NONE | Mods::SHIFT) => Some(Key::Enter),
        (ENTER, Mods::CTRL) => Some(Key::CtrlEnter),
        (RIGHT | END, Mods::NONE) => Some(Key::Accept),
        (DELETE, Mods::SHIFT) => Some(Key::ShiftDelete),
        _ => None,
    }
}

/// Backspace, Delete and Ctrl+X take text away, and the edit they make gets no inline
/// completion, as in Chrome: otherwise deleting the completion would bring it straight back.
pub(crate) fn deletes(vk: u16, mods: Mods) -> bool {
    matches!(vk, 0x08 | 0x2E) || (vk == 0x58 && mods == Mods::CTRL)
}

#[cfg(test)]
mod tests {
    use vsesvit_core::Url;
    use vsesvit_core::search::{NavTarget, Suggestions};

    use super::*;

    fn suggestion(source: SuggestionSource, title: &str, url: &str) -> Suggestion {
        Suggestion {
            source,
            title: title.into(),
            target: NavTarget::Url(Url::parse(url).unwrap()),
            fill: url
                .trim_start_matches("https://")
                .trim_end_matches('/')
                .into(),
        }
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
        let t = suggestion(SuggestionSource::Typed, "t.test", "https://t.test/");
        assert_eq!(label(&t), "https://t.test/");
    }

    #[test]
    fn blank_page_shows_empty_address() {
        assert_eq!(display_url("about:blank"), "");
        assert_eq!(display_url("https://a.test/"), "https://a.test/");
    }

    #[test]
    fn only_pages_with_an_address_have_a_link() {
        assert!(has_link("https://e.test/") && !has_link("about:blank") && !has_link(""));
        for url in [
            "",
            "about:blank",
            "https://a.test/",
            "about:home",
            "file:///x",
        ] {
            assert_eq!(has_link(url), !display_url(url).is_empty(), "{url}");
        }
    }

    #[test]
    fn shell_writing_is_told_from_typing() {
        let edit = Address::Editing(Edit::new("fixture.test".into(), Suggestions::default()));
        assert!(edit.written("fixture.test"));
        assert!(!edit.written("fixt"));
        assert!(Address::Page("a.test".into()).written("a.test"));
    }

    #[test]
    fn keys_map_as_in_chrome() {
        assert_eq!(key(0x28, Mods::NONE, false), Some(Key::Down));
        assert_eq!(key(0x09, Mods::NONE, false), None);
        assert_eq!(key(0x09, Mods::NONE, true), Some(Key::Down));
        assert_eq!(key(0x09, Mods::SHIFT, true), Some(Key::Up));
        assert_eq!(key(0x0D, Mods::CTRL, false), Some(Key::CtrlEnter));
        assert_eq!(key(0x2E, Mods::SHIFT, true), Some(Key::ShiftDelete));
        assert_eq!(key(0x27, Mods::SHIFT, true), None);
        assert!(deletes(0x08, Mods::NONE));
        assert!(deletes(0x58, Mods::CTRL));
        assert!(!deletes(0x58, Mods::NONE));
    }
}
