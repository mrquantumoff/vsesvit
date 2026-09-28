//! Scripted steps through the star's bookmark editor and the bookmarks bar's context menus.
//! Controls are set programmatically and buttons and menu entries invoked through their
//! automation peers, which runs the same handlers a click does.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::Url;
use vsesvit_core::bookmarks::{BookmarkId, InsertAt, NodeKind};
use windows_core::{HSTRING, Interface};
use windows_reference::IReference;

use super::dialog_steps::invoke;
use super::{shoot, wait_loaded};
use crate::bindings::*;
use crate::bookmark_editor::Editor;
use crate::bookmarks_bar::BarItem;
use crate::browser::Browser;
use crate::exec;
use crate::window::BrowserWindow;

const WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(100);

async fn until<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
    exec::wait_for(WAIT, POLL, &mut f).await
}

async fn settle() {
    exec::sleep(Duration::from_millis(500)).await;
}

fn text_of(editor: &Editor, name: &str) -> String {
    editor
        .part::<TextBlock>(name)
        .and_then(|t| t.Text().ok())
        .map(|t| t.to_string())
        .unwrap_or_default()
}

fn set_text(editor: &Editor, name: &str, text: &str) -> Result<(), String> {
    editor
        .part::<TextBox>(name)
        .ok_or(format!("no {name}"))?
        .SetText(text)
        .map_err(|e| e.to_string())
}

fn click(editor: &Editor, name: &str) -> Result<(), String> {
    let button = editor.part::<Button>(name).ok_or(format!("no {name}"))?;
    invoke(&button).map_err(|e| e.to_string())
}

fn is_enabled(editor: &Editor, name: &str) -> bool {
    editor
        .part::<Control>(name)
        .and_then(|c| c.IsEnabled().ok())
        .unwrap_or(false)
}

fn part_shown(editor: &Editor, name: &str) -> bool {
    editor
        .part::<UIElement>(name)
        .is_some_and(|p| crate::xaml::is_visible(&p))
}

/// Picks the folder whose label (without its indent) is `title` in the editor's folder box.
fn choose_folder(editor: &Editor, title: &str) -> Result<(), String> {
    let folders = editor
        .part::<ComboBox>("EditorFolder")
        .ok_or("no folder box")?;
    let items = folders
        .cast::<ItemsControl>()
        .and_then(|c| c.Items())
        .map_err(|e| e.to_string())?;
    let index = (0..items.Size().unwrap_or(0))
        .find(|&i| {
            items
                .GetAt(i)
                .ok()
                .and_then(|c| c.cast::<IReference<HSTRING>>().ok())
                .and_then(|c| c.Value().ok())
                .is_some_and(|t| t.to_string_lossy().trim_start_matches('\u{2003}') == title)
        })
        .ok_or(format!("no folder {title}"))?;
    folders
        .cast::<Selector>()
        .and_then(|s| s.SetSelectedIndex(index as i32))
        .map_err(|e| e.to_string())
}

