//! Picture-in-picture is off for a site until the user turns it on with the address bar's
//! button, which shows while the selected tab plays a video: a click turns it on for the site
//! and puts the video in the pane's box at once, the page's place saying where it went, and a
//! click on the box brings it home. Then the site's row in site info and in Settings, the
//! button turning it off again, and the Settings switches that turn off picture-in-picture and
//! the whole player.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::permissions::{Origin, Permission, Setting};

use super::permission_steps::{in_popup, open_site_info, select as select_choice};
use super::tab_steps::select;
use super::{STEP_TIMEOUT, settings_on, shoot};
use crate::bindings::*;
use crate::tab::Tab;
use crate::window::BrowserWindow;
use crate::{exec, xaml};

/// How long the box is given to show something it must not.
const SETTLE: Duration = Duration::from_millis(1500);

fn stored(window: &BrowserWindow, origin: &Origin) -> Option<Setting> {
    let browser = window.browser()?;
    browser.core(|p| {
        p.site_permissions()
            .get(origin, Permission::PictureInPicture)
    })
}

async fn until(f: impl FnMut() -> Option<()>) -> bool {
    exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), f)
        .await
        .is_some()
}

/// From a site that never allowed it, the video stays in its tab; the button turns it on.
pub(super) async fn opt_in(
    window: &Rc<BrowserWindow>,
    first: &Rc<Tab>,
    media: &Rc<Tab>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let origin = media.origin().ok_or("the media page has no origin")?;
    select(window, first);
    exec::sleep(SETTLE).await;
    steps.push(json!({
        "name": "36f-no-pip-before-the-site-allows-it",
        "followed": window.media_tab(),
        "pip": window.pip_tab(),
        "stored": format!("{:?}", stored(window, &origin)),
        "ok": window.media_tab() == Some(media.id) && window.player_shown()
            && window.pip_tab().is_none() && stored(window, &origin).is_none()
            && window.pip_button_shown().is_none(),
    }));

    select(window, media);
    let shown = until(|| (window.pip_button_shown() == Some(false)).then_some(())).await;
    // The selected page's frame.
    exec::sleep(Duration::from_millis(700)).await;
    shoot(
        window,
        out_dir,
        "36g-pip-button-for-a-playing-video",
        steps,
        |w| {
            json!({
                "button": w.pip_button_shown(),
                "ok": shown && w.pip_button_shown() == Some(false),
            })
        },
    )
    .await;

    window.pip_clicked();
    let started = until(|| {
        (window.pip_tab() == Some(media.id) && window.pip_placeholder_shown()).then_some(())
    })
    .await;
    exec::sleep(Duration::from_millis(500)).await;
    shoot(
        window,
        out_dir,
        "36h-pip-button-turns-it-on-and-starts-it",
        steps,
        |w| {
            json!({
                "stored": format!("{:?}", stored(w, &origin)),
                "pip": w.pip_tab(),
                "button": w.pip_button_shown(),
                "placeholder": w.pip_placeholder_shown(),
                "ok": started && stored(w, &origin) == Some(Setting::Allow)
                    && w.pip_button_shown() == Some(true)
                    && w.active_tab().is_some_and(|t| t.id == media.id),
            })
        },
    )
    .await;

    window.click_pip_box();
    let home =
        until(|| (window.pip_tab().is_none() && !window.pip_placeholder_shown()).then_some(()))
            .await;
    steps.push(json!({
        "name": "36i-the-box-brings-the-video-home",
        "pip": window.pip_tab(),
        "view_shown": xaml::is_visible(media.view()),
        "ok": home && xaml::is_visible(media.view())
            && window.active_tab().is_some_and(|t| t.id == media.id),
    }));
    Ok(())
}

