//! Address-box details around vsesvit-core's omnibox: engine URLs that core does not classify,
//! how suggestions read in the list, what the box shows for a committed URL, and the box while
//! the user edits it (Chrome's keys over the suggestion list and the inline completion).

use vsesvit_core::Url;
use vsesvit_core::search::{self, Suggestion, SuggestionSource, Suggestions};

use crate::shortcuts::Mods;

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
        SuggestionSource::Search => suggestion.title.clone(),
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

/// The user's edit in the address box and the suggestion list under it, as Chrome's omnibox
/// keeps them: the box reads `typed` plus the selected inline completion while row 0 is
/// highlighted, and the highlighted row's `fill` otherwise.
#[derive(Debug)]
pub(crate) struct Edit {
    /// What the user typed, without the inline completion.
    typed: String,
    /// core's suggestions for `typed`; the inline completion is `list.inline`.
    list: Suggestions,
    /// The highlighted row. Row 0, the default match, is highlighted whenever the list has rows.
    row: usize,
}

/// A key the address box handles itself while the user edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    /// Down, or Tab while the list is open.
    Down,
    /// Up, or Shift+Tab while the list is open.
    Up,
    Escape,
    Enter,
    CtrlEnter,
    /// Right or End while the inline completion is selected.
    Accept,
    ShiftDelete,
}

/// What a key did to the edit, for the window to carry out.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Nothing: the text box handles the key as usual.
    Pass,
    /// The box shows `Edit::shown` and the list highlights `Edit::row`, without suggesting again.
    Show,
    /// Opens a URL.
    Open(String),
    /// Opens what the box's text resolves to, as there is no row to open.
    Resolve,
    /// Editing is over: the box shows the page's URL again.
    Revert,
    /// Deletes a URL from history; the list is then suggested again for the same text.
    Forget(Url),
}

/// The box's text, and its selection in UTF-16 units as XAML counts them.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Shown {
    pub text: String,
    pub selection_start: usize,
    pub selection_length: usize,
}

impl Edit {
    pub fn new(typed: String, list: Suggestions) -> Self {
        Self {
            typed,
            list,
            row: 0,
        }
    }

    pub fn typed(&self) -> &str {
        &self.typed
    }

    pub fn items(&self) -> &[Suggestion] {
        &self.list.items
    }

    pub fn inline(&self) -> Option<&str> {
        self.list.inline.as_deref()
    }

    pub fn row(&self) -> usize {
        self.row
    }

    pub fn shown(&self) -> Shown {
        let caret = |text: &str| Shown {
            text: text.to_owned(),
            selection_start: utf16_len(text),
            selection_length: 0,
        };
        match (self.row, self.inline()) {
            (0, Some(inline)) => Shown {
                text: format!("{}{inline}", self.typed),
                selection_start: utf16_len(&self.typed),
                selection_length: utf16_len(inline),
            },
            (0, None) => caret(&self.typed),
            (row, _) => caret(&self.list.items[row].fill),
        }
    }

    /// New suggestions for the same text, after one was deleted: the highlight stays on its
    /// row where the list still reaches it.
    pub fn refill(&mut self, list: Suggestions) {
        self.row = self.row.min(list.items.len().saturating_sub(1));
        self.list = list;
    }

    /// The list closed: no row is left to open or highlight. The inline completion stays in
    /// the box, as it was written.
    pub fn clear_rows(&mut self) {
        self.list.items.clear();
        self.row = 0;
    }

