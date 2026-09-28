//! The Settings dialog, bound to core's preferences: startup behaviour and homepage, tab
//! position (applied to every window at once), default search engine, theme (through
//! `AdwStyleManager`), the bookmarks bar, and where downloads go.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::prefs::{Startup, TabsPosition, Theme, keys};
use vsesvit_core::search::SearchEngine;

use super::confirm;
use crate::browser::Browser;
use crate::session::now_ms;
use crate::window::BrowserWindow;

const STARTUPS: [(Startup, &str); 3] = [
    (Startup::RestoreSession, "Restore the previous session"),
    (Startup::Homepage, "Open the homepage"),
    (Startup::NewTab, "Open a new tab"),
];

const POSITIONS: [(TabsPosition, &str); 3] = [
    (TabsPosition::Left, "Left sidebar"),
    (TabsPosition::Right, "Right sidebar"),
    (TabsPosition::Top, "Top bar"),
];

const THEMES: [(Theme, &str); 3] = [
    (Theme::System, "Follow the system"),
    (Theme::Light, "Light"),
    (Theme::Dark, "Dark"),
];

pub(crate) fn present(window: &BrowserWindow) {
    let dialog = adw::PreferencesDialog::builder().title("Settings").build();
    dialog.add(&general_page(window));
    dialog.add(&privacy_page(window));
    dialog.present(Some(window));
}

fn general_page(window: &BrowserWindow) -> adw::PreferencesPage {
    let browser = window.browser().clone();

    let startup = adw::PreferencesGroup::builder().title("Startup").build();
    startup.add(&startup_row(&browser));
    startup.add(&homepage_row(&browser));

    let tabs = adw::PreferencesGroup::builder().title("Tabs").build();
    tabs.add(&tabs_position_row(&browser));

    let search = adw::PreferencesGroup::builder().title("Search").build();
    search.add(&search_engine_row(&browser));

    let appearance = adw::PreferencesGroup::builder().title("Appearance").build();
    appearance.add(&theme_row(&browser));
    appearance.add(&bookmarks_bar_row(&browser));

    let files = adw::PreferencesGroup::builder().title("Downloads").build();
    files.add(&download_folder_row(window));
    files.add(&download_ask_row(&browser));

    let page = adw::PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();
    for group in [&startup, &tabs, &search, &appearance, &files] {
        page.add(group);
    }
    if let Some(updates) = updates_group(&browser) {
        page.add(&updates);
    }
    page
}

fn privacy_page(window: &BrowserWindow) -> adw::PreferencesPage {
    let clear = adw::ButtonRow::builder()
        .title("Clear Browsing Data…")
        .start_icon_name("user-trash-symbolic")
        .build();
    clear.connect_activated(glib::clone!(
        #[weak]
        window,
        move |row| {
            let row = row.clone();
            glib::spawn_future_local(async move {
                let ok = confirm(
                    &row,
                    "Clear Browsing Data?",
                    "History on every synced device, and this device's cookies, site storage and cache.",
                    "_Clear",
                )
                .await;
                if !ok {
                    return;
                }
                let browser = window.browser();
                let result = browser.core().borrow_mut().history().delete_range(0, now_ms());
                if let Err(e) = result {
                    window.toast(adw::Toast::new(&format!("History: {e}")));
                }
                if let Some(manager) = browser.engine().session().website_data_manager() {
                    manager.clear(
                        webkit::WebsiteDataTypes::ALL,
                        glib::TimeSpan::from_seconds(0),
                        None::<&gtk::gio::Cancellable>,
                        |result| {
                            if let Err(e) = result {
                                log::warn!("clearing website data: {e}");
                            }
                        },
                    );
                }
                window.toast(adw::Toast::new("Browsing data cleared"));
            });
        }
    ));
    let data = adw::PreferencesGroup::builder()
        .title("Browsing Data")
        .description("History, cookies and cached files")
        .build();
    data.add(&clear);
    let page = adw::PreferencesPage::builder()
        .title("Privacy")
        .icon_name("security-high-symbolic")
        .build();
    page.add(&data);
    page
}

