//! Settings' site data, a subpage of the Privacy page as Chrome's "See all site data" is: every
//! site WebKit keeps cookies, storage or cached files for, with what it holds, removed one site
//! at a time or all at once.

use adw::prelude::*;
use gtk::glib;

use super::{confirm, row_button};
use crate::browser::Browser;
use crate::cookies::{self, remove_data};

/// The Privacy page's row that opens the list.
pub(crate) const SEE_ALL_ROW: &str = "See All Site Data";

pub(super) fn row(browser: &Browser) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(SEE_ALL_ROW)
        .subtitle("Cookies and other data sites keep on this device")
        .activatable(true)
        .build();
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row.connect_activated(glib::clone!(
        #[strong]
        browser,
        move |row| {
            if let Some(dialog) = row.ancestor(adw::PreferencesDialog::static_type()).and_downcast::<adw::PreferencesDialog>() {
                dialog.push_subpage(&page(&browser));
            }
        }
    ));
    row
}

fn page(browser: &Browser) -> adw::NavigationPage {
    let content = adw::Bin::new();
    fill(&content, browser);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&content));
    adw::NavigationPage::builder().title("Site Data").tag("site-data").child(&toolbar).build()
}

/// Lists what WebKit holds now, by site.
fn fill(content: &adw::Bin, browser: &Browser) {
    let Some(manager) = browser.engine().session().website_data_manager() else { return };
    let (content, browser) = (content.downgrade(), browser.clone());
    glib::spawn_future_local(async move {
        let mut records = manager.fetch_future(webkit::WebsiteDataTypes::ALL).await.unwrap_or_else(|e| {
            log::warn!("site data: {e}");
            Vec::new()
        });
        let Some(content) = content.upgrade() else { return };
        records.retain(|r| r.name().is_some());
        records.sort_by_key(|r| r.name());
        show(&content, &browser, &manager, records);
    });
}

fn show(content: &adw::Bin, browser: &Browser, manager: &webkit::WebsiteDataManager, records: Vec<webkit::WebsiteData>) {
    if records.is_empty() {
        let empty = adw::StatusPage::builder()
            .icon_name("security-high-symbolic")
            .title("No Site Data")
            .description("Sites that keep cookies or other data on this device show here.")
            .build();
        content.set_child(Some(&empty));
        return;
    }
    let remove_all = adw::ButtonRow::builder().title("Remove All…").start_icon_name("user-trash-symbolic").build();
    remove_all.add_css_class("destructive-action");
    let all = adw::PreferencesGroup::new();
    all.add(&remove_all);
    let sites = adw::PreferencesGroup::new();
    for record in &records {
        let row = adw::ActionRow::builder()
            .title(record.name().unwrap_or_default())
            .subtitle(cookies::holds(record.types()))
            .use_markup(false)
            .build();
        let button = row_button("user-trash-symbolic", "Remove");
        button.connect_clicked(glib::clone!(
            #[weak]
            content,
            #[strong]
            browser,
            #[strong]
            manager,
            #[strong]
            record,
            move |_| remove(&content, &browser, &manager, vec![record.clone()])
        ));
        row.add_suffix(&button);
        sites.add(&row);
    }
    remove_all.connect_activated(glib::clone!(
        #[weak]
        content,
        #[strong]
        browser,
        #[strong]
        manager,
        move |row| {
            let (row, content, browser, manager, records) = (row.clone(), content.clone(), browser.clone(), manager.clone(), records.clone());
            glib::spawn_future_local(async move {
                let body = "Sites will forget you, and you will be signed out of them.";
                if confirm(&row, "Remove All Site Data?", body, "_Remove").await {
                    remove(&content, &browser, &manager, records);
                }
            });
        }
    ));
    let page = adw::PreferencesPage::new();
    page.add(&all);
    page.add(&sites);
    content.set_child(Some(&page));
}

fn remove(content: &adw::Bin, browser: &Browser, manager: &webkit::WebsiteDataManager, records: Vec<webkit::WebsiteData>) {
    let (content, browser, manager) = (content.downgrade(), browser.clone(), manager.clone());
    glib::spawn_future_local(async move {
        if let Err(e) = remove_data(&manager, webkit::WebsiteDataTypes::ALL, &records).await {
            log::warn!("site data: {e}");
        }
        if let Some(content) = content.upgrade() {
            fill(&content, &browser);
        }
    });
}
