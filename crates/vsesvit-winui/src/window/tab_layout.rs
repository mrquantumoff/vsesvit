//! Which tab list the window shows, where, and its geometry.

use vsesvit_core::prefs::TabsPosition;
use windows_core::{Interface, Result};

use super::BrowserWindow;
use crate::bindings::*;
use crate::layout::{Rect, StripKind};
use crate::strip::TabStrip;
use crate::xaml;

impl BrowserWindow {
    pub(super) fn strip(&self) -> &dyn TabStrip {
        match StripKind::of(self.tabs_position.get()) {
            StripKind::Top => self.top.as_ref(),
            StripKind::Side => self.side.as_ref(),
        }
    }

    /// Moves the tab list, keeping the order of tabs and the selected tab.
    pub fn set_tabs_position(&self, position: TabsPosition) {
        let old = self.tabs_position.get();
        if old == position {
            return;
        }
        let order = self.strip().order();
        let selected = self.strip().selected();
        self.tabs_position.set(position);
        if StripKind::of(old) != StripKind::of(position) {
            let (from, to): (&dyn TabStrip, &dyn TabStrip) = match StripKind::of(position) {
                StripKind::Top => (self.side.as_ref(), self.top.as_ref()),
                StripKind::Side => (self.top.as_ref(), self.side.as_ref()),
            };
            if let Err(e) = from.clear() {
                log::warn!("clearing the old tab list: {e}");
            }
            for (index, id) in order.iter().enumerate() {
                if let Some(tab) = self.tab(*id) {
                    let index = u32::try_from(index).unwrap_or(u32::MAX);
                    if let Err(e) = to.insert(index, tab.id, &tab.look()) {
                        log::warn!("tab list: {e}");
                    }
                }
            }
            if let Some(id) = selected {
                let _ = to.select(id);
            }
        }
        if let Err(e) = self.show_layout(position) {
            log::warn!("tab layout: {e}");
        }
        self.sync_selection();
    }

    /// Shows the parts of `position`'s layout and makes its drag region the title bar.
    pub(super) fn show_layout(&self, position: TabsPosition) -> Result<()> {
        let fullscreen = self.fullscreen.get();
        let vertical = StripKind::of(position) == StripKind::Side;
        xaml::set_visible(&self.ui.tab_view, !vertical && !fullscreen)?;
        xaml::set_visible(&self.ui.toolbar_drag, vertical)?;
        let pane = self.side.element().cast::<UIElement>()?;
        for host in [&self.ui.left_host, &self.ui.right_host] {
            let children = host.Children()?;
            let mut index = 0;
            if children.IndexOf(&pane, &mut index)? {
                children.RemoveAt(index)?;
            }
        }
        let host = match position {
            TabsPosition::Left => Some(&self.ui.left_host),
            TabsPosition::Right => Some(&self.ui.right_host),
            TabsPosition::Top => None,
        };
        if let Some(host) = host {
            host.Children()?.Append(&pane)?;
        }
        for candidate in [&self.ui.left_host, &self.ui.right_host] {
            let shown = host.is_some_and(|h| xaml::same_object(h, candidate)) && !fullscreen;
            xaml::set_visible(candidate, shown)?;
        }
        let title_bar = if vertical {
            &self.ui.toolbar_drag
        } else {
            &self.ui.drag_region
        };
        self.window.SetTitleBar(title_bar)
    }

    pub fn is_pane_collapsed(&self) -> bool {
        self.side.is_compact()
    }

    pub fn set_pane_collapsed(&self, collapsed: bool) {
        self.side.set_compact(collapsed);
    }

    /// Window-relative bounds of the vertical pane, the horizontal strip and the active web view
    /// (`None` for parts not shown).
    pub fn layout_geometry(&self) -> (Option<Rect>, Option<Rect>, Option<Rect>) {
        let pane = self.side.element();
        // Read from the tree, not from `tabs_position`: the pane counts as shown only while a
        // visible host holds it.
        let pane_shown = [&self.ui.left_host, &self.ui.right_host]
            .iter()
            .any(|host| {
                let mut index = 0;
                xaml::is_visible(*host)
                    && pane.cast::<UIElement>().is_ok_and(|pane| {
                        host.Children()
                            .and_then(|c| c.IndexOf(&pane, &mut index))
                            .unwrap_or(false)
                    })
            });
        let pane = pane_shown.then(|| self.bounds_of(pane)).flatten();
        let strip = self
            .ui
            .tab_view
            .cast::<FrameworkElement>()
            .ok()
            .filter(xaml::is_visible)
            .and_then(|s| self.bounds_of(&s));
        let view = self
            .active_tab()
            .and_then(|t| t.view().cast::<FrameworkElement>().ok())
            .filter(xaml::is_visible)
            .and_then(|v| self.bounds_of(&v));
        (pane, strip, view)
    }

    pub(super) fn bounds_of(&self, element: &FrameworkElement) -> Option<Rect> {
        let origin = element
            .cast::<UIElement>()
            .and_then(|e| e.TransformToVisual(None::<&UIElement>))
            .and_then(|t| t.TransformPoint(Point { x: 0.0, y: 0.0 }))
            .ok()?;
        Some(Rect {
            x: f64::from(origin.x),
            y: f64::from(origin.y),
            width: element.ActualWidth().ok()?,
            height: element.ActualHeight().ok()?,
        })
    }
}
