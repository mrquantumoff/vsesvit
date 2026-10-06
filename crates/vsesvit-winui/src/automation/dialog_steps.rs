//! Scripted steps through the dialogs. Controls are set programmatically and buttons invoked
//! through their automation peers, which runs each dialog's own handlers on core data without
//! any OS input.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::Url;
use vsesvit_core::bookmarks::{BookmarkId, NodeKind};
use vsesvit_core::cookies::ThirdPartyCookies;
use vsesvit_core::extensions::ExtensionId;
use vsesvit_core::history::Transition;
use vsesvit_core::import;
use vsesvit_core::prefs::{TabsPosition, keys};
use vsesvit_core::search::FormField;
use vsesvit_core::testkit;
use vsesvit_core::trackers::TrackingProtection;
use windows_core::{IInspectable, Interface, Result};

use super::{shoot, wait_layout};
use crate::bindings::*;
use crate::browser::Browser;
use crate::dialogs::search_engines::Engines;
use crate::dialogs::{self, Dialog, Preview, SETTINGS_CATEGORIES};
use crate::window::{Backdrop, BrowserWindow};
use crate::{engine, exec, selftest, xaml};

const WAIT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(100);

/// Clicks a button the way assistive technology does.
pub(crate) fn invoke(element: &impl Interface) -> Result<()> {
    let element = element.cast::<UIElement>()?;
    // A peer of an element that has left the tree must not be invoked.
    element.XamlRoot()?;
    let peer = FrameworkElementAutomationPeer::CreatePeerForElement(&element)?;
    peer.GetPattern(PatternInterface::Invoke)?
        .cast::<IInvokeProvider>()?
        .Invoke()
}

/// A button's text.
pub(crate) fn label(button: &Button) -> String {
    button
        .cast::<ContentControl>()
        .and_then(|c| c.Content())
        .and_then(|c| c.cast::<windows_reference::IReference<windows_core::HSTRING>>())
        .and_then(|c| c.Value())
        .map(|c| c.to_string_lossy())
        .unwrap_or_default()
}

/// Invokes the button named `name` in `button`'s flyout, which must be open.
pub(crate) fn confirm_flyout(button: &Button, name: &str) -> Result<()> {
    let content: DependencyObject = button
        .cast::<IButton>()?
        .Flyout()?
        .cast::<Flyout>()?
        .Content()?
        .cast()?;
    let confirm: Button = xaml::find_named(&content, name)
        .ok_or_else(|| windows_core::Error::new(E_FAIL, format!("no {name} in the flyout")))?;
    invoke(&confirm)
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

/// Selects the Settings category whose panel is `panel`.
fn select_category(preview: &Preview, panel: &str) -> Result<()> {
    let index = SETTINGS_CATEGORIES
        .iter()
        .position(|c| c.panel == panel)
        .ok_or_else(|| {
            windows_core::Error::new(E_FAIL, format!("no Settings category {panel:?}"))
        })?;
    select_index(&preview.find::<ListView>("SettingsCategories")?, index)
}

/// Settings over the window, on the category showing `panel`.
pub(crate) async fn settings_on(window: &Rc<BrowserWindow>, panel: &str) -> Result<Preview> {
    let preview = open(window, Dialog::Settings).await?;
    select_category(&preview, panel)?;
    settle().await;
    Ok(preview)
}

/// Lets the dialog finish rebuilding after a change.
async fn settle() {
    exec::sleep(Duration::from_millis(500)).await;
}

async fn until<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
    exec::wait_for(WAIT, POLL, &mut f).await
}

