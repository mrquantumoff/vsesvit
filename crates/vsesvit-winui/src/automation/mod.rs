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
use vsesvit_core::Url;
use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::extensions::{ExtensionId, InstallSource};
use vsesvit_core::prefs::{TabsPosition, Theme, keys};
use vsesvit_core::testkit::{self, FixtureServer};
use windows_core::Interface;

mod bookmark_steps;
mod connection_steps;
mod dialog_steps;
mod progress_steps;
mod tab_steps;
mod toolbar_steps;

use crate::bindings::*;
use crate::bookmarks_bar::BarItem;
use crate::browser::Browser;
use crate::layout;
use crate::popup::Activation;
use crate::shortcuts::Command;
use crate::tab::Tab;
use crate::window::BrowserWindow;
use crate::{app, capture, engine, exec, xaml};

const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const STEP_TIMEOUT: Duration = Duration::from_secs(10);
const SECOND_TAB: &str = "data:text/html,<title>Second tab</title>\
    <link rel=\"icon\" href=\"data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'>\
    <circle cx='8' cy='8' r='7' fill='crimson'/></svg>\">\
    <body style='font:24px sans-serif'><h1>Second tab</h1></body>";

/// The bookmarks bar: a bookmarked page keeps its favicon, a drag reorders the bookmarks in
/// core, and a bar with more items than fit shows the rest in the chevron's menu, also after
/// the window narrows and widens again.
async fn bar_steps(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    second: &Rc<Tab>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let page = second.state().url;
    browser.bookmark_page(&page, "Second tab");
    if let Some(png) = second.favicon_png() {
        browser.record_favicon(&page, &png);
    }
    let saved = Url::parse(&page)
        .ok()
        .and_then(|url| browser.core(|p| p.favicons().get(&url).ok().flatten()));
    let shown = || {
        window
            .bookmarks_bar_items()
            .iter()
            .any(|item| matches!(item, BarItem::Link { url, icon: Some(_), .. } if *url == page))
    };
    exec::sleep(Duration::from_millis(500)).await;
    shoot(window, out_dir, "08b-bookmark-favicon", steps, |_| {
        json!({
            "saved_bytes": saved.as_ref().map(Vec::len),
            "in_bar": shown(),
            "ok": saved.is_some() && shown(),
        })
    })
    .await;

    // The drag itself is the list's own; what it leaves behind is a new item order.
    let list = window.bookmarks_bar_list();
    let entries = list
        .cast::<ItemsControl>()
        .and_then(|c| c.Items())
        .map_err(|e| e.to_string())?;
    let last = entries.Size().map_err(|e| e.to_string())? - 1;
    let dragged = window.bookmarks_bar_items().last().map(BarItem::id);
    let entry = entries.GetAt(last).map_err(|e| e.to_string())?;
    entries.RemoveAt(last).map_err(|e| e.to_string())?;
    entries.InsertAt(0, &entry).map_err(|e| e.to_string())?;
    window.bar_item_dropped();
    let first_now = browser.core(|p| {
        p.bookmarks()
            .children(BookmarkId::TOOLBAR)
            .first()
            .map(|n| n.id)
    });
    steps.push(json!({
        "name": "08c-bar-drag-reorders",
        "dragged": format!("{dragged:?}"),
        "first_in_core": format!("{first_now:?}"),
        "ok": dragged.is_some() && first_now == dragged && window.bookmarks_bar_items().first().map(BarItem::id) == dragged,
    }));

    folder_menu(browser, window, &page, out_dir, steps).await?;

    let extra: Vec<BookmarkId> = browser.core(|p| {
        let mut bookmarks = p.bookmarks();
        (0..40)
            .filter_map(|i| {
                let url = Url::parse(&format!("https://site{i}.example/")).ok()?;
                bookmarks
                    .add_url(
                        BookmarkId::TOOLBAR,
                        InsertAt::End,
                        &format!("Bookmark number {i}"),
                        &url,
                    )
                    .ok()
            })
            .collect()
    });
    browser.bookmarks_changed();
    exec::sleep(Duration::from_millis(500)).await;
    shoot(window, out_dir, "08d-bar-overflow-chevron", steps, bar_fit).await;
    let menu = window
        .show_bookmarks_overflow()
        .map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(600)).await;
    let (_, overflow) = window.bookmarks_bar_split();
    let listed = menu.Items().and_then(|i| i.Size()).unwrap_or(0) as usize;
    shoot(window, out_dir, "08e-bar-overflow-menu", steps, |_| {
        json!({ "overflow": overflow.len(), "menu_entries": listed, "ok": listed == overflow.len() && listed > 0 })
    })
    .await;
    let _ = menu.cast::<FlyoutBase>().and_then(|m| m.Hide());

    let ((_, _, width, height), _) = window.bounds().ok_or("no window bounds")?;
    let shown_wide = window.bookmarks_bar_split().0.len();
    window
        .resize(760, height as i32)
        .map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(800)).await;
    shoot(window, out_dir, "08f-bar-narrow-window", steps, |w| {
        let mut step = bar_fit(w);
        let shown = w.bookmarks_bar_split().0.len();
        step["shown_when_wide"] = json!(shown_wide);
        if shown >= shown_wide {
            step["ok"] = json!(false);
        }
        step
    })
    .await;
    window
        .resize(width as i32, height as i32)
        .map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(800)).await;
    steps.push(json!({
        "name": "08g-bar-refits-when-wider",
        "shown": window.bookmarks_bar_split().0.len(),
        "ok": window.bookmarks_bar_split().0.len() == shown_wide,
    }));
    browser.core(|p| {
        let mut bookmarks = p.bookmarks();
        for id in extra {
            let _ = bookmarks.remove(id);
        }
    });
    browser.bookmarks_changed();
    Ok(())
}

