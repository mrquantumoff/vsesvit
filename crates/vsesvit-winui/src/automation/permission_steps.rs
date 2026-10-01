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

use super::{STEP_TIMEOUT, devtools, eval, settings_on, shoot, wait_loaded};
use crate::bindings::*;
use crate::browser::Browser;
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
    exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window.permission_prompt()
    })
    .await?;
    // The flyout's opening animation, and its input guard; a request that joins it meanwhile
    // gives it new content.
    exec::sleep(Duration::from_millis(600)).await;
    window.permission_prompt()
}

/// An element of the site-info popup's Permissions section, which has a namescope of its own.
fn in_popup<T: Interface>(popup: &FrameworkElement, name: &str) -> Result<T, String> {
    popup
        .cast::<DependencyObject>()
        .ok()
        .and_then(|root| xaml::find_named(&root, name))
        .ok_or_else(|| format!("no {name} in the site-info popup"))
}

/// The text of the `TextBlock` named `name` under `root`, or "" without one.
pub(super) fn text_of(root: &FrameworkElement, name: &str) -> String {
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

/// The site-info popup, opened from the security icon as a click does.
pub(super) async fn open_site_info(window: &Rc<BrowserWindow>) -> Result<FrameworkElement, String> {
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
    shoot_clear(window, out_dir, "24a-permission-prompt", steps, |_| {
        json!({
            "heading": heading,
            "ok": heading == "Show notifications?",
        })
    })
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
    steps.push(engine_state(&tab, "24c2-the-page-reads-granted", ("granted", "granted")).await);

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
    steps.push(engine_state(&tab, "24e2-the-page-reads-denied", ("denied", "denied")).await);

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
    prompt_rules(browser, window, server, out_dir, steps).await?;
    other_tabs_settle(browser, window, server, steps).await?;
    browser
        .core(|c| c.site_permissions().reset_site(&origin))
        .map_err(|e| e.to_string())?;
    crate::permissions::settings_changed(browser);
    steps.push(
        engine_state(
            &tab,
            "24s-a-reset-clears-what-the-page-reads",
            ("default", "prompt"),
        )
        .await,
    );
    window.close_tab(tab.id);
    Ok(())
}

/// What the page reads back, `Notification.permission` and `navigator.permissions.query`,
/// once it is `want` (the engine's copy is written a moment after the setting).
async fn engine_state(tab: &Tab, name: &str, want: (&str, &str)) -> Value {
    const READ: &str = "window.__query = ''; navigator.permissions.query({ name: 'notifications' })\
        .then(s => window.__query = s.state); Notification.permission";
    let mut seen = (String::new(), String::new());
    let deadline = Instant::now() + STEP_TIMEOUT;
    while Instant::now() < deadline {
        let permission = eval(tab, READ).await.unwrap_or_default();
        let query = page_value(tab, "window.__query").await.unwrap_or_default();
        seen = (permission.trim_matches('"').to_owned(), query);
        if (seen.0.as_str(), seen.1.as_str()) == want {
            break;
        }
        exec::sleep(Duration::from_millis(200)).await;
    }
    json!({
        "name": name,
        "notification_permission": seen.0,
        "permissions_query": seen.1,
        "ok": (seen.0.as_str(), seen.1.as_str()) == want,
    })
}

/// An answer that stores a setting answers the same site's requests waiting in other tabs,
/// without a prompt of their own. `localhost` is another origin than `127.0.0.1`, with
/// nothing stored.
async fn other_tabs_settle(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    server: &FixtureServer,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let url = format!("http://localhost:{}/page2.html", server.port());
    let origin = Origin::parse(&url).ok_or("no localhost origin")?;
    let behind = window
        .open_url_tab(&url, false)
        .map_err(|e| e.to_string())?;
    wait_loaded(&behind).await?;
    eval(&behind, ASK_NOTIFICATIONS).await?;
    let front = window.open_url_tab(&url, true).map_err(|e| e.to_string())?;
    wait_loaded(&front).await?;
    eval(&front, ASK_NOTIFICATIONS).await?;
    let prompt = wait_prompt(window).await.ok_or("no notification prompt")?;
    answer(&prompt, "AnswerAllowWhileVisiting")?;
    let result = page_value(&behind, "window.__notify").await;
    let before = window.permission_prompts_shown();
    select_tab(window, &behind)?;
    exec::sleep(Duration::from_millis(800)).await;
    steps.push(json!({
        "name": "24t-an-answer-settles-the-same-sites-requests-in-other-tabs",
        "other_tab": result,
        "prompts_on_selecting_it": window.permission_prompts_shown() - before,
        "ok": result.as_deref() == Some("granted") && window.permission_prompts_shown() == before,
    }));
    browser
        .core(|c| c.site_permissions().reset_site(&origin))
        .map_err(|e| e.to_string())?;
    window.close_tab(front.id);
    window.close_tab(behind.id);
    // The history steps count the visits the other steps make.
    let visited = vsesvit_core::Url::parse(&url).map_err(|e| e.to_string())?;
    browser
        .core(|c| c.history().delete_url(&visited))
        .map_err(|e| e.to_string())?;
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
    exec::sleep(Duration::from_millis(300)).await;
    block_the_camera(window, tab, steps).await
}

/// Block chosen for the camera in the site-info popup while both capture stops the camera
/// and keeps the microphone. The one-time grant still covers the new `getUserMedia`.
async fn block_the_camera(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let before = window.permission_prompts_shown();
    eval(tab, OPEN_CAMERA).await?;
    let opened = page_value(tab, "window.__gum").await;
    exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (window.capture_button_shown().as_deref() == Some("Using your camera and microphone"))
            .then_some(())
    })
    .await;
    let popup = open_site_info(window).await?;
    select(&popup, "PermissionChoiceCamera", 3)?;
    let left = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window
            .capture_button_shown()
            .filter(|d| d == "Using your microphone")
    })
    .await;
    let ended = page_value(tab, "String(window.__ended)").await;
    steps.push(json!({
        "name": "24h2-blocking-the-camera-while-in-use-stops-only-the-camera",
        "opened": opened,
        "prompts": window.permission_prompts_shown() - before,
        "still_capturing": left,
        "ended_events": ended,
        "ok": opened.as_deref() == Some("ok") && window.permission_prompts_shown() == before
            && left.is_some() && ended.as_deref() == Some("1"),
    }));
    exec::sleep(Duration::from_millis(300)).await;
    let popup = window
        .connection_popup()
        .ok_or("the site-info popup closed")?;
    super::dialog_steps::invoke(&in_popup::<Button>(&popup, "PermissionStopMicrophone")?)
        .map_err(|e| e.to_string())?;
    exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window.capture_button_shown().is_none().then_some(())
    })
    .await;
    window.hide_connection();
    exec::sleep(Duration::from_millis(300)).await;
    allowed_camera_shows(window, tab, steps).await
}

