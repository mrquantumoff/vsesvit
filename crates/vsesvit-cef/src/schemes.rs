//! Owned by W2.5.
//! The `vsesvit://` handler factory: the new tab page, error and warning pages (IO thread), and
//! `load_html` (UA S:9). The bodies below are W1.3's dispatch stubs (design section 8.1): an HTML page
//! shows `about:blank` until W2.5.

use vsesvit_engine::{TabId, Url};

/// Registers the scheme handler factories, from `on_context_initialized`.
pub fn register_handlers() {}

/// `Engine::load_html`, and a tab created with `InitialPage::Html`.
pub fn load_html(_tab: TabId, _html: String, _shown_url: Option<&Url>) {}
