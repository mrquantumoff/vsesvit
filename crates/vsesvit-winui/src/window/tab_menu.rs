//! A tab's context menu, the same in both tab lists.

use windows_core::{Interface, Result};

use super::BrowserWindow;
use crate::bindings::*;
use crate::exec;
use crate::tab::TabId;

/// What a tab's menu does to its tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabAction {
    /// Shows the tab beside the selected one.
    SplitWithActive,
    /// Shows the (selected) tab beside a new tab.
    SplitWithNewTab,
    CloseSplit,
    Pin(bool),
    Mute(bool),
    /// Copies the tab's address without its tracking parameters.
    CopyLink,
    Close,
}

/// What the menu depends on.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TabFacts {
    pub active: bool,
    pub in_split: bool,
    pub pinned: bool,
    pub muted: bool,
    /// The tab shows a page with an address worth copying.
    pub has_link: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Entry {
    /// Label, Segoe Fluent glyph, action.
    Action(&'static str, &'static str, TabAction),
    Separator,
}

pub(super) fn entries(facts: TabFacts) -> Vec<Entry> {
    use Entry::{Action, Separator};
    let split = match (facts.in_split, facts.active) {
        (true, _) => Action("Close split view", "\u{E89F}", TabAction::CloseSplit),
        (false, true) => Action("Split view with new tab", "\u{E8A0}", TabAction::SplitWithNewTab),
        (false, false) => Action(
            "Split view with current tab",
            "\u{E8A0}",
            TabAction::SplitWithActive,
        ),
    };
    let pin = if facts.pinned {
        Action("Unpin tab", "\u{E77A}", TabAction::Pin(false))
    } else {
        Action("Pin tab", "\u{E718}", TabAction::Pin(true))
    };
    let mute = if facts.muted {
        Action("Unmute tab", "\u{E767}", TabAction::Mute(false))
    } else {
        Action("Mute tab", "\u{E74F}", TabAction::Mute(true))
    };
    let mut entries = vec![split, pin, mute];
    if facts.has_link {
        entries.push(Action("Copy link", "\u{E8C8}", TabAction::CopyLink));
    }
    entries.extend([Separator, Action("Close tab", "\u{E711}", TabAction::Close)]);
    entries
}

impl BrowserWindow {
    /// Fills a tab's context menu as it opens.
    pub(crate) fn fill_tab_menu(&self, id: TabId, menu: &MenuFlyout) {
        let Some(tab) = self.tab(id) else { return };
        let state = tab.state();
        let facts = TabFacts {
            active: self.active_tab().is_some_and(|a| a.id == id),
            in_split: self.split.get().is_some_and(|s| s.has(id)),
            pinned: tab.is_pinned(),
            muted: state.muted,
            has_link: has_link(&state.url),
        };
        if let Err(e) = self.fill_menu(menu, id, &entries(facts)) {
            log::warn!("tab menu: {e}");
        }
    }

    fn fill_menu(&self, menu: &MenuFlyout, id: TabId, entries: &[Entry]) -> Result<()> {
        let items = menu.Items()?;
        items.Clear()?;
        for entry in entries {
            let Entry::Action(label, glyph, action) = *entry else {
                items.Append(&MenuFlyoutSeparator::new()?.cast::<MenuFlyoutItemBase>()?)?;
                continue;
            };
            let item = MenuFlyoutItem::new()?;
            item.SetText(label)?;
            let icon = FontIcon::new()?;
            icon.SetGlyph(glyph)?;
            item.SetIcon(&icon.cast::<IconElement>()?)?;
            let me = self.me.clone();
            // After the menu has closed: an action may remove the row the menu belongs to.
            item.Click(move |_, _| {
                let me = me.clone();
                exec::spawn(async move {
                    if let Some(window) = me.upgrade() {
                        window.tab_action(id, action);
                    }
                });
            })?
            .forget();
            items.Append(&item.cast::<MenuFlyoutItemBase>()?)?;
        }
        Ok(())
    }
}

/// Whether `url` is a page's address (not a blank tab or the new tab page).
pub(super) fn has_link(url: &str) -> bool {
    !url.is_empty() && url != "about:blank"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actions(facts: TabFacts) -> Vec<TabAction> {
        entries(facts)
            .into_iter()
            .filter_map(|e| match e {
                Entry::Action(_, _, action) => Some(action),
                Entry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn split_depends_on_the_tab() {
        let other = TabFacts::default();
        assert_eq!(actions(other)[0], TabAction::SplitWithActive);
        let active = TabFacts { active: true, ..other };
        assert_eq!(actions(active)[0], TabAction::SplitWithNewTab);
        let split = TabFacts { in_split: true, active: true, ..other };
        assert_eq!(actions(split)[0], TabAction::CloseSplit);
    }

    #[test]
    fn toggles_offer_the_opposite_state() {
        let plain = actions(TabFacts::default());
        assert!(plain.contains(&TabAction::Pin(true)) && plain.contains(&TabAction::Mute(true)));
        let set = actions(TabFacts {
            pinned: true,
            muted: true,
            ..TabFacts::default()
        });
        assert!(set.contains(&TabAction::Pin(false)) && set.contains(&TabAction::Mute(false)));
    }

    #[test]
    fn copy_link_only_for_pages() {
        assert!(!actions(TabFacts::default()).contains(&TabAction::CopyLink));
        let page = TabFacts {
            has_link: true,
            ..TabFacts::default()
        };
        assert!(actions(page).contains(&TabAction::CopyLink));
        assert!(has_link("https://e.test/") && !has_link("about:blank") && !has_link(""));
    }

    #[test]
    fn close_comes_last() {
        assert_eq!(
            entries(TabFacts::default()).last(),
            Some(&Entry::Action("Close tab", "\u{E711}", TabAction::Close))
        );
    }
}
