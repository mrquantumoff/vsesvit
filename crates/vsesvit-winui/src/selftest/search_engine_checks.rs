//! The `search_engines` check: Settings' Search page adds an engine through its editor flyout,
//! the address box searches it by its shortcut, and the row's menu deletes it.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use vsesvit_core::search::{FormField, SearchEngine};
use windows_core::Interface;

use super::{Probe, until};
use crate::automation::{invoke, settings_on};
use crate::bindings::*;
use crate::browser::Browser;
use crate::dialogs::Preview;
use crate::dialogs::search_engines::Engines;
use crate::exec;
use crate::shortcuts::Mods;
use crate::window::BrowserWindow;

const SETTLE: Duration = Duration::from_millis(300);
const ENTER: u16 = 0x0D;
const NAME: &str = "Fixture engine";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn engine_named(browser: &Browser, name: &str) -> Option<SearchEngine> {
    browser
        .core(|p| p.search_engines().list())
        .ok()?
        .into_iter()
        .find(|e| e.name == name)
}

/// Settings over the window, on its Search page.
async fn search_page(window: &Rc<BrowserWindow>) -> Result<(Preview, Rc<Engines>), String> {
    let preview = settings_on(window, "SearchPanel").await.map_err(err)?;
    let engines = preview
        .wired::<Engines>()
        .ok_or("the Settings dialog has no search engines")?;
    Ok((preview, engines))
}

/// The texts of the menu behind a row's More actions button.
fn menu_items(more: &Button) -> Result<Vec<(String, MenuFlyoutItem)>, String> {
    let menu = more
        .cast::<IButton>()
        .and_then(|b| b.Flyout())
        .and_then(|f| f.cast::<MenuFlyout>())
        .map_err(err)?;
    let mut items = Vec::new();
    for item in menu.Items().map_err(err)? {
        let item = item.cast::<MenuFlyoutItem>().map_err(err)?;
        items.push((item.Text().map_err(err)?.to_string(), item));
    }
    Ok(items)
}

pub(super) async fn search_engines(
    window: &Rc<BrowserWindow>,
    search_url: &str,
    index: &str,
    out_dir: &Path,
    p: &Probe,
) -> Result<String, String> {
    let browser = window.browser().ok_or("no browser")?;
    let result = run(window, &browser, search_url, index, out_dir, p).await;
    if let Some(engine) = engine_named(&browser, NAME)
        && let Err(e) = browser.core(|c| c.search_engines().remove(&engine.id))
    {
        log::warn!("self-test: removing the fixture engine: {e}");
    }
    result
}

async fn run(
    window: &Rc<BrowserWindow>,
    browser: &Browser,
    search_url: &str,
    index: &str,
    out_dir: &Path,
    p: &Probe,
) -> Result<String, String> {
    let mut detail = Vec::new();
    let (preview, engines) = search_page(window).await?;
    let rows = engines.rows().map_err(err)?;
    let default_menu: Vec<String> = rows
        .iter()
        .find(|(name, _)| name == "DuckDuckGo (Default)")
        .map(|(_, more)| menu_items(more))
        .transpose()?
        .ok_or_else(|| format!("no \"DuckDuckGo (Default)\" row among {:?}", rows.iter().map(|r| &r.0).collect::<Vec<_>>()))?
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    detail.push(format!("{} rows; the default's menu {default_menu:?}", rows.len()));
    if default_menu != ["Edit"] {
        return Err(detail.join("; "));
    }

    p.observe("adding an engine in the editor");
    invoke(&preview.find::<Button>("SearchEngineAdd").map_err(err)?).map_err(err)?;
    let editor = until(p, |p| {
        p.observe("Add opened no editor");
        engines.editor()
    })
    .await;
    editor.fill(FormField::Name, NAME).map_err(err)?;
    editor.fill(FormField::Keyword, "w").map_err(err)?;
    exec::sleep(SETTLE).await;
    let taken = editor.shown(FormField::Keyword);
    editor.fill(FormField::Keyword, "fx").map_err(err)?;
    editor.fill(FormField::Url, &search_url.replace("{searchTerms}", "%s")).map_err(err)?;
    exec::sleep(SETTLE).await;
    let ready = editor.shown(FormField::Url);
    let shot = window.capture().await.map_err(err)?;
    std::fs::write(out_dir.join("search-engine-editor.png"), &shot.png).map_err(err)?;
    detail.push(format!("shortcut w: {taken:?}; filled in: {ready:?}"));
    if taken != (Some("Another search engine has this shortcut".to_owned()), false) || ready != (None, true) {
        editor.close();
        return Err(detail.join("; "));
    }
    editor.save();
    drop(editor);
    let added = until(p, |p| {
        p.observe("core has no fixture engine");
        engine_named(browser, NAME)
    })
    .await;
    until(p, |p| {
        let names: Vec<String> = engines.rows().unwrap_or_default().into_iter().map(|r| r.0).collect();
        p.observe(format!("the list shows {names:?}"));
        names.iter().any(|n| n == NAME).then_some(())
    })
    .await;
    exec::sleep(SETTLE).await;
    let shot = window.capture().await.map_err(err)?;
    std::fs::write(out_dir.join("settings-search-engines.png"), &shot.png).map_err(err)?;
    drop(preview);
    detail.push(format!("added {} with {}", added.name, added.search_url.0));

    p.observe("searching the engine by its shortcut");
    let tab = window.active_tab().ok_or("no active tab")?;
    let expected = search_url.replace("{searchTerms}", "vsesvit+fixture");
    window.type_address("fx vsesvit fixture");
    window.address_key_down(ENTER, Mods::NONE);
    let opened = until(p, |p| {
        let url = tab.state().url;
        p.observe(format!("\"fx vsesvit fixture\" opened {url:?}"));
        (url == expected).then_some(url)
    })
    .await;
    detail.push(format!("\"fx vsesvit fixture\" opened {opened}"));
    window.address_submitted(index);
    until(p, |p| {
        let s = tab.state();
        p.observe(format!("back to the fixture page: at {:?} loading {}", s.url, s.loading()));
        (s.url == index && !s.loading()).then_some(())
    })
    .await;

    p.observe("deleting the engine from its menu");
    let (preview, engines) = search_page(window).await?;
    let more = engines
        .rows()
        .map_err(err)?
        .into_iter()
        .find(|(name, _)| name == NAME)
        .map(|(_, more)| more)
        .ok_or("Settings opened again lists no fixture engine")?;
    invoke(&more).map_err(err)?;
    exec::sleep(SETTLE).await;
    let delete = menu_items(&more)?
        .into_iter()
        .find(|(text, _)| text == "Delete")
        .map(|(_, item)| item)
        .ok_or("the fixture engine's menu has no Delete")?;
    invoke(&delete).map_err(err)?;
    until(p, |p| {
        let listed = engines.rows().unwrap_or_default().iter().any(|r| r.0 == NAME);
        let stored = engine_named(browser, NAME).is_some();
        p.observe(format!("after Delete: in core {stored}, listed {listed}"));
        (!stored && !listed).then_some(())
    })
    .await;
    drop(preview);
    detail.push("Delete removed it from core and the list".to_owned());
    Ok(detail.join("; "))
}
