//! The History window: recent visits or a search over core's history, opening a page in a
//! new tab, forgetting one URL, and clearing a time range (which writes a synced
//! deletion directive). Above the visits, Chrome's "Tabs from other devices" lists what the
//! devices syncing with this one have open.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use vsesvit_core::Url;
use vsesvit_core::history::{ClearRange, HistoryEntry};
use vsesvit_core::session::TabSnapshot;

use super::{LibraryWindow, Windowed, format_time, prompt_choice};
use crate::browser::Browser;
use crate::favicons;
use crate::session::now_ms;
use crate::tab::display_uri;
use crate::window::{BrowserWindow, Focus};

const RECENT_LIMIT: usize = 300;
const SEARCH_LIMIT: usize = 100;

struct Row {
    entry: HistoryEntry,
    /// The visit time when listing recent visits; the last visit when searching.
    at_ms: i64,
}

/// Holds no [`Browser`] or profile of its own, which the window must not keep open. The
/// window holds it until it is destroyed; the widgets' handlers hold it weakly.
struct State {
    window: glib::WeakRef<BrowserWindow>,
    ui: LibraryWindow,
    list: gtk::ListBox,
    stack: gtk::Stack,
    devices: gtk::Box,
    /// Tells the visits from the devices' tabs; hidden with them while searching.
    visits_heading: gtk::Label,
    /// Kept for [`Browser::watch_history`], which holds it weakly.
    watch: Rc<dyn Fn()>,
}

pub(crate) fn present(window: &BrowserWindow) {
    super::present_window(window, Windowed::History, || build(window));
}

