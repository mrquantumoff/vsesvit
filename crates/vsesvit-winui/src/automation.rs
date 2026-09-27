//! Scripted UI runs. `--ui-smoke OUT_DIR` drives the real window the way a user would, saves
//! in-app screenshots, writes `smoke.json` and exits with 0 only if every step held. Nothing
//! here sends OS input or activates the window: page input goes through the DevTools protocol,
//! which delivers trusted events straight to the renderer. The self-test builds on these steps.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::bookmarks_bar::BarItem;
use crate::browser::Browser;
use crate::dialogs::{self, Dialog};
use crate::popup::Activation;
use crate::shortcuts::Command;
use crate::tab::Tab;
use crate::window::{BrowserWindow, Show};
use crate::{app, capture, exec, omnibox};

const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const STEP_TIMEOUT: Duration = Duration::from_secs(10);
const SECOND_TAB: &str = "data:text/html,<title>Second tab</title>\
    <link rel=\"icon\" href=\"data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'>\
    <circle cx='8' cy='8' r='7' fill='crimson'/></svg>\">\
    <body style='font:24px sans-serif'><h1>Second tab</h1></body>";

pub(crate) async fn ui_smoke(browser: Rc<Browser>, out_dir: PathBuf) {
    let mut steps = Vec::new();
    let result = run(&browser, &out_dir, &mut steps).await;
    let ok = result.is_ok() && steps.iter().all(|s| s["ok"] == true);
    let report = json!({ "ok": ok, "error": result.err(), "steps": steps });
    let written = std::fs::write(
        out_dir.join("smoke.json"),
        serde_json::to_vec_pretty(&report).unwrap_or_default(),
    );
    log::info!(
        "ui smoke: ok={ok}, report in {} ({written:?})",
        out_dir.display()
    );
    app::exit(if ok { 0 } else { 1 });
}

