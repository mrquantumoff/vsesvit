//! A tab's context menu, the same in both tab lists.

use windows_core::{Interface, Result};

use super::BrowserWindow;
use crate::bindings::*;
use crate::omnibox::has_link;
use crate::exec;
use crate::tab::TabId;

/// What a tab's menu does to its tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabAction {
    /// Shows the tab beside another open tab.
    SplitWith(TabId),
    /// Shows the tab beside a new tab.
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
    pub in_split: bool,
    pub pinned: bool,
    pub muted: bool,
    /// The tab shows a page with an address worth copying.
    pub has_link: bool,
}

/// Another open tab, as the "Split view with" submenu lists it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SplitTarget {
    pub tab: TabId,
    pub title: String,
    pub favicon: Option<ImageSource>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Entry {
    /// Label, Segoe Fluent glyph, action.
    Action(&'static str, &'static str, TabAction),
    /// The "Split view with" submenu: a new tab, then the other open tabs.
    SplitWith(Vec<SplitTarget>),
    Separator,
}

/// A tab's menu; `others` are the window's other tabs, in tab list order.
pub(super) fn entries(facts: TabFacts, others: Vec<SplitTarget>) -> Vec<Entry> {
    use Entry::{Action, Separator};
    let split = if facts.in_split {
        Action("Close split view", "\u{E89F}", TabAction::CloseSplit)
    } else {
        Entry::SplitWith(others)
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
            in_split: self.split.get().is_some_and(|s| s.has(id)),
            pinned: tab.is_pinned(),
            muted: state.muted,
            has_link: has_link(&state.url),
        };
        let others = self
            .tabs_in_order()
            .into_iter()
            .filter(|t| t.id != id)
            .map(|t| {
                let look = t.look();
                SplitTarget {
                    tab: t.id,
                    title: look.title,
                    favicon: look.favicon,
                }
            })
            .collect();
        if let Err(e) = self.fill_menu(menu, id, &entries(facts, others)) {
            log::warn!("tab menu: {e}");
        }
    }

    fn fill_menu(&self, menu: &MenuFlyout, id: TabId, entries: &[Entry]) -> Result<()> {
        let items = menu.Items()?;
        items.Clear()?;
        for entry in entries {
            let element: MenuFlyoutItemBase = match entry {
                Entry::Action(label, glyph, action) => {
                    self.menu_item(id, label, &glyph_icon(glyph)?, *action)?.cast()?
                }
                Entry::SplitWith(others) => {
                    let submenu = MenuFlyoutSubItem::new()?;
                    submenu.SetText("Split view with")?;
                    submenu.SetIcon(&glyph_icon("\u{E8A0}")?)?;
                    let children = submenu.Items()?;
                    let new_tab = glyph_icon("\u{E710}")?;
                    children.Append(
                        &self
                            .menu_item(id, "New tab", &new_tab, TabAction::SplitWithNewTab)?
                            .cast::<MenuFlyoutItemBase>()?,
                    )?;
                    if !others.is_empty() {
                        children.Append(&MenuFlyoutSeparator::new()?.cast::<MenuFlyoutItemBase>()?)?;
                    }
                    for other in others {
                        let icon = match &other.favicon {
                            Some(favicon) => {
                                let image = ImageIcon::new()?;
                                image.SetSource(favicon)?;
                                image.cast()?
                            }
                            None => glyph_icon("\u{E774}")?,
                        };
                        let action = TabAction::SplitWith(other.tab);
                        let item = self.menu_item(id, &other.title, &icon, action)?;
                        children.Append(&item.cast::<MenuFlyoutItemBase>()?)?;
                    }
                    submenu.cast()?
                }
                Entry::Separator => MenuFlyoutSeparator::new()?.cast()?,
            };
            items.Append(&element)?;
        }
        Ok(())
    }

    fn menu_item(
        &self,
        id: TabId,
        label: &str,
        icon: &IconElement,
        action: TabAction,
    ) -> Result<MenuFlyoutItem> {
        let item = MenuFlyoutItem::new()?;
        item.SetText(label)?;
        item.SetIcon(icon)?;
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
        Ok(item)
    }
}

fn glyph_icon(glyph: &str) -> Result<IconElement> {
    let icon = FontIcon::new()?;
    icon.SetGlyph(glyph)?;
    icon.cast()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn other(tab: TabId) -> SplitTarget {
        SplitTarget {
            tab,
            title: format!("Tab {tab}"),
            favicon: None,
        }
    }

    fn actions(facts: TabFacts) -> Vec<TabAction> {
        entries(facts, vec![])
            .into_iter()
            .filter_map(|e| match e {
                Entry::Action(_, _, action) => Some(action),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn split_lists_the_other_tabs_unless_the_tab_is_split() {
        let others = vec![other(2), other(3)];
        assert_eq!(
            entries(TabFacts::default(), others.clone())[0],
            Entry::SplitWith(others.clone())
        );
        let split = TabFacts {
            in_split: true,
            ..TabFacts::default()
        };
        assert_eq!(
            entries(split, others)[0],
            Entry::Action("Close split view", "\u{E89F}", TabAction::CloseSplit)
        );
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
    }

    #[test]
    fn close_comes_last() {
        assert_eq!(
            entries(TabFacts::default(), vec![]).last(),
            Some(&Entry::Action("Close tab", "\u{E711}", TabAction::Close))
        );
    }
}