fn build(window: &BrowserWindow) -> adw::Window {
    let clear = gtk::Button::builder()
        .label("_Clear…")
        .use_underline(true)
        .tooltip_text("Clear browsing history")
        .css_classes(["destructive-action"])
        .build();
    let ui = LibraryWindow::new("History", "Search history", &[clear.upcast_ref()]);

    let list = super::boxed_list();
    let empty = adw::StatusPage::builder()
        .icon_name("document-open-recent-symbolic")
        .title("No History")
        .description("Pages you visit appear here")
        .build();
    let stack = gtk::Stack::builder().vhomogeneous(false).build();
    stack.add_named(&list, Some("list"));
    stack.add_named(&empty, Some("empty"));
    let devices = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(12)
        .build();
    let visits_heading = gtk::Label::builder()
        .label("Recent History")
        .xalign(0.0)
        .css_classes(["heading"])
        .margin_start(18)
        .margin_top(6)
        .build();
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.append(&devices);
    column.append(&visits_heading);
    column.append(&stack);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&column)
        .vexpand(true)
        .build();
    ui.content.set_child(Some(&scroller));

    let state = Rc::new_cyclic(|weak: &Weak<State>| {
        let weak = weak.clone();
        State {
            window: window.downgrade(),
            ui,
            list,
            stack,
            devices,
            visits_heading,
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
        #[weak]
        state,
        move |_| state.refresh()
    ));
    clear.connect_clicked(glib::clone!(
        #[weak]
        state,
        move |_| {
            let state = state.clone();
            glib::spawn_future_local(async move { state.clear().await });
        }
    ));
    let owner = RefCell::new(Some(state.clone()));
    state.ui.window.connect_unrealize(move |_| drop(owner.take()));
    state.ui.window.clone()
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
        self.refresh_devices();
        let rows = self.load();
        self.list.remove_all();
        let page = if rows.is_empty() { "empty" } else { "list" };
        for row in rows {
            self.list.append(&self.row_widget(row));
        }
        self.stack.set_visible_child_name(page);
    }

    /// Each other device with when it last synced, then its tabs. Hidden while searching, which
    /// searches the visits only.
    fn refresh_devices(&self) {
        while let Some(child) = self.devices.first_child() {
            self.devices.remove(&child);
        }
        let shown = self.ui.search.text().trim().is_empty();
        self.devices.set_visible(shown);
        self.visits_heading.set_visible(shown);
        let Some(browser) = self.browser().filter(|_| shown) else { return };
        let signed_in = browser.sync().is_signed_in();
        let devices = if signed_in {
            browser.core().borrow_mut().session().other_devices().unwrap_or_else(|e| {
                log::warn!("tabs from other devices: {e}");
                Vec::new()
            })
        } else {
            Vec::new()
        };
        let heading = adw::PreferencesGroup::builder().title("Tabs from Other Devices").build();
        if !signed_in {
            heading.set_description(Some("Sign in to sync to see tabs from your other devices."));
        } else if devices.is_empty() {
            heading.set_description(Some("Tabs from your other devices appear here when they sync."));
        }
        self.devices.append(&heading);
        let now = now_ms();
        for device in devices {
            let group = adw::PreferencesGroup::builder()
                .title(&device.device_name)
                .description(synced_ago(now - device.updated_ms))
                .build();
            for tab in device.windows.iter().flat_map(|w| &w.tabs) {
                group.add(&self.device_tab_row(&browser, tab));
            }
            self.devices.append(&group);
        }
    }

    fn device_tab_row(&self, browser: &Browser, tab: &TabSnapshot) -> adw::ActionRow {
        let title = if tab.title.trim().is_empty() { display_uri(tab.url.as_str()) } else { tab.title.clone() };
        let row = adw::ActionRow::builder()
            .title(&title)
            .tooltip_text(tab.url.as_str())
            .activatable(true)
            .use_markup(false)
            .build();
        let icon = favicons::stored(&mut browser.core().borrow_mut(), &tab.url);
        row.add_prefix(&favicons::image(icon.as_ref(), "web-browser-symbolic"));
        let (window, url) = (self.window.clone(), tab.url.to_string());
        row.connect_activated(move |_| {
            if let Some(window) = window.upgrade() {
                window.open_tab(Some(&url), None, Focus::Foreground);
            }
        });
        row
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
        let forget = super::row_button("user-trash-symbolic", "Forget this page");
        forget.connect_clicked(glib::clone!(
            #[weak(rename_to = state)]
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
        let names = ClearRange::ALL.map(ClearRange::label);
        let Some(index) = prompt_choice(
            &self.ui.window,
            "Clear Browsing History",
            "Visits in the chosen range are forgotten on this device and on every synced device.",
            &names,
            "_Clear",
        )
        .await
        else {
            return;
        };
        let Some(range) = ClearRange::ALL.get(index as usize) else { return };
        let now = now_ms();
        let Some(browser) = self.browser() else { return };
        let result = browser.core().borrow_mut().history().delete_range(range.start(now), now);
        if let Err(e) = result {
            self.ui.toast(&format!("History: {e}"));
        }
        self.refresh();
    }
}

/// When another device's tabs were last synced, for its heading.
fn synced_ago(ms: i64) -> String {
    format!("Synced {}", vsesvit_sync::status::ago(u64::try_from(ms.max(0) / 1000).unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{browser, wait_until};

    #[test]
    fn a_device_says_how_long_ago_it_synced() {
        assert_eq!(synced_ago(-5), "Synced just now");
        assert_eq!(synced_ago(59_000), "Synced just now");
        assert_eq!(synced_ago(60_000), "Synced 1 minute ago");
        assert_eq!(synced_ago(5 * 60_000), "Synced 5 minutes ago");
        assert_eq!(synced_ago(2 * 3_600_000), "Synced 2 hours ago");
        assert_eq!(synced_ago(3 * 86_400_000), "Synced 3 days ago");
    }

    #[gtk::test]
    fn a_closed_window_goes() {
        let opener = BrowserWindow::new(&browser());
        let window = build(&opener);
        let weak = window.downgrade();
        window.present();
        window.close();
        drop(window);
        wait_until("the window to go", || weak.upgrade().is_none());
        opener.destroy();
    }
}
