//! The profile button's menu and Manage profiles, with a second profile added through core:
//! the menu lists both, the current one checked, then Add profile and Manage profiles.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::profiles::ProfileColor;
use windows_core::Interface;

use super::shoot;
use crate::bindings::*;
use crate::browser::Browser;
use crate::dialogs::{self, Dialog};
use crate::exec;
use crate::window::BrowserWindow;

pub(super) async fn run(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let home = browser.home().ok_or("the run's profile is not in a profile list")?.clone();
    let (work, registry) = home.dir.add("Work", ProfileColor::Green).map_err(|e| e.to_string())?;
    browser.set_profiles(registry);

    let menu = window.show_profile_menu().map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(500)).await;
    let mut entries = Vec::new();
    for item in &menu.Items().map_err(|e| e.to_string())? {
        if let Ok(toggle) = item.cast::<ToggleMenuFlyoutItem>() {
            let text = toggle.cast::<MenuFlyoutItem>().and_then(|i| i.Text()).map(|t| t.to_string());
            entries.push(json!([text.unwrap_or_default(), toggle.IsChecked().unwrap_or(false)]));
        } else if let Ok(item) = item.cast::<MenuFlyoutItem>() {
            entries.push(json!(item.Text().map(|t| t.to_string()).unwrap_or_default()));
        }
    }
    let expected = json!([["Person 1", true], ["Work", false], "Add profile", "Manage profiles"]);
    shoot(window, out_dir, "21b-profile-menu", steps, |w| {
        json!({ "entries": entries, "title": w.title(), "ok": entries == expected.as_array().cloned().unwrap_or_default() })
    })
    .await;
    let _ = menu.cast::<FlyoutBase>().and_then(|m| m.Hide());

    let manage = dialogs::preview(window, Dialog::Profiles).map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(500)).await;
    let rows: Vec<String> = (0..2)
        .filter_map(|i| manage.find::<TextBlock>(&format!("ProfileRowName{i}")).ok())
        .filter_map(|t| t.Text().ok().map(|t| t.to_string()))
        .collect();
    shoot(window, out_dir, "21c-manage-profiles", steps, |_| {
        json!({ "rows": rows, "ok": rows == ["Person 1", "Work"] })
    })
    .await;
    drop(manage);

    browser.remove_profile(&work)
}
