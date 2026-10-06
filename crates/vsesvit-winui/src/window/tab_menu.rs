//! A tab's context menu, with Chrome's items, the same in both tab lists. The menu is filled
//! as it opens, so its labels follow the tab (Pin or Unpin, to the right or below).

use vsesvit_core::prefs::TabsPosition;
use vsesvit_core::tab_place::TabPlace;
use windows_core::{Interface, Result};

use super::BrowserWindow;
use crate::bindings::*;
use crate::omnibox::has_link;
use crate::exec;
use crate::tab::TabId;

/// What a tab's menu does to its tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabAction {
    NewTabNext,
    /// Shows the tab beside another open tab.
    SplitWith(TabId),
    /// Shows the tab beside a new tab.
    SplitWithNewTab,
    CloseSplit,
    MoveToNewWindow,
    Reload,
    Duplicate,
    Pin(bool),
    Mute(bool),
    /// Copies the tab's address without its tracking parameters.
    CopyLink,
    Close,
    CloseOthers,
    CloseAfter,
    ReopenClosed,
}

/// What the menu depends on.
#[derive(Clone, Copy, Debug)]
pub(super) struct TabFacts {
    pub place: TabPlace,
    /// The tabs are a list in the pane, where Chrome's "to the right" reads "below".
    pub vertical: bool,
    pub in_split: bool,
    pub muted: bool,
    /// The tab shows a page with an address worth copying.
    pub has_link: bool,
    pub can_reopen: bool,
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
    Action {
        label: &'static str,
        /// A Segoe Fluent glyph.
        glyph: Option<&'static str>,
        action: TabAction,
        enabled: bool,
    },
    /// The "Split view with" submenu: a new tab, then the other open tabs.
    SplitWith(Vec<SplitTarget>),
    Separator,
}

/// A tab's menu in Chrome's order; `others` are the window's other tabs, in tab list order.
pub(super) fn entries(facts: TabFacts, others: Vec<SplitTarget>) -> Vec<Entry> {
    use TabAction::*;
    let place = facts.place;
    let item = |label, glyph, action, enabled| Entry::Action {
        label,
        glyph,
        action,
        enabled,
    };
    let on = |label, glyph, action| item(label, Some(glyph), action, true);
    let (new_tab, close_after) = if facts.vertical {
        ("New tab below", "Close tabs below")
    } else {
        ("New tab to the right", "Close tabs to the right")
    };
    let split = if facts.in_split {
        on("Close split view", "\u{E89F}", CloseSplit)
    } else {
        Entry::SplitWith(others)
    };
    let pin = if place.is_pinned() {
        on("Unpin tab", "\u{E77A}", Pin(false))
    } else {
        on("Pin tab", "\u{E718}", Pin(true))
    };
    let mute = if facts.muted {
        on("Unmute tab", "\u{E767}", Mute(false))
    } else {
        on("Mute tab", "\u{E74F}", Mute(true))
    };
    let move_out = place.can_move_out();
    let mut entries = vec![
        on(new_tab, "\u{E710}", NewTabNext),
        split,
        item("Move tab to new window", Some("\u{E78B}"), MoveToNewWindow, move_out),
        Entry::Separator,
        on("Reload", "\u{E72C}", Reload),
        on("Duplicate", "\u{E8C8}", Duplicate),
        pin,
        mute,
    ];
    if facts.has_link {
        entries.push(on("Copy link", "\u{E71B}", CopyLink));
    }
    entries.extend([
        Entry::Separator,
        on("Close tab", "\u{E711}", Close),
        item("Close other tabs", None, CloseOthers, !place.closes_others().is_empty()),
        item(close_after, None, CloseAfter, !place.closes_after().is_empty()),
        item("Reopen closed tab", Some("\u{E7A7}"), ReopenClosed, facts.can_reopen),
    ]);
    entries
}

