//! Owned by W3.3.
//! Downloads and the keep-alive of parked browsers (design section 4.2). The bodies below are W1.3's
//! dispatch stubs (design section 8.1): until W3.3, Chromium handles downloads itself and a parked
//! browser stays until shutdown.

use std::path::PathBuf;
use std::sync::Arc;

use vsesvit_engine::{EngineDownloadId, ParkedId};

use crate::client::HandlerCtx;
use crate::registry::BrowserId;

/// The `DownloadHandler` of a client; none yet.
pub fn handler(_ctx: &Arc<HandlerCtx>) -> Option<cef::DownloadHandler> {
    None
}

/// A tab was detached; its browser stays parked until its downloads end (`life_span::close_parked`).
pub fn on_parked(_parked: ParkedId, _browser: BrowserId) {}

/// `Engine::continue_download`.
pub fn continue_download(_id: EngineDownloadId, _path: PathBuf) {}

/// `Engine::cancel_download`.
pub fn cancel_download(_id: EngineDownloadId) {}

/// `Engine::pause_download`.
pub fn pause_download(_id: EngineDownloadId) {}

/// `Engine::resume_download`.
pub fn resume_download(_id: EngineDownloadId) {}
