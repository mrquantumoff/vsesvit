//! The `profiles` check: the profile menu, window titles with the profile's name, Manage profiles
//! and the picker switch in Settings, on a profile list in the output directory. No process is
//! started: adding goes through core, as the Add profile flyout's answer does.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use vsesvit_core::profiles::ProfileColor;

use super::{Probe, until};
use windows_core::Interface;

use crate::bindings::{FlyoutBase, TextBlock, ToggleSwitch};
use crate::browser::Browser;
use crate::dialogs::{self, Dialog};
use crate::exec;
use crate::window::BrowserWindow;

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
    browser.edit_profile(&home.id, "Tester", ProfileColor::Teal)?;
    let renamed = window.title();
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
        "alone: menu {:?}, title {:?}; with Work: menu {:?}, title {:?}; renamed: {renamed:?}; \
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
        && renamed == format!("{page} - Tester - Vsesvit")
        && rows == ["Tester", "Work"]
        && picker_shown
        && !picker_stored
        && after.1 == checked(&["Tester"], 0)
        && after.2 == format!("{page} - Vsesvit");
    ok.then_some(detail.clone()).ok_or(detail)
}
