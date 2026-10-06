//! Tracking protection in the site-info popup, on the fixture page whose image comes from
//! `localhost`, a tracker in scripted runs: the switch on with that tracker counted, and turning
//! it off storing the site's exception and loading the image. Then the popup's cookie choice for
//! the site: Default with what that means, and Clear on exit stored when chosen.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::cookies;
use vsesvit_core::permissions::{Origin, Setting};
use vsesvit_core::private::Browsing;
use vsesvit_core::testkit::FixtureServer;
use vsesvit_core::trackers::{self, Category, TrackerList};

use super::permission_steps::{in_popup, open_site_info, select, text_of};
use super::{shoot, wait_title};
use crate::bindings::*;
use crate::browser::Browser;
use crate::exec;
use crate::tab::Tab;
use crate::window::BrowserWindow;

pub(super) async fn run(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    server: &FixtureServer,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let page = server.url("/trackers.html");
    let site = Origin::parse(page.as_str()).ok_or("no fixture origin")?;
    browser.set_trackers(
        TrackerList::bundled()
            .clone()
            .with_tracker("localhost", Category::Analytics),
    );
    let tab = window
        .open_url_tab(page.as_str(), true)
        .map_err(|e| e.to_string());
    let result = match &tab {
        Ok(tab) => site_info(browser, window, server, tab, &site, out_dir, steps).await,
        Err(e) => Err(e.clone()),
    };
    window.hide_connection();
    if let Err(e) = browser.core(|c| trackers::set_allowed(c, &site, false)) {
        log::warn!("tracking protection for {}: {e}", site.as_str());
    }
    match browser.core(|c| cookies::set(c, &site, None)) {
        Ok(()) => crate::permissions::settings_changed(browser),
        Err(e) => log::warn!("cookies for {}: {e}", site.as_str()),
    }
    if let Ok(tab) = tab {
        window.close_tab(tab.id);
    }
    browser.set_trackers(TrackerList::bundled().clone());
    result
}

async fn site_info(
    browser: &Browser,
    window: &Rc<BrowserWindow>,
    server: &FixtureServer,
    tab: &Rc<Tab>,
    site: &Origin,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let pixel = || server.hits().iter().any(|h| h == "/tracker/pixel.png");
    wait_title(tab, "tracker blocked").await?;
    let served = pixel();
    let popup = open_site_info(window).await?;
    let switch: ToggleSwitch = in_popup(&popup, "TrackingProtectionSwitch")?;
    let on = switch.IsOn().unwrap_or(false);
    let status = text_of(&popup, "TrackingProtectionStatus");
    shoot(
        window,
        out_dir,
        "43a-site-info-tracking-protection",
        steps,
        |_| {
            json!({
                "switch_on": on,
                "status": status,
                "server_saw_the_image": served,
                "ok": on && status == "1 tracker blocked on this page" && !served,
            })
        },
    )
    .await;

    switch.SetIsOn(false).map_err(|e| e.to_string())?;
    let loaded = wait_title(tab, "tracker loaded").await;
    exec::sleep(Duration::from_millis(300)).await;
    let stored = browser.core(|c| trackers::allowed(c, site));
    let status = text_of(&popup, "TrackingProtectionStatus");
    steps.push(json!({
        "name": "43b-tracking-protection-off-for-the-site",
        "stored": stored,
        "load_error": loaded.as_ref().err(),
        "server_saw_the_image": pixel(),
        "status": status,
        "ok": stored && loaded.is_ok() && pixel() && status == "Off for this site",
    }));

    window.hide_connection();
    exec::sleep(Duration::from_millis(300)).await;
    let popup = open_site_info(window).await?;
    let choice: Selector = in_popup(&popup, "CookiesChoice")?;
    let selected = choice.SelectedIndex().unwrap_or(-1);
    let status = text_of(&popup, "CookiesStatus");
    let expected = browser.core(|c| {
        cookies::site_status(
            cookies::third_party_blocked(c, Browsing::Normal, Some(site)),
            None,
        )
    });
    let choices = cookies::site_choices(None, true);
    let clear = choices
        .iter()
        .position(|c| *c == Some(Setting::ClearOnExit))
        .ok_or("no Clear on exit choice")?;
    select(&popup, "CookiesChoice", i32::try_from(clear).unwrap_or(-1))?;
    exec::sleep(Duration::from_millis(300)).await;
    let stored = browser.core(|c| cookies::setting(c, site));
    let chosen_status = text_of(&popup, "CookiesStatus");
    steps.push(json!({
        "name": "43c-site-info-cookies",
        "selected": selected,
        "status": status,
        "stored": format!("{stored:?}"),
        "status_after": chosen_status,
        "ok": selected == 0
            && status == expected
            && stored == Some(Setting::ClearOnExit)
            && chosen_status == cookies::site_status(false, Some(Setting::ClearOnExit)),
    }));
    Ok(())
}