async fn run(browser: &Rc<Browser>, out_dir: &Path, steps: &mut Vec<Value>) -> Result<(), String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let window = browser.windows().into_iter().next().ok_or("no window")?;
    let first = window.active_tab().ok_or("no tab")?;
    wait_loaded(&first).await?;
    let start_tabs = browser.config().start_urls.len().max(1);
    let first_state = first.state();
    shoot(&window, out_dir, "01-first-tab", steps, |w| {
        json!({
            "tabs": w.tab_count(),
            "url": first_state.url,
            "address": w.address_text(),
            "title": first_state.title,
            "ok": w.tab_count() == start_tabs
                && w.address_text() == first_state.url
                && !first_state.title.is_empty()
                && first_state.title != "New tab",
        })
    })
    .await;
    if let Some(core) = first.core() {
        match capture::web_png(core).await {
            Ok(png) => save(out_dir, "01-first-tab-web", &png)?,
            Err(e) => log::warn!("web capture: {e}"),
        }
    }

    // A second tab: the second start URL, or one opened here.
    let second = match window.tabs_in_order().get(1).cloned() {
        Some(tab) => tab,
        None => window
            .open_url_tab(SECOND_TAB, false)
            .map_err(|e| e.to_string())?,
    };
    window.run(Command::SelectTab(1));
    wait_loaded(&second).await?;
    exec::wait_for(Duration::from_secs(3), Duration::from_millis(100), || {
        second.has_favicon().then_some(())
    })
    .await;
    let second_state = second.state();
    shoot(&window, out_dir, "02-second-tab", steps, |w| {
        json!({
            "tabs": w.tab_count(),
            "address": w.address_text(),
            "title": second_state.title,
            "favicon": second.has_favicon(),
            "ok": w.tab_count() == 2 && active_is(w, &second) && w.address_text() == second_state.url,
        })
    })
    .await;

    let options = browser
        .engine()
        .find_options("e")
        .map_err(|e| e.to_string())?;
    let matches = exec::timeout(STEP_TIMEOUT, second.find(options))
        .await
        .unwrap_or_else(|| Err(windows_core::Error::empty()));
    exec::sleep(Duration::from_millis(500)).await;
    shoot(&window, out_dir, "03-find", steps, |_| {
        json!({ "term": "e", "matches": format!("{matches:?}"), "ok": matches.as_ref().is_ok_and(|n| *n >= 1) })
    })
    .await;
    second.stop_find();

    window.run(Command::CloseTab);
    exec::sleep(Duration::from_millis(500)).await;
    shoot(&window, out_dir, "04-closed-second-tab", steps, |w| {
        json!({
            "tabs": w.tab_count(),
            "address": w.address_text(),
            "ok": w.tab_count() == 1 && active_is(w, &first) && w.address_text() == first.state().url,
        })
    })
    .await;

    window.run(Command::ReopenClosedTab);
    let reopened = wait_for_tab_count(&window, 2).await?;
    wait_loaded(&reopened).await?;
    let reopened_url = reopened.state().url;
    steps.push(json!({
        "name": "05-reopen-closed-tab",
        "url": reopened_url,
        "ok": active_is(&window, &reopened) && reopened_url == second_state.url,
    }));
    window.close_tab(reopened.id);

    // Keyboard shortcuts typed into the page reach the injected script as trusted key events.
    press(&first, 0x54, 2).await?;
    let blank = wait_for_tab_count(&window, 2).await?;
    wait_loaded(&blank).await?;
    let opened_blank = active_is(&window, &blank) && blank.state().url == "about:blank";
    press(&blank, 0x57, 2).await?;
    let closed = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (window.tab_count() == 1).then_some(())
    })
    .await
    .is_some();
    shoot(&window, out_dir, "06-page-shortcuts", steps, |w| {
        json!({
            "ctrl_t_opened_blank_tab": opened_blank,
            "ctrl_w_closed_it": closed,
            "ok": opened_blank && closed && active_is(w, &first),
        })
    })
    .await;

    // window.open becomes a tab right after its opener, and keeps window.opener.
    eval(&first, "window.open('/page2.html'); 0").await?;
    let popup = wait_for_tab_count(&window, 2).await?;
    wait_loaded(&popup).await?;
    let has_opener = eval(&popup, "window.opener !== null")
        .await
        .unwrap_or_default()
        == "true";
    let order_ok = window
        .tabs_in_order()
        .get(1)
        .is_some_and(|t| t.id == popup.id);
    shoot(&window, out_dir, "07-window-open", steps, |w| {
        json!({
            "url": popup.state().url,
            "has_opener": has_opener,
            "after_opener": order_ok,
            "ok": has_opener && order_ok && active_is(w, &popup),
        })
    })
    .await;
    window.close_tab(popup.id);
    window.run(Command::SelectTab(0));

    // Ctrl+click on a link opens it in a background tab.
    const LINK_CENTER: &str = "(() => { const a = document.querySelector('a[href]'); \
        if (!a) return null; const r = a.getBoundingClientRect(); \
        return [r.x + r.width / 2, r.y + r.height / 2]; })()";
    let link = eval(&first, LINK_CENTER).await?;
    let background = match serde_json::from_str::<Vec<f64>>(&link) {
        Ok(point) if point.len() == 2 => {
            ctrl_click(&first, point[0], point[1]).await?;
            let tab = wait_for_tab_count(&window, 2).await?;
            wait_loaded(&tab).await?;
            Some(tab)
        }
        _ => None,
    };
    shoot(&window, out_dir, "08-ctrl-click", steps, |w| {
        json!({
            "link_found": background.is_some(),
            "opened": background.as_ref().map(|t| t.state().url),
            "ok": background.is_some() && active_is(w, &first) && w.tab_count() == 2,
        })
    })
    .await;
    if let Some(tab) = background {
        window.close_tab(tab.id);
    }

    // The bar's widgets, with items this run supplies itself (bookmark data arrives with core).
    let sample = vec![
        BarItem::Link {
            title: "Smoke link".into(),
            url: first.state().url,
        },
        BarItem::Folder {
            title: "Smoke folder".into(),
            children: vec![BarItem::Link {
                title: "Second tab".into(),
                url: SECOND_TAB.into(),
            }],
        },
    ];
    window.set_bookmarks_bar(&sample);
    exec::sleep(Duration::from_millis(300)).await;
    shoot(
        &window,
        out_dir,
        "09-bookmarks-bar",
        steps,
        |w| json!({ "ok": w.bookmarks_bar_items() == sample }),
    )
    .await;
    window.set_bookmarks_bar(&browser.bookmarks_bar_items());

    // Enter in the address box: a typed fragment is a same-document navigation.
    let typed = format!(
        "{}#typed",
        first
            .state()
            .url
            .split_once("://")
            .map_or("", |(_, rest)| rest)
    );
    let expected = omnibox::navigation_target(&typed).unwrap_or_default();
    window.address_submitted(&typed);
    let committed = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (first.state().url == expected).then_some(())
    })
    .await
    .is_some();
    shoot(&window, out_dir, "10-address-enter", steps, |w| {
        json!({
            "typed": typed,
            "url": first.state().url,
            "address": w.address_text(),
            "can_go_back": first.state().can_go_back,
            "ok": committed && w.address_text() == expected && first.state().can_go_back,
        })
    })
    .await;

    // A second window shares the engine environment.
    let other = browser
        .open_window(&[], Show::NoActivate)
        .map_err(|e| e.to_string())?;
    let other_tab = other.active_tab().ok_or("second window has no tab")?;
    wait_loaded(&other_tab).await?;
    shoot(&other, out_dir, "11-second-window", steps, |w| {
        json!({ "windows": browser.windows().len(), "tabs": w.tab_count(), "ok": browser.windows().len() == 2 })
    })
    .await;
    other.close_tab(other_tab.id);
    exec::sleep(Duration::from_millis(300)).await;

    let built: Vec<Value> = Dialog::ALL
        .iter()
        .map(|&d| json!({ "dialog": format!("{d:?}"), "built": dialogs::build(&window, d).map(drop).map_err(|e| e.to_string()) }))
        .collect();
    let all_built = built.iter().all(|d| d["built"].get("Ok").is_some());
    steps.push(json!({ "name": "12-dialogs-build", "dialogs": built, "ok": all_built, "windows": browser.windows().len() }));

    if !browser.extension_actions().is_empty() {
        window
            .open_extension_popup(0, Activation::Keep)
            .map_err(|e| format!("popup: {e}"))?;
        exec::sleep(Duration::from_secs(3)).await;
        shoot(
            &window,
            out_dir,
            "13-extension-popup",
            steps,
            |_| json!({ "ok": true }),
        )
        .await;
    }
    Ok(())
}

