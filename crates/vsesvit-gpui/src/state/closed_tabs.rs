//! Owned by W1.5.
//! Recently closed tabs, for Reopen Closed Tab and tab search: core's stacks, one per kind of window,
//! holding each tab's icon as it was when it closed.

use gpui::{App, Context, Entity};
use vsesvit_core::closed_tabs::ClosedTab;
use vsesvit_core::private::Browsing;
use vsesvit_core::tab_groups::GroupId;
use vsesvit_core::tab_search::{self, Listed, Row};
use vsesvit_engine::TabId;

use super::BrowserState;
use crate::tab::{Tab, TabIcon};

/// Names a closed tab in tab search: when it closed, on the use clock.
pub type ClosedKey = u64;

impl BrowserState {
    /// Remembers a tab the user closed, unless it showed nothing.
    pub(super) fn tab_closed(&mut self, tab: &Entity<Tab>, group: Option<GroupId>, cx: &mut Context<Self>) {
        let closed = self.tick();
        let tab = tab.read(cx);
        let Some(url) = tab.link() else { return };
        let record = ClosedTab {
            url: url.to_string(),
            title: tab.display_title().to_string(),
            favicon: Some(tab.icon()),
            closed,
            group,
        };
        let browsing = tab.browsing();
        self.closed.push(browsing, record);
    }

    pub fn can_reopen_closed_tab(&self, browsing: Browsing) -> bool {
        !self.closed.of(browsing).is_empty()
    }

    /// The tab closed at `key`, or the one closed last, taken off `browsing`'s stack to reopen.
    pub fn take_closed(&mut self, browsing: Browsing, key: Option<ClosedKey>) -> Option<ClosedTab<TabIcon>> {
        match key {
            Some(key) => self.closed.take(browsing, key),
            None => self.closed.pop(browsing),
        }
    }

    pub fn closed_tabs(&self, browsing: Browsing) -> &[ClosedTab<TabIcon>] {
        self.closed.of(browsing)
    }

    /// Tab search's rows in a window of `browsing`'s kind: that kind's open tabs, then the ones
    /// closed in its windows, as core narrows and orders them.
    pub fn search_tabs(&self, browsing: Browsing, query: &str, cx: &App) -> Vec<Row<TabId, ClosedKey>> {
        let open = self
            .tabs
            .values()
            .map(|e| e.tab.read(cx))
            .filter(|tab| tab.browsing() == browsing && !tab.is_ending())
            .map(|tab| Listed {
                key: tab.id(),
                title: tab.display_title().to_string(),
                url: tab.session_url().map(String::from).unwrap_or_default(),
                used: tab.used(),
            })
            .collect();
        let closed = self
            .closed
            .of(browsing)
            .iter()
            .map(|t| Listed { key: t.closed, title: t.title.clone(), url: t.url.clone(), used: t.closed })
            .collect();
        tab_search::rows(query, open, closed)
    }
}
