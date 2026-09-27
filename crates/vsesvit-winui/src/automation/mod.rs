//! `--ui-smoke OUT_DIR` drives the real window the way a user would, beyond what the self-test
//! checks: tab commands and page shortcuts, every tab layout, the collapsed pane, the dialogs,
//! the popup, a forwarded second launch and the light theme. It saves an in-app screenshot per
//! step, writes `smoke.json` and exits with 0 only if every step held. Nothing here sends OS
//! input or activates a window: page input goes through the DevTools protocol, and dialogs are
//! shown as previews over the window instead of modally.

use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::extensions::InstallSource;
use vsesvit_core::prefs::{TabsPosition, Theme};
use vsesvit_core::testkit::{self, FixtureServer};

mod dialog_steps;

use crate::browser::Browser;
use crate::layout;
use crate::popup::Activation;
use crate::shortcuts::Command;
use crate::tab::Tab;
use crate::window::BrowserWindow;
use crate::{app, capture, exec};

const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const STEP_TIMEOUT: Duration = Duration::from_secs(10);
const SECOND_TAB: &str = "data:text/html,<title>Second tab</title>\
    <link rel=\"icon\" href=\"data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'>\
    <circle cx='8' cy='8' r='7' fill='crimson'/></svg>\">\
    <body style='font:24px sans-serif'><h1>Second tab</h1></body>";

