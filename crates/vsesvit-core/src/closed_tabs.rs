//! Owned by W0.3.
//! The closed-tab stacks behind "reopen closed tab" and tab search's recently closed tabs, one for
//! normal and one for private windows, each capped at [`KEPT`] (docs/design/linux-cef/design.md
//! section 4.3, row 3). `Icon` is whatever the shell draws a tab's icon from.

use crate::private::Browsing;
use crate::tab_groups::GroupId;

/// Recently closed tabs kept per kind of window, as Chrome keeps for Ctrl+Shift+T.
pub const KEPT: usize = 25;

#[derive(Clone, Debug, PartialEq)]
pub struct ClosedTab<Icon> {
    pub url: String,
    pub title: String,
    /// The tab's icon when it closed, for tab search.
    pub favicon: Option<Icon>,
    /// When it closed, on the shell's use clock. No two closed tabs share it, so tab search names
    /// the tab by it: unlike its place among them, it stays put while other tabs close.
    pub closed: u64,
    /// The group it was in, which it goes back into if its window still has it.
    pub group: Option<GroupId>,
}

/// Recently closed tabs, a stack per kind of window: neither kind sees the other's.
#[derive(Debug)]
pub struct ClosedTabs<Icon> {
    normal: Vec<ClosedTab<Icon>>,
    private: Vec<ClosedTab<Icon>>,
}

impl<Icon> Default for ClosedTabs<Icon> {
    fn default() -> Self {
        ClosedTabs { normal: Vec::new(), private: Vec::new() }
    }
}

impl<Icon> ClosedTabs<Icon> {
    /// `browsing`'s closed tabs, oldest first.
    pub fn of(&self, browsing: Browsing) -> &[ClosedTab<Icon>] {
        match browsing {
            Browsing::Normal => &self.normal,
            Browsing::Private => &self.private,
        }
    }

    fn of_mut(&mut self, browsing: Browsing) -> &mut Vec<ClosedTab<Icon>> {
        match browsing {
            Browsing::Normal => &mut self.normal,
            Browsing::Private => &mut self.private,
        }
    }

    /// Remembers `tab` among `browsing`'s, forgetting the oldest beyond [`KEPT`].
    pub fn push(&mut self, browsing: Browsing, tab: ClosedTab<Icon>) {
        let closed = self.of_mut(browsing);
        closed.push(tab);
        let excess = closed.len().saturating_sub(KEPT);
        closed.drain(..excess);
    }

    /// The most recently closed tab of `browsing`'s kind.
    pub fn pop(&mut self, browsing: Browsing) -> Option<ClosedTab<Icon>> {
        self.of_mut(browsing).pop()
    }

    /// The closed tab of `browsing`'s kind that closed at `closed`, taken from wherever it is
    /// among them.
    pub fn take(&mut self, browsing: Browsing, closed: u64) -> Option<ClosedTab<Icon>> {
        let tabs = self.of_mut(browsing);
        let at = tabs.iter().position(|t| t.closed == closed)?;
        Some(tabs.remove(at))
    }

    /// The private session ended: its closed tabs go with it.
    pub fn end_private_session(&mut self) {
        self.private.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(closed: u64) -> ClosedTab<()> {
        ClosedTab { url: format!("https://{closed}.test/"), title: String::new(), favicon: None, closed, group: None }
    }

    fn closed(tabs: &ClosedTabs<()>, browsing: Browsing) -> Vec<u64> {
        tabs.of(browsing).iter().map(|t| t.closed).collect()
    }

    #[test]
    fn pops_most_recent_first_and_forgets_the_oldest_beyond_the_cap() {
        let mut tabs = ClosedTabs::default();
        for at in 1..=KEPT as u64 + 2 {
            tabs.push(Browsing::Normal, tab(at));
        }
        assert_eq!(tabs.of(Browsing::Normal).len(), KEPT);
        assert_eq!(tabs.of(Browsing::Normal)[0].closed, 3, "the two oldest went");
        assert_eq!(tabs.pop(Browsing::Normal).map(|t| t.closed), Some(KEPT as u64 + 2));
        assert_eq!(tabs.pop(Browsing::Normal).map(|t| t.closed), Some(KEPT as u64 + 1));
    }

    #[test]
    fn a_tab_is_taken_by_when_it_closed_while_others_come_and_go() {
        let mut tabs = ClosedTabs::default();
        for at in [1, 2, 3] {
            tabs.push(Browsing::Normal, tab(at));
        }
        assert_eq!(tabs.take(Browsing::Normal, 2).map(|t| t.closed), Some(2));
        assert_eq!(tabs.take(Browsing::Normal, 2), None);
        tabs.push(Browsing::Normal, tab(4));
        assert_eq!(closed(&tabs, Browsing::Normal), [1, 3, 4]);
    }

    #[test]
    fn normal_and_private_windows_never_see_each_others_and_private_ones_end_with_the_session() {
        let mut tabs = ClosedTabs::default();
        tabs.push(Browsing::Normal, tab(1));
        tabs.push(Browsing::Private, tab(2));
        assert_eq!(closed(&tabs, Browsing::Normal), [1]);
        assert_eq!(closed(&tabs, Browsing::Private), [2]);
        assert_eq!(tabs.take(Browsing::Normal, 2), None);
        assert_eq!(tabs.pop(Browsing::Private).map(|t| t.closed), Some(2));
        tabs.push(Browsing::Private, tab(3));
        tabs.end_private_session();
        assert!(tabs.of(Browsing::Private).is_empty());
        assert_eq!(closed(&tabs, Browsing::Normal), [1]);
    }
}
