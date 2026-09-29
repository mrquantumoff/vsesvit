//! Site permissions on a fixture page: a notification request's prompt and its answer, which
//! the next request and a reload remember; the site-info popup's Permissions section blocking
//! it; camera and microphone (the engine's fake devices) as one prompt, with the in-use
//! indicators and Stop; a blocked screen share that shows no picker; and the Settings page
//! listing what is stored.

use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use vsesvit_core::permissions::{Origin, Permission, Setting};
use vsesvit_core::testkit::FixtureServer;
use windows_core::Interface;

use super::{STEP_TIMEOUT, devtools, eval, shoot, wait_loaded};
use crate::bindings::*;
use crate::browser::Browser;
use crate::dialogs::{self, Dialog};
use crate::shortcuts::Command;
use crate::tab::Tab;
use crate::window::BrowserWindow;
use crate::{exec, xaml};

const ASK_NOTIFICATIONS: &str =
    "window.__notify = ''; Notification.requestPermission().then(r => window.__notify = r); 0";

const ASK_LOCATION: &str = "navigator.geolocation.getCurrentPosition(() => 0, () => 0); 0";

const OPEN_CAMERA: &str = "window.__gum = ''; window.__ended = 0; \
    navigator.mediaDevices.getUserMedia({ video: true, audio: true }).then(s => { \
      window.__stream = s; \
      for (const t of s.getTracks()) t.addEventListener('ended', () => window.__ended++); \
      window.__gum = 'ok'; }, e => window.__gum = e.name); 0";

const SHARE_SCREEN: &str = "window.__gdm = ''; \
    navigator.mediaDevices.getDisplayMedia({ video: true }).then(s => { \
      window.__screen = s; window.__gdm = 'ok'; }, e => window.__gdm = e.name); 0";

/// The page's value of `expression` once it is not `""`, as JSON.
async fn page_value(tab: &Tab, expression: &str) -> Option<String> {
    let deadline = Instant::now() + STEP_TIMEOUT;
    while Instant::now() < deadline {
        let value = eval(tab, expression).await.unwrap_or_default();
        if !value.is_empty() && value != "\"\"" && value != "null" {
            return Some(value.trim_matches('"').to_owned());
        }
        exec::sleep(Duration::from_millis(100)).await;
    }
    None
}

/// A screenshot of what these steps show, which the address box's suggestion list must not
/// cover.
async fn shoot_clear(
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    name: &str,
    steps: &mut Vec<Value>,
    check: impl FnOnce(&BrowserWindow) -> Value,
) {
    shoot(window, out_dir, name, steps, |w| {
        let mut step = check(w);
        let open = w.suggestions_open();
        step["suggestions_open"] = json!(open);
        if open {
            step["ok"] = json!(false);
        }
        step
    })
    .await;
}

async fn wait_prompt(window: &BrowserWindow) -> Option<FrameworkElement> {
    let prompt = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window.permission_prompt()
    })
    .await;
    // The flyout's opening animation.
    exec::sleep(Duration::from_millis(600)).await;
    prompt
}

/// An element of the site-info popup's Permissions section, which has a namescope of its own.
fn in_popup<T: Interface>(popup: &FrameworkElement, name: &str) -> Result<T, String> {
    popup
        .cast::<DependencyObject>()
        .ok()
        .and_then(|root| xaml::find_named(&root, name))
        .ok_or_else(|| format!("no {name} in the site-info popup"))
}

fn text_of(root: &FrameworkElement, name: &str) -> String {
    xaml::find::<TextBlock>(root, name)
        .and_then(|t| t.Text())
        .map(|t| t.to_string())
        .unwrap_or_default()
}

fn answer(prompt: &FrameworkElement, name: &str) -> Result<(), String> {
    let button: Button = xaml::find(prompt, name).map_err(|e| format!("{name}: {e}"))?;
    super::dialog_steps::invoke(&button).map_err(|e| e.to_string())
}

