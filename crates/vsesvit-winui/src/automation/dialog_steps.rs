//! Scripted steps through the dialogs. Controls are set programmatically and buttons invoked
//! through their automation peers, which runs each dialog's own handlers on core data without
//! any OS input.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::Url;
use vsesvit_core::bookmarks::{BookmarkId, NodeKind};
use vsesvit_core::extensions::ExtensionId;
use vsesvit_core::prefs::TabsPosition;
use vsesvit_core::testkit;
use windows_core::{IInspectable, Interface, Result};

use super::{shoot, wait_layout};
use crate::bindings::*;
use crate::browser::Browser;
use crate::dialogs::{self, Dialog, Preview};
use crate::window::BrowserWindow;
use crate::{engine, exec};

const WAIT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(100);

/// Clicks a button the way assistive technology does.
fn invoke(element: &impl Interface) -> Result<()> {
    let element = element.cast::<UIElement>()?;
    // A peer of an element that has left the tree must not be invoked.
    element.XamlRoot()?;
    let peer = FrameworkElementAutomationPeer::CreatePeerForElement(&element)?;
    peer.GetPattern(PatternInterface::Invoke)?
        .cast::<IInvokeProvider>()?
        .Invoke()
}

fn click(preview: &Preview, name: &str) -> Result<()> {
    invoke(&preview.find::<Button>(name)?)
}

fn select_index(selector: &impl Interface, index: usize) -> Result<()> {
    selector
        .cast::<Selector>()?
        .SetSelectedIndex(i32::try_from(index).unwrap_or(-1))
}

/// A dialog's content over the window, laid out.
async fn open(window: &Rc<BrowserWindow>, dialog: Dialog) -> Result<Preview> {
    let preview = dialogs::preview(window, dialog)?;
    exec::sleep(Duration::from_millis(500)).await;
    Ok(preview)
}

/// Lets the dialog finish rebuilding after a change.
async fn settle() {
    exec::sleep(Duration::from_millis(500)).await;
}

async fn until<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
    exec::wait_for(WAIT, POLL, &mut f).await
}

