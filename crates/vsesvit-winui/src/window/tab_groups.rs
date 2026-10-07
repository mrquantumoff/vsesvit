//! The window's tab groups (`vsesvit_core::tab_groups`): the model is settled after anything
//! changes the window's tabs, then the live tab list is redrawn from its rows; the menu and
//! header actions change the model, apply its steps with settling held off, and settle once.

use vsesvit_core::prefs::Theme;
use vsesvit_core::tab_groups::{GroupId, Step, TabGroup, TabGroups, WindowTabs};

use super::{BrowserWindow, Placement};
use crate::group_header::GroupEvent;
use crate::platform;
use crate::tab::{Initial, TabId};

impl BrowserWindow {
    /// The window's tabs as the groups see them.
    fn window_tabs(&self) -> WindowTabs<TabId> {
        let strip = self.strip();
        WindowTabs {
            order: strip.order(),
            pinned: self.pinned_count(None),
            active: strip.selected(),
        }
    }

    /// Makes the groups follow the tabs after a change, `moved` being the tab that just moved
    /// or opened if known, and redraws them. Nothing happens while a group action is applying
    /// its steps, or while the redraw itself runs.
    pub(super) fn settle_groups(&self, moved: Option<TabId>) {
        if self.groups_busy.replace(true) {
            return;
        }
        let tabs = self.window_tabs();
        self.groups.borrow_mut().settle(&tabs, moved.as_ref());
        *self.order_seen.borrow_mut() = tabs.order.clone();
        self.show_groups(&tabs);
        self.groups_busy.set(false);
    }

    fn show_groups(&self, tabs: &WindowTabs<TabId>) {
        let rows = self.groups.borrow().rows(tabs);
        // Headers moving among the items can move the list's selection for a moment.
        self.reordering.set(true);
        let shown = self.strip().show_groups(&rows, self.dark());
        self.reordering.set(false);
        if let Err(e) = shown {
            log::warn!("tab groups: {e}");
        }
    }

    /// Whether the window shows the dark theme, for the groups' shades.
    pub(super) fn dark(&self) -> bool {
        let theme = self.browser().map_or(Theme::System, |b| b.theme());
        match self.theme_for(theme) {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => platform::apps_dark(),
        }
    }

    /// Applies a group action's steps, then settles once.
    fn apply_group_steps(&self, steps: Vec<Step<TabId>>) {
        self.groups_busy.set(true);
        for step in steps {
            match step {
                Step::Unpin(id) => {
                    if let Some(tab) = self.tab(id) {
                        self.set_pinned(&tab, false);
                    }
                }
                Step::Move(id, to) => {
                    if let Err(e) = self.move_tab(id, to) {
                        log::warn!("tab group: moving tab {id}: {e}");
                    }
                }
                Step::Activate(id) => self.select_tab(id),
                Step::OpenTab => {
                    if let Err(e) = self.open_blank_tab() {
                        log::error!("tab group: new tab: {e}");
                    }
                }
            }
        }
        self.groups_busy.set(false);
        self.settle_groups(None);
        if let Some(browser) = self.browser() {
            browser.session_changed();
        }
    }

    /// "Add tab to new group": the tab alone in a new group, whose editor opens at once to name
    /// it, as in Chrome.
    pub(super) fn new_group(&self, id: TabId) {
        let tabs = self.window_tabs();
        let (group, steps) = self.groups.borrow_mut().new_group(&tabs, &id);
        self.apply_group_steps(steps);
        if let Err(e) = self.strip().edit_group(group, self.is_foreground()) {
            log::warn!("group editor: {e}");
        }
    }

    pub(super) fn join_group(&self, id: TabId, group: GroupId) {
        let tabs = self.window_tabs();
        let steps = self.groups.borrow_mut().join(&tabs, &id, group);
        self.apply_group_steps(steps);
    }

    pub(super) fn leave_group(&self, id: TabId) {
        let tabs = self.window_tabs();
        let steps = self.groups.borrow_mut().leave(&tabs, &id);
        self.apply_group_steps(steps);
    }

    /// Collapses or expands `group`, as a click on its header does.
    pub fn set_group_collapsed(&self, group: GroupId, collapsed: bool) {
        let tabs = self.window_tabs();
        let steps = self
            .groups
            .borrow_mut()
            .set_collapsed(&tabs, group, collapsed);
        self.apply_group_steps(steps);
    }

    pub(super) fn group_action(&self, (group, event): (GroupId, GroupEvent)) {
        self.group_event(group, event);
    }

