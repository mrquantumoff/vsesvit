//! Owned by W1.5.
//! One window's tabs: their order, which lead pinned, which is selected, which opened which, and
//! the window's groups (UA section 5.2). The group rules are core's `TabGroups`; this model applies
//! the steps they return with settling held until the last one, and settles after every other change.

use std::collections::HashMap;

use gpui::{Context, EventEmitter};
use vsesvit_core::tab_groups::{Row, Step, TabGroup, TabGroups, WindowTabs};
use vsesvit_core::tab_place::TabPlace;
use vsesvit_engine::TabId;

/// Where a new tab goes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Insert {
    /// After every tab.
    End,
    /// After its opener and the tabs the opener opened before it, in the opener's group (Chrome's
    /// and AdwTabView's rule).
    AfterOpener(TabId),
    /// At this index among the unpinned tabs' places, clamped there.
    Index(usize),
    /// Pinned, at this index among the pinned tabs, clamped there.
    Pinned(usize),
}

/// What a removed tab left behind, for the closed-tab stack and the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Removed {
    pub index: usize,
    pub pinned: bool,
    pub group: Option<TabGroup>,
    /// The tab selected in its place, when it was the selected one.
    pub selected: Option<TabId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabListEvent {
    Inserted { tab: TabId, index: usize },
    Removed { tab: TabId },
    Moved { tab: TabId, to: usize },
    Selected { old: Option<TabId>, new: Option<TabId> },
    PinnedChanged { tab: TabId },
    GroupsChanged,
    /// A group step wants a new tab at the end, selected (collapsing the selected tab's group when
    /// no other tab shows). The window opens it.
    OpenTabRequested,
}

#[derive(Default)]
pub struct TabListModel {
    order: Vec<TabId>,
    pinned: usize,
    selected: Option<TabId>,
    groups: TabGroups<TabId>,
    /// Which tab opened which, while both are here: the next tab an opener opens goes after this
    /// one's run, and closing the selected tab may select its sibling.
    parents: HashMap<TabId, TabId>,
    settle_held: bool,
}

impl EventEmitter<TabListEvent> for TabListModel {}

impl TabListModel {
    pub fn tabs(&self) -> &[TabId] {
        &self.order
    }

    pub fn pinned_count(&self) -> usize {
        self.pinned
    }

    pub fn selected(&self) -> Option<TabId> {
        self.selected
    }

    pub fn index_of(&self, tab: TabId) -> Option<usize> {
        self.order.iter().position(|&t| t == tab)
    }

    pub fn is_pinned(&self, tab: TabId) -> bool {
        self.index_of(tab).is_some_and(|i| i < self.pinned)
    }

    /// The tab's place, for the tab menu. A tab not in this window reads as past the end.
    pub fn place(&self, tab: TabId) -> TabPlace {
        TabPlace { index: self.index_of(tab).unwrap_or(self.order.len()), count: self.order.len(), pinned: self.pinned }
    }

    pub fn window_tabs(&self) -> WindowTabs<TabId> {
        WindowTabs { order: self.order.clone(), pinned: self.pinned, active: self.selected }
    }

    pub fn insert(&mut self, tab: TabId, at: Insert, select: bool, cx: &mut Context<Self>) {
        if self.index_of(tab).is_some() {
            return;
        }
        let index = match at {
            Insert::End => self.order.len(),
            Insert::Index(i) => i.clamp(self.pinned, self.order.len()),
            Insert::Pinned(i) => i.min(self.pinned),
            Insert::AfterOpener(opener) => match self.index_of(opener) {
                Some(at) => {
                    let after_run = (at + 1..self.order.len())
                        .find(|&i| !self.descends_from(self.order[i], opener))
                        .unwrap_or(self.order.len());
                    after_run.max(self.pinned)
                }
                None => self.order.len(),
            },
        };
        self.order.insert(index, tab);
        if matches!(at, Insert::Pinned(_)) {
            self.pinned += 1;
        }
        if let Insert::AfterOpener(opener) = at
            && self.index_of(opener).is_some()
        {
            self.parents.insert(tab, opener);
            self.groups.adopt(tab, &opener);
        }
        cx.emit(TabListEvent::Inserted { tab, index });
        if select || self.selected.is_none() {
            self.set_selected(Some(tab), cx);
        }
        self.settle(Some(tab), cx);
    }