/// The star on a page that is not bookmarked adds it and opens "Bookmark added"; an address
/// that does not parse disables Done; Done saves name, URL and folder; on a bookmarked page the
/// star opens "Edit bookmark", whose Remove deletes it.
pub(super) async fn star_bubble(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    page: &Url,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let tab = window
        .open_url_tab(page.as_str(), true)
        .map_err(|e| e.to_string())?;
    wait_loaded(&tab).await?;
    let before = browser.core(|p| p.bookmarks().find_by_url(page).len());
    window.star_clicked();
    let editor = until(|| window.bookmark_editor())
        .await
        .ok_or("the star opened no editor")?;
    settle().await;
    let added = browser.core(|p| p.bookmarks().find_by_url(page));
    shoot(window, out_dir, "08h-star-bubble-added", steps, |w| {
        json!({
            "title": text_of(&editor, "EditorTitle"),
            "bookmarks_of_the_page": [before, added.len()],
            "star": tab.state().starred,
            "remove_shown": part_shown(&editor, "EditorRemove"),
            "ok": text_of(&editor, "EditorTitle") == "Bookmark added"
                && before == 0 && added.len() == 1
                && added[0].parent == BookmarkId::TOOLBAR
                && tab.state().starred && part_shown(&editor, "EditorRemove")
                && w.bookmarks_bar_items().iter().any(|i| i.id() == added[0].id),
        })
    })
    .await;
    let id = added
        .first()
        .map(|n| n.id)
        .ok_or("nothing was bookmarked")?;

    set_text(&editor, "EditorUrl", "not a web address")?;
    settle().await;
    shoot(window, out_dir, "08i-star-bubble-bad-url", steps, |_| {
        json!({
            "done_enabled": is_enabled(&editor, "EditorDone"),
            "marked": part_shown(&editor, "EditorUrlInvalid"),
            "ok": !is_enabled(&editor, "EditorDone") && part_shown(&editor, "EditorUrlInvalid"),
        })
    })
    .await;

    let edited = "https://edited.example/page";
    set_text(&editor, "EditorName", "Edited in the bubble")?;
    set_text(&editor, "EditorUrl", edited)?;
    choose_folder(&editor, "Fixture folder")?;
    settle().await;
    click(&editor, "EditorDone")?;
    let saved = until(|| {
        browser
            .bookmark(id)
            .filter(|n| n.title == "Edited in the bubble")
    })
    .await;
    let folder = saved.as_ref().and_then(|n| browser.bookmark(n.parent));
    steps.push(json!({
        "name": "08j-star-bubble-saves",
        "saved": format!("{saved:?}"),
        "folder": folder.as_ref().map(|f| f.title.clone()),
        "closed": window.bookmark_editor().is_none(),
        "ok": saved.as_ref().is_some_and(|n| n.url.as_ref().is_some_and(|u| u.as_str() == edited))
            && folder.is_some_and(|f| f.title == "Fixture folder")
            && window.bookmark_editor().is_none()
            && until(|| (!tab.state().starred).then_some(())).await.is_some(),
    }));
    browser.remove_bookmark(id);

    window.star_clicked();
    let first = until(|| window.bookmark_editor())
        .await
        .ok_or("no editor")?;
    first.close();
    window.star_clicked();
    let editor = until(|| window.bookmark_editor())
        .await
        .ok_or("the star opened no editor on a bookmarked page")?;
    settle().await;
    let title = text_of(&editor, "EditorTitle");
    shoot(
        window,
        out_dir,
        "08k-star-bubble-edit",
        steps,
        |_| json!({ "title": title, "ok": title == "Edit bookmark" }),
    )
    .await;
    click(&editor, "EditorRemove")?;
    let removed = until(|| {
        (browser.core(|p| p.bookmarks().find_by_url(page).is_empty()) && !tab.state().starred)
            .then_some(())
    })
    .await;
    steps.push(json!({
        "name": "08l-star-bubble-removes",
        "ok": removed.is_some() && window.bookmark_editor().is_none(),
    }));
    window.close_tab(tab.id);
    Ok(())
}

/// The labels of a menu's entries (separators as "-") and the entries themselves.
fn entries(menu: &MenuFlyout) -> Vec<(String, Option<MenuFlyoutItem>)> {
    let Ok(items) = menu.Items() else {
        return Vec::new();
    };
    (&items)
        .into_iter()
        .map(|item| match item.cast::<MenuFlyoutItem>() {
            Ok(entry) => (
                entry.Text().map(|t| t.to_string()).unwrap_or_default(),
                Some(entry),
            ),
            Err(_) => ("-".to_owned(), None),
        })
        .collect()
}

fn labels(menu: &MenuFlyout) -> Vec<String> {
    entries(menu).into_iter().map(|(label, _)| label).collect()
}

fn choose(menu: &MenuFlyout, label: &str) -> Result<(), String> {
    let entry = entries(menu)
        .into_iter()
        .find_map(|(l, entry)| (l == label).then_some(entry).flatten())
        .ok_or(format!("no menu entry {label:?}"))?;
    invoke(&entry).map_err(|e| e.to_string())
}

async fn context_menu(
    window: &Rc<BrowserWindow>,
    id: Option<BookmarkId>,
) -> Result<MenuFlyout, String> {
    let menu = window
        .show_bookmark_context_menu(id)
        .map_err(|e| e.to_string())?;
    settle().await;
    Ok(menu)
}

