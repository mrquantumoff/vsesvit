//! The address box while the user edits it: suggestions as they type, the inline completion,
//! and Chrome's keys over the suggestion list (see `omnibox::Edit`). The box's text box never
//! sees the keys handled here, and the text written here is never taken for typing.

use vsesvit_core::Url;
use vsesvit_core::history::Transition;
use vsesvit_core::suggest::SearchSuggestions;
use windows_core::{IInspectable, Interface};

use super::BrowserWindow;
use super::wiring::with;
use crate::bindings::*;
use crate::omnibox::{self, Address, Edit, Key, Outcome};
use crate::shortcuts::Mods;
use crate::{exec, xaml};

impl BrowserWindow {
    /// Opens `text` from the address box: the list's row labelled `text` (a click on it), or
    /// what `text` resolves to.
    pub fn address_submitted(&self, text: &str) {
        let row = self.address.borrow().edit().and_then(|edit| {
            edit.items()
                .iter()
                .find(|s| omnibox::label(s) == text)
                .map(|s| s.target.url().to_string())
        });
        let url = row.or_else(|| self.browser().and_then(|b| b.resolve_input(text)));
        if let Some(url) = url {
            self.open_from_address(&url);
        }
    }

    fn open_from_address(&self, url: &str) {
        self.address.replace(Address::Page(String::new()));
        self.close_suggestions();
        match self.active_tab() {
            Some(tab) => {
                tab.navigate_as(url, Transition::Typed);
                if self.is_foreground() {
                    tab.focus_page();
                }
            }
            None => {
                if let Err(e) = self.open_url_tab(url, true) {
                    log::error!("open {url}: {e}");
                }
            }
        }
        self.refresh_chrome();
    }

    /// Puts `text` in the box with the caret at its end, as typing it would, and opens the
    /// list, as typing into the focused box does; a scripted run's keyboard.
    pub fn type_address(&self, text: &str) {
        let _ = self.ui.address.SetText(text);
        if let Some(text_box) = self.address_text_box() {
            let end = i32::try_from(text.encode_utf16().count()).unwrap_or(i32::MAX);
            let _ = text_box.Select(end, 0);
        }
        self.address_changed();
        if !self.suggestion_labels().is_empty() {
            let _ = self.ui.address.SetIsSuggestionListOpen(true);
        }
    }

    fn suggest_for(&self, typed: String, allow_inline: bool) {
        let Some(browser) = self.browser() else {
            return;
        };
        let list = browser.suggest(&typed, allow_inline);
        let request = browser.suggest_request(self.browsing, &typed, &self.search_queries);
        self.address
            .replace(Address::Editing(Edit::new(typed.clone(), list)));
        self.fill_list();
        self.show_edit();
        if let Some(request) = request {
            let me = self.me.clone();
            exec::spawn(async move {
                if let Ok(Some(found)) = exec::background(move || request.run()).await {
                    with(&me, |w| w.add_search_suggestions(&typed, found));
                }
            });
        }
    }

    /// Lists what the engine suggested for `typed`, unless the user typed again or left the box
    /// since. Only the list changes: the box's text, inline completion and selection stay as
    /// the user has them.
    fn add_search_suggestions(&self, typed: &str, found: SearchSuggestions) {
        match &mut *self.address.borrow_mut() {
            Address::Editing(edit) if found.is_current() && edit.typed() == typed => {
                edit.add_search_suggestions(found);
            }
            _ => return,
        }
        self.fill_list();
    }

    fn fill_list(&self) {
        let items: Vec<Option<IInspectable>> = self
            .suggestion_labels()
            .iter()
            .map(|label| xaml::boxed(label).ok())
            .collect();
        let source = windows_collections::IVector::<IInspectable>::from(items);
        let _ = self
            .ui
            .address
            .cast::<ItemsControl>()
            .and_then(|list| list.SetItemsSource(&source));
    }

    /// Writes the edit into the box and highlights its row in the list.
    fn show_edit(&self) {
        let Some(shown) = self.address.borrow().edit().map(Edit::shown) else {
            return;
        };
        if self.address_text() != shown.text {
            let _ = self.ui.address.SetText(&shown.text);
        }
        if let Some(text_box) = self.address_text_box() {
            let _ = text_box.Select(
                i32::try_from(shown.selection_start).unwrap_or(i32::MAX),
                i32::try_from(shown.selection_length).unwrap_or(0),
            );
        }
        self.show_highlight();
    }

