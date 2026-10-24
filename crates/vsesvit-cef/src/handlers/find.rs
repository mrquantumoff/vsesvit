//! Owned by W2.4.
//! `find`, `stop_finding`, `on_find_result`. The bodies below are W1.3's dispatch stubs (design
//! section 8.1): find in page does nothing yet.

use std::sync::Arc;

use vsesvit_engine::TabId;

use crate::client::HandlerCtx;

/// The `FindHandler` of a client; none yet.
pub fn handler(_ctx: &Arc<HandlerCtx>) -> Option<cef::FindHandler> {
    None
}

/// `Engine::find`.
pub fn find(_tab: TabId, _text: &str, _forward: bool, _find_next: bool) {}

/// `Engine::stop_finding`.
pub fn stop_finding(_tab: TabId, _clear_selection: bool) {}
