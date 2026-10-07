//! The `tab_menu` check: the tab context menu's items, and what each of them does, without OS
//! input and without touching the user's clipboard; and the page menu's Copy link without
//! tracking and Open link in private window on a link.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::json;
use vsesvit_core::private::Browsing;
use vsesvit_core::testkit::FixtureServer;
use windows_core::Interface;

use super::{FIXTURE_TITLE, PAGE2_TITLE, Probe, eval, tab_ids, until};
use crate::bindings::ICoreWebView2_11;
use crate::browser::Browser;
use crate::omnibox::has_link;
use crate::shortcuts::Command;
use crate::tab::{CLEAN_LINK_ITEM, PRIVATE_LINK_ITEM, Tab};
use crate::tab_header::Audio;
use crate::window::{BrowserWindow, TabAction};

/// Adds a link to the page's top and says where its middle is, in CSS pixels.
const ADD_LINK: &str = "(() => { const a = document.createElement('a'); a.id = 'vsesvit-link'; \
    a.href = '/page2.html?utm_source=self-test&a=1'; a.textContent = 'link'; \
    a.style.cssText = 'display:block;font-size:48px'; document.body.prepend(a); \
    const r = a.getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn lines(lines: &[(&str, bool)]) -> Vec<(String, bool)> {
    lines
        .iter()
        .map(|&(label, on)| (label.to_owned(), on))
        .collect()
}

/// Waits until `tab` settles at `url` titled `title`.
pub(super) async fn loaded(tab: &Tab, url: &str, title: &str, p: &Probe) {
    until(p, |p| {
        let s = tab.state();
        p.observe(format!(
            "tab {} at {} titled {:?}, loading={}; want {url}",
            tab.id,
            s.url,
            s.title,
            s.loading()
        ));
        (s.url == url && s.title == title && !s.loading()).then_some(())
    })
    .await;
}

