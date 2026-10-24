//! Owned by W2.6.
//! JS dialogs and `beforeunload` as prompts (design section 1.5). The body below is W1.3's dispatch
//! stub (design section 8.1): until W2.6, Chromium shows its own dialogs.

use std::sync::Arc;

use crate::client::HandlerCtx;

/// The `JsdialogHandler` of a client; none yet.
pub fn handler(_ctx: &Arc<HandlerCtx>) -> Option<cef::JsdialogHandler> {
    None
}
