//! Tab groups, as Chrome's: a named, coloured run of a window's tabs that collapses to its
//! header. The same rules in both shells.
//!
//! Each shell keeps one [`TabGroups`] per window, beside its own order of tabs (the `AdwTabView`,
//! the XAML items), which stays the one source of the order: every call is handed the window's
//! tabs as they are now ([`WindowTabs`]). The menu actions change the groups and return the
//! [`Step`]s that move tabs to match; the shell applies them, then runs [`TabGroups::settle`]. It
//! also runs `settle` after anything else that adds, closes, moves, pins or selects a tab,
//! whatever did it (a drag in either tab list, an extension's `tabs.move`, a tab moved to another
//! window), so the groups follow where the tabs are:
//!
//! - pinned tabs are in no group;
//! - a group's tabs are next to each other, and a group without tabs ceases to be;
//! - a tab dropped between two tabs of a group joins it, and one dragged away from the rest of its
//!   group leaves it;
//! - the selected tab is never hidden in a collapsed group: selecting one expands it.
//!
//! A group lives in its window. The session saves each tab's group with it
//! ([`crate::session::TabSnapshot::group`]); private windows' tabs, and so their groups, are never
//! saved.

use std::collections::HashMap;
use std::hash::Hash;

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// A group's identity, the same across restarts and on other devices.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GroupId(pub Uuid);

impl GroupId {
    pub fn new() -> Self {
        GroupId(Uuid::new_v4())
    }
}

impl Default for GroupId {
    fn default() -> Self {
        GroupId::new()
    }
}

impl std::fmt::Display for GroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for GroupId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(GroupId)
    }
}

/// `chrome.tabs.Tab.groupId`: -1 (`chrome.tabGroups.TAB_GROUP_ID_NONE`) for a tab in no group,
/// else a positive integer folded from the group's id, so every part of the extension runtime
/// agrees on it without keeping a table.
pub fn extension_group_id(group: Option<GroupId>) -> i32 {
    let Some(GroupId(id)) = group else { return -1 };
    let wide = id.as_u128();
    let folded = (wide ^ (wide >> 32) ^ (wide >> 64) ^ (wide >> 96)) as u32 & 0x7fff_ffff;
    folded.max(1).cast_signed()
}

/// Chrome's nine group colours, in Chrome's order. A colour a newer build adds reads as grey, so
/// it never fails the session record it comes in.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupColor {
    #[default]
    Grey,
    Blue,
    Red,
    Yellow,
    Green,
    Pink,
    Purple,
    Cyan,
    Orange,
}

impl GroupColor {
    pub const ALL: [GroupColor; 9] =
        [Self::Grey, Self::Blue, Self::Red, Self::Yellow, Self::Green, Self::Pink, Self::Purple, Self::Cyan, Self::Orange];

    pub fn label(self) -> &'static str {
        match self {
            Self::Grey => "Grey",
            Self::Blue => "Blue",
            Self::Red => "Red",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Pink => "Pink",
            Self::Purple => "Purple",
            Self::Cyan => "Cyan",
            Self::Orange => "Orange",
        }
    }

    /// Chrome's shade of it, for a light or a dark theme.
    pub fn rgb(self, dark: bool) -> [u8; 3] {
        let hex: u32 = match (self, dark) {
            (Self::Grey, false) => 0x5f6368,
            (Self::Blue, false) => 0x1a73e8,
            (Self::Red, false) => 0xd93025,
            (Self::Yellow, false) => 0xf9ab00,
            (Self::Green, false) => 0x1e8e3e,
            (Self::Pink, false) => 0xd01884,
            (Self::Purple, false) => 0x9334e6,
            (Self::Cyan, false) => 0x007b83,
            (Self::Orange, false) => 0xfa903e,
            (Self::Grey, true) => 0xbdc1c6,
            (Self::Blue, true) => 0x8ab4f8,
            (Self::Red, true) => 0xf28b82,
            (Self::Yellow, true) => 0xfdd663,
            (Self::Green, true) => 0x81c995,
            (Self::Pink, true) => 0xff8bcb,
            (Self::Purple, true) => 0xc58af9,
            (Self::Cyan, true) => 0x78d9ec,
            (Self::Orange, true) => 0xfcad70,
        };
        let [_, r, g, b] = hex.to_be_bytes();
        [r, g, b]
    }

    /// The colour Chrome gives a new group: the one fewest of `used` have, the first in Chrome's
    /// order among equals.
    pub fn next(used: impl IntoIterator<Item = GroupColor>) -> GroupColor {
        let mut counts = [0usize; 9];
        for color in used {
            counts[color as usize] += 1;
        }
        let fewest = counts.iter().min().copied().unwrap_or(0);
        Self::ALL[counts.iter().position(|&n| n == fewest).unwrap_or(0)]
    }
}

