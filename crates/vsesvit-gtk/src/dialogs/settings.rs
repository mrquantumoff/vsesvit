//! The Settings dialog, bound to core's preferences: General (startup, downloads, scrolling and
//! the GPU, updates, the profile folder), Sync (the account, what it syncs and its server),
//! Appearance (theme, tabs, bars and buttons), Search (the engine, the address bar and what it
//! suggests), Privacy (pop-ups, site permissions, browsing data) and Shortcuts
//! (`shortcut_settings`). Every change applies at once, in every window, and an open dialog
//! follows what sync changes.
//!
//! WebKitGTK keeps no passwords and fills no forms, so `autofill.*` has no rows here.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::permissions::{Origin, Permission, Setting, SiteSetting};
use vsesvit_core::prefs::{Pref, Startup, TabsPosition, Theme, UpdateChannel, keys};
use vsesvit_core::search::SearchEngine;
use vsesvit_core::sync::DataType;
use vsesvit_sync::status::{Action, DELETE_CONFIRMATION, State};

use super::confirm;
use crate::browser::Browser;
use crate::permissions;
use crate::session::now_ms;
use crate::sync::Syncer;
use crate::updates::{Status, StatusButton, Updates};
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

/// Adwaita has no symbolic sync icon, so its reload arrows stand in, here and in the welcome.
pub(crate) const SYNC_ICON: &str = "view-refresh-symbolic";

/// The account row's buttons, in the order each state lists the ones it shows. Deleting the data
/// on the server has a row of its own, apart from the everyday buttons.
const SYNC_ACTIONS: [Action; 4] = [Action::SignIn, Action::Cancel, Action::SyncNow, Action::SignOut];

const CHANNELS: [(UpdateChannel, &str); 4] = [
    (UpdateChannel::Stable, "Stable"),
    (UpdateChannel::Beta, "Beta"),
    (UpdateChannel::Weekly, "Weekly"),
    (UpdateChannel::Nightly, "Nightly"),
];

pub(crate) fn present(window: &BrowserWindow) {
    // Wide enough that the six page names fit in the header rather than a bar at the bottom.
    let dialog = adw::PreferencesDialog::builder()
        .title("Settings")
        .content_width(1040)
        .build();
    dialog.add(&general_page(window));
    dialog.add(&sync_page(window.browser()));
    dialog.add(&appearance_page(window.browser()));
    dialog.add(&search_page(window.browser()));
    dialog.add(&privacy_page(window));
    dialog.add(&super::shortcut_settings::page(window.browser()));
    dialog.present(Some(window));
}

/// `name` is what `AdwPreferencesDialog:visible-page-name` selects it by.
fn page(name: &str, title: &str, icon: &str, groups: &[adw::PreferencesGroup]) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .name(name)
        .title(title)
        .icon_name(icon)
        .build();
    for group in groups {
        page.add(group);
    }
    page
}

/// An empty `title` for a group about the page's own subject, which the page already names.
fn group(title: &str) -> adw::PreferencesGroup {
    adw::PreferencesGroup::builder().title(title).build()
}

fn sync_page(browser: &Browser) -> adw::PreferencesPage {
    let account = group("");
    account.add(&sync_account_row(browser.sync()));
    let server = group("");
    server.add(&sync_server_row(browser, &server));
    let groups = [account, sync_types_group(browser), server, delete_server_data_group(browser.sync())];
    page("sync", "Sync", SYNC_ICON, &groups)
}

/// Chrome's "Customize sync": everything, or the data types chosen one by one. The switches'
/// handlers hold it, so it holds the switches weakly.
struct SyncTypes {
    browser: Browser,
    everything: glib::WeakRef<adw::SwitchRow>,
    rows: Vec<(DataType, glib::WeakRef<adw::SwitchRow>)>,
    /// "Sync Everything" was turned off while every type is still chosen.
    customizing: Cell<bool>,
}