/// A folder's menu from the bar: links with their favicons (or the page glyph), subfolders with
/// the folder glyph, and long titles cut short.
async fn folder_menu(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    favicon_page: &str,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let folder = window
        .bookmarks_bar_items()
        .iter()
        .find_map(|item| match item {
            BarItem::Folder { id, title, .. } if title == "Fixture folder" => Some(*id),
            _ => None,
        })
        .ok_or("no fixture folder on the bar")?;
    let long = "A bookmark whose title is much longer than any menu should ever be wide";
    let added = browser.core(|p| {
        let mut bookmarks = p.bookmarks();
        let page = Url::parse(favicon_page).map_err(|e| e.to_string())?;
        let link = bookmarks.add_url(folder, InsertAt::End, long, &page);
        let sub = bookmarks.add_folder(folder, InsertAt::End, "Nested folder");
        Ok::<_, String>(vec![link, sub])
    })?;
    browser.bookmarks_changed();
    exec::sleep(Duration::from_millis(300)).await;
    let menu = window
        .open_bookmarks_folder(folder)
        .map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(600)).await;
    let labels: Vec<(String, String)> = menu
        .Items()
        .map(|items| {
            (&items)
                .into_iter()
                .filter_map(|item| {
                    let (text, icon) = if let Ok(link) = item.cast::<MenuFlyoutItem>() {
                        (link.Text().ok()?, link.Icon().ok())
                    } else {
                        let sub = item.cast::<MenuFlyoutSubItem>().ok()?;
                        (sub.Text().ok()?, sub.Icon().ok())
                    };
                    let kind = match icon {
                        Some(icon) if icon.cast::<ImageIcon>().is_ok() => "favicon",
                        Some(icon) if icon.cast::<FontIcon>().is_ok() => "glyph",
                        _ => "none",
                    };
                    Some((text.to_string(), kind.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    shoot(window, out_dir, "08c2-folder-menu-icons", steps, |_| {
        let cut = labels
            .iter()
            .any(|(t, _)| t.ends_with('\u{2026}') && t.chars().count() <= 50);
        let favicon = labels.iter().any(|(_, k)| k == "favicon");
        json!({
            "entries": labels,
            "ok": labels.len() == 3 && cut && favicon && labels.iter().all(|(_, k)| k != "none"),
        })
    })
    .await;
    let _ = menu.cast::<FlyoutBase>().and_then(|m| m.Hide());
    browser.core(|p| {
        let mut bookmarks = p.bookmarks();
        for id in added.into_iter().flatten() {
            let _ = bookmarks.remove(id);
        }
    });
    browser.bookmarks_changed();
    Ok(())
}

/// The bar holds every item, shows only whole ones inside its width, and lists the rest
/// behind the chevron, which shows exactly when something overflows.
fn bar_fit(window: &BrowserWindow) -> Value {
    let (shown, overflow) = window.bookmarks_bar_split();
    let list = window.bookmarks_bar_list();
    let list_width = list
        .cast::<FrameworkElement>()
        .and_then(|l| l.ActualWidth())
        .unwrap_or(0.0);
    let entries = list
        .cast::<ItemsControl>()
        .and_then(|c| c.Items())
        .and_then(|i| i.cast::<windows_collections::IVector<windows_core::IInspectable>>());
    let mut rights = Vec::new();
    if let Ok(entries) = entries {
        for element in &entries {
            let Ok(element) = element.cast::<FrameworkElement>() else {
                continue;
            };
            if !xaml::is_visible(&element) {
                continue;
            }
            let left = element
                .cast::<UIElement>()
                .and_then(|e| e.TransformToVisual(&list.cast::<UIElement>()?))
                .and_then(|t| t.TransformPoint(Point { x: 0.0, y: 0.0 }))
                .map_or(f32::NAN, |p| p.x);
            rights.push(f64::from(left) + element.ActualWidth().unwrap_or(0.0));
        }
    }
    let chevron = window.bookmarks_overflow_shown();
    let whole = rights.iter().all(|r| *r <= list_width + 0.5);
    json!({
        "shown": shown.len(),
        "overflow": overflow.len(),
        "visible_entries": rights.len(),
        "list_width": list_width,
        "rightmost": rights.iter().copied().fold(0.0, f64::max),
        "chevron": chevron,
        "ok": whole && rights.len() == shown.len() && !overflow.is_empty() && chevron
            && shown.len() + overflow.len() == window.bookmarks_bar_buttons() as usize,
    })
}

/// The zoom chip shows the page's zoom when it is not 100%, follows the selected tab, and opens
/// the zoom bubble. Without OS input the page cannot be zoomed as a person would, so the page's
/// pixel ratio is emulated at 110% of the window's scale instead, which is what zoom changes,
/// with the resize event a real zoom fires.
async fn zoom_steps(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    window.run(Command::SelectTab(0));
    let scale = window
        .xaml_root()
        .and_then(|r| r.RasterizationScale())
        .map_err(|e| e.to_string())?;
    let emulate =
        json!({ "width": 0, "height": 0, "deviceScaleFactor": scale * 1.1, "mobile": false });
    devtools(tab, "Emulation.setDeviceMetricsOverride", &emulate).await?;
    // Emulation changes the ratio without the resize a real zoom brings.
    eval(tab, "dispatchEvent(new Event('resize'))").await?;
    let chip = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window.zoom_chip_shown()
    })
    .await;
    shoot(
        window,
        out_dir,
        "12c-zoom-chip",
        steps,
        |_| json!({ "chip": chip, "ok": chip.as_deref() == Some("110%") }),
    )
    .await;
    window.show_zoom_bubble().map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(500)).await;
    shoot(
        window,
        out_dir,
        "12d-zoom-bubble",
        steps,
        |w| json!({ "ok": w.zoom_chip_shown().as_deref() == Some("110%") }),
    )
    .await;
    window.run(Command::SelectTab(1));
    exec::sleep(Duration::from_millis(300)).await;
    let other_tab = window.zoom_chip_shown();
    window.run(Command::SelectTab(0));
    exec::sleep(Duration::from_millis(300)).await;
    let back = window.zoom_chip_shown();
    devtools(tab, "Emulation.clearDeviceMetricsOverride", &json!({})).await?;
    eval(tab, "dispatchEvent(new Event('resize'))").await?;
    let cleared = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window.zoom_chip_shown().is_none().then_some(())
    })
    .await;
    steps.push(json!({
        "name": "12e-zoom-follows-the-tab",
        "other_tab": other_tab,
        "back": back,
        "hidden_at_100": cleared.is_some(),
        "ok": other_tab.is_none() && back.as_deref() == Some("110%") && cleared.is_some(),
    }));
    Ok(())
}

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

    // A session saved before a tab's engine view exists still has the URL the tab will load.
    let planned = server.url("/page2.html");
    let pending = window
        .open_url_tab(planned.as_str(), false)
        .map_err(|e| e.to_string())?;
    let before_start = pending.session_url();
    steps.push(json!({
        "name": "01b-planned-url-before-the-engine",
        "session_url": before_start,
        "ok": before_start == planned.as_str(),
    }));
    wait_loaded(&pending).await?;
    window.close_tab(pending.id);

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
    steps.push(page_forgery(&window, index.as_str()).await?);
    close_all_but(&window, &first);
    window.run(Command::SelectTab(0));

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

    // A window.open no user gesture led to (at load, from a timer) is blocked.
    let target = server.url("/page2.html");
    let opener_page = format!(
        "data:text/html,<title>Popup opener</title><script>window.open('{target}');\
         setTimeout(() => window.open('{target}'), 300)</script>"
    );
    let before = window.tab_count();
    let opener = window
        .open_url_tab(&opener_page, true)
        .map_err(|e| e.to_string())?;
    wait_loaded(&opener).await?;
    exec::sleep(Duration::from_millis(1500)).await;
    let popups = window.tab_count().saturating_sub(before + 1);
    steps.push(json!({
        "name": "06b-popups-without-a-gesture",
        "popups": popups,
        "ok": popups == 0 && window.tab(opener.id).is_some(),
    }));
    close_all_but(&window, &first);
    window.run(Command::SelectTab(0));

    // With "Block pop-ups" off, the same page opens both.
    browser.write_pref(&keys::BLOCK_POPUPS, &false);
    let before = window.tab_count();
    let opener = window
        .open_url_tab(&opener_page, true)
        .map_err(|e| e.to_string())?;
    wait_loaded(&opener).await?;
    let allowed = wait_for_tab_count(&window, before + 3).await.is_ok();
    browser.write_pref(&keys::BLOCK_POPUPS, &true);
    steps.push(json!({
        "name": "06c-popups-allowed",
        "popups": window.tab_count().saturating_sub(before + 1),
        "ok": allowed,
    }));
    close_all_but(&window, &first);
    window.run(Command::SelectTab(0));

    // Ctrl+click on a link opens it in a background tab.
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
            "star_bubble": w.bookmark_editor().is_some(),
            "ok": folder.is_ok() && w.bookmarks_bar_items().len() == 2 && first.state().starred
                && w.bookmark_editor().is_some(),
        })
    })
    .await;
    if let Some(editor) = window.bookmark_editor() {
        editor.close();
    }
    bar_steps(browser, &window, &second, out_dir, steps).await?;
    let star_page = Url::parse("data:text/html,<title>Star test</title><h1>Star test</h1>")
        .map_err(|e| e.to_string())?;
    bookmark_steps::star_bubble(browser, &window, &star_page, out_dir, steps).await?;
    bookmark_steps::context_menus(browser, &window, out_dir, steps).await?;
    bookmark_steps::preload_favicons(browser, &window, out_dir, steps).await?;

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

    // Ctrl+S typed into the page collapses the vertical tab list and expands it again; with
    // tabs on top it does nothing.
    let mut toggled = Vec::new();
    for _ in 0..2 {
        let before = window.is_pane_collapsed();
        press(&first, 0x53, 2).await?;
        let changed = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
            (window.is_pane_collapsed() != before).then_some(())
        })
        .await;
        toggled.push(changed.is_some());
    }
    browser.set_tabs_position(TabsPosition::Top);
    wait_layout(&window, TabsPosition::Top).await;
    let top_before = window.is_pane_collapsed();
    window.run(Command::ToggleTabPane);
    let top_after = window.is_pane_collapsed();
    browser.set_tabs_position(TabsPosition::Left);
    wait_layout(&window, TabsPosition::Left).await;
    steps.push(json!({
        "name": "12b-ctrl-s-toggles-the-tab-pane",
        "toggled": toggled,
        "no_op_with_tabs_on_top": top_before == top_after,
        "ok": toggled == [true, true] && !window.is_pane_collapsed() && top_before == top_after,
    }));

    zoom_steps(&window, &first, out_dir, steps).await?;
    progress_steps::run(&window, out_dir, steps).await?;
    connection_steps::run(&window, &server, out_dir, steps).await?;
    tab_steps::run(&window, &server, out_dir, steps).await?;

    let count = window.show_suggestions("fixture");
    steps.push(json!({
        "name": "13-omnibox-suggestions",
        "labels": window.suggestion_labels(),
        "ok": count >= 2,
    }));

    let page2 = server.url("/page2.html");
    if let Err(e) = dialog_steps::settings(&window, out_dir, &page2, steps).await {
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

    if let Err(e) = toolbar_steps::run(browser, &window, out_dir, steps).await {
        steps.push(json!({ "name": "18b-extension-toolbar", "error": e, "ok": false }));
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

    steps.push(xpi_update(browser, &first, &server, out_dir).await);

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

    // Last: it clears the site data every step above may rely on.
    let result = dialog_steps::clear_browsing_data(browser, &window, out_dir, &page2, steps).await;
    if let Err(e) = result {
        dialog_steps::failed(steps, "23-clear-browsing-data", &e);
    }
    Ok(())
}

const LINK_CENTER: &str = "(() => { const a = document.querySelector('a[href]'); \
    if (!a) return null; const r = a.getBoundingClientRect(); \
    return [r.x + r.width / 2, r.y + r.height / 2]; })()";

/// Every real key press reads as Ctrl+W to anything that asks the page's `KeyboardEvent`.
const REMAP_KEYS: &str = "(() => { const p = KeyboardEvent.prototype; \
    for (const [name, value] of [['keyCode', 87], ['which', 87], ['key', 'w'], ['code', 'KeyW'], \
      ['ctrlKey', true], ['shiftKey', false], ['altKey', false], ['metaKey', false]]) \
      Object.defineProperty(p, name, { configurable: true, get() { return value; } }); \
    return 1; })()";

/// Records the `vsesvit` member of any object serialized with `JSON.stringify`.
const WATCH_JSON: &str = "(() => { window.__leaked = []; \
    Object.defineProperty(Object.prototype, 'toJSON', { configurable: true, get() { \
      if (this && typeof this.vsesvit === 'string') window.__leaked.push(this.vsesvit); \
      return undefined; } }); \
    return 1; })()";

/// Posts a Ctrl+T with every secret seen, and calls anything shortcut-like the page can reach.
const FORGE: &str = "(() => { const leaked = window.__leaked || []; let tries = 0; \
    for (const secret of new Set(leaked)) { try { \
      chrome.webview.postMessage(JSON.stringify({ vsesvit: secret, t: 'key', vk: 84, m: 1 })); \
      tries++; } catch (e) {} } \
    for (const name of Object.getOwnPropertyNames(window)) { \
      if (/vsesvit/i.test(name) && typeof window[name] === 'function') { \
        try { window[name](JSON.stringify({ t: 'key', vk: 84, m: 1 })); tries++; } catch (e) {} } } \
    return leaked.length + ':' + tries; })()";

/// A page must not be able to run browser commands: not by redefining the getters its real key
/// presses are read through, and not by posting shortcut messages of its own, even after
/// watching the shell's page script serialize its messages.
async fn page_forgery(window: &Rc<BrowserWindow>, url: &str) -> Result<Value, String> {
    let open = || async {
        let tab = window.open_url_tab(url, true).map_err(|e| e.to_string())?;
        wait_loaded(&tab).await?;
        Ok::<_, String>(tab)
    };
    let attack = open().await?;
    eval(&attack, REMAP_KEYS).await?;
    press(&attack, 0x41, 0).await?;
    exec::sleep(Duration::from_millis(1500)).await;
    let remap_closed_the_tab = window.tab(attack.id).is_none();
    let attack = if remap_closed_the_tab {
        open().await?
    } else {
        attack
    };

    eval(&attack, WATCH_JSON).await?;
    let link = eval(&attack, LINK_CENTER).await?;
    let point: Vec<f64> = serde_json::from_str(&link).map_err(|e| format!("{link}: {e}"))?;
    let before_click = window.tab_count();
    ctrl_click(&attack, point[0], point[1]).await?;
    let background = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (window.tab_count() > before_click)
            .then(|| window.tabs_in_order().into_iter().max_by_key(|t| t.id))
            .flatten()
    })
    .await;
    if let Some(tab) = &background {
        let _ = wait_loaded(tab).await;
        window.close_tab(tab.id);
    }
    let leaked = eval(&attack, "(window.__leaked || []).length").await?;
    let before_forge = window.tab_count();
    let forged = eval(&attack, FORGE).await?;
    exec::sleep(Duration::from_millis(1500)).await;
    let forged_command_ran = window.tab_count() != before_forge;
    Ok(json!({
        "name": "05b-pages-cannot-forge-shortcuts",
        "remapped_key_closed_the_tab": remap_closed_the_tab,
        "secrets_seen": leaked,
        "forge_attempts": forged,
        "forged_command_ran": forged_command_ran,
        "ctrl_click_still_opens_a_background_tab": background.is_some(),
        "ok": !remap_closed_the_tab
            && leaked == "0"
            && !forged_command_ran
            && background.is_some()
            && active_is(window, &attack),
    }))
}

