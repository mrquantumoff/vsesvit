//! What a tab's context menu and the copy-link commands do: split view, pinning, muting and
//! copying a tab's address.
//!
//! Split view shows two tabs side by side in the page grid's outer columns. It stays while
//! either of them is selected; selecting another tab shows that one alone until one of the pair
//! is selected again. Pinned tabs lead the tab list, in the order they were pinned.

use windows_core::{Interface, Result};

use super::tab_menu::{TabAction, has_link};
use super::{BrowserWindow, Placement};
use crate::bindings::*;
use crate::tab::{Initial, Tab, TabId};
use crate::{exec, platform, xaml};

/// Two tabs shown side by side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Split {
    pub left: TabId,
    pub right: TabId,
}

impl Split {
    pub fn has(self, id: TabId) -> bool {
        self.left == id || self.right == id
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

/// How long the copy button shows its check mark.
const COPIED_FOR: std::time::Duration = std::time::Duration::from_millis(1200);

impl BrowserWindow {
    pub(crate) fn tab_action(&self, id: TabId, action: TabAction) {
        let Some(tab) = self.tab(id) else { return };
        match action {
            TabAction::SplitWithActive => {
                if let Some(active) = self.active_tab()
                    && active.id != id
                {
                    self.split.set(Some(Split {
                        left: active.id,
                        right: id,
                    }));
                    self.sync_selection();
                }
            }
            TabAction::SplitWithNewTab => {
                match self.open_tab(Initial::Blank, Placement::After(id), true, None) {
                    Ok(new) => {
                        self.split.set(Some(Split {
                            left: id,
                            right: new.id,
                        }));
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
            TabAction::Pin(pinned) => self.set_pinned(&tab, pinned),
            TabAction::Mute(muted) => tab.set_muted(muted),
            TabAction::CopyLink => self.copy_link(&tab, true),
            TabAction::Close => self.close_tab(id),
        }
    }

    /// Shows the selected tab's web view, or both of its split view, and hides the rest.
    pub(super) fn place_views(&self, active: Option<TabId>) {
        let split = self
            .split
            .get()
            .filter(|s| active.is_some_and(|a| s.has(a)));
        let tabs = self.tabs.borrow().clone();
        for tab in tabs {
            if self.in_pip(tab.id) {
                continue;
            }
            let column = split.and_then(|s| s.column_of(tab.id));
            let visible = column.is_some() || (split.is_none() && active == Some(tab.id));
            if let Ok(view) = tab.view().cast::<FrameworkElement>() {
                let _ = Grid::SetColumn(&view, column.unwrap_or(0));
                let _ = Grid::SetColumnSpan(&view, if column.is_some() { 1 } else { 3 });
            }
            if xaml::is_visible(tab.view()) != visible {
                let _ = xaml::set_visible(tab.view(), visible);
            }
        }
        let _ = xaml::set_visible(&self.ui.split_divider, split.is_some());
    }

    /// A click into one page of the split view selects its tab, so the toolbar follows it.
    pub(super) fn page_focused(&self, id: TabId) {
        let in_split = self.split.get().is_some_and(|s| s.has(id));
        if in_split && self.active_tab().is_some_and(|a| a.id != id) {
            let _ = self.strip().select(id);
            self.sync_selection();
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
        strip.remove(id)?;
        strip.insert(u32::try_from(index).unwrap_or(u32::MAX), id, &tab.look())?;
        if selected {
            strip.select(id)?;
        }
        self.sync_selection();
        Ok(())
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
