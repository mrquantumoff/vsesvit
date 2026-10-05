//! The `shortcuts`, `shortcuts_sync` and `save_page` checks: reassigning shortcuts through
//! Settings or by a sync applies at once to the page already loaded and to the window, and
//! Ctrl+Shift+S saves the page; and the `extension_commands` check of the probe's shortcuts.

use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::json;
use vsesvit_core::crdt::{DeviceId, Hlc, JsonText, Lww, Seq, Stamp};
use vsesvit_core::prefs::{PrefRecord, keys};
use vsesvit_core::shortcuts::{Command as Core, Keymap};
use vsesvit_core::sync::{Kind, WireRecord};
use vsesvit_core::testkit;
use windows_core::Interface;

use super::{POLL, Probe, eval, tab_ids, until, visits, wait_ready};
use crate::automation::{confirm_flyout, invoke, press, settings_on};
use crate::browser::Browser;
use crate::bindings::{
    Button, CoreWebView2SaveAsKind, FrameworkElement, ICoreWebView2_9, ICoreWebView2_25,
    IScrollViewer, Point, UIElement,
};
use crate::dialogs::{
    Dialog, Preview, ShortcutsPage, extension_shortcut_row_name, shortcut_row_name,
};
use crate::exec;
use crate::shortcuts::{self, Command, InPage, Mods};
use crate::tab::Tab;
use crate::window::BrowserWindow;

const SETTLE: Duration = Duration::from_millis(500);
/// DevTools modifier bits for `press`.
const ALT: u8 = 1;
const CTRL: u8 = 2;
const SHIFT: u8 = 8;
/// How long the check waits for WebView2 to fire the probe's `commands.onCommand`.
const ON_COMMAND_WAIT: Duration = Duration::from_secs(5);
/// `VirtualKeyModifiers` bits of an accelerator.
const ACCEL_CTRL: u32 = 1;
const ACCEL_SHIFT: u32 = 4;
/// Page-side record of every key press the page itself sees, and whether it arrived prevented.
const RECORD_KEYS: &str = "window.__vsesvitMark = 1; window.__keys = []; \
    addEventListener('keydown', e => __keys.push(e.keyCode + ':' + e.defaultPrevented)); 1";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Settings over the window, on its Keyboard shortcuts page.
async fn shortcuts_page(
    window: &Rc<BrowserWindow>,
) -> Result<(Preview, Rc<ShortcutsPage>), String> {
    let preview = settings_on(window, "ShortcutsPanel").await.map_err(err)?;
    let page = preview
        .wired::<ShortcutsPage>()
        .ok_or("the Settings dialog has no shortcuts page")?;
    Ok((preview, page))
}

/// Scrolls `command`'s row into view and activates it, which opens the capture flyout.
async fn open_capture(preview: &Preview, command: Core) -> Result<(), String> {
    let row: FrameworkElement = preview.find(&shortcut_row_name(command)).map_err(err)?;
    let panel: IScrollViewer = preview.find("ShortcutsPanel").map_err(err)?;
    let top = row
        .cast::<UIElement>()
        .and_then(|r| r.TransformToVisual(&panel.cast::<UIElement>()?))
        .and_then(|t| t.TransformPoint(Point { x: 0.0, y: 0.0 }))
        .map_err(err)?
        .y;
    let offset = panel.VerticalOffset().map_err(err)? + f64::from(top) - 120.0;
    panel
        .ChangeViewWithOptionalAnimation(None, Some(offset.max(0.0)), None, true)
        .map_err(err)?;
    exec::sleep(Duration::from_millis(300)).await;
    invoke(&row).map_err(err)?;
    exec::sleep(SETTLE).await;
    Ok(())
}

fn has_accelerator(window: &BrowserWindow, vk: i32, mods: u32) -> bool {
    window.accelerator_keys().contains(&(vk, mods))
}

async fn marked(tab: &Tab) -> bool {
    eval(tab, "String(window.__vsesvitMark)").await.as_deref() == Ok("\"1\"")
}

/// Waits up to `limit` for the scripted run's dialog, then closes it.
async fn dialog_opened(window: &BrowserWindow, limit: Duration) -> Option<Dialog> {
    let shown = exec::wait_for(limit, Duration::from_millis(100), || {
        window.scripted_dialog()
    })
    .await;
    window.close_scripted_dialog();
    shown
}