/// With the camera allowed for the site, the engine may let a page open it without asking the
/// shell; the in-use indicators show it all the same.
async fn allowed_camera_shows(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let popup = open_site_info(window).await?;
    select(&popup, "PermissionChoiceCamera", 1)?;
    exec::sleep(Duration::from_millis(300)).await;
    window.hide_connection();
    tab.reload();
    exec::sleep(Duration::from_millis(300)).await;
    wait_loaded(tab).await?;
    let before = window.permission_prompts_shown();
    eval(
        tab,
        "window.__gum = ''; navigator.mediaDevices.getUserMedia({ video: true }).then(s => { \
         window.__stream = s; window.__gum = 'ok'; }, e => window.__gum = e.name); 0",
    )
    .await?;
    let opened = page_value(tab, "window.__gum").await;
    let indicator = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window
            .capture_button_shown()
            .filter(|d| d == "Using your camera")
    })
    .await;
    steps.push(json!({
        "name": "24h3-an-allowed-camera-shows-without-a-prompt",
        "opened": opened,
        "prompts": window.permission_prompts_shown() - before,
        "indicator": indicator,
        "ok": opened.as_deref() == Some("ok") && window.permission_prompts_shown() == before
            && indicator.is_some(),
    }));
    eval(
        tab,
        "window.__stream && window.__stream.getTracks().forEach(t => t.stop()); 0",
    )
    .await?;
    exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window.capture_button_shown().is_none().then_some(())
    })
    .await;
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

const ASK_LOCATION_WATCHED: &str = "window.__geo = ''; navigator.geolocation.getCurrentPosition(\
    () => window.__geo = 'ok', e => window.__geo = 'err' + e.code); 0";