/// Right-click menus on a bar link, a bar folder, the bar's empty space and a link inside a
/// folder's menu, and what their entries do.
pub(super) async fn context_menus(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let bar = window.bookmarks_bar_items();
    let link = bar
        .iter()
        .find(|i| matches!(i, BarItem::Link { title, .. } if title == "Vsesvit fixture"))
        .ok_or("no fixture link on the bar")?
        .clone();
    let (folder, inner) = bar
        .iter()
        .find_map(|i| match i {
            BarItem::Folder { id, children, .. } => Some((*id, children.first()?.id())),
            BarItem::Link { .. } => None,
        })
        .ok_or("no folder with a bookmark on the bar")?;

    let menu = context_menu(window, Some(link.id())).await?;
    let link_labels = labels(&menu);
    shoot(window, out_dir, "08m-bar-link-context-menu", steps, |_| {
        json!({
            "entries": link_labels,
            "ok": link_labels == ["Open in new tab", "Open in new window", "-", "Edit\u{2026}", "Copy link", "-", "Delete"],
        })
    })
    .await;
    let tabs = window.tab_count();
    choose(&menu, "Open in new tab")?;
    let opened = until(|| (window.tab_count() == tabs + 1).then_some(())).await;
    steps.push(json!({ "name": "08n-context-open-in-new-tab", "ok": opened.is_some() }));
    if let Some(tab) = window.tabs_in_order().into_iter().max_by_key(|t| t.id)
        && opened.is_some()
    {
        window.close_tab(tab.id);
    }

    let menu = context_menu(window, Some(link.id())).await?;
    choose(&menu, "Edit\u{2026}")?;
    let editor = until(|| window.bookmark_editor())
        .await
        .ok_or("Edit\u{2026} opened no editor")?;
    settle().await;
    let title = editor
        .part::<TextBlock>("EditorTitle")
        .and_then(|t| t.Text().ok())
        .map(|t| t.to_string())
        .unwrap_or_default();
    let name = editor
        .part::<TextBox>("EditorName")
        .and_then(|t| t.Text().ok())
        .map(|t| t.to_string())
        .unwrap_or_default();
    shoot(window, out_dir, "08o-context-edit", steps, |_| {
        json!({ "title": title, "name": name, "ok": title == "Edit bookmark" && name == "Vsesvit fixture" })
    })
    .await;
    editor.close();

    let menu = context_menu(window, Some(folder)).await?;
    let folder_labels = labels(&menu);
    shoot(window, out_dir, "08p-bar-folder-context-menu", steps, |_| {
        json!({
            "entries": folder_labels,
            "ok": folder_labels.first().is_some_and(|l| l.starts_with("Open all (") && l.ends_with(") in new tabs"))
                && folder_labels.contains(&"Rename\u{2026}".to_owned())
                && folder_labels.contains(&"Delete".to_owned()),
        })
    })
    .await;
    let _ = menu.cast::<FlyoutBase>().and_then(|m| m.Hide());

    let folder_menu = window
        .open_bookmarks_folder(folder)
        .map_err(|e| e.to_string())?;
    settle().await;
    let menu = context_menu(window, Some(inner)).await?;
    let inner_labels = labels(&menu);
    shoot(window, out_dir, "08q-menu-item-context-menu", steps, |_| {
        json!({ "entries": inner_labels, "ok": inner_labels.contains(&"Edit\u{2026}".to_owned()) })
    })
    .await;
    let _ = menu.cast::<FlyoutBase>().and_then(|m| m.Hide());
    let _ = folder_menu.cast::<FlyoutBase>().and_then(|m| m.Hide());

    let menu = context_menu(window, None).await?;
    let bar_labels = labels(&menu);
    shoot(window, out_dir, "08r-bar-context-menu", steps, |_| {
        json!({
            "entries": bar_labels,
            "ok": bar_labels == ["Add page\u{2026}", "Add folder\u{2026}", "-", "Show bookmarks bar", "Bookmark manager"],
        })
    })
    .await;
    choose(&menu, "Add folder\u{2026}")?;
    let editor = until(|| window.bookmark_editor())
        .await
        .ok_or("Add folder\u{2026} opened no editor")?;
    set_text(&editor, "EditorName", "Folder from the menu")?;
    click(&editor, "EditorDone")?;
    let made = until(|| {
        browser
            .core(|p| p.bookmarks().children(BookmarkId::TOOLBAR))
            .into_iter()
            .find(|n| n.kind == NodeKind::Folder && n.title == "Folder from the menu")
    })
    .await;
    settle().await;
    let deleted = match &made {
        Some(made) => {
            let menu = context_menu(window, Some(made.id)).await?;
            choose(&menu, "Delete")?;
            until(|| browser.bookmark(made.id).is_none().then_some(())).await
        }
        None => None,
    };
    steps.push(json!({
        "name": "08s-context-add-and-delete-folder",
        "made": made.is_some(),
        "deleted": deleted.is_some(),
        "ok": made.is_some() && deleted.is_some(),
    }));

    let temp = browser.core(|p| {
        let url = Url::parse("https://delete-me.example/").map_err(|e| e.to_string())?;
        p.bookmarks()
            .add_url(BookmarkId::TOOLBAR, InsertAt::Index(0), "Delete me", &url)
            .map_err(|e| e.to_string())
    })?;
    browser.bookmarks_changed();
    settle().await;
    let menu = context_menu(window, Some(temp)).await?;
    choose(&menu, "Delete")?;
    let gone = until(|| browser.bookmark(temp).is_none().then_some(())).await;
    steps.push(json!({ "name": "08t-context-delete-link", "ok": gone.is_some() }));
    Ok(())
}