/// Starts from a fresh profile when the run uses the one inside `out_dir`.
pub(crate) fn prepare(out_dir: &Path, profile_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    if profile_dir == out_dir.join("profile") {
        match std::fs::remove_dir_all(profile_dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

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
    let server = FixtureServer::start().map_err(|e| format!("fixture server: {e}"))?;
    let window = browser.windows().into_iter().next().ok_or("no window")?;
    let first = window.active_tab().ok_or("no tab")?;
    wait_loaded(&first).await?;

    let crx = out_dir.join("probe.crx");
    std::fs::write(&crx, testkit::probe_crx()).map_err(|e| e.to_string())?;
    let source = InstallSource::from_path(&crx).map_err(|e| e.to_string())?;
    let probe = browser.install_extension(source, &|_| {}).await;
    steps.push(json!({
        "name": "00-install-probe",
        "result": format!("{:?}", probe.as_ref().map(|e| (&e.engine_id, &e.verification))),
        "ok": probe.as_ref().is_ok_and(|e| e.engine_id.as_deref() == Some(testkit::PROBE_ID)),
    }));

    let index = server.url("/index.html");
    window.address_submitted(index.as_str());
    wait_title(&first, "Vsesvit fixture").await?;
    let second = window
        .open_url_tab(SECOND_TAB, false)
        .map_err(|e| e.to_string())?;
    wait_loaded(&second).await?;
    exec::wait_for(Duration::from_secs(3), Duration::from_millis(100), || {
        second.has_favicon().then_some(())
    })
    .await;
    shoot(&window, out_dir, "01-left-pane", steps, |w| {
        let seen = observed_layout(w);
        json!({
            "tabs": w.tab_count(),
            "address": w.address_text(),
            "layout": format!("{seen:?}"),
            "favicon": second.has_favicon(),
            "ok": w.tab_count() == 2 && seen == Some(TabsPosition::Left) && w.address_text() == index.as_str(),
        })
    })
    .await;
    if let Some(core) = first.core() {
        match capture::web_png(core).await {
            Ok(png) => save(out_dir, "01-first-tab-web", &png)?,
            Err(e) => log::warn!("web capture: {e}"),
        }
    }

    window.run(Command::SelectTab(1));
    let options = browser
        .engine()
        .find_options("e")
        .map_err(|e| e.to_string())?;
    let matches = exec::timeout(STEP_TIMEOUT, second.find(options))
        .await
        .unwrap_or_else(|| Err(windows_core::Error::empty()));
    exec::sleep(Duration::from_millis(500)).await;
    shoot(&window, out_dir, "02-find", steps, |w| {
        json!({
            "term": "e",
            "matches": format!("{matches:?}"),
            "ok": matches.as_ref().is_ok_and(|n| *n >= 1) && active_is(w, &second),
        })
    })
    .await;
    second.stop_find();

    window.run(Command::CloseTab);
    exec::sleep(Duration::from_millis(500)).await;
    steps.push(json!({
        "name": "03-close-tab",
        "tabs": window.tab_count(),
        "ok": window.tab_count() == 1 && active_is(&window, &first),
    }));
    window.run(Command::ReopenClosedTab);
    let reopened = wait_for_tab_count(&window, 2).await?;
    wait_loaded(&reopened).await?;
    steps.push(json!({
        "name": "04-reopen-closed-tab",
        "url_prefix": reopened.state().url.chars().take(30).collect::<String>(),
        "ok": active_is(&window, &reopened) && reopened.state().url.starts_with("data:text/html"),
    }));
    window.close_tab(reopened.id);

    // Keyboard shortcuts typed into the page reach the injected script as trusted key events.
    press(&first, 0x54, 2).await?;
    let blank = wait_for_tab_count(&window, 2).await?;
    wait_loaded(&blank).await?;
    let opened_blank = active_is(&window, &blank) && blank.state().url == "about:blank";
    // The blank tab can report loaded while its final document (the one with the page script)
    // is still being created, so a key press that found no script is repeated once.
    let mut presses = 0;
    let mut closed = false;
    while !closed && presses < 2 {
        presses += 1;
        press(&blank, 0x57, 2).await?;
        closed = exec::wait_for(STEP_TIMEOUT / 2, Duration::from_millis(100), || {
            (window.tab_count() == 1).then_some(())
        })
        .await
        .is_some();
    }
    steps.push(json!({
        "name": "05-page-shortcuts",
        "ctrl_t_opened_blank_tab": opened_blank,
        "ctrl_w_closed_it": closed,
        "ctrl_w_presses": presses,
        "ok": opened_blank && closed && active_is(&window, &first),
    }));

    // window.open becomes a tab right after its opener, and keeps window.opener.
    eval(&first, "window.open('/page2.html'); 0").await?;
    let popup_tab = wait_for_tab_count(&window, 2).await?;
    wait_loaded(&popup_tab).await?;
    let has_opener = eval(&popup_tab, "window.opener !== null")
        .await
        .unwrap_or_default()
        == "true";
    let order_ok = window
        .tabs_in_order()
        .get(1)
        .is_some_and(|t| t.id == popup_tab.id);
    steps.push(json!({
        "name": "06-window-open",
        "url": popup_tab.state().url,
        "ok": has_opener && order_ok && active_is(&window, &popup_tab),
    }));
    window.close_tab(popup_tab.id);
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
    steps.push(json!({
        "name": "07-ctrl-click",
        "opened": background.as_ref().map(|t| t.state().url),
        "ok": background.is_some() && active_is(&window, &first) && window.tab_count() == 2,
    }));

    // Bookmarks: the star, plus a folder on the bar with a link in it.
    window.star_clicked();
    let folder = browser.core(|p| {
        let mut bookmarks = p.bookmarks();
        let folder = bookmarks.add_folder(BookmarkId::TOOLBAR, InsertAt::End, "Fixture folder")?;
        bookmarks.add_url(
            folder,
            InsertAt::End,
            "Second fixture page",
            &server.url("/page2.html"),
        )?;
        Ok::<_, vsesvit_core::Error>(folder)
    });
    browser.bookmarks_changed();
    exec::sleep(Duration::from_millis(300)).await;
    shoot(&window, out_dir, "08-bookmarks-bar", steps, |w| {
        json!({
            "bar": format!("{:?}", w.bookmarks_bar_items()),
            "starred": first.state().starred,
            "ok": folder.is_ok() && w.bookmarks_bar_items().len() == 2 && first.state().starred,
        })
    })
    .await;

    // The tab layouts, set through the preference as the Settings dialog does.
    for (position, name) in [
        (TabsPosition::Right, "09-right-pane"),
        (TabsPosition::Top, "10-top-strip"),
        (TabsPosition::Left, "11-left-pane-again"),
    ] {
        browser.set_tabs_position(position);
        let seen = wait_layout(&window, position).await;
        shoot(&window, out_dir, name, steps, |w| {
            json!({
                "layout": format!("{seen:?}"),
                "tabs": w.tabs_in_order().len(),
                "ok": seen == Some(position) && w.tab_count() == 2,
            })
        })
        .await;
    }
    browser.set_tab_pane_collapsed(true);
    exec::sleep(Duration::from_millis(400)).await;
    shoot(&window, out_dir, "12-collapsed-pane", steps, |w| {
        let (pane, _, _) = w.layout_geometry();
        json!({
            "pane": format!("{pane:?}"),
            "ok": pane.is_some_and(|p| p.width <= 49.0) && w.is_pane_collapsed(),
        })
    })
    .await;
    browser.set_tab_pane_collapsed(false);

    let count = window.show_suggestions("fixture");
    steps.push(json!({
        "name": "13-omnibox-suggestions",
        "labels": window.suggestion_labels(),
        "ok": count >= 2,
    }));

    let page2 = server.url("/page2.html");
    if let Err(e) = dialog_steps::settings(&window, out_dir, steps).await {
        dialog_steps::failed(steps, "14-settings-dialog", &e);
    }
    if let Err(e) = dialog_steps::extensions(browser, &window, out_dir, &crx, steps).await {
        dialog_steps::failed(steps, "15-extensions-dialog", &e);
    }
    match folder {
        Ok(folder) => {
            let result =
                dialog_steps::bookmarks(browser, &window, out_dir, folder, &page2, steps).await;
            if let Err(e) = result {
                dialog_steps::failed(steps, "16-bookmarks-dialog", &e);
            }
        }
        Err(e) => steps
            .push(json!({ "name": "16-bookmarks-dialog", "error": e.to_string(), "ok": false })),
    }
    if let Err(e) = dialog_steps::history(browser, &window, out_dir, &page2, steps).await {
        dialog_steps::failed(steps, "17-history-dialog", &e);
    }

    match window.open_extension_popup(testkit::PROBE_ID, Activation::Keep) {
        Ok(popup) => {
            let title = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
                popup.title().filter(|t| t.starts_with("visits="))
            })
            .await;
            exec::sleep(Duration::from_millis(500)).await;
            shoot(
                &window,
                out_dir,
                "18-extension-popup",
                steps,
                |_| json!({ "title": title, "ok": title.is_some() }),
            )
            .await;
            popup.hide();
        }
        Err(e) => {
            steps.push(json!({ "name": "18-extension-popup", "error": e.to_string(), "ok": false }))
        }
    }

    // A second launch on this profile hands its URL to this process and exits.
    let before = window.tab_count();
    let forwarded = server.url("/page2.html");
    let launch = second_launch(browser, forwarded.as_str()).await;
    let arrived = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window
            .tabs_in_order()
            .into_iter()
            .find(|t| t.state().url == forwarded.as_str())
    })
    .await;
    if let Some(tab) = &arrived {
        let _ = wait_loaded(tab).await;
    }
    shoot(&window, out_dir, "19-forwarded-launch", steps, |w| {
        json!({
            "second_process": launch,
            "tabs_before": before,
            "tabs_after": w.tab_count(),
            "ok": launch.as_deref() == Ok("exit code 0") && arrived.is_some(),
        })
    })
    .await;

    browser.set_theme(Theme::Light);
    exec::sleep(Duration::from_millis(500)).await;
    shoot(
        &window,
        out_dir,
        "20-light-theme",
        steps,
        |_| json!({ "ok": true }),
    )
    .await;
    browser.set_theme(Theme::System);

    let other = browser
        .open_blank_window(crate::window::Show::NoActivate)
        .map_err(|e| e.to_string())?;
    let other_tab = other.active_tab().ok_or("second window has no tab")?;
    wait_loaded(&other_tab).await?;
    shoot(&other, out_dir, "21-second-window", steps, |w| {
        json!({ "windows": browser.windows().len(), "tabs": w.tab_count(), "ok": browser.windows().len() == 2 })
    })
    .await;
    other.close_tab(other_tab.id);
    exec::sleep(Duration::from_millis(300)).await;
    Ok(())
}

