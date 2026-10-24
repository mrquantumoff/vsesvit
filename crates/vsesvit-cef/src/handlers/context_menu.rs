//! Owned by W2.5.
//! `run_context_menu` → an owned `MenuModel` prompt (design section 1.5). The body below is W1.3's
//! dispatch stub (design section 8.1): until W2.5, Chromium shows its own menu.

use std::sync::Arc;

use crate::client::HandlerCtx;

/// The `ContextMenuHandler` of a client; none yet.
pub fn handler(_ctx: &Arc<HandlerCtx>) -> Option<cef::ContextMenuHandler> {
    None
}