pub(super) async fn tab_menu(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    first: &Rc<Tab>,
    server: &FixtureServer,
    p: &Probe,
) -> Result<String, String> {
    let mut detail = Vec::new();
    let index = server.url("/index.html");
    let tracked = server.url("/page2.html?utm_source=self-test&a=1");
    let second = window.open_url_tab(tracked.as_str(), false).map_err(err)?;
    loaded(&second, tracked.as_str(), PAGE2_TITLE, p).await;

    let seen = window.tab_menu_lines(first.id);
    let split = format!("Split view with > New tab, {}", second.state().title);
    let want = lines(&[
        ("New tab below", true),
        ("Add tab to new group", true),
        (split.as_str(), true),
        ("Move tab to new window", true),
        ("Reload", true),
        ("Duplicate", true),
        ("Pin tab", true),
        ("Mute tab", true),
        ("Copy link", true),
        ("Close tab", true),
        ("Close other tabs", true),
        ("Close tabs below", true),
        ("Reopen closed tab", browser.can_reopen_closed_tab(Browsing::Normal)),
    ]);
    detail.push(format!("the first tab's menu: {seen:?}"));
    if seen != want {
        return Err(detail.join("; "));
    }

    let menu = link_menu(first, p).await?;
    let copy = menu.iter().position(|(name, _)| name == "copyLinkLocation");
    let ours = menu.iter().position(|(_, label)| label == CLEAN_LINK_ITEM);
    let new_window = menu.iter().position(|(name, _)| name == "openLinkInNewWindow");
    let private = menu.iter().position(|(_, label)| label == PRIVATE_LINK_ITEM);
    detail.push(format!("a link's menu (name, label): {menu:?}"));
    if copy.is_none() || ours != copy.map(|i| i + 1) {
        return Err(detail.join("; "));
    }
    if new_window.is_none() || private != new_window.map(|i| i + 1) {
        return Err(detail.join("; "));
    }

    let open = tab_ids(window);
    window.tab_action(first.id, TabAction::Duplicate);
    let copy = until(p, |p| {
        p.observe(format!("Duplicate: tabs {:?}", tab_ids(window)));
        window
            .tabs_in_order()
            .into_iter()
            .find(|t| !open.contains(&t.id))
    })
    .await;
    loaded(&copy, index.as_str(), FIXTURE_TITLE, p).await;
    let order = tab_ids(window);
    let selected = window.active_tab().is_some_and(|t| t.id == copy.id);
    detail.push(format!(
        "Duplicate: tabs {order:?} (first {}, copy {}, second {}), the copy selected {selected}",
        first.id, copy.id, second.id
    ));
    if order != [first.id, copy.id, second.id] || !selected {
        return Err(detail.join("; "));
    }

    window.tab_action(second.id, TabAction::Pin(true));
    let order = tab_ids(window);
    let saved = browser.save_session_now();
    let restored = browser
        .core(|c| c.session().restore())
        .map_err(err)?
        .ok_or("the session restored nothing")?;
    let saved_pinned = restored
        .windows
        .iter()
        .flat_map(|w| &w.tabs)
        .any(|t| t.id == second.session_id && t.pinned);
    let unpin = window
        .tab_menu_lines(second.id)
        .contains(&("Unpin tab".to_owned(), true));
    detail.push(format!(
        "Pin tab: tabs {order:?}, pinned {}, saved {saved} with it pinned {saved_pinned}, its menu offers Unpin tab {unpin}",
        second.is_pinned()
    ));
    if order.first() != Some(&second.id) || !second.is_pinned() || !saved_pinned || !unpin {
        return Err(detail.join("; "));
    }

    window.tab_action(second.id, TabAction::Mute(true));
    until(p, |p| {
        let (muted, audio) = (second.state().muted, second.look().audio);
        p.observe(format!("Mute tab: muted {muted}, speaker {audio:?}"));
        (muted && audio == Audio::Muted).then_some(())
    })
    .await;
    let unmute = window
        .tab_menu_lines(second.id)
        .contains(&("Unmute tab".to_owned(), true));
    window.tab_action(second.id, TabAction::Mute(false));
    until(p, |p| {
        p.observe("Unmute tab: still muted");
        (!second.state().muted).then_some(())
    })
    .await;
    detail.push(format!(
        "Mute tab muted it, its menu offers Unmute tab {unmute}, which unmuted it"
    ));
    if !unmute {
        return Err(detail.join("; "));
    }

    window.tab_action(first.id, TabAction::CloseOthers);
    let order = tab_ids(window);
    detail.push(format!("Close other tabs: tabs {order:?}"));
    if order != [second.id, first.id] {
        return Err(detail.join("; "));
    }

    window.tab_action(first.id, TabAction::ReopenClosed);
    let open = [second.id, first.id];
    let reopened = until(p, |p| {
        p.observe(format!("Reopen closed tab: tabs {:?}", tab_ids(window)));
        window
            .tabs_in_order()
            .into_iter()
            .find(|t| !open.contains(&t.id))
    })
    .await;
    loaded(&reopened, index.as_str(), FIXTURE_TITLE, p).await;
    let order = tab_ids(window);
    detail.push(format!("Reopen closed tab: tabs {order:?}"));
    if order != [second.id, first.id, reopened.id] {
        return Err(detail.join("; "));
    }

    window.tab_action(first.id, TabAction::CloseAfter);
    let order = tab_ids(window);
    detail.push(format!("Close tabs below: tabs {order:?}"));
    if order != [second.id, first.id] {
        return Err(detail.join("; "));
    }

    window.tab_action(first.id, TabAction::NewTabNext);
    let tabs = window.tabs_in_order();
    let new = tabs.get(2).filter(|t| !open.contains(&t.id)).cloned();
    let selected = new
        .as_ref()
        .is_some_and(|n| window.active_tab().is_some_and(|t| t.id == n.id));
    let blank = new.as_ref().is_some_and(|n| !has_link(&n.state().url));
    detail.push(format!(
        "New tab below: tabs {:?}, the third selected {selected} and blank {blank}",
        tab_ids(window)
    ));
    if let Some(new) = &new {
        window.close_tab(new.id);
    }
    if tabs.len() != 3 || !selected || !blank {
        return Err(detail.join("; "));
    }

    let before = browser.windows();
    window.tab_action(second.id, TabAction::MoveToNewWindow);
    let target = browser
        .windows()
        .into_iter()
        .find(|w| !before.iter().any(|b| Rc::ptr_eq(b, w)))
        .ok_or_else(|| {
            format!(
                "{}; Move tab to new window opened no window",
                detail.join("; ")
            )
        })?;
    let moved = until(p, |p| {
        let tabs = target.tabs_in_order();
        p.observe(format!(
            "Move tab to new window: the new window holds {:?}",
            tabs.iter().map(|t| t.state().url).collect::<Vec<_>>()
        ));
        let [moved] = tabs.as_slice() else {
            return None;
        };
        let s = moved.state();
        (s.url == tracked.as_str() && !s.loading()).then(|| moved.clone())
    })
    .await;
    let left = tab_ids(window);
    let same = moved.session_id == second.session_id;
    detail.push(format!(
        "Move tab to new window: this window holds {left:?}, the new one {} at {}, pinned {}, the same session tab {same}",
        moved.id,
        moved.state().url,
        moved.is_pinned()
    ));
    target.close_tab(moved.id);
    until(p, |p| {
        p.observe(format!(
            "{} windows after closing the moved tab, {} before",
            browser.windows().len(),
            before.len()
        ));
        (browser.windows().len() == before.len()).then_some(())
    })
    .await;
    if left != [first.id] || !moved.is_pinned() || !same {
        return Err(detail.join("; "));
    }
    Ok(detail.join("; "))
}