impl BrowserWindow {
    /// Fills a tab's context menu as it opens.
    pub(crate) fn fill_tab_menu(&self, id: TabId, menu: &MenuFlyout) {
        let (Some(tab), Some(place)) = (self.tab(id), self.place_of(id)) else {
            return;
        };
        let state = tab.state();
        let facts = TabFacts {
            place,
            vertical: self.tabs_position.get() != TabsPosition::Top,
            in_split: self.split.get().is_some_and(|s| s.has(id)),
            muted: state.muted,
            has_link: has_link(&state.url),
            can_reopen: self
                .browser()
                .is_some_and(|b| b.can_reopen_closed_tab(self.browsing)),
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

    /// A tab's menu as it would open: each line's label, a submenu's as `label > child, child`,
    /// and whether it can be chosen.
    pub(crate) fn tab_menu_lines(&self, id: TabId) -> Vec<(String, bool)> {
        let Ok(menu) = MenuFlyout::new() else {
            return Vec::new();
        };
        self.fill_tab_menu(id, &menu);
        let line = |item: MenuFlyoutItemBase| -> Option<(String, bool)> {
            let enabled = item.cast::<Control>().ok()?.IsEnabled().ok()?;
            if let Ok(item) = item.cast::<MenuFlyoutItem>() {
                return Some((item.Text().ok()?, enabled));
            }
            let submenu = item.cast::<MenuFlyoutSubItem>().ok()?;
            let children: Vec<String> = submenu
                .Items()
                .ok()?
                .into_iter()
                .filter_map(|child| child.cast::<MenuFlyoutItem>().ok()?.Text().ok())
                .collect();
            let label = format!("{} > {}", submenu.Text().ok()?, children.join(", "));
            Some((label, enabled))
        };
        menu.Items()
            .map(|items| items.into_iter().filter_map(line).collect())
            .unwrap_or_default()
    }

    fn fill_menu(&self, menu: &MenuFlyout, id: TabId, entries: &[Entry]) -> Result<()> {
        let items = menu.Items()?;
        items.Clear()?;
        for entry in entries {
            let element: MenuFlyoutItemBase = match entry {
                Entry::Action {
                    label,
                    glyph,
                    action,
                    enabled,
                } => {
                    let icon = glyph.map(glyph_icon).transpose()?;
                    let item = self.menu_item(id, label, icon.as_ref(), *action)?;
                    if !enabled {
                        item.cast::<Control>()?.SetIsEnabled(false)?;
                    }
                    item.cast()?
                }
                Entry::SplitWith(others) => {
                    let submenu = MenuFlyoutSubItem::new()?;
                    submenu.SetText("Split view with")?;
                    submenu.SetIcon(&glyph_icon("\u{E8A0}")?)?;
                    let children = submenu.Items()?;
                    let new_tab = glyph_icon("\u{E710}")?;
                    children.Append(
                        &self
                            .menu_item(id, "New tab", Some(&new_tab), TabAction::SplitWithNewTab)?
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
                        let item = self.menu_item(id, &other.title, Some(&icon), action)?;
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
        icon: Option<&IconElement>,
        action: TabAction,
    ) -> Result<MenuFlyoutItem> {
        let item = MenuFlyoutItem::new()?;
        item.SetText(label)?;
        if let Some(icon) = icon {
            item.SetIcon(icon)?;
        }
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

    /// A tab in the top strip, showing a page, with a closed tab to reopen.
    fn facts(index: usize, count: usize, pinned: usize) -> TabFacts {
        TabFacts {
            place: TabPlace {
                index,
                count,
                pinned,
            },
            vertical: false,
            in_split: false,
            muted: false,
            has_link: true,
            can_reopen: true,
        }
    }

    /// The second of three tabs, none pinned.
    fn typical() -> TabFacts {
        facts(1, 3, 0)
    }

    fn labels(facts: TabFacts) -> Vec<&'static str> {
        entries(facts, vec![])
            .into_iter()
            .map(|e| match e {
                Entry::Action { label, .. } => label,
                Entry::SplitWith(_) => "Split view with",
                Entry::Separator => "-",
            })
            .collect()
    }

    fn enabled(facts: TabFacts, wanted: TabAction) -> Option<bool> {
        entries(facts, vec![]).into_iter().find_map(|e| match e {
            Entry::Action {
                action, enabled, ..
            } if action == wanted => Some(enabled),
            _ => None,
        })
    }

    #[test]
    fn the_menu_has_chromes_items_in_chromes_order() {
        assert_eq!(
            labels(typical()),
            [
                "New tab to the right",
                "Split view with",
                "Move tab to new window",
                "-",
                "Reload",
                "Duplicate",
                "Pin tab",
                "Mute tab",
                "Copy link",
                "-",
                "Close tab",
                "Close other tabs",
                "Close tabs to the right",
                "Reopen closed tab",
            ]
        );
    }

    #[test]
    fn a_pane_says_below_instead_of_to_the_right() {
        let vertical = TabFacts {
            vertical: true,
            ..typical()
        };
        let labels = labels(vertical);
        assert!(
            labels.contains(&"New tab below") && labels.contains(&"Close tabs below"),
            "{labels:?}"
        );
    }

    #[test]
    fn split_lists_the_other_tabs_unless_the_tab_is_split() {
        let others = vec![other(2), other(3)];
        assert!(entries(typical(), others.clone()).contains(&Entry::SplitWith(others.clone())));
        let split = TabFacts {
            in_split: true,
            ..typical()
        };
        let menu = entries(split, others);
        assert!(!menu.iter().any(|e| matches!(e, Entry::SplitWith(_))));
        assert_eq!(enabled(split, TabAction::CloseSplit), Some(true));
        assert_eq!(labels(split)[1], "Close split view");
    }

    #[test]
    fn toggles_offer_the_opposite_state() {
        let plain = typical();
        assert_eq!(enabled(plain, TabAction::Pin(true)), Some(true));
        assert_eq!(enabled(plain, TabAction::Mute(true)), Some(true));
        let set = TabFacts {
            muted: true,
            ..facts(0, 2, 1)
        };
        assert_eq!(enabled(set, TabAction::Pin(false)), Some(true));
        assert_eq!(enabled(set, TabAction::Mute(false)), Some(true));
    }

    #[test]
    fn copy_link_only_for_pages() {
        let blank = TabFacts {
            has_link: false,
            ..typical()
        };
        assert_eq!(enabled(blank, TabAction::CopyLink), None);
        assert_eq!(enabled(typical(), TabAction::CopyLink), Some(true));
    }

    #[test]
    fn items_that_would_do_nothing_are_off() {
        let alone = facts(0, 1, 0);
        assert_eq!(enabled(alone, TabAction::MoveToNewWindow), Some(false));
        assert_eq!(enabled(alone, TabAction::CloseOthers), Some(false));
        assert_eq!(enabled(alone, TabAction::CloseAfter), Some(false));
        let nothing_closed = TabFacts {
            can_reopen: false,
            ..alone
        };
        assert_eq!(enabled(nothing_closed, TabAction::ReopenClosed), Some(false));
        assert_eq!(enabled(facts(1, 2, 2), TabAction::CloseOthers), Some(false));
        assert_eq!(enabled(facts(0, 2, 0), TabAction::CloseAfter), Some(true));
    }

    #[test]
    fn close_tab_is_always_there() {
        assert_eq!(enabled(facts(0, 1, 0), TabAction::Close), Some(true));
        assert_eq!(enabled(facts(0, 1, 1), TabAction::Close), Some(true));
    }
}
