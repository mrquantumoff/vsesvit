//! Owned by W2.6.
//! The file chooser as a prompt (design section 1.5). The body below is W1.3's dispatch stub (design
//! section 8.1): until W2.6, Chromium shows its own chooser.

use std::sync::Arc;

use crate::client::HandlerCtx;

/// The `DialogHandler` of a client; none yet.
pub fn handler(_ctx: &Arc<HandlerCtx>) -> Option<cef::DialogHandler> {
    None
}
