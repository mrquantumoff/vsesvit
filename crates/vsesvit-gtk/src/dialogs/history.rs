//! The History dialog: recent visits or a search over core's history, opening a page in a
//! new tab, forgetting one URL, and clearing a time range (which writes a synced
//! deletion directive).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use vsesvit_core::Url;
use vsesvit_core::history::HistoryEntry;

use super::{LibraryDialog, format_time, prompt_choice};
use crate::browser::Browser;
use crate::session::now_ms;
use crate::tab::display_uri;
use crate::window::{BrowserWindow, Focus};

const RECENT_LIMIT: usize = 300;
const SEARCH_LIMIT: usize = 100;
const HOUR_MS: i64 = 60 * 60 * 1000;

/// The ranges offered by "Clear…", in display order.
const RANGES: [(&str, Option<i64>); 4] = [
    ("Last hour", Some(HOUR_MS)),
    ("Last 24 hours", Some(24 * HOUR_MS)),
    ("Last 7 days", Some(7 * 24 * HOUR_MS)),
    ("All time", None),
];

struct Row {
    entry: HistoryEntry,
    /// The visit time when listing recent visits; the last visit when searching.
    at_ms: i64,
}

/// Holds no [`Browser`] or profile of its own: the widgets' handlers keep this state alive
/// for as long as the dialog's widgets exist, which must not keep the profile open.
struct State {
    window: glib::WeakRef<BrowserWindow>,
    ui: LibraryDialog,
    list: gtk::ListBox,
    stack: gtk::Stack,
    rows: RefCell<Vec<gtk::Widget>>,
    /// Kept for [`Browser::watch_history`], which holds it weakly.
    watch: Rc<dyn Fn()>,
}

pub(crate) fn present(window: &BrowserWindow) {
    let clear = gtk::Button::builder()
        .label("_Clear…")
        .use_underline(true)
        .tooltip_text("Clear browsing history")
        .css_classes(["destructive-action"])
        .build();
    let ui = LibraryDialog::new("History", "Search history", &[clear.upcast_ref()]);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(12)
        .valign(gtk::Align::Start)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .vexpand(true)
        .build();
    let empty = adw::StatusPage::builder()
        .icon_name("document-open-recent-symbolic")
        .title("No History")
        .description("Pages you visit appear here")
        .build();
    let stack = gtk::Stack::new();
    stack.add_named(&scroller, Some("list"));
    stack.add_named(&empty, Some("empty"));
    ui.content.set_child(Some(&stack));

    let state = Rc::new_cyclic(|weak: &Weak<State>| {
        let weak = weak.clone();
        State {
            window: window.downgrade(),
            ui,
            list,
            stack,
            rows: RefCell::new(Vec::new()),
            watch: Rc::new(move || {
                if let Some(state) = weak.upgrade() {
                    state.refresh();
                }
            }),
        }
    });
    window.browser().watch_history(&state.watch);
    state.refresh();

    state.ui.search.connect_search_changed(glib::clone!(
        #[strong]
        state,
        move |_| state.refresh()
    ));
    clear.connect_clicked(glib::clone!(
        #[strong]
        state,
        move |_| {
            let state = state.clone();
            glib::spawn_future_local(async move { state.clear().await });
        }
    ));
    state.ui.dialog.present(Some(window));
}

impl State {
    fn browser(&self) -> Option<Browser> {
        self.window.upgrade().map(|window| window.browser().clone())
    }

    fn load(&self) -> Vec<Row> {
        let query = self.ui.search.text();
        let query = query.trim();
        let Some(browser) = self.browser() else { return Vec::new() };
        let mut profile = browser.core().borrow_mut();
        let mut history = profile.history();
        let loaded = if query.is_empty() {
            history
                .visits_between(0, now_ms() + 1, RECENT_LIMIT)
                .map(|visits| visits.into_iter().map(|(entry, visit)| Row { entry, at_ms: visit.at_ms }).collect())
        } else {
            history
                .search(query, SEARCH_LIMIT)
                .map(|entries| entries.into_iter().map(|entry| Row { at_ms: entry.last_visit_ms, entry }).collect())
        };
        loaded.unwrap_or_else(|e| {
            log::warn!("history: {e}");
            Vec::new()
        })
    }

    fn refresh(self: &Rc<Self>) {
        let rows = self.load();
        for row in self.rows.take() {
            self.list.remove(&row);
        }
        let mut widgets = Vec::with_capacity(rows.len());
        for row in rows {
            let widget = self.row_widget(row);
            self.list.append(&widget);
            widgets.push(widget.upcast());
        }
        let page = if widgets.is_empty() { "empty" } else { "list" };
        self.rows.replace(widgets);
        self.stack.set_visible_child_name(page);
    }

    fn row_widget(self: &Rc<Self>, row: Row) -> adw::ActionRow {
        let url = row.entry.url;
        let title = if row.entry.title.trim().is_empty() { display_uri(url.as_str()) } else { row.entry.title };
        let widget = adw::ActionRow::builder()
            .title(&title)
            .subtitle(format!("{} · {}", format_time(row.at_ms), display_uri(url.as_str())))
            .activatable(true)
            .use_markup(false)
            .build();
        widget.add_prefix(&gtk::Image::from_icon_name("web-browser-symbolic"));
        let forget = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Forget this page")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        forget.connect_clicked(glib::clone!(
            #[strong(rename_to = state)]
            self,
            #[strong]
            url,
            move |_| state.forget(&url)
        ));
        widget.add_suffix(&forget);
        let window = self.window.clone();
        widget.connect_activated(move |_| {
            if let Some(window) = window.upgrade() {
                window.open_tab(Some(url.as_str()), None, Focus::Foreground);
            }
        });
        widget
    }

    fn forget(self: &Rc<Self>, url: &Url) {
        let Some(browser) = self.browser() else { return };
        let result = browser.core().borrow_mut().history().delete_url(url);
        if let Err(e) = result {
            self.ui.toast(&format!("History: {e}"));
        }
        self.refresh();
    }

    async fn clear(self: Rc<Self>) {
        let names: Vec<&str> = RANGES.iter().map(|(name, _)| *name).collect();
        let Some(index) = prompt_choice(
            &self.ui.dialog,
            "Clear Browsing History",
            "Visits in the chosen range are forgotten on this device and on every synced device.",
            &names,
            "_Clear",
        )
        .await
        else {
            return;
        };
        let Some((_, span)) = RANGES.get(index as usize) else { return };
        let now = now_ms();
        let from = span.map_or(0, |span| now - span);
        let Some(browser) = self.browser() else { return };
        let result = browser.core().borrow_mut().history().delete_range(from, now);
        if let Err(e) = result {
            self.ui.toast(&format!("History: {e}"));
        }
        self.refresh();
    }
}
