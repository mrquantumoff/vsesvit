//! Find in page, driving the selected tab's WebKit `FindController`.

use std::rc::{Rc, Weak};

use gtk::{glib, prelude::*};
use webkit::prelude::*;

use crate::tab::FindResult;

const MAX_MATCHES: u32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Next,
    Previous,
}

#[derive(Clone)]
pub(crate) struct FindBar(Rc<Inner>);

struct Inner {
    bar: gtk::SearchBar,
    entry: gtk::SearchEntry,
    status: gtk::Label,
    target: glib::WeakRef<webkit::WebView>,
}

impl FindBar {
    pub(crate) fn new() -> Self {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Find in page")
            .width_chars(28)
            .build();
        let previous = gtk::Button::builder()
            .icon_name("go-up-symbolic")
            .tooltip_text("Previous Match")
            .build();
        let next = gtk::Button::builder()
            .icon_name("go-down-symbolic")
            .tooltip_text("Next Match")
            .build();
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        buttons.add_css_class("linked");
        buttons.append(&previous);
        buttons.append(&next);
        let status = gtk::Label::builder()
            .css_classes(["dim-label", "numeric"])
            .build();
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        content.append(&entry);
        content.append(&buttons);
        content.append(&status);
        let bar = gtk::SearchBar::builder()
            .child(&content)
            .show_close_button(true)
            .build();
        bar.connect_entry(&entry);

        let find = FindBar(Rc::new(Inner {
            bar,
            entry,
            status,
            target: glib::WeakRef::new(),
        }));
        let weak = Rc::downgrade(&find.0);
        let on = move |f: fn(&FindBar)| {
            let weak: Weak<Inner> = weak.clone();
            move || {
                if let Some(inner) = weak.upgrade() {
                    f(&FindBar(inner));
                }
            }
        };
        let search = on(FindBar::search);
        find.0.entry.connect_search_changed(move |_| search());
        let step_next = on(|f| f.step(Direction::Next));
        find.0.entry.connect_activate(move |_| step_next());
        let step_next = on(|f| f.step(Direction::Next));
        find.0.entry.connect_next_match(move |_| step_next());
        let step_previous = on(|f| f.step(Direction::Previous));
        find.0
            .entry
            .connect_previous_match(move |_| step_previous());
        let step_next = on(|f| f.step(Direction::Next));
        next.connect_clicked(move |_| step_next());
        let step_previous = on(|f| f.step(Direction::Previous));
        previous.connect_clicked(move |_| step_previous());
        let closed = on(FindBar::closed);
        find.0.bar.connect_search_mode_enabled_notify(move |bar| {
            if !bar.is_search_mode() {
                closed();
            }
        });
        find
    }

    pub(crate) fn widget(&self) -> &gtk::SearchBar {
        &self.0.bar
    }

    pub(crate) fn open(&self, target: &webkit::WebView) {
        self.0.target.set(Some(target));
        self.0.bar.set_search_mode(true);
        self.0.entry.grab_focus();
        self.0.entry.select_region(0, -1);
        self.search();
    }

    /// Follows the selected tab while open.
    pub(crate) fn retarget(&self, target: &webkit::WebView) {
        if self.0.target.upgrade().as_ref() == Some(target) {
            return;
        }
        self.finish();
        self.0.target.set(Some(target));
        if self.0.bar.is_search_mode() {
            self.search();
        }
    }

    pub(crate) fn step(&self, direction: Direction) {
        let Some(controller) = self.controller() else {
            return;
        };
        if !self.0.bar.is_search_mode() || self.0.entry.text().is_empty() {
            return;
        }
        match direction {
            Direction::Next => controller.search_next(),
            Direction::Previous => controller.search_previous(),
        }
    }

    pub(crate) fn show_result(&self, result: FindResult) {
        let (text, found) = status(result);
        let empty = self.0.entry.text().is_empty();
        self.0.status.set_label(if empty { "" } else { &text });
        if found || empty {
            self.0.entry.remove_css_class("error");
        } else {
            self.0.entry.add_css_class("error");
        }
    }

    fn controller(&self) -> Option<webkit::FindController> {
        self.0
            .target
            .upgrade()
            .and_then(|view| view.find_controller())
    }

    fn search(&self) {
        let Some(controller) = self.controller() else {
            return;
        };
        let text = self.0.entry.text();
        if text.is_empty() {
            controller.search_finish();
            self.show_result(FindResult::Matches(0));
            return;
        }
        let options =
            (webkit::FindOptions::CASE_INSENSITIVE | webkit::FindOptions::WRAP_AROUND).bits();
        controller.count_matches(&text, options, MAX_MATCHES);
        controller.search(&text, options, MAX_MATCHES);
    }

    fn finish(&self) {
        if let Some(controller) = self.controller() {
            controller.search_finish();
        }
    }

    fn closed(&self) {
        self.finish();
        self.0.status.set_label("");
        self.0.entry.remove_css_class("error");
        if let Some(view) = self.0.target.upgrade() {
            view.grab_focus();
        }
    }
}

/// The status text for `result`, and whether anything was found. WebKit reports more than
/// [`MAX_MATCHES`] as `u32::MAX`.
fn status(result: FindResult) -> (String, bool) {
    match result {
        FindResult::Matches(1) => ("1 match".to_owned(), true),
        FindResult::Matches(n) if n > MAX_MATCHES => {
            (format!("More than {MAX_MATCHES} matches"), true)
        }
        FindResult::Matches(n) => (format!("{n} matches"), n > 0),
        FindResult::NotFound => ("No matches".to_owned(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_counts_up_to_the_most_webkit_counts() {
        assert_eq!(status(FindResult::Matches(0)), ("0 matches".to_owned(), false));
        assert_eq!(status(FindResult::Matches(1)), ("1 match".to_owned(), true));
        assert_eq!(status(FindResult::Matches(MAX_MATCHES)), ("1000 matches".to_owned(), true));
        assert_eq!(status(FindResult::Matches(u32::MAX)), ("More than 1000 matches".to_owned(), true));
        assert_eq!(status(FindResult::NotFound), ("No matches".to_owned(), false));
    }
}