/// Runs this executable again on the same profile; it should forward and exit at once.
async fn second_launch(browser: &Browser, url: &str) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = Process::new(exe)
        .arg("--profile-dir")
        .arg(browser.profile_dir())
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(format!("exit code {}", status.code().unwrap_or(-1))),
            Ok(None) if Instant::now() < deadline => exec::sleep(Duration::from_millis(100)).await,
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the second process did not exit within 20 s".into());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn observed_layout(window: &BrowserWindow) -> Option<TabsPosition> {
    let (pane, strip, view) = window.layout_geometry();
    view.and_then(|view| layout::observed(pane, strip, view))
}

pub(super) async fn wait_layout(
    window: &Rc<BrowserWindow>,
    want: TabsPosition,
) -> Option<TabsPosition> {
    exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (observed_layout(window) == Some(want)).then_some(())
    })
    .await;
    exec::sleep(Duration::from_millis(300)).await;
    observed_layout(window)
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

async fn wait_title(tab: &Rc<Tab>, title: &str) -> Result<(), String> {
    exec::wait_for(LOAD_TIMEOUT, Duration::from_millis(100), || {
        let state = tab.state();
        (state.title == title && !state.loading).then_some(())
    })
    .await
    .ok_or_else(|| format!("tab {} never showed {title:?}: {:?}", tab.id, tab.state()))?;
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

pub(super) async fn shoot(
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
