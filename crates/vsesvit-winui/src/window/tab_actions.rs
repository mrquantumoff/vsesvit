//! What a tab's context menu and the copy-link commands do: a new tab beside it, split view,
//! moving it to a new window, reloading, duplicating, pinning, muting, copying its address,
//! closing it or the tabs around it, and reopening a closed tab.
//!
//! Split view shows two tabs side by side in the page grid's outer columns. It stays while
//! either of them is selected; selecting another tab shows that one alone until one of the pair
//! is selected again. Dragging the divider shares the width; letting it go with a page squeezed
//! under [`MIN_SPLIT_SHARE`] ends the split and keeps the other page. Pinned tabs lead the tab
//! list, in the order they were pinned.
//!
//! WebView2's XAML control never lets its engine move to another window, so moving a tab opens
//! its address in the new window, and duplicating one opens its address again; WebView2 cannot
//! restore a back/forward history, so neither keeps it.

use std::rc::Rc;

use vsesvit_core::tab_place::TabPlace;
use windows_core::{Interface, Result};

use super::tab_menu::TabAction;
use super::{BrowserWindow, Placement};
use crate::bindings::*;
use crate::omnibox::has_link;
use crate::session::{TabPlan, WindowPlan};
use crate::shortcuts::Command;
use crate::tab::{Initial, Tab, TabId};
use crate::{exec, platform, xaml};

/// Two tabs shown side by side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Split {
    pub left: TabId,
    pub right: TabId,
    /// The left page's share of the width.
    pub share: f64,
}

impl Split {
    pub fn has(self, id: TabId) -> bool {
        self.left == id || self.right == id
    }

    fn new(left: TabId, right: TabId) -> Self {
        Self {
            left,
            right,
            share: 0.5,
        }
    }

    /// What letting the divider go does: the tab left alone if a page is squeezed out.
    fn kept_after_drag(self) -> Option<TabId> {
        if self.share < MIN_SPLIT_SHARE {
            Some(self.right)
        } else if self.share > 1.0 - MIN_SPLIT_SHARE {
            Some(self.left)
        } else {
            None
        }
    }

    /// The page grid column of a tab of the pair.
    fn column_of(self, id: TabId) -> Option<i32> {
        match id {
            id if id == self.left => Some(0),
            id if id == self.right => Some(2),
            _ => None,
        }
    }
}

/// A page left narrower than this share when the divider is let go leaves the split view.
const MIN_SPLIT_SHARE: f64 = 0.15;
/// How far the divider drags: past [`MIN_SPLIT_SHARE`], so the page can be squeezed out.
const DRAG_SHARES: (f64, f64) = (0.05, 0.95);

/// How long the copy button shows its check mark.
const COPIED_FOR: std::time::Duration = std::time::Duration::from_millis(1200);

impl BrowserWindow {
    pub(crate) fn tab_action(&self, id: TabId, action: TabAction) {
        let Some(tab) = self.tab(id) else { return };
        match action {
            TabAction::NewTabNext => {
                match self.open_tab(Initial::Blank, Placement::After(id), true, None) {
                    Ok(_) => self.focus_address(),
                    Err(e) => log::error!("new tab: {e}"),
                }
            }
            TabAction::SplitWith(other) => {
                if other != id && self.tab(other).is_some() {
                    self.show_split(Split::new(id, other));
                }
            }
            TabAction::SplitWithNewTab => {
                match self.open_tab(Initial::Blank, Placement::After(id), false, None) {
                    Ok(new) => {
                        self.show_split(Split::new(id, new.id));
                        let _ = self.strip().select(new.id);
                        self.sync_selection();
                        self.focus_address();
                    }
                    Err(e) => log::error!("split view: {e}"),
                }
            }
            TabAction::CloseSplit => {
                self.split.set(None);
                self.sync_selection();
            }
            TabAction::MoveToNewWindow => self.move_to_new_window(&tab),
            TabAction::Reload => tab.reload(),
            TabAction::Duplicate => {
                let plan = plan_of(&tab, None);
                let initial = plan.url.clone().map_or(Initial::Blank, Initial::Url);
                if let Err(e) = self.open_tab(initial, Placement::After(id), true, Some(&plan)) {
                    log::error!("duplicate tab: {e}");
                }
            }
            TabAction::Pin(pinned) => self.set_pinned(&tab, pinned),
            TabAction::Mute(muted) => tab.set_muted(muted),
            TabAction::CopyLink => self.copy_link(&tab, true),
            TabAction::Close => self.close_tab(id),
            TabAction::CloseOthers | TabAction::CloseAfter => {
                let Some(place) = self.place_of(id) else { return };
                let closes = if action == TabAction::CloseOthers {
                    place.closes_others()
                } else {
                    place.closes_after()
                };
                let order = self.strip().order();
                for &other in closes.iter().filter_map(|&i| order.get(i)) {
                    self.close_tab(other);
                }
            }
            TabAction::ReopenClosed => self.run(Command::ReopenClosedTab),
        }
    }