impl SyncTypes {
    fn chosen(&self) -> Vec<DataType> {
        self.browser.core().borrow_mut().prefs().get(&keys::SYNC_TYPES)
    }

    fn show(&self) {
        let chosen = self.chosen();
        let everything = !self.customizing.get() && every_type(&chosen);
        if let Some(row) = self.everything.upgrade() {
            row.set_active(everything);
        }
        for (data_type, row) in &self.rows {
            if let Some(row) = row.upgrade() {
                row.set_active(everything || chosen.contains(data_type));
                row.set_sensitive(!everything);
            }
        }
    }

    fn choose(&self, types: &[DataType]) {
        if self.chosen() == types {
            return;
        }
        if let Err(e) = self.browser.core().borrow_mut().prefs().set(&keys::SYNC_TYPES, &types.to_vec()) {
            log::warn!("prefs: {e}");
        }
        self.browser.sync().types_changed();
    }
}

fn sync_types_group(browser: &Browser) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Customize Sync")
        .description("What this device syncs")
        .build();
    let everything = adw::SwitchRow::builder().title("Sync Everything").build();
    group.add(&everything);
    let rows: Vec<(DataType, adw::SwitchRow)> = DataType::ALL
        .into_iter()
        .map(|data_type| {
            let row = adw::SwitchRow::builder().title(data_type.label()).build();
            group.add(&row);
            (data_type, row)
        })
        .collect();
    let types = Rc::new(SyncTypes {
        browser: browser.clone(),
        everything: everything.downgrade(),
        rows: rows.iter().map(|(data_type, row)| (*data_type, row.downgrade())).collect(),
        customizing: Cell::new(false),
    });
    types.show();
    everything.connect_active_notify(glib::clone!(
        #[strong]
        types,
        move |row| {
            if row.is_active() {
                if types.customizing.replace(false) || !every_type(&types.chosen()) {
                    types.choose(&DataType::ALL);
                    types.show();
                }
            } else if !types.customizing.get() && every_type(&types.chosen()) {
                types.customizing.set(true);
                types.show();
            }
        }
    ));
    for (data_type, row) in rows {
        row.connect_active_notify(glib::clone!(
            #[strong]
            types,
            move |row| {
                let chosen = types.chosen();
                let next = toggled(&chosen, data_type, row.is_active());
                if row.is_sensitive() && next != chosen {
                    types.choose(&next);
                    types.show();
                }
            }
        ));
    }
    group
}

fn every_type(types: &[DataType]) -> bool {
    DataType::ALL.iter().all(|t| types.contains(t))
}

/// `types` with `data_type` turned on or off, in [`DataType::ALL`]'s order.
fn toggled(types: &[DataType], data_type: DataType, on: bool) -> Vec<DataType> {
    DataType::ALL
        .into_iter()
        .filter(|t| if *t == data_type { on } else { types.contains(t) })
        .collect()
}

/// Shown while signed in; asks first, and a failure leaves the profile signed in.
fn delete_server_data_group(syncer: &Syncer) -> adw::PreferencesGroup {
    let row = adw::ButtonRow::builder().title(Action::DeleteServerData.label()).build();
    row.add_css_class("destructive-action");
    row.connect_activated(glib::clone!(
        #[strong]
        syncer,
        move |row| {
            let (row, syncer) = (row.clone(), syncer.clone());
            glib::spawn_future_local(async move {
                let (title, body, accept) = DELETE_CONFIRMATION;
                if !confirm(&row, title, body, accept).await {
                    return;
                }
                row.set_sensitive(false);
                let deleted = syncer.delete_server_data().await;
                row.set_sensitive(true);
                if let Err(e) = deleted
                    && let Some(dialog) = row.ancestor(adw::PreferencesDialog::static_type()).and_downcast::<adw::PreferencesDialog>()
                {
                    dialog.add_toast(adw::Toast::new(&format!("Could not delete the data on the server: {e}")));
                }
            });
        }
    ));
    let group = group("");
    group.add(&row);
    syncer.watch(glib::clone!(
        #[weak]
        group,
        #[upgrade_or]
        false,
        move |state: &State| {
            group.set_visible(state.status(0).actions.contains(&Action::DeleteServerData));
            true
        }
    ));
    group
}