/// Settings: "Vertical, on the right" in the Tabs box moves the tab list; "on the left" brings
/// it back.
pub(super) async fn settings(
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<()> {
    let preview = open(window, Dialog::Settings).await?;
    let tabs: ComboBox = preview.find("TabsPosition")?;
    select_index(&tabs, 1)?;
    let moved = wait_layout(window, TabsPosition::Right).await;
    exec::sleep(Duration::from_millis(400)).await;
    shoot(window, out_dir, "14-settings-dialog", steps, |_| {
        json!({ "tabs_box": "Vertical, on the right", "layout": format!("{moved:?}"), "ok": moved == Some(TabsPosition::Right) })
    })
    .await;
    select_index(&tabs, 0)?;
    let back = wait_layout(window, TabsPosition::Left).await;
    steps.push(json!({ "name": "14b-settings-back-to-left", "layout": format!("{back:?}"), "ok": back == Some(TabsPosition::Left) }));
    Ok(())
}

/// The tree node showing `label`.
fn tree_node(tree: &TreeView, label: &str) -> Option<TreeViewNode> {
    fn search(
        nodes: &windows_collections::IVector<TreeViewNode>,
        label: &str,
    ) -> Option<TreeViewNode> {
        for node in nodes {
            let text = node
                .Content()
                .ok()
                .and_then(|c| {
                    c.cast::<windows_reference::IReference<windows_core::HSTRING>>()
                        .ok()
                })
                .and_then(|c| c.Value().ok())
                .map(|t| t.to_string_lossy());
            if text.as_deref() == Some(label) {
                return Some(node);
            }
            if let Some(found) = node.Children().ok().and_then(|c| search(&c, label)) {
                return Some(found);
            }
        }
        None
    }
    search(&tree.RootNodes().ok()?, label)
}

fn select_node(tree: &TreeView, label: &str) -> Result<()> {
    let node = tree_node(tree, label)
        .ok_or_else(|| windows_core::Error::new(E_FAIL, format!("no tree node {label:?}")))?;
    tree.cast::<ITreeView2>()?.SetSelectedNode(&node)
}

/// Bookmarks: rename a folder, add a folder inside it and delete that again, and move a
/// bookmark to "Other bookmarks", each checked in core.
pub(super) async fn bookmarks(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    folder: BookmarkId,
    page2: &Url,
    steps: &mut Vec<Value>,
) -> Result<()> {
    let preview = open(window, Dialog::Bookmarks).await?;
    let tree: TreeView = preview.find("BookmarksTree")?;
    let children = |id| browser.core(|p| p.bookmarks().children(id));

    select_node(&tree, "\u{1F4C1} Fixture folder")?;
    let name: TextBox = preview.find("BookmarkName")?;
    let shown = name.Text().unwrap_or_default();
    name.SetText("Renamed folder")?;
    click(&preview, "BookmarkSave")?;
    let renamed = until(|| {
        browser
            .core(|p| p.bookmarks().get(folder))
            .filter(|n| n.title == "Renamed folder")
    })
    .await;

    click(&preview, "BookmarkAddFolder")?;
    let added = until(|| {
        children(folder)
            .into_iter()
            .find(|n| n.kind == NodeKind::Folder)
    })
    .await;
    click(&preview, "BookmarkDelete")?;
    let deleted =
        until(|| (!children(folder).iter().any(|n| n.kind == NodeKind::Folder)).then_some(()))
            .await;

    select_node(&tree, "Second fixture page")?;
    let folders: ComboBox = preview.find("BookmarkFolder")?;
    let items = folders.cast::<ItemsControl>()?.Items()?;
    let other = (0..items.Size()?).find(|&i| {
        items
            .GetAt(i)
            .ok()
            .and_then(|c| {
                c.cast::<windows_reference::IReference<windows_core::HSTRING>>()
                    .ok()
            })
            .and_then(|c| c.Value().ok())
            .is_some_and(|t| t.to_string_lossy() == "Other bookmarks")
    });
    if let Some(other) = other {
        select_index(&folders, other as usize)?;
    }
    click(&preview, "BookmarkMove")?;
    let moved = until(|| {
        browser
            .core(|p| p.bookmarks().find_by_url(page2))
            .into_iter()
            .find(|n| n.parent == BookmarkId::OTHER)
    })
    .await;
    exec::sleep(Duration::from_millis(400)).await;
    shoot(window, out_dir, "16-bookmarks-dialog", steps, |w| {
        json!({
            "editor_showed": shown,
            "renamed": renamed.is_some(),
            "added_folder": added.map(|n| n.title),
            "deleted_it": deleted.is_some(),
            "moved_to_other": moved.is_some(),
            "bar": format!("{:?}", w.bookmarks_bar_items()),
            "ok": shown == "Fixture folder" && renamed.is_some() && deleted.is_some() && moved.is_some(),
        })
    })
    .await;
    Ok(())
}

/// Extensions: switch the probe off and on (the engine and the toolbar follow), remove it, and
/// install it again from its file through the install box.
pub(super) async fn extensions(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    crx: &Path,
    steps: &mut Vec<Value>,
) -> Result<()> {
    let preview = open(window, Dialog::Extensions).await?;
    // The list holds only the probe, so its row's parts are the only ones with these names.
    let list: ItemsControl = preview.find("ExtensionsList")?;
    // The probe's row as the list holds it now: the dialog rebuilds its rows after each change.
    let row_part = |name: &str| -> Result<IInspectable> {
        let row: FrameworkElement = list.Items()?.GetAt(0)?.cast()?;
        row.FindName(name)
    };
    let probe = ExtensionId::parse(testkit::PROBE_ID)
        .map_err(|e| windows_core::Error::new(E_FAIL, e.to_string()))?;
    let engine_enabled = || async {
        let profile = browser.engine_profile().await?;
        let listed = exec::timeout(WAIT, engine::extensions(&profile))
            .await?
            .ok()?;
        Some(
            listed
                .iter()
                .find(|e| e.id == testkit::PROBE_ID)
                .map(|e| e.enabled),
        )
    };
    let has_action = || {
        browser
            .extension_actions()
            .iter()
            .any(|a| a.extension_id == testkit::PROBE_ID)
    };

    row_part("Enabled")?
        .cast::<ToggleSwitch>()?
        .SetIsOn(false)?;
    let mut off = None;
    for _ in 0..(WAIT.as_millis() / POLL.as_millis()) {
        let state = engine_enabled().await;
        if state == Some(Some(false)) && !has_action() {
            off = state;
            break;
        }
        exec::sleep(POLL).await;
    }
    exec::sleep(Duration::from_millis(400)).await;
    shoot(
        window,
        out_dir,
        "15-extensions-dialog",
        steps,
        |_| json!({ "switched_off": format!("{off:?}"), "ok": off == Some(Some(false)) }),
    )
    .await;

    settle().await;
    row_part("Enabled")?.cast::<ToggleSwitch>()?.SetIsOn(true)?;
    let on = until(|| has_action().then_some(())).await;

    settle().await;
    invoke(&row_part("Remove")?)?;
    let removed = until(|| {
        let gone = browser
            .core(|p| p.extensions().get(&probe))
            .ok()
            .flatten()
            .is_none();
        (gone && !has_action()).then_some(())
    })
    .await;
    let engine_after_remove = engine_enabled().await;

    let source: TextBox = preview.find("InstallSource")?;
    source.SetText(&crx.to_string_lossy())?;
    click(&preview, "InstallButton")?;
    let reinstalled = until(|| has_action().then_some(())).await;
    let status = preview
        .find::<TextBlock>("InstallStatus")?
        .Text()
        .unwrap_or_default();
    steps.push(json!({
        "name": "15b-extensions-toggle-remove-install",
        "switched_on": on.is_some(),
        "removed": removed.is_some(),
        "engine_after_remove": format!("{engine_after_remove:?}"),
        "reinstalled": reinstalled.is_some(),
        "status": status,
        "ok": on.is_some() && removed.is_some() && engine_after_remove == Some(None) && reinstalled.is_some(),
    }));
    Ok(())
}

/// History: search for the second page, delete it, then clear the last hour.
pub(super) async fn history(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    page2: &Url,
    steps: &mut Vec<Value>,
) -> Result<()> {
    let preview = open(window, Dialog::History).await?;
    let search: AutoSuggestBox = preview.find("HistorySearch")?;
    let list: ListView = preview.find("HistoryList")?;
    let shown = || {
        list.cast::<ItemsControl>()
            .and_then(|l| l.Items())
            .and_then(|i| i.Size())
            .unwrap_or(0)
    };
    let all = shown();
    search.SetText("fixture 2")?;
    let found = until(|| (shown() == 1).then_some(())).await;
    exec::sleep(Duration::from_millis(400)).await;
    shoot(window, out_dir, "17-history-dialog", steps, |_| {
        json!({ "rows_before_search": all, "search_rows": shown(), "ok": all >= 2 && found.is_some() })
    })
    .await;
    select_index(&list, 0)?;
    click(&preview, "HistoryDelete")?;
    let deleted = until(|| {
        let left = browser
            .core(|p| p.history().search(page2.as_str(), 10))
            .unwrap_or_default();
        left.iter().all(|e| e.url != *page2).then_some(())
    })
    .await;
    search.SetText("")?;
    select_index(&preview.find::<ComboBox>("HistoryRange")?, 0)?;
    click(&preview, "HistoryClear")?;
    let cleared = until(|| {
        let visits = browser
            .core(|p| p.history().visits_between(0, i64::MAX, 10))
            .unwrap_or_default();
        visits.is_empty().then_some(())
    })
    .await;
    steps.push(json!({
        "name": "17b-history-delete-and-clear",
        "deleted_page2": deleted.is_some(),
        "cleared_last_hour": cleared.is_some(),
        "rows_after": shown(),
        "ok": deleted.is_some() && cleared.is_some() && shown() == 0,
    }));
    Ok(())
}

/// A step that could not drive its dialog.
pub(super) fn failed(steps: &mut Vec<Value>, name: &str, error: &windows_core::Error) {
    steps.push(json!({ "name": name, "error": error.message(), "ok": false }));
}