fn startup_row(browser: &Browser) -> adw::ComboRow {
    let names: Vec<&str> = STARTUPS.iter().map(|(_, name)| *name).collect();
    let current = browser.core().borrow_mut().prefs().get(&keys::STARTUP);
    let row = adw::ComboRow::builder()
        .title("On Startup")
        .model(&gtk::StringList::new(&names))
        .selected(index_of(&STARTUPS, &current))
        .build();
    row.connect_selected_notify(glib::clone!(
        #[strong]
        browser,
        move |row| {
            if let Some((startup, _)) = STARTUPS.get(row.selected() as usize) {
                let set = browser.core().borrow_mut().prefs().set(&keys::STARTUP, startup);
                if let Err(e) = set {
                    log::warn!("prefs: {e}");
                }
            }
        }
    ));
    row
}

fn homepage_row(browser: &Browser) -> adw::EntryRow {
    let current = browser.core().borrow_mut().prefs().get(&keys::HOMEPAGE);
    let row = adw::EntryRow::builder()
        .title("Homepage")
        .text(&current)
        .show_apply_button(true)
        .input_purpose(gtk::InputPurpose::Url)
        .build();
    row.connect_apply(glib::clone!(
        #[strong]
        browser,
        move |row| {
            let text = row.text().trim().to_owned();
            let value = if text.is_empty() { "about:home".to_owned() } else { text };
            let set = browser.core().borrow_mut().prefs().set(&keys::HOMEPAGE, &value);
            if let Err(e) = set {
                log::warn!("prefs: {e}");
            }
        }
    ));
    row
}

fn tabs_position_row(browser: &Browser) -> adw::ComboRow {
    let names: Vec<&str> = POSITIONS.iter().map(|(_, name)| *name).collect();
    let row = adw::ComboRow::builder()
        .title("Tab Position")
        .subtitle("Vertical tabs in a sidebar, or a bar above the page")
        .model(&gtk::StringList::new(&names))
        .selected(index_of(&POSITIONS, &browser.tabs_position()))
        .build();
    row.connect_selected_notify(glib::clone!(
        #[strong]
        browser,
        move |row| {
            if let Some((position, _)) = POSITIONS.get(row.selected() as usize) {
                browser.set_tabs_position(*position);
            }
        }
    ));
    row
}

fn search_engine_row(browser: &Browser) -> adw::ComboRow {
    let (engines, default): (Vec<SearchEngine>, _) = {
        let mut profile = browser.core().borrow_mut();
        let mut engines = profile.search_engines();
        let list = engines.list().unwrap_or_default();
        let default = engines.default_engine().ok().map(|e| e.id);
        (list, default)
    };
    let names: Vec<&str> = engines.iter().map(|e| e.name.as_str()).collect();
    let selected = default
        .and_then(|id| engines.iter().position(|e| e.id == id))
        .unwrap_or(0);
    let row = adw::ComboRow::builder()
        .title("Search Engine")
        .subtitle("Used for words typed in the address bar")
        .model(&gtk::StringList::new(&names))
        .selected(u32::try_from(selected).unwrap_or(0))
        .build();
    row.connect_selected_notify(glib::clone!(
        #[strong]
        browser,
        move |row| {
            if let Some(engine) = engines.get(row.selected() as usize) {
                let set = browser.core().borrow_mut().search_engines().set_default(&engine.id);
                if let Err(e) = set {
                    log::warn!("search engines: {e}");
                }
            }
        }
    ));
    row
}

fn theme_row(browser: &Browser) -> adw::ComboRow {
    let names: Vec<&str> = THEMES.iter().map(|(_, name)| *name).collect();
    let row = adw::ComboRow::builder()
        .title("Theme")
        .model(&gtk::StringList::new(&names))
        .selected(index_of(&THEMES, &browser.theme()))
        .build();
    row.connect_selected_notify(glib::clone!(
        #[strong]
        browser,
        move |row| {
            if let Some((theme, _)) = THEMES.get(row.selected() as usize) {
                browser.set_theme(*theme);
            }
        }
    ));
    row
}