/// Right-clicks a link added to `tab`'s page and lists the menu that would open, by item name
/// and label. The check's handler chooses nothing and keeps the menu from showing.
async fn link_menu(tab: &Rc<Tab>, p: &Probe) -> Result<Vec<(String, String)>, String> {
    let spot: serde_json::Value = serde_json::from_str(&eval(tab, ADD_LINK).await?).map_err(err)?;
    let (Some(x), Some(y)) = (spot[0].as_f64(), spot[1].as_f64()) else {
        return Err(format!("adding a link gave {spot}"));
    };
    let menu = Rc::new(RefCell::new(None::<Vec<(String, String)>>));
    let seen = menu.clone();
    let _watch = tab
        .core()
        .ok_or("no engine view")?
        .cast::<ICoreWebView2_11>()
        .and_then(|core| {
            core.ContextMenuRequested(move |_, args| {
                let Some(args) = args.as_ref() else { return };
                let listed = args
                    .MenuItems()
                    .map(|items| {
                        items
                            .into_iter()
                            .map(|i| (i.Name().unwrap_or_default(), i.Label().unwrap_or_default()))
                            .collect()
                    })
                    .unwrap_or_default();
                let _ = args.SetHandled(true);
                *seen.borrow_mut() = Some(listed);
            })
        })
        .map_err(err)?;
    for kind in ["mousePressed", "mouseReleased"] {
        let params = json!({
            "type": kind, "x": x, "y": y, "button": "right", "buttons": 2, "clickCount": 1,
        });
        tab.devtools("Input.dispatchMouseEvent", &params.to_string())
            .await
            .map_err(|e| format!("right click: {e}"))?;
    }
    let listed = until(p, |p| {
        p.observe(format!(
            "right-clicked the link at {spot}; no context menu yet"
        ));
        menu.borrow_mut().take()
    })
    .await;
    eval(tab, "document.getElementById('vsesvit-link').remove(), 0").await?;
    Ok(listed)
}

/// Leaves `window` with only `first`, selected, and closes the windows that were not in
/// `windows`, however the check ended.
pub(super) fn tidy(
    browser: &Browser,
    windows: &[Rc<BrowserWindow>],
    window: &BrowserWindow,
    first: &Tab,
) {
    for other in browser.windows() {
        if !windows.iter().any(|w| Rc::ptr_eq(w, &other)) {
            for tab in other.tabs_in_order() {
                other.close_tab(tab.id);
            }
        }
    }
    for id in tab_ids(window).into_iter().filter(|id| *id != first.id) {
        window.close_tab(id);
    }
    window.run(Command::SelectTab(0));
}