/// Settings: each category shows its own panel alone, and one taller than the dialog scrolls
/// to its end; then the settings themselves, category by category.
pub(super) async fn settings(
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    page2: &Url,
    steps: &mut Vec<Value>,
) -> Result<()> {
    let preview = open(window, Dialog::Settings).await?;
    let categories: ListView = preview.find("SettingsCategories")?;
    for (index, category) in SETTINGS_CATEGORIES.iter().enumerate() {
        select_index(&categories, index)?;
        settle().await;
        category_steps(window, out_dir, &preview, category.panel, steps).await?;
    }

    select_category(&preview, "AppearancePanel")?;
    settle().await;
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

    let transparent: ToggleSwitch = preview.find("Transparent")?;
    let browser = window.browser().ok_or_else(windows_core::Error::empty)?;
    let before = browser.backdrop();
    transparent.SetIsOn(true)?;
    let on = browser.backdrop();
    transparent.SetIsOn(false)?;
    let off = browser.backdrop();
    steps.push(json!({
        "name": "14c-settings-transparent-window",
        "backdrops": format!("{before:?} -> {on:?} -> {off:?}"),
        "ok": before == Backdrop::Mica && on == Backdrop::Acrylic && off == Backdrop::Mica,
    }));
    home_button(window, out_dir, &preview, page2, steps).await?;

    select_category(&preview, "SearchPanel")?;
    settle().await;
    let compact: ToggleSwitch = preview.find("CompactAddress")?;
    let full_urls: ToggleSwitch = preview.find("FullUrls")?;
    let defaults = (compact.IsOn()?, full_urls.IsOn()?);
    let narrow = window.address_width();
    compact.SetIsOn(false)?;
    exec::sleep(Duration::from_millis(300)).await;
    let wide = window.address_width();
    compact.SetIsOn(true)?;
    exec::sleep(Duration::from_millis(300)).await;
    steps.push(json!({
        "name": "14d-settings-address-bar",
        "compact_and_full_urls": defaults,
        "widths": [narrow, wide],
        "ok": defaults == (true, false) && narrow > 0.0 && narrow <= 720.5 && wide > narrow,
    }));
    search_engines(window, out_dir, &preview, steps).await?;

    select_category(&preview, "GeneralPanel")?;
    settle().await;
    let gpu: ToggleSwitch = preview.find("HardwareAcceleration")?;
    let smooth: ToggleSwitch = preview.find("SmoothScrolling")?;
    let arguments = || engine::browser_arguments(|pref| browser.core(|p| p.prefs().get(pref)));
    let defaults = arguments();
    gpu.SetIsOn(false)?;
    smooth.SetIsOn(false)?;
    let off = arguments();
    gpu.SetIsOn(true)?;
    smooth.SetIsOn(true)?;
    steps.push(json!({
        "name": "14f-settings-engine-startup-switches",
        "arguments": [defaults, off, arguments()],
        "ok": defaults.is_empty() && off == "--disable-smooth-scrolling --disable-gpu" && arguments().is_empty(),
    }));

    select_category(&preview, "PrivacyPanel")?;
    settle().await;
    let notice = xaml::is_visible(&preview.find::<UIElement>("PasswordsNotice")?);
    let switch = preview.find::<UIElement>("SavePasswords").is_ok();
    let tab = window.active_tab().ok_or_else(windows_core::Error::empty)?;
    let autosave = tab
        .core()
        .ok_or_else(windows_core::Error::empty)?
        .Settings()?
        .cast::<ICoreWebView2Settings4>()?
        .IsPasswordAutosaveEnabled()?;
    steps.push(json!({
        "name": "14g-settings-no-passwords",
        "notice_shown": notice,
        "save_passwords_switch": switch,
        "engine_password_autosave": autosave,
        "ok": notice && !switch && !autosave,
    }));

    let tracking: ComboBox = preview.find("TrackingProtection")?;
    let description: TextBlock = preview.find("TrackingProtectionDescription")?;
    let seen = || {
        let shown = dialogs::selected_index(&tracking)
            .and_then(|i| TrackingProtection::ALL.get(i).copied());
        let stored = browser.core(|p| p.prefs().get(&keys::TRACKING_PROTECTION));
        let text = description
            .Text()
            .map(|t| t.to_string())
            .unwrap_or_default();
        let ok = shown == Some(stored) && text == stored.description();
        (format!("{shown:?} stored {stored:?}: {text}"), stored, ok)
    };
    let default = seen();
    select_index(&tracking, 2)?;
    let strict = seen();
    select_index(&tracking, 1)?;
    let restored = seen();
    steps.push(json!({
        "name": "14h-settings-tracking-protection",
        "seen": [default.0, strict.0, restored.0],
        "ok": default.2 && strict.2 && restored.2
            && default.1 == TrackingProtection::Standard
            && strict.1 == TrackingProtection::Strict
            && restored.1 == TrackingProtection::Standard,
    }));

    let https: ToggleSwitch = preview.find("HttpsOnly")?;
    let stored = || browser.core(|p| p.prefs().get(&keys::HTTPS_ONLY));
    let default = (https.IsOn()?, stored());
    https.SetIsOn(true)?;
    let on = stored();
    https.SetIsOn(false)?;
    let dns: TextBlock = preview.find("SecureDns")?;
    let dns = dns.Text()?.to_string();
    steps.push(json!({
        "name": "14j-settings-https-only",
        "default": default,
        "stored_on": on,
        "secure_dns": dns,
        "ok": default == (false, false) && on && !stored() && !dns.is_empty(),
    }));

    let cookies: ComboBox = preview.find("ThirdPartyCookies")?;
    let description: TextBlock = preview.find("ThirdPartyCookiesDescription")?;
    let seen = || {
        let shown =
            dialogs::selected_index(&cookies).and_then(|i| ThirdPartyCookies::ALL.get(i).copied());
        let stored = browser.core(|p| p.prefs().get(&keys::THIRD_PARTY_COOKIES));
        let text = description
            .Text()
            .map(|t| t.to_string())
            .unwrap_or_default();
        let ok = shown == Some(stored) && text == stored.description();
        (format!("{shown:?} stored {stored:?}: {text}"), stored, ok)
    };
    let default = seen();
    select_index(&cookies, 2)?;
    let block = seen();
    select_index(&cookies, 1)?;
    let restored = seen();
    steps.push(json!({
        "name": "14k-settings-third-party-cookies",
        "seen": [default.0, block.0, restored.0],
        "ok": default.2 && block.2 && restored.2
            && default.1 == ThirdPartyCookies::BlockInPrivate
            && block.1 == ThirdPartyCookies::Block
            && restored.1 == ThirdPartyCookies::BlockInPrivate,
    }));
    Ok(())
}

