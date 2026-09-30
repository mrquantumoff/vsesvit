//! Address-bar suggestions: core's omnibox (search, typed URL, bookmarks, history, and the
//! inline completion) first, then matching open tabs, which only the shell knows about.

use std::rc::Rc;

use adw::prelude::*;
use vsesvit_core::history::Transition;
use vsesvit_core::search::{NavTarget, SuggestionSource};

use crate::address_bar::{Suggestion, Suggestions};
use crate::browser::Browser;
use crate::tab::display_uri;
use crate::window::BrowserWindow;

const MAX_CORE_SUGGESTIONS: usize = 8;
const MAX_TAB_SUGGESTIONS: usize = 4;

pub(crate) fn suggestions(browser: &Browser, window: &BrowserWindow, text: &str, allow_inline: bool) -> Suggestions {
    let needle = text.trim();
    if needle.is_empty() {
        return Suggestions::default();
    }
    let from_core = browser.core().borrow_mut().omnibox().suggest(needle, MAX_CORE_SUGGESTIONS, allow_inline);
    let from_core = from_core.unwrap_or_else(|e| {
        log::warn!("omnibox: {e}");
        Default::default()
    });
    let mut rows: Vec<Suggestion> = from_core.items.into_iter().map(|s| row(window, s)).collect();
    rows.extend(open_tab_suggestions(browser, window, &needle.to_lowercase()));
    Suggestions { rows, inline: from_core.inline }
}

fn row(window: &BrowserWindow, suggestion: vsesvit_core::search::Suggestion) -> Suggestion {
    let url = suggestion.target.url().clone();
    let (icon_name, subtitle, transition) = match (&suggestion.source, &suggestion.target) {
        (SuggestionSource::Search, NavTarget::Search { .. }) => {
            ("system-search-symbolic", "Search".to_owned(), Transition::Typed)
        }
        (SuggestionSource::Search, NavTarget::Url(_)) | (SuggestionSource::Typed, _) => {
            ("web-browser-symbolic", display_uri(url.as_str()), Transition::Typed)
        }
        (SuggestionSource::Bookmark, _) => ("user-bookmarks-symbolic", display_uri(url.as_str()), Transition::Bookmark),
        (SuggestionSource::History, _) => ("document-open-recent-symbolic", display_uri(url.as_str()), Transition::Typed),
    };
    let forget = (suggestion.source == SuggestionSource::History).then(|| forget_visits(window, url.clone()));
    let window = window.downgrade();
    Suggestion {
        title: suggestion.title,
        subtitle,
        icon_name,
        fill: suggestion.fill,
        activate: Rc::new(move || {
            if let Some(window) = window.upgrade() {
                window.navigate_with(url.as_str(), transition);
            }
        }),
        forget,
    }
}

/// Deletes every visit to `url` from history.
fn forget_visits(window: &BrowserWindow, url: vsesvit_core::Url) -> Rc<dyn Fn()> {
    let window = window.downgrade();
    Rc::new(move || {
        let Some(window) = window.upgrade() else { return };
        if let Err(e) = window.browser().core().borrow_mut().history().delete_url(&url) {
            log::warn!("history: {e}");
        }
    })
}

fn open_tab_suggestions(browser: &Browser, current: &BrowserWindow, needle: &str) -> Vec<Suggestion> {
    let selected = current.selected_tab();
    browser
        .windows()
        .into_iter()
        .flat_map(|window| {
            window
                .tabs()
                .into_iter()
                .map(move |tab| (window.clone(), tab))
        })
        .filter(|(_, tab)| Some(tab) != selected.as_ref())
        .filter_map(|(window, tab)| {
            let uri = tab.committed_uri()?;
            let title = tab.display_title();
            let matches = title.to_lowercase().contains(needle) || uri.to_lowercase().contains(needle);
            let (window, tab) = (window.downgrade(), tab.downgrade());
            matches.then(|| Suggestion {
                title,
                subtitle: format!("Switch to tab · {}", display_uri(&uri)),
                icon_name: "view-paged-symbolic",
                fill: display_uri(&uri),
                activate: Rc::new(move || {
                    if let (Some(window), Some(tab)) = (window.upgrade(), tab.upgrade()) {
                        window.select_tab(&tab);
                        window.present();
                    }
                }),
                forget: None,
            })
        })
        .take(MAX_TAB_SUGGESTIONS)
        .collect()
}