/// Who is signed in and how the last sync went, with the buttons the state offers, kept current
/// while the dialog is open.
fn sync_account_row(syncer: &Syncer) -> adw::ActionRow {
    let row = adw::ActionRow::builder().use_markup(false).build();
    let buttons: Vec<(Action, glib::WeakRef<gtk::Button>)> = SYNC_ACTIONS
        .into_iter()
        .map(|action| {
            let button = gtk::Button::builder()
                .label(action.label())
                .valign(gtk::Align::Center)
                .build();
            button.connect_clicked(glib::clone!(
                #[strong]
                syncer,
                move |_| syncer.act(action)
            ));
            row.add_suffix(&button);
            (action, button.downgrade())
        })
        .collect();
    syncer.watch(glib::clone!(
        #[weak]
        row,
        #[upgrade_or]
        false,
        move |state: &State| {
            let status = state.status(vsesvit_sync::now_secs());
            row.set_title(&status.title);
            row.set_subtitle(&status.subtitle);
            for (action, button) in &buttons {
                let Some(button) = button.upgrade() else { return false };
                button.set_visible(status.actions.contains(action));
                button.set_sensitive(!(status.busy && *action == Action::SyncNow));
                if status.actions.first() == Some(action) {
                    button.add_css_class("suggested-action");
                } else {
                    button.remove_css_class("suggested-action");
                }
            }
            true
        }
    ));
    row
}

/// The server to sign in to, which only changes while signed out. An address that is not one is
/// refused in `group`'s description, above the field. The welcome shows it too.
pub(crate) fn sync_server_row(browser: &Browser, group: &adw::PreferencesGroup) -> adw::EntryRow {
    let current = browser.core().borrow_mut().prefs().get(&keys::SYNC_SERVER);
    let row = adw::EntryRow::builder()
        .title("Sync Server")
        .text(&current)
        .show_apply_button(true)
        .input_purpose(gtk::InputPurpose::Url)
        .build();
    let show_error = glib::clone!(
        #[weak]
        row,
        #[weak]
        group,
        move |error: Option<&str>| {
            group.set_description(error);
            if error.is_some() {
                row.add_css_class("error");
            } else {
                row.remove_css_class("error");
            }
        }
    );
    row.connect_changed(glib::clone!(
        #[strong]
        show_error,
        move |_| show_error(None)
    ));
    row.connect_apply(glib::clone!(
        #[strong]
        browser,
        move |row| match vsesvit_sync::normalize_base_url(&row.text()) {
            Ok(server) => {
                let set = browser.core().borrow_mut().prefs().set(&keys::SYNC_SERVER, &server);
                if let Err(e) = set {
                    log::warn!("prefs: {e}");
                }
                if row.text() != server {
                    row.set_text(&server);
                }
            }
            Err(e) => show_error(Some(&e.to_string())),
        }
    ));
    browser.sync().watch(glib::clone!(
        #[weak]
        row,
        #[upgrade_or]
        false,
        move |state: &State| {
            row.set_sensitive(state.status(0).server_editable);
            true
        }
    ));
    row
}