impl<'de> Deserialize<'de> for GroupColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(Self::ALL.into_iter().find(|c| c.label().eq_ignore_ascii_case(&name)).unwrap_or_default())
    }
}

/// A group apart from its tabs. Each member tab's session record carries a copy.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TabGroup {
    pub id: GroupId,
    /// Empty until the user names it; the header then shows the colour alone.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub color: GroupColor,
    #[serde(default)]
    pub collapsed: bool,
}

impl TabGroup {
    /// How menus name it: its title, or its colour's while it has none ("Blue group").
    pub fn name(&self) -> String {
        if self.title.is_empty() { format!("{} group", self.color.label()) } else { self.title.clone() }
    }
}

/// A window's tabs as the shell has them now, built for each call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowTabs<K> {
    /// Every tab, in the window's order, hidden ones too.
    pub order: Vec<K>,
    /// The first `pinned` tabs of `order` are pinned.
    pub pinned: usize,
    /// The selected tab.
    pub active: Option<K>,
}

/// What the shell does to its tabs after a group action, in order. With the shell's `settle`
/// held off until the last step is done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step<K> {
    /// Unpin the tab, which puts it first among the unpinned tabs, as both shells' Unpin does.
    Unpin(K),
    /// Move the tab so it ends at this index.
    Move(K, usize),
    Activate(K),
    /// Open a new tab at the end of the window and select it.
    OpenTab,
}

/// One line of a tab list. Tabs of a collapsed group keep their line, hidden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row<K> {
    /// Before the group's first tab.
    Header(TabGroup),
    Tab { tab: K, group: Option<GroupId>, hidden: bool },
}

/// One window's groups and which of its tabs are in which. After [`TabGroups::settle`]: every
/// member is an unpinned tab of the window, every group has a member, a group's members are
/// next to each other, and the active tab is not in a collapsed group.
#[derive(Clone, Debug)]
pub struct TabGroups<K> {
    groups: Vec<TabGroup>,
    members: HashMap<K, GroupId>,
}

impl<K> Default for TabGroups<K> {
    fn default() -> Self {
        TabGroups { groups: Vec::new(), members: HashMap::new() }
    }
}

impl<K: Clone + Eq + Hash> TabGroups<K> {
    /// From a restored window's tabs and the groups their records name. The first tab naming a
    /// group says what the group is. The record may come from another build or be damaged, so the
    /// shell settles after.
    pub fn restore(tabs: impl IntoIterator<Item = (K, Option<TabGroup>)>) -> Self {
        let mut restored = TabGroups::default();
        for (tab, group) in tabs {
            let Some(group) = group else { continue };
            restored.members.insert(tab, group.id);
            if restored.get(group.id).is_none() {
                restored.groups.push(group);
            }
        }
        restored
    }

    pub fn group_of(&self, tab: &K) -> Option<&TabGroup> {
        self.members.get(tab).and_then(|&id| self.get(id))
    }

    pub fn get(&self, id: GroupId) -> Option<&TabGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// The window's groups in the order they show, for the tab menu's "Add tab to group".
    pub fn in_order(&self, tabs: &WindowTabs<K>) -> Vec<TabGroup> {
        let mut seen = Vec::new();
        for tab in &tabs.order {
            if let Some(group) = self.group_of(tab)
                && !seen.contains(group)
            {
                seen.push(group.clone());
            }
        }
        seen
    }

    /// The group's tabs, in order. Close Group closes these.
    pub fn members(&self, tabs: &WindowTabs<K>, group: GroupId) -> Vec<K> {
        tabs.order.iter().filter(|t| self.members.get(*t) == Some(&group)).cloned().collect()
    }

    /// "Add tab to new group": the tab, unpinned, alone in a new group of the next colour. One in
    /// the middle of another group leaves it for just after it; otherwise it stays where it is.
    pub fn new_group(&mut self, tabs: &WindowTabs<K>, tab: &K) -> (GroupId, Vec<Step<K>>) {
        let color = GroupColor::next(self.groups.iter().map(|g| g.color));
        let group = TabGroup { id: GroupId::new(), title: String::new(), color, collapsed: false };
        let id = group.id;
        let steps = self.regroup(tabs, tab, Some(group));
        (id, steps)
    }