    pub fn key(&mut self, key: Key) -> Outcome {
        let rows = self.list.items.len();
        match key {
            Key::Down | Key::Up if rows == 0 => Outcome::Pass,
            Key::Down => {
                self.row = (self.row + 1).min(rows - 1);
                Outcome::Show
            }
            Key::Up => {
                self.row = self.row.saturating_sub(1);
                Outcome::Show
            }
            Key::Escape if self.row > 0 => {
                self.row = 0;
                Outcome::Show
            }
            Key::Escape => Outcome::Revert,
            Key::CtrlEnter | Key::Enter => {
                let www = (key == Key::CtrlEnter)
                    .then(|| search::ctrl_enter_url(&self.typed))
                    .flatten();
                match (www, self.list.items.get(self.row)) {
                    (Some(url), _) => Outcome::Open(url.to_string()),
                    (None, Some(row)) => Outcome::Open(row.target.url().to_string()),
                    (None, None) => Outcome::Resolve,
                }
            }
            Key::Accept => match self.list.inline.take() {
                Some(inline) if self.row == 0 => {
                    self.typed.push_str(&inline);
                    Outcome::Show
                }
                inline => {
                    self.list.inline = inline;
                    Outcome::Pass
                }
            },
            Key::ShiftDelete => match self.list.items.get(self.row) {
                Some(row) if row.source == SuggestionSource::History => {
                    Outcome::Forget(row.target.url().clone())
                }
                _ => Outcome::Pass,
            },
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

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use vsesvit_core::search::NavTarget;

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

    /// `fix` typed and completed inline to `fixture.test`, then the typed row, a history row and
    /// a bookmark.
    fn fixture_edit() -> Edit {
        let search = Suggestion {
            source: SuggestionSource::Search,
            title: "Search for \"fix\"".into(),
            target: NavTarget::Url(Url::parse("https://search.test/?q=fix").unwrap()),
            fill: "fix".into(),
        };
        let list = Suggestions {
            items: vec![
                suggestion(
                    SuggestionSource::History,
                    "Fixture",
                    "https://fixture.test/",
                ),
                search,
                suggestion(
                    SuggestionSource::History,
                    "Page",
                    "https://fixture.test/page",
                ),
                suggestion(SuggestionSource::Bookmark, "Mark", "https://fix.test/mark"),
            ],
            inline: Some("ture.test".into()),
        };
        Edit::new("fix".into(), list)
    }

    fn text(edit: &Edit) -> String {
        edit.shown().text
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
        let t = suggestion(SuggestionSource::Typed, "t.test", "https://t.test/");
        assert_eq!(label(&t), "https://t.test/");
    }

    #[test]
    fn blank_page_shows_empty_address() {
        assert_eq!(display_url("about:blank"), "");
        assert_eq!(display_url("https://a.test/"), "https://a.test/");
    }

    #[test]
    fn row_zero_shows_the_inline_completion_selected() {
        let edit = fixture_edit();
        assert_eq!(
            edit.shown(),
            Shown {
                text: "fixture.test".into(),
                selection_start: 3,
                selection_length: 9,
            }
        );
        assert_eq!(edit.row(), 0);
    }

    #[test]
    fn arrows_move_the_highlight_and_clamp_at_both_ends() {
        let mut edit = fixture_edit();
        assert_eq!(edit.key(Key::Up), Outcome::Show);
        assert_eq!(edit.row(), 0);
        assert_eq!(edit.key(Key::Down), Outcome::Show);
        assert_eq!(
            edit.shown(),
            Shown {
                text: "fix".into(),
                selection_start: 3,
                selection_length: 0,
            }
        );
        edit.key(Key::Down);
        assert_eq!(text(&edit), "fixture.test/page");
        edit.key(Key::Down);
        edit.key(Key::Down);
        assert_eq!(edit.row(), 3);
        assert_eq!(text(&edit), "fix.test/mark");
        edit.key(Key::Up);
        edit.key(Key::Up);
        edit.key(Key::Up);
        assert_eq!(
            edit.shown().selection_length,
            9,
            "row 0 restores the completion"
        );
    }

    #[test]
    fn escape_goes_back_to_row_zero_then_reverts() {
        let mut edit = fixture_edit();
        edit.key(Key::Down);
        edit.key(Key::Down);
        assert_eq!(edit.key(Key::Escape), Outcome::Show);
        assert_eq!(edit.row(), 0);
        assert_eq!(text(&edit), "fixture.test");
        assert_eq!(edit.key(Key::Escape), Outcome::Revert);
    }

    #[test]
    fn enter_opens_the_highlighted_row() {
        let mut edit = fixture_edit();
        assert_eq!(
            edit.key(Key::Enter),
            Outcome::Open("https://fixture.test/".into())
        );
        edit.key(Key::Down);
        assert_eq!(
            edit.key(Key::Enter),
            Outcome::Open("https://search.test/?q=fix".into())
        );
        let mut empty = Edit::new("fix".into(), Suggestions::default());
        assert_eq!(empty.key(Key::Enter), Outcome::Resolve);
        assert_eq!(empty.key(Key::Down), Outcome::Pass);
    }

    #[test]
    fn ctrl_enter_adds_www_and_com_to_a_word() {
        let mut edit = fixture_edit();
        assert_eq!(
            edit.key(Key::CtrlEnter),
            Outcome::Open("https://www.fix.com/".into())
        );
        let mut spaced = Edit::new("two words".into(), Suggestions::default());
        assert_eq!(spaced.key(Key::CtrlEnter), Outcome::Resolve);
    }

    #[test]
    fn accepting_the_completion_keeps_row_zero() {
        let mut edit = fixture_edit();
        assert_eq!(edit.key(Key::Accept), Outcome::Show);
        assert_eq!(edit.typed(), "fixture.test");
        assert_eq!(edit.row(), 0);
        assert_eq!(edit.shown().selection_start, 12);
        assert_eq!(edit.shown().selection_length, 0);
        assert_eq!(edit.key(Key::Accept), Outcome::Pass);
        edit.key(Key::Down);
        assert_eq!(edit.key(Key::Accept), Outcome::Pass);
    }

    #[test]
    fn shift_delete_forgets_history_rows_only() {
        let mut edit = fixture_edit();
        assert_eq!(
            edit.key(Key::ShiftDelete),
            Outcome::Forget(Url::parse("https://fixture.test/").unwrap())
        );
        edit.key(Key::Down);
        assert_eq!(edit.key(Key::ShiftDelete), Outcome::Pass);
        edit.key(Key::Down);
        edit.key(Key::Down);
        assert_eq!(edit.key(Key::ShiftDelete), Outcome::Pass, "a bookmark");
        edit.refill(Suggestions::default());
        assert_eq!(edit.row(), 0);
    }

    #[test]
    fn shell_writing_is_told_from_typing() {
        let edit = Address::Editing(fixture_edit());
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