fn general_page(window: &BrowserWindow) -> adw::PreferencesPage {
    let browser = window.browser();

    let startup = group("Startup");
    startup.add(&startup_row(browser));
    startup.add(&homepage_row(browser));

    let downloads = group("Downloads");
    downloads.add(&download_folder_row(window));
    downloads.add(&pref_switch_row(
        browser,
        "Ask Where to Save Each File",
        None,
        &keys::DOWNLOADS_ASK,
        Browser::set_switch,
    ));

    let system = group("System");
    system.add(&pref_switch_row(
        browser,
        "Smooth Scrolling",
        None,
        &keys::SMOOTH_SCROLLING,
        Browser::set_engine_switch,
    ));
    system.add(&pref_switch_row(
        browser,
        "Hardware Acceleration",
        Some("Use the graphics card to draw pages when available"),
        &keys::HARDWARE_ACCELERATION,
        Browser::set_engine_switch,
    ));
    system.add(&profile_folder_row(browser));

    let mut groups = vec![startup, downloads, system];
    // Only for a copy that updates itself; a package from a distribution or Flatpak has no group.
    if let Some(updates) = browser.updates() {
        let group = group("Updates");
        group.add(&update_status_row(updates));
        group.add(&update_channel_row(browser));
        group.add(&pref_switch_row(
            browser,
            "Automatic Updates",
            Some("Download new versions of Vsesvit in the background"),
            &keys::UPDATES_AUTOMATIC,
            |b, _, on| b.set_updates_automatic(on),
        ));
        groups.push(group);
    }
    page("general", "General", "preferences-system-symbolic", &groups)
}

/// What the updater is doing, kept current while the dialog is open, with a button to check
/// or, once the banner offers one, the banner's own button.
fn update_status_row(updates: &Updates) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .subtitle(format!("Version {}", env!("CARGO_PKG_VERSION")))
        .use_markup(false)
        .build();
    let check = gtk::Button::builder()
        .label("Check for Updates")
        .valign(gtk::Align::Center)
        .build();
    let update = gtk::Button::builder()
        .action_name("app.update")
        .valign(gtk::Align::Center)
        .build();
    check.connect_clicked(glib::clone!(
        #[strong]
        updates,
        move |_| updates.check()
    ));
    row.add_suffix(&check);
    row.add_suffix(&update);
    updates.watch_status(glib::clone!(
        #[weak]
        row,
        #[weak]
        check,
        #[weak]
        update,
        #[upgrade_or]
        false,
        move |status: &Status| {
            row.set_title(&status.title);
            match status.button {
                StatusButton::Check { enabled } => check.set_sensitive(enabled),
                StatusButton::Update(label) => update.set_label(label),
            }
            let offers_update = matches!(status.button, StatusButton::Update(_));
            check.set_visible(!offers_update);
            update.set_visible(offers_update);
            true
        }
    ));
    row
}

fn update_channel_row(browser: &Browser) -> adw::ComboRow {
    let names: Vec<&str> = CHANNELS.iter().map(|(_, name)| *name).collect();
    let current = browser
        .core()
        .borrow_mut()
        .prefs()
        .get(&keys::UPDATES_CHANNEL);
    let row = adw::ComboRow::builder()
        .title("Update Channel")
        .subtitle("Vsesvit moves to a steadier channel once that channel has a version newer than this one.")
        .model(&gtk::StringList::new(&names))
        .selected(index_of(&CHANNELS, &current))
        .build();
    row.connect_selected_notify(glib::clone!(
        #[strong]
        browser,
        move |row| {
            if let Some((channel, _)) = CHANNELS.get(row.selected() as usize) {
                browser.set_updates_channel(*channel);
            }
        }
    ));
    row
}

fn appearance_page(browser: &Browser) -> adw::PreferencesPage {
    let appearance = group("");
    appearance.add(&theme_row(browser));
    appearance.add(&tabs_position_row(browser));
    appearance.add(&pref_switch_row(
        browser,
        "Show Bookmarks Bar",
        None,
        &keys::SHOW_BOOKMARKS_BAR,
        |b, _, on| b.set_bookmarks_bar_visible(on),
    ));
    appearance.add(&pref_switch_row(
        browser,
        "Show Home Button",
        Some("Next to Reload, opens the homepage"),
        &keys::SHOW_HOME_BUTTON,
        |b, _, on| b.set_home_button_visible(on),
    ));
    page("appearance", "Appearance", "applications-graphics-symbolic", &[appearance])
}