fn bookmarks_bar_row(browser: &Browser) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title("Show Bookmarks Bar")
        .active(browser.bookmarks_bar_visible())
        .build();
    row.connect_active_notify(glib::clone!(
        #[strong]
        browser,
        move |row| browser.set_bookmarks_bar_visible(row.is_active())
    ));
    row
}

/// The effective folder, a folder picker, and a way back to the platform's Downloads
/// folder while another one is chosen.
fn download_folder_row(window: &BrowserWindow) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title("Download Folder")
        .subtitle_selectable(true)
        .build();
    let reset = gtk::Button::builder()
        .label("Use Default")
        .tooltip_text("Save to the Downloads folder")
        .valign(gtk::Align::Center)
        .build();
    let change = gtk::Button::builder()
        .label("Change…")
        .valign(gtk::Align::Center)
        .build();
    row.add_suffix(&reset);
    row.add_suffix(&change);
    let browser = window.browser().clone();
    let show = glib::clone!(
        #[strong]
        browser,
        #[weak]
        row,
        #[weak]
        reset,
        move || {
            row.set_subtitle(&browser.downloads().directory().display().to_string());
            let custom = browser.core().borrow_mut().prefs().get(&keys::DOWNLOADS_DIR).is_some();
            reset.set_visible(custom);
        }
    );
    show();
    let show = Rc::new(show);
    change.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[strong]
        show,
        move |_| {
            let show = show.clone();
            let dialog = gtk::FileDialog::builder()
                .title("Download Folder")
                .initial_folder(&gio::File::for_path(window.browser().downloads().directory()))
                .modal(true)
                .build();
            glib::spawn_future_local(async move {
                let Ok(folder) = dialog.select_folder_future(Some(&window)).await else { return };
                let Some(path) = folder.path() else { return };
                let set = window.browser().core().borrow_mut().prefs().set(&keys::DOWNLOADS_DIR, &Some(path));
                if let Err(e) = set {
                    log::warn!("prefs: {e}");
                }
                show();
            });
        }
    ));
    reset.connect_clicked(move |_| {
        let reset = browser.core().borrow_mut().prefs().reset(&keys::DOWNLOADS_DIR);
        if let Err(e) = reset {
            log::warn!("prefs: {e}");
        }
        show();
    });
    row
}

fn download_ask_row(browser: &Browser) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title("Ask Where to Save Each File")
        .active(browser.core().borrow_mut().prefs().get(&keys::DOWNLOADS_ASK))
        .build();
    row.connect_active_notify(glib::clone!(
        #[strong]
        browser,
        move |row| {
            let set = browser.core().borrow_mut().prefs().set(&keys::DOWNLOADS_ASK, &row.is_active());
            if let Err(e) = set {
                log::warn!("prefs: {e}");
            }
        }
    ));
    row
}

/// Only for a copy that updates itself; a package from a distribution or Flatpak has no switch.
fn updates_group(browser: &Browser) -> Option<adw::PreferencesGroup> {
    browser.updates()?;
    let row = adw::SwitchRow::builder()
        .title("Automatic Updates")
        .subtitle("Download new versions of Vsesvit in the background")
        .active(browser.updates_automatic())
        .build();
    row.connect_active_notify(glib::clone!(
        #[strong]
        browser,
        move |row| browser.set_updates_automatic(row.is_active())
    ));
    let group = adw::PreferencesGroup::builder().title("Updates").build();
    group.add(&row);
    Some(group)
}

fn index_of<T: PartialEq, const N: usize>(options: &[(T, &str); N], value: &T) -> u32 {
    options
        .iter()
        .position(|(v, _)| v == value)
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0)
}