fn select_tab(window: &BrowserWindow, tab: &Tab) -> Result<(), String> {
    let index = window
        .tabs_in_order()
        .iter()
        .position(|t| t.id == tab.id)
        .ok_or("the tab is gone")?;
    window.run(Command::SelectTab(
        u8::try_from(index).map_err(|e| e.to_string())?,
    ));
    Ok(())
}

/// The prompt as soon as it shows, before its input guard ends.
async fn first_sight_of_prompt(window: &BrowserWindow) -> Result<FrameworkElement, String> {
    exec::wait_for(STEP_TIMEOUT, Duration::from_millis(10), || {
        window.permission_prompt()
    })
    .await
    .ok_or("no location prompt".into())
}

/// Chrome's rules for what ends a prompt: an answer pressed right after it shows is ignored;
/// a tab switch or the site-info popup only withdraw it, and it comes back, also when the old
/// flyout's close arrives late; a click elsewhere on its tab dismisses it.
async fn prompt_rules(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    server: &FixtureServer,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let origin = Origin::parse(&server.origin()).ok_or("no fixture origin")?;
    let location = || browser.core(|c| c.site_permissions().get(&origin, Permission::Location));
    let other = window.active_tab().ok_or("no tab")?;
    let tab = window
        .open_url_tab(server.url("/page2.html").as_str(), true)
        .map_err(|e| e.to_string())?;
    wait_loaded(&tab).await?;
    let waiting = || async { eval(&tab, "window.__geo").await.is_ok_and(|v| v == "\"\"") };

    eval(&tab, ASK_LOCATION_WATCHED).await?;
    let prompt = first_sight_of_prompt(window).await?;
    let seen = Instant::now();
    let pressed = exec::wait_for(
        Duration::from_millis(400),
        Duration::from_millis(10),
        || {
            answer(&prompt, "AnswerAllowWhileVisiting")
                .ok()
                .map(|()| seen.elapsed().as_millis())
        },
    )
    .await;
    exec::sleep(Duration::from_millis(700)).await;
    steps.push(json!({
        "name": "24o-an-answer-right-after-the-prompt-shows-is-ignored",
        "pressed_after_ms": pressed,
        "stored": format!("{:?}", location()),
        "ok": pressed.is_some() && location().is_none() && window.permission_prompt().is_some()
            && waiting().await,
    }));

    if std::env::var("VSESVIT_SMOKE_KEYBOARD").as_deref() == Ok("1") {
        steps.push(keyboard_guard(window, &tab, &location).await);
        steps.push(typed_list_makes_way(window, &tab).await);
    }

    select_tab(window, &other)?;
    exec::sleep(Duration::from_millis(100)).await;
    let withdrawn = window.permission_prompt().is_none();
    select_tab(window, &tab)?;
    let back = wait_prompt(window).await.is_some();
    exec::sleep(Duration::from_millis(500)).await;
    steps.push(json!({
        "name": "24p-a-tab-switch-withdraws-the-prompt-and-it-comes-back",
        "withdrawn": withdrawn,
        "back": back,
        "ok": withdrawn && back && window.permission_prompt().is_some() && waiting().await,
    }));

    window.show_connection().map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(600)).await;
    let popup = window.connection_popup();
    let withdrawn = window.permission_prompt().is_none();
    // The site's stored Block shows though the popup opened as the prompt closed.
    let rows = popup
        .as_ref()
        .is_some_and(|p| in_popup::<UIElement>(p, "PermissionRowNotifications").is_ok());
    let popup = popup.is_some();
    shoot_clear(
        window,
        out_dir,
        "24q-site-info-over-a-waiting-prompt",
        steps,
        |_| {
            json!({
                "popup": popup,
                "prompt_withdrawn": withdrawn,
                "notifications_row": rows,
                "ok": popup && withdrawn && rows,
            })
        },
    )
    .await;
    window.hide_connection();
    let back = wait_prompt(window).await.is_some();
    steps.push(json!({
        "name": "24q2-the-prompt-comes-back-when-site-info-closes",
        "ok": back && waiting().await,
    }));

    window.close_prompt_flyout();
    let denied = page_value(&tab, "window.__geo").await;
    steps.push(json!({
        "name": "24r-a-click-elsewhere-on-its-tab-dismisses-the-prompt",
        "page": denied,
        "ok": denied.as_deref() == Some("err1") && location().is_none()
            && window.permission_prompt().is_none(),
    }));
    window.close_tab(tab.id);
    Ok(())
}