fn search_page(browser: &Browser) -> adw::PreferencesPage {
    let engine = group("");
    engine.add(&search_engine_row(browser));

    let address_bar = group("Address Bar");
    address_bar.add(&pref_switch_row(
        browser,
        "Compact Address Bar",
        Some("A narrow address bar in the middle of the toolbar"),
        &keys::COMPACT_ADDRESS_BAR,
        |b, _, on| b.set_compact_address_bar(on),
    ));
    address_bar.add(&pref_switch_row(
        browser,
        "Always Show Full URLs",
        Some("Otherwise https:// and www. show only while editing the address"),
        &keys::SHOW_FULL_URLS,
        |b, _, on| b.set_full_urls(on),
    ));

    let suggestions = group("Suggestions");
    suggestions.add(&pref_switch_row(
        browser,
        "Browsing History",
        None,
        &keys::SUGGEST_HISTORY,
        Browser::set_switch,
    ));
    suggestions.add(&pref_switch_row(
        browser,
        "Bookmarks",
        None,
        &keys::SUGGEST_BOOKMARKS,
        Browser::set_switch,
    ));

    page("search", "Search", "system-search-symbolic", &[engine, address_bar, suggestions])
}

fn privacy_page(window: &BrowserWindow) -> adw::PreferencesPage {
    let popups = group("Pop-ups");
    popups.add(&pref_switch_row(
        window.browser(),
        "Block Pop-ups",
        Some("Sites can still open windows when you click"),
        &keys::BLOCK_POPUPS,
        Browser::set_engine_switch,
    ));

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

    let site_permissions = adw::ActionRow::builder()
        .title("Site Permissions")
        .subtitle("Camera, microphone, location, notifications and more")
        .activatable(true)
        .build();
    site_permissions.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    site_permissions.connect_activated(glib::clone!(
        #[strong(rename_to = browser)]
        window.browser(),
        move |row| {
            if let Some(dialog) = row.ancestor(adw::PreferencesDialog::static_type()).and_downcast::<adw::PreferencesDialog>() {
                dialog.push_subpage(&site_permissions_page(&browser));
            }
        }
    ));
    let permissions = group("Permissions");
    permissions.add(&site_permissions);
    page("privacy", "Privacy", "security-high-symbolic", &[popups, permissions, data])
}

/// Every stored site setting, by site, each with its choice and a way back to asking.
fn site_permissions_page(browser: &Browser) -> adw::NavigationPage {
    let content = adw::Bin::new();
    fill_site_permissions(&content, browser);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&content));
    adw::NavigationPage::builder().title("Site Permissions").tag("site-permissions").child(&toolbar).build()
}

fn fill_site_permissions(content: &adw::Bin, browser: &Browser) {
    let settings = browser.core().borrow_mut().site_permissions().all();
    if settings.is_empty() {
        let empty = adw::StatusPage::builder()
            .icon_name("security-high-symbolic")
            .title("No Site Permissions")
            .description("Sites you allow or block show here.")
            .build();
        content.set_child(Some(&empty));
        return;
    }
    let page = adw::PreferencesPage::new();
    let sites: Vec<&[SiteSetting]> = settings.chunk_by(|a, b| a.origin == b.origin).collect();
    let hosts: Vec<String> = sites.iter().map(|site| site[0].origin.host_for_display()).collect();
    for (&site, host) in sites.iter().zip(&hosts) {
        let group = group(&site_heading(&site[0].origin, host, &hosts));
        for setting in site {
            group.add(&site_setting_row(content, browser, setting));
        }
        page.add(&group);
    }
    content.set_child(Some(&page));
}

/// A site by its host, or by its whole origin when another listed site has the same host
/// (`http://example.com` and `https://example.com`).
fn site_heading(origin: &Origin, host: &str, hosts: &[String]) -> String {
    if hosts.iter().filter(|h| *h == host).count() > 1 { origin.as_str().to_owned() } else { host.to_owned() }
}

