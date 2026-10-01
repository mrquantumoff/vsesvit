//! Which tab list the window shows, where, and its geometry.

use vsesvit_core::prefs::TabsPosition;
use windows_core::{Interface, Result};

use super::BrowserWindow;
use crate::bindings::*;
use crate::layout::{Rect, StripKind};
use crate::strip::{PaneSide, TabStrip};
use crate::tab::TabId;
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

    /// Shows the parts of `position`'s layout and sets what drags the window: with vertical
    /// tabs, the empty stretches of the toolbar (see `update_drag_regions`); with the top
    /// strip, the strip's footer.
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
        if position == TabsPosition::Right {
            self.side.set_side(PaneSide::Right);
        } else {
            self.side.set_side(PaneSide::Left);
        }
        for candidate in [&self.ui.left_host, &self.ui.right_host] {
            let shown = host.is_some_and(|h| xaml::same_object(h, candidate)) && !fullscreen;
            xaml::set_visible(candidate, shown)?;
        }
        if vertical {
            // The toolbar is the title bar: its gaps are caption regions, set below.
            self.window.SetTitleBar(None::<&UIElement>)?;
        } else {
            self.window.SetTitleBar(&self.ui.drag_region)?;
        }
        *self.drag_regions.borrow_mut() = None;
        self.update_drag_regions();
        self.update_pip();
        Ok(())
    }

    /// With vertical tabs the toolbar is the title bar: the space between its controls drags
    /// the window. Those gaps are the window's caption regions, recomputed whenever the layout
    /// moves the controls and sent only when they changed. In fullscreen the page covers the
    /// toolbar, and there are none. (With the top strip, XAML keeps the strip's footer as the
    /// title bar.)
    pub(super) fn update_drag_regions(&self) {
        if StripKind::of(self.tabs_position.get()) != StripKind::Side {
            return;
        }
        self.fit_caption_spacer();
        let fullscreen = self.fullscreen.get();
        let gaps = if fullscreen {
            Vec::new()
        } else {
            self.toolbar_gaps()
        };
        let Some(rects) = caption_update(self.drag_regions.borrow().as_deref(), fullscreen, gaps)
        else {
            return;
        };
        let set = self
            .window_id()
            .and_then(InputNonClientPointerSource::GetForWindowId)
            .and_then(|source| source.SetRegionRects(NonClientRegionKind::Caption, &rects));
        match set {
            Ok(()) => *self.drag_regions.borrow_mut() = Some(rects),
            Err(e) => log::warn!("toolbar drag regions: {e}"),
        }
    }

    /// Makes the spacer at the toolbar's end exactly as wide as the window's caption buttons,
    /// which Windows draws over it, so no empty stretch is left between the menu and them.
    fn fit_caption_spacer(&self) {
        let scale = self
            .xaml_root()
            .and_then(|root| root.RasterizationScale())
            .unwrap_or(1.0);
        let inset = self
            .app_window()
            .and_then(|w| w.TitleBar())
            .and_then(|t| t.RightInset());
        if let (Ok(inset), Ok(spacer)) = (inset, self.ui.toolbar_drag.cast::<FrameworkElement>()) {
            let width = (f64::from(inset) / scale).ceil();
            if width > 0.0 && spacer.ActualWidth().is_ok_and(|w| (w - width).abs() > 0.5) {
                let _ = spacer.SetWidth(width);
            }
        }
    }

    /// The toolbar's empty stretches, full height, in physical pixels of the window.
    fn toolbar_gaps(&self) -> Vec<RectInt32> {
        let scale = self
            .xaml_root()
            .and_then(|root| root.RasterizationScale())
            .unwrap_or(1.0);
        let ui = &self.ui;
        let Some(bar) = ui
            .toolbar
            .cast::<FrameworkElement>()
            .ok()
            .and_then(|t| self.bounds_of(&t))
        else {
            return Vec::new();
        };
        let controls = [
            ui.back.cast::<FrameworkElement>(),
            ui.forward.cast::<FrameworkElement>(),
            ui.reload.cast::<FrameworkElement>(),
            ui.home.cast::<FrameworkElement>(),
            Ok(ui.address_pill.clone()),
            ui.extension_actions.cast::<FrameworkElement>(),
            ui.downloads.cast::<FrameworkElement>(),
            Ok(ui.more.clone()),
        ];
        let mut spans: Vec<(f64, f64)> = controls
            .into_iter()
            .filter_map(Result::ok)
            .filter(xaml::is_visible)
            .filter_map(|control| self.bounds_of(&control))
            .filter(|r| r.width > 0.0)
            .map(|r| (r.x, r.x + r.width))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        gaps(bar.x, bar.x + bar.width, &spans)
            .into_iter()
            .map(|(left, right)| RectInt32 {
                x: (left * scale).ceil() as i32,
                y: (bar.y * scale).floor() as i32,
                width: ((right - left) * scale).floor() as i32,
                height: (bar.height * scale).ceil() as i32,
            })
            .filter(|r| r.width > 0)
            .collect()
    }

    pub fn is_pane_collapsed(&self) -> bool {
        self.side.is_compact()
    }

    /// The vertical pane's rows, for scripted runs (see `SidePane::rows`).
    pub fn pane_rows(&self) -> Vec<(Option<TabId>, FrameworkElement)> {
        self.side.rows()
    }

    /// The horizontal strip's tab widths in display order, for scripted runs.
    pub fn top_tab_widths(&self) -> Vec<(TabId, f64)> {
        self.top.widths()
    }

    pub fn set_pane_width(&self, width: u32) {
        self.side.set_width(f64::from(width));
    }

    pub fn set_pane_collapsed(&self, collapsed: bool) {
        self.side.set_compact(collapsed);
        self.player.set_compact(collapsed);
        self.update_pip();
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

/// The caption regions to send, given those `sent` last (`None`: not known) and the toolbar's
/// gaps; `None` when nothing needs sending.
fn caption_update(
    sent: Option<&[RectInt32]>,
    fullscreen: bool,
    gaps: Vec<RectInt32>,
) -> Option<Vec<RectInt32>> {
    // Fullscreen clears the regions; otherwise no gaps is a layout pass that has not placed
    // the toolbar yet.
    let rects = if fullscreen { Vec::new() } else { gaps };
    let skip = (rects.is_empty() && !fullscreen) || sent == Some(rects.as_slice());
    (!skip).then_some(rects)
}

/// The stretches of `[start, end)` not covered by `spans` (sorted by start).
fn gaps(start: f64, end: f64, spans: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut at = start;
    for &(left, right) in spans {
        if left > at {
            out.push((at, left.min(end)));
        }
        at = at.max(right);
    }
    if at < end {
        out.push((at, end));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{caption_update, gaps};
    use crate::bindings::RectInt32;

    const GAP: RectInt32 = RectInt32 {
        x: 10,
        y: 0,
        width: 40,
        height: 40,
    };

    #[test]
    fn fullscreen_clears_the_caption_regions_once() {
        assert_eq!(caption_update(Some(&[GAP]), true, vec![GAP]), Some(vec![]));
        assert_eq!(caption_update(None, true, vec![]), Some(vec![]));
        assert_eq!(caption_update(Some(&[]), true, vec![]), None);
    }

    #[test]
    fn caption_regions_are_sent_when_they_change() {
        assert_eq!(caption_update(None, false, vec![GAP]), Some(vec![GAP]));
        assert_eq!(caption_update(Some(&[]), false, vec![GAP]), Some(vec![GAP]));
        assert_eq!(caption_update(Some(&[GAP]), false, vec![GAP]), None);
        // A layout pass that has not placed the toolbar yet leaves the regions alone.
        assert_eq!(caption_update(Some(&[GAP]), false, vec![]), None);
    }

    #[test]
    fn gaps_are_what_the_controls_leave() {
        assert_eq!(
            gaps(0.0, 100.0, &[(10.0, 20.0), (20.0, 30.0), (50.0, 60.0)]),
            [(0.0, 10.0), (30.0, 50.0), (60.0, 100.0)]
        );
        assert_eq!(gaps(0.0, 100.0, &[(0.0, 100.0)]), []);
        assert_eq!(gaps(0.0, 100.0, &[]), [(0.0, 100.0)]);
        assert_eq!(
            gaps(0.0, 100.0, &[(5.0, 40.0), (30.0, 50.0)]),
            [(0.0, 5.0), (50.0, 100.0)]
        );
    }
}
