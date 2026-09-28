//! The extension toolbar with two actions: both pinned in install order, the Extensions menu,
//! a drag that reorders them, Unpin from a button's context menu, the popup of an unpinned
//! action under the Extensions button, and pinning it again from the menu. Order and pins are
//! read back from core's synced preference.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::extensions::InstallSource;
use vsesvit_core::extensions::toolbar::{Entry, TOOLBAR};
use vsesvit_core::testkit::{self, CrxKey};
use windows_core::{IInspectable, Interface};

use super::dialog_steps::invoke;
use super::shoot;
use crate::bindings::*;
use crate::browser::Browser;
use crate::exec;
use crate::popup::Activation;
use crate::window::BrowserWindow;

const WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(100);

const SECOND_FILES: &[(&str, &[u8])] = &[
    (
        "manifest.json",
        br#"{"manifest_version": 3, "name": "Second probe", "version": "1.0",
             "action": {"default_title": "Second probe", "default_popup": "popup.html"}}"#,
    ),
    (
        "popup.html",
        b"<!doctype html><title>second</title><body style='width:200px;font:14px sans-serif'>Second probe</body>",
    ),
];

async fn until<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
    exec::wait_for(WAIT, POLL, &mut f).await
}

fn saved(browser: &Browser) -> Vec<Entry> {
    browser.core(|p| p.prefs().get(&TOOLBAR))
}

pub(super) async fn run(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let crx = out_dir.join("second.crx");
    std::fs::write(&crx, testkit::write_crx3(SECOND_FILES, &CrxKey::second()))
        .map_err(|e| e.to_string())?;
    let source = InstallSource::from_path(&crx).map_err(|e| e.to_string())?;
    let second = browser.install_extension(source, &|_| {}).await?;
    let probe = browser
        .extension_actions()
        .into_iter()
        .find(|a| a.extension_id == testkit::PROBE_ID)
        .map(|a| a.id)
        .ok_or("the probe has no action")?;
    let other = second.id.as_str().to_owned();
    let toolbar = window.extension_toolbar();
    let both = until(|| (toolbar.pinned().len() == 2).then_some(())).await;
    settle().await;
    shoot(window, out_dir, "18b-extension-toolbar", steps, |_| {
        json!({
            "pinned": toolbar.pinned(),
            "ok": both.is_some() && toolbar.pinned() == [probe.clone(), other.clone()],
        })
    })
    .await;

    let menu = toolbar.show_menu().map_err(|e| e.to_string())?;
    settle().await;
    let content: FrameworkElement = menu
        .Content()
        .and_then(|c| c.cast())
        .map_err(|e| e.to_string())?;
    let glyph = |i: usize| -> Option<String> {
        let icon: FontIcon = content
            .FindName(&format!("PinGlyph{i}"))
            .ok()?
            .cast()
            .ok()?;
        Some(icon.Glyph().ok()?.to_string())
    };
    let checked: Vec<bool> = (0..2)
        .map(|i| glyph(i).as_deref() == Some("\u{E842}"))
        .collect();
    shoot(
        window,
        out_dir,
        "18c-extensions-menu",
        steps,
        |_| json!({ "pins": checked, "ok": checked == [true, true] }),
    )
    .await;
    let _ = menu.cast::<FlyoutBase>().and_then(|m| m.Hide());

    // The drag itself is the list's own; what it leaves behind is a new item order.
    let entries = toolbar
        .list()
        .cast::<ItemsControl>()
        .and_then(|c| c.Items())
        .and_then(|i| i.cast::<windows_collections::IVector<IInspectable>>())
        .map_err(|e| e.to_string())?;
    let last = entries.GetAt(1).map_err(|e| e.to_string())?;
    entries.RemoveAt(1).map_err(|e| e.to_string())?;
    entries.InsertAt(0, &last).map_err(|e| e.to_string())?;
    toolbar.dropped();
    let reordered =
        until(|| (toolbar.pinned() == [other.clone(), probe.clone()]).then_some(())).await;
    let order: Vec<String> = saved(browser).into_iter().map(|e| e.id).collect();
    steps.push(json!({
        "name": "18d-extension-drag-reorders",
        "saved": order,
        "ok": reordered.is_some() && order.first() == Some(&other),
    }));

    let menu = toolbar
        .show_button_menu(&other)
        .map_err(|e| e.to_string())?;
    settle().await;
    let labels: Vec<String> = menu
        .Items()
        .map(|items| {
            (&items)
                .into_iter()
                .filter_map(|i| i.cast::<MenuFlyoutItem>().ok()?.Text().ok())
                .map(|t| t.to_string())
                .collect()
        })
        .unwrap_or_default();
    shoot(
        window,
        out_dir,
        "18e-extension-button-menu",
        steps,
        |_| json!({ "entries": labels, "ok": labels == ["Unpin", "Manage extensions"] }),
    )
    .await;
    let unpin = menu
        .Items()
        .and_then(|items| items.GetAt(0))
        .map_err(|e| e.to_string())?;
    invoke(&unpin).map_err(|e| e.to_string())?;
    let unpinned = until(|| (toolbar.pinned() == [probe.clone()]).then_some(())).await;
    let entry = saved(browser).into_iter().find(|e| e.id == other);
    steps.push(json!({
        "name": "18f-extension-unpinned",
        "entry": format!("{entry:?}"),
        "ok": unpinned.is_some() && entry.is_some_and(|e| !e.pinned),
    }));

    let popup = window
        .open_extension_popup(
            second.engine_id.as_deref().unwrap_or_default(),
            Activation::Keep,
        )
        .map_err(|e| e.to_string())?;
    let title = until(|| popup.title().filter(|t| t == "second")).await;
    settle().await;
    shoot(
        window,
        out_dir,
        "18g-unpinned-popup-under-the-menu",
        steps,
        |_| json!({ "title": title, "ok": title.is_some() }),
    )
    .await;
    popup.hide();

    let menu = toolbar.show_menu().map_err(|e| e.to_string())?;
    settle().await;
    let content: FrameworkElement = menu
        .Content()
        .and_then(|c| c.cast())
        .map_err(|e| e.to_string())?;
    let pin: Button = content
        .FindName("Pin1")
        .and_then(|p| p.cast())
        .map_err(|e| e.to_string())?;
    invoke(&pin).map_err(|e| e.to_string())?;
    let _ = menu.cast::<FlyoutBase>().and_then(|m| m.Hide());
    let pinned_again = until(|| (toolbar.pinned().len() == 2).then_some(())).await;
    steps.push(json!({
        "name": "18h-extension-pinned-again",
        "pinned": toolbar.pinned(),
        "ok": pinned_again.is_some() && toolbar.pinned() == [other.clone(), probe.clone()],
    }));

    browser.uninstall_extension(&second.id).await?;
    Ok(())
}

async fn settle() {
    exec::sleep(Duration::from_millis(500)).await;
}