    /// The list inside the box's template takes each new set of items a moment after they are
    /// set, without a highlight: the edit's row is highlighted again as the rows fill.
    pub(super) fn watch_suggestion_list(&self) {
        let list = self
            .suggestion_list()
            .and_then(|list| list.cast::<ListViewBase>().ok());
        let Some(list) = list else {
            log::warn!("the address box has no suggestion list");
            return;
        };
        let me = self.me.clone();
        match list.ContainerContentChanging(move |_, _| with(&me, BrowserWindow::show_highlight)) {
            Ok(watch) => *self.suggestion_list_watch.borrow_mut() = Some(watch),
            Err(e) => log::warn!("suggestion list: {e}"),
        }
    }

    /// Highlights the edit's row in the list, once the list holds it.
    fn show_highlight(&self) {
        let Some(row) = self.address.borrow().edit().map(Edit::row) else {
            return;
        };
        let Some(list) = self.suggestion_list() else {
            return;
        };
        let held = list
            .cast::<ItemsControl>()
            .and_then(|l| l.Items())
            .and_then(|items| items.Size())
            .unwrap_or(0);
        let row = i32::try_from(row).unwrap_or(-1);
        if u32::try_from(row).is_ok_and(|r| r < held) && list.SelectedIndex() != Ok(row) {
            let _ = list.SetSelectedIndex(row);
        }
    }

    /// Anything that opens over the address box, or takes the focus from it, closes its list.
    /// The list is emptied too: a focused box with suggestions opens it again by itself, and
    /// only typing should. The engine is not asked, or its answer listed, for the text left.
    pub(super) fn close_suggestions(&self) {
        self.search_queries.cancel();
        let _ = self.ui.address.SetIsSuggestionListOpen(false);
        if let Address::Editing(edit) = &mut *self.address.borrow_mut() {
            edit.clear_rows();
        }
        self.fill_list();
    }

    pub fn suggestions_open(&self) -> bool {
        self.ui.address.IsSuggestionListOpen().unwrap_or(false)
    }

    /// The labels the suggestion list currently holds.
    pub fn suggestion_labels(&self) -> Vec<String> {
        self.address
            .borrow()
            .edit()
            .map(|edit| edit.items().iter().map(omnibox::label).collect())
            .unwrap_or_default()
    }

    /// The highlighted row of the open list, as the list itself has it.
    pub fn highlighted_suggestion(&self) -> Option<usize> {
        crate::dialogs::selected_index(&self.suggestion_list()?)
    }

    /// Each row's text for the box while it is highlighted.
    pub fn suggestion_fills(&self) -> Vec<String> {
        self.address
            .borrow()
            .edit()
            .map(|edit| edit.items().iter().map(|s| s.fill.clone()).collect())
            .unwrap_or_default()
    }

    /// The box's selected text range, in UTF-16 units: (start, length).
    pub fn address_selection(&self) -> Option<(usize, usize)> {
        let text_box = self.address_text_box()?;
        let start = usize::try_from(text_box.SelectionStart().ok()?).ok()?;
        let length = usize::try_from(text_box.SelectionLength().ok()?).ok()?;
        Some((start, length))
    }

    pub(super) fn address_text_box(&self) -> Option<TextBox> {
        let root = self.ui.address.cast::<DependencyObject>().ok()?;
        xaml::find_descendant(&root)
    }

    /// The keyboard focus is in the address box or in its list.
    pub(super) fn focus_in_address(&self) -> bool {
        let Ok(root) = self.xaml_root() else {
            return false;
        };
        let list = self.suggestion_list();
        let mut node = FocusManager::GetFocusedElementWithRoot(&root)
            .and_then(|f| f.cast::<DependencyObject>())
            .ok();
        while let Some(element) = node {
            if xaml::same_object(&element, &self.ui.address)
                || list
                    .as_ref()
                    .is_some_and(|l| xaml::same_object(&element, l))
            {
                return true;
            }
            node = VisualTreeHelper::GetParent(&element).ok();
        }
        false
    }