/// Enter and Space pressed on the keyboard while the prompt shows, first within its input
/// guard and then after it, answer nothing: the focus is on its heading. Real key presses need
/// the window in the foreground, so this only runs with `VSESVIT_SMOKE_KEYBOARD=1`, and only
/// sends them while this window is the foreground one.
async fn keyboard_guard(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    location: &dyn Fn() -> Option<Setting>,
) -> Value {
    const VK_RETURN: u16 = 0x0D;
    const VK_SPACE: u16 = 0x20;
    let name = "24o2-enter-and-space-on-the-prompt-answer-nothing";
    window.activate();
    let foreground = exec::wait_for(Duration::from_secs(3), Duration::from_millis(50), || {
        window.in_foreground().then_some(())
    })
    .await;
    // Skipped, the step leaves the waiting prompt as it found it.
    if foreground.is_none() {
        return json!({ "name": name, "skipped": "the window could not come to the foreground", "ok": true });
    }
    window.close_prompt_flyout();
    exec::sleep(Duration::from_millis(600)).await;
    if let Err(e) = eval(tab, ASK_LOCATION_WATCHED).await {
        return json!({ "name": name, "error": e, "ok": false });
    }
    if first_sight_of_prompt(window).await.is_err() {
        return json!({ "name": name, "error": "no prompt", "ok": false });
    }
    let mut sent = Vec::new();
    for pause in [0, 700] {
        exec::sleep(Duration::from_millis(pause)).await;
        let ours = window.in_foreground();
        if ours {
            send_keys(&[VK_RETURN, VK_SPACE]);
        }
        sent.push(ours);
    }
    exec::sleep(Duration::from_millis(500)).await;
    let focused = window.focused_name();
    let still_open = window.permission_prompt().is_some();
    json!({
        "name": name,
        "keys_sent": sent,
        "focused": focused,
        "stored": format!("{:?}", location()),
        "ok": sent == [true, true] && still_open && location().is_none()
            && focused.as_deref() == Some("PromptFocus"),
    })
}

/// Typing in the address box opens its suggestion list, and a permission prompt that shows then
/// closes it. Real key presses, so only with `VSESVIT_SMOKE_KEYBOARD=1` (see `keyboard_guard`).
async fn typed_list_makes_way(window: &Rc<BrowserWindow>, tab: &Rc<Tab>) -> Value {
    const TYPED: [u16; 3] = [0x46, 0x49, 0x58];
    let name = "24o3-a-prompt-closes-the-list-typing-opened";
    if !window.in_foreground() {
        return json!({ "name": name, "skipped": "the window is not in the foreground", "ok": true });
    }
    window.close_prompt_flyout();
    exec::sleep(Duration::from_millis(600)).await;
    window.run(Command::FocusAddress);
    exec::sleep(Duration::from_millis(300)).await;
    if !window.in_foreground() {
        return json!({ "name": name, "error": "the window left the foreground", "ok": false });
    }
    send_keys(&TYPED);
    let opened = exec::wait_for(Duration::from_secs(3), Duration::from_millis(50), || {
        window.suggestions_open().then_some(())
    })
    .await
    .is_some();
    if let Err(e) = eval(tab, ASK_LOCATION_WATCHED).await {
        return json!({ "name": name, "error": e, "ok": false });
    }
    let prompt = wait_prompt(window).await.is_some();
    let closed = !window.suggestions_open();
    json!({
        "name": name,
        "list_opened_by_typing": opened,
        "prompt": prompt,
        "list_closed": closed,
        "ok": opened && prompt && closed,
    })
}

fn send_keys(keys: &[u16]) {
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD as u32,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: if up { KEYEVENTF_KEYUP as u32 } else { 0 },
                ..KEYBDINPUT::default()
            },
        },
    };
    let inputs: Vec<INPUT> = keys
        .iter()
        .flat_map(|&vk| [key(vk, false), key(vk, true)])
        .collect();
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
}

/// The Settings page lists the fixture site's blocked notifications and screen sharing.
async fn settings(
    window: &Rc<BrowserWindow>,
    origin: &Origin,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let preview = settings_on(window, "SitePermissionsPanel")
        .await
        .map_err(|e| e.to_string())?;
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
