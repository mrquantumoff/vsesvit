//! The `page_commands` check: Print, View page source and the developer tools on their keys and
//! in the page's context menu, without opening a print dialog or DevTools.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::json;
use vsesvit_core::view_source;
use windows_core::Interface;

use super::shortcut_checks::binding;
use super::{FIXTURE_TITLE, Probe, eval, tab_ids, until};
use crate::automation::press;
use crate::bindings::{ICoreWebView2_11, ICoreWebView2_16};
use crate::shortcuts::{Command, InPage, Mods};
use crate::tab::{Tab, TabId, VIEW_SOURCE_ITEM};
use crate::window::BrowserWindow;

/// DevTools modifier bit for `press`.
const CTRL: u8 = 2;
/// Clears the selection and says where an empty part of the page is, in CSS pixels, and what
/// is there.
const EMPTY_SPOT: &str = "(() => { getSelection().removeAllRanges(); \
    const x = innerWidth - 24, y = innerHeight - 24; \
    return [x, y, document.elementFromPoint(x, y)?.tagName ?? null]; })()";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// What the page's context menu held when it opened, and the item the check chose.
#[derive(Debug, Default)]
struct Menu {
    names: Vec<String>,
    ours: Option<usize>,
}

pub(super) async fn page_commands(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    p: &Probe,
) -> Result<String, String> {
    let mut detail = Vec::new();
    let page = tab.state().url;
    let expected = view_source::source_url(&page).ok_or(format!("{page} has no source"))?;

    let keys = [
        (0x50, Mods::CTRL, Command::Print, InPage::Native),
        (
            0x49,
            Mods::of(true, true, false),
            Command::DeveloperTools,
            InPage::Native,
        ),
        (0x7B, Mods::NONE, Command::DeveloperTools, InPage::Native),
        (
            0x4A,
            Mods::of(true, true, false),
            Command::JavaScriptConsole,
            InPage::Native,
        ),
        (0x55, Mods::CTRL, Command::ViewSource, InPage::Overridable),
    ];
    let bound: Vec<_> = keys
        .iter()
        .map(|&(vk, mods, ..)| binding(vk, mods))
        .collect();
    detail.push(format!(
        "Ctrl+P, Ctrl+Shift+I, F12, Ctrl+Shift+J, Ctrl+U bind {bound:?}"
    ));
    let as_expected = keys
        .iter()
        .zip(&bound)
        .all(|(&(.., command, kind), &b)| b == Some((command, kind)));
    let print_ui = tab
        .core()
        .ok_or("no engine view")?
        .cast::<ICoreWebView2_16>()
        .map(|_| ())
        .map_err(err);
    detail.push(format!("ICoreWebView2_16 for ShowPrintUI: {print_ui:?}"));
    if !as_expected || print_ui.is_err() {
        return Err(detail.join("; "));
    }

    p.observe("Ctrl+U in the page");
    let open = tab_ids(window);
    press(tab, 0x55, CTRL).await?;
    let (shown, ok) = source_tab(window, tab, &open, &expected, p).await;
    detail.push(format!("Ctrl+U: {shown}"));
    if !ok {
        return Err(detail.join("; "));
    }

    let spot: serde_json::Value =
        serde_json::from_str(&eval(tab, EMPTY_SPOT).await?).map_err(err)?;
    let (Some(x), Some(y)) = (spot[0].as_f64(), spot[1].as_f64()) else {
        return Err(format!("finding an empty spot gave {spot}"));
    };
    // Runs after the tab's own handler, which added its items. Choosing ours here is what a
    // click on it does, and keeps WebView2's menu from showing on the user's screen.
    let menu = Rc::new(RefCell::new(None::<Menu>));
    let seen = menu.clone();
    let _watch = tab
        .core()
        .ok_or("no engine view")?
        .cast::<ICoreWebView2_11>()
        .and_then(|core| {
            core.ContextMenuRequested(move |_, args| {
                let Some(args) = args.as_ref() else { return };
                let mut menu = Menu::default();
                if let Ok(items) = args.MenuItems() {
                    let items: Vec<_> = items.into_iter().collect();
                    menu.names = items.iter().map(|i| i.Name().unwrap_or_default()).collect();
                    menu.ours = items.iter().position(|i| {
                        i.Name().is_ok_and(|n| n == "custom")
                            && i.Label().is_ok_and(|l| l == "View page source")
                    });
                    if let Some(id) = menu.ours.and_then(|i| items[i].CommandId().ok()) {
                        let _ = args.SetSelectedCommandId(id);
                    }
                }
                let _ = args.SetHandled(true);
                *seen.borrow_mut() = Some(menu);
            })
        })
        .map_err(err)?;
    let open = tab_ids(window);
    for kind in ["mousePressed", "mouseReleased"] {
        let params = json!({
            "type": kind, "x": x, "y": y, "button": "right", "buttons": 2, "clickCount": 1,
        });
        tab.devtools("Input.dispatchMouseEvent", &params.to_string())
            .await
            .map_err(|e| format!("right click: {e}"))?;
    }
    let menu = until(p, |p| {
        p.observe(format!("right-clicked {spot}; no context menu yet"));
        menu.borrow_mut().take()
    })
    .await;
    let at = |name: &str| menu.names.iter().position(|n| n == name);
    let built_in = at(VIEW_SOURCE_ITEM).is_some();
    let before_inspect = menu.ours.is_some_and(|i| at("inspect") == Some(i + 1));
    detail.push(format!(
        "right-click at {spot}: menu {:?}, ours at {:?}",
        menu.names, menu.ours
    ));
    if at("print").is_none() || at("inspect").is_none() || !(built_in || before_inspect) {
        return Err(detail.join("; "));
    }
    if menu.ours.is_some() {
        let (shown, ok) = source_tab(window, tab, &open, &expected, p).await;
        detail.push(format!("View page source: {shown}"));
        if !ok {
            return Err(detail.join("; "));
        }
    }
    Ok(detail.join("; "))
}

/// Waits for the tab showing `expected` that `page` opened, reads where it opened and whether
/// its document shows the fixture page's source, and closes it.
async fn source_tab(
    window: &BrowserWindow,
    page: &Tab,
    open: &[TabId],
    expected: &str,
    p: &Probe,
) -> (String, bool) {
    let (index, source) = until(p, |p| {
        let tabs = window.tabs_in_order();
        p.observe(format!(
            "tabs {:?}",
            tabs.iter().map(|t| t.state().url).collect::<Vec<_>>()
        ));
        let index = tabs.iter().position(|t| !open.contains(&t.id))?;
        (tabs[index].state().url == expected && !tabs[index].state().loading())
            .then(|| (index, tabs[index].clone()))
    })
    .await;
    let page_index = window.tabs_in_order().iter().position(|t| t.id == page.id);
    let foreground = window.active_tab().is_some_and(|t| t.id == source.id);
    let title = format!("<title>{FIXTURE_TITLE}</title>");
    let script = format!("document.body?.textContent.includes({title:?}) ?? false");
    let shows = eval(&source, &script).await;
    window.close_tab(source.id);
    let ok =
        page_index.map(|i| i + 1) == Some(index) && foreground && shows.as_deref() == Ok("true");
    let shown = format!(
        "{expected} opened at {index} after the page at {page_index:?}, foreground {foreground}, \
         its text holds {title:?}: {shows:?}"
    );
    (shown, ok)
}