    /// "Add tab to group": the tab, unpinned, joins the end of `group`, which expands.
    pub fn join(&mut self, tabs: &WindowTabs<K>, tab: &K, group: GroupId) -> Vec<Step<K>> {
        if self.members.get(tab) == Some(&group) {
            return Vec::new();
        }
        let Some(target) = self.get(group).cloned() else { return Vec::new() };
        self.regroup(tabs, tab, Some(TabGroup { collapsed: false, ..target }))
    }

    /// "Remove from group": a tab in the middle of its group leaves it for just after it; one at
    /// either end stays where it is.
    pub fn leave(&mut self, tabs: &WindowTabs<K>, tab: &K) -> Vec<Step<K>> {
        if !self.members.contains_key(tab) {
            return Vec::new();
        }
        self.regroup(tabs, tab, None)
    }

    /// Collapses or expands `group`. Collapsing the group of the selected tab selects the nearest
    /// shown tab after the group, else before it, else a new tab.
    pub fn set_collapsed(&mut self, tabs: &WindowTabs<K>, group: GroupId, collapsed: bool) -> Vec<Step<K>> {
        let Some(index) = self.groups.iter().position(|g| g.id == group) else { return Vec::new() };
        self.groups[index].collapsed = collapsed;
        let Some(active) = tabs.active.as_ref().filter(|a| collapsed && self.members.get(*a) == Some(&group)) else {
            return Vec::new();
        };
        let at = tabs.order.iter().position(|t| t == active).unwrap_or(0);
        vec![self.nearest_shown(tabs, at).map_or(Step::OpenTab, |t| Step::Activate(t.clone()))]
    }

    /// Ctrl+Tab's tab, or Ctrl+Shift+Tab's going `back`: the next tab from the selected one that
    /// way, round the end, past those hidden in collapsed groups, as in Chrome. `None` when no
    /// other tab shows.
    pub fn next_shown(&self, tabs: &WindowTabs<K>, back: bool) -> Option<K> {
        let at = tabs.order.iter().position(|t| Some(t) == tabs.active.as_ref())?;
        let n = tabs.order.len();
        (1..n)
            .map(|i| &tabs.order[if back { (at + n - i) % n } else { (at + i) % n }])
            .find(|t| self.is_shown(t))
            .cloned()
    }

    /// The tab selected when the selected tab `closing` closes, as Chrome picks it: `opener`,
    /// the shell's pick by which tab opened which, if it shows; else the tab beside it in its
    /// group, the one after first; else the nearest shown tab after it, then before it; else
    /// the tab beside it, whose group selecting it expands. `None` for the window's last tab.
    pub fn after_closing(&self, tabs: &WindowTabs<K>, closing: &K, opener: Option<&K>) -> Option<K> {
        let at = tabs.order.iter().position(|t| t == closing)?;
        if let Some(opener) = opener.filter(|o| *o != closing && tabs.order.contains(o) && self.is_shown(o)) {
            return Some(opener.clone());
        }
        let beside = [tabs.order.get(at + 1), at.checked_sub(1).map(|i| &tabs.order[i])];
        let own = self.members.get(closing);
        let mate = beside.into_iter().flatten().find(|t| own.is_some() && self.members.get(*t) == own);
        mate.or_else(|| self.nearest_shown(tabs, at)).or_else(|| beside.into_iter().flatten().next()).cloned()
    }