fn select(popup: &FrameworkElement, name: &str, index: i32) -> Result<(), String> {
    in_popup::<Selector>(popup, name)?
        .SetSelectedIndex(index)
        .map_err(|e| format!("{name}: {e}"))
}

async fn open_site_info(window: &Rc<BrowserWindow>) -> Result<FrameworkElement, String> {
    window.show_connection().map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(600)).await;
    window
        .connection_popup()
        .ok_or("the site-info popup did not open".into())
}

pub(super) async fn run(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    server: &FixtureServer,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let origin = Origin::parse(&server.origin()).ok_or("no fixture origin")?;
    let stored = |p: Permission| browser.core(|c| c.site_permissions().get(&origin, p));
    let tab = window
        .open_url_tab(server.url("/page2.html").as_str(), true)
        .map_err(|e| e.to_string())?;
    wait_loaded(&tab).await?;

    eval(&tab, ASK_NOTIFICATIONS).await?;
    let prompt = wait_prompt(window).await.ok_or("no notification prompt")?;
    let heading = text_of(&prompt, "PromptHeading");
    shoot_clear(
        window,
        out_dir,
        "24a-permission-prompt",
        steps,
        |_| json!({ "heading": heading, "ok": heading == "Show notifications?" }),
    )
    .await;
    answer(&prompt, "AnswerAllowWhileVisiting")?;
    let result = page_value(&tab, "window.__notify").await;
    steps.push(json!({
        "name": "24b-permission-allowed",
        "page": result,
        "stored": format!("{:?}", stored(Permission::Notifications)),
        "ok": result.as_deref() == Some("granted") && stored(Permission::Notifications) == Some(Setting::Allow),
    }));

    tab.reload();
    exec::sleep(Duration::from_millis(300)).await;
    wait_loaded(&tab).await?;
    let before = window.permission_prompts_shown();
    eval(&tab, ASK_NOTIFICATIONS).await?;
    let result = page_value(&tab, "window.__notify").await;
    let prompted = window.permission_prompts_shown() - before;
    steps.push(json!({
        "name": "24c-permission-remembered-after-reload",
        "page": result,
        "prompts": prompted,
        "ok": result.as_deref() == Some("granted") && prompted == 0,
    }));

    let popup = open_site_info(window).await?;
    let row = in_popup::<UIElement>(&popup, "PermissionRowNotifications").is_ok();
    shoot_clear(
        window,
        out_dir,
        "24d-site-info-permissions",
        steps,
        |_| json!({ "notifications_row": row, "ok": row }),
    )
    .await;
    select(&popup, "PermissionChoiceNotifications", 2)?;
    exec::sleep(Duration::from_millis(300)).await;
    window.hide_connection();
    let before = window.permission_prompts_shown();
    eval(&tab, ASK_NOTIFICATIONS).await?;
    let result = page_value(&tab, "window.__notify").await;
    let prompted = window.permission_prompts_shown() - before;
    steps.push(json!({
        "name": "24e-blocked-in-site-info-denies-without-a-prompt",
        "stored": format!("{:?}", stored(Permission::Notifications)),
        "page": result,
        "prompts": prompted,
        "ok": stored(Permission::Notifications) == Some(Setting::Block)
            && result.as_deref() == Some("denied") && prompted == 0,
    }));

    camera(window, &tab, out_dir, steps).await?;

    browser
        .core(|c| {
            c.site_permissions()
                .set(&origin, Permission::ScreenShare, Some(Setting::Block))
        })
        .map_err(|e| e.to_string())?;
    let gesture = json!({ "expression": SHARE_SCREEN, "userGesture": true });
    devtools(&tab, "Runtime.evaluate", &gesture).await?;
    let started = Instant::now();
    let result = page_value(&tab, "window.__gdm").await;
    steps.push(json!({
        "name": "24i-blocked-screen-share-shows-no-picker",
        "page": result,
        "ms": started.elapsed().as_millis(),
        "ok": result.as_deref() == Some("NotAllowedError") && started.elapsed() < Duration::from_secs(3),
    }));

    settings(window, &origin, out_dir, steps).await?;
    browser
        .core(|c| {
            c.site_permissions()
                .set(&origin, Permission::ScreenShare, None)
        })
        .map_err(|e| e.to_string())?;
    screen_share(window, &tab, out_dir, steps).await?;
    background_tab(window, server, out_dir, steps).await?;
    browser
        .core(|c| c.site_permissions().reset_site(&origin))
        .map_err(|e| e.to_string())?;
    window.close_tab(tab.id);
    Ok(())
}