    /// Takes `tab` out, selecting the tab core's `after_closing` picks when it was the selected one.
    pub fn remove(&mut self, tab: TabId, cx: &mut Context<Self>) -> Option<Removed> {
        let index = self.index_of(tab)?;
        let selected = if self.selected == Some(tab) {
            let opener = self.sibling_pick(index);
            self.groups.after_closing(&self.window_tabs(), &tab, opener.as_ref())
        } else {
            self.selected
        };
        let removed = Removed {
            index,
            pinned: index < self.pinned,
            group: self.groups.group_of(&tab).cloned(),
            selected: (self.selected == Some(tab)).then_some(selected).flatten(),
        };
        self.order.remove(index);
        if removed.pinned {
            self.pinned -= 1;
        }
        match self.parents.remove(&tab) {
            Some(grand) => self.parents.values_mut().filter(|p| **p == tab).for_each(|p| *p = grand),
            None => self.parents.retain(|_, p| *p != tab),
        }
        cx.emit(TabListEvent::Removed { tab });
        if self.selected == Some(tab) {
            self.set_selected(selected, cx);
        }
        self.settle(None, cx);
        Some(removed)
    }

    pub fn select(&mut self, tab: TabId, cx: &mut Context<Self>) {
        if self.index_of(tab).is_some() && self.selected != Some(tab) {
            self.set_selected(Some(tab), cx);
            self.settle(None, cx);
        }
    }

    /// Moves `tab` to `index`, clamped to its own section: pinned tabs stay among the pinned ones.
    pub fn move_to(&mut self, tab: TabId, index: usize, cx: &mut Context<Self>) {
        let Some(from) = self.index_of(tab) else { return };
        let to = if from < self.pinned {
            index.min(self.pinned - 1)
        } else {
            index.clamp(self.pinned, self.order.len() - 1)
        };
        if from != to {
            let tab = self.order.remove(from);
            self.order.insert(to, tab);
            cx.emit(TabListEvent::Moved { tab, to });
        }
        self.settle(Some(tab), cx);
    }

    /// Pinning puts the tab last among the pinned tabs; unpinning, first among the others.
    pub fn set_pinned(&mut self, tab: TabId, pinned: bool, cx: &mut Context<Self>) {
        let Some(from) = self.index_of(tab) else { return };
        if (from < self.pinned) == pinned {
            return;
        }
        self.order.remove(from);
        if pinned {
            self.order.insert(self.pinned, tab);
            self.pinned += 1;
        } else {
            self.pinned -= 1;
            self.order.insert(self.pinned, tab);
        }
        cx.emit(TabListEvent::PinnedChanged { tab });
        self.settle(Some(tab), cx);
    }

    /// Ctrl+Tab's tab, or Ctrl+Shift+Tab's going `back`, past the tabs of collapsed groups.
    pub fn next_shown(&self, back: bool) -> Option<TabId> {
        self.groups.next_shown(&self.window_tabs(), back)
    }

    pub fn group_of(&self, tab: TabId) -> Option<&TabGroup> {
        self.groups.group_of(&tab)
    }

    pub fn rows(&self) -> Vec<Row<TabId>> {
        self.groups.rows(&self.window_tabs())
    }

    pub fn groups_in_order(&self) -> Vec<TabGroup> {
        self.groups.in_order(&self.window_tabs())
    }

    /// Runs a group action and applies the steps it returns, settling once after the last one, so a
    /// tab being moved into a group is not dropped from it halfway.
    pub fn change_groups<R>(
        &mut self,
        change: impl FnOnce(&mut TabGroups<TabId>, &WindowTabs<TabId>) -> (R, Vec<Step<TabId>>),
        cx: &mut Context<Self>,
    ) -> R {
        let tabs = self.window_tabs();
        let (result, steps) = change(&mut self.groups, &tabs);
        self.settle_held = true;
        for step in steps {
            match step {
                Step::Unpin(tab) => self.set_pinned(tab, false, cx),
                Step::Move(tab, to) => self.move_to(tab, to, cx),
                Step::Activate(tab) => self.select(tab, cx),
                Step::OpenTab => cx.emit(TabListEvent::OpenTabRequested),
            }
        }
        self.settle_held = false;
        self.settle(None, cx);
        result
    }

    /// A restored window's groups, from its tabs' records, once all of them are in.
    pub fn restore_groups(&mut self, tabs: Vec<(TabId, Option<TabGroup>)>, cx: &mut Context<Self>) {
        self.groups = TabGroups::restore(tabs);
        self.settle(None, cx);
    }

