//! Tab search (Chrome's Ctrl+Shift+A): a popover with a search entry over the open tabs of
//! every window and the recently closed ones, of the window's kind only (normal or private),
//! listed again from core on every edit. The first
//! row is selected; Up and Down move the selection, wrapping around at the ends, Enter or a
//! click goes to the row's tab, and Escape closes the popover.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::{gdk, glib};
use vsesvit_core::tab_search::{Hit, Row};
use vsesvit_webext::TabId;

use super::BrowserWindow;
use crate::browser::TabHit;
use crate::closed_tabs::ClosedKey;

const WIDTH: i32 = 360;
/// How tall the list grows before it scrolls.
const MAX_LIST_HEIGHT: i32 = 440;

#[derive(Clone)]
pub(crate) struct TabSearch(Rc<Inner>);

struct Inner {
    window: glib::WeakRef<BrowserWindow>,
    popover: gtk::Popover,
    entry: gtk::SearchEntry,
    list: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    #[cfg_attr(not(feature = "self-test"), allow(dead_code))]
    placeholder: gtk::Label,
    /// What each row leads to, in the list's order; the headings are drawn from it.
    hits: Rc<RefCell<Vec<TabHit>>>,
}

impl TabSearch {
    /// The popover for `window`, listing every tab; [`TabSearch::open`] shows it.
    pub(super) fn new(window: &BrowserWindow) -> Self {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Search tabs")
            .build();
        let placeholder = gtk::Label::builder()
            .label("No results found")
            .css_classes(["dimmed"])
            .margin_top(12)
            .margin_bottom(12)
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["navigation-sidebar"])
            .build();
        list.set_placeholder(Some(&placeholder));
        let hits: Rc<RefCell<Vec<TabHit>>> = Rc::default();
        list.set_header_func(glib::clone!(
            #[strong]
            hits,
            move |row, before| {
                let hits = hits.borrow();
                let closed = |row: &gtk::ListBoxRow| {
                    usize::try_from(row.index()).ok().and_then(|i| hits.get(i)).map(|hit| matches!(hit, Hit::Closed(_)))
                };
                let here = closed(row);
                if before.and_then(closed) == here {
                    row.set_header(None::<&gtk::Widget>);
                } else {
                    row.set_header(Some(&heading(if here == Some(true) { "Recently closed" } else { "Open tabs" })));
                }
            }
        ));
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(MAX_LIST_HEIGHT)
            .child(&list)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.set_width_request(WIDTH);
        content.append(&entry);
        content.append(&scroller);
        let popover = gtk::Popover::builder()
            .child(&content)
            .position(gtk::PositionType::Bottom)
            .build();
        let search = TabSearch(Rc::new(Inner {
            window: window.downgrade(),
            popover,
            entry,
            list,
            scroller,
            placeholder,
            hits,
        }));
        search.connect_signals();
        search.refresh();
        search
    }

    fn connect_signals(&self) {
        let inner = &self.0;
        let on = |f: fn(&TabSearch)| {
            let weak: Weak<Inner> = Rc::downgrade(inner);
            move || {
                if let Some(inner) = weak.upgrade() {
                    f(&TabSearch(inner));
                }
            }
        };
        let refresh = on(TabSearch::refresh);
        inner.entry.connect_changed(move |_| refresh());
        let choose = on(|search| {
            if let Some(row) = search.0.list.selected_row() {
                search.choose(&row);
            }
        });
        inner.entry.connect_activate(move |_| choose());
        let close = on(TabSearch::close);
        inner.entry.connect_stop_search(move |_| close());

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (up, down) = (on(|search| search.step(-1)), on(|search| search.step(1)));
        keys.connect_key_pressed(move |_, key, _, _| match key {
            gdk::Key::Up | gdk::Key::KP_Up => {
                up();
                glib::Propagation::Stop
            }
            gdk::Key::Down | gdk::Key::KP_Down => {
                down();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        inner.entry.add_controller(keys);

        let weak = Rc::downgrade(inner);
        inner.list.connect_row_activated(move |_, row| {
            if let Some(inner) = weak.upgrade() {
                TabSearch(inner).choose(row);
            }
        });
    }

    /// Opens the popover on `anchor`, at `pointing_to` in it if given, typing into the entry.
    pub(super) fn open(&self, anchor: &gtk::Widget, pointing_to: Option<&gdk::Rectangle>) {
        self.0.popover.set_pointing_to(pointing_to);
        crate::popup(&self.0.popover, anchor);
        self.0.entry.grab_focus();
    }

    pub(super) fn close(&self) {
        self.0.popover.popdown();
    }

    pub(crate) fn popover(&self) -> &gtk::Popover {
        &self.0.popover
    }

    /// Lists the tabs for the entry's text, as live now, and selects the first.
    fn refresh(&self) {
        let Some(window) = self.0.window.upgrade() else { return };
        let browser = window.browser();
        let rows = browser.search_tabs(window.browsing(), &self.0.entry.text());
        let list = &self.0.list;
        list.remove_all();
        *self.0.hits.borrow_mut() = rows.iter().map(|row| row.hit).collect();
        for row in &rows {
            list.append(&list_row(row, browser.tab_icon(window.browsing(), row.hit)));
        }
        list.select_row(list.row_at_index(0).as_ref());
    }

    /// Moves the selection `by` rows, keeping it in view; past either end it wraps around, as
    /// Chrome's list does.
    pub(crate) fn step(&self, by: i32) {
        let list = &self.0.list;
        let count = i32::try_from(self.0.hits.borrow().len()).unwrap_or(i32::MAX);
        let Some(from) = list.selected_row().map(|row| row.index()).filter(|_| count > 0) else { return };
        let Some(row) = list.row_at_index((from + by).rem_euclid(count)) else { return };
        list.select_row(Some(&row));
        if let Some(bounds) = row.compute_bounds(list) {
            let (top, bottom) = (f64::from(bounds.y()), f64::from(bounds.y() + bounds.height()));
            self.0.scroller.vadjustment().clamp_page(top, bottom);
        }
    }

    /// Goes to `row`'s tab, closing the popover.
    fn choose(&self, row: &gtk::ListBoxRow) {
        let hit = usize::try_from(row.index()).ok().and_then(|i| self.0.hits.borrow().get(i).copied());
        let (Some(hit), Some(window)) = (hit, self.0.window.upgrade()) else { return };
        self.close();
        window.browser().go_to_tab(&window, hit);
    }
}

#[cfg(feature = "self-test")]
impl TabSearch {
    pub(crate) fn set_query(&self, text: &str) {
        self.0.entry.set_text(text);
    }

    /// The rows as shown: the heading over a row that starts a section, its title and site.
    pub(crate) fn shown(&self) -> Vec<(Option<String>, String, String)> {
        let mut shown = Vec::new();
        let mut at = 0;
        while let Some(row) = self.0.list.row_at_index(at) {
            let heading = row.header().and_downcast::<gtk::Label>().map(|label| label.label().into());
            if let Ok(row) = row.downcast::<adw::ActionRow>() {
                shown.push((heading, row.title().into(), row.subtitle().map(String::from).unwrap_or_default()));
            }
            at += 1;
        }
        shown
    }

    pub(crate) fn selected(&self) -> Option<i32> {
        self.0.list.selected_row().map(|row| row.index())
    }

    pub(crate) fn says_no_results(&self) -> bool {
        self.0.placeholder.is_child_visible()
    }

    /// Clicks the row at `index`. False when there is none.
    pub(crate) fn click(&self, index: i32) -> bool {
        self.0.list.row_at_index(index).is_some_and(|row| row.activate())
    }

    pub(crate) fn press_enter(&self) {
        self.0.entry.emit_activate();
    }

    pub(crate) fn is_open(&self) -> bool {
        self.0.popover.is_visible()
    }

    pub(crate) fn anchor(&self) -> Option<gtk::Widget> {
        self.0.popover.parent()
    }
}

fn heading(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(["heading"])
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(6)
        .build()
}

fn list_row(row: &Row<TabId, ClosedKey>, icon: Option<gdk::Texture>) -> adw::ActionRow {
    let image = match icon {
        Some(icon) => gtk::Image::from_paintable(Some(&icon)),
        None => gtk::Image::from_icon_name("web-browser-symbolic"),
    };
    image.set_pixel_size(16);
    let item = adw::ActionRow::builder()
        .title(&row.title)
        .subtitle(&row.site)
        .use_markup(false)
        .title_lines(1)
        .subtitle_lines(1)
        .activatable(true)
        .build();
    item.add_prefix(&image);
    item
}