/// One `getUserMedia` for camera and microphone: one prompt, the in-use icon on the tab and
/// in the address bar, and Stop in the site-info popup ending both tracks.
async fn camera(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let before = window.permission_prompts_shown();
    eval(tab, OPEN_CAMERA).await?;
    let prompt = wait_prompt(window).await.ok_or("no camera prompt")?;
    let heading = text_of(&prompt, "PromptHeading");
    answer(&prompt, "AnswerAllowThisTime")?;
    let result = page_value(tab, "window.__gum").await;
    exec::sleep(Duration::from_millis(1000)).await;
    let prompts = window.permission_prompts_shown() - before;
    let indicator = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window
            .capture_button_shown()
            .filter(|_| window.tab_capture_shown(tab.id))
    })
    .await;
    shoot_clear(window, out_dir, "24f-camera-and-microphone-in-use", steps, |_| {
        json!({
            "heading": heading,
            "page": result,
            "prompts": prompts,
            "indicator": indicator,
            "ok": heading == "Use your camera and microphone?" && result.as_deref() == Some("ok")
                && prompts == 1 && indicator.as_deref() == Some("Using your camera and microphone"),
        })
    })
    .await;

    let popup = open_site_info(window).await?;
    let (camera, microphone) = (
        in_popup::<UIElement>(&popup, "PermissionStopCamera").is_ok(),
        in_popup::<UIElement>(&popup, "PermissionStopMicrophone").is_ok(),
    );
    shoot_clear(window, out_dir, "24g-site-info-in-use", steps, |_| {
        json!({ "stop_camera": camera, "stop_microphone": microphone, "ok": camera && microphone })
    })
    .await;
    for name in ["PermissionStopCamera", "PermissionStopMicrophone"] {
        let popup = window
            .connection_popup()
            .ok_or("the site-info popup closed")?;
        let stop: Button = in_popup(&popup, name)?;
        super::dialog_steps::invoke(&stop).map_err(|e| e.to_string())?;
        exec::sleep(Duration::from_millis(800)).await;
    }
    let gone = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (window.capture_button_shown().is_none() && !window.tab_capture_shown(tab.id)).then_some(())
    })
    .await;
    let ended = page_value(tab, "String(window.__ended)").await;
    shoot_clear(window, out_dir, "24h-stop-ends-the-capture", steps, |_| {
        json!({ "ended_events": ended, "indicators_gone": gone.is_some(), "ok": ended.as_deref() == Some("2") && gone.is_some() })
    })
    .await;
    window.hide_connection();
    Ok(())
}