    /// The box template's list of suggestions, inside its popup.
    fn suggestion_list(&self) -> Option<Selector> {
        let root = self.ui.address.cast::<DependencyObject>().ok()?;
        let popup = xaml::find_descendant::<Popup>(&root)?;
        let child = popup.Child().ok()?.cast::<DependencyObject>().ok()?;
        match child.cast::<ListView>() {
            Ok(list) => list.cast().ok(),
            Err(_) => xaml::find_descendant::<ListView>(&child)?.cast().ok(),
        }
    }

    /// The box's `TextChanged`, which says `UserInput` for some of the shell's own writing too:
    /// only a focused box can be typed in. The list drops its highlight after any change of
    /// the text, the shell's writing included, so the highlight comes back once it has.
    pub(super) fn address_text_changed(&self, user_input: bool) {
        if self.address.borrow().written(&self.address_text()) {
            let me = self.me.clone();
            exec::spawn(async move { with(&me, BrowserWindow::show_highlight) });
        } else if user_input && self.address_focused.get() {
            self.address_changed();
        }
    }

    /// The box's text changed. Unless the shell wrote it, the user edited it, and the list
    /// follows; the text completes inline only after typing at the end, never after a deletion.
    pub fn address_changed(&self) {
        let text = self.address_text();
        if self.address.borrow().written(&text) {
            return;
        }
        let deleted = self.address_deleting.take();
        let caret_at_end = self.address_selection() == Some((text.encode_utf16().count(), 0));
        self.suggest_for(text, caret_at_end && !deleted);
    }

    /// A key pressed in the address box, before its text box sees it. True when the address
    /// box handled the key, which the text box then never gets.
    pub fn address_key_down(&self, vk: u16, mods: Mods) -> bool {
        self.address_deleting.set(omnibox::deletes(vk, mods));
        let list_open = self.suggestions_open() && !self.suggestion_labels().is_empty();
        match omnibox::key(vk, mods, list_open) {
            Some(Key::Accept) if !self.inline_selected() => false,
            Some(key) => self.address_key(key),
            None => false,
        }
    }

    /// The inline completion is still in the box, selected, as `show_edit` wrote it.
    fn inline_selected(&self) -> bool {
        let shown = self.address.borrow().edit().map(Edit::shown);
        shown.is_some_and(|shown| {
            shown.selection_length > 0
                && self.address_selection() == Some((shown.selection_start, shown.selection_length))
        })
    }

    fn address_key(&self, key: Key) -> bool {
        let outcome = match &mut *self.address.borrow_mut() {
            Address::Editing(edit) => edit.key(key),
            Address::Page(_) => match key {
                Key::Enter | Key::CtrlEnter => Outcome::Resolve,
                Key::Escape => Outcome::Revert,
                _ => Outcome::Pass,
            },
        };
        match outcome {
            Outcome::Pass => return false,
            Outcome::Show => self.show_edit(),
            Outcome::Open(url) => self.open_from_address(&url),
            Outcome::Resolve => self.address_submitted(&self.address_text()),
            Outcome::Revert => self.revert_address(),
            Outcome::Forget(url) => self.forget_suggestion(&url),
        }
        true
    }

    /// Escape: the list closes and the box shows the page's URL again, selected.
    fn revert_address(&self) {
        self.address.replace(Address::Page(String::new()));
        self.close_suggestions();
        self.refresh_chrome();
        if let Some(text_box) = self.address_text_box() {
            let _ = text_box.SelectAll();
        }
    }

    /// Shift+Delete on a history row: the URL leaves history, and the list is suggested again
    /// for the same text.
    fn forget_suggestion(&self, url: &Url) {
        let Some(browser) = self.browser() else {
            return;
        };
        browser.forget_visit(url);
        let Some((typed, inline)) = self
            .address
            .borrow()
            .edit()
            .map(|e| (e.typed().to_owned(), e.inline().is_some()))
        else {
            return;
        };
        let list = browser.suggest(&typed, inline);
        if let Address::Editing(edit) = &mut *self.address.borrow_mut() {
            edit.refill(list);
        }
        self.fill_list();
        self.show_edit();
    }
}
