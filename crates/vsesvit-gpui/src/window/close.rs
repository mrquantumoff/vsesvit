//! Owned by W1.7.
//! How a window and its tabs close (UA section 1.6, design section 4.2). A closing tab goes through
//! the state's close path with this window's surface, which parks the page of a tab with a running
//! download in the hidden host and closes every other browser. The window goes once its last tab has
//! ended: only then is its surface detached, so no page is ever left without its host. Closing the only
//! window, by its close button or by closing its last tab, quits instead.

use gpui::{App, Context, Window};
use vsesvit_engine::TabId;

use super::BrowserWindow;
use crate::app::quit::{self, ExitIntent};
use crate::engine_bridge;
use crate::state::CloseReason;

impl BrowserWindow {
    /// The window's close button. GPUI never closes the window itself: its tabs close first.
    pub(super) fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.closed {
            return true;
        }
        if quit::is_quitting(cx) {
            return false;
        }
        if self.is_only_window(cx) {
            // Inside this callback the window is checked out of the app, so the quit starts after it.
            cx.defer(|cx| quit::request(ExitIntent::Quit, cx));
            return false;
        }
        let tabs = self.tabs.read(cx).tabs().to_vec();
        for tab in tabs {
            self.close_tab_with(tab, CloseReason::Window, cx);
        }
        self.close_if_empty(window, cx);
        false
    }

    /// Closes one tab, as its close button or Ctrl+W does.
    pub(super) fn close_tab(&mut self, tab: TabId, cx: &mut Context<Self>) {
        self.close_tab_with(tab, CloseReason::Tab, cx);
    }

    fn close_tab_with(&mut self, tab: TabId, reason: CloseReason, cx: &mut Context<Self>) {
        let Some(state) = self.state.upgrade() else { return };
        let surface = self.surface.clone();
        let mut host = surface.as_ref().and_then(|s| s.try_borrow_mut().ok());
        state.update(cx, |state, cx| state.close_tab(tab, reason, host.as_deref_mut().map(|h| &mut **h), cx));
    }

    /// After a tab left the window: an empty window goes, or quits the browser when it was the only one.
    pub(super) fn close_if_empty(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || !self.tabs.read(cx).tabs().is_empty() {
            return;
        }
        if !quit::is_quitting(cx) && self.is_only_window(cx) {
            cx.defer(|cx| quit::request(ExitIntent::Quit, cx));
            return;
        }
        self.closed = true;
        engine_bridge::focus(cx).remove_window(self.gpui_id);
        self.overlays.update(cx, |overlays, cx| overlays.window_closing(window, cx));
        if let Some(surface) = self.surface.take() {
            match std::rc::Rc::try_unwrap(surface) {
                Ok(surface) => surface.into_inner().detach(),
                Err(_) => log::error!("window {}: its surface is still shared, so its host stays until the process ends", self.id.0),
            }
        }
        if let Some(state) = self.state.upgrade() {
            let id = self.id;
            cx.defer(move |cx| state.update(cx, |state, cx| state.window_closed(id, cx)));
        }
        window.remove_window();
    }

    fn is_only_window(&self, cx: &App) -> bool {
        self.state.upgrade().is_some_and(|state| state.read(cx).windows().len() == 1)
    }
}
