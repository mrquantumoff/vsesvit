//! Owned by W3.1.
//! IO-thread request filtering over the privacy snapshot. The bodies below are W1.3's dispatch stubs
//! (design section 8.1): until W3.1, nothing is filtered.

use std::sync::Arc;

use cef::{Browser, CefString, Frame, Request, ResourceRequestHandler};
use vsesvit_engine::RequestFilter;

use crate::client::HandlerCtx;

/// `RequestHandler::get_resource_request_handler` (IO thread).
#[allow(clippy::too_many_arguments, reason = "CEF's callback, passed through")]
pub fn handler(
    _ctx: &Arc<HandlerCtx>,
    _browser: Option<&mut Browser>,
    _frame: Option<&mut Frame>,
    _request: Option<&mut Request>,
    _is_navigation: bool,
    _is_download: bool,
    _initiator: Option<&CefString>,
    _disable_default_handling: Option<&mut i32>,
) -> Option<ResourceRequestHandler> {
    None
}

/// `Engine::set_request_filter`.
pub fn set_request_filter(_filter: Arc<dyn RequestFilter>) {}