fn site_setting_row(content: &adw::Bin, browser: &Browser, setting: &SiteSetting) -> adw::ComboRow {
    let choices: Vec<Setting> = [Setting::Allow, Setting::Block]
        .into_iter()
        .filter(|s| *s == Setting::Block || setting.permission.remembers_allow())
        .collect();
    let names: Vec<&str> = choices.iter().map(|s| if *s == Setting::Allow { "Allow" } else { "Block" }).collect();
    let row = adw::ComboRow::builder()
        .title(setting.permission.label())
        .model(&gtk::StringList::new(&names))
        .selected(choices.iter().position(|s| *s == setting.setting).and_then(|i| u32::try_from(i).ok()).unwrap_or(0))
        .build();
    let (origin, permission) = (setting.origin.clone(), setting.permission);
    row.connect_selected_notify(glib::clone!(
        #[weak]
        content,
        #[strong]
        browser,
        #[strong]
        origin,
        move |row| {
            if let Some(&chosen) = choices.get(row.selected() as usize) {
                change_site_setting(&content, &browser, &origin, permission, Some(chosen));
            }
        }
    ));
    let remove = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Remove")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    remove.connect_clicked(glib::clone!(
        #[weak]
        content,
        #[strong]
        browser,
        move |_| change_site_setting(&content, &browser, &origin, permission, None)
    ));
    row.add_suffix(&remove);
    row
}

