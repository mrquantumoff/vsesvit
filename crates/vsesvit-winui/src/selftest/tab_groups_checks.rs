//! The `tab_groups` check: a group made from the tab menu shows its header before the tab in
//! both tab lists, collapsing hides the tab and selects a shown one, the group's name and
//! colour follow the editor, the saved session carries the tab's group, and Ungroup takes it
//! away again; Ctrl+Tab, Ctrl+Shift+Tab and closing the selected tab pass over a collapsed
//! group's tab, and a grouped tab closed and reopened goes back into its group. Nothing here
//! sends OS input.

use std::rc::Rc;

use vsesvit_core::prefs::TabsPosition;
use vsesvit_core::session::SessionSnapshot;
use vsesvit_core::tab_groups::{GroupColor, TabGroup};
use vsesvit_core::testkit::FixtureServer;

use super::tab_menu_checks::loaded;
use super::{PAGE2_TITLE, Probe, expect_layout};
use crate::browser::Browser;
use crate::group_header::GroupEvent;
use crate::shortcuts::Command;
use crate::tab::Tab;
use crate::window::{BrowserWindow, TabAction};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// The group the saved session gives `tab`, after saving it now.
fn saved_group(browser: &Browser, tab: &Tab) -> Result<Option<TabGroup>, String> {
    if !browser.save_session_now() {
        return Err("the session did not save".to_owned());
    }
    let restored: SessionSnapshot = browser
        .core(|c| c.session().restore())
        .map_err(err)?
        .ok_or("the session restored nothing")?;
    let saved = restored
        .windows
        .iter()
        .flat_map(|w| &w.tabs)
        .find(|t| t.id == tab.session_id)
        .ok_or("the saved session has no record of the tab")?;
    Ok(saved.group.clone())
}