/// A screen share the engine's picker grants (it picks the whole screen by itself in scripted
/// runs): the sharing bar over the page, and its Stop sharing ending the share.
async fn screen_share(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let gesture = json!({ "expression": SHARE_SCREEN, "userGesture": true });
    devtools(tab, "Runtime.evaluate", &gesture).await?;
    let result = page_value(tab, "window.__gdm").await;
    let bar = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window.share_bar_shown()
    })
    .await;
    exec::sleep(Duration::from_millis(500)).await;
    let tab_icon = window.tab_capture_shown(tab.id);
    shoot_clear(window, out_dir, "24k-sharing-the-screen", steps, |_| {
        json!({
            "page": result,
            "bar": bar,
            "tab_icon": tab_icon,
            "ok": result.as_deref() == Some("ok") && tab_icon
                && bar.as_deref().is_some_and(|b| b.starts_with("Sharing your screen with 127.0.0.1:")),
        })
    })
    .await;
    eval(
        tab,
        "window.__ended = 0; window.__screen && window.__screen.getTracks()\
        .forEach(t => t.addEventListener('ended', () => window.__ended++)); 0",
    )
    .await?;
    super::dialog_steps::invoke(&window.stop_sharing_button()).map_err(|e| e.to_string())?;
    let gone = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window.share_bar_shown().is_none().then_some(())
    })
    .await;
    let ended = page_value(tab, "String(window.__ended)").await;
    steps.push(json!({
        "name": "24l-stop-sharing",
        "ended_events": ended,
        "bar_gone": gone.is_some(),
        "ok": ended.as_deref() == Some("1") && gone.is_some(),
    }));
    Ok(())
}

/// A background tab's request waits for its tab to be selected, as in Chrome, and the prompt
/// goes when the tab navigates to another site.
async fn background_tab(
    window: &Rc<BrowserWindow>,
    server: &FixtureServer,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let tab = window
        .open_url_tab(server.url("/page2.html").as_str(), false)
        .map_err(|e| e.to_string())?;
    wait_loaded(&tab).await?;
    eval(&tab, ASK_LOCATION).await?;
    exec::sleep(Duration::from_millis(1000)).await;
    let while_behind = window.permission_prompt().is_some();
    let index = window
        .tabs_in_order()
        .iter()
        .position(|t| t.id == tab.id)
        .ok_or("the background tab is gone")?;
    window.run(Command::SelectTab(
        u8::try_from(index).map_err(|e| e.to_string())?,
    ));
    let prompt = wait_prompt(window).await;
    let heading = prompt.as_ref().map(|p| text_of(p, "PromptHeading"));
    shoot_clear(
        window,
        out_dir,
        "24m-background-request-waits-for-its-tab",
        steps,
        |_| {
            json!({
                "prompt_while_behind": while_behind,
                "heading_once_selected": heading,
                "ok": !while_behind && heading.as_deref() == Some("Know your location?"),
            })
        },
    )
    .await;
    tab.navigate("data:text/html,<title>Elsewhere</title>elsewhere");
    let gone = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window.permission_prompt().is_none().then_some(())
    })
    .await;
    steps.push(json!({
        "name": "24n-navigating-away-takes-the-prompt-down",
        "ok": gone.is_some(),
    }));
    window.close_tab(tab.id);
    Ok(())
}

/// The Settings page lists the fixture site's blocked notifications and screen sharing.
async fn settings(
    window: &Rc<BrowserWindow>,
    origin: &Origin,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let preview = dialogs::preview(window, Dialog::Settings).map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(500)).await;
    let categories: ListView = preview
        .find("SettingsCategories")
        .map_err(|e| e.to_string())?;
    let index = dialogs::SETTINGS_CATEGORIES
        .iter()
        .position(|c| c.panel == "SitePermissionsPanel")
        .ok_or("no Site permissions category")?;
    categories
        .cast::<Selector>()
        .and_then(|s| s.SetSelectedIndex(index as i32))
        .map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(500)).await;
    let site = preview
        .find::<TextBlock>("Site0")
        .and_then(|t| t.Text())
        .map(|t| t.to_string())
        .unwrap_or_default();
    let listed = ["SiteChoice0notifications", "SiteChoice0screen_share"]
        .iter()
        .all(|name| preview.find::<UIElement>(name).is_ok());
    shoot_clear(
        window,
        out_dir,
        "24j-settings-site-permissions",
        steps,
        |_| {
            json!({
                "site": site,
                "notifications_and_screen_sharing": listed,
                "ok": site == origin.host_for_display() && listed,
            })
        },
    )
    .await;
    Ok(())
}