const XPI_NAME: &str = "Vsesvit Probe XPI";

/// The probe as an unsigned add-on file: a gecko id, no `key`, and page attributes of its own
/// so the CRX probe's content script does not answer for it.
fn probe_xpi(gecko_id: &str, version: &str) -> Vec<u8> {
    let files: Vec<(&str, Vec<u8>)> = testkit::PROBE_FILES
        .iter()
        .map(|(name, bytes)| {
            let data = match *name {
                "manifest.json" => {
                    let mut manifest: Value = serde_json::from_slice(bytes).unwrap_or_default();
                    manifest["name"] = json!(XPI_NAME);
                    manifest["version"] = json!(version);
                    manifest["browser_specific_settings"] = json!({ "gecko": { "id": gecko_id } });
                    if let Some(manifest) = manifest.as_object_mut() {
                        manifest.remove("action");
                        manifest.remove("declarative_net_request");
                    }
                    serde_json::to_vec_pretty(&manifest).unwrap_or_default()
                }
                "content.js" => String::from_utf8_lossy(bytes)
                    .replace("vsesvitProbe", "vsesvitXpiProbe")
                    .replace("vsesvitVisits", "vsesvitXpiVisits")
                    .into_bytes(),
                _ => bytes.to_vec(),
            };
            (*name, data)
        })
        .collect();
    let entries: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
    testkit::zip_files(&entries)
}

