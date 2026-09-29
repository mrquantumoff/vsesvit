//! The welcome, page by page through its own Back and Next: the search engines with the
//! default chosen (another choice becomes core's default on Next), the browsers to import from
//! (and its import, on a fixture file), a row per recommended extension, the default-browser status as
//! Windows has it, and the last page closing the dialog. Nothing here installs from the network
//! or opens Windows Settings. Then the first pages again in the light theme.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::bookmarks::BookmarkId;
use vsesvit_core::import::Source;
use vsesvit_core::onboarding::RECOMMENDED_EXTENSIONS;
use vsesvit_core::prefs::Theme;
use windows_core::{Interface, Result};

use super::dialog_steps::invoke;
use super::shoot;
use crate::bindings::*;
use crate::browser::Browser;
use crate::dialogs::{self, Dialog, Preview, WELCOME_PAGES, WelcomePage, describe_default_browser};
use crate::window::BrowserWindow;
use crate::{exec, platform, xaml};

const WAIT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(50);
/// Longer than a page takes to come in.
const SETTLE: Duration = Duration::from_millis(500);

async fn shown(preview: &Preview, page: WelcomePage) -> bool {
    exec::wait_for(WAIT, POLL, || preview.find::<UIElement>(page.name()).ok())
        .await
        .is_some()
}

fn text(preview: &Preview, name: &str) -> String {
    preview
        .find::<TextBlock>(name)
        .and_then(|t| t.Text())
        .map(|t| t.to_string())
        .unwrap_or_default()
}

fn label(button: &Button) -> String {
    button
        .cast::<ContentControl>()
        .and_then(|c| c.Content())
        .and_then(|c| c.cast::<windows_reference::IReference<windows_core::HSTRING>>())
        .and_then(|c| c.Value())
        .map(|c| c.to_string_lossy())
        .unwrap_or_default()
}

