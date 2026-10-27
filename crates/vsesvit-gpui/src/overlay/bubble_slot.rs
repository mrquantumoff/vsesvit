//! Owned by W2.1.
//! One address-bar bubble at a time (UA section 1.7, omnibox-privacy section 1.11): site info, the
//! zoom bubble, the star bubble and the permission prompt share a slot, and showing one closes the
//! one before it with `Replaced`. The omnibox owns the slot; the window's host draws the bubble.

use gpui::{App, Entity, Window};

use super::{DismissReason, OverlayHost, OverlayId, OverlaySpec};

#[derive(Debug, Default)]
pub struct BubbleSlot {
    shown: Option<OverlayId>,
}

impl BubbleSlot {
    /// Shows `spec` in place of the bubble shown now, if any.
    pub fn show(&mut self, host: &Entity<OverlayHost>, spec: OverlaySpec, window: &mut Window, cx: &mut App) -> OverlayId {
        let old = self.shown.take();
        let id = host.update(cx, |host, cx| {
            if let Some(old) = old {
                host.close_with(old, DismissReason::Replaced, window, cx);
            }
            host.open(spec, window, cx)
        });
        self.shown = Some(id);
        id
    }

    /// The bubble shown now; `None` once it closed, by any route.
    pub fn current(&self, host: &Entity<OverlayHost>, cx: &App) -> Option<OverlayId> {
        self.shown.filter(|id| host.read(cx).is_open(*id))
    }

    pub fn close(&mut self, host: &Entity<OverlayHost>, window: &mut Window, cx: &mut App) {
        if let Some(id) = self.shown.take() {
            host.update(cx, |host, cx| host.close(id, window, cx));
        }
    }
}
