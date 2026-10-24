//! Owned by W1.5.
//! What a sync round changed, told to the views: core never calls back, so the shell fans the
//! round's `Changed` out as `BrowserStateEvent`s.

use gpui::Context;
use vsesvit_core::prefs::keys;
use vsesvit_core::sync::Changed;

use super::{BrowserState, BrowserStateEvent};

impl BrowserState {
    pub fn sync_applied(&mut self, changed: &Changed, cx: &mut Context<Self>) {
        if changed.bookmarks {
            cx.emit(BrowserStateEvent::BookmarksChanged);
        }
        if changed.history || changed.sessions {
            cx.emit(BrowserStateEvent::HistoryChanged);
        }
        if changed.prefs.iter().any(|key| key == keys::SHORTCUTS.key) {
            cx.emit(BrowserStateEvent::KeymapChanged);
        }
        if changed.prefs.iter().any(|key| key == keys::THEME.key) {
            cx.emit(BrowserStateEvent::ThemeChanged);
        }
        if !changed.prefs.is_empty() {
            cx.emit(BrowserStateEvent::PrefsChanged(changed.prefs.clone()));
        }
    }
}