fn children<T: Interface>(preview: &Preview, panel: &str) -> Vec<T> {
    preview
        .find::<Panel>(panel)
        .and_then(|p| p.Children())
        .map(|c| {
            (0..c.Size().unwrap_or(0))
                .filter_map(|i| c.GetAt(i).ok()?.cast().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn checked(toggle: &IToggleButton) -> bool {
    toggle.IsChecked().unwrap_or(false)
}

fn next(preview: &Preview) -> Result<()> {
    invoke(&preview.find::<Button>("WelcomeNext")?)
}

pub(super) async fn run(
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<()> {
    let browser = window.browser().ok_or_else(windows_core::Error::empty)?;
    let preview = dialogs::preview(window, Dialog::Welcome)?;
    let mut pages_seen = Vec::new();
    for page in WELCOME_PAGES {
        let is_shown = shown(&preview, page).await;
        exec::sleep(SETTLE).await;
        pages_seen.push(is_shown);
        let first = page == WELCOME_PAGES[0];
        let last = page == WELCOME_PAGES[WELCOME_PAGES.len() - 1];
        let back = preview.find::<UIElement>("WelcomeBack")?;
        let skip = preview.find::<UIElement>("WelcomeSkip")?;
        let next_label = label(&preview.find("WelcomeNext")?);
        let chrome_ok = xaml::is_visible(&back) != first
            && xaml::is_visible(&skip) != last
            && !next_label.is_empty();
        match page {
            WelcomePage::Hello => {
                shoot(window, out_dir, "25a-welcome-hello", steps, |_| {
                    json!({ "shown": is_shown, "next": next_label, "ok": is_shown && chrome_ok })
                })
                .await;
            }
            WelcomePage::Search => search(&browser, &preview, window, out_dir, steps).await?,
            WelcomePage::Import => {
                let boxes: Vec<IToggleButton> = children(&preview, "WelcomeBrowsers");
                let none_shown = preview
                    .find::<UIElement>("WelcomeNoBrowsers")
                    .is_ok_and(|e| xaml::is_visible(&e));
                let import_button = preview
                    .find::<UIElement>("WelcomeImport")
                    .is_ok_and(|b| xaml::is_visible(&b));
                shoot(window, out_dir, "25d-welcome-import", steps, |_| {
                    json!({
                        "browsers": boxes.len(),
                        "all_checked": boxes.iter().all(checked),
                        "none_found_shown": none_shown,
                        "ok": is_shown && chrome_ok && none_shown == boxes.is_empty()
                            && import_button != boxes.is_empty() && boxes.iter().all(checked),
                    })
                })
                .await;
                steps.push(import_file(&browser, out_dir).await);
            }
            WelcomePage::Extensions => {
                let buttons: Vec<String> =
                    children::<DependencyObject>(&preview, "WelcomeExtensionRows")
                        .iter()
                        .filter_map(|row| xaml::find_named::<Button>(row, "Install"))
                        .map(|b| label(&b))
                        .collect();
                shoot(window, out_dir, "25e-welcome-extensions", steps, |_| {
                    json!({
                        "buttons": buttons,
                        "ok": is_shown && chrome_ok && buttons.len() == RECOMMENDED_EXTENSIONS.len()
                            && buttons.iter().all(|l| l == "Install" || l == "Installed"),
                    })
                })
                .await;
            }
            WelcomePage::DefaultBrowser => {
                let state = platform::default_browser();
                let status = text(&preview, "DefaultBrowserStatus");
                let button = preview
                    .find::<UIElement>("DefaultBrowserMake")
                    .is_ok_and(|b| xaml::is_visible(&b));
                shoot(
                    window,
                    out_dir,
                    "25f-welcome-default-browser",
                    steps,
                    |_| {
                        json!({
                            "windows_says": format!("{state:?}"),
                            "status": status,
                            "button_shown": button,
                            "ok": is_shown && chrome_ok
                                && status == describe_default_browser(state).0
                                && button == (state == platform::DefaultBrowser::Other),
                        })
                    },
                )
                .await;
            }
            WelcomePage::Done => {
                shoot(window, out_dir, "25g-welcome-done", steps, |_| {
                    json!({
                        "next": next_label,
                        "ok": is_shown && chrome_ok && next_label == "Start browsing",
                    })
                })
                .await;
            }
        }
        next(&preview)?;
    }
    steps.push(json!({
        "name": "25h-welcome-every-page-in-turn",
        "shown": pages_seen,
        "ok": pages_seen.iter().all(|s| *s),
    }));
    drop(preview);

    browser.set_theme(Theme::Light);
    let preview = dialogs::preview(window, Dialog::Welcome)?;
    let hello = shown(&preview, WelcomePage::Hello).await;
    exec::sleep(SETTLE).await;
    shoot(
        window,
        out_dir,
        "25i-welcome-light",
        steps,
        |_| json!({ "ok": hello }),
    )
    .await;
    for _ in 0..3 {
        next(&preview)?;
    }
    let extensions = shown(&preview, WelcomePage::Extensions).await;
    exec::sleep(SETTLE).await;
    shoot(
        window,
        out_dir,
        "25j-welcome-light-extensions",
        steps,
        |_| json!({ "ok": extensions }),
    )
    .await;
    drop(preview);
    browser.set_theme(Theme::System);
    Ok(())
}

/// The import the welcome runs, on a bookmarks file rather than the browsers of the machine
/// running the check: read on a worker thread, then a folder on the bookmarks bar.
async fn import_file(browser: &Browser, out_dir: &Path) -> Value {
    const FOLDER: &str = "Welcome fixture";
    let file = out_dir.join("welcome-bookmarks.html");
    let written = std::fs::write(&file, BOOKMARKS_HTML);
    let status = dialogs::import_bookmarks(browser, FOLDER, "a fixture", Source::File(file)).await;
    let folder = browser.core(|p| {
        p.bookmarks()
            .children(BookmarkId::TOOLBAR)
            .into_iter()
            .find(|n| n.title == FOLDER)
    });
    let links = folder
        .as_ref()
        .map_or(0, |f| browser.core(|p| p.bookmarks().children(f.id).len()));
    if let Some(folder) = folder {
        let _ = browser.core(|p| p.bookmarks().remove(folder.id));
        browser.bookmarks_changed();
    }
    json!({
        "name": "25d2-welcome-import-reads-off-the-ui-thread",
        "status": status,
        "links": links,
        "ok": written.is_ok() && links == 2 && status.starts_with("Imported 2 items"),
    })
}

const BOOKMARKS_HTML: &str = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE><H1>Bookmarks</H1>
<DL><p>
<DT><A HREF="https://example.com/">Example</A>
<DT><A HREF="https://example.org/">Example org</A>
</DL><p>
"#;

/// The engines with the default chosen; another choice is core's default after Next, and Back
/// shows it still chosen.
async fn search(
    browser: &Browser,
    preview: &Preview,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<()> {
    let engines = browser.core(|p| p.search_engines().list().unwrap_or_default());
    let default = browser.core(|p| p.search_engines().default_engine().ok().map(|e| e.id));
    let radios: Vec<IToggleButton> = children(preview, "WelcomeEngines");
    let checked_index = radios.iter().position(checked);
    let default_index = engines.iter().position(|e| Some(&e.id) == default.as_ref());
    let other = (0..radios.len()).find(|i| Some(*i) != checked_index);
    shoot(window, out_dir, "25b-welcome-search", steps, |_| {
        json!({
            "engines": engines.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            "radios": radios.len(),
            "chosen": checked_index,
            "ok": radios.len() == engines.len() && radios.len() >= 2 && checked_index.is_some()
                && checked_index == default_index,
        })
    })
    .await;

    let Some(other) = other else {
        return Ok(());
    };
    radios[other].SetIsChecked(Some(true))?;
    let before = browser.core(|p| p.search_engines().default_engine().ok().map(|e| e.id));
    next(preview)?;
    let after = browser.core(|p| p.search_engines().default_engine().ok().map(|e| e.id));
    invoke(&preview.find::<Button>("WelcomeBack")?)?;
    let back = shown(preview, WelcomePage::Search).await;
    exec::sleep(SETTLE).await;
    let still_chosen = checked(&radios[other]);
    let wanted = engines.get(other).map(|e| e.id.clone());
    steps.push(json!({
        "name": "25c-welcome-search-choice-applies-on-next",
        "before": format!("{before:?}"),
        "after": format!("{after:?}"),
        "ok": before == default && after == wanted && after != default && back && still_chosen,
    }));
    // The run's Next from here puts the old default back.
    if let Some(index) = checked_index {
        radios[index].SetIsChecked(Some(true))?;
    }
    Ok(())
}