/// The site's row in site info and in Settings, the button turning it off, and the switches.
/// Leaves the site with nothing stored.
pub(super) async fn rows_and_switches(
    window: &Rc<BrowserWindow>,
    first: &Rc<Tab>,
    media: &Rc<Tab>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let origin = media.origin().ok_or("the media page has no origin")?;
    select(window, media);
    until(|| (window.pip_button_shown() == Some(true)).then_some(())).await;
    window.pip_clicked();
    let off = until(|| (window.pip_button_shown() == Some(false)).then_some(())).await;
    select(window, first);
    exec::sleep(SETTLE).await;
    steps.push(json!({
        "name": "41b-pip-button-turns-it-off",
        "stored": format!("{:?}", stored(window, &origin)),
        "pip": window.pip_tab(),
        "ok": off && stored(window, &origin) == Some(Setting::Block)
            && window.pip_tab().is_none(),
    }));

    select(window, media);
    let popup = open_site_info(window).await?;
    let row = in_popup::<UIElement>(&popup, "PermissionRowPictureInPicture").is_ok();
    let choices = in_popup::<ItemsControl>(&popup, "PermissionChoicePictureInPicture")
        .ok()
        .and_then(|c| c.Items().ok()?.Size().ok());
    shoot(
        window,
        out_dir,
        "41c-site-info-pip-row",
        steps,
        |_| json!({ "row": row, "choices": choices, "ok": row && choices == Some(2) }),
    )
    .await;
    // Allow, then Block, the row's two choices.
    select_choice(&popup, "PermissionChoicePictureInPicture", 0)?;
    exec::sleep(Duration::from_millis(300)).await;
    window.hide_connection();
    let allowed = stored(window, &origin) == Some(Setting::Allow);
    let button = until(|| (window.pip_button_shown() == Some(true)).then_some(())).await;
    select(window, first);
    let pip = until(|| (window.pip_tab() == Some(media.id)).then_some(())).await;
    steps.push(json!({
        "name": "41d-site-info-allows-pip",
        "stored": format!("{:?}", stored(window, &origin)),
        "button_on": button,
        "pip": window.pip_tab(),
        "ok": allowed && button && pip,
    }));

    switches(window, first, media, out_dir, steps).await?;

    let preview = settings_on(window, "SitePermissionsPanel")
        .await
        .map_err(|e| e.to_string())?;
    let listed = preview
        .find::<UIElement>("SiteChoice0picture_in_picture")
        .is_ok();
    shoot(
        window,
        out_dir,
        "41g-settings-pip-row",
        steps,
        |_| json!({ "listed": listed, "ok": listed }),
    )
    .await;
    let remove: Button = preview
        .find("SiteRemove0picture_in_picture")
        .map_err(|e| e.to_string())?;
    super::invoke(&remove).map_err(|e| e.to_string())?;
    drop(preview);
    let reset = stored(window, &origin).is_none();
    let gone = until(|| window.pip_tab().is_none().then_some(())).await;
    steps.push(json!({
        "name": "41h-settings-removes-pip",
        "stored": format!("{:?}", stored(window, &origin)),
        "pip": window.pip_tab(),
        "ok": reset && gone,
    }));
    Ok(())
}

/// Picture-in-picture showing, its switch off ends it and hides the button, and the player's
/// switch hides the player and the box; both on again bring them back.
async fn switches(
    window: &Rc<BrowserWindow>,
    first: &Rc<Tab>,
    media: &Rc<Tab>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let browser = window.browser().ok_or("no browser")?;
    let preview = settings_on(window, "AppearancePanel")
        .await
        .map_err(|e| e.to_string())?;
    let player: ToggleSwitch = preview.find("ShowMediaPlayer").map_err(|e| e.to_string())?;
    let pip: ToggleSwitch = preview
        .find("PictureInPicture")
        .map_err(|e| e.to_string())?;
    let on_by_default = player.IsOn().unwrap_or(false) && pip.IsOn().unwrap_or(false);

    pip.SetIsOn(false).map_err(|e| e.to_string())?;
    let ended = until(|| window.pip_tab().is_none().then_some(())).await;
    select(window, media);
    exec::sleep(Duration::from_millis(500)).await;
    let button_hidden = window.pip_button_shown().is_none();
    let player_stays = window.player_shown();
    select(window, first);
    exec::sleep(SETTLE).await;
    let none_later = window.pip_tab().is_none();
    shoot(window, out_dir, "41e-pip-switch-off", steps, |_| {
        json!({
            "on_by_default": on_by_default,
            "pip_ended": ended,
            "button_hidden": button_hidden,
            "player_stays": player_stays,
            "no_pip_later": none_later,
            "pref": browser.pip_enabled(),
            "ok": on_by_default && ended && button_hidden && player_stays && none_later
                && !browser.pip_enabled(),
        })
    })
    .await;
    pip.SetIsOn(true).map_err(|e| e.to_string())?;
    let back = until(|| (window.pip_tab() == Some(media.id)).then_some(())).await;

    player.SetIsOn(false).map_err(|e| e.to_string())?;
    let hidden =
        until(|| (!window.player_shown() && window.pip_tab().is_none()).then_some(())).await;
    select(window, media);
    exec::sleep(Duration::from_millis(500)).await;
    let button_hidden = window.pip_button_shown().is_none();
    select(window, first);
    player.SetIsOn(true).map_err(|e| e.to_string())?;
    let shown =
        until(|| (window.player_shown() && window.pip_tab() == Some(media.id)).then_some(())).await;
    steps.push(json!({
        "name": "41f-player-switch",
        "pip_back_with_its_switch": back,
        "player_and_box_hidden": hidden,
        "button_hidden": button_hidden,
        "player_and_box_back": shown,
        "pref": browser.media_player_visible(),
        "ok": back && hidden && button_hidden && shown && browser.media_player_visible(),
    }));
    Ok(())
}