    /// Opens `tab`'s address in a new window, pinned if it was, and takes the tab out of this
    /// one. It moved rather than closed, so it is not one to reopen.
    fn move_to_new_window(&self, tab: &Rc<Tab>) {
        let Some(browser) = self.browser() else { return };
        if !self.place_of(tab.id).is_some_and(TabPlace::can_move_out) {
            return;
        }
        let plan = WindowPlan::with_tabs(vec![plan_of(tab, Some(tab.session_id))]);
        if let Err(e) = browser.open_window(&plan, browser.show_mode()) {
            log::error!("move tab to new window: {e}");
            return;
        }
        if let Err(e) = self.remove_tab(tab) {
            log::warn!("move tab {}: {e}", tab.id);
        }
        browser.session_changed();
    }

    /// Shows the selected tab's web view, or both of its split view, and hides the rest.
    pub(super) fn place_views(&self, active: Option<TabId>) {
        let split = self
            .split
            .get()
            .filter(|s| active.is_some_and(|a| s.has(a)));
        let tabs = self.tabs.borrow().clone();
        // The page grid's place of a tab whose web view is in picture-in-picture.
        let mut placeholder = None;
        for tab in tabs {
            let column = split.and_then(|s| s.column_of(tab.id));
            let visible = column.is_some() || (split.is_none() && active == Some(tab.id));
            if self.in_pip(tab.id) {
                if visible {
                    placeholder = Some(column);
                }
                continue;
            }
            if let Ok(view) = tab.view().cast::<FrameworkElement>() {
                let _ = Grid::SetColumn(&view, column.unwrap_or(0));
                let _ = Grid::SetColumnSpan(&view, if column.is_some() { 1 } else { 3 });
            }
            if xaml::is_visible(tab.view()) != visible {
                let _ = xaml::set_visible(tab.view(), visible);
            }
        }
        let _ = xaml::set_visible(&self.ui.split_divider, split.is_some());
        if let Some(column) = placeholder {
            let _ = Grid::SetColumn(&self.ui.pip_placeholder, column.unwrap_or(0));
            let _ = Grid::SetColumnSpan(
                &self.ui.pip_placeholder,
                if column.is_some() { 1 } else { 3 },
            );
        }
        let _ = xaml::set_visible(&self.ui.pip_placeholder, placeholder.is_some());
    }

    /// Shows `split`, with its left tab selected; it replaces any split view there was.
    fn show_split(&self, split: Split) {
        self.split.set(Some(split));
        self.set_split_share(split.share);
        let _ = self.strip().select(split.left);
        self.sync_selection();
    }

    /// Dragging the split view's divider shares the page grid's width between the two pages.
    pub(super) fn wire_split_divider(&self) -> Result<()> {
        let me = self.me.clone();
        let shown = move || me.upgrade().is_some_and(|w| xaml::is_visible(&w.ui.split_divider));
        let me = self.me.clone();
        let moved = move |x: f64| {
            if let Some(w) = me.upgrade() {
                w.drag_split_to(x);
            }
        };
        let me = self.me.clone();
        let ended = move || {
            if let Some(w) = me.upgrade() {
                w.split_drag_ended();
            }
        };
        xaml::drag_handle(&self.ui.split_divider, shown, moved, ended)
    }

    fn drag_split_to(&self, x: f64) {
        let Some(pages) = self
            .ui
            .pages
            .cast::<FrameworkElement>()
            .ok()
            .and_then(|p| self.bounds_of(&p))
            .filter(|p| p.width > 0.0)
        else {
            return;
        };
        self.split_dragged((x - pages.x) / pages.width);
    }

    /// The divider dragged to give the left page `share` of the width.
    pub(crate) fn split_dragged(&self, share: f64) {
        let Some(split) = self.split.get() else { return };
        let share = share.clamp(DRAG_SHARES.0, DRAG_SHARES.1);
        self.split.set(Some(Split { share, ..split }));
        self.set_split_share(share);
    }

    /// The divider let go: a page squeezed out leaves the other one alone.
    pub(crate) fn split_drag_ended(&self) {
        let Some(kept) = self.split.get().and_then(Split::kept_after_drag) else {
            return;
        };
        self.split.set(None);
        let _ = self.strip().select(kept);
        self.sync_selection();
    }

    /// The left page's share of the split view's width.
    fn set_split_share(&self, share: f64) {
        let star = |value| GridLength {
            value,
            grid_unit_type: GridUnitType::Star,
        };
        let set = self
            .ui
            .pages
            .cast::<Grid>()
            .and_then(|grid| grid.ColumnDefinitions())
            .and_then(|columns| {
                columns.GetAt(0)?.SetWidth(star(share))?;
                columns.GetAt(2)?.SetWidth(star(1.0 - share))
            });
        if let Err(e) = set {
            log::warn!("split view width: {e}");
        }
    }

