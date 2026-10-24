//! Owned by W2.6.
//! `get_auth_credentials` (IO thread) → an `Auth` prompt admitted on main (design section 1.5). The
//! body below is W1.3's dispatch stub (design section 8.1): until W2.6, no credentials are asked for.

use std::sync::Arc;

use cef::{AuthCallback, Browser, CefString};

use crate::client::HandlerCtx;

/// `RequestHandler::get_auth_credentials`: true keeps the callback for a prompt.
#[allow(clippy::too_many_arguments, reason = "CEF's callback, passed through")]
pub fn get_auth_credentials(
    _ctx: &Arc<HandlerCtx>,
    _browser: Option<&mut Browser>,
    _origin_url: Option<&CefString>,
    _is_proxy: bool,
    _host: Option<&CefString>,
    _port: i32,
    _realm: Option<&CefString>,
    _scheme: Option<&CefString>,
    _callback: Option<&mut AuthCallback>,
) -> bool {
    false
}