    /// The shown tab nearest after the one at `at`, else before it.
    fn nearest_shown<'a>(&self, tabs: &'a WindowTabs<K>, at: usize) -> Option<&'a K> {
        let (before, after) = tabs.order.split_at(at.min(tabs.order.len()));
        after.iter().skip(1).chain(before.iter().rev()).find(|t| self.is_shown(t))
    }

    /// Whether the tab shows: it is in no group, or in an expanded one.
    fn is_shown(&self, tab: &K) -> bool {
        !self.group_of(tab).is_some_and(|g| g.collapsed)
    }

    pub fn set_title(&mut self, group: GroupId, title: &str) {
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == group) {
            title.clone_into(&mut g.title);
        }
    }

    pub fn set_color(&mut self, group: GroupId, color: GroupColor) {
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == group) {
            g.color = color;
        }
    }

    /// "Ungroup": the tabs stay where they are, in no group.
    pub fn ungroup(&mut self, group: GroupId) {
        self.members.retain(|_, g| *g != group);
        self.groups.retain(|g| g.id != group);
    }

    /// A tab opened from `opener` (New Tab to the Right, Duplicate, a link opened in a new tab)
    /// starts in its group, as in Chrome. The shell puts it beside `opener`.
    pub fn adopt(&mut self, tab: K, opener: &K) {
        if let Some(&group) = self.members.get(opener) {
            self.members.insert(tab, group);
        }
    }

    /// Makes the groups follow the window's tabs after any change. `moved` is the tab that just
    /// moved or opened, if the shell knows it: it joins the group it landed inside, and leaves its
    /// group when it landed away from the group's other tabs. Otherwise a group split in parts
    /// keeps its longest part. Settling twice changes nothing the second time.
    pub fn settle(&mut self, tabs: &WindowTabs<K>, moved: Option<&K>) {
        let unpinned = &tabs.order[tabs.pinned.min(tabs.order.len())..];
        let known: Vec<GroupId> = self.groups.iter().map(|g| g.id).collect();
        self.members.retain(|tab, group| unpinned.contains(tab) && known.contains(group));

        if let Some(i) = moved.and_then(|m| unpinned.iter().position(|t| t == m)) {
            let of = |i: usize| self.members.get(&unpinned[i]).copied();
            let (left, right) = (i.checked_sub(1).and_then(of), unpinned.get(i + 1).and_then(|_| of(i + 1)));
            let own = of(i);
            if let Some(inside) = left.filter(|_| left == right) {
                self.members.insert(unpinned[i].clone(), inside);
            } else if own.is_some() && left != own && right != own {
                self.members.remove(&unpinned[i]);
            }
        }

        let mut runs: Vec<(GroupId, usize, usize)> = Vec::new();
        for (i, tab) in unpinned.iter().enumerate() {
            let Some(&group) = self.members.get(tab) else { continue };
            match runs.last_mut() {
                Some((g, _, end)) if *g == group && *end == i => *end = i + 1,
                _ => runs.push((group, i, i + 1)),
            }
        }
        let mut kept: HashMap<GroupId, (usize, usize)> = HashMap::new();
        for &(group, start, end) in &runs {
            let longer = kept.get(&group).is_none_or(|&(s, e)| end - start > e - s);
            if longer {
                kept.insert(group, (start, end));
            }
        }
        for (i, tab) in unpinned.iter().enumerate() {
            if let Some(group) = self.members.get(tab)
                && kept.get(group).is_none_or(|&(start, end)| !(start..end).contains(&i))
            {
                self.members.remove(tab);
            }
        }

        self.groups.retain(|g| kept.contains_key(&g.id));
        if let Some(&group) = tabs.active.as_ref().and_then(|a| self.members.get(a))
            && let Some(g) = self.groups.iter_mut().find(|g| g.id == group)
        {
            g.collapsed = false;
        }
    }

    /// The window's tab list: a header before each group, then its tabs, hidden while it is
    /// collapsed.
    pub fn rows(&self, tabs: &WindowTabs<K>) -> Vec<Row<K>> {
        let mut rows = Vec::with_capacity(tabs.order.len());
        let mut previous = None;
        for tab in &tabs.order {
            let group = self.group_of(tab);
            if let Some(g) = group
                && previous != Some(g.id)
            {
                rows.push(Row::Header(g.clone()));
            }
            previous = group.map(|g| g.id);
            rows.push(Row::Tab { tab: tab.clone(), group: previous, hidden: group.is_some_and(|g| g.collapsed) });
        }
        rows
    }

    /// Puts `tab` in `group` (or in none), unpinning it, and says how to move it there: to the end
    /// of a group that has tabs; otherwise where it is, unless that would split the group around
    /// it, in which case just after that group.
    fn regroup(&mut self, tabs: &WindowTabs<K>, tab: &K, group: Option<TabGroup>) -> Vec<Step<K>> {
        let Some(mut at) = tabs.order.iter().position(|t| t == tab) else { return Vec::new() };
        let mut steps = Vec::new();
        let mut order = tabs.order.clone();
        if at < tabs.pinned {
            steps.push(Step::Unpin(tab.clone()));
            let unpinned = order.remove(at);
            at = tabs.pinned - 1;
            order.insert(at, unpinned);
        }
        order.remove(at);
        let of = |i: usize| order.get(i).and_then(|t| self.members.get(t)).copied();
        let target = group.as_ref().map(|g| g.id);
        let run_end = |g: GroupId| order.iter().rposition(|t| self.members.get(t) == Some(&g)).map(|i| i + 1);
        let to = match target.and_then(run_end) {
            Some(end) => end,
            None => match at.checked_sub(1).and_then(of) {
                Some(around) if of(at) == Some(around) && target != Some(around) => run_end(around).unwrap_or(at),
                _ => at,
            },
        };
        if to != at {
            steps.push(Step::Move(tab.clone(), to));
        }
        match group {
            Some(group) => {
                let id = group.id;
                match self.groups.iter_mut().find(|g| g.id == id) {
                    Some(existing) => *existing = group,
                    None => self.groups.push(group),
                }
                self.members.insert(tab.clone(), id);
            }
            None => {
                self.members.remove(tab);
            }
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tabs are letters; `pinned` lead.
    fn tabs(order: &str, pinned: usize, active: Option<char>) -> WindowTabs<char> {
        WindowTabs { order: order.chars().collect(), pinned, active }
    }

    fn group_with(groups: &mut TabGroups<char>, members: &str) -> GroupId {
        let id = GroupId(Uuid::from_u128(groups.groups.len() as u128 + 1));
        groups.groups.push(TabGroup { id, title: String::new(), color: GroupColor::Grey, collapsed: false });
        for tab in members.chars() {
            groups.members.insert(tab, id);
        }
        id
    }

    /// Which tabs share a group, as the strings of each group's tabs in window order.
    fn grouping(groups: &TabGroups<char>, tabs: &WindowTabs<char>) -> Vec<String> {
        groups.in_order(tabs).iter().map(|g| groups.members(tabs, g.id).into_iter().collect()).collect()
    }

    fn apply(tabs: &WindowTabs<char>, steps: &[Step<char>]) -> WindowTabs<char> {
        let mut after = tabs.clone();
        for step in steps {
            match step {
                Step::Unpin(t) => {
                    let at = after.order.iter().position(|o| o == t).unwrap();
                    let t = after.order.remove(at);
                    after.pinned -= 1;
                    after.order.insert(after.pinned, t);
                }
                Step::Move(t, to) => {
                    let at = after.order.iter().position(|o| o == t).unwrap();
                    let t = after.order.remove(at);
                    after.order.insert(*to, t);
                }
                Step::Activate(t) => after.active = Some(*t),
                Step::OpenTab => {
                    after.order.push('+');
                    after.active = Some('+');
                }
            }
        }
        after
    }

    #[test]
    fn a_new_group_takes_the_least_used_colour_first_in_chromes_order() {
        assert_eq!(GroupColor::next([]), GroupColor::Grey);
        assert_eq!(GroupColor::next([GroupColor::Grey, GroupColor::Blue]), GroupColor::Red);
        let all_once_grey_twice = GroupColor::ALL.into_iter().chain([GroupColor::Grey]);
        assert_eq!(GroupColor::next(all_once_grey_twice), GroupColor::Blue);
    }

    #[test]
    fn an_unknown_colour_reads_as_grey() {
        let group: TabGroup = serde_json::from_str(&format!(r#"{{"id":"{}","title":"x","color":"teal"}}"#, Uuid::nil())).unwrap();
        assert_eq!(group.color, GroupColor::Grey);
        assert!(!group.collapsed);
        assert_eq!(group.name(), "x");
        assert_eq!(TabGroup { title: String::new(), color: GroupColor::Cyan, ..group }.name(), "Cyan group");
        assert_eq!(serde_json::to_string(&GroupColor::Cyan).unwrap(), r#""cyan""#);
    }

    #[test]
    fn extensions_see_minus_one_or_a_positive_id_that_stays_the_same() {
        assert_eq!(extension_group_id(None), -1);
        let id = GroupId(Uuid::from_u128(0xdead_beef_0000_0001_8000_0000_ffff_ffff));
        assert!(extension_group_id(Some(id)) > 0);
        assert_eq!(extension_group_id(Some(id)), extension_group_id(Some(id)));
        assert_eq!(extension_group_id(Some(GroupId(Uuid::nil()))), 1);
        assert_eq!(id.to_string().parse::<GroupId>().unwrap(), id);
    }

    #[test]
    fn grouping_a_pinned_tab_unpins_it_where_the_pinned_tabs_end() {
        let mut groups = TabGroups::default();
        let window = tabs("pqab", 2, Some('a'));
        let (id, steps) = groups.new_group(&window, &'p');
        assert_eq!(steps, [Step::Unpin('p')]);
        let after = apply(&window, &steps);
        assert_eq!((after.order.iter().collect::<String>(), after.pinned), ("qpab".into(), 1));
        groups.settle(&after, None);
        assert_eq!(groups.group_of(&'p').map(|g| g.id), Some(id));
    }

    #[test]
    fn a_new_group_from_the_middle_of_another_starts_just_after_it() {
        let mut groups = TabGroups::default();
        group_with(&mut groups, "abc");
        let window = tabs("abcd", 0, None);
        let (_, steps) = groups.new_group(&window, &'b');
        assert_eq!(steps, [Step::Move('b', 2)]);
        let after = apply(&window, &steps);
        groups.settle(&after, None);
        assert_eq!(grouping(&groups, &after), ["ac", "b"]);
    }

    #[test]
    fn joining_a_group_moves_the_tab_to_its_end_and_expands_it() {
        let mut groups = TabGroups::default();
        let g = group_with(&mut groups, "bc");
        groups.groups[0].collapsed = true;
        let window = tabs("abcd", 0, Some('d'));
        let steps = groups.join(&window, &'a', g);
        let after = apply(&window, &steps);
        groups.settle(&after, None);
        assert_eq!(after.order.iter().collect::<String>(), "bcad");
        assert_eq!(grouping(&groups, &after), ["bca"]);
        assert!(!groups.get(g).unwrap().collapsed);
        assert!(groups.join(&after, &'a', g).is_empty(), "already in it");
    }

    #[test]
    fn joining_from_another_group_takes_the_tab_out_of_it() {
        let mut groups = TabGroups::default();
        group_with(&mut groups, "ab");
        let h = group_with(&mut groups, "cd");
        let window = tabs("abcd", 0, None);
        let after = apply(&window, &groups.join(&window, &'a', h));
        groups.settle(&after, None);
        assert_eq!(after.order.iter().collect::<String>(), "bcda");
        assert_eq!(grouping(&groups, &after), ["b", "cda"]);
    }

    #[test]
    fn leaving_from_the_middle_lands_after_the_group_and_from_an_edge_stays() {
        let mut groups = TabGroups::default();
        group_with(&mut groups, "abc");
        let window = tabs("abcd", 0, None);
        let steps = groups.leave(&window, &'b');
        assert_eq!(steps, [Step::Move('b', 2)]);
        let after = apply(&window, &steps);
        groups.settle(&after, None);
        assert_eq!(grouping(&groups, &after), ["ac"]);
        assert!(groups.leave(&after, &'c').is_empty(), "at the end it stays");
        groups.settle(&after, None);
        assert_eq!(grouping(&groups, &after), ["a"]);
    }

    #[test]
    fn the_last_tab_leaving_ends_the_group() {
        let mut groups = TabGroups::default();
        let g = group_with(&mut groups, "a");
        let window = tabs("ab", 0, None);
        groups.leave(&window, &'a');
        groups.settle(&window, None);
        assert!(groups.get(g).is_none());
        assert!(groups.is_empty());
    }

    #[test]
    fn collapsing_the_selected_tabs_group_selects_the_nearest_shown_tab() {
        let mut groups = TabGroups::default();
        let g = group_with(&mut groups, "bc");
        assert_eq!(groups.set_collapsed(&tabs("abcd", 0, Some('b')), g, true), [Step::Activate('d')]);
        groups.set_collapsed(&tabs("abcd", 0, None), g, false);
        assert_eq!(groups.set_collapsed(&tabs("abc", 0, Some('c')), g, true), [Step::Activate('a')]);
        groups.set_collapsed(&tabs("abc", 0, None), g, false);
        let h = group_with(&mut groups, "d");
        groups.set_collapsed(&tabs("bcd", 0, None), h, true);
        assert_eq!(groups.set_collapsed(&tabs("bcd", 0, Some('b')), g, true), [Step::OpenTab], "d is hidden too");
        assert!(groups.set_collapsed(&tabs("abcd", 0, Some('a')), g, true).is_empty(), "the selected tab is not in it");
    }

    #[test]
    fn ctrl_tab_steps_round_the_end_past_collapsed_groups() {
        let mut groups = TabGroups::default();
        let g = group_with(&mut groups, "bc");
        groups.set_collapsed(&tabs("abcd", 0, None), g, true);
        assert_eq!(groups.next_shown(&tabs("abcd", 0, Some('a')), false), Some('d'));
        assert_eq!(groups.next_shown(&tabs("abcd", 0, Some('d')), true), Some('a'));
        assert_eq!(groups.next_shown(&tabs("abcd", 0, Some('d')), false), Some('a'), "round the end");
        assert_eq!(groups.next_shown(&tabs("abc", 0, Some('a')), false), None, "no other tab shows");
        groups.set_collapsed(&tabs("abcd", 0, None), g, false);
        assert_eq!(groups.next_shown(&tabs("abcd", 0, Some('a')), false), Some('b'));
        assert_eq!(groups.next_shown(&tabs("abcd", 0, None), false), None);
    }

    #[test]
    fn closing_the_selected_tab_selects_a_shown_one() {
        let mut groups = TabGroups::default();
        let g = group_with(&mut groups, "bc");
        groups.set_collapsed(&tabs("abcd", 0, None), g, true);
        assert_eq!(groups.after_closing(&tabs("abcd", 0, Some('a')), &'a', None), Some('d'));
        assert_eq!(groups.after_closing(&tabs("abcd", 0, Some('d')), &'d', None), Some('a'));
        assert_eq!(groups.after_closing(&tabs("abcd", 0, Some('a')), &'a', Some(&'c')), Some('d'), "the opener is hidden");
        assert_eq!(groups.after_closing(&tabs("abcd", 0, Some('a')), &'a', Some(&'d')), Some('d'));
        assert_eq!(groups.after_closing(&tabs("abc", 0, Some('a')), &'a', None), Some('b'), "only hidden tabs are left");
        assert_eq!(groups.after_closing(&tabs("a", 0, Some('a')), &'a', None), None);
    }

    #[test]
    fn closing_a_groups_last_tab_selects_the_one_before_it_in_the_group() {
        let mut groups = TabGroups::default();
        group_with(&mut groups, "bc");
        assert_eq!(groups.after_closing(&tabs("abcd", 0, Some('c')), &'c', None), Some('b'));
        assert_eq!(groups.after_closing(&tabs("abcd", 0, Some('b')), &'b', None), Some('c'));
        assert_eq!(groups.after_closing(&tabs("abcd", 0, Some('a')), &'a', None), Some('b'));
    }

    #[test]
    fn selecting_a_hidden_tab_expands_its_group() {
        let mut groups = TabGroups::default();
        let g = group_with(&mut groups, "bc");
        groups.set_collapsed(&tabs("abc", 0, Some('a')), g, true);
        groups.settle(&tabs("abc", 0, Some('a')), None);
        assert!(groups.get(g).unwrap().collapsed);
        groups.settle(&tabs("abc", 0, Some('c')), None);
        assert!(!groups.get(g).unwrap().collapsed);
    }

    #[test]
    fn a_tab_dropped_inside_a_group_joins_it_and_one_dragged_away_leaves() {
        let mut groups = TabGroups::default();
        group_with(&mut groups, "bc");
        groups.settle(&tabs("bacd", 0, None), Some(&'a'));
        assert_eq!(grouping(&groups, &tabs("bacd", 0, None)), ["bac"]);
        groups.settle(&tabs("bcda", 0, None), Some(&'a'));
        assert_eq!(grouping(&groups, &tabs("bcda", 0, None)), ["bc"]);
    }

    #[test]
    fn a_tab_dragged_to_its_groups_edge_stays_in_it() {
        let mut groups = TabGroups::default();
        group_with(&mut groups, "abc");
        groups.settle(&tabs("bcad", 0, None), Some(&'a'));
        assert_eq!(grouping(&groups, &tabs("bcad", 0, None)), ["bca"]);
    }

    #[test]
    fn of_a_two_tab_group_the_moved_tab_is_the_one_that_leaves() {
        let mut groups = TabGroups::default();
        group_with(&mut groups, "bc");
        groups.settle(&tabs("cabd", 0, None), Some(&'c'));
        assert_eq!(grouping(&groups, &tabs("cabd", 0, None)), ["b"]);
    }

    #[test]
    fn a_scattered_group_keeps_its_longest_run() {
        let mut groups = TabGroups::default();
        group_with(&mut groups, "abcd");
        let window = tabs("axbcdey", 0, None);
        groups.members.insert('y', groups.groups[0].id);
        groups.settle(&window, None);
        assert_eq!(grouping(&groups, &window), ["bcd"]);
    }

    #[test]
    fn pinned_closed_and_unknown_members_are_forgotten() {
        let mut groups = TabGroups::default();
        let g = group_with(&mut groups, "abz");
        groups.members.insert('c', GroupId(Uuid::from_u128(99)));
        let window = tabs("abc", 1, None);
        groups.settle(&window, None);
        assert_eq!(grouping(&groups, &window), ["b"]);
        assert_eq!(groups.group_of(&'c'), None);
        groups.settle(&tabs("ac", 1, None), None);
        assert!(groups.get(g).is_none(), "its last tab closed");
    }

    #[test]
    fn rows_head_each_group_and_hide_collapsed_tabs() {
        let mut groups = TabGroups::default();
        let g = group_with(&mut groups, "bc");
        let h = group_with(&mut groups, "d");
        groups.set_collapsed(&tabs("abcd", 0, None), h, true);
        let group = |id| groups.get(id).unwrap().clone();
        let row = |tab, group, hidden| Row::Tab { tab, group, hidden };
        assert_eq!(
            groups.rows(&tabs("abcde", 0, None)),
            [
                row('a', None, false),
                Row::Header(group(g)),
                row('b', Some(g), false),
                row('c', Some(g), false),
                Row::Header(group(h)),
                row('d', Some(h), true),
                row('e', None, false),
            ]
        );
    }

    #[test]
    fn restore_takes_each_group_from_its_first_tab() {
        let id = GroupId(Uuid::from_u128(1));
        let named = |title: &str| Some(TabGroup { id, title: title.into(), color: GroupColor::Blue, collapsed: false });
        let groups = TabGroups::restore([('a', named("first")), ('b', None), ('c', named("second"))]);
        assert_eq!(groups.group_of(&'c').unwrap().title, "first");
        assert_eq!(groups.group_of(&'b'), None);
    }

    #[test]
    fn a_new_tab_from_a_grouped_tab_joins_its_group() {
        let mut groups = TabGroups::default();
        group_with(&mut groups, "ab");
        groups.adopt('n', &'b');
        groups.adopt('m', &'x');
        let window = tabs("abnmx", 0, None);
        groups.settle(&window, None);
        assert_eq!(grouping(&groups, &window), ["abn"]);
    }

    mod settling {
        use proptest::prelude::*;

        use super::*;

        proptest! {
            #[test]
            fn settles_any_window_into_the_invariants_and_then_stays(
                order in Just("abcdefgh".chars().collect::<Vec<char>>()).prop_shuffle(),
                pinned in 0usize..3,
                assigned in proptest::collection::vec(proptest::option::of(0u8..3), 8),
                moved in proptest::option::of(0usize..8),
                active in proptest::option::of(0usize..8),
            ) {
                let mut groups = TabGroups::default();
                for n in 0..3u128 {
                    groups.groups.push(TabGroup { id: GroupId(Uuid::from_u128(n)), title: String::new(), color: GroupColor::Grey, collapsed: n == 1 });
                }
                for (tab, group) in order.iter().zip(&assigned) {
                    if let Some(n) = group {
                        groups.members.insert(*tab, GroupId(Uuid::from_u128(u128::from(*n))));
                    }
                }
                let window = WindowTabs { order: order.clone(), pinned, active: active.map(|i| order[i]) };
                groups.settle(&window, moved.map(|i| &order[i]));
                for tab in &order[..pinned] {
                    prop_assert!(groups.group_of(tab).is_none());
                }
                for group in &groups.groups {
                    let at: Vec<usize> = order.iter().enumerate().filter(|(_, t)| groups.group_of(t).map(|g| g.id) == Some(group.id)).map(|(i, _)| i).collect();
                    prop_assert!(!at.is_empty());
                    prop_assert_eq!(at.last().unwrap() - at[0] + 1, at.len());
                }
                if let Some(active) = &window.active {
                    prop_assert!(!groups.group_of(active).is_some_and(|g| g.collapsed));
                }
                let settled = (groups.groups.clone(), groups.members.clone());
                groups.settle(&window, None);
                prop_assert_eq!((groups.groups.clone(), groups.members.clone()), settled);
            }
        }
    }
}