/// Settings, Search: the list shows every engine with the default marked, and Add opens the
/// editor, which refuses a shortcut another engine has. It closes without saving.
async fn search_engines(
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    preview: &Preview,
    steps: &mut Vec<Value>,
) -> Result<()> {
    let engines = preview
        .wired::<Engines>()
        .ok_or_else(|| windows_core::Error::new(E_FAIL, "the Settings dialog has no search engines"))?;
    let names: Vec<String> = engines.rows()?.into_iter().map(|(name, _)| name).collect();
    click(preview, "SearchEngineAdd")?;
    let editor = until(|| engines.editor())
        .await
        .ok_or_else(|| windows_core::Error::new(E_FAIL, "Add opened no editor"))?;
    editor.fill(FormField::Name, "Example")?;
    editor.fill(FormField::Keyword, "w")?;
    settle().await;
    let taken = editor.shown(FormField::Keyword);
    shoot(window, out_dir, "14i-settings-search-engine-editor", steps, |_| {
        json!({
            "engines": names,
            "shortcut_w": taken,
            "ok": names.len() == 4
                && names.iter().any(|n| n == "DuckDuckGo (Default)")
                && taken == (Some("Another search engine has this shortcut".to_owned()), false),
        })
    })
    .await;
    editor.close();
    settle().await;
    Ok(())
}

/// One category's panel, alone on screen; a panel taller than the dialog also scrolled to its
/// end, where the General panel shows the profile folder.
async fn category_steps(
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    preview: &Preview,
    panel_name: &str,
    steps: &mut Vec<Value>,
) -> Result<()> {
    let panel: IScrollViewer = preview.find(panel_name)?;
    let alone = SETTINGS_CATEGORIES.iter().all(|c| {
        preview
            .find::<UIElement>(c.panel)
            .is_ok_and(|p| xaml::is_visible(&p) == (c.panel == panel_name))
    });
    let slug = panel_name.trim_end_matches("Panel").to_lowercase();
    let scrollable = panel.ScrollableHeight()?;
    shoot(window, out_dir, &format!("14-settings-{slug}"), steps, |_| {
        json!({ "panel": panel_name, "shown_alone": alone, "scrollable_height": scrollable, "ok": alone })
    })
    .await;
    if scrollable <= 0.0 {
        return Ok(());
    }
    panel.ChangeViewWithOptionalAnimation(None, Some(scrollable), None, true)?;
    settle().await;
    let offset = panel.VerticalOffset()?;
    let profile_in_view = (panel_name == "GeneralPanel")
        .then(|| in_view(&panel, &preview.find::<UIElement>("ProfilePath")?))
        .transpose()?;
    shoot(
        window,
        out_dir,
        &format!("14-settings-{slug}-scrolled"),
        steps,
        |_| {
            json!({
                "offset": offset,
                "scrollable_height": scrollable,
                "profile_folder_in_view": profile_in_view,
                "ok": (offset - scrollable).abs() < 1.0 && profile_in_view != Some(false),
            })
        },
    )
    .await;
    panel.ChangeViewWithOptionalAnimation(None, Some(0.0), None, true)?;
    Ok(())
}