fn active_is(window: &BrowserWindow, tab: &Tab) -> bool {
    window.active_tab().is_some_and(|t| t.id == tab.id)
}

async fn wait_for_tab_count(window: &Rc<BrowserWindow>, count: usize) -> Result<Rc<Tab>, String> {
    exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        let tabs = window.tabs_in_order();
        (tabs.len() == count)
            .then(|| tabs.iter().max_by_key(|t| t.id).cloned())
            .flatten()
    })
    .await
    .ok_or_else(|| format!("expected {count} tabs, have {}", window.tab_count()))
}

async fn wait_loaded(tab: &Rc<Tab>) -> Result<(), String> {
    exec::wait_for(LOAD_TIMEOUT, Duration::from_millis(100), || {
        let state = tab.state();
        (tab.is_ready() && !state.loading && !state.url.is_empty()).then_some(())
    })
    .await
    .ok_or_else(|| format!("tab {} did not finish loading: {:?}", tab.id, tab.state()))?;
    // Let XAML lay out and the engine present the new frame.
    exec::sleep(Duration::from_millis(700)).await;
    Ok(())
}

/// A key press with DevTools modifiers (1 Alt, 2 Ctrl, 4 Meta, 8 Shift). The press may close
/// the tab, and then its DevTools calls never answer, hence the bound.
async fn press(tab: &Tab, vk: u16, modifiers: u8) -> Result<(), String> {
    let key = |kind| json!({ "type": kind, "modifiers": modifiers, "windowsVirtualKeyCode": vk });
    devtools(tab, "Input.dispatchKeyEvent", &key("rawKeyDown")).await?;
    // The key down may have closed the tab; a failed key up then means nothing.
    let _ = devtools(tab, "Input.dispatchKeyEvent", &key("keyUp")).await;
    Ok(())
}

async fn ctrl_click(tab: &Tab, x: f64, y: f64) -> Result<(), String> {
    for kind in ["mousePressed", "mouseReleased"] {
        let params = json!({
            "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1, "modifiers": 2,
        });
        devtools(tab, "Input.dispatchMouseEvent", &params).await?;
    }
    Ok(())
}

async fn devtools(tab: &Tab, method: &str, params: &Value) -> Result<(), String> {
    match exec::timeout(
        Duration::from_secs(2),
        tab.devtools(method, &params.to_string()),
    )
    .await
    {
        Some(Err(e)) => Err(format!("{method}: {e}")),
        Some(Ok(_)) | None => Ok(()),
    }
}

async fn eval(tab: &Tab, script: &str) -> Result<String, String> {
    exec::timeout(STEP_TIMEOUT, tab.eval(script))
        .await
        .ok_or_else(|| format!("script timed out in tab {}", tab.id))?
        .map_err(|e| e.to_string())
}

async fn shoot(
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    name: &str,
    steps: &mut Vec<Value>,
    check: impl FnOnce(&BrowserWindow) -> Value,
) {
    let mut step = check(window);
    match window.capture().await {
        Ok(shot) => {
            let saved = save(out_dir, name, &shot.png);
            step["screenshot"] = json!({
                "file": format!("{name}.png"),
                "width": shot.width,
                "height": shot.height,
                "flat": shot.flat,
                "saved": saved.is_ok(),
            });
            if shot.flat || saved.is_err() {
                step["ok"] = json!(false);
            }
        }
        Err(e) => {
            step["screenshot"] = json!({ "error": e.to_string() });
            step["ok"] = json!(false);
        }
    }
    step["name"] = json!(name);
    log::info!("ui smoke step {step}");
    steps.push(step);
}

fn save(out_dir: &Path, name: &str, png: &[u8]) -> Result<(), String> {
    let path = out_dir.join(format!("{name}.png"));
    std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))
}