    fn set_selected(&mut self, new: Option<TabId>, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.selected, new);
        if old != new {
            cx.emit(TabListEvent::Selected { old, new });
        }
    }

    fn settle(&mut self, moved: Option<TabId>, cx: &mut Context<Self>) {
        if self.settle_held {
            return;
        }
        let tabs = self.window_tabs();
        let before = self.groups.rows(&tabs);
        self.groups.settle(&tabs, moved.as_ref());
        if self.groups.rows(&tabs) != before {
            cx.emit(TabListEvent::GroupsChanged);
        }
    }

    fn descends_from(&self, tab: TabId, ancestor: TabId) -> bool {
        let mut up = self.parents.get(&tab).copied();
        while let Some(parent) = up {
            if parent == ancestor {
                return true;
            }
            up = self.parents.get(&parent).copied();
        }
        false
    }

    /// AdwTabView's pick when the selected tab at `index` closes: the tab before it, when that is
    /// its opener or another tab its opener opened; or its pinned opener, from a pinned tab.
    fn sibling_pick(&self, index: usize) -> Option<TabId> {
        let parent = *self.parents.get(&self.order[index])?;
        let before = *self.order.get(index.checked_sub(1)?)?;
        if before == parent || self.descends_from(before, parent) {
            return Some(before);
        }
        let pinned = |t: TabId| self.index_of(t).is_some_and(|i| i < self.pinned);
        (pinned(before) && pinned(parent)).then_some(parent)
    }
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext, Entity, TestAppContext};
    use vsesvit_core::tab_groups::{GroupColor, GroupId};

    use super::*;

    fn model(cx: &mut TestAppContext) -> Entity<TabListModel> {
        cx.update(|cx| cx.new(|_| TabListModel::default()))
    }

    fn ids<const N: usize>() -> [TabId; N] {
        std::array::from_fn(|_| TabId::next())
    }

    fn insert(cx: &mut TestAppContext, m: &Entity<TabListModel>, tab: TabId, at: Insert, select: bool) {
        m.update(cx, |m, cx| m.insert(tab, at, select, cx));
    }

    fn order(cx: &mut TestAppContext, m: &Entity<TabListModel>) -> Vec<TabId> {
        m.read_with(cx, |m, _| m.tabs().to_vec())
    }

    #[gpui::test]
    fn a_tab_opened_from_another_goes_after_it_and_the_tabs_it_opened_before(cx: &mut TestAppContext) {
        let m = model(cx);
        let [a, b, c, d, e] = ids();
        insert(cx, &m, a, Insert::End, true);
        insert(cx, &m, b, Insert::End, false);
        insert(cx, &m, c, Insert::AfterOpener(a), false);
        insert(cx, &m, e, Insert::AfterOpener(c), false);
        insert(cx, &m, d, Insert::AfterOpener(a), false);
        assert_eq!(order(cx, &m), [a, c, e, d, b], "d after c and c's own child e");
    }

    #[gpui::test]
    fn a_tab_opened_from_a_grouped_tab_joins_its_group(cx: &mut TestAppContext) {
        let m = model(cx);
        let [a, b, c] = ids();
        insert(cx, &m, a, Insert::End, true);
        insert(cx, &m, b, Insert::End, false);
        let group = m.update(cx, |m, cx| m.change_groups(|g, tabs| g.new_group(tabs, &a), cx));
        insert(cx, &m, c, Insert::AfterOpener(a), false);
        assert_eq!(order(cx, &m), [a, c, b]);
        assert_eq!(m.read_with(cx, |m, _| m.group_of(c).map(|g| g.id)), Some(group));
    }

    #[gpui::test]
    fn pinned_tabs_lead_and_keep_to_their_section(cx: &mut TestAppContext) {
        let m = model(cx);
        let [a, b, c, d] = ids();
        insert(cx, &m, a, Insert::End, true);
        insert(cx, &m, b, Insert::End, false);
        insert(cx, &m, c, Insert::Pinned(9), false);
        assert_eq!(order(cx, &m), [c, a, b], "a pinned tab goes among the pinned ones");
        insert(cx, &m, d, Insert::Index(0), false);
        assert_eq!(order(cx, &m), [c, d, a, b], "an unpinned one never goes before them");
        m.update(cx, |m, cx| m.set_pinned(b, true, cx));
        assert_eq!(order(cx, &m), [c, b, d, a], "pinning puts it last among the pinned");
        m.update(cx, |m, cx| m.move_to(b, 3, cx));
        assert_eq!(order(cx, &m), [c, b, d, a], "a pinned tab stays among the pinned");
        m.update(cx, |m, cx| m.move_to(a, 0, cx));
        assert_eq!(order(cx, &m), [c, b, a, d], "an unpinned tab stays after them");
        m.update(cx, |m, cx| m.set_pinned(c, false, cx));
        assert_eq!(order(cx, &m), [b, c, a, d], "unpinning puts it first among the others");
        assert_eq!(m.read_with(cx, |m, _| m.pinned_count()), 1);
        let opened = TabId::next();
        insert(cx, &m, opened, Insert::AfterOpener(b), false);
        assert_eq!(order(cx, &m), [b, opened, c, a, d], "a tab a pinned tab opens goes first among the others");
        assert!(!m.read_with(cx, |m, _| m.is_pinned(opened)));
    }

    #[gpui::test]
    fn closing_the_selected_tab_selects_its_opener_sibling_else_the_next_one(cx: &mut TestAppContext) {
        let m = model(cx);
        let [a, b, c, d] = ids();
        insert(cx, &m, a, Insert::End, true);
        insert(cx, &m, d, Insert::End, false);
        insert(cx, &m, b, Insert::AfterOpener(a), false);
        insert(cx, &m, c, Insert::AfterOpener(a), true);
        assert_eq!(order(cx, &m), [a, b, c, d]);
        let removed = m.update(cx, |m, cx| m.remove(c, cx)).unwrap();
        assert_eq!(removed.selected, Some(b), "the tab before it, which its opener also opened");
        let removed = m.update(cx, |m, cx| m.remove(b, cx)).unwrap();
        assert_eq!(removed.selected, Some(a), "its opener, just before it");
        m.update(cx, |m, cx| m.select(a, cx));
        let removed = m.update(cx, |m, cx| m.remove(a, cx)).unwrap();
        assert_eq!(removed.selected, Some(d), "no opener: the next tab");
        let removed = m.update(cx, |m, cx| m.remove(d, cx)).unwrap();
        assert_eq!(removed.selected, None, "the last tab leaves nothing selected");
        assert_eq!(m.read_with(cx, |m, _| m.selected()), None);
    }

    #[gpui::test]
    fn closing_a_tab_in_a_group_selects_its_group_mate(cx: &mut TestAppContext) {
        let m = model(cx);
        let [a, b, c] = ids();
        for t in [a, b, c] {
            insert(cx, &m, t, Insert::End, false);
        }
        m.update(cx, |m, cx| {
            let group = m.change_groups(|g, tabs| g.new_group(tabs, &a), cx);
            m.change_groups(|g, tabs| ((), g.join(tabs, &b, group)), cx);
            m.select(b, cx);
        });
        let removed = m.update(cx, |m, cx| m.remove(b, cx)).unwrap();
        assert_eq!(removed.selected, Some(a), "the mate before it, not c outside the group");
    }

    #[gpui::test]
    fn next_shown_passes_over_collapsed_groups(cx: &mut TestAppContext) {
        let m = model(cx);
        let [a, b, c, d] = ids();
        for t in [a, b, c, d] {
            insert(cx, &m, t, Insert::End, false);
        }
        m.update(cx, |m, cx| {
            let group = m.change_groups(|g, tabs| g.new_group(tabs, &b), cx);
            m.change_groups(|g, tabs| ((), g.join(tabs, &c, group)), cx);
            m.change_groups(|g, tabs| ((), g.set_collapsed(tabs, group, true)), cx);
        });
        assert_eq!(m.read_with(cx, |m, _| m.selected()), Some(a));
        assert_eq!(m.read_with(cx, |m, _| m.next_shown(false)), Some(d));
        assert_eq!(m.read_with(cx, |m, _| m.next_shown(true)), Some(d));
    }

    #[gpui::test]
    fn group_steps_apply_with_settling_held(cx: &mut TestAppContext) {
        let m = model(cx);
        let [p, a, b, c] = ids();
        insert(cx, &m, p, Insert::Pinned(0), true);
        for t in [a, b, c] {
            insert(cx, &m, t, Insert::End, false);
        }
        let group = m.update(cx, |m, cx| {
            let group = m.change_groups(|g, tabs| g.new_group(tabs, &b), cx);
            m.change_groups(|g, tabs| ((), g.join(tabs, &c, group)), cx);
            group
        });
        // Unpin, then move to the group's end: settling after the unpin alone would find p apart
        // from the group and drop it.
        m.update(cx, |m, cx| m.change_groups(|g, tabs| ((), g.join(tabs, &p, group)), cx));
        assert_eq!(order(cx, &m), [a, b, c, p]);
        assert_eq!(m.read_with(cx, |m, _| m.pinned_count()), 0);
        assert_eq!(m.read_with(cx, |m, _| m.group_of(p).map(|g| g.id)), Some(group));
    }

    #[gpui::test]
    fn restored_groups_settle_and_collapsing_the_last_shown_tab_asks_for_a_new_one(cx: &mut TestAppContext) {
        let m = model(cx);
        let [a, b] = ids();
        insert(cx, &m, a, Insert::End, true);
        insert(cx, &m, b, Insert::End, false);
        let work = TabGroup { id: GroupId::new(), title: "Work".into(), color: GroupColor::Blue, collapsed: false };
        m.update(cx, |m, cx| m.restore_groups(vec![(a, Some(work.clone())), (b, Some(work.clone()))], cx));
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = events.clone();
        let _sub = cx.update(|cx| cx.subscribe(&m, move |_, e: &TabListEvent, _| seen.borrow_mut().push(e.clone())));
        m.update(cx, |m, cx| m.change_groups(|g, tabs| ((), g.set_collapsed(tabs, work.id, true)), cx));
        cx.run_until_parked();
        assert!(events.borrow().contains(&TabListEvent::OpenTabRequested), "{:?}", events.borrow());
    }
}