/// An add-on installed from a file and then updated to a newer version keeps its engine id,
/// and with it what it stored: its content script's visit count goes on from 1 to 2.
async fn xpi_update(
    browser: &Rc<Browser>,
    tab: &Rc<Tab>,
    server: &FixtureServer,
    out_dir: &Path,
) -> Value {
    const GECKO_ID: &str = "probe-xpi@vsesvit.test";
    let mut engine_ids = Vec::new();
    let mut visits = Vec::new();
    for version in ["1.0.0", "1.0.1"] {
        let path = out_dir.join(format!("probe-{version}.xpi"));
        let installed = match std::fs::write(&path, probe_xpi(GECKO_ID, version)) {
            Ok(()) => match InstallSource::from_path(&path) {
                Ok(source) => browser.install_extension(source, &|_| {}).await,
                Err(e) => Err(e.to_string()),
            },
            Err(e) => Err(e.to_string()),
        };
        engine_ids.push(installed.map(|e| e.engine_id));
        tab.navigate(server.url("/index.html").as_str());
        exec::sleep(Duration::from_millis(300)).await;
        let _ = wait_loaded(tab).await;
        let mut seen = String::new();
        let deadline = Instant::now() + STEP_TIMEOUT;
        while Instant::now() < deadline {
            seen = eval(
                tab,
                "document.documentElement.dataset.vsesvitXpiVisits || ''",
            )
            .await
            .unwrap_or_default();
            if seen.trim_matches('"').is_empty() {
                exec::sleep(Duration::from_millis(200)).await;
            } else {
                break;
            }
        }
        visits.push(seen.trim_matches('"').to_owned());
    }
    let loaded = match browser.engine_profile().await {
        Some(profile) => exec::timeout(STEP_TIMEOUT, engine::extensions(&profile))
            .await
            .and_then(Result::ok)
            .map(|list| list.iter().filter(|e| e.name == XPI_NAME).count()),
        None => None,
    };
    let removed = match ExtensionId::parse(GECKO_ID) {
        Ok(id) => browser.uninstall_extension(&id).await,
        Err(e) => Err(e.to_string()),
    };
    let same_id = matches!(
        (engine_ids.first(), engine_ids.get(1)),
        (Some(Ok(Some(a))), Some(Ok(Some(b)))) if a == b
    );
    json!({
        "name": "22-xpi-update-keeps-its-engine-id",
        "engine_ids": format!("{engine_ids:?}"),
        "visits": visits,
        "engine_entries": loaded,
        "removed": format!("{removed:?}"),
        "ok": same_id && visits == ["1", "2"] && loaded == Some(1) && removed.is_ok(),
    })
}

fn close_all_but(window: &BrowserWindow, keep: &Tab) {
    for tab in window.tabs_in_order() {
        if tab.id != keep.id {
            window.close_tab(tab.id);
        }
    }
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

pub(super) async fn wait_loaded(tab: &Rc<Tab>) -> Result<(), String> {
    exec::wait_for(LOAD_TIMEOUT, Duration::from_millis(100), || {
        let state = tab.state();
        (tab.is_ready() && !state.loading() && !state.url.is_empty()).then_some(())
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
        (state.title == title && !state.loading()).then_some(())
    })
    .await
    .ok_or_else(|| format!("tab {} never showed {title:?}: {:?}", tab.id, tab.state()))?;
    exec::sleep(Duration::from_millis(700)).await;
    Ok(())
}

/// A key press with DevTools modifiers (1 Alt, 2 Ctrl, 4 Meta, 8 Shift). The press may close
/// the tab, and then its DevTools calls never answer, hence the bound.
pub(super) async fn press(tab: &Tab, vk: u16, modifiers: u8) -> Result<(), String> {
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

pub(super) async fn devtools(tab: &Tab, method: &str, params: &Value) -> Result<(), String> {
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