pub(super) async fn tab_groups(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    first: &Rc<Tab>,
    server: &FixtureServer,
    p: &Probe,
) -> Result<String, String> {
    let mut detail = Vec::new();
    let page2 = server.url("/page2.html");
    let second = window.open_url_tab(page2.as_str(), false).map_err(err)?;
    loaded(&second, page2.as_str(), PAGE2_TITLE, p).await;
    let tab = |t: &Tab| t.id.to_string();

    window.tab_action(first.id, TabAction::NewGroup);
    let group = window
        .group_of(first.id)
        .ok_or("Add tab to new group put the tab in no group")?;
    let header = format!("[{}]", group.name());
    let lines = window.group_lines();
    let menu_first = window.tab_menu_lines(first.id);
    let menu_second = window.tab_menu_lines(second.id);
    let join = format!("Add tab to group > New group, {}", group.name());
    detail.push(format!(
        "Add tab to new group: the pane shows {lines:?}; the grouped tab's menu offers Remove from group {}, the other's {join:?} {}",
        menu_first.contains(&("Remove from group".to_owned(), true)),
        menu_second.contains(&(join.clone(), true))
    ));
    if lines != [header.clone(), tab(first), tab(&second)]
        || !menu_first.contains(&("Remove from group".to_owned(), true))
        || !menu_second.contains(&(join, true))
    {
        return Err(detail.join("; "));
    }

    browser.set_tabs_position(TabsPosition::Top);
    expect_layout(window, TabsPosition::Top, p).await;
    let on_top = window.group_lines();
    browser.set_tabs_position(TabsPosition::Left);
    expect_layout(window, TabsPosition::Left, p).await;
    detail.push(format!("the top strip shows {on_top:?}"));
    if on_top != [header.clone(), tab(first), tab(&second)] {
        return Err(detail.join("; "));
    }

    window.set_group_collapsed(group.id, true);
    let lines = window.group_lines();
    let active = window.active_tab().map(|t| t.id);
    detail.push(format!(
        "collapsed: the pane shows {lines:?}, the selected tab is {active:?} (second {})",
        second.id
    ));
    let hidden = format!("{} hidden", first.id);
    if lines != [header.clone(), hidden, tab(&second)] || active != Some(second.id) {
        return Err(detail.join("; "));
    }
    window.set_group_collapsed(group.id, false);
    let lines = window.group_lines();
    detail.push(format!("expanded: the pane shows {lines:?}"));
    if lines != [header, tab(first), tab(&second)] {
        return Err(detail.join("; "));
    }

    window.group_event(group.id, GroupEvent::Rename("Work".to_owned()));
    window.group_event(group.id, GroupEvent::Recolor(GroupColor::Green));
    let lines = window.group_lines();
    let saved = saved_group(browser, first)?;
    let named = saved.as_ref().is_some_and(|g| {
        g.id == group.id && g.title == "Work" && g.color == GroupColor::Green && !g.collapsed
    });
    detail.push(format!(
        "named Work and made green: the pane shows {lines:?}; the saved session gives the tab {saved:?}"
    ));
    if lines.first().map(String::as_str) != Some("[Work]") || !named {
        return Err(detail.join("; "));
    }

    window.group_event(group.id, GroupEvent::Ungroup);
    let lines = window.group_lines();
    let saved = saved_group(browser, first)?;
    detail.push(format!(
        "Ungroup: the pane shows {lines:?}; the saved session gives the tab {saved:?}"
    ));
    let in_group = window.group_of(first.id).is_some();
    if lines != [tab(first), tab(&second)] || saved.is_some() || in_group {
        return Err(detail.join("; "));
    }

    let third = window.open_url_tab(page2.as_str(), false).map_err(err)?;
    loaded(&third, page2.as_str(), PAGE2_TITLE, p).await;
    window.tab_action(second.id, TabAction::NewGroup);
    let middle = window
        .group_of(second.id)
        .ok_or("Add tab to new group put the second tab in no group")?;
    window.select_tab(first.id);
    window.set_group_collapsed(middle.id, true);
    window.run(Command::NextTab);
    let forward = window.active_tab().map(|t| t.id);
    window.run(Command::PreviousTab);
    let back = window.active_tab().map(|t| t.id);
    window.select_tab(third.id);
    window.close_tab(third.id);
    let after_close = window.active_tab().map(|t| t.id);
    let collapsed = window.group_of(second.id).is_some_and(|g| g.collapsed);
    detail.push(format!(
        "with the second tab grouped and collapsed between the first and a third ({}): Ctrl+Tab from the first selected {forward:?}, Ctrl+Shift+Tab then {back:?}; closing the selected third selected {after_close:?}, the group still collapsed: {collapsed}",
        third.id
    ));
    if forward != Some(third.id) || back != Some(first.id) || after_close != Some(first.id) || !collapsed {
        return Err(detail.join("; "));
    }

    window.set_group_collapsed(middle.id, false);
    window.run(Command::ReopenClosedTab);
    let reopened = window
        .active_tab()
        .filter(|t| t.id != first.id && t.id != second.id)
        .ok_or("Reopen closed tab selected no new tab")?;
    let outside = window.group_of(reopened.id).is_none();
    loaded(&reopened, page2.as_str(), PAGE2_TITLE, p).await;
    window.tab_action(reopened.id, TabAction::JoinGroup(middle.id));
    window.close_tab(reopened.id);
    window.run(Command::ReopenClosedTab);
    let back_in = window
        .active_tab()
        .filter(|t| t.id != first.id && t.id != second.id)
        .ok_or("Reopen closed tab selected no new tab")?;
    let lines = window.group_lines();
    let regrouped = window.group_of(back_in.id).map(|g| g.id) == Some(middle.id);
    detail.push(format!(
        "the third tab reopened in no group: {outside}; joined to the group, closed and reopened ({}), it is in the group again: {regrouped}, the pane showing {lines:?}",
        back_in.id
    ));
    if !outside || !regrouped || lines != [tab(first), format!("[{}]", middle.name()), tab(&second), tab(&back_in)] {
        return Err(detail.join("; "));
    }
    Ok(detail.join("; "))
}