    /// A group's header or its editor was used.
    pub(crate) fn group_event(&self, group: GroupId, event: GroupEvent) {
        match event {
            GroupEvent::ToggleCollapsed => {
                let collapsed = self.groups.borrow().get(group).is_some_and(|g| g.collapsed);
                self.set_group_collapsed(group, !collapsed);
            }
            GroupEvent::Rename(title) => {
                self.groups.borrow_mut().set_title(group, &title);
                self.apply_group_steps(Vec::new());
            }
            GroupEvent::Recolor(color) => {
                self.groups.borrow_mut().set_color(group, color);
                self.apply_group_steps(Vec::new());
            }
            GroupEvent::NewTab => {
                let tabs = self.window_tabs();
                let last = self.groups.borrow().members(&tabs, group).last().copied();
                let Some(last) = last else { return };
                match self.open_tab(Initial::Blank, Placement::After(last), true, None) {
                    Ok(_) => self.focus_address(),
                    Err(e) => log::error!("new tab in group: {e}"),
                }
            }
            GroupEvent::Ungroup => {
                self.groups.borrow_mut().ungroup(group);
                self.apply_group_steps(Vec::new());
            }
            GroupEvent::Close => {
                let tabs = self.window_tabs();
                let members = self.groups.borrow().members(&tabs, group);
                for member in members {
                    self.close_tab(member);
                }
            }
        }
    }

    /// The tabs of this window were dragged into a new order in a tab list.
    pub(super) fn tabs_reordered(&self) {
        let before = self.order_seen.borrow().clone();
        self.groups_busy.set(true);
        self.keep_pinned_first();
        self.groups_busy.set(false);
        let moved = moved_tab(&before, &self.strip().order());
        self.settle_groups(moved);
        if let Some(browser) = self.browser() {
            browser.session_changed();
        }
    }

    /// The groups of restored tabs, from their session records.
    pub(super) fn restore_groups(&self, tabs: Vec<(TabId, Option<TabGroup>)>) {
        *self.groups.borrow_mut() = TabGroups::restore(tabs);
        self.settle_groups(None);
    }

    /// The tab's group, for its session record and its menu.
    pub fn group_of(&self, id: TabId) -> Option<TabGroup> {
        self.groups.borrow().group_of(&id).cloned()
    }

    /// The window's groups in the order they show.
    pub(super) fn groups_in_order(&self) -> Vec<TabGroup> {
        self.groups.borrow().in_order(&self.window_tabs())
    }

    /// The live tab list as it shows its headers and tabs (`TabStrip::group_lines`).
    pub fn group_lines(&self) -> Vec<String> {
        self.strip().group_lines()
    }
}

/// The tab that moved between two orders of the same tabs: the one whose removal makes them
/// agree. Neighbours that swapped both explain the change; the one that went furthest, then the
/// later one, counts as moved.
pub(super) fn moved_tab(before: &[TabId], after: &[TabId]) -> Option<TabId> {
    if before == after || before.len() != after.len() {
        return None;
    }
    let without = |order: &[TabId], tab: TabId| -> Vec<TabId> {
        order.iter().copied().filter(|t| *t != tab).collect()
    };
    let mut candidates: Vec<(usize, usize, TabId)> = after
        .iter()
        .enumerate()
        .filter(|(_, tab)| without(before, **tab) == without(after, **tab))
        .map(|(to, tab)| {
            let from = before.iter().position(|t| t == tab).unwrap_or(to);
            (to.abs_diff(from), to, *tab)
        })
        .collect();
    candidates.sort_unstable();
    candidates.last().map(|&(_, _, tab)| tab)
}

#[cfg(test)]
mod tests {
    use super::moved_tab;

    #[test]
    fn the_moved_tab_is_the_one_whose_removal_makes_the_orders_agree() {
        assert_eq!(moved_tab(&[1, 2, 3, 4], &[1, 3, 4, 2]), Some(2));
        assert_eq!(moved_tab(&[1, 2, 3, 4], &[4, 1, 2, 3]), Some(4));
        assert_eq!(moved_tab(&[1, 2, 3, 4], &[1, 2, 3, 4]), None);
        assert_eq!(moved_tab(&[1, 2, 3], &[1, 2, 3, 4]), None);
    }

    #[test]
    fn swapped_neighbours_read_as_the_later_one_having_moved() {
        assert_eq!(moved_tab(&[1, 2, 3, 4], &[1, 3, 2, 4]), Some(2));
    }
}
