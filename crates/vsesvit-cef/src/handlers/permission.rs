//! Owned by W3.1.
//! Permission and media-access prompts (design section 1.5), site settings, capture state. The bodies
//! below are W1.3's dispatch stubs (design section 8.1): until W3.1, Chromium's own prompts show.

use std::sync::Arc;

use vsesvit_engine::{Browsing, Origin, Permission, Setting};

use crate::client::HandlerCtx;
use crate::registry::BrowserId;

/// The `PermissionHandler` of a client; none yet.
pub fn handler(_ctx: &Arc<HandlerCtx>) -> Option<cef::PermissionHandler> {
    None
}

/// `DisplayHandler::on_media_access_change`.
pub fn on_media_access_change(_browser: BrowserId, _video: bool, _audio: bool) {}

/// `Engine::set_site_setting`.
pub fn set_site_setting(_browsing: Browsing, _origin: &Origin, _permission: Permission, _setting: Option<Setting>) {}