/// Whether all of `element` is inside the visible part of `viewer`.
fn in_view(viewer: &IScrollViewer, element: &UIElement) -> Result<bool> {
    let viewer = viewer.cast::<FrameworkElement>()?;
    let top = element
        .TransformToVisual(&viewer.cast::<UIElement>()?)?
        .TransformPoint(Point { x: 0.0, y: 0.0 })?
        .y;
    let height = element.cast::<FrameworkElement>()?.ActualHeight()?;
    Ok(top >= 0.0 && f64::from(top) + height <= viewer.ActualHeight()? + 0.5)
}

/// The Home button follows its switch in the toolbar at once, and opens the home page in the
/// current tab.
async fn home_button(
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    preview: &Preview,
    page2: &Url,
    steps: &mut Vec<Value>,
) -> Result<()> {
    let browser = window.browser().ok_or_else(windows_core::Error::empty)?;
    let switch: ToggleSwitch = preview.find("ShowHomeButton")?;
    let hidden_by_default = !switch.IsOn()? && !window.home_button_shown();
    switch.SetIsOn(true)?;
    settle().await;
    let shown = window.home_button_shown();
    shoot(window, out_dir, "14e-settings-home-button", steps, |_| {
        json!({ "hidden_by_default": hidden_by_default, "shown": shown, "ok": hidden_by_default && shown })
    })
    .await;

    browser.write_pref(&keys::HOMEPAGE, &page2.to_string());
    let tab = window.active_tab().ok_or_else(windows_core::Error::empty)?;
    let was = tab.state().url;
    window.go_home();
    let went = until(|| (tab.state().url == page2.as_str()).then_some(())).await;
    tab.go_back();
    let returned = until(|| (tab.state().url == was && !tab.state().loading()).then_some(())).await;
    if let Err(e) = browser.core(|p| p.prefs().reset(&keys::HOMEPAGE)) {
        log::warn!("home page: {e}");
    }
    switch.SetIsOn(false)?;
    let hidden_again = !window.home_button_shown();
    steps.push(json!({
        "name": "14e-settings-home-button-opens-home-page",
        "opened_home_page": went.is_some(),
        "back_to": returned.map(|()| was),
        "hidden_again": hidden_again,
        "ok": went.is_some() && hidden_again,
    }));
    Ok(())
}