/// The chords stored for History.
fn history_chords(browser: &Browser) -> Vec<String> {
    browser
        .core(|c| c.prefs().keymap())
        .chords(Core::ShowHistory)
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// What the window's bindings run on `vk` with `mods`, and how the page may override it.
pub(super) fn binding(vk: u16, mods: Mods) -> Option<(Command, InPage)> {
    shortcuts::current()
        .list()
        .iter()
        .find(|b| b.vk == vk && b.mods == mods)
        .map(|b| (b.command, b.in_page))
}

/// With History moved to Ctrl+Shift+Y: that chord opens it from the loaded page, Ctrl+H no
/// longer does, and neither reloads the page.
async fn history_moved(
    window: &BrowserWindow,
    tab: &Tab,
    p: &Probe,
    detail: &mut Vec<String>,
) -> Result<(), String> {
    p.observe("Ctrl+Shift+Y and Ctrl+H in the page");
    press(tab, 0x59, CTRL | SHIFT).await?;
    let new_chord = dialog_opened(window, Duration::from_secs(5)).await;
    press(tab, 0x48, CTRL).await?;
    let old_chord = dialog_opened(window, Duration::from_millis(1500)).await;
    let kept = marked(tab).await;
    detail.push(format!(
        "in the loaded page Ctrl+Shift+Y opened {new_chord:?}, Ctrl+H opened {old_chord:?}, page not reloaded {kept}"
    ));
    if new_chord != Some(Dialog::History) || old_chord.is_some() || !kept {
        return Err(detail.join("; "));
    }
    Ok(())
}

pub(super) async fn shortcuts(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    out_dir: &Path,
    p: &Probe,
) -> Result<String, String> {
    let browser = window.browser().ok_or("no browser")?;
    eval(tab, RECORD_KEYS).await?;
    let mut detail = Vec::new();

    let (preview, page) = shortcuts_page(window).await?;
    let shot = window.capture().await.map_err(err)?;
    std::fs::write(out_dir.join("settings-shortcuts.png"), &shot.png).map_err(err)?;
    let listed: Vec<Core> = shortcuts::listed().collect();
    let rows = listed
        .iter()
        .filter(|&&c| {
            preview
                .find::<FrameworkElement>(&shortcut_row_name(c))
                .is_ok()
        })
        .count();
    detail.push(format!(
        "settings lists {rows} of {} commands",
        listed.len()
    ));
    if rows != listed.len()
        || preview
            .find::<FrameworkElement>(&shortcut_row_name(Core::ZoomIn))
            .is_ok()
    {
        return Err(detail.join("; "));
    }

    p.observe("capturing a shortcut for History");
    open_capture(&preview, Core::ShowHistory).await?;
    let suspended = window.accelerator_keys().is_empty();
    page.capture_key(0x54, Mods::CTRL);
    exec::sleep(Duration::from_millis(300)).await;
    let conflict = page.capture_note();
    let shot = window.capture().await.map_err(err)?;
    std::fs::write(out_dir.join("shortcut-capture.png"), &shot.png).map_err(err)?;
    page.capture_key(0x59, Mods::of(true, true, false));
    let free = page.capture_note();
    let scroller: IScrollViewer = preview.find("ShortcutsPanel").map_err(err)?;
    let scrolled = scroller.VerticalOffset().map_err(err)?;
    page.capture_key(0x0D, Mods::NONE);
    exec::sleep(SETTLE).await;
    let kept_scroll =
        scrolled > 0.0 && (scroller.VerticalOffset().map_err(err)? - scrolled).abs() < 1.0;
    drop(preview);
    let stored = history_chords(&browser);
    let xaml_y = has_accelerator(window, 0x59, ACCEL_CTRL | ACCEL_SHIFT);
    let xaml_h = has_accelerator(window, 0x48, ACCEL_CTRL);
    detail.push(format!(
        "capture: accelerators off while open {suspended}, Ctrl+T noted {conflict:?}, Ctrl+Shift+Y noted {free:?}; stored {stored:?}, the list kept its scroll position {kept_scroll}; window accelerators Ctrl+Shift+Y={xaml_y} Ctrl+H={xaml_h}"
    ));
    if !(suspended
        && conflict == "Also used by New tab. Saving moves it here."
        && free.is_empty()
        && stored == ["Ctrl+Shift+Y"]
        && kept_scroll
        && xaml_y
        && !xaml_h)
    {
        return Err(detail.join("; "));
    }

    history_moved(window, tab, p, &mut detail).await?;

    p.observe("giving Ctrl+R to New tab");
    let (preview, page) = shortcuts_page(window).await?;
    open_capture(&preview, Core::NewTab).await?;
    page.capture_key(0x52, Mods::CTRL);
    let taken = page.capture_note();
    page.capture_key(0x0D, Mods::NONE);
    exec::sleep(SETTLE).await;
    drop(preview);
    let before = window.tab_count();
    let open = tab_ids(window);
    press(tab, 0x52, CTRL).await?;
    let opened = exec::wait_for(Duration::from_secs(5), Duration::from_millis(100), || {
        window
            .tabs_in_order()
            .into_iter()
            .find(|t| !open.contains(&t.id))
    })
    .await;
    if let Some(new) = &opened {
        window.close_tab(new.id);
    }
    let kept = marked(tab).await;
    detail.push(format!(
        "Ctrl+R noted {taken:?}; in the page it opened a tab {} ({before} -> {}), page not reloaded {kept}",
        opened.is_some(),
        before + usize::from(opened.is_some())
    ));
    if taken != "Also used by Reload. Saving moves it here." || opened.is_none() || !kept {
        return Err(detail.join("; "));
    }

    p.observe("moving Reload to Ctrl+Shift+E");
    let chord = "Ctrl+Shift+E".parse().map_err(err)?;
    browser.edit_keymap(|k| {
        k.assign(Core::Reload, [chord]);
    });
    exec::sleep(SETTLE).await;
    eval(tab, "window.__keys = []; 1").await?;
    press(tab, 0x74, 0).await?;
    exec::sleep(Duration::from_millis(300)).await;
    let f5 = eval(tab, "__keys.join(' ')").await?;
    let hidden = eval(tab, "typeof globalThis.vsesvitKeys").await?;
    let kind = binding(0x45, Mods::of(true, true, false));
    press(tab, 0x45, CTRL | SHIFT).await?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut reloaded = false;
    while !reloaded && std::time::Instant::now() < deadline {
        exec::sleep(Duration::from_millis(100)).await;
        reloaded =
            eval(tab, "String(window.__vsesvitMark)").await.as_deref() == Ok("\"undefined\"");
    }
    wait_ready(tab, p).await;
    detail.push(format!(
        "Reload on Ctrl+Shift+E is {kind:?}; F5 reached the page as {f5}; the page sees vsesvitKeys as {hidden}; Ctrl+Shift+E reloaded {reloaded}"
    ));
    if kind != Some((Command::Reload, InPage::Overridable))
        || f5 != "\"116:true\""
        || hidden != "\"undefined\""
        || !reloaded
    {
        return Err(detail.join("; "));
    }

    p.observe("Reset all");
    let (preview, _page) = shortcuts_page(window).await?;
    let button = preview.find::<Button>("ShortcutsResetAll").map_err(err)?;
    invoke(&button).map_err(err)?;
    exec::sleep(SETTLE).await;
    confirm_flyout(&button, "ShortcutsResetAllConfirm").map_err(err)?;
    exec::sleep(SETTLE).await;
    drop(preview);
    let defaults = browser.core(|c| c.prefs().keymap()) == Keymap::default();
    let mut toggled = Vec::new();
    for _ in 0..2 {
        let before = window.is_pane_collapsed();
        press(tab, 0x53, CTRL).await?;
        let changed = exec::wait_for(Duration::from_secs(5), Duration::from_millis(100), || {
            (window.is_pane_collapsed() != before).then_some(())
        })
        .await;
        toggled.push(changed.is_some());
    }
    let save = binding(0x53, Mods::of(true, true, false));
    press(tab, 0x48, CTRL).await?;
    let history = dialog_opened(window, Duration::from_secs(5)).await;
    detail.push(format!(
        "after Reset all the keymap is the default {defaults}; Ctrl+S toggled the tab list {toggled:?}; Ctrl+Shift+S is {save:?}; Ctrl+H opened {history:?}"
    ));
    let ok = defaults
        && toggled == [true, true]
        && save == Some((Command::SavePage, InPage::Overridable))
        && history == Some(Dialog::History);
    let detail = detail.join("; ");
    ok.then_some(detail.clone()).ok_or(detail)
}

/// `keyboard.shortcuts` as another device sends it, stamped just after the value held here
/// so it wins.
fn remote_shortcuts(
    browser: &Browser,
    value: Option<serde_json::Value>,
) -> Result<WireRecord, String> {
    let key = keys::SHORTCUTS.key;
    let local = browser
        .core(|c| c.sync().changes_since(Kind::Prefs, Seq::ZERO, 1024))
        .map_err(err)?
        .records
        .into_iter()
        .find(|r| r.id == key)
        .map(|r| serde_json::from_slice::<PrefRecord>(&r.body))
        .transpose()
        .map_err(err)?
        .map_or(Stamp::ZERO, |r| r.value.at);
    let at = Stamp {
        hlc: Hlc(local.hlc.0 + 1),
        device: DeviceId(0x5e11),
    };
    let record = PrefRecord {
        key: key.to_owned(),
        value: Lww::new(value.as_ref().map(JsonText::from_value), at),
    };
    Ok(WireRecord {
        kind: Kind::Prefs,
        id: key.to_owned(),
        body: serde_json::to_vec(&record).map_err(err)?,
    })
}

/// Applies `record` the way a sync engine does: `sync().apply`, then `Browser::sync_applied`.
async fn apply_remote(browser: &Browser, record: WireRecord) -> Result<Vec<String>, String> {
    let site_settings = browser.core(|c| c.site_permissions().all());
    let report = browser
        .core(|c| c.sync().apply(vec![record]))
        .map_err(err)?;
    browser.sync_applied(&report.changed, &site_settings);
    exec::sleep(SETTLE).await;
    Ok(report.changed.prefs)
}

pub(super) async fn shortcuts_sync(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    p: &Probe,
) -> Result<String, String> {
    let browser = window.browser().ok_or("no browser")?;
    eval(tab, RECORD_KEYS).await?;
    let mut detail = Vec::new();

    p.observe("applying a remote record moving History to Ctrl+Shift+Y");
    let record = remote_shortcuts(&browser, Some(json!({"show-history": ["Ctrl+Shift+Y"]})))?;
    let changed = apply_remote(&browser, record).await?;
    let stored = history_chords(&browser);
    let xaml_y = has_accelerator(window, 0x59, ACCEL_CTRL | ACCEL_SHIFT);
    let xaml_h = has_accelerator(window, 0x48, ACCEL_CTRL);
    detail.push(format!(
        "remote record changed {changed:?}; History reads {stored:?}; window accelerators Ctrl+Shift+Y={xaml_y} Ctrl+H={xaml_h}"
    ));
    if changed != [keys::SHORTCUTS.key] || stored != ["Ctrl+Shift+Y"] || !xaml_y || xaml_h {
        return Err(detail.join("; "));
    }

    history_moved(window, tab, p, &mut detail).await?;

    p.observe("applying a newer remote record resetting the shortcuts");
    let record = remote_shortcuts(&browser, None)?;
    let changed = apply_remote(&browser, record).await?;
    let defaults = browser.core(|c| c.prefs().keymap()) == Keymap::default();
    press(tab, 0x48, CTRL).await?;
    let history = dialog_opened(window, Duration::from_secs(5)).await;
    let kept = marked(tab).await;
    detail.push(format!(
        "reset record changed {changed:?}; the keymap is the default {defaults}; Ctrl+H opened {history:?}, page not reloaded {kept}"
    ));
    let ok = changed == [keys::SHORTCUTS.key]
        && defaults
        && history == Some(Dialog::History)
        && kept;
    let detail = detail.join("; ");
    ok.then_some(detail.clone()).ok_or(detail)
}

/// Types Ctrl+Shift+S into the fixture page with the Save As dialog answered by
/// `SaveAsUIShowing`: a single-file save to `<out_dir>/saved-page.mhtml`.
pub(super) async fn save_page(tab: &Rc<Tab>, out_dir: &Path, p: &Probe) -> Result<String, String> {
    let path = out_dir.join("saved-page.mhtml");
    let _ = std::fs::remove_file(&path);
    let core = tab.core().ok_or("the tab has no engine view")?;
    let target = path.to_string_lossy().into_owned();
    let mime = Rc::new(std::cell::RefCell::new(String::new()));
    let seen = mime.clone();
    let revoker = core
        .cast::<ICoreWebView2_25>()
        .and_then(|c| {
            c.SaveAsUIShowing(move |_, args| {
                let Some(args) = args.as_ref() else { return };
                *seen.borrow_mut() = args.ContentMimeType().unwrap_or_default();
                let answered = args
                    .SetSuppressDefaultDialog(true)
                    .and_then(|()| args.SetSaveAsFilePath(&target))
                    .and_then(|()| args.SetKind(CoreWebView2SaveAsKind::SingleFile))
                    .and_then(|()| args.SetAllowReplace(true));
                if let Err(e) = answered {
                    log::warn!("SaveAsUIShowing: {e}");
                }
            })
        })
        .map_err(err)?;
    press(tab, 0x53, CTRL | SHIFT).await?;
    let text = until(p, |p| {
        let text = std::fs::read(&path)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        p.observe(format!(
            "{} {}",
            path.display(),
            text.as_ref()
                .map_or("not written".to_owned(), |t| format!("{} bytes", t.len()))
        ));
        text.filter(|t| t.contains("Vsesvit fixture"))
    })
    .await;
    drop(revoker);
    exec::sleep(Duration::from_millis(700)).await;
    let edge_flyout = core
        .cast::<ICoreWebView2_9>()
        .and_then(|c| c.IsDefaultDownloadDialogOpen())
        .map_err(err)?;
    let detail = format!(
        "Ctrl+Shift+S in the page: SaveAsUIShowing for {:?}, saved {} ({} bytes) as a single file containing \"Vsesvit fixture\"; WebView2's download flyout open afterwards: {edge_flyout}",
        mime.borrow(),
        path.display(),
        text.len()
    );
    (!edge_flyout).then_some(detail.clone()).ok_or(detail)
}

/// The `extension_commands` check, with the probe installed and the fixture page open: the
/// probe's action command (Alt+Shift+P) is bound before the page and opens its popup from the
/// page, its named command (Alt+Shift+K) is not bound, and Settings lists the action command.
/// Whether WebView2 itself fires `commands.onCommand` for the named command is reported, not
/// judged.
pub(super) async fn extension_commands(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    p: &Probe,
) -> Result<String, String> {
    let browser = window.browser().ok_or("no browser")?;
    let mut detail = Vec::new();
    let alt_shift = Mods::of(false, true, true);
    let bindings = shortcuts::current();
    let action = binding(0x50, alt_shift);
    let opens = match action {
        Some((Command::ExtensionAction(index), _)) => bindings.extension_action(index),
        _ => None,
    };
    let named = binding(0x4B, alt_shift);
    detail.push(format!(
        "Alt+Shift+P binds {action:?}, opening the action of {opens:?}; Alt+Shift+K binds {named:?}"
    ));
    if action.map(|(_, kind)| kind) != Some(InPage::Reserved)
        || opens != Some(testkit::PROBE_ID)
        || named.is_some()
    {
        return Err(detail.join("; "));
    }

    let expected = browser
        .extension_actions()
        .into_iter()
        .find(|a| a.extension_id == testkit::PROBE_ID)
        .and_then(|a| a.popup_url())
        .ok_or("the probe's action has no popup")?;
    if let Some(open) = window.extension_popup() {
        open.hide();
    }
    p.observe("Alt+Shift+P in the page");
    press(tab, 0x50, ALT | SHIFT).await?;
    let (url, title) = until(p, |p| {
        let shown = window
            .extension_popup()
            .map(|popup| (popup.url(), popup.title()));
        p.observe(format!("open popup (address, title): {shown:?}"));
        match shown? {
            (Some(url), Some(title)) if url == expected && visits(&title).is_some() => {
                Some((url, title))
            }
            _ => None,
        }
    })
    .await;
    if let Some(popup) = window.extension_popup() {
        popup.hide();
    }
    detail.push(format!(
        "Alt+Shift+P in the page opened the popup {url}, titled {title:?}"
    ));

    eval(
        tab,
        "delete document.documentElement.dataset.vsesvitProbeCommand; 1",
    )
    .await?;
    p.observe("Alt+Shift+K in the page");
    press(tab, 0x4B, ALT | SHIFT).await?;
    let deadline = Instant::now() + ON_COMMAND_WAIT;
    let fired = loop {
        let seen = eval(
            tab,
            "document.documentElement.dataset.vsesvitProbeCommand || null",
        )
        .await
        .unwrap_or_default();
        let fired = serde_json::from_str::<Option<String>>(&seen).ok().flatten();
        if fired.is_some() || Instant::now() >= deadline {
            break fired;
        }
        exec::sleep(POLL).await;
    };
    detail.push(match fired {
        Some(command) => {
            format!("Alt+Shift+K in the page: WebView2 fired commands.onCommand with {command}")
        }
        None => format!(
            "Alt+Shift+K in the page: WebView2 fired no commands.onCommand within {} s",
            ON_COMMAND_WAIT.as_secs()
        ),
    });

    let probe_action = bindings
        .extensions()
        .iter()
        .find(|(c, _)| c.extension.as_str() == testkit::PROBE_ID && c.command.activates_action())
        .map(|(c, _)| extension_shortcut_row_name(c))
        .ok_or("the bindings hold no action command of the probe")?;
    let (preview, _page) = shortcuts_page(window).await?;
    let listed = preview.find::<FrameworkElement>(&probe_action).is_ok();
    drop(preview);
    detail.push(format!("Settings lists {probe_action}: {listed}"));
    let detail = detail.join("; ");
    listed.then_some(detail.clone()).ok_or(detail)
}
