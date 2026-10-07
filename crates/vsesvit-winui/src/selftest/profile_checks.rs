//! The `profiles` check: the profile menu, window titles with the profile's name, a private
//! window's too, the name and picture a sync sign-in gives the profile, Manage profiles and the
//! picker switch in Settings, on a profile list in the output directory. No process is started:
//! adding goes through core, as the Add profile flyout's answer does. The account picture is the
//! one a sign-in would fetch, decoded here: the self-test reaches no network.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use vsesvit_core::private::Browsing;
use vsesvit_core::profiles::{AccountDetails, AccountPicture, ProfileColor};

use super::{Probe, until};
use windows_core::Interface;

use crate::bindings::{FlyoutBase, TextBlock, ToggleSwitch};
use crate::browser::Browser;
use crate::dialogs::{self, Dialog};
use crate::exec;
use crate::session::{TabPlan, WindowPlan};
use crate::sync::take_account_details;
use crate::window::BrowserWindow;

const PICTURE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/site/allowed.png"
));

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Saves the window as `name`, a moment after what it shows has opened.
async fn shoot(window: &BrowserWindow, out_dir: &Path, name: &str) -> Result<(), String> {
    exec::sleep(Duration::from_millis(500)).await;
    let shot = window
        .capture()
        .await
        .map_err(|e| format!("capture: {e}"))?;
    let path = out_dir.join(name);
    std::fs::write(&path, &shot.png).map_err(|e| format!("{}: {e}", path.display()))
}

/// The title of a private window on the normal window's page, and whether it shows the profile
/// button, which it hides as Chrome's incognito windows do.
async fn private_title(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    p: &Probe,
) -> Result<(String, bool), String> {
    let page = window.active_tab().map(|t| t.state()).unwrap_or_default();
    let plan = WindowPlan::with_tabs(vec![TabPlan::url(page.url.clone())]);
    let private = browser
        .open_window(Browsing::Private, &plan, browser.show_mode())
        .map_err(err)?;
    let tab = until(p, |p| {
        p.observe("the private window has no tab yet");
        private.active_tab()
    })
    .await;
    until(p, |p| {
        let s = tab.state();
        p.observe(format!("private tab at {:?} titled {:?}", s.url, s.title));
        (s.url == page.url && s.title == page.title && !s.loading()).then_some(())
    })
    .await;
    let shown = (private.title(), private.shows_profile_button());
    private.close_tab(tab.id);
    until(p, |p| {
        p.observe("the private window is still open");
        browser.windows_of(Browsing::Private).is_empty().then_some(())
    })
    .await;
    Ok(shown)
}

pub(super) async fn profiles(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    p: &Probe,
) -> Result<String, String> {
    let home = browser
        .home()
        .ok_or("the self-test profile is not in its profile list")?
        .clone();
    let menu = || window.profile_menu_lines().map_err(err);
    let page = window
        .active_tab()
        .map(|t| t.state().title)
        .unwrap_or_default();
    let alone = (menu()?, window.title());

    let (work, registry) = home.dir.add("Work", ProfileColor::Green).map_err(err)?;
    browser.set_profiles(registry);
    let together = (menu()?, window.title());
    let private = private_title(browser, window, p).await?;
    let account = |name: &str| AccountDetails {
        name: Some(name.to_owned()),
        picture: None,
    };
    take_account_details(browser, account("Alex"));
    let picture = AccountPicture::decode(PICTURE).ok_or("the fixture picture does not decode")?;
    let registry = home
        .dir
        .take_account_details(&home.id, None, Some(&picture))
        .map_err(err)?;
    browser.set_profiles(registry);
    let from_account = (menu()?, window.shows_profile_picture());
    shoot(window, out_dir, "profile-account.png").await?;
    browser.edit_profile(&home.id, "Tester", ProfileColor::Teal)?;
    let renamed = window.title();
    take_account_details(browser, account("Sam"));
    let by_hand = (menu()?, window.shows_profile_picture());
    let shown = window.show_profile_menu().map_err(err)?;
    let shot = shoot(window, out_dir, "profile-menu.png").await;
    let _ = shown.cast::<FlyoutBase>().and_then(|m| m.Hide());
    shot?;

    let manage = dialogs::preview(window, Dialog::Profiles).map_err(err)?;
    let rows = until(p, |p| {
        let names: Vec<String> = (0..2)
            .filter_map(|i| manage.find::<TextBlock>(&format!("ProfileRowName{i}")).ok())
            .filter_map(|t| t.Text().ok().map(|t| t.to_string()))
            .collect();
        p.observe(format!("Manage profiles lists {names:?}"));
        (names.len() == 2).then_some(names)
    })
    .await;
    shoot(window, out_dir, "profiles-manage.png").await?;
    drop(manage);

    let settings = dialogs::preview(window, Dialog::Settings).map_err(err)?;
    let switch = until(p, |p| {
        p.observe("Settings has no profile picker switch yet");
        settings.find::<ToggleSwitch>("ProfilePicker").ok()
    })
    .await;
    let picker_shown = switch.IsOn().map_err(err)?;
    switch.SetIsOn(false).map_err(err)?;
    let picker_stored = home.dir.load().show_picker();
    drop(settings);

    browser.remove_profile(&work)?;
    let removed = until(p, |p| {
        p.observe("the removed profile's directory is still there");
        (!home.dir.root(&work).exists()).then_some(())
    })
    .await;
    let after = (removed, menu()?, window.title());

    let detail = format!(
        "alone: menu {:?}, title {:?}; with Work: menu {:?}, title {:?}, a private window's title and profile button {private:?}; \
         signed in to sync as Alex: menu and account picture {from_account:?} (profile-account.png); renamed and recoloured by hand: {renamed:?}, \
         then a sync sign-in as Sam left menu and picture {by_hand:?}; \
         Manage profiles lists {rows:?} (profile-menu.png, profiles-manage.png); Settings' picker switch was on={picker_shown}, off stored \
         on={picker_stored}; Work removed: menu {:?}, title {:?}",
        alone.0, alone.1, together.0, together.1, after.1, after.2
    );
    let checked = |names: &[&str], current: usize| -> Vec<(String, bool)> {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| ((*n).to_owned(), i == current))
            .collect()
    };
    let ok = alone == (checked(&["Person 1"], 0), format!("{page} - Vsesvit"))
        && together
            == (
                checked(&["Person 1", "Work"], 0),
                format!("{page} - Person 1 - Vsesvit"),
            )
        && private == (format!("{page} - Person 1 - Vsesvit (Private)"), false)
        && from_account == (checked(&["Alex", "Work"], 0), true)
        && renamed == format!("{page} - Tester - Vsesvit")
        && by_hand == (checked(&["Tester", "Work"], 0), false)
        && rows == ["Tester", "Work"]
        && picker_shown
        && !picker_stored
        && after.1 == checked(&["Tester"], 0)
        && after.2 == format!("{page} - Vsesvit");
    ok.then_some(detail.clone()).ok_or(detail)
}