/// `None` goes back to asking. The list is rebuilt once the row that changed has finished
/// emitting.
fn change_site_setting(content: &adw::Bin, browser: &Browser, origin: &Origin, permission: Permission, setting: Option<Setting>) {
    if let Err(e) = browser.core().borrow_mut().site_permissions().set(origin, permission, setting) {
        log::warn!("site permissions: {e}");
    }
    permissions::enforce(browser);
    glib::idle_add_local_once(glib::clone!(
        #[weak]
        content,
        #[strong]
        browser,
        move || fill_site_permissions(&content, &browser)
    ));
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
            if let Some((startup, _)) = STARTUPS.get(row.selected() as usize)
                && browser.core().borrow_mut().prefs().get(&keys::STARTUP) != *startup
            {
                let set = browser.core().borrow_mut().prefs().set(&keys::STARTUP, startup);
                if let Err(e) = set {
                    log::warn!("prefs: {e}");
                }
            }
        }
    ));
    browser.watch_prefs(glib::clone!(
        #[weak]
        row,
        #[upgrade_or]
        false,
        move |browser: &Browser| {
            row.set_selected(index_of(&STARTUPS, &browser.core().borrow_mut().prefs().get(&keys::STARTUP)));
            true
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
    browser.watch_prefs(glib::clone!(
        #[weak]
        row,
        #[upgrade_or]
        false,
        move |browser: &Browser| {
            let homepage = browser.core().borrow_mut().prefs().get(&keys::HOMEPAGE);
            if row.text() != homepage {
                row.set_text(&homepage);
            }
            true
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
            if let Some((position, _)) = POSITIONS.get(row.selected() as usize)
                && browser.tabs_position() != *position
            {
                browser.set_tabs_position(*position);
            }
        }
    ));
    browser.watch_prefs(glib::clone!(
        #[weak]
        row,
        #[upgrade_or]
        false,
        move |browser: &Browser| {
            row.set_selected(index_of(&POSITIONS, &browser.tabs_position()));
            true
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
    let ids: Vec<_> = engines.iter().map(|e| e.id.clone()).collect();
    let default_id = |browser: &Browser| browser.core().borrow_mut().search_engines().default_engine().ok().map(|e| e.id);
    row.connect_selected_notify(glib::clone!(
        #[strong]
        browser,
        move |row| {
            if let Some(engine) = engines.get(row.selected() as usize)
                && default_id(&browser).as_ref() != Some(&engine.id)
            {
                let set = browser.core().borrow_mut().search_engines().set_default(&engine.id);
                if let Err(e) = set {
                    log::warn!("search engines: {e}");
                }
            }
        }
    ));
    browser.watch_prefs(glib::clone!(
        #[weak]
        row,
        #[upgrade_or]
        false,
        move |browser: &Browser| {
            if let Some(index) = default_id(browser).and_then(|id| ids.iter().position(|i| *i == id)) {
                row.set_selected(u32::try_from(index).unwrap_or(0));
            }
            true
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
            if let Some((theme, _)) = THEMES.get(row.selected() as usize)
                && browser.theme() != *theme
            {
                browser.set_theme(*theme);
            }
        }
    ));
    browser.watch_prefs(glib::clone!(
        #[weak]
        row,
        #[upgrade_or]
        false,
        move |browser: &Browser| {
            row.set_selected(index_of(&THEMES, &browser.theme()));
            true
        }
    ));
    row
}

/// A switch for an on/off preference. `set` writes it and applies it: the change shows at
/// once in every window. It is not called for a value the preference already has, so showing a
/// value sync brought does not write it back.
fn pref_switch_row(
    browser: &Browser,
    title: &str,
    subtitle: Option<&str>,
    pref: &'static Pref<bool>,
    set: fn(&Browser, &Pref<bool>, bool),
) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(title)
        .subtitle(subtitle.unwrap_or_default())
        .active(browser.switch(pref))
        .build();
    row.connect_active_notify(glib::clone!(
        #[strong]
        browser,
        move |row| {
            if browser.switch(pref) != row.is_active() {
                set(&browser, pref, row.is_active());
            }
        }
    ));
    browser.watch_prefs(glib::clone!(
        #[weak]
        row,
        #[upgrade_or]
        false,
        move |browser: &Browser| {
            row.set_active(browser.switch(pref));
            true
        }
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

fn profile_folder_row(browser: &Browser) -> adw::ActionRow {
    let root = browser.core().borrow().paths().root.display().to_string();
    adw::ActionRow::builder()
        .title("Profile Folder")
        .subtitle(&root)
        .subtitle_selectable(true)
        .build()
}

fn index_of<T: PartialEq, const N: usize>(options: &[(T, &str); N], value: &T) -> u32 {
    options
        .iter()
        .position(|(v, _)| v == value)
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sync_state_lists_its_buttons_in_their_fixed_order() {
        let signed_in = |needs_sign_in| State::SignedIn {
            name: None,
            server: "https://sync.example".to_owned(),
            last_synced: None,
            syncing: false,
            error: None,
            needs_sign_in,
        };
        for state in [State::SignedOut { error: None }, State::SigningIn, signed_in(false), signed_in(true)] {
            let actions = state.status(0).actions;
            let order: Vec<usize> = actions
                .iter()
                .filter(|a| **a != Action::DeleteServerData)
                .map(|a| SYNC_ACTIONS.iter().position(|b| a == b).unwrap())
                .collect();
            assert!(order.is_sorted(), "{state:?} lists {actions:?}");
        }
    }

    #[test]
    fn turning_a_type_off_and_on_keeps_the_order() {
        let without_history = toggled(&DataType::ALL, DataType::History, false);
        assert_eq!(without_history, [DataType::Bookmarks, DataType::Tabs, DataType::Extensions, DataType::Settings]);
        assert!(!every_type(&without_history));
        let back = toggled(&without_history, DataType::History, true);
        assert_eq!(back, DataType::ALL);
        assert!(every_type(&back));
        assert_eq!(toggled(&[DataType::Tabs], DataType::Bookmarks, true), [DataType::Bookmarks, DataType::Tabs]);
    }

    #[test]
    fn sites_on_one_host_are_told_apart_by_scheme() {
        let origins = ["http://example.com", "https://example.com", "https://meet.example"].map(|o| Origin::parse(o).expect("an origin"));
        let hosts: Vec<String> = origins.iter().map(Origin::host_for_display).collect();
        let headings: Vec<String> = origins.iter().zip(&hosts).map(|(o, h)| site_heading(o, h, &hosts)).collect();
        assert_eq!(headings, ["http://example.com", "https://example.com", "meet.example"]);
    }
}