    /// A click into one page of the split view selects its tab, so the toolbar follows it.
    pub(super) fn page_focused(&self, id: TabId) {
        let in_split = self.split.get().is_some_and(|s| s.has(id));
        if in_split && self.active_tab().is_some_and(|a| a.id != id) {
            let _ = self.strip().select(id);
            self.sync_selection();
        }
        // A page that came while the window was in the background gets its site's zoom now.
        if let Some(tab) = self.tab(id) {
            self.take_to_site_zoom(&tab);
        }
    }

    /// A tab left the window: a split view it was part of ends.
    pub(super) fn forget_split_of(&self, id: TabId) {
        if self.split.get().is_some_and(|s| s.has(id)) {
            self.split.set(None);
        }
    }

    fn set_pinned(&self, tab: &Tab, pinned: bool) {
        if tab.is_pinned() == pinned {
            return;
        }
        tab.set_pinned(pinned);
        // Both land at the boundary: a new pin after the pinned tabs, an unpinned tab first of
        // the rest.
        let boundary = self.pinned_count(Some(tab.id));
        if let Err(e) = self.move_tab(tab.id, boundary) {
            log::warn!("pin tab: {e}");
        }
        if let Some(browser) = self.browser() {
            browser.session_changed();
        }
    }

    /// Pinned tabs in the tab list, not counting `except`.
    pub(super) fn pinned_count(&self, except: Option<TabId>) -> usize {
        self.strip()
            .order()
            .into_iter()
            .filter(|id| Some(*id) != except)
            .filter(|id| self.tab(*id).is_some_and(|t| t.is_pinned()))
            .count()
    }

    /// Where a tab is in the tab list.
    pub(super) fn place_of(&self, id: TabId) -> Option<TabPlace> {
        let order = self.strip().order();
        Some(TabPlace {
            index: order.iter().position(|t| *t == id)?,
            count: order.len(),
            pinned: self.pinned_count(None),
        })
    }

    /// After a drag in a tab list: pinned tabs move back in front of the others.
    pub(super) fn keep_pinned_first(&self) {
        let order = self.strip().order();
        let (mut pinned, rest): (Vec<TabId>, Vec<TabId>) = order
            .iter()
            .partition(|id| self.tab(**id).is_some_and(|t| t.is_pinned()));
        pinned.extend(rest);
        for (index, id) in pinned.into_iter().enumerate() {
            if self.strip().order().get(index) != Some(&id)
                && let Err(e) = self.move_tab(id, index)
            {
                log::warn!("keeping pinned tabs first: {e}");
            }
        }
    }

    fn move_tab(&self, id: TabId, index: usize) -> Result<()> {
        let strip = self.strip();
        if strip.order().iter().position(|t| *t == id) == Some(index) {
            return Ok(());
        }
        let Some(tab) = self.tab(id) else {
            return Ok(());
        };
        let selected = strip.selected() == Some(id);
        // Removing the selected row moves the strip's selection to a neighbour for a moment;
        // that must not switch tabs.
        self.reordering.set(true);
        let moved = (|| {
            strip.remove(id)?;
            strip.insert(u32::try_from(index).unwrap_or(u32::MAX), id, &tab.look())?;
            if selected {
                strip.select(id)?;
            }
            Ok(())
        })();
        self.reordering.set(false);
        self.sync_selection();
        moved
    }

    /// Copies a tab's address, without its tracking parameters if `clean`, and flashes the
    /// address bar's copy button.
    pub(super) fn copy_link(&self, tab: &Tab, clean: bool) {
        let url = tab.state().url;
        if !has_link(&url) {
            return;
        }
        let text = if clean {
            vsesvit_core::clean_url::clean(&url)
        } else {
            url
        };
        if let Err(e) = platform::copy_text(&text) {
            log::warn!("copy link: {e}");
            return;
        }
        let glyph = self.ui.copy_link_glyph.clone();
        let _ = glyph.SetGlyph("\u{E73E}");
        exec::spawn(async move {
            exec::sleep(COPIED_FOR).await;
            let _ = glyph.SetGlyph("\u{E8C8}");
        });
    }
}

/// What opens `tab` again: its address (none for a blank tab), its title until the page reports
/// one, and its pin; `id` keeps naming it in the saved session.
fn plan_of(tab: &Tab, id: Option<vsesvit_core::session::TabId>) -> TabPlan {
    let url = tab.session_url();
    TabPlan {
        url: has_link(&url).then_some(url),
        id,
        title: tab.state().title,
        pinned: tab.is_pinned(),
    }
}

#[cfg(test)]
mod tests {
    use super::Split;

    #[test]
    fn letting_go_with_a_page_squeezed_out_keeps_the_other() {
        let at = |share| Split { share, ..Split::new(1, 2) }.kept_after_drag();
        assert_eq!(at(0.1), Some(2));
        assert_eq!(at(0.9), Some(1));
        assert_eq!(at(0.15), None);
        assert_eq!(at(0.5), None);
        assert_eq!(at(0.85), None);
    }
}