/// Settings, Privacy and security: the Clear browsing data flyout's Clear deletes history in
/// core and the page's cookies in the engine, and says so under the button.
pub(super) async fn clear_browsing_data(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    page: &Url,
    steps: &mut Vec<Value>,
) -> Result<()> {
    let tab = window.active_tab().ok_or_else(windows_core::Error::empty)?;
    if let Err(e) = browser.core(|p| p.history().record_visit(page, Transition::Link)) {
        log::warn!("history: {e}");
    }
    tab.eval("document.cookie = 'vsesvit_probe=1; max-age=3600'; document.cookie")
        .await?;
    let cookie_before = tab.eval("document.cookie").await?;
    let visits = || {
        browser
            .core(|p| p.history().visits_between(0, i64::MAX, 10))
            .map(|v| v.len())
            .unwrap_or_default()
    };
    let visits_before = visits();

    let preview = settings_on(window, "PrivacyPanel").await?;
    let button: Button = preview.find("ClearBrowsingData")?;
    invoke(&button)?;
    settle().await;
    shoot(
        window,
        out_dir,
        "23-clear-browsing-data-flyout",
        steps,
        |_| json!({ "ok": true }),
    )
    .await;
    confirm_flyout(&button, "ClearBrowsingDataConfirm")?;
    let status: TextBlock = preview.find("ClearBrowsingDataStatus")?;
    let reported = until(|| {
        status
            .Text()
            .ok()
            .filter(|t| !t.is_empty() && xaml::is_visible(&status))
    })
    .await;
    let cookie_after = tab.eval("document.cookie").await?;
    exec::sleep(Duration::from_millis(400)).await;
    shoot(window, out_dir, "23-clear-browsing-data", steps, |_| {
        json!({
            "visits": [visits_before, visits()],
            "cookie": [cookie_before, cookie_after],
            "status": reported,
            "ok": visits_before > 0 && visits() == 0
                && cookie_before.contains("vsesvit_probe") && !cookie_after.contains("vsesvit_probe")
                && reported.as_deref() == Some("Browsing data cleared."),
        })
    })
    .await;
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
                .and_then(|c| c.cast::<FrameworkElement>().ok())
                .and_then(|c| xaml::find::<TextBlock>(&c, "Label").ok())
                .and_then(|t| t.Text().ok());
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

/// How many favicons the tree shows on screen: visible images in its visual tree. An image is
/// made visible only once its PNG has decoded.
fn favicons_shown(tree: &TreeView) -> usize {
    fn count(element: &DependencyObject) -> usize {
        let own = element
            .cast::<Image>()
            .is_ok_and(|image| xaml::is_visible(&image))
            && element
                .cast::<FrameworkElement>()
                .and_then(|e| e.Name())
                .is_ok_and(|name| name == "Favicon");
        let children = VisualTreeHelper::GetChildrenCount(element).unwrap_or(0);
        usize::from(own)
            + (0..children)
                .filter_map(|i| VisualTreeHelper::GetChild(element, i).ok())
                .map(|child| count(&child))
                .sum::<usize>()
    }
    tree.cast::<DependencyObject>()
        .map(|t| count(&t))
        .unwrap_or(0)
}

/// Whether the tree shows `label` as rendered text (not only as node content).
fn label_rendered(tree: &TreeView, label: &str) -> bool {
    fn find(element: &DependencyObject, label: &str) -> bool {
        if element
            .cast::<TextBlock>()
            .is_ok_and(|t| t.Text().is_ok_and(|text| text == label))
        {
            return true;
        }
        let children = VisualTreeHelper::GetChildrenCount(element).unwrap_or(0);
        (0..children)
            .filter_map(|i| VisualTreeHelper::GetChild(element, i).ok())
            .any(|child| find(&child, label))
    }
    tree.cast::<DependencyObject>()
        .is_ok_and(|t| find(&t, label))
}

fn select_node(tree: &TreeView, label: &str) -> Result<()> {
    let node = tree_node(tree, label)
        .ok_or_else(|| windows_core::Error::new(E_FAIL, format!("no tree node {label:?}")))?;
    tree.cast::<ITreeView2>()?.SetSelectedNode(&node)
}

/// Bookmarks: rename a folder, add a folder inside it and delete that again, and move a
/// bookmark to "Other bookmarks", each checked in core; then export them to a file and read
/// the moved bookmark back from it.
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

    select_node(&tree, "Fixture folder")?;
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
    let favicons = until(|| (favicons_shown(&tree) > 0).then(|| favicons_shown(&tree))).await;
    preview.find::<Button>("ExportRun")?;
    let path = out_dir.join("bookmarks.html");
    // An earlier run's file would pass for one this run failed to write.
    let _ = std::fs::remove_file(&path);
    dialogs::export_bookmarks(browser, path.clone()).await;
    let exported = std::fs::read_to_string(&path)
        .is_ok_and(|text| selftest::has_link(&import::parse_html(&text), page2.as_str()));
    exec::sleep(Duration::from_millis(400)).await;
    shoot(window, out_dir, "16-bookmarks-dialog", steps, |w| {
        json!({
            "editor_showed": shown,
            "renamed": renamed.is_some(),
            "added_folder": added.map(|n| n.title),
            "deleted_it": deleted.is_some(),
            "moved_to_other": moved.is_some(),
            "favicons_shown": favicons,
            "exported": exported,
            "labels_rendered": label_rendered(&tree, "Other bookmarks") && label_rendered(&tree, "Second tab"),
            "bar": format!("{:?}", w.bookmarks_bar_items()),
            "ok": shown == "Fixture folder" && renamed.is_some() && deleted.is_some() && moved.is_some()
                && favicons.is_some()
                && exported
                && label_rendered(&tree, "Other bookmarks")
                && label_rendered(&tree, "Second tab"),
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
    // A title XML cannot carry must not cost its page its row: every row below it would then
    // open and delete the page above it.
    if let Err(e) = browser.core(|p| p.history().set_title(page2, "Vsesvit\u{FFFF} fixture 2")) {
        log::warn!("history title: {e}");
    }
    let pages = browser
        .core(|p| p.history().visits_between(0, i64::MAX, 300))
        .map(|visits| {
            let urls: std::collections::HashSet<_> =
                visits.into_iter().map(|(entry, _)| entry.url).collect();
            urls.len()
        })
        .unwrap_or_default();
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
    steps.push(json!({
        "name": "17a-history-rows-match-pages",
        "pages": pages,
        "rows": all,
        "ok": all as usize == pages,
    }));
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
